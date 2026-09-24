//! A DNSService candidate batch is a snapshot, not completion of all A/AAAA answers.
use super::*;
use std::{
    ffi::{c_char, c_void, CString},
    net::{Ipv4Addr, Ipv6Addr},
    ptr,
};

type Service = *mut c_void;
type Callback = unsafe extern "C" fn(
    Service,
    u32,
    u32,
    i32,
    *const c_char,
    *const libc::sockaddr,
    u32,
    *mut c_void,
);
// macOS exports DNSService symbols through libSystem's libsystem_dnssd re-export.
#[link(name = "System")]
unsafe extern "C" {
    fn DNSServiceGetAddrInfo(
        service: *mut Service,
        flags: u32,
        interface: u32,
        protocol: u32,
        hostname: *const c_char,
        callback: Callback,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(service: Service) -> i32;
    fn DNSServiceProcessResult(service: Service) -> i32;
    fn DNSServiceRefDeallocate(service: Service);
}

struct Answers {
    values: Vec<SocketAddr>,
    batch_ended: bool,
    error: Option<StopReason>,
    callbacks: usize,
}
impl Answers {
    fn new() -> Self {
        Self {
            values: Vec::with_capacity(MAX_ADDRESSES),
            batch_ended: false,
            error: None,
            callbacks: 0,
        }
    }
    fn record(&mut self, flags: u32, error: i32, address: Option<IpAddr>) {
        self.callbacks += 1;
        if self.error.is_some() {
            return;
        }
        if error != 0 {
            self.error = Some(StopReason::Network);
            return;
        }
        if self.callbacks > MAX_ADDRESSES * 4 {
            self.error = Some(StopReason::Policy);
            return;
        }
        let Some(address) = address else {
            self.error = Some(StopReason::Network);
            return;
        };
        // Inspect every observed address, including removal notifications.
        if !policy::public_ip(address) {
            self.error = Some(StopReason::Policy);
            return;
        }
        let address = SocketAddr::new(address, 443);
        if flags & 2 != 0 {
            if !self.values.contains(&address) {
                if self.values.len() == MAX_ADDRESSES {
                    self.error = Some(StopReason::Policy);
                    return;
                }
                self.values.push(address);
            }
        } else {
            self.values.retain(|value| *value != address);
        }
        // Only batch information. We intentionally stop our continuing subscription
        // after this batch and make no complete RRset/address-family claim.
        self.batch_ended = flags & 1 == 0;
    }
}

unsafe extern "C" fn answer(
    _: Service,
    flags: u32,
    _: u32,
    error: i32,
    _: *const c_char,
    address: *const libc::sockaddr,
    _: u32,
    context: *mut c_void,
) {
    // SAFETY: context is the stable Box<Answers> owned by Operation; callbacks run
    // only inside its serialized ProcessResult call, before service deallocation.
    let answers = unsafe { &mut *context.cast::<Answers>() };
    let ip = if error != 0 || address.is_null() {
        None
    } else {
        // SAFETY: dns_sd supplies sockaddr storage for the callback duration. Check
        // its family and Darwin length before interpreting the matching structure.
        unsafe {
            match ((*address).sa_family as i32, (*address).sa_len as usize) {
                (libc::AF_INET, length) if length >= std::mem::size_of::<libc::sockaddr_in>() => {
                    let address = &*address.cast::<libc::sockaddr_in>();
                    Some(IpAddr::V4(Ipv4Addr::from(
                        address.sin_addr.s_addr.to_ne_bytes(),
                    )))
                }
                (libc::AF_INET6, length) if length >= std::mem::size_of::<libc::sockaddr_in6>() => {
                    let address = &*address.cast::<libc::sockaddr_in6>();
                    Some(IpAddr::V6(Ipv6Addr::from(address.sin6_addr.s6_addr)))
                }
                _ => None,
            }
        }
    };
    answers.record(flags, error, ip);
}

struct Operation {
    service: Service,
    answers: Box<Answers>,
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.service.is_null() {
            // SAFETY: no dispatch queue or concurrent callback thread is used. The
            // poll registration is a stack value and ProcessResult has returned.
            unsafe { DNSServiceRefDeallocate(self.service) };
            self.service = ptr::null_mut();
        }
    }
}

pub(super) fn native(
    host: &str,
    window: &mut ExecutionWindow,
    cancellation: &CancellationToken,
) -> Result<ResolvedCandidates, StopReason> {
    let hostname = CString::new(host).map_err(|_| StopReason::Policy)?;
    let mut operation = Operation {
        service: ptr::null_mut(),
        answers: Box::new(Answers::new()),
    };
    let deadline = Instant::now() + Duration::from_secs(5).min(window.remaining());
    window.check(cancellation)?;
    // SAFETY: all inputs/context outlive the continuing service, deallocated by
    // Operation on every return. Request both families without forcing multicast.
    let status = unsafe {
        DNSServiceGetAddrInfo(
            &mut operation.service,
            0,
            0,
            3,
            hostname.as_ptr(),
            answer,
            (&mut *operation.answers as *mut Answers).cast(),
        )
    };
    if status != 0 {
        // Failed creation does not transfer a valid service reference to us.
        operation.service = ptr::null_mut();
        return Err(StopReason::ResolverUnavailable);
    }
    if operation.service.is_null() {
        return Err(StopReason::ResolverUnavailable);
    }
    // SAFETY: the successful service remains live throughout this owned loop.
    let fd = unsafe { DNSServiceRefSockFD(operation.service) };
    if fd < 0 {
        return Err(StopReason::ResolverUnavailable);
    }
    loop {
        window.check(cancellation)?;
        if Instant::now() >= deadline {
            return Err(StopReason::Timeout);
        }
        let timeout = POLL
            .min(window.remaining())
            .min(deadline.saturating_duration_since(Instant::now()));
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: a single valid pollfd is borrowed only for this bounded syscall.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout.as_millis() as i32) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(StopReason::Network);
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(StopReason::Network);
        }
        if descriptor.revents & libc::POLLIN == 0 {
            continue;
        }
        // SAFETY: called only on a readable live daemon connection and only here;
        // the OS API itself is trusted to return from processing its local message.
        if unsafe { DNSServiceProcessResult(operation.service) } != 0 {
            return Err(StopReason::Network);
        }
        if let Some(error) = operation.answers.error {
            return Err(error);
        }
        if operation.answers.batch_ended {
            if operation.answers.values.is_empty() {
                return Err(StopReason::Network);
            }
            let addresses = std::mem::take(&mut operation.answers.values);
            drop(operation); // The owned subscription ends before candidates escape.
            return Ok(ResolvedCandidates {
                addresses,
                method: "macos_dns_service_observed_batch",
                authoritative_complete_set: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_marker_is_not_an_address_family_completion_signal() {
        let mut answers = Answers::new();
        answers.record(3, 0, Some("1.1.1.1".parse().unwrap()));
        assert!(!answers.batch_ended);
        answers.record(2, 0, Some("8.8.8.8".parse().unwrap()));
        assert!(answers.batch_ended);
        assert_eq!(answers.values.len(), 2);
        // A continuing query could later deliver more. The first false flag was
        // only batch information; neither IPv6 nor authoritative completeness is known.
        answers.record(2, 0, Some("2606:4700:4700::1111".parse().unwrap()));
        assert_eq!(answers.values.len(), 3);
    }
    #[test]
    fn every_observed_address_and_bounded_notification_count_are_checked() {
        let mut answers = Answers::new();
        answers.record(3, 0, Some("1.1.1.1".parse().unwrap()));
        answers.record(0, 0, Some("127.0.0.1".parse().unwrap()));
        assert_eq!(answers.error, Some(StopReason::Policy));
        let mut answers = Answers::new();
        for _ in 0..=MAX_ADDRESSES * 4 {
            answers.record(3, 0, Some("1.1.1.1".parse().unwrap()));
        }
        assert_eq!(answers.error, Some(StopReason::Policy));
        let mut answers = Answers::new();
        answers.record(0, -1, None);
        assert_eq!(answers.error, Some(StopReason::Network));
    }
}
