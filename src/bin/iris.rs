//! Iris CLI — talks to a running iris-server over HTTP.
//! Does NOT import any server-internal modules; pure HTTP client.

use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ──────────────────────────────────────────────────────────────
// CLI definition
// ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "iris", about = "Iris email client CLI", version)]
struct Cli {
    /// Output raw JSON
    #[arg(long, global = true)]
    json: bool,

    /// Minimal output (IDs / counts only)
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CLI configuration
    Init {
        /// Server URL
        #[arg(long, default_value = "http://localhost:3000")]
        url: String,
        /// API key (iris_xxx…)
        #[arg(long)]
        key: Option<String>,
    },
    /// Show inbox messages
    Inbox {
        /// Show all (not just unread)
        #[arg(long)]
        all: bool,
        /// Number of messages to show
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Account filter
        #[arg(long)]
        account: Option<String>,
    },
    /// Read a thread
    Read {
        thread_id: String,
    },
    /// Search emails
    Search {
        query: String,
        /// Use semantic (vector) search
        #[arg(long)]
        semantic: bool,
        /// Limit results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Send an email
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        body: String,
        /// Reply to a thread ID
        #[arg(long)]
        reply_to: Option<String>,
        /// Account ID to send from
        #[arg(long)]
        account: Option<String>,
    },
    /// Manage drafts
    Draft {
        #[command(subcommand)]
        action: DraftAction,
    },
    /// Chat with AI
    Chat {
        message: String,
        /// Chat session ID (omit for new session)
        #[arg(long)]
        session: Option<String>,
        /// Account context
        #[arg(long)]
        account: Option<String>,
    },
    /// AI operations
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
    /// Get or set CLI configuration values
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Server health and status
    Status,
    /// API key management
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
}

#[derive(Subcommand)]
enum DraftAction {
    /// Create a draft
    Create {
        #[arg(long)]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        body: String,
        /// Account ID
        #[arg(long)]
        account: Option<String>,
    },
    /// List all drafts
    List {
        #[arg(long)]
        account: Option<String>,
    },
    /// Delete a draft
    Delete {
        draft_id: String,
    },
}

#[derive(Subcommand)]
enum AiAction {
    /// Classify a message
    Classify {
        message_id: String,
    },
    /// Summarize a thread
    Summarize {
        thread_id: String,
        /// Account ID
        #[arg(long)]
        account: Option<String>,
    },
    /// Get AI queue status
    Queue,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Get a config value (or show all)
    Get {
        key: Option<String>,
    },
    /// Set a config value
    Set {
        key: String,
        value: String,
    },
}

#[derive(Subcommand)]
enum KeysAction {
    /// List API keys
    List,
    /// Create an API key
    Create {
        /// Human-readable name
        #[arg(long)]
        name: String,
        /// Permission level: read, write, send, admin
        #[arg(long, default_value = "read")]
        permission: String,
    },
    /// Revoke an API key
    Revoke {
        key_id: String,
    },
}

// ──────────────────────────────────────────────────────────────
// Config file (~/.iris/config.toml)
// ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize, Default, Clone)]
struct Config {
    server_url: Option<String>,
    api_key: Option<String>,
}

impl Config {
    fn path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".iris").join("config.toml")
    }

    fn load() -> Self {
        let p = Self::path();
        if !p.exists() {
            return Self::default();
        }
        let raw = fs::read_to_string(&p).unwrap_or_default();
        toml::from_str(&raw).unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let p = Self::path();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&p, raw).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn server_url(&self) -> String {
        self.server_url
            .clone()
            .unwrap_or_else(|| "http://localhost:3000".into())
    }
}

// ──────────────────────────────────────────────────────────────
// HTTP client
// ──────────────────────────────────────────────────────────────

struct Client {
    inner: reqwest::blocking::Client,
    base: String,
    api_key: Option<String>,
    json_mode: bool,
    quiet: bool,
}

impl Client {
    fn new(cfg: &Config, json_mode: bool, quiet: bool) -> Self {
        let inner = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            inner,
            base: cfg.server_url(),
            api_key: cfg.api_key.clone(),
            json_mode,
            quiet,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn get(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let mut req = self.inner.get(self.url(path));
        if let Some(k) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        req
    }

    fn post(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let mut req = self.inner.post(self.url(path));
        if let Some(k) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        req.header("Content-Type", "application/json")
    }

    fn delete(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let mut req = self.inner.delete(self.url(path));
        if let Some(k) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        req
    }

    /// Execute a request, return parsed JSON. Prints raw if --json.
    fn execute(&self, req: reqwest::blocking::RequestBuilder) -> Result<serde_json::Value, String> {
        let resp = req.send().map_err(|e| format!("Request failed: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("Read body failed: {e}"))?;
        if self.json_mode {
            println!("{body}");
        }
        if !status.is_success() {
            return Err(format!("Server returned {status}: {body}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("JSON parse error: {e}\nBody: {body}"))
    }
}

// ──────────────────────────────────────────────────────────────
// Formatting helpers
// ──────────────────────────────────────────────────────────────

fn print_header(label: &str) {
    println!("\n{}", label.bold().underline());
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn str_val<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn bool_val(v: &serde_json::Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

fn print_message_row(msg: &serde_json::Value, idx: usize) {
    let unread = !bool_val(msg, "is_read");
    let sender = truncate(str_val(msg, "from_name").if_empty(str_val(msg, "from_address")), 24);
    let subject = truncate(str_val(msg, "subject"), 52);
    let date = truncate(str_val(msg, "date"), 16);
    let id = str_val(msg, "id");

    let row = format!("{:>3}  {:<24}  {:<52}  {:<16}  {}", idx + 1, sender, subject, date, id);
    if unread {
        println!("{}", row.bold());
    } else {
        println!("{}", row.dimmed());
    }
}

trait StrExtIfEmpty<'a> {
    fn if_empty(self, fallback: &'a str) -> &'a str;
}
impl<'a> StrExtIfEmpty<'a> for &'a str {
    fn if_empty(self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

// ──────────────────────────────────────────────────────────────
// Command handlers
// ──────────────────────────────────────────────────────────────

fn cmd_init(url: String, key: Option<String>) -> Result<(), String> {
    let mut cfg = Config::load();
    cfg.server_url = Some(url.clone());

    let key = match key {
        Some(k) => k,
        None => {
            // Interactive prompt
            print!("API key (iris_…): ");
            io::stdout().flush().ok();
            let stdin = io::stdin();
            let line = stdin.lock().lines().next().unwrap_or(Ok(String::new())).unwrap_or_default();
            if line.trim().is_empty() {
                return Err("API key is required. Pass --key iris_xxx or enter it interactively.".into());
            }
            line.trim().to_string()
        }
    };

    cfg.api_key = Some(key);
    cfg.save()?;

    // Test connection
    let client = Client::new(&cfg, false, false);
    match client.execute(client.get("/api/health")) {
        Ok(health) => {
            println!("{} Config saved to {}", "✓".green().bold(), Config::path().display());
            println!("  Server: {}", url.cyan());
            println!("  Status: {}", str_val(&health, "status").green());
            Ok(())
        }
        Err(e) => {
            println!("{} Config saved, but server check failed: {}", "!".yellow().bold(), e);
            println!("  Config: {}", Config::path().display());
            Ok(()) // Still save config even if server is down
        }
    }
}

fn cmd_inbox(client: &Client, all: bool, limit: usize, account: Option<String>) -> Result<(), String> {
    let mut path = format!("/api/messages?limit={limit}");
    if !all {
        path.push_str("&is_read=false");
    }
    if let Some(acc) = &account {
        path.push_str(&format!("&account_id={acc}"));
    }

    let data = client.execute(client.get(&path))?;
    if client.json_mode {
        return Ok(());
    }

    let empty = vec![];
    let messages = data.as_array()
        .or_else(|| data.get("messages").and_then(|m| m.as_array()))
        .unwrap_or(&empty);

    if client.quiet {
        for msg in messages {
            println!("{}", str_val(msg, "id"));
        }
        return Ok(());
    }

    if messages.is_empty() {
        println!("{}", "Inbox empty.".dimmed());
        return Ok(());
    }

    print_header(&format!("Inbox ({} messages)", messages.len()));
    println!("{}", format!("{:>3}  {:<24}  {:<52}  {:<16}  {}", "#", "FROM", "SUBJECT", "DATE", "ID").dimmed());
    println!("{}", "─".repeat(110).dimmed());
    for (i, msg) in messages.iter().enumerate() {
        print_message_row(msg, i);
    }
    println!();
    Ok(())
}

fn cmd_read(client: &Client, thread_id: &str) -> Result<(), String> {
    let data = client.execute(client.get(&format!("/api/threads/{thread_id}")))?;
    if client.json_mode {
        return Ok(());
    }

    let empty = vec![];
    let messages = data.as_array()
        .or_else(|| data.get("messages").and_then(|m| m.as_array()))
        .unwrap_or(&empty);

    if messages.is_empty() {
        println!("{}", "Thread not found or empty.".yellow());
        return Ok(());
    }

    if client.quiet {
        for msg in messages {
            println!("{}", str_val(msg, "id"));
        }
        return Ok(());
    }

    print_header(&format!("Thread: {thread_id}"));
    for msg in messages {
        let from = str_val(msg, "from_name").if_empty(str_val(msg, "from_address"));
        let date = str_val(msg, "date");
        let subject = str_val(msg, "subject");
        let body = msg.get("body_text")
            .or_else(|| msg.get("snippet"))
            .and_then(|b| b.as_str())
            .unwrap_or("");

        println!("\n{} {} {}", "▸".cyan(), from.bold(), format!("<{date}>").dimmed());
        if !subject.is_empty() {
            println!("  Subject: {subject}");
        }
        println!("  {}", truncate(body, 500).dimmed());
        println!("  {}", format!("id: {}", str_val(msg, "id")).dimmed());
    }
    println!();
    Ok(())
}

fn cmd_search(client: &Client, query: &str, semantic: bool, limit: usize) -> Result<(), String> {
    let encoded = urlencoding_simple(query);
    let path = format!("/api/search?q={encoded}&limit={limit}&semantic={semantic}");
    let data = client.execute(client.get(&path))?;
    if client.json_mode {
        return Ok(());
    }

    let empty = vec![];
    let results = data.as_array()
        .or_else(|| data.get("results").and_then(|r| r.as_array()))
        .unwrap_or(&empty);

    if client.quiet {
        for r in results {
            println!("{}", str_val(r, "id"));
        }
        return Ok(());
    }

    print_header(&format!("Search: \"{}\" ({} results)", query, results.len()));
    for (i, r) in results.iter().enumerate() {
        let from = str_val(r, "from_name").if_empty(str_val(r, "from_address"));
        let subject = str_val(r, "subject");
        let snippet = r.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
        let date = str_val(r, "date");
        println!("\n  {}. {} {} {}", i + 1, from.bold(), format!("— {subject}").cyan(), format!("({date})").dimmed());
        if !snippet.is_empty() {
            println!("     {}", truncate(snippet, 120).dimmed());
        }
        println!("     {}", format!("id: {}", str_val(r, "id")).dimmed());
    }
    if results.is_empty() {
        println!("{}", "No results found.".dimmed());
    }
    println!();
    Ok(())
}

fn cmd_send(
    client: &Client,
    to: &str,
    subject: &str,
    body: &str,
    reply_to: Option<&str>,
    account: Option<&str>,
) -> Result<(), String> {
    let mut payload = serde_json::json!({
        "to": to,
        "subject": subject,
        "body": body,
    });
    if let Some(rt) = reply_to {
        payload["reply_to_thread_id"] = serde_json::Value::String(rt.to_string());
    }
    if let Some(acc) = account {
        payload["account_id"] = serde_json::Value::String(acc.to_string());
    }

    let data = client.execute(client.post("/api/send").json(&payload))?;
    if client.json_mode {
        return Ok(());
    }
    if client.quiet {
        println!("{}", str_val(&data, "message_id").if_empty(str_val(&data, "id")));
        return Ok(());
    }
    println!("{} Email sent.", "✓".green().bold());
    let msg_id = str_val(&data, "message_id").if_empty(str_val(&data, "id"));
    if !msg_id.is_empty() {
        println!("  Message ID: {msg_id}");
    }
    Ok(())
}

fn cmd_draft(client: &Client, action: &DraftAction) -> Result<(), String> {
    match action {
        DraftAction::Create { to, subject, body, account } => {
            let mut payload = serde_json::json!({
                "to": to,
                "subject": subject,
                "body": body,
            });
            if let Some(acc) = account {
                payload["account_id"] = serde_json::Value::String(acc.clone());
            }
            let data = client.execute(client.post("/api/drafts").json(&payload))?;
            if client.json_mode { return Ok(()); }
            println!("{} Draft created.", "✓".green().bold());
            println!("  Draft ID: {}", str_val(&data, "id").cyan());
            Ok(())
        }
        DraftAction::List { account } => {
            let mut path = "/api/drafts".to_string();
            if let Some(acc) = account {
                path.push_str(&format!("?account_id={acc}"));
            }
            let data = client.execute(client.get(&path))?;
            if client.json_mode { return Ok(()); }
            let empty = vec![];
            let drafts = data.as_array()
                .or_else(|| data.get("drafts").and_then(|d| d.as_array()))
                .unwrap_or(&empty);
            if client.quiet {
                for d in drafts { println!("{}", str_val(d, "id")); }
                return Ok(());
            }
            print_header(&format!("Drafts ({} total)", drafts.len()));
            for (i, d) in drafts.iter().enumerate() {
                let to = str_val(d, "to");
                let subject = str_val(d, "subject");
                let id = str_val(d, "id");
                println!("  {}. {} {} {}", i + 1, to.bold(), format!("— {subject}").dimmed(), id.dimmed());
            }
            if drafts.is_empty() { println!("{}", "No drafts.".dimmed()); }
            println!();
            Ok(())
        }
        DraftAction::Delete { draft_id } => {
            client.execute(client.delete(&format!("/api/drafts/{draft_id}")))?;
            if client.json_mode { return Ok(()); }
            println!("{} Draft {} deleted.", "✓".green().bold(), draft_id.cyan());
            Ok(())
        }
    }
}

fn cmd_chat(client: &Client, message: &str, session: Option<&str>, account: Option<&str>) -> Result<(), String> {
    let mut payload = serde_json::json!({ "message": message });
    if let Some(s) = session {
        payload["session_id"] = serde_json::Value::String(s.to_string());
    }
    if let Some(acc) = account {
        payload["account_id"] = serde_json::Value::String(acc.to_string());
    }

    let data = client.execute(client.post("/api/ai/chat").json(&payload))?;
    if client.json_mode { return Ok(()); }

    let reply = data.get("reply")
        .or_else(|| data.get("response"))
        .or_else(|| data.get("message"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    println!("\n{}", reply);

    // Citations
    if let Some(citations) = data.get("citations").and_then(|c| c.as_array()) {
        if !citations.is_empty() && !client.quiet {
            println!("\n{}", "Sources:".dimmed());
            for c in citations {
                let from = str_val(c, "from_name").if_empty(str_val(c, "from_address"));
                let subject = str_val(c, "subject");
                let id = str_val(c, "id");
                println!("  {} {} {}", "·".dimmed(), format!("{from} — {subject}").dimmed(), id.dimmed());
            }
        }
    }

    // Session ID for follow-ups
    if let Some(sid) = data.get("session_id").and_then(|s| s.as_str()) {
        if !client.quiet {
            println!("\n{}", format!("session: {sid}").dimmed());
        } else {
            println!("{sid}");
        }
    }

    println!();
    Ok(())
}

fn cmd_ai(client: &Client, action: &AiAction) -> Result<(), String> {
    match action {
        AiAction::Classify { message_id } => {
            let payload = serde_json::json!({ "message_id": message_id });
            let data = client.execute(client.post("/api/ai/classify").json(&payload))?;
            if client.json_mode { return Ok(()); }
            println!("{} Classified:", "✓".green().bold());
            if let Some(obj) = data.as_object() {
                for (k, v) in obj {
                    println!("  {}: {}", k.cyan(), v);
                }
            }
            Ok(())
        }
        AiAction::Summarize { thread_id, account } => {
            let mut path = format!("/api/threads/{thread_id}/summary");
            if let Some(acc) = account {
                path.push_str(&format!("?account_id={acc}"));
            }
            let data = client.execute(client.get(&path))?;
            if client.json_mode { return Ok(()); }
            let summary = data.get("summary")
                .or_else(|| data.get("text"))
                .and_then(|s| s.as_str())
                .unwrap_or("");
            println!("\n{}", "Summary".bold().underline());
            println!("{summary}");
            println!();
            Ok(())
        }
        AiAction::Queue => {
            let data = client.execute(client.get("/api/ai/queue-status"))?;
            if client.json_mode { return Ok(()); }
            print_header("AI Queue Status");
            if let Some(obj) = data.as_object() {
                for (k, v) in obj {
                    println!("  {}: {}", k.cyan(), v);
                }
            }
            println!();
            Ok(())
        }
    }
}

fn cmd_config_action(action: &ConfigAction) -> Result<(), String> {
    match action {
        ConfigAction::Get { key } => {
            let cfg = Config::load();
            match key {
                None => {
                    println!("server_url = {}", cfg.server_url.as_deref().unwrap_or("(not set)"));
                    println!("api_key    = {}", cfg.api_key.as_deref().map(|k| mask_key(k)).unwrap_or_else(|| "(not set)".into()));
                    println!("\nConfig file: {}", Config::path().display());
                }
                Some(k) => {
                    let val = match k.as_str() {
                        "server_url" => cfg.server_url.unwrap_or_default(),
                        "api_key" => cfg.api_key.unwrap_or_default(),
                        _ => return Err(format!("Unknown config key: {k}")),
                    };
                    println!("{val}");
                }
            }
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let mut cfg = Config::load();
            match key.as_str() {
                "server_url" => cfg.server_url = Some(value.clone()),
                "api_key" => cfg.api_key = Some(value.clone()),
                _ => return Err(format!("Unknown config key: {key}")),
            }
            cfg.save()?;
            println!("{} Set {}={}", "✓".green().bold(), key.cyan(), value);
            Ok(())
        }
    }
}

fn cmd_status(client: &Client) -> Result<(), String> {
    let data = client.execute(client.get("/api/health"))?;
    if client.json_mode { return Ok(()); }

    let status = str_val(&data, "status");
    let status_color = if status == "ok" || status == "healthy" {
        status.green().bold()
    } else {
        status.yellow().bold()
    };

    print_header("Iris Server Status");
    println!("  Status:  {status_color}");
    println!("  Server:  {}", client.base.cyan());

    if let Some(checks) = data.get("checks").and_then(|c| c.as_object()) {
        println!("  Checks:");
        for (name, val) in checks {
            let s = val.as_str().unwrap_or("ok");
            let colored = if s == "ok" || s == "healthy" {
                s.green().to_string()
            } else {
                s.yellow().to_string()
            };
            println!("    {}: {}", name.cyan(), colored);
        }
    }
    if let Some(ver) = data.get("version").and_then(|v| v.as_str()) {
        println!("  Version: {ver}");
    }
    println!();
    Ok(())
}

fn cmd_keys(client: &Client, action: &KeysAction) -> Result<(), String> {
    match action {
        KeysAction::List => {
            let data = client.execute(client.get("/api/api-keys"))?;
            if client.json_mode { return Ok(()); }
            let empty = vec![];
            let keys = data.as_array()
                .or_else(|| data.get("keys").and_then(|k| k.as_array()))
                .unwrap_or(&empty);
            if client.quiet {
                for k in keys { println!("{}", str_val(k, "id")); }
                return Ok(());
            }
            print_header(&format!("API Keys ({} total)", keys.len()));
            println!("{}", format!("  {:<36}  {:<20}  {:<10}  {}", "ID", "NAME", "PERMISSION", "CREATED").dimmed());
            println!("{}", format!("  {}", "─".repeat(90)).dimmed());
            for k in keys {
                println!("  {:<36}  {:<20}  {:<10}  {}",
                    str_val(k, "id"),
                    truncate(str_val(k, "name"), 20),
                    str_val(k, "permission").cyan(),
                    str_val(k, "created_at").dimmed(),
                );
            }
            if keys.is_empty() { println!("  {}", "No API keys.".dimmed()); }
            println!();
            Ok(())
        }
        KeysAction::Create { name, permission } => {
            let payload = serde_json::json!({ "name": name, "permission": permission });
            let data = client.execute(client.post("/api/api-keys").json(&payload))?;
            if client.json_mode { return Ok(()); }
            let key = str_val(&data, "key").if_empty(str_val(&data, "api_key"));
            let id = str_val(&data, "id");
            println!("{} API key created.", "✓".green().bold());
            println!("  ID:         {id}");
            println!("  Name:       {name}");
            println!("  Permission: {}", permission.cyan());
            if !key.is_empty() {
                println!("  Key:        {}", key.yellow().bold());
                println!("  {}", "Save this key — it won't be shown again.".red());
            }
            println!();
            Ok(())
        }
        KeysAction::Revoke { key_id } => {
            client.execute(client.delete(&format!("/api/api-keys/{key_id}")))?;
            if client.json_mode { return Ok(()); }
            println!("{} Key {} revoked.", "✓".green().bold(), key_id.cyan());
            Ok(())
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Utilities
// ──────────────────────────────────────────────────────────────

fn urlencoding_simple(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        ' ' => "+".to_string(),
        c => format!("%{:02X}", c as u32),
    }).collect()
}

fn mask_key(k: &str) -> String {
    if k.len() <= 8 {
        return "*".repeat(k.len());
    }
    format!("{}…{}", &k[..4], &k[k.len() - 4..])
}

// ──────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let cfg = Config::load();

    let result = match &cli.command {
        Commands::Init { url, key } => {
            cmd_init(url.clone(), key.clone())
        }
        Commands::Config { action } => {
            cmd_config_action(action)
        }
        other => {
            let client = Client::new(&cfg, cli.json, cli.quiet);
            match other {
                Commands::Status => cmd_status(&client),
                Commands::Inbox { all, limit, account } => {
                    cmd_inbox(&client, *all, *limit, account.clone())
                }
                Commands::Read { thread_id } => cmd_read(&client, thread_id),
                Commands::Search { query, semantic, limit } => {
                    cmd_search(&client, query, *semantic, *limit)
                }
                Commands::Send { to, subject, body, reply_to, account } => {
                    cmd_send(&client, to, subject, body, reply_to.as_deref(), account.as_deref())
                }
                Commands::Draft { action } => cmd_draft(&client, action),
                Commands::Chat { message, session, account } => {
                    cmd_chat(&client, message, session.as_deref(), account.as_deref())
                }
                Commands::Ai { action } => cmd_ai(&client, action),
                Commands::Keys { action } => cmd_keys(&client, action),
                Commands::Init { .. } | Commands::Config { .. } => unreachable!(),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}
