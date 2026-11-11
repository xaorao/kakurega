mod agent;

/// Tests that change the process environment (proxy variables) and tests that make network calls to this computer take this lock,
/// so they never run at the same moment.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
mod approve;
mod approver;
mod backup;
mod chain;
mod config;
mod diagnostics;
mod doctor;
mod extension;
mod log;
mod mcp;
mod mcp_install;
#[cfg(test)]
mod meaning_set;
mod memory_import;
mod near;
mod ocr;
mod outside;
mod pack;
mod parse_child;
#[cfg(test)]
mod recall_bench;
mod runner;
mod script;
mod serve;
mod telegram;
mod tui;
mod update;
mod verify;

use std::process::ExitCode;

const USAGE: &str = "kaku: private code review with checkable results

Usage:
  kaku [tui] [--resume] [--linear]  open the full-screen workspace (starts the service if needed); --resume opens the newest kept conversation, --resume ID opens that one (ids: kaku sessions), --resume alone opens the newest of this folder's project, --fork opens a branch instead;
                                    --linear (or KAKU_LINEAR=1) is plain lines in and out for screen readers and pipes;
                                    the theme setting (dark or light, in config.toml or KAKU_THEME) uses fixed colours measured for that background
  kaku serve [--require-approver] [--data-dir DIR] [--ui-dir DIR] [--root DIR]... [--model NAME] [--ollama URL] [--context TOKENS]
             [--route near --near-model NAME [--near-key-file FILE] [--near-url URL]]   answer on NEAR AI Cloud's confidential hardware (the key: NEAR_AI_API_KEY or the file)
             [--route outside --outside-model anthropic:MODEL]   for testing only: answer chat, roundtable and team questions with a hosted model that is NOT private (the key: ANTHROPIC_API_KEY); reviews stay on the local model
             [--transport lima|local] [--vm NAME] [--worker-bin PATH] [--launcher PATH]
  kaku review DIR --for RECIPIENT [--json] [--timeout SECONDS]   start a review and follow it; ends at awaiting_approval (exit 0), 3 nothing to release, 4 failed
  kaku code DIR TASK... [--json]                          a local model changes the project in memory, its tests run in the sandbox; shows the patch, applies only when you say
  kaku code show|apply|undo ID, kaku code list              read, apply (--digest from the patch) or undo a coding task
  kaku jobs [--json] | kaku findings JOB [--json] | kaku follow JOB [--timeout S]   list reviews, read what one found, watch one until it ends
  kaku ask QUESTION --folder DIR [--json]   ask about the documents in a folder; every quote is checked (exit 3 if any is not found)
  kaku chat MESSAGE|- [--json] [--keep | --session ID]   one message to the private assistant; nothing is kept unless --keep starts a kept conversation or --session continues one
  kaku team plan GOAL [--experts ID,ID] | list | show ID | start ID [--wait] | stop ID | answer ID TASK TEXT | delete ID   a lead splits a goal into tasks for experts; nothing runs until you start the plan
  kaku stop ID                      stop a running review (job-N), coding task (code-N) or team run (team-...)
  kaku work [--json]                what is working now (reviews, coding tasks, team runs) and what has gone quiet
  kaku roundtable QUESTION --experts ID,ID[,ID...] [--json]   one question to 2 to 5 experts, each alone, then a lead's resolution in four parts (kaku roundtable --list names the experts)
  kaku sessions [--json]            list the kept conversations; `kaku sessions export ID [--out FILE]` writes one as Markdown; these commands cannot approve anything
  kaku pair                         print a new one-time pairing link for the browser
  kaku approve [APPROVAL-ID]        review the exact release in this terminal and approve it
  kaku mcp [--read-only] [--allow TOOL,TOOL] [--leased]   speak MCP on stdin and stdout for an agent client (cannot approve; --read-only hides the tools that start a review or ask for an approval; --allow gives the agent exactly the tools named; --calls-per-minute N, --starts-per-hour N and --approvals-per-hour N lower the limits, which are 60, 4 and 4; an answer over 256 KiB is not sent)
  kaku telegram set-token | clear-token   keep the bot token in the encrypted store (piped in, never on the command line)
  kaku telegram pair | kaku telegram [--clear-webhook]   notifications and counts on your phone (never contents); --clear-webhook removes a webhook that blocks polling
  kaku approver enroll|list|revoke   key that signs approvals (required once enrolled)
  kaku mcp install --client claude-code|claude-desktop|cursor|generic --print [limits]   print the exact text that makes an MCP client start this server (full path, your limits, no token; it never edits the client)
  kaku agent grant TOOL [--uses N] [--minutes M] | revoke [TOOL] | status   lease the tools that act to an agent started with `kaku mcp --leased`
  kaku extension pair|status|revoke  connect the browser extension (it can only queue suggestions)
  kaku backup [--out FILE]          save an encrypted copy of all your data; prints a recovery key once
  kaku restore FILE [--data-dir DIR] restore a backup into an empty folder (asks for the recovery key)
  kaku memory import DIR [--source NAME]   add notes from another assistant's markdown files as untrusted notes
  kaku update check|verify-file|keygen|sign   signed update announcements (nothing is downloaded or installed)
  kaku config [show|path|check|init] [--json]   your display settings in config.toml (notify, color, quiet, linear): what is in force and where it came from, check the file, or write a commented template
  kaku doctor [--json] [--fix] [--skip ID,ID]   why Kakurega does not start or work, with nothing running: data folder, service, sandbox, model; --fix repairs only local permissions and stale files
  kaku diagnostics [--out FILE]     a readable report of versions, checks and counts to attach to a bug report (no names, paths or text)
  kaku log export [--out FILE]      save the signed event log
  kaku log verify FILE [--pin KEY] [--anchor COUNT:HEAD]   check a saved log offline
  kaku share JOB FINDING [--out FILE]   one finding of a delivered review as a file with a proof; it shows nothing of the rest of the release
  kaku notes FILE [--title TEXT] [--out FILE]   notes from a meeting transcript; a point is kept only when its quote is in the transcript
  kaku verify FILE [--pin KEY] [--out DIR]   check a capsule or a share offline (KEY: base64url public key); --out writes a share's files into a new folder
  kaku chain hardware KEY | capsule FILE | order ID   what NEAR holds: a model key's hardware record, who registered a capsule, an order (reads only; --network, --registry, --json)
  kaku chain attest --model NAME --key-file FILE   have NEAR's own verifier check that model's hardware proof and record its signing key (testnet; the account in the key file pays)
  kaku chain register JOB --key-file FILE [--order ID]   publish a delivered review's capsule digest and model receipts on NEAR, after showing exactly what becomes public (testnet)
  kaku chain quote --pay-with SYMBOL@chain --usdc AMOUNT ...   a price from NEAR Intents for paying an order from another chain, with the order's terms as the transfer's message (a dry quote: nothing is sent)

Options for every command: --data-dir DIR (default ~/.kakurega)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("serve") => serve::run(&args[1..]),
        // started by the service to read one attached file; not for people to run
        Some("parse-attachment") => return parse_child::child(&args[1..]),
        // internal: records a deliberate crash in the folder given, so the suite can read what a real one leaves behind
        Some("crash-probe") => {
            let dir = std::path::PathBuf::from(args.get(1).cloned().unwrap_or_default());
            kk_service::crash::install(&dir);
            let key = format!("{}{}", "sk_li", "ve_abcdefghijklmnop0123");
            panic!("probe: could not read /Users/someone/private/notes.txt holding {key}");
        }
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
        Some("diagnostics") => diagnostics::run(&args[1..]),
        Some("config") => config::run(&args[1..]),
        Some("doctor") => doctor::run(&args[1..]),
        Some("agent") => agent::run(&args[1..]),
        Some("extension") => extension::run(&args[1..]),
        Some("approver") => approver::run(&args[1..]),
        Some("telegram") => telegram::run(&args[1..]),
        Some("tui") => tui::run(&args[1..]),
        Some("jobs") => script::jobs(&args[1..]),
        Some("review") => script::review(&args),
        Some("findings") => script::findings(&args),
        Some("follow") => script::follow(&args),
        Some("ask") => script::ask(&args),
        Some("code") => script::code(&args),
        Some("chat") => script::chat(&args),
        Some("roundtable") => script::roundtable(&args),
        Some("team") => script::team(&args),
        Some("work") => script::work(&args),
        Some("stop") => script::stop(&args),
        Some("share") => script::share(&args),
        Some("notes") => script::notes(&args),
        Some("sessions") => script::sessions(&args[1..]),
        Some("update") => update::run(&args[1..]),
        Some("pack") => pack::run(&args[1..]),
        Some("chain") => chain::run(&args[1..]),
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
