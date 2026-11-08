use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use agent_client_protocol::schema::v1::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub session_id: String,
    pub cwd: PathBuf,
    pub history: Vec<Value>,
    #[serde(default)]
    pub model: String,
    pub updated_at: u64,
    #[serde(default)]
    pub cost_usd: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    sessions: Vec<PersistedSession>,
}

#[derive(Debug)]
pub struct SessionStore {
    path: PathBuf,
    ttl_seconds: u64,
    io: Mutex<()>,
}

impl SessionStore {
    pub fn new(path: PathBuf, ttl_seconds: u64) -> Self {
        Self {
            path,
            ttl_seconds,
            io: Mutex::new(()),
        }
    }

    pub fn load(&self) -> Result<HashMap<SessionId, PersistedSession>, String> {
        let _guard = self.io.lock().map_err(|_| "session store lock poisoned")?;
        let _file_guard = self.lock_file()?;
        let mut file = self.read_unlocked()?;
        let now = now_seconds();
        let original_len = file.sessions.len();
        file.sessions
            .retain(|session| now.saturating_sub(session.updated_at) <= self.ttl_seconds);
        if file.sessions.len() != original_len {
            self.write_unlocked(&file)?;
        }
        Ok(file
            .sessions
            .into_iter()
            .map(|session| (SessionId::new(session.session_id.clone()), session))
            .collect())
    }

    pub fn upsert(&self, mut session: PersistedSession) -> Result<(), String> {
        let _guard = self.io.lock().map_err(|_| "session store lock poisoned")?;
        let _file_guard = self.lock_file()?;
        let mut file = self.read_unlocked()?;
        session.updated_at = now_seconds();
        file.sessions
            .retain(|saved| saved.session_id != session.session_id);
        file.sessions.push(session);
        self.write_unlocked(&file)
    }

    pub fn delete(&self, session_id: &SessionId) -> Result<bool, String> {
        let _guard = self.io.lock().map_err(|_| "session store lock poisoned")?;
        let _file_guard = self.lock_file()?;
        let mut file = self.read_unlocked()?;
        let original_len = file.sessions.len();
        file.sessions
            .retain(|session| session.session_id != session_id.to_string());
        let deleted = file.sessions.len() != original_len;
        if deleted {
            self.write_unlocked(&file)?;
        }
        Ok(deleted)
    }

    // Lock a stable sidecar, not the JSON file replaced by rename. Keep it on
    // disk so every process locks the same file; closing the handle releases
    // ownership, including after a crash. The lock covers the entire
    // read/modify/replace operation (including expiry pruning in load).
    fn lock_file(&self) -> Result<File, String> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create session store directory: {error}"))?;
        }
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        let path = self.path.with_file_name(name);
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|error| {
            format!(
                "failed to open session store lock {}: {error}",
                path.display()
            )
        })?;
        file.lock().map_err(|error| {
            format!(
                "failed to lock session store {}: {error}",
                self.path.display()
            )
        })?;
        Ok(file)
    }

    fn read_unlocked(&self) -> Result<StoreFile, String> {
        match std::fs::read(&self.path) {
            Ok(contents) => serde_json::from_slice(&contents)
                .map_err(|error| format!("invalid session store {}: {error}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(StoreFile::default()),
            Err(error) => Err(format!(
                "failed to read session store {}: {error}",
                self.path.display()
            )),
        }
    }

    fn write_unlocked(&self, file: &StoreFile) -> Result<(), String> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create session store directory: {error}"))?;
        }
        let temporary = temporary_path(&self.path);
        let contents = serde_json::to_vec(file)
            .map_err(|error| format!("failed to encode session store: {error}"))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary).map_err(|error| {
            format!(
                "failed to open session store {}: {error}",
                temporary.display()
            )
        })?;
        output.write_all(&contents).map_err(|error| {
            format!(
                "failed to write session store {}: {error}",
                temporary.display()
            )
        })?;
        output.sync_all().map_err(|error| {
            format!(
                "failed to sync session store {}: {error}",
                temporary.display()
            )
        })?;
        drop(output);
        std::fs::rename(&temporary, &self.path).map_err(|error| {
            format!(
                "failed to replace session store {}: {error}",
                self.path.display()
            )
        })
    }
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn session(id: String) -> PersistedSession {
        PersistedSession {
            session_id: id,
            cwd: PathBuf::from("/workspace"),
            history: vec![json!({"role": "user", "content": "saved history"})],
            model: "test-model".into(),
            updated_at: 0,
            cost_usd: 0.0,
        }
    }

    // Invoked in a fresh test process by concurrent_processes_preserve_sessions.
    #[test]
    fn session_store_process_worker() {
        let Some(root) = std::env::var_os("AMARCODE_TEST_SESSION_STORE_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let worker = std::env::var("AMARCODE_TEST_SESSION_STORE_WORKER").unwrap();
        let store = SessionStore::new(root.join("nested/sessions.json"), 86400);
        std::fs::write(root.join(format!("ready-{worker}")), "").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !root.join("start").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "start signal timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for index in 0..24 {
            let id = format!("{worker}-{index}");
            store.upsert(session(id.clone())).unwrap();
            assert!(store.load().unwrap().contains_key(&SessionId::new(id)));
        }
        assert!(store
            .delete(&SessionId::new(format!("obsolete-{worker}")))
            .unwrap());
    }

    #[test]
    fn concurrent_processes_preserve_sessions() {
        use std::{
            process::Command,
            time::{Duration, Instant},
        };

        let root =
            std::env::temp_dir().join(format!("amarcode-session-processes-{}", Uuid::new_v4()));
        let store = SessionStore::new(root.join("nested/sessions.json"), 86400);
        for worker in 0..3 {
            store.upsert(session(format!("obsolete-{worker}"))).unwrap();
        }
        {
            let _guard = store.lock_file().unwrap();
            let mut saved = store.read_unlocked().unwrap();
            saved.sessions.push(session("expired".into()));
            store.write_unlocked(&saved).unwrap();
        }
        // Keep children alive and clean them up even if an assertion fails.
        struct Children(Vec<std::process::Child>);
        impl Drop for Children {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        let mut children = Children(Vec::new());
        let guard = store.lock_file().unwrap();
        for worker in 0..3 {
            children.0.push(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "persistence::tests::session_store_process_worker",
                        "--nocapture",
                    ])
                    .env("AMARCODE_TEST_SESSION_STORE_ROOT", &root)
                    .env("AMARCODE_TEST_SESSION_STORE_WORKER", worker.to_string())
                    .spawn()
                    .unwrap(),
            );
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !(0..3).all(|worker| root.join(format!("ready-{worker}")).exists()) {
            assert!(Instant::now() < deadline, "workers did not start");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(root.join("start"), "").unwrap();
        std::thread::sleep(Duration::from_millis(150));
        // A process-local mutex would let the workers modify this file now.
        assert_eq!(store.read_unlocked().unwrap().sessions.len(), 4);
        for child in &mut children.0 {
            assert!(
                child.try_wait().unwrap().is_none(),
                "worker bypassed the lock"
            );
        }
        drop(guard);
        for child in &mut children.0 {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "worker failed: {status}");
                    break;
                }
                assert!(Instant::now() < deadline, "worker did not finish");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let saved = store.load().unwrap();
        assert_eq!(saved.len(), 72);
        for worker in 0..3 {
            for index in 0..24 {
                let id = SessionId::new(format!("{worker}-{index}"));
                assert_eq!(saved[&id].history, session(id.to_string()).history);
            }
        }
        drop(children);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn store() -> (SessionStore, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "amarcode-acp-session-store-{}.json",
            Uuid::new_v4()
        ));
        (SessionStore::new(path.clone(), 24 * 60 * 60), path)
    }

    #[test]
    fn sessions_round_trip_and_delete() {
        let (store, path) = store();
        let id = SessionId::new("session-one");
        store
            .upsert(PersistedSession {
                session_id: id.to_string(),
                cwd: PathBuf::from("/workspace"),
                history: vec![json!({ "role": "user", "content": "hello" })],
                model: "test-model".into(),
                updated_at: 0,
                cost_usd: 0.0,
            })
            .expect("persist session");

        let loaded = store.load().expect("load sessions");
        assert_eq!(loaded[&id].model, "test-model");
        assert_eq!(loaded[&id].history[0]["content"], "hello");
        assert!(store.delete(&id).expect("delete session"));
        assert!(store.load().expect("reload sessions").is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn expired_sessions_are_pruned() {
        let (mut store, path) = store();
        store.ttl_seconds = 1;
        let file = StoreFile {
            sessions: vec![PersistedSession {
                session_id: "expired".into(),
                cwd: PathBuf::from("/workspace"),
                history: Vec::new(),
                model: "test-model".into(),
                updated_at: now_seconds().saturating_sub(2),
                cost_usd: 0.0,
            }],
        };
        store.write_unlocked(&file).expect("write expired fixture");
        assert!(store.load().expect("prune sessions").is_empty());
        let _ = std::fs::remove_file(path);
    }
}
