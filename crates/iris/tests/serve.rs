//! Start the real `iris serve` program and get the page over HTTP.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

#[test]
fn version_flag_prints_the_version_line() {
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("iris "));
}

#[test]
fn serve_shows_a_page() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["serve", "--addr", "127.0.0.1:0", "--data"])
        .arg(std::env::temp_dir().join(format!("iris-serve-page-{}", std::process::id())))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = line.trim().rsplit("http://").next().unwrap().to_string();

    let mut stream = TcpStream::connect(&addr).unwrap();
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("<title>Iris</title>"), "{response}");
}

/// A service manager or a parent can close the stderr pipe. A failed log
/// write must not stop the engine.
#[test]
fn serve_keeps_running_when_stderr_is_closed() {
    // The test picks a free port and frees it, and another test can take it
    // before `iris serve` binds it (issue #81). A program that ends at once
    // gets a new port, at most three times.
    let mut tries = 0;
    let (mut child, addr) = loop {
        tries += 1;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let addr = format!("127.0.0.1:{port}");
        let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["serve", "--addr", &addr, "--data"])
            .arg(std::env::temp_dir().join(format!("iris-serve-closed-{}", std::process::id())))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Close the read end before the program writes its first line.
        drop(child.stderr.take());
        let begin = std::time::Instant::now();
        let mut up = false;
        while begin.elapsed() < std::time::Duration::from_secs(10) {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if TcpStream::connect(&addr).is_ok() {
                up = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if up || tries == 3 {
            break (child, addr);
        }
        let _ = child.wait();
    };

    let mut response = String::new();
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        let request = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        if stream.write_all(request.as_bytes()).is_ok() {
            let _ = stream.read_to_string(&mut response);
        }
    }
    let exited = child.try_wait().unwrap();
    let _ = child.kill();
    let _ = child.wait();

    assert!(exited.is_none(), "iris serve ended: {exited:?}");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}
