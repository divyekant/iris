//! The commands of cost control (docs/specs/2026-11-16-cost-control.md,
//! "Commands"): `iris model set`, `iris model show`, and `iris understand`
//! with the estimate, the approval of a budget and the run.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use iris_store::Store;
use iris_understand::batch::{self, Limits, Report};
use iris_understand::budget::{model_row, ModelRow};
use rusqlite::{params, OptionalExtension};

/// The most USD for one million tokens.
pub const MAX_PRICE_USD: u64 = 1_000;
/// The most USD of one budget.
pub const MAX_BUDGET_USD: u64 = 100_000;
/// How long an estimate receipt approves a budget.
pub const RECEIPT_MS: i64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_TIMEOUT_S: u32 = 120;

/// A note that each command that shows a cost prints.
const PRICE_NOTE: &str = "Iris computes the cost from the price that you gave. \
If the price is too low, the provider can charge more.";

/// A decimal USD amount from 0 to `max_usd`, with at most 6 decimals, as
/// micro-USD. It refuses a sign, an exponent, `inf` and `NaN`.
pub fn parse_usd(text: &str, max_usd: u64) -> Result<u64, String> {
    let bad =
        || format!("`{text}` is not an amount from 0 to {max_usd} USD with at most 6 decimals");
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || whole.len() > 12 || frac.len() > 6 || !digits(whole) || !digits(frac) {
        return Err(bad());
    }
    if text.ends_with('.') {
        return Err(bad());
    }
    let whole: u64 = whole.parse().map_err(|_| bad())?;
    let frac: u64 = format!("{frac:0<6}").parse().map_err(|_| bad())?;
    let micro = whole
        .checked_mul(1_000_000)
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(bad)?;
    if micro > max_usd * 1_000_000 {
        return Err(bad());
    }
    Ok(micro)
}

/// Micro-USD as USD text, for example `1.25`.
pub fn usd(micro: i64) -> String {
    let sign = if micro < 0 { "-" } else { "" };
    let m = micro.unsigned_abs();
    let frac = format!("{:06}", m % 1_000_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{sign}{}", m / 1_000_000)
    } else {
        format!("{sign}{}.{frac}", m / 1_000_000)
    }
}

/// The parts of a local model URL: the host (`localhost` or a loopback IP
/// literal, IPv6 in brackets), the port and the path with the query.
struct LocalUrl {
    host: String,
    port: u16,
    path: String,
}

/// Parses a local model URL with `http::Uri`. Only `http` is accepted: TLS
/// on loopback adds nothing, and the run connects to an IP literal (see
/// `pin_local_url`), which a certificate for `localhost` does not match.
fn parse_local(url: &str) -> Result<LocalUrl, String> {
    // No error holds the URL text: a user part, a query or a fragment can
    // hold a secret, and the command prints the error.
    let refuse = || {
        "the model URL is not on this machine: use http:// with localhost, 127.0.0.1 or [::1]"
            .to_string()
    };
    // The model client adds `/chat/completions` to the URL text, so a query
    // or a fragment would change the request path (#135).
    if url.contains(['?', '#']) {
        return Err(
            "the model URL has a query or a fragment: give the base URL only, \
             for example http://localhost:8080/v1"
                .to_string(),
        );
    }
    let uri: http::Uri = url.parse().map_err(|_| refuse())?;
    if uri.scheme_str() != Some("http") {
        return Err(refuse());
    }
    let authority = uri.authority().ok_or_else(refuse)?;
    if authority.as_str().contains('@') {
        return Err(refuse());
    }
    let host = authority.host().to_ascii_lowercase();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = host == "localhost" || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        return Err(refuse());
    }
    // `Authority::port` gives `None` for a port that is not a u16, so read
    // the port text after the host.
    let port_text = &authority.as_str()[authority.host().len()..];
    let port = match port_text.strip_prefix(':') {
        None if port_text.is_empty() => 80,
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            p.parse::<u16>().map_err(|_| refuse())?
        }
        _ => return Err(refuse()),
    };
    let path = uri.path_and_query().map_or("", |p| p.as_str()).to_string();
    Ok(LocalUrl { host, port, path })
}

/// The host of a local model URL, if it is `localhost` or a loopback IP
/// literal (`127.0.0.0/8`, `::1`). Any other host gives an error, so the
/// mail text stays on the machine.
pub fn local_host(url: &str) -> Result<String, String> {
    parse_local(url).map(|u| u.host)
}

/// The local URL with its host replaced by a loopback IP literal. Each
/// address of the host must be a loopback address, also after the name
/// lookup of `localhost`. The client connects to the checked address only,
/// so a later lookup cannot give another address.
fn pin_local_url(url: &str) -> Result<String, String> {
    let u = parse_local(url)?;
    let bare = u.host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<_> = (bare, u.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {}: {e}", u.host))?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !a.ip().is_loopback()) {
        return Err(format!(
            "{} has an address that is not on this machine",
            u.host
        ));
    }
    let ip = match reachable(&addrs).ip() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    Ok(format!("http://{ip}:{}{}", u.port, u.path))
}

/// The first address of the checked loopback set that accepts a TCP
/// connection, else the first address. `localhost` can give `::1` and
/// `127.0.0.1`, and a server can listen on one of them only.
fn reachable(addrs: &[SocketAddr]) -> SocketAddr {
    if addrs.len() == 1 {
        return addrs[0]; // An IP literal: no probe connection.
    }
    addrs
        .iter()
        .find(|a| TcpStream::connect_timeout(a, Duration::from_secs(1)).is_ok())
        .unwrap_or(&addrs[0])
        .to_owned()
}

/// What `iris model set` writes.
pub struct NewModel {
    pub local: bool,
    pub base_url: String,
    pub model: String,
    /// Micro-USD for one million tokens; `None` for a local model.
    pub price: Option<(u64, u64)>,
    pub api_key: Option<String>,
    pub max_tokens: u32,
    pub timeout_s: u32,
}

/// Writes the model row with a larger version and no budget. It takes the
/// run lock, so it fails while a run is in progress.
pub fn set_model(store: &Store, m: &NewModel) -> Result<i64, String> {
    if m.local {
        local_host(&m.base_url)?;
    }
    let _lock = batch::lock(store).map_err(|e| e.to_string())?;
    let sealed = match &m.api_key {
        Some(k) => Some(store.seal(k.as_bytes()).map_err(|e| e.to_string())?),
        None => None,
    };
    store
        .write(|tx| {
            let version: i64 = tx.query_row(
                "SELECT coalesce(max(version), 0) + 1 FROM model_setting",
                [],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO model_setting(id, provider, base_url, model, version,
                   price_in, price_out, api_key, max_tokens, timeout_s, budget_micro_usd)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
                params![
                    if m.local { "local" } else { "anthropic" },
                    m.base_url,
                    m.model,
                    version,
                    m.price.map(|p| p.0 as i64),
                    m.price.map(|p| p.1 as i64),
                    sealed,
                    m.max_tokens,
                    m.timeout_s
                ],
            )?;
            Ok(version)
        })
        .map_err(|e| e.to_string())
}

/// `iris model set anthropic`: reads the key from standard input, with no
/// echo on a terminal. The key is never an argument.
pub fn read_key() -> Result<String, String> {
    let stdin = std::io::stdin();
    // The guard turns the echo on again on each path out of this function.
    let _echo_off = if stdin.is_terminal() {
        let _ = write!(std::io::stderr(), "API key (no echo): ");
        let _ = std::io::stderr().flush();
        Some(EchoOff::new()?)
    } else {
        None
    };
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let key = line.trim().to_string();
    if key.is_empty() {
        return Err("no API key on standard input".into());
    }
    Ok(key)
}

/// The echo of the terminal is off while this value lives. It uses `stty`
/// (macOS and Linux). If `stty` fails, no key is read.
struct EchoOff;

impl EchoOff {
    fn new() -> Result<EchoOff, String> {
        if stty("-echo") {
            Ok(EchoOff)
        } else {
            Err("cannot turn off the echo of the terminal, so the key is not read".into())
        }
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        stty("echo");
        let _ = writeln!(std::io::stderr());
    }
}

fn stty(arg: &str) -> bool {
    std::process::Command::new("stty")
        .arg(arg)
        .stdin(std::process::Stdio::inherit())
        .status()
        .is_ok_and(|s| s.success())
}

/// `iris model show`: the settings and the budget left, never the key.
pub fn show_model(store: &Store) -> Result<String, String> {
    let row = store
        .read(|c| Ok(model_row(c)?))
        .map_err(|e| e.to_string())?;
    let Some(r) = row else {
        return Ok("No model is set. Use `iris model set`.".into());
    };
    let mut out = format!(
        "Model: {} ({})\nURL: {}\nVersion: {}\nMax tokens: {}\nTime limit: {} s\nKey: {}\n",
        r.model,
        r.provider,
        r.base_url,
        r.version,
        r.max_tokens,
        r.timeout_s,
        if r.api_key.is_some() { "set" } else { "none" },
    );
    match r.price {
        Some(p) => {
            out += &format!(
                "Price: {} USD in, {} USD out, for one million tokens\n",
                usd(p.input_micro_usd_per_mtok as i64),
                usd(p.output_micro_usd_per_mtok as i64)
            );
            out += &match r.budget_micro_usd {
                Some(b) => format!("Budget left: {} USD\n", usd(b)),
                None => "Budget: none approved. Paid calls are refused.\n".into(),
            };
            out += PRICE_NOTE;
        }
        None => out += "Price: none (a local model needs no budget)",
    }
    Ok(out)
}

/// 16 random bytes as hex, from the random device of the system.
fn random_code() -> Result<String, String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("no random bytes: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// `iris understand --estimate`: the estimate, and a receipt whose code
/// approves a budget for this model version for 24 hours. No model call.
pub fn estimate(store: &Store, now_ms: i64) -> Result<String, String> {
    let e = batch::estimate(store, now_ms).map_err(|e| e.to_string())?;
    let mut out = format!(
        "Messages to understand: {} ({} in the last 90 days)\nTokens: about {} in, {} out\n",
        e.messages, e.recent, e.input_tokens, e.output_tokens
    );
    let (Some(cost), Some(version)) = (e.cost_micro_usd, e.model_version) else {
        out += if e.model_version.is_some() {
            "Cost: none (a local model needs no budget)"
        } else {
            "No model is set. Use `iris model set`."
        };
        return Ok(out);
    };
    let code = random_code()?;
    store
        .write(|tx| {
            tx.execute(
                "DELETE FROM understand_estimate WHERE created_at < ?1",
                [now_ms - RECEIPT_MS],
            )?;
            tx.execute(
                "INSERT INTO understand_estimate(code, model_version, messages, input_tokens,
                   output_tokens, cost_micro_usd, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    code,
                    version,
                    e.messages as i64,
                    e.input_tokens as i64,
                    e.output_tokens as i64,
                    cost as i64,
                    now_ms
                ],
            )?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    out += &format!(
        "Cost: about {} USD (one call for each message)\n{PRICE_NOTE}\n\n\
         To approve a budget, type in a terminal:\n  iris understand --approve {code} --budget <usd>\n\
         The code works one time, for 24 hours, for this model version.",
        usd(cost as i64)
    );
    Ok(out)
}

/// A receipt of `--estimate`.
#[derive(Debug)]
pub struct Receipt {
    pub messages: i64,
    pub cost_micro_usd: i64,
    pub budget_left: Option<i64>,
}

/// The receipt of `code`, if it approves now: it exists, its model version
/// is the version of the row, and it is not older than 24 hours.
fn receipt(c: &rusqlite::Connection, code: &str, now_ms: i64) -> rusqlite::Result<Option<Receipt>> {
    c.query_row(
        "SELECT e.messages, coalesce(e.cost_micro_usd, 0), m.budget_micro_usd
         FROM understand_estimate e JOIN model_setting m ON m.version = e.model_version
         WHERE e.code = ?1 AND e.created_at >= ?2",
        params![code, now_ms - RECEIPT_MS],
        |r| {
            Ok(Receipt {
                messages: r.get(0)?,
                cost_micro_usd: r.get(1)?,
                budget_left: r.get(2)?,
            })
        },
    )
    .optional()
}

/// Reads the receipt of `code` for the confirmation, or refuses.
pub fn check_receipt(store: &Store, code: &str, now_ms: i64) -> Result<Receipt, String> {
    store
        .read(|c| Ok(receipt(c, code, now_ms)?))
        .map_err(|e| e.to_string())?
        .ok_or_else(refused_code)
}

fn refused_code() -> String {
    "no estimate approves this code: it is unknown, used, older than 24 hours, or for another \
     model version. Run `iris understand --estimate` again"
        .into()
}

/// Sets the budget to `budget` (it replaces what is left) and deletes the
/// receipt, in one write, under the run lock. The checks of the receipt run
/// again in the write.
pub fn approve(store: &Store, code: &str, budget: u64, now_ms: i64) -> Result<(), String> {
    let _lock = batch::lock(store).map_err(|e| e.to_string())?;
    let ok = store
        .write(|tx| {
            if receipt(tx, code, now_ms)?.is_none() {
                return Ok(false);
            }
            tx.execute(
                "UPDATE model_setting SET budget_micro_usd = ?1 WHERE id = 1",
                [budget as i64],
            )?;
            tx.execute("DELETE FROM understand_estimate WHERE code = ?1", [code])?;
            // The approval is the new input of each call that the budget
            // refused.
            batch::clear_waiting(tx)?;
            Ok(true)
        })
        .map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err(refused_code())
    }
}

/// `iris understand --approve`: needs a terminal, shows the receipt and the
/// budget left, and asks for the budget again.
pub fn approve_command(store: &Store, code: &str, budget: &str) -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        return Err(
            "the approval of a budget needs a terminal: standard input is not a TTY".into(),
        );
    }
    let stdin = std::io::stdin();
    confirm(
        store,
        code,
        budget,
        &mut stdin.lock(),
        &mut std::io::stderr(),
        &crate::engine::now_ms,
    )
}

/// The confirmation of `approve_command`, with its input, output and clock.
/// The final check reads the clock again after the input, so a receipt
/// that gets too old while the prompt waits does not approve.
pub fn confirm(
    store: &Store,
    code: &str,
    budget: &str,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    clock: &dyn Fn() -> i64,
) -> Result<String, String> {
    let new = parse_usd(budget, MAX_BUDGET_USD)?;
    let r = check_receipt(store, code, clock())?;
    let _ = writeln!(
        out,
        "Estimate: {} messages, about {} USD.\nBudget left now: {}\nNew budget: {} USD\n{PRICE_NOTE}\nType the new budget again to approve:",
        r.messages,
        usd(r.cost_micro_usd),
        r.budget_left.map_or("none".into(), |b| format!("{} USD", usd(b))),
        usd(new as i64),
    );
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    if parse_usd(line.trim(), MAX_BUDGET_USD) != Ok(new) {
        return Err("the two budgets are not the same: nothing changed".into());
    }
    approve(store, code, new, clock())?;
    Ok(format!("Budget approved: {} USD.", usd(new as i64)))
}

/// Makes the `iris-model` client of the row. A local model must be on this
/// machine, also after the name lookup.
pub fn connect(store: &Store, row: &ModelRow) -> Result<Box<dyn iris_understand::Model>, String> {
    let local = row.provider == "local";
    let base_url = if local {
        pin_local_url(&row.base_url)?
    } else {
        row.base_url.clone()
    };
    let api_key = match &row.api_key {
        Some(sealed) => {
            let plain = store.unseal(sealed).map_err(|e| e.to_string())?;
            let key =
                String::from_utf8(plain).map_err(|_| "the stored key is not text".to_string())?;
            Some(iris_model::ApiKey::new(key))
        }
        None => None,
    };
    let client = iris_model::Client::new(iris_model::Config {
        provider: if local {
            iris_model::Provider::OpenAiCompatible
        } else {
            iris_model::Provider::Anthropic
        },
        base_url,
        model: row.model.clone(),
        api_key,
        max_tokens: row.max_tokens,
        timeout: Duration::from_secs(row.timeout_s.max(1) as u64),
        price: row.price,
    });
    Ok(Box::new(iris_understand::Named {
        client,
        name: row.model.clone(),
    }))
}

/// `iris understand`: one run of the batch runner with the budget left.
pub fn run(store: &Store, limits: &Limits, now_ms: i64) -> Result<String, String> {
    let connect = |row: &ModelRow| connect(store, row);
    let r = batch::run(store, &connect, limits, now_ms).map_err(|e| e.to_string())?;
    report_of(store, &r)
}

/// The result of [`after_sync`].
#[derive(Debug, PartialEq, Eq)]
pub enum AfterSync {
    /// The report of a run that read a message or made a call. `retry` is
    /// true if the run stopped on a model, unknown-cost or store error, so
    /// the same pending mail can need a later run.
    Ran { report: String, retry: bool },
    /// Nothing to do: the run deleted its row.
    Idle,
    /// Another run holds the lock. Run again after it.
    Busy,
}

/// The run after a sync (R1-28), in `iris serve` and `iris mirror sync`:
/// the batch runner with no limit but the budget. An idle run leaves no
/// row. After a run that stopped on a model error, the runner honors the
/// wait of [`batch::retry_after`] for that model version, under its lock
/// (`Limits::wait_after_error`).
pub fn after_sync(store: &Store, now_ms: i64) -> Result<AfterSync, String> {
    let connect = |row: &ModelRow| connect(store, row);
    let limits = Limits {
        keep_idle: false,
        wait_after_error: true,
        ..Limits::default()
    };
    match batch::run(store, &connect, &limits, now_ms) {
        Ok(r) if r.is_idle() => Ok(AfterSync::Idle),
        Ok(r) => Ok(AfterSync::Ran {
            retry: matches!(r.stop.as_deref(), Some("model" | "unknown" | "error")),
            report: report_of(store, &r)?,
        }),
        Err(batch::Error::Busy) => Ok(AfterSync::Busy),
        Err(e) => Err(e.to_string()),
    }
}

/// The proposal step of matters (`docs/specs/2026-11-16-matters.md`):
/// `Gate::Hourly` from `iris serve`, `Gate::Now` from `iris matters
/// propose`. The text says what the step did; `None` when an
/// understanding run holds the lock, so the step did nothing.
pub fn propose_matters(
    store: &Store,
    gate: iris_matters::Gate,
    now_ms: i64,
) -> Result<Option<String>, String> {
    let connect = |row: &ModelRow| connect(store, row);
    let o = match iris_matters::propose(store, &connect, gate, now_ms) {
        Ok(o) => o,
        Err(iris_matters::Error::Busy) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    Ok(Some(matters_line(&o)))
}

/// The line that `iris matters propose` and `iris serve` print for one
/// step: what it did, with the counts of proposals and additions.
fn matters_line(o: &iris_matters::Outcome) -> String {
    use iris_matters::Skip;
    let what = match o.skip {
        Some(Skip::NoModel) => "no model is set".to_string(),
        Some(Skip::Wait) => "the wait after a model error is not over".into(),
        Some(Skip::Hourly) => "the last call is less than 1 hour old".into(),
        Some(Skip::Few) => "the candidates come from fewer than 2 threads".into(),
        Some(Skip::Same) => "the candidates did not change since the last answer".into(),
        Some(Skip::Budget) => "the budget cannot pay the call; approve a budget to go on".into(),
        None if o.failed => "the model call failed; the step tries again after the wait".into(),
        None => format!(
            "{} new proposals, {} items to add, {} dropped",
            o.proposed, o.added, o.dropped
        ),
    };
    format!(
        "Matters: {what}. {} proposals that were not valid now are gone.",
        o.cleaned
    )
}

fn report_of(store: &Store, r: &Report) -> Result<String, String> {
    let left = store
        .read(|c| Ok(model_row(c)?))
        .map_err(|e| e.to_string())?
        .and_then(|m| m.price.and(Some(m.budget_micro_usd)));
    Ok(report_text(r, left))
}

fn report_text(r: &Report, left: Option<Option<i64>>) -> String {
    let stop = r.stop.as_deref().unwrap_or("error");
    let why = match stop {
        "done" => "no message is left that this run could reach",
        "budget" => "the budget cannot pay the next call; approve a budget to go on",
        "no_model" => "no model is set; the free steps ran",
        "cost" => "the --max-cost limit",
        "messages" => "the --max-messages limit",
        "model" => "the model refused the key or the local server failed",
        "unknown" => "a call had an unknown cost; its worst case stays counted",
        _ => "an error",
    };
    let mut out = format!(
        "Run {}: {} messages, {} calls, {} tokens in, {} tokens out\n",
        r.run, r.messages, r.calls, r.input_tokens, r.output_tokens
    );
    if let Some(cost) = r.cost_micro_usd {
        let unknown = r.unknown_cost_micro_usd.unwrap_or(0);
        out += &format!(
            "Cost: {} USD, and {} calls with an unknown cost (at most {} USD)\n",
            usd(cost),
            r.unknown_calls,
            usd(unknown)
        );
    } else if r.unknown_calls > 0 {
        out += &format!("{} calls gave no token counts\n", r.unknown_calls);
    }
    match left {
        Some(Some(b)) if b < 0 => {
            out += &format!(
                "Budget left: {} USD (the provider billed more than the bound)\n",
                usd(b)
            )
        }
        Some(Some(b)) => out += &format!("Budget left: {} USD\n", usd(b)),
        Some(None) => out += "Budget: none approved\n",
        None => {}
    }
    out += &format!("Stop: {stop} ({why})");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matters_line_counts_the_additions() {
        let o = iris_matters::Outcome {
            proposed: 1,
            added: 3,
            dropped: 2,
            cleaned: 4,
            ..Default::default()
        };
        assert_eq!(
            matters_line(&o),
            "Matters: 1 new proposals, 3 items to add, 2 dropped. \
             4 proposals that were not valid now are gone."
        );
    }

    #[test]
    fn amounts_parse_with_bounds() {
        assert_eq!(parse_usd("0", 1000), Ok(0));
        assert_eq!(parse_usd("4", 1000), Ok(4_000_000));
        assert_eq!(parse_usd("0.000001", 1000), Ok(1));
        assert_eq!(parse_usd("1000", 1000), Ok(1_000_000_000));
        assert_eq!(parse_usd("12.5", 1000), Ok(12_500_000));
        for bad in [
            "-1",
            "+1",
            "1e3",
            "1e309",
            "inf",
            "NaN",
            "",
            ".5",
            "5.",
            "1.1234567",
            "1000.000001",
            "1001",
            "1 0",
            "0x10",
            "99999999999999999999",
        ] {
            assert!(parse_usd(bad, 1000).is_err(), "{bad}");
        }
    }

    #[test]
    fn usd_text() {
        assert_eq!(usd(0), "0");
        assert_eq!(usd(1_250_000), "1.25");
        assert_eq!(usd(1), "0.000001");
        assert_eq!(usd(-5_000_000), "-5");
    }

    #[test]
    fn only_a_loopback_host_is_local() {
        for ok in [
            "http://localhost:8080/v1",
            "http://127.0.0.1:1234/v1",
            "http://127.9.9.9/v1",
            "http://[::1]:8443/v1",
            "http://LOCALHOST/v1",
        ] {
            assert!(local_host(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://remote.example/v1",
            "http://10.0.0.1/v1",
            "http://127.0.0.1@remote.example/v1",
            "http://localhost.remote.example/v1",
            "ftp://127.0.0.1/v1",
            "127.0.0.1:8080",
            "http://[::2]/v1",
            "http://0.0.0.0/v1",
            "https://localhost/v1",
            "https://[::1]:8443/v1",
            "http://[::1/v1",
            "http://[::1",
            "http://localhost:/v1",
            "http://localhost:99999/v1",
            "http://[::ffff:10.0.0.1]/v1",
            "http://localhost:8080/v1?token=x",
            "http://localhost:8080/v1?",
            "http://localhost:8080/v1#x",
        ] {
            assert!(local_host(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_local_url_is_pinned_to_a_loopback_literal() {
        assert_eq!(
            pin_local_url("http://127.0.0.1:9/v1").unwrap(),
            "http://127.0.0.1:9/v1"
        );
        assert_eq!(
            pin_local_url("http://[::1]:9/v1").unwrap(),
            "http://[::1]:9/v1"
        );
        // `localhost` can give `::1` first (as on CI runners), and an IPv6
        // host comes in `[]`, as in a URL.
        for url in ["http://[::1]:9/v1", "http://LocalHost:9/v1"] {
            let pinned = pin_local_url(url).unwrap();
            let host = local_host(&pinned).unwrap();
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            assert!(bare.parse::<IpAddr>().unwrap().is_loopback(), "{pinned}");
            assert!(pinned.ends_with(":9/v1"), "{pinned}");
        }
    }

    /// A small xorshift generator with a fixed seed, so a failure repeats.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n.max(1)
        }
        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.below(items.len() as u64) as usize]
        }
    }

    /// Property: a URL is local if and only if its scheme is `http`, it has
    /// no user part, no query and no fragment, and its host is `localhost`
    /// or a loopback literal. A
    /// pinned URL keeps the port and the path and has a loopback IP host.
    #[test]
    fn property_local_urls() {
        // (host text, true if the host is loopback)
        const HOSTS: &[(&str, bool)] = &[
            ("localhost", true),
            ("LocalHost", true),
            ("127.0.0.1", true),
            ("127.200.0.9", true),
            ("[::1]", true),
            ("[0:0:0:0:0:0:0:1]", true),
            ("localhost.evil.example", false),
            ("127.0.0.1.evil.example", false),
            ("evil.example", false),
            ("10.0.0.1", false),
            ("0.0.0.0", false),
            ("[::]", false),
            ("[::ffff:127.0.0.1]", false),
            ("[::1", false),
            ("", false),
        ];
        const SCHEMES: &[&str] = &["http://", "https://", "ftp://", "", "HTTP://"];
        const USERS: &[&str] = &["", "", "", "user@", "127.0.0.1@", "a:b@"];
        const PORTS: &[&str] = &["", ":1", ":8080", ":65535", ":", ":65536", ":x"];
        const PATHS: &[&str] = &["", "/", "/v1", "/v1?x=1", "/a/b#f", "/@evil.example"];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..20_000 {
            let (host, loopback) = HOSTS[rng.below(HOSTS.len() as u64) as usize];
            let scheme = rng.pick(SCHEMES);
            let user = rng.pick(USERS);
            let port = rng.pick(PORTS);
            let path = rng.pick(PATHS);
            let url = format!("{scheme}{user}{host}{port}{path}");
            let port_ok = !matches!(port, ":" | ":65536" | ":x");
            let scheme_ok = scheme.eq_ignore_ascii_case("http://");
            let plain = !path.contains(['?', '#']);
            let expect = loopback && scheme_ok && user.is_empty() && port_ok && plain;
            let got = parse_local(&url);
            assert_eq!(got.is_ok(), expect, "{url}");
            if let Err(e) = &got {
                // No secret of the URL goes to the error text.
                for part in ["x=1", "#f", "a:b", "127.0.0.1@", "user@", "evil"] {
                    assert!(!e.contains(part), "{url}: {e}");
                }
            }
            if let Ok(u) = got {
                let bare = u.host.trim_start_matches('[').trim_end_matches(']');
                assert!(u.host == "localhost" || bare.parse::<IpAddr>().unwrap().is_loopback());
                if u.host != "localhost" {
                    let pinned = pin_local_url(&url).unwrap();
                    let p = parse_local(&pinned).unwrap();
                    let ip = |h: &str| {
                        h.trim_start_matches('[')
                            .trim_end_matches(']')
                            .parse::<IpAddr>()
                            .unwrap()
                    };
                    assert_eq!(ip(&p.host), ip(&u.host), "{url}");
                    assert_eq!((p.port, p.path), (u.port, u.path), "{url}");
                }
            }
        }
        // Random bytes never panic, and each accepted URL has a loopback host.
        let bytes = b"htp:/[]@.:1lochst?#%0";
        for _ in 0..20_000 {
            let n = rng.below(24) as usize;
            let s: String = (0..n)
                .map(|_| bytes[rng.below(bytes.len() as u64) as usize] as char)
                .collect();
            if let Ok(h) = local_host(&s) {
                let bare = h.trim_start_matches('[').trim_end_matches(']');
                assert!(
                    h == "localhost" || bare.parse::<IpAddr>().unwrap().is_loopback(),
                    "{s}"
                );
            }
        }
    }

    /// Property: each amount in micro-USD gives back the same amount through
    /// its text, inside the bound only; random text gives an amount inside
    /// the bound or an error.
    #[test]
    fn property_amounts() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..20_000 {
            let max = rng.below(100_001);
            let micro = rng.below(max * 1_000_000 + 2_000_000);
            let got = parse_usd(&usd(micro as i64), max);
            if micro <= max * 1_000_000 {
                assert_eq!(got, Ok(micro), "{micro} {max}");
            } else {
                assert!(got.is_err(), "{micro} {max}");
            }
        }
        let chars = b"0123456789.-+e ";
        for _ in 0..20_000 {
            let n = rng.below(16) as usize;
            let s: String = (0..n)
                .map(|_| chars[rng.below(chars.len() as u64) as usize] as char)
                .collect();
            if let Ok(v) = parse_usd(&s, 1000) {
                assert!(v <= 1_000_000_000, "{s}");
                assert!(s.bytes().all(|b| b.is_ascii_digit() || b == b'.'), "{s}");
                assert_eq!(parse_usd(&usd(v as i64), 1000), Ok(v), "{s}");
            }
        }
    }

    #[test]
    fn the_pinned_address_is_one_that_listens() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // 127.0.0.2 is loopback too, and nothing listens there on this port.
        let dead: SocketAddr = format!("127.0.0.2:{port}").parse().unwrap();
        let live: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        assert_eq!(reachable(&[dead, live]), live);
        assert_eq!(reachable(&[live, dead]), live);
        // Another test can bind the freed port on 127.0.0.1 (#325), so the
        // case "none listens" uses two addresses where no test binds.
        let other: SocketAddr = format!("127.0.0.3:{port}").parse().unwrap();
        assert_eq!(reachable(&[dead, other]), dead, "none listens: the first");
        drop(l);
    }
}
