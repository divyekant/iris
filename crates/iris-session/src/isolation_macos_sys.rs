//! The process facts of macOS for the sandbox of a session: `libproc`,
//! the coalition counters and `launchctl`. macOS only. The Mac acceptance
//! of R4-05m tests it.

use std::ffi::{c_int, c_void};
use std::io;
use std::mem::{size_of, MaybeUninit};
use std::process::Stdio;

use super::macos::{Proc, Sys};

/// `PROC_PIDT_BSDINFOWITHUNIQID` of `<sys/proc_info.h>`.
const PIDT_BSDINFOWITHUNIQID: c_int = 18;
/// `PROC_PIDCOALITIONINFO` of `<sys/proc_info.h>`.
const PIDCOALITIONINFO: c_int = 20;
/// The exit codes of `launchctl bootout` for a job that is not loaded:
/// 3 ("No such process") and 113 ("Could not find service").
const NOT_LOADED: [i32; 2] = [3, 113];

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

/// Reads the `flavor` of `pid` into `T`. `None` if the size is not the
/// full size of `T` (no such process, or no permission).
fn pidinfo<T: Copy>(pid: i32, flavor: c_int) -> Option<T> {
    let mut v = MaybeUninit::<T>::zeroed();
    let size = size_of::<T>() as c_int;
    // SAFETY: the buffer has `size` bytes, and the kernel writes at most
    // `size` bytes. A zeroed `T` of plain integers is a valid value.
    let n = unsafe { libc::proc_pidinfo(pid, flavor, 0, v.as_mut_ptr().cast(), size) };
    // SAFETY: the buffer was zeroed, so each byte is set.
    (n == size).then(|| unsafe { v.assume_init() })
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

    fn proc(&self, pid: i32) -> Option<Proc> {
        let coalition = pidinfo::<[u64; 5]>(pid, PIDCOALITIONINFO)?[0];
        let info = pidinfo::<BsdInfoUniq>(pid, PIDT_BSDINFOWITHUNIQID)?;
        Some(Proc {
            coalition,
            stopped: info.bsd.pbi_status == libc::SSTOP,
            unique: info.uniq.unique,
        })
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

    fn bootout(&self, label: &str) -> io::Result<()> {
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let status = std::process::Command::new("/bin/launchctl")
            .arg("bootout")
            .arg(format!("gui/{uid}/{label}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        match status.code() {
            Some(0) => Ok(()),
            Some(c) if NOT_LOADED.contains(&c) => Ok(()),
            _ => Err(io::Error::other(format!("launchctl bootout: {status}"))),
        }
    }
}
