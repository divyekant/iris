pub mod ai;
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod imap;
pub mod jobs;
pub mod models;
pub mod smtp;
pub mod ws;

use axum::{middleware, routing::{delete, get, patch, post, put}, Router};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db: db::DbPool,
    pub config: config::Config,
    pub ws_hub: ws::hub::WsHub,
    pub providers: ai::provider::ProviderPool,
    pub memories: ai::memories::MemoriesClient,
    pub session_token: String,
}

pub fn build_app(state: Arc<AppState>) -> Router {
    // Agent-facing endpoints (API key auth via Bearer token)
    let agent_routes = Router::new()
        .route("/search", get(api::agent::agent_search))
        .route("/messages/{id}", get(api::agent::agent_get_message))
        .route("/threads/{id}", get(api::agent::agent_get_thread))
        .route("/drafts", post(api::agent::agent_create_draft))
        .route("/send", post(api::agent::agent_send))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            api::agent::agent_auth_middleware,
        ));

    // Public API routes (no session auth)
    let public_api = Router::new()
        .route("/health", get(api::health::health))
        .route("/auth/bootstrap", get(api::session_auth::bootstrap_token))
        .route("/auth/oauth/{provider}", get(auth::oauth::start_oauth));

    // Protected API routes (session auth required)
    let protected_api = Router::new()
        .route("/accounts", get(api::accounts::list_accounts).post(api::accounts::create_account))
        .route("/accounts/{id}", get(api::accounts::get_account).delete(api::accounts::delete_account))
        .route("/accounts/{id}/notifications", get(api::accounts::get_notifications).put(api::accounts::set_notifications))
        .route("/messages", get(api::messages::list_messages))
        .route("/messages/{id}", get(api::messages::get_message))
        .route("/messages/{id}/read", put(api::messages::mark_message_read))
        .route("/messages/{id}/attachments", get(api::attachments::list_attachments))
        .route("/attachments/{id}/download", get(api::attachments::download_attachment))
        .route("/messages/{id}/unsubscribe", post(api::messages::unsubscribe))
        .route("/messages/fix-encoding", post(api::messages::fix_encoding))
        .route("/messages/snooze", post(api::messages::snooze_messages))
        .route("/messages/unsnooze", post(api::messages::unsnooze_messages))
        .route("/messages/snoozed", get(api::messages::list_snoozed))
        .route("/messages/{id}/ai-feedback", put(api::ai_feedback::submit_feedback))
        .route("/messages/batch", patch(api::messages::batch_update_messages))
        .route("/messages/report-spam", post(api::blocked_senders::report_spam))
        .route("/blocked-senders", get(api::blocked_senders::list_blocked_senders).post(api::blocked_senders::block_sender))
        .route("/blocked-senders/{id}", delete(api::blocked_senders::unblock_sender))
        .route("/threads/{id}", get(api::threads::get_thread))
        .route("/threads/{id}/mute", get(api::muted_threads::get_mute_status).put(api::muted_threads::mute_thread).delete(api::muted_threads::unmute_thread))
        .route("/threads/{id}/summarize", post(api::ai_actions::summarize_thread))
        .route("/muted-threads", get(api::muted_threads::list_muted))
        .route("/search", get(api::search::search))
        .route("/saved-searches", get(api::saved_searches::list_saved_searches).post(api::saved_searches::create_saved_search))
        .route("/saved-searches/{id}", delete(api::saved_searches::delete_saved_search))
        .route("/ai/assist", post(api::ai_actions::ai_assist))
        .route("/ai/suggest-subject", post(api::ai_actions::suggest_subject))
        .route("/ai/grammar-check", post(api::ai_actions::grammar_check))
        .route("/ai/feedback-stats", get(api::ai_feedback::feedback_stats))
        .route("/ai/briefing", get(api::briefing::get_briefing))
        .route("/ai/chat", post(api::chat::chat))
        .route("/ai/chat/confirm", post(api::chat::confirm_action))
        .route("/ai/chat/memory", get(api::chat::get_chat_memory))
        .route("/ai/chat/{session_id}", get(api::chat::get_history))
        .route("/ai/queue-status", get(api::queue_status::queue_status))
        .route("/ai/reprocess", post(api::queue_status::reprocess_untagged))
        .route("/ai/inbox-stats", get(api::inbox_stats::get_inbox_stats))
        .route("/config", get(api::config::get_config))
        .route("/config/theme", put(api::config::set_theme))
        .route("/config/view-mode", put(api::config::set_view_mode))
        .route("/config/ai", get(api::ai_config::get_ai_config).put(api::ai_config::set_ai_config))
        .route("/config/ai/test", post(api::ai_config::test_ai_connection))
        .route("/api-keys", get(api::agent::list_keys_handler).post(api::agent::create_key_handler))
        .route("/api-keys/{id}", delete(api::agent::revoke_key_handler))
        .route("/audit-log", get(api::agent::get_audit_log_handler))
        .route("/subscriptions/audit", get(api::subscriptions::subscription_audit))
        .route("/send", post(api::compose::send_message))
        .route("/send/cancel/{id}", post(api::compose::cancel_send))
        .route("/config/undo-send-delay", get(api::compose::get_undo_send_delay).put(api::compose::set_undo_send_delay))
        .route("/send/scheduled", get(api::compose::list_scheduled_sends))
        .route("/send/scheduled/{id}", delete(api::compose::cancel_scheduled))
        .route("/drafts", get(api::compose::list_drafts).post(api::compose::save_draft))
        .route("/drafts/{id}", delete(api::compose::delete_draft))
        .route("/signatures", get(api::signatures::list_signatures).post(api::signatures::create_signature))
        .route("/signatures/{id}", put(api::signatures::update_signature).delete(api::signatures::delete_signature))
        .route("/templates", get(api::templates::list_templates).post(api::templates::create_template))
        .route("/templates/{id}", put(api::templates::update_template).delete(api::templates::delete_template))
        .route("/labels", get(api::labels::list_labels).post(api::labels::create_label))
        .route("/labels/{id}", put(api::labels::update_label).delete(api::labels::delete_label))
        .route("/filter-rules", get(api::filter_rules::list_filter_rules).post(api::filter_rules::create_filter_rule))
        .route("/filter-rules/{id}", put(api::filter_rules::update_filter_rule).delete(api::filter_rules::delete_filter_rule))
        .route("/aliases", get(api::aliases::list_aliases).post(api::aliases::create_alias))
        .route("/aliases/{id}", put(api::aliases::update_alias).delete(api::aliases::delete_alias))
        .nest("/agent", agent_routes)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            api::session_auth::session_auth_middleware,
        ));

    let api_routes = Router::new()
        .merge(public_api)
        .merge(protected_api);

    let spa = ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"));

    Router::new()
        .route("/ws", get(ws::ws_handler))
        .route("/auth/callback", get(auth::oauth::oauth_callback))
        .nest("/api", api_routes)
        .fallback_service(spa)
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list([
                    "http://localhost:3000".parse().unwrap(),
                    "http://localhost:5173".parse().unwrap(),
                    "http://127.0.0.1:3000".parse().unwrap(),
                    "http://127.0.0.1:5173".parse().unwrap(),
                ]))
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}
