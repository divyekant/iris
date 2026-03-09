use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::models::filter_rule::{self, Action, Condition, FilterRule};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateFilterRuleRequest {
    pub name: String,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFilterRuleRequest {
    pub name: String,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    pub is_active: bool,
}

pub async fn list_filter_rules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<FilterRule>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rules = filter_rule::list(&conn).map_err(|e| {
        tracing::error!("Failed to list filter rules: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(rules))
}

pub async fn create_filter_rule(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateFilterRuleRequest>,
) -> Result<(StatusCode, Json<FilterRule>), StatusCode> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if body.conditions.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if body.actions.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rule = filter_rule::create(&conn, name, &body.conditions, &body.actions, body.account_id.as_deref())
        .map_err(|e| {
            tracing::error!("Failed to create filter rule: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok((StatusCode::CREATED, Json(rule)))
}

pub async fn update_filter_rule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateFilterRuleRequest>,
) -> Result<Json<FilterRule>, StatusCode> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rule = filter_rule::update(&conn, &id, name, &body.conditions, &body.actions, body.is_active)
        .map_err(|e| {
            tracing::error!("Failed to update filter rule: {e}");
            match e {
                rusqlite::Error::QueryReturnedNoRows => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;
    Ok(Json(rule))
}

pub async fn delete_filter_rule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = filter_rule::delete(&conn, &id).map_err(|e| {
        tracing::error!("Failed to delete filter rule: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
