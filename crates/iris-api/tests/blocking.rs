//! #814, #817: a request holds at most one blocking thread for its handler,
//! and at most `MAX_REQUESTS` requests run at the same time. A handler that
//! waits for a second blocking task can wait for ever when the requests hold
//! each thread of the pool.
//!
//! This file is its own test program, because the request slots of the API
//! are one static for each process.

use std::sync::{mpsc, Arc};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use iris_api::{Auth, BLOCKING_THREADS, MAX_REQUESTS};
use iris_store::Store;
use tower::ServiceExt;

fn request(auth: &Auth, uri: &str) -> Request<Body> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        // A read of the measures counts its own day, so each one waits for
        // the writer before its handler runs.
        .header(iris_api::measures::DAY, &iris_api::rfc3339(now)[..10])
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .body(Body::empty())
        .unwrap()
}

fn post(auth: &Auth, uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .body(Body::empty())
        .unwrap()
}

/// Waits for each future, in order.
async fn futures_join<F: std::future::Future>(fs: Vec<F>) -> Vec<F::Output> {
    let mut out = Vec::new();
    for f in fs {
        out.push(f.await);
    }
    out
}

/// Holds the writer of `s` until the returned sender sends or drops.
fn hold_writer(s: &Arc<Store>) -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (held, wait) = mpsc::channel();
    let (free, until) = mpsc::channel::<()>();
    let s = s.clone();
    let holder = std::thread::spawn(move || {
        s.write(|_| {
            held.send(()).unwrap();
            let _ = until.recv();
            Ok(())
        })
        .unwrap();
    });
    wait.recv().unwrap();
    (free, holder)
}

#[test]
fn requests_never_wait_for_a_thread_of_their_own_pool() {
    let dir = std::env::temp_dir().join(format!("iris-api-blocking-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let router = iris_api::router(s.clone(), auth.clone());
    // The pool of `iris serve` less the command socket.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(BLOCKING_THREADS)
        .build()
        .unwrap();
    // A new store for phase 1b, so that each request there waits for its writer.
    let s3 = Arc::new(Store::open(&dir.join("3")).unwrap());
    let router3 = iris_api::router(s3.clone(), auth.clone());
    rt.block_on(async {
        // Phase 1: the slots are full, so one more request waits for a slot.
        let (free, holder) = hold_writer(&s);
        let held: Vec<_> = (0..MAX_REQUESTS)
            .map(|_| tokio::spawn(router.clone().oneshot(request(&auth, "/api/measures"))))
            .collect();
        tokio::time::sleep(Duration::from_secs(1)).await;
        let status = tokio::spawn(router.clone().oneshot(request(&auth, "/api/status")));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!status.is_finished(), "a request ran past the slot limit");
        drop(free);
        holder.join().unwrap();
        for r in held {
            assert_eq!(r.await.unwrap().unwrap().status(), StatusCode::OK);
        }
        assert_eq!(status.await.unwrap().unwrap().status(), StatusCode::OK);

        // Phase 1b: a client that goes away does not free the slot while
        // its handler still runs.
        let (free, holder) = hold_writer(&s3);
        let gone: Vec<_> = (0..MAX_REQUESTS - 1)
            .map(|_| tokio::spawn(router3.clone().oneshot(request(&auth, "/api/measures"))))
            .collect();
        tokio::time::sleep(Duration::from_secs(1)).await;
        for g in &gone {
            g.abort();
        }
        let status = tokio::spawn(router3.clone().oneshot(request(&auth, "/api/status")));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!status.is_finished(), "a gone client freed its slot");
        drop(free);
        holder.join().unwrap();
        assert_eq!(status.await.unwrap().unwrap().status(), StatusCode::OK);

        // Phase 1c: answers that their clients do not read hold each other
        // slot, and the Stop control still answers.
        let held: Vec<_> = futures_join(
            (0..MAX_REQUESTS - 1)
                .map(|_| router3.clone().oneshot(request(&auth, "/api/status")))
                .collect(),
        )
        .await;
        let stop = tokio::time::timeout(
            Duration::from_secs(10),
            router3.clone().oneshot(post(&auth, "/api/stop")),
        )
        .await
        .expect("Stop waited for a slot")
        .unwrap();
        assert_eq!(stop.status(), StatusCode::OK);
        // Each slot of the other requests is held: one more waits.
        let more = tokio::spawn(router3.clone().oneshot(request(&auth, "/api/status")));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!more.is_finished(), "the Stop slot took another request");
        drop(stop);
        drop(held);
        assert_eq!(more.await.unwrap().unwrap().status(), StatusCode::OK);

        // Phase 2: as many requests as the pool has threads, each one in its
        // handler at the same time. A handler that starts a second blocking
        // task (the measures read the memory of the process) must not wait
        // for a thread that the other requests hold.
        // A new store, so that each request counts the day again.
        let s = Arc::new(Store::open(&dir.join("2")).unwrap());
        let router = iris_api::router(s.clone(), auth.clone());
        let (free, holder) = hold_writer(&s);
        let all: Vec<_> = (0..BLOCKING_THREADS)
            .map(|_| tokio::spawn(router.clone().oneshot(request(&auth, "/api/measures"))))
            .collect();
        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(free);
        holder.join().unwrap();
        let done = tokio::time::timeout(Duration::from_secs(30), async {
            for r in all {
                assert_eq!(r.await.unwrap().unwrap().status(), StatusCode::OK);
            }
        })
        .await;
        assert!(done.is_ok(), "the requests waited for a blocking thread");
    });
    let _ = std::fs::remove_dir_all(&dir);
}
