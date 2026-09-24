//! Hold a real fork-inherited descriptor across shutdown. No retry or sleeps hide contention.
use super::*;
#[cfg(unix)]
#[test]
fn joined_shutdown_releases_ownership_while_a_fork_inherits_the_descriptor() {
    use std::io::{Read, Write};
    use std::os::{fd::AsRawFd, unix::net::UnixStream};
    struct Child {
        pid: libc::pid_t,
        gate: UnixStream,
    }
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.gate.write_all(&[1]);
            // The child performs only a bounded handshake and _exit. Reap even
            // when a parent assertion fails; never leave a synthetic child behind.
            loop {
                let result = unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
                if result >= 0
                    || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    break;
                }
            }
        }
    }
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let coordinator = JobCoordinator::start(Workspace::open(&root).unwrap(), 1).unwrap();
    let (gate, child_gate) = UnixStream::pair().unwrap();
    gate.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let descriptor = child_gate.as_raw_fd();
    // All allocation and descriptor setup precedes fork. In the child, use only
    // async-signal-safe read/write/_exit; do not touch Rust locks or destructors.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let ready = [1u8];
        let mut release = [0u8];
        unsafe {
            if libc::write(descriptor, ready.as_ptr().cast(), 1) != 1 {
                libc::_exit(71);
            }
            if libc::read(descriptor, release.as_mut_ptr().cast(), 1) != 1 {
                libc::_exit(72);
            }
            libc::_exit(0);
        }
    }
    drop(child_gate);
    let mut child = Child { pid, gate };
    let mut ready = [0u8];
    child.gate.read_exact(&mut ready).unwrap();
    assert_eq!(ready, [1]);
    assert!(
        JobCoordinator::start(Workspace::open(&root).unwrap(), 1).is_err(),
        "Active owner must remain exclusive"
    );
    coordinator.shutdown().unwrap();
    drop(coordinator);
    // Child is deliberately still alive with the inherited ownership descriptor.
    let reopened = JobCoordinator::start(Workspace::open(&root).unwrap(), 1);
    drop(child);
    let reopened =
        reopened.expect("Joined coordinator failed to release an inherited ownership lock");
    reopened.shutdown().unwrap();
}

#[test]
fn repeated_shutdown_cannot_release_a_later_coordinators_ownership() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let first = JobCoordinator::start(Workspace::open(&root).unwrap(), 1).unwrap();
    first.shutdown().unwrap();
    let second = JobCoordinator::start(Workspace::open(&root).unwrap(), 1).unwrap();
    first.shutdown().unwrap();
    drop(first);
    assert!(JobCoordinator::start(Workspace::open(&root).unwrap(), 1).is_err());
    second.shutdown().unwrap();
}
