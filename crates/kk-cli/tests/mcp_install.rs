use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn kaku() -> &'static str {
    env!("CARGO_BIN_EXE_kaku")
}

struct Printed {
    code: i32,
    out: String,
    err: String,
}

fn install(args: &[&str]) -> Printed {
    let o = Command::new(kaku())
        .args(["mcp", "install"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Printed {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-mcpi-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// What a client would run: the command and its arguments, read from the printed JSON.
fn from_json(text: &str, wrapper_client: &str) -> (String, Vec<String>) {
    let v: Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("{wrapper_client}: not JSON ({e}): {text}"));
    let s = &v["mcpServers"]["kakurega"];
    (
        s["command"].as_str().unwrap().to_string(),
        s["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect(),
    )
}

/// What a real shell makes of the printed Claude Code line after the `--`: exactly what Claude Code is told to run.
fn from_shell_line(line: &str) -> Vec<String> {
    let prefix = "claude mcp add --transport stdio kakurega -- ";
    let rest = line
        .trim_end()
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("unexpected line: {line}"));
    let o = Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\0' {rest}"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut parts: Vec<String> = o
        .stdout
        .split(|b| *b == 0)
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    parts.pop();
    parts
}

fn spawn_listing(command: &str, args: &[String]) -> Vec<String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    )
    .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    drop(stdin);
    child.wait().unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn same_file(a: &str, b: &str) -> bool {
    Path::new(a).canonicalize().unwrap() == Path::new(b).canonicalize().unwrap()
}

#[test]
fn mi01_each_client_gets_the_form_it_reads_and_nothing_else_on_stdout() {
    let dir = scratch("shapes");
    let d = dir.to_str().unwrap();
    let desktop = install(&["--client", "claude-desktop", "--print", "--data-dir", d]);
    assert_eq!(desktop.code, 0, "{}", desktop.err);
    let (cmd, args) = from_json(&desktop.out, "claude-desktop");
    assert!(same_file(&cmd, kaku()));
    assert_eq!(args, ["mcp", "--data-dir", d]);
    let generic = install(&["--client", "generic", "--print", "--data-dir", d]);
    assert_eq!(
        generic.out, desktop.out,
        "generic is the same mcpServers block"
    );
    let cursor = install(&["--client", "cursor", "--print", "--data-dir", d]);
    let v: Value = serde_json::from_str(&cursor.out).unwrap();
    assert_eq!(v["mcpServers"]["kakurega"]["type"], "stdio");
    assert_eq!(from_json(&cursor.out, "cursor").1, args);
    let code = install(&["--client", "claude-code", "--print", "--data-dir", d]);
    assert_eq!(code.code, 0, "{}", code.err);
    assert_eq!(
        code.out.lines().count(),
        1,
        "one line to paste: {}",
        code.out
    );
    let parts = from_shell_line(&code.out);
    assert!(same_file(&parts[0], kaku()));
    assert_eq!(parts[1..], args[..]);
}

#[test]
fn mi02_what_is_printed_is_exactly_what_the_client_will_run_and_carries_every_limit() {
    let dir = scratch("run");
    let d = dir.to_str().unwrap();
    let everything = spawn_listing(kaku(), &["mcp".into(), "--data-dir".into(), d.into()]);
    assert!(
        everything.contains(&"start_review".to_string()),
        "{everything:?}"
    );
    // (flags given to install, the tools the printed command must offer)
    let cases: Vec<(Vec<&str>, Vec<String>)> = vec![
        (vec![], everything.clone()),
        (
            vec!["--read-only"],
            everything
                .iter()
                .filter(|t| !["start_review", "request_approval"].contains(&t.as_str()))
                .cloned()
                .collect(),
        ),
        (
            vec!["--allow", "overview, verify_log"],
            vec!["overview".into(), "verify_log".into()],
        ),
        (
            vec!["--read-only", "--allow", "overview,start_review"],
            vec!["overview".into()],
        ),
    ];
    for (flags, expect) in cases {
        let mut argv = vec!["--client", "claude-desktop", "--print", "--data-dir", d];
        argv.extend_from_slice(&flags);
        let p = install(&argv);
        assert_eq!(p.code, 0, "{flags:?}: {}", p.err);
        let (cmd, args) = from_json(&p.out, "desktop");
        assert_eq!(spawn_listing(&cmd, &args), expect, "{flags:?}");
        // the Claude Code line runs the same thing
        let mut argv = vec!["--client", "claude-code", "--print", "--data-dir", d];
        argv.extend_from_slice(&flags);
        let parts = from_shell_line(&install(&argv).out);
        assert_eq!(spawn_listing(&parts[0], &parts[1..]), expect, "{flags:?}");
    }
    // --leased is carried too (the service is not running, so no tool that acts is offered)
    let p = install(&[
        "--client",
        "generic",
        "--print",
        "--data-dir",
        d,
        "--leased",
    ]);
    let (cmd, args) = from_json(&p.out, "leased");
    assert!(args.contains(&"--leased".to_string()));
    let tools = spawn_listing(&cmd, &args);
    assert!(
        !tools.contains(&"start_review".to_string()) && tools.contains(&"overview".to_string()),
        "{tools:?}"
    );
}

#[test]
fn mi03_awkward_paths_survive_a_real_shell_and_never_run_anything() {
    let root = scratch("awkward");
    let marker = root.join("ran");
    let awkward = [
        "with space",
        "it's",
        "dq\"uote",
        "back\\slash",
        "$(touch ran)",
        "`touch ran`",
        "a;touch ran",
        "a&&touch ran",
        "new\nline",
        "tab\there",
        "star*glob?",
        "~tilde",
        "-leading-dash",
        "ünïcode-目录",
    ];
    for name in awkward {
        let dir = root.join(name);
        let d = dir.to_str().unwrap();
        let code = install(&["--client", "claude-code", "--print", "--data-dir", d]);
        assert_eq!(code.code, 0, "{name:?}: {}", code.err);
        let parts = from_shell_line(&code.out);
        assert_eq!(
            parts[1..],
            ["mcp", "--data-dir", d],
            "{name:?}: {}",
            code.out
        );
        let json = install(&["--client", "claude-desktop", "--print", "--data-dir", d]);
        assert_eq!(
            from_json(&json.out, name).1,
            ["mcp", "--data-dir", d],
            "{name:?}"
        );
    }
    assert!(!marker.exists(), "no printed text executed anything");
}

#[test]
fn mi04_no_secret_is_ever_printed_or_read_into_the_output() {
    let dir = scratch("secret");
    std::fs::write(
        dir.join("admin.json"),
        r#"{"port":9,"token":"SECRET-TOKEN-xyz-0123456789","pid":1}"#,
    )
    .unwrap();
    let before: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let o = Command::new(kaku())
        .args([
            "mcp",
            "install",
            "--client",
            "generic",
            "--print",
            "--data-dir",
        ])
        .arg(&dir)
        .env("KAKU_ADMIN_TOKEN", "SECRET-ENV-TOKEN-abc")
        .env("KAKU_MCP_READ_ONLY", "1")
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success());
    for secret in [
        "SECRET-TOKEN-xyz",
        "SECRET-ENV-TOKEN",
        "token",
        "admin.json",
    ] {
        assert!(!all.contains(secret), "{secret}: {all}");
    }
    assert!(
        !String::from_utf8_lossy(&o.stdout).contains("--read-only"),
        "the environment is not copied into the config: {all}"
    );
    let after: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after, "nothing is written anywhere");
}

#[test]
fn mi05_anything_unclear_is_refused_with_a_reason_and_prints_nothing() {
    let dir = scratch("refuse");
    let d = dir.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec![],
        vec!["--print"],
        vec!["--client", "claude-desktop"],
        vec!["--client", "nonsense", "--print"],
        vec!["--client", "--print"],
        vec!["--client", "cursor", "--client", "generic", "--print"],
        vec!["--client", "cursor", "--print", "--read-onyl"],
        vec!["--client", "cursor", "--print", "--allow", "nonsense"],
        vec!["--client", "cursor", "--print", "--allow", ""],
        vec!["--client", "cursor", "--print", "--allow"],
        vec!["--client", "cursor", "--print", "stray"],
        vec![
            "--client",
            "cursor",
            "--print",
            "--data-dir",
            d,
            "--data-dir",
            d,
        ],
    ];
    for argv in cases {
        let p = install(&argv);
        assert_eq!(p.code, 2, "{argv:?}: out={} err={}", p.out, p.err);
        assert!(
            p.out.is_empty(),
            "{argv:?}: nothing printed to paste: {}",
            p.out
        );
        assert!(!p.err.is_empty(), "{argv:?}: says why");
    }
}

#[test]
fn mi06_the_file_that_will_run_is_named_with_its_digest_on_stderr() {
    let p = install(&["--client", "generic", "--print"]);
    assert_eq!(p.code, 0);
    let bytes = std::fs::read(kaku()).unwrap();
    let digest = kk_evidence::capsule::sha256_hex(&bytes);
    assert!(p.err.contains(&digest), "{}", p.err);
    let canonical = Path::new(kaku()).canonicalize().unwrap();
    assert!(p.err.contains(canonical.to_str().unwrap()), "{}", p.err);
    assert!(!p.out.contains(&digest), "stdout stays paste-ready");
}

#[test]
fn mi07_allow_names_are_written_without_stray_spaces_or_duplicates() {
    let p = install(&[
        "--client",
        "generic",
        "--print",
        "--allow",
        " overview , verify_log,overview ,",
    ]);
    assert_eq!(p.code, 0, "{}", p.err);
    let (_, args) = from_json(&p.out, "allow");
    assert_eq!(args, ["mcp", "--allow", "overview,verify_log"]);
}

#[test]
fn mi08_a_relative_data_folder_is_written_as_the_full_path_it_means_now() {
    let cwd = scratch("rel").canonicalize().unwrap();
    let o = Command::new(kaku())
        .current_dir(&cwd)
        .args([
            "mcp",
            "install",
            "--client",
            "generic",
            "--print",
            "--data-dir",
            "here/data",
        ])
        .output()
        .unwrap();
    assert!(o.status.success());
    let (_, args) = from_json(&String::from_utf8_lossy(&o.stdout), "rel");
    assert_eq!(
        args,
        ["mcp", "--data-dir", cwd.join("here/data").to_str().unwrap()]
    );
}
