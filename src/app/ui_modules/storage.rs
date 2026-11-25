//! Each mod's and map's local key-value store (RFC 10, "Storage"): under
//! the user's config directory, `wasm-storage/<mod Name or map StorageId>/`.
//! Keys 1..64 ASCII bytes, values up to 16 KiB, up to 256 keys and
//! 256 KiB (keys and values) per store; over the limit a write is
//! `Refused` and nothing changes. An empty value deletes the key.

use vera20k_wasm_ext::{Invalid, Status};

/// One source's store.
pub(crate) struct Storage {}

impl Storage {
    /// `client_storage_read`.
    pub(crate) fn read(&mut self, _key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid> {
        Ok((None, Status::Unsupported))
    }

    /// `client_storage_write`.
    pub(crate) fn write(&mut self, _key: &str, _data: &[u8]) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
}
