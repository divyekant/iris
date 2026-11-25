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

/// The file is read only up to its limit: a larger file is refused, and
/// the file at the limit is read.
#[test]
fn the_file_is_read_up_to_its_limit() {
    let dir = std::env::temp_dir().join(format!("iris-cfg-size-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut text = String::from("[session]\nmax_minutes = 5\n");
    text.push_str(&"#".repeat(64 * 1024 - text.len()));
    std::fs::write(dir.join(FILE), &text).unwrap();
    assert_eq!(read_agents(&dir).unwrap().max_minutes, 5);
    text.push('#');
    std::fs::write(dir.join(FILE), &text).unwrap();
    assert!(read_agents(&dir).unwrap_err().message.contains("larger"));
    let _ = std::fs::remove_dir_all(&dir);
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
    // Each error names the line of the field or the item that is wrong,
    // not the line of its table.
    let cases = [
        ("[agent.x]\ncommand = [\"a\"]\nflag = 1\n", 3),
        ("[agent.claude-code]\ncommand = [\"a\"]\n", 1),
        ("[agent.codex]\ncommand = [\"a\"]\n", 1),
        ("[agent.My-Tool]\ncommand = [\"a\"]\n", 1),
        ("[agent.x]\ncommand = []\n", 2),
        ("[agent.x]\ncommand = [\"\"]\n", 2),
        ("[agent.x]\ncommand = [\n  \"a\",\n  \"b\\u0000\",\n]\n", 4),
        ("[agent.x]\n\ncommand = [\"a\"]\npass_env = [\"IRIS_TOKEN\"]\n", 4),
        ("[agent.x]\ncommand = [\"a\"]\npass_env = [\"HTTPS_PROXY\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\npass_env = [\"https_proxy\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\npass_env = [\"No_Proxy\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"api.example:443\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"*.example.com\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"10.0.0.1\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"::1\"]\n", 3),
        ("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\n  \"ok.example\",\n  \"API.example.com\",\n]\n", 5),
        ("[session]\n\nmax_minutes = 0\n", 3),
        ("[session]\nmax_minutes = 1441\n", 2),
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
/// with no port, no wildcard and no IP address form. A name whose last
/// label is a number (decimal, or hexadecimal with 0x, as the system
/// resolver reads it) is an IPv4 address form.
#[test]
fn an_allowed_host_passes_only_as_a_dns_name() {
    let mut r = Rng(23);
    let alphabet = b"ab0x9f.-:*[]A_ ";
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
            && !ends_in_a_number(labels.last().unwrap());
        assert_eq!(parse_agents(&text).is_ok(), want, "{host:?}");
    }
}

/// The rule of the URL Standard for a label that makes a host an IPv4 form.
fn ends_in_a_number(l: &str) -> bool {
    l.bytes().all(|b| b.is_ascii_digit())
        || l.strip_prefix("0x")
            .is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[test]
fn each_numeric_ip_form_is_refused() {
    for host in [
        "8.8.8.8",
        "134744072",
        "0x08080808",
        "0x8.0x8.0x8.0x8",
        "010.0.0.1",
        "a.0x",
        "a.0xff",
        "a.1",
    ] {
        let text = format!("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"{host}\"]\n");
        assert!(parse_agents(&text).is_err(), "{host}");
    }
    for host in ["0xa.example", "api.0xg", "x1.example", "a.b1"] {
        let text = format!("[agent.x]\ncommand = [\"a\"]\nallow_hosts = [\"{host}\"]\n");
        assert!(parse_agents(&text).is_ok(), "{host}");
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
        ("CLAUDE_CODE_OAUTH_TOKEN", "k0"),
        ("OPENAI_API_KEY", "k2"),
        ("CODEX_HOME", "/c"),
        ("MY_TOOL_KEY", "k3"),
        ("OTHER", "x"),
    ])
}

fn env_of(a: &Adapter, port: Option<u16>) -> HashMap<String, String> {
    env_with(a, port.map(|port| Endpoint { port, token: None }))
}

fn env_with(a: &Adapter, end: Option<Endpoint>) -> HashMap<String, String> {
    let e = engine_env();
    agent_env(a, end.as_ref(), |k| e.get(k).map(|v| v.into()))
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
            "CLAUDE_CODE_OAUTH_TOKEN",
            "HOME",
            "HTTPS_PROXY",
            "LANG",
            "PATH",
            "https_proxy"
        ]
    );
    assert_eq!(c["HTTPS_PROXY"], "http://127.0.0.1:4000");
    assert_eq!(c["https_proxy"], "http://127.0.0.1:4000");
    // macOS: the URL holds the token of the session, in both variables.
    let t = env_with(
        &adapter(&f, "claude-code", Some(&path)).unwrap(),
        Some(Endpoint {
            port: 4003,
            token: Some("ab12".into()),
        }),
    );
    assert_eq!(t["HTTPS_PROXY"], "http://iris:ab12@127.0.0.1:4003");
    assert_eq!(t["https_proxy"], "http://iris:ab12@127.0.0.1:4003");
    let x = env_of(&adapter(&f, "codex", Some(&path)).unwrap(), Some(4001));
    assert!(x.contains_key("OPENAI_API_KEY") && x.contains_key("CODEX_HOME"));
    assert!(!x.contains_key("ANTHROPIC_API_KEY") && !x.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
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

/// The program is looked up only in the absolute folders of the PATH of
/// the engine: a relative or empty entry depends on the working folder of
/// the engine, which is not fixed for a service, so it is skipped.
#[test]
fn a_relative_path_entry_is_skipped() {
    let bin = Bin::new("rel", &["my-tool"]);
    let f = parse_agents("[agent.mine]\ncommand = [\"my-tool\"]\n").unwrap();
    let rel = bin.0.strip_prefix("/").unwrap();
    for path in [
        format!("{}", rel.display()),
        String::new(),
        format!(":{}", rel.display()),
    ] {
        // Each entry names the same folder only when the working folder is /.
        assert!(
            adapter(&f, "mine", Some(OsStr::new(&path))).is_err(),
            "{path:?}"
        );
    }
    let abs = format!("/no/such:{}", bin.0.display());
    assert_eq!(
        adapter(&f, "mine", Some(OsStr::new(&abs))).unwrap().program,
        bin.0.join("my-tool")
    );
}

/// Runs [`the_lookup_skips_a_file_that_the_engine_cannot_run`] again in a
/// child process as the user `nobody` (65534). The checkout and `TMPDIR`
/// of root can be closed to other users or on a `noexec` mount. So for
/// each base folder in turn, the child gets a copy of this test program
/// and its own `TMPDIR` (owned by `nobody`) in one new folder of the base.
/// The first base where the child program starts gives the result: the
/// test files are on the same mount as the program, so they can run too.
fn as_nobody() {
    use std::os::unix::fs::chown;
    use std::os::unix::process::CommandExt;
    let bases = [
        std::env::temp_dir(),
        "/tmp".into(),
        "/var/tmp".into(),
        "/dev/shm".into(),
    ];
    let mut why = Vec::new();
    for base in &bases {
        let home = base.join(format!("iris-cfg-nobody-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let tmp = home.join("tmp");
        let exe = home.join("test");
        let made = std::fs::create_dir(&home)
            .and_then(|()| std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)))
            .and_then(|()| std::fs::copy(std::env::current_exe()?, &exe))
            .and_then(|_| std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)))
            .and_then(|()| std::fs::create_dir(&tmp))
            .and_then(|()| chown(&tmp, Some(65534), Some(65534)));
        let child = made.and_then(|()| {
            std::process::Command::new(&exe)
                .args([
                    "--exact",
                    "config::tests::the_lookup_skips_a_file_that_the_engine_cannot_run",
                    "--test-threads=1",
                ])
                .env("TMPDIR", &tmp)
                .current_dir(&tmp)
                .uid(65534)
                .gid(65534)
                .output()
        });
        let _ = std::fs::remove_dir_all(&home);
        // The program did not start from this base (no path for nobody,
        // a `noexec` mount): try the next base.
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                why.push(format!("{}: {e}", base.display()));
                continue;
            }
        };
        let out = String::from_utf8_lossy(&child.stdout);
        // The child ran the test: a filter that matched nothing also succeeds.
        assert!(
            child.status.success() && out.contains("1 passed"),
            "the child as nobody: {out}{}",
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    panic!("the child as nobody started from no base: {why:?}");
}

/// A file with an execute bit that does not let the engine user run it is
/// not a program, and the lookup goes on to the next folder of PATH (#432).
/// Root can run a file with any execute bit, so as root the test runs
/// itself again in a child process as the user `nobody` (65534), which
/// owns the files that it makes (#458).
#[test]
fn the_lookup_skips_a_file_that_the_engine_cannot_run() {
    // SAFETY: geteuid has no memory effects.
    if unsafe { libc::geteuid() } == 0 {
        as_nobody();
    }
    let first = Bin::new("noexec-1", &["my-tool"]);
    let second = Bin::new("noexec-2", &["my-tool"]);
    // Only "others" may run the first file; the engine user owns it.
    std::fs::set_permissions(
        first.0.join("my-tool"),
        std::fs::Permissions::from_mode(0o001),
    )
    .unwrap();
    let path = std::env::join_paths([&first.0, &second.0]).unwrap();
    let f = parse_agents("[agent.mine]\ncommand = [\"my-tool\"]\n").unwrap();
    let got = adapter(&f, "mine", Some(&path)).unwrap();
    // SAFETY: geteuid has no memory effects.
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(got.program, second.0.join("my-tool"));
    }
    // A file with no execute bit is refused for each user, root too.
    std::fs::set_permissions(
        second.0.join("my-tool"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    std::fs::set_permissions(
        first.0.join("my-tool"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(adapter(&f, "mine", Some(&path)).is_err());
}

#[test]
fn read_paths_are_absolute_with_no_dots() {
    let ok = parse_agents(
        "[agent.t]\ncommand = [\"/bin/t\"]\nread_paths = [\"/opt/t\", \"/home/a/.nvm\"]\n",
    )
    .unwrap();
    assert_eq!(
        ok.agents["t"].read_paths,
        vec![PathBuf::from("/opt/t"), PathBuf::from("/home/a/.nvm")]
    );
    for bad in ["\"rel/x\"", "\"\"", "\"/a/../b\"", "\"/a/\\u0000\""] {
        let text = format!("[agent.t]\ncommand = [\"/bin/t\"]\nread_paths = [{bad}]\n");
        let e = parse_agents(&text).unwrap_err();
        assert_eq!(e.line, 3, "{bad}: {e}");
        assert!(e.message.contains("read_paths"), "{bad}: {e}");
    }
    let many: Vec<String> = (0..17).map(|i| format!("\"/p{i}\"")).collect();
    let text = format!(
        "[agent.t]\ncommand = [\"/bin/t\"]\nread_paths = [{}]\n",
        many.join(", ")
    );
    assert!(parse_agents(&text)
        .unwrap_err()
        .message
        .contains("more than 16"));
}

#[test]
fn allow_local_is_one_port_from_1_to_65535() {
    let ok = parse_agents("[agent.t]\ncommand = [\"/bin/t\"]\nallow_local = 11434\n").unwrap();
    assert_eq!(ok.agents["t"].allow_local, Some(11434));
    let none = parse_agents("[agent.t]\ncommand = [\"/bin/t\"]\n").unwrap();
    assert_eq!(none.agents["t"].allow_local, None);
    for bad in ["0", "65536", "-1", "\"11434\"", "[11434]", "1.5"] {
        let text = format!("[agent.t]\ncommand = [\"/bin/t\"]\nallow_local = {bad}\n");
        let e = parse_agents(&text).unwrap_err();
        assert_eq!(e.line, 3, "{bad}: {e}");
        // A value of another type gets the error of the parser.
        assert!(
            e.message.contains("allow_local") || e.message.contains("invalid type"),
            "{bad}: {e}"
        );
    }
}
