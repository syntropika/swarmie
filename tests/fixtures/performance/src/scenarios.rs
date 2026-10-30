use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use swarmie_git::{
    ArchiveDirectory, ArchiveRestoreRequest, Authority, Cancellation, CommandOutput, CommandSpec,
    DirectoryJournal, Error, Git, Intent, NativeProcess, ProcessExecutor, RepositoryTarget,
    Request, Response, Result,
};

#[derive(Default)]
struct Counts {
    commands: usize,
    output_bytes: usize,
}

struct Executor {
    env: BTreeMap<String, String>,
    counts: Mutex<Counts>,
}

impl ProcessExecutor for Executor {
    fn run(&self, command: &CommandSpec, cancellation: &Cancellation) -> Result<CommandOutput> {
        let mut command = command.clone();
        command.env.extend(self.env.clone());
        let output = NativeProcess.run(&command, cancellation)?;
        let mut counts = self
            .counts
            .lock()
            .map_err(|_| Error::Invalid("counter unavailable".into()))?;
        counts.commands += 1;
        counts.output_bytes += output.stdout.len() + output.stderr.len();
        Ok(output)
    }
}

struct FixtureAuthority(PathBuf);

impl Authority for FixtureAuthority {
    fn authorize(&self, principal: &str, intent: &Intent) -> Result<()> {
        self.authorize_discovery(
            principal,
            Path::new(
                intent
                    .target
                    .root
                    .as_deref()
                    .unwrap_or(&intent.target.git_dir),
            ),
        )
    }

    fn authorize_discovery(&self, principal: &str, path: &Path) -> Result<()> {
        if principal == "performance-fixture" && fs::canonicalize(path)?.starts_with(&self.0) {
            Ok(())
        } else {
            Err(Error::Denied(
                "the performance worker can access only its synthetic fixture".into(),
            ))
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    source: PathBuf,
    git: Git,
    executor: Arc<Executor>,
}

impl Fixture {
    fn new(root: &Path, cancellation: &Cancellation) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("sample-")
            .tempdir_in(root)?;
        let source = directory.path().join("source");
        let home = directory.path().join("home");
        let templates = directory.path().join("templates");
        for path in [&source, &home, &templates] {
            fs::create_dir(path)?;
        }
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let env = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("XDG_CONFIG_HOME", home.to_string_lossy().into_owned()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("GIT_CONFIG_GLOBAL", null.into()),
            ("GIT_CONFIG_SYSTEM", null.into()),
            ("GIT_TEMPLATE_DIR", templates.to_string_lossy().into_owned()),
            ("GIT_TERMINAL_PROMPT", "0".into()),
            ("LC_ALL", "C".into()),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect();
        let executor = Arc::new(Executor {
            env,
            counts: Mutex::new(Counts::default()),
        });
        let git = Git::new(
            executor.clone(),
            Arc::new(DirectoryJournal::open(directory.path().join("journal"))?),
            Arc::new(FixtureAuthority(fs::canonicalize(directory.path())?)),
        );
        let fixture = Self {
            directory,
            source,
            git,
            executor,
        };
        fixture.native(&["init", "--quiet", "-b", "main"], cancellation)?;
        fixture.native(
            &["config", "user.name", "Performance Fixture"],
            cancellation,
        )?;
        fixture.native(
            &["config", "user.email", "fixture@example.invalid"],
            cancellation,
        )?;
        fixture.native(&["config", "commit.gpgsign", "false"], cancellation)?;
        fixture.native(
            &["config", "core.hooksPath", &templates.to_string_lossy()],
            cancellation,
        )?;
        Ok(fixture)
    }

    fn native(&self, args: &[&str], cancellation: &Cancellation) -> Result<()> {
        let output = self.executor.run(
            &CommandSpec {
                program: "git".into(),
                cwd: self.source.clone(),
                args: args.iter().map(|value| (*value).into()).collect(),
                env: BTreeMap::new(),
                timeout: Duration::from_secs(30),
                output_limit: 4 * 1024 * 1024,
            },
            cancellation,
        )?;
        if output.status != 0 || output.truncated {
            return Err(Error::Invalid(format!(
                "synthetic fixture Git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    fn discover(&self, cancellation: &Cancellation) -> Result<RepositoryTarget> {
        match self.git.execute(
            "performance-fixture",
            Request::Discover {
                path: self.source.to_string_lossy().into_owned(),
            },
            cancellation,
        )? {
            Response::Repository {
                repository: Some(target),
            } => Ok(target),
            _ => Err(Error::Invalid(
                "synthetic repository discovery failed".into(),
            )),
        }
    }

    fn reset(&self) {
        *self.executor.counts.lock().expect("fixture counter") = Counts::default();
    }

    fn finish(&self, started: Instant, extra: serde_json::Value) -> serde_json::Value {
        let counts = self.executor.counts.lock().expect("fixture counter");
        serde_json::json!({"elapsed_ms": started.elapsed().as_secs_f64() * 1000.0, "native_commands": counts.commands, "command_output_bytes": counts.output_bytes, "verified_items": extra})
    }
}

pub(super) fn run(
    operation: &str,
    size: usize,
    root: &Path,
    cancellation: &Cancellation,
) -> Result<serde_json::Value> {
    let fixture = Fixture::new(root, cancellation)?;
    match operation {
        "inventory" => inventory(&fixture, size, cancellation),
        "archive" => archive(&fixture, size, cancellation),
        "history" if size <= 200 => history(&fixture, size, cancellation),
        _ => Err(Error::Invalid("unsupported performance operation".into())),
    }
}

fn inventory(
    fixture: &Fixture,
    size: usize,
    cancellation: &Cancellation,
) -> Result<serde_json::Value> {
    for index in 0..size {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        fs::write(
            fixture
                .source
                .join(format!("{index:05}-{}.txt", "a".repeat(90))),
            b"x\n",
        )?;
    }
    fixture.reset();
    let started = Instant::now();
    let target = fixture.discover(cancellation)?;
    let mut status_count = 0;
    let mut file_count = 0;
    let mut status_pages = 0;
    let mut file_pages = 0;
    let mut cursor = None;
    loop {
        let Response::Status { status, next_page } = fixture.git.execute(
            "performance-fixture",
            Request::Status {
                target: target.clone(),
                page: cursor,
            },
            cancellation,
        )?
        else {
            return Err(Error::Invalid("unexpected status response".into()));
        };
        status_count += status.entries.len();
        status_pages += 1;
        cursor = next_page;
        if cursor.is_none() {
            break;
        }
    }
    let mut cursor = None;
    loop {
        let Response::Files { paths, next_page } = fixture.git.execute(
            "performance-fixture",
            Request::Files {
                target: target.clone(),
                page: cursor,
            },
            cancellation,
        )?
        else {
            return Err(Error::Invalid("unexpected files response".into()));
        };
        file_count += paths.len();
        file_pages += 1;
        cursor = next_page;
        if cursor.is_none() {
            break;
        }
    }
    if status_count != size || file_count != size {
        return Err(Error::Invalid("incomplete performance inventory".into()));
    }
    Ok(fixture.finish(started, serde_json::json!({"files": file_count, "status": status_count, "status_pages": status_pages, "file_pages": file_pages})))
}

fn archive(
    fixture: &Fixture,
    size: usize,
    cancellation: &Cancellation,
) -> Result<serde_json::Value> {
    for index in 0..size {
        fs::write(
            fixture.source.join(format!("file-{index:05}.txt")),
            "original\n",
        )?;
    }
    fs::write(fixture.source.join(".gitignore"), "ignored/\n")?;
    fixture.native(&["add", "."], cancellation)?;
    fixture.native(
        &[
            "commit",
            "--quiet",
            "-m",
            "feat: create performance fixture",
        ],
        cancellation,
    )?;
    fs::write(fixture.source.join("file-00000.txt"), "staged\n")?;
    fixture.native(&["add", "file-00000.txt"], cancellation)?;
    fs::write(fixture.source.join("file-00000.txt"), "working\n")?;
    fs::create_dir(fixture.source.join("ignored"))?;
    fs::write(
        fixture.source.join("ignored/cache.txt"),
        "ignored fixture bytes\n",
    )?;
    let target = fixture.discover(cancellation)?;
    let archives = ArchiveDirectory::open(fixture.directory.path().join("archives"))?;
    fixture.reset();
    let started = Instant::now();
    archives.capture(
        &fixture.git,
        "performance-fixture",
        "archive",
        &target,
        cancellation,
    )?;
    let destination = fixture.directory.path().join("restored");
    let restored = archives.restore(
        &fixture.git,
        "performance-fixture",
        &target,
        &ArchiveRestoreRequest {
            archive_id: "archive".into(),
            managed_root: fixture.directory.path().into(),
            destination: destination.clone(),
        },
        cancellation,
    )?;
    if !restored.restore_verified
        || fs::read(destination.join("file-00000.txt"))? != b"working\n"
        || fs::read(destination.join("ignored/cache.txt"))? != b"ignored fixture bytes\n"
    {
        return Err(Error::Invalid("archive verification failed".into()));
    }
    Ok(fixture.finish(
        started,
        serde_json::json!({"tracked_files": size, "verified_restore": true}),
    ))
}

fn history(
    fixture: &Fixture,
    size: usize,
    cancellation: &Cancellation,
) -> Result<serde_json::Value> {
    for index in 0..size {
        fs::write(
            fixture.source.join("history.txt"),
            format!("revision {index}\n"),
        )?;
        fixture.native(&["add", "history.txt"], cancellation)?;
        fixture.native(
            &[
                "commit",
                "--quiet",
                "-m",
                &format!("feat: fixture revision {index}"),
            ],
            cancellation,
        )?;
    }
    let target = fixture.discover(cancellation)?;
    fixture.reset();
    let started = Instant::now();
    let Response::History { commits } = fixture.git.execute(
        "performance-fixture",
        Request::History {
            target,
            limit: size as u32,
        },
        cancellation,
    )?
    else {
        return Err(Error::Invalid("unexpected history response".into()));
    };
    if commits.len() != size {
        return Err(Error::Invalid("incomplete performance history".into()));
    }
    Ok(fixture.finish(started, serde_json::json!({"commits": commits.len()})))
}
