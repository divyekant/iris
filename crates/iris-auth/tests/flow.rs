//! Sign-in tests against a fake OAuth token endpoint. No network, no real
//! client and no real token: all values here are made up.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use iris_auth::{Client, Error, Secret, CALLBACK, SCOPE};
use sha2::{Digest, Sha256};

const CLIENT_SECRET: &str = "fake-client-secret-77";
const ACCESS: &str = "fake-access-token-11";
const REFRESH: &str = "fake-refresh-token-22";

#[derive(Default)]
struct Fake {
    answers: Mutex<VecDeque<(u16, String)>>,
    forms: Mutex<Vec<HashMap<String, String>>>,
}

/// A token endpoint on a random port. Each request takes the next queued answer.
fn token_server(answers: Vec<(u16, String)>) -> (Arc<Fake>, String) {
    let fake = Arc::new(Fake::default());
    *fake.answers.lock().unwrap() = answers.into();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/token", listener.local_addr().unwrap());
    let f = fake.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = BufReader::new(stream.unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                stream.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let form = form_urlencoded::parse(&body).into_owned().collect();
            f.forms.lock().unwrap().push(form);
            let (status, body) = f.answers.lock().unwrap().pop_front().unwrap();
            write!(
                stream.get_mut(),
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (fake, uri)
}

/// The file names an attacker endpoint. Iris must never use it.
const ATTACKER: &str = "http://127.0.0.1:80@attacker.example/token";

fn test_client(token_uri: Option<&str>) -> Client {
    let client = Client::from_json(client_json(ATTACKER).as_bytes()).unwrap();
    match token_uri {
        Some(uri) => client.with_test_token_uri(uri).unwrap(),
        None => client,
    }
}

fn client_json(token_uri: &str) -> String {
    format!(
        r#"{{"installed":{{"client_id":"fake-id.apps.googleusercontent.com","project_id":"p",
        "auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"{token_uri}",
        "client_secret":"{CLIENT_SECRET}","redirect_uris":["http://localhost"]}}}}"#
    )
}

fn good_answer() -> (u16, String) {
    (
        200,
        format!(
            r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}","scope":"{SCOPE}","token_type":"Bearer"}}"#
        ),
    )
}

fn params(url: &str) -> HashMap<String, String> {
    let query = url.split_once('?').unwrap().1;
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

/// Acts as the browser: one GET to the loopback listener. Gives the status line.
fn browser_get(redirect_uri: &str, query: &str) -> String {
    let rest = redirect_uri.strip_prefix("http://").unwrap();
    let (host, _) = rest.split_once('/').unwrap();
    let path = if query.is_empty() {
        "/favicon.ico".to_string()
    } else {
        format!("{CALLBACK}?{query}")
    };
    let mut s = TcpStream::connect(host).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.lines().next().unwrap().to_string()
}

#[test]
fn loopback_sign_in_with_pkce() {
    let (fake, uri) = token_server(vec![good_answer()]);
    let client = test_client(Some(&uri));
    let sign_in = client.sign_in().unwrap();
    let p = params(&sign_in.url());
    assert_eq!(p["scope"], SCOPE, "only gmail.readonly");
    assert_eq!(p["code_challenge_method"], "S256");
    assert_eq!(p["access_type"], "offline");
    assert_eq!(p["prompt"], "consent");
    assert_eq!(p["response_type"], "code");
    let redirect = p["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with(CALLBACK));
    assert_eq!(redirect, sign_in.redirect_uri());
    let state = p["state"].clone();
    let challenge = p["code_challenge"].clone();

    let browser = std::thread::spawn(move || {
        let other = browser_get(&redirect, "");
        let wrong = browser_get(&redirect, "state=other&code=c1");
        let right = browser_get(&redirect, &format!("state={state}&code=c2"));
        (other, wrong, right)
    });
    let tokens = sign_in.wait(Duration::from_secs(10)).unwrap();
    let (other, wrong, right) = browser.join().unwrap();
    assert!(other.contains("404"), "{other}");
    assert!(
        wrong.contains("400"),
        "a wrong state does not end the wait: {wrong}"
    );
    assert!(right.contains("200"), "{right}");

    assert_eq!(tokens.access.token.expose(), ACCESS);
    assert_eq!(tokens.refresh.expose(), REFRESH);
    assert_eq!(tokens.refresh_expires_in, None);

    let forms = fake.forms.lock().unwrap();
    assert_eq!(forms.len(), 1, "the wrong state made no exchange");
    let form = &forms[0];
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "c2");
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["redirect_uri"], sign_in.redirect_uri());
    let verifier = &form["code_verifier"];
    assert_eq!(verifier.len(), 43);
    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier)), challenge);
}

#[test]
fn each_sign_in_has_a_new_state_and_verifier() {
    let client = test_client(None);
    let a = params(&client.sign_in().unwrap().url());
    let b = params(&client.sign_in().unwrap().url());
    assert_ne!(a["state"], b["state"]);
    assert_ne!(a["code_challenge"], b["code_challenge"]);
}

#[test]
fn copied_address_completes_a_remote_sign_in() {
    let (fake, uri) = token_server(vec![good_answer()]);
    let client = test_client(Some(&uri));
    let sign_in = client.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let base = sign_in.redirect_uri().to_string();

    assert!(matches!(
        sign_in.finish(&format!("{base}?state=bad&code=c")),
        Err(Error::State)
    ));
    assert!(matches!(sign_in.finish(&base), Err(Error::BadAddress)));
    assert!(matches!(
        sign_in.finish(&format!("{base}?state={state}")),
        Err(Error::BadAddress)
    ));
    let tokens = sign_in
        .finish(&format!("{base}?state={state}&code=c3&scope=x"))
        .unwrap();
    assert_eq!(tokens.refresh.expose(), REFRESH);
    assert_eq!(fake.forms.lock().unwrap()[0]["code"], "c3");
}

#[test]
fn refused_consent_is_denied() {
    let client = test_client(None);
    let sign_in = client.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let address = format!(
        "{}?error=access_denied&state={state}",
        sign_in.redirect_uri()
    );
    match sign_in.finish(&address) {
        Err(Error::Denied(code)) => assert_eq!(code, "access_denied"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn wait_ends_at_the_time_limit() {
    let client = test_client(None);
    let sign_in = client.sign_in().unwrap();
    assert!(matches!(
        sign_in.wait(Duration::from_millis(200)),
        Err(Error::Timeout)
    ));
}

fn finish_with(answer: (u16, String)) -> Result<iris_auth::Tokens, Error> {
    let (_fake, uri) = token_server(vec![answer]);
    let client = test_client(Some(&uri));
    let sign_in = client.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    sign_in.finish(&format!("{}?state={state}&code=c", sign_in.redirect_uri()))
}

#[test]
fn a_cleared_gmail_box_is_scope_missing() {
    let answer = format!(
        r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}","scope":"openid"}}"#
    );
    assert!(matches!(
        finish_with((200, answer)),
        Err(Error::ScopeMissing)
    ));
}

#[test]
fn no_refresh_token_is_an_error() {
    let answer = format!(r#"{{"access_token":"{ACCESS}","expires_in":3599,"scope":"{SCOPE}"}}"#);
    assert!(matches!(
        finish_with((200, answer)),
        Err(Error::NoRefreshToken)
    ));
}

#[test]
fn testing_status_limit_is_reported() {
    let answer = format!(
        r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}","refresh_token_expires_in":604799,"scope":"{SCOPE}"}}"#
    );
    assert_eq!(
        finish_with((200, answer)).unwrap().refresh_expires_in,
        Some(604799)
    );
}

#[test]
fn refresh_gets_a_new_access_token() {
    let answer = format!(r#"{{"access_token":"new-access","expires_in":3599,"scope":"{SCOPE}"}}"#);
    let (fake, uri) = token_server(vec![
        (200, answer),
        (
            400,
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#
                .into(),
        ),
    ]);
    let client = test_client(Some(&uri));
    let refresh = Secret::new(REFRESH);
    assert_eq!(
        client.refresh(&refresh).unwrap().token.expose(),
        "new-access"
    );
    let form = fake.forms.lock().unwrap()[0].clone();
    assert_eq!(form["grant_type"], "refresh_token");
    assert_eq!(form["refresh_token"], REFRESH);
    assert!(matches!(client.refresh(&refresh), Err(Error::Revoked)));
}

#[test]
fn client_file_errors() {
    let web = r#"{"web":{"client_id":"a","client_secret":"b"}}"#;
    let e = Client::from_json(web.as_bytes()).err().unwrap();
    assert!(e.to_string().contains("Desktop app"), "{e}");
    let empty = r#"{"installed":{"client_id":"a","client_secret":"","auth_uri":"https://a","token_uri":"https://b"}}"#;
    assert!(Client::from_json(empty.as_bytes()).is_err());
}

/// No secret may show in a `Debug` text or in an error text.
#[test]
fn secrets_stay_out_of_logs_and_errors() {
    let mut texts = Vec::new();

    // A client file with a wrong type: the parser error would quote the value.
    let bad =
        format!(r#"{{"installed":{{"client_id":"a","client_secret":1,"x":"{CLIENT_SECRET}"}}}}"#);
    let e = Client::from_json(bad.as_bytes()).err().unwrap();
    texts.push(format!("{e} {e:?}"));
    let bad = format!(
        r#"{{"installed":{{"client_id":"a","client_secret":"{CLIENT_SECRET}","client_id":7}}}}"#
    );
    let e = Client::from_json(bad.as_bytes()).err().unwrap();
    texts.push(format!("{e} {e:?}"));

    // A token endpoint that echoes the secrets back in its error.
    let echo =
        format!(r#"{{"error":"bad {REFRESH} {CLIENT_SECRET}","error_description":"{ACCESS}"}}"#);
    let e = finish_with((500, echo)).err().unwrap();
    texts.push(format!("{e} {e:?}"));
    let e = finish_with((200, format!(r#"{{"access_token":"{ACCESS}"}}"#)))
        .err()
        .unwrap();
    texts.push(format!("{e} {e:?}"));

    // A redirect error that carries a token value.
    let client = test_client(None);
    let sign_in = client.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let e = sign_in
        .finish(&format!(
            "{}?error=x%20{ACCESS}&state={state}",
            sign_in.redirect_uri()
        ))
        .err()
        .unwrap();
    texts.push(format!("{e} {e:?}"));

    let tokens = finish_with(good_answer()).unwrap();
    texts.push(format!("{tokens:?}"));

    for text in &texts {
        for secret in [CLIENT_SECRET, ACCESS, REFRESH] {
            assert!(!text.contains(secret), "{secret} in: {text}");
        }
    }
}

#[test]
fn an_expired_code_is_not_a_revoked_sign_in() {
    let answer = (
        400,
        r#"{"error":"invalid_grant","error_description":"Bad Request"}"#.to_string(),
    );
    match finish_with(answer) {
        Err(Error::Token { status: 400, code }) => assert_eq!(code, "invalid_grant"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_huge_expiry_is_a_bad_answer() {
    let answer = format!(
        r#"{{"access_token":"{ACCESS}","expires_in":18446744073709551615,"refresh_token":"{REFRESH}","scope":"{SCOPE}"}}"#
    );
    assert!(matches!(finish_with((200, answer)), Err(Error::BadAnswer)));
}

#[test]
fn busy_connections_do_not_hold_wait_past_its_time_limit() {
    let client = test_client(None);
    let sign_in = client.sign_in().unwrap();
    let host = sign_in.redirect_uri()[7..]
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s = stop.clone();
    // Connects again and again and never ends a request line.
    let noise = std::thread::spawn(move || {
        let mut open = Vec::new();
        while !s.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(mut c) = TcpStream::connect(&host) {
                let _ = c.write_all(b"GET /oauth/cal");
                open.push(c);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let start = std::time::Instant::now();
    let result = sign_in.wait(Duration::from_millis(300));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    noise.join().unwrap();
    assert!(matches!(result, Err(Error::Timeout)), "{result:?}");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn the_file_cannot_choose_the_endpoints() {
    let file = client_json("https://attacker.example/token")
        .replace("https://accounts.google.com/o/oauth2/auth", "https://attacker.example/auth");
    let client = Client::from_json(file.as_bytes()).unwrap();
    assert_eq!(client.token_uri(), iris_auth::GOOGLE_TOKEN_URI);
    assert!(client.sign_in().unwrap().url().starts_with("https://accounts.google.com/o/oauth2/auth?"));
    assert_eq!(test_client(None).token_uri(), iris_auth::GOOGLE_TOKEN_URI);
    for bad in [
        ATTACKER,
        "https://attacker.example/token",
        "http://127.0.0.1.attacker.example/token",
        "http://127.0.0.1:x@attacker.example/token",
        "http://127.0.0.1:/token",
    ] {
        assert!(test_client(None).with_test_token_uri(bad).is_err(), "{bad}");
    }
}
