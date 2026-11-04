use crate::service::{JobRunner, Service};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type RunnerFactory = Box<dyn Fn() -> Arc<dyn JobRunner> + Send + Sync>;

/// One fully separate service per tenant: its own data directory (vault keys, memory, jobs, approvals, approvers, event
/// log, signing keys) and its own file area. Nothing is shared between tenants, so isolation is structural and does not
/// depend on every query remembering a tenant filter.
pub struct Tenants {
    base: PathBuf,
    runner: RunnerFactory,
    open: Mutex<HashMap<String, Arc<Service>>>,
    test_mode: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TenantError {
    BadId,
    Busy,
    Storage(String),
}

/// Tenant ids come from the authentication layer, never from a path a user typed. They are lowercase so two ids can
/// never name the same directory on a case-insensitive disk.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (3..=40).contains(&b.len())
        && b[0].is_ascii_lowercase() | b[0].is_ascii_digit()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

impl Tenants {
    pub fn new(
        base: &Path,
        runner: RunnerFactory,
        test_mode: bool,
    ) -> Result<Tenants, TenantError> {
        std::fs::create_dir_all(base.join("tenants"))
            .map_err(|e| TenantError::Storage(e.to_string()))?;
        Ok(Tenants {
            base: base.to_path_buf(),
            runner,
            open: Mutex::new(HashMap::new()),
            test_mode,
        })
    }

    fn dir(&self, id: &str) -> PathBuf {
        self.base.join("tenants").join(id)
    }

    pub fn files_dir(&self, id: &str) -> Option<PathBuf> {
        valid_id(id).then(|| self.dir(id).join("files"))
    }

    /// The tenant's service, created on first use.
    pub fn get_or_create(&self, id: &str) -> Result<Arc<Service>, TenantError> {
        use std::os::unix::fs::PermissionsExt;
        if !valid_id(id) {
            return Err(TenantError::BadId);
        }
        let mut open = self.open.lock().expect("lock");
        if let Some(s) = open.get(id) {
            return Ok(s.clone());
        }
        let dir = self.dir(id);
        let files = dir.join("files");
        std::fs::create_dir_all(&files).map_err(|e| TenantError::Storage(e.to_string()))?;
        for d in [&dir, &files] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| TenantError::Storage(e.to_string()))?;
        }
        let root = files
            .canonicalize()
            .map_err(|e| TenantError::Storage(e.to_string()))?;
        let svc = Service::new(
            &dir.join("data"),
            vec![root],
            (self.runner)(),
            self.test_mode,
        )
        .map_err(TenantError::Storage)?;
        open.insert(id.to_string(), svc.clone());
        Ok(svc)
    }

    pub fn exists(&self, id: &str) -> bool {
        valid_id(id) && self.dir(id).is_dir()
    }

    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.base.join("tenants"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_id(n))
            .collect();
        v.sort();
        v
    }

    /// Removes the tenant and all of its data. Refused while anything still holds the tenant's service, so a request in
    /// flight cannot be left writing into a deleted directory.
    pub fn delete(&self, id: &str) -> Result<bool, TenantError> {
        if !valid_id(id) {
            return Err(TenantError::BadId);
        }
        let mut open = self.open.lock().expect("lock");
        if let Some(s) = open.get(id) {
            if Arc::strong_count(s) > 1 {
                return Err(TenantError::Busy);
            }
        }
        open.remove(id);
        let dir = self.dir(id);
        if !dir.is_dir() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&dir).map_err(|e| TenantError::Storage(e.to_string()))?;
        Ok(true)
    }
}
