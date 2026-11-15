//! The web client of Iris. The client workstream replaces the placeholder page
//! with the files of the Svelte build (`client/dist`), embedded in the program.

use axum::{response::Html, routing::get, Router};

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Iris</title><body><h1>Iris</h1></body></html>";

/// Return the routes that serve the web client.
pub fn router() -> Router {
    Router::new().route("/", get(|| async { Html(PLACEHOLDER) }))
}
