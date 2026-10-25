//! A bounded, synchronous SQLite pool. Connections are exclusively owned while checked out.
use rusqlite::{Connection, Result};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct DbPool(Arc<Inner>);

struct Inner {
    source: String,
    memory: bool,
    state: Mutex<State>,
    available: Condvar,
}

struct State {
    // None is a free slot whose connection needs reopening.
    idle: Vec<Option<Connection>>,
    // Keep an in-memory database alive even if all checked-out connections break.
    _anchor: Option<Connection>,
}

impl DbPool {
    pub fn open(path: &str, size: usize) -> Result<Self> {
        Self::new(path.to_owned(), false, size)
    }

    pub fn memory(size: usize) -> Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self::new(
            format!("file:bloglite-{id}?mode=memory&cache=shared"),
            true,
            size,
        )
    }

    fn new(source: String, memory: bool, size: usize) -> Result<Self> {
        assert!(size > 0, "A database pool needs at least one connection");
        let anchor = if memory {
            Some(Connection::open(&source)?)
        } else {
            None
        };
        let pool = Self(Arc::new(Inner {
            source,
            memory,
            state: Mutex::new(State {
                idle: Vec::with_capacity(size),
                _anchor: anchor,
            }),
            available: Condvar::new(),
        }));
        for _ in 0..size {
            let conn = pool.0.connect()?;
            pool.0.state.lock().unwrap().idle.push(Some(conn));
        }
        Ok(pool)
    }

    /// Wait up to 30 seconds, matching the previous pool's checkout timeout.
    pub fn get(&self) -> Result<PooledConnection> {
        self.get_timeout(Duration::from_secs(30))
    }

    pub fn get_timeout(&self, timeout: Duration) -> Result<PooledConnection> {
        let start = Instant::now();
        let mut state = self.0.state.lock().unwrap();
        loop {
            if let Some(slot) = state.idle.pop() {
                drop(state);
                let mut guard = PooledConnection {
                    pool: self.clone(),
                    conn: slot,
                };
                if guard.conn.is_none() {
                    guard.conn = Some(self.0.connect()?);
                }
                return Ok(guard);
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("Timed out waiting for a database connection".to_owned()),
                ));
            }
            state = self.0.available.wait_timeout(state, remaining).unwrap().0;
        }
    }
}

impl Inner {
    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.source)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        if !self.memory {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        Ok(conn)
    }
}

/// Returns its connection to the pool when dropped, including during unwinding.
pub struct PooledConnection {
    pool: DbPool,
    conn: Option<Connection>,
}

impl Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().unwrap()
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        let conn = self
            .conn
            .take()
            .filter(|conn| conn.is_autocommit() || conn.execute_batch("ROLLBACK").is_ok());
        self.pool.0.state.lock().unwrap().idle.push(conn);
        self.pool.0.available.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_checkouts_and_reuses_connections() {
        let pool = DbPool::memory(2).unwrap();
        let first = pool.get().unwrap();
        first
            .execute_batch("CREATE TEMP TABLE reused (id INTEGER)")
            .unwrap();
        let second = pool.get().unwrap();
        assert!(pool.get_timeout(Duration::from_millis(10)).is_err());
        drop(first);
        pool.get()
            .unwrap()
            .execute("INSERT INTO reused VALUES (1)", [])
            .unwrap();
        drop(second);
    }

    #[test]
    fn shares_memory_within_pool_and_isolates_other_pools() {
        let pool = DbPool::memory(2).unwrap();
        let first = pool.get().unwrap();
        first
            .execute_batch("CREATE TABLE shared (id); INSERT INTO shared VALUES (42)")
            .unwrap();
        let second = pool.get().unwrap();
        assert_eq!(
            second
                .query_row("SELECT id FROM shared", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            42
        );
        let other = DbPool::memory(1).unwrap();
        assert!(
            other
                .get()
                .unwrap()
                .prepare("SELECT * FROM shared")
                .is_err()
        );
    }

    #[test]
    fn returns_connection_after_unwind_and_rolls_back() {
        let pool = DbPool::memory(1).unwrap();
        pool.get()
            .unwrap()
            .execute_batch("CREATE TABLE example (id)")
            .unwrap();
        let worker_pool = pool.clone();
        assert!(
            std::thread::spawn(move || {
                let conn = worker_pool.get().unwrap();
                conn.execute_batch("BEGIN; INSERT INTO example VALUES (1)")
                    .unwrap();
                panic!("Simulate a request panicking");
            })
            .join()
            .is_err()
        );
        let conn = pool.get_timeout(Duration::ZERO).unwrap();
        assert!(conn.is_autocommit());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM example", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn supports_concurrent_checkouts_and_wakes_waiters() {
        let pool = DbPool::memory(4).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(5));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let pool = pool.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let conn = pool.get().unwrap();
                    barrier.wait();
                    barrier.wait();
                    drop(conn);
                });
            }
            barrier.wait();
            assert!(pool.get_timeout(Duration::ZERO).is_err());
            let waiter = scope.spawn(|| pool.get_timeout(Duration::from_secs(2)).unwrap());
            barrier.wait();
            drop(waiter.join().unwrap());
        });
    }

    #[test]
    fn configures_every_file_connection_and_preserves_data() {
        let path =
            std::env::temp_dir().join(format!("bloglite-pool-{}.sqlite", std::process::id()));
        let pool = DbPool::open(path.to_str().unwrap(), 4).unwrap();
        let connections: Vec<_> = (0..4).map(|_| pool.get().unwrap()).collect();
        for conn in &connections {
            assert_eq!(
                conn.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "wal"
            );
            assert_eq!(
                conn.query_row("PRAGMA synchronous", [], |r| r.get::<_, i32>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i32>(0))
                    .unwrap(),
                5000
            );
        }
        connections[0]
            .execute_batch("CREATE TABLE persisted (id); INSERT INTO persisted VALUES (7)")
            .unwrap();
        drop(connections);
        drop(pool);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("SELECT id FROM persisted", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            7
        );
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }
}
