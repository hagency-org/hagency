//! Host-owned, fresh Codex namespaces. No auth discovery, import or wire grant.
use super::{DomainRepository, OwnedClaimProfile, OwnedDispatchScope, serialize};
use crate::{
    Error, ResourceConfigurationResult, ResourcePublicationAccess, ResourcePublicationCommand,
    ResourcePublicationRetirement, private, resource_publication_revision,
};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use hagency_core::{
    allocation::Ceiling,
    canonical,
    project::{Resource, Seat},
    qualification,
};
use hagency_platform::{DirectoryIdentity, directory_identity, same_directory};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub const ACCOUNT_PROFILE: &str = "codex-default-namespace-v1";
const LIMIT: usize = 16;
fn random_id(prefix: &str) -> Result<String, Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(format!("{prefix}{}", hex(&bytes)))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn valid_id(id: &str) -> Result<(), Error> {
    if !id.strip_prefix("account_").is_some_and(|s| {
        s.len() == 32
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(hagency_core::InvalidInput("invalid native account selection").into());
    }
    Ok(())
}
fn open_root(path: &Path) -> Result<Dir, Error> {
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
            Component::Normal(name) => names.push(name.to_owned()),
            _ => return Err(Error::Private),
        }
    }
    if !path.is_absolute() {
        return Err(Error::Private);
    }
    let mut dir = Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
    for name in names {
        dir = dir.open_dir_nofollow(name)?;
    }
    private::check_handle(&dir.try_clone()?.into_std_file())?;
    Ok(dir)
}
struct Root {
    path: PathBuf,
    dir: Dir,
    identity: DirectoryIdentity,
    retired: AtomicBool,
}
impl Root {
    fn check(&self) -> Result<(), Error> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Error::LocalAuthority);
        }
        let file = self.dir.try_clone()?.into_std_file();
        private::check_handle(&file)?;
        let current = open_root(&self.path)?.into_std_file();
        if directory_identity(&file)? != self.identity || !same_directory(&file, &current)? {
            return Err(Error::Private);
        }
        Ok(())
    }
}
struct Binding {
    root: Arc<Root>,
    file: File,
    association: Association,
    namespace: DirectoryIdentity,
    retired: AtomicBool,
}
impl Binding {
    fn check(&self) -> Result<(), Error> {
        self.root.check()?;
        if self.retired.load(Ordering::Acquire) {
            return Err(Error::LocalAuthority);
        }
        private::check_handle(&self.file)?;
        let current = self
            .root
            .dir
            .open_dir_nofollow(&self.association.id)?
            .into_std_file();
        private::check_handle(&current)?;
        if directory_identity(&self.file)? != self.namespace
            || !same_directory(&self.file, &current)?
        {
            return Err(Error::Private);
        }
        Ok(())
    }
}
/// Data captured by the original writer, not a deserializable account authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Association {
    id: String,
    generation: u64,
    seat: String,
}

/// Never Clone or Deserialize; only the original registry constructs this owner.
pub struct ManagedAccount {
    binding: Arc<Binding>,
}
/// Original namespace custody retained through child cleanup and unknown outcomes.
pub struct ManagedLaunch {
    binding: Arc<Binding>,
}
impl ManagedLaunch {
    pub fn check(&self) -> Result<(), Error> {
        self.binding.check()
    }
    /// Concrete trusted Host operation. No caller-supplied path or generic callback.
    pub fn apply_codex_environment(
        &self,
        environment: &mut BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    ) -> Result<(), Error> {
        self.check()?;
        if environment.keys().any(|key| {
            matches!(
                key.to_string_lossy().to_ascii_uppercase().as_str(),
                "OPENAI_API_KEY" | "CODEX_API_KEY" | "OPENAI_BASE_URL" | "AZURE_OPENAI_API_KEY"
            )
        }) {
            return Err(Error::Unqualified);
        }
        let path = self.binding.root.path.join(&self.binding.association.id);
        let path = path.to_str().ok_or(Error::Private)?.to_owned();
        environment.insert("HOME".into(), path.clone().into());
        environment.insert("CODEX_HOME".into(), path.into());
        Ok(())
    }
}
impl ManagedAccount {
    /// Immediately fences this original binding; durable retirement still requires
    /// the original writer and may report unknown until that observation completes.
    pub fn retire(&self) {
        self.binding.retired.store(true, Ordering::Release);
    }
    pub fn id(&self) -> &str {
        &self.binding.association.id
    }
    pub fn prepare_launch(&self, scope: &OwnedDispatchScope) -> Result<ManagedLaunch, Error> {
        self.binding.check()?;
        if scope.account.as_ref() != Some(&self.binding.association)
            || scope.resource().seat_id != self.binding.association.seat
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(ManagedLaunch {
            binding: self.binding.clone(),
        })
    }
    pub fn bind_claim_profile(
        &self,
        profile: OwnedClaimProfile,
    ) -> Result<OwnedClaimProfile, Error> {
        self.binding.check()?;
        let mut value: serde_json::Value = serde_json::from_str(profile.encoded())?;
        value["account"] = serde_json::to_value(&self.binding.association)?;
        let encoded = serde_json::to_string(&value)?;
        if encoded.len() > 16 * 1024 {
            return Err(Error::Capacity);
        }
        Ok(OwnedClaimProfile::from_account_binding(encoded))
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    Preparing,
    Active,
    Uncertain,
    Retired,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountChoice {
    pub id: String,
    pub ordinal: u8,
    pub state: AccountState,
    pub revision: String,
    pub profile: String,
    pub authentication: String,
    pub quota: Option<u64>,
}
impl AccountChoice {
    fn new(id: String, ordinal: u8, state: AccountState) -> Result<Self, Error> {
        let revision = canonical::digest(
            &serde_json::json!({"version":1,"id":id,"state":state,"ordinal":ordinal,"profile":ACCOUNT_PROFILE}),
        )?;
        Ok(Self {
            id,
            ordinal,
            state,
            revision,
            profile: ACCOUNT_PROFILE.into(),
            authentication: "unknown".into(),
            quota: None,
        })
    }
}
struct Preparation {
    deadline: Instant,
    directory: Option<Dir>,
}
pub(super) struct Registry {
    root: Arc<Root>,
    bindings: BTreeMap<String, Arc<Binding>>,
    preparations: BTreeMap<String, Preparation>,
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.root.retired.store(true, Ordering::Release);
    }
}
impl Registry {
    pub(super) fn open(db: &Connection, directory: &Path) -> Result<Self, Error> {
        // database::open already checks the explicitly selected final private root.
        // Canonicalization resolves host-provisioned ancestors; qualification still
        // walks every resulting component NoFollow and retains the actual object.
        let path = directory.canonicalize()?;
        let dir = open_root(&path)?;
        let identity = directory_identity(&dir.try_clone()?.into_std_file())?;
        let root = Arc::new(Root {
            path,
            dir,
            identity,
            retired: AtomicBool::new(false),
        });
        let mut registry = Self {
            root,
            bindings: BTreeMap::new(),
            preparations: BTreeMap::new(),
        };
        let count: usize =
            db.query_row("SELECT COUNT(*) FROM managed_accounts", [], |r| r.get(0))?;
        if count > LIMIT {
            return Err(Error::Capacity);
        }
        if count != 0 {
            registry.key(db)?;
        }
        let mut query = db.prepare("SELECT id,namespace_identity,seat_id FROM managed_accounts WHERE state='active' ORDER BY ordinal")?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, identity, seat) in rows {
            valid_id(&id)?;
            let file = registry.root.dir.open_dir_nofollow(&id)?.into_std_file();
            let binding = Arc::new(Binding {
                root: registry.root.clone(),
                file,
                association: Association {
                    id: id.clone(),
                    generation: 1,
                    seat,
                },
                namespace: serde_json::from_str(&identity)?,
                retired: AtomicBool::new(false),
            });
            binding.check()?;
            let (key, deployment) = registry.key(db)?;
            let (seat, tuple) = keyed_identity(&key, &deployment, &binding.namespace)?;
            let stored_tuple: String = db.query_row(
                "SELECT identity_tuple FROM managed_accounts WHERE id=?1",
                [&id],
                |r| r.get(0),
            )?;
            if seat != binding.association.seat || tuple != stored_tuple {
                return Err(Error::Private);
            }
            registry.bindings.insert(id, binding);
        }
        Ok(registry)
    }
    fn key(&self, db: &Connection) -> Result<([u8; 32], String), Error> {
        let row: Option<(Vec<u8>,String,String)> = db.query_row("SELECT secret,deployment,root_identity FROM account_identity_key WHERE singleton=1", [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (secret, deployment, identity) = row.ok_or(Error::Schema)?;
        if serde_json::from_str::<DirectoryIdentity>(&identity)? != self.root.identity
            || deployment.len() != 32
        {
            return Err(Error::Private);
        }
        Ok((secret.try_into().map_err(|_| Error::Schema)?, deployment))
    }
    fn current(&self, db: &Connection, binding: &Arc<Binding>) -> Result<(), Error> {
        let original = self
            .bindings
            .get(&binding.association.id)
            .ok_or(Error::LocalAuthority)?;
        if !Arc::ptr_eq(original, binding) {
            return Err(Error::LocalAuthority);
        }
        binding.check()?;
        self.key(db)?;
        let actual = account_association(db, &binding.association.id)?;
        if actual != binding.association {
            return Err(Error::LocalAuthority);
        }
        Ok(())
    }
    pub(super) fn check_resource(&self, db: &Connection, resource: &Resource) -> Result<(), Error> {
        if let Some(association) = association(db, resource)? {
            let binding = self
                .bindings
                .get(&association.id)
                .ok_or(Error::LocalAuthority)?;
            self.current(db, binding)?;
        }
        Ok(())
    }
    pub(super) fn check_profile(
        &self,
        db: &Connection,
        profile: Option<&str>,
    ) -> Result<(), Error> {
        let Some(profile) = profile else {
            return Ok(());
        };
        let value: serde_json::Value = serde_json::from_str(profile)?;
        if let Some(value) = value.get("account") {
            let association: Association = serde_json::from_value(value.clone())?;
            let binding = self
                .bindings
                .get(&association.id)
                .ok_or(Error::LocalAuthority)?;
            self.current(db, binding)?;
            if binding.association != association {
                return Err(Error::LocalAuthority);
            }
        }
        Ok(())
    }
}
fn keyed_identity(
    key: &[u8; 32],
    deployment: &str,
    namespace: &DirectoryIdentity,
) -> Result<(String, String), Error> {
    let tuple = canonical::encode(
        &serde_json::json!({"version":"native-namespace-v1","deployment":deployment,"namespace":namespace,"source":"codex_default_namespace"}),
    )?;
    let mut hmac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| Error::Schema)?;
    hmac.update(tuple.as_bytes());
    Ok((
        format!("seat_native_{}", hex(&hmac.finalize().into_bytes())),
        tuple,
    ))
}
fn account_association(db: &Connection, id: &str) -> Result<Association, Error> {
    db.query_row(
        "SELECT id,generation,seat_id FROM managed_accounts WHERE id=?1 AND state='active'",
        [id],
        |r| {
            Ok(Association {
                id: r.get(0)?,
                generation: r.get(1)?,
                seat: r.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::LocalAuthority)
}
pub(super) fn association(
    db: &Connection,
    resource: &Resource,
) -> Result<Option<Association>, Error> {
    let mapped: Option<(String, u64)> = db
        .query_row(
            "SELECT account_id,binding_generation FROM resource_accounts WHERE preset_id=?1",
            [&resource.preset_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match mapped {
        Some((id, generation)) => {
            let association = account_association(db, &id)?;
            if association.generation != generation
                || association.seat != resource.seat_id
                || resource.framework != "codex"
                || resource.provider.as_deref().is_some_and(|p| p != "openai")
            {
                return Err(Error::Unqualified);
            }
            Ok(Some(association))
        }
        None => {
            let managed: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_accounts WHERE seat_id=?1)",
                [&resource.seat_id],
                |r| r.get(0),
            )?;
            if managed || resource.seat_id.starts_with("seat_native_") {
                return Err(Error::Unqualified);
            }
            Ok(None)
        }
    }
}
pub(super) fn copy_association(
    db: &Connection,
    source: &Resource,
    target: &Resource,
) -> Result<(), Error> {
    if let Some(account) = association(db, source)? {
        db.execute("INSERT INTO resource_accounts(preset_id,account_id,binding_generation) VALUES(?1,?2,?3)", params![target.preset_id,account.id,account.generation])?;
    }
    Ok(())
}
fn preparation_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        Err(Error::OutcomeUnknown)
    } else {
        Ok(())
    }
}
fn sync_directory(dir: &Dir) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use cap_fs_ext::{
            FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt, OpenOptionsSyncExt,
        };
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .maybe_dir(true)
            .nonblock(true);
        let file = dir.open_with(".", &options)?.into_std();
        private::check_handle(&file)?;
        if !same_directory(&file, &dir.try_clone()?.into_std_file())? {
            return Err(Error::Private);
        }
        file.sync_all()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        private::WindowsDirectorySync::open(dir)?
            .ok_or(Error::PlatformUnavailable)?
            .sync()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        Err(Error::PlatformUnavailable)
    }
}
impl DomainRepository {
    pub fn account_choices(&self) -> Result<Vec<AccountChoice>, Error> {
        let mut query = self
            .db
            .prepare("SELECT id,ordinal,state FROM managed_accounts ORDER BY ordinal LIMIT 17")?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u8>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > LIMIT {
            return Err(Error::Capacity);
        }
        rows.into_iter()
            .map(|(id, ordinal, state)| {
                AccountChoice::new(
                    id,
                    ordinal,
                    serde_json::from_value(serde_json::Value::String(state))?,
                )
            })
            .collect()
    }
    pub fn managed_account(&self, id: &str) -> Result<ManagedAccount, Error> {
        valid_id(id)?;
        let binding = self
            .accounts
            .bindings
            .get(id)
            .ok_or(Error::LocalAuthority)?;
        self.accounts.current(&self.db, binding)?;
        Ok(ManagedAccount {
            binding: binding.clone(),
        })
    }
    /// Reserve the original public identity before filesystem effects. Inspection
    /// remains possible if the following one-shot materialization is interrupted.
    pub fn reserve_account(&mut self, profile: &str) -> Result<AccountChoice, Error> {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        if profile != ACCOUNT_PROFILE {
            return Err(Error::Unqualified);
        }
        self.accounts.root.check()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        preparation_deadline(deadline)?;
        let count: usize =
            tx.query_row("SELECT COUNT(*) FROM managed_accounts", [], |r| r.get(0))?;
        if count >= LIMIT {
            return Err(Error::Capacity);
        }
        if count == 0 {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM account_identity_key)",
                [],
                |r| r.get(0),
            )?;
            if !exists {
                let mut secret = [0; 32];
                getrandom::fill(&mut secret).map_err(|_| Error::Unavailable)?;
                tx.execute(
                    "INSERT INTO account_identity_key VALUES(1,?1,?2,?3)",
                    params![
                        secret.as_slice(),
                        random_id("")?,
                        serialize(&self.accounts.root.identity)?
                    ],
                )?;
            }
        }
        self.accounts.key(&tx)?;
        let id = random_id("account_")?;
        tx.execute(
            "INSERT INTO managed_accounts(id,ordinal,generation,state) VALUES(?1,?2,1,'preparing')",
            params![id, count + 1],
        )?;
        preparation_deadline(deadline)?;
        tx.commit().map_err(|_| Error::OutcomeUnknown)?;
        self.accounts.preparations.insert(
            id.clone(),
            Preparation {
                deadline,
                directory: None,
            },
        );
        preparation_deadline(deadline)?;
        AccountChoice::new(id, (count + 1) as u8, AccountState::Preparing)
    }
    /// Exactly one attempt from this original writer. Never adopts an existing
    /// directory or re-arms a preparation recovered after caller/process loss.
    pub fn materialize_account(&mut self, id: &str) -> Result<AccountChoice, Error> {
        valid_id(id)?;
        self.accounts.root.check()?;
        let preparation = self.accounts.preparations.get_mut(id).ok_or(Error::State)?;
        let deadline = preparation.deadline;
        preparation_deadline(deadline)?;
        if preparation.directory.is_some() {
            return Err(Error::State);
        }
        // The durable state consumes this attempt before mkdir, including IO refusal.
        if self.db.execute(
            "UPDATE managed_accounts SET state='uncertain' WHERE id=?1 AND state='preparing'",
            [id],
        )? != 1
        {
            return Err(Error::State);
        }
        preparation_deadline(deadline)?;
        self.accounts.root.check()?;
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            let mut builder = cap_std::fs::DirBuilder::new();
            builder.mode(0o700);
            self.accounts.root.dir.create_dir_with(id, &builder)?;
        }
        #[cfg(windows)]
        private::create_directory_new(&self.accounts.root.path.join(id))?;
        #[cfg(not(any(unix, windows)))]
        return Err(Error::PlatformUnavailable);
        // Retain the exact opened new object in repository-owned custody BEFORE
        // validation or sync can fail. The fixed stable ancestor premise applies.
        preparation.directory = Some(self.accounts.root.dir.open_dir_nofollow(id)?);
        let dir = preparation.directory.as_ref().ok_or(Error::State)?;
        let file = dir.try_clone()?.into_std_file();
        private::check_handle(&file)?;
        let namespace = directory_identity(&file)?;
        preparation_deadline(deadline)?;
        sync_directory(dir)?;
        preparation_deadline(deadline)?;
        sync_directory(&self.accounts.root.dir)?;
        preparation_deadline(deadline)?;
        self.accounts.root.check()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        preparation_deadline(deadline)?;
        let (key, deployment) = self.accounts.key(&tx)?;
        let (seat, tuple) = keyed_identity(&key, &deployment, &namespace)?;
        let collision:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_accounts WHERE seat_id=?1 OR identity_tuple=?2) OR EXISTS(SELECT 1 FROM seats WHERE id=?1)",params![seat,tuple],|r|r.get(0))?;
        if collision {
            return Err(Error::Conflict);
        }
        super::bounded_row(&tx, "seats", "id", &seat, 2048)?;
        tx.execute(
            "INSERT INTO seats(id,config) VALUES(?1,?2)",
            params![
                seat,
                serialize(&Seat {
                    id: seat.clone(),
                    declaration: None
                })?
            ],
        )?;
        if tx.execute("UPDATE managed_accounts SET state='active',namespace_identity=?2,identity_tuple=?3,seat_id=?4 WHERE id=?1 AND state='uncertain'",params![id,serialize(&namespace)?,tuple,seat])? != 1 { return Err(Error::State); }
        let binding = Arc::new(Binding {
            root: self.accounts.root.clone(),
            file,
            association: Association {
                id: id.into(),
                generation: 1,
                seat,
            },
            namespace,
            retired: AtomicBool::new(false),
        });
        binding.check()?;
        // Custody precedes commit and survives a possible-commit error.
        self.accounts.bindings.insert(id.into(), binding);
        preparation_deadline(deadline)?;
        tx.commit().map_err(|_| Error::OutcomeUnknown)?;
        preparation_deadline(deadline)?;
        self.account_choices()?
            .into_iter()
            .find(|choice| choice.id == id)
            .ok_or(Error::Schema)
    }
    pub fn retire_account(&mut self, id: &str) -> Result<AccountChoice, Error> {
        valid_id(id)?;
        if let Some(binding) = self.accounts.bindings.get(id) {
            binding.retired.store(true, Ordering::Release);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE managed_accounts SET state='retired' WHERE id=?1 AND state='active'",
            [id],
        )? != 1
        {
            return Err(Error::State);
        }
        tx.execute("UPDATE resources SET config=json_set(config,'$.published',json('false')) WHERE preset_id IN (SELECT preset_id FROM resource_accounts WHERE account_id=?1)",[id])?;
        tx.commit().map_err(|_| Error::OutcomeUnknown)?;
        self.account_choices()?
            .into_iter()
            .find(|choice| choice.id == id)
            .ok_or(Error::Schema)
    }
}
/// Separate trusted-host enrollment authority. Existing configuration/publication
/// grants cannot construct this consuming command.
pub struct AccountEnrollmentAccess(ResourcePublicationAccess);
pub struct AccountEnrollmentCommand {
    gate: ResourcePublicationCommand,
    binding: Arc<Binding>,
    preset: String,
    model: String,
    reasoning: Option<String>,
    ceiling: Option<Ceiling>,
}
impl AccountEnrollmentAccess {
    pub fn new(expires: Instant, retirement: ResourcePublicationRetirement) -> Self {
        Self(ResourcePublicationAccess::new(expires, retirement))
    }
    pub fn revoke(&self) -> Result<(), Error> {
        self.0.revoke()
    }
    pub fn prepare(
        &self,
        account: &ManagedAccount,
        revision: String,
        model: String,
        reasoning: Option<String>,
        ceiling: Option<Ceiling>,
        deadline: Instant,
    ) -> Result<AccountEnrollmentCommand, Error> {
        if model.len() > 256 || reasoning.as_ref().is_some_and(|s| s.len() > 128) {
            return Err(Error::Unqualified);
        }
        let gate = self
            .0
            .prepare(account.id().into(), revision, false, deadline)?;
        Ok(AccountEnrollmentCommand {
            gate,
            binding: account.binding.clone(),
            preset: random_id("preset_")?,
            model,
            reasoning,
            ceiling,
        })
    }
}
impl AccountEnrollmentCommand {
    pub(crate) fn weight(&self) -> u32 {
        2048
    }
}
impl DomainRepository {
    pub fn enroll_account_resource(
        &mut self,
        command: AccountEnrollmentCommand,
    ) -> Result<ResourceConfigurationResult, Error> {
        self.enroll_account_resource_clock(command, Instant::now)
    }
    fn enroll_account_resource_clock(
        &mut self,
        command: AccountEnrollmentCommand,
        mut clock: impl FnMut() -> Instant,
    ) -> Result<ResourceConfigurationResult, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let gate = &command.gate;
        let revoked = gate
            .access
            .revoked
            .try_lock()
            .map_err(super::resource_publication::lock_error)?;
        let check = |now| gate.access.check_at(*revoked, gate.deadline, now);
        check(clock())?;
        self.accounts.current(&tx, &command.binding)?;
        let ordinal = tx.query_row(
            "SELECT ordinal FROM managed_accounts WHERE id=?1",
            [&gate.resource],
            |r| r.get(0),
        )?;
        if AccountChoice::new(gate.resource.clone(), ordinal, AccountState::Active)?.revision
            != gate.revision
        {
            return Err(Error::Conflict);
        }
        let resource = Resource {
            preset_id: command.preset,
            seat_id: command.binding.association.seat.clone(),
            framework: "codex".into(),
            provider: None,
            model: command.model,
            reasoning: command.reasoning,
            ceiling: command.ceiling,
            roles: Vec::new(),
            published: true,
        };
        if !qualification::configuration_choices(&resource.profile())?
            .iter()
            .any(|c| c.model == resource.model && c.reasoning == resource.reasoning)
        {
            return Err(Error::Unqualified);
        }
        tx.execute("INSERT INTO resource_accounts(preset_id,account_id,binding_generation) VALUES(?1,?2,1)",params![resource.preset_id,gate.resource])?;
        let resource = super::prepare_resource_write(&tx, &resource, Some(true), true)?;
        let result = ResourceConfigurationResult {
            resource_id: resource.id(),
            revision: resource_publication_revision(&resource)?,
            published: true,
        };
        check(clock())?;
        self.accounts.current(&tx, &command.binding)?;
        super::write_resource_configuration(&tx, &resource, true)?;
        check(clock())?;
        self.accounts.current(&tx, &command.binding)?;
        tx.commit().map_err(|_| Error::OutcomeUnknown)?;
        check(clock())
            .and_then(|()| self.accounts.current(&self.db, &command.binding))
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(result)
    }
}

#[cfg(test)]
use crate::domain_worker::clock_fixtures as account_test_common;
#[cfg(test)]
mod tests {
    use super::*;
    use hagency_core::tasks::{DispatchInput, ResourceLease, SessionBinding};
    use std::time::Duration;
    #[test]
    fn native_account_identity() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/account-identity.json"))
                .unwrap();
        for vector in vectors["vectors"].as_array().unwrap() {
            let key: [u8; 32] = serde_json::from_value(vector["key"].clone()).unwrap();
            let namespace = serde_json::from_value(vector["namespace"].clone()).unwrap();
            let (seat, tuple) =
                keyed_identity(&key, vector["deployment"].as_str().unwrap(), &namespace).unwrap();
            assert_eq!(seat, vector["seat"].as_str().unwrap());
            assert_eq!(tuple, vector["canonical"].as_str().unwrap());
        }
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let expired = db.reserve_account(ACCOUNT_PROFILE).unwrap();
        db.accounts
            .preparations
            .get_mut(&expired.id)
            .unwrap()
            .deadline = Instant::now();
        assert!(matches!(
            db.materialize_account(&expired.id),
            Err(Error::OutcomeUnknown)
        ));
        assert!(!state.join(&expired.id).exists());
        let failed = db.reserve_account(ACCOUNT_PROFILE).unwrap();
        db.db.execute_batch("CREATE TEMP TRIGGER account_activation_failure BEFORE UPDATE OF state ON managed_accounts WHEN NEW.state='active' BEGIN SELECT RAISE(ABORT,'fixture activation failure'); END;").unwrap();
        assert!(db.materialize_account(&failed.id).is_err());
        let original = db.accounts.preparations[&failed.id]
            .directory
            .as_ref()
            .unwrap()
            .try_clone()
            .unwrap()
            .into_std_file();
        assert!(
            same_directory(
                &original,
                &open_root(&state.join(&failed.id).canonicalize().unwrap())
                    .unwrap()
                    .into_std_file()
            )
            .unwrap()
        );
        assert!(db.materialize_account(&failed.id).is_err());
        db.db
            .execute_batch("DROP TRIGGER account_activation_failure")
            .unwrap();
        let a = db.reserve_account(ACCOUNT_PROFILE).unwrap();
        let a = db.materialize_account(&a.id).unwrap();
        let b = db.reserve_account(ACCOUNT_PROFILE).unwrap();
        let b = db.materialize_account(&b.id).unwrap();
        let one = &db.accounts.bindings[&a.id];
        let two = &db.accounts.bindings[&b.id];
        assert!(!same_directory(&one.file, &two.file).unwrap());
        assert_eq!(
            directory_identity(&one.file).unwrap(),
            directory_identity(
                &one.root
                    .dir
                    .open_dir_nofollow(&a.id)
                    .unwrap()
                    .into_std_file()
            )
            .unwrap()
        );
        assert_ne!(one.association.seat, two.association.seat);
        let safe = serde_json::to_string(&db.account_choices().unwrap()).unwrap();
        for secret in [
            state.to_str().unwrap(),
            one.association.seat.as_str(),
            "namespace_identity",
            "identity_tuple",
            "secret",
        ] {
            assert!(!safe.contains(secret));
        }
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(state.join(&a.id), &alias).unwrap();
            assert_eq!(
                directory_identity(&one.file).unwrap(),
                directory_identity(&File::open(&alias).unwrap()).unwrap()
            );
            assert!(open_root(&alias).is_err());
        }
    }
    fn make_resource(db: &mut DomainRepository) -> (ManagedAccount, Resource) {
        let choice = db.reserve_account(ACCOUNT_PROFILE).unwrap();
        let choice = db.materialize_account(&choice.id).unwrap();
        let account = db.managed_account(&choice.id).unwrap();
        let access = AccountEnrollmentAccess::new(
            Instant::now() + Duration::from_secs(30),
            Default::default(),
        );
        let command = access
            .prepare(
                &account,
                choice.revision,
                "gpt-5.6-sol".into(),
                Some("medium".into()),
                Some(
                    serde_json::from_value(serde_json::json!({"tokens":1000,"period":"monthly"}))
                        .unwrap(),
                ),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let result = db.enroll_account_resource(command).unwrap();
        let resource = db.resource_configuration(&result.resource_id).unwrap();
        (account, resource)
    }
    #[test]
    fn native_account_owned_current() {
        use account_test_common::*;
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let (account, resource) = make_resource(&mut db);
        db.register(&registration()).unwrap();
        let proof = proof(&request("account-owned", "Worker", &resource, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approval", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &crate::EffectOutcome::Applied {
                receipt: "offline".into(),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: "session".into(),
            engagement_id: e.id,
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .unwrap();
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "managed task", 1000)
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: serde_json::json!({"instruction":"offline"}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("host", 1000, 10000, 10000, 1)
            .unwrap()
            .unwrap();
        let scope = db.owned_dispatch_scope(&cap, 1001).unwrap();
        assert!(scope.requires_managed_account());
        db.start_owned_dispatch(&cap, scope.fingerprint(), 1001)
            .unwrap();
        let observer = Connection::open(state.join("domain.sqlite3")).unwrap();
        observer.busy_timeout(Duration::ZERO).unwrap();
        let result = db.check_owned_clock(&cap, scope.fingerprint(), || {
            let error = observer.execute_batch("BEGIN IMMEDIATE").unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            account.retire(); // Actual original fence becomes current after SQLite admission.
            Ok(1002)
        });
        assert!(matches!(result, Err(Error::LocalAuthority)));
        assert!(account.prepare_launch(&scope).is_err());
        let leased: u64 = observer
            .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r.get(0))
            .unwrap();
        assert_eq!(leased, 1);
        // Original enrollment clock checks both its actual gate and actual SQLite
        // transaction, including retirement after a possible commit.
        for retire_at in [0, 1, 2, 3] {
            let (binding, _) = make_resource(&mut db);
            let choice = db
                .account_choices()
                .unwrap()
                .into_iter()
                .find(|c| c.id == binding.id())
                .unwrap();
            let access = AccountEnrollmentAccess::new(
                Instant::now() + Duration::from_secs(30),
                Default::default(),
            );
            let command = access
                .prepare(
                    &binding,
                    choice.revision,
                    "gpt-5.6-sol".into(),
                    Some("medium".into()),
                    None,
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            let gate = command.gate.access.clone();
            let mut sampled = 0;
            let before: u64 = observer
                .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
                .unwrap();
            let result = db.enroll_account_resource_clock(command, || {
                assert!(matches!(
                    gate.revoked.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                if sampled < 3 {
                    assert_eq!(
                        observer
                            .execute_batch("BEGIN IMMEDIATE")
                            .unwrap_err()
                            .sqlite_error_code(),
                        Some(rusqlite::ErrorCode::DatabaseBusy)
                    );
                } else {
                    observer.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
                }
                if sampled == retire_at {
                    binding.retire();
                }
                sampled += 1;
                Instant::now()
            });
            assert!(result.is_err());
            let after: u64 = observer
                .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
                .unwrap();
            assert_eq!(after, before + u64::from(retire_at == 3));
            if retire_at == 3 {
                assert!(matches!(result, Err(Error::OutcomeUnknown)));
            }
        }
    }
}
