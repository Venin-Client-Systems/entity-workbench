//! Owned overlapped system resolution. Cancellation requires observed completion.
#[cfg(test)]
use super::native_windows_proof as native_proof;
use super::*;
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Networking::WinSock::*,
    System::{
        Threading::{CreateEventW, WaitForSingleObject},
        IO::OVERLAPPED,
    },
};

const _: () = assert!(
    WSAEINPROGRESS == 10036
        && WSA_IO_INCOMPLETE == 996
        && WSA_IO_PENDING == 997
        && WSA_E_CANCELLED == 10111
);

struct Winsock;
impl Drop for Winsock {
    fn drop(&mut self) {
        unsafe {
            let _status = WSACleanup();
            #[cfg(test)]
            native_proof::wsa_cleanup(_status);
        }
    }
}
struct Operation {
    _winsock: Winsock,
    name: Vec<u16>,
    service: Vec<u16>,
    hints: ADDRINFOEXW,
    overlapped: OVERLAPPED,
    handle: HANDLE,
    result: *mut ADDRINFOEXW,
}
impl Drop for Operation {
    fn drop(&mut self) {
        // SAFETY: this destructor is reached only before launch or after observed
        // completion. An unacknowledged operation is deliberately retained instead.
        unsafe {
            if !self.result.is_null() {
                FreeAddrInfoExW(self.result);
                #[cfg(test)]
                native_proof::result_freed();
            }
            if !self.overlapped.hEvent.is_null() {
                let _closed = CloseHandle(self.overlapped.hEvent);
                #[cfg(test)]
                native_proof::event_closed(_closed != 0);
            }
        }
        #[cfg(test)]
        native_proof::context_dropped();
    }
}
fn unverified(operation: Box<Operation>) -> Result<ResolvedCandidates, ResolverFailure> {
    // Keep every pointer, handle and WSA reference alive if Windows has not
    // acknowledged completion. The later coordinator must suspend execution.
    #[cfg(test)]
    native_proof::context_retained();
    let _ = Box::leak(operation);
    Err(ResolverFailure::QuiescenceUnverified(ResolverUncertainty {
        method: "windows_overlapped_dns",
        caller_context: CallerContextState::RetainedPendingCompletion,
    }))
}

pub(super) fn native(
    host: &str,
    window: &mut ExecutionWindow,
    cancellation: &CancellationToken,
) -> Result<ResolvedCandidates, ResolverFailure> {
    window.check(cancellation)?;
    let deadline = Instant::now() + Duration::from_secs(5).min(window.remaining());
    // SAFETY: zeroed WSADATA is output-only, version 2.2 is requested explicitly.
    let mut data: WSADATA = unsafe { std::mem::zeroed() };
    let startup = unsafe { WSAStartup(0x0202, &mut data) };
    #[cfg(test)]
    native_proof::wsa_startup(startup);
    if startup != 0 {
        return Err(StopReason::ResolverUnavailable.into());
    }
    let winsock = Winsock;
    if data.wVersion != 0x0202 {
        return Err(StopReason::ResolverUnavailable.into());
    }
    // SAFETY: unnamed, manual-reset event with default security; not inheritable.
    let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if event.is_null() {
        return Err(StopReason::ResolverUnavailable.into());
    }
    #[cfg(test)]
    native_proof::event_created();
    let mut operation = Box::new(Operation {
        _winsock: winsock,
        name: host.encode_utf16().chain(Some(0)).collect(),
        service: "443".encode_utf16().chain(Some(0)).collect(),
        hints: ADDRINFOEXW {
            ai_family: AF_UNSPEC as i32,
            ai_socktype: SOCK_STREAM,
            ai_protocol: IPPROTO_TCP,
            ..unsafe { std::mem::zeroed() }
        },
        overlapped: OVERLAPPED {
            hEvent: event,
            ..unsafe { std::mem::zeroed() }
        },
        handle: ptr::null_mut(),
        result: ptr::null_mut(),
    });
    #[cfg(test)]
    native_proof::context_created();
    // SAFETY: all pointers refer to stable Box/Vec allocations that remain alive
    // until completion (or are retained on unverified cancellation). DNS only.
    let status = unsafe {
        GetAddrInfoExW(
            operation.name.as_ptr(),
            operation.service.as_ptr(),
            NS_DNS,
            ptr::null(),
            &operation.hints,
            &mut operation.result,
            ptr::null(),
            &operation.overlapped,
            None,
            &mut operation.handle,
        )
    };
    #[cfg(test)]
    native_proof::launched(status, cancellation);
    let mut stop = None;
    if status == WSA_IO_PENDING {
        let mut cleanup_deadline = None;
        loop {
            if stop.is_none() {
                stop = window
                    .check(cancellation)
                    .err()
                    .or_else(|| (Instant::now() >= deadline).then_some(StopReason::Timeout));
                if stop.is_some() {
                    // The cancel return alone never authorizes freeing the context.
                    unsafe {
                        let _status = GetAddrInfoExCancel(&operation.handle);
                        #[cfg(test)]
                        native_proof::cancel_returned(_status);
                    }
                    cleanup_deadline = Some(Instant::now() + Duration::from_secs(1));
                }
            }
            // SAFETY: event belongs to the still-live operation.
            let signalled = unsafe { WaitForSingleObject(event, POLL.as_millis() as u32) };
            #[cfg(test)]
            native_proof::wait_returned(signalled);
            if signalled == WAIT_OBJECT_0 {
                let completed = unsafe { GetAddrInfoExOverlappedResult(&operation.overlapped) };
                #[cfg(test)]
                native_proof::completion_returned(completed);
                if !windows_result_pending(completed) {
                    if stop.is_some() || completed == WSA_E_CANCELLED {
                        // Completion releases OUR context. Microsoft explicitly
                        // permits synchronous namespace providers to keep running
                        // after cancellation; transport must quarantine the process.
                        drop(operation);
                        return Err(ResolverFailure::QuiescenceUnverified(ResolverUncertainty {
                            method: "windows_overlapped_dns",
                            caller_context: CallerContextState::ReleasedAfterCompletion,
                        }));
                    }
                    if completed != 0 {
                        return Err(StopReason::Network.into());
                    }
                    break;
                }
                std::thread::sleep(POLL); // A stale signalled event cannot spin.
            } else if signalled != WAIT_TIMEOUT {
                unsafe {
                    let _status = GetAddrInfoExCancel(&operation.handle);
                    #[cfg(test)]
                    native_proof::cancel_returned(_status);
                }
                return unverified(operation);
            }
            if cleanup_deadline.is_some_and(|end| Instant::now() >= end) {
                return unverified(operation);
            }
        }
    } else if status == WSA_E_CANCELLED {
        drop(operation);
        return Err(ResolverFailure::QuiescenceUnverified(ResolverUncertainty {
            method: "windows_overlapped_dns",
            caller_context: CallerContextState::ReleasedAfterCompletion,
        }));
    } else if status != 0 {
        return Err(StopReason::Network.into());
    }
    window.check(cancellation)?;
    let mut addresses = Vec::with_capacity(MAX_ADDRESSES);
    let mut entry = operation.result;
    let mut count = 0;
    while !entry.is_null() {
        count += 1;
        if count > MAX_ADDRESSES {
            return Err(StopReason::Policy.into());
        }
        // SAFETY: successful GetAddrInfoEx owns a completed linked list until
        // FreeAddrInfoExW. Check sockaddr family/length before interpreting it.
        let current = unsafe { &*entry };
        if current.ai_addr.is_null() {
            return Err(StopReason::Network.into());
        }
        let ip = unsafe {
            match current.ai_family {
                family
                    if family == AF_INET as i32
                        && current.ai_addrlen >= std::mem::size_of::<SOCKADDR_IN>() =>
                {
                    let value = &*current.ai_addr.cast::<SOCKADDR_IN>();
                    IpAddr::V4(Ipv4Addr::from(value.sin_addr.S_un.S_addr.to_ne_bytes()))
                }
                family
                    if family == AF_INET6 as i32
                        && current.ai_addrlen >= std::mem::size_of::<SOCKADDR_IN6>() =>
                {
                    let value = &*current.ai_addr.cast::<SOCKADDR_IN6>();
                    IpAddr::V6(Ipv6Addr::from(value.sin6_addr.u.Byte))
                }
                _ => return Err(StopReason::Network.into()),
            }
        };
        if !policy::public_ip(ip) {
            return Err(StopReason::Policy.into());
        }
        let address = SocketAddr::new(ip, 443);
        if !addresses.contains(&address) {
            addresses.push(address);
        }
        entry = current.ai_next;
    }
    if addresses.is_empty() {
        return Err(StopReason::Network.into());
    }
    drop(operation);
    Ok(ResolvedCandidates {
        addresses,
        method: "windows_completed_system_candidates",
        authoritative_complete_set: false,
    })
}
