mod agent;

/// Tests that change the process environment (proxy variables) and tests that make network calls to this computer take this lock,
/// so they never run at the same moment.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
mod approve;
mod approver;
mod backup;
mod bench;
mod calendar;
mod chain;
mod config;
mod diagnostics;
mod doctor;
mod extension;
mod github;
mod handoff;
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
mod profile;
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
             [--release-days N]   how long a release stays valid for delivery and checking (1 to 730 days; 1 unless given)
             [--route near --near-model NAME [--near-key-file FILE] [--near-url URL]]   answer on NEAR AI Cloud's confidential hardware (the key: NEAR_AI_API_KEY or the file)
             [--route outside --outside-model anthropic:MODEL]   for testing only: answer chat, roundtable and team questions with a hosted model that is NOT private (the key: ANTHROPIC_API_KEY); reviews stay on the local model
             [--transport lima|local] [--vm NAME] [--worker-bin PATH] [--launcher PATH]
  kaku review DIR --for RECIPIENT [--json] [--timeout SECONDS]   start a review and follow it; ends at awaiting_approval (exit 0), 3 nothing to release, 4 failed
  kaku code DIR TASK... [--json]                          the model changes the project in memory, its tests run in the sandbox; shows the patch, applies only when you say
  kaku code show|apply|undo ID, kaku code list              read, apply (--digest from the patch) or undo a coding task
  kaku jobs [--json] | kaku findings JOB [--json] | kaku follow JOB [--timeout S]   list reviews, read what one found, watch one until it ends
  kaku explore DIR QUESTION... [--depth quick|normal|thorough] [--json]   an agent reads the project and answers (with the model here or in confidential hardware); every line it rests on is checked (exit 3 if one is not found)
  kaku explore show ID | list | forget ID          read, list or forget questions about projects
  kaku ask QUESTION --folder DIR [--json]   ask about the documents in a folder (the web's Ask your documents; for Ask a project use kaku explore); every quote is checked (exit 3 if any is not found)
  kaku chat MESSAGE|- [--json] [--keep | --session ID]   one message to the private assistant; nothing is kept unless --keep starts a kept conversation or --session continues one
  kaku team plan GOAL [--experts ID,ID] | list | show ID | start ID [--wait] | stop ID | answer ID TASK TEXT | delete ID   a lead splits a goal into tasks for experts; nothing runs until you start the plan
  kaku stop ID                      stop a running review (job-N), coding task (code-N), project question (question-N) or team run (team-...)
  kaku work [--json]                what is working now (reviews, coding tasks, project questions, team runs) and what has gone quiet
  kaku roundtable QUESTION --experts ID,ID[,ID...] [--json]   one question to 2 to 5 experts, each alone, then a lead's resolution in four parts (kaku roundtable --list names the experts)
  kaku sessions [--json]            list the kept conversations; `kaku sessions export ID [--out FILE]` writes one as Markdown; these commands cannot approve anything
  kaku pair                         print a new one-time pairing link for the browser
  kaku approve [APPROVAL-ID]        review the exact release in this terminal and approve it
  kaku mcp [--read-only] [--allow TOOL,TOOL] [--leased]   speak MCP on stdin and stdout for an agent client (cannot approve; --read-only hides the tools that start a review or ask for an approval; --allow gives the agent exactly the tools named; --calls-per-minute N, --starts-per-hour N and --approvals-per-hour N lower the limits, which are 60, 4 and 4; an answer over 256 KiB is not sent)
  kaku telegram set-token | clear-token   keep the bot token in the encrypted store (piped in, never on the command line)
  kaku telegram pair | unpair | status | doctor | kaku telegram [--clear-webhook]   notifications and counts on your phone (never contents); --clear-webhook removes a webhook that blocks polling
  kaku approver enroll|list|revoke   key that signs approvals (required once enrolled)
  kaku mcp install --client claude-code|claude-desktop|cursor|generic --print [limits]   print the exact text that makes an MCP client start this server (full path, your limits, no token; it never edits the client)
  kaku agent grant TOOL [--uses N] [--minutes M] | revoke [TOOL] | status   lease the tools that act to an agent started with `kaku mcp --leased`
  kaku extension pair|status|revoke  connect the browser extension (it can only queue suggestions)
  kaku backup [--out FILE]          save an encrypted copy of all your data; prints a recovery key once
  kaku restore FILE [--data-dir DIR] restore a backup into an empty folder (asks for the recovery key)
  kaku memory import DIR [--source NAME]   add notes from another assistant's markdown files as untrusted notes
  kaku update check|verify-file|keygen|sign   signed update announcements (nothing is downloaded or installed)
  kaku pack keygen|sign|check|catalog|check-catalog   make and check signed method packs and their catalog (for a publisher; `kaku pack` alone shows each form)
  kaku config [show|path|check|init] [--json]   your display settings in config.toml (notify, color, quiet, linear): what is in force and where it came from, check the file, or write a commented template
  kaku doctor [--json] [--fix] [--skip ID,ID]   why Kakurega does not start or work, with nothing running: data folder, service, sandbox, model; --fix repairs only local permissions and stale files
  kaku diagnostics [--out FILE]     a readable report of versions, checks and counts to attach to a bug report (no names, paths or text)
  kaku log export [--out FILE]      save the signed event log
  kaku log verify FILE [--pin KEY] [--anchor COUNT:HEAD]   check a saved log offline
  kaku share JOB FINDING [--out FILE]   one finding of a delivered review as a file with a proof; it shows nothing of the rest of the release
  kaku notes FILE [--title TEXT] [--out FILE]   notes from a meeting transcript; a point is kept only when its quote is in the transcript
  kaku bench sign|verify FILE          sign the leak benchmark's results, or check a signed results file against a pinned key
  kaku verify FILE [--pin KEY] [--out DIR]   check a capsule or a share offline (KEY: the signer's key, or the 'signed by' line a check prints); --out writes the released files into a new folder, to read them and re-run the checks
  kaku chain hardware KEY | capsule FILE | order ID   what NEAR holds: a model key's hardware record, who registered a capsule, an order (reads only; --network, --registry, --json)
  kaku chain order open ... | fees | storage ACCOUNT   set NEAR aside for a review, what the registry takes and owes, NEAR set aside for token orders (`kaku chain` alone lists every option)
  kaku chain attest --model NAME --key-file FILE   have NEAR's own verifier check that model's hardware proof and record its signing key (testnet; the account in the key file pays)
  kaku chain register JOB --key-file FILE [--order ID]   publish a delivered review's capsule digest and model receipts on NEAR, after showing exactly what becomes public (testnet)
  kaku chain quote --pay-with SYMBOL@chain --usdc AMOUNT ...   a price from NEAR Intents for paying an order from another chain, with the order's terms as the transfer's message (a dry quote: nothing is sent)

  kaku profile [list] | new NAME | use NAME | current | remove NAME [--confirm NAME]   separate sets of keys, memory, skills and settings

Options for every command: --profile NAME (a profile made with `kaku profile new`; with ask, chat, roundtable, team, code and notes put it before the command or right after it, since what follows is your own text), or --data-dir DIR (default ~/.kakurega, the profile `default`)";

/// The usage lines of one command, with the lines that continue them, and the options for every command; `None` when the
/// page has no line for it.
fn usage_of(command: &str) -> Option<String> {
    let start = format!("  kaku {command} ");
    let start_bare = format!("  kaku {command}");
    let mut out: Vec<&str> = Vec::new();
    let mut taking = false;
    for line in USAGE.lines() {
        if line.starts_with("  kaku ") {
            taking = line.starts_with(&start) || line == start_bare;
        } else if !line.starts_with("    ") {
            taking = false;
        }
        if taking {
            out.push(line);
        }
    }
    if out.is_empty() {
        return None;
    }
    let every = USAGE
        .lines()
        .find(|l| l.starts_with("Options for every command"));
    Some(format!(
        "Usage:\n{}{}",
        out.join("\n"),
        every.map(|e| format!("\n\n{e}")).unwrap_or_default()
    ))
}

/// Where `--profile` stands as an option. After a command that takes free text ("kaku chat explain the --profile flag")
/// the words are the owner's: there it is an option only when it stands before the command or right after it.
fn profile_option(args: &[String]) -> Option<usize> {
    let free_text = args.first().is_some_and(|c| {
        matches!(
            c.as_str(),
            "ask" | "chat" | "roundtable" | "team" | "code" | "notes"
        )
    });
    args.iter()
        .position(|a| a == "--profile")
        .filter(|i| *i <= 1 || !free_text)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--profile NAME` may stand anywhere. It is taken out of the arguments here, checked, and handed on as the
    // KAKU_PROFILE variable, which `data_dir` reads: no command sees an extra word among its own, and a service this
    // command starts works in the same profile. `--data-dir` still wins over it.
    if let Some(i) = profile_option(&args) {
        let Some(name) = args.get(i + 1).cloned() else {
            eprintln!("--profile needs a profile's name");
            return ExitCode::from(2);
        };
        args.drain(i..=i + 1);
        if let Err(why) = profile::resolve(&profile::home(), None, Some(name.clone()), None) {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
        // set before any thread exists
        std::env::set_var("KAKU_PROFILE", name.trim());
    }
    // An option that takes a value and was given none is a mistake to say, not to ignore: `follow job-1 --timeout --json`
    // would otherwise wait for ever, and `log export --out` would print to the screen.
    if let Some(name) = option_without_value(&args) {
        eprintln!("{name} needs a value after it. Run kaku --help for the form of each command.");
        return ExitCode::from(2);
    }
    // a help flag after a command asks for help, never for the command to run (`kaku serve --help` must not start a service).
    // Commands that take free text or run another program keep their arguments as they are.
    // a free-text command given only a help flag asks for help: "--help" is not a question anyone means to send
    let only_help = args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h");
    let wants_help = only_help
        || args.len() > 1
            && !matches!(
                args[0].as_str(),
                "parse-attachment"
                    | "crash-probe"
                    | "ask"
                    | "code"
                    | "notes"
                    | "mcp"
                    | "chat"
                    | "roundtable"
                    | "team"
                    | "explore"
            )
            && args[1..].iter().any(|a| a == "--help" || a == "-h");
    let first = if wants_help {
        Some("--help")
    } else {
        args.first().map(String::as_str)
    };
    let code = match first {
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
        Some("profile") => profile::run(&args[1..]) as u8,
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
        Some("explore") => script::explore(&args),
        Some("chat") => script::chat(&args),
        Some("roundtable") => script::roundtable(&args),
        Some("team") => script::team(&args),
        Some("work") => script::work(&args),
        Some("stop") => script::stop(&args),
        Some("share") => script::share(&args),
        Some("bench") => bench::run(&args[1..]),
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
        // the workspace's own options without the word `tui`, as the usage line writes them: `kaku --resume`,
        // `kaku --linear`, `kaku --data-dir DIR`; and plain-line mode asked for by its variable, which needs no terminal
        Some(
            "--resume" | "--linear" | "--fork" | "--data-dir" | "--root" | "--model" | "--ollama"
            | "--profile",
        ) => tui::run(&args),
        None if std::env::var("KAKU_LINEAR").is_ok_and(|v| v == "1") => tui::run(&args),
        Some("--version") | Some("-V") | Some("version") => {
            println!("kaku {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help") | Some("-h") | None => {
            match args
                .first()
                .filter(|_| wants_help)
                .and_then(|c| usage_of(c))
            {
                Some(lines) => println!("{lines}"),
                None => println!("{USAGE}"),
            }
            0
        }
        Some(other) => {
            eprintln!("unknown command `{other}`\n\n{USAGE}");
            2
        }
    };
    ExitCode::from(code)
}

/// What a command printed as a hint needs after it to reach the same data as the command that prints it: the data folder
/// when one was named, else the profile when one is in use, else nothing. Without it a copied hint would look in the
/// default place and say that nothing is running.
pub fn here(args: &[String]) -> String {
    if let Some(dir) = flag(args, "--data-dir") {
        // as a full path, so the hint works from any folder
        let full = std::fs::canonicalize(&dir)
            .map(|p| p.display().to_string())
            .unwrap_or(dir);
        return format!(" --data-dir {}", shell_word(&full));
    }
    match std::env::var("KAKU_PROFILE") {
        Ok(name) if !name.trim().is_empty() => format!(" --profile {}", shell_word(name.trim())),
        _ => String::new(),
    }
}

/// Every option that is followed by a value.
const VALUE_OPTIONS: &[&str] = &[
    "--account",
    "--allow",
    "--anchor",
    "--api-base",
    "--approval",
    "--catalog",
    "--channel",
    "--context",
    "--current",
    "--data-dir",
    "--days",
    "--deposit",
    "--depth",
    "--digest",
    "--dir",
    "--experts",
    "--file",
    "--folder",
    "--for",
    "--hours",
    "--key",
    "--key-file",
    "--label",
    "--launcher",
    "--manifest",
    "--model",
    "--name",
    "--near-key-file",
    "--near-model",
    "--near-url",
    "--network",
    "--ocr",
    "--ollama",
    "--order",
    "--out",
    "--outside-model",
    "--outside-url",
    "--pack",
    "--pin",
    "--platform",
    "--profile",
    "--project",
    "--registry",
    "--release-days",
    "--root",
    "--route",
    "--scripted",
    "--session",
    "--skip",
    "--source",
    "--timeout",
    "--title",
    "--token-file",
    "--transport",
    "--trusted",
    "--ui-dir",
    "--vm",
    "--worker-bin",
];

/// The first option that needs a value and has none: it is followed by another option, or by nothing. After a command
/// whose words are the person's own text (a question, a message), only "followed by another option" counts, since the
/// last word of a sentence may happen to be an option's name.
fn option_without_value(args: &[String]) -> Option<&str> {
    let own_text = matches!(
        args.first().map(String::as_str),
        Some("ask" | "chat" | "roundtable" | "team" | "code" | "notes" | "explore")
    );
    args.iter().enumerate().find_map(|(i, a)| {
        if !VALUE_OPTIONS.contains(&a.as_str()) {
            return None;
        }
        match args.get(i + 1) {
            Some(next) if next.starts_with("--") => Some(a.as_str()),
            None if !own_text => Some(a.as_str()),
            _ => None,
        }
    })
}

/// A path or a name as one word of a command a person can copy: quoted when it holds a space or a shell character.
pub fn shell_word(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-~+:@%".contains(c))
    {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The value given after `name`. Another option is never taken as the value (`--for --data-dir X` names no recipient):
/// the commands that need a value then say it is missing instead of working with the wrong one.
pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|v| !v.starts_with("--"))
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

/// The data folder this command works in: `--data-dir`, else the profile asked for or chosen (see `profile.rs`). A profile
/// that was asked for by name and does not exist ends the command; it never falls back to another profile's data.
pub fn data_dir(args: &[String]) -> std::path::PathBuf {
    match profile::resolve(
        &profile::home(),
        flag(args, "--data-dir"),
        flag(args, "--profile"),
        std::env::var("KAKU_PROFILE").ok(),
    ) {
        // a folder named relative to where the command runs is made whole, so every part (the sandbox machine included,
        // which sees paths as they are written) finds the same folder
        Ok((dir, _)) if dir.is_relative() => std::env::current_dir()
            .map(|here| here.join(&dir))
            .unwrap_or(dir),
        Ok((dir, _)) => dir,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod profile_option_tests {
    use super::profile_option;

    #[test]
    fn the_option_is_read_anywhere_except_inside_a_free_text_command() {
        let of =
            |line: &str| profile_option(&line.split(' ').map(String::from).collect::<Vec<_>>());
        assert_eq!(of("--profile work config path"), Some(0));
        assert_eq!(of("config --profile work path"), Some(1));
        assert_eq!(of("serve --profile work"), Some(1));
        assert_eq!(of("--profile work chat hello there"), Some(0));
        assert_eq!(of("chat --profile work hello there"), Some(1));
        assert_eq!(of("chat explain the --profile flag"), None);
        assert_eq!(of("ask what does --profile work do"), None);
        assert_eq!(of("status"), None);
    }
}

#[cfg(test)]
mod usage_tests {
    #[test]
    fn a_commands_help_is_its_own_lines() {
        let chat = super::usage_of("chat").unwrap();
        assert!(
            chat.contains("kaku chat MESSAGE")
                && !chat.contains("kaku review")
                && chat.contains("Options for every command"),
            "{chat}"
        );
        let serve = super::usage_of("serve").unwrap();
        assert!(
            serve.contains("--route near")
                && serve.contains("--transport lima|local")
                && !serve.contains("kaku review"),
            "{serve}"
        );
        let explore = super::usage_of("explore").unwrap();
        assert!(
            explore.contains("kaku explore DIR") && explore.contains("kaku explore show ID"),
            "{explore}"
        );
        assert!(super::usage_of("nonsense").is_none());
    }
}

#[cfg(test)]
mod option_tests {
    use super::option_without_value;

    fn a(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn mn_use01_an_option_given_no_value_is_named_and_a_complete_command_is_not() {
        assert_eq!(
            option_without_value(&a(&["follow", "job-1", "--timeout", "--json"])),
            Some("--timeout")
        );
        assert_eq!(
            option_without_value(&a(&["log", "export", "--out"])),
            Some("--out")
        );
        assert_eq!(
            option_without_value(&a(&["chat", "--session", "--json", "hello"])),
            Some("--session")
        );
        assert_eq!(
            option_without_value(&a(&["explore", "dir", "--depth", "--json", "why"])),
            Some("--depth")
        );
        for ok in [
            &["follow", "job-1", "--timeout", "30", "--json"][..],
            &["verify", "x.kkg", "--pin", "abc", "--out", "dir"],
            &["chat", "what", "does", "--timeout"],
            &["serve", "--data-dir", "/tmp/x", "--root", "/tmp/y"],
            &["jobs", "--json"],
        ] {
            assert_eq!(option_without_value(&a(ok)), None, "{ok:?}");
        }
    }
}
