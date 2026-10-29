use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{
    Authority, Cancellation, CommandOutput, CommandSpec, Commit, Diff, Error, ExtensionOperation,
    Intent, IntentOperation, Journal, JournalEntry, Operation, Outcome, PendingOperation,
    ProcessExecutor, Receipt, Remote, RepositoryTarget, Request, Response, Result, Status,
    repository::{TEXT_LIMIT, cwd, preview, redact, relative_path, utf8},
};

fn inventory_page<T: serde::Serialize>(
    values: Vec<T>,
    cursor: Option<&crate::PageCursor>,
    fingerprint: &str,
) -> Result<(Vec<T>, Option<crate::PageCursor>)> {
    use sha2::{Digest, Sha256};

    // Leave room for the response envelope and captured checkout identity.
    const PAGE_BYTES: usize = 24 * 1024;
    let encoded = serde_json::to_vec(&values).map_err(|error| Error::Invalid(error.to_string()))?;
    let mut hash = Sha256::new();
    hash.update(fingerprint.as_bytes());
    hash.update(&encoded);
    let digest = format!("{:x}", hash.finalize());
    let offset = cursor.map_or(0, |page| page.offset);
    if offset > values.len() || cursor.is_some_and(|page| page.digest != digest) {
        return Err(Error::Stale);
    }
    let total = values.len();
    let mut bytes = 2;
    let mut selected = Vec::new();
    for value in values.into_iter().skip(offset) {
        let size = serde_json::to_vec(&value)
            .map_err(|error| Error::Invalid(error.to_string()))?
            .len()
            + 1;
        if bytes + size > PAGE_BYTES {
            if selected.is_empty() {
                return Err(Error::OutputLimit);
            }
            break;
        }
        bytes += size;
        selected.push(value);
    }
    let next_offset = offset + selected.len();
    let next_page = (next_offset < total).then_some(crate::PageCursor {
        offset: next_offset,
        digest,
    });
    Ok((selected, next_page))
}

/// Synchronous capability handle. Hosts offload calls to their execution workers.
#[derive(Clone)]
pub struct Git {
    pub(crate) executor: Arc<dyn ProcessExecutor>,
    pub(crate) journal: Arc<dyn Journal>,
    pub(crate) authority: Arc<dyn Authority>,
    mutations: Arc<Mutex<HashMap<String, Weak<AtomicBool>>>>,
    active: Arc<Mutex<HashSet<String>>>,
}

impl Git {
    pub fn new(
        executor: Arc<dyn ProcessExecutor>,
        journal: Arc<dyn Journal>,
        authority: Arc<dyn Authority>,
    ) -> Self {
        Self {
            executor,
            journal,
            authority,
            mutations: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub(crate) fn run(
        &self,
        cwd: &Path,
        args: &[String],
        mutation: bool,
        cancellation: &Cancellation,
    ) -> Result<CommandOutput> {
        self.run_with_env(cwd, args, mutation, cancellation, BTreeMap::new())
    }

    fn run_with_env(
        &self,
        cwd: &Path,
        args: &[String],
        mutation: bool,
        cancellation: &Cancellation,
        mut env: BTreeMap<String, String>,
    ) -> Result<CommandOutput> {
        env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        // Native hooks and signing remain enabled for user-requested mutations.
        if !mutation {
            env.insert("GIT_OPTIONAL_LOCKS".into(), "0".into());
        }
        let mut command_args = vec![
            "--literal-pathspecs".into(),
            "-c".into(),
            "core.quotepath=false".into(),
        ];
        command_args.extend_from_slice(args);
        self.executor.run(
            &CommandSpec {
                program: "git".into(),
                cwd: cwd.into(),
                args: command_args,
                env,
                timeout: Duration::from_secs(120),
                output_limit: 4 * 1024 * 1024,
            },
            cancellation,
        )
    }

    pub(crate) fn run_checked(
        &self,
        cwd: &Path,
        args: &[String],
        mutation: bool,
        cancellation: &Cancellation,
    ) -> Result<Vec<u8>> {
        let output = self.run(cwd, args, mutation, cancellation)?;
        if output.truncated {
            return Err(Error::OutputLimit);
        }
        if output.status != 0 {
            return Err(Error::Command {
                status: output.status,
                message: receipt_output(&output.stderr),
            });
        }
        Ok(output.stdout)
    }

    /// Authorization validates the complete typed intent; it does not reserve a checkout.
    pub fn validate(&self, principal: &str, request: &Request) -> Result<()> {
        if principal.is_empty() {
            return Err(Error::Denied(
                "an authenticated principal is required".into(),
            ));
        }
        let (target, operation) = match request {
            Request::Discover { path } => {
                return self
                    .authority
                    .authorize_discovery(principal, Path::new(path));
            }
            Request::Recover { operation_id } => {
                let entry = self
                    .journal
                    .load(operation_id)?
                    .ok_or_else(|| Error::Invalid("unknown operation ID".into()))?;
                let (owner, target, operation) = match entry {
                    JournalEntry::Pending(pending) => {
                        (pending.principal, pending.target, pending.operation)
                    }
                    JournalEntry::Settled(receipt) => {
                        (receipt.principal, receipt.target, receipt.operation)
                    }
                };
                if owner != principal {
                    return Err(Error::Denied(
                        "operation belongs to another principal".into(),
                    ));
                }
                return self.authority.authorize(
                    principal,
                    &Intent {
                        target,
                        operation: IntentOperation::Recover {
                            operation_id: operation_id.clone(),
                            operation,
                        },
                    },
                );
            }
            Request::Mutate {
                target, operation, ..
            } => (
                target,
                IntentOperation::Mutate {
                    operation: operation.clone(),
                },
            ),
            Request::Status { target, .. } => (target, IntentOperation::Status),
            Request::Diff {
                target,
                path,
                staged,
                mode,
            } => (
                target,
                IntentOperation::Diff {
                    path: path.clone(),
                    staged: *staged,
                    mode: *mode,
                },
            ),
            Request::History { target, limit } => {
                (target, IntentOperation::History { limit: *limit })
            }
            Request::Remotes { target } => (target, IntentOperation::Remotes),
            Request::Files { target, .. } => (target, IntentOperation::Files),
            Request::File {
                target,
                path,
                revision,
            } => (
                target,
                IntentOperation::File {
                    path: path.clone(),
                    revision: revision.clone(),
                },
            ),
            Request::Operations { target, .. } => (target, IntentOperation::Operations),
        };
        self.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation,
            },
        )
    }

    pub fn execute(
        &self,
        principal: &str,
        request: Request,
        cancellation: &Cancellation,
    ) -> Result<Response> {
        if let Err(error) = self.validate(principal, &request) {
            if let Request::Mutate {
                operation_id,
                target,
                operation,
            } = &request
            {
                return self
                    .reject_before_admission(
                        principal,
                        operation_id.clone(),
                        target.clone(),
                        operation.clone(),
                        error,
                    )
                    .map(|receipt| Response::Receipt {
                        receipt: Box::new(receipt),
                    });
            }
            return Err(error);
        }
        match request {
            Request::Discover { path } => {
                let repository = self.discover(Path::new(&path), cancellation)?;
                if let Some(target) = &repository {
                    self.authority.authorize(
                        principal,
                        &Intent {
                            target: target.clone(),
                            operation: IntentOperation::Discover,
                        },
                    )?;
                }
                Ok(Response::Repository { repository })
            }
            Request::Status { target, page } => {
                let target = self.refresh(&target, cancellation)?;
                if target.observation_error.is_some() {
                    return Err(Error::OutputLimit);
                }
                let entries = crate::parse_status(&self.status_bytes(&target, cancellation)?)?;
                let (entries, next_page) =
                    inventory_page(entries, page.as_ref(), &target.fingerprint)?;
                Ok(Response::Status {
                    status: Status { target, entries },
                    next_page,
                })
            }
            Request::Diff {
                target,
                path,
                staged,
                mode,
            } => {
                let target = self.refresh(&target, cancellation)?;
                if !staged && let Some(path) = path.as_deref() {
                    relative_path(path)?;
                    let untracked =
                        crate::parse_status(&self.status_bytes(&target, cancellation)?)?
                            .iter()
                            .any(|entry| {
                                entry.path == path
                                    && matches!(entry.kind, crate::EntryKind::Untracked)
                            });
                    if untracked {
                        let root = target.root.as_ref().ok_or_else(|| {
                            Error::Invalid("untracked diff requires a checkout".into())
                        })?;
                        crate::worktrees::read_file(Path::new(root), path)?.ok_or(Error::Stale)?;
                        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
                        let output = self.run(
                            cwd(&target),
                            &[
                                "diff",
                                "--no-index",
                                "--no-ext-diff",
                                "--no-textconv",
                                "--no-color",
                                "--",
                                null,
                                path,
                            ]
                            .map(str::to_owned),
                            false,
                            cancellation,
                        )?;
                        if !matches!(output.status, 0 | 1) {
                            return Err(Error::Command {
                                status: output.status,
                                message: receipt_output(&output.stderr),
                            });
                        }
                        let (text, truncated) = preview(&output.stdout)?;
                        return Ok(Response::Diff {
                            diff: Diff {
                                text,
                                truncated: truncated || output.truncated,
                                mode,
                            },
                        });
                    }
                }
                let mut args: Vec<String> = [
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--no-renames",
                ]
                .map(str::to_owned)
                .to_vec();
                if staged {
                    args.push("--cached".into());
                }
                args.push("--".into());
                if let Some(path) = path {
                    relative_path(&path)?;
                    args.push(path);
                }
                let output = self.run(cwd(&target), &args, false, cancellation)?;
                if output.status != 0 {
                    return Err(Error::Command {
                        status: output.status,
                        message: receipt_output(&output.stderr),
                    });
                }
                let (text, truncated) = preview(&output.stdout)?;
                Ok(Response::Diff {
                    diff: Diff {
                        text,
                        truncated: output.truncated || truncated,
                        mode,
                    },
                })
            }
            Request::History { target, limit } => {
                let target = self.refresh(&target, cancellation)?;
                if target.head.is_none() {
                    return Ok(Response::History {
                        commits: Vec::new(),
                    });
                }
                let bytes = self.run_checked(
                    cwd(&target),
                    &[
                        "log".into(),
                        format!("-n{}", limit.clamp(1, 200)),
                        "--format=%H%x00%P%x00%an%x00%aI%x00%s%x00".into(),
                        "HEAD".into(),
                        "--".into(),
                    ],
                    false,
                    cancellation,
                )?;
                let mut fields = bytes.split(|byte| *byte == 0);
                let mut commits = Vec::new();
                while let Some(oid) = fields.next() {
                    let oid = utf8(oid)?.trim_matches('\n');
                    if oid.is_empty() {
                        continue;
                    }
                    let mut next = || {
                        fields
                            .next()
                            .ok_or_else(|| Error::Invalid("malformed history record".into()))
                            .and_then(utf8)
                            .map(str::to_owned)
                    };
                    commits.push(Commit {
                        oid: oid.into(),
                        parents: next()?.split_whitespace().map(str::to_owned).collect(),
                        author: next()?,
                        authored_at: next()?,
                        subject: next()?,
                    });
                }
                Ok(Response::History { commits })
            }
            Request::Remotes { target } => Ok(Response::Remotes {
                remotes: self.remotes(&self.refresh(&target, cancellation)?, cancellation)?,
            }),
            Request::Files { target, page } => {
                let target = self.refresh(&target, cancellation)?;
                if target.bare {
                    return Ok(Response::Files {
                        paths: Vec::new(),
                        next_page: None,
                    });
                }
                let bytes = self.run_checked(
                    cwd(&target),
                    &[
                        "ls-files",
                        "--cached",
                        "--others",
                        "--exclude-standard",
                        "-z",
                    ]
                    .map(str::to_owned),
                    false,
                    cancellation,
                )?;
                let mut paths = bytes
                    .split(|byte| *byte == 0)
                    .filter(|value| !value.is_empty())
                    .map(|value| utf8(value).map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?;
                paths.sort();
                paths.dedup();
                let (paths, next_page) = inventory_page(paths, page.as_ref(), &target.fingerprint)?;
                Ok(Response::Files { paths, next_page })
            }
            Request::File {
                target,
                path,
                revision,
            } => {
                let target = self.refresh(&target, cancellation)?;
                relative_path(&path)?;
                let bytes = if let Some(revision) = revision {
                    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
                        return Err(Error::Invalid("invalid revision".into()));
                    }
                    let oid = self.value(
                        cwd(&target),
                        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
                        cancellation,
                    )?;
                    self.run_checked(
                        cwd(&target),
                        &["show".into(), format!("{oid}:{path}")],
                        false,
                        cancellation,
                    )?
                } else {
                    let root = target.root.as_ref().ok_or_else(|| {
                        Error::Invalid("bare repository has no working files".into())
                    })?;
                    let canonical = fs::canonicalize(Path::new(root).join(path))?;
                    if !canonical.starts_with(root) {
                        return Err(Error::Denied("file resolves outside the checkout".into()));
                    }
                    let inside = canonical
                        .strip_prefix(root)
                        .map_err(|_| Error::Denied("file resolves outside the checkout".into()))?;
                    relative_path(
                        inside
                            .to_str()
                            .ok_or_else(|| Error::Invalid("non-UTF-8 file path".into()))?,
                    )?;
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    if !fs::metadata(&canonical)?.is_file() {
                        return Err(Error::Invalid("only regular files can be previewed".into()));
                    }
                    if cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let file = crate::repository::open_regular_file(&canonical)?;
                    file.take(TEXT_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
                    if cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    bytes
                };
                if bytes.contains(&0) {
                    return Err(Error::Invalid("binary file preview is unavailable".into()));
                }
                let (text, truncated) = preview(&bytes)?;
                Ok(Response::File { text, truncated })
            }
            Request::Mutate {
                operation_id,
                target,
                operation,
            } => {
                let outcome = self.mutate(
                    principal,
                    operation_id.clone(),
                    target.clone(),
                    operation.clone(),
                    cancellation,
                );
                let receipt = match outcome {
                    Ok(receipt) => receipt,
                    Err(error) => self.reject_before_admission(
                        principal,
                        operation_id,
                        target,
                        operation,
                        error,
                    )?,
                };
                Ok(Response::Receipt {
                    receipt: Box::new(receipt),
                })
            }
            Request::Operations { target, page } => {
                self.operations(principal, &target, page.as_ref())
            }
            Request::Recover { operation_id } => Ok(Response::Receipt {
                receipt: Box::new(self.inspect_recovery(principal, &operation_id, cancellation)?),
            }),
        }
    }

    fn refresh(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<RepositoryTarget> {
        let current = self
            .discover(cwd(target), cancellation)?
            .ok_or(Error::Stale)?;
        if target.git_dir != current.git_dir
            || target.common_dir != current.common_dir
            || target.root != current.root
        {
            return Err(Error::Stale);
        }
        Ok(current)
    }

    pub(crate) fn remotes(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<Vec<Remote>> {
        let names = self.value(cwd(target), &["remote"], cancellation)?;
        names
            .lines()
            .map(|name| {
                let fetch_urls = self
                    .value(
                        cwd(target),
                        &["remote", "get-url", "--all", name],
                        cancellation,
                    )?
                    .lines()
                    .map(redact)
                    .collect();
                let push_urls = self
                    .value(
                        cwd(target),
                        &["remote", "get-url", "--push", "--all", name],
                        cancellation,
                    )?
                    .lines()
                    .map(redact)
                    .collect();
                Ok(Remote {
                    name: name.into(),
                    fetch_urls,
                    push_urls,
                })
            })
            .collect()
    }

    fn mutate(
        &self,
        principal: &str,
        operation_id: String,
        target: RepositoryTarget,
        operation: Operation,
        cancellation: &Cancellation,
    ) -> Result<Receipt> {
        let _guard = self.mutation_guard(&target, cancellation)?;
        if let Some(entry) = self.journal.load(&operation_id)? {
            let (original_target, original_operation) = match &entry {
                JournalEntry::Pending(value) => (&value.target, &value.operation),
                JournalEntry::Settled(value) => (&value.target, &value.operation),
            };
            if original_target != &target
                || serde_json::to_value(original_operation).ok()
                    != serde_json::to_value(&operation).ok()
            {
                return Err(Error::Invalid(
                    "operation ID was already used for another request".into(),
                ));
            }
            return self.recover(principal, &operation_id);
        }
        self.verify(&target, cancellation)?;
        let args = self.operation_args(&target, &operation, cancellation)?;
        // Recheck after argument preparation; authorizers may wait for user input.
        self.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: IntentOperation::Mutate {
                    operation: operation.clone(),
                },
            },
        )?;
        self.verify(&target, cancellation)?;
        let reviewed_entries = if matches!(operation, Operation::Commit { .. }) {
            Some(self.run_checked(
                cwd(&target),
                &["ls-files", "--stage", "-z"].map(str::to_owned),
                false,
                cancellation,
            )?)
        } else {
            None
        };
        let pending = PendingOperation {
            operation_id: operation_id.clone(),
            principal: principal.into(),
            target: target.clone(),
            operation: operation.clone(),
        };
        let Some(_active) = self.admit_live(&pending)? else {
            return self.recover(principal, &operation_id);
        };
        let outcome = self.run_mutation(&target, &operation, &args, cancellation);
        let before_effect = matches!(&outcome, Err(MutationFailure::BeforeEffect(_)));
        let (mut state, mut message) = match outcome {
            Ok(output) if output.status == 0 => {
                (Outcome::Completed, receipt_output(&output.stdout))
            }
            Ok(output) => (Outcome::Failed, receipt_output(&output.stderr)),
            Err(MutationFailure::BeforeEffect(error)) => (
                Outcome::Failed,
                format!("Request rejected before execution: {error}"),
            ),
            Err(MutationFailure::AfterEffect(error)) => (
                Outcome::Unknown,
                format!("{error}; inspect and reconcile before another operation"),
            ),
        };
        let head = if before_effect {
            target.head.clone()
        } else {
            self.optional_value(
                cwd(&target),
                &["rev-parse", "--verify", "HEAD"],
                &Cancellation::default(),
            )
            .unwrap_or(None)
        };
        if state == Outcome::Failed
            && !before_effect
            && (matches!(operation, Operation::Push { .. } | Operation::Fetch { .. })
                || head != target.head)
        {
            state = Outcome::Unknown;
            message.push_str("\nGit reported failure but effects may have occurred. Inspect local and remote state; automatic replay is prohibited.");
        }
        if state == Outcome::Completed
            && let Some(reviewed) = reviewed_entries
        {
            match self.committed_index(&target, &Cancellation::default()) {
                    Ok(actual) if actual == reviewed => {},
                    Ok(_) => message.push_str("\nNative hooks changed the committed tree from the reviewed index. The commit exists; inspect it before continuing."),
                    Err(error) => message.push_str(&format!("\nCommit exists but its reviewed tree could not be verified: {error}")),
                }
        }
        let receipt = Receipt {
            operation_id,
            principal: principal.into(),
            target,
            operation,
            result: None,
            state,
            message,
            head,
        };
        self.journal.finish(&receipt)?;
        Ok(receipt)
    }

    /// Persist a rejection only when the journal proves no operation was admitted.
    /// Existing admission is recovered without replacing its outcome. Journal failure
    /// remains unresolved; transport error classification is never evidence of rejection.
    pub(crate) fn reject_before_admission(
        &self,
        principal: &str,
        operation_id: String,
        target: RepositoryTarget,
        operation: Operation,
        error: Error,
    ) -> Result<Receipt> {
        if principal.is_empty() {
            return Err(error);
        }
        if let Some(entry) = self.journal.load(&operation_id)? {
            let (owner, original_target, original_operation) = match &entry {
                JournalEntry::Pending(value) => (&value.principal, &value.target, &value.operation),
                JournalEntry::Settled(value) => (&value.principal, &value.target, &value.operation),
            };
            if owner != principal
                || original_target != &target
                || serde_json::to_value(original_operation).ok()
                    != serde_json::to_value(&operation).ok()
            {
                return Err(error);
            }
            return self.recover(principal, &operation_id);
        }
        let pending = PendingOperation {
            operation_id: operation_id.clone(),
            principal: principal.into(),
            target: target.clone(),
            operation: operation.clone(),
        };
        if !self.journal.admit(&pending)? {
            return self.recover_matching(
                principal,
                &operation_id,
                &target,
                &operation,
                &Cancellation::default(),
            );
        }
        let receipt = Receipt {
            operation_id,
            principal: principal.into(),
            head: target.head.clone(),
            target,
            operation,
            state: Outcome::Failed,
            result: None,
            message: format!("Request rejected before execution: {error}"),
        };
        self.journal.finish(&receipt)?;
        Ok(receipt)
    }

    fn run_mutation(
        &self,
        target: &RepositoryTarget,
        operation: &Operation,
        args: &[String],
        cancellation: &Cancellation,
    ) -> std::result::Result<CommandOutput, MutationFailure> {
        if !matches!(
            operation,
            Operation::Stage { .. } | Operation::Unstage { .. } | Operation::Commit { .. }
        ) {
            return self
                .run(cwd(target), args, true, cancellation)
                .map_err(MutationFailure::dispatch);
        }
        let index_path = self.value(
            cwd(target),
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
            cancellation,
        )?;
        let mut session = IndexSession::open(Path::new(&index_path))?;
        self.verify(target, cancellation)?;
        let mut env = BTreeMap::new();
        env.insert(
            "GIT_INDEX_FILE".into(),
            session
                .private_index
                .to_str()
                .ok_or_else(|| Error::Invalid("non-UTF-8 private index path".into()))?
                .into(),
        );
        if let Operation::Stage { paths } = operation {
            let root = target
                .root
                .as_ref()
                .ok_or_else(|| Error::Invalid("staging requires a checkout".into()))?;
            let snapshot = session.directory.path().join("working");
            fs::create_dir(&snapshot)?;
            let mut attributes = std::collections::BTreeSet::new();
            for path in paths {
                let ignored = self.run(
                    cwd(target),
                    &["check-ignore", "--quiet", "--", path].map(str::to_owned),
                    false,
                    cancellation,
                )?;
                if ignored.status == 0 {
                    return Err(Error::Invalid(
                        "ignored files cannot be staged by this profile".into(),
                    )
                    .into());
                }
                if let Some(file) = crate::worktrees::read_file(Path::new(root), path)? {
                    crate::worktrees::write_file(&snapshot, &file)?;
                }
                let mut parent = Path::new(path).parent();
                while let Some(directory) = parent {
                    for name in [".gitattributes", ".gitignore"] {
                        attributes.insert(directory.join(name));
                    }
                    parent = directory.parent();
                }
            }
            for path in attributes {
                let path = path
                    .to_str()
                    .ok_or_else(|| Error::Invalid("non-UTF-8 attributes path".into()))?;
                if let Some(file) = crate::worktrees::read_file(Path::new(root), path)? {
                    crate::worktrees::write_file(&snapshot, &file)?;
                }
            }
            env.insert(
                "GIT_WORK_TREE".into(),
                snapshot
                    .to_str()
                    .ok_or_else(|| Error::Invalid("non-UTF-8 working snapshot path".into()))?
                    .into(),
            );
        }
        self.verify(target, cancellation)?;
        let mut output = self
            .run_with_env(cwd(target), args, true, cancellation, env)
            .map_err(MutationFailure::dispatch)?;
        let failed_commit_index = output.status != 0
            && matches!(operation, Operation::Commit { .. })
            && session.changed().map_err(MutationFailure::AfterEffect)?;
        if output.status == 0 || failed_commit_index {
            session.install().map_err(MutationFailure::AfterEffect)?;
        }
        if failed_commit_index {
            output
                .stderr
                .extend_from_slice(b"\nThe index written during the failed commit was kept.");
        }
        Ok(output)
    }

    fn operation_args(
        &self,
        target: &RepositoryTarget,
        operation: &Operation,
        cancellation: &Cancellation,
    ) -> Result<Vec<String>> {
        let args = match operation {
            Operation::Extension { .. } => {
                return Err(Error::Invalid(
                    "extension operations require the owning service facade".into(),
                ));
            }
            Operation::Stage { paths } | Operation::Unstage { paths } => {
                if target.bare || paths.is_empty() {
                    return Err(Error::Invalid("selected working files are required".into()));
                }
                for path in paths {
                    relative_path(path)?;
                }
                let mut args = if matches!(operation, Operation::Stage { .. }) {
                    vec!["add".into(), "--".into()]
                } else if target.head.is_some() {
                    vec![
                        "restore".into(),
                        "--staged".into(),
                        "--source=HEAD".into(),
                        "--".into(),
                    ]
                } else {
                    vec!["update-index".into(), "--force-remove".into(), "--".into()]
                };
                args.extend(paths.iter().cloned());
                args
            }
            Operation::Commit {
                message,
                reviewed_index,
            } => {
                if target.bare || message.trim().is_empty() || message.contains('\0') {
                    return Err(Error::Invalid(
                        "a nonempty commit message and checkout are required".into(),
                    ));
                }
                if reviewed_index != &target.index_fingerprint {
                    return Err(Error::Stale);
                }
                vec!["commit".into(), "-m".into(), message.clone()]
            }
            Operation::CreateBranch { name } => {
                if target.bare || name.starts_with('-') || name.contains('\0') {
                    return Err(Error::Invalid(
                        "invalid branch name or bare repository".into(),
                    ));
                }
                self.run_checked(
                    cwd(target),
                    &["check-ref-format".into(), "--branch".into(), name.clone()],
                    false,
                    cancellation,
                )?;
                vec![
                    "switch".into(),
                    "--no-guess".into(),
                    "-c".into(),
                    name.clone(),
                ]
            }
            Operation::Fetch { remote } => {
                self.selected_remote(target, remote, false, cancellation)?;
                vec![
                    "fetch".into(),
                    "--no-recurse-submodules".into(),
                    "--".into(),
                    remote.clone(),
                ]
            }
            Operation::Push {
                remote,
                source_oid,
                destination_ref,
            } => {
                self.selected_remote(target, remote, true, cancellation)?;
                if Some(source_oid) != target.head.as_ref()
                    || !source_oid.bytes().all(|value| value.is_ascii_hexdigit())
                    || !matches!(source_oid.len(), 40 | 64)
                {
                    return Err(Error::Invalid(
                        "push requires the captured HEAD object ID".into(),
                    ));
                }
                if !destination_ref.starts_with("refs/heads/") {
                    return Err(Error::Invalid(
                        "push requires an explicit branch destination".into(),
                    ));
                }
                self.run_checked(
                    cwd(target),
                    &["check-ref-format".into(), destination_ref.clone()],
                    false,
                    cancellation,
                )?;
                vec![
                    "push".into(),
                    "--porcelain".into(),
                    "--".into(),
                    remote.clone(),
                    format!("{source_oid}:{destination_ref}"),
                ]
            }
        };
        Ok(args)
    }

    /// Admit a typed owner facade effect before dispatch. A failed or interrupted
    /// effect is conservatively unknown and is never automatically replayed.
    pub(crate) fn execute_extension(
        &self,
        principal: &str,
        extension: ExtensionOperation,
        cancellation: &Cancellation,
        effect: impl FnOnce() -> Result<serde_json::Value>,
    ) -> Result<Receipt> {
        let ExtensionOperation {
            intent,
            operation_id,
            target,
            name,
            arguments,
        } = extension;
        if principal.is_empty() {
            return Err(Error::Denied(
                "an authenticated principal is required".into(),
            ));
        }
        let _guard = self.mutation_guard(&target, cancellation)?;
        let operation = Operation::Extension { name, arguments };
        if self.journal.load(&operation_id)?.is_some() {
            return self.recover_matching(
                principal,
                &operation_id,
                &target,
                &operation,
                cancellation,
            );
        }
        self.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: intent,
            },
        )?;
        self.verify(&target, cancellation)?;
        let pending = PendingOperation {
            operation_id: operation_id.clone(),
            principal: principal.into(),
            target: target.clone(),
            operation: operation.clone(),
        };
        let Some(_active) = self.admit_live(&pending)? else {
            return self.recover_matching(
                principal,
                &operation_id,
                &target,
                &operation,
                cancellation,
            );
        };
        let (state, result, message) = match effect() {
            Ok(value) => (Outcome::Completed, Some(value), String::new()),
            Err(Error::NotStarted(message)) => (
                Outcome::Failed,
                None,
                format!("Request rejected before execution: {message}"),
            ),
            Err(error) => (
                Outcome::Unknown,
                None,
                format!(
                    "{error}; effect outcome needs reconciliation and cannot be automatically replayed"
                ),
            ),
        };
        let head = self
            .optional_value(
                cwd(&target),
                &["rev-parse", "--verify", "HEAD"],
                &Cancellation::default(),
            )
            .unwrap_or(None);
        let receipt = Receipt {
            operation_id,
            principal: principal.into(),
            target,
            operation,
            result,
            state,
            message,
            head,
        };
        self.journal.finish(&receipt)?;
        Ok(receipt)
    }

    fn mutation_guard(
        &self,
        target: &RepositoryTarget,
        cancel: &Cancellation,
    ) -> Result<MutationGuard> {
        let flag = {
            let mut locks = self
                .mutations
                .lock()
                .map_err(|_| Error::Journal("mutation lock poisoned".into()))?;
            locks.retain(|_, value| value.strong_count() > 0);
            if let Some(flag) = locks.get(&target.common_dir).and_then(Weak::upgrade) {
                flag
            } else {
                let flag = Arc::new(AtomicBool::new(false));
                locks.insert(target.common_dir.clone(), Arc::downgrade(&flag));
                flag
            }
        };
        loop {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if flag
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(MutationGuard(flag));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn admit_live(&self, pending: &PendingOperation) -> Result<Option<ActiveOperation>> {
        // Recovery waits for publication and live ownership to become visible together.
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Journal("active operation lock poisoned".into()))?;
        if !self.journal.admit(pending)? {
            return Ok(None);
        }
        active.insert(pending.operation_id.clone());
        Ok(Some(ActiveOperation {
            active: self.active.clone(),
            id: pending.operation_id.clone(),
        }))
    }

    fn is_active(&self, operation_id: &str) -> Result<bool> {
        Ok(self
            .active
            .lock()
            .map_err(|_| Error::Journal("active operation lock poisoned".into()))?
            .contains(operation_id))
    }

    fn operations(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        page: Option<&crate::PageCursor>,
    ) -> Result<Response> {
        use sha2::{Digest, Sha256};
        let scan = self.journal.scan()?;
        let mut unavailable = scan.unavailable;
        let mut operations = Vec::new();
        for entry in scan.entries {
            let (owner, repository, id) = match &entry {
                JournalEntry::Pending(value) => {
                    (&value.principal, &value.target, &value.operation_id)
                }
                JournalEntry::Settled(value) => {
                    (&value.principal, &value.target, &value.operation_id)
                }
            };
            if owner == principal && repository.common_dir == target.common_dir {
                let receipt = self.recover(principal, id)?;
                let mut message = receipt.message;
                if message.len() > 2048 {
                    let mut end = 2048;
                    while !message.is_char_boundary(end) {
                        end -= 1;
                    }
                    message.truncate(end);
                    message.push_str("\nAdditional receipt text is omitted from this catalog.");
                }
                let entry = crate::RecoveryEntry {
                    operation_id: receipt.operation_id,
                    principal: receipt.principal,
                    target: receipt.target,
                    state: receipt.state,
                    message,
                    head: receipt.head,
                    operation_digest: format!(
                        "{:x}",
                        Sha256::digest(
                            serde_json::to_vec(&receipt.operation)
                                .map_err(|error| Error::Journal(error.to_string()))?
                        )
                    ),
                };
                if serde_json::to_vec(&entry)
                    .map_err(|error| Error::Journal(error.to_string()))?
                    .len()
                    > 24 * 1024
                {
                    if unavailable.len() < 33 {
                        unavailable.push(crate::JournalUnavailable {entry_id:format!("{:x}",Sha256::digest(id.as_bytes())),message:"Operation identity exceeds the catalog budget; retain its durable record for explicit recovery.".into()});
                    }
                } else {
                    operations.push(entry);
                }
            }
        }
        let (operations, next_page) = inventory_page(operations, page, &target.common_dir)?;
        Ok(Response::Operations {
            operations,
            unavailable,
            next_page,
        })
    }

    fn committed_index(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<Vec<u8>> {
        let tree = self.run_checked(
            cwd(target),
            &["ls-tree", "--full-tree", "-r", "-z", "HEAD"].map(str::to_owned),
            false,
            cancellation,
        )?;
        let mut index = Vec::new();
        for record in tree
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let (metadata, path) = utf8(record)?
                .split_once('\t')
                .ok_or_else(|| Error::Invalid("malformed committed tree".into()))?;
            let fields = metadata.split(' ').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(Error::Invalid("malformed committed tree metadata".into()));
            }
            index
                .extend_from_slice(format!("{} {} 0\t{}\0", fields[0], fields[2], path).as_bytes());
        }
        Ok(index)
    }

    fn selected_remote(
        &self,
        target: &RepositoryTarget,
        name: &str,
        pushing: bool,
        cancellation: &Cancellation,
    ) -> Result<()> {
        let remote = self
            .remotes(target, cancellation)?
            .into_iter()
            .find(|remote| remote.name == name)
            .ok_or_else(|| Error::Invalid("select an existing named remote".into()))?;
        let urls = if pushing {
            remote.push_urls
        } else {
            remote.fetch_urls
        };
        if urls.len() != 1 || name.starts_with('-') {
            return Err(Error::Invalid(
                "remote must have exactly one explicit transport URL".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn recover_matching(
        &self,
        principal: &str,
        operation_id: &str,
        target: &RepositoryTarget,
        operation: &Operation,
        _cancellation: &Cancellation,
    ) -> Result<Receipt> {
        self.validate(
            principal,
            &Request::Recover {
                operation_id: operation_id.into(),
            },
        )?;
        let entry = self
            .journal
            .load(operation_id)?
            .ok_or_else(|| Error::Invalid("unknown operation ID".into()))?;
        let (original_target, original_operation) = match &entry {
            JournalEntry::Pending(value) => (&value.target, &value.operation),
            JournalEntry::Settled(value) => (&value.target, &value.operation),
        };
        if original_target != target
            || serde_json::to_value(original_operation).ok() != serde_json::to_value(operation).ok()
        {
            return Err(Error::Invalid(
                "recovery arguments differ from admitted operation".into(),
            ));
        }
        self.recover(principal, operation_id)
    }

    fn inspect_recovery(
        &self,
        principal: &str,
        operation_id: &str,
        cancellation: &Cancellation,
    ) -> Result<Receipt> {
        let mut receipt = self.recover(principal, operation_id)?;
        if receipt.state != Outcome::Unknown {
            return Ok(receipt);
        }
        self.verify_identity(&receipt.target, cancellation)?;
        let current = self
            .discover(cwd(&receipt.target), cancellation)?
            .ok_or(Error::Stale)?;
        self.verify_identity(&receipt.target, cancellation)?;
        let mut inspection = crate::RecoveryInspection {
            head_matches_admission: current.head == receipt.target.head,
            index_matches_admission: !current.index_fingerprint.is_empty()
                && current.index_fingerprint == receipt.target.index_fingerprint,
            repository: current,
            remote_ref: None,
            causality_confirmed: false,
        };
        if let Operation::Push {
            remote,
            source_oid,
            destination_ref,
        } = &receipt.operation
        {
            // Recovery authorization sees the original complete typed operation.
            // A changed remote configuration is observed as current configuration,
            // never silently asserted to be the originally authorized endpoint.
            let remote_state =
                self.inspect_remote_ref(&receipt.target, remote, destination_ref, cancellation);
            let (observed_oid, error) = match remote_state {
                Ok(value) => (value, None),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => (None, Some(redact(&error.to_string()))),
            };
            inspection.remote_ref = Some(crate::RemoteRefInspection {
                remote: remote.clone(),
                destination_ref: destination_ref.clone(),
                matches_requested_source: observed_oid.as_ref() == Some(source_oid),
                observed_oid,
                endpoint_matches_admission: None,
                error,
            });
        }
        let branch = inspection
            .repository
            .branch
            .as_deref()
            .unwrap_or("detached or unborn");
        let head = inspection.repository.head.as_deref().unwrap_or("unborn");
        receipt.message.push_str(&format!("\nRead-only recovery observation: current HEAD {head}; branch {branch}; HEAD matches admission: {}; index matches admission: {}. Observed state does not establish causality or authorize replay.",inspection.head_matches_admission,inspection.index_matches_admission));
        if let Some(remote) = &inspection.remote_ref {
            if let Some(error) = &remote.error {
                receipt.message.push_str(&format!(
                    "\nRemote ref inspection could not be confirmed: {error}"
                ));
            } else {
                receipt.message.push_str(&format!("\nCurrent configured remote {} ref {} is {}; matches requested source: {}. Remote endpoint identity against admission is unverified.",remote.remote,remote.destination_ref,remote.observed_oid.as_deref().unwrap_or("absent"),remote.matches_requested_source));
            }
        }
        // Preserve the immutable durable receipt; inspection is a fresh projection.
        let mut result = match receipt.result.take() {
            Some(serde_json::Value::Object(value)) => value,
            Some(value) => serde_json::Map::from_iter([("effect_result".into(), value)]),
            None => serde_json::Map::new(),
        };
        result.insert(
            "reconciliation".into(),
            serde_json::to_value(inspection).map_err(|error| Error::Invalid(error.to_string()))?,
        );
        receipt.result = Some(serde_json::Value::Object(result));
        Ok(receipt)
    }

    fn inspect_remote_ref(
        &self,
        target: &RepositoryTarget,
        remote: &str,
        destination_ref: &str,
        cancellation: &Cancellation,
    ) -> Result<Option<String>> {
        if !destination_ref.starts_with("refs/heads/") {
            return Err(Error::Invalid(
                "recovery requires an explicit branch ref".into(),
            ));
        }
        self.selected_remote(target, remote, true, cancellation)?;
        let endpoint = self.value(
            cwd(target),
            &["remote", "get-url", "--push", "--all", remote],
            cancellation,
        )?;
        if endpoint.lines().count() != 1 || endpoint.is_empty() || endpoint.starts_with('-') {
            return Err(Error::Invalid(
                "remote recovery requires one explicit push endpoint".into(),
            ));
        }
        self.run_checked(
            cwd(target),
            &["check-ref-format".into(), destination_ref.into()],
            false,
            cancellation,
        )?;
        let output = self.run(
            cwd(target),
            &[
                "ls-remote".into(),
                "--refs".into(),
                "--exit-code".into(),
                "--".into(),
                endpoint,
                destination_ref.into(),
            ],
            false,
            cancellation,
        )?;
        if output.truncated {
            return Err(Error::OutputLimit);
        }
        if output.status == 2 {
            return Ok(None);
        }
        if output.status != 0 {
            return Err(Error::Command {
                status: output.status,
                message: receipt_output(&output.stderr),
            });
        }
        let mut observed = None;
        for line in utf8(&output.stdout)?.lines() {
            let Some((oid, reference)) = line.split_once('\t') else {
                return Err(Error::Invalid("malformed remote ref observation".into()));
            };
            if reference != destination_ref
                || !matches!(oid.len(), 40 | 64)
                || !oid.bytes().all(|value| value.is_ascii_hexdigit())
                || observed.is_some()
            {
                return Err(Error::Invalid("ambiguous remote ref observation".into()));
            }
            observed = Some(oid.into());
        }
        Ok(observed)
    }

    fn recover(&self, principal: &str, operation_id: &str) -> Result<Receipt> {
        self.validate(
            principal,
            &Request::Recover {
                operation_id: operation_id.into(),
            },
        )?;
        match self
            .journal
            .load(operation_id)?
            .ok_or_else(|| Error::Invalid("unknown operation ID".into()))?
        {
            JournalEntry::Settled(receipt) => Ok(receipt),
            JournalEntry::Pending(pending) => {
                let live = self.is_active(operation_id)?;
                Ok(Receipt { operation_id: operation_id.into(), principal: pending.principal, head: pending.target.head.clone(), target: pending.target, operation: pending.operation, result: None,
                    state: if live { Outcome::InProgress } else {Outcome::Unknown},
                    message: if live {"Operation is still running on this execution handle. Wait for its confirmed receipt; do not replay it."} else {"Operation was admitted without a confirmed receipt. Inspect repository and remote state; automatic replay is prohibited."}.into()
                })
            }
        }
    }
}

fn receipt_output(bytes: &[u8]) -> String {
    const LIMIT: usize = 12 * 1024;
    // Redact the complete transport token before clipping its display text.
    let mut text = redact(&String::from_utf8_lossy(bytes));
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    if bytes.len() > LIMIT {
        format!("{text}\nAdditional native command output was omitted from this receipt.")
    } else {
        text
    }
}

enum MutationFailure {
    BeforeEffect(Error),
    AfterEffect(Error),
}
impl MutationFailure {
    fn dispatch(error: Error) -> Self {
        if matches!(error, Error::NotStarted(_)) {
            Self::BeforeEffect(error)
        } else {
            Self::AfterEffect(error)
        }
    }
}
impl From<Error> for MutationFailure {
    fn from(error: Error) -> Self {
        Self::BeforeEffect(error)
    }
}
impl From<std::io::Error> for MutationFailure {
    fn from(error: std::io::Error) -> Self {
        Self::BeforeEffect(error.into())
    }
}

struct MutationGuard(Arc<AtomicBool>);
impl Drop for MutationGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct ActiveOperation {
    active: Arc<Mutex<HashSet<String>>>,
    id: String,
}
impl Drop for ActiveOperation {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}

/// Hold Git's native index lock without stealing or deleting another writer's lock.
struct IndexSession {
    directory: tempfile::TempDir,
    private_index: std::path::PathBuf,
    // A sibling keeps split-index shared-file lookup relative to the live index.
    _private_index_owner: tempfile::TempPath,
    private_lock: std::path::PathBuf,
    live_index: std::path::PathBuf,
    lock_path: std::path::PathBuf,
    lock: Option<fs::File>,
    owns_lock: bool,
    original: Option<Vec<u8>>,
}

impl IndexSession {
    fn open(index: &Path) -> Result<Self> {
        let parent = index
            .parent()
            .ok_or_else(|| Error::Invalid("index has no parent directory".into()))?;
        let lock_path = index.with_extension("lock");
        let lock = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock_path)?;
        let directory = match tempfile::Builder::new()
            .prefix("swarmie-reviewed-")
            .tempdir_in(parent)
        {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&lock_path);
                return Err(error.into());
            }
        };
        let private_index_owner = match tempfile::Builder::new()
            .prefix("swarmie-reviewed-index-")
            .tempfile_in(parent)
        {
            Ok(file) => file.into_temp_path(),
            Err(error) => {
                let _ = fs::remove_file(&lock_path);
                return Err(error.into());
            }
        };
        let private_index = private_index_owner.to_path_buf();
        let mut private_lock_name = private_index.as_os_str().to_owned();
        private_lock_name.push(".lock");
        let mut session = Self {
            directory,
            private_index,
            _private_index_owner: private_index_owner,
            private_lock: private_lock_name.into(),
            live_index: index.into(),
            lock_path,
            lock: Some(lock),
            owns_lock: true,
            original: None,
        };
        // An unborn repository must start without an empty, invalid index file.
        fs::remove_file(&session.private_index)?;
        match fs::read(index) {
            Ok(bytes) => {
                fs::write(&session.private_index, &bytes)?;
                session.original = Some(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(session)
    }

    fn changed(&self) -> Result<bool> {
        let current = match fs::read(&self.private_index) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(current != self.original)
    }

    fn install(&mut self) -> Result<()> {
        use std::io::Write;
        let current = match fs::read(&self.live_index) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if current != self.original {
            return Err(Error::Stale);
        }
        let bytes = fs::read(&self.private_index)?;
        let lock = self
            .lock
            .as_mut()
            .ok_or_else(|| Error::Invalid("index lock is not owned".into()))?;
        lock.write_all(&bytes)?;
        lock.sync_all()?;
        drop(self.lock.take());
        fs::rename(&self.lock_path, &self.live_index)?;
        self.owns_lock = false;
        if let Some(parent) = self.live_index.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

impl Drop for IndexSession {
    fn drop(&mut self) {
        // Only this unique private command's lock is removed after physical settlement.
        let _ = fs::remove_file(&self.private_lock);
        drop(self.lock.take());
        if self.owns_lock {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

#[cfg(test)]
mod index_tests {
    #[test]
    fn failed_index_write_releases_only_the_owned_lock() {
        let directory = tempfile::tempdir().unwrap();
        let index = directory.path().join("index");
        std::fs::write(&index, b"original").unwrap();
        let mut session = super::IndexSession::open(&index).unwrap();
        std::fs::write(&session.private_index, b"changed").unwrap();
        let lock = session.lock_path.clone();
        // A read-only descriptor makes write_all fail deterministically, like an I/O fault.
        session.lock = Some(std::fs::File::open(&lock).unwrap());
        assert!(session.install().is_err());
        drop(session);
        assert!(!lock.exists());
        assert_eq!(std::fs::read(index).unwrap(), b"original");
    }
}
