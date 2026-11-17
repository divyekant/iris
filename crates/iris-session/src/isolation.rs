//! The isolation interface of a session (launcher spec "Isolation"). The
//! sandbox of each system is in `docs/specs/2026-11-17-session-sandbox.md`:
//! [`linux`] on Linux. On another system [`system`] gives none, and a run is
//! refused.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Child;

#[cfg(target_os = "linux")]
#[path = "isolation_linux.rs"]
pub mod linux;

/// What the hold helper runs: a program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// Runs the agent of a session so that it cannot reach the data folder, the
/// engine socket, the socket of another session, or the network other than
/// the egress port of the session.
///
/// Lifetime rule: when the helper process ends for any reason, also
/// `SIGKILL`, each process in the isolation of the session ends, with no
/// call from the engine. An isolation that cannot keep this rule must fail
/// [`Isolation::check`].
pub trait Isolation: Send + Sync {
    /// Checks that the isolation works on this system. The error is the
    /// reason, for the person.
    fn check(&self) -> Result<(), String>;

    /// The program and arguments that run `program args` inside the
    /// isolation of `session`, in `workspace`, with `egress_port` as the
    /// only network that it permits (none if `None`).
    fn command(
        &self,
        session: i64,
        workspace: &Path,
        egress_port: Option<u16>,
        program: &Path,
        args: &[String],
    ) -> Command;

    /// The durable identity of the isolation of one session. It depends
    /// only on the session ID, so a new engine finds it after a restart.
    fn name(&self, session: i64) -> String;

    /// Ends each process in the isolation of the session, also one that left
    /// the process group. Returns when no process of the session runs, or
    /// with an error.
    fn stop(&self, session: i64) -> io::Result<()>;

    /// True only if the isolation holds no process of the session.
    fn ended(&self, session: i64) -> io::Result<bool>;

    /// Starts the command that [`Isolation::command`] gave, in the hold
    /// helper. It returns when the agent can run. If it fails after a
    /// process started, it kills and reaps that process first, so no
    /// process of the session stays and no code of the agent ran.
    fn spawn(&self, session: i64, command: &mut std::process::Command) -> io::Result<Child> {
        let _ = session;
        command.spawn()
    }
}

/// The isolation of this system for the data folder `data_dir`, or the
/// reason. A run with no isolation is refused: no agent runs as the user
/// with access to the data folder. The engine and the helper make it with
/// the same data folder.
pub fn system(data_dir: &Path) -> Result<Box<dyn Isolation>, String> {
    #[cfg(target_os = "linux")]
    {
        linux::Bwrap::new(data_dir).map(|b| Box::new(b) as Box<dyn Isolation>)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = data_dir;
        Err(
            "agent sessions need Linux: this system has no sandbox that can stop each \
             process of a session for sure (R4-05)"
                .into(),
        )
    }
}

/// The test isolation: it runs the command with no isolation and records
/// each call. Only tests can make it (`cfg(test)` or the feature
/// `test-isolation`, which the program does not enable).
#[cfg(any(test, feature = "test-isolation"))]
pub mod test {
    use super::*;
    use std::sync::Mutex;

    /// A call to the test isolation.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Stop(i64),
        Ended(i64),
    }

    #[derive(Default)]
    pub struct TestIsolation {
        pub calls: Mutex<Vec<Call>>,
        /// The count of the next `stop` calls that fail.
        pub fail_stops: Mutex<u32>,
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn a_system_other_than_linux_has_no_isolation() {
        assert!(system(Path::new("/tmp")).is_err());
    }

    impl Isolation for TestIsolation {
        fn check(&self) -> Result<(), String> {
            Ok(())
        }

        fn command(
            &self,
            _session: i64,
            _workspace: &Path,
            _egress_port: Option<u16>,
            program: &Path,
            args: &[String],
        ) -> Command {
            Command {
                program: program.to_path_buf(),
                args: args.iter().map(OsString::from).collect(),
            }
        }

        fn name(&self, session: i64) -> String {
            format!("test-{session}")
        }

        fn stop(&self, session: i64) -> io::Result<()> {
            self.calls.lock().unwrap().push(Call::Stop(session));
            let mut fail = self.fail_stops.lock().unwrap();
            if *fail > 0 {
                *fail -= 1;
                return Err(io::Error::other("test stop failure"));
            }
            Ok(())
        }

        /// The test isolation has no process of its own, so it confirms at
        /// once.
        fn ended(&self, session: i64) -> io::Result<bool> {
            self.calls.lock().unwrap().push(Call::Ended(session));
            Ok(true)
        }
    }
}
