//! Bounded reads for runtime hashing/copying, with cooperative cancellation.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]
use crate::{outcomes::check_cancelled, Error, Result};
use std::io::Read;

/// At most 64 KiB is read between cancellation checkpoints. The expected length
/// is verified independently of the caller's digest; no EOF or growth is hidden.
pub(crate) fn stream_exact(
    source: &mut impl Read,
    expected: u64,
    cancelled: &impl Fn() -> bool,
    mut consume: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        check_cancelled(cancelled)?;
        let size = source.read(&mut buffer)?;
        check_cancelled(cancelled)?;
        if size == 0 {
            break;
        }
        count = count
            .checked_add(size as u64)
            .ok_or(Error::Blocked("runtime size overflow"))?;
        if count > expected {
            return Err(Error::Blocked("runtime grew during preparation"));
        }
        consume(&buffer[..size])?;
    }
    if count != expected {
        return Err(Error::Blocked("runtime truncated during preparation"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io::Cursor};
    #[test]
    fn exact_stream_rejects_short_and_growing_input() {
        for size in [2, 4] {
            assert!(matches!(
                stream_exact(&mut Cursor::new(b"abc"), size, &|| false, |_| Ok(())),
                Err(Error::Blocked(_))
            ));
        }
        let mut accepted = Vec::new();
        stream_exact(&mut Cursor::new(b"abc"), 3, &|| false, |bytes| {
            accepted.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(accepted, b"abc");
    }
    #[test]
    fn cancellation_before_and_between_chunks_stops_consumption() {
        struct NoRead;
        impl Read for NoRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("pre-cancel read")
            }
        }
        assert!(matches!(
            stream_exact(&mut NoRead, 1, &|| true, |_| panic!("pre-cancel write")),
            Err(Error::Cancelled)
        ));
        let cancelled = Cell::new(false);
        let mut accepted = 0;
        assert!(matches!(
            stream_exact(
                &mut Cursor::new(vec![0; 131072]),
                131072,
                &|| cancelled.get(),
                |bytes| {
                    accepted += bytes.len();
                    cancelled.set(true);
                    Ok(())
                }
            ),
            Err(Error::Cancelled)
        ));
        assert_eq!(accepted, 65536);
    }

    #[test]
    fn cancellation_during_a_read_prevents_the_chunk_being_written() {
        struct CancelRead<'a>(&'a Cell<bool>);
        impl Read for CancelRead<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                bytes[0] = 1;
                self.0.set(true);
                Ok(1)
            }
        }
        let cancelled = Cell::new(false);
        let result = stream_exact(&mut CancelRead(&cancelled), 1, &|| cancelled.get(), |_| {
            panic!("cancelled bytes must not be consumed")
        });
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}
