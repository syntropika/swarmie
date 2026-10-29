mod scenarios;

use std::{path::PathBuf, sync::mpsc, time::Duration};
use swarmie_git::Cancellation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("expected operation, size, output directory and timeout milliseconds".into());
    }
    let size: usize = args[1].parse()?;
    let timeout: u64 = args[3].parse()?;
    if !(1..=5000).contains(&size) || !(100..=600_000).contains(&timeout) {
        return Err("fixture size or deadline is outside its supported range".into());
    }
    let cancellation = Cancellation::default();
    let deadline = cancellation.clone();
    let (send, receive) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if receive.recv_timeout(Duration::from_millis(timeout))
            == Err(mpsc::RecvTimeoutError::Timeout)
        {
            deadline.cancel();
        }
    });
    let result = scenarios::run(&args[0], size, &PathBuf::from(&args[2]), &cancellation);
    drop(send);
    watchdog.join().map_err(|_| "deadline observer panicked")?;
    println!("{}", result?);
    Ok(())
}
