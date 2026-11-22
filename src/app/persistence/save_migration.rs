use super::SaveRepository;
use crate::sim::snapshot::migration::{self, MigrationError};
use crate::sim::snapshot::{GameSnapshot, GameSnapshotHeader, SnapshotError};
use crate::util::file_system;
use crate::util::time::SystemTime;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveMigrationError {
    #[error("could not read the original save: {0}")]
    Read(#[source] io::Error),
    #[error("this save is outside the repository or has an unsupported layout")]
    Unsupported,
    #[error("could not read the original header: {0}")]
    Header(#[from] SnapshotError),
    #[error("could not write the original save backup: {0}")]
    Backup(#[source] io::Error),
    #[error(transparent)]
    Convert(#[from] MigrationError),
    #[error("could not write the converted save: {0}")]
    Write(#[source] io::Error),
}

impl SaveMigrationError {
    pub(crate) fn message_key(&self) -> &'static str {
        match self {
            Self::Read(_) => "VERA:SaveConversionReadFailed",
            Self::Backup(_) => "VERA:SaveConversionBackupFailed",
            Self::Convert(MigrationError::TunnelTransit(_)) => "VERA:SaveConversionTunnel",
            Self::Write(_) => "VERA:SaveConversionWriteFailed",
            Self::Unsupported | Self::Header(_) | Self::Convert(_) => "VERA:SaveConversionFailed",
        }
    }
}

impl SaveRepository {
    pub(crate) fn can_migrate(header: &GameSnapshotHeader) -> bool {
        // The browser host's memory write does not confirm IndexedDB persistence.
        !cfg!(all(
            feature = "wasm",
            target_arch = "wasm32",
            target_os = "unknown"
        )) && migration::supports(header)
    }

    pub(super) fn migrate_for_load(&self, source: &Path) -> Result<PathBuf, SaveMigrationError> {
        if source.parent() != Some(self.directory.as_path()) || !Self::is_save_file(source) {
            return Err(SaveMigrationError::Unsupported);
        }
        let mut bytes = self.read(source).map_err(SaveMigrationError::Read)?;
        if !Self::can_migrate(&GameSnapshot::read_metadata(&bytes)?) {
            return Err(SaveMigrationError::Unsupported);
        }

        let id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("migrated-{}-{id}.sav", std::process::id());
        let backups = self.directory.join("backups");
        file_system::create_dir_all(&backups).map_err(SaveMigrationError::Backup)?;
        let backup = backups.join(&name);
        // Persist the exact bytes used by the converter before it mutates them.
        file_system::write_new(&backup, &bytes).map_err(SaveMigrationError::Backup)?;

        migration::migrate(&mut bytes)?;
        let destination = self.directory.join(name);
        file_system::write_new(&destination, &bytes).map_err(SaveMigrationError::Write)?;
        log::info!(
            "Converted save {} to {}; original backup: {}",
            source.display(),
            destination.display(),
            backup.display(),
        );
        Ok(destination)
    }
}
