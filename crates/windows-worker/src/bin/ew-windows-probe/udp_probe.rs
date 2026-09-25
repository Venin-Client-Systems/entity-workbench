//! Loopback delivery observations for the synthetic harness, not a DNS/network guarantee.
use serde::{Deserialize, Serialize};
use std::{
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use workbench_windows_worker::{Error, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UdpObservation {
    pub send_accepted: bool,
    pub reply_received: bool,
}

pub(super) fn exchange(destination: SocketAddr, marker: &str) -> Result<UdpObservation> {
    if marker.is_empty() || marker.len() > 64 || !destination.ip().is_loopback() {
        return Err(Error::Blocked("invalid synthetic UDP control"));
    }
    let Ok(socket) = UdpSocket::bind("127.0.0.1:0") else {
        return Ok(UdpObservation {
            send_accepted: false,
            reply_received: false,
        });
    };
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    socket.set_write_timeout(Some(Duration::from_secs(1)))?;
    let send_accepted = socket
        .send_to(marker.as_bytes(), destination)
        .is_ok_and(|count| count == marker.len());
    let mut response = [0; 65];
    let reply_received = send_accepted
        && socket
            .recv_from(&mut response)
            .is_ok_and(|(count, sender)| {
                sender == destination && &response[..count] == marker.as_bytes()
            });
    Ok(UdpObservation {
        send_accepted,
        reply_received,
    })
}

#[derive(Clone)]
pub(super) struct Markers {
    pub before: String,
    pub confined: String,
    pub after: String,
}
impl Markers {
    pub fn new() -> Self {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        Self {
            before: format!("b:{nonce}"),
            confined: format!("c:{nonce}"),
            after: format!("a:{nonce}"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub(super) struct DeliveryCounts {
    pub before: usize,
    pub confined: usize,
    pub after: usize,
    pub unexpected: usize,
}

/// One owned listener spans both controls and the confined worker. Finish keeps
/// receiving for 250 ms after the worker/control exit, with a bounded 50 ms read.
/// Unexpected I/O errors and a failed echo fail the probe; they cannot look like
/// a healthy listener receiving zero confined packets.
pub(super) struct Listener {
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<DeliveryCounts>>>,
}
impl Listener {
    pub fn start(markers: Markers) -> Result<Self> {
        let socket = UdpSocket::bind("127.0.0.1:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(50)))?;
        socket.set_write_timeout(Some(Duration::from_millis(50)))?;
        let address = socket.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut counts = DeliveryCounts::default();
            let mut stop_at = None;
            let mut bytes = [0; 65];
            loop {
                if worker_stop.load(Ordering::Acquire) && stop_at.is_none() {
                    stop_at = Some(Instant::now() + Duration::from_millis(250));
                }
                if stop_at.is_some_and(|end| Instant::now() >= end) {
                    return Ok(counts);
                }
                match socket.recv_from(&mut bytes) {
                    Ok((count, sender)) => {
                        let value = &bytes[..count];
                        let counter = if value == markers.before.as_bytes() {
                            &mut counts.before
                        } else if value == markers.confined.as_bytes() {
                            &mut counts.confined
                        } else if value == markers.after.as_bytes() {
                            &mut counts.after
                        } else {
                            &mut counts.unexpected
                        };
                        *counter = counter.saturating_add(1);
                        if socket.send_to(value, sender)? != count {
                            return Err(Error::Blocked("synthetic UDP echo incomplete"));
                        }
                    }
                    Err(error)
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(error) => return Err(error.into()),
                }
            }
        });
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }
    pub fn finish(mut self) -> Result<DeliveryCounts> {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .ok_or(Error::Blocked("UDP listener already joined"))?
            .join()
            .map_err(|_| Error::Blocked("synthetic UDP listener failed"))?
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A send call's success is deliberately not a delivery assertion. The raw
/// confined observation remains in evidence, whether the call succeeds or fails.
pub(super) fn accept_delivery_denial(
    before: UdpObservation,
    confined: UdpObservation,
    after: UdpObservation,
    worker_completed: bool,
    counts: DeliveryCounts,
) -> Result<()> {
    if !worker_completed
        || !before.send_accepted
        || !before.reply_received
        || !after.send_accepted
        || !after.reply_received
        || counts.before == 0
        || counts.after == 0
    {
        return Err(Error::Blocked("UDP delivery controls incomplete"));
    }
    if confined.reply_received || counts.confined != 0 || counts.unexpected != 0 {
        return Err(Error::Blocked("confined UDP delivery boundary failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONTROL: UdpObservation = UdpObservation {
        send_accepted: true,
        reply_received: true,
    };
    fn counts() -> DeliveryCounts {
        DeliveryCounts {
            before: 1,
            after: 1,
            ..Default::default()
        }
    }

    #[test]
    fn send_acceptance_alone_never_proves_delivery_or_denial() {
        for send_accepted in [false, true] {
            let confined = UdpObservation {
                send_accepted,
                reply_received: false,
            };
            assert!(accept_delivery_denial(CONTROL, confined, CONTROL, true, counts()).is_ok());
            assert!(accept_delivery_denial(CONTROL, confined, CONTROL, false, counts()).is_err());
            for failed in [
                UdpObservation {
                    send_accepted: false,
                    reply_received: false,
                },
                UdpObservation {
                    send_accepted: true,
                    reply_received: false,
                },
            ] {
                assert!(accept_delivery_denial(failed, confined, CONTROL, true, counts()).is_err());
                assert!(accept_delivery_denial(CONTROL, confined, failed, true, counts()).is_err());
            }
            for failed_counts in [
                DeliveryCounts {
                    before: 0,
                    ..counts()
                },
                DeliveryCounts {
                    after: 0,
                    ..counts()
                },
                DeliveryCounts {
                    confined: 1,
                    ..counts()
                },
                DeliveryCounts {
                    unexpected: 1,
                    ..counts()
                },
            ] {
                assert!(
                    accept_delivery_denial(CONTROL, confined, CONTROL, true, failed_counts)
                        .is_err()
                );
            }
        }
        assert!(accept_delivery_denial(CONTROL, CONTROL, CONTROL, true, counts()).is_err());
    }

    #[test]
    fn actual_loopback_controls_and_delivered_confined_marker_cannot_pass() {
        let markers = Markers::new();
        let listener = Listener::start(markers.clone()).unwrap();
        let before = exchange(listener.address, &markers.before).unwrap();
        let confined = exchange(listener.address, &markers.confined).unwrap();
        let after = exchange(listener.address, &markers.after).unwrap();
        let counts = listener.finish().unwrap();
        assert_eq!(
            (
                counts.before,
                counts.confined,
                counts.after,
                counts.unexpected
            ),
            (1, 1, 1, 0)
        );
        // Even a worker falsely reporting no reply cannot hide parent delivery.
        assert!(accept_delivery_denial(
            before,
            UdpObservation {
                reply_received: false,
                ..confined
            },
            after,
            true,
            counts
        )
        .is_err());
    }

    #[test]
    fn listener_drains_queued_packets_and_both_positive_controls_are_required() {
        let markers = Markers::new();
        let listener = Listener::start(markers.clone()).unwrap();
        let before = exchange(listener.address, &markers.before).unwrap();
        let after = exchange(listener.address, &markers.after).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to(markers.confined.as_bytes(), listener.address)
            .unwrap();
        let counts = listener.finish().unwrap();
        assert!(counts.confined > 0);
        assert!(accept_delivery_denial(
            before,
            UdpObservation {
                send_accepted: true,
                reply_received: false
            },
            after,
            true,
            counts
        )
        .is_err());
    }
}
