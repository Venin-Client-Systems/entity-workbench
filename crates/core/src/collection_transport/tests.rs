use super::*;
use crate::collection_jobs::CollectionTicket;
use std::{
    io::{Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Mutex,
    },
    thread::{self, JoinHandle},
};

const CA: &[u8] = include_bytes!("../../../../fixtures/collection-transport/ca.der");
const CERT: &[u8] = include_bytes!("../../../../fixtures/collection-transport/server.der");
const KEY: &[u8] = include_bytes!("../../../../fixtures/collection-transport/server-test-key.der");

pub(super) struct TestEndpoint {
    pub address: SocketAddr,
    pub ca: Vec<u8>,
    pub trust_fixture_ca: bool,
    pub body_started: Option<mpsc::Sender<()>>,
    pub cancel_on_complete: Option<CancellationToken>,
}
struct FixedResolver {
    calls: AtomicUsize,
    ips: Vec<IpAddr>,
    released: Arc<AtomicUsize>,
    hang: bool,
}
impl FixedResolver {
    fn public() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            ips: vec!["1.1.1.1".parse().unwrap()],
            released: Arc::new(AtomicUsize::new(0)),
            hang: false,
        }
    }
}
struct Release(Arc<AtomicUsize>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl Resolver for FixedResolver {
    fn resolve(
        &self,
        _: &str,
        window: &mut ExecutionWindow,
        cancel: &CancellationToken,
    ) -> Result<ResolvedCandidates, ResolverFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _released = Release(self.released.clone());
        if self.hang {
            loop {
                window.check(cancel)?;
                thread::sleep(POLL);
            }
        }
        Ok(ResolvedCandidates {
            addresses: self
                .ips
                .iter()
                .map(|ip| SocketAddr::new(*ip, 443))
                .collect(),
            method: "synthetic_fixed_candidates",
            authoritative_complete_set: false,
        })
    }
}

fn input(host: &str) -> (RequestTicket, CollectionInput) {
    let url = format!("https://{host}/synthetic?selected=yes");
    (
        RequestTicket {
            run: CollectionTicket {
                job_id: uuid::Uuid::new_v4().to_string(),
                generation: 1,
                lease: uuid::Uuid::new_v4().to_string(),
            },
            sequence: 0,
            url: url.clone(),
        },
        CollectionInput {
            urls: vec![url],
            max_hops: 2,
            max_requests: 50,
            max_seconds: 600,
        },
    )
}
fn window(duration: Duration) -> ExecutionWindow {
    ExecutionWindow::new(
        wall_now() + duration.as_millis() as i64,
        wall_now(),
        duration,
    )
    .unwrap()
}
fn stopped(observation: &Observation, reason: StopReason) {
    assert!(
        matches!(observation.outcome, Outcome::Stopped { reason: actual, .. } if actual == reason),
        "{observation:?}"
    );
    assert!(observation.locally_quiescent);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Accepted,
    Request,
    Body,
}
enum Mode {
    TlsStall,
    HeadersStall,
    BodyStall,
    Response(Vec<u8>),
    Truncated(Vec<u8>),
    CloseAfterRequest,
}
#[derive(Default, Debug)]
struct ServerResult {
    connections: usize,
    requests: Vec<String>,
    closed: bool,
}
struct Server {
    address: SocketAddr,
    stages: Option<mpsc::Receiver<Stage>>,
    result: Arc<Mutex<ServerResult>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Server {
    fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (send, stages) = mpsc::channel();
        let result = Arc::new(Mutex::new(ServerResult::default()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result_worker = result.clone();
        let stop_worker = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                if stop_worker.load(Ordering::SeqCst) || Instant::now() >= deadline {
                    return;
                }
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("local accept: {e}"),
                }
            };
            result_worker.lock().unwrap().connections += 1;
            socket
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            send.send(Stage::Accepted).ok();
            if matches!(mode, Mode::TlsStall) {
                result_worker.lock().unwrap().closed =
                    wait_close(&mut socket, deadline, &stop_worker);
                return;
            }
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()),
                ),
            )
            .unwrap();
            let mut stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(Arc::new(config)).unwrap(),
                socket,
            );
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while request.len() < 16 * 1024
                && !request.ends_with(b"\r\n\r\n")
                && Instant::now() < deadline
                && !stop_worker.load(Ordering::SeqCst)
            {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => request.extend_from_slice(&buffer[..count]),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue
                    }
                    Err(_) => break,
                }
            }
            if request.ends_with(b"\r\n\r\n") {
                result_worker
                    .lock()
                    .unwrap()
                    .requests
                    .push(String::from_utf8(request).unwrap());
                send.send(Stage::Request).ok();
                match mode {
                    Mode::BodyStall => {
                        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 100\r\n\r\nabc").ok();
                        stream.flush().ok();
                        send.send(Stage::Body).ok();
                    }
                    Mode::Response(bytes) => {
                        write_tls(&mut stream, &bytes, deadline);
                    }
                    Mode::Truncated(bytes) => {
                        write_tls(&mut stream, &bytes, deadline);
                        drop(stream);
                        result_worker.lock().unwrap().closed = true;
                        return;
                    }
                    Mode::CloseAfterRequest => {
                        drop(stream);
                        let until = Instant::now() + Duration::from_millis(250);
                        while Instant::now() < until {
                            if listener.accept().is_ok() {
                                result_worker.lock().unwrap().connections += 1;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        result_worker.lock().unwrap().closed = true;
                        return;
                    }
                    _ => {}
                }
            }
            result_worker.lock().unwrap().closed =
                wait_close(&mut stream.sock, deadline, &stop_worker);
        });
        Self {
            address,
            stages: Some(stages),
            result,
            stop,
            thread: Some(worker),
        }
    }
    fn configuration(&self) -> Configuration<'static> {
        Configuration {
            endpoint: Some(TestEndpoint {
                address: self.address,
                ca: CA.to_vec(),
                trust_fixture_ca: true,
                body_started: None,
                cancel_on_complete: None,
            }),
            ..Default::default()
        }
    }
    fn cancel_at(&mut self, stage: Stage, cancel: CancellationToken) -> JoinHandle<()> {
        let receiver = self.stages.take().unwrap();
        thread::spawn(move || {
            while let Ok(observed) = receiver.recv_timeout(Duration::from_secs(3)) {
                if observed == stage {
                    cancel.cancel();
                    return;
                }
            }
            panic!("Requested local-server stage was not observed");
        })
    }
    fn finish(&mut self) -> std::sync::MutexGuard<'_, ServerResult> {
        self.thread.take().unwrap().join().unwrap();
        let result = self.result.lock().unwrap();
        assert!(
            result.closed,
            "client did not close owned socket: {result:?}"
        );
        result
    }
}
fn write_tls(
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, TcpStream>,
    bytes: &[u8],
    deadline: Instant,
) {
    let mut written = 0;
    while written < bytes.len() && Instant::now() < deadline {
        match stream.write(&bytes[written..bytes.len().min(written + 16 * 1024)]) {
            Ok(0) => return,
            Ok(count) => written += count,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
    while Instant::now() < deadline {
        match stream.flush() {
            Ok(()) => return,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.thread.take() {
            worker.join().unwrap();
        }
    }
}
fn wait_close(
    socket: &mut TcpStream,
    deadline: Instant,
    stop: &std::sync::atomic::AtomicBool,
) -> bool {
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        match socket.read(&mut buffer) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                return true
            }
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn selected_scope_public_addresses_and_prelaunch_cancel_precede_any_connector() {
    let (ticket, input) = input("collection.invalid");
    let resolver = FixedResolver::public();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    stopped(
        &execute(
            &ticket,
            &input,
            window(Duration::from_secs(1)),
            Instant::now(),
            &cancelled,
            &resolver,
            &Configuration::default(),
        ),
        StopReason::Cancelled,
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    let mut wrong = ticket.clone();
    wrong.url = "https://other.invalid/".into();
    stopped(
        &execute(
            &wrong,
            &input,
            window(Duration::from_secs(1)),
            Instant::now(),
            &CancellationToken::default(),
            &resolver,
            &Configuration::default(),
        ),
        StopReason::Policy,
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    for ips in [
        vec![],
        vec!["1.1.1.1", "127.0.0.1"],
        vec!["169.254.1.1"],
        vec!["::1"],
        vec!["::ffff:127.0.0.1"],
        vec!["1.1.1.1"; MAX_ADDRESSES + 1],
    ] {
        let resolver = FixedResolver {
            ips: ips.iter().map(|ip| ip.parse().unwrap()).collect(),
            ..FixedResolver::public()
        };
        stopped(
            &execute(
                &ticket,
                &input,
                window(Duration::from_secs(1)),
                Instant::now(),
                &CancellationToken::default(),
                &resolver,
                &Configuration::default(),
            ),
            StopReason::Policy,
        );
        assert_eq!(resolver.released.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn cancelled_pacing_and_dns_return_only_after_owned_resolver_cleanup() {
    let (ticket, input) = input("collection.invalid");
    for dns in [false, true] {
        let resolver = FixedResolver {
            hang: dns,
            ..FixedResolver::public()
        };
        let cancel = CancellationToken::default();
        let token = cancel.clone();
        let cancelling = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            token.cancel();
        });
        let result = execute(
            &ticket,
            &input,
            window(Duration::from_secs(2)),
            Instant::now()
                + if dns {
                    Duration::ZERO
                } else {
                    Duration::from_secs(1)
                },
            &cancel,
            &resolver,
            &Configuration::default(),
        );
        cancelling.join().unwrap();
        stopped(&result, StopReason::Cancelled);
        assert_eq!(result.phase, if dns { Phase::Dns } else { Phase::Pacing });
        assert_eq!(resolver.released.load(Ordering::SeqCst), usize::from(dns));
        assert!(result.elapsed_milliseconds < 1000);
    }
}

#[test]
fn local_tls_headers_and_body_cancellation_closes_owned_connections() {
    for (mode, stage, phase, headers) in [
        (
            Mode::TlsStall,
            Stage::Accepted,
            Phase::ConnectTlsHeaders,
            false,
        ),
        (
            Mode::HeadersStall,
            Stage::Request,
            Phase::ConnectTlsHeaders,
            false,
        ),
        (Mode::BodyStall, Stage::Body, Phase::Body, true),
    ] {
        let mut server = Server::start(mode);
        let cancel = CancellationToken::default();
        let mut configuration = server.configuration();
        let cancelling = if stage == Stage::Body {
            let (send, receive) = mpsc::channel();
            configuration.endpoint.as_mut().unwrap().body_started = Some(send);
            let token = cancel.clone();
            thread::spawn(move || {
                receive.recv_timeout(Duration::from_secs(3)).unwrap();
                token.cancel();
            })
        } else {
            server.cancel_at(stage, cancel.clone())
        };
        let (ticket, input) = input("collection.invalid");
        let result = execute(
            &ticket,
            &input,
            window(Duration::from_secs(2)),
            Instant::now(),
            &cancel,
            &FixedResolver::public(),
            &configuration,
        );
        cancelling.join().unwrap();
        stopped(&result, StopReason::Cancelled);
        assert_eq!(result.phase, phase);
        assert!(
            matches!(&result.outcome, Outcome::Stopped { head, .. } if head.is_some() == headers)
        );
        assert_eq!(server.finish().connections, 1);
    }
}

#[test]
fn fixed_cancellation_header_gate_uses_real_token_wait_and_closes_owned_tls_response() {
    let mut server = Server::start(Mode::BodyStall);
    let (gate, received) = cancellation_probe::ResponseGate::new();
    let cancel = CancellationToken::default();
    let controller_token = cancel.clone();
    let controller = thread::spawn(move || {
        let head = received.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(head.status, 200);
        controller_token.cancel();
    });
    let mut configuration = server.configuration();
    configuration.response_gate = Some(&gate);
    let (ticket, input) = input("collection.invalid");
    let observation = execute(
        &ticket,
        &input,
        window(Duration::from_secs(3)),
        Instant::now(),
        &cancel,
        &FixedResolver::public(),
        &configuration,
    );
    controller.join().unwrap();
    stopped(&observation, StopReason::Cancelled);
    assert_eq!(observation.phase, Phase::Body);
    assert_eq!(observation.stop_observed, Some(StopReason::Cancelled));
    assert!(matches!(
        observation.outcome,
        Outcome::Stopped { head: Some(_), .. }
    ));
    let state = gate.state();
    assert!(state.reached_headers && state.cancellation_observed);
    assert!(!state.handshake_expired && !state.notification_failed);
    let server = server.finish();
    assert_eq!(server.requests.len(), 1);
    assert!(server.closed);
}

#[test]
fn fixed_cancellation_missing_controller_or_expired_handshake_never_fabricates_cancelled() {
    let head = ResponseHead {
        status: 200,
        media_type: None,
        redirect_url: None,
        identity_encoding: true,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let token = CancellationToken::default();
    let (gate, receiver) = cancellation_probe::ResponseGate::new();
    assert_eq!(
        runtime.block_on(gate.pause(&head, &token, Duration::ZERO)),
        Err(StopReason::Timeout)
    );
    assert_eq!(receiver.try_recv().unwrap(), head);
    assert!(!token.is_cancelled());
    assert!(gate.state().handshake_expired);
    assert!(!gate.state().cancellation_observed);
    let (gate, receiver) = cancellation_probe::ResponseGate::new();
    drop(receiver);
    assert_eq!(
        runtime.block_on(gate.pause(&head, &token, Duration::from_secs(1))),
        Err(StopReason::Policy)
    );
    assert!(gate.state().notification_failed);
    assert!(!gate.state().cancellation_observed);
}

#[test]
fn complete_empty_error_redirect_and_encoded_bodies_are_exact_and_never_followed() {
    for (status, extra, body, encoding) in [
        (200, "", b"synthetic\0bytes".as_slice(), true),
        (204, "", b"".as_slice(), true),
        (500, "", b"synthetic error".as_slice(), true),
        (
            302,
            "Location: https://other.invalid/not-fetched\r\n",
            b"redirect".as_slice(),
            true,
        ),
        (
            200,
            "Content-Encoding: gzip\r\n",
            b"not decompressed".as_slice(),
            false,
        ),
        (
            200,
            "Content-Encoding: identity\r\nContent-Encoding: gzip\r\n",
            b"ambiguous encoding remains raw".as_slice(),
            false,
        ),
    ] {
        let response = [format!("HTTP/1.1 {status} Test\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n{extra}\r\n", body.len()).into_bytes(), body.to_vec()].concat();
        let mut server = Server::start(Mode::Response(response));
        let (ticket, input) = input("collection.invalid");
        let resolver = FixedResolver::public();
        let result = execute(
            &ticket,
            &input,
            window(Duration::from_secs(2)),
            Instant::now(),
            &CancellationToken::default(),
            &resolver,
            &server.configuration(),
        );
        assert!(result.locally_quiescent);
        let Outcome::Complete {
            head,
            body: received,
        } = result.outcome
        else {
            panic!("{result:?}")
        };
        assert_eq!(received, body);
        assert_eq!(head.status, status);
        assert_eq!(head.identity_encoding, encoding);
        assert_eq!(
            head.redirect_url,
            (status == 302).then(|| "https://other.invalid/not-fetched".into())
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        let result = server.finish();
        assert_eq!(result.requests.len(), 1);
        let request = result.requests[0].to_ascii_lowercase();
        assert!(request.contains("accept-encoding: identity"));
        assert!(
            !request.contains("authorization:")
                && !request.contains("cookie:")
                && !request.contains("referer:")
        );
    }
}

#[test]
fn declared_chunked_and_truncated_bodies_cannot_publish_partial_originals() {
    let exact = vec![b'x'; PAGE_BYTES];
    let mut server = Server::start(Mode::Response(
        [
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", exact.len()).into_bytes(),
            exact.clone(),
        ]
        .concat(),
    ));
    let (ticket, input) = input("collection.invalid");
    let result = execute(
        &ticket,
        &input,
        window(Duration::from_secs(2)),
        Instant::now(),
        &CancellationToken::default(),
        &FixedResolver::public(),
        &server.configuration(),
    );
    assert!(matches!(result.outcome, Outcome::Complete { body, .. } if body == exact));
    assert_eq!(server.finish().requests.len(), 1);
    let large = vec![b'x'; PAGE_BYTES + 1];
    let cases = [
        (
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                PAGE_BYTES + 1
            )
            .into_bytes(),
            StopReason::BodyLimit,
            false,
        ),
        (
            [
                format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
                    large.len()
                )
                .into_bytes(),
                large,
                b"\r\n0\r\n\r\n".to_vec(),
            ]
            .concat(),
            StopReason::BodyLimit,
            false,
        ),
        (
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ninvalid\r\n".to_vec(),
            StopReason::Network,
            true,
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc".to_vec(),
            StopReason::Network,
            true,
        ),
    ];
    for (response, reason, truncate) in cases {
        let mut server = Server::start(if truncate {
            Mode::Truncated(response)
        } else {
            Mode::Response(response)
        });
        let result = execute(
            &ticket,
            &input,
            window(Duration::from_secs(2)),
            Instant::now(),
            &CancellationToken::default(),
            &FixedResolver::public(),
            &server.configuration(),
        );
        stopped(&result, reason);
        assert_eq!(result.phase, Phase::Body);
        assert_eq!(server.finish().requests.len(), 1);
    }
}

#[test]
fn complete_bytes_are_retained_when_cancellation_races_observed_eof() {
    let mut server = Server::start(Mode::Response(
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
    ));
    let cancel = CancellationToken::default();
    let mut configuration = server.configuration();
    configuration.endpoint.as_mut().unwrap().cancel_on_complete = Some(cancel.clone());
    let (ticket, input) = input("collection.invalid");
    let result = execute(
        &ticket,
        &input,
        window(Duration::from_secs(2)),
        Instant::now(),
        &cancel,
        &FixedResolver::public(),
        &configuration,
    );
    assert!(matches!(&result.outcome, Outcome::Complete { body, .. } if body == b"abc"));
    assert_eq!(result.stop_observed, Some(StopReason::Cancelled));
    assert!(result.locally_quiescent);
    assert_eq!(server.finish().requests.len(), 1);
}

#[test]
fn tls_verification_and_single_send_survive_test_endpoint_override() {
    for wrong_name in [false, true] {
        let mut server = Server::start(Mode::HeadersStall);
        let mut config = server.configuration();
        config.endpoint.as_mut().unwrap().trust_fixture_ca = wrong_name;
        let (ticket, input) = input(if wrong_name {
            "wrong.invalid"
        } else {
            "collection.invalid"
        });
        let result = execute(
            &ticket,
            &input,
            window(Duration::from_secs(2)),
            Instant::now(),
            &CancellationToken::default(),
            &FixedResolver::public(),
            &config,
        );
        stopped(&result, StopReason::Network);
        assert!(server.finish().requests.is_empty());
    }
    let mut server = Server::start(Mode::CloseAfterRequest);
    let (ticket, input) = input("collection.invalid");
    let result = execute(
        &ticket,
        &input,
        window(Duration::from_secs(2)),
        Instant::now(),
        &CancellationToken::default(),
        &FixedResolver::public(),
        &server.configuration(),
    );
    stopped(&result, StopReason::Network);
    let result = server.finish();
    assert_eq!(result.connections, 1);
    assert_eq!(result.requests.len(), 1);
}

#[test]
fn wall_rollback_and_monotonic_budget_fail_without_fabricated_times() {
    let (ticket, input) = input("collection.invalid");
    let mut deadline = window(Duration::from_secs(1));
    deadline.previous_wall_ms = wall_now() + 10_000;
    let resolver = FixedResolver::public();
    stopped(
        &execute(
            &ticket,
            &input,
            deadline,
            Instant::now(),
            &CancellationToken::default(),
            &resolver,
            &Configuration::default(),
        ),
        StopReason::ClockChanged,
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    let mut deadline = window(Duration::from_secs(10));
    deadline.monotonic_deadline = Instant::now() + Duration::from_millis(50);
    let resolver = FixedResolver {
        hang: true,
        ..FixedResolver::public()
    };
    let result = execute(
        &ticket,
        &input,
        deadline,
        Instant::now(),
        &CancellationToken::default(),
        &resolver,
        &Configuration::default(),
    );
    stopped(&result, StopReason::Deadline);
    assert_eq!(resolver.released.load(Ordering::SeqCst), 1);
    assert!(result.elapsed_milliseconds < 1000);
}

#[test]
fn final_wall_observation_is_the_exact_checked_sample_even_when_it_moves_backwards() {
    static READS: AtomicUsize = AtomicUsize::new(0);
    static BACKWARDS_AT: AtomicUsize = AtomicUsize::new(2);
    fn sequence() -> i64 {
        if READS.fetch_add(1, Ordering::SeqCst) >= BACKWARDS_AT.load(Ordering::SeqCst) {
            999
        } else {
            1000
        }
    }
    let (mut ticket, input) = input("collection.invalid");
    ticket.url = "https://not-selected.invalid/".into();
    for (backwards_at, expected_wall, expected_stop) in
        [(2, 1000, None), (1, 999, Some(StopReason::ClockChanged))]
    {
        READS.store(0, Ordering::SeqCst);
        BACKWARDS_AT.store(backwards_at, Ordering::SeqCst);
        let mut window = ExecutionWindow::new(2000, 1000, Duration::from_secs(1)).unwrap();
        window.wall_now = sequence;
        let result = execute(
            &ticket,
            &input,
            window,
            Instant::now(),
            &CancellationToken::default(),
            &FixedResolver::public(),
            &Configuration::default(),
        );
        stopped(&result, StopReason::Policy);
        assert_eq!(result.observed_wall_ms, expected_wall);
        assert_eq!(result.stop_observed, expected_stop);
        assert_eq!(READS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn pinned_resolver_never_falls_back_to_an_unexpected_name() {
    let resolver = PinnedResolver {
        host: "collection.invalid".into(),
        addresses: vec!["1.1.1.1:443".parse().unwrap()],
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    use reqwest::dns::Resolve;
    assert!(runtime
        .block_on(resolver.resolve("other.invalid".parse().unwrap()))
        .is_err());
    assert_eq!(
        runtime
            .block_on(resolver.resolve("collection.invalid".parse().unwrap()))
            .unwrap()
            .collect::<Vec<_>>(),
        ["1.1.1.1:443".parse::<SocketAddr>().unwrap()]
    );
}

#[test]
fn unknown_resolver_completion_suspends_further_attempts_without_false_cancel_ack() {
    struct Unverified(AtomicUsize, CallerContextState);
    impl Resolver for Unverified {
        fn resolve(
            &self,
            _: &str,
            _: &mut ExecutionWindow,
            _: &CancellationToken,
        ) -> Result<ResolvedCandidates, ResolverFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(ResolverFailure::QuiescenceUnverified(ResolverUncertainty {
                method: "synthetic_unknown_completion",
                caller_context: self.1,
            }))
        }
    }
    for context in [
        CallerContextState::ReleasedAfterCompletion,
        CallerContextState::RetainedPendingCompletion,
    ] {
        let recovery = AtomicBool::new(false);
        let configuration = Configuration {
            recovery_required: &recovery,
            endpoint: None,
            response_gate: None,
        };
        let resolver = Unverified(AtomicUsize::new(0), context);
        let (ticket, input) = input("collection.invalid");
        let token = CancellationToken::default();
        for expected in [
            StopReason::QuiescenceUnverified,
            StopReason::RecoveryRequired,
        ] {
            let result = execute(
                &ticket,
                &input,
                window(Duration::from_secs(1)),
                Instant::now(),
                &token,
                &resolver,
                &configuration,
            );
            assert!(
                matches!(result.outcome, Outcome::Stopped { reason, .. } if reason == expected)
            );
            assert!(!result.locally_quiescent);
            assert_eq!(
                result
                    .resolver_uncertainty
                    .map(|value| value.caller_context),
                (expected == StopReason::QuiescenceUnverified).then_some(context)
            );
            token.cancel();
        }
        assert_eq!(resolver.0.load(Ordering::SeqCst), 1);
        assert!(recovery.load(Ordering::Acquire));
    }
}

/// Fixed TLS fixture reused by the canonical integration tests. No endpoint is
/// admitted by the production destination policy or production entrypoint.
pub(crate) fn canonical_tls(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    not_before: Instant,
    cancel: &CancellationToken,
    response: Vec<u8>,
    cancel_complete: bool,
) -> Observation {
    canonical_response(
        ticket,
        input,
        window,
        not_before,
        cancel,
        Mode::Response(response),
        cancel_complete,
    )
}

pub(crate) fn canonical_truncated_tls(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    not_before: Instant,
    cancel: &CancellationToken,
    response: Vec<u8>,
) -> Observation {
    canonical_response(
        ticket,
        input,
        window,
        not_before,
        cancel,
        Mode::Truncated(response),
        false,
    )
}

fn canonical_response(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    not_before: Instant,
    cancel: &CancellationToken,
    mode: Mode,
    cancel_complete: bool,
) -> Observation {
    let mut server = Server::start(mode);
    let mut configuration = server.configuration();
    if cancel_complete {
        configuration.endpoint.as_mut().unwrap().cancel_on_complete = Some(cancel.clone());
    }
    let resolver = FixedResolver::public();
    let observed = execute(
        ticket,
        input,
        window,
        not_before,
        cancel,
        &resolver,
        &configuration,
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.finish().requests.len(), 1);
    observed
}

/// Exercises the real initial stop check, with no listener or external resolver.
pub(crate) fn canonical_before_http(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    not_before: Instant,
    cancel: &CancellationToken,
) -> Observation {
    let resolver = FixedResolver::public();
    let recovery = AtomicBool::new(false);
    let observed = execute(
        ticket,
        input,
        window,
        not_before,
        cancel,
        &resolver,
        &Configuration {
            recovery_required: &recovery,
            endpoint: None,
            response_gate: None,
        },
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(observed.phase, Phase::BeforeRequest);
    observed
}
