use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
}

impl JobStatus {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobSnapshot {
    pub id: u64,
    pub label: String,
    pub detail: String,
    pub status: JobStatus,
    pub started_at: Instant,
    pub updated_at: Instant,
}

impl JobSnapshot {
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.updated_at.saturating_duration_since(self.started_at)
    }
}

#[derive(Clone, Debug)]
struct JobRecord {
    id: u64,
    label: String,
    detail: String,
    status: JobStatus,
    started_at: Instant,
    updated_at: Instant,
}

impl JobRecord {
    fn snapshot(&self) -> JobSnapshot {
        JobSnapshot {
            id: self.id,
            label: self.label.clone(),
            detail: self.detail.clone(),
            status: self.status.clone(),
            started_at: self.started_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug)]
struct JobsState {
    next_id: AtomicU64,
    jobs: Mutex<Vec<JobRecord>>,
    jobs_window_opened_once: AtomicBool,
}

impl Default for JobsState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            jobs: Mutex::new(Vec::new()),
            jobs_window_opened_once: AtomicBool::new(false),
        }
    }
}

static JOBS_STATE: OnceLock<JobsState> = OnceLock::new();

fn jobs_state() -> &'static JobsState {
    JOBS_STATE.get_or_init(JobsState::default)
}

#[must_use]
pub fn mark_jobs_window_auto_opened() -> bool {
    !jobs_state()
        .jobs_window_opened_once
        .swap(true, Ordering::AcqRel)
}

#[derive(Debug)]
pub struct JobHandle {
    id: u64,
    completed: bool,
}

impl JobHandle {
    pub fn update(&self, detail: impl Into<String>) {
        update_job(self.id, detail, JobStatus::Running);
    }

    pub fn complete(mut self, detail: impl Into<String>) {
        update_job(self.id, detail, JobStatus::Completed);
        self.completed = true;
    }

    pub fn fail(mut self, detail: impl Into<String>) {
        update_job(self.id, detail, JobStatus::Failed);
        self.completed = true;
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        if !self.completed {
            update_job(
                self.id,
                "worker ended before reporting completion",
                JobStatus::Failed,
            );
        }
    }
}

pub fn start_job(label: impl Into<String>, detail: impl Into<String>) -> JobHandle {
    let state = jobs_state();
    let id = state.next_id.fetch_add(1, Ordering::AcqRel);
    let now = Instant::now();
    let mut jobs = state
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    jobs.push(JobRecord {
        id,
        label: label.into(),
        detail: detail.into(),
        status: JobStatus::Running,
        started_at: now,
        updated_at: now,
    });
    JobHandle {
        id,
        completed: false,
    }
}

pub fn record_failed_job(label: impl Into<String>, detail: impl Into<String>) {
    start_job(label, "starting").fail(detail);
}

fn update_job(id: u64, detail: impl Into<String>, status: JobStatus) {
    let mut jobs = jobs_state()
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
        job.detail = detail.into();
        job.status = status;
        job.updated_at = Instant::now();
    }
}

#[must_use]
pub fn job_snapshots() -> Arc<[JobSnapshot]> {
    let jobs = jobs_state()
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    jobs.iter()
        .rev()
        .take(64)
        .map(JobRecord::snapshot)
        .collect::<Vec<_>>()
        .into()
}

#[must_use]
pub fn has_job_snapshots() -> bool {
    let jobs = jobs_state()
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    !jobs.is_empty()
}

#[must_use]
pub fn running_job_count() -> usize {
    let jobs = jobs_state()
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    jobs.iter()
        .filter(|job| job.status == JobStatus::Running)
        .count()
}
