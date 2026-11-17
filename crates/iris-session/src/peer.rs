//! The system calls of the session socket: the user of a peer, and a wait
//! for a connection with a time limit. Linux and macOS.

use std::io;
use std::os::fd::AsRawFd;
use std::time::Duration;

/// The user ID of the process at the other end of `stream`.
pub fn uid(stream: &impl AsRawFd) -> io::Result<u32> {
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "linux")]
    {
        // SAFETY: ucred is plain data; all zero bytes is a valid value.
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: cred and len are valid for the call, and len is the size
        // of cred.
        let r = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if r != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.uid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: uid and gid are valid for the call.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(uid)
    }
}

/// The effective user ID of this process.
pub fn own_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

/// Waits until `fd` can be read (for a listener: a connection waits), for
/// at most `wait`. Gives `false` after the wait.
pub fn readable(fd: &impl AsRawFd, wait: Duration) -> io::Result<bool> {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = wait.as_millis().clamp(1, i32::MAX as u128) as i32;
    // SAFETY: p is one valid pollfd for the call.
    match unsafe { libc::poll(&mut p, 1, ms) } {
        r if r < 0 => {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(e)
            }
        }
        r => Ok(r > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn the_peer_of_a_pair_is_this_user() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(uid(&a).unwrap(), own_uid());
    }

    #[test]
    fn readable_waits_for_a_byte() {
        use std::io::Write;
        let (a, mut b) = UnixStream::pair().unwrap();
        assert!(!readable(&a, Duration::from_millis(10)).unwrap());
        b.write_all(b"x").unwrap();
        assert!(readable(&a, Duration::from_millis(10)).unwrap());
    }
}
