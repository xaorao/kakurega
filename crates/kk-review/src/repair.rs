//! The loop that lets a model learn, inside its session, whether a finding it reported reproduces and whether a fix it proposed
//! works. The broker calls `Repair::on_write` after the worker records a finding or a fix; the answer (`feedback.rs`) goes back to
//! the model in the same tool result. When the session ends the same object answers for the report, so a result already computed
//! for identical bytes is never computed twice.
//!
//! Which attempt counts when a finding was reported more than once (the same title in the same file: reporting it again is how a
//! model corrects it): a confirmed one is never replaced by a later one that is not confirmed (a slip after success must not lose
//! the finding); otherwise the latest wins. Two findings that share a title in different files are two findings. Every distinct fix
//! stays in the report with its own status, as before; the owner sees which worked.

use crate::align::align_fix;
use crate::check::{
    consolidate, parse_finding, parse_fix, run_reproducer, verify_fix_with_tests, CheckEnv,
    CheckResult, CheckStatus, Finding, Fix, FixStatus,
};
use crate::feedback;
use crate::import::ImportedFile;
use kk_sandbox::Transport;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A finding as recorded: the bytes it came in (the key everything else is cached under), what it parsed to, what the sandbox said.
#[derive(Clone)]
pub struct CheckedFinding {
    pub bytes: Vec<u8>,
    pub finding: Finding,
    pub check: CheckResult,
}

/// A fix as checked: the change as it will be applied (aligned to the file and made one exact change), and what checking it said.
#[derive(Clone)]
pub struct CheckedFix {
    pub fix: Fix,
    pub status: FixStatus,
    pub tests: String,
}

pub struct Repair {
    known: BTreeSet<String>,
    files: Vec<ImportedFile>,
    transport: Transport,
    scratch: PathBuf,
    python: String,
    node: String,
    findings: HashMap<Vec<u8>, CheckedFinding>,
    fixes: HashMap<(Vec<u8>, Vec<u8>), CheckedFix>,
    /// The finding as it stands, by (title, file), and the order those were first reported in.
    latest: HashMap<(String, String), CheckedFinding>,
    order: Vec<(String, String)>,
    finding_attempts: HashMap<(String, String), usize>,
    fix_attempts: HashMap<String, usize>,
    checked_findings: usize,
    checked_fixes: usize,
    spent: Duration,
}

/// Whether a result for a title should replace the one already kept.
fn replaces(old: &CheckStatus, new: &CheckStatus) -> bool {
    !(*old == CheckStatus::Reproduced && *new != CheckStatus::Reproduced)
}

impl Repair {
    pub fn new(
        known: BTreeSet<String>,
        files: Vec<ImportedFile>,
        transport: Transport,
        scratch: PathBuf,
        python: String,
        node: String,
    ) -> Repair {
        Repair {
            known,
            files,
            transport,
            scratch,
            python,
            node,
            findings: HashMap::new(),
            fixes: HashMap::new(),
            latest: HashMap::new(),
            order: Vec::new(),
            finding_attempts: HashMap::new(),
            fix_attempts: HashMap::new(),
            checked_findings: 0,
            checked_fixes: 0,
            spent: Duration::ZERO,
        }
    }

    fn within_bounds(&self, attempts: usize, checked: usize) -> bool {
        attempts < feedback::MAX_ATTEMPTS_PER_FINDING
            && checked < feedback::MAX_CHECKED
            && self.spent < feedback::MAX_TIME_SPENT
    }

    /// The finding for these bytes: the one already checked, or checked now. None when the bytes are not a finding.
    pub fn finding(&mut self, bytes: &[u8]) -> Option<CheckedFinding> {
        if let Some(hit) = self.findings.get(bytes) {
            return Some(hit.clone());
        }
        let finding = parse_finding(bytes, &self.known)?;
        let check = {
            let index: BTreeMap<String, &ImportedFile> =
                self.files.iter().map(|f| (f.path.clone(), f)).collect();
            let env = CheckEnv {
                transport: &self.transport,
                scratch: &self.scratch,
                python: &self.python,
                node: &self.node,
            };
            let started = Instant::now();
            let check = run_reproducer(&finding, &index, &env);
            self.spent += started.elapsed();
            check
        };
        let checked = CheckedFinding {
            bytes: bytes.to_vec(),
            finding,
            check,
        };
        self.findings.insert(bytes.to_vec(), checked.clone());
        Some(checked)
    }

    /// The fix for these bytes against this finding: the one already checked, or checked now. None when the bytes are not a fix or
    /// its changes cannot be made as one exact change; the second value says which.
    pub fn fix(
        &mut self,
        bytes: &[u8],
        against: &CheckedFinding,
    ) -> Result<CheckedFix, FixRefusal> {
        let key = (bytes.to_vec(), against.bytes.clone());
        if let Some(hit) = self.fixes.get(&key) {
            return Ok(hit.clone());
        }
        let parsed = parse_fix(bytes, &self.known).ok_or(FixRefusal::NotAFix)?;
        let index: BTreeMap<String, &ImportedFile> =
            self.files.iter().map(|f| (f.path.clone(), f)).collect();
        // a change that differs from the file only in look-alike characters, line endings or the ends of lines is made to fit it,
        // and several changes in one file become one exact change; a set that cannot be made is not offered
        let aligned = align_fix(&index, parsed);
        let fix = consolidate(&index, aligned).map_err(|_| FixRefusal::CannotMake)?;
        let env = CheckEnv {
            transport: &self.transport,
            scratch: &self.scratch,
            python: &self.python,
            node: &self.node,
        };
        let started = Instant::now();
        let (status, tests) =
            verify_fix_with_tests(&against.finding, &against.check, &fix, &index, &env);
        self.spent += started.elapsed();
        let checked = CheckedFix { fix, status, tests };
        self.fixes.insert(key, checked.clone());
        Ok(checked)
    }

    /// The broker's hook: what to tell the model about the artifact it just wrote, or nothing for a kind that is not judged.
    pub fn on_write(&mut self, kind: &str, bytes: &[u8]) -> Option<Value> {
        match kind {
            "finding" => Some(self.judge_finding(bytes)),
            "fix" => Some(self.judge_fix(bytes)),
            _ => None,
        }
    }

    fn judge_finding(&mut self, bytes: &[u8]) -> Value {
        if let Some(hit) = self.findings.get(bytes) {
            // the same finding again: the same answer, at no cost and without using up an attempt
            return feedback::finding_of(&hit.check);
        }
        let Some(finding) = parse_finding(bytes, &self.known) else {
            return feedback::rejected_finding();
        };
        let identity = (finding.title.clone(), finding.file.clone());
        let attempts = self.finding_attempts.get(&identity).copied().unwrap_or(0);
        if !self.within_bounds(attempts, self.checked_findings) {
            return feedback::not_checked_limit();
        }
        self.finding_attempts.insert(identity.clone(), attempts + 1);
        self.checked_findings += 1;
        let Some(checked) = self.finding(bytes) else {
            return feedback::rejected_finding();
        };
        let answer = feedback::finding_of(&checked.check);
        if !self.order.contains(&identity) {
            self.order.push(identity.clone());
        }
        match self.latest.get(&identity) {
            Some(old) if !replaces(&old.check.status, &checked.check.status) => {}
            _ => {
                self.latest.insert(identity, checked);
            }
        }
        answer
    }

    fn judge_fix(&mut self, bytes: &[u8]) -> Value {
        // the title is needed before anything else, to find the finding it belongs to
        let Some(parsed) = parse_fix(bytes, &self.known) else {
            return feedback::rejected_fix();
        };
        let Some(against) = self.finding_titled(&parsed.title) else {
            return feedback::no_such_finding();
        };
        let key = (bytes.to_vec(), against.bytes.clone());
        if let Some(hit) = self.fixes.get(&key) {
            return feedback::fix(&hit.status);
        }
        let attempts = self.fix_attempts.get(&parsed.title).copied().unwrap_or(0);
        if !self.within_bounds(attempts, self.checked_fixes) {
            return feedback::not_checked_limit();
        }
        self.fix_attempts.insert(parsed.title.clone(), attempts + 1);
        self.checked_fixes += 1;
        match self.fix(bytes, &against) {
            Ok(checked) => feedback::fix(&checked.status),
            Err(FixRefusal::NotAFix) => feedback::rejected_fix(),
            Err(FixRefusal::CannotMake) => feedback::cannot_make_fix(),
        }
    }

    /// The first finding reported under this title, as it stands now: the one a fix with this title is checked against, here and in
    /// the report.
    pub fn finding_titled(&self, title: &str) -> Option<CheckedFinding> {
        self.order
            .iter()
            .find(|(t, _)| t == title)
            .and_then(|id| self.latest.get(id))
            .cloned()
    }

    /// Whether the version already checked for these bytes confirmed the finding (used to pick among attempts at the end).
    pub fn is_confirmed(&self, bytes: &[u8]) -> bool {
        self.findings
            .get(bytes)
            .is_some_and(|c| c.check.status == CheckStatus::Reproduced)
    }
}

/// Why a fix could not be turned into a checked change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixRefusal {
    NotAFix,
    CannotMake,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_types::label::{Label, Level};
    use serde_json::json;

    fn file(path: &str) -> ImportedFile {
        ImportedFile {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: b"x = 1\n".to_vec(),
            label: Label::new(Level::Internal),
            notes: vec![],
        }
    }

    fn repair() -> Repair {
        Repair::new(
            ["a.py".to_string(), "b.py".to_string()].into(),
            vec![file("a.py"), file("b.py")],
            Transport::Local,
            std::env::temp_dir(),
            "/nonexistent/python".into(),
            "/nonexistent/node".into(),
        )
    }

    fn finding_bytes(title: &str, path: &str) -> Vec<u8> {
        json!({"title": title, "file": path, "line": 1, "explanation": "e", "reproducer": "print(1)"})
            .to_string()
            .into_bytes()
    }

    fn fix_bytes(title: &str, path: &str) -> Vec<u8> {
        json!({"title": title, "file": path, "old_text": "x = 1", "new_text": "x = 2"})
            .to_string()
            .into_bytes()
    }

    fn status(v: &Value) -> &str {
        v["status"].as_str().unwrap()
    }

    fn checked(title: &str, path: &str, status: CheckStatus) -> CheckedFinding {
        CheckedFinding {
            bytes: finding_bytes(title, path),
            finding: parse_finding(
                &finding_bytes(title, path),
                &["a.py".to_string(), "b.py".to_string()].into(),
            )
            .unwrap(),
            check: CheckResult {
                status,
                stdout: String::new(),
                sample_paths: vec![],
            },
        }
    }

    #[test]
    fn rp01_a_confirmed_attempt_is_not_replaced_by_one_that_is_not_confirmed_and_anything_else_is()
    {
        use CheckStatus::*;
        assert!(!replaces(&Reproduced, &NotReproduced));
        assert!(!replaces(&Reproduced, &Failed("x".into())));
        assert!(!replaces(&Reproduced, &Unavailable("x".into())));
        assert!(
            replaces(&Reproduced, &Reproduced),
            "a newer confirmed attempt wins"
        );
        assert!(replaces(&NotReproduced, &Reproduced));
        assert!(
            replaces(&NotReproduced, &Failed("x".into())),
            "the latest attempt counts"
        );
        assert!(replaces(&Failed("x".into()), &NotReproduced));
    }

    #[test]
    fn rp02_what_is_not_a_finding_or_a_fix_is_rejected_and_a_fix_needs_its_finding_first() {
        let mut r = repair();
        assert_eq!(
            status(&r.on_write("finding", b"not json").unwrap()),
            "rejected"
        );
        assert_eq!(
            status(
                &r.on_write("finding", &finding_bytes("T", "missing.py"))
                    .unwrap()
            ),
            "rejected",
            "a file the project does not show"
        );
        assert_eq!(status(&r.on_write("fix", b"{}").unwrap()), "rejected");
        assert_eq!(
            status(&r.on_write("fix", &fix_bytes("T", "missing.py")).unwrap()),
            "rejected"
        );
        // a readable fix before any finding with its title
        let v = r.on_write("fix", &fix_bytes("T", "a.py")).unwrap();
        assert_eq!(status(&v), "not_checked");
        assert!(v["note"]
            .as_str()
            .unwrap()
            .contains("Report the finding first"));
        // kinds that are not judged get nothing at all
        assert!(r.on_write("memory_suggestion", b"x").is_none());
        assert!(r.on_write("note", b"x").is_none());
    }

    #[test]
    fn rp03_the_limits_stop_a_check_before_it_runs_and_say_the_last_version_is_still_checked_at_the_end(
    ) {
        let limited = |v: Value| {
            assert_eq!(status(&v), "not_checked", "{v}");
            assert!(v["note"]
                .as_str()
                .unwrap()
                .contains("checked once more when you finish"));
        };
        // twice for one finding
        let mut r = repair();
        r.finding_attempts.insert(
            ("T".into(), "a.py".into()),
            feedback::MAX_ATTEMPTS_PER_FINDING,
        );
        limited(r.on_write("finding", &finding_bytes("T", "a.py")).unwrap());
        // ...but another finding is not affected: it gets past the limit check and runs (and fails to start the missing interpreter)
        let other = r.on_write("finding", &finding_bytes("T", "b.py")).unwrap();
        assert_ne!(status(&other), "not_checked", "{other}");
        // the number of findings checked in the session
        let mut r = repair();
        r.checked_findings = feedback::MAX_CHECKED;
        limited(r.on_write("finding", &finding_bytes("U", "a.py")).unwrap());
        // the time spent
        let mut r = repair();
        r.spent = feedback::MAX_TIME_SPENT;
        limited(r.on_write("finding", &finding_bytes("U", "a.py")).unwrap());
        // one under each limit still runs
        let mut r = repair();
        r.finding_attempts.insert(
            ("T".into(), "a.py".into()),
            feedback::MAX_ATTEMPTS_PER_FINDING - 1,
        );
        r.checked_findings = feedback::MAX_CHECKED - 1;
        r.spent = feedback::MAX_TIME_SPENT - Duration::from_secs(1);
        assert_ne!(
            status(&r.on_write("finding", &finding_bytes("T", "a.py")).unwrap()),
            "not_checked"
        );
    }

    #[test]
    fn rp04_the_fix_limits_work_the_same_way_and_a_fix_is_checked_against_the_first_finding_with_its_title(
    ) {
        let limited = |v: Value| assert_eq!(status(&v), "not_checked", "{v}");
        let mut r = repair();
        // two findings share a title in different files; the first one reported is the one a fix is checked against
        let first = checked("T", "a.py", CheckStatus::Reproduced);
        let second = checked("T", "b.py", CheckStatus::NotReproduced);
        r.order = vec![("T".into(), "a.py".into()), ("T".into(), "b.py".into())];
        r.latest.insert(("T".into(), "a.py".into()), first.clone());
        r.latest.insert(("T".into(), "b.py".into()), second);
        assert_eq!(r.finding_titled("T").unwrap().finding.file, "a.py");
        assert!(r.finding_titled("nobody").is_none());
        r.fix_attempts
            .insert("T".into(), feedback::MAX_ATTEMPTS_PER_FINDING);
        limited(r.on_write("fix", &fix_bytes("T", "a.py")).unwrap());
        let mut r2 = repair();
        r2.order = vec![("T".into(), "a.py".into())];
        r2.latest.insert(("T".into(), "a.py".into()), first);
        r2.checked_fixes = feedback::MAX_CHECKED;
        limited(r2.on_write("fix", &fix_bytes("T", "a.py")).unwrap());
        let mut r3 = repair();
        r3.order = vec![("T".into(), "a.py".into())];
        r3.latest.insert(
            ("T".into(), "a.py".into()),
            checked("T", "a.py", CheckStatus::Reproduced),
        );
        r3.spent = feedback::MAX_TIME_SPENT;
        limited(r3.on_write("fix", &fix_bytes("T", "a.py")).unwrap());
    }
}
