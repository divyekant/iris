//! The checks of the read paths of an adapter
//! (`docs/specs/2026-11-25-agents-read-paths-local-port.md`, "The checks at
//! each run").
//!
//! Invariant: no read path shows an object of a protected folder (the data
//! folder, a home folder as a whole, a folder of system state), under any
//! name. So the checks compare the place of a folder in its file system
//! (Linux: the device and the path in the file system, from
//! `/proc/self/mountinfo`), not its name. The walk refuses a nested mount,
//! a socket, a FIFO and a device in the tree.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The most read paths of one adapter (`agents.toml`).
pub const MAX_READS: usize = 16;

/// The most entries that the walk of one read path visits.
pub const MAX_ENTRIES: usize = 200_000;

/// The folders of system state. A read path is not one, is not inside one,
/// and holds none.
pub const STATE: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/var/run",
    "/tmp",
    "/var/tmp",
    "/private/var/run",
    "/private/tmp",
    "/private/var/folders",
    "/Volumes",
    "/System/Volumes/Data",
];

/// File system types with system state, refused as the file system of a
/// read path (Linux).
const VIRTUAL: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "securityfs",
    "debugfs",
    "tracefs",
    "bpf",
    "fusectl",
    "configfs",
    "mqueue",
    "hugetlbfs",
    "pstore",
    "binfmt_misc",
    "efivarfs",
];

/// A read path that passed the checks: its real path and the open object.
/// On Linux the helper binds the object of `fd`, not a path.
#[derive(Debug)]
pub struct Checked {
    pub real: PathBuf,
    pub fd: OwnedFd,
}

/// The place of a folder: the file system and the path in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub dev: String,
    pub path: PathBuf,
    pub fstype: String,
}

impl Place {
    /// True if `self` is `other` or holds it.
    pub fn holds(&self, other: &Place) -> bool {
        self.dev == other.dev && other.path.starts_with(&self.path)
    }
}

/// Resolves, opens and checks the read path `path`. `data_dir` is the data
/// folder, `home` the `HOME` of the engine. The error is the reason, for
/// the person.
pub fn check(path: &Path, data_dir: &Path, home: Option<&OsStr>) -> Result<Checked, String> {
    let show = path.display();
    let real = std::fs::canonicalize(path)
        .map_err(|e| format!("the read path {show} cannot be resolved: {e}"))?;
    let fd =
        open_no_links(&real).map_err(|e| format!("the read path {show} cannot be opened: {e}"))?;
    let refuse = |why: String| format!("the read path {show} is refused: {why}");
    let place = place_of(&fd, &real).map_err(|e| refuse(e.to_string()))?;
    if VIRTUAL.contains(&place.fstype.as_str()) {
        return Err(refuse(format!("it is on a {} file system", place.fstype)));
    }
    let data = protected_place(data_dir).map_err(|e| refuse(format!("the data folder: {e}")))?;
    if place.holds(&data) || data.holds(&place) {
        return Err(refuse(
            "it is the data folder, is inside it or holds it".into(),
        ));
    }
    for h in homes(home).map_err(refuse)? {
        if let Ok(p) = protected_place(&h) {
            if place.holds(&p) {
                return Err(refuse(format!(
                    "it is the home folder {} or holds it",
                    h.display()
                )));
            }
        }
    }
    for s in STATE {
        if let Ok(p) = protected_place(Path::new(s)) {
            if place.holds(&p) || p.holds(&place) {
                return Err(refuse(format!("it is, is inside or holds {s}")));
            }
        }
        // Also by name: a folder of system state can be a link on one
        // system and a mount on another.
        let s = Path::new(s);
        if real.starts_with(s) || s.starts_with(&real) {
            return Err(refuse(format!("it is, is inside or holds {}", s.display())));
        }
    }
    let mut count = 0;
    walk(&fd, &real, &mount_of(&fd)?, &mut count).map_err(refuse)?;
    Ok(Checked { real, fd })
}

/// The place of a protected folder. It is opened like a read path, but a
/// link in its name is resolved first.
fn protected_place(path: &Path) -> io::Result<Place> {
    let real = std::fs::canonicalize(path)?;
    let fd = open_no_links(&real)?;
    place_of(&fd, &real)
}

/// The protected home folders: the real path of `home`, the home folder of
/// the user of the engine, and the home folder of each account of the user
/// database. If the user database gives no home folder for the user of the
/// engine, the error.
fn homes(home: Option<&OsStr>) -> Result<Vec<PathBuf>, String> {
    // getpwent has one state for the process.
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<PathBuf> = home.map(PathBuf::from).into_iter().collect();
    let dir = |pw: *mut libc::passwd| -> Option<PathBuf> {
        // SAFETY: pw is null or a valid entry until the next call of the
        // user database, which the lock keeps out.
        unsafe {
            if pw.is_null() || (*pw).pw_dir.is_null() {
                return None;
            }
            let d = CStr::from_ptr((*pw).pw_dir).to_bytes();
            (!d.is_empty()).then(|| PathBuf::from(OsString::from_vec(d.to_vec())))
        }
    };
    // SAFETY: geteuid and getpwuid have no memory effects other than the
    // static entry that `dir` reads under the lock.
    let mine = dir(unsafe { libc::getpwuid(libc::geteuid()) })
        .ok_or("the user database gives no home folder for the user of Iris")?;
    out.push(mine);
    // SAFETY: as above; setpwent and endpwent bracket the enumeration.
    unsafe {
        libc::setpwent();
        loop {
            let pw = libc::getpwent();
            if pw.is_null() {
                break;
            }
            if let Some(d) = dir(pw) {
                out.push(d);
            }
        }
        libc::endpwent();
    }
    Ok(out)
}

/// Opens `real` with no link in any part. Linux: `openat2` with
/// `RESOLVE_NO_SYMLINKS` and `O_PATH`. macOS: `O_NOFOLLOW` for the last
/// part; Seatbelt checks the real path of each access.
pub fn open_no_links(real: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(real.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("the path holds a NUL byte"))?;
    #[cfg(target_os = "linux")]
    let fd = {
        // SAFETY: open_how is plain data; zero is a valid value.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW) as u64;
        how.resolve = libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;
        // SAFETY: c and how are valid for the call.
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                libc::AT_FDCWD,
                c.as_ptr(),
                &how as *const libc::open_how,
                std::mem::size_of::<libc::open_how>(),
            ) as RawFd
        }
    };
    #[cfg(not(target_os = "linux"))]
    // SAFETY: c is a valid C string.
    let fd = unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The mount ID (Linux) or the device (macOS) of an open object.
fn mount_of(fd: &OwnedFd) -> Result<u64, String> {
    mount_at(fd.as_raw_fd(), c"").map_err(|e| format!("its mount cannot be read: {e}"))
}

#[cfg(target_os = "linux")]
fn mount_at(dir: RawFd, name: &CStr) -> io::Result<u64> {
    // SAFETY: statx writes into sx; name is a valid C string.
    let mut sx: libc::statx = unsafe { std::mem::zeroed() };
    let flags = libc::AT_SYMLINK_NOFOLLOW
        | libc::AT_NO_AUTOMOUNT
        | if name.is_empty() {
            libc::AT_EMPTY_PATH
        } else {
            0
        };
    let r = unsafe { libc::statx(dir, name.as_ptr(), flags, libc::STATX_MNT_ID, &mut sx) };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    if sx.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(io::Error::other(
            "the kernel gives no mount ID (Linux 5.8 or later)",
        ));
    }
    Ok(sx.stx_mnt_id)
}

#[cfg(not(target_os = "linux"))]
fn mount_at(dir: RawFd, name: &CStr) -> io::Result<u64> {
    // SAFETY: fstatat writes into st; name is a valid C string.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let (n, flags) = if name.is_empty() {
        (c".", 0)
    } else {
        (name, libc::AT_SYMLINK_NOFOLLOW)
    };
    if unsafe { libc::fstatat(dir, n.as_ptr(), &mut st, flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(st.st_dev as u64)
}

/// The place of the open object `fd`, whose real path is `real`.
#[cfg(target_os = "linux")]
fn place_of(fd: &OwnedFd, real: &Path) -> io::Result<Place> {
    let id = mount_at(fd.as_raw_fd(), c"")?;
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    let m = parse_mountinfo(&text)
        .ok_or_else(|| io::Error::other("/proc/self/mountinfo cannot be parsed"))?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| io::Error::other("its mount is not in /proc/self/mountinfo"))?;
    Ok(m.place(real))
}

#[cfg(not(target_os = "linux"))]
fn place_of(fd: &OwnedFd, real: &Path) -> io::Result<Place> {
    Ok(Place {
        dev: mount_at(fd.as_raw_fd(), c"")?.to_string(),
        path: real.to_path_buf(),
        fstype: String::new(),
    })
}

/// One line of `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub id: u64,
    pub dev: String,
    pub root: PathBuf,
    pub point: PathBuf,
    pub fstype: String,
}

impl Mount {
    /// The place of `real`, a path at or below the mount point.
    pub fn place(&self, real: &Path) -> Place {
        let rest = real.strip_prefix(&self.point).unwrap_or(real);
        Place {
            dev: self.dev.clone(),
            path: if rest.as_os_str().is_empty() {
                self.root.clone()
            } else {
                self.root.join(rest)
            },
            fstype: self.fstype.clone(),
        }
    }
}

/// Parses `/proc/self/mountinfo` (proc(5)). `None` if a line has another
/// form, so the caller fails closed.
pub fn parse_mountinfo(text: &str) -> Option<Vec<Mount>> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let f: Vec<&str> = left.split(' ').collect();
            let r: Vec<&str> = right.split(' ').collect();
            if f.len() < 6 || r.is_empty() || !f[2].contains(':') {
                return None;
            }
            Some(Mount {
                id: f[0].parse().ok()?,
                dev: f[2].to_string(),
                root: unescape(f[3])?,
                point: unescape(f[4])?,
                fstype: r[0].to_string(),
            })
        })
        .collect()
}

/// The octal escapes of `mountinfo` (`\040` for a space, `\011`, `\012`,
/// `\134`). An absolute path only.
fn unescape(s: &str) -> Option<PathBuf> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let oct = b.get(i + 1..i + 4)?;
            if !oct.iter().all(|c| (b'0'..=b'7').contains(c)) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(oct).ok()?, 8).ok()?);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let p = PathBuf::from(OsString::from_vec(out));
    p.is_absolute().then_some(p)
}

/// Walks the tree of the open object `fd` with no link followed. Refuses a
/// socket, a FIFO, a device, an entry of another mount than `mount`, a
/// folder that cannot be read, and more than [`MAX_ENTRIES`] entries.
fn walk(fd: &OwnedFd, path: &Path, mount: &u64, count: &mut usize) -> Result<(), String> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat writes into st.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(format!(
            "{} cannot be read: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    if st.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return kind_ok(st.st_mode, path);
    }
    // A descriptor to read the names: O_PATH (Linux) cannot list.
    // SAFETY: openat with a valid C string.
    let dir = unsafe {
        libc::openat(
            fd.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if dir < 0 {
        return Err(format!(
            "the folder {} cannot be read: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    // SAFETY: dir is a new descriptor; fdopendir takes it, closedir closes it.
    let d = unsafe { libc::fdopendir(dir) };
    if d.is_null() {
        unsafe { libc::close(dir) };
        return Err(format!("the folder {} cannot be read", path.display()));
    }
    struct Dir(*mut libc::DIR);
    impl Drop for Dir {
        fn drop(&mut self) {
            // SAFETY: the pointer came from fdopendir and is closed once.
            unsafe { libc::closedir(self.0) };
        }
    }
    let d = Dir(d);
    loop {
        // SAFETY: readdir on an open DIR; the entry is valid until the
        // next call, and the name is copied before it.
        let e = unsafe { libc::readdir(d.0) };
        if e.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*e).d_name.as_ptr()) }.to_owned();
        if name.as_bytes() == b"." || name.as_bytes() == b".." {
            continue;
        }
        *count += 1;
        let child = path.join(OsStr::from_bytes(name.as_bytes()));
        if *count > MAX_ENTRIES {
            return Err(format!("its tree has more than {MAX_ENTRIES} entries"));
        }
        let fdir = unsafe { libc::dirfd(d.0) };
        let mut cs: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstatat writes into cs; name is a valid C string.
        if unsafe { libc::fstatat(fdir, name.as_ptr(), &mut cs, libc::AT_SYMLINK_NOFOLLOW) } != 0 {
            return Err(format!(
                "{} cannot be read: {}",
                child.display(),
                io::Error::last_os_error()
            ));
        }
        let kind = cs.st_mode & libc::S_IFMT;
        if kind == libc::S_IFLNK {
            continue;
        }
        kind_ok(cs.st_mode, &child)?;
        let m = mount_at(fdir, &name)
            .map_err(|e| format!("the mount of {} cannot be read: {e}", child.display()))?;
        if m != *mount {
            return Err(format!("{} is a mount point", child.display()));
        }
        if kind == libc::S_IFDIR {
            // SAFETY: openat with a valid C string; O_NOFOLLOW keeps a link
            // that replaced the folder out.
            let sub = unsafe {
                libc::openat(
                    fdir,
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if sub < 0 {
                return Err(format!(
                    "the folder {} cannot be read: {}",
                    child.display(),
                    io::Error::last_os_error()
                ));
            }
            // SAFETY: sub is a new descriptor that nothing else owns.
            let sub = unsafe { OwnedFd::from_raw_fd(sub) };
            walk(&sub, &child, mount, count)?;
        }
    }
    Ok(())
}

/// A regular file, a folder or a link passes. A socket, a FIFO or a device
/// does not.
fn kind_ok(mode: libc::mode_t, path: &Path) -> Result<(), String> {
    match mode & libc::S_IFMT {
        libc::S_IFREG | libc::S_IFDIR | libc::S_IFLNK => Ok(()),
        _ => Err(format!(
            "{} is a socket, a FIFO or a device",
            path.display()
        )),
    }
}

#[cfg(test)]
#[path = "read_paths_tests.rs"]
mod tests;
