pub mod ai;
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod imap;
pub mod jobs;
pub mod models;
pub mod secrets;
pub mod smtp;
pub mod utils;
pub mod ws;

use axum::{
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, ORIGIN},
        HeaderName, HeaderValue, Method,
    },
    middleware,
    routing::{delete, get, patch, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
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
        .route("/auth/login", post(api::session_auth::login))
        .route("/auth/logout", post(api::session_auth::logout))
        .route("/auth/oauth/{provider}", get(auth::oauth::start_oauth));

    // Protected API routes (session auth required)
    let protected_api = Router::new()
        .route("/accounts", get(api::accounts::list_accounts).post(api::accounts::create_account))
        .route("/accounts/{id}", get(api::accounts::get_account).delete(api::accounts::delete_account))
        .route("/accounts/{id}/notifications", get(api::accounts::get_notifications).put(api::accounts::set_notifications))
        .route("/messages", get(api::messages::list_messages))
        .route("/messages/needs-reply", get(api::messages::list_needs_reply))
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
        .route("/messages/{id}/redirect", post(api::compose::redirect_message))
        .route("/messages/batch", patch(api::messages::batch_update_messages))
        .route("/messages/report-spam", post(api::blocked_senders::report_spam))
        .route("/blocked-senders", get(api::blocked_senders::list_blocked_senders).post(api::blocked_senders::block_sender))
        .route("/blocked-senders/{id}", delete(api::blocked_senders::unblock_sender))
        .route("/threads/{id}", get(api::threads::get_thread))
        .route("/threads/{id}/mute", get(api::muted_threads::get_mute_status).put(api::muted_threads::mute_thread).delete(api::muted_threads::unmute_thread))
        .route("/threads/{id}/summarize", post(api::ai_actions::summarize_thread))
        .route("/muted-threads", get(api::muted_threads::list_muted))
        .route("/threads/{thread_id}/notes", get(api::thread_notes::list_notes).post(api::thread_notes::create_note))
        .route("/threads/{thread_id}/notes/{id}", put(api::thread_notes::update_note).delete(api::thread_notes::delete_note))
        .route("/search", get(api::search::search))
        .route("/saved-searches", get(api::saved_searches::list_saved_searches).post(api::saved_searches::create_saved_search))
        .route("/saved-searches/{id}", delete(api::saved_searches::delete_saved_search))
        .route("/ai/assist", post(api::ai_actions::ai_assist))
        .route("/ai/suggest-subject", post(api::ai_actions::suggest_subject))
        .route("/ai/grammar-check", post(api::ai_actions::grammar_check))
        .route("/ai/draft-from-intent", post(api::ai_actions::draft_from_intent))
        .route("/ai/extract-tasks", post(api::ai_actions::extract_tasks))
        .route("/ai/multi-reply", post(api::ai_actions::multi_reply))
        .route("/ai/feedback-stats", get(api::ai_feedback::feedback_stats))
        .route("/ai/briefing", get(api::briefing::get_briefing))
        .route("/ai/chat", post(api::chat::chat))
        .route("/ai/chat/confirm", post(api::chat::confirm_action))
        .route("/ai/chat/memory", get(api::chat::get_chat_memory))
        .route("/ai/chat/{session_id}", get(api::chat::get_history))
        .route("/ai/queue-status", get(api::queue_status::queue_status))
        .route("/ai/reprocess", post(api::queue_status::reprocess_untagged))
        .route("/ai/inbox-stats", get(api::inbox_stats::get_inbox_stats))
        .route("/ai/detect-intent", post(api::intent::detect_intent))
        .route("/ai/extract-deadlines", post(api::deadlines::extract_deadlines))
        .route("/ai/autocomplete", post(api::autocomplete::autocomplete))
        .route("/ai/scan-followups", post(api::followups::scan_followups))
        .route("/ai/suggest-cc", post(api::cc_suggestions::suggest_cc))
        .route("/ai/relationship-priority", post(api::relationship_priority::compute_relationship_scores))
        .route("/ai/detect-social-engineering", post(api::social_engineering::detect_social_engineering))
        .route("/ai/translate", post(api::translate::translate))
        .route("/ai/detect-language", post(api::translate::detect_language))
        .route("/ai/translate-email", post(api::translate::translate_email))
        .route("/compose/markdown-preview", post(api::markdown::markdown_preview))
        .route("/compose/scan-dlp", post(api::dlp::scan_dlp))
        .route("/compose/dlp-override", post(api::dlp::dlp_override))
        .route("/config", get(api::config::get_config))
        .route("/config/theme", put(api::config::set_theme))
        .route("/config/view-mode", put(api::config::set_view_mode))
        .route("/config/appearance", get(api::config::get_appearance).put(api::config::set_appearance))
        .route("/config/ai", get(api::ai_config::get_ai_config).put(api::ai_config::set_ai_config))
        .route("/config/ai/test", post(api::ai_config::test_ai_connection))
        .route("/api-keys", get(api::agent::list_keys_handler).post(api::agent::create_key_handler))
        .route("/api-keys/{id}", delete(api::agent::revoke_key_handler))
        .route("/audit-log", get(api::agent::get_audit_log_handler))
        .route("/contacts/top", get(api::contacts::get_top_contacts))
        .route("/contacts/{email}/topics", get(api::contacts::get_contact_topics))
        .route("/contacts/{email}/response-times", get(api::contacts::get_response_times))
        .route("/contacts/vip", get(api::vip::list_vip))
        .route("/contacts/vip/compute", post(api::vip::compute_vip))
        .route("/contacts/{email}/vip", put(api::vip::set_vip))
        .route("/contacts/{email}/vip-score", get(api::vip::get_vip_score))
        .route("/contacts/{email}/relationship", get(api::relationship_priority::get_contact_relationship))
        .route("/messages/prioritized", get(api::relationship_priority::get_prioritized_messages))
        .route("/contacts/relationships/compute", post(api::relationship_scoring::compute_relationships))
        .route("/contacts/relationships/stats", get(api::relationship_scoring::relationship_stats))
        .route("/contacts/relationships/{email}", get(api::relationship_scoring::get_relationship))
        .route("/contacts/relationships", get(api::relationship_scoring::list_relationships))
        .route("/messages/{id}/scan-links", post(api::link_safety::scan_message_links))
        .route("/messages/{id}/intent", get(api::intent::get_intent))
        .route("/messages/{id}/social-engineering", get(api::social_engineering::get_social_engineering))
        .route("/deadlines", get(api::deadlines::list_deadlines))
        .route("/deadlines/{id}/complete", put(api::deadlines::complete_deadline))
        .route("/deadlines/{id}", delete(api::deadlines::delete_deadline))
        .route("/threads/{id}/deadlines", get(api::deadlines::thread_deadlines))
        .route("/followups", get(api::followups::list_followups))
        .route("/followups/{id}/snooze", put(api::followups::snooze_followup))
        .route("/followups/{id}/dismiss", put(api::followups::dismiss_followup))
        .route("/followups/{id}/acted", put(api::followups::mark_acted))
        .route("/subscriptions/audit", get(api::subscriptions::subscription_audit))
        .route("/privacy/report", get(api::privacy_report::privacy_report))
        .route("/privacy/trackers", get(api::privacy_report::list_trackers))
        .route("/contacts/intelligence/summary", get(api::relationship_intel::get_intelligence_summary))
        .route("/contacts/{email}/intelligence", get(api::relationship_intel::get_contact_intelligence))
        .route("/contacts/{email}/intelligence/ai-summary", post(api::relationship_intel::get_contact_ai_summary))
        // Batch 7: Auto-archive patterns
        .route("/ai/archive-patterns/compute", post(api::archive_patterns::compute_patterns))
        .route("/ai/archive-patterns", get(api::archive_patterns::list_patterns))
        .route("/ai/archive-patterns/suggest", post(api::archive_patterns::suggest_archive))
        .route("/ai/archive-patterns/{id}", delete(api::archive_patterns::delete_pattern).put(api::archive_patterns::update_pattern))
        // Batch 7: Newsletter digest
        .route("/ai/newsletter-digest", post(api::newsletter_digest::generate_digest))
        .route("/ai/newsletter-digest/sources", get(api::newsletter_digest::list_sources))
        .route("/ai/newsletter-digest/preview", post(api::newsletter_digest::preview_digest))
        .route("/ai/newsletter-digest/history", get(api::newsletter_digest::digest_history))
        // Batch 7: Template suggestions
        .route("/ai/template-suggestions/scan", post(api::template_suggestions::scan_templates))
        .route("/ai/template-suggestions", get(api::template_suggestions::list_suggestions))
        .route("/ai/template-suggestions/{id}/accept", post(api::template_suggestions::accept_suggestion))
        .route("/ai/template-suggestions/{id}", delete(api::template_suggestions::dismiss_suggestion))
        // Batch 7: Notification routing
        .route("/notifications/routing/config", get(api::notification_routing::get_config).put(api::notification_routing::update_config))
        .route("/notifications/routing/classify", post(api::notification_routing::classify))
        .route("/notifications/digest", get(api::notification_routing::get_digest))
        .route("/notifications/digest/clear", post(api::notification_routing::clear_digest))
        // Batch 7: Follow-up tracking
        .route("/followup-tracking", get(api::followup_tracking::list_followups).post(api::followup_tracking::create_followup))
        .route("/followup-tracking/due", get(api::followup_tracking::due_followups))
        .route("/followup-tracking/check-replies", post(api::followup_tracking::check_replies))
        .route("/followup-tracking/{id}", put(api::followup_tracking::update_followup).delete(api::followup_tracking::delete_followup))
        // Batch 7: Effectiveness scoring
        .route("/compose/effectiveness-score", post(api::effectiveness::score_effectiveness))
        .route("/compose/effectiveness-history", get(api::effectiveness::effectiveness_history))
        .route("/compose/effectiveness-tips", post(api::effectiveness::effectiveness_tips))
        // Batch 8: Webhooks (#82)
        .route("/webhooks", get(api::webhooks::list_webhooks).post(api::webhooks::create_webhook))
        .route("/webhooks/{id}", get(api::webhooks::get_webhook_handler).put(api::webhooks::update_webhook).delete(api::webhooks::delete_webhook))
        .route("/webhooks/{id}/deliveries", get(api::webhooks::list_webhook_deliveries))
        .route("/webhooks/{id}/test", post(api::webhooks::test_webhook))
        // Batch 8: Structured data extraction (#83)
        .route("/extract/{message_id}", post(api::extracted_data::extract_from_message))
        .route("/extracted-data", get(api::extracted_data::list_extracted_data))
        .route("/extracted-data/summary", get(api::extracted_data::extracted_data_summary))
        .route("/extracted-data/{id}", delete(api::extracted_data::delete_extracted_datum))
        // Batch 8: Health reports (#85)
        .route("/health-reports/generate", post(api::health_reports::generate_report))
        .route("/health-reports", get(api::health_reports::list_reports))
        .route("/health-reports/{id}", get(api::health_reports::get_report).delete(api::health_reports::delete_report))
        // Batch 8: Newsletter feeds (#39)
        .route("/newsletter-feeds", get(api::newsletter_feeds::list_feeds))
        .route("/newsletter-feeds/discover", post(api::newsletter_feeds::discover_feeds))
        .route("/newsletter-feeds/{id}", put(api::newsletter_feeds::update_feed).delete(api::newsletter_feeds::delete_feed))
        .route("/newsletter-feeds/{id}/articles", get(api::newsletter_feeds::list_articles))
        .route("/newsletter-feeds/{id}/mark-read", post(api::newsletter_feeds::mark_feed_read))
        // Batch 8: Subscription management (#40)
        .route("/subscriptions", get(api::subscription_management::list_subscriptions))
        .route("/subscriptions/scan", post(api::subscription_management::scan_subscriptions))
        .route("/subscriptions/bulk-action", post(api::subscription_management::bulk_action))
        .route("/subscriptions/stats", get(api::subscription_management::subscription_stats))
        .route("/subscriptions/{id}", get(api::subscription_management::get_subscription))
        .route("/subscriptions/{id}/status", put(api::subscription_management::update_status))
        // Batch 8: Analytics dashboard (#84)
        .route("/analytics/overview", get(api::analytics::overview))
        .route("/analytics/volume", get(api::analytics::volume))
        .route("/analytics/categories", get(api::analytics::categories))
        .route("/analytics/top-contacts", get(api::analytics::top_contacts))
        .route("/analytics/hourly-distribution", get(api::analytics::hourly_distribution))
        .route("/analytics/response-times", get(api::analytics::response_times))
        .route("/analytics/snapshot", post(api::analytics::save_snapshot))
        // Batch 9: Attachment content search (#41)
        .route("/attachments/index/{message_id}", post(api::attachment_search::index_message_attachments))
        .route("/attachments/search", get(api::attachment_search::search_attachments))
        .route("/attachments/search/stats", get(api::attachment_search::search_stats))
        .route("/attachments/reindex", post(api::attachment_search::reindex_attachments))
        // Batch 9: Thread clustering (#50)
        .route("/thread-clusters/compute", post(api::thread_clusters::compute_clusters))
        .route("/thread-clusters", get(api::thread_clusters::list_clusters))
        .route("/thread-clusters/{id}", get(api::thread_clusters::get_cluster).delete(api::thread_clusters::delete_cluster))
        .route("/thread-clusters/{id}/merge", post(api::thread_clusters::merge_clusters))
        .route("/thread-clusters/{cluster_id}/members/{thread_id}", delete(api::thread_clusters::remove_member))
        // Batch 9: Phishing detection (#71)
        .route("/security/phishing-scan/{message_id}", post(api::phishing_detection::scan_message))
        .route("/security/phishing-report/{message_id}", get(api::phishing_detection::get_report))
        .route("/security/phishing-reports", get(api::phishing_detection::list_reports))
        .route("/security/phishing-bulk-scan", post(api::phishing_detection::bulk_scan))
        .route("/security/phishing-stats", get(api::phishing_detection::phishing_stats))
        // Batch 9: Contact profiles (#77)
        .route("/contacts/profiles", get(api::contact_profiles::list_profiles))
        .route("/contacts/profiles/generate-all", post(api::contact_profiles::generate_all_profiles))
        .route("/contacts/profiles/search", get(api::contact_profiles::search_profiles))
        .route("/contacts/profiles/generate/{email}", post(api::contact_profiles::generate_profile))
        .route("/contacts/profiles/{email}", get(api::contact_profiles::get_profile).delete(api::contact_profiles::delete_profile))
        // Batch 9: MCP Server (#81)
        .route("/mcp/initialize", post(api::mcp_server::initialize))
        .route("/mcp/tools/call", post(api::mcp_server::tool_call))
        .route("/mcp/tools/list", get(api::mcp_server::list_tools))
        .route("/mcp/sessions", get(api::mcp_server::list_sessions))
        .route("/mcp/sessions/{session_id}", delete(api::mcp_server::delete_session))
        .route("/mcp/sessions/{session_id}/history", get(api::mcp_server::session_history))
        // Delegation agent
        .route("/delegation/playbooks", get(api::delegation::list_playbooks).post(api::delegation::create_playbook))
        .route("/delegation/playbooks/{id}", put(api::delegation::update_playbook).delete(api::delegation::delete_playbook))
        .route("/delegation/process/{message_id}", post(api::delegation::process_message))
        .route("/delegation/actions", get(api::delegation::list_actions))
        .route("/delegation/actions/{id}/undo", post(api::delegation::undo_action))
        .route("/delegation/summary", get(api::delegation::get_summary))
        // Custom categories
        .route("/categories/custom", get(api::custom_categories::list_custom_categories).post(api::custom_categories::create_custom_category))
        .route("/categories/custom/{id}", put(api::custom_categories::update_custom_category).delete(api::custom_categories::delete_custom_category))
        .route("/categories/analyze/{account_id}", post(api::custom_categories::analyze_categories))
        .route("/categories/custom/{id}/accept", post(api::custom_categories::accept_category))
        .route("/categories/custom/{id}/dismiss", post(api::custom_categories::dismiss_category))
        .route("/categories/explain/{message_id}", get(api::custom_categories::explain_category))
        // Writing style learning
        .route("/style/{account_id}", get(api::writing_style::get_style))
        .route("/style/{account_id}/analyze", post(api::writing_style::analyze_style))
        // Auto-draft
        .route("/auto-draft/{message_id}", get(api::auto_draft::check_auto_draft))
        .route("/auto-draft/generate/{message_id}", post(api::auto_draft::generate_auto_draft))
        .route("/auto-draft/{draft_id}/feedback", post(api::auto_draft::auto_draft_feedback))
        .route("/config/auto-draft", get(api::auto_draft::get_auto_draft_config).put(api::auto_draft::set_auto_draft_config))
        // Knowledge graph
        .route("/graph", get(api::knowledge_graph::query_graph))
        .route("/graph/entities", get(api::knowledge_graph::list_entities))
        .route("/graph/extract/{message_id}", post(api::knowledge_graph::extract_entities))
        // Temporal reasoning
        .route("/search/temporal", post(api::temporal::temporal_search))
        .route("/timeline", get(api::temporal::list_events))
        .route("/send", post(api::compose::send_message))
        .route("/send/cancel/{id}", post(api::compose::cancel_send))
        .route("/config/undo-send-delay", get(api::compose::get_undo_send_delay).put(api::compose::set_undo_send_delay))
        .route("/send/scheduled", get(api::compose::list_scheduled_sends))
        .route("/send/scheduled/{id}", delete(api::compose::cancel_scheduled))
        .route("/drafts", get(api::compose::list_drafts).post(api::compose::save_draft))
        .route("/drafts/{id}", delete(api::compose::delete_draft))
        .route("/drafts/{draft_id}/versions", post(api::draft_versions::save_version).get(api::draft_versions::list_versions))
        .route("/drafts/{draft_id}/versions/diff", get(api::draft_versions::diff_versions))
        .route("/drafts/{draft_id}/versions/{version_number}", get(api::draft_versions::get_version))
        .route("/drafts/{draft_id}/versions/{version_number}/restore", post(api::draft_versions::restore_version))
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

    // Rate-limited protected API: 100 requests/min per session token
    let rate_limited_api = protected_api.layer(api::rate_limit::rate_limit_layer());

    let api_routes = Router::new()
        .merge(public_api)
        .merge(rate_limited_api);

    let spa = ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"));

    // Configurable CORS origins via IRIS_CORS_ORIGINS env var (comma-separated).
    // Falls back to localhost dev origins if not set.
    let cors_origins = build_cors_origins();

    Router::new()
        .route("/ws", get(ws::ws_handler))
        .route("/auth/callback", get(auth::oauth::oauth_callback))
        .nest("/api", api_routes)
        .fallback_service(spa)
        .layer(
            CorsLayer::new()
                .allow_origin(cors_origins)
                .allow_credentials(true)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([
                    ACCEPT,
                    AUTHORIZATION,
                    CONTENT_TYPE,
                    ORIGIN,
                    HeaderName::from_static("sec-fetch-site"),
                    HeaderName::from_static("x-session-token"),
                ]),
        )
        .with_state(state)
}

/// Build CORS allowed origins from the IRIS_CORS_ORIGINS env var.
/// Accepts a comma-separated list of origins (e.g. "http://localhost:3000,https://app.example.com").
/// Falls back to default dev origins if the env var is unset or empty.
fn build_cors_origins() -> AllowOrigin {
    let default_origins = "http://localhost:1420,http://localhost:5173";

    let raw = std::env::var("IRIS_CORS_ORIGINS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_origins.to_string());

    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            s.parse::<HeaderValue>().ok().or_else(|| {
                tracing::warn!("Invalid CORS origin, skipping: {}", s);
                None
            })
        })
        .collect();

    if origins.is_empty() {
        tracing::warn!("No valid CORS origins configured, falling back to defaults");
        // Re-parse defaults through the same pipeline (no separate unwrap path)
        let fallback: Vec<HeaderValue> = default_origins
            .split(',')
            .filter_map(|s| s.trim().parse::<HeaderValue>().ok())
            .collect();
        AllowOrigin::list(fallback)
    } else {
        tracing::info!("CORS origins: {:?}", origins);
        AllowOrigin::list(origins)
    }
}
