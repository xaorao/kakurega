mod approve;
mod extension;
mod log;
mod mcp;
mod runner;
mod serve;
mod tui;
mod verify;

use std::process::ExitCode;

const USAGE: &str = "kaku: private code review with checkable results

Usage:
  kaku [tui]                        open the full-screen workspace (starts the service if needed)
  kaku serve [--data-dir DIR] [--ui-dir DIR] [--root DIR]... [--model NAME] [--ollama URL]
             [--transport lima|local] [--vm NAME] [--worker-bin PATH] [--launcher PATH]
  kaku pair                         print a new one-time pairing link for the browser
  kaku approve [APPROVAL-ID]        review the exact release in this terminal and approve it
  kaku mcp                          speak MCP on stdin and stdout for an agent client (cannot approve)
  kaku extension pair|status|revoke  connect the browser extension (it can only queue suggestions)
  kaku log export [--out FILE]      save the signed event log
  kaku log verify FILE [--pin KEY] [--anchor COUNT:HEAD]   check a saved log offline
  kaku verify CAPSULE [--pin KEY]   check a capsule offline (KEY: base64url public key)

Options for every command: --data-dir DIR (default ~/.kakurega)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("serve") => serve::run(&args[1..]),
        Some("pair") => approve::pair(&args[1..]),
        Some("approve") => approve::run(&args[1..]),
        Some("verify") => verify::run(&args[1..]),
        Some("mcp") => mcp::run(&args[1..]),
        Some("log") => log::run(&args[1..]),
        Some("extension") => extension::run(&args[1..]),
        Some("tui") => tui::run(&args[1..]),
        None if std::io::IsTerminal::is_terminal(&std::io::stdout())
            && std::io::IsTerminal::is_terminal(&std::io::stdin()) =>
        {
            tui::run(&args)
        }
        Some("--help") | Some("-h") | None => {
            println!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("unknown command `{other}`\n\n{USAGE}");
            2
        }
    };
    ExitCode::from(code)
}

pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

pub fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1))
        .cloned()
        .collect()
}

pub fn data_dir(args: &[String]) -> std::path::PathBuf {
    flag(args, "--data-dir")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".kakurega")
        })
}
