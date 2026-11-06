//! `kaku mcp install --client NAME --print`: the exact text that makes an MCP client start this server.
//!
//! It only prints. A client's configuration is a file that runs programs with the owner's privileges, so it is never edited
//! here; the owner sees the command and places it. The text names this very binary by its full path, carries every limit the
//! owner chose, and contains no secret: the server finds the service's token itself, from the data folder, when it starts.

use std::path::{Path, PathBuf};

const CLIENTS: [&str; 4] = ["claude-code", "claude-desktop", "cursor", "generic"];
const USAGE: &str = "usage: kaku mcp install --client claude-code|claude-desktop|cursor|generic --print [--read-only] [--allow TOOL,TOOL] [--leased] [--data-dir DIR]";

struct Choice {
    client: String,
    read_only: bool,
    leased: bool,
    allow: Option<Vec<String>>,
    data_dir: Option<PathBuf>,
}

fn value_of<'a>(args: &'a [String], i: usize, flag: &str) -> Result<&'a str, String> {
    match args.get(i + 1).map(String::as_str) {
        Some(v) if !v.is_empty() && !v.starts_with("--") => Ok(v),
        _ => Err(format!("{flag} needs a value")),
    }
}

fn parse(args: &[String]) -> Result<Choice, String> {
    let (mut client, mut allow, mut data_dir) = (None, None, None);
    let (mut print, mut read_only, mut leased) = (false, false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--print" => print = true,
            "--read-only" => read_only = true,
            "--leased" => leased = true,
            flag @ ("--client" | "--allow" | "--data-dir") => {
                let v = value_of(args, i, flag)?;
                let taken = match flag {
                    "--client" => client.replace(v.to_string()).is_some(),
                    "--allow" => allow.replace(v.to_string()).is_some(),
                    _ => data_dir.replace(v.to_string()).is_some(),
                };
                if taken {
                    return Err(format!("{flag} was given twice"));
                }
                i += 1;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    if !print {
        return Err(
            "this command only prints: add --print. It never edits a client's configuration".into(),
        );
    }
    let client = client.ok_or("--client is required")?;
    if !CLIENTS.contains(&client.as_str()) {
        return Err(format!(
            "unknown client `{client}`; the clients are {}",
            CLIENTS.join(", ")
        ));
    }
    let allow = allow.map(|l| crate::mcp::parse_allow(&l)).transpose()?;
    let data_dir = data_dir
        .map(|d| {
            let p = PathBuf::from(d);
            if p.is_absolute() {
                Ok(p)
            } else {
                std::env::current_dir()
                    .map(|c| c.join(p))
                    .map_err(|e| format!("cannot make --data-dir absolute: {e}"))
            }
        })
        .transpose()?;
    Ok(Choice {
        client,
        read_only,
        leased,
        allow,
        data_dir,
    })
}

/// The arguments after the binary, in a fixed order.
fn server_args(c: &Choice) -> Vec<String> {
    let mut a = vec!["mcp".to_string()];
    if c.read_only {
        a.push("--read-only".into());
    }
    if c.leased {
        a.push("--leased".into());
    }
    if let Some(names) = &c.allow {
        a.push("--allow".into());
        a.push(names.join(","));
    }
    if let Some(d) = &c.data_dir {
        a.push("--data-dir".into());
        a.push(d.to_string_lossy().into_owned());
    }
    a
}

/// One word for a POSIX shell: plain characters stay as they are, anything else is wrapped in single quotes.
fn sh_word(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:=@%+,-".contains(&b));
    if plain {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn snippet(c: &Choice, binary: &Path) -> Result<String, String> {
    let command = binary.to_string_lossy().into_owned();
    let args = server_args(c);
    if c.client == "claude-code" {
        if cfg!(windows) {
            return Err(
                "the Claude Code line is for a POSIX shell; use --client generic for the JSON form"
                    .into(),
            );
        }
        let words: Vec<String> = std::iter::once(command)
            .chain(args)
            .map(|w| sh_word(&w))
            .collect();
        return Ok(format!(
            "claude mcp add --transport stdio kakurega -- {}\n",
            words.join(" ")
        ));
    }
    let mut server = serde_json::json!({"command": command, "args": args});
    if c.client == "cursor" {
        server = serde_json::json!({"type": "stdio", "command": command, "args": server["args"]});
    }
    let doc = serde_json::json!({"mcpServers": {"kakurega": server}});
    serde_json::to_string_pretty(&doc)
        .map(|t| t + "\n")
        .map_err(|e| e.to_string())
}

fn placement(client: &str) -> &'static str {
    match client {
        "claude-code" => "Run the line above in a terminal.",
        "claude-desktop" => "Merge it into claude_desktop_config.json (macOS: ~/Library/Application Support/Claude/, Windows: %APPDATA%\\Claude\\, Linux: ~/.config/Claude/), keeping any servers already there.",
        "cursor" => "Merge it into the mcpServers of your Cursor mcp.json (a project's is .cursor/mcp.json), keeping any servers already there.",
        _ => "Merge it into the mcpServers block of your client's configuration, keeping any servers already there.",
    }
}

pub fn run(args: &[String]) -> u8 {
    let choice = match parse(args) {
        Ok(c) => c,
        Err(why) => {
            eprintln!("{why}\n{USAGE}");
            return 2;
        }
    };
    let binary = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot find this program's own path: {e}");
            return 1;
        }
    };
    let bytes = match std::fs::read(&binary) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {} to take its digest: {e}", binary.display());
            return 1;
        }
    };
    let text = match snippet(&choice, &binary) {
        Ok(t) => t,
        Err(why) => {
            eprintln!("{why}");
            return 2;
        }
    };
    print!("{text}");
    eprintln!("binary: {}", binary.display());
    eprintln!("sha256: {}", kk_evidence::capsule::sha256_hex(&bytes));
    eprintln!("The client will run exactly that file, with your privileges, every time it starts. If you did not install kaku yourself, check the digest against one you trust before using this.");
    if !choice.read_only && !choice.leased && choice.allow.is_none() {
        eprintln!("No limit was chosen, so the agent is offered every tool. Add --read-only, --allow TOOL,TOOL or --leased to narrow it.");
    }
    eprintln!("Nothing was written. {}", placement(&choice.client));
    0
}
