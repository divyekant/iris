//! The reap check of a session (spec "The reap check"). The engine sets
//! `reaped_at` only after this check confirms that each process of the
//! session ended. No wait and no time sets it.

use std::io;

use iris_job::Session;
use iris_store::Store;
use iris_workspace::{default_dir, read_notes, Notes};

use crate::isolation::Isolation;

/// Rules 1 to 3 of the reap check. Rule 0 (no launch thread holds the
/// session) is the duty of the caller: the launch runs the check only after
/// its own cleanup, and the supervisor skips each session of a running
/// launch. `helper_reaped` is true if this engine reaped the helper (`wait`
/// gave its exit): a helper that is a zombie of this engine still answers
/// `kill(pid, 0)`.
///
/// If each rule holds, it reads `notes.md` of the job, and sets `reaped_at`
/// and saves the copy of the notes in one write transaction
/// (docs/specs/2026-11-17-job-memory.md). The session is live until that
/// transaction commits, so no new session of the job can write the file
/// during the read. It gives the result of the save: a bad `notes.md` does
/// not stop the reap. Else it gives the reason, and the session stays live.
pub fn reap_check(
    store: &Store,
    isolation: &dyn Isolation,
    session: &Session,
    helper_reaped: bool,
    now_ms: i64,
) -> Result<Notes, String> {
    if session.ended_ms.is_none() {
        return Err("the session did not end".into());
    }
    // 1. The helper ended.
    if !helper_reaped {
        if let Some(pid) = session.helper_pid {
            if alive(pid as i32)? {
                return Err(format!("the helper (process {pid}) still runs"));
            }
        }
    }
    // 2. The group of the agent is empty.
    if let Some(pgid) = session.agent_pgid {
        if alive(-(pgid as i32))? {
            return Err(format!(
                "the process group {pgid} of the agent is not empty"
            ));
        }
    }
    // 3. The isolation holds no process of the session.
    isolation
        .stop(session.id)
        .map_err(|e| format!("the isolation stop failed: {e}"))?;
    match isolation.ended(session.id) {
        Ok(true) => {}
        Ok(false) => return Err("the isolation still holds a process of the session".into()),
        Err(e) => return Err(format!("the isolation check failed: {e}")),
    }
    // The private files of the session go only now; a failure keeps the
    // session live, and the next turn tries again.
    isolation
        .remove_files(session.id)
        .map_err(|e| format!("the isolation files were not removed: {e}"))?;
    let read = read_notes(&default_dir(store.dir(), session.job_id));
    let saved = iris_job::reap_with_notes(store, session.id, read.as_deref().ok(), now_ms)
        .map_err(|e| format!("the reap write failed: {e}"))?;
    Ok(match (read, saved) {
        (Err(why), _) => why,
        (Ok(_), true) => Notes::Saved,
        (Ok(_), false) => Notes::Done,
    })
}

/// `kill(target, 0)`: a process ID, or a group ID as a negative number.
/// Only `ESRCH` means that it is gone. Each other error keeps the session
/// live (a false "still runs", never a false "ended").
fn alive(target: i32) -> Result<bool, String> {
    // A value of 0, 1 or -1 is never a session process: kill() would reach
    // the group of the engine or each process of the user.
    if (-1..=1).contains(&target) {
        return Err(format!("{target} is not a session process"));
    }
    // SAFETY: kill with signal 0 sends nothing and has no memory effects.
    if unsafe { libc::kill(target, 0) } == 0 {
        return Ok(true);
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        _ => Err(format!("kill({target}, 0) failed: {e}")),
    }
}
