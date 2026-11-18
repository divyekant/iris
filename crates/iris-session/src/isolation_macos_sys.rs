//! The process facts of macOS for the sandbox of a session: `libproc`,
//! the coalition counters and `launchctl`. macOS only. The Mac acceptance
//! of R4-05m tests it.

use std::ffi::{c_int, c_void, OsString};
use std::io;
use std::mem::{size_of, MaybeUninit};
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::macos::{Proc, Sys};
use crate::macos_job::Launchd;

/// `PROC_PIDT_BSDINFOWITHUNIQID` of `<sys/proc_info.h>`.
const PIDT_BSDINFOWITHUNIQID: c_int = 18;
/// `PROC_PIDCOALITIONINFO` of `<sys/proc_info.h>`.
const PIDCOALITIONINFO: c_int = 20;
/// `struct proc_uniqidentifierinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
struct UniqInfo {
    uuid: [u8; 16],
    unique: u64,
    parent_unique: u64,
    idversion: i32,
    orig_ppidversion: i32,
    reserve2: u64,
    reserve3: u64,
}

/// `struct proc_bsdinfowithuniqid`.
#[repr(C)]
#[derive(Clone, Copy)]
struct BsdInfoUniq {
    bsd: libc::proc_bsdinfo,
    uniq: UniqInfo,
}

extern "C" {
    /// `<sys/coalition.h>`, in `libsystem_kernel`. The first two fields of
    /// `struct coalition_resource_usage` are `tasks_started` and
    /// `tasks_exited`.
    fn coalition_info_resource_usage(cid: u64, cru: *mut c_void, sz: usize) -> c_int;
}

/// The real process facts of macOS.
pub struct MacSys;

/// Reads the `flavor` of `pid` into `T`. `Ok(None)` if no process has the
/// ID (`ESRCH`). Each other failure, also a short read, is an error.
fn pidinfo<T: Copy>(pid: i32, flavor: c_int) -> io::Result<Option<T>> {
    let mut v = MaybeUninit::<T>::zeroed();
    let size = size_of::<T>() as c_int;
    // SAFETY: the buffer has `size` bytes, and the kernel writes at most
    // `size` bytes. A zeroed `T` of plain integers is a valid value.
    let n = unsafe { libc::proc_pidinfo(pid, flavor, 0, v.as_mut_ptr().cast(), size) };
    if n == size {
        // SAFETY: the buffer was zeroed, so each byte is set.
        return Ok(Some(unsafe { v.assume_init() }));
    }
    let e = io::Error::last_os_error();
    if n <= 0 && e.raw_os_error() == Some(libc::ESRCH) {
        return Ok(None);
    }
    Err(io::Error::other(format!(
        "proc_pidinfo {flavor} of {pid}: {n} bytes ({e})"
    )))
}

impl Sys for MacSys {
    fn boot_time(&self) -> io::Result<String> {
        let mut tv = MaybeUninit::<libc::timeval>::zeroed();
        let mut len = size_of::<libc::timeval>();
        // SAFETY: the name is a C string, and `len` is the size of `tv`.
        let r = unsafe {
            libc::sysctlbyname(
                c"kern.boottime".as_ptr(),
                tv.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 || len != size_of::<libc::timeval>() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the kernel wrote the whole value.
        let tv = unsafe { tv.assume_init() };
        Ok(format!("{}.{:06}", tv.tv_sec, tv.tv_usec))
    }

    fn pids(&self) -> io::Result<Vec<i32>> {
        // SAFETY: a null buffer asks for the count only.
        let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if n <= 0 {
            return Err(io::Error::last_os_error());
        }
        // Room for processes that start between the two calls.
        let mut buf = vec![0i32; n as usize + 256];
        let bytes = (buf.len() * size_of::<i32>()) as c_int;
        // SAFETY: the buffer has `bytes` bytes.
        let n = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate((n as usize).min(buf.len()));
        buf.retain(|p| *p > 0);
        Ok(buf)
    }

    fn proc(&self, pid: i32) -> io::Result<Option<Proc>> {
        let Some(c) = pidinfo::<[u64; 5]>(pid, PIDCOALITIONINFO)? else {
            return Ok(None);
        };
        let Some(info) = pidinfo::<BsdInfoUniq>(pid, PIDT_BSDINFOWITHUNIQID)? else {
            return Ok(None);
        };
        let coalition = c[0];
        Ok(Some(Proc {
            coalition,
            stopped: info.bsd.pbi_status == libc::SSTOP,
            unique: info.uniq.unique,
        }))
    }

    fn counts(&self, cid: u64) -> io::Result<Option<(u64, u64)>> {
        let mut buf = [0u64; 128];
        // SAFETY: the buffer has the size that the call gets.
        let r = unsafe {
            coalition_info_resource_usage(cid, buf.as_mut_ptr().cast(), size_of::<[u64; 128]>())
        };
        if r == 0 {
            return Ok(Some((buf[0], buf[1])));
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(e),
        }
    }

    fn signal(&self, pid: i32, sig: i32) -> io::Result<()> {
        // A process ID of 0 or less names a group: never send to it.
        if pid <= 0 {
            return Err(io::Error::other("not a single process"));
        }
        // SAFETY: kill has no memory arguments.
        if unsafe { libc::kill(pid, sig) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn uid(&self) -> u32 {
        // SAFETY: getuid cannot fail.
        unsafe { libc::getuid() }
    }

    fn run(&self, argv: &[OsString], wait: Duration) -> io::Result<i32> {
        let (program, args) = argv.split_first().ok_or(io::ErrorKind::InvalidInput)?;
        let mut child = std::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let end = Instant::now() + wait;
        loop {
            if let Some(status) = child.try_wait()? {
                return status
                    .code()
                    .ok_or_else(|| io::Error::other(format!("{status}")));
            }
            if Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The real launchd calls of the shim.
pub struct MacLaunchd;

/// The time limit of one `launchctl` call of the shim.
const LAUNCHCTL_WAIT: Duration = Duration::from_secs(2);

impl Launchd for MacLaunchd {
    fn bootstrap(&self, plist: &std::path::Path) -> io::Result<()> {
        let argv: Vec<OsString> = vec![
            "/bin/launchctl".into(),
            "bootstrap".into(),
            format!("gui/{}", self.uid()).into(),
            plist.into(),
        ];
        match MacSys.run(&argv, LAUNCHCTL_WAIT)? {
            0 => Ok(()),
            c => Err(io::Error::other(format!(
                "launchctl bootstrap: exit code {c}"
            ))),
        }
    }

    fn bootout(&self, label: &str) -> io::Result<()> {
        let argv: Vec<OsString> = vec![
            "/bin/launchctl".into(),
            "bootout".into(),
            format!("gui/{}/{label}", self.uid()).into(),
        ];
        match MacSys.run(&argv, LAUNCHCTL_WAIT)? {
            0 | 3 | 113 => Ok(()),
            c => Err(io::Error::other(format!(
                "launchctl bootout: exit code {c}"
            ))),
        }
    }

    fn peer(&self, stream: &std::os::unix::net::UnixStream) -> io::Result<(u32, i32)> {
        use std::os::fd::AsRawFd;
        let fd = stream.as_raw_fd();
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: getpeereid writes two integers.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pid: c_int = 0;
        let mut len = size_of::<c_int>() as libc::socklen_t;
        // SAFETY: LOCAL_PEERPID of SOL_LOCAL (0) writes one int.
        let r = unsafe {
            libc::getsockopt(
                fd,
                0,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut c_int).cast(),
                &mut len,
            )
        };
        if r != 0 || pid <= 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((uid, pid))
    }

    fn uid(&self) -> u32 {
        MacSys.uid()
    }
}
