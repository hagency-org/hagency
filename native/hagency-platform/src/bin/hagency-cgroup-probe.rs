//! Opt-in Linux qualification executable, not an automatic passing/ignored test.
//! Requires a host-provisioned private test directory and inherited descriptors
//! passed as three distinct numeric arguments: directory, procs WRONLY, kill WRONLY. A trusted
//! provisioner must empty capability sets/bounding set, set NNP and drop root
//! before exec. No cgroup mount, creation, chmod, setuid or cap change occurs here.
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("refused: Linux cgroup qualification is unavailable on this platform");
    std::process::exit(78);
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod linux {
    use hagency_platform::{CgroupRecovery, Launch, StopCause, SupervisedProcess};
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        io::{self, Read},
        os::{
            fd::{FromRawFd, OwnedFd},
            unix::fs::MetadataExt,
        },
        path::Path,
        time::{Duration, Instant},
    };

    fn inherited(number: i32) -> io::Result<OwnedFd> {
        // SAFETY: This single-threaded disposable executable owns the three
        // explicitly supplied initial FDs. Validate each before adopting it once;
        // seal it before any spawning or thread creation. No arbitrary fd API.
        unsafe {
            if libc::fcntl(number, libc::F_GETFD) < 0 {
                return Err(io::Error::last_os_error());
            }
            let owned = OwnedFd::from_raw_fd(number);
            if libc::fcntl(number, libc::F_SETFD, libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(owned)
        }
    }
    fn pulse(path: &Path) -> u64 {
        fs::metadata(path.with_extension("pulse")).map_or(0, |m| m.len())
    }
    fn fresh_pulse(path: &Path) -> io::Result<()> {
        let before = pulse(path);
        let until = Instant::now() + Duration::from_secs(2);
        while pulse(path) <= before {
            if Instant::now() >= until {
                return Err(io::Error::other("live fixture made no progress"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
    fn empty(mut events: File) -> io::Result<()> {
        let mut bytes = String::new();
        events.by_ref().take(4097).read_to_string(&mut bytes)?;
        if bytes.len() > 4096 || !bytes.lines().any(|v| v == "populated 0") {
            return Err(io::Error::other("cgroup is not observed empty"));
        }
        Ok(())
    }
    pub fn run() -> io::Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|v| v == "guardian") {
            return hagency_platform::run_guardian();
        }
        if args.len() != 5
            || ![
                "guardian-death",
                "stop",
                "failed-spawn",
                "guarantee-refused",
                "custodians-abort",
            ]
            .iter()
            .any(|v| args[0] == *v)
        {
            return Err(io::Error::other(
                "qualification mode, private absolute test directory and exactly three inherited FDs required",
            ));
        }
        let mut numbers = Vec::with_capacity(3);
        for value in &args[2..] {
            let number = value
                .to_str()
                .and_then(|v| v.parse::<i32>().ok())
                .filter(|v| *v >= 3 && !numbers.contains(v))
                .ok_or_else(|| io::Error::other("three distinct inherited FDs >=3 required"))?;
            numbers.push(number);
        }
        let directory = inherited(numbers[0])?;
        let procs = inherited(numbers[1])?;
        let kill = inherited(numbers[2])?;
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)?;
        let events = File::from(rustix::fs::openat(
            &directory,
            "cgroup.events",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )?);
        let recovery = CgroupRecovery::from_host_files(directory, procs, kill)?;
        // Admission runs first so namespace fault fixtures cannot be confused
        // with a mapped UID's inability to own this ordinary work directory.
        let root = Path::new(&args[1]);
        let meta = fs::symlink_metadata(root)?;
        if !root.is_absolute()
            || !meta.is_dir()
            || meta.mode() & 0o077 != 0
            || meta.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(io::Error::other("private test directory required"));
        }
        let marker = root.join("custody");
        if marker.with_extension("pulse").exists() || marker.with_extension("kill").exists() {
            return Err(io::Error::other("fresh test directory required"));
        }
        let mut launch = Launch {
            executable: std::env::current_exe()?.with_file_name("hagency-platform-probe"),
            arguments: vec![
                if args[0] == "guardian-death" || args[0] == "custodians-abort" {
                    "detached-kill-guardian".into()
                } else {
                    "detached-root".into()
                },
                marker.as_os_str().into(),
            ],
            directory: root.into(),
            environment: BTreeMap::new(),
            require_crash_containment: args[0] == "guarantee-refused",
        };
        if args[0] == "failed-spawn" {
            launch.executable = root.join("missing-native-executable");
        }
        let outcome = SupervisedProcess::spawn_piped_with_recovery(
            &std::env::current_exe()?,
            &launch,
            recovery,
        );
        if args[0] == "failed-spawn" || args[0] == "guarantee-refused" {
            match outcome {
                Err(e)
                    if args[0] != "guarantee-refused" || e.kind() == io::ErrorKind::Unsupported => {
                }
                _ => return Err(io::Error::other("admission unexpectedly succeeded")),
            }
            empty(events)?;
            if pulse(&marker) != 0 {
                return Err(io::Error::other("refused workspace executed"));
            }
            println!("qualified: requested admission refusal kept the cgroup empty");
            return Ok(());
        }
        let (mut owner, pipes) = outcome?;
        fresh_pulse(&marker)?;
        drop(pipes); // Closing IO is deliberately not the process stop authority.
        fresh_pulse(&marker)?;
        if args[0] == "custodians-abort" {
            fs::write(marker.with_extension("kill"), b"kill guardian")?;
            let until = Instant::now() + Duration::from_secs(2);
            while !marker.with_extension("guardian-killed").exists() {
                if Instant::now() >= until {
                    return Err(io::Error::other("guardian fault not injected"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            fresh_pulse(&marker)?;
            // Intentional abrupt host loss: do not run owner/Recovery Drop.
            // The CI provisioner's retained subtree capability must clean up.
            std::process::exit(87);
        }
        let report = if args[0] == "guardian-death" {
            fs::write(marker.with_extension("kill"), b"kill guardian")?;
            let report = owner
                .wait(Duration::from_secs(3))?
                .ok_or_else(|| io::Error::other("no guardian-loss recovery report"))?;
            if report.cause != StopCause::GuardianLost {
                return Err(io::Error::other("guardian fault not observed"));
            }
            report
        } else {
            owner.stop(Duration::from_secs(3))?
        };
        if !report.scope.whole_tree_stopped || !report.scope.leader_exited {
            return Err(io::Error::other("scope cleanup remains unknown"));
        }
        empty(events)?;
        let final_pulse = pulse(&marker);
        std::thread::sleep(Duration::from_millis(150));
        if pulse(&marker) != final_pulse {
            return Err(io::Error::other(
                "detached fixture continued after cleanup proof",
            ));
        }
        println!("qualified: guardian and detached descendant have no live cgroup execution");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("refused or unresolved: {error}");
        std::process::exit(78);
    }
}
