//! The isolation interface of a session (spec "Isolation"). R4-05 gives an
//! isolation for macOS and one for Linux. Until then [`system`] gives none,
//! and a run is refused.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

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
}

/// The isolation of this system, or `None` with the reason. A run with no
/// isolation is refused: no agent runs as the user with access to the data
/// folder.
pub fn system() -> Result<Box<dyn Isolation>, String> {
    Err("no isolation for sessions on this system yet (R4-05)".into())
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

    #[test]
    fn the_system_has_no_isolation_until_r4_05() {
        assert!(system().is_err());
    }
}
