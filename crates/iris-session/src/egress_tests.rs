//! Tests of the egress proxy. They are unit tests, because only a test can
//! make a proxy that connects to a loopback test server.

use super::*;
use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, Ipv6Addr};
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
    let proxy =
        Proxy::start_with(allowed(), file, Box::new(Fixed(to)), public_only, log_limit).unwrap();
    Box_ {
        proxy,
        log: LogFile(log),
    }
}

fn send(p: &Proxy, head: &str) -> (TcpStream, String) {
    let mut s = TcpStream::connect(("127.0.0.1", p.port())).unwrap();
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
    s.write_all(b"ping").unwrap();
    let mut got = [0u8; 4];
    s.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"ping");
    drop(s);
    b.proxy.stop().unwrap();
    let text = log_text(&b.log);
    assert!(text.contains("host=api.example.test up=4 down=4"), "{text}");
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
    let file = File::options().create_new(true).append(true).open(&log).unwrap();
    let proxy = Proxy::start_with(allowed(), file, Box::new(Stalled), false, LOG_LIMIT).unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n").unwrap();
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
                2 => s.write_all(b"CONNECT evil.test:443 HTTP/1.1\r\n\r\n").unwrap(),
                3 => s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\nhi").unwrap(),
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
