use hagency::App;
use hagency_store::*;
use salvo::prelude::*;

#[path = "../../../hagency-store/tests/common/mod.rs"]
mod common;
pub use common::*;

pub const TOKEN: &str = "fixture_operator_token_32_bytes_minimum";
pub const BASE: &str = "http://127.0.0.1:13300";
/// Generous but JSON-safe: `Tokens` refuses anything above JSON_SAFE_MAX, so
/// an unrepresentable "infinite" ceiling would panic the fixture at
/// deserialization rather than stage the overrun.
pub const GENEROUS: u64 = 9_000_000_000_000_000;

pub struct Fixture {
    pub service: Service,
    pub domain: DomainStore,
    pub custody: Store,
    pub state: std::path::PathBuf,
    _root: tempfile::TempDir,
}

impl Fixture {
    /// The retained commit-then-lower flow: an engagement made under a
    /// generous ceiling, then the ceiling lowered under it — an overrun
    /// exists precisely because the commitment was admissible when made.
    /// With `two`, a second resource on its own preset is seeded as well.
    pub fn new(two: bool) -> Self {
        Self::with_capacity(two, 16)
    }
    /// E4 needs a saturated writer queue to produce the STORE's own
    /// `Error::Busy` (the mpsc `try_send` refusal), not the SQLite busy
    /// timeout's catch-all: capacity 1 keeps one job queued while the writer
    /// grinds on another.
    pub fn with_capacity(two: bool, capacity: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let seed = |db: &mut DomainRepository, preset: &str, commit: u64, lowered: u64| {
            let generous = resource(preset, &format!("{preset}_seat"), GENEROUS);
            db.put_resource(&generous).unwrap();
            // One live agent name per project: admission refuses a collision.
            let ask = request(
                &format!("{preset}_request"),
                &format!("Worker_{preset}"),
                &generous,
                commit,
            );
            let proof = proof(&ask);
            db.admit(&proof, 1000).unwrap();
            db.approve(&format!("approve_{preset}"), &proof, 1000)
                .unwrap();
            db.put_resource(&resource(preset, &format!("{preset}_seat"), lowered))
                .unwrap();
        };
        seed(&mut db, "alerts_pool_a", 1_500_000, 1_000_000);
        if two {
            seed(&mut db, "alerts_pool_b", 2_500_000, 2_000_000);
        }
        let domain = DomainStore::start(db, capacity).unwrap();
        let app = App::new(
            custody.clone(),
            TOKEN.as_bytes(),
            "127.0.0.1:13300".parse().unwrap(),
        )
        .unwrap()
        .with_domain(domain.clone());
        Self {
            service: Service::new(app.router()),
            domain,
            custody,
            state: state.join("domain.sqlite3"),
            _root: root,
        }
    }
    pub fn url(&self) -> String {
        format!("{BASE}/api/native/v1/alerts")
    }
    pub async fn close(self) {
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
    }
}
