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
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The largest request head that the proxy reads.
pub const HEAD_MAX: usize = 8 * 1024;
/// The most connections that a session can have open at one time.
pub const MAX_CONNECTIONS: usize = 8;
/// The most resolver workers that the Iris process keeps live, over all
/// proxies. A worker that waits on a stalled resolver keeps its place.
pub const MAX_RESOLVERS: usize = 16;
/// The largest ClientHello that the proxy reads.
const HELLO_MAX: usize = 16 * 1024;
/// The size limit of the egress log.
pub const LOG_LIMIT: u64 = 1024 * 1024;
/// The end of the log that only the limit line and the count line can use.
pub const LOG_RESERVE: u64 = 4096;

/// The time that a client has to send its request head.
const HEAD_TIME: Duration = Duration::from_secs(10);
/// The time to connect to one address of the model host.
const CONNECT_TIME: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(10)
};
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
    NotTls,
    SniNotAllowed,
    /// The ClientHello hides the true server name: Encrypted Client Hello
    /// or the older ESNI draft (issue 426).
    Ech,
    /// The proxy had no file descriptor or thread for the connection.
    NoResources,
    /// The client is not of the session: on macOS, not from `127.0.0.1`
    /// or with no token of the session.
    NotSession,
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
            Reason::NotTls => "not_tls",
            Reason::SniNotAllowed => "sni_not_allowed",
            Reason::Ech => "ech",
            Reason::NoResources => "no_resources",
            Reason::NotSession => "not_session",
        }
    }
}

/// The port of the egress listener in the network namespace of a Linux
/// session. The namespace is private, so a fixed port is free. It is
/// `EGRESS_PORT + 1` if the local port of the adapter is `EGRESS_PORT`.
pub const EGRESS_PORT: u16 = 3128;

/// The port of the egress listener in a Linux session with the local port
/// `local`: never the local port.
pub fn sandbox_port(local: Option<u16>) -> u16 {
    if local == Some(EGRESS_PORT) {
        EGRESS_PORT + 1
    } else {
        EGRESS_PORT
    }
}

/// The file descriptor of the helper end of the socket pair in the hold
/// helper of a Linux session ([`Proxy::start_channel`]).
pub const EGRESS_FD: RawFd = 3;

/// The proxy endpoint that the agent of a session gets: the port, and on
/// macOS the token of the session (macOS sandbox spec, "Egress on macOS").
/// On Linux, `channel` is the helper end of the socket pair of the proxy
/// ([`Proxy::start_channel`]): the launch gives it to the helper.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub port: u16,
    pub token: Option<String>,
    pub channel: Option<Arc<OwnedFd>>,
}

/// A new random token of 32 bytes, as 64 hex digits, from the random
/// source of the system.
pub fn new_token() -> io::Result<String> {
    let mut b = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// The token check of a request head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Auth {
    /// The head is not complete yet.
    Partial,
    /// The head is complete, and no `Proxy-Authorization` header has the
    /// value `Basic` + base64 of `iris:<token>`. A head that does not parse
    /// is `Bad` too.
    Bad,
    Good,
}

/// Checks the token of `head`. The compare of a value takes the same time
/// for each value of the same length.
fn auth(head: &[u8], token: &str) -> Auth {
    use base64::Engine;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(head) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) => return Auth::Partial,
        Err(_) => return Auth::Bad,
    }
    let want = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("iris:{token}"))
    );
    let mut found = false;
    for h in req.headers.iter() {
        if h.name.eq_ignore_ascii_case("proxy-authorization") {
            found |= same(h.value, want.as_bytes());
        }
    }
    if found {
        Auth::Good
    } else {
        Auth::Bad
    }
}

/// Compares `a` and `b` in a time that depends only on their lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Connects to `127.0.0.1:<port>` from the loopback address `from`, with
/// `socket2` (bind before connect). `Socket::new` sets close-on-exec. Linux takes
/// each address of `127.0.0.0/8` as a local address with no setup.
pub fn connect_from(from: Ipv4Addr, port: u16) -> io::Result<TcpStream> {
    use socket2::{Domain, SockAddr, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.bind(&SockAddr::from(SocketAddr::from((from, 0))))?;
    socket.connect(&SockAddr::from(SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        port,
    ))))?;
    Ok(socket.into())
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
    resolver: Arc<dyn Resolve>,
    public_only: bool,
    /// The only client address that the proxy takes (macOS). None for a
    /// proxy that takes its clients from its socket pair (Linux), and in
    /// tests.
    peer: Option<Ipv4Addr>,
    /// The token that each `CONNECT` must give (macOS only).
    token: Option<String>,
    /// The live resolver workers of the process (one count for all proxies).
    /// A worker can outlive its connection and the proxy, so it holds only
    /// this count, the resolver and the name.
    workers: Arc<AtomicUsize>,
    log: Mutex<Log>,
    conns: Mutex<Conns>,
    stopping: AtomicBool,
    /// A setup failure that a test makes.
    #[cfg(test)]
    fault: Mutex<Option<Fault>>,
}

/// A setup step that can fail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
enum Fault {
    /// The clone of the client socket at the accept.
    Clone,
    /// The thread of the connection.
    Spawn,
    /// The thread of the resolver worker.
    ResolverSpawn,
    /// A clone of the upstream or client socket for the tunnel.
    UpstreamClone,
    /// The thread of the up direction of the tunnel.
    RelaySpawn,
}

/// True if a test made the setup step `f` fail.
#[cfg(test)]
fn faulted(shared: &Shared, f: Fault) -> bool {
    *lock(&shared.fault) == Some(f)
}

#[cfg(not(test))]
fn faulted(_: &Shared, _: Fault) -> bool {
    false
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The egress proxy of one session.
pub struct Proxy {
    port: u16,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
    /// The write end of the wake pipe of the accept loop. A stop drops it.
    wake: Option<UnixStream>,
}

impl Proxy {
    /// Starts the proxy on a free port of `127.0.0.1` (macOS). `log` is the
    /// egress log of the session, open for append. `peer` is `127.0.0.1`:
    /// the proxy refuses a client from another address. With a `token`, the
    /// proxy answers each `CONNECT` that does not give it with `407`.
    pub fn start(
        allowed: Vec<String>,
        log: File,
        resolver: Box<dyn Resolve>,
        peer: Ipv4Addr,
        token: Option<String>,
    ) -> io::Result<Proxy> {
        Proxy::start_with(
            allowed,
            log,
            resolver,
            true,
            Some(peer),
            token,
            LOG_LIMIT,
            workers(),
        )
    }

    /// Starts the proxy of a Linux session on a new socket pair, and gives
    /// the helper end of the pair. The proxy has no port: the helper sends
    /// it each connection of the agent as a file descriptor. The pair has no
    /// name, so no other process can take the place of either end (issue
    /// 507). `port` is the port of the listener in the namespace of the
    /// session ([`sandbox_port`]), which the agent gets.
    #[cfg(target_os = "linux")]
    pub fn start_channel(
        allowed: Vec<String>,
        log: File,
        resolver: Box<dyn Resolve>,
        port: u16,
    ) -> io::Result<(Proxy, OwnedFd)> {
        let (engine, helper) = UnixStream::pair()?;
        let proxy = Proxy::start_on(
            Source::Channel(engine),
            port,
            allowed,
            log,
            resolver,
            true,
            None,
            None,
            LOG_LIMIT,
            workers(),
        )?;
        Ok((proxy, helper.into()))
    }

    /// `public_only` is false only in tests, which use a loopback server.
    #[allow(clippy::too_many_arguments)]
    fn start_with(
        allowed: Vec<String>,
        file: File,
        resolver: Box<dyn Resolve>,
        public_only: bool,
        peer: Option<Ipv4Addr>,
        token: Option<String>,
        log_limit: u64,
        workers: Arc<AtomicUsize>,
    ) -> io::Result<Proxy> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        Proxy::start_on(
            Source::Tcp(listener),
            port,
            allowed,
            file,
            resolver,
            public_only,
            peer,
            token,
            log_limit,
            workers,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_on(
        source: Source,
        port: u16,
        allowed: Vec<String>,
        file: File,
        resolver: Box<dyn Resolve>,
        public_only: bool,
        peer: Option<Ipv4Addr>,
        token: Option<String>,
        log_limit: u64,
        workers: Arc<AtomicUsize>,
    ) -> io::Result<Proxy> {
        let written = file.metadata()?.len();
        let shared = Arc::new(Shared {
            allowed,
            resolver: Arc::from(resolver),
            public_only,
            peer,
            token,
            workers,
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
            #[cfg(test)]
            fault: Mutex::new(None),
        });
        // The accept loop waits in poll on the listener and on the read end
        // of this pair, which a stop closes.
        let (wake, woken) = UnixStream::pair()?;
        source.set_nonblocking()?;
        let s = shared.clone();
        let accept = thread::Builder::new()
            .name("iris-egress".into())
            .spawn(move || accept_loop(source, woken, s))?;
        Ok(Proxy {
            wake: Some(wake),
            port,
            shared,
            accept: Some(accept),
        })
    }

    /// The port that the agent gets in `HTTPS_PROXY`: on Linux the port in
    /// the namespace of the session.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The endpoint that the agent gets: the port and the token.
    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            port: self.port,
            token: self.shared.token.clone(),
            channel: None,
        }
    }

    /// Ends each open connection and each resolve in progress, and answers
    /// each later connection with a close and `ended reason=stopped`. The
    /// port stays bound until [`Proxy::stop`], so it cannot go to the proxy
    /// of another session. Call it when the grant of the session ends.
    pub fn close(&self) {
        // The accept loop reads `stopping` while it holds this lock, so no
        // connection is added after the shutdowns below.
        let c = lock(&self.shared.conns);
        self.shared.stopping.store(true, Ordering::SeqCst);
        for s in c.streams.values().flatten() {
            let _ = s.shutdown(Shutdown::Both);
        }
        for tx in c.cancels.values() {
            let _ = tx.send(Phase::Cancel);
        }
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
        // Close the write end of the wake pipe: the accept loop sees the
        // hang-up in poll and returns. This opens no file descriptor, so it
        // cannot fail when the process has none left.
        drop(self.wake.take());
        let mut panicked = accept.join().is_err();
        self.close();
        let threads = std::mem::take(&mut lock(&self.shared.conns).threads);
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

/// The live resolver workers of the process: one count for all proxies.
fn workers() -> Arc<AtomicUsize> {
    static WORKERS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(AtomicUsize::new(0)))
        .clone()
}

/// Where the clients of a proxy come from.
enum Source {
    /// A TCP listener on `127.0.0.1` (macOS, and tests).
    Tcp(TcpListener),
    /// The engine end of the socket pair of a Linux session. Each message
    /// holds one TCP connection of the agent as a file descriptor.
    #[cfg(target_os = "linux")]
    Channel(UnixStream),
}

/// The result of one accept.
enum Next {
    Client(TcpStream, Option<SocketAddr>),
    /// No client now.
    Nothing,
    /// No client ever again: the helper closed its end of the pair.
    Ended,
}

impl Source {
    fn fd(&self) -> RawFd {
        match self {
            Source::Tcp(l) => l.as_raw_fd(),
            #[cfg(target_os = "linux")]
            Source::Channel(c) => c.as_raw_fd(),
        }
    }

    fn set_nonblocking(&self) -> io::Result<()> {
        match self {
            Source::Tcp(l) => l.set_nonblocking(true),
            #[cfg(target_os = "linux")]
            Source::Channel(c) => c.set_nonblocking(true),
        }
    }

    fn accept(&self) -> Next {
        match self {
            Source::Tcp(l) => match l.accept() {
                Ok((s, from)) => Next::Client(s, Some(from)),
                Err(_) => Next::Nothing,
            },
            #[cfg(target_os = "linux")]
            Source::Channel(c) => match recv_fd(c.as_raw_fd()) {
                Ok(Some(fd)) if is_tcp_stream(&fd) => Next::Client(TcpStream::from(fd), None),
                // Not a TCP connection: the descriptor closes here.
                Ok(Some(_)) => Next::Nothing,
                Ok(None) => Next::Ended,
                // Only the end of the pair or an error of the socket ends
                // the source; a message with no descriptor is dropped.
                Err(e) if NOT_ENDED.contains(&e.kind()) => Next::Nothing,
                Err(_) => Next::Ended,
            },
        }
    }
}

/// The errors of [`recv_fd`] after which the pair still works.
#[cfg(target_os = "linux")]
const NOT_ENDED: [io::ErrorKind; 3] = [
    io::ErrorKind::WouldBlock,
    io::ErrorKind::Interrupted,
    io::ErrorKind::InvalidData,
];

/// True if `fd` is a TCP stream socket.
#[cfg(target_os = "linux")]
fn is_tcp_stream(fd: &OwnedFd) -> bool {
    let s = socket2::SockRef::from(fd);
    matches!(s.r#type(), Ok(socket2::Type::STREAM))
        && s.local_addr().is_ok_and(|a| a.as_socket().is_some())
}

/// Receives one message of [`send_fd`](crate::isolation_linux) from the
/// Unix socket `sock`. Gives the file descriptor of the message, `None` at
/// the end of the stream, or an error: `InvalidData` for a message with no
/// descriptor. Each descriptor gets close-on-exec.
#[cfg(target_os = "linux")]
pub(crate) fn recv_fd(sock: RawFd) -> io::Result<Option<OwnedFd>> {
    use std::os::fd::FromRawFd;
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr() as *mut libc::c_void,
        iov_len: 1,
    };
    let mut space = [0u64; 4];
    // SAFETY: msg points to valid buffers that live for the call. The
    // kernel closes each descriptor that does not fit in `space`.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = std::mem::size_of_val(&space) as _;
        let n = libc::recvmsg(sock, &mut msg, libc::MSG_CMSG_CLOEXEC | libc::MSG_DONTWAIT);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        let has_fd = !cmsg.is_null()
            && (*cmsg).cmsg_level == libc::SOL_SOCKET
            && (*cmsg).cmsg_type == libc::SCM_RIGHTS
            && (*cmsg).cmsg_len as usize
                >= libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as usize;
        if !has_fd {
            return if n == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a message with no file descriptor",
                ))
            };
        }
        let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg) as *const RawFd);
        Ok(Some(OwnedFd::from_raw_fd(fd)))
    }
}

/// Waits until `source` has a client (true) or the wake pipe is closed
/// (false). A negative `source` is not polled. The wait has no time limit.
fn wait(source: RawFd, woken: &UnixStream) -> bool {
    let mut fds = [
        libc::pollfd {
            fd: source,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: woken.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: fds is a valid array of two pollfd values for the call,
        // and both descriptors stay open while this function runs.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        if fds[1].revents != 0 {
            return false;
        }
        if fds[0].revents != 0 {
            return true;
        }
    }
}

fn accept_loop(source: Source, woken: UnixStream, shared: Arc<Shared>) {
    // After the end of the source, the loop waits only for the stop.
    let mut fd = source.fd();
    while wait(fd, &woken) {
        let (stream, from) = match source.accept() {
            Next::Client(s, from) => (s, from),
            Next::Nothing => continue,
            Next::Ended => {
                fd = -1;
                continue;
            }
        };
        // macOS gives the accepted socket the non-blocking mode of the
        // listener; Linux does not. The connection code needs blocking reads.
        if stream.set_nonblocking(false).is_err() {
            finish(&shared, refuse(stream, Reason::NoResources));
            continue;
        }
        let mut c = lock(&shared.conns);
        if shared.stopping.load(Ordering::SeqCst) {
            // Each client accepted after a close, from any address, gets
            // its line and a close.
            drop(c);
            drop(stream);
            finish(
                &shared,
                Finish {
                    end: End::Stopped,
                    answer: None,
                },
            );
            continue;
        }
        if shared
            .peer
            .is_some_and(|peer| from.map(|f| f.ip()) != Some(IpAddr::V4(peer)))
        {
            drop(c);
            finish(&shared, refuse(stream, Reason::NotSession));
            continue;
        }
        c.threads.retain(|t| !t.is_finished());
        if c.streams.len() >= MAX_CONNECTIONS {
            drop(c);
            finish(&shared, refuse(stream, Reason::TooMany));
            continue;
        }
        let clone = match stream.try_clone() {
            Ok(clone) if !faulted(&shared, Fault::Clone) => clone,
            _ => {
                drop(c);
                finish(&shared, refuse(stream, Reason::NoResources));
                continue;
            }
        };
        let id = c.next;
        c.next += 1;
        c.streams.insert(id, vec![clone]);
        let s = shared.clone();
        let run = move || {
            let f = handle(&s, stream, id);
            lock(&s.conns).cancels.remove(&id);
            // The connection keeps its place in the count until its
            // socket closes, so a stop can close it during the drain.
            finish(&s, f);
            lock(&s.conns).streams.remove(&id);
        };
        let spawned = if faulted(&shared, Fault::Spawn) {
            drop(run);
            Err(io::Error::other("fault"))
        } else {
            thread::Builder::new()
                .name("iris-egress-conn".into())
                .spawn(run)
        };
        match spawned {
            Ok(t) => c.threads.push(t),
            Err(_) => {
                // The thread took the stream with it; the clone is the
                // same socket, so it gets the answer and the line.
                let clone = c.streams.remove(&id).and_then(|mut l| l.pop());
                drop(c);
                if let Some(clone) = clone {
                    finish(&shared, refuse(clone, Reason::NoResources));
                }
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

const AUTH_REQUIRED: &[u8] = b"HTTP/1.1 407 Proxy Authentication Required\r\n\
Proxy-Authenticate: Basic realm=\"iris\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
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
    /// [`MAX_RESOLVERS`] resolver workers are still live.
    Busy,
    /// No thread for the resolver worker.
    NoResources,
    /// A stop came first.
    Cancel,
}

/// Reads the head of one connection, checks it, and relays the tunnel.
fn handle(shared: &Shared, mut client: TcpStream, id: u64) -> Finish {
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
        // With a token, the token comes first: a client with no token gets
        // 407 for each request, also for a host that is not allowed.
        let ready = match &shared.token {
            None => true,
            Some(t) => match auth(&head, t) {
                // A head that fills the limit and is still not complete
                // gives no token.
                Auth::Partial if head.len() < HEAD_MAX => false,
                Auth::Partial => {
                    return Finish {
                        end: End::Refused(Reason::NotSession),
                        answer: Some((client, AUTH_REQUIRED)),
                    }
                }
                Auth::Good => true,
                Auth::Bad => {
                    return Finish {
                        end: End::Refused(Reason::NotSession),
                        answer: Some((client, AUTH_REQUIRED)),
                    }
                }
            },
        };
        if ready {
            match check_head(&head, &shared.allowed) {
                Ok(Some(found)) => break found,
                Ok(None) => {}
                Err(r) => return refuse(client, r),
            }
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
        Phase::Busy => return refuse(client, Reason::TooMany),
        Phase::NoResources => return refuse(client, Reason::NoResources),
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
    // Take each socket clone of the tunnel before the 200: a failure then
    // refuses the CONNECT, and no tunnel runs outside the reach of a stop.
    let clones = (|| {
        if faulted(shared, Fault::UpstreamClone) {
            return None;
        }
        Some((
            upstream.try_clone().ok()?,
            upstream.try_clone().ok()?,
            client.try_clone().ok()?,
        ))
    })();
    let Some((registered, upstream_up, client_up)) = clones else {
        return refuse(client, Reason::NoResources);
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
        match c.streams.get_mut(&id) {
            Some(list) => list.push(registered),
            None => return refuse(client, Reason::NoResources),
        }
    }
    // The tunnel opens only for a TLS ClientHello whose server name is the
    // host of the CONNECT: two names can share one address.
    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .is_err()
    {
        return Finish {
            end: tunnel(host, 0, 0, "closed"),
            answer: None,
        };
    }
    let first = match read_hello(shared, &mut client, head[len..].to_vec(), host) {
        Ok(first) => first,
        Err(end) => return Finish { end, answer: None },
    };
    let _ = client.set_read_timeout(None);
    let Some((up, down)) = relay(shared, client, upstream, client_up, upstream_up, &first) else {
        return Finish {
            end: End::Refused(Reason::NoResources),
            answer: None,
        };
    };
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
/// that is still in the resolver stays until the resolver returns, and keeps
/// its place in the count: at most [`MAX_RESOLVERS`] workers of the process
/// live at one time, and a CONNECT past the bound gets `too_many`.
fn connect(shared: &Shared, host: usize, id: u64) -> Phase {
    let (tx, rx) = mpsc::channel();
    {
        let mut c = lock(&shared.conns);
        if shared.stopping.load(Ordering::SeqCst) {
            return Phase::Cancel;
        }
        c.cancels.insert(id, tx.clone());
    }
    // Take a place in the count before the spawn; the worker gives it back
    // when it ends, also on a panic.
    let workers = shared.workers.clone();
    let took = workers.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
        (n < MAX_RESOLVERS).then_some(n + 1)
    });
    if took.is_err() {
        return Phase::Busy;
    }
    let place = Place(workers);
    let resolver = shared.resolver.clone();
    let name = shared.allowed[host].clone();
    let public_only = shared.public_only;
    let deadline = Instant::now() + CONNECT_TIME;
    let work = move || {
        let _place = place;
        let _ = tx.send(resolve_and_connect(
            &*resolver,
            &name,
            public_only,
            deadline,
        ));
    };
    let spawned = if faulted(shared, Fault::ResolverSpawn) {
        drop(work);
        Err(io::Error::other("fault"))
    } else {
        thread::Builder::new()
            .name("iris-egress-resolve".into())
            .spawn(work)
    };
    if spawned.is_err() {
        return Phase::NoResources;
    }
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(Phase::Failed)
}

/// A place in the count of resolver workers, given back on drop.
struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The addresses that the proxy can connect to: each public address of the
/// resolver, on port 443. `public_only` is false only in tests, which keep
/// the address and port of a loopback test server.
fn targets(addrs: Vec<SocketAddr>, public_only: bool) -> Vec<SocketAddr> {
    if !public_only {
        return addrs;
    }
    addrs
        .into_iter()
        .filter(|a| is_public(a.ip()))
        .map(|mut a| {
            a.set_port(443);
            a
        })
        .collect()
}

fn resolve_and_connect(
    resolver: &dyn Resolve,
    name: &str,
    public_only: bool,
    deadline: Instant,
) -> Phase {
    let Ok(addrs) = resolver.resolve(name, 443) else {
        return Phase::Failed;
    };
    let checked = targets(addrs, public_only);
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

/// Reads the client bytes until they hold a full ClientHello, and checks
/// that its server name is `name` (ASCII case is ignored). Gives the bytes
/// read, which the tunnel sends to the host first. `buf` holds the bytes
/// that came after the head.
fn read_hello(
    shared: &Shared,
    client: &mut TcpStream,
    mut buf: Vec<u8>,
    host: usize,
) -> Result<Vec<u8>, End> {
    let name = &shared.allowed[host];
    let end = Instant::now() + HEAD_TIME;
    loop {
        match check_hello(&buf, name) {
            Ok(true) => return Ok(buf),
            Ok(false) => {}
            Err(r) => return Err(End::Refused(r)),
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || client.set_read_timeout(Some(left)).is_err() {
            return Err(End::Refused(Reason::Timeout));
        }
        let mut chunk = [0u8; 1024];
        let room = (HELLO_MAX - buf.len()).min(chunk.len());
        match client.read(&mut chunk[..room]) {
            Ok(0) | Err(_) if shared.stopping.load(Ordering::SeqCst) => {
                return Err(tunnel(host, 0, 0, "stopped"))
            }
            Ok(0) => return Err(End::Refused(Reason::NotTls)),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(End::Refused(Reason::Timeout))
            }
            Err(_) => return Err(End::Refused(Reason::NotTls)),
        }
    }
}

/// Checks the first bytes of a tunnel with the ClientHello parser of
/// rustls. `Ok(false)` means that the ClientHello is not complete.
fn check_hello(buf: &[u8], name: &str) -> Result<bool, Reason> {
    let mut acceptor = rustls::server::Acceptor::default();
    let mut rd = buf;
    while !rd.is_empty() {
        match acceptor.read_tls(&mut rd) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return Err(Reason::NotTls),
        }
    }
    match acceptor.accept() {
        Ok(None) if buf.len() >= HELLO_MAX => Err(Reason::TooLong),
        Ok(None) => Ok(false),
        Ok(Some(accepted)) => match accepted.client_hello().server_name() {
            Some(sni) if sni.eq_ignore_ascii_case(name) => {
                // With ECH, the SNI is only the outer name: the server can
                // serve another site from the encrypted inner name.
                let mut hidden = false;
                match hello_extensions(buf, |t| hidden |= HIDDEN_NAME.contains(&t)) {
                    Some(()) if hidden => Err(Reason::Ech),
                    Some(()) => Ok(true),
                    None => Err(Reason::NotTls),
                }
            }
            _ => Err(Reason::SniNotAllowed),
        },
        Err(_) => Err(Reason::NotTls),
    }
}

/// The extension types that hide the true server name: Encrypted Client
/// Hello (`encrypted_client_hello`) and the older ESNI draft
/// (`encrypted_server_name`).
const HIDDEN_NAME: [u16; 2] = [0xfe0d, 0xffce];

/// Gives each extension type of the ClientHello at the start of `buf` to
/// `each`, in order. The ClientHello can come in more than one TLS record.
/// `None` if `buf` does not start with one whole ClientHello. It reads the
/// records in place: no copy and no allocation.
fn hello_extensions(buf: &[u8], mut each: impl FnMut(u16)) -> Option<()> {
    let mut r = Records {
        fragment: &[],
        rest: buf,
        at: 0,
        end: 4,
    };
    if r.number(1)? != 1 {
        return None; // not a ClientHello
    }
    r.end = 4 + r.number(3)?;
    r.skip(2 + 32)?; // version, random
    for width in [1, 2, 1] {
        // session ID, cipher suites, compression methods
        let n = r.number(width)?;
        r.skip(n)?;
    }
    let list = r.number(2)?;
    let list_end = r.at.checked_add(list).filter(|e| *e <= r.end)?;
    while r.at < list_end {
        each(r.number(2)? as u16);
        let n = r.number(2)?;
        r.skip(n)?;
    }
    if r.at != list_end {
        return None;
    }
    // The rest of the message must be in `buf` too.
    r.skip(r.end - r.at)
}

/// A reader of one handshake message over the fragments of its TLS
/// records, in place.
struct Records<'a> {
    /// The unread part of the fragment of the current record.
    fragment: &'a [u8],
    /// The records after it.
    rest: &'a [u8],
    /// The bytes of the message read so far, and the length of the message.
    at: usize,
    end: usize,
}

impl Records<'_> {
    fn byte(&mut self) -> Option<u8> {
        if self.at >= self.end {
            return None;
        }
        while self.fragment.is_empty() {
            let (head, tail) = self.rest.split_at_checked(5)?;
            if head[0] != 22 {
                return None; // not a handshake record
            }
            let len = u16::from_be_bytes([head[3], head[4]]) as usize;
            (self.fragment, self.rest) = tail.split_at_checked(len)?;
        }
        let (b, tail) = self.fragment.split_first()?;
        self.fragment = tail;
        self.at += 1;
        Some(*b)
    }

    /// A big-endian number of `width` bytes.
    fn number(&mut self, width: usize) -> Option<usize> {
        (0..width).try_fold(0usize, |n, _| Some(n << 8 | self.byte()? as usize))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        (0..n).try_for_each(|_| self.byte().map(drop))
    }
}

/// Relays both directions until both end. Gives the bytes sent up (client to
/// host) and down. `rest` holds the bytes that came after the head.
/// `client_up` and `upstream_up` are clones for the up direction. `None`
/// means that the up thread did not start: both sockets are then closed
/// before a byte reaches the host.
fn relay(
    shared: &Shared,
    client: TcpStream,
    upstream: TcpStream,
    client_up: TcpStream,
    mut upstream_up: TcpStream,
    rest: &[u8],
) -> Option<(u64, u64)> {
    let close = || {
        let _ = client.shutdown(Shutdown::Both);
        let _ = upstream.shutdown(Shutdown::Both);
    };
    let up = if faulted(shared, Fault::RelaySpawn) {
        Err(io::Error::other("fault"))
    } else {
        let rest = rest.to_vec();
        thread::Builder::new()
            .name("iris-egress-up".into())
            .spawn(move || {
                if upstream_up.write_all(&rest).is_err() {
                    let _ = upstream_up.shutdown(Shutdown::Both);
                    let _ = client_up.shutdown(Shutdown::Both);
                    return 0;
                }
                rest.len() as u64 + copy(client_up, upstream_up)
            })
    };
    let Ok(up) = up else {
        close();
        return None;
    };
    let down = copy(upstream, client);
    Some((up.join().unwrap_or(0), down))
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
