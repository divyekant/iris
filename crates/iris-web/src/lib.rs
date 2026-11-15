//! The web client of Iris. `build.rs` embeds the files of the client build
//! (`client/dist`) in the program, so no Node runs at run time.

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

include!(concat!(env!("OUT_DIR"), "/files.rs"));

/// The page when the program was built with no client build.
const FALLBACK: &[u8] = b"<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Iris</title><body><h1>Iris</h1><p>This program has no web client. Build the client first (see client/README.md).</p></body></html>";

/// Return the routes that serve the web client. Only GET and HEAD are allowed.
pub fn router() -> Router {
    Router::new().fallback(get(serve))
}

/// Return the paths of the embedded files.
pub fn paths() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(path, _)| *path)
}

async fn serve(uri: Uri) -> Response {
    let path = match uri.path() {
        "/" => "/index.html",
        path => path,
    };
    let body = match FILES.iter().find(|(p, _)| *p == path) {
        Some((_, body)) => *body,
        None if path == "/index.html" => FALLBACK,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type(path))], body).into_response()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}
