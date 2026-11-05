mod approve;
mod approver;
mod backup;
mod extension;
mod log;
mod mcp;
mod memory_import;
mod runner;
mod serve;
mod telegram;
mod tui;
mod update;
mod verify;

use std::process::ExitCode;

const USAGE: &str = "kaku: private code review with checkable results

Usage:
  kaku [tui]                        open the full-screen workspace (starts the service if needed)
  kaku serve [--data-dir DIR] [--ui-dir DIR] [--root DIR]... [--model NAME] [--ollama URL] [--context TOKENS]
             [--transport lima|local] [--vm NAME] [--worker-bin PATH] [--launcher PATH]
  kaku pair                         print a new one-time pairing link for the browser
  kaku approve [APPROVAL-ID]        review the exact release in this terminal and approve it
  kaku mcp [--read-only] [--allow TOOL,TOOL]   speak MCP on stdin and stdout for an agent client (cannot approve; --read-only hides the tools that start a review or ask for an approval; --allow gives the agent exactly the tools named)
  kaku telegram pair | kaku telegram   notifications and counts on your phone (never contents)
  kaku approver enroll|list|revoke   key that signs approvals (required once enrolled)
  kaku extension pair|status|revoke  connect the browser extension (it can only queue suggestions)
  kaku backup [--out FILE]          save an encrypted copy of all your data; prints a recovery key once
  kaku restore FILE [--data-dir DIR] restore a backup into an empty folder (asks for the recovery key)
  kaku memory import DIR [--source NAME]   add notes from another assistant's markdown files as untrusted notes
  kaku update check|verify-file|keygen|sign   signed update announcements (nothing is downloaded or installed)
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
        Some("backup") => backup::make(&args[1..]),
        Some("restore") => backup::restore(&args[1..]),
        Some("memory") if args.get(1).map(String::as_str) == Some("import") => {
            memory_import::run(&args[2..])
        }
        Some("log") => log::run(&args[1..]),
        Some("extension") => extension::run(&args[1..]),
        Some("approver") => approver::run(&args[1..]),
        Some("telegram") => telegram::run(&args[1..]),
        Some("tui") => tui::run(&args[1..]),
        Some("update") => update::run(&args[1..]),
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
