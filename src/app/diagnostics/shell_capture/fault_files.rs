//! Immutable diagnostic receipts and bounded external-owned-fault controls.

use super::*;
use serde_json::Value;
use std::io::Read;

fn sidecar(prefix: &Path, kind: &str) -> PathBuf {
    let mut name = prefix.as_os_str().to_owned();
    name.push(format!(".{kind}.json"));
    PathBuf::from(name)
}

pub(crate) fn write_atomic(path: &Path, value: &Value) -> Result<()> {
    ensure!(!path.try_exists()?, "stale fault receipt exists");
    let pending = sidecar(path, "pending");
    write_new_file(&pending, &serde_json::to_vec_pretty(value)?)?;
    // One writer owns each named sidecar. Publish only its complete immutable
    // bytes; a reader never sees an incomplete Ready/kill-boundary receipt.
    fs::rename(pending, path)?;
    Ok(())
}

pub(crate) fn read_control(path: &Path, byte_limit: u64) -> Result<Option<Value>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= byte_limit,
        "invalid fault control file"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(byte_limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= byte_limit,
        "fault control file grew beyond its bound"
    );
    Ok(Some(serde_json::from_slice(&bytes)?))
}
