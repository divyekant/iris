//! The engine socket (`docs/specs/2026-11-16-engine-socket.md`): `iris.sock`
//! in the data folder, mode `0600`. One request and one answer on each
//! connection, each one line of JSON. The command verbs that change
//! changesets go through it, never through the store (specification 2,
//! section 8.1).

use std::io::Write;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iris_changeset::{format_changeset_id, parse_changeset_id, Author, AuthorKind, Verb};
use iris_store::{FileLock, Store};
use serde_json::{json, Value};

/// The socket file in the data folder.
pub const SOCKET: &str = "iris.sock";
/// The engine lock in the data folder: one engine for each data folder.
pub const LOCK: &str = "engine.lock";
/// The longest request line, with its newline. It holds each draft whose
/// content passes the checks (`iris_compose::MAX_BODY`): JSON writes one
/// byte of a body as at most 6 bytes (`\u0001`), and 64 KiB holds the
/// other fields. The engine reads a line only from the same user, and
/// grows the buffer only as bytes arrive.
pub const MAX_LINE: usize = 6 * iris_compose::MAX_BODY + 64 * 1024;
const MAX_LINE_TEXT: &str = "the request is longer than the limit (1.6 MiB)";
/// The longest wait of the engine for a request line.
pub const READ_WAIT: Duration = Duration::from_secs(10);
/// The most connections at the same time.
pub const MAX_CONNECTIONS: usize = 64;
/// The longest wait of the command for the answer.
pub const ANSWER_WAIT: Duration = Duration::from_secs(30);
/// The most drafts that `drafts` lists.
pub const MAX_DRAFTS: i64 = 200;

/// The exit codes of specification 2, section 8.4.
pub mod code {
    pub const ERROR: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const DENIED: u8 = 3;
    pub const UNREACHABLE: u8 = 4;
    pub const OPEN: u8 = 5;
}

/// The author of each request in host mode.
pub fn local() -> Author {
    Author {
        kind: AuthorKind::Local,
        name: None,
        session: None,
        job: None,
    }
}

/// A failed request: the exit code and the reason.
#[derive(Debug, PartialEq)]
pub struct Failure {
    pub code: u8,
    pub error: String,
}

impl Failure {
    fn new(code: u8, error: impl Into<String>) -> Failure {
        Failure {
            code,
            error: error.into(),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

/// The engine side of the socket: the lock, and the bound socket. The drop
/// removes the socket file, then releases the lock.
pub struct Bound {
    pub listener: std::os::unix::net::UnixListener,
    pub path: PathBuf,
    /// The user ID that owns the engine: the owner of the lock file that
    /// this process made or opened for writing.
    pub uid: u32,
    _lock: FileLock,
}

impl Drop for Bound {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Take the engine lock of `dir`, remove a socket file that a stopped engine
/// left, bind the socket and set its mode to `0600`.
pub fn bind(dir: &Path) -> std::io::Result<Bound> {
    let lock_path = dir.join(LOCK);
    let Some(lock) = FileLock::try_take(&lock_path)? else {
        return Err(std::io::Error::other(format!(
            "another engine runs on this data folder ({})",
            dir.display()
        )));
    };
    let uid = std::fs::metadata(&lock_path)?.uid();
    let path = dir.join(SOCKET);
    match std::fs::symlink_metadata(&path) {
        // With the lock, a socket file is left from a stopped engine.
        Ok(m) if m.file_type().is_socket() => std::fs::remove_file(&path)?,
        Ok(_) => {
            return Err(std::io::Error::other(format!(
                "{} is not a socket: move it away, then start the engine",
                path.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    // The data folder has the mode 0700, so no other user reaches the file
    // before this change.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Bound {
        listener,
        path,
        uid,
        _lock: lock,
    })
}

/// The limits of the socket. Tests set smaller ones.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub read_wait: Duration,
    pub write_wait: Duration,
    pub connections: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            read_wait: READ_WAIT,
            write_wait: WRITE_WAIT,
            connections: MAX_CONNECTIONS,
        }
    }
}

/// Accept connections until the future ends. Each connection is one task.
pub async fn serve(bound: &Bound, store: Arc<Store>) -> std::io::Result<()> {
    serve_with(bound, store, Limits::default()).await
}

/// [`serve`] with other limits.
pub async fn serve_with(bound: &Bound, store: Arc<Store>, limits: Limits) -> std::io::Result<()> {
    bound.listener.set_nonblocking(true)?;
    let listener = tokio::net::UnixListener::from_std(bound.listener.try_clone()?)?;
    let slots = Arc::new(tokio::sync::Semaphore::new(limits.connections));
    let uid = bound.uid;
    loop {
        // At most MAX_CONNECTIONS: the next accept waits for a free slot.
        let slot = slots.clone().acquire_owned().await.expect("never closed");
        let (stream, _) = listener.accept().await?;
        let store = store.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let _ = connection(stream, store, uid, limits).await;
        });
    }
}

/// True when the peer runs as the user of the engine.
pub fn same_user(peer: u32, engine: u32) -> bool {
    peer == engine
}

async fn connection(
    stream: tokio::net::UnixStream,
    store: Arc<Store>,
    uid: u32,
    limits: Limits,
) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    // Another user: close with no answer.
    if !same_user(stream.peer_cred()?.uid(), uid) {
        return Ok(());
    }
    let mut line = Vec::new();
    let mut limited = tokio::io::BufReader::new(stream.take(MAX_LINE as u64));
    let got = tokio::time::timeout(limits.read_wait, limited.read_until(b'\n', &mut line)).await;
    // One request on each connection: bytes after the line are dropped.
    let stream = limited.into_inner().into_inner();
    match got {
        // A timeout with no byte: close with no answer. A partial line (the
        // read appends what it got before the timeout) gets code 2 below.
        Err(_) if line.is_empty() => return Ok(()),
        Err(_) => {}
        Ok(Err(e)) => return Err(e),
        Ok(Ok(_)) => {}
    }
    // The work and the answer run on a blocking thread, with one deadline
    // for the whole answer: the answer streams, so memory stays at one
    // buffer, and a peer that reads slowly or not at all cannot hold the
    // slot for more than the write wait.
    let stream = stream.into_std()?;
    tokio::task::spawn_blocking(move || {
        stream.set_nonblocking(false)?;
        let mut out = std::io::BufWriter::new(Deadline {
            stream: &stream,
            end: Instant::now() + limits.write_wait,
        });
        if line.last() != Some(&b'\n') {
            out.write_all(&failure_line(&usage(
                "the request is longer than the limit (1.6 MiB), or has no newline",
            )))?;
        } else {
            answer(&store, &line, &mut out)?;
        }
        out.flush()?;
        drop(out);
        stream.shutdown(std::net::Shutdown::Write)
    })
    .await
    .unwrap_or_else(|_| Err(std::io::Error::other("the engine failed")))
}

/// A writer that fails after `end`: one deadline for all writes.
struct Deadline<'a> {
    stream: &'a std::os::unix::net::UnixStream,
    end: Instant,
}

impl Write for Deadline<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_write_timeout(Some(left))?;
        (&mut &*self.stream).write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The longest time that the engine writes an answer to a peer.
pub const WRITE_WAIT: Duration = Duration::from_secs(30);

fn failure_line(f: &Failure) -> Vec<u8> {
    let mut v = serde_json::to_vec(&json!({"ok": false, "code": f.code, "error": f.error}))
        .expect("plain JSON");
    v.push(b'\n');
    v
}

/// The result of a request: a changeset document, which streams from the
/// store, or a small JSON value.
enum Done {
    Document(i64),
    Value(Value),
}

/// Writes `prefix` before the first byte, so a document that fails before
/// its first byte leaves `out` empty and the answer can be a failure.
struct Prefixed<'a, W: Write> {
    out: &'a mut W,
    prefix: Option<&'static [u8]>,
}

impl<W: Write> Write for Prefixed<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(p) = self.prefix.take() {
            self.out.write_all(p)?;
        }
        self.out.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

const OK: &[u8] = b"{\"ok\":true,\"result\":";

/// Write the answer line for one request line to `out`. Each request is one
/// transaction. A changeset document streams. If a document fails after its
/// first byte, the error ends the connection, and the command says that the
/// result is not known.
pub fn answer(store: &Store, line: &[u8], out: &mut impl Write) -> std::io::Result<()> {
    match work(store, line) {
        Err(f) => out.write_all(&failure_line(&f)),
        Ok(Done::Value(v)) => {
            out.write_all(OK)?;
            serde_json::to_writer(&mut *out, &v)?;
            out.write_all(b"}\n")
        }
        Ok(Done::Document(cs)) => {
            let mut w = Prefixed {
                out: &mut *out,
                prefix: Some(OK),
            };
            match iris_changeset::write_document(store, cs, &mut w) {
                Ok(()) => out.write_all(b"}\n"),
                Err(e) if w.prefix.is_some() => out.write_all(&failure_line(&failure(e, cs))),
                Err(e) => Err(std::io::Error::other(e.to_string())),
            }
        }
    }
}

fn usage(why: impl Into<String>) -> Failure {
    Failure::new(code::USAGE, why)
}

fn text<'a>(
    args: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, Failure> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(usage(format!("{name} must be a text"))),
    }
}

fn work(store: &Store, line: &[u8]) -> Result<Done, Failure> {
    let request: Value =
        serde_json::from_slice(line).map_err(|e| usage(format!("the request is not JSON: {e}")))?;
    let Some(fields) = request.as_object() else {
        return Err(usage("the request is not a JSON object"));
    };
    if fields
        .keys()
        .any(|k| !matches!(k.as_str(), "v" | "verb" | "args"))
    {
        return Err(usage("the request has an unknown field"));
    }
    if request["v"] != json!(1) {
        return Err(usage("the request version must be 1"));
    }
    let args = request["args"]
        .as_object()
        .ok_or_else(|| usage("args must be a JSON object"))?;
    let only = |names: &[&str]| -> Result<(), Failure> {
        match args.keys().find(|k| !names.contains(&k.as_str())) {
            Some(k) => Err(usage(format!("unknown argument {k}"))),
            None => Ok(()),
        }
    };
    let changeset = || -> Result<Option<i64>, Failure> {
        text(args, "changeset")?
            .map(|t| {
                parse_changeset_id(t).ok_or_else(|| usage(format!("{t} is not a changeset ID")))
            })
            .transpose()
    };
    let need = || changeset()?.ok_or_else(|| usage("give the changeset ID, for example cs-12"));
    let me = local();
    match request["verb"].as_str() {
        Some("show") => {
            only(&["changeset"])?;
            Ok(Done::Document(need()?))
        }
        Some("withdraw") => {
            only(&["changeset"])?;
            let cs = need()?;
            iris_changeset::withdraw(store, cs, Some(&me)).map_err(|e| failure(e, cs))?;
            Ok(Done::Document(cs))
        }
        Some("drafts") => {
            only(&[])?;
            drafts(store).map(Done::Value)
        }
        Some("add") => {
            only(&["verb", "targets", "args", "why", "changeset"])?;
            let verb = text(args, "verb")?
                .and_then(Verb::parse)
                .ok_or_else(|| usage("verb must be a verb of specification 2, section 4"))?;
            let targets: Vec<&str> = args["targets"]
                .as_array()
                .filter(|t| !t.is_empty())
                .and_then(|t| t.iter().map(Value::as_str).collect())
                .ok_or_else(|| usage("targets must be a list of one or more IDs"))?;
            let action_args = match args.get("args") {
                None => serde_json::Map::new(),
                Some(Value::Object(m)) => m.clone(),
                Some(_) => return Err(usage("args of the action must be a JSON object")),
            };
            let why = text(args, "why")?;
            let given = changeset()?;
            let now = crate::engine::now_ms();
            let (cs, n) = iris_changeset::transaction(store, |c| {
                let cs = match given {
                    Some(cs) => cs,
                    None => iris_changeset::create_in(c, &me, now)?,
                };
                let n =
                    iris_changeset::add_action_in(c, cs, &me, verb, &targets, &action_args, why)?;
                Ok((cs, n))
            })
            .map_err(|e| failure(e, given.unwrap_or_default()))?;
            Ok(Done::Value(
                json!({ "changeset": format_changeset_id(cs), "n": n }),
            ))
        }
        Some("submit") => {
            only(&["changeset", "why"])?;
            let cs = need()?;
            let why = text(args, "why")?.ok_or_else(|| usage("give the reason with --why"))?;
            let now = crate::engine::now_ms();
            // The submit and the decisions of the policy in one transaction.
            iris_changeset::transaction(store, |c| {
                iris_changeset::submit_in(c, cs, &me, why, now)?;
                let revision: i64 =
                    c.query_row("SELECT revision FROM changeset WHERE id = ?1", [cs], |r| {
                        r.get(0)
                    })?;
                iris_triage::decide_submitted(c, cs, revision, iris_policy::Author::Local, now)
            })
            .map_err(|e| failure(e, cs))?;
            Ok(Done::Document(cs))
        }
        Some(other) => Err(usage(format!("unknown verb {other}"))),
        None => Err(usage("the request has no verb")),
    }
}

fn failure(e: iris_changeset::Error, cs: i64) -> Failure {
    match e {
        iris_changeset::Error::NotFound => {
            usage(format!("no changeset {}", format_changeset_id(cs)))
        }
        iris_changeset::Error::Refused(why) => usage(why),
        iris_changeset::Error::Store(e) => Failure::new(code::ERROR, format!("store: {e}")),
    }
}

/// The drafts of the author `local`, the newest first.
fn drafts(store: &Store) -> Result<Value, Failure> {
    let ids: Vec<i64> = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT id FROM changeset WHERE author_kind = 'local' AND state = 'draft'
                 ORDER BY id DESC LIMIT ?1",
            )?;
            let ids = st
                .query_map([MAX_DRAFTS], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })
        .map_err(|e| Failure::new(code::ERROR, format!("store: {e}")))?;
    let ids: Vec<String> = ids.into_iter().map(format_changeset_id).collect();
    Ok(json!({ "changesets": ids }))
}

/// The most bytes of an answer that the command reads.
pub const MAX_ANSWER: u64 = 64 * 1024 * 1024;

/// The command side: send one request to the engine of `dir`, and give the
/// result. Code 4 when no engine answers the connect. Code 1 when the
/// answer does not come.
pub fn request(dir: &Path, verb: &str, args: Value) -> Result<Value, Failure> {
    request_with(dir, verb, args, ANSWER_WAIT)
}

/// [`request`] with another wait for the whole answer, for tests.
pub fn request_with(dir: &Path, verb: &str, args: Value, wait: Duration) -> Result<Value, Failure> {
    let mut line =
        serde_json::to_vec(&json!({"v": 1, "verb": verb, "args": args})).expect("plain JSON");
    line.push(b'\n');
    if line.len() > MAX_LINE {
        return Err(usage(MAX_LINE_TEXT));
    }
    let path = dir.join(SOCKET);
    let mut stream = std::os::unix::net::UnixStream::connect(&path).map_err(|_| {
        Failure::new(
            code::UNREACHABLE,
            "the engine is not reachable: start it with `iris serve`",
        )
    })?;
    let unknown = |hint: &str| {
        Failure::new(
            code::ERROR,
            format!("the result is not known: run `iris changeset {hint}`"),
        )
    };
    let hint = args["changeset"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| "--drafts".into());
    stream
        .set_read_timeout(Some(wait))
        .and_then(|()| stream.set_write_timeout(Some(wait)))
        .and_then(|()| stream.write_all(&line))
        .map_err(|_| unknown(&hint))?;
    // One deadline for the whole answer: a peer that sends one byte at a
    // time cannot hold the command past `wait`. At most MAX_ANSWER.
    let deadline = Instant::now() + wait;
    let mut answer = Vec::new();
    let mut chunk = [0u8; 8192];
    while answer.last() != Some(&b'\n') {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || answer.len() as u64 > MAX_ANSWER {
            return Err(unknown(&hint));
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|_| unknown(&hint))?;
        match std::io::Read::read(&mut stream, &mut chunk) {
            Ok(0) | Err(_) => return Err(unknown(&hint)),
            Ok(n) => answer.extend_from_slice(&chunk[..n]),
        }
    }
    parse_answer(verb, &answer).ok_or_else(|| unknown(&hint))?
}

/// Read one answer line of the engine for `verb`. `None` when the line is
/// not a valid answer: the result is then not known. A success must carry
/// the result shape of the verb; a failure must carry a code of 1 to 5.
pub fn parse_answer(verb: &str, line: &[u8]) -> Option<Result<Value, Failure>> {
    let answer: Value = serde_json::from_slice(line).ok()?;
    match answer["ok"].as_bool()? {
        true => {
            let r = &answer["result"];
            let valid = match verb {
                "drafts" => r["changesets"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string)),
                "add" => r["changeset"].is_string() && r["n"].is_u64(),
                _ => r["id"].is_string() && r["state"].is_string() && r["actions"].is_array(),
            };
            valid.then(|| Ok(r.clone()))
        }
        false => {
            let code = answer["code"]
                .as_u64()
                .and_then(|c| u8::try_from(c).ok())
                .filter(|c| (1..=5).contains(c))?;
            Some(Err(Failure::new(
                code,
                answer["error"].as_str()?.to_string(),
            )))
        }
    }
}
