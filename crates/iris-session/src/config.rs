//! The config of a session (spec "Adapters"): the file `agents.toml`, the
//! adapter of each agent tool, and the environment of the agent.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use crate::egress::Endpoint;

/// The fixed prompt of each agent. It holds no mail text and no goal text.
pub const PROMPT: &str = "Read JOB.md in this folder and do the job that it gives. \
Use only the files of this folder and the iris command.";

/// The name of the file in the data folder.
pub const FILE: &str = "agents.toml";

const BUILT_IN: [&str; 2] = ["claude-code", "codex"];
const MAX_COMMAND: usize = 32;
const MAX_PASS_ENV: usize = 32;
const MAX_HOSTS: usize = 16;
const MAX_READ_PATHS: usize = crate::read_paths::MAX_READS;
const MAX_BYTES: usize = 64 * 1024;
pub const DEFAULT_MINUTES: u32 = 30;
const PROXY_NAMES: [&str; 4] = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "NO_PROXY"];

/// The variables that each agent gets, if the engine has them.
const BASE_ENV: [&str; 9] = [
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "TMPDIR", "SHELL",
];

/// A checked `agents.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsFile {
    pub agents: BTreeMap<String, Custom>,
    /// The time limit of a session, from 1 to 1440 minutes.
    pub max_minutes: u32,
}

impl Default for AgentsFile {
    fn default() -> Self {
        AgentsFile {
            agents: BTreeMap::new(),
            max_minutes: DEFAULT_MINUTES,
        }
    }
}

/// An adapter of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Custom {
    pub command: Vec<String>,
    pub pass_env: Vec<String>,
    pub allow_hosts: Vec<String>,
    /// Folders and files that the session can read, not write
    /// (`docs/specs/2026-11-25-agents-read-paths-local-port.md`). The
    /// launch checks them with [`crate::read_paths::check`].
    pub read_paths: Vec<PathBuf>,
    /// One TCP port on `127.0.0.1` that the session can connect to (same
    /// spec).
    pub allow_local: Option<u16>,
}

/// An error of the file, with its line (from 1; 0 if no line applies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{FILE}: {}", self.message)
        } else {
            write!(f, "{FILE} line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    agent: BTreeMap<String, Spanned<RawAgent>>,
    session: Option<Spanned<RawSession>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgent {
    command: Spanned<Vec<Spanned<String>>>,
    #[serde(default)]
    pass_env: Vec<Spanned<String>>,
    #[serde(default)]
    allow_hosts: Option<Spanned<Vec<Spanned<String>>>>,
    #[serde(default)]
    read_paths: Option<Spanned<Vec<Spanned<String>>>>,
    #[serde(default)]
    allow_local: Option<Spanned<i64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSession {
    max_minutes: Option<Spanned<i64>>,
}

/// Reads `agents.toml` of the data folder. A missing file gives the
/// defaults: no adapter of the file, and a time limit of 30 minutes.
pub fn read_agents(data_dir: &Path) -> Result<AgentsFile, ConfigError> {
    let path = data_dir.join(FILE);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(AgentsFile::default()),
        Err(e) => return Err(no_line(format!("cannot read the file: {e}"))),
    };
    // Read at most one byte past the limit, so a large file costs no memory.
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| no_line(format!("cannot read the file: {e}")))?;
    if bytes.len() > MAX_BYTES {
        return Err(no_line(format!(
            "the file is larger than {} KiB",
            MAX_BYTES / 1024
        )));
    }
    match String::from_utf8(bytes) {
        Ok(text) => parse_agents(&text),
        Err(_) => Err(no_line("the file is not UTF-8".into())),
    }
}

fn no_line(message: String) -> ConfigError {
    ConfigError { line: 0, message }
}

/// Reads and checks the text of `agents.toml`. The error names the line.
pub fn parse_agents(text: &str) -> Result<AgentsFile, ConfigError> {
    if text.len() > MAX_BYTES {
        return Err(no_line(format!(
            "the file is larger than {} KiB",
            MAX_BYTES / 1024
        )));
    }
    let raw: RawFile = toml::from_str(text).map_err(|e| ConfigError {
        line: e.span().map_or(0, |s| line_of(text, s.start)),
        message: e.message().trim().to_string(),
    })?;
    let mut file = AgentsFile::default();
    if let Some(s) = raw.session {
        if let Some(m) = s.into_inner().max_minutes {
            let line = line_of(text, m.span().start);
            let m = m.into_inner();
            if !(1..=1440).contains(&m) {
                return Err(ConfigError {
                    line,
                    message: format!("max_minutes {m} is not from 1 to 1440"),
                });
            }
            file.max_minutes = m as u32;
        }
    }
    for (name, spanned) in raw.agent {
        let table = spanned.span().start;
        let agent = check_agent(&name, spanned.into_inner(), table).map_err(|(at, message)| {
            ConfigError {
                line: line_of(text, at),
                message,
            }
        })?;
        file.agents.insert(name, agent);
    }
    Ok(file)
}

/// The line, from 1, of a byte offset in the text.
fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Checks one adapter of the file. The error gives the byte offset of the
/// field or item that is wrong (`table` for the name), and the message.
fn check_agent(name: &str, raw: RawAgent, table: usize) -> Result<Custom, (usize, String)> {
    if !is_adapter_name(name) {
        return Err((
            table,
            format!("the agent name {name:?} must have 1 to 64 characters of a-z, 0-9 and -"),
        ));
    }
    if BUILT_IN.contains(&name) {
        return Err((
            table,
            format!("the agent name {name} is a built-in adapter"),
        ));
    }
    let at = raw.command.span().start;
    let command = raw.command.into_inner();
    if command.is_empty() || command.len() > MAX_COMMAND {
        return Err((at, format!("command must have 1 to {MAX_COMMAND} strings")));
    }
    if command[0].get_ref().is_empty() {
        return Err((
            command[0].span().start,
            "the program (the first string of command) is empty".into(),
        ));
    }
    if let Some(bad) = command.iter().find(|s| s.get_ref().contains('\0')) {
        return Err((
            bad.span().start,
            "a string of command holds a NUL character".into(),
        ));
    }
    if raw.pass_env.len() > MAX_PASS_ENV {
        return Err((
            raw.pass_env[MAX_PASS_ENV].span().start,
            format!("pass_env has more than {MAX_PASS_ENV} names"),
        ));
    }
    for v in &raw.pass_env {
        check_env_name(v.get_ref()).map_err(|m| (v.span().start, m))?;
    }
    let hosts = match raw.allow_hosts {
        Some(h) => {
            let at = h.span().start;
            let h = h.into_inner();
            if h.len() > MAX_HOSTS {
                return Err((at, format!("allow_hosts has more than {MAX_HOSTS} hosts")));
            }
            h
        }
        None => Vec::new(),
    };
    for h in &hosts {
        check_host(h.get_ref()).map_err(|m| (h.span().start, m))?;
    }
    let reads = match raw.read_paths {
        Some(r) => {
            let at = r.span().start;
            let r = r.into_inner();
            if r.len() > MAX_READ_PATHS {
                return Err((
                    at,
                    format!("read_paths has more than {MAX_READ_PATHS} paths"),
                ));
            }
            r
        }
        None => Vec::new(),
    };
    for r in &reads {
        check_read_path(r.get_ref()).map_err(|m| (r.span().start, m))?;
    }
    let allow_local = match raw.allow_local {
        Some(p) => Some(
            u16::try_from(*p.get_ref())
                .ok()
                .filter(|p| *p != 0)
                .ok_or((
                    p.span().start,
                    format!("allow_local {} is not a port from 1 to 65535", p.get_ref()),
                ))?,
        ),
        None => None,
    };
    Ok(Custom {
        command: command.into_iter().map(Spanned::into_inner).collect(),
        pass_env: raw.pass_env.into_iter().map(Spanned::into_inner).collect(),
        allow_hosts: hosts.into_iter().map(Spanned::into_inner).collect(),
        read_paths: reads
            .into_iter()
            .map(|r| PathBuf::from(r.into_inner()))
            .collect(),
        allow_local,
    })
}

/// A read path is absolute, not empty, with no `..` part and no NUL.
fn check_read_path(p: &str) -> Result<(), String> {
    let path = Path::new(p);
    let dots = path
        .components()
        .any(|c| c == std::path::Component::ParentDir);
    if p.is_empty() || p.contains('\0') || !path.is_absolute() || dots {
        return Err(format!(
            "read_paths {p:?} is not an absolute path with no .. part"
        ));
    }
    Ok(())
}

/// The rule of the store for an agent name: 1 to 64 of a-z, 0-9 and `-`.
fn is_adapter_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn check_env_name(v: &str) -> Result<(), String> {
    let b = v.as_bytes();
    let form = !b.is_empty()
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_');
    if PROXY_NAMES.iter().any(|p| p.eq_ignore_ascii_case(v)) {
        return Err(format!(
            "pass_env {v:?} is a proxy name; only the engine sets it"
        ));
    }
    if !form {
        return Err(format!(
            "pass_env {v:?} is not a name of the form [A-Z_][A-Z0-9_]*"
        ));
    }
    if v.starts_with("IRIS") {
        return Err(format!(
            "pass_env {v:?} starts with IRIS; no variable of Iris goes to an agent"
        ));
    }
    Ok(())
}

/// A host is a DNS name in lower case: labels of 1 to 63 characters of a-z,
/// 0-9 and `-` (not at the start or the end of a label), at most 253
/// characters. A name whose last label is a number (decimal, or 0x and
/// hexadecimal) is refused, because a resolver reads it as an IPv4 address.
fn check_host(h: &str) -> Result<(), String> {
    let label_ok = |l: &str| {
        (1..=63).contains(&l.len())
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    // The rule of the URL Standard ("ends in a number"): a last label of
    // digits, or 0x and hexadecimal digits, makes the name an IPv4 form,
    // which the system resolver reads as an address (also 0x08080808).
    let numeric_end = h.rsplit('.').next().is_some_and(|l| {
        l.bytes().all(|b| b.is_ascii_digit())
            || l.strip_prefix("0x")
                .is_some_and(|x| x.bytes().all(|b| b.is_ascii_hexdigit()))
    });
    if h.len() > 253 || !h.split('.').all(label_ok) || numeric_end {
        return Err(format!(
            "allow_hosts {h:?} is not a DNS name in lower case (no port, no wildcard, no IP address)"
        ));
    }
    Ok(())
}

/// The adapter of one agent tool: the program, its arguments with the
/// prompt, the variables that it gets, and the hosts that it can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    /// The full path of the program.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The variables of the adapter, after the base list.
    pub pass_env: Vec<String>,
    pub allow_hosts: Vec<String>,
    /// The read paths of the file, as written. The built-in adapters have
    /// none.
    pub read_paths: Vec<PathBuf>,
    /// The local port of the file. The built-in adapters have none.
    pub allow_local: Option<u16>,
}

/// Resolves the adapter `name`: a built-in adapter or one of the file. The
/// program must be an executable file, found on `path` (the `PATH` of the
/// engine) if its name has no `/`. A missing program is refused here, before
/// a session row is written.
pub fn adapter(file: &AgentsFile, name: &str, path: Option<&OsStr>) -> Result<Adapter, String> {
    let prompt = PROMPT.to_string();
    let mut read_paths = Vec::new();
    let mut allow_local = None;
    let (program, args, pass_env, allow_hosts) = match name {
        "claude-code" => (
            "claude".to_string(),
            vec![
                "-p".into(),
                prompt,
                "--allowedTools".into(),
                "Bash(iris:*)".into(),
                "Read".into(),
                "Write".into(),
                "Edit".into(),
                "Glob".into(),
                "Grep".into(),
            ],
            vec![
                "ANTHROPIC_API_KEY".to_string(),
                // A token of `claude setup-token`: the session has no home of the person.
                "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
            ],
            vec!["api.anthropic.com".to_string()],
        ),
        "codex" => (
            "codex".to_string(),
            vec![
                "exec".into(),
                "--sandbox".into(),
                "workspace-write".into(),
                "--skip-git-repo-check".into(),
                prompt,
            ],
            vec!["OPENAI_API_KEY".to_string(), "CODEX_HOME".to_string()],
            vec!["api.openai.com".to_string()],
        ),
        _ => {
            let c = file
                .agents
                .get(name)
                .ok_or_else(|| format!("no agent {name:?}: not built in and not in {FILE}"))?;
            let mut args: Vec<String> = c.command[1..].to_vec();
            args.push(prompt);
            read_paths = c.read_paths.clone();
            allow_local = c.allow_local;
            (
                c.command[0].clone(),
                args,
                c.pass_env.clone(),
                c.allow_hosts.clone(),
            )
        }
    };
    let program = find_program(&program, path)
        .ok_or_else(|| format!("the program {program:?} of the agent {name} is not found"))?;
    Ok(Adapter {
        name: name.to_string(),
        program,
        args,
        pass_env,
        allow_hosts,
        read_paths,
        allow_local,
    })
}

fn find_program(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    // A regular file that the engine user may run: access(X_OK) checks the
    // owner, group and other bits against this user, not only that one
    // execute bit exists (#432).
    let executable = |p: &Path| {
        let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: c is a valid C string for the call.
        std::fs::metadata(p).is_ok_and(|m| m.is_file())
            && unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0
    };
    if program.contains('/') {
        let p = PathBuf::from(program);
        return (p.is_absolute() && executable(&p)).then_some(p);
    }
    std::env::split_paths(path?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(program))
        .find(|p| executable(p))
}

/// The environment of the agent: empty, then only the base list and the
/// variables of the adapter, with the values of the engine (`get`). The
/// proxy variables come only from the egress port of the session, and only
/// for an adapter with allowed hosts. With a token (macOS), the proxy URL
/// holds `iris:<token>`. No variable of Iris goes to the agent.
pub fn agent_env(
    adapter: &Adapter,
    egress: Option<&Endpoint>,
    get: impl Fn(&str) -> Option<OsString>,
) -> Vec<(String, OsString)> {
    let mut env: Vec<(String, OsString)> = Vec::new();
    for name in BASE_ENV
        .iter()
        .copied()
        .chain(adapter.pass_env.iter().map(|s| s.as_str()))
    {
        let refused = name.starts_with("IRIS")
            || PROXY_NAMES.iter().any(|p| p.eq_ignore_ascii_case(name))
            || env.iter().any(|(k, _)| k == name);
        if refused {
            continue;
        }
        if let Some(v) = get(name) {
            env.push((name.to_string(), v));
        }
    }
    if let (Some(e), false) = (egress, adapter.allow_hosts.is_empty()) {
        let user = e
            .token
            .as_ref()
            .map(|t| format!("iris:{t}@"))
            .unwrap_or_default();
        let url = OsString::from(format!("http://{user}127.0.0.1:{}", e.port));
        env.push(("HTTPS_PROXY".into(), url.clone()));
        env.push(("https_proxy".into(), url));
    }
    env
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
