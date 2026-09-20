//! No-dependency evidence-integrity primitives for tactical capture.
//!
//! This module owns strict duplicate-free JSON parsing, reparse-safe absolute
//! file checks, stable reads, and streaming SHA-256. It has no app or sim
//! authority.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail, ensure};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::util::sha256::{Sha256, digest_hex};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct SealedJsonFile<T> {
    pub(crate) path: PathBuf,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) value: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDigest {
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

pub(crate) fn validate_new_output_directory(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "--output must be an absolute path");
    reject_reparse_ancestors(path, "tactical output")?;
    match fs::symlink_metadata(path) {
        Ok(_) => bail!(
            "--output already exists; tactical bundles are immutable: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect output {}", path.display()));
        }
    }
    let parent = path
        .parent()
        .context("--output must have a parent directory")?;
    reject_reparse_ancestors(parent, "tactical output parent")?;
    let metadata = fs::metadata(parent)
        .with_context(|| format!("cannot inspect output parent {}", parent.display()))?;
    ensure!(
        metadata.is_dir(),
        "tactical output parent is not a directory: {}",
        parent.display()
    );
    Ok(())
}

pub(crate) fn require_absolute_regular_non_reparse(path: &Path, label: &str) -> Result<()> {
    ensure!(path.is_absolute(), "{label} must be an absolute path");
    reject_reparse_ancestors(path, label)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("{label} does not exist: {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "{label} must be a regular non-link file: {}",
        path.display()
    );
    Ok(())
}

fn reject_reparse_ancestors(path: &Path, label: &str) -> Result<()> {
    for component in path.ancestors() {
        let metadata = match fs::symlink_metadata(component) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot inspect {label} ancestor {}", component.display())
                });
            }
        };
        ensure!(
            !metadata.file_type().is_symlink() && !metadata_is_reparse(&metadata),
            "{label} crosses a link, junction, or reparse point: {}",
            component.display()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(_metadata: &fs::Metadata) -> bool {
    false
}

fn metadata_stamp(metadata: &fs::Metadata) -> (u64, Option<SystemTime>) {
    (metadata.len(), metadata.modified().ok())
}

pub(crate) fn read_stable_regular_bytes(path: &Path, label: &str) -> Result<(Vec<u8>, FileDigest)> {
    require_absolute_regular_non_reparse(path, label)?;
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.len() <= MAX_JSON_BYTES,
        "{label} exceeds JSON size limit"
    );
    let bytes =
        fs::read(path).with_context(|| format!("cannot read {label} {}", path.display()))?;
    let after = fs::symlink_metadata(path)?;
    ensure!(
        metadata_stamp(&before) == metadata_stamp(&after) && bytes.len() as u64 == before.len(),
        "{label} changed while being read: {}",
        path.display()
    );
    let digest = FileDigest {
        byte_length: bytes.len() as u64,
        sha256: sha256_hex(&bytes),
    };
    Ok((bytes, digest))
}

pub(crate) fn sha256_file(path: &Path, label: &str) -> Result<FileDigest> {
    require_absolute_regular_non_reparse(path, label)?;
    let before = fs::symlink_metadata(path)?;
    let file =
        File::open(path).with_context(|| format!("cannot open {label} {}", path.display()))?;
    let handle_before = file.metadata()?;
    let mut reader = BufReader::new(file);
    let mut sha = Sha256::new();
    let mut byte_length = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot hash {label} {}", path.display()))?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
        byte_length = byte_length
            .checked_add(count as u64)
            .context("file length overflow while hashing")?;
    }
    let handle_after = reader.get_ref().metadata()?;
    let after = fs::symlink_metadata(path)?;
    ensure!(
        metadata_stamp(&before) == metadata_stamp(&handle_before)
            && metadata_stamp(&before) == metadata_stamp(&handle_after)
            && metadata_stamp(&before) == metadata_stamp(&after)
            && byte_length == before.len(),
        "{label} changed while being hashed: {}",
        path.display()
    );
    Ok(FileDigest {
        byte_length,
        sha256: digest_hex(sha.finalize()),
    })
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    crate::util::sha256::sha256_hex(bytes)
}

pub(crate) fn parse_strict_json<T>(bytes: &[u8], label: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let strict = StrictJson::deserialize(&mut deserializer)
        .with_context(|| format!("{label} is not strict JSON"))?;
    deserializer
        .end()
        .with_context(|| format!("{label} has trailing data"))?;
    serde_json::from_value(strict.0)
        .with_context(|| format!("{label} has invalid types, values, or keys"))
}

struct StrictJson(Value);

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonSeed;

impl<'de> DeserializeSeed<'de> for StrictJsonSeed {
    type Value = StrictJson;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJson::deserialize(deserializer)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("strict finite JSON")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if !value.is_finite() {
            return Err(E::custom("non-finite JSON number"));
        }
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictJson)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::String(value)))
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::Null))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(StrictJson(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJson::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictJsonSeed)? {
            values.push(value.0);
        }
        Ok(StrictJson(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object key {key:?}"
                )));
            }
            let value = map.next_value_seed(StrictJsonSeed)?;
            values.insert(key, value.0);
        }
        Ok(StrictJson(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let bytes = b"chunk boundaries must not change a streaming SHA-256 digest";
        let mut chunked = Sha256::new();
        for chunk in bytes.chunks(7) {
            chunked.update(chunk);
        }
        assert_eq!(digest_hex(chunked.finalize()), sha256_hex(bytes));
    }
}
