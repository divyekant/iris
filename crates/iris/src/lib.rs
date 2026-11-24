//! The Iris engine and the `iris` command.

/// Write one line to stderr. A failed write (for example a closed pipe under
/// a service manager) is ignored: `eprintln!` panics, and that stops the engine.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

pub mod cost;
pub mod engine;
pub mod notify;
pub mod nudge;
pub mod reply;
pub mod session_socket;
pub mod sessions;
pub mod socket;
pub mod verbs;

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use iris_store::Store;

/// The default address of `iris serve`. Only this machine can connect to it.
pub const DEFAULT_ADDR: &str = "127.0.0.1:4800";

/// The longest wait for the browser to come back after a sign-in.
pub const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

#[derive(Parser)]
#[command(name = "iris", version = VERSION, about = "Iris: local, agent-first email.")]
pub struct Cli {
    /// The data folder. The default is the variable IRIS_DATA, else the
    /// folder of the system (specification 1, section 5.1).
    #[arg(long, global = true)]
    pub data: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the engine: sync each account and serve the web client.
    Serve {
        /// The IP address and port to listen on, for example 127.0.0.1:4800.
        #[arg(long, default_value = DEFAULT_ADDR)]
        addr: SocketAddr,
    },
    /// Add or sign in again to a Gmail account.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Sync or verify the mirror now, outside the schedule of `iris serve`.
    Mirror {
        #[command(subcommand)]
        command: MirrorCommand,
    },
    /// Show the accounts, the sync and the message counts.
    Status {
        /// Print the JSON of the local API (`/api/status`).
        #[arg(long)]
        json: bool,
    },
    /// Search the mail (specification 1, section 5.6), newest first.
    Search {
        /// The search text, for example "contract from:dana after:2026-10-01".
        text: String,
        /// The most messages to show, 1 to 200.
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Print the JSON of the local API (`/api/search`).
        #[arg(long)]
        json: bool,
    },
    /// Show one message (`m-42`) or one item (`c-7`).
    Show {
        id: String,
        /// Print the JSON of the local API (`/api/messages/{id}`). An item is always JSON.
        #[arg(long)]
        json: bool,
    },
    /// In a job workspace: list the items of the job, of one form if given.
    /// A verb of a session only.
    List {
        /// commitment, conversation, record, feed or signal.
        form: Option<String>,
        /// Print one JSON line for each item.
        #[arg(long)]
        json: bool,
    },
    /// In a job workspace: end the session, and wait for a reply on a thread
    /// of the job or for a date. Iris starts a new session of the job when
    /// the reply comes or the date passes. A verb of a session only.
    Wait {
        /// The end of the wait: a day YYYY-MM-DD (its start, UTC), or an
        /// RFC 3339 time with its offset in whole seconds, for example
        /// 2026-11-18T09:30:00-08:00.
        #[arg(long)]
        until: String,
        /// The thread (`t-8841`) whose reply ends the wait first.
        #[arg(long)]
        reply: Option<String>,
        /// The reason of the wait, for the person.
        #[arg(long)]
        why: Option<String>,
    },
    /// In a job workspace: end the session, and ask the person one
    /// question. Iris shows it in the brief, and starts a new session of
    /// the job with the answer in `JOB.md`. A verb of a session only.
    Ask {
        /// The question, at most 2000 characters.
        question: String,
    },
    /// In a job workspace: the job is complete. The session ends, and Iris
    /// removes the mail files of the workspace. A verb of a session only.
    Done {
        /// What the agent did, for the person. At most 2000 characters.
        #[arg(long)]
        why: Option<String>,
    },
    /// Copy the data folder to a new folder while the engine runs.
    Backup {
        /// The backup folder. It must not exist, or must be empty.
        path: PathBuf,
    },
    /// Write each item as JSON Lines, and copy each raw message. The export
    /// holds no secret.
    Export {
        /// The export folder. It must not exist, or must be empty.
        path: PathBuf,
    },
    /// Install or remove the background service that runs `iris serve`.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Set or show the model of understanding, its price and its budget.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Show the send switch, or turn it on or off. No send leaves while it
    /// is off. `on` needs a terminal and a typed "yes".
    Sending {
        #[arg(value_enum)]
        state: Option<Switch>,
    },
    /// Stop all agent work: end each session of a job, and apply and send
    /// nothing until `iris resume`. `--show` only shows the state.
    Stop {
        #[arg(long)]
        show: bool,
    },
    /// End the stop. Actions apply again, and each held send waits a full
    /// hold before it leaves.
    Resume,
    /// Cancel the send of a draft (`d-4`) while it waits in the outbox.
    Cancel { id: String },
    /// Close a commitment (`c-7`) in a draft changeset. Needs the engine.
    Close {
        id: String,
        /// The reason, at most 500 characters. The default is "Done.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Decline a commitment in a draft changeset. Needs the engine.
    Decline {
        id: String,
        /// The reason, at most 500 characters. The default is "Declined.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Skip a feed item (`f-3`) in a draft changeset. Needs the engine.
    Skip {
        id: String,
        /// The reason, at most 500 characters. The default is "Skipped.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Mute a conversation item in a draft changeset. Needs the engine.
    Mute {
        id: String,
        /// The reason, at most 500 characters. The default is "Muted.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Dismiss a signal item in a draft changeset. Needs the engine.
    Dismiss {
        id: String,
        /// The reason, at most 500 characters. The default is "Dismissed.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Defer a commitment to a date in a draft changeset. Needs the engine.
    Defer {
        id: String,
        /// The day it opens again, YYYY-MM-DD.
        #[arg(long)]
        date: String,
        #[command(flatten)]
        to: Draft,
    },
    /// Correct one field of an item in a draft changeset. Needs the engine.
    Correct {
        id: String,
        /// The field, a dotted path, for example due.date.
        #[arg(long)]
        field: String,
        /// The new value, as JSON. A value that is not JSON is a text.
        #[arg(long)]
        value: String,
        #[command(flatten)]
        to: Draft,
    },
    /// Archive threads or messages in a draft changeset. Needs the engine.
    Archive(Targets),
    /// Move threads or messages to the inbox in a draft changeset.
    Inbox(Targets),
    /// Mark threads or messages read in a draft changeset.
    Read(Targets),
    /// Mark threads or messages unread in a draft changeset.
    Unread(Targets),
    /// Move threads or messages to the trash in a draft changeset.
    Trash(Targets),
    /// Add a label in a draft changeset.
    Label(LabelTargets),
    /// Remove a label in a draft changeset.
    Unlabel(LabelTargets),
    /// Write a reply to a thread (`t-3`) or a new mail from an account
    /// (`a-1`) in a draft changeset. The file is a JSON object with `to`,
    /// `cc`, `subject` (not for a reply) and `body`. Needs the engine.
    #[command(name = "draft")]
    Compose {
        target: String,
        file: PathBuf,
        #[command(flatten)]
        to: Draft,
    },
    /// Send a draft that Iris made (`d-4`), in a draft changeset. Only a
    /// person approves a send, and it waits for the hold. Needs the engine.
    Send {
        id: String,
        #[command(flatten)]
        to: Draft,
    },
    /// Submit a draft changeset. Prints the state of each action. Needs the engine.
    Submit {
        /// The changeset ID, for example cs-12.
        id: String,
        /// The reason of the changeset, for the person.
        #[arg(long)]
        why: String,
        /// Print the changeset document.
        #[arg(long)]
        json: bool,
    },
    /// Serve the verbs as MCP (Model Context Protocol) tools on standard
    /// input and output, for an MCP client. The same verbs, author and
    /// policy as the command.
    Mcp,
    /// Open the web client of the running engine in the browser, with a new
    /// one-time sign-in address. Needs the engine.
    Open {
        /// Print the address and open nothing.
        #[arg(long)]
        print: bool,
    },
    /// Show a changeset (`cs-12`), or list your drafts. Needs the engine.
    #[command(arg_required_else_help = true)]
    Changeset {
        /// The changeset ID, for example cs-12.
        #[arg(required_unless_present = "drafts", conflicts_with = "drafts")]
        id: Option<String>,
        /// List the drafts of the author `local`, the newest first.
        #[arg(long)]
        drafts: bool,
        /// Print the changeset document (specification 2, section 5.1).
        #[arg(long)]
        json: bool,
    },
    /// Withdraw a changeset that this command made. Applied actions stay.
    /// Needs the engine.
    Withdraw {
        /// The changeset ID, for example cs-12.
        id: String,
        /// Print the changeset document after the withdraw.
        #[arg(long)]
        json: bool,
    },
    /// Start a new draft revision of a submitted changeset of yours, after
    /// a review comment. Add the new actions with `--changeset`, then
    /// submit. Needs the engine.
    Revise {
        /// The changeset ID, for example cs-12.
        id: String,
        /// Print the changeset document.
        #[arg(long)]
        json: bool,
    },
    /// Understand the mail: an estimate, the approval of a budget, or a run.
    Understand {
        /// Print the estimate and an approval code. No model call.
        #[arg(long, conflicts_with_all = ["approve", "max_cost", "max_messages"])]
        estimate: bool,
        /// Approve a budget with the code of `--estimate`. Needs a terminal.
        #[arg(long, requires = "budget", conflicts_with_all = ["max_cost", "max_messages"])]
        approve: Option<String>,
        /// The new budget in USD. It replaces the budget that is left.
        #[arg(long, requires = "approve")]
        budget: Option<String>,
        /// Stop the run when its cost reaches this many USD.
        #[arg(long)]
        max_cost: Option<String>,
        /// Stop the run after this many messages.
        #[arg(long)]
        max_messages: Option<u64>,
    },
    /// Write a draft of a reply in your voice, from your sent mail. It saves
    /// and prints the draft. It sends nothing.
    Write {
        /// The thread (`t-42`), or a message of the thread (`m-42`).
        id: String,
        /// What the reply must say, for example "yes, Friday at 10".
        #[arg(long)]
        intent: Option<String>,
    },
    /// Watch for a mail: one notice when a new mail matches a search.
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
    /// Run, stop or show the session of an agent on a job (`j-3`). Only
    /// the engine starts and stops sessions.
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    /// The matters that Iris proposes and that you confirmed. Prints the
    /// JSON of the local API (`/api/matters`).
    Matters {
        #[command(subcommand)]
        command: Option<MattersCommand>,
    },
}

#[derive(Subcommand)]
pub enum JobCommand {
    /// Start a session of an agent on the job. The command gives the
    /// session ID when the engine registered it; `iris job session` shows
    /// what happened next.
    Run {
        id: String,
        /// The agent: `claude-code`, `codex`, or an agent of `agents.toml`.
        #[arg(long)]
        agent: String,
    },
    /// Stop the session of the job, and wait up to 15 seconds until each
    /// of its processes stopped.
    Stop { id: String },
    /// Show the last session of the job and its state.
    Session { id: String },
    /// Answer the question of the job. Iris starts a new session of the
    /// job with the answer in `JOB.md`.
    Answer {
        id: String,
        /// The answer, at most 10000 characters.
        text: String,
    },
    /// End the job as complete, or with `--cancel` as cancelled. A session
    /// of the job ends, and Iris removes the mail files of its workspace.
    Finish {
        id: String,
        #[arg(long)]
        cancel: bool,
    },
}

#[derive(Subcommand)]
pub enum MattersCommand {
    /// The valid proposals and the confirmed matters (the default).
    List,
    /// One matter (`g-4`) with its members, by form.
    Show { id: String },
    /// Ask the model for new proposals now: at most one call, after the
    /// wait of a model error, and only for a new set of candidates.
    Propose,
}

#[derive(Subcommand)]
pub enum WatchCommand {
    /// Watch for a new mail that matches the search text, for example
    /// "visa from:embassy". `iris serve` checks each minute.
    Add { text: String },
    /// Show each watch, newest first, with the mail that arrived.
    List {
        /// Print the JSON of the local API (`/api/watches`), one line each.
        #[arg(long)]
        json: bool,
    },
    /// Remove a watch (`w-3`): one that waits, or one that you saw.
    Remove { id: String },
}

/// The draft changeset of a verb that adds an action.
#[derive(clap::Args, Clone)]
pub struct Draft {
    /// Add to this draft changeset. Without it, the command makes a new one.
    #[arg(long)]
    pub changeset: Option<String>,
    /// The reason of the action.
    #[arg(long)]
    pub why: Option<String>,
}

#[derive(clap::Args, Clone)]
pub struct Targets {
    /// Thread (`t-3`) or message (`m-42`) IDs.
    #[arg(required = true)]
    pub ids: Vec<String>,
    #[command(flatten)]
    pub to: Draft,
}

#[derive(clap::Args, Clone)]
pub struct LabelTargets {
    #[arg(required = true)]
    pub ids: Vec<String>,
    /// The label name.
    #[arg(long)]
    pub label: String,
    #[command(flatten)]
    pub to: Draft,
}

/// The action of a verb that adds one: the verb, the targets, the
/// arguments and the draft.
type Action<'a> = (&'static str, Vec<String>, serde_json::Value, &'a Draft);

fn action(c: &Command) -> Result<Option<Action<'_>>, socket::Failure> {
    use serde_json::json;
    let reason = |r: &Option<String>, default: &str| json!({ "reason": r.clone().unwrap_or_else(|| default.into()) });
    Ok(Some(match c {
        Command::Compose { target, file, to } => {
            let usage = |error: String| socket::Failure {
                code: socket::code::USAGE,
                error,
            };
            let text = std::fs::read_to_string(file)
                .map_err(|e| usage(format!("{}: {e}", file.display())))?;
            let args: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                usage(format!(
                    "{}: not a JSON object of the draft: {e}",
                    file.display()
                ))
            })?;
            ("draft", vec![target.clone()], args, to)
        }
        Command::Send { id, to } => ("send", vec![id.clone()], json!({}), to),
        Command::Close { id, reason: r, to } => ("close", vec![id.clone()], reason(r, "Done."), to),
        Command::Decline { id, reason: r, to } => {
            ("decline", vec![id.clone()], reason(r, "Declined."), to)
        }
        Command::Skip { id, reason: r, to } => {
            ("skip", vec![id.clone()], reason(r, "Skipped."), to)
        }
        Command::Mute { id, reason: r, to } => ("mute", vec![id.clone()], reason(r, "Muted."), to),
        Command::Dismiss { id, reason: r, to } => {
            ("dismiss", vec![id.clone()], reason(r, "Dismissed."), to)
        }
        Command::Defer { id, date, to } => ("defer", vec![id.clone()], json!({ "date": date }), to),
        Command::Correct {
            id,
            field,
            value,
            to,
        } => {
            let value = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
            (
                "correct",
                vec![id.clone()],
                json!({ "field": field, "value": value }),
                to,
            )
        }
        Command::Archive(t) => ("archive", t.ids.clone(), json!({}), &t.to),
        Command::Inbox(t) => ("inbox", t.ids.clone(), json!({}), &t.to),
        Command::Read(t) => ("read", t.ids.clone(), json!({}), &t.to),
        Command::Unread(t) => ("unread", t.ids.clone(), json!({}), &t.to),
        Command::Trash(t) => ("trash", t.ids.clone(), json!({}), &t.to),
        Command::Label(t) => ("label", t.ids.clone(), json!({ "label": t.label }), &t.to),
        Command::Unlabel(t) => ("unlabel", t.ids.clone(), json!({ "label": t.label }), &t.to),
        _ => return Ok(None),
    }))
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Switch {
    On,
    Off,
}

#[derive(Subcommand)]
pub enum ModelCommand {
    /// Set the model. This clears the budget: approve a new one.
    Set {
        #[command(subcommand)]
        provider: SetModel,
    },
    /// Show the model, the price and the budget left. Never the key.
    Show,
}

#[derive(Subcommand)]
pub enum SetModel {
    /// A model of Anthropic. The API key comes from standard input.
    Anthropic {
        model: String,
        /// USD for one million input tokens, 0 to 1000.
        #[arg(long)]
        price_in: String,
        /// USD for one million output tokens, 0 to 1000.
        #[arg(long)]
        price_out: String,
        #[arg(long, default_value_t = cost::DEFAULT_MAX_TOKENS)]
        max_tokens: u32,
        /// The time limit of one call, in seconds.
        #[arg(long, default_value_t = cost::DEFAULT_TIMEOUT_S)]
        timeout: u32,
    },
    /// A local OpenAI-compatible server on this machine. No key, no price.
    Local {
        /// For example http://127.0.0.1:8080/v1. Only localhost, 127.0.0.0/8 or [::1].
        base_url: String,
        model: String,
        #[arg(long, default_value_t = cost::DEFAULT_MAX_TOKENS)]
        max_tokens: u32,
        /// The time limit of one call, in seconds.
        #[arg(long, default_value_t = cost::DEFAULT_TIMEOUT_S)]
        timeout: u32,
    },
}

#[derive(Subcommand)]
pub enum MirrorCommand {
    /// Sync each account one time (specification 1, section 4.3).
    Sync,
    /// Compare the mirror of each account with Gmail, and check the local
    /// content (specification 1, section 4.8). A difference makes the next
    /// sync run a full resync.
    Verify,
}

#[derive(Subcommand)]
pub enum AccountCommand {
    /// Sign in with the JSON file of your own OAuth client ("Desktop app").
    Add {
        /// The client file from the Google Cloud console.
        client: PathBuf,
        /// Paste the address of the browser after the sign-in, in place of
        /// the wait for the redirect. Use it when the browser is on another
        /// machine than the engine.
        #[arg(long)]
        paste: bool,
        /// Ask Google for write access (`gmail.modify`), so that Iris can
        /// apply the changes and sends that you approve. Without it, Iris
        /// reads only.
        #[arg(long)]
        write: bool,
    },
    /// Sign in to an Outlook mailbox (Microsoft Graph) with your own app
    /// registration in Microsoft Entra (docs/decisions/2026-11-microsoft-sign-in.md).
    AddOutlook {
        /// The application (client) ID of the app registration.
        client_id: String,
        /// `common`, or the directory (tenant) ID of a single-tenant
        /// registration.
        #[arg(long, default_value = "common")]
        tenant: String,
        /// Paste the address of the browser after the sign-in, in place of
        /// the wait for the redirect.
        #[arg(long)]
        paste: bool,
    },
    /// Remove an account and its mail from Iris. The mailbox at the
    /// provider does not change. Backups keep the mail.
    Remove {
        /// The mail address of the account.
        address: String,
        /// Do not ask for the address again.
        #[arg(long)]
        yes: bool,
    },
    /// Add an IMAP mailbox, or give a new password to one. The command reads
    /// the password (an app password of the provider) from the terminal
    /// with no echo, or one line of standard input, and signs in once to
    /// check it.
    AddImap {
        /// The mail address of the mailbox.
        address: String,
        /// The IMAP server, for example imap.fastmail.com. TLS only. Leave
        /// it out to give a new password to an IMAP account that exists:
        /// the command then uses its saved server, port and user.
        #[arg(long)]
        host: Option<String>,
        /// The IMAP port. The default is 993.
        #[arg(long, requires = "host")]
        port: Option<u16>,
        /// The IMAP user name. The default is the address.
        #[arg(long, requires = "host")]
        user: Option<String>,
    },
}

#[derive(Clone, Copy, Subcommand)]
pub enum ServiceAction {
    /// Install the service for this program and start it now.
    Install,
    /// Stop the service and remove its file.
    Remove,
}

/// Run `iris service install` or `iris service remove`. Return the message
/// for the user.
pub fn service(action: ServiceAction) -> std::io::Result<String> {
    match action {
        ServiceAction::Install => {
            // The service file holds this path, so give the real file, not a link.
            let program = std::env::current_exe()?.canonicalize()?;
            let file = iris_service::install(&program)?;
            Ok(format!(
                "iris: installed and started the service ({})",
                file.display()
            ))
        }
        ServiceAction::Remove => Ok(match iris_service::remove()? {
            Some(file) => format!("iris: stopped the service and removed {}", file.display()),
            None => "iris: no service is installed".to_string(),
        }),
    }
}

/// The version of the program: the release tag that `deploy.yml` sets in
/// `IRIS_RELEASE` at build time, or the crate version for other builds.
pub const VERSION: &str = match option_env!("IRIS_RELEASE") {
    Some(tag) => tag,
    None => env!("CARGO_PKG_VERSION"),
};

/// Return the version line that `iris --version` prints.
pub fn version_line() -> String {
    format!("iris {VERSION}")
}

/// Run one command. Gives the exit code (specification 2, section 8.4) and
/// the error text for standard error.
pub fn run(cli: Cli) -> Result<(), socket::Failure> {
    // Session mode: never the store or the data folder, only the session
    // socket.
    if let Some(path) = session_socket() {
        return session_run(&path, &cli.command);
    }
    // The verbs that change changesets go through the engine socket, never
    // through the store (specification 2, section 8.1).
    let data = engine::data_dir(cli.data.clone()).map_err(|e| socket::Failure {
        code: socket::code::ERROR,
        error: e.to_string(),
    });
    if let Command::Open { print } = &cli.command {
        return open_web(&data?, *print);
    }
    let request = socket_request(&cli.command)?;
    if let Command::Job { command } = &cli.command {
        // A bad job ID is a usage error, before the engine or the store.
        let (JobCommand::Run { id, .. }
        | JobCommand::Stop { id }
        | JobCommand::Session { id }
        | JobCommand::Answer { id, .. }
        | JobCommand::Finish { id, .. }) = command;
        if iris_job::parse_job_id(id).is_none() {
            return Err(socket::Failure {
                code: socket::code::USAGE,
                error: format!("{id} is not a job ID"),
            });
        }
        let dir = match &data {
            Ok(d) => d,
            Err(f) => {
                return Err(socket::Failure {
                    code: f.code,
                    error: f.error.clone(),
                })
            }
        };
        if let Some(r) = job_request(dir, command) {
            return r;
        }
    }
    if let Command::List { .. }
    | Command::Wait { .. }
    | Command::Ask { .. }
    | Command::Done { .. } = &cli.command
    {
        return Err(socket::Failure {
            code: socket::code::USAGE,
            error: "this is a verb of a session: run it in a job workspace".into(),
        });
    }
    if let Some((verb, args, json)) = request {
        let out = socket_verb(&data?, verb, args, json)?;
        print!("{}", out.text);
        return out.failure.map_or(Ok(()), Err);
    }
    run_store(cli).map_err(|error| socket::Failure {
        code: socket::code::ERROR,
        error,
    })
}

/// The request on the engine socket of a verb that changes changesets
/// (specification 2, section 8.1): the verb, its arguments and `--json`.
/// `None` for the other verbs.
fn socket_request(
    c: &Command,
) -> Result<Option<(&'static str, serde_json::Value, bool)>, socket::Failure> {
    Ok(match c {
        Command::Submit { id, why, json } => Some((
            "submit",
            serde_json::json!({ "changeset": id, "why": why }),
            *json,
        )),
        Command::Changeset {
            drafts: true, json, ..
        } => Some(("drafts", serde_json::json!({}), *json)),
        Command::Changeset { id, json, .. } => Some((
            "show",
            serde_json::json!({ "changeset": id.as_deref().unwrap_or_default() }),
            *json,
        )),
        Command::Withdraw { id, json } => {
            Some(("withdraw", serde_json::json!({ "changeset": id }), *json))
        }
        Command::Revise { id, json } => {
            Some(("revise", serde_json::json!({ "changeset": id }), *json))
        }
        c => add_request(c)?.map(|r| ("add", r, false)),
    })
}

/// The session socket of the job workspace that holds the current folder:
/// `.iris/socket` of the nearest folder (the current folder or a folder
/// above it) that has `.iris/`. `None` in host mode.
pub fn session_socket() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let iris = dir.join(".iris");
        if iris.is_dir() {
            return Some(iris.join("socket"));
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// One command in session mode
/// (docs/specs/2026-11-17-session-launcher.md, "The command"): each verb
/// goes to the session socket at `path`, and the command never opens the
/// store. Code 4 if the socket does not answer.
fn session_run(path: &std::path::Path, c: &Command) -> Result<(), socket::Failure> {
    let unreachable = |f: socket::Failure| match f.code {
        socket::code::UNREACHABLE => socket::Failure {
            code: f.code,
            error: format!("the session socket {} does not answer", path.display()),
        },
        _ => f,
    };
    let denied = |error: &str| socket::Failure {
        code: socket::code::DENIED,
        error: error.into(),
    };
    // The order of the checks: the socket file, then the class of the
    // verb, and only then the work of the verb (a draft file is read
    // only for a verb that a session can send).
    use std::os::unix::fs::FileTypeExt;
    let there = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket());
    if !there {
        return Err(unreachable(socket::Failure {
            code: socket::code::UNREACHABLE,
            error: String::new(),
        }));
    }
    if let Command::Status { json } = c {
        let r = socket::request_at(
            path,
            "status",
            serde_json::json!({}),
            socket::ANSWER_WAIT,
            "iris status",
        )
        .map_err(unreachable)?;
        print!("{}", session_status_text(&r, *json));
        return Ok(());
    }
    // A job has no grant of an account or a draft (docs/specs/2026-11-17-job-verbs.md).
    if matches!(c, Command::Compose { .. } | Command::Send { .. }) {
        return Err(denied(
            "a session cannot draft, send or nudge: a job has no grant of an account or a draft",
        ));
    }
    if matches!(
        c,
        Command::Skip { .. } | Command::Mute { .. } | Command::Dismiss { .. }
    ) {
        return Err(denied(
            "a session cannot skip, mute or dismiss: a job grants no feed, conversation or signal item",
        ));
    }
    if let Command::Wait { until, reply, why } = c {
        // The wait ends this session: after a lost answer, `iris status`
        // in a new session, or the job page, shows the state of the job.
        let args = serde_json::json!({ "until": until, "reply": reply, "why": why });
        let r = socket::request_at(path, "wait", args, socket::ANSWER_WAIT, "iris status")
            .map_err(unreachable)?;
        println!(
            "{} waits until {}",
            verbs::field(r["job"].as_str().unwrap_or("-")),
            verbs::field(r["until"].as_str().unwrap_or("-"))
        );
        return Ok(());
    }
    if let Command::Ask { question } = c {
        // The ask ends this session: after a lost answer, `iris status` in
        // a new session, or the job page, shows the state of the job.
        let args = serde_json::json!({ "question": question });
        let r = socket::request_at(path, "ask", args, socket::ANSWER_WAIT, "iris status")
            .map_err(unreachable)?;
        println!(
            "{} asks the person and waits for the answer",
            verbs::field(r["job"].as_str().unwrap_or("-"))
        );
        return Ok(());
    }
    if let Command::Done { why } = c {
        // The done ends this session: after a lost answer, the job page
        // shows the state of the job.
        let args = serde_json::json!({ "why": why });
        let r = socket::request_at(path, "done", args, socket::ANSWER_WAIT, "the job page")
            .map_err(unreachable)?;
        println!("{} is done", verbs::field(r["job"].as_str().unwrap_or("-")));
        return Ok(());
    }
    // The read verbs of a session (R4-04f): only the grant of the job.
    let read = match c {
        Command::Search { text, limit, json } => Some((
            "search",
            serde_json::json!({ "text": text, "limit": limit }),
            *json,
        )),
        Command::Show { id, json } => Some(("show", serde_json::json!({ "id": id }), *json)),
        Command::List { form, json } => Some(("list", serde_json::json!({ "form": form }), *json)),
        _ => None,
    };
    if let Some((verb, args, json)) = read {
        // A read changes nothing: after a lost answer, run it again.
        let r = socket::request_at(path, verb, args, socket::ANSWER_WAIT, "the same command")
            .map_err(unreachable)?;
        print!("{}", session_read_text(&r, json));
        return Ok(());
    }
    // Only the write verbs of a session (R4-03): `drafts`, for example, is
    // a host verb of the socket.
    const SESSION_VERBS: [&str; 5] = ["add", "submit", "show", "withdraw", "revise"];
    let request = socket_request(c)?.filter(|(verb, ..)| SESSION_VERBS.contains(verb));
    let Some((verb, args, json)) = request else {
        return Err(denied("this verb is not a verb of a session"));
    };
    let out = socket_verb_at(path, verb, args, json).map_err(unreachable)?;
    print!("{}", out.text);
    out.failure.map_or(Ok(()), Err)
}

/// What `iris search`, `iris show` and `iris list` print in session mode.
/// Mail text is data: each field goes through [`verbs::field`] and the text
/// of a message through [`verbs::clean`].
fn session_read_text(r: &serde_json::Value, json: bool) -> String {
    use std::fmt::Write;
    let s = |v: &serde_json::Value| verbs::field(v.as_str().unwrap_or("-"));
    let mut t = String::new();
    // A list is JSON Lines (specification 2, section 8.3).
    let list = r["messages"].as_array().or(r["items"].as_array());
    match (list, json) {
        (Some(l), true) => l.iter().for_each(|v| {
            let _ = writeln!(t, "{v}");
        }),
        (Some(l), false) => {
            for v in l {
                let line = match v["form"].as_str() {
                    Some(form) => format!("{}  {form}", s(&v["id"])),
                    None => format!(
                        "{}  {}  {}  {}",
                        s(&v["id"]),
                        v["date"].as_str().and_then(|d| d.get(..10)).unwrap_or("-"),
                        s(&v["from"]),
                        s(&v["subject"])
                    ),
                };
                let _ = writeln!(t, "{line}");
            }
            if l.is_empty() {
                t.push_str(if r["items"].is_array() {
                    "No items.\n"
                } else {
                    "No messages.\n"
                });
            }
        }
        // One JSON document of the object. An item is always JSON, as in
        // host mode.
        (None, _) if json || r["item"].is_object() => {
            let v = if r["item"].is_object() {
                &r["item"]
            } else {
                &r["message"]
            };
            let _ = writeln!(t, "{}", serde_json::to_string_pretty(v).unwrap_or_default());
        }
        (None, _) => {
            let m = &r["message"];
            let names = |v: &serde_json::Value| {
                let a: Vec<String> = v.as_array().into_iter().flatten().map(s).collect();
                if a.is_empty() {
                    "-".into()
                } else {
                    a.join(", ")
                }
            };
            let _ = writeln!(t, "ID:      {} (thread {})", s(&m["id"]), s(&m["thread"]));
            let _ = writeln!(t, "Date:    {}", s(&m["date"]));
            let from = match m["from_name"].as_str() {
                Some(n) => verbs::field(&format!("{n} <{}>", m["from"].as_str().unwrap_or("-"))),
                None => s(&m["from"]),
            };
            let _ = writeln!(t, "From:    {from}");
            let _ = writeln!(t, "To:      {}", names(&m["to"]));
            if m["cc"].as_array().is_some_and(|a| !a.is_empty()) {
                let _ = writeln!(t, "Cc:      {}", names(&m["cc"]));
            }
            let _ = writeln!(t, "Subject: {}", s(&m["subject"]));
            if m["items"].as_array().is_some_and(|a| !a.is_empty()) {
                let _ = writeln!(t, "Items:   {}", names(&m["items"]));
            }
            t.push('\n');
            t.push_str(&verbs::clean(m["text"].as_str().unwrap_or_default()));
            t.push('\n');
        }
    }
    t
}

/// What `iris status` prints in session mode.
fn session_status_text(r: &serde_json::Value, json: bool) -> String {
    if json {
        return format!("{r}\n");
    }
    let mut t = format!(
        "{} ({})\nGoal: {}\n",
        r["job"].as_str().unwrap_or_default(),
        r["state"].as_str().unwrap_or_default(),
        r["goal"].as_str().unwrap_or_default()
    );
    if let Some(due) = r["due"].as_str() {
        t.push_str(&format!("Due: {due}\n"));
    }
    t.push_str(&format!(
        "Granted: {} threads, {} messages, {} items\n",
        r["threads"], r["messages"], r["items"]
    ));
    t
}

/// `iris job run` and `iris job stop`: requests on the engine socket.
/// `None` for `iris job session`, which reads the store.
fn job_request(dir: &std::path::Path, c: &JobCommand) -> Option<Result<(), socket::Failure>> {
    let (verb, id, args) = match c {
        JobCommand::Run { id, agent } => (
            "job.run",
            id,
            serde_json::json!({ "job": id, "agent": agent }),
        ),
        JobCommand::Stop { id } => ("job.stop", id, serde_json::json!({ "job": id })),
        JobCommand::Session { .. } | JobCommand::Answer { .. } | JobCommand::Finish { .. } => {
            return None
        }
    };
    Some(
        match socket::request_with(
            dir,
            verb,
            args,
            sessions::STOP_WAIT + std::time::Duration::from_secs(15),
            // A lost answer: the run may be registered, or the stop may
            // have ended the session. The store shows which.
            &format!("iris job session {id}"),
        ) {
            Ok(r) => {
                let session = r["session"].as_str().unwrap_or_default();
                match verb {
                    "job.run" => {
                        println!(
                            "{session}: registered. Run `iris job session {id}` for its state."
                        );
                        Ok(())
                    }
                    _ if r["reaped"] == true => {
                        println!("{session}: stopped.");
                        Ok(())
                    }
                    _ => Err(socket::Failure {
                        code: socket::code::ERROR,
                        error: format!(
                            "{session}: the session ended, but its processes still stop ({}); \
                         run `iris job session {id}` later",
                            r["why"].as_str().unwrap_or("reason not known")
                        ),
                    }),
                }
            }
            Err(f) => Err(f),
        },
    )
}

/// The `add` request on the engine socket of a verb that adds an action.
/// `None` for the other verbs.
pub fn add_request(c: &Command) -> Result<Option<serde_json::Value>, socket::Failure> {
    let Some((verb, targets, args, to)) = action(c)? else {
        return Ok(None);
    };
    let mut request = serde_json::json!({ "verb": verb, "targets": targets, "args": args });
    if let Some(cs) = &to.changeset {
        request["changeset"] = cs.clone().into();
    }
    if let Some(why) = &to.why {
        request["why"] = why.clone().into();
    }
    Ok(Some(request))
}

/// What a verb prints, and the failure (exit code and error text) that the
/// command gives after the print.
pub struct Printed {
    pub text: String,
    pub failure: Option<socket::Failure>,
}

/// Send the request of a verb on the engine socket, and give what the
/// command prints. The command and `iris mcp` both use it, so they give the
/// same result and the same exit code (specification 2, sections 8.3 to 8.5).
pub fn socket_verb(
    dir: &std::path::Path,
    verb: &str,
    args: serde_json::Value,
    json: bool,
) -> Result<Printed, socket::Failure> {
    socket_verb_at(&dir.join(socket::SOCKET), verb, args, json)
}

/// `iris open`: a new sign-in address from the engine (the socket verb
/// `sign-in`, `docs/specs/2026-11-17-mac-app.md`), printed or opened in the
/// browser. The code never goes into the arguments of a process: another
/// OS user can read them. The browser gets the path of a new page of [`open_page`].
fn open_web(dir: &std::path::Path, print: bool) -> Result<(), socket::Failure> {
    let r = socket::request_with(
        dir,
        "sign-in",
        serde_json::json!({}),
        socket::ANSWER_WAIT,
        "iris open",
    )?;
    let url = r["url"].as_str().unwrap_or_default();
    let error = |e: String| socket::Failure {
        code: socket::code::ERROR,
        error: e,
    };
    if print {
        println!("{url}");
        return Ok(());
    }
    let page = open_page(dir, url).map_err(|e| error(format!("open: {e}")))?;
    let opened = opener(std::env::consts::OS, &page)
        .is_some_and(|mut c| c.status().is_ok_and(|s| s.success()));
    if !opened {
        return Err(error(format!(
            "open: no browser opened. Open {} or run iris open --print",
            page.display()
        )));
    }
    Ok(())
}

/// The redirect pages of `iris open` in the data folder:
/// `open-<pid>-<ns>.html`, one page for each call.
pub const OPEN_PAGE: (&str, &str) = ("open-", ".html");

/// Write a new redirect page to `url` with mode `0600`. Each call gets its
/// own file, so two calls never give the browser the same page. The file is
/// new (`create_new`), so the command never writes through a file or a link
/// that is there. Pages older than the life of a code are removed. Gives the
/// path of the page.
pub fn open_page(dir: &std::path::Path, url: &str) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // The engine gives `http://<ip>:<port>/#code=<hex>`. Only these bytes,
    // so the page needs no HTML escape.
    let plain = url
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b":/.#=[]".contains(&b));
    if !plain || !url.starts_with("http://") {
        return Err(std::io::Error::other(
            "the engine gave an address that is not valid",
        ));
    }
    remove_old_pages(dir);
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let (start, end) = OPEN_PAGE;
    let path = dir.join(format!("{start}{}-{ns}{end}", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    f.write_all(
        format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Iris</title>\
             <meta http-equiv=\"refresh\" content=\"0;url={url}\">\n"
        )
        .as_bytes(),
    )?;
    f.sync_all()?;
    Ok(path)
}

/// Remove the redirect pages whose code ended: older than
/// [`iris_api::CODE_LIFE`].
fn remove_old_pages(dir: &std::path::Path) {
    let (start, end) = OPEN_PAGE;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > iris_api::CODE_LIFE);
        if name.starts_with(start) && name.ends_with(end) && old {
            // A link is removed, not followed.
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The command that opens `page` in the browser on the system `os`. Its
/// only argument is the path.
pub fn opener(os: &str, page: &std::path::Path) -> Option<std::process::Command> {
    let program = match os {
        "macos" => "/usr/bin/open",
        "linux" => "xdg-open",
        _ => return None,
    };
    let mut c = std::process::Command::new(program);
    c.arg(page)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Some(c)
}

/// [`socket_verb`] on the socket at `path`.
fn socket_verb_at(
    path: &std::path::Path,
    verb: &str,
    args: serde_json::Value,
    json: bool,
) -> Result<Printed, socket::Failure> {
    let id = args["changeset"].as_str().unwrap_or_default().to_string();
    let recover = format!(
        "iris changeset {}",
        args["changeset"].as_str().unwrap_or("--drafts")
    );
    let r = socket::request_at(path, verb, args, socket::ANSWER_WAIT, &recover)?;
    let mut failure = None;
    let text = match verb {
        "add" if json => format!("{r}\n"),
        "add" => format!(
            "{} {}\n",
            r["changeset"].as_str().unwrap_or_default(),
            r["n"]
        ),
        "drafts" => drafts_text(&r, json),
        "submit" => {
            let code = submit_code(&r);
            if code != 0 {
                failure = Some(socket::Failure {
                    code,
                    error: format!("{id}: not each action is applied (exit code {code})"),
                });
            }
            changeset_text(&r, json)
        }
        _ => changeset_text(&r, json),
    };
    Ok(Printed { text, failure })
}

/// The most tool calls of `iris mcp` that run at the same time.
pub const MCP_CALLS: usize = 4;

/// `iris mcp`: serve the MCP tools on standard input and output. Each tool
/// call takes the path of its verb: the engine socket, or a read verb on
/// the store.
fn mcp(dir: PathBuf) -> Result<(), String> {
    use iris_mcp::{Call, Outcome};
    let run = move |c: Call| -> Outcome {
        let fail = |f: socket::Failure| Outcome::fail(f.code, f.error);
        // The read verbs give each error with code 1, as `run_store` does.
        let read = |out: verbs::Output| match out {
            Ok(text) => Outcome::ok(text),
            Err(e) => Outcome::fail(socket::code::ERROR, e),
        };
        let open = || Store::open(&dir).map_err(|e| format!("store: {e}"));
        let watch = |c: WatchCommand| read(open().and_then(|s| verbs::watch(&s, &c)));
        match c {
            Call::Socket { verb, args } => match socket_verb(&dir, verb, args, true) {
                Ok(p) => Outcome {
                    text: p.text,
                    failure: p.failure.map(|f| (f.code, f.error)),
                },
                Err(f) => fail(f),
            },
            Call::Status => read(open().and_then(|s| verbs::status(&s, true))),
            Call::Search { text, limit } => {
                read(open().and_then(|s| verbs::search(&s, &text, limit, true)))
            }
            Call::Show { id } => read(open().and_then(|s| verbs::show(&s, &id, true))),
            Call::WatchAdd { text } => watch(WatchCommand::Add { text }),
            Call::WatchList => watch(WatchCommand::List { json: true }),
            Call::WatchRemove { id } => watch(WatchCommand::Remove { id }),
            // `iris matters` prints its text with a newline (`println!`).
            Call::MattersList => read(
                open()
                    .and_then(|s| verbs::matters(&s, &MattersCommand::List))
                    .map(|t| t + "\n"),
            ),
            Call::MattersShow { id } => read(
                open()
                    .and_then(|s| verbs::matters(&s, &MattersCommand::Show { id }))
                    .map(|t| t + "\n"),
            ),
        }
    };
    // rmcp uses timers for its time limits. At most MCP_CALLS calls run at
    // the same time; the others wait in the queue of the runtime.
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .max_blocking_threads(MCP_CALLS)
        .build()
        .map_err(|e| format!("mcp: {e}"))?
        .block_on(iris_mcp::serve(Arc::new(run)))
        .map_err(|e| format!("mcp: {e}"))
}

/// The exit code of `iris submit` (specification 2, section 8.4), from the
/// actions of the revision that it submitted, in the order 2, 1, 3, 5, 0.
pub fn submit_code(doc: &serde_json::Value) -> u8 {
    let mut codes = Vec::new();
    for a in doc["actions"].as_array().into_iter().flatten() {
        if a["revision"] != doc["revision"] {
            continue;
        }
        let d = &a["decision"];
        codes.push(match (d["state"].as_str(), d["by"].as_str()) {
            (Some("denied"), Some("engine")) => socket::code::USAGE,
            (Some("failed"), _) => socket::code::ERROR,
            (Some("denied"), _) => socket::code::DENIED,
            (Some("applied"), _) => 0,
            _ => socket::code::OPEN,
        });
    }
    [
        socket::code::USAGE,
        socket::code::ERROR,
        socket::code::DENIED,
        socket::code::OPEN,
    ]
    .into_iter()
    .find(|c| codes.contains(c))
    .unwrap_or(0)
}

/// `iris changeset --drafts`: one changeset ID on each line.
fn drafts_text(r: &serde_json::Value, json: bool) -> String {
    let ids = r["changesets"].as_array().cloned().unwrap_or_default();
    if json {
        return ids.iter().map(|i| format!("{i}\n")).collect();
    }
    if ids.is_empty() {
        return "No draft.\n".into();
    }
    ids.iter()
        .map(|i| format!("{}\n", i.as_str().unwrap_or_default()))
        .collect()
}

/// A changeset as text (specification 2, section 8.3): a label line for the
/// changeset, then one line for each action, which starts with
/// `<revision>.<n>`.
pub fn changeset_text(doc: &serde_json::Value, json: bool) -> String {
    if json {
        return format!("{doc}\n");
    }
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    let mut out = format!(
        "ID: {}\nState: {}\nRevision: {}\n",
        s(&doc["id"]),
        s(&doc["state"]),
        doc["revision"]
    );
    if let Some(why) = doc["why"].as_str() {
        out.push_str(&format!("Why: {}\n", verbs::field(why)));
    }
    for a in doc["actions"].as_array().into_iter().flatten() {
        let state = a["decision"]["state"].as_str().unwrap_or("draft");
        let targets: Vec<String> = a["targets"]
            .as_object()
            .into_iter()
            .flat_map(|t| t.values())
            .flat_map(|v| v.as_array().cloned().unwrap_or_default())
            .map(|v| verbs::field(v.as_str().unwrap_or_default()))
            .collect();
        let mut line = format!(
            "{}.{} {} {} {state}",
            a["revision"],
            a["n"],
            s(&a["verb"]),
            targets.join(",")
        );
        if let Some(args) = a["args"].as_object().filter(|o| !o.is_empty()) {
            line.push_str(&format!(
                " args: {}",
                verbs::field(&serde_json::Value::from(args.clone()).to_string())
            ));
        }
        if let Some(why) = a["why"].as_str() {
            line.push_str(&format!(" why: {}", verbs::field(why)));
        }
        if let Some(reason) = a["decision"]["reason"].as_str() {
            line.push_str(&format!(" reason: {}", verbs::field(reason)));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Run one command that uses the store. Gives the error text for standard error.
fn run_store(cli: Cli) -> Result<(), String> {
    let data = || engine::data_dir(cli.data.clone()).map_err(|e| e.to_string());
    let open =
        || -> Result<Store, String> { Store::open(&data()?).map_err(|e| format!("store: {e}")) };
    let result = command(&cli, &open);
    // The commands that write items or the facts of their messages: write the
    // attention of each changed item before the exit, also after an error
    // (spec 2 section 6.4). `iris serve` does it in its own thread.
    if matches!(
        cli.command,
        Command::Mirror { .. } | Command::Understand { .. }
    ) {
        let attention = open().and_then(|s| {
            iris_policy::refresh_attention(&s, engine::now_ms().div_euclid(1000))
                .map_err(|e| format!("attention: {e}"))?;
            // The measure "time to the first brief" ends at the first brief with content.
            iris_api::measures::check_brief(&s, engine::now_ms())
                .map_err(|e| format!("measures: {e}"))
        });
        if let Err(e) = attention {
            return Err(match result {
                Err(first) => format!("{first}\n{e}"),
                Ok(()) => e,
            });
        }
    }
    result
}

fn command(cli: &Cli, open: &dyn Fn() -> Result<Store, String>) -> Result<(), String> {
    match &cli.command {
        Command::Serve { addr } => {
            // Check before the store opens, so a refused address makes no data folder.
            check_loopback(*addr).map_err(|e| e.to_string())?;
            serve(Arc::new(open()?), *addr).map_err(|e| e.to_string())
        }
        Command::Account {
            command:
                AccountCommand::Add {
                    client,
                    paste,
                    write,
                },
        } => account_add(&open()?, client, *paste, *write),
        Command::Account {
            command:
                AccountCommand::AddImap {
                    address,
                    host,
                    port,
                    user,
                },
        } => {
            let store = open()?;
            let server = match host {
                Some(host) => engine::ImapServer {
                    host: host.clone(),
                    port: port.unwrap_or(993),
                    user: user.clone().unwrap_or_else(|| address.clone()),
                },
                // A new password for an account that exists: the saved server.
                None => engine::saved_imap(&store, address)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| {
                        format!("no IMAP account has the address {address}: give --host")
                    })?,
            };
            account_add_imap(&store, address, &server)
        }
        Command::Account {
            command:
                AccountCommand::AddOutlook {
                    client_id,
                    tenant,
                    paste,
                },
        } => account_add_outlook(&open()?, client_id, tenant, *paste),
        Command::Account {
            command: AccountCommand::Remove { address, yes },
        } => {
            let store = open()?;
            if !yes {
                let stdin = std::io::stdin();
                remove_confirm(address, &mut stdin.lock())?;
            }
            print(account_remove(&store, address))
        }
        Command::Mirror { command } => mirror(Arc::new(open()?), command),
        Command::Mcp => mcp(engine::data_dir(cli.data.clone()).map_err(|e| e.to_string())?),
        Command::Status { json } => print(verbs::status(&open()?, *json)),
        Command::Search { text, limit, json } => {
            print(verbs::search(&open()?, text, *limit, *json))
        }
        Command::Show { id, json } => print(verbs::show(&open()?, id, *json)),
        // `run` refuses it in host mode, before the store.
        Command::List { .. }
        | Command::Wait { .. }
        | Command::Ask { .. }
        | Command::Done { .. } => Err("this is a verb of a session".into()),
        Command::Watch { command } => print(verbs::watch(&open()?, command)),
        Command::Job {
            command: JobCommand::Session { id },
        } => {
            let job = iris_job::parse_job_id(id).ok_or(format!("{id} is not a job ID"))?;
            let store = open()?;
            match sessions::last_session(&store, job)? {
                Some(s) => {
                    println!(
                        "{} {}: {}",
                        iris_job::format_session_id(s.id),
                        s.agent,
                        sessions::state(&s)
                    );
                    Ok(())
                }
                None => Err(format!("{id} has no session")),
            }
        }
        Command::Job {
            command: JobCommand::Answer { id, text },
        } => {
            let job = iris_job::parse_job_id(id).ok_or(format!("{id} is not a job ID"))?;
            let store = open()?;
            iris_api::job_row(&store, id)
                .map_err(|e| e.to_string())?
                .ok_or(format!("no job has the ID {id}"))?;
            iris_api::answer_job(&store, job, text, None, engine::now_ms())
                .map_err(|e| e.to_string())?;
            println!("{id}: answered. Iris starts a new session of the job.");
            Ok(())
        }
        Command::Job {
            command: JobCommand::Finish { id, cancel },
        } => {
            let job = iris_job::parse_job_id(id).ok_or(format!("{id} is not a job ID"))?;
            let store = open()?;
            iris_api::job_row(&store, id)
                .map_err(|e| e.to_string())?
                .ok_or(format!("no job has the ID {id}"))?;
            let end = if *cancel {
                iris_job::End::Cancel
            } else {
                iris_job::End::Complete
            };
            iris_api::finish_job(&store, job, end, engine::now_ms()).map_err(|e| e.to_string())?;
            println!(
                "{id}: done. The engine removes the mail files of its workspace when no \
                 session of the job runs, or at its next start."
            );
            Ok(())
        }
        Command::Job { .. } => Err("the engine socket runs this verb".into()),
        Command::Matters { command } => {
            let store = open()?;
            let text = match command.as_ref().unwrap_or(&MattersCommand::List) {
                MattersCommand::Propose => {
                    cost::propose_matters(&store, iris_matters::Gate::Now, engine::now_ms())?
                        .ok_or("an understanding run holds the lock; try again later")?
                }
                c => verbs::matters(&store, c)?,
            };
            println!("{text}");
            Ok(())
        }
        Command::Backup { path } => {
            let files = open()?.backup(path).map_err(|e| format!("backup: {e}"))?;
            println!("iris: backup in {} ({files} content files)", path.display());
            Ok(())
        }
        Command::Export { path } => {
            let n = iris_api::export(&open()?, path).map_err(|e| format!("export: {e}"))?;
            println!(
                "iris: export in {} ({} items, {} messages, {} raw messages)",
                path.display(),
                n.items,
                n.messages,
                n.files
            );
            Ok(())
        }
        Command::Service { action } => {
            println!("{}", service(*action).map_err(|e| e.to_string())?);
            Ok(())
        }
        Command::Model { command } => {
            let text = match command {
                ModelCommand::Show => cost::show_model(&open()?)?,
                ModelCommand::Set { provider } => model_set(&open()?, provider)?,
            };
            println!("{text}");
            Ok(())
        }
        Command::Sending { state } => {
            let store = open()?;
            match state {
                Some(Switch::On) => {
                    {
                        use std::io::IsTerminal;
                        let stdin = std::io::stdin();
                        sending_on_confirm(stdin.is_terminal(), &mut stdin.lock())?;
                    }
                    iris_apply::set_sending(&store, true).map_err(|e| e.to_string())?;
                }
                Some(Switch::Off) => match iris_apply::set_sending(&store, false) {
                    Ok(()) => {}
                    Err(iris_apply::Error::Busy) => {
                        return Err("A send is in progress. Run the command again.".into())
                    }
                    Err(e) => return Err(e.to_string()),
                },
                None => {}
            }
            println!(
                "{}",
                match iris_apply::sending(&store).map_err(|e| e.to_string())? {
                    Some(_) => "Sending is on.",
                    None => "Sending is off. Sends wait in the outbox.",
                }
            );
            Ok(())
        }
        Command::Stop { show } => {
            let store = open()?;
            if *show {
                let now = iris_apply::stop_state(&store).map_err(|e| e.to_string())?;
                println!("{}", stop_state(now.as_ref()));
                return Ok(());
            }
            let s = iris_apply::stop(&store).map_err(|e| e.to_string())?;
            let text = stop_text(&s);
            if s.live > 0 || s.busy {
                return Err(text);
            }
            println!("{text}");
            Ok(())
        }
        Command::Resume => {
            let store = open()?;
            let resumed = iris_apply::resume(&store).map_err(|e| e.to_string())?;
            println!(
                "{}",
                if resumed {
                    "Iris runs again. Each held send waits a full hold before it leaves."
                } else {
                    "Iris was not stopped. Nothing changed."
                }
            );
            Ok(())
        }
        Command::Cancel { id } => {
            let store = open()?;
            println!("{}", cancel_send(&store, id, engine::now_ms())?);
            Ok(())
        }
        Command::Changeset { .. }
        | Command::Withdraw { .. }
        | Command::Revise { .. }
        | Command::Submit { .. }
        | Command::Close { .. }
        | Command::Decline { .. }
        | Command::Skip { .. }
        | Command::Mute { .. }
        | Command::Dismiss { .. }
        | Command::Defer { .. }
        | Command::Correct { .. }
        | Command::Archive(_)
        | Command::Inbox(_)
        | Command::Read(_)
        | Command::Unread(_)
        | Command::Trash(_)
        | Command::Label(_)
        | Command::Unlabel(_)
        | Command::Compose { .. }
        | Command::Send { .. }
        | Command::Open { .. } => unreachable!("run() sends these verbs to the socket"),
        Command::Understand {
            estimate,
            approve,
            budget,
            max_cost,
            max_messages,
        } => {
            let store = open()?;
            let now = engine::now_ms();
            let text = match (estimate, approve, budget) {
                (true, _, _) => cost::estimate(&store, now)?,
                (_, Some(code), Some(budget)) => cost::approve_command(&store, code, budget)?,
                _ => {
                    let limits = iris_understand::batch::Limits {
                        max_messages: *max_messages,
                        max_cost_micro_usd: max_cost
                            .as_deref()
                            .map(|c| cost::parse_usd(c, cost::MAX_BUDGET_USD))
                            .transpose()?,
                        ..Default::default()
                    };
                    cost::run(&store, &limits, now)?
                }
            };
            println!("{text}");
            Ok(())
        }
        Command::Write { id, intent } => {
            let store = open()?;
            let connect = |row: &iris_understand::budget::ModelRow| cost::connect(&store, row);
            let text = reply::write(&store, id, intent.as_deref(), &connect, engine::now_ms())?;
            print!("{text}");
            Ok(())
        }
    }
}

/// `iris model set`: checks the input first, then reads the key, then
/// writes the row.
fn model_set(store: &Store, provider: &SetModel) -> Result<String, String> {
    let m = match provider {
        SetModel::Anthropic {
            model,
            price_in,
            price_out,
            max_tokens,
            timeout,
        } => {
            let price = (
                cost::parse_usd(price_in, cost::MAX_PRICE_USD)?,
                cost::parse_usd(price_out, cost::MAX_PRICE_USD)?,
            );
            cost::NewModel {
                local: false,
                base_url: "https://api.anthropic.com".into(),
                model: model.clone(),
                price: Some(price),
                api_key: Some(cost::read_key()?),
                max_tokens: *max_tokens,
                timeout_s: *timeout,
            }
        }
        SetModel::Local {
            base_url,
            model,
            max_tokens,
            timeout,
        } => {
            cost::local_host(base_url)?;
            cost::NewModel {
                local: true,
                base_url: base_url.clone(),
                model: model.clone(),
                price: None,
                api_key: None,
                max_tokens: *max_tokens,
                timeout_s: *timeout,
            }
        }
    };
    if m.max_tokens == 0 || m.timeout_s == 0 {
        return Err("--max-tokens and --timeout must be larger than 0".into());
    }
    let version = cost::set_model(store, &m)?;
    Ok(format!(
        "Model set: {} (version {version}). The budget is cleared: run `iris understand --estimate`.",
        m.model
    ))
}

/// The line of `iris mirror verify` for an account whose first copy runs
/// in another process.
const FIRST_COPY_RUNS: &str =
    "the first copy runs now, so no full mirror exists to verify. Run verify after it ends.";

/// The first copy of the account ended (`account.mirror_ready_at` is set).
fn mirror_ready(store: &Store, account: i64) -> Result<bool, String> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT mirror_ready_at IS NOT NULL FROM account WHERE id = ?1",
                [account],
                |r| r.get(0),
            )?)
        })
        .map_err(|e| e.to_string())
}

/// `iris mirror sync|verify`: each account that has a sign-in, one at a time.
/// If another process syncs an account, that account gives an error,
/// except for a verify during the first copy of the account.
fn mirror(store: Arc<Store>, command: &MirrorCommand) -> Result<(), String> {
    let config = gmail_config()?;
    let accounts = engine::signed_in_accounts(&store).map_err(|e| e.to_string())?;
    if accounts.is_empty() {
        return Err("no account to sync: run `iris account add <client.json>`".into());
    }
    let mut failed = false;
    for (account, state) in accounts {
        // Report each account, so a success means that each mailbox ran.
        if state.as_deref() == Some(engine::STATE_NEEDS_SIGN_IN) {
            failed = true;
            say!("iris: account {account}: {}", engine::Error::NeedsSignIn);
            continue;
        }
        // Another process runs the first copy: no full mirror exists to
        // compare (issue #679). The check comes before any token or
        // provider call, so their errors cannot hide it.
        if matches!(command, MirrorCommand::Verify)
            && !mirror_ready(&store, account)?
            && iris_sync::other_sync_runs(&store, account).map_err(|e| e.to_string())?
        {
            println!("account {account}: {FIRST_COPY_RUNS}");
            continue;
        }
        let mail = match engine::provider_for(&store, account, &config) {
            Ok(m) => m,
            Err(e) => {
                failed = true;
                say!("iris: account {account}: {e}");
                continue;
            }
        };
        let mut worker = engine::Worker::boxed(
            store.clone(),
            mail,
            account,
            std::env::var("IRIS_TEST_TOKEN_URI").ok(),
        );
        let line = match command {
            MirrorCommand::Sync => worker.sync().map(|s| {
                format!(
                    "synced to history {}: fetched {}, gone {}, failed {}{}",
                    s.history_id,
                    s.fetched,
                    s.gone,
                    s.failed,
                    if s.resync { ", full resync" } else { "" }
                )
            }),
            MirrorCommand::Verify => worker.verify().map(|r| {
                let mut text = format!(
                    "verified at history {}: {} differences, {} skipped{}",
                    r.history_id,
                    r.total,
                    r.skipped,
                    if r.resync {
                        ", the next sync runs a full resync"
                    } else {
                        ""
                    }
                );
                for d in &r.differences {
                    text.push_str(&format!("\n  {}", verbs::field(&d.to_string())));
                }
                text
            }),
        };
        match line {
            Ok(text) => println!("account {account}: {text}"),
            // A first copy that started after the check above.
            Err(engine::Error::Sync(iris_sync::Error::OtherSync))
                if matches!(command, MirrorCommand::Verify) && !mirror_ready(&store, account)? =>
            {
                println!("account {account}: {FIRST_COPY_RUNS}")
            }
            Err(e) => {
                failed = true;
                say!("iris: account {account}: {e}");
            }
        }
    }
    // Understanding gives the words and the senders that search finds
    // (spec 1 section 5.6). It also runs if one account failed. If another
    // run holds the lock, it can be past the new mail: wait, then run.
    if matches!(command, MirrorCommand::Sync) {
        let mut said = false;
        loop {
            match cost::after_sync(&store, engine::now_ms()) {
                Ok(cost::AfterSync::Ran { report, .. }) => {
                    print!("{}", prefix_lines("understand: ", &report))
                }
                Ok(cost::AfterSync::Idle) => println!("understand: no message to read"),
                Ok(cost::AfterSync::Busy) => {
                    if !said {
                        println!("understand: waits for another run to end");
                        said = true;
                    }
                    std::thread::sleep(engine::BUSY_RETRY);
                    continue;
                }
                Err(e) => {
                    failed = true;
                    say!("iris: understand: {e}");
                }
            }
            break;
        }
    }
    if failed {
        Err("one or more accounts, or the understanding run, failed".into())
    } else {
        Ok(())
    }
}

fn prefix_lines(prefix: &str, text: &str) -> String {
    text.lines().map(|l| format!("{prefix}{l}\n")).collect()
}

fn print(out: verbs::Output) -> Result<(), String> {
    print!("{}", out?);
    Ok(())
}

/// `iris account add`: sign in, then save the client and the sealed refresh token.
fn account_add(
    store: &Store,
    client_path: &std::path::Path,
    paste: bool,
    write: bool,
) -> Result<(), String> {
    let json = std::fs::read(client_path).map_err(|e| format!("{}: {e}", client_path.display()))?;
    let client = test_token_uri(iris_auth::Client::from_json(&json).map_err(|e| e.to_string())?)?;
    // Check the Gmail settings before the sign-in, so a bad setting fails
    // before a token exists.
    let gmail = gmail()?;
    let sign_in = if write {
        client.sign_in_write()
    } else {
        client.sign_in()
    }
    .map_err(|e| e.to_string())?;
    let tokens = browser_sign_in(sign_in, paste)?;
    let (_, address) =
        engine::save_sign_in(store, &gmail, &json, &tokens).map_err(|e| e.to_string())?;
    println!(
        "iris: added {address}, {}",
        if tokens.write {
            "with write access"
        } else {
            "read only: run the command again with --write to let Iris change mail"
        }
    );
    if tokens.refresh_expires_in.is_some() {
        say!(
            "iris: caution: the OAuth client is in the \"Testing\" status, so the sign-in \
             ends after 7 days. Publish the app to \"In production\"."
        );
    }
    Ok(())
}

/// The sign-in in a browser: the redirect to this machine, or with
/// `paste` the address that the person pastes.
fn browser_sign_in(sign_in: iris_auth::SignIn, paste: bool) -> Result<iris_auth::Tokens, String> {
    say!(
        "iris: open this address in a browser and sign in:\n{}",
        sign_in.url()
    );
    if paste {
        say!("iris: then paste the address of the browser here:");
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        sign_in.finish(line.trim())
    } else {
        sign_in.wait(SIGN_IN_WAIT)
    }
    .map_err(|e| e.to_string())
}

/// `iris account add-outlook`: sign in to Microsoft, read the address of
/// the mailbox, then save the account with the refresh token sealed.
fn account_add_outlook(
    store: &Store,
    client_id: &str,
    tenant: &str,
    paste: bool,
) -> Result<(), String> {
    let client = test_token_uri(
        iris_auth::Client::microsoft(client_id, tenant).map_err(|e| e.to_string())?,
    )?;
    // Check the Graph setting before the sign-in, so a bad setting fails
    // before a token exists.
    let mail = engine::outlook_provider(Vec::new()).map_err(|e| e.to_string())?;
    let tokens = browser_sign_in(client.sign_in().map_err(|e| e.to_string())?, paste)?;
    let address = mail
        .profile(tokens.access.token.expose())
        .map_err(|e| format!("Outlook: {e}"))?
        .email_address;
    let id = engine::save_outlook(store, &address, client_id, tenant, &tokens.refresh)
        .map_err(|e| e.to_string())?;
    println!("iris: added {address} (account {id})");
    Ok(())
}

/// `iris account add-imap`: read the password, sign in once to check it,
/// then save the account with the password sealed. The password is never
/// in a log or an error.
fn account_add_imap(
    store: &Store,
    address: &str,
    server: &engine::ImapServer,
) -> Result<(), String> {
    if !address.contains('@') || address.trim() != address {
        return Err(format!("{address:?} is not a mail address"));
    }
    let password = read_password(&format!("iris: the password of {}: ", server.user))?;
    let mail = engine::imap_provider(address, server).map_err(|e| e.to_string())?;
    match mail.profile(password.expose()) {
        Ok(_) => {}
        Err(iris_provider::Error::Unauthorized) => {
            return Err("the IMAP server refused the user or the password".into())
        }
        Err(e) => return Err(format!("the sign-in check failed: {e}")),
    }
    let id = engine::save_imap(store, address, server, &password).map_err(|e| e.to_string())?;
    println!("iris: added {address} (account {id})");
    Ok(())
}

/// One line of the password: from the terminal with no echo, else from
/// standard input (a script or a test). The line end does not count.
fn read_password(prompt: &str) -> Result<iris_auth::Secret, String> {
    use std::io::{BufRead, IsTerminal};
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.is_terminal() {
        say!("{prompt}");
        let _echo = no_echo::Guard::new().map_err(|e| format!("terminal: {e}"))?;
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
    } else {
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
    }
    let password = line.trim_end_matches(['\r', '\n']).to_string();
    if password.is_empty() {
        return Err("no password".into());
    }
    Ok(iris_auth::Secret::new(password))
}

/// Terminal echo off while a guard lives (POSIX termios, macOS and Linux).
/// A signal that ends the process (Ctrl-C and others) does not run `Drop`,
/// so a handler puts the saved terminal state back first, then ends the
/// process with the same signal.
mod no_echo {
    use std::sync::OnceLock;

    /// The terminal state before the first guard. The handler reads it.
    static SAVED: OnceLock<libc::termios> = OnceLock::new();
    const SIGNALS: [libc::c_int; 4] = [libc::SIGINT, libc::SIGTERM, libc::SIGQUIT, libc::SIGHUP];

    pub struct Guard {
        old: libc::termios,
        handlers: [libc::sighandler_t; 4],
    }

    extern "C" fn restore(sig: libc::c_int) {
        // SAFETY: tcsetattr, signal and raise are async-signal-safe, and
        // SAVED is set before the handler is installed.
        unsafe {
            if let Some(t) = SAVED.get() {
                libc::tcsetattr(0, libc::TCSANOW, t);
            }
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    impl Guard {
        pub fn new() -> std::io::Result<Guard> {
            // SAFETY: tcgetattr and tcsetattr write and read one termios
            // value of this frame, for standard input (fd 0). signal
            // installs `restore`, an extern "C" fn of one c_int.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(0, &mut t) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let old = t;
                SAVED.get_or_init(|| old);
                let handler = restore as extern "C" fn(libc::c_int) as libc::sighandler_t;
                let handlers = SIGNALS.map(|sig| {
                    let before = libc::signal(sig, handler);
                    // A signal that the parent ignores stays ignored.
                    if before == libc::SIG_IGN {
                        libc::signal(sig, libc::SIG_IGN);
                    }
                    before
                });
                let guard = Guard { old, handlers };
                t.c_lflag &= !libc::ECHO;
                if libc::tcsetattr(0, libc::TCSANOW, &t) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(guard)
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: as in `new`, with the values that `new` read.
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &self.old);
                for (sig, before) in SIGNALS.iter().zip(self.handlers) {
                    libc::signal(*sig, before);
                }
            }
            eprintln!();
        }
    }
}

/// The Gmail client. `IRIS_GMAIL_QUOTA` changes the quota units for each
/// minute, so a change at Google needs no new release. Tests set
/// `IRIS_TEST_GMAIL_URL` to a fake server on 127.0.0.1. Any other address is
/// refused, so the token cannot go elsewhere.
pub fn gmail() -> Result<iris_gmail::Gmail, String> {
    gmail_config().map(iris_gmail::Gmail::new)
}

fn gmail_config() -> Result<iris_gmail::Config, String> {
    let mut config = iris_gmail::Config::default();
    if let Ok(quota) = std::env::var("IRIS_GMAIL_QUOTA") {
        config.quota_per_minute = quota
            .parse()
            .ok()
            .filter(|q| *q > 0)
            .ok_or("IRIS_GMAIL_QUOTA must be a number above 0")?;
    }
    if let Some(url) = std::env::var_os("IRIS_TEST_GMAIL_URL") {
        let url = url.into_string().unwrap_or_default();
        if !is_local_url(&url) {
            return Err("IRIS_TEST_GMAIL_URL must be http://127.0.0.1:<port>".into());
        }
        config.base_url = url;
    }
    Ok(config)
}

/// True for `http://127.0.0.1:<port>` with an optional path. The host part
/// ends at the first `/`, so `http://127.0.0.1:80@other.host/` is refused.
pub fn is_local_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let port = rest.split('/').next().unwrap_or_default();
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

/// Tests set `IRIS_TEST_TOKEN_URI` to a fake token endpoint on 127.0.0.1.
fn test_token_uri(client: iris_auth::Client) -> Result<iris_auth::Client, String> {
    match std::env::var("IRIS_TEST_TOKEN_URI") {
        Ok(uri) => client.with_test_token_uri(&uri).map_err(|e| e.to_string()),
        Err(_) => Ok(client),
    }
}

/// Return all the routes of the engine.
pub fn app(store: Arc<Store>, auth: Arc<iris_api::Auth>) -> axum::Router {
    // The model client of `POST /api/ask`, from the model row of each call.
    let model_store = store.clone();
    let connect: iris_api::Connect = Arc::new(move |row| cost::connect(&model_store, row));
    // The API routes come before the fallback of the web client.
    iris_api::router_with_model(store, auth, connect).merge(iris_web::router())
}

/// The API has no TLS, so only this machine may connect (specification 2,
/// section 8.2).
fn check_loopback(addr: SocketAddr) -> std::io::Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{addr} is not a loopback address: use 127.0.0.1 or [::1]"
        )))
    }
}

/// Start the sessions of jobs: end each open session of the last engine,
/// and start the supervisor thread. The helper of each session is this
/// program with `session-hold`.
pub fn start_sessions(store: Arc<Store>) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let data = store.dir().to_path_buf();
    let helper: Arc<sessions::HelperCommand> = Arc::new(move |args: &[std::ffi::OsString]| {
        let mut c = std::process::Command::new(&exe);
        c.arg(SESSION_HOLD).arg(&data).args(args);
        c
    });
    let isolation = iris_session::isolation::system(store.dir())
        .map(|b| -> Arc<dyn iris_session::isolation::Isolation> { Arc::from(b) });
    let s = sessions::Sessions::new(store, isolation, helper).map_err(std::io::Error::other)?;
    let _ = sessions::SESSIONS.set(s.clone());
    std::thread::Builder::new()
        .name("sessions".into())
        .spawn(move || s.supervise_forever())?;
    Ok(())
}

/// The hidden first argument of the hold helper of a session. `main` runs
/// it before the parse of the command line. The command line is
/// `iris session-hold <data dir> <hold args>`: the helper builds the
/// isolation with the same data folder as the engine.
pub const SESSION_HOLD: &str = "session-hold";

/// Serve the engine on `addr` until the process gets Ctrl-C. A thread runs
/// the sync schedule.
pub fn serve(store: Arc<Store>, addr: SocketAddr) -> std::io::Result<()> {
    check_loopback(addr)?;
    let gmail = gmail_config().map_err(std::io::Error::other)?;
    // The engine lock and the socket come first: a second engine stops here.
    let bound = socket::bind(store.dir())?;
    let api_store = store.clone();
    start_sessions(store.clone())?;
    let sync = engine::Engine::new(store, gmail, std::env::var("IRIS_TEST_TOKEN_URI").ok());
    std::thread::Builder::new()
        .name("sync".into())
        .spawn(move || sync.run())?;

    // One thread: the lightest memory footprint has priority.
    // The time driver: the socket waits at most READ_WAIT for a request.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        // The API checks the Host against the bound address.
        let auth = Arc::new(iris_api::Auth::new(local)?);
        say!("iris: serving on http://{local}");
        say!("iris: commands connect to {}", bound.path.display());
        let socket_store = api_store.clone();
        let commands = async {
            if let Err(e) = socket::serve(&bound, socket_store).await {
                say!("iris: socket: {e}");
            }
            std::future::pending::<()>().await
        };
        // The one-time code of the API sign-in (specification 2, section 8.6).
        // `iris open` asks the socket for more codes.
        let _ = socket::AUTH.set(auth.clone());
        if let Some(url) = auth.sign_in_url() {
            say!("iris: sign-in address of the local API: {url}");
        }
        let web = axum::serve(listener, app(api_store, auth)).with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        // The web server ends at Ctrl-C. Then `bound` drops: the socket file
        // goes, and the lock is released.
        tokio::select! {
            r = web => r,
            () = commands => Ok(()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_program() {
        assert!(version_line().starts_with("iris "));
    }

    #[test]
    fn cli_parses_serve_with_default_addr() {
        let cli = Cli::try_parse_from(["iris", "serve"]).unwrap();
        let Command::Serve { addr } = cli.command else {
            panic!("not serve")
        };
        assert_eq!(addr.to_string(), DEFAULT_ADDR);
    }

    #[test]
    fn cli_parses_service_install_and_remove() {
        for (arg, install) in [("install", true), ("remove", false)] {
            let cli = Cli::try_parse_from(["iris", "service", arg]).unwrap();
            let Command::Service { action } = cli.command else {
                panic!("not service");
            };
            assert_eq!(matches!(action, ServiceAction::Install), install);
        }
    }

    #[test]
    fn serve_refuses_an_address_that_is_not_loopback() {
        let dir = std::env::temp_dir().join(format!("iris-serve-open-{}", std::process::id()));
        let store = Arc::new(Store::open(&dir).unwrap());
        for addr in ["0.0.0.0:0", "192.0.2.1:4800", "[::]:0"] {
            let err = serve(store.clone(), addr.parse().unwrap()).unwrap_err();
            assert!(
                err.to_string().contains("not a loopback address"),
                "{addr}: {err}"
            );
        }
    }

    #[test]
    fn only_a_loopback_url_is_local() {
        assert!(is_local_url("http://127.0.0.1:4800"));
        assert!(is_local_url("http://127.0.0.1:4800/gmail/v1"));
        for bad in [
            "http://127.0.0.1:80@evil.example",
            "http://127.0.0.1:80@evil.example/gmail/v1",
            "http://127.0.0.1:/x",
            "http://127.0.0.1:80x/",
            "http://127.0.0.1.evil.example:80/",
            "https://evil.example/",
        ] {
            assert!(!is_local_url(bad), "{bad}");
        }
    }

    #[test]
    fn cli_parses_an_imap_address_that_starts_with_a_dash_after_the_end_of_options() {
        // The web client shows this command for such an address (issue 694).
        let cli =
            Cli::try_parse_from(["iris", "account", "add-imap", "--", "-x@example.org"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Account {
                command: AccountCommand::AddImap { address, .. }
            } if address == "-x@example.org"
        ));
        assert!(Cli::try_parse_from(["iris", "account", "add-imap", "-x@example.org"]).is_err());
    }

    #[test]
    fn cli_parses_the_data_folder_after_the_command() {
        let cli = Cli::try_parse_from(["iris", "backup", "/tmp/b", "--data", "/tmp/d"]).unwrap();
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/d")));
        let cli = Cli::try_parse_from(["iris", "export", "/tmp/e"]).unwrap();
        assert!(matches!(cli.command, Command::Export { path } if path.as_os_str() == "/tmp/e"));
    }
}

/// The state of `iris stop --show`: only a complete stop says that no
/// mail leaves, as in [`stop_text`].
fn stop_state(now: Option<&iris_apply::Stopped>) -> String {
    match now {
        None => "Iris runs. `iris stop` stops all agent work.".into(),
        Some(s) if s.live == 0 && !s.busy => format!(
            "Stopped since {}. No action applies and no mail leaves until `iris resume`.",
            iris_api::rfc3339(s.since)
        ),
        Some(s) => {
            let text = stop_text(s);
            format!(
                "The stop switch is set since {}.\n{}",
                iris_api::rfc3339(s.since),
                text.split_once('\n')
                    .map_or(text.as_str(), |(_, rest)| rest)
            )
        }
    }
}

/// The answer of `iris stop`. The stop is complete only with no live
/// session and no Gmail call in progress (docs/specs/2026-11-17-stop.md).
/// Only a complete stop says that no mail leaves.
fn stop_text(s: &iris_apply::Stopped) -> String {
    if s.live == 0 && !s.busy {
        return format!(
            "Stopped. {} session(s) ended. No action applies and no mail leaves until `iris resume`.",
            s.sessions
        );
    }
    let mut text = format!(
        "The stop is not complete. {} session(s) ended, and no new action or send starts.",
        s.sessions
    );
    if s.busy {
        text.push_str(
            "\nA Gmail call was in progress, and it can still finish: one change or one send. \
             The ledger shows its result.",
        );
    }
    if s.live > 0 {
        text.push_str(&format!(
            "\n{} session(s) still stop. `iris job session <j-id>` shows each.",
            s.live
        ));
    }
    text.push_str("\nRun `iris stop` again.");
    text
}

/// What stays after a removal. The question says it before the removal,
/// and the result says it again.
const REMOVE_KEEPS: &str = "A backup copy that you made, the copies iris.db.v*.bak in the \
     data folder and the notes of jobs still hold this mail.";

/// The question of `iris account remove`, before the removal.
fn remove_question(address: &str) -> String {
    format!(
        "This removes {address} and its mail from Iris on this machine. \
         The mailbox at the provider does not change. The files of agents in the \
         workspaces of its jobs go. {REMOVE_KEEPS} Type the address to remove it: "
    )
}

/// Ask for the address of the account again before a removal. A wrong
/// answer refuses the removal.
fn remove_confirm(address: &str, input: &mut impl BufRead) -> Result<(), String> {
    eprint!("{}", remove_question(address));
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.trim() != address {
        return Err("The account stays.".into());
    }
    Ok(())
}

/// `iris account remove`: the removal, then the delete of the mail. The
/// delete waits for a sync or a backup that holds its lock.
fn account_remove(store: &Store, address: &str) -> Result<String, String> {
    let id = store
        .remove_account(address, engine::now_ms())
        .map_err(|e| e.to_string())?;
    let done = engine::delete_removed(store, Some(id)).map_err(|e| e.to_string())?;
    let state: Option<String> = store
        .read(|c| Ok(c.query_row("SELECT state FROM account WHERE id = ?1", [id], |r| r.get(0))?))
        .map_err(|e| e.to_string())?;
    let first = if state.as_deref() == Some(iris_store::STATE_REMOVED) {
        format!(
            "iris: removed {address} and its mail ({} messages).",
            done.map_or(0, |d| d.messages)
        )
    } else {
        // A busy checkpoint: the next engine tick ends the delete.
        format!("iris: removed {address}. `iris serve` ends the delete of its mail.")
    };
    Ok(format!("{first}\niris: {REMOVE_KEEPS}\n"))
}

/// The confirmation of `iris sending on`: a terminal and a typed "yes". So
/// a script with no terminal does not turn sending on by mistake.
/// `terminal` is true when standard input is a terminal: a script or an
/// agent without a terminal cannot turn sending on.

fn sending_on_confirm(terminal: bool, input: &mut impl BufRead) -> Result<(), String> {
    if !terminal {
        return Err("`iris sending on` needs a terminal: standard input is not a TTY".into());
    }
    eprint!(
        "Approved sends leave Gmail after their hold. \
         Check Gmail on this machine first (issue #302). Type yes to turn sending on: "
    );
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.trim() != "yes" {
        return Err("Sending stays off.".into());
    }
    Ok(())
}

/// `iris cancel d-4`: cancel the open send of the draft.
fn cancel_send(store: &Store, id: &str, now: i64) -> Result<&'static str, String> {
    let draft = match iris_store::parse_id(id) {
        Some((iris_store::Kind::Draft, n)) => n,
        _ => return Err(format!("{id} is not a draft ID such as d-4")),
    };
    match iris_changeset::cancel_draft(store, draft, now).map_err(|e| e.to_string())? {
        None => Err(format!("{id} has no send")),
        Some(iris_changeset::Cancel::Done) => Ok("Cancelled. The send does not leave."),
        Some(iris_changeset::Cancel::Started) => {
            Err("The send started. It cannot be cancelled.".into())
        }
        Some(iris_changeset::Cancel::NotOpen) => Err(format!("{id} has no send that waits")),
    }
}

#[cfg(test)]
mod outbox_tests {
    use super::*;

    /// `iris cancel` takes only a draft ID, and says so for a draft with no
    /// send.
    #[test]
    fn cancel_names_a_draft() {
        let dir = std::env::temp_dir().join(format!("iris-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        assert!(cancel_send(&store, "t-4", 0)
            .unwrap_err()
            .contains("not a draft ID"));
        assert!(cancel_send(&store, "d-x", 0)
            .unwrap_err()
            .contains("not a draft ID"));
        assert_eq!(
            cancel_send(&store, "d-4", 0).unwrap_err(),
            "d-4 has no send"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only the typed word "yes" at a terminal turns sending on.
    #[test]
    fn stop_text_says_when_the_stop_is_not_complete() {
        let mut s = iris_apply::Stopped {
            since: 0,
            sessions: 2,
            live: 0,
            busy: false,
        };
        assert!(stop_text(&s).contains("no mail leaves"));
        s.live = 1;
        let t = stop_text(&s);
        assert!(
            t.contains("not complete") && t.contains("1 session(s) still stop"),
            "{t}"
        );
        assert!(!t.contains("no mail leaves"), "{t}");
        s.live = 0;
        s.busy = true;
        let t = stop_text(&s);
        assert!(
            t.contains("not complete") && t.contains("can still finish"),
            "{t}"
        );
        assert!(!t.contains("no mail leaves"), "{t}");
    }

    #[test]
    fn only_yes_turns_sending_on() {
        assert_eq!(sending_on_confirm(true, &mut "yes\n".as_bytes()), Ok(()));
        assert_eq!(sending_on_confirm(true, &mut "  yes \n".as_bytes()), Ok(()));
        for answer in ["no\n", "\n", "", "yes please\n", "y\n", "YES!\n"] {
            assert_eq!(
                sending_on_confirm(true, &mut answer.as_bytes()),
                Err("Sending stays off.".to_string()),
                "{answer:?}"
            );
        }
        let r = sending_on_confirm(false, &mut "yes\n".as_bytes());
        assert!(r.unwrap_err().contains("needs a terminal"));
    }

    #[test]
    fn only_the_address_removes_an_account() {
        let a = "a@example.org";
        assert_eq!(remove_confirm(a, &mut "a@example.org\n".as_bytes()), Ok(()));
        for answer in ["yes\n", "\n", "", "b@example.org\n", "A@example.org\n"] {
            assert_eq!(
                remove_confirm(a, &mut answer.as_bytes()),
                Err("The account stays.".to_string()),
                "{answer:?}"
            );
        }
        // The question says what stays before the removal (spec, "Command").
        let q = remove_question(a);
        for word in ["backup copy", "iris.db.v*.bak", "notes of jobs", "files of agents", "provider does not change"] {
            assert!(q.contains(word), "{word}: {q}");
        }
    }

    /// A store with account 1 and draft d-1 (a new mail that the apply
    /// made), and a function that submits and approves a send of d-1.
    fn draft_store(name: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("iris-cancel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        store.add_account("gmail", "owner@example.com").unwrap();
        store
            .write(|tx| {
                tx.execute_batch(
                    "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0);
                     INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                       VALUES (1, 1, 1, 'draft', '{\"accounts\":[\"a-1\"]}', '{}');",
                )?;
                let c = iris_compose::Content {
                    to: vec!["priya@example.com".into()],
                    cc: Vec::new(),
                    subject: "Plan".into(),
                    body: "Hello".into(),
                }
                .check()
                .unwrap();
                let raw =
                    iris_compose::compose("owner@example.com", "iris.1@example.com", &c, None, 1);
                tx.execute(
                    "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash,
                                       raw, action_id, created_at, updated_at)
                     VALUES (1, 'r1', 'iris.1@example.com', '{}', ?1, ?2, 1, 0, 0)",
                    rusqlite::params![iris_compose::hash(&c, None), raw],
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    /// Submit a send of d-1 as the person, and approve it. Gives its ID.
    fn approved_send(store: &Store) -> i64 {
        let who = iris_changeset::Author {
            kind: iris_changeset::AuthorKind::Person,
            name: None,
            session: None,
            job: None,
        };
        let now = engine::now_ms();
        let cs = iris_changeset::create(store, &who, now).unwrap();
        let verb = iris_changeset::Verb::Send;
        iris_changeset::add_action(store, cs, &who, verb, &["d-1"], &Default::default(), None)
            .unwrap();
        iris_changeset::submit(store, cs, &who, "Send.", now).unwrap();
        store
            .write(|tx| {
                let id: i64 = tx.query_row(
                    "SELECT id FROM changeset_action WHERE changeset_id = ?1",
                    [cs],
                    |r| r.get(0),
                )?;
                let n = tx.execute(
                    "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                       decided_at = ?2 WHERE id = ?1 AND state IN ('proposed', 'in_review')",
                    rusqlite::params![id, now],
                )?;
                assert_eq!(n, 1, "the send was not proposed");
                Ok(id)
            })
            .unwrap()
    }

    fn decision(store: &Store, action: i64) -> (String, String, String) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state, decided_by, coalesce(reason, '') FROM changeset_action
                     WHERE id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap()
    }

    /// `iris cancel d-1` cancels an approved send and a held send of the
    /// draft. A second cancel finds no send that waits.
    #[test]
    fn cancel_removes_the_send_of_a_draft() {
        let (dir, store) = draft_store("send");
        let want = (
            "denied".to_string(),
            "person".to_string(),
            iris_changeset::CANCELLED.to_string(),
        );
        let s = approved_send(&store);
        let now = engine::now_ms();
        assert_eq!(
            cancel_send(&store, "d-1", now),
            Ok("Cancelled. The send does not leave.")
        );
        assert_eq!(decision(&store, s), want);
        assert_eq!(
            cancel_send(&store, "d-1", now).unwrap_err(),
            "d-1 has no send that waits"
        );
        // A held send: the cancel finds the open send, not the old one.
        let s = approved_send(&store);
        assert_eq!(iris_changeset::hold(&store, 1, now).unwrap(), 1);
        assert_eq!(decision(&store, s).0, "held");
        assert_eq!(
            cancel_send(&store, "d-1", now),
            Ok("Cancelled. The send does not leave.")
        );
        assert_eq!(decision(&store, s), want);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
