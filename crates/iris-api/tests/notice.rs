//! The routes of notices by rule (card R5-08): `GET /api/notices` and
//! `POST /api/notices/{id}/done`.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use iris_api::Auth;
use iris_store::{NewMessage, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(store: &Arc<Store>, method: Method, uri: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn list_and_mark_a_notice_done() {
    let dir = std::env::temp_dir().join(format!("iris-api-notice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(1_000),
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
             VALUES (1, 7, 'Clinic', 1, 'Dr Lee', 'Results', 0),
                    (99, 7, 'Clinic', 1, 'Dr Lee', 'Old', 0);",
        )?;
        Ok(())
    })
    .unwrap();

    let (code, list) = call(&s, Method::GET, "/api/notices").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
    // Newest first. The mail of the second notice is gone: no message.
    assert_eq!(list[0]["id"], json!("n-2"));
    assert_eq!(list[0]["message"], Value::Null);
    assert_eq!(
        (
            &list[1]["rule"],
            &list[1]["rule_name"],
            &list[1]["title"],
            &list[1]["text"]
        ),
        (
            &json!(7),
            &json!("Clinic"),
            &json!("Dr Lee"),
            &json!("Results")
        )
    );
    assert_eq!(list[1]["message"]["id"], json!("m-1"));

    let (code, _) = call(&s, Method::POST, "/api/notices/n-1/done").await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = call(&s, Method::POST, "/api/notices/n-1/done").await;
    assert_eq!(code, StatusCode::OK, "done again is fine");
    let (code, _) = call(&s, Method::POST, "/api/notices/n-9/done").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = call(&s, Method::POST, "/api/notices/w-1/done").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (_, list) = call(&s, Method::GET, "/api/notices").await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Issue #1045: with a model set, both notice outputs of the API (`GET
/// /api/notices` and the page of the Mac app) show no sender and no
/// subject of a mail whose understanding did not finish, and show them
/// after it finished.
#[tokio::test]
async fn a_notice_of_unchecked_mail_shows_no_mail_words() {
    let dir =
        std::env::temp_dir().join(format!("iris-api-notice-unchecked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(1_000),
        header: iris_store::Header {
            from_addr: Some("lee@clinic.example".into()),
            from_name: Some("Dr Lee".into()),
            subject: Some("Results".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens, timeout_s)
               VALUES (1, 'local', 'http://127.0.0.1:1', 'fake', 1, 1000, 30);
             INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
               VALUES (1, 7, 'Clinic', 1, 'Dr Lee', 'Results', 0);",
        )?;
        Ok(())
    })
    .unwrap();
    let words = |n: &Value| -> Vec<Value> {
        vec![
            n["title"].clone(),
            n["text"].clone(),
            n["message"]["from"]["name"].clone(),
            n["message"]["from"]["address"].clone(),
            n["message"]["subject"].clone(),
        ]
    };
    let (_, list) = call(&s, Method::GET, "/api/notices").await;
    let shown = &list[0];
    assert_eq!(shown["message"]["id"], json!("m-1"));
    assert_eq!(
        words(shown),
        [
            json!(iris_policy::notice::UNCHECKED_TITLE),
            json!(iris_policy::notice::UNCHECKED_TEXT),
            Value::Null,
            Value::Null,
            Value::Null
        ]
    );
    let app =
        serde_json::to_value(&iris_api::notices_after(&s, 0, 10).unwrap().notices[0].1).unwrap();
    assert_eq!(words(&app), words(shown));
    s.write(|tx| {
        tx.execute("UPDATE message SET understood_at = 1 WHERE id = 1", [])?;
        Ok(())
    })
    .unwrap();
    let (_, list) = call(&s, Method::GET, "/api/notices").await;
    assert_eq!(
        words(&list[0]),
        [
            json!("Dr Lee"),
            json!("Results"),
            json!("Dr Lee"),
            json!("lee@clinic.example"),
            json!("Results")
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
