use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveState {
    Deferred,
    Pending,
    Completed,
    Failed,
    Unknown,
}

/// Metadata confirmed by the conversation owner, with physical activity reported separately.
#[derive(Clone, Debug)]
pub struct ConversationHoldUpdate {
    pub conversation_id: String,
    pub pinned: bool,
    pub archived: bool,
    /// Known checkout use reported by the host, including other bound conversations.
    pub active: bool,
}

/// A conversation's retention hold is separate from its organizational metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationHold {
    pub conversation_id: String,
    pub target: RepositoryTarget,
    pub pinned: bool,
    pub archived: bool,
    /// Known checkout use reported by the host, including other bound conversations.
    pub active: bool,
    pub generation: u64,
    pub archive_id: Option<String>,
    pub archive_state: Option<ArchiveState>,
    pub warning: Option<String>,
}

fn same_checkout(left: &RepositoryTarget, right: &RepositoryTarget) -> bool {
    left.root == right.root && left.git_dir == right.git_dir && left.common_dir == right.common_dir
}

impl GitService {
    /// Record host-confirmed conversation metadata and physical execution activity.
    ///
    /// The host must hold its profile writer lock and report active=false only after
    /// all known bound conversation executions settle. This does not establish
    /// external-editor or external-process quiescence. This does not grant filesystem authority. Archive
    /// failure remains a separate warning and cannot undo confirmed chat metadata.
    pub fn set_conversation_hold(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        update: ConversationHoldUpdate,
        cancel: &Cancellation,
    ) -> Result<ConversationHold> {
        let ConversationHoldUpdate {
            conversation_id,
            pinned,
            archived,
            active,
        } = update;
        let conversation_id = conversation_id.as_str();
        if conversation_id.is_empty() || conversation_id.len() > 256 {
            return Err(Error::Invalid("invalid conversation identity".into()));
        }
        self.authorize(
            principal,
            target,
            IntentOperation::ConversationHold {
                conversation_id: conversation_id.into(),
                pinned,
                archived,
                active,
            },
        )?;
        self.git.verify_identity(target, cancel)?;
        let directory = self.directory.join("conversation-holds");
        fs::create_dir_all(&directory)?;
        let binding = serde_json::to_vec(&(principal, conversation_id)).map_err(invalid_json)?;
        let key = format!("{:x}", Sha256::digest(binding));
        let path = directory.join(format!("{key}.json"));
        let (mut hold, capture) = {
            let _guard = self
                .retention_lock
                .lock()
                .map_err(|_| Error::Journal("conversation holds are unavailable".into()))?;
            let old = self.read_hold(&path)?;
            if old
                .as_ref()
                .is_some_and(|entry| !same_checkout(&entry.target, target))
            {
                return Err(Error::Denied(
                    "conversation retention belongs to another checkout".into(),
                ));
            }
            let transition = old
                .as_ref()
                .is_none_or(|entry| !entry.archived || entry.active);
            let mut hold = old.unwrap_or_else(|| ConversationHold {
                conversation_id: conversation_id.into(),
                target: target.clone(),
                pinned,
                archived: false,
                active: false,
                generation: 0,
                archive_id: None,
                archive_state: None,
                warning: None,
            });
            // An interrupted archive is inspected through its operation receipt. It
            // must never be silently replayed by a metadata refresh or restart.
            if hold.archive_state == Some(ArchiveState::Pending) {
                hold.archive_state = Some(ArchiveState::Unknown);
                hold.warning =
                    Some("The archive outcome requires inspection; it was not replayed.".into());
            }
            let capture = archived
                && !active
                && (transition || hold.archive_state == Some(ArchiveState::Deferred))
                && hold.archive_state != Some(ArchiveState::Unknown);
            hold.target = target.clone();
            hold.pinned = pinned;
            hold.archived = archived;
            hold.active = active;
            if archived && active {
                if hold.archive_state != Some(ArchiveState::Unknown) {
                    hold.archive_state = Some(ArchiveState::Deferred);
                    hold.warning = Some("The checkout is retained. Archive capture waits for physical execution to settle.".into());
                }
            } else if capture {
                hold.generation = hold
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| Error::Journal("retention generation exhausted".into()))?;
                hold.archive_id = Some(format!("conversation-{key}-{}", hold.generation));
                hold.archive_state = Some(ArchiveState::Pending);
                hold.warning =
                    Some("Archive capture is pending; the checkout remains retained.".into());
            }
            self.write_hold(&path, &hold)?;
            (hold, capture)
        };
        if !capture {
            return Ok(hold);
        }
        let archive_id = hold
            .archive_id
            .clone()
            .ok_or_else(|| Error::Journal("missing archive identity".into()))?;
        let response = self.execute(
            principal,
            WireRequest::Managed(ManagedRequest::ArchiveCreate {
                operation_id: archive_id.clone(),
                target: target.clone(),
                archive_id: archive_id.clone(),
            }),
            cancel,
        );
        let (state, warning) = match response {
            Ok(WireResponse::Managed(ManagedResponse::Archive { .. })) => {
                (ArchiveState::Completed, None)
            }
            Ok(WireResponse::Core(response)) => match *response {
                Response::Receipt { receipt } => match receipt.state {
                    Outcome::Completed => (ArchiveState::Completed, None),
                    Outcome::Failed => (ArchiveState::Failed, Some(receipt.message)),
                    Outcome::InProgress | Outcome::Unknown => {
                        (ArchiveState::Unknown, Some(receipt.message))
                    }
                },
                _ => (
                    ArchiveState::Unknown,
                    Some("The archive result was not confirmed.".into()),
                ),
            },
            Ok(_) => (
                ArchiveState::Unknown,
                Some("The archive result was not confirmed.".into()),
            ),
            Err(error) => (ArchiveState::Unknown, Some(error.to_string())),
        };
        let _guard = self
            .retention_lock
            .lock()
            .map_err(|_| Error::Journal("conversation holds are unavailable".into()))?;
        let mut current = self
            .read_hold(&path)?
            .ok_or_else(|| Error::Journal("conversation hold disappeared".into()))?;
        if current.generation == hold.generation && current.archive_id.as_ref() == Some(&archive_id)
        {
            current.archive_state = Some(state);
            current.warning = warning;
            self.write_hold(&path, &current)?;
        }
        hold = current;
        Ok(hold)
    }

    fn read_hold(&self, path: &Path) -> Result<Option<ConversationHold>> {
        use std::io::Read;
        match fs::File::open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > 64 * 1024 {
                    return Err(Error::Journal("conversation hold exceeds its limit".into()));
                }
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(invalid_json)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write_hold(&self, path: &Path, hold: &ConversationHold) -> Result<()> {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}-{sequence}.pending", std::process::id()));
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(hold).map_err(invalid_json)?)?;
            file.sync_all()?;
            crate::persistence::replace_file(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
