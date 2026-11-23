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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let day = iris_api::rfc3339(now)[..10].to_string();
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
