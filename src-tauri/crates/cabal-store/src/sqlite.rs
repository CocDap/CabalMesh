//! One local database per app-data directory. JSON files are import sources only.

use crate::StoreError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, PoisonError, Weak},
    time::Duration,
};

pub const DATABASE_FILE: &str = "cabalmesh.sqlite3";
type SharedConnection = Arc<Mutex<Connection>>;
static CONNECTIONS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<Connection>>>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Database {
    path: PathBuf,
    connection: SharedConnection,
}

pub struct IntentRow {
    pub id: String,
    pub network: String,
    pub created_ms: i64,
    pub status: String,
    pub action: String,
    pub asset: String,
    pub amount_raw: String,
    pub decimals: u8,
    pub payload: String,
}

pub struct TokenRow {
    pub contract: String,
    pub symbol: String,
    pub decimals: u8,
    pub wallet: String,
    pub in_intents: String,
}

pub struct SnapshotRow {
    pub account: String,
    pub network: String,
    pub network_label: String,
    pub native: Option<String>,
    pub native_available: Option<String>,
    pub wrapper: Option<String>,
    pub updated_ms: i64,
    pub payload: String,
    pub tokens: Vec<TokenRow>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| StoreError::Read {
                    path: path.to_path_buf(),
                    source,
                })?
                .join(path)
        };
        let mut cache = CONNECTIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(connection) = cache.get(&path).and_then(Weak::upgrade) {
            return Ok(Self { path, connection });
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Write {
                path: path.clone(),
                source,
            })?;
        }
        let connection = Connection::open(&path).map_err(|source| StoreError::Database {
            path: path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|source| {
                StoreError::Write {
                    path: path.clone(),
                    source,
                }
            })?;
        }
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|source| StoreError::Database {
                path: path.clone(),
                source,
            })?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
            )
            .map_err(|source| StoreError::Database {
                path: path.clone(),
                source,
            })?;
        let database = Self {
            path: path.clone(),
            connection: Arc::new(Mutex::new(connection)),
        };
        database.with(|connection| {
            let version: u32 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version > 1 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            if version == 0 {
                let tx = connection.transaction()?;
                tx.execute_batch(include_str!("schema.sql"))?;
                tx.pragma_update(None, "user_version", 1)?;
                tx.commit()?;
            }
            Ok(())
        })?;
        cache.retain(|_, connection| connection.strong_count() > 0);
        cache.insert(path, Arc::downgrade(&database.connection));
        Ok(database)
    }

    pub fn beside(legacy: &Path) -> Result<Self, StoreError> {
        Self::open(legacy.with_file_name(DATABASE_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
    ) -> Result<T, StoreError> {
        operation(
            &mut self
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
        .map_err(|source| StoreError::Database {
            path: self.path.clone(),
            source,
        })
    }

    pub fn migrated(&self, name: &str) -> Result<bool, StoreError> {
        self.with(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name=?1)",
                [name],
                |row| row.get(0),
            )
        })
    }

    pub fn intent_payloads(&self) -> Result<Vec<String>, StoreError> {
        self.with(|connection| {
            let mut query = connection
                .prepare("SELECT payload_json FROM intents ORDER BY created_ms DESC, rowid DESC")?;
            let rows = query.query_map([], |row| row.get(0))?;
            rows.collect()
        })
    }

    pub fn import_intents(&self, name: &str, rows: &[IntentRow]) -> Result<(), StoreError> {
        self.with(|connection| {
            let tx = connection.transaction()?;
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name=?1)",
                [name],
                |row| row.get::<_, bool>(0),
            )? {
                for row in rows.iter().rev() {
                    insert_intent(&tx, row, false)?;
                }
                tx.execute("INSERT INTO legacy_imports(name) VALUES(?1)", [name])?;
            }
            tx.commit()
        })
    }

    pub fn save_intent(&self, row: &IntentRow, create: bool) -> Result<(), StoreError> {
        self.with(|connection| {
            insert_intent(connection, row, !create)?;
            Ok(())
        })
    }

    pub fn snapshot(&self, account: &str, network: &str) -> Result<Option<String>, StoreError> {
        self.with(|connection| {
            connection
                .query_row(
                    "SELECT payload_json FROM account_snapshots WHERE account_id=?1 AND network=?2",
                    params![account, network],
                    |row| row.get(0),
                )
                .optional()
        })
    }

    /// Replace the complete account snapshot and all token rows atomically.
    pub fn save_snapshot(&self, snapshot: &SnapshotRow) -> Result<(), StoreError> {
        self.with(|connection| save_snapshot(connection, snapshot))
    }

    pub fn import_snapshot(
        &self,
        name: &str,
        snapshot: Option<&SnapshotRow>,
    ) -> Result<(), StoreError> {
        self.with(|connection| {
            let tx = connection.transaction()?;
            if !tx.query_row("SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name=?1)", [name], |row| row.get::<_, bool>(0))? {
                if let Some(snapshot) = snapshot {
                    let exists = tx.query_row("SELECT EXISTS(SELECT 1 FROM account_snapshots WHERE account_id=?1 AND network=?2)", params![snapshot.account, snapshot.network], |row| row.get::<_, bool>(0))?;
                    if !exists { write_snapshot(&tx, snapshot)?; }
                }
                tx.execute("INSERT INTO legacy_imports(name) VALUES(?1)", [name])?;
            }
            tx.commit()
        })
    }
}

fn insert_intent(connection: &Connection, row: &IntentRow, update: bool) -> rusqlite::Result<()> {
    let query = if update {
        "UPDATE intents SET network=?2,created_ms=?3,status=?4,action=?5,asset=?6,amount_raw=?7,decimals=?8,payload_json=?9 WHERE id=?1 AND payload_json<>?9"
    } else {
        "INSERT INTO intents(id,network,created_ms,status,action,asset,amount_raw,decimals,payload_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"
    };
    let changed = connection.execute(
        query,
        params![
            row.id,
            row.network,
            row.created_ms,
            row.status,
            row.action,
            row.asset,
            row.amount_raw,
            row.decimals,
            row.payload
        ],
    )?;
    if changed != 1
        && !(update
            && connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM intents WHERE id=?1)",
                [&row.id],
                |row| row.get::<_, bool>(0),
            )?)
    {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

fn save_snapshot(connection: &mut Connection, snapshot: &SnapshotRow) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    write_snapshot(&tx, snapshot)?;
    tx.commit()
}

fn write_snapshot(connection: &Connection, snapshot: &SnapshotRow) -> rusqlite::Result<()> {
    connection.execute("INSERT INTO networks(id,label) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET label=excluded.label", params![snapshot.network,snapshot.network_label])?;
    connection.execute("INSERT INTO account_snapshots(account_id,network,native_wallet_raw,native_available_raw,wrapper_contract,updated_ms,payload_json)
        VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(account_id,network) DO UPDATE SET native_wallet_raw=excluded.native_wallet_raw,
        native_available_raw=excluded.native_available_raw,wrapper_contract=excluded.wrapper_contract,updated_ms=excluded.updated_ms,payload_json=excluded.payload_json",
        params![snapshot.account,snapshot.network,snapshot.native,snapshot.native_available,snapshot.wrapper,snapshot.updated_ms,snapshot.payload])?;
    connection.execute(
        "DELETE FROM account_tokens WHERE account_id=?1 AND network=?2",
        params![snapshot.account, snapshot.network],
    )?;
    for token in &snapshot.tokens {
        connection.execute("INSERT INTO assets(network,contract_id,symbol,decimals) VALUES(?1,?2,?3,?4) ON CONFLICT(network,contract_id) DO UPDATE SET symbol=excluded.symbol,decimals=excluded.decimals", params![snapshot.network,token.contract,token.symbol,token.decimals])?;
        connection.execute("INSERT INTO account_tokens(account_id,network,contract_id,symbol,decimals,wallet_raw,in_intents_raw) VALUES(?1,?2,?3,?4,?5,?6,?7)
            ON CONFLICT(account_id,network,contract_id) DO UPDATE SET symbol=excluded.symbol,decimals=excluded.decimals,wallet_raw=excluded.wallet_raw,in_intents_raw=excluded.in_intents_raw",
            params![snapshot.account,snapshot.network,token.contract,token.symbol,token.decimals,token.wallet,token.in_intents])?;
    }
    Ok(())
}

/// Small local documents share SQLite while keeping their typed serialization.
/// `path` identifies the old JSON file, which is never deleted or rewritten.
#[derive(Clone, Debug)]
pub struct DocumentStore {
    path: PathBuf,
}

impl DocumentStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn compact(path: impl Into<PathBuf>) -> Self {
        Self::new(path)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn database(&self) -> Result<Database, StoreError> {
        Database::beside(&self.path)
    }
    fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    pub fn load<T: DeserializeOwned>(&self) -> Result<T, StoreError> {
        let database = self.database()?;
        let name = self.name();
        let saved: Option<String> = database.with(|connection| {
            connection
                .query_row(
                    "SELECT payload_json FROM documents WHERE name=?1",
                    [&name],
                    |row| row.get(0),
                )
                .optional()
        })?;
        if let Some(saved) = saved {
            return serde_json::from_str(&saved).map_err(|source| StoreError::Decode {
                path: database.path.clone(),
                source,
            });
        }
        if database.migrated(&format!("document:{name}"))? {
            return Err(StoreError::Read {
                path: self.path.clone(),
                source: std::io::ErrorKind::NotFound.into(),
            });
        }
        let raw = fs::read_to_string(&self.path).map_err(|source| StoreError::Read {
            path: self.path.clone(),
            source,
        })?;
        let _: T = serde_json::from_str(&raw).map_err(|source| StoreError::Decode {
            path: self.path.clone(),
            source,
        })?;
        database.with(|connection| {
            let tx = connection.transaction()?;
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name=?1)",
                [format!("document:{name}")],
                |row| row.get::<_, bool>(0),
            )? {
                tx.execute(
                    "INSERT OR IGNORE INTO documents(name,payload_json) VALUES(?1,?2)",
                    params![name, raw],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO legacy_imports(name) VALUES(?1)",
                    [format!("document:{name}")],
                )?;
            }
            tx.commit()
        })?;
        // Another writer may have saved while the legacy file was being read.
        database
            .with(|connection| {
                connection.query_row(
                    "SELECT payload_json FROM documents WHERE name=?1",
                    [&name],
                    |row| row.get::<_, String>(0),
                )
            })
            .and_then(|raw| {
                serde_json::from_str(&raw).map_err(|source| StoreError::Decode {
                    path: database.path.clone(),
                    source,
                })
            })
    }

    pub fn load_optional<T: DeserializeOwned>(&self) -> Result<Option<T>, StoreError> {
        match self.load() {
            Ok(value) => Ok(Some(value)),
            Err(StoreError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn load_or<T: DeserializeOwned>(&self, default: T) -> T {
        match self.load_optional() {
            Ok(Some(value)) => value,
            Ok(None) => default,
            Err(error) => {
                tracing::warn!(%error,"could not read local document");
                default
            }
        }
    }

    pub fn save<T: Serialize>(&self, value: &T) -> Result<(), StoreError> {
        let raw = serde_json::to_string(value).map_err(|source| StoreError::Encode {
            path: self.path.clone(),
            source,
        })?;
        let name = self.name();
        self.database()?.with(|connection| {
            let tx = connection.transaction()?;
            tx.execute("INSERT INTO documents(name,payload_json) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET payload_json=excluded.payload_json", params![name,raw])?;
            tx.execute("INSERT OR IGNORE INTO legacy_imports(name) VALUES(?1)", [format!("document:{name}")])?;
            tx.commit()
        })
    }

    pub fn delete(&self) -> Result<(), StoreError> {
        let name = self.name();
        self.database()?.with(|connection| {
            let tx = connection.transaction()?;
            tx.execute("DELETE FROM documents WHERE name=?1", [&name])?;
            tx.execute(
                "INSERT OR IGNORE INTO legacy_imports(name) VALUES(?1)",
                [format!("document:{name}")],
            )?;
            tx.commit()
        })
    }

    pub fn exists(&self) -> bool {
        self.load::<serde::de::IgnoredAny>().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(account: &str, network: &str, amount: &str) -> SnapshotRow {
        SnapshotRow {
            account: account.into(),
            network: network.into(),
            network_label: format!("NEAR {network}"),
            native: Some(amount.into()),
            native_available: Some("0".into()),
            wrapper: Some("wrap.testnet".into()),
            updated_ms: 100,
            payload: format!("snapshot:{account}:{network}:{amount}"),
            tokens: vec![TokenRow {
                contract: "wrap.testnet".into(),
                symbol: "wNEAR".into(),
                decimals: 24,
                wallet: amount.into(),
                in_intents: "0".into(),
            }],
        }
    }

    #[test]
    fn documents_import_once_and_never_rewrite_the_legacy_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "41").unwrap();
        let store = DocumentStore::new(&path);
        assert_eq!(store.load::<u64>().unwrap(), 41);
        store.save(&42u64).unwrap();
        assert_eq!(store.load::<u64>().unwrap(), 42);
        assert_eq!(fs::read_to_string(&path).unwrap(), "41");
        store.delete().unwrap();
        assert!(store.load_optional::<u64>().unwrap().is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "41");
        let header = fs::read(dir.path().join(DATABASE_FILE)).unwrap();
        assert_eq!(&header[..16], b"SQLite format 3\0");
    }

    #[test]
    fn corrupt_legacy_documents_can_be_repaired_without_a_false_import_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "not JSON").unwrap();
        let store = DocumentStore::new(&path);
        assert!(store.load::<u64>().is_err());
        assert!(!store
            .database()
            .unwrap()
            .migrated("document:settings.json")
            .unwrap());
        fs::write(&path, "10").unwrap();
        assert_eq!(store.load::<u64>().unwrap(), 10);
    }

    #[test]
    fn large_integer_documents_and_raw_token_amounts_keep_every_digit() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocumentStore::new(dir.path().join("large.json"));
        store.save(&u128::MAX).unwrap();
        assert_eq!(store.load::<u128>().unwrap(), u128::MAX);
        let database = store.database().unwrap();
        database
            .save_snapshot(&snapshot(
                "alice.testnet",
                "testnet",
                &u128::MAX.to_string(),
            ))
            .unwrap();
        let (amount, storage): (String, String) = database
            .with(|connection| {
                connection.query_row(
                    "SELECT wallet_raw,typeof(wallet_raw) FROM account_tokens",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(amount, u128::MAX.to_string());
        assert_eq!(storage, "text");
    }

    #[test]
    fn account_snapshots_are_separate_by_account_and_network() {
        let dir = tempfile::tempdir().unwrap();
        let database = Database::open(dir.path().join(DATABASE_FILE)).unwrap();
        for (account, network, amount) in [
            ("alice", "testnet", "1"),
            ("bob", "testnet", "2"),
            ("alice", "mainnet", "3"),
        ] {
            let expected = snapshot(account, network, amount);
            database.save_snapshot(&expected).unwrap();
        }
        for (account, network, amount) in [
            ("alice", "testnet", "1"),
            ("bob", "testnet", "2"),
            ("alice", "mainnet", "3"),
        ] {
            assert_eq!(
                database.snapshot(account, network).unwrap().unwrap(),
                snapshot(account, network, amount).payload
            );
        }
        let counts: (u32,u32) = database.with(|connection| connection.query_row(
            "SELECT (SELECT COUNT(*) FROM account_snapshots),(SELECT COUNT(*) FROM networks)",[],|row|Ok((row.get(0)?,row.get(1)?)))).unwrap();
        assert_eq!(counts, (3, 2));
    }

    #[test]
    fn snapshot_and_token_rows_roll_back_together() {
        let dir = tempfile::tempdir().unwrap();
        let database = Database::open(dir.path().join(DATABASE_FILE)).unwrap();
        let old = snapshot("alice", "testnet", "1");
        database.save_snapshot(&old).unwrap();
        database.with(|connection| connection.execute_batch("CREATE TRIGGER reject_token BEFORE INSERT ON account_tokens BEGIN SELECT RAISE(ABORT,'injected disk failure'); END")).unwrap();
        assert!(database
            .save_snapshot(&snapshot("alice", "testnet", "2"))
            .is_err());
        assert_eq!(
            database.snapshot("alice", "testnet").unwrap().unwrap(),
            old.payload
        );
        let amount: String = database
            .with(|connection| {
                connection.query_row("SELECT wallet_raw FROM account_tokens", [], |row| {
                    row.get(0)
                })
            })
            .unwrap();
        assert_eq!(amount, "1");
    }

    #[test]
    fn cache_reads_do_not_write_the_snapshot_or_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let database = Database::open(dir.path().join(DATABASE_FILE)).unwrap();
        database
            .save_snapshot(&snapshot("alice", "testnet", "1"))
            .unwrap();
        let changes = database
            .with(|connection| {
                connection.query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
            })
            .unwrap();
        for _ in 0..5 {
            assert!(database.snapshot("alice", "testnet").unwrap().is_some());
        }
        assert!(database.snapshot("missing", "testnet").unwrap().is_none());
        let after = database
            .with(|connection| {
                connection.query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
            })
            .unwrap();
        assert_eq!(changes, after);
    }

    #[test]
    fn invalid_sqlite_file_is_not_recreated_or_emptied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        fs::write(&path, b"invalid database").unwrap();
        assert!(Database::open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid database");
    }
}
