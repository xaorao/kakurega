use kk_evidence::b64;
use kk_review::check::CheckStatus;
use kk_review::job::JobReport;
use kk_review::release::{ReleaseDraft, Removal};
use kk_types::label::Level;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn level_name(l: Level) -> &'static str {
    match l {
        Level::Public => "public",
        Level::Internal => "internal",
        Level::Confidential => "confidential",
        Level::Restricted => "restricted",
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapFile {
    pub path: String,
    pub bytes: usize,
    pub level: String,
    pub compartments: Vec<String>,
    pub withheld: bool,
    pub notes: Vec<String>,
    pub text: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapFinding {
    pub title: String,
    pub file: String,
    pub line: i64,
    pub explanation: String,
    pub reproducer: String,
    pub status: String,
    pub reproduced: bool,
    pub output: String,
    pub sample: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapFix {
    pub title: String,
    pub file: String,
    pub old_text: String,
    pub new_text: String,
    pub explanation: String,
    pub status: String,
    #[serde(default)]
    pub tests: String,
    pub verified: bool,
    pub diff: String,
    pub file_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapAudit {
    pub request: u64,
    pub op: String,
    pub outcome: String,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnapDraft {
    pub recipient: String,
    pub policy_version: String,
    pub created: i64,
    pub expires: i64,
    pub digest: String,
    pub files: BTreeMap<String, String>,
    pub removed: Vec<(String, String)>,
    pub not_confirmed: Vec<(String, String)>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub summary: Value,
    pub files: Vec<SnapFile>,
    pub findings: Vec<SnapFinding>,
    #[serde(default)]
    pub fixes: Vec<SnapFix>,
    pub unparsed: usize,
    pub audit: Vec<SnapAudit>,
    pub draft: Option<SnapDraft>,
    pub rejection: Option<String>,
    pub commitment: String,
    pub manifest_files: i64,
    pub route_class: String,
    pub error: Option<String>,
}

fn status_text(s: &CheckStatus) -> String {
    match s {
        CheckStatus::Reproduced => "reproduced".into(),
        CheckStatus::NotReproduced => "not_reproduced".into(),
        CheckStatus::Nondeterministic => "nondeterministic".into(),
        CheckStatus::Failed(m) => format!("failed: {m}"),
        CheckStatus::Unavailable(m) => format!("unavailable: {m}"),
    }
}

impl Snapshot {
    pub fn from_report(r: &JobReport) -> Snapshot {
        let summary = json!({
            "imported": r.imported,
            "skipped": r.skipped.iter().map(|(p, why)| json!({"path": p, "reason": why})).collect::<Vec<_>>(),
            "withheld": r.withheld.iter().map(|(p, n)| json!({"path": p, "notes": n})).collect::<Vec<_>>(),
            "injection_suspects": r.injection_suspects,
            "route": r.route_evidence.iter().map(|e| serde_json::to_value(e).unwrap_or(Value::Null)).collect::<Vec<_>>(),
            "worker_exit": r.worker_exit.as_ref().map(|e| format!("{e:?}")),
            "final_taint": r.final_taint.as_ref().map(|t| json!({"level": level_name(t.level), "integrity": format!("{:?}", t.integrity).to_lowercase(), "compartments": t.compartments})),
            "findings": r.findings.len(),
            "confirmed": r.findings.iter().filter(|(_, c)| c.status == CheckStatus::Reproduced).count(),
            "manifest_digest": r.manifest.digest,
            "files_in_manifest": r.manifest.file_count,
        });
        let draft = r.draft.as_ref().ok().map(|d| SnapDraft {
            recipient: d.recipient.clone(),
            policy_version: d.policy_version.clone(),
            created: d.created,
            expires: d.expires,
            digest: d.digest.clone(),
            files: d
                .files
                .iter()
                .map(|(n, b)| (n.clone(), b64::encode(b)))
                .collect(),
            removed: d
                .removed
                .iter()
                .map(|x| (x.what.clone(), x.why.clone()))
                .collect(),
            not_confirmed: d.not_confirmed.clone(),
        });
        Snapshot {
            summary,
            files: r
                .files
                .iter()
                .map(|f| SnapFile {
                    path: f.path.clone(),
                    bytes: f.bytes.len(),
                    level: level_name(f.label.level).into(),
                    compartments: f.label.compartments.iter().cloned().collect(),
                    withheld: f.label.level >= Level::Restricted,
                    notes: f.notes.clone(),
                    text: (f.label.level < Level::Restricted)
                        .then(|| String::from_utf8_lossy(&f.bytes).into_owned()),
                })
                .collect(),
            findings: r
                .findings
                .iter()
                .map(|(f, c)| SnapFinding {
                    title: f.title.clone(),
                    file: f.file.clone(),
                    line: f.line,
                    explanation: f.explanation.clone(),
                    reproducer: f.reproducer.clone(),
                    status: status_text(&c.status),
                    reproduced: c.status == CheckStatus::Reproduced,
                    output: c.stdout.clone(),
                    sample: c.sample_paths.clone(),
                })
                .collect(),
            fixes: r
                .fixes
                .iter()
                .map(|f| SnapFix {
                    title: f.finding_title.clone(),
                    file: f.fix.file.clone(),
                    old_text: f.fix.old_text.clone(),
                    new_text: f.fix.new_text.clone(),
                    explanation: f.fix.explanation.clone(),
                    status: match &f.status {
                        kk_review::check::FixStatus::FixesTheReproducer => {
                            "fixes_the_reproducer".into()
                        }
                        kk_review::check::FixStatus::StillReproduced => "still_reproduced".into(),
                        kk_review::check::FixStatus::CouldNotApply(m) => {
                            format!("could_not_apply: {m}")
                        }
                        kk_review::check::FixStatus::NotChecked(m) if m.contains("Failed(") => {
                            "not_checked: the reproducer no longer runs correctly after this change"
                                .to_string()
                        }
                        kk_review::check::FixStatus::NotChecked(m) => format!("not_checked: {m}"),
                        kk_review::check::FixStatus::BreaksTests(m) => {
                            format!("breaks_tests: {m}")
                        }
                    },
                    tests: f.tests.clone(),
                    verified: f.status == kk_review::check::FixStatus::FixesTheReproducer,
                    diff: f.diff.clone(),
                    file_sha256: f.file_sha256.clone(),
                })
                .collect(),
            unparsed: r.unparsed_findings,
            audit: r
                .audit
                .iter()
                .map(|e| SnapAudit {
                    request: e.request_id,
                    op: e.op.clone(),
                    outcome: e.outcome.clone(),
                    detail: e.detail.clone(),
                })
                .collect(),
            draft,
            rejection: r.draft.as_ref().err().map(|e| format!("{e:?}")),
            commitment: r.commitment.clone(),
            manifest_files: r.manifest.file_count as i64,
            route_class: r
                .route_evidence
                .first()
                .map(|e| e.class.clone())
                .unwrap_or_else(|| "device_only".into()),
            error: r.error.clone(),
        }
    }

    pub fn release_draft(&self) -> Option<ReleaseDraft> {
        let d = self.draft.as_ref()?;
        let mut files = BTreeMap::new();
        for (n, t) in &d.files {
            files.insert(n.clone(), b64::decode(t).ok()?);
        }
        Some(ReleaseDraft {
            recipient: d.recipient.clone(),
            policy_version: d.policy_version.clone(),
            created: d.created,
            expires: d.expires,
            files,
            removed: d
                .removed
                .iter()
                .map(|(a, b)| Removal {
                    what: a.clone(),
                    why: b.clone(),
                })
                .collect(),
            not_confirmed: d.not_confirmed.clone(),
            digest: d.digest.clone(),
        })
    }
}
