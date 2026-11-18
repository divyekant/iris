//! Sign-in tests against a fake OAuth token endpoint. No network, no real
//! client and no real token: all values here are made up.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use iris_auth::{Client, Error, Secret, CALLBACK, SCOPE, WRITE_SCOPE};
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
    finish_as(answer, false)
}

/// A sign-in that asks for write access when `write` is true.
fn finish_as(answer: (u16, String), write: bool) -> Result<iris_auth::Tokens, Error> {
    let (_fake, uri) = token_server(vec![answer]);
    let client = test_client(Some(&uri));
    let sign_in = if write {
        client.sign_in_write().unwrap()
    } else {
        client.sign_in().unwrap()
    };
    let state = params(&sign_in.url())["state"].clone();
    sign_in.finish(&format!("{}?state={state}&code=c", sign_in.redirect_uri()))
}

fn granted(scope: &str) -> (u16, String) {
    (
        200,
        format!(
            r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}","scope":"{scope}"}}"#
        ),
    )
}

#[test]
fn a_write_sign_in_asks_for_gmail_modify_only() {
    let client = test_client(None);
    assert_eq!(
        params(&client.sign_in_write().unwrap().url())["scope"],
        WRITE_SCOPE
    );
    // The write sign-in does not change the client: its next sign-in reads only.
    assert_eq!(params(&client.sign_in().unwrap().url())["scope"], SCOPE);
}

/// The access of a sign-in comes from the granted scopes, never from the
/// scope that Iris asked for.
#[test]
fn write_access_comes_from_the_granted_scope() {
    let both = format!("{SCOPE} {WRITE_SCOPE}");
    let other = format!("openid {WRITE_SCOPE}");
    // (asked for write, granted scope text, expected: Some(write) or None for an error)
    let cases: [(bool, &str, Option<bool>); 8] = [
        (false, SCOPE, Some(false)),
        (false, WRITE_SCOPE, Some(true)),
        (false, &both, Some(true)),
        (true, WRITE_SCOPE, Some(true)),
        (true, &other, Some(true)),
        (true, &both, Some(true)),
        (true, SCOPE, None),
        (true, "openid", None),
    ];
    for (write, scope, want) in cases {
        let got = finish_as(granted(scope), write);
        match want {
            Some(w) => assert_eq!(got.unwrap().write, w, "{write} {scope}"),
            None => assert!(
                matches!(got, Err(Error::WriteScopeMissing)),
                "{write} {scope}: {got:?}"
            ),
        }
    }
    // A scope that only starts like the write scope is not the write scope.
    let near = format!("{SCOPE} {WRITE_SCOPE}.x");
    assert!(!finish_as(granted(&near), false).unwrap().write);
    assert!(matches!(
        finish_as(granted(&near), true),
        Err(Error::WriteScopeMissing)
    ));
}

#[test]
fn a_write_sign_in_with_no_scope_text_is_write_scope_missing() {
    let answer =
        format!(r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}"}}"#);
    assert!(matches!(
        finish_as((200, answer), true),
        Err(Error::WriteScopeMissing)
    ));
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
            // No pause: keep the queue of the listener full.
            if open.len() > 2000 {
                open.clear();
            }
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
    let file = client_json("https://attacker.example/token").replace(
        "https://accounts.google.com/o/oauth2/auth",
        "https://attacker.example/auth",
    );
    let client = Client::from_json(file.as_bytes()).unwrap();
    assert_eq!(client.token_uri(), iris_auth::GOOGLE_TOKEN_URI);
    assert!(client
        .sign_in()
        .unwrap()
        .url()
        .starts_with("https://accounts.google.com/o/oauth2/auth?"));
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

#[test]
fn a_huge_wait_time_does_not_panic() {
    let sign_in = test_client(None).sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let redirect = sign_in.redirect_uri().to_string();
    let browser = std::thread::spawn(move || {
        browser_get(&redirect, &format!("error=access_denied&state={state}"))
    });
    assert!(matches!(sign_in.wait(Duration::MAX), Err(Error::Denied(_))));
    assert!(browser.join().unwrap().contains("200"));
}

#[test]
fn an_answer_with_no_scope_is_scope_missing() {
    let answer =
        format!(r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"{REFRESH}"}}"#);
    assert!(matches!(
        finish_with((200, answer)),
        Err(Error::ScopeMissing)
    ));
}

#[test]
fn a_refresh_answer_with_no_scope_is_accepted() {
    let (_fake, uri) = token_server(vec![(
        200,
        r#"{"access_token":"new-access","expires_in":3599}"#.into(),
    )]);
    let client = test_client(Some(&uri));
    assert_eq!(
        client
            .refresh(&Secret::new(REFRESH))
            .unwrap()
            .token
            .expose(),
        "new-access"
    );
}

#[test]
fn an_idle_connection_does_not_hide_the_callback() {
    let sign_in = test_client(None).sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let redirect = sign_in.redirect_uri().to_string();
    let host = redirect[7..].split('/').next().unwrap().to_string();
    // A browser can open a spare connection that sends nothing.
    let _idle = TcpStream::connect(&host).unwrap();
    let browser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        browser_get(&redirect, &format!("error=access_denied&state={state}"))
    });
    let start = std::time::Instant::now();
    assert!(matches!(
        sign_in.wait(Duration::from_secs(3)),
        Err(Error::Denied(_))
    ));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert!(browser.join().unwrap().contains("200"));
}

#[test]
fn empty_tokens_are_refused() {
    let empty_refresh = format!(
        r#"{{"access_token":"{ACCESS}","expires_in":3599,"refresh_token":"","scope":"{SCOPE}"}}"#
    );
    assert!(matches!(
        finish_with((200, empty_refresh)),
        Err(Error::NoRefreshToken)
    ));
    let empty_access = format!(
        r#"{{"access_token":"","expires_in":3599,"refresh_token":"{REFRESH}","scope":"{SCOPE}"}}"#
    );
    assert!(matches!(
        finish_with((200, empty_access)),
        Err(Error::BadAnswer)
    ));
    let (_fake, uri) = token_server(vec![(
        200,
        r#"{"access_token":"","expires_in":3599}"#.into(),
    )]);
    assert!(matches!(
        test_client(Some(&uri)).refresh(&Secret::new(REFRESH)),
        Err(Error::BadAnswer)
    ));
}

// Microsoft sign-in (Outlook spec section 7). All values are made up.

const APP_ID: &str = "00000000-1111-2222-3333-444444444444";
const MS_SCOPE: &str =
    "https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send https://graph.microsoft.com/MailboxSettings.Read https://graph.microsoft.com/User.Read";

fn ms_client(token_uri: &str) -> Client {
    Client::microsoft(APP_ID, "common")
        .unwrap()
        .with_test_token_uri(token_uri)
        .unwrap()
}

fn ms_answer(refresh: &str, scope: &str) -> (u16, String) {
    (
        200,
        format!(
            r#"{{"token_type":"Bearer","scope":"{scope}","expires_in":3599,"access_token":"{ACCESS}","refresh_token":"{refresh}"}}"#
        ),
    )
}

#[test]
fn microsoft_sign_in_with_pkce_and_no_secret() {
    let (fake, uri) = token_server(vec![ms_answer(REFRESH, MS_SCOPE)]);
    let client = ms_client(&uri);
    let sign_in = client.sign_in().unwrap();
    let url = sign_in.url();
    assert!(
        url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"),
        "{url}"
    );
    let p = params(&url);
    assert_eq!(p["scope"], iris_auth::MICROSOFT_SCOPE);
    assert!(p["scope"].split(' ').any(|s| s == "offline_access"));
    assert_eq!(p["client_id"], APP_ID);
    assert_eq!(p["code_challenge_method"], "S256");
    assert!(!p.contains_key("access_type"));
    let redirect = p["redirect_uri"].clone();
    assert!(redirect.starts_with("http://localhost:") && redirect.ends_with(CALLBACK));
    let state = p["state"].clone();
    let challenge = p["code_challenge"].clone();
    let browser =
        std::thread::spawn(move || browser_get(&redirect, &format!("state={state}&code=m1")));
    let tokens = sign_in.wait(Duration::from_secs(10)).unwrap();
    assert!(browser.join().unwrap().contains("200"));
    assert_eq!(tokens.refresh.expose(), REFRESH);
    let form = fake.forms.lock().unwrap()[0].clone();
    assert_eq!(form["code"], "m1");
    assert_eq!(form["client_id"], APP_ID);
    assert!(
        !form.contains_key("client_secret"),
        "a public client has no secret"
    );
    assert_eq!(form["redirect_uri"], sign_in.redirect_uri());
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(&form["code_verifier"])),
        challenge
    );
}

#[test]
fn a_microsoft_refresh_keeps_the_new_refresh_token() {
    let (fake, uri) = token_server(vec![
        ms_answer("rotated-refresh-33", MS_SCOPE),
        (
            200,
            format!(r#"{{"access_token":"a2","expires_in":3599,"scope":"{MS_SCOPE}"}}"#),
        ),
        (
            400,
            r#"{"error":"invalid_grant","error_description":"AADSTS70000"}"#.into(),
        ),
    ]);
    let client = ms_client(&uri);
    let r = client.refresh_tokens(&Secret::new(REFRESH)).unwrap();
    assert_eq!(r.refresh.unwrap().expose(), "rotated-refresh-33");
    assert_eq!(r.access.token.expose(), ACCESS);
    let form = fake.forms.lock().unwrap()[0].clone();
    assert_eq!(form["grant_type"], "refresh_token");
    assert_eq!(form["refresh_token"], REFRESH);
    assert_eq!(form["scope"], iris_auth::MICROSOFT_SCOPE);
    assert!(!form.contains_key("client_secret"));
    // No new token in the answer: keep the old one.
    assert!(client
        .refresh_tokens(&Secret::new(REFRESH))
        .unwrap()
        .refresh
        .is_none());
    assert!(matches!(
        client.refresh_tokens(&Secret::new(REFRESH)),
        Err(Error::Revoked)
    ));
}

#[test]
fn a_missing_mail_permission_is_named() {
    let cases = [
        (
            "https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/User.Read",
            Some("Mail.Send"),
        ),
        (
            "https://graph.microsoft.com/Mail.Send",
            Some("Mail.ReadWrite"),
        ),
        ("Mail.Read Mail.Send", Some("Mail.ReadWrite")),
        ("", Some("Mail.ReadWrite")),
        // The permissions of the old scope: the category list cannot be read.
        (
            "https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send https://graph.microsoft.com/User.Read",
            Some("MailboxSettings.Read"),
        ),
        // Without the resource prefix, and in another case: accepted.
        (
            "mail.readwrite MAIL.SEND mailboxsettings.read User.Read openid profile",
            None,
        ),
        (MS_SCOPE, None),
    ];
    for (scope, missing) in cases {
        let (_, uri) = token_server(vec![ms_answer(REFRESH, scope), ms_answer(REFRESH, scope)]);
        let client = ms_client(&uri);
        let sign_in = client.sign_in().unwrap();
        let state = params(&sign_in.url())["state"].clone();
        let got = sign_in.finish(&format!("http://localhost/?state={state}&code=c"));
        let refreshed = client.refresh_tokens(&Secret::new(REFRESH));
        match missing {
            Some(p) => {
                assert!(
                    matches!(got, Err(Error::PermissionMissing(x)) if x == p),
                    "{scope}: {got:?}"
                );
                assert!(
                    matches!(refreshed, Err(Error::PermissionMissing(x)) if x == p),
                    "{scope}"
                );
                assert!(got.unwrap_err().to_string().contains(p));
            }
            None => {
                assert!(got.is_ok(), "{scope}: {got:?}");
                assert!(refreshed.is_ok(), "{scope}");
            }
        }
    }
    // A code exchange must name the scope; a refresh may leave it out.
    let no_scope = (
        200,
        format!(r#"{{"access_token":"{ACCESS}","expires_in":1,"refresh_token":"{REFRESH}"}}"#),
    );
    let (_, uri) = token_server(vec![no_scope.clone(), no_scope]);
    let client = ms_client(&uri);
    let sign_in = client.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    let got = sign_in.finish(&format!("http://localhost/?state={state}&code=c"));
    assert!(matches!(got, Err(Error::PermissionMissing(_))), "{got:?}");
    assert!(client.refresh_tokens(&Secret::new(REFRESH)).is_ok());
}

/// Examples: the client ID and the tenant are GUIDs (the tenant may also be
/// `common`), so the endpoints always stay on the fixed host.
#[test]
fn only_a_guid_or_common_reaches_the_endpoints() {
    let parts = [
        "common",
        "organizations",
        "consumers",
        "COMMON",
        APP_ID,
        "00000000-1111-2222-3333-44444444444",
        "00000000-1111-2222-3333-4444444444444",
        "0000000g-1111-2222-3333-444444444444",
        "00000000-1111-2222-3333-444444444444/x",
        "evil.example",
        "../x",
        "",
        "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",
        "00000000_1111-2222-3333-444444444444",
        "common@evil.example",
    ];
    let guid = |s: &str| {
        let p: Vec<&str> = s.split('-').collect();
        p.len() == 5
            && p.iter()
                .zip([8, 4, 4, 4, 12])
                .all(|(x, n)| x.len() == n && x.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for id in parts {
        for tenant in parts {
            let got = Client::microsoft(id, tenant);
            let ok = guid(id) && (tenant == "common" || guid(tenant));
            assert_eq!(got.is_ok(), ok, "{id} {tenant}");
            if let Ok(c) = got {
                let t = tenant.to_ascii_lowercase();
                assert_eq!(
                    c.token_uri(),
                    format!("https://login.microsoftonline.com/{t}/oauth2/v2.0/token")
                );
                let url = c.sign_in().unwrap().url();
                assert!(url.starts_with(&format!(
                    "https://login.microsoftonline.com/{t}/oauth2/v2.0/authorize?"
                )));
            }
        }
    }
}

#[test]
fn microsoft_secrets_stay_out_of_debug_text() {
    let (_, uri) = token_server(vec![ms_answer("rotated-refresh-33", MS_SCOPE)]);
    let r = ms_client(&uri)
        .refresh_tokens(&Secret::new(REFRESH))
        .unwrap();
    let text = format!("{r:?}");
    assert!(
        !text.contains("rotated-refresh-33") && !text.contains(ACCESS),
        "{text}"
    );
}

#[test]
fn a_microsoft_client_cannot_drop_the_new_refresh_token() {
    let (fake, uri) = token_server(vec![]);
    let got = ms_client(&uri).refresh(&Secret::new(REFRESH));
    assert!(matches!(got, Err(Error::RotatingRefresh)), "{got:?}");
    assert!(fake.forms.lock().unwrap().is_empty(), "no request");
}

/// A token endpoint that answers 307 to another listener. The other
/// listener must get no connection: the code, the verifier and the refresh
/// token stay on the token host.
#[test]
fn a_token_endpoint_redirect_is_not_followed() {
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = format!("http://{}/steal", other.local_addr().unwrap());
    other.set_nonblocking(true).unwrap();
    let endpoint = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/token", endpoint.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in endpoint.incoming() {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = write!(
                stream,
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    });
    let google = test_client(Some(&uri));
    let ms = ms_client(&uri);
    assert!(google.refresh(&Secret::new(REFRESH)).is_err());
    assert!(ms.refresh_tokens(&Secret::new(REFRESH)).is_err());
    let sign_in = ms.sign_in().unwrap();
    let state = params(&sign_in.url())["state"].clone();
    assert!(sign_in
        .finish(&format!("http://localhost/?state={state}&code=c"))
        .is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        other.accept().is_err(),
        "the redirect target got a connection"
    );
}
