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

    pub fn of(job: &JobKey, index: u64) -> ArtifactHandle {
        ArtifactHandle {
            job: job.clone(),
            index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    UnknownHandle,
    UnknownStep,
    LabelExceedsClearance,
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::UnknownHandle => f.write_str("UNKNOWN_HANDLE"),
            KernelError::UnknownStep => f.write_str("UNKNOWN_STEP"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepId(u64);

struct StepState {
    job: JobKey,
    clearance: Label,
    taint: Label,
    read_log: Vec<u64>,
}

#[derive(Default)]
pub struct Kernel {
    jobs: HashMap<JobKey, JobStore>,
    steps: HashMap<StepId, StepState>,
    next_step: u64,
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

    pub fn open_step(&mut self, job: &JobKey, clearance: Label) -> StepId {
        self.jobs.entry(job.clone()).or_default();
        let id = StepId(self.next_step);
        self.next_step += 1;
        self.steps.insert(
            id,
            StepState {
                job: job.clone(),
                clearance,
                taint: Label::bottom(),
                read_log: Vec::new(),
            },
        );
        id
    }

    pub fn close_step(&mut self, step: StepId) {
        self.steps.remove(&step);
    }

    pub fn taint(&self, step: StepId) -> Option<&Label> {
        self.steps.get(&step).map(|s| &s.taint)
    }

    pub fn clearance(&self, step: StepId) -> Option<&Label> {
        self.steps.get(&step).map(|s| &s.clearance)
    }

    pub fn read_log(&self, step: StepId) -> Option<&[u64]> {
        self.steps.get(&step).map(|s| s.read_log.as_slice())
    }

    pub fn read_artifact(
        &mut self,
        step: StepId,
        handle: &ArtifactHandle,
    ) -> Result<Vec<u8>, KernelError> {
        let state = self.steps.get_mut(&step).ok_or(KernelError::UnknownStep)?;
        if handle.job != state.job {
            return Err(KernelError::UnknownHandle);
        }
        let artifact = self
            .jobs
            .get(&state.job)
            .and_then(|s| s.artifacts.get(&handle.index))
            .ok_or(KernelError::UnknownHandle)?;
        if !artifact.label.confidentiality_within(&state.clearance) {
            return Err(KernelError::LabelExceedsClearance);
        }
        state.taint = state.taint.join(&artifact.label);
        state.read_log.push(handle.index);
        Ok(artifact.bytes.clone())
    }

    pub fn absorb(&mut self, step: StepId, label: &Label) -> Result<(), KernelError> {
        let state = self.steps.get_mut(&step).ok_or(KernelError::UnknownStep)?;
        if !label.confidentiality_within(&state.clearance) {
            return Err(KernelError::LabelExceedsClearance);
        }
        state.taint = state.taint.join(label);
        Ok(())
    }

    pub fn write_artifact(
        &mut self,
        step: StepId,
        bytes: Vec<u8>,
        derived_from_hint: &[ArtifactHandle],
    ) -> Result<ArtifactHandle, KernelError> {
        let _advisory = derived_from_hint;
        let state = self.steps.get(&step).ok_or(KernelError::UnknownStep)?;
        let label = state.taint.clone();
        let job = state.job.clone();
        let store = self.jobs.entry(job.clone()).or_default();
        let index = store.next;
        store.next += 1;
        store.artifacts.insert(index, Artifact { bytes, label });
        Ok(ArtifactHandle { job, index })
    }
}
