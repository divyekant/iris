//! The egress proxy of a session (spec "Model egress").
//!
//! Each session gets one [`Proxy`] on a port of `127.0.0.1`. It takes only
//! `CONNECT <host>:443` for a host of the allowed list, resolves the host
//! itself, connects only to a public address that it checked, and relays the
//! bytes. It does not read the bytes of a tunnel. It writes one line for each
//! connection to the egress log, with no text of the request.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The largest request head that the proxy reads.
pub const HEAD_MAX: usize = 8 * 1024;
/// The most connections that a session can have open at one time.
pub const MAX_CONNECTIONS: usize = 8;
/// The size limit of the egress log.
pub const LOG_LIMIT: u64 = 1024 * 1024;
/// The end of the log that only the limit line and the count line can use.
pub const LOG_RESERVE: u64 = 4096;

/// The time that a client has to send its request head.
const HEAD_TIME: Duration = Duration::from_secs(10);
/// The time to connect to one address of the model host.
const CONNECT_TIME: Duration = Duration::from_secs(10);
/// The time that a refusal waits for the rest of the request before it closes.
const DRAIN_TIME: Duration = Duration::from_secs(1);
/// The relay buffer of each direction.
const BUF: usize = 16 * 1024;

/// The fixed reason of a refused request. The log holds only this word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NotConnect,
    HostNotAllowed,
    PortNotAllowed,
    AddressNotPublic,
    TooLong,
    TooMany,
    Timeout,
}

impl Reason {
    fn word(self) -> &'static str {
        match self {
            Reason::NotConnect => "not_connect",
            Reason::HostNotAllowed => "host_not_allowed",
            Reason::PortNotAllowed => "port_not_allowed",
            Reason::AddressNotPublic => "address_not_public",
            Reason::TooLong => "too_long",
            Reason::TooMany => "too_many",
            Reason::Timeout => "timeout",
        }
    }
}

/// True if a connection to `ip` can leave the machine to the public internet.
/// An IPv4 address mapped in IPv6 gets the answer of the IPv4 address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => v4_public(a),
        IpAddr::V6(a) => match a.to_ipv4_mapped() {
            Some(v4) => v4_public(v4),
            None => v6_public(a),
        },
    }
}

fn v4_public(a: Ipv4Addr) -> bool {
    // The special-purpose ranges of RFC 6890 and its updates, as (net, prefix).
    const RESERVED: [(u32, u32); 15] = [
        (0x0000_0000, 8),  // 0.0.0.0/8 this network
        (0x0a00_0000, 8),  // 10.0.0.0/8 private
        (0x6440_0000, 10), // 100.64.0.0/10 shared address space
        (0x7f00_0000, 8),  // 127.0.0.0/8 loopback
        (0xa9fe_0000, 16), // 169.254.0.0/16 link-local
        (0xac10_0000, 12), // 172.16.0.0/12 private
        (0xc000_0000, 24), // 192.0.0.0/24 protocol assignments
        (0xc000_0200, 24), // 192.0.2.0/24 documentation
        (0xc058_6300, 24), // 192.88.99.0/24 6to4 relay
        (0xc0a8_0000, 16), // 192.168.0.0/16 private
        (0xc612_0000, 15), // 198.18.0.0/15 benchmark
        (0xc633_6400, 24), // 198.51.100.0/24 documentation
        (0xcb00_7100, 24), // 203.0.113.0/24 documentation
        (0xe000_0000, 4),  // 224.0.0.0/4 multicast
        (0xf000_0000, 4),  // 240.0.0.0/4 reserved and broadcast
    ];
    let a = u32::from(a);
    !RESERVED
        .iter()
        .any(|&(net, prefix)| a >> (32 - prefix) == net >> (32 - prefix))
}

fn v6_public(a: Ipv6Addr) -> bool {
    let a = u128::from(a);
    let in_net = |net: u128, prefix: u32| a >> (128 - prefix) == net >> (128 - prefix);
    // Only global unicast (2000::/3) can be public. Inside it, refuse the
    // documentation, protocol and 6to4 ranges: a 6to4 address can carry a
    // private IPv4 address.
    in_net(0x2000 << 112, 3)
        && !in_net(0x2001_0db8 << 96, 32)
        && !in_net(0x2001 << 112, 23)
        && !in_net(0x2002 << 112, 16)
        && !in_net(0x3fff << 112, 20)
}

/// Checks a request head. `Ok(None)` means that the head is not complete.
/// `Ok(Some((host, len)))` gives the index of the host in `allowed` and the
/// length of the head; the bytes after it belong to the tunnel.
fn check_head(buf: &[u8], allowed: &[String]) -> Result<Option<(usize, usize)>, Reason> {
    // Compare the method as bytes first: the parser skips empty lines at the
    // start, and a head must start with the request line.
    const METHOD: &[u8] = b"CONNECT ";
    let n = buf.len().min(METHOD.len());
    if buf[..n] != METHOD[..n] {
        return Err(Reason::NotConnect);
    }
    let buf = &buf[..buf.len().min(HEAD_MAX)];
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    let len = match req.parse(buf) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) if buf.len() >= HEAD_MAX => return Err(Reason::TooLong),
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(httparse::Error::TooManyHeaders) => return Err(Reason::TooLong),
        Err(_) => return Err(Reason::NotConnect),
    };
    let target = req.path.ok_or(Reason::NotConnect)?;
    let (host, port) = target.rsplit_once(':').ok_or(Reason::NotConnect)?;
    if port != "443" {
        return Err(Reason::PortNotAllowed);
    }
    let i = allowed
        .iter()
        .position(|a| a.eq_ignore_ascii_case(host))
        .ok_or(Reason::HostNotAllowed)?;
    Ok(Some((i, len)))
}

/// Resolves a host name. Tests give a fixed resolver.
pub trait Resolve: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

/// The resolver of the system.
pub struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok((host, port).to_socket_addrs()?.collect())
    }
}

/// The egress log, with its limit.
struct Log {
    file: File,
    written: u64,
    limit: u64,
    cut: bool,
    dropped: u64,
    error: Option<io::Error>,
}

impl Log {
    fn line(&mut self, text: &str) {
        let line = format!("t={} {text}\n", now());
        if !self.cut && self.written + line.len() as u64 <= self.limit.saturating_sub(LOG_RESERVE) {
            self.write(&line);
        } else {
            if !self.cut {
                self.cut = true;
                self.write(&format!(
                    "t={} log limit reached, the rest is dropped\n",
                    now()
                ));
            }
            self.dropped += 1;
        }
    }

    fn write(&mut self, line: &str) {
        match self.file.write_all(line.as_bytes()) {
            Ok(()) => self.written += line.len() as u64,
            Err(e) => {
                self.error.get_or_insert(e);
            }
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The open connections: a clone of each stream, so that a stop can close it.
struct Conns {
    next: u64,
    streams: HashMap<u64, Vec<TcpStream>>,
    threads: Vec<JoinHandle<()>>,
    /// The connections in the resolve and connect phase.
    cancels: HashMap<u64, mpsc::Sender<Phase>>,
}

struct Shared {
    allowed: Vec<String>,
    resolver: Box<dyn Resolve>,
    public_only: bool,
    log: Mutex<Log>,
    conns: Mutex<Conns>,
    stopping: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The egress proxy of one session.
pub struct Proxy {
    port: u16,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl Proxy {
    /// Starts the proxy on a free port of `127.0.0.1`. `log` is the egress
    /// log of the session, open for append.
    pub fn start(allowed: Vec<String>, log: File, resolver: Box<dyn Resolve>) -> io::Result<Proxy> {
        Proxy::start_with(allowed, log, resolver, true, LOG_LIMIT)
    }

    /// `public_only` is false only in tests, which use a loopback server.
    fn start_with(
        allowed: Vec<String>,
        file: File,
        resolver: Box<dyn Resolve>,
        public_only: bool,
        log_limit: u64,
    ) -> io::Result<Proxy> {
        let written = file.metadata()?.len();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            allowed,
            resolver,
            public_only,
            log: Mutex::new(Log {
                file,
                written,
                limit: log_limit,
                cut: false,
                dropped: 0,
                error: None,
            }),
            conns: Mutex::new(Conns {
                next: 0,
                streams: HashMap::new(),
                threads: Vec::new(),
                cancels: HashMap::new(),
            }),
            stopping: AtomicBool::new(false),
        });
        let s = shared.clone();
        let accept = thread::Builder::new()
            .name("iris-egress".into())
            .spawn(move || accept_loop(listener, s))?;
        Ok(Proxy {
            port,
            shared,
            accept: Some(accept),
        })
    }

    /// The port that the agent gets in `HTTPS_PROXY`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Closes the port and each open connection, waits for each thread, and
    /// writes the count of dropped log lines.
    pub fn stop(mut self) -> io::Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> io::Result<()> {
        let Some(accept) = self.accept.take() else {
            return Ok(());
        };
        self.shared.stopping.store(true, Ordering::SeqCst);
        // Wake the accept loop, which then sees the stop and drops the listener.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        let mut panicked = accept.join().is_err();
        let threads = {
            let mut c = lock(&self.shared.conns);
            for s in c.streams.values().flatten() {
                let _ = s.shutdown(Shutdown::Both);
            }
            for tx in c.cancels.values() {
                let _ = tx.send(Phase::Cancel);
            }
            std::mem::take(&mut c.threads)
        };
        for t in threads {
            panicked |= t.join().is_err();
        }
        let mut log = lock(&self.shared.log);
        if log.cut {
            let line = format!("dropped {} lines\n", log.dropped);
            log.write(&line);
        }
        if let Some(e) = log.error.take() {
            return Err(e);
        }
        if panicked {
            return Err(io::Error::other("an egress thread panicked"));
        }
        Ok(())
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let mut c = lock(&shared.conns);
        c.threads.retain(|t| !t.is_finished());
        if c.streams.len() >= MAX_CONNECTIONS {
            drop(c);
            finish(&shared, refuse(stream, Reason::TooMany));
            continue;
        }
        let Ok(clone) = stream.try_clone() else {
            continue;
        };
        let id = c.next;
        c.next += 1;
        c.streams.insert(id, vec![clone]);
        let s = shared.clone();
        let spawned = thread::Builder::new()
            .name("iris-egress-conn".into())
            .spawn(move || {
                let f = handle(&s, stream, id);
                {
                    let mut c = lock(&s.conns);
                    c.streams.remove(&id);
                    c.cancels.remove(&id);
                }
                finish(&s, f);
            });
        match spawned {
            Ok(t) => c.threads.push(t),
            Err(_) => {
                c.streams.remove(&id);
            }
        }
    }
}

/// How a connection ended. Each accepted connection gives exactly one, and
/// [`finish`] writes it as the one log line of the connection.
enum End {
    Refused(Reason),
    /// The client closed before a full head.
    NoHead,
    /// A stop came before a full head.
    Stopped,
    Tunnel {
        host: usize,
        up: u64,
        down: u64,
        result: &'static str,
    },
}

/// The end of a connection, and the answer to send before the close.
struct Finish {
    end: End,
    answer: Option<(TcpStream, &'static [u8])>,
}

const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

fn refuse(stream: TcpStream, reason: Reason) -> Finish {
    Finish {
        end: End::Refused(reason),
        answer: Some((stream, FORBIDDEN)),
    }
}

fn tunnel(host: usize, up: u64, down: u64, result: &'static str) -> End {
    End::Tunnel {
        host,
        up,
        down,
        result,
    }
}

/// Writes the one log line of a connection, then sends the answer and reads
/// the rest of the request for a short time, so that the close does not
/// reset the connection.
fn finish(shared: &Shared, f: Finish) {
    let line = match f.end {
        End::Refused(r) => format!("refused reason={}", r.word()),
        End::NoHead => "ended reason=no_head".to_string(),
        End::Stopped => "ended reason=stopped".to_string(),
        End::Tunnel {
            host,
            up,
            down,
            result,
        } => {
            format!(
                "host={} up={up} down={down} result={result}",
                shared.allowed[host]
            )
        }
    };
    lock(&shared.log).line(&line);
    if let Some((mut stream, answer)) = f.answer {
        let _ = stream.write_all(answer);
        drain(stream);
    }
}

fn drain(mut stream: TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let end = Instant::now() + DRAIN_TIME;
    let mut buf = [0u8; 1024];
    let mut total = 0;
    while total < 64 * 1024 {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
    }
}

/// The result of the resolve and connect phase of a connection.
enum Phase {
    Up(TcpStream),
    NotPublic,
    Failed,
    /// A stop came first.
    Cancel,
}

/// Reads the head of one connection, checks it, and relays the tunnel.
fn handle(shared: &Arc<Shared>, mut client: TcpStream, id: u64) -> Finish {
    let mut head = Vec::with_capacity(512);
    let end = Instant::now() + HEAD_TIME;
    let stopped_or = |e: End| {
        if shared.stopping.load(Ordering::SeqCst) {
            End::Stopped
        } else {
            e
        }
    };
    let (host, len) = loop {
        match check_head(&head, &shared.allowed) {
            Ok(Some(found)) => break found,
            Ok(None) => {}
            Err(r) => return refuse(client, r),
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || client.set_read_timeout(Some(left)).is_err() {
            return refuse(client, Reason::Timeout);
        }
        let mut buf = [0u8; 1024];
        let room = (HEAD_MAX - head.len()).min(buf.len());
        match client.read(&mut buf[..room]) {
            Ok(0) => {
                return Finish {
                    end: stopped_or(End::NoHead),
                    answer: None,
                }
            }
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return refuse(client, Reason::Timeout);
            }
            Err(_) => {
                return Finish {
                    end: stopped_or(End::NoHead),
                    answer: None,
                }
            }
        }
    };
    let upstream = match connect(shared, host, id) {
        Phase::Up(u) => u,
        Phase::NotPublic => return refuse(client, Reason::AddressNotPublic),
        Phase::Failed => {
            return Finish {
                end: tunnel(host, 0, 0, "connect_failed"),
                answer: Some((client, BAD_GATEWAY)),
            }
        }
        Phase::Cancel => {
            return Finish {
                end: tunnel(host, 0, 0, "stopped"),
                answer: None,
            }
        }
    };
    {
        let mut c = lock(&shared.conns);
        if shared.stopping.load(Ordering::SeqCst) {
            return Finish {
                end: tunnel(host, 0, 0, "stopped"),
                answer: None,
            };
        }
        // A stop closes this stream with the client stream.
        if let (Ok(clone), Some(list)) = (upstream.try_clone(), c.streams.get_mut(&id)) {
            list.push(clone);
        }
    }
    let _ = client.set_read_timeout(None);
    let (up, down) = relay(client, upstream, &head[len..]);
    let result = if shared.stopping.load(Ordering::SeqCst) {
        "stopped"
    } else {
        "closed"
    };
    Finish {
        end: tunnel(host, up, down, result),
        answer: None,
    }
}

/// Resolves the host and connects to a checked address, in a worker thread,
/// so that a stalled resolver cannot hold the connection or a stop. The wait
/// ends at [`CONNECT_TIME`] or at a stop, whichever comes first. A worker
/// that is still in the resolver then ends later, by itself, and its
/// connection (if any) is dropped.
fn connect(shared: &Arc<Shared>, host: usize, id: u64) -> Phase {
    let (tx, rx) = mpsc::channel();
    {
        let mut c = lock(&shared.conns);
        if shared.stopping.load(Ordering::SeqCst) {
            return Phase::Cancel;
        }
        c.cancels.insert(id, tx.clone());
    }
    let s = shared.clone();
    let deadline = Instant::now() + CONNECT_TIME;
    let spawned = thread::Builder::new()
        .name("iris-egress-resolve".into())
        .spawn(move || {
            let _ = tx.send(resolve_and_connect(&s, host, deadline));
        });
    if spawned.is_err() {
        return Phase::Failed;
    }
    rx.recv_timeout(CONNECT_TIME + Duration::from_secs(1))
        .unwrap_or(Phase::Failed)
}

fn resolve_and_connect(shared: &Shared, host: usize, deadline: Instant) -> Phase {
    let Ok(addrs) = shared.resolver.resolve(&shared.allowed[host], 443) else {
        return Phase::Failed;
    };
    let checked: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|a| !shared.public_only || is_public(a.ip()))
        .collect();
    if checked.is_empty() {
        return Phase::NotPublic;
    }
    // Connect to a checked address only, never to the name again.
    for a in &checked {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        if let Ok(u) = TcpStream::connect_timeout(a, left) {
            return Phase::Up(u);
        }
    }
    Phase::Failed
}

/// Relays both directions until both end. Gives the bytes sent up (client to
/// host) and down. `rest` holds the bytes that came after the head.
fn relay(mut client: TcpStream, mut upstream: TcpStream, rest: &[u8]) -> (u64, u64) {
    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .is_err()
        || upstream.write_all(rest).is_err()
    {
        let _ = client.shutdown(Shutdown::Both);
        let _ = upstream.shutdown(Shutdown::Both);
        return (0, 0);
    }
    let (Ok(c2), Ok(u2)) = (client.try_clone(), upstream.try_clone()) else {
        let _ = client.shutdown(Shutdown::Both);
        let _ = upstream.shutdown(Shutdown::Both);
        return (0, 0);
    };
    let up = thread::spawn(move || copy(c2, u2));
    let down = copy(upstream, client);
    let up = up.join().unwrap_or(0);
    (rest.len() as u64 + up, down)
}

/// Copies `from` to `to` until the end. On an end of file it closes the write
/// side of `to`; on an error it closes both streams.
fn copy(mut from: TcpStream, mut to: TcpStream) -> u64 {
    let mut buf = vec![0u8; BUF];
    let mut n = 0u64;
    loop {
        match from.read(&mut buf) {
            Ok(0) => {
                let _ = to.shutdown(Shutdown::Write);
                return n;
            }
            Ok(k) if to.write_all(&buf[..k]).is_ok() => n += k as u64,
            Ok(_) | Err(_) => {
                let _ = from.shutdown(Shutdown::Both);
                let _ = to.shutdown(Shutdown::Both);
                return n;
            }
        }
    }
}

#[cfg(test)]
#[path = "egress_tests.rs"]
mod tests;
