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
    /// Cancel the send of a draft (`d-4`) while it waits in the outbox.
    Cancel { id: String },
    /// Close a commitment (`c-7`) in a draft changeset. Needs the engine.
    Close {
        id: String,
        /// The reason. The default is "Done.".
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        to: Draft,
    },
    /// Decline a commitment in a draft changeset. Needs the engine.
    Decline {
        id: String,
        /// The reason. The default is "Declined.".
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
    /// Watch for a mail: one notice when a new mail matches a search.
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
    /// The matters that Iris proposes and that you confirmed. Prints the
    /// JSON of the local API (`/api/matters`).
    Matters {
        #[command(subcommand)]
        command: Option<MattersCommand>,
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
    // The verbs that change changesets go through the engine socket, never
    // through the store (specification 2, section 8.1).
    let data = engine::data_dir(cli.data.clone()).map_err(|e| socket::Failure {
        code: socket::code::ERROR,
        error: e.to_string(),
    });
    if let Some((verb, targets, args, to)) = action(&cli.command)? {
        let mut request = serde_json::json!({ "verb": verb, "targets": targets, "args": args });
        if let Some(cs) = &to.changeset {
            request["changeset"] = cs.clone().into();
        }
        if let Some(why) = &to.why {
            request["why"] = why.clone().into();
        }
        let r = socket::request(&data?, "add", request)?;
        println!("{} {}", r["changeset"].as_str().unwrap_or_default(), r["n"]);
        return Ok(());
    }
    if let Command::Submit { id, why, json } = &cli.command {
        let doc = socket::request(
            &data?,
            "submit",
            serde_json::json!({ "changeset": id, "why": why }),
        )?;
        print!("{}", changeset_text(&doc, *json));
        return match submit_code(&doc) {
            0 => Ok(()),
            code => Err(socket::Failure {
                code,
                error: format!("{id}: not each action is applied (exit code {code})"),
            }),
        };
    }
    let text = match &cli.command {
        Command::Changeset { id, drafts, json } => {
            let dir = data?;
            if *drafts {
                let r = socket::request(&dir, "drafts", serde_json::json!({}))?;
                Some(drafts_text(&r, *json))
            } else {
                let id = id.as_deref().unwrap_or_default();
                let r = socket::request(&dir, "show", serde_json::json!({ "changeset": id }))?;
                Some(changeset_text(&r, *json))
            }
        }
        Command::Withdraw { id, json } => {
            let r = socket::request(&data?, "withdraw", serde_json::json!({ "changeset": id }))?;
            Some(changeset_text(&r, *json))
        }
        Command::Revise { id, json } => {
            let r = socket::request(&data?, "revise", serde_json::json!({ "changeset": id }))?;
            Some(changeset_text(&r, *json))
        }
        _ => None,
    };
    if let Some(text) = text {
        print!("{text}");
        return Ok(());
    }
    run_store(cli).map_err(|error| socket::Failure {
        code: socket::code::ERROR,
        error,
    })
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
                .map_err(|e| format!("attention: {e}"))
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
            command: AccountCommand::Add { client, paste },
        } => account_add(&open()?, client, *paste),
        Command::Mirror { command } => mirror(Arc::new(open()?), command),
        Command::Status { json } => print(verbs::status(&open()?, *json)),
        Command::Search { text, limit, json } => {
            print(verbs::search(&open()?, text, *limit, *json))
        }
        Command::Show { id, json } => print(verbs::show(&open()?, id, *json)),
        Command::Watch { command } => print(verbs::watch(&open()?, command)),
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
        | Command::Send { .. } => unreachable!("run() sends these verbs to the socket"),
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

/// `iris mirror sync|verify`: each account that has a sign-in, one at a time.
/// If another process syncs an account, that account gives an error.
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
        let mut worker = engine::Worker::new(
            store.clone(),
            iris_gmail::Gmail::new(config.clone()),
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
fn account_add(store: &Store, client_path: &std::path::Path, paste: bool) -> Result<(), String> {
    let json = std::fs::read(client_path).map_err(|e| format!("{}: {e}", client_path.display()))?;
    let client = test_token_uri(iris_auth::Client::from_json(&json).map_err(|e| e.to_string())?)?;
    // Check the Gmail settings before the sign-in, so a bad setting fails
    // before a token exists.
    let gmail = gmail()?;
    let sign_in = client.sign_in().map_err(|e| e.to_string())?;
    say!(
        "iris: open this address in a browser and sign in:\n{}",
        sign_in.url()
    );
    let tokens = if paste {
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
    .map_err(|e| e.to_string())?;
    let (_, address) =
        engine::save_sign_in(store, &gmail, &json, &tokens).map_err(|e| e.to_string())?;
    println!("iris: added {address}");
    if tokens.refresh_expires_in.is_some() {
        say!(
            "iris: caution: the OAuth client is in the \"Testing\" status, so the sign-in \
             ends after 7 days. Publish the app to \"In production\"."
        );
    }
    Ok(())
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

/// Serve the engine on `addr` until the process gets Ctrl-C. A thread runs
/// the sync schedule.
pub fn serve(store: Arc<Store>, addr: SocketAddr) -> std::io::Result<()> {
    check_loopback(addr)?;
    let gmail = gmail_config().map_err(std::io::Error::other)?;
    // The engine lock and the socket come first: a second engine stops here.
    let bound = socket::bind(store.dir())?;
    let api_store = store.clone();
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
    fn cli_parses_the_data_folder_after_the_command() {
        let cli = Cli::try_parse_from(["iris", "backup", "/tmp/b", "--data", "/tmp/d"]).unwrap();
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/d")));
        let cli = Cli::try_parse_from(["iris", "export", "/tmp/e"]).unwrap();
        assert!(matches!(cli.command, Command::Export { path } if path.as_os_str() == "/tmp/e"));
    }
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
    let action: Option<i64> = store
        .read(|c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row(
                "SELECT a.id FROM action_draft ad JOIN changeset_action a ON a.id = ad.action_id
                 WHERE ad.draft_id = ?1 AND a.verb = 'send'
                 ORDER BY a.state IN ('approved', 'held') DESC, a.id DESC LIMIT 1",
                [draft],
                |r| r.get(0),
            )
            .optional()?)
        })
        .map_err(|e| e.to_string())?;
    let Some(action) = action else {
        return Err(format!("{id} has no send"));
    };
    match iris_changeset::cancel(store, action, now).map_err(|e| e.to_string())? {
        iris_changeset::Cancel::Done => Ok("Cancelled. The send does not leave."),
        iris_changeset::Cancel::Started => Err("The send started. It cannot be cancelled.".into()),
        iris_changeset::Cancel::NotOpen => Err(format!("{id} has no send that waits")),
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
