//! The same test cases run against a fake server of each provider. No network, no real key.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use iris_model::{ApiKey, Client, Config, Error, Price, Provider, Stop, DATA_END, DATA_START};
use serde_json::{json, Value};

const KEY: &str = "test-key-not-real-0123456789";
const PROVIDERS: [Provider; 2] = [Provider::Anthropic, Provider::OpenAiCompatible];

struct Seen {
    path: String,
    headers: HeaderMap,
    body: Value,
}

/// A queued answer: status, extra headers, body.
type Answer = (u16, Vec<(&'static str, String)>, Vec<u8>);

#[derive(Default)]
struct Fake {
    answers: Mutex<VecDeque<Answer>>,
    seen: Mutex<Vec<Seen>>,
}

/// Starts a fake server on a random port in a background thread.
fn start(fake: Arc<Fake>) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let app = Router::new().fallback(move |uri: Uri, headers: HeaderMap, body: Bytes| {
                let fake = fake.clone();
                async move {
                    fake.seen.lock().unwrap().push(Seen {
                        path: uri.path().to_string(),
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                    });
                    let (status, extra, body) = fake
                        .answers
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("no answer queued");
                    let mut resp: Response = body.into_response();
                    *resp.status_mut() = StatusCode::from_u16(status).unwrap();
                    for (k, v) in extra {
                        resp.headers_mut().insert(k, v.parse().unwrap());
                    }
                    resp
                }
            });
            axum::serve(listener, app).await.unwrap();
        });
    });
    addr
}

/// A successful answer in the format of the provider.
fn ok_body(p: Provider, text: &str, input: u64, output: u64, stop: &str) -> String {
    match p {
        Provider::Anthropic => json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "fake-model",
            "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": text}],
            "stop_reason": stop,
            "usage": {"input_tokens": input, "output_tokens": output}
        }),
        Provider::OpenAiCompatible => json!({
            "id": "c1", "object": "chat.completion", "model": "fake-model",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": stop}],
            "usage": {"prompt_tokens": input, "completion_tokens": output}
        }),
    }
    .to_string()
}

fn stop_word(p: Provider, stop: Stop) -> &'static str {
    match (p, stop) {
        (Provider::Anthropic, Stop::End) => "end_turn",
        (Provider::Anthropic, Stop::Truncated) => "max_tokens",
        (Provider::Anthropic, Stop::Refused) => "refusal",
        (Provider::OpenAiCompatible, Stop::End) => "stop",
        (Provider::OpenAiCompatible, Stop::Truncated) => "length",
        (Provider::OpenAiCompatible, Stop::Refused) => "content_filter",
    }
}

fn setup(p: Provider, answers: Vec<Answer>) -> (Client, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    fake.answers.lock().unwrap().extend(answers);
    let addr = start(fake.clone());
    let base_url = match p {
        Provider::Anthropic => format!("http://{addr}"),
        Provider::OpenAiCompatible => format!("http://{addr}/v1"),
    };
    let client = Client::new(Config {
        provider: p,
        base_url,
        model: "model-from-config".into(),
        api_key: Some(ApiKey::new(KEY)),
        max_tokens: 1000,
        timeout: Duration::from_secs(10),
        price: Some(Price {
            input_micro_usd_per_mtok: 4_000_000,
            output_micro_usd_per_mtok: 20_000_000,
        }),
    });
    (client, fake)
}

#[test]
fn answer_gives_json_tokens_and_cost() {
    for p in PROVIDERS {
        let body = ok_body(
            p,
            r#"{"items": [], "phishing": false}"#,
            1200,
            300,
            stop_word(p, Stop::End),
        );
        let (client, fake) = setup(p, vec![(200, vec![], body.into())]);
        let reply = client
            .ask("Find the items.", "Hello Ana, lunch on Friday?")
            .unwrap();
        assert_eq!(reply.stop, Stop::End, "{p:?}");
        assert_eq!(
            reply.json().unwrap(),
            json!({"items": [], "phishing": false})
        );
        assert_eq!(reply.usage.input_tokens, 1200);
        assert_eq!(reply.usage.output_tokens, 300);
        // 1200 * 4 + 300 * 20 micro-USD = 10,800 micro-USD.
        assert_eq!(reply.usage.cost_micro_usd, Some(10_800));

        let seen = fake.seen.lock().unwrap();
        let req = &seen[0];
        assert_eq!(req.body["model"], "model-from-config");
        assert_eq!(req.body["max_tokens"], 1000);
        let user = match p {
            Provider::Anthropic => {
                assert_eq!(req.path, "/v1/messages");
                assert_eq!(req.headers["x-api-key"], KEY);
                assert_eq!(req.headers["anthropic-version"], "2023-06-01");
                assert!(req.body["system"]
                    .as_str()
                    .unwrap()
                    .contains("Find the items."));
                req.body["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .to_string()
            }
            Provider::OpenAiCompatible => {
                assert_eq!(req.path, "/v1/chat/completions");
                assert_eq!(req.headers["authorization"], format!("Bearer {KEY}"));
                assert_eq!(req.body["messages"][0]["role"], "system");
                assert!(req.body["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("Find the items."));
                req.body["messages"][1]["content"]
                    .as_str()
                    .unwrap()
                    .to_string()
            }
        };
        assert!(
            user.starts_with(DATA_START) && user.ends_with(DATA_END),
            "{user}"
        );
        assert!(user.contains("Hello Ana, lunch on Friday?"));
    }
}

#[test]
fn json_in_a_code_fence_is_read() {
    for p in PROVIDERS {
        let body = ok_body(p, "```json\n{\"a\": 1}\n```", 1, 1, stop_word(p, Stop::End));
        let (client, _) = setup(p, vec![(200, vec![], body.into())]);
        assert_eq!(
            client.ask("x", "y").unwrap().json().unwrap(),
            json!({"a": 1})
        );
    }
}

#[test]
fn text_that_is_not_json_keeps_the_usage() {
    for p in PROVIDERS {
        let body = ok_body(
            p,
            "Sure! Here are the items.",
            50,
            7,
            stop_word(p, Stop::End),
        );
        let (client, _) = setup(p, vec![(200, vec![], body.into())]);
        let reply = client.ask("x", "y").unwrap();
        assert!(matches!(reply.json(), Err(Error::NotJson)), "{p:?}");
        assert_eq!(reply.usage.output_tokens, 7);
    }
}

#[test]
fn truncated_and_refused_answers_are_marked() {
    for p in PROVIDERS {
        for stop in [Stop::Truncated, Stop::Refused] {
            let body = ok_body(p, "{\"a\":", 5, 5, stop_word(p, stop));
            let (client, _) = setup(p, vec![(200, vec![], body.into())]);
            assert_eq!(client.ask("x", "y").unwrap().stop, stop, "{p:?}");
        }
    }
}

#[test]
fn rate_limit_gives_the_wait_time() {
    for p in PROVIDERS {
        let (client, _) = setup(
            p,
            vec![(429, vec![("retry-after", "17".into())], b"{}".to_vec())],
        );
        match client.ask("x", "y") {
            Err(Error::RateLimited { retry_after }) => {
                assert_eq!(retry_after, Some(Duration::from_secs(17)))
            }
            other => panic!("{p:?}: {other:?}"),
        }
    }
}

#[test]
fn server_errors_are_retryable() {
    for p in PROVIDERS {
        for status in [500, 503, 529] {
            let (client, _) = setup(p, vec![(status, vec![], b"busy".to_vec())]);
            match client.ask("x", "y") {
                Err(e @ Error::Server(s)) => {
                    assert_eq!(s, status);
                    assert!(e.is_retryable());
                }
                other => panic!("{p:?}: {other:?}"),
            }
        }
    }
}

#[test]
fn rejection_never_shows_the_key() {
    for p in PROVIDERS {
        // A hostile or careless server echoes the key in its error message.
        let body =
            json!({"error": {"type": "authentication_error", "message": format!("bad key {KEY}")}})
                .to_string();
        let (client, _) = setup(p, vec![(401, vec![], body.into())]);
        let err = client.ask("x", "y").unwrap_err();
        assert!(
            matches!(err, Error::Rejected { status: 401, .. }),
            "{p:?}: {err:?}"
        );
        assert!(!err.is_retryable());
        for text in [format!("{err}"), format!("{err:?}")] {
            assert!(!text.contains(KEY), "{text}");
            assert!(text.contains("bad key"), "{text}");
        }
    }
}

#[test]
fn local_model_needs_no_key_and_no_price() {
    let fake = Arc::new(Fake::default());
    let p = Provider::OpenAiCompatible;
    fake.answers
        .lock()
        .unwrap()
        .push_back((200, vec![], ok_body(p, "{}", 10, 2, "stop").into()));
    let addr = start(fake.clone());
    let client = Client::new(Config {
        api_key: None,
        price: None,
        ..Config::local(format!("http://{addr}/v1"), "qwen-local")
    });
    let reply = client.ask("x", "y").unwrap();
    assert_eq!(reply.usage.cost_micro_usd, None);
    assert!(!fake.seen.lock().unwrap()[0]
        .headers
        .contains_key("authorization"));
}

#[test]
fn hosted_model_without_a_key_does_not_call() {
    let client = Client::new(Config {
        api_key: None,
        ..Config::anthropic("model-from-config", ApiKey::new(KEY))
    });
    assert!(matches!(client.ask("x", "y"), Err(Error::NoKey)));
}

#[test]
fn transport_error_when_no_server() {
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    for p in PROVIDERS {
        let client = Client::new(Config {
            provider: p,
            base_url: format!("http://{addr}"),
            ..Config::anthropic("m", ApiKey::new(KEY))
        });
        let err = client.ask("x", "y").unwrap_err();
        assert!(matches!(err, Error::Transport(_)), "{p:?}: {err:?}");
        assert!(err.is_retryable());
    }
}

#[test]
fn config_debug_hides_the_key() {
    let config = Config::anthropic("m", ApiKey::new(KEY));
    assert!(!format!("{config:?}").contains(KEY));
}

#[test]
fn key_across_the_cut_is_hidden() {
    for p in PROVIDERS {
        // The key starts just before the cut at 300 characters.
        let message = format!("{}{KEY} end", "x".repeat(290));
        let body = json!({"error": {"message": message}}).to_string();
        let (client, _) = setup(p, vec![(401, vec![], body.into())]);
        let err = client.ask("x", "y").unwrap_err();
        let text = format!("{err} {err:?}");
        assert!(!text.contains(&KEY[..8]), "{p:?}: {text}");
    }
}

#[test]
fn status_decides_the_error_for_any_body() {
    let big = vec![b'a'; 2 << 20];
    let bad_utf8 = vec![0xff, 0xfe, b'k', b'e', b'y'];
    for p in PROVIDERS {
        let (client, _) = setup(
            p,
            vec![(429, vec![("retry-after", "3".into())], big.clone())],
        );
        match client.ask("x", "y") {
            Err(Error::RateLimited { retry_after }) => {
                assert_eq!(retry_after, Some(Duration::from_secs(3)))
            }
            other => panic!("{p:?}: {other:?}"),
        }
        let (client, _) = setup(p, vec![(503, vec![], big.clone())]);
        assert!(
            matches!(client.ask("x", "y"), Err(Error::Server(503))),
            "{p:?}"
        );
        let (client, _) = setup(p, vec![(401, vec![], bad_utf8.clone())]);
        assert!(
            matches!(
                client.ask("x", "y"),
                Err(Error::Rejected { status: 401, .. })
            ),
            "{p:?}"
        );
        let (client, _) = setup(p, vec![(400, vec![], big.clone())]);
        assert!(
            matches!(
                client.ask("x", "y"),
                Err(Error::Rejected { status: 400, .. })
            ),
            "{p:?}"
        );
    }
}

#[test]
fn openai_refusal_field_is_a_refusal() {
    let p = Provider::OpenAiCompatible;
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "refusal": "I cannot help."}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 3, "completion_tokens": 4}
    })
    .to_string();
    let (client, _) = setup(p, vec![(200, vec![], body.into())]);
    assert_eq!(client.ask("x", "y").unwrap().stop, Stop::Refused);
}

#[test]
fn retry_after_as_a_date_gives_the_wait_time() {
    let at = std::time::SystemTime::now() + Duration::from_secs(120);
    let date = httpdate::fmt_http_date(at);
    for p in PROVIDERS {
        let (client, _) = setup(
            p,
            vec![(429, vec![("retry-after", date.clone())], b"{}".to_vec())],
        );
        match client.ask("x", "y") {
            Err(Error::RateLimited {
                retry_after: Some(d),
            }) => {
                assert!((100..=121).contains(&d.as_secs()), "{p:?}: {d:?}")
            }
            other => panic!("{p:?}: {other:?}"),
        }
        // A date in the past means: try now.
        let (client, _) = setup(
            p,
            vec![(
                429,
                vec![("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT".into())],
                b"{}".to_vec(),
            )],
        );
        assert!(matches!(
            client.ask("x", "y"),
            Err(Error::RateLimited {
                retry_after: Some(Duration::ZERO)
            })
        ));
    }
}

#[test]
fn oversized_answer_is_a_bad_answer() {
    for p in PROVIDERS {
        let body = ok_body(p, &"a".repeat(2 << 20), 1, 1, stop_word(p, Stop::End));
        let (client, _) = setup(p, vec![(200, vec![], body.into())]);
        let err = client.ask("x", "y").unwrap_err();
        assert!(matches!(err, Error::BadAnswer(_)), "{p:?}: {err:?}");
        assert!(!err.is_retryable());
    }
}

#[test]
fn malformed_answers_are_bad_answers() {
    let anthropic = [
        json!({"content": [{"type": "text", "text": "{}"}], "usage": {}}),
        json!({"content": [{"type": "text", "text": "{}"}], "stop_reason": "tool_use"}),
        json!({"stop_reason": "end_turn"}),
    ];
    let openai = [
        json!({"choices": [{"message": {"content": "{}"}}]}),
        json!({"choices": [{"message": {"content": "{}"}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [{"message": {}, "finish_reason": "stop"}]}),
        json!({"choices": [{"finish_reason": "stop"}]}),
        json!({"choices": []}),
    ];
    for (p, bodies) in [
        (Provider::Anthropic, &anthropic[..]),
        (Provider::OpenAiCompatible, &openai[..]),
    ] {
        for body in bodies {
            let (client, _) = setup(p, vec![(200, vec![], body.to_string().into())]);
            let err = client.ask("x", "y").unwrap_err();
            assert!(matches!(err, Error::BadAnswer(_)), "{p:?} {body}: {err:?}");
        }
    }
}

#[test]
fn truncated_answer_with_no_text_is_allowed() {
    let p = Provider::OpenAiCompatible;
    let body =
        json!({"choices": [{"message": {"content": null}, "finish_reason": "length"}]}).to_string();
    let (client, _) = setup(p, vec![(200, vec![], body.into())]);
    let reply = client.ask("x", "y").unwrap();
    assert_eq!((reply.stop, reply.text.as_str()), (Stop::Truncated, ""));
}
