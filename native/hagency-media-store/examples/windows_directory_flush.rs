//! Disposable native qualification, never an application durability setter.
#[cfg(windows)]
#[path = "windows_directory_flush/fixture.rs"]
mod fixture;
#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "windows_directory_flush/handles.rs"]
mod handles;
#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "windows_directory_flush/token.rs"]
mod token;

#[cfg(windows)]
#[derive(Clone, Copy)]
struct Failure {
    phase: &'static str,
    code: i64,
}
#[cfg(windows)]
impl Failure {
    fn refused(phase: &'static str) -> Self {
        Self { phase, code: -1 }
    }
    fn io(phase: &'static str, error: std::io::Error) -> Self {
        Self {
            phase,
            code: error.raw_os_error().map_or(-1, i64::from),
        }
    }
    fn last(phase: &'static str) -> Self {
        Self::io(phase, std::io::Error::last_os_error())
    }
}
#[cfg(windows)]
type Result<T> = std::result::Result<T, Failure>;

#[cfg(windows)]
fn execute() -> i32 {
    let result = match std::env::var("HAGENCY_DIRECTORY_PROBE_CHILD").as_deref() {
        Ok("stage") => fixture::child(false),
        Ok("restore") => fixture::child(true),
        Err(std::env::VarError::NotPresent) => fixture::controller(),
        _ => Err(Failure::refused("mode")),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!(
                "{{\"phase\":\"{}\",\"qualified\":false,\"code\":{}}}",
                error.phase, error.code
            );
            78
        }
    }
}
fn main() {
    #[cfg(windows)]
    std::process::exit(execute());
    #[cfg(not(windows))]
    {
        eprintln!("{{\"phase\":\"platform\",\"qualified\":false,\"code\":-1}}");
        std::process::exit(78);
    }
}

#[cfg(all(test, windows))]
#[test]
fn native_windows_directory_probe() {
    assert_eq!(
        execute(),
        0,
        "actual Windows qualification refused; preserve original evidence"
    );
}

// MS-FSCC FileFsDeviceInformation. Keep this pure gate visible to native tests;
// every accepted actual device still needs the complete Windows evidence path.
#[cfg(any(windows, test))]
fn supported_profile(device: u32, characteristics: u32) -> bool {
    const FILE_DEVICE_IS_MOUNTED: u32 = 0x20;
    const FILE_DEVICE_ALLOW_APPCONTAINER_TRAVERSAL: u32 = 0x20000;
    device == 7
        && characteristics & FILE_DEVICE_IS_MOUNTED != 0
        && characteristics & !(FILE_DEVICE_IS_MOUNTED | FILE_DEVICE_ALLOW_APPCONTAINER_TRAVERSAL)
            == 0
}

#[test]
fn native_windows_directory_profile_flags() {
    assert!(supported_profile(7, 0x20));
    assert!(supported_profile(7, 0x20020));
    for value in [0, 0x20000, u32::MAX] {
        assert!(!supported_profile(7, value));
    }
    for bit in 0..32 {
        if bit != 5 && bit != 17 {
            assert!(!supported_profile(7, 0x20 | (1 << bit)));
        }
    }
    for device in [0, 2, 8, 0x14, u32::MAX] {
        assert!(!supported_profile(device, 0x20));
        assert!(!supported_profile(device, 0x20020));
    }
    #[cfg(windows)]
    {
        use windows_sys::Wdk::System::SystemServices::{
            FILE_DEVICE_ALLOW_APPCONTAINER_TRAVERSAL, FILE_DEVICE_IS_MOUNTED,
        };
        assert_eq!(FILE_DEVICE_IS_MOUNTED, 0x20);
        assert_eq!(FILE_DEVICE_ALLOW_APPCONTAINER_TRAVERSAL, 0x20000);
    }
}
