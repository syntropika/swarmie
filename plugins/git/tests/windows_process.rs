#![cfg(windows)]

use std::{
    collections::BTreeMap,
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    thread,
    time::{Duration, Instant},
};

use swarmie_git::{Cancellation, CommandSpec, Error, NativeProcess, ProcessExecutor};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

fn command(script: &str) -> CommandSpec {
    CommandSpec {
        program: "powershell.exe".into(),
        cwd: std::env::current_dir().unwrap(),
        args: vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!("[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding; {script}"),
        ],
        env: BTreeMap::new(),
        timeout: Duration::from_secs(20),
        output_limit: 4096,
    }
}

#[test]
fn windows_output_is_bounded_without_pipe_deadlock() {
    let output = NativeProcess
        .run(
            &command(
                "[Console]::Out.Write(('o' * 100000)); [Console]::Error.Write(('e' * 100000))",
            ),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(output.status, 0);
    assert_eq!(output.stdout.len(), 4096);
    assert_eq!(output.stderr.len(), 4096);
    assert!(output.truncated);
}

#[test]
fn windows_cancellation_before_dispatch_and_missing_executable_have_no_effect() {
    let cancellation = Cancellation::default();
    cancellation.cancel();
    assert!(matches!(
        NativeProcess.run(&command("exit 0"), &cancellation),
        Err(Error::NotStarted(_))
    ));
    let mut missing = command("exit 0");
    missing.program = "swarmie-missing-process-fixture.exe".into();
    assert!(matches!(
        NativeProcess.run(&missing, &Cancellation::default()),
        Err(Error::NotStarted(_))
    ));
}

#[test]
fn windows_arguments_cwd_and_environment_reach_the_child() {
    let directory = tempfile::tempdir().unwrap();
    let mut request = command(
        "[Console]::Write($env:SWARMIE_ARGUMENT_FIXTURE); [Console]::Write('|'); [Console]::Write((Get-Location).Path)",
    );
    request.cwd = directory.path().into();
    let value = "space \"quoted\" & semicolon; \\ unicode-\u{e9}";
    request
        .env
        .insert("SWARMIE_ARGUMENT_FIXTURE".into(), value.into());
    let result = NativeProcess
        .run(&request, &Cancellation::default())
        .unwrap();
    assert_eq!(result.status, 0);
    let expected = format!("{value}|{}", directory.path().display());
    assert_eq!(String::from_utf8(result.stdout).unwrap(), expected);
}

fn descendant(mode: &str) {
    let directory = tempfile::tempdir().unwrap();
    let pid_file = directory.path().join("child.pid");
    let mut request = command(
        "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; $info.Arguments='-NoProfile -NonInteractive -Command \"Start-Sleep -Seconds 120\"'; $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $p=[Diagnostics.Process]::Start($info); [IO.File]::WriteAllText($env:SWARMIE_CHILD_PID, [string]$p.Id); if($env:SWARMIE_PARENT_MODE -eq 'exit') { Start-Sleep -Milliseconds 500; exit 0 }; Start-Sleep -Seconds 120",
    );
    request.env.insert(
        "SWARMIE_CHILD_PID".into(),
        pid_file.to_string_lossy().into_owned(),
    );
    request
        .env
        .insert("SWARMIE_PARENT_MODE".into(), mode.into());
    if mode == "timeout" {
        request.timeout = Duration::from_secs(3);
    }
    let cancellation = Cancellation::default();
    let worker_cancel = cancellation.clone();
    let worker = thread::spawn(move || NativeProcess.run(&request, &worker_cancel));
    let started = Instant::now();
    let pid = loop {
        if let Ok(value) = fs::read_to_string(&pid_file)
            && let Ok(pid) = value.parse::<u32>()
        {
            break pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "child did not start"
        );
        thread::sleep(Duration::from_millis(5));
    };
    // SAFETY: this read-only process handle pins the observed child's identity.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(
        !handle.is_null(),
        "cannot observe child: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: OpenProcess transfers one uniquely owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if mode == "cancel" {
        cancellation.cancel();
    }
    let result = worker.join().unwrap();
    match mode {
        "cancel" => assert!(matches!(result, Err(Error::Cancelled)), "{result:?}"),
        "timeout" => assert!(matches!(result, Err(Error::Timeout)), "{result:?}"),
        "exit" => assert_eq!(result.unwrap().status, 0),
        _ => unreachable!(),
    }
    // SAFETY: the retained handle observes this exact child, independent of PID reuse.
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        0,
        "descendant survived settlement"
    );
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn windows_cancel_settles_descendants_before_returning() {
    descendant("cancel");
}

#[test]
fn windows_timeout_settles_descendants_before_returning() {
    descendant("timeout");
}

#[test]
fn windows_natural_exit_settles_inherited_descendants() {
    descendant("exit");
}
