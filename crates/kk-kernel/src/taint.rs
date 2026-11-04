use kk_types::label::Label;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobKey {
    pub tenant: String,
    pub job: String,
}

impl JobKey {
    pub fn new(tenant: &str, job: &str) -> Self {
        JobKey {
            tenant: tenant.to_string(),
            job: job.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactHandle {
    job: JobKey,
    index: u64,
}

impl ArtifactHandle {
    pub fn index(&self) -> u64 {
        self.index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    UnknownHandle,
    LabelExceedsClearance,
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::UnknownHandle => f.write_str("UNKNOWN_HANDLE"),
            KernelError::LabelExceedsClearance => f.write_str("LABEL_EXCEEDS_CLEARANCE"),
        }
    }
}

impl std::error::Error for KernelError {}

struct Artifact {
    bytes: Vec<u8>,
    label: Label,
}

#[derive(Default)]
struct JobStore {
    artifacts: HashMap<u64, Artifact>,
    next: u64,
}

#[derive(Default)]
pub struct Kernel {
    jobs: HashMap<JobKey, JobStore>,
}

impl Kernel {
    pub fn new() -> Self {
        Kernel::default()
    }

    pub fn import(&mut self, job: &JobKey, bytes: Vec<u8>, label: Label) -> ArtifactHandle {
        let store = self.jobs.entry(job.clone()).or_default();
        let index = store.next;
        store.next += 1;
        store.artifacts.insert(index, Artifact { bytes, label });
        ArtifactHandle {
            job: job.clone(),
            index,
        }
    }

    pub fn label_of(&self, handle: &ArtifactHandle) -> Option<Label> {
        self.jobs
            .get(&handle.job)
            .and_then(|s| s.artifacts.get(&handle.index))
            .map(|a| a.label.clone())
    }

    pub fn open_step(&mut self, job: &JobKey, clearance: Label) -> StepSession<'_> {
        let store = self.jobs.entry(job.clone()).or_default();
        StepSession {
            job: job.clone(),
            store,
            clearance,
            taint: Label::bottom(),
            read_log: Vec::new(),
        }
    }
}

pub struct StepSession<'k> {
    job: JobKey,
    store: &'k mut JobStore,
    clearance: Label,
    taint: Label,
    read_log: Vec<u64>,
}

impl StepSession<'_> {
    pub fn taint(&self) -> &Label {
        &self.taint
    }

    pub fn read_log(&self) -> &[u64] {
        &self.read_log
    }

    pub fn read_artifact(&mut self, handle: &ArtifactHandle) -> Result<Vec<u8>, KernelError> {
        if handle.job != self.job {
            return Err(KernelError::UnknownHandle);
        }
        let artifact = self
            .store
            .artifacts
            .get(&handle.index)
            .ok_or(KernelError::UnknownHandle)?;
        if !artifact.label.confidentiality_within(&self.clearance) {
            return Err(KernelError::LabelExceedsClearance);
        }
        self.taint = self.taint.join(&artifact.label);
        self.read_log.push(handle.index);
        Ok(artifact.bytes.clone())
    }

    pub fn absorb(&mut self, label: &Label) -> Result<(), KernelError> {
        if !label.confidentiality_within(&self.clearance) {
            return Err(KernelError::LabelExceedsClearance);
        }
        self.taint = self.taint.join(label);
        Ok(())
    }

    pub fn write_artifact(
        &mut self,
        bytes: Vec<u8>,
        derived_from_hint: &[ArtifactHandle],
    ) -> ArtifactHandle {
        let _advisory = derived_from_hint;
        let label = self.taint.clone();
        let index = self.store.next;
        self.store.next += 1;
        self.store
            .artifacts
            .insert(index, Artifact { bytes, label });
        ArtifactHandle {
            job: self.job.clone(),
            index,
        }
    }
}
