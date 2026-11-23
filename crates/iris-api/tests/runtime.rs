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

fn request(auth: &Auth, method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap()
}

/// A write route waits for the writer connection, which another thread
/// holds. On the one runtime thread of the test, a read request still gets
/// its answer before the writer is free.
#[tokio::test(flavor = "current_thread")]
async fn a_store_wait_does_not_stop_other_requests() {
    let dir = std::env::temp_dir().join(format!("iris-api-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let router = iris_api::router(s.clone(), auth.clone());

    // Another thread holds the writer for 3 seconds.
    let (held, wait) = mpsc::channel();
    let holder = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|_| {
                held.send(()).unwrap();
                std::thread::sleep(Duration::from_secs(3));
                Ok(())
            })
            .unwrap();
        })
    };
    wait.recv().unwrap();

    let start = Instant::now();
    let write = router
        .clone()
        .oneshot(request(&auth, Method::POST, "/api/notices/n-1/done"));
    let read = async {
        // The write request starts first.
        tokio::task::yield_now().await;
        let res = router
            .clone()
            .oneshot(request(&auth, Method::GET, "/api/status"))
            .await
            .unwrap();
        (res.status(), start.elapsed())
    };
    let (w, (code, took)) = tokio::join!(write, read);
    assert_eq!(code, StatusCode::OK);
    assert!(
        took < Duration::from_secs(2),
        "the read waited for the writer: {took:?}"
    );
    assert!(w.is_ok());
    holder.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
