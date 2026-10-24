//! Single-owner SQLite worker. No connection or blocking receive enters a world tick.
use protocol::{Credits, DisplayName, OpaqueId, ProfileView};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    InsufficientFunds,
    Conflict,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Load {
        hash: String,
        new_id: String,
    },
    Name {
        profile: String,
        name: String,
    },
    Economic {
        profile: String,
        event: String,
        delta: i64,
        job: bool,
    },
    FinishRepair {
        event: String,
    },
}
pub struct Completion {
    pub id: String,
    pub command: Command,
    pub result: Result<ProfileView, StoreError>,
}
pub struct SqliteWorker {
    sender: Option<SyncSender<(String, Command)>>,
    receiver: Receiver<Completion>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl SqliteWorker {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create database directory: {e}"))?;
        }
        let lease_path = path.with_file_name(format!(
            "{}.lock",
            path.file_name()
                .ok_or("database filename missing")?
                .to_string_lossy()
        ));
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lease_path)
            .map_err(|e| format!("open database ownership lock: {e}"))?;
        lease
            .try_lock()
            .map_err(|_| "profile database already owned by another server")?;
        let path = path.to_owned();
        let (sender, requests) = mpsc::sync_channel::<(String, Command)>(64);
        let (results, receiver) = mpsc::sync_channel(64);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = thread::Builder::new()
            .name("sqlite-profiles".into())
            .spawn(move || {
                let _lease = lease;
                let mut connection = match open(&path) {
                    Ok(c) => {
                        let _ = ready_tx.send(Ok(()));
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while !signal.load(Ordering::Relaxed) {
                    let (id, command) = match requests.recv_timeout(Duration::from_millis(50)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut result = Err(StoreError::Unavailable);
                    for attempt in 0..3 {
                        match execute(&mut connection, &command) {
                            Ok(view) => {
                                result = Ok(view);
                                break;
                            }
                            Err(DbError::Rejected(e)) => {
                                result = Err(e);
                                break;
                            }
                            Err(DbError::Sql(e)) => {
                                let busy = matches!(
                                    e.sqlite_error_code(),
                                    Some(
                                        rusqlite::ErrorCode::DatabaseBusy
                                            | rusqlite::ErrorCode::DatabaseLocked
                                    )
                                );
                                if !busy || signal.load(Ordering::Relaxed) {
                                    break;
                                }
                                thread::sleep(Duration::from_millis(10 << attempt));
                            }
                        }
                    }
                    let mut completion = Completion {
                        id,
                        command,
                        result,
                    };
                    loop {
                        match results.try_send(completion) {
                            Ok(()) => break,
                            Err(TrySendError::Disconnected(_)) => return,
                            Err(TrySendError::Full(done)) => {
                                if signal.load(Ordering::Relaxed) {
                                    return;
                                }
                                completion = done;
                                thread::sleep(Duration::from_millis(1));
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("start persistence worker: {e}"))?;
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "database startup timed out")??;
        Ok(Self {
            sender: Some(sender),
            receiver,
            stop,
            thread: Some(thread),
        })
    }
    pub fn submit(&self, id: String, command: Command) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("worker stopped")?
            .try_send((id, command))
            .map_err(|e| match e {
                TrySendError::Full(_) => "persistence queue full".into(),
                TrySendError::Disconnected(_) => "persistence worker stopped".into(),
            })
    }
    pub fn poll(&self) -> Vec<Completion> {
        self.receiver.try_iter().take(64).collect()
    }
    pub fn healthy(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}
impl Drop for SqliteWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.sender.take();
        // At most 64 outstanding operations, so a result send cannot fill this queue beyond capacity.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn open(path: &Path) -> Result<Connection, String> {
    let mut c = Connection::open(path).map_err(|e| format!("open profiles database: {e}"))?;
    c.busy_timeout(Duration::from_millis(50))
        .map_err(|e| e.to_string())?;
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .map_err(|e| e.to_string())?;
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if version > 1 {
        return Err("profile database schema is newer than this server".into());
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    if version == 0 {
        tx.execute_batch(include_str!("../migrations/001_profiles.sql"))
            .map_err(|e| format!("profile migration: {e}"))?;
    }
    let migrations: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if migrations != 1 {
        return Err("profile migration record missing".into());
    }
    // A restarted world cannot apply a previous world's repair. Recover its unresolved debit once.
    let unresolved: Vec<(String, String)> = {
        let mut s = tx
            .prepare("SELECT event_id, profile_id FROM repair_pending")
            .map_err(|e| e.to_string())?;
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?
    };
    for (event, profile) in unresolved {
        economic(&tx, &profile, &format!("refund:{event}"), 50, false)
            .map_err(|_| "repair restart compensation failed")?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    // Validate expected tables even on an already-migrated file.
    c.prepare("SELECT p.cookie_secret_hash, e.kind FROM profiles p LEFT JOIN economic_events e ON e.profile_id=p.profile_id").map_err(|e| e.to_string())?;
    Ok(c)
}
enum DbError {
    Sql(rusqlite::Error),
    Rejected(StoreError),
}
impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
fn profile(c: &Connection, id: &str) -> Result<ProfileView, DbError> {
    Ok(c.query_row(
        "SELECT profile_id,display_name,credits,completed_jobs FROM profiles WHERE profile_id=?1",
        [id],
        |r| {
            Ok(ProfileView {
                profile_id: OpaqueId(r.get(0)?),
                display_name: DisplayName(r.get(1)?),
                credits: Credits(r.get(2)?),
                completed_jobs: r.get(3)?,
            })
        },
    )?)
}
fn execute(c: &mut Connection, cmd: &Command) -> Result<ProfileView, DbError> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let view = match cmd {
        Command::Load { hash, new_id } => {
            tx.execute("INSERT INTO profiles(profile_id,cookie_secret_hash,display_name,credits,completed_jobs,updated_at_unix_ms) VALUES(?1,?2,'Local Guest',0,0,?3) ON CONFLICT(cookie_secret_hash) DO NOTHING", params![new_id,hash.as_bytes(),now()])?;
            let id: String = tx.query_row(
                "SELECT profile_id FROM profiles WHERE cookie_secret_hash=?1",
                [hash.as_bytes()],
                |r| r.get(0),
            )?;
            profile(&tx, &id)?
        }
        Command::Name { profile: id, name } => {
            tx.execute(
                "UPDATE profiles SET display_name=?1,updated_at_unix_ms=?2 WHERE profile_id=?3",
                params![name, now(), id],
            )?;
            profile(&tx, id)?
        }
        Command::Economic {
            profile: id,
            event,
            delta,
            job,
        } => economic(&tx, id, event, *delta, *job)?,
        Command::FinishRepair { event } => {
            let id: String = tx.query_row(
                "SELECT profile_id FROM economic_events WHERE event_id=?1 AND kind='repair'",
                [event],
                |r| r.get(0),
            )?;
            tx.execute("DELETE FROM repair_pending WHERE event_id=?1", [event])?;
            profile(&tx, &id)?
        }
    };
    tx.commit()?;
    Ok(view)
}
fn economic(
    c: &Connection,
    id: &str,
    event: &str,
    delta: i64,
    job: bool,
) -> Result<ProfileView, DbError> {
    let kind = if job && delta >= 0 && event.starts_with("mission:") {
        "mission"
    } else if !job && delta == -50 && event.starts_with("repair:") {
        "repair"
    } else if !job && delta == 50 && event.starts_with("refund:repair:") {
        "refund"
    } else {
        return Err(DbError::Rejected(StoreError::Conflict));
    };
    let old: Option<(String, String, i64)> = c
        .query_row(
            "SELECT profile_id,kind,amount FROM economic_events WHERE event_id=?1",
            [event],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some(old) = old {
        if old != (id.into(), kind.into(), delta) {
            return Err(DbError::Rejected(StoreError::Conflict));
        }
        return profile(c, id);
    }
    if kind == "mission" {
        let repairing: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM repair_pending WHERE profile_id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if repairing {
            return Err(DbError::Rejected(StoreError::Unavailable));
        }
    }
    if kind == "refund" {
        let debit = event.strip_prefix("refund:").expect("refund prefix");
        let valid: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM repair_pending WHERE event_id=?1 AND profile_id=?2)",
            params![debit, id],
            |r| r.get(0),
        )?;
        if !valid {
            return Err(DbError::Rejected(StoreError::Conflict));
        }
    }
    let view = profile(c, id)?;
    let balance = i64::from(view.credits.0) + delta;
    if balance < 0 {
        return Err(DbError::Rejected(StoreError::InsufficientFunds));
    }
    if balance > 1_000_000_000 || (job && view.completed_jobs == u32::MAX) {
        return Err(DbError::Rejected(StoreError::Conflict));
    }
    c.execute("INSERT INTO economic_events(event_id,profile_id,kind,amount,created_at_unix_ms) VALUES(?1,?2,?3,?4,?5)", params![event,id,kind,delta,now()])?;
    c.execute("UPDATE profiles SET credits=?1,completed_jobs=completed_jobs+?2,updated_at_unix_ms=?3 WHERE profile_id=?4", params![balance,u32::from(job),now(),id])?;
    if kind == "repair" {
        c.execute(
            "INSERT INTO repair_pending(event_id,profile_id) VALUES(?1,?2)",
            params![event, id],
        )?;
    } else if kind == "refund" {
        c.execute(
            "DELETE FROM repair_pending WHERE event_id=?1",
            [event.strip_prefix("refund:").expect("refund prefix")],
        )?;
    }
    profile(c, id)
}
