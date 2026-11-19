use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use vera20k_relay_protocol::FederationOutcome;

use super::Room;

const MAX_RECEIPTS: i64 = 4096;

fn sqlite_time(seconds: u64) -> rusqlite::Result<i64> {
    i64::try_from(seconds).map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Delivery {
    Accepted,
    Retry,
    Refused,
}

impl Delivery {
    pub(super) fn from_status(status: u16) -> Self {
        match status {
            204 => Self::Accepted,
            408 | 429 | 500..=599 => Self::Retry,
            _ => Self::Refused,
        }
    }
}

pub(super) struct Outbox(Connection);

impl Outbox {
    pub(super) fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        let mut create = OpenOptions::new();
        create.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            create.mode(0o600);
        }
        match create.open(path) {
            Ok(file) => file.sync_all()?,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => (),
            Err(err) => return Err(err.into()),
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
            }
        }
        // SQLite NOFOLLOW applies to every component. A normal system path
        // such as macOS /var may have an alias above the private state directory.
        let parent = path
            .parent()
            .ok_or(io::ErrorKind::InvalidInput)
            .map_err(io::Error::from)?;
        let name = path
            .file_name()
            .ok_or(io::ErrorKind::InvalidInput)
            .map_err(io::Error::from)?;
        let conn = Connection::open_with_flags(
            parent.canonicalize()?.join(name),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch(
            "PRAGMA page_size=4096;
             PRAGMA journal_mode=DELETE;
             PRAGMA synchronous=EXTRA;
             PRAGMA fullfsync=ON;
             PRAGMA trusted_schema=OFF;
             PRAGMA max_page_count=2048;
             CREATE TABLE IF NOT EXISTS outcomes (
                 room BLOB PRIMARY KEY CHECK(length(room)=16),
                 body BLOB NOT NULL CHECK(length(body)=94),
                 expires INTEGER NOT NULL,
                 next_try INTEGER NOT NULL,
                 delay INTEGER NOT NULL CHECK(delay BETWEEN 10 AND 3600)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS outcomes_next ON outcomes(next_try, room);",
        )?;
        Ok(Self(conn))
    }

    /// The acknowledgement list is released only after EXTRA-synchronous commit.
    /// On quota exhaustion, retain the other rooms in the ordering owner's queue.
    pub(super) fn save(
        &mut self,
        batch: &[FederationOutcome],
        now: u64,
    ) -> rusqlite::Result<Vec<Room>> {
        let now = sqlite_time(now)?;
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM outcomes WHERE expires < ?1", [now])?;
        let mut count: i64 = tx.query_row("SELECT count(*) FROM outcomes", [], |row| row.get(0))?;
        let mut saved = Vec::new();
        for receipt in batch {
            let room = receipt.room();
            let existing: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT body FROM outcomes WHERE room=?1",
                    [room.as_slice()],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(body) = existing {
                if body.as_slice() == receipt.as_bytes() {
                    saved.push(room);
                }
            } else if count < MAX_RECEIPTS && now <= sqlite_time(receipt.expires_at())? {
                tx.execute(
                    "INSERT INTO outcomes(room,body,expires,next_try,delay) VALUES(?1,?2,?3,?4,10)",
                    params![
                        room.as_slice(),
                        receipt.as_bytes().as_slice(),
                        sqlite_time(receipt.expires_at())?,
                        now
                    ],
                )?;
                count += 1;
                saved.push(room);
            }
        }
        tx.commit()?;
        Ok(saved)
    }

    /// Commit the next retry time before POST. A restarted worker cannot send
    /// faster than its backoff, even if it stopped in the middle of a request.
    pub(super) fn claim(&mut self, now: u64) -> rusqlite::Result<Option<FederationOutcome>> {
        let now = sqlite_time(now)?;
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM outcomes WHERE expires < ?1", [now])?;
        let row: Option<(Vec<u8>, i64)> = tx
            .query_row(
                "SELECT body,delay FROM outcomes WHERE next_try<=?1 ORDER BY next_try,room LIMIT 1",
                [now],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((bytes, delay)) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let receipt = bytes
            .try_into()
            .ok()
            .and_then(FederationOutcome::from_bytes)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        tx.execute(
            "UPDATE outcomes SET next_try=?1, delay=?2 WHERE room=?3",
            params![
                now.saturating_add(delay),
                delay.saturating_mul(2).min(3600),
                receipt.room().as_slice()
            ],
        )?;
        tx.commit()?;
        Ok(Some(receipt))
    }

    pub(super) fn settle(
        &self,
        receipt: &FederationOutcome,
        delivery: Delivery,
    ) -> rusqlite::Result<()> {
        let room = receipt.room();
        match delivery {
            Delivery::Accepted => {
                self.0.execute(
                    "DELETE FROM outcomes WHERE room=?1 AND body=?2",
                    params![room.as_slice(), receipt.as_bytes().as_slice()],
                )?;
            }
            Delivery::Retry => (),
            Delivery::Refused => {
                self.0.execute(
                    "UPDATE outcomes SET next_try=?1 WHERE room=?2 AND body=?3",
                    params![
                        sqlite_time(receipt.expires_at())?.saturating_add(1),
                        room.as_slice(),
                        receipt.as_bytes().as_slice()
                    ],
                )?;
            }
        }
        Ok(())
    }
}
