//! Controlled native fixture for platform ownership checks; never launches models.
use hagency_platform::{Launch, OwnedProcess, SupervisedProcess};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{Duration, Instant},
};

fn environment() -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "".into());
    if let Some(root) = std::env::var_os("SystemRoot") {
        env.insert("SystemRoot".into(), root);
    }
    env
}
fn pulse(marker: &Path) -> io::Result<()> {
    pulse_with_gate(marker, false)
}
fn pulse_with_gate(marker: &Path, pausable: bool) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker.with_extension("pulse"))?;
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        if pausable && marker.with_extension("pause").exists() {
            fs::write(marker.with_extension("paused"), b"paused")?;
            while marker.with_extension("pause").exists() {
                if Instant::now() >= until {
                    return Err(io::Error::other("fixture heartbeat pause timed out"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        file.write_all(b"x")?;
        file.flush()?;
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn detached_command() -> io::Result<std::process::Command> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // The leaf calls setsid itself, after it is a non-group-leader child.
        command.process_group(rustix::process::getpgrp().as_raw_pid());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(command)
}
fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|v| v == "guardian") {
        return hagency_platform::run_guardian();
    }
    if args.len() < 2 {
        return Err(io::Error::other("probe mode and marker required"));
    }
    let marker = Path::new(&args[1]);
    match args[0].to_str() {
        Some("leaf") => {
            fs::write(marker.with_extension("entered"), b"entered")?;
            pulse(marker)
        }
        Some("pausable-leaf") => pulse_with_gate(marker, true),
        Some("detached-leaf") => {
            #[cfg(unix)]
            rustix::process::setsid()?;
            fs::write(marker.with_extension("entered"), b"detached")?;
            pulse(marker)
        }
        Some("detached-middle") => {
            let child = detached_command()?
                .arg("detached-leaf")
                .arg(marker)
                .spawn()?;
            fs::write(marker.with_extension("detached"), child.id().to_string())?;
            // No wait and no destructor: its child must be adopted by the kernel.
            std::process::exit(0);
        }
        Some("detached-root") | Some("detached-early") | Some("detached-kill-guardian") => {
            let mut child = detached_command()?
                .arg("detached-middle")
                .arg(marker)
                .spawn()?;
            let status = child.wait()?;
            if !status.success() {
                return Err(io::Error::other("detached intermediate failed"));
            }
            fs::write(marker.with_extension("middle-exited"), b"exited")?;
            let until = Instant::now() + Duration::from_secs(3);
            while fs::metadata(marker.with_extension("pulse")).map_or(true, |v| v.len() < 2) {
                if Instant::now() >= until {
                    return Err(io::Error::other("detached leaf did not start"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if args[0] == "detached-early" {
                return Ok(());
            }
            #[cfg(target_os = "linux")]
            if args[0] == "detached-kill-guardian" {
                // Deliberate fault injection in this offline fixture only. The
                // host first confirms Started, then releases this test gate.
                let until = Instant::now() + Duration::from_secs(3);
                while !marker.with_extension("kill").exists() {
                    if Instant::now() >= until {
                        return Err(io::Error::other("guardian fault gate timed out"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let parent = rustix::process::getppid()
                    .ok_or_else(|| io::Error::other("no guardian parent"))?;
                if parent.as_raw_pid() <= 1 {
                    return Err(io::Error::other("invalid fixture guardian"));
                }
                rustix::process::kill_process(parent, rustix::process::Signal::KILL)?;
                fs::write(marker.with_extension("guardian-killed"), b"signal injected")?;
            }
            std::thread::sleep(Duration::from_secs(8));
            Ok(())
        }
        #[cfg(unix)]
        Some("exec-on-command") => {
            use std::os::unix::process::CommandExt;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(marker.with_extension("pulse"))?;
            let until = Instant::now() + Duration::from_secs(8);
            while !marker.with_extension("exec").exists() {
                if Instant::now() >= until {
                    return Err(io::Error::other("exec fixture timed out"));
                }
                file.write_all(b"x")?;
                file.flush()?;
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(file);
            Err(std::process::Command::new(std::env::current_exe()?)
                .arg("leaf")
                .arg(marker)
                .exec())
        }
        Some("leader") | Some("early") => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let mut sockets = 0;
                for entry in fs::read_dir("/dev/fd")? {
                    if fs::metadata(entry?.path()).is_ok_and(|v| v.file_type().is_socket()) {
                        sockets += 1;
                    }
                }
                fs::write(marker.with_extension("sockets"), sockets.to_string())?;
            }
            fs::write(
                marker.with_extension("environment"),
                format!(
                    "{}:{}:{}",
                    std::env::var_os("PATH").is_some_and(|v| v.is_empty()),
                    std::env::var_os("HOME").is_none() && std::env::var_os("USERPROFILE").is_none(),
                    std::env::var("HAGENCY_PROBE_ALLOWED").unwrap_or_default()
                ),
            )?;
            let mut child = std::process::Command::new(std::env::current_exe()?)
                .arg("leaf")
                .arg(marker)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            fs::write(
                marker.with_extension("arguments"),
                args[2..]
                    .iter()
                    .map(|v| v.to_str().unwrap_or("invalid Unicode"))
                    .collect::<Vec<_>>()
                    .join("\0"),
            )?;
            fs::write(marker.with_extension("child"), child.id().to_string())?;
            if args[0] == "early" {
                // Deliberately exit without waiting: cleanup must retain the group/job.
                std::process::exit(0);
            }
            let status = child.wait()?;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other("fixture child stopped"))
            }
        }
        Some("controller-crash") => {
            let _owned = OwnedProcess::spawn(&Launch {
                executable: std::env::current_exe()?,
                arguments: vec!["leaf".into(), marker.as_os_str().into()],
                directory: std::env::current_dir()?,
                environment: environment(),
                require_crash_containment: true,
            })?;
            let until = Instant::now() + Duration::from_secs(5);
            while fs::metadata(marker.with_extension("pulse")).map_or(true, |v| v.len() < 2) {
                if Instant::now() >= until {
                    return Err(io::Error::other("fixture did not become ready"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            fs::write(marker.with_extension("ready"), b"ready")?;
            // Models abrupt owner exit: destructors are intentionally not run.
            std::process::exit(0);
        }
        Some("supervisor-crash") | Some("supervisor-detached-crash") => {
            let executable = std::env::current_exe()?;
            let _owned = SupervisedProcess::spawn(
                &executable,
                &Launch {
                    executable: executable.clone(),
                    arguments: vec![
                        if args[0] == "supervisor-detached-crash" {
                            "detached-root".into()
                        } else {
                            "leader".into()
                        },
                        marker.as_os_str().into(),
                    ],
                    directory: std::env::current_dir()?,
                    environment: environment(),
                    require_crash_containment: cfg!(windows),
                },
            )?;
            let until = Instant::now() + Duration::from_secs(5);
            while fs::metadata(marker.with_extension("pulse")).map_or(true, |v| v.len() < 2) {
                if Instant::now() >= until {
                    return Err(io::Error::other("supervised fixture did not start"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            fs::write(marker.with_extension("ready"), b"ready")?;
            std::process::exit(0);
        }
        _ => Err(io::Error::other("unknown native probe mode")),
    }
}
