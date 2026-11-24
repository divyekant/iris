//! #700: no handler uses the store on the runtime thread. `iris serve` has
//! one runtime thread, so a handler that waits for a store connection
//! there stops each other request.

use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use iris_api::Auth;
use iris_store::Store;
use tower::ServiceExt;

/// A request of the web client: with the day header, so the measure
/// count runs for it.
fn request(auth: &Auth, method: Method, uri: &str) -> Request<Body> {
    request_of(auth, method, uri, 0)
}

/// The day `days` days after today, as the web client sends it.
fn day(days: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    iris_api::rfc3339(now + days * 86_400_000)[..10].to_string()
}

/// A request of the web client with the day `days` days after today.
fn request_of(auth: &Auth, method: Method, uri: &str, days: i64) -> Request<Body> {
    let day = day(days);
    Request::builder()
        .method(method)
        .uri(uri)
        .header(iris_api::measures::DAY, day)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap()
}

/// A write route waits for the writer connection, which another thread
/// holds. The write request comes first, so it also counts the day of the
/// web client and holds the day lock (#700). On the one runtime thread of
/// the test, a later read request with the same day still gets its answer
/// before the writer is free.
#[tokio::test(flavor = "current_thread")]
async fn a_store_wait_does_not_stop_other_requests() {
    let dir = std::env::temp_dir().join(format!("iris-api-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let router = iris_api::router(s.clone(), auth.clone());

    // Another thread holds the writer for 6 seconds.
    let (held, wait) = mpsc::channel();
    let holder = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|_| {
                held.send(()).unwrap();
                std::thread::sleep(Duration::from_secs(6));
                Ok(())
            })
            .unwrap();
        })
    };
    wait.recv().unwrap();

    let write = tokio::spawn(router.clone().oneshot(request(
        &auth,
        Method::POST,
        "/api/notices/n-1/done",
    )));
    // The write request has its blocking thread, the day lock and the wait
    // for the writer before the read starts.
    tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1)))
        .await
        .unwrap();
    assert!(
        !write.is_finished(),
        "the write did not wait for the writer"
    );
    let start = Instant::now();
    let res = router
        .clone()
        .oneshot(request(&auth, Method::GET, "/api/status"))
        .await
        .unwrap();
    let took = start.elapsed();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        took < Duration::from_secs(3),
        "the read waited for the writer: {took:?}"
    );
    assert!(write.await.unwrap().is_ok());
    holder.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two requests of two days while the writer is held: each day is counted
/// once the writer is free, and a third request of the first day does not
/// wait (#700).
#[tokio::test(flavor = "current_thread")]
async fn each_day_is_counted_and_no_other_request_waits() {
    let dir = std::env::temp_dir().join(format!("iris-api-runtime-days-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let router = iris_api::router(s.clone(), auth.clone());
    let (held, wait) = mpsc::channel();
    let holder = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|_| {
                held.send(()).unwrap();
                std::thread::sleep(Duration::from_secs(6));
                Ok(())
            })
            .unwrap();
        })
    };
    wait.recv().unwrap();
    let pause = || async {
        tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1)))
            .await
            .unwrap()
    };
    // Yesterday, then today: each request counts its own day.
    let first = tokio::spawn(router.clone().oneshot(request_of(
        &auth,
        Method::POST,
        "/api/notices/n-1/done",
        -1,
    )));
    pause().await;
    let second = tokio::spawn(router.clone().oneshot(request_of(
        &auth,
        Method::GET,
        "/api/status",
        0,
    )));
    pause().await;
    // A third request of yesterday: its day is counted by the first one.
    let start = Instant::now();
    let res = router
        .clone()
        .oneshot(request_of(&auth, Method::GET, "/api/status", -1))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let took = start.elapsed();
    assert!(took < Duration::from_secs(3), "the read waited: {took:?}");
    assert!(first.await.unwrap().is_ok());
    assert_eq!(second.await.unwrap().unwrap().status(), StatusCode::OK);
    holder.join().unwrap();
    let days: Vec<String> = s
        .read(|c| {
            Ok(c.prepare("SELECT day FROM use_day ORDER BY day")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert_eq!(days, vec![day(-1), day(0)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request of today counts the day and waits for the writer. A read of
/// the measures of the same day must not pass that count: its answer has
/// today, after the writer is free.
#[tokio::test(flavor = "current_thread")]
async fn a_read_of_the_measures_has_the_day_that_another_request_counts() {
    let dir = std::env::temp_dir().join(format!("iris-api-runtime-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let router = iris_api::router(s.clone(), auth.clone());
    let (held, wait) = mpsc::channel();
    let holder = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|_| {
                held.send(()).unwrap();
                std::thread::sleep(Duration::from_secs(4));
                Ok(())
            })
            .unwrap();
        })
    };
    wait.recv().unwrap();
    let first = tokio::spawn(
        router
            .clone()
            .oneshot(request(&auth, Method::GET, "/api/status")),
    );
    tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1)))
        .await
        .unwrap();
    assert!(
        !first.is_finished(),
        "the count did not wait for the writer"
    );
    let res = router
        .clone()
        .oneshot(request(&auth, Method::GET, "/api/measures"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let m: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(m["days"], serde_json::json!([{"day": day(0), "opens": 0}]));
    assert!(first.await.unwrap().is_ok());
    holder.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
