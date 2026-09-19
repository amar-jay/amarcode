use std::{
    collections::HashMap,
    fs::OpenOptions,
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
    pub mode: String,
    pub updated_at: u64,
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
        let mut file = self.read_unlocked()?;
        session.updated_at = now_seconds();
        file.sessions
            .retain(|saved| saved.session_id != session.session_id);
        file.sessions.push(session);
        self.write_unlocked(&file)
    }

    pub fn delete(&self, session_id: &SessionId) -> Result<bool, String> {
        let _guard = self.io.lock().map_err(|_| "session store lock poisoned")?;
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
        if let Some(parent) = self.path.parent() {
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
                mode: "code".into(),
                updated_at: 0,
            })
            .expect("persist session");

        let loaded = store.load().expect("load sessions");
        assert_eq!(loaded[&id].mode, "code");
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
                mode: "ask".into(),
                updated_at: now_seconds().saturating_sub(2),
            }],
        };
        store.write_unlocked(&file).expect("write expired fixture");
        assert!(store.load().expect("prune sessions").is_empty());
        let _ = std::fs::remove_file(path);
    }
}
