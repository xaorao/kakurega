//! What the kernel tells the model, inside the session, about a finding or a fix it just reported.
//!
//! Every answer is `{status, note}` and is chosen from a fixed table. Nothing the script printed, nothing from its error stream
//! and no text taken from a project file is ever put in it: the verifier's own messages are not forwarded either (two of them
//! carry the debug form of a status, which can hold error text), they are recognised by their fixed opening words and answered
//! with the wording here. The name of a withheld file is never given.

use crate::check::{msg, CheckResult, CheckStatus, FixStatus};
use serde_json::{json, Value};
use std::time::Duration;

/// The checks a finding (a title in a file), and a fix title, get in one session before the answer is `not_checked`.
pub const MAX_ATTEMPTS_PER_FINDING: usize = 2;
/// The findings, and the fixes, checked in one session. The last version of each title is still checked once when the session ends.
pub const MAX_CHECKED: usize = 8;
/// The time the checks may take in one session; the model waits while they run.
pub const MAX_TIME_SPENT: Duration = Duration::from_secs(600);

fn verdict(status: &str, note: &str) -> Value {
    json!({"status": status, "note": note})
}

fn starts(m: &str, prefix: &str) -> bool {
    m.starts_with(prefix)
}

/// The answer to a finding whose reproducer was run, using what the run showed. Of the script's output only whether two exact marker
/// lines were there is used (never its text), and only to tell apart the two ways a script that ran can fail to show the defect:
/// it concluded there is none, or it ended without either verdict line.
pub fn finding_of(result: &CheckResult) -> Value {
    if result.status != CheckStatus::NotReproduced {
        return finding(&result.status);
    }
    let line = |word: &str| result.stdout.lines().any(|l| l.trim() == word);
    let (control, concluded_no) = (line("CONTROL_OK"), line("NOT REPRODUCED"));
    match (concluded_no, control) {
        (true, true) => verdict(
            "not_reproduced",
            "The script ran, showed the normal behaviour working, and concluded the defect is not present. If you believe the defect is real, your reproducer is not exercising it: call the code path that has the defect, then report the finding again with the same title.",
        ),
        (true, false) => verdict(
            "not_reproduced",
            "The script printed NOT REPRODUCED without first printing CONTROL_OK. First exercise and assert the normal behaviour and print CONTROL_OK on its own line, then print your verdict; if the defect is real, make the script reach it. Report the finding again with the same title.",
        ),
        (false, true) => verdict(
            "not_reproduced",
            "The script printed CONTROL_OK but neither REPRODUCED nor NOT REPRODUCED. End it with exactly one of those two verdicts on its own line, then report the finding again with the same title.",
        ),
        (false, false) => verdict(
            "not_reproduced",
            "The script ran but printed neither CONTROL_OK nor a verdict. It must print CONTROL_OK on its own line after exercising the normal behaviour, then exactly one of REPRODUCED or NOT REPRODUCED on its own line. Report the finding again with the same title.",
        ),
    }
}

/// The answer to a finding whose reproducer was run.
pub fn finding(status: &CheckStatus) -> Value {
    match status {
        CheckStatus::Reproduced => verdict(
            "confirmed",
            "The kernel ran your script in a sealed sandbox: it printed CONTROL_OK, then REPRODUCED, gave the same output twice, and stopped printing REPRODUCED when the cited file was emptied. This finding is confirmed.",
        ),
        CheckStatus::NotReproduced => verdict(
            "not_reproduced",
            "The script ran but did not print a REPRODUCED line, so the defect was not shown. If it is real, correct the reproducer and call report_finding again with the same title; if not, leave it.",
        ),
        CheckStatus::Nondeterministic => verdict(
            "nondeterministic",
            "The two runs printed different output. Remove anything that varies (the time, random numbers, ids, ordering) and call report_finding again with the same title.",
        ),
        CheckStatus::Failed(m) => {
            if starts(m, msg::NO_CONTROL) {
                verdict("failed", "The script printed REPRODUCED without first printing CONTROL_OK. First exercise and assert the normal behaviour and print CONTROL_OK on its own line, then print REPRODUCED or NOT REPRODUCED on its own line.")
            } else if starts(m, msg::INDEPENDENT) {
                verdict("failed", "The script still printed REPRODUCED when the cited file was emptied, so its result does not depend on the code you named. Make the verdict come from calling that code.")
            } else if starts(m, msg::ENDED) {
                verdict("failed", "The script ended with an error or hit a limit. It must exit normally after printing its verdict lines, use no network, touch no file outside the project and finish quickly.")
            } else {
                verdict("failed", "The script could not be run. Check that it is complete and uses only the project's own files.")
            }
        }
        CheckStatus::Unavailable(m) => {
            if starts(m, msg::NO_REPRODUCER) {
                verdict("unavailable", "The finding has no reproducer. Add one, and report the finding again with the same title.")
            } else if starts(m, msg::SANDBOX_PATH) {
                verdict("unavailable", "The script names a path that exists only inside the sandbox. Find the project through the current folder instead, and report the finding again with the same title.")
            } else if starts(m, msg::WITHHELD_FILE) {
                verdict("unavailable", "The code the script needs depends on a file that is withheld from you, so this cannot be checked.")
            } else if starts(m, msg::NOT_IN_PROJECT) {
                verdict("unavailable", "The cited file is not in the project. Use the path exactly as list_files shows it.")
            } else if starts(m, msg::NEEDS_PACKAGE) {
                verdict("unavailable", "The script needs a package that is not available in the sandbox. Use only the project's own files.")
            } else {
                verdict("unavailable", "This finding cannot be checked in this environment. It stays reported, marked as not checked.")
            }
        }
    }
}

/// The answer to a fix whose effect was checked.
pub fn fix(status: &FixStatus) -> Value {
    match status {
        FixStatus::FixesTheReproducer => verdict(
            "verified",
            "After your change the script prints CONTROL_OK and NOT REPRODUCED, so the defect no longer shows. The fix is recorded.",
        ),
        FixStatus::StillReproduced => verdict(
            "still_reproduced",
            "After your change the reproducer still prints REPRODUCED, so the defect is still there. Propose a different change for the same finding, with the same title.",
        ),
        FixStatus::CouldNotApply(m) => {
            if starts(m, msg::FIX_FILE_MISSING) {
                verdict("could_not_apply", "A file the fix names is not in the project. Use the path exactly as list_files shows it.")
            } else if starts(m, msg::FIX_TEST_FILE) {
                verdict("could_not_apply", "A fix may not change a test file: a change to the tests it must pass proves nothing. Change the code, not the tests.")
            } else if starts(m, msg::FIX_ONCE) {
                verdict("could_not_apply", "The old_text must appear exactly once in the file. Copy it exactly from read_file, with enough surrounding lines to be unique, and propose the fix again with the same title.")
            } else {
                verdict("could_not_apply", "The change could not be applied. Check the file and the exact old_text.")
            }
        }
        FixStatus::NotChecked(m) => {
            if starts(m, msg::FINDING_NOT_REPRODUCED) {
                verdict("not_checked", "The finding this fix belongs to was not confirmed, so the fix cannot be checked. Correct the finding's reproducer first.")
            } else if starts(m, msg::NO_CONTROL_AFTER) {
                verdict("not_checked", "After your change the script no longer shows the normal behaviour working, so this cannot be told apart from removing the feature. Keep the behaviour and remove only the defect.")
            } else if starts(m, msg::NO_VERDICT_AFTER) {
                verdict("not_checked", "After your change the script ended without printing NOT REPRODUCED, so this cannot be told apart from the code ending the program early. Make the code finish normally.")
            } else {
                verdict("not_checked", "The check after your change could not be trusted, so the fix is recorded as not verified.")
            }
        }
        FixStatus::BreaksTests(_) => verdict(
            "breaks_tests",
            "The reproducer stops reproducing, but the project's own tests passed before your change and do not all pass after it. Propose a smaller change that keeps them passing, with the same title.",
        ),
    }
}

/// A finding the kernel could not read as a finding at all.
pub fn rejected_finding() -> Value {
    verdict(
        "rejected",
        "The finding was not accepted: it needs a title, a line, an explanation and the path of a file the project shows you, within the size limits.",
    )
}

/// A fix the kernel could not read as a fix at all.
pub fn rejected_fix() -> Value {
    verdict(
        "rejected",
        "The fix was not accepted: it needs the title of a reported finding, the path of a project file, the exact old_text and its replacement, within the size limits.",
    )
}

/// A fix whose changes cannot be made as one exact change of the file.
pub fn cannot_make_fix() -> Value {
    verdict(
        "could_not_apply",
        "The change could not be made as one exact change of the file. Copy the old_text exactly from read_file, and keep the changes in one place or a few separate ones that do not overlap.",
    )
}

/// A fix whose finding has not been reported (under that exact title) in this session.
pub fn no_such_finding() -> Value {
    verdict(
        "not_checked",
        "No finding with this exact title has been reported in this session. Report the finding first, then propose its fix with the same title.",
    )
}

/// A write past one of the bounds above: recorded, but not checked now.
pub fn not_checked_limit() -> Value {
    verdict(
        "not_checked",
        "Not checked now: a finding is checked at most twice and a session checks a limited number of findings and fixes in the time it has. The last version you reported is checked once more when you finish.",
    )
}
