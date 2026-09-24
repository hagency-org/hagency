//! Explicit provider-owned local directories, not managed account readiness.
use crate::Failure;
use cap_std::{ambient_authority, fs::Dir};
use hagency_store::{OwnedClaimProfile, OwnedDispatchScope};
use std::{collections::BTreeMap, ffi::OsString, fs::File, path::PathBuf};

struct Directory {
    path: PathBuf,
    file: File,
}
impl Directory {
    fn open(path: PathBuf) -> Result<Self, Failure> {
        if !path.is_absolute()
            || path.as_os_str().as_encoded_bytes().len() > 4096
            || path.canonicalize().ok().as_ref() != Some(&path)
        {
            return Err(Failure::Admission);
        }
        let file = Dir::open_ambient_dir(&path, ambient_authority())
            .map_err(|_| Failure::Admission)?
            .into_std_file();
        let value = Self { path, file };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<(), Failure> {
        let lost = || Failure::lost_io(crate::AuthoritySite::LocalCodexCheck);
        if self.path.canonicalize().ok().as_ref() != Some(&self.path) {
            return Err(lost());
        }
        let current = Dir::open_ambient_dir(&self.path, ambient_authority())
            .map_err(|_| lost())?
            .into_std_file();
        if !hagency_platform::same_directory(&self.file, &current).map_err(|_| lost())? {
            return Err(lost());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self.file.metadata().map_err(|_| lost())?;
            if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0
            {
                return Err(lost());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(Failure::Admission)
        }
    }
}

/// Host-only explicit selection; never an authentication fact or a wire grant.
/// The provider alone reads its files. Stable ancestors and a trusted same-user
/// host remain required; these checks do not isolate a hostile OS user.
pub struct LocalCodex {
    preset: String,
    seat: String,
    home: Directory,
    codex: Directory,
    path: Option<OsString>,
}
impl LocalCodex {
    pub fn new(
        preset: String,
        seat: String,
        home: PathBuf,
        codex_home: PathBuf,
    ) -> Result<Self, Failure> {
        for id in [&preset, &seat] {
            hagency_core::project::identifier(id, 128).map_err(|_| Failure::Admission)?;
        }
        let value = Self {
            preset,
            seat,
            home: Directory::open(home)?,
            codex: Directory::open(codex_home)?,
            path: std::env::var_os("PATH"),
        };
        value.check()?;
        Ok(value)
    }
    pub(crate) fn check(&self) -> Result<(), Failure> {
        self.home.check()?;
        self.codex.check()
    }
    pub(crate) fn bind_claim(
        &self,
        profile: OwnedClaimProfile,
    ) -> Result<OwnedClaimProfile, Failure> {
        self.check()?;
        profile
            .restrict_resource(self.preset.clone(), self.seat.clone())
            .map_err(|_| Failure::Admission)
    }
    pub(crate) fn admit(&self, scope: &OwnedDispatchScope) -> Result<(), Failure> {
        self.check()?;
        if scope.requires_managed_account()
            || scope.resource().preset_id != self.preset
            || scope.resource().seat_id != self.seat
            || scope.resource().framework != "codex"
            || scope
                .resource()
                .provider
                .as_deref()
                .is_some_and(|provider| provider != "openai")
        {
            return Err(Failure::Admission);
        }
        Ok(())
    }
    pub(crate) fn admit_provision(
        &self,
        scope: &hagency_store::OwnedProvisionScope,
    ) -> Result<(), Failure> {
        self.check()?;
        if scope.requires_managed_account()
            || scope.resource().preset_id != self.preset
            || scope.resource().seat_id != self.seat
            || scope.resource().framework != "codex"
            || scope
                .resource()
                .provider
                .as_deref()
                .is_some_and(|provider| provider != "openai")
        {
            return Err(Failure::Admission);
        }
        Ok(())
    }
    pub(crate) fn separate_from(&self, workspace: &std::path::Path) -> Result<(), Failure> {
        if self.home.path.starts_with(workspace) || self.codex.path.starts_with(workspace) {
            return Err(Failure::Admission);
        }
        Ok(())
    }
    pub(crate) fn apply(
        &self,
        environment: &mut BTreeMap<OsString, OsString>,
    ) -> Result<(), Failure> {
        self.check()?;
        if environment.keys().any(|key| {
            matches!(
                key.to_string_lossy().to_ascii_uppercase().as_str(),
                "OPENAI_API_KEY"
                    | "CODEX_API_KEY"
                    | "OPENAI_BASE_URL"
                    | "AZURE_OPENAI_API_KEY"
                    | "ANTHROPIC_API_KEY"
                    | "API_TOKEN"
                    | "MATRIX_BRIDGE_SECRET"
                    | "HAGENCY_DASHBOARD_TOKEN"
                    | "HAGENCY_SUBCONSCIOUS_EVENT_TOKEN"
                    | "MATRIX_BOT_PASSWORD"
                    | "MATRIX_REG_TOKEN"
                    | "MATRIX_AGENT_PASSWORD_SECRET"
            )
        }) {
            return Err(Failure::Admission);
        }
        environment.insert("HOME".into(), self.home.path.clone().into_os_string());
        environment.insert(
            "CODEX_HOME".into(),
            self.codex.path.clone().into_os_string(),
        );
        // Explicit local-profile opt-in, matching TS runnerEnv's executable
        // search path. Do not inherit keys, proxies or arbitrary host variables.
        if let Some(path) = &self.path {
            environment.insert("PATH".into(), path.clone());
        }
        Ok(())
    }
    pub(crate) async fn watch<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, Failure>>,
    ) -> Result<T, Failure> {
        tokio::pin!(future);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _=tick.tick()=>self.check()?,
                result=&mut future=>{self.check()?;return result;}
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[tokio::test]
    async fn native_local_codex_binding() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        let codex = root.join("codex");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&codex).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
        // An unreadable auth sentinel cannot be inspected by this binding.
        let auth = codex.join("auth.json");
        std::fs::write(&auth, b"opaque provider-owned fixture").unwrap();
        std::fs::set_permissions(&auth, std::fs::Permissions::from_mode(0o000)).unwrap();
        let binding =
            LocalCodex::new("pool".into(), "seat".into(), home.clone(), codex.clone()).unwrap();
        let mut environment = BTreeMap::new();
        binding.apply(&mut environment).unwrap();
        assert_eq!(
            environment.get(&OsString::from("HOME")),
            Some(&home.clone().into_os_string())
        );
        assert_eq!(
            environment.get(&OsString::from("CODEX_HOME")),
            Some(&codex.clone().into_os_string())
        );
        assert!(
            environment
                .keys()
                .all(|key| matches!(key.to_str(), Some("HOME" | "CODEX_HOME" | "PATH")))
        );
        environment.insert("OPENAI_API_KEY".into(), "synthetic-forbidden".into());
        assert!(binding.apply(&mut environment).is_err());
        assert_eq!(
            std::fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0
        );
        let alias = root.join("alias");
        symlink(&codex, &alias).unwrap();
        assert!(LocalCodex::new("pool".into(), "seat".into(), home.clone(), alias).is_err());
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(binding.check().is_err());
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
        let changed = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            std::fs::rename(&codex, root.join("original")).unwrap();
            std::fs::create_dir(&codex).unwrap();
            std::future::pending::<Result<(), Failure>>().await
        };
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), binding.watch(changed))
                .await
                .unwrap(),
            Err(Failure::lost_io(crate::AuthoritySite::LocalCodexCheck))
        );
        // Keep the original provider sentinel unchanged; only fixture teardown
        // restores read permission before the temporary directory is removed.
        std::fs::set_permissions(
            root.join("original/auth.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
}
