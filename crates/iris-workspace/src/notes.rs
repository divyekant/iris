//! The memory of a job between sessions (docs/specs/2026-11-17-job-memory.md):
//! a copy of `notes.md` in the store, saved while no process of the job runs,
//! and written back when the file is not in the workspace.

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::Path;

use iris_job::{Result, NOTES_MAX};
use iris_store::Store;

/// The result of [`save_notes`].
#[derive(Debug, PartialEq, Eq)]
pub enum Notes {
    /// The store copy is now the bytes of the file.
    Saved,
    /// The job is done: it has no copy.
    Done,
    /// No `notes.md`. The copy did not change.
    Absent,
    /// `notes.md` is a link, a folder or another file that is not a regular
    /// file. The copy did not change, and Iris read nothing through it.
    NotAFile,
    /// `notes.md` has more than [`NOTES_MAX`] bytes. The copy did not change.
    TooLarge,
    /// Another error of the open or the read (for example, no read
    /// permission). The copy did not change.
    Unreadable(String),
}

impl Notes {
    /// The line for the engine log when a save kept the last copy, or
    /// `None` when there is nothing to report. Each reap and build path
    /// uses this one text.
    pub fn problem(&self) -> Option<String> {
        let why = match self {
            Notes::Saved | Notes::Done | Notes::Absent => return None,
            Notes::NotAFile => "notes.md is not a regular file".to_string(),
            Notes::TooLarge => format!("notes.md is larger than {NOTES_MAX} bytes"),
            Notes::Unreadable(e) => format!("notes.md cannot be read: {e}"),
        };
        Some(format!("the store keeps its last copy of the notes: {why}"))
    }
}

/// Read `<dir>/notes.md` for a save: the bytes of a regular file of at most
/// [`NOTES_MAX`] bytes, else the reason that there are none. Read it only
/// while no process of the job can write the file.
pub fn read_notes(dir: &Path) -> std::result::Result<Vec<u8>, Notes> {
    let unreadable = |e: io::Error| Notes::Unreadable(e.to_string());
    let mut o = OpenOptions::new();
    o.read(true);
    // No read through a link, and no block on a FIFO.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match o.open(dir.join("notes.md")) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Notes::Absent),
        #[cfg(unix)]
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(Notes::NotAFile),
        Err(e) => return Err(unreadable(e)),
    };
    // The type of the open file, not of the path.
    if !file.metadata().map_err(unreadable)?.is_file() {
        return Err(Notes::NotAFile);
    }
    let mut text = Vec::new();
    file.take(NOTES_MAX as u64 + 1)
        .read_to_end(&mut text)
        .map_err(unreadable)?;
    if text.len() > NOTES_MAX {
        return Err(Notes::TooLarge);
    }
    Ok(text)
}

/// Save `<dir>/notes.md` of `job` in the store. Call it only while no
/// process of the job can write the file: at a build. Only a store error is
/// `Err`.
pub fn save_notes(store: &Store, job: i64, dir: &Path, now_ms: i64) -> Result<Notes> {
    match read_notes(dir) {
        Ok(text) => Ok(match iris_job::save_notes(store, job, &text, now_ms)? {
            true => Notes::Saved,
            false => Notes::Done,
        }),
        Err(why) => Ok(why),
    }
}
