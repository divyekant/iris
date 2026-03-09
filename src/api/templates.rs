use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use std::sync::Arc;

use crate::models::template::{CreateTemplate, Template, UpdateTemplate};
use crate::AppState;

/// GET /api/templates — list all templates
pub async fn list_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Template>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(Template::list(&conn)))
}

/// POST /api/templates — create a template
pub async fn create_template(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateTemplate>,
) -> Result<(StatusCode, Json<Template>), (StatusCode, Json<serde_json::Value>)> {
    if input.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        ));
    }
    if input.body_text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "body_text is required"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let template = Template::create(&conn, &input);
    Ok((StatusCode::CREATED, Json(template)))
}

/// PUT /api/templates/:id — update a template
pub async fn update_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTemplate>,
) -> Result<Json<Template>, (StatusCode, Json<serde_json::Value>)> {
    if input.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        ));
    }
    if input.body_text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "body_text is required"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    Template::update(&conn, &id, &input).map(Json).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "template not found"})),
        )
    })
}

/// DELETE /api/templates/:id — delete a template
pub async fn delete_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if Template::delete(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
