//! The commands of cost control (docs/specs/2026-11-16-cost-control.md,
//! "Commands"): `iris model set`, `iris model show`, and `iris understand`
//! with the estimate, the approval of a budget and the run.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
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

/// The host of an `http` or `https` URL, if it is `localhost` or a loopback
/// IP literal (`127.0.0.0/8`, `::1`). Any other host gives an error, so the
/// mail text stays on the machine.
pub fn local_host(url: &str) -> Result<String, String> {
    let refuse = || format!("`{url}` is not on this machine: use localhost, 127.0.0.1 or [::1]");
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(refuse)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(refuse());
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if loopback {
        Ok(host.to_ascii_lowercase())
    } else {
        Err(refuse())
    }
}

/// The local URL with its host replaced by a loopback IP literal. Each
/// address of the host must be a loopback address, also after the name
/// lookup of `localhost`. The client connects to the checked address only,
/// so a later lookup cannot give another address.
fn pin_local_url(url: &str) -> Result<String, String> {
    let host = local_host(url)?;
    let authority = url.split("://").nth(1).unwrap_or_default();
    let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
    let https = url.starts_with("https://");
    let port = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once("]:").map(|(_, p)| p),
        None => authority.rsplit_once(':').map(|(_, p)| p),
    };
    let port = match port {
        Some(p) => p
            .parse::<u16>()
            .map_err(|_| format!("`{url}` has a bad port"))?,
        None if https => 443,
        None => 80,
    };
    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {host}: {e}"))?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !a.ip().is_loopback()) {
        return Err(format!("{host} has an address that is not on this machine"));
    }
    if host != "localhost" {
        return Ok(url.to_string());
    }
    let ip = match addrs[0].ip() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    let (scheme, rest) = url.split_once("://").unwrap_or_default();
    // `local_host` accepted the URL, so the authority starts with the host.
    Ok(format!("{scheme}://{ip}{}", &rest["localhost".len()..]))
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
    let tty = stdin.is_terminal();
    if tty {
        let _ = write!(std::io::stderr(), "API key (no echo): ");
        let _ = std::io::stderr().flush();
        echo(false);
    }
    let mut line = String::new();
    let r = stdin.lock().read_line(&mut line);
    if tty {
        echo(true);
        let _ = writeln!(std::io::stderr());
    }
    r.map_err(|e| e.to_string())?;
    let key = line.trim().to_string();
    if key.is_empty() {
        return Err("no API key on standard input".into());
    }
    Ok(key)
}

/// Turns the echo of the terminal on or off with `stty` (macOS and Linux).
fn echo(on: bool) {
    let _ = std::process::Command::new("stty")
        .arg(if on { "echo" } else { "-echo" })
        .stdin(std::process::Stdio::inherit())
        .status();
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
pub fn approve_command(
    store: &Store,
    code: &str,
    budget: &str,
    now_ms: i64,
) -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        return Err(
            "the approval of a budget needs a terminal: standard input is not a TTY".into(),
        );
    }
    let new = parse_usd(budget, MAX_BUDGET_USD)?;
    let r = check_receipt(store, code, now_ms)?;
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "Estimate: {} messages, about {} USD.\nBudget left now: {}\nNew budget: {} USD\n{PRICE_NOTE}\nType the new budget again to approve:",
        r.messages,
        usd(r.cost_micro_usd),
        r.budget_left.map_or("none".into(), |b| format!("{} USD", usd(b))),
        usd(new as i64),
    );
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if parse_usd(line.trim(), MAX_BUDGET_USD) != Ok(new) {
        return Err("the two budgets are not the same: nothing changed".into());
    }
    approve(store, code, new, now_ms)?;
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
    let left = store
        .read(|c| Ok(model_row(c)?))
        .map_err(|e| e.to_string())?
        .and_then(|m| m.price.and(Some(m.budget_micro_usd)));
    Ok(report_text(&r, left))
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
            "https://[::1]:8443/v1",
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
        let pinned = pin_local_url("http://LocalHost:9/v1").unwrap();
        let host = local_host(&pinned).unwrap();
        assert!(host.parse::<IpAddr>().unwrap().is_loopback(), "{pinned}");
        assert!(pinned.ends_with(":9/v1"), "{pinned}");
    }
}
