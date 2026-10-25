//! The ordering owner's encrypted full-match archive. Sequence offsets stay on
//! disk; the in-memory deque is only a bounded cache of this same stream.

use super::sequenced::{DEPARTURE_EVENT, FRAME_END_EVENT};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const ROOM_QUOTA: u64 = 1024 * 1024 * 1024;
const GLOBAL_QUOTA: u64 = 8 * ROOM_QUOTA;
const FRAME_END_BYTES: u64 = 4 + 13 + 8;
const DEPARTURE_BYTES: u64 = 4 + 14 + 8;
const MAX_RECORD_BYTES: usize = 22 + 40 + super::ORDERING_BATCH_CAPACITY;

pub(super) struct JournalDirectory {
    root: PathBuf,
    // Reservation is logical, never a disk preallocation. Abandoned journals
    // found after restart retain their quota and are never overwritten.
    reserved: BTreeSet<[u8; 16]>,
}

impl JournalDirectory {
    pub(super) fn open(root: &Path) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(root)?;
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
        }
        let mut reserved = BTreeSet::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.len() != 32 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            let mut room = [0; 16];
            for (index, byte) in room.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&name[index * 2..index * 2 + 2], 16)
                    .map_err(|_| io::ErrorKind::InvalidData)?;
            }
            reserved.insert(room);
        }
        Ok(Self {
            root: root.to_owned(),
            reserved,
        })
    }

    pub(super) fn create(&mut self, room: [u8; 16], tick_hz: u16) -> io::Result<Journal> {
        if self.reserved.contains(&room) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if self.reserved.len() as u64 >= GLOBAL_QUOTA / ROOM_QUOTA {
            return Err(io::ErrorKind::StorageFull.into());
        }
        let name: String = room.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = self.root.join(name);
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        self.reserved.insert(room);
        let data = private_new(&path.join("stream.data"))?;
        let index = private_new(&path.join("stream.index"))?;
        let reserved_tail = (24 * 60 * 60 * u64::from(tick_hz) + 2) * FRAME_END_BYTES
            + u64::from(super::MAX_PEERS) * DEPARTURE_BYTES;
        Ok(Journal {
            room,
            path,
            data,
            index,
            entries: 0,
            used: 0,
            reserved_tail,
            pending: 0,
            failed: false,
        })
    }

    pub(super) fn cleanup(&mut self, journal: Journal) -> io::Result<()> {
        // Only a clean, legitimately ended match may retire its exact files.
        // Never remove an I/O-failed archive or recursively delete a directory.
        if journal.failed {
            return Err(io::ErrorKind::Other.into());
        }
        let Journal {
            room,
            path,
            data,
            index,
            ..
        } = journal;
        // Windows can defer deletion until the last file handle closes. Close
        // both files before removing them and their containing directory.
        drop(data);
        drop(index);
        fs::remove_file(path.join("stream.data"))?;
        fs::remove_file(path.join("stream.index"))?;
        fs::remove_dir(&path)?;
        self.reserved.remove(&room);
        Ok(())
    }
}

fn private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(super) struct Journal {
    room: [u8; 16],
    path: PathBuf,
    data: File,
    index: File,
    entries: u64,
    used: u64,
    reserved_tail: u64,
    pending: u64,
    failed: bool,
}

impl Journal {
    pub(super) fn reserve_batch(&mut self, bytes: usize) -> bool {
        let bytes = bytes as u64 + 12;
        if self.failed || self.used + self.reserved_tail + self.pending + bytes > ROOM_QUOTA {
            return false;
        }
        self.pending += bytes;
        true
    }

    /// One checked append transaction per complete frame. A partial write is
    /// left intact for inspection, never truncated or treated as eviction.
    pub(super) fn commit(&mut self, records: &[Vec<u8>]) -> io::Result<()> {
        if self.failed {
            return Err(io::ErrorKind::Other.into());
        }
        let result = self.commit_inner(records);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn commit_inner(&mut self, records: &[Vec<u8>]) -> io::Result<()> {
        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(records.len() * 8);
        let mut offset = self.data.seek(SeekFrom::End(0))?;
        let mut mandatory = 0;
        let mut optional = 0;
        for record in records {
            if record.len() > MAX_RECORD_BYTES {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let bytes = record.len() as u64 + 12;
            if matches!(record.first(), Some(&FRAME_END_EVENT | &DEPARTURE_EVENT)) {
                mandatory += bytes;
            } else {
                optional += bytes;
            }
            offsets.extend_from_slice(&offset.to_le_bytes());
            data.extend_from_slice(&(record.len() as u32).to_le_bytes());
            data.extend_from_slice(record);
            offset += 4 + record.len() as u64;
        }
        if optional > self.pending || mandatory > self.reserved_tail {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.data.write_all(&data)?;
        self.index.seek(SeekFrom::End(0))?;
        self.index.write_all(&offsets)?;
        // These are unbuffered File writes. Restart migration is deliberately
        // unsupported; existing files are refused rather than resumed unsafely.
        self.entries += records.len() as u64;
        self.used += data.len() as u64 + offsets.len() as u64;
        self.pending -= optional;
        self.reserved_tail -= mandatory;
        Ok(())
    }

    pub(super) fn read(&mut self, sequence: u64, count: usize) -> io::Result<Vec<Vec<u8>>> {
        if self.failed || sequence == 0 || sequence > self.entries + 1 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let result = self.read_inner(sequence, count);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn read_inner(&mut self, sequence: u64, count: usize) -> io::Result<Vec<Vec<u8>>> {
        let count = (self.entries + 1 - sequence).min(count as u64) as usize;
        let mut offsets = vec![0; count * 8];
        self.index.seek(SeekFrom::Start((sequence - 1) * 8))?;
        self.index.read_exact(&mut offsets)?;
        let mut output = Vec::with_capacity(count);
        for (index, offset) in offsets.chunks_exact(8).enumerate() {
            self.data.seek(SeekFrom::Start(u64::from_le_bytes(
                offset.try_into().unwrap(),
            )))?;
            let mut length = [0; 4];
            self.data.read_exact(&mut length)?;
            let length = u32::from_le_bytes(length) as usize;
            if !(13..=MAX_RECORD_BYTES).contains(&length) {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let mut bytes = vec![0; length];
            self.data.read_exact(&mut bytes)?;
            if u64::from_le_bytes(bytes[1..9].try_into().unwrap()) != sequence + index as u64 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            output.push(bytes);
        }
        Ok(output)
    }
}
