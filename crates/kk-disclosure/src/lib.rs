use kk_vault::rusqlite::{params, Connection};
use kk_vault::Vault;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug)]
pub struct DisclosureError(pub String);

impl fmt::Display for DisclosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "disclosure ledger error: {}", self.0)
    }
}

impl std::error::Error for DisclosureError {}

impl From<kk_vault::rusqlite::Error> for DisclosureError {
    fn from(e: kk_vault::rusqlite::Error) -> Self {
        DisclosureError(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    pub escalate_coverage: f64,
    pub block_coverage: f64,
    pub max_releases_per_day: usize,
    pub min_project_files: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            escalate_coverage: 0.30,
            block_coverage: 0.60,
            max_releases_per_day: 5,
            min_project_files: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub recipient: String,
    pub source_paths: Vec<String>,
    pub project_files: usize,
    pub project: String,
    /// Size in bytes of each source file that would be shown (a file missing here counts as 0).
    pub source_sizes: BTreeMap<String, u64>,
    /// Total bytes of the project's releasable files; 0 means unknown, and then only file counts are used.
    pub project_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Allow,
    Escalate,
    Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub verdict: Verdict,
    pub reasons: Vec<String>,
    pub checked: Vec<String>,
    pub not_checked: Vec<String>,
    pub coverage_after: f64,
    pub byte_coverage_after: f64,
    pub files_after: usize,
    pub releases_before: usize,
}

pub struct Ledger {
    vault: Vault,
    policy: Policy,
}

pub fn canonical_recipient(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS disclosures (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    recipient_key TEXT NOT NULL,
    recipient_shown TEXT NOT NULL,
    project TEXT NOT NULL,
    release_digest TEXT NOT NULL,
    at INTEGER NOT NULL,
    source_path TEXT NOT NULL,
    bytes INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS disclosures_r ON disclosures(recipient_key, project);
CREATE TABLE IF NOT EXISTS releases (
    release_digest TEXT NOT NULL,
    recipient_key TEXT NOT NULL,
    project TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (release_digest, recipient_key)
);
";

impl Ledger {
    pub fn open(vault: Vault, policy: Policy) -> Result<Ledger, DisclosureError> {
        vault.connection().execute_batch(SCHEMA)?;
        // Older ledgers have no size column; adding it twice is an error we ignore.
        let _ = vault.connection().execute(
            "ALTER TABLE disclosures ADD COLUMN bytes INTEGER NOT NULL DEFAULT 0",
            [],
        );
        Ok(Ledger { vault, policy })
    }

    fn conn(&self) -> &Connection {
        self.vault.connection()
    }

    fn disclosed(
        &self,
        key: &str,
        project: &str,
    ) -> Result<BTreeMap<String, u64>, DisclosureError> {
        let mut st = self.conn().prepare("SELECT source_path, MAX(bytes) FROM disclosures WHERE recipient_key = ?1 AND project = ?2 GROUP BY source_path")?;
        let rows = st.query_map(params![key, project], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn assess(&self, now: i64, plan: &Plan) -> Result<Assessment, DisclosureError> {
        let key = canonical_recipient(&plan.recipient);
        let mut files = self.disclosed(&key, &plan.project)?;
        for p in &plan.source_paths {
            let size = plan.source_sizes.get(p).copied().unwrap_or(0);
            let e = files.entry(p.clone()).or_insert(0);
            *e = (*e).max(size);
        }
        let releases_before: i64 = self.conn().query_row(
            "SELECT count(*) FROM releases WHERE recipient_key = ?1 AND project = ?2",
            params![key, plan.project],
            |r| r.get(0),
        )?;
        let recent: i64 = self.conn().query_row(
            "SELECT count(*) FROM releases WHERE recipient_key = ?1 AND at > ?2",
            params![key, now - 86_400],
            |r| r.get(0),
        )?;
        let total = plan.project_files.max(1);
        let file_coverage = files.len() as f64 / total as f64;
        let disclosed_bytes: u64 = files.values().sum();
        let byte_coverage = if plan.project_bytes > 0 {
            (disclosed_bytes as f64 / plan.project_bytes as f64).min(1.0)
        } else {
            0.0
        };
        // A few large files can hold most of a project: the larger of the two measures decides.
        let coverage = file_coverage.max(byte_coverage);
        let by_size = byte_coverage > file_coverage;
        let measure = if by_size { "by size" } else { "by file count" };
        let mut verdict = Verdict::Allow;
        let mut reasons = Vec::new();
        if plan.project_files >= self.policy.min_project_files {
            if coverage > self.policy.block_coverage {
                verdict = Verdict::Block;
                reasons.push(format!("Together with earlier releases, {} of this project's {} files ({:.0}% {measure}) would have been shown to this recipient, above the {:.0}% limit.", files.len(), plan.project_files, coverage * 100.0, self.policy.block_coverage * 100.0));
            } else if coverage > self.policy.escalate_coverage {
                verdict = Verdict::Escalate;
                reasons.push(format!("Together with earlier releases, {} of this project's {} files ({:.0}% {measure}) would have been shown to this recipient, above the {:.0}% review threshold.", files.len(), plan.project_files, coverage * 100.0, self.policy.escalate_coverage * 100.0));
            }
        }
        if recent as usize >= self.policy.max_releases_per_day {
            verdict = Verdict::Block;
            reasons.push(format!(
                "{recent} releases went to this recipient in the last 24 hours; the limit is {}.",
                self.policy.max_releases_per_day
            ));
        }
        Ok(Assessment {
            verdict,
            reasons,
            checked: vec![
                "Distinct source files already shown to this recipient for this project, with every spelling of the recipient name that reduces to the same letters and digits.".into(),
                "Number of releases to this recipient in the last 24 hours.".into(),
                "Share of the project's bytes already shown, so a few large files cannot hide behind a low file count.".into(),
            ],
            not_checked: vec![
                "Whether combined excerpts reveal something that no single file shows.".into(),
                "Other names or organisations that the same people may use.".into(),
                "Information the recipient obtained from sources other than Kakurega.".into(),
            ],
            coverage_after: coverage,
            byte_coverage_after: byte_coverage,
            files_after: files.len(),
            releases_before: releases_before as usize,
        })
    }

    pub fn record_delivered(
        &self,
        now: i64,
        plan: &Plan,
        release_digest: &str,
    ) -> Result<(), DisclosureError> {
        let key = canonical_recipient(&plan.recipient);
        let inserted = self.conn().execute("INSERT OR IGNORE INTO releases(release_digest, recipient_key, project, at) VALUES (?1, ?2, ?3, ?4)", params![release_digest, key, plan.project, now])?;
        if inserted == 0 {
            return Ok(());
        }
        for p in &plan.source_paths {
            self.conn().execute("INSERT INTO disclosures(recipient_key, recipient_shown, project, release_digest, at, source_path, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![key, plan.recipient, plan.project, release_digest, now, p, plan.source_sizes.get(p).copied().unwrap_or(0) as i64])?;
        }
        Ok(())
    }

    pub fn history(&self, recipient: &str) -> Result<Vec<(String, i64, usize)>, DisclosureError> {
        let key = canonical_recipient(recipient);
        let mut st = self.conn().prepare("SELECT r.project, r.at, (SELECT count(*) FROM disclosures d WHERE d.release_digest = r.release_digest AND d.recipient_key = r.recipient_key) FROM releases r WHERE r.recipient_key = ?1 ORDER BY r.at")?;
        let rows = st.query_map(params![key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)? as usize,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
