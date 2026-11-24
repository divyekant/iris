//! An OS file lock (`flock` on Linux and macOS) that a drop releases at once.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// An OS file lock. While a value exists, no other holder of the lock on the
/// same path can run: in this process or in another one. The OS releases it
/// when the process ends, also after a crash.
///
/// The drop unlocks the file before it closes it. A child process that a
/// thread spawns gets a copy of each open file between its fork and its exec,
/// and a close alone keeps the lock until that copy closes too. The unlock
/// releases the lock for all copies.
pub struct FileLock(File);

/// The result of [`FileLock::probe`].
pub enum Probe {
    /// The lock file does not exist.
    Missing,
    /// Another holder has the lock.
    Held,
    /// No exclusive holder. The probe holds a shared lock now.
    Free(FileLock),
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

impl FileLock {
    /// Takes the lock on `path`, or gives `None` if another holder has it.
    pub fn try_take(path: &Path) -> io::Result<Option<FileLock>> {
        let f = open(path)?;
        match f.try_lock() {
            Ok(()) => Ok(Some(FileLock(f))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Probes the lock on `path` with a shared lock, and creates no file.
    /// [`Probe::Held`]: a holder of the exclusive lock ([`FileLock::try_take`])
    /// has it. Two probes do not see each other: both get [`Probe::Free`],
    /// which holds the shared lock until its drop.
    pub fn probe(path: &Path) -> io::Result<Probe> {
        let f = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Probe::Missing),
            Err(e) => return Err(e),
        };
        match f.try_lock_shared() {
            Ok(()) => Ok(Probe::Free(FileLock(f))),
            Err(TryLockError::WouldBlock) => Ok(Probe::Held),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Takes the lock on `path`. Waits while another holder has it.
    pub fn take(path: &Path) -> io::Result<FileLock> {
        let f = open(path)?;
        f.lock()?;
        Ok(FileLock(f))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // An error here leaves the lock to the close, as before.
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Invariant: in one process, a lock that a drop released is free for the
    // next taker at once, also while other threads spawn child processes.
    // Invariant: a probe sees only the exclusive holder, never another
    // probe, and creates no file (issue #710).
    #[test]
    fn a_probe_sees_the_holder_and_not_another_probe() {
        let dir = std::env::temp_dir().join(format!("iris-file-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.lock");
        assert!(matches!(FileLock::probe(&path).unwrap(), Probe::Missing));
        assert!(!path.exists());
        let holder = FileLock::try_take(&path).unwrap().unwrap();
        assert!(matches!(FileLock::probe(&path).unwrap(), Probe::Held));
        drop(holder);
        let first = FileLock::probe(&path).unwrap();
        let second = FileLock::probe(&path).unwrap();
        assert!(matches!(
            (&first, &second),
            (Probe::Free(_), Probe::Free(_))
        ));
        drop((first, second));
        assert!(FileLock::try_take(&path).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dropped_lock_is_free_while_threads_spawn_children() {
        let dir = std::env::temp_dir().join(format!("iris-file-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.lock");
        let stop = Arc::new(AtomicBool::new(false));
        let spawners: Vec<_> = (0..4)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = Command::new("true").status();
                    }
                })
            })
            .collect();
        let mut busy = 0;
        for _ in 0..2_000 {
            match FileLock::try_take(&path).unwrap() {
                Some(held) => drop(held),
                None => busy += 1,
            }
        }
        stop.store(true, Ordering::Relaxed);
        for s in spawners {
            s.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(busy, 0, "a dropped lock was still held");
    }
}
