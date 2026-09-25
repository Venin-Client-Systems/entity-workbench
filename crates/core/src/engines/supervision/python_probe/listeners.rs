//! Host-owned IPv4 loopback observation. No worker network permission is granted.
use super::*;
use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    pub before: u32,
    pub after: u32,
    pub confined: u32,
    pub unexpected: u32,
    pub errors: u32,
}
impl Counts {
    fn record(&mut self, bytes: &[u8], markers: &[Vec<u8>; 3]) {
        if bytes == markers[0] {
            self.before += 1;
        } else if bytes == markers[1] {
            self.after += 1;
        } else if bytes == markers[2] {
            self.confined += 1;
        } else {
            self.unexpected += 1;
        }
    }
    pub fn qualifies(&self) -> bool {
        self.before == 1
            && self.after == 1
            && self.confined == 0
            && self.unexpected == 0
            && self.errors == 0
    }
}

pub(super) struct Listener {
    pub port: u16,
    tcp: bool,
    markers: [Vec<u8>; 3],
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Counts>>,
}
impl Listener {
    pub fn start(tcp: bool, confined: &str) -> Result<Self> {
        let markers = [
            uuid::Uuid::new_v4().simple().to_string().into_bytes(),
            uuid::Uuid::new_v4().simple().to_string().into_bytes(),
            confined.as_bytes().to_vec(),
        ];
        require(
            markers.iter().all(|value| value.len() == 32)
                && markers[0] != markers[1]
                && markers[0] != markers[2]
                && markers[1] != markers[2],
            "Invalid synthetic network markers",
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let expected = markers.clone();
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let (port, thread) = if tcp {
            let listener = TcpListener::bind(address)?;
            listener.set_nonblocking(true)?;
            let port = listener.local_addr()?.port();
            (
                port,
                thread::spawn(move || observe_tcp(listener, flag, expected)),
            )
        } else {
            let listener = UdpSocket::bind(address)?;
            listener.set_nonblocking(true)?;
            let port = listener.local_addr()?.port();
            (
                port,
                thread::spawn(move || observe_udp(listener, flag, expected)),
            )
        };
        Ok(Self {
            port,
            tcp,
            markers,
            stop,
            thread: Some(thread),
        })
    }
    pub fn positive(&self, after: bool) -> Result<()> {
        let marker = &self.markers[usize::from(after)];
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port);
        if self.tcp {
            let mut client =
                TcpStream::connect_timeout(&address.into(), Duration::from_millis(500))?;
            client.set_read_timeout(Some(Duration::from_millis(500)))?;
            client.set_write_timeout(Some(Duration::from_millis(500)))?;
            client.write_all(marker)?;
            let mut reply = [0; 32];
            client.read_exact(&mut reply)?;
            require(reply.as_slice() == marker, "TCP host control mismatch")?;
        } else {
            let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            client.set_read_timeout(Some(Duration::from_millis(500)))?;
            client.set_write_timeout(Some(Duration::from_millis(500)))?;
            require(
                client.send_to(marker, address)? == marker.len(),
                "UDP host control send incomplete",
            )?;
            let mut reply = [0; 64];
            let (size, from) = client.recv_from(&mut reply)?;
            require(
                from == address.into() && &reply[..size] == marker,
                "UDP host control mismatch",
            )?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<Counts> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .ok_or_else(|| Error::Validation("Listener already joined".into()))?
            .join()
            .map_err(|_| Error::Cleanup("Host observation thread failed".into()))
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn observe_tcp(listener: TcpListener, stop: Arc<AtomicBool>, markers: [Vec<u8>; 3]) -> Counts {
    observe(stop, |counts| match listener.accept() {
        Ok((mut stream, _)) => {
            if stream
                .set_read_timeout(Some(Duration::from_millis(250)))
                .is_err()
                || stream
                    .set_write_timeout(Some(Duration::from_millis(250)))
                    .is_err()
            {
                counts.errors += 1;
                return true;
            }
            let mut bytes = [0; 32];
            if stream.read_exact(&mut bytes).is_err() {
                counts.errors += 1;
            } else {
                counts.record(&bytes, &markers);
                if stream.write_all(&bytes).is_err() {
                    counts.errors += 1;
                }
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(_) => {
            counts.errors += 1;
            true
        }
    })
}
fn observe_udp(listener: UdpSocket, stop: Arc<AtomicBool>, markers: [Vec<u8>; 3]) -> Counts {
    observe(stop, |counts| {
        let mut bytes = [0; 64];
        match listener.recv_from(&mut bytes) {
            Ok((size, peer)) => {
                counts.record(&bytes[..size], &markers);
                if listener.send_to(&bytes[..size], peer).ok() != Some(size) {
                    counts.errors += 1;
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(_) => {
                counts.errors += 1;
                true
            }
        }
    })
}
fn observe(stop: Arc<AtomicBool>, mut poll: impl FnMut(&mut Counts) -> bool) -> Counts {
    let started = Instant::now();
    let mut draining = None;
    let mut events = 0;
    let mut counts = Counts::default();
    loop {
        if stop.load(Ordering::SeqCst) && draining.is_none() {
            draining = Some(Instant::now());
        }
        if draining.is_some_and(|time| time.elapsed() >= Duration::from_millis(250)) {
            break;
        }
        if started.elapsed() >= Duration::from_secs(60) || events >= 8 {
            counts.errors += 1;
            break;
        }
        if poll(&mut counts) {
            events += 1;
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    counts
}

#[test]
fn host_network_counts_require_both_controls_and_no_confined_or_unknown_delivery() {
    let markers = [b"before".to_vec(), b"after".to_vec(), b"confined".to_vec()];
    let mut counts = Counts::default();
    counts.record(&markers[0], &markers);
    assert!(!counts.qualifies());
    counts.record(&markers[1], &markers);
    assert!(counts.qualifies());
    for changed in [
        Counts {
            confined: 1,
            ..counts.clone()
        },
        Counts {
            unexpected: 1,
            ..counts.clone()
        },
        Counts {
            errors: 1,
            ..counts.clone()
        },
        Counts {
            before: 2,
            ..counts.clone()
        },
    ] {
        assert!(!changed.qualifies());
    }
    counts.record(&markers[2], &markers);
    assert!(!counts.qualifies());
}
