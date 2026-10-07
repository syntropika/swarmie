use async_trait::async_trait;
use mpai::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Durable native references, delivery queues, metadata, and canonical event transcripts.
pub struct FileSessionStore {
    root: PathBuf,
    journal_locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}
impl FileSessionStore {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        tokio::fs::create_dir_all(&root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).await?;
        }
        Ok(Self {
            root,
            journal_locks: Mutex::new(BTreeMap::new()),
        })
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(Error::Invalid("session id must be a UUID".into()));
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    async fn journal_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.journal_locks
            .lock()
            .await
            .entry(id.into())
            .or_default()
            .clone()
    }
    fn events_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.path(id)?.with_extension("jsonl"))
    }
}
#[async_trait]
impl SessionStore for FileSessionStore {
    fn transcript_durability(&self) -> Option<&'static str> {
        Some("per-event")
    }
    async fn save(&self, s: &SessionSnapshot) -> Result<()> {
        let path = self.path(&s.id)?;
        let tmp = self.root.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let result = async {
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&tmp).await?;
            file.write_all(&serde_json::to_vec(s)?).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&tmp, &path).await?;
            #[cfg(unix)]
            {
                let directory = tokio::fs::File::open(&self.root).await?;
                directory.sync_all().await?;
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        result
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        match tokio::fs::read(self.path(id)?).await {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        let mut files = tokio::fs::read_dir(&self.root).await?;
        let mut result: Vec<SessionSnapshot> = Vec::new();
        while let Some(file) = files.next_entry().await? {
            if file.path().extension().is_some_and(|e| e == "json") {
                let bytes = tokio::fs::read(file.path()).await?;
                result.push(serde_json::from_slice(&bytes)?);
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }
    async fn append_event(&self, event: &RuntimeEvent) -> Result<()> {
        let lock = self.journal_lock(&event.session_id).await;
        let _guard = lock.lock().await;
        let path = self.events_path(&event.session_id)?;
        let mut options = tokio::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&path).await?;
        let length = file.metadata().await?.len();
        if length > 0 {
            file.seek(std::io::SeekFrom::End(-1)).await?;
            let mut last = [0];
            file.read_exact(&mut last).await?;
            // Recover only an incomplete final write. Complete malformed records remain errors.
            if last[0] != b'\n' {
                file.seek(std::io::SeekFrom::Start(0)).await?;
                let mut bytes = vec![];
                file.read_to_end(&mut bytes).await?;
                let valid_length = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |n| n + 1);
                file.set_len(valid_length as u64).await?;
            }
        }
        file.seek(std::io::SeekFrom::End(0)).await?;
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        #[cfg(unix)]
        if length == 0 {
            tokio::fs::File::open(&self.root).await?.sync_all().await?;
        }
        Ok(())
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        let lock = self.journal_lock(id).await;
        let _guard = lock.lock().await;
        let file = match tokio::fs::File::open(self.events_path(id)?).await {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut reader = BufReader::new(file);
        let mut result = vec![];
        let mut line = vec![];
        let mut previous_sequence = 0;
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line).await? == 0 || !line.ends_with(b"\n") {
                break;
            }
            let event: RuntimeEvent = serde_json::from_slice(&line)?;
            if event.session_id != id {
                return Err(Error::Storage(
                    "transcript contains a different session id".into(),
                ));
            }
            if event.sequence <= previous_sequence {
                return Err(Error::Storage("transcript sequences must increase".into()));
            }
            previous_sequence = event.sequence;
            if event.sequence > after {
                result.push(event);
                if result.len() == limit {
                    break;
                }
            }
        }
        Ok(result)
    }
}
