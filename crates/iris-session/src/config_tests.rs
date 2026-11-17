//! Tests of the session config: `agents.toml`, the adapters and the
//! environment of the agent.

use super::*;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;

/// A small random number source for the property tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }

    fn word(&mut self, alphabet: &[u8], max: u64) -> String {
        let len = self.next(max + 1) as usize;
        (0..len)
            .map(|_| alphabet[self.next(alphabet.len() as u64) as usize] as char)
            .collect()
    }
}

fn err_line(text: &str) -> usize {
    match parse_agents(text) {
        Ok(_) => panic!("accepted:\n{text}"),
        Err(e) => e.line,
    }
}

#[test]
fn an_empty_or_missing_file_gives_the_defaults() {
    let f = parse_agents("").unwrap();
    assert!(f.agents.is_empty());
    assert_eq!(f.max_minutes, 30);
    let dir = std::env::temp_dir().join(format!("iris-cfg-none-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    assert_eq!(read_agents(&dir).unwrap().max_minutes, 30);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_good_file_parses() {
    let f = parse_agents(
        r#"
[session]
max_minutes = 90

[agent.my-tool]
command = ["/usr/local/bin/my-tool", "--run"]
pass_env = ["MY_TOOL_KEY"]
allow_hosts = ["api.my-tool.example"]
"#,
    )
    .unwrap();
    assert_eq!(f.max_minutes, 90);
    let a = &f.agents["my-tool"];
    assert_eq!(a.command, ["/usr/local/bin/my-tool", "--run"]);
    assert_eq!(a.pass_env, ["MY_TOOL_KEY"]);
    assert_eq!(a.allow_hosts, ["api.my-tool.example"]);
}

#[test]
fn each_bad_entry_is_refused_with_its_line() {
    let cases = [
        ("[agent.x]\ncommand = [\"a\"]\nflag = 1\n", 3),
        ("[agent.claude-code]\ncommand = [\"a\"]\n", 1),
        ("[agent.codex]\ncommand = [\"a\"]\n", 1),
        ("[agent.My-Tool]\ncommand = [\"a\"]\n", 1),
        ("[agent.x]\ncommand = []\n", 1),
        ("[agent.x]\ncommand = [\"\"]\n", 1),
        (
            "[agent.x]\n\ncommand = [\"a\"]\npass_env = [\"IRIS_TOKEN\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\npass_env = [\"HTTPS_PROXY\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\npass_env = [\"https_proxy\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\npass_env = [\"No_Proxy\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"api.example:443\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"*.example.com\"]\n",
            1,
        ),
        (
            "[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"10.0.0.1\"]\n",
            1,
        ),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"::1\"]\n", 1),
        (
            "[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"API.example.com\"]\n",
            1,
        ),
        ("[session]\nmax_minutes = 0\n", 1),
        ("[session]\nmax_minutes = 1441\n", 1),
        ("[session]\nlimit = 3\n", 2),
        ("top = 1\n", 1),
    ];
    for (text, line) in cases {
        assert_eq!(err_line(text), line, "{text}");
    }
    let many: Vec<String> = (0..33).map(|i| format!("\"a{i}\"")).collect();
    err_line(&format!("[agent.x]\ncommand = [{}]\n", many.join(",")));
    let hosts: Vec<String> = (0..17).map(|i| format!("\"h{i}.example\"")).collect();
    err_line(&format!(
        "[agent.x]\ncommand = [\"a\"]\nallow_hosts = [{}]\n",
        hosts.join(",")
    ));
}

/// Property test over adapter names: a name passes only if it has 1 to 64
/// characters of a-z, 0-9 and `-` (the rule of the store), and is not a
/// built-in name.
#[test]
fn an_adapter_name_passes_only_in_the_store_form() {
    let mut r = Rng(17);
    let alphabet = b"abcz09-_A.\"' \\x";
    for _ in 0..20_000 {
        let name = r.word(alphabet, 70);
        let text = format!(
            "[agent.\"{}\"]\ncommand = [\"a\"]\n",
            name.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let want = (1..=64).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && name != "claude-code"
            && name != "codex";
        assert_eq!(parse_agents(&text).is_ok(), want, "{name:?}");
    }
}

/// Property test over variable names: a name passes only in the form
/// [A-Z_][A-Z0-9_]*, with no IRIS prefix and no proxy name.
#[test]
fn a_pass_env_name_passes_only_in_its_form() {
    let mut r = Rng(19);
    let alphabet = b"AZIRS_09az-HTPOXYLN";
    for _ in 0..20_000 {
        let name = r.word(alphabet, 14);
        let text = format!("[agent.x]\ncommand = [\"a\"]\npass_env = [\"{name}\"]\n");
        let b = name.as_bytes();
        let form = !b.is_empty()
            && (b[0].is_ascii_uppercase() || b[0] == b'_')
            && b.iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_');
        let proxy = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "NO_PROXY"]
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&name));
        let want = form && !name.starts_with("IRIS") && !proxy;
        assert_eq!(parse_agents(&text).is_ok(), want, "{name:?}");
    }
}

/// Property test over hosts: a host passes only as a lower-case DNS name
/// with no port, no wildcard and no IP address form.
#[test]
fn an_allowed_host_passes_only_as_a_dns_name() {
    let mut r = Rng(23);
    let alphabet = b"ab09.-:*[]A_ ";
    for _ in 0..20_000 {
        let host = r.word(alphabet, 20);
        let text = format!("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"{host}\"]\n");
        let labels: Vec<&str> = host.split('.').collect();
        let label_ok = |l: &&str| {
            (1..=63).contains(&l.len())
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        };
        let want = host.len() <= 253
            && labels.iter().all(label_ok)
            && !labels.last().unwrap().bytes().all(|b| b.is_ascii_digit());
        assert_eq!(parse_agents(&text).is_ok(), want, "{host:?}");
    }
}

/// A folder with one program file in it, for the adapter tests.
struct Bin(std::path::PathBuf);

impl Bin {
    fn new(name: &str, programs: &[&str]) -> Bin {
        let dir = std::env::temp_dir().join(format!("iris-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for p in programs {
            let path = dir.join(p);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Bin(dir)
    }
}

impl Drop for Bin {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_built_in_adapters_give_their_lines() {
    let bin = Bin::new("builtin", &["claude", "codex"]);
    let path = bin.0.as_os_str().to_owned();
    let none = AgentsFile::default();
    let c = adapter(&none, "claude-code", Some(&path)).unwrap();
    assert_eq!(c.program, bin.0.join("claude"));
    assert_eq!(
        c.args,
        [
            "-p",
            PROMPT,
            "--allowedTools",
            "Bash(iris:*)",
            "Read",
            "Write",
            "Edit",
            "Glob",
            "Grep"
        ]
    );
    assert_eq!(c.allow_hosts, ["api.anthropic.com"]);
    let x = adapter(&none, "codex", Some(&path)).unwrap();
    assert_eq!(
        x.args,
        [
            "exec",
            "--sandbox",
            "workspace-write",
            "--skip-git-repo-check",
            PROMPT
        ]
    );
    assert_eq!(x.allow_hosts, ["api.openai.com"]);
}

#[test]
fn a_file_adapter_gets_the_prompt_last_and_a_missing_program_is_refused() {
    let bin = Bin::new("file", &["my-tool"]);
    let path = bin.0.as_os_str().to_owned();
    let f = parse_agents("[agent.mine]\ncommand = [\"my-tool\", \"--run\"]\n").unwrap();
    let a = adapter(&f, "mine", Some(&path)).unwrap();
    assert_eq!(a.program, bin.0.join("my-tool"));
    assert_eq!(a.args, ["--run", PROMPT]);
    assert!(a.allow_hosts.is_empty());
    let abs = format!(
        "[agent.mine]\ncommand = [\"{}\"]\n",
        bin.0.join("my-tool").display()
    );
    assert!(adapter(&parse_agents(&abs).unwrap(), "mine", None).is_ok());
    for (text, name) in [
        ("[agent.mine]\ncommand = [\"not-there\"]\n", "mine"),
        ("[agent.mine]\ncommand = [\"/no/such/file\"]\n", "mine"),
        ("", "unknown"),
    ] {
        assert!(
            adapter(&parse_agents(text).unwrap(), name, Some(&path)).is_err(),
            "{text}"
        );
    }
    // A file that is not executable is not a program.
    std::fs::set_permissions(
        bin.0.join("my-tool"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(adapter(&f, "mine", Some(&path)).is_err());
    // Without the program on PATH, a built-in adapter is refused too.
    assert!(adapter(&AgentsFile::default(), "claude-code", Some(&path)).is_err());
}

fn engine_env() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("PATH", "/usr/bin"),
        ("HOME", "/home/u"),
        ("LANG", "C.UTF-8"),
        ("IRIS_DATA", "/secret"),
        ("IRIS_TOKEN", "t"),
        ("HTTPS_PROXY", "http://engine:1"),
        ("https_proxy", "http://engine:1"),
        ("ANTHROPIC_API_KEY", "k1"),
        ("OPENAI_API_KEY", "k2"),
        ("CODEX_HOME", "/c"),
        ("MY_TOOL_KEY", "k3"),
        ("OTHER", "x"),
    ])
}

fn env_of(a: &Adapter, port: Option<u16>) -> HashMap<String, String> {
    let e = engine_env();
    agent_env(a, port, |k| e.get(k).map(|v| v.into()))
        .into_iter()
        .map(|(k, v)| (k, v.into_string().unwrap()))
        .collect()
}

#[test]
fn the_agent_env_has_only_the_listed_variables() {
    let bin = Bin::new("env", &["claude", "codex", "my-tool"]);
    let path = bin.0.as_os_str().to_owned();
    let f = parse_agents(
        "[agent.mine]\ncommand = [\"my-tool\"]\npass_env = [\"MY_TOOL_KEY\", \"PATH\"]\n\
         [agent.net]\ncommand = [\"my-tool\"]\nallow_hosts = [\"api.example.test\"]\n",
    )
    .unwrap();
    let c = env_of(
        &adapter(&f, "claude-code", Some(&path)).unwrap(),
        Some(4000),
    );
    let mut keys: Vec<&str> = c.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "ANTHROPIC_API_KEY",
            "HOME",
            "HTTPS_PROXY",
            "LANG",
            "PATH",
            "https_proxy"
        ]
    );
    assert_eq!(c["HTTPS_PROXY"], "http://127.0.0.1:4000");
    assert_eq!(c["https_proxy"], "http://127.0.0.1:4000");
    let x = env_of(&adapter(&f, "codex", Some(&path)).unwrap(), Some(4001));
    assert!(x.contains_key("OPENAI_API_KEY") && x.contains_key("CODEX_HOME"));
    assert!(!x.contains_key("ANTHROPIC_API_KEY"));
    // No allowed host: no proxy variable, also when a port is given and
    // the engine has its own proxy variables.
    let m = env_of(&adapter(&f, "mine", Some(&path)).unwrap(), Some(4002));
    let mut keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["HOME", "LANG", "MY_TOOL_KEY", "PATH"]);
    // Allowed hosts but no port: no proxy variable from the engine.
    let n = env_of(&adapter(&f, "net", Some(&path)).unwrap(), None);
    assert!(!n.keys().any(|k| k.eq_ignore_ascii_case("https_proxy")));
    for env in [c, x, m, n] {
        assert!(!env.keys().any(|k| k.starts_with("IRIS")));
    }
}
