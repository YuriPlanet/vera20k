//! Typed tactical evidence envelope and atomic whole-directory publication.
//!
//! The child writes only inside one private sibling staging directory, fsyncs
//! both artifacts and that directory, then renames the complete directory to
//! the required nonexistent final path. The wrapper never accepts staging.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::profile::{
    CHECKPOINT_RADAR_ONLINE_V2, CONTRACT_SCHEMA, EMBEDDED_CONTRACT, FRAME_FILE_NAME,
    MANIFEST_FILE_NAME, PROFILE_SCHEMA, SealedJsonFile, TacticalCaptureContract,
    TacticalCaptureProfile, sha256_hex, validate_new_output_directory,
};

pub(crate) const CAPTURE_SCHEMA: &str = "vera20k.tactical-capture.v2";
const NATIVE_COMPARATOR_NONE: &str = "NONE";
const PARITY_CERTIFICATION_NONE: &str = "NONE";
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum TacticalCaptureStatus {
    Complete,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactIdentity {
    pub(crate) path: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileIdentity {
    pub(crate) path: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
    pub(crate) schema_version: String,
    pub(crate) profile_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContractIdentity {
    pub(crate) path: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
    pub(crate) schema_version: String,
    pub(crate) embedded_sha256: String,
    pub(crate) bytes_equal: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrameArtifact {
    pub(crate) file_name: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) row_stride: u32,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
    pub(crate) surface_format: String,
    pub(crate) pixel_layout: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailureDiagnostic {
    pub(crate) stage: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalCaptureManifest {
    pub(crate) schema_version: String,
    pub(crate) status: TacticalCaptureStatus,
    pub(crate) checkpoint: String,
    pub(crate) profile: ProfileIdentity,
    pub(crate) contract: ContractIdentity,
    pub(crate) frame: Option<FrameArtifact>,
    pub(crate) evidence: Option<Value>,
    pub(crate) failure: Option<FailureDiagnostic>,
    pub(crate) native_comparator: String,
    pub(crate) parity_certification: String,
    pub(crate) evidence_limitations: Vec<String>,
}

impl ArtifactIdentity {
    fn from_sealed<T>(sealed: &SealedJsonFile<T>) -> Self {
        Self {
            path: sealed.path.display().to_string(),
            byte_length: sealed.byte_length,
            sha256: sealed.sha256.clone(),
        }
    }
}

impl ProfileIdentity {
    fn new(profile: &SealedJsonFile<TacticalCaptureProfile>) -> Self {
        let artifact = ArtifactIdentity::from_sealed(profile);
        Self {
            path: artifact.path,
            byte_length: artifact.byte_length,
            sha256: artifact.sha256,
            schema_version: PROFILE_SCHEMA.to_owned(),
            profile_id: profile.value.profile_id.clone(),
        }
    }
}

impl ContractIdentity {
    fn new(contract: &SealedJsonFile<TacticalCaptureContract>) -> Self {
        let artifact = ArtifactIdentity::from_sealed(contract);
        Self {
            path: artifact.path,
            byte_length: artifact.byte_length,
            sha256: artifact.sha256,
            schema_version: CONTRACT_SCHEMA.to_owned(),
            embedded_sha256: sha256_hex(EMBEDDED_CONTRACT.as_bytes()),
            bytes_equal: contract.bytes == EMBEDDED_CONTRACT.as_bytes(),
        }
    }
}

impl FrameArtifact {
    pub(crate) fn from_bgra(
        width: u32,
        height: u32,
        surface_format: impl Into<String>,
        frame: &[u8],
    ) -> Result<Self> {
        let row_stride = width.checked_mul(4).context("BGRA row stride overflow")?;
        let expected_length = u64::from(row_stride)
            .checked_mul(u64::from(height))
            .context("BGRA frame length overflow")?;
        ensure!(
            frame.len() as u64 == expected_length,
            "BGRA frame has {} bytes, expected {expected_length}",
            frame.len()
        );
        let surface_format = surface_format.into();
        ensure!(
            matches!(surface_format.as_str(), "Bgra8Unorm" | "Bgra8UnormSrgb"),
            "unsupported tactical surface format {surface_format:?}"
        );
        let artifact = Self {
            file_name: FRAME_FILE_NAME.to_owned(),
            width,
            height,
            row_stride,
            byte_length: expected_length,
            sha256: sha256_hex(frame),
            surface_format,
            pixel_layout: "BGRA8".to_owned(),
        };
        artifact.validate()?;
        Ok(artifact)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.file_name == FRAME_FILE_NAME, "wrong frame file name");
        ensure!(
            self.width > 0 && self.height > 0,
            "frame dimensions must be nonzero"
        );
        let expected_stride = self
            .width
            .checked_mul(4)
            .context("BGRA row stride overflow")?;
        ensure!(
            self.row_stride == expected_stride,
            "frame row stride differs from BGRA8 width"
        );
        let expected_length = u64::from(expected_stride)
            .checked_mul(u64::from(self.height))
            .context("BGRA frame length overflow")?;
        ensure!(
            self.byte_length == expected_length,
            "frame byte length differs from its dimensions"
        );
        ensure!(
            is_lower_sha256(&self.sha256),
            "frame SHA-256 is not a lowercase digest"
        );
        ensure!(
            matches!(
                self.surface_format.as_str(),
                "Bgra8Unorm" | "Bgra8UnormSrgb"
            ),
            "unsupported tactical surface format {:?}",
            self.surface_format
        );
        ensure!(self.pixel_layout == "BGRA8", "wrong frame pixel layout");
        Ok(())
    }
}

impl TacticalCaptureManifest {
    pub(crate) fn complete(
        profile: &SealedJsonFile<TacticalCaptureProfile>,
        contract: &SealedJsonFile<TacticalCaptureContract>,
        frame: FrameArtifact,
        evidence: Value,
    ) -> Result<Self> {
        ensure!(
            frame.width == profile.value.capture.output_width
                && frame.height == profile.value.capture.output_height,
            "frame extent differs from the sealed tactical profile"
        );
        ensure!(
            profile
                .value
                .capture
                .surface_formats
                .contains(&frame.surface_format),
            "frame surface format differs from the sealed tactical profile"
        );
        let manifest = Self {
            schema_version: CAPTURE_SCHEMA.to_owned(),
            status: TacticalCaptureStatus::Complete,
            checkpoint: CHECKPOINT_RADAR_ONLINE_V2.to_owned(),
            profile: ProfileIdentity::new(profile),
            contract: ContractIdentity::new(contract),
            frame: Some(frame),
            evidence: Some(evidence),
            failure: None,
            native_comparator: NATIVE_COMPARATOR_NONE.to_owned(),
            parity_certification: PARITY_CERTIFICATION_NONE.to_owned(),
            evidence_limitations: profile.value.evidence_limitations.clone(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn failed(
        profile: &SealedJsonFile<TacticalCaptureProfile>,
        contract: &SealedJsonFile<TacticalCaptureContract>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self> {
        let manifest = Self {
            schema_version: CAPTURE_SCHEMA.to_owned(),
            status: TacticalCaptureStatus::Failed,
            checkpoint: CHECKPOINT_RADAR_ONLINE_V2.to_owned(),
            profile: ProfileIdentity::new(profile),
            contract: ContractIdentity::new(contract),
            frame: None,
            evidence: None,
            failure: Some(FailureDiagnostic {
                stage: stage.into(),
                message: message.into(),
            }),
            native_comparator: NATIVE_COMPARATOR_NONE.to_owned(),
            parity_certification: PARITY_CERTIFICATION_NONE.to_owned(),
            evidence_limitations: profile.value.evidence_limitations.clone(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == CAPTURE_SCHEMA,
            "wrong manifest schema"
        );
        ensure!(
            self.checkpoint == CHECKPOINT_RADAR_ONLINE_V2,
            "wrong manifest checkpoint"
        );
        ensure!(
            self.profile.schema_version == PROFILE_SCHEMA
                && self.contract.schema_version == CONTRACT_SCHEMA,
            "manifest identity schema mismatch"
        );
        ensure!(
            self.contract.bytes_equal && self.contract.sha256 == self.contract.embedded_sha256,
            "external/embedded tactical contract identity mismatch"
        );
        ensure!(
            self.native_comparator == NATIVE_COMPARATOR_NONE
                && self.parity_certification == PARITY_CERTIFICATION_NONE,
            "tactical v2 has no native comparator or parity certification"
        );
        ensure!(
            !self.evidence_limitations.is_empty()
                && self
                    .evidence_limitations
                    .iter()
                    .all(|value| !value.is_empty()),
            "manifest evidence limitations must be nonempty"
        );
        match self.status {
            TacticalCaptureStatus::Complete => {
                ensure!(
                    self.frame.is_some() && self.evidence.is_some() && self.failure.is_none(),
                    "COMPLETE requires frame/evidence and forbids failure"
                );
                self.frame.as_ref().expect("checked").validate()?;
                validate_evidence_shape(self.evidence.as_ref().expect("checked"))?;
            }
            TacticalCaptureStatus::Failed => {
                ensure!(
                    self.frame.is_none() && self.evidence.is_none() && self.failure.is_some(),
                    "FAILED requires failure and forbids frame/evidence"
                );
                let failure = self.failure.as_ref().expect("checked");
                ensure!(
                    !failure.stage.is_empty() && !failure.message.is_empty(),
                    "failure diagnostic fields must be nonempty"
                );
            }
        }
        let value = serde_json::to_value(self)?;
        ensure!(
            !contains_forbidden_verdict(&value),
            "manifest cannot emit a native result verdict without a comparator"
        );
        Ok(())
    }
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_evidence_shape(evidence: &Value) -> Result<()> {
    let object = evidence
        .as_object()
        .context("manifest evidence must be an object")?;
    ensure!(
        object.len() == 2 && object.contains_key("stable") && object.contains_key("run"),
        "manifest evidence must contain exactly stable and run objects"
    );
    ensure!(
        object["stable"].is_object() && object["run"].is_object(),
        "manifest stable/run evidence values must be objects"
    );
    Ok(())
}

fn contains_forbidden_verdict(value: &Value) -> bool {
    match value {
        Value::String(value) => matches!(value.as_str(), "MATCH" | "DRIFT"),
        Value::Array(values) => values.iter().any(contains_forbidden_verdict),
        Value::Object(values) => values.values().any(contains_forbidden_verdict),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(crate) fn publish_complete(
    output_dir: &Path,
    manifest: &TacticalCaptureManifest,
    frame: &[u8],
) -> Result<()> {
    ensure!(
        manifest.status == TacticalCaptureStatus::Complete,
        "publish_complete requires COMPLETE manifest"
    );
    manifest.validate()?;
    let frame_identity = manifest.frame.as_ref().context("missing frame identity")?;
    ensure!(
        frame_identity.byte_length == frame.len() as u64
            && frame_identity.sha256 == sha256_hex(frame),
        "frame bytes differ from manifest identity"
    );
    publish_transaction(output_dir, manifest, Some(frame), PublishFault::None)
}

pub(crate) fn publish_failure(output_dir: &Path, manifest: &TacticalCaptureManifest) -> Result<()> {
    ensure!(
        manifest.status == TacticalCaptureStatus::Failed,
        "publish_failure requires FAILED manifest"
    );
    manifest.validate()?;
    publish_transaction(output_dir, manifest, None, PublishFault::None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishFault {
    None,
    FrameWrite,
    ManifestWrite,
    FrameSync,
    ManifestSync,
    DirectorySync,
    FinalRename,
    SimulatedCrashAfterDirectorySync,
}

pub(crate) fn publish_transaction(
    output_dir: &Path,
    manifest: &TacticalCaptureManifest,
    frame: Option<&[u8]>,
    fault: PublishFault,
) -> Result<()> {
    validate_new_output_directory(output_dir)?;
    let parent = output_dir.parent().context("output has no parent")?;
    let stage = create_private_staging(parent, output_dir)?;
    let leave_staging = fault == PublishFault::SimulatedCrashAfterDirectorySync;

    let result = (|| -> Result<()> {
        let frame_path = stage.join(FRAME_FILE_NAME);
        let manifest_path = stage.join(MANIFEST_FILE_NAME);
        let mut frame_file = None;
        if let Some(frame) = frame {
            injected(
                fault,
                PublishFault::FrameWrite,
                "injected frame write failure",
            )?;
            let mut file = create_new_file(&frame_path)?;
            file.write_all(frame)
                .with_context(|| format!("cannot write {}", frame_path.display()))?;
            frame_file = Some(file);
        }

        injected(
            fault,
            PublishFault::ManifestWrite,
            "injected manifest write failure",
        )?;
        let mut serialized = serde_json::to_vec_pretty(manifest)?;
        serialized.push(b'\n');
        let mut manifest_file = create_new_file(&manifest_path)?;
        manifest_file
            .write_all(&serialized)
            .with_context(|| format!("cannot write {}", manifest_path.display()))?;

        if let Some(file) = frame_file.as_mut() {
            injected(
                fault,
                PublishFault::FrameSync,
                "injected frame sync failure",
            )?;
            file.sync_all()
                .with_context(|| format!("cannot sync {}", frame_path.display()))?;
        }
        injected(
            fault,
            PublishFault::ManifestSync,
            "injected manifest sync failure",
        )?;
        manifest_file
            .sync_all()
            .with_context(|| format!("cannot sync {}", manifest_path.display()))?;
        drop(frame_file);
        drop(manifest_file);

        injected(
            fault,
            PublishFault::DirectorySync,
            "injected directory sync failure",
        )?;
        sync_directory(&stage)?;
        if leave_staging {
            bail!("injected simulated crash after staging-directory sync");
        }
        injected(
            fault,
            PublishFault::FinalRename,
            "injected final rename failure",
        )?;
        fs::rename(&stage, output_dir).with_context(|| {
            format!(
                "cannot atomically publish {} as {}",
                stage.display(),
                output_dir.display()
            )
        })?;
        Ok(())
    })();

    if result.is_err() && !leave_staging {
        // This task owns exactly the private sibling it created. It never
        // removes or rewrites the final output path.
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn create_private_staging(parent: &Path, output: &Path) -> Result<PathBuf> {
    let output_name = output
        .file_name()
        .context("tactical output needs a final component")?
        .to_string_lossy();
    for _ in 0..1024 {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{output_name}.tactical-staging-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot create private staging {}", candidate.display())
                });
            }
        }
    }
    bail!("could not allocate a unique tactical staging directory")
}

fn create_new_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create immutable artifact {}", path.display()))
}

fn injected(active: PublishFault, expected: PublishFault, message: &str) -> Result<()> {
    if active == expected {
        bail!("{message}");
    }
    Ok(())
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let directory = OpenOptions::new()
        // FlushFileBuffers requires GENERIC_WRITE even for a directory handle.
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .with_context(|| format!("cannot open staging directory {}", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("cannot sync staging directory {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed_inputs() -> (
        SealedJsonFile<TacticalCaptureProfile>,
        SealedJsonFile<TacticalCaptureContract>,
    ) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let profile = TacticalCaptureProfile::load_strict(
            &root.join("tools/tactical_certification/profiles/soviet-radar-online-v2.json"),
        )
        .expect("profile");
        let contract = TacticalCaptureContract::load_external(
            &root.join("src/app/diagnostics/tactical_capture/contract.v2.json"),
        )
        .expect("contract");
        (profile, contract)
    }

    fn temp_parent(label: &str) -> PathBuf {
        let parent = std::env::temp_dir().join(format!(
            "vera20k-tactical-publish-{label}-{}-{}",
            std::process::id(),
            STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&parent).expect("create test parent");
        // macOS keeps the temp dir under `/var`, a symlink to `/private/var`,
        // which the output guard rightly refuses; test the resolved path.
        #[cfg(unix)]
        let parent = fs::canonicalize(&parent).expect("canonical test parent");
        parent
    }

    fn complete_fixture(
        profile: &SealedJsonFile<TacticalCaptureProfile>,
        contract: &SealedJsonFile<TacticalCaptureContract>,
    ) -> (Vec<u8>, TacticalCaptureManifest) {
        let frame = vec![0_u8; 800 * 600 * 4];
        let frame_identity =
            FrameArtifact::from_bgra(800, 600, "Bgra8UnormSrgb", &frame).expect("frame");
        let evidence = serde_json::json!({"stable": {}, "run": {}});
        let manifest =
            TacticalCaptureManifest::complete(profile, contract, frame_identity, evidence)
                .expect("manifest");
        (frame, manifest)
    }

    #[test]
    fn complete_and_failure_bundles_publish_only_the_permitted_inventory() {
        let (profile, contract) = sealed_inputs();
        let (frame, manifest) = complete_fixture(&profile, &contract);
        let parent = temp_parent("success");
        let complete = parent.join("complete");
        publish_complete(&complete, &manifest, &frame).expect("publish complete");
        let mut complete_names: Vec<_> = fs::read_dir(&complete)
            .expect("inventory")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        complete_names.sort();
        assert_eq!(
            complete_names,
            [
                std::ffi::OsString::from(MANIFEST_FILE_NAME),
                std::ffi::OsString::from(FRAME_FILE_NAME),
            ]
        );

        let failed = parent.join("failed");
        let failure_manifest =
            TacticalCaptureManifest::failed(&profile, &contract, "test", "diagnostic")
                .expect("failure manifest");
        publish_failure(&failed, &failure_manifest).expect("publish failure");
        let failure_names: Vec<_> = fs::read_dir(&failed)
            .expect("inventory")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(
            failure_names,
            [std::ffi::OsString::from(MANIFEST_FILE_NAME)]
        );
        fs::remove_dir_all(parent).expect("remove test parent");
    }

    #[test]
    fn every_prepublication_fault_keeps_final_output_absent() {
        let (profile, contract) = sealed_inputs();
        let (frame, manifest) = complete_fixture(&profile, &contract);
        let parent = temp_parent("faults");
        for fault in [
            PublishFault::FrameWrite,
            PublishFault::ManifestWrite,
            PublishFault::FrameSync,
            PublishFault::ManifestSync,
            PublishFault::DirectorySync,
            PublishFault::FinalRename,
        ] {
            let output = parent.join(format!("{fault:?}"));
            publish_transaction(&output, &manifest, Some(&frame), fault)
                .expect_err("fault must fail");
            assert!(!output.exists(), "{fault:?} exposed final output");
        }
        assert_eq!(
            fs::read_dir(&parent).expect("inventory").count(),
            0,
            "handled failures must clean only their owned staging directories"
        );
        fs::remove_dir(parent).expect("remove test parent");
    }

    #[test]
    fn simulated_crash_leaves_only_private_staging_and_no_final_path() {
        let (profile, contract) = sealed_inputs();
        let (frame, manifest) = complete_fixture(&profile, &contract);
        let parent = temp_parent("crash");
        let output = parent.join("capture");
        publish_transaction(
            &output,
            &manifest,
            Some(&frame),
            PublishFault::SimulatedCrashAfterDirectorySync,
        )
        .expect_err("simulated crash");
        assert!(!output.exists());
        let entries: Vec<PathBuf> = fs::read_dir(&parent)
            .expect("inventory")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0]
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with(".capture.tactical-staging-")
        );
        fs::remove_dir_all(parent).expect("remove test parent");
    }
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> Result<()> {
    let directory = File::open(path)
        .with_context(|| format!("cannot open staging directory {}", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("cannot sync staging directory {}", path.display()))
}
