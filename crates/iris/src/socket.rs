//! The engine socket (`docs/specs/2026-11-16-engine-socket.md`): `iris.sock`
//! in the data folder, mode `0600`. One request and one answer on each
//! connection, each one line of JSON. The command verbs that change
//! changesets go through it, never through the store (specification 2,
//! section 8.1).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iris_changeset::{format_changeset_id, parse_changeset_id, Author, AuthorKind};
use iris_store::{FileLock, Store};
use serde_json::{json, Value};

/// The socket file in the data folder.
pub const SOCKET: &str = "iris.sock";
/// The engine lock in the data folder: one engine for each data folder.
pub const LOCK: &str = "engine.lock";
/// The longest request line, with its newline.
pub const MAX_LINE: usize = 64 * 1024;
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
    let listener = std::os::unix::net::UnixListener::bind(&path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("{}: {e}", path.display()))
    })?;
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
    pub connections: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            read_wait: READ_WAIT,
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
            let _ = connection(stream, store, uid, limits.read_wait).await;
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
    read_wait: Duration,
) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    // Another user: close with no answer.
    if !same_user(stream.peer_cred()?.uid(), uid) {
        return Ok(());
    }
    let (read, mut write) = stream.into_split();
    let mut line = Vec::new();
    let mut limited = tokio::io::BufReader::new(read.take(MAX_LINE as u64));
    let got = tokio::time::timeout(read_wait, limited.read_until(b'\n', &mut line)).await;
    let answer = match got {
        Err(_) => return Ok(()),
        Ok(Err(e)) => return Err(e),
        Ok(Ok(_)) if line.last() != Some(&b'\n') => {
            failure_line(&Failure::new(code::USAGE, "the request is longer than 64 KiB, or has no newline"))
        }
        Ok(Ok(_)) => tokio::task::spawn_blocking(move || answer(&store, &line))
            .await
            .unwrap_or_else(|_| failure_line(&Failure::new(code::ERROR, "the engine failed"))),
    };
    write.write_all(&answer).await?;
    write.shutdown().await
}

fn failure_line(f: &Failure) -> Vec<u8> {
    let mut v = serde_json::to_vec(&json!({"ok": false, "code": f.code, "error": f.error}))
        .expect("plain JSON");
    v.push(b'\n');
    v
}

fn ok_line(result: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(result.len() + 16);
    v.extend_from_slice(b"{\"ok\":true,\"result\":");
    v.extend_from_slice(result);
    v.extend_from_slice(b"}\n");
    v
}

/// The answer line for one request line. Each request is one transaction.
pub fn answer(store: &Store, line: &[u8]) -> Vec<u8> {
    match work(store, line) {
        Ok(result) => ok_line(&result),
        Err(f) => failure_line(&f),
    }
}

fn usage(why: impl Into<String>) -> Failure {
    Failure::new(code::USAGE, why)
}

fn work(store: &Store, line: &[u8]) -> Result<Vec<u8>, Failure> {
    let request: Value =
        serde_json::from_slice(line).map_err(|e| usage(format!("the request is not JSON: {e}")))?;
    let Some(fields) = request.as_object() else {
        return Err(usage("the request is not a JSON object"));
    };
    if fields.keys().any(|k| !matches!(k.as_str(), "v" | "verb" | "args")) {
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
    let changeset = || -> Result<i64, Failure> {
        let text = args["changeset"]
            .as_str()
            .ok_or_else(|| usage("give the changeset ID, for example cs-12"))?;
        parse_changeset_id(text).ok_or_else(|| usage(format!("{text} is not a changeset ID")))
    };
    match request["verb"].as_str() {
        Some("show") => {
            only(&["changeset"])?;
            document(store, changeset()?)
        }
        Some("withdraw") => {
            only(&["changeset"])?;
            let cs = changeset()?;
            iris_changeset::withdraw(store, cs, Some(&local())).map_err(|e| failure(e, cs))?;
            document(store, cs)
        }
        Some("drafts") => {
            only(&[])?;
            drafts(store)
        }
        Some(other) => Err(usage(format!("unknown verb {other}"))),
        None => Err(usage("the request has no verb")),
    }
}

fn failure(e: iris_changeset::Error, cs: i64) -> Failure {
    match e {
        iris_changeset::Error::NotFound => usage(format!("no changeset {}", format_changeset_id(cs))),
        iris_changeset::Error::Refused(why) => usage(why),
        iris_changeset::Error::Store(e) => Failure::new(code::ERROR, format!("store: {e}")),
    }
}

/// The changeset document (specification 2, section 5.1).
fn document(store: &Store, cs: i64) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    iris_changeset::write_document(store, cs, &mut out).map_err(|e| failure(e, cs))?;
    Ok(out)
}

/// The drafts of the author `local`, the newest first.
fn drafts(store: &Store) -> Result<Vec<u8>, Failure> {
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
    Ok(serde_json::to_vec(&json!({ "changesets": ids })).expect("plain JSON"))
}

/// The command side: send one request to the engine of `dir`, and give the
/// result. Code 4 when no engine answers the connect. Code 1 when the
/// answer does not come.
pub fn request(dir: &Path, verb: &str, args: Value) -> Result<Value, Failure> {
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
    let mut line = serde_json::to_vec(&json!({"v": 1, "verb": verb, "args": args})).expect("plain JSON");
    line.push(b'\n');
    stream
        .set_read_timeout(Some(ANSWER_WAIT))
        .and_then(|()| stream.set_write_timeout(Some(ANSWER_WAIT)))
        .and_then(|()| stream.write_all(&line))
        .map_err(|_| unknown(&hint))?;
    let mut answer = Vec::new();
    BufReader::new(&mut stream)
        .read_until(b'\n', &mut answer)
        .map_err(|_| unknown(&hint))?;
    let answer: Value = serde_json::from_slice(&answer).map_err(|_| unknown(&hint))?;
    if answer["ok"] == json!(true) {
        return Ok(answer["result"].clone());
    }
    let code = answer["code"]
        .as_u64()
        .and_then(|c| u8::try_from(c).ok())
        .filter(|c| (1..=5).contains(c))
        .unwrap_or(code::ERROR);
    Err(Failure::new(
        code,
        answer["error"].as_str().unwrap_or("the engine failed").to_string(),
    ))
}
