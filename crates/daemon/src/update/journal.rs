//! Durable update jobs, independent of SQLite and the daemon instance lock.
use super::releases::atomic_write;
use amarcode_protocol::rpc::{DaemonUpdateJob, DaemonUpdateStatus};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Phase {
    Queued,
    Staging,
    Installing,
    Restarting,
    RollingBack,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub job: DaemonUpdateJob,
    pub phase: Phase,
    pub address: String,
    #[serde(default)]
    pub recovery_failures: u32,
    #[serde(default)]
    pub last_error: Option<String>,
}
impl Record {
    pub fn directory(&self, root: &Path) -> PathBuf {
        root.join("updates").join(&self.job.id)
    }
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let directory = self.directory(root);
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        atomic_write(
            &directory.join("job.json"),
            &serde_json::to_vec(self).map_err(|e| e.to_string())?,
        )?;
        atomic_write(
            &directory.join("status.json"),
            &serde_json::to_vec(&self.job).map_err(|e| e.to_string())?,
        )
    }
    pub fn progress(
        &mut self,
        root: &Path,
        phase: Phase,
        status: DaemonUpdateStatus,
    ) -> Result<(), String> {
        self.phase = phase;
        self.job.status = status;
        self.save(root)
    }
    pub fn finish(&mut self, root: &Path, status: DaemonUpdateStatus) -> Result<(), String> {
        self.job.finished = true;
        self.progress(root, Phase::Finished, status)
    }
}
pub fn load(root: &Path, id: Option<&str>) -> Result<Option<Record>, String> {
    let id = match id {
        Some(id) => id.to_owned(),
        None => match fs::read_to_string(root.join("updates/latest")) {
            Ok(id) => id,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        },
    };
    uuid::Uuid::parse_str(&id).map_err(|_| "invalid update job id".to_string())?;
    let path = root.join("updates").join(&id).join("job.json");
    let record: Record = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if record.job.id != id {
        return Err("update job id mismatch".into());
    }
    Ok(Some(record))
}
pub fn publish(root: &Path, record: &Record) -> Result<(), String> {
    record.save(root)?;
    atomic_write(&root.join("updates/latest"), record.job.id.as_bytes())
}
pub use super::releases::lock_update as lock;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_lock_is_exclusive_and_released_after_process_owner_drops() {
        let directory = tempfile::tempdir().unwrap();
        let first = lock(directory.path(), "request.lock").unwrap();
        assert!(lock(directory.path(), "request.lock").is_err());
        drop(first);
        assert!(lock(directory.path(), "request.lock").is_ok());
    }
    #[test]
    fn client_job_ids_cannot_escape_the_release_store() {
        let directory = tempfile::tempdir().unwrap();
        for id in ["../outside", "/etc/passwd", "", ".."] {
            assert!(load(directory.path(), Some(id)).is_err());
            assert!(crate::update::releases::read_update_job(directory.path(), Some(id)).is_err());
        }
    }
}
