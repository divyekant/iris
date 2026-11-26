//! Tests of the egress proxy. They are unit tests, because only a test can
//! make a proxy that connects to a loopback test server.

use super::*;
use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

/// A small random number source for the property tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// The reserved IPv4 ranges of the spec, as (first address, prefix).
const V4_RESERVED: [([u8; 4], u32); 15] = [
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 0, 2, 0], 24),
    ([192, 88, 99, 0], 24),
    ([192, 168, 0, 0], 16),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 4),
    ([240, 0, 0, 0], 4),
];

fn in_v4(a: u32, (net, prefix): ([u8; 4], u32)) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    a & mask == u32::from_be_bytes(net) & mask
}

#[test]
fn an_ipv4_address_is_public_only_outside_each_reserved_range() {
    let mut r = Rng(7);
    for _ in 0..200_000 {
        // Half of the samples come from inside a reserved range.
        let a = if r.next(2) == 0 {
            let (net, prefix) = V4_RESERVED[r.next(V4_RESERVED.len() as u64) as usize];
            let host = if prefix == 32 {
                0
            } else {
                r.next(1 << (32 - prefix)) as u32
            };
            u32::from_be_bytes(net) | host
        } else {
            r.next(1 << 32) as u32
        };
        let want = !V4_RESERVED.iter().any(|&n| in_v4(a, n));
        let ip = Ipv4Addr::from(a);
        assert_eq!(is_public(IpAddr::V4(ip)), want, "{ip}");
        // The same address mapped in IPv6 gets the same answer.
        assert_eq!(
            is_public(IpAddr::V6(ip.to_ipv6_mapped())),
            want,
            "{ip} mapped"
        );
    }
    for ok in ["8.8.8.8", "1.1.1.1", "160.79.104.10"] {
        assert!(is_public(ok.parse().unwrap()), "{ok}");
    }
}

#[test]
fn an_ipv6_address_is_public_only_in_global_unicast() {
    let mut r = Rng(11);
    for _ in 0..200_000 {
        let hi = r.next(1 << 32) as u128;
        let lo = r.next(1 << 32) as u128;
        let a = (hi << 96) | lo;
        let ip = Ipv6Addr::from(a);
        if ip.to_ipv4_mapped().is_some() {
            continue;
        }
        let top = (a >> 112) as u16;
        let want = (0x2000..=0x3fff).contains(&top)
            && (a >> 96) as u32 != 0x2001_0db8
            && (a >> 105) != (0x2001u128 << 7) // 2001::/23
            && top != 0x2002
            && (a >> 108) != 0x3fff0; // 3fff::/20
        assert_eq!(is_public(IpAddr::V6(ip)), want, "{ip}");
    }
    for bad in [
        "::",
        "::1",
        "fe80::1",
        "fc00::1",
        "fd12::1",
        "ff02::1",
        "2001:db8::1",
        "2001::1",
        "2002:c000:0204::1",
        "64:ff9b::808:808",
        "::808:808",
        "3fff::1",
    ] {
        assert!(!is_public(bad.parse().unwrap()), "{bad}");
    }
    assert!(is_public("2606:4700::1111".parse().unwrap()));
}

fn allowed() -> Vec<String> {
    vec!["api.example.test".into(), "other.example.test".into()]
}

#[test]
fn a_head_passes_only_as_a_connect_to_an_allowed_host_on_443() {
    let ok = b"CONNECT api.example.test:443 HTTP/1.1\r\nHost: api.example.test:443\r\n\r\nrest";
    assert_eq!(check_head(ok, &allowed()), Ok(Some((0, ok.len() - 4))));
    let upper = b"CONNECT API.Example.TEST:443 HTTP/1.1\r\n\r\n";
    assert_eq!(check_head(upper, &allowed()), Ok(Some((0, upper.len()))));
    assert_eq!(check_head(b"CONNECT api.exa", &allowed()), Ok(None));
    let cases: [(&[u8], Reason); 8] = [
        (
            b"GET http://api.example.test/ HTTP/1.1\r\n\r\n",
            Reason::NotConnect,
        ),
        (
            b"CONNECT api.example.test HTTP/1.1\r\n\r\n",
            Reason::NotConnect,
        ),
        (b"\x00\x01garbage\r\n\r\n", Reason::NotConnect),
        (
            b"CONNECT api.example.test:80 HTTP/1.1\r\n\r\n",
            Reason::PortNotAllowed,
        ),
        (
            b"CONNECT api.example.test:0443 HTTP/1.1\r\n\r\n",
            Reason::PortNotAllowed,
        ),
        (
            b"CONNECT evil.test:443 HTTP/1.1\r\n\r\n",
            Reason::HostNotAllowed,
        ),
        (
            b"CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n",
            Reason::HostNotAllowed,
        ),
        (
            b"CONNECT api.example.test.:443 HTTP/1.1\r\n\r\n",
            Reason::HostNotAllowed,
        ),
    ];
    for (head, want) in cases {
        assert_eq!(
            check_head(head, &allowed()),
            Err(want),
            "{:?}",
            String::from_utf8_lossy(head)
        );
    }
    let mut long = b"CONNECT ".to_vec();
    long.resize(HEAD_MAX + 1, b'a');
    assert_eq!(check_head(&long, &allowed()), Err(Reason::TooLong));
}

/// Random heads and random changes of a good head: a head passes only if
/// its request line names an allowed host on port 443 with CONNECT.
#[test]
fn no_random_head_passes_for_another_host() {
    let mut r = Rng(3);
    let good = b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n".to_vec();
    let bytes = b"CONNECT api.example.test:443 HTTP/1.1\r\n\x00 .:[]@%/evil";
    for _ in 0..100_000 {
        let mut head = good.clone();
        for _ in 0..=r.next(4) {
            let i = r.next(head.len() as u64) as usize;
            match r.next(3) {
                0 => head[i] = bytes[r.next(bytes.len() as u64) as usize],
                1 => {
                    head.remove(i);
                }
                _ => head.insert(i, bytes[r.next(bytes.len() as u64) as usize]),
            }
        }
        if let Ok(Some((host, len))) = check_head(&head, &allowed()) {
            let line = String::from_utf8_lossy(&head[..len]).to_ascii_lowercase();
            let name = &allowed()[host];
            assert!(
                line.starts_with(&format!("connect {name}:443 ")),
                "{line:?}"
            );
        }
    }
}

/// A resolver that maps each name to one fixed address.
struct Fixed(SocketAddr);

impl Resolve for Fixed {
    fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok(vec![self.0])
    }
}

/// A server that echoes each byte back, on a loopback port.
fn echo() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            thread::spawn(move || {
                let mut r = s.try_clone().unwrap();
                let _ = io::copy(&mut r, &mut s);
            });
        }
    });
    a
}

/// A log file that the test removes at the end.
struct LogFile(std::path::PathBuf);

impl Drop for LogFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Box_ {
    proxy: Proxy,
    log: LogFile,
}

fn proxy(name: &str, to: SocketAddr, public_only: bool, log_limit: u64) -> Box_ {
    let log = std::env::temp_dir().join(format!("iris-egress-{name}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Fixed(to)),
        public_only,
        None,
        None,
        log_limit,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    Box_ {
        proxy,
        log: LogFile(log),
    }
}

fn send(p: &Proxy, head: &str) -> (TcpStream, String) {
    send_on(TcpStream::connect(("127.0.0.1", p.port())).unwrap(), head)
}

fn send_on(mut s: TcpStream, head: &str) -> (TcpStream, String) {
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut rest = String::new();
    while r.read_line(&mut rest).unwrap() > 2 {
        rest.clear();
    }
    (s, line)
}

fn log_text(log: &LogFile) -> String {
    std::fs::read_to_string(&log.0).unwrap()
}

#[test]
fn an_allowed_connect_reaches_the_host_and_logs_its_bytes() {
    let b = proxy("ok", echo(), false, LOG_LIMIT);
    let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    let mut sent = hello("api.example.test");
    sent.extend_from_slice(b"ping");
    s.write_all(&sent).unwrap();
    let mut got = vec![0u8; sent.len()];
    s.read_exact(&mut got).unwrap();
    assert_eq!(got, sent);
    drop(s);
    b.proxy.stop().unwrap();
    let text = log_text(&b.log);
    let n = sent.len();
    assert!(
        text.contains(&format!("host=api.example.test up={n} down={n}")),
        "{text}"
    );
}

#[test]
fn each_refusal_gets_403_and_a_fixed_reason_with_no_request_text() {
    let b = proxy("refuse", echo(), false, LOG_LIMIT);
    for (head, reason) in [
        ("GET http://marker-one/ HTTP/1.1\r\n\r\n", "not_connect"),
        (
            "CONNECT marker-two.test:443 HTTP/1.1\r\n\r\n",
            "host_not_allowed",
        ),
        (
            "CONNECT api.example.test:8443 HTTP/1.1\r\n\r\n",
            "port_not_allowed",
        ),
    ] {
        let (_s, line) = send(&b.proxy, head);
        assert!(line.starts_with("HTTP/1.1 403"), "{line}");
        thread::sleep(Duration::from_millis(50));
        assert!(
            log_text(&b.log).contains(&format!("reason={reason}")),
            "{reason}"
        );
    }
    let text = log_text(&b.log);
    assert!(!text.contains("marker"), "{text}");
}

#[test]
fn an_allowed_host_at_a_reserved_address_is_refused() {
    let b = proxy("reserved", echo(), true, LOG_LIMIT);
    let (_s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    b.proxy.stop().unwrap();
    assert!(log_text(&b.log).contains("reason=address_not_public"));
}

#[test]
fn a_ninth_open_connection_is_refused() {
    let b = proxy("many", echo(), false, LOG_LIMIT);
    let open: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n").0)
        .collect();
    let (_s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    drop(open);
    b.proxy.stop().unwrap();
    assert!(log_text(&b.log).contains("reason=too_many"));
}

#[test]
fn the_log_stops_at_its_limit_and_counts_the_rest() {
    let limit = LOG_RESERVE + 512;
    let b = proxy("limit", echo(), false, limit);
    for _ in 0..200 {
        let _ = send(&b.proxy, "CONNECT nope.test:443 HTTP/1.1\r\n\r\n");
    }
    b.proxy.stop().unwrap();
    let text = log_text(&b.log);
    assert!(text.len() as u64 <= limit, "{}", text.len());
    assert!(text.contains("the rest is dropped"), "{text}");
    let last = text.lines().last().unwrap();
    assert!(last.starts_with("dropped "), "{last}");
}

#[test]
fn a_stop_closes_each_open_connection() {
    let b = proxy("stop", echo(), false, LOG_LIMIT);
    let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    let port = b.proxy.port();
    b.proxy.stop().unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(s.read(&mut buf).unwrap_or(0), 0);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

/// A resolver that never answers, as a stalled system DNS.
struct Stalled;

impl Resolve for Stalled {
    fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
        loop {
            thread::park();
        }
    }
}

#[test]
fn a_stop_does_not_wait_for_a_stalled_resolver() {
    let log = std::env::temp_dir().join(format!("iris-egress-stall-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let guard = LogFile(log.clone());
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Stalled),
        false,
        None,
        None,
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    proxy.stop().unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(log_text(&guard).contains("host=api.example.test up=0 down=0 result=stopped"));
}

/// Each accepted connection writes exactly one log line, whatever the
/// client does: a random mix of early closes, part heads, refusals and
/// tunnels, then a stop with some of them still open.
#[test]
fn each_connection_writes_one_log_line() {
    let mut r = Rng(5);
    for round in 0..4 {
        let b = proxy(&format!("one-{round}"), echo(), false, LOG_LIMIT);
        let mut held = Vec::new();
        let n = 6;
        for _ in 0..n {
            let mut s = TcpStream::connect(("127.0.0.1", b.proxy.port())).unwrap();
            match r.next(5) {
                0 => {}
                1 => s.write_all(b"CONNECT api.exa").unwrap(),
                2 => s
                    .write_all(b"CONNECT evil.test:443 HTTP/1.1\r\n\r\n")
                    .unwrap(),
                3 => s
                    .write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\nhi")
                    .unwrap(),
                _ => {
                    s.write_all(b"CONNECT api.exa").unwrap();
                    held.push(s);
                    continue;
                }
            }
            if r.next(2) == 0 {
                held.push(s);
            }
        }
        thread::sleep(Duration::from_millis(200));
        b.proxy.stop().unwrap();
        let text = log_text(&b.log);
        assert_eq!(text.lines().count(), n, "{text}");
        drop(held);
    }
}

/// A stalled resolver that counts its calls.
struct Counted(Arc<AtomicUsize>);

impl Resolve for Counted {
    fn resolve(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        loop {
            thread::park();
        }
    }
}

/// Timed-out connects leave their resolver workers behind, also after a
/// stop. The process keeps at most MAX_RESOLVERS of them over all proxies,
/// and refuses each CONNECT past the bound.
#[test]
fn stalled_resolver_workers_stay_bounded_over_proxies() {
    let calls = Arc::new(AtomicUsize::new(0));
    let workers = Arc::new(AtomicUsize::new(0));
    let mut too_many = 0;
    for (i, tries) in [(0, MAX_RESOLVERS + 8), (1, 4)] {
        let log =
            std::env::temp_dir().join(format!("iris-egress-bound-{i}-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let guard = LogFile(log.clone());
        let file = File::options()
            .create_new(true)
            .append(true)
            .open(&log)
            .unwrap();
        let proxy = Proxy::start_with(
            allowed(),
            file,
            Box::new(Counted(calls.clone())),
            false,
            None,
            None,
            LOG_LIMIT,
            workers.clone(),
        )
        .unwrap();
        for _ in 0..tries {
            let (_s, line) = send(&proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
            assert!(!line.starts_with("HTTP/1.1 200"), "{line}");
        }
        let t = Instant::now();
        proxy.stop().unwrap();
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        too_many += log_text(&guard).matches("reason=too_many").count();
    }
    assert_eq!(calls.load(Ordering::SeqCst), MAX_RESOLVERS);
    assert_eq!(workers.load(Ordering::SeqCst), MAX_RESOLVERS);
    assert_eq!(too_many, 8 + 4);
}

/// A refused connection keeps its place in the count until its socket
/// closes, and a stop closes it.
#[test]
fn a_refused_connection_counts_until_it_closes() {
    let b = proxy("held", echo(), false, LOG_LIMIT);
    let mut held: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| send(&b.proxy, "CONNECT evil.test:443 HTTP/1.1\r\n\r\n").0)
        .collect();
    let (_s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    assert!(log_text(&b.log).contains("reason=too_many"));
    b.proxy.stop().unwrap();
    let mut buf = [0u8; 8];
    for s in &mut held {
        assert_eq!(s.read(&mut buf).unwrap_or(0), 0);
    }
}

/// The first bytes of a TLS connection (the ClientHello) with `name` in the
/// server name indication (SNI), made by rustls.
fn hello(name: &str) -> Vec<u8> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(name.to_string()).unwrap();
    let mut c = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
    let mut out = Vec::new();
    c.write_tls(&mut out).unwrap();
    out
}

/// Two names can share one address. The tunnel opens only for a TLS
/// connection whose SNI is the host of the CONNECT, so a CONNECT to an
/// allowed host cannot reach another site at the same address.
#[test]
fn a_tunnel_needs_the_sni_of_its_allowed_host() {
    let b = proxy("sni", echo(), false, LOG_LIMIT);
    let cases: [(&[u8], &str); 4] = [
        (&hello("other-site.test"), "reason=sni_not_allowed"),
        (&hello("other.example.test"), "reason=sni_not_allowed"),
        (b"\x16\x03\x01\x00\x05hello", "reason=not_tls"),
        (b"GET / HTTP/1.1\r\n\r\n", "reason=not_tls"),
    ];
    for (bytes, want) in cases {
        let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
        assert!(line.starts_with("HTTP/1.1 200"), "{line}");
        s.write_all(bytes).unwrap();
        let mut buf = [0u8; 64];
        // Nothing comes back from the host: the proxy closes the tunnel.
        assert_eq!(s.read(&mut buf).unwrap_or(0), 0, "{want}");
        thread::sleep(Duration::from_millis(50));
        assert!(log_text(&b.log).contains(want), "{want}");
    }
    let (mut s, _) = send(&b.proxy, "CONNECT API.example.test:443 HTTP/1.1\r\n\r\n");
    let sent = hello("api.EXAMPLE.test");
    s.write_all(&sent).unwrap();
    let mut got = vec![0u8; sent.len()];
    s.read_exact(&mut got).unwrap();
    assert_eq!(got, sent);
    drop(s);
    b.proxy.stop().unwrap();
    assert!(!log_text(&b.log).contains("other"), "{}", log_text(&b.log));
}

/// Property test of the ClientHello gate over truncated, mutated, random
/// and oversized bytes: only a complete ClientHello with the server name of
/// the host passes, and no input makes the check panic.
#[test]
fn only_a_complete_hello_with_the_host_name_passes() {
    let name = "api.example.test";
    let good = hello(name);
    let other = hello("zzz.other-site.test");
    // Each strict prefix is incomplete; the full hello passes.
    for n in 0..good.len() {
        assert_eq!(check_hello(&good[..n], name), Ok(false), "prefix {n}");
    }
    assert_eq!(check_hello(&good, name), Ok(true));
    for n in 0..=other.len() {
        assert_ne!(check_hello(&other[..n], name), Ok(true), "other prefix {n}");
    }
    let mut r = Rng(13);
    let needle = name.as_bytes();
    for _ in 0..20_000 {
        let base = if r.next(2) == 0 { &good } else { &other };
        let mut b = base.clone();
        for _ in 0..=r.next(4) {
            let i = r.next(b.len() as u64) as usize;
            match r.next(3) {
                0 => b[i] = r.next(256) as u8,
                1 => {
                    b.remove(i);
                }
                _ => b.insert(i, r.next(256) as u8),
            }
        }
        if check_hello(&b, name) == Ok(true) {
            // The server name is plain text in the ClientHello: a pass needs
            // the name in the bytes, so a mutated or other name never passes.
            let lower = b.to_ascii_lowercase();
            assert!(lower.windows(needle.len()).any(|w| w == needle), "{b:?}");
            assert!(std::ptr::eq(base, &good));
        }
    }
    for _ in 0..20_000 {
        let len = r.next(600) as usize;
        let mut b: Vec<u8> = (0..len).map(|_| r.next(256) as u8).collect();
        if r.next(2) == 0 && b.len() >= 5 {
            b[..5].copy_from_slice(&[0x16, 0x03, 0x01, (len >> 8) as u8, len as u8]);
        }
        assert_ne!(check_hello(&b, name), Ok(true));
    }
    // A ClientHello that claims more than HELLO_MAX bytes is refused at the
    // limit, not read without end.
    let mut big = vec![0x16, 0x03, 0x01, 0x40, 0x00, 0x01, 0x00, 0xff, 0xff];
    big.resize(HELLO_MAX, 0);
    assert!(check_hello(&big, name).is_err());
}

/// A connection whose setup fails (no file descriptor for a clone, no
/// thread) at any step still gets its one log line, closes, and leaves no
/// tunnel that a stop cannot reach.
#[test]
fn a_failed_setup_still_writes_one_line() {
    for fault in [
        Fault::Clone,
        Fault::Spawn,
        Fault::ResolverSpawn,
        Fault::UpstreamClone,
        Fault::RelaySpawn,
    ] {
        let b = proxy("fault", echo(), false, LOG_LIMIT);
        *lock(&b.proxy.shared.fault) = Some(fault);
        let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
        if fault == Fault::RelaySpawn {
            // The fault comes after the ClientHello: the 200 is sent, then
            // the tunnel closes before a byte reaches the host.
            assert!(line.starts_with("HTTP/1.1 200"), "{line}");
            s.write_all(&hello("api.example.test")).unwrap();
            let mut buf = [0u8; 64];
            assert_eq!(s.read(&mut buf).unwrap_or(0), 0);
        } else {
            assert!(line.starts_with("HTTP/1.1 403"), "{fault:?} {line}");
        }
        let t = Instant::now();
        b.proxy.stop().unwrap();
        assert!(t.elapsed() < Duration::from_secs(2), "{fault:?}");
        let text = log_text(&b.log);
        assert_eq!(text.lines().count(), 1, "{fault:?} {text}");
        assert!(text.contains("reason=no_resources"), "{fault:?} {text}");
    }
}

/// The proxy connects only to port 443 at a checked address, whatever port
/// the resolver gives. Only the test mode (no public check) keeps the port
/// of the resolver, for the loopback test server.
#[test]
fn a_target_is_a_checked_address_on_port_443() {
    let given: Vec<SocketAddr> = vec![
        "1.1.1.1:8443".parse().unwrap(),
        "10.0.0.1:443".parse().unwrap(),
        "[2606:4700::1111]:80".parse().unwrap(),
        "[::1]:443".parse().unwrap(),
    ];
    let want: Vec<SocketAddr> = vec![
        "1.1.1.1:443".parse().unwrap(),
        "[2606:4700::1111]:443".parse().unwrap(),
    ];
    assert_eq!(targets(given.clone(), true), want);
    assert_eq!(targets(given.clone(), false), given);
}

/// A client that connects while the proxy stops gets one log line if the
/// proxy accepted it, and none if the kernel reset it.
#[test]
fn a_connection_that_races_a_stop_writes_one_line() {
    for round in 0..20 {
        let b = proxy(&format!("race-{round}"), echo(), false, LOG_LIMIT);
        let port = b.proxy.port();
        let clients = thread::spawn(move || {
            let mut accepted = 0;
            for _ in 0..20 {
                let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
                    continue;
                };
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let mut buf = [0u8; 8];
                // A FIN (Ok(0)) means that the proxy accepted the socket and
                // closed it; a reset means that it never accepted it.
                if let Ok(0) = s.read(&mut buf) {
                    accepted += 1;
                }
            }
            accepted
        });
        thread::sleep(Duration::from_millis(round % 5));
        b.proxy.stop().unwrap();
        let accepted = clients.join().unwrap();
        let text = log_text(&b.log);
        assert_eq!(text.lines().count(), accepted, "{text}");
    }
}

/// A stop after the 200 and before a full ClientHello logs the allowed
/// host with the result stopped, as each allowed CONNECT does.
#[test]
fn a_stop_during_the_hello_logs_the_host() {
    let b = proxy("hello-stop", echo(), false, LOG_LIMIT);
    let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    let h = hello("api.example.test");
    s.write_all(&h[..h.len() / 2]).unwrap();
    thread::sleep(Duration::from_millis(100));
    b.proxy.stop().unwrap();
    let text = log_text(&b.log);
    assert!(
        text.contains("host=api.example.test up=0 down=0 result=stopped"),
        "{text}"
    );
}

/// A proxy with a peer address refuses a client from each other address.
#[cfg(target_os = "linux")]
#[test]
fn the_proxy_takes_only_its_peer_address() {
    let (own, foreign) = (Ipv4Addr::new(127, 128, 0, 7), Ipv4Addr::new(127, 128, 0, 8));
    let log = std::env::temp_dir().join(format!("iris-egress-peer-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let guard = LogFile(log.clone());
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Fixed(echo())),
        false,
        Some(own),
        None,
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    let head = "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n";
    let other = connect_from(foreign, proxy.port()).unwrap();
    let (_s, line) = send_on(other, head);
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    let (_s, line) = send(&proxy, head);
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    let own = connect_from(own, proxy.port()).unwrap();
    let (_s, line) = send_on(own, head);
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    drop(_s);
    proxy.stop().unwrap();
    let text = log_text(&guard);
    assert_eq!(
        text.matches("refused reason=not_session").count(),
        2,
        "{text}"
    );
}

/// macOS: a process of another session (no token, a wrong token or the
/// token of another session) gets 407 before any upstream connection.
#[test]
fn the_proxy_takes_only_the_token_of_its_session() {
    use base64::Engine;
    let log = std::env::temp_dir().join(format!("iris-egress-token-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let guard = LogFile(log.clone());
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let mine = new_token().unwrap();
    let other = new_token().unwrap();
    assert_eq!(mine.len(), 64);
    assert_ne!(mine, other);
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Fixed(echo())),
        false,
        Some(Ipv4Addr::LOCALHOST),
        Some(mine.clone()),
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    assert_eq!(proxy.endpoint().token.as_deref(), Some(mine.as_str()));
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    let with = |cred: &str| {
        format!(
            "CONNECT api.example.test:443 HTTP/1.1\r\nProxy-Authorization: Basic {}\r\n\r\n",
            b64(cred)
        )
    };
    let mut cut = mine.clone();
    cut.pop();
    let refused = [
        "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n".to_string(),
        // The token comes before the host and port checks.
        "CONNECT blocked.example:443 HTTP/1.1\r\n\r\n".to_string(),
        "CONNECT api.example.test:80 HTTP/1.1\r\n\r\n".to_string(),
        "GET / HTTP/1.1\r\n\r\n".to_string(),
        with(&format!("iris:{other}")).replace("api.example.test", "blocked.example"),
        with(&format!("iris:{other}")),
        with(&format!("iris:{cut}")),
        with(&format!("root:{mine}")),
        with(&format!("iris:{mine}x")),
        format!(
            "CONNECT api.example.test:443 HTTP/1.1\r\nProxy-Authorization: Bearer {mine}\r\n\r\n"
        ),
    ];
    for head in &refused {
        let (_s, line) = send(&proxy, head);
        assert!(line.starts_with("HTTP/1.1 407"), "{head}: {line}");
    }
    let (s, line) = send(&proxy, &with(&format!("iris:{mine}")));
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    drop(s);
    proxy.stop().unwrap();
    let text = log_text(&guard);
    assert_eq!(
        text.matches("refused reason=not_session").count(),
        refused.len(),
        "{text}"
    );
    assert!(!text.contains(&mine) && !text.contains(&other), "{text}");
}

/// An oracle for [`auth`]: split the head into lines, and look for a
/// header line with the name `Proxy-Authorization` (any case) whose value,
/// with the spaces and tabs at its ends taken out, is `want`.
fn auth_oracle(head: &str, want: &str) -> bool {
    head.split("\r\n").skip(1).any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("proxy-authorization")
                && value.trim_matches([' ', '\t']) == want
        })
    })
}

/// Random heads with random authorization headers: only the exact value of
/// the token passes, also with other headers, duplicates and mutations.
#[test]
fn no_random_authorization_passes_but_the_token() {
    use base64::Engine;
    let b64 = |s: &[u8]| base64::engine::general_purpose::STANDARD.encode(s);
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let want = format!("Basic {}", b64(format!("iris:{token}").as_bytes()));
    let mut rng = Rng(7);
    let mut good = 0;
    for _ in 0..20_000 {
        let mut lines = vec!["CONNECT api.example.test:443 HTTP/1.1".to_string()];
        for _ in 0..rng.next(4) {
            let name = [
                "Proxy-Authorization",
                "proxy-authorization",
                "PROXY-AUTHORIZATION",
                "Proxy-Authorizatio",
                "Authorization",
                "Host",
            ][rng.next(6) as usize];
            let mut value = match rng.next(6) {
                0 => want.clone(),
                1 => format!("basic {}", b64(format!("iris:{token}").as_bytes())),
                2 => format!("Basic {}", b64(format!("iris:{}", &token[1..]).as_bytes())),
                3 => format!("Basic {}", b64(format!("IRIS:{token}").as_bytes())),
                4 => format!("Basic {}", b64(&[rng.next(256) as u8; 9])),
                _ => format!("Basic  {}", b64(format!("iris:{token}").as_bytes())),
            };
            // A mutation of one byte of the value.
            if rng.next(4) == 0 && !value.is_empty() {
                let i = rng.next(value.len() as u64) as usize;
                let c = b"aZ9+/= x"[rng.next(8) as usize] as char;
                value.replace_range(i..i + 1, &c.to_string());
            }
            lines.push(format!(
                "{name}:{}{value}",
                [" ", "", "\t"][rng.next(3) as usize]
            ));
        }
        let head = format!("{}\r\n\r\n", lines.join("\r\n"));
        let got = auth(head.as_bytes(), token);
        let expect = auth_oracle(&head, &want);
        assert_eq!(got == Auth::Good, expect, "{head:?}");
        assert_ne!(got, Auth::Partial, "{head:?}");
        good += usize::from(expect);
        // Each cut of the head before its end is not complete.
        let cut = rng.next(head.len() as u64 - 1) as usize;
        assert_ne!(auth(&head.as_bytes()[..cut], token), Auth::Good, "{head:?}");
    }
    assert!(good > 1000, "{good}");
}

/// The grant ends: the tunnel ends at once, a new client gets no tunnel,
/// and the port stays bound until the stop.
#[test]
fn close_ends_each_tunnel_and_keeps_the_port() {
    /// An end (EOF or an error) well before the 5 s read time-out. A
    /// time-out is a failure, not a close.
    fn ended_soon(s: &mut TcpStream) {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let t = Instant::now();
        let mut buf = [0u8; 64];
        loop {
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) => {
                    assert!(
                        !matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ),
                        "no end: {e}"
                    );
                    break;
                }
            }
        }
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }
    let b = proxy("close", echo(), false, LOG_LIMIT);
    let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    // The tunnel relays bytes before the close.
    let mut sent = hello("api.example.test");
    sent.extend_from_slice(b"ping");
    s.write_all(&sent).unwrap();
    let mut got = vec![0u8; sent.len()];
    s.read_exact(&mut got).unwrap();
    assert_eq!(got, sent);
    b.proxy.close();
    ended_soon(&mut s);
    let mut late = TcpStream::connect(("127.0.0.1", b.proxy.port())).unwrap();
    let _ = late.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    ended_soon(&mut late);
    assert!(
        TcpListener::bind(("127.0.0.1", b.proxy.port())).is_err(),
        "the port stays bound"
    );
    b.proxy.stop().unwrap();
    let text = log_text(&b.log);
    assert!(
        text.contains("result=stopped") || text.contains("result=closed"),
        "{text}"
    );
    assert!(text.contains("ended reason=stopped"), "{text}");
}

/// A close ends a resolve in progress at once: the client does not wait
/// for the connect time.
#[test]
fn close_ends_a_stalled_resolve_at_once() {
    let log = std::env::temp_dir().join(format!("iris-egress-cstall-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let guard = LogFile(log.clone());
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Stalled),
        false,
        None,
        None,
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    // The worker is in the resolver now.
    assert!(wait_until(
        || proxy.shared.workers.load(Ordering::SeqCst) == 1
    ));
    let t = Instant::now();
    proxy.close();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut x = [0u8; 1];
    assert_eq!(s.read(&mut x).unwrap_or(0), 0);
    assert!(t.elapsed() < CONNECT_TIME / 2, "{:?}", t.elapsed());
    assert!(wait_until(
        || log_text(&guard).contains("host=api.example.test up=0 down=0 result=stopped")
    ));
    proxy.stop().unwrap();
}

/// After a close, a client from another address also gets the stopped
/// close, not a refusal.
#[cfg(target_os = "linux")]
#[test]
fn close_comes_before_the_address_check() {
    let log = std::env::temp_dir().join(format!("iris-egress-cpeer-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let guard = LogFile(log.clone());
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let proxy = Proxy::start_with(
        allowed(),
        file,
        Box::new(Fixed(echo())),
        false,
        Some(Ipv4Addr::new(127, 128, 0, 7)),
        None,
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    proxy.close();
    for from in [Ipv4Addr::new(127, 128, 0, 8), Ipv4Addr::LOCALHOST] {
        let mut s = connect_from(from, proxy.port()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let _ = s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
        let mut x = [0u8; 1];
        assert_eq!(s.read(&mut x).unwrap_or(0), 0, "no 403 after a close");
    }
    proxy.stop().unwrap();
    let text = log_text(&guard);
    assert_eq!(text.matches("ended reason=stopped").count(), 2, "{text}");
    assert!(!text.contains("not_session"), "{text}");
}

fn wait_until(f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The ClientHello of `name` with `extra` extensions (type, body) put in
/// its extension list at `at` (cut to the list), in TLS records of at most
/// `record` bytes each.
fn hello_with(name: &str, extra: &[(u16, Vec<u8>)], at: usize, record: usize) -> Vec<u8> {
    let one = hello(name);
    assert_eq!(one[0], 22);
    let mut msg = one[5..].to_vec();
    // The start of the extension list: after the type, length, version,
    // random, session ID, cipher suites and compression methods.
    let mut p = 4 + 2 + 32;
    p += 1 + msg[p] as usize;
    p += 2 + u16::from_be_bytes([msg[p], msg[p + 1]]) as usize;
    p += 1 + msg[p] as usize;
    let list = p + 2;
    // The offset of the extension number `at`, or the end of the list.
    let mut q = list;
    for _ in 0..at {
        if q >= msg.len() {
            break;
        }
        q += 4 + u16::from_be_bytes([msg[q + 2], msg[q + 3]]) as usize;
    }
    let mut added = Vec::new();
    for (t, body) in extra {
        added.extend_from_slice(&t.to_be_bytes());
        added.extend_from_slice(&(body.len() as u16).to_be_bytes());
        added.extend_from_slice(body);
    }
    msg.splice(q.min(msg.len())..q.min(msg.len()), added);
    let ext_len = (msg.len() - list) as u16;
    msg[p..p + 2].copy_from_slice(&ext_len.to_be_bytes());
    let body_len = (msg.len() - 4) as u32;
    msg[1..4].copy_from_slice(&body_len.to_be_bytes()[1..]);
    let mut out = Vec::new();
    for chunk in msg.chunks(record.max(1)) {
        out.extend_from_slice(&[22, 3, 1]);
        out.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// The body of an outer `encrypted_client_hello` extension: HKDF-SHA256,
/// AES-128-GCM, config 7, a 32-byte `enc` and a 16-byte payload.
fn ech_outer() -> Vec<u8> {
    let mut b = vec![0, 0, 1, 0, 1, 7, 0, 32];
    b.extend_from_slice(&[0x11; 32]);
    b.extend_from_slice(&[0, 16]);
    b.extend_from_slice(&[0x22; 16]);
    b
}

/// Issue 426: a ClientHello with Encrypted Client Hello (or the older ESNI
/// draft) and the SNI of the allowed host gets no tunnel: its true server
/// name is hidden from the proxy.
#[test]
fn a_tunnel_refuses_a_hidden_server_name() {
    let b = proxy("ech", echo(), false, LOG_LIMIT);
    for t in HIDDEN_NAME {
        let bytes = hello_with("api.example.test", &[(t, ech_outer())], 3, 300);
        let (mut s, line) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
        assert!(line.starts_with("HTTP/1.1 200"), "{line}");
        s.write_all(&bytes).unwrap();
        let mut buf = [0u8; 64];
        assert_eq!(s.read(&mut buf).unwrap_or(0), 0, "{t:#x}");
    }
    // An unknown extension and records of 100 bytes still pass.
    let bytes = hello_with("api.example.test", &[(0x7a7a, vec![9])], 0, 100);
    let (mut s, _) = send(&b.proxy, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    s.write_all(&bytes).unwrap();
    let mut got = vec![0u8; bytes.len()];
    s.read_exact(&mut got).unwrap();
    assert_eq!(got, bytes);
    drop(s);
    b.proxy.stop().unwrap();
    assert_eq!(log_text(&b.log).matches("reason=ech").count(), 2);
}

/// Property test of the extension reader: for random extension lists, at
/// random places and in random record sizes, the check refuses the hello
/// exactly when it holds a type that hides the name. Cut and mutated bytes
/// never panic, and never pass with a hidden name.
#[test]
fn the_extension_reader_finds_each_hidden_name() {
    let mut r = Rng(426);
    for _ in 0..400 {
        let mut extra = Vec::new();
        for _ in 0..r.next(4) {
            let t = match r.next(6) {
                0 => HIDDEN_NAME[r.next(2) as usize],
                // Types that rustls does not know, so no duplicate.
                _ => 0x7000 + r.next(0x0fff) as u16,
            };
            if extra.iter().any(|(e, _)| *e == t) {
                continue;
            }
            let body: Vec<u8> = if HIDDEN_NAME.contains(&t) && r.next(2) == 0 {
                ech_outer()
            } else {
                (0..r.next(40)).map(|_| r.next(256) as u8).collect()
            };
            extra.push((t, body));
        }
        let hidden = extra.iter().any(|(t, _)| HIDDEN_NAME.contains(t));
        let bytes = hello_with(
            "api.example.test",
            &extra,
            r.next(12) as usize,
            1 + r.next(600) as usize,
        );
        let types = hello_extensions(&bytes).expect("a whole hello");
        for (t, _) in &extra {
            assert!(types.contains(t), "{t:#x} in {types:x?}");
        }
        let got = check_hello(&bytes, "api.example.test");
        if hidden {
            // rustls refuses an `encrypted_client_hello` body that it
            // cannot read (`not_tls`); each other hidden name is `ech`.
            let read = extra
                .iter()
                .all(|(t, b)| *t != HIDDEN_NAME[0] || *b == ech_outer());
            assert!(
                matches!(got, Err(Reason::Ech)) || (!read && matches!(got, Err(Reason::NotTls))),
                "{extra:x?}"
            );
        } else {
            assert!(matches!(got, Ok(true)), "{extra:x?}");
        }
        // A cut or a changed byte: no panic, and no pass of a hidden name.
        let cut = &bytes[..r.next(bytes.len() as u64) as usize];
        let _ = hello_extensions(cut);
        let mut bad = bytes.clone();
        let i = r.next(bad.len() as u64) as usize;
        bad[i] ^= 1 + r.next(255) as u8;
        // A changed byte can also change the type of the extension.
        if matches!(check_hello(&bad, "api.example.test"), Ok(true)) {
            let types = hello_extensions(&bad).expect("a passed hello is whole");
            assert!(!types.iter().any(|t| HIDDEN_NAME.contains(t)), "{types:x?}");
        }
    }
}

/// A proxy of a Linux session on its socket pair, with a loopback server.
#[cfg(target_os = "linux")]
fn channel_proxy(name: &str) -> (Proxy, UnixStream, LogFile) {
    let log = std::env::temp_dir().join(format!("iris-egress-{name}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let file = File::options()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let (engine, helper) = UnixStream::pair().unwrap();
    let proxy = Proxy::start_on(
        Source::Channel(engine),
        EGRESS_PORT,
        allowed(),
        file,
        Box::new(Fixed(echo())),
        false,
        None,
        None,
        LOG_LIMIT,
        Arc::new(AtomicUsize::new(0)),
    )
    .unwrap();
    (proxy, helper, LogFile(log))
}

/// A connection of the agent, as the helper hands it over: the proxy end
/// goes on the pair, the agent end stays with the test.
#[cfg(target_os = "linux")]
fn hand_over(helper: &UnixStream) -> io::Result<TcpStream> {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let agent = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (inner, _) = l.accept().unwrap();
    // SAFETY: both descriptors are open for the call.
    if unsafe { crate::isolation::linux::send_fd(helper.as_raw_fd(), inner.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(agent)
}

/// Issue 507: the proxy of a Linux session has no port. A connection that
/// the helper sends on the pair gets a tunnel; after the end of the proxy
/// side, the helper cannot send one more, so it reaches no other process.
#[cfg(target_os = "linux")]
#[test]
fn a_channel_proxy_takes_connections_only_from_its_pair() {
    let (proxy, helper, log) = channel_proxy("channel");
    assert_eq!(proxy.endpoint().port, EGRESS_PORT);
    let agent = hand_over(&helper).unwrap();
    let (mut s, line) = send_on(agent, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    let mut sent = hello("api.example.test");
    sent.extend_from_slice(b"ping");
    s.write_all(&sent).unwrap();
    let mut got = vec![0u8; sent.len()];
    s.read_exact(&mut got).unwrap();
    assert_eq!(got, sent);
    drop(s);
    // The engine ends: the helper end gets an error, never a new peer.
    proxy.stop().unwrap();
    let mut refused = 0;
    for _ in 0..3 {
        refused += hand_over(&helper).is_err() as usize;
    }
    assert_eq!(refused, 3);
    let text = log_text(&log);
    assert_eq!(text.lines().count(), 1, "{text}");
}

/// The helper ends first: the proxy sees the end of the pair, does not spin,
/// and stops at once.
#[cfg(target_os = "linux")]
#[test]
fn a_channel_proxy_stops_after_the_end_of_the_helper() {
    let (proxy, helper, _log) = channel_proxy("channel-end");
    drop(helper);
    thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    proxy.stop().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
}

/// A descriptor that is not a TCP connection is closed with no answer and
/// no log line, a message with no descriptor is dropped, and the proxy
/// goes on.
#[cfg(target_os = "linux")]
#[test]
fn a_channel_proxy_closes_a_descriptor_that_is_not_tcp() {
    let (proxy, helper, log) = channel_proxy("channel-unix");
    let (mut mine, theirs) = UnixStream::pair().unwrap();
    // SAFETY: both descriptors are open for the call.
    assert_eq!(
        unsafe { crate::isolation::linux::send_fd(helper.as_raw_fd(), theirs.as_raw_fd()) },
        0
    );
    drop(theirs);
    mine.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut b = [0u8; 1];
    assert_eq!(mine.read(&mut b).unwrap(), 0, "the proxy closed it");
    // A message with no descriptor is dropped too; the pair still works.
    (&helper).write_all(b"x").unwrap();
    let agent = hand_over(&helper).unwrap();
    let (_s, line) = send_on(agent, "CONNECT api.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(line.starts_with("HTTP/1.1 200"), "{line}");
    drop(_s);
    proxy.stop().unwrap();
    assert_eq!(log_text(&log).lines().count(), 1);
}

#[test]
fn the_sandbox_port_is_never_the_local_port() {
    for local in [
        None,
        Some(1),
        Some(EGRESS_PORT - 1),
        Some(EGRESS_PORT),
        Some(EGRESS_PORT + 1),
        Some(u16::MAX),
    ] {
        assert_ne!(Some(sandbox_port(local)), local);
    }
}
