use super::SaveRepository;
use crate::sim::snapshot::migration::{self, MigrationError};
use crate::sim::snapshot::{GameSnapshot, GameSnapshotHeader, SnapshotError};
use crate::util::file_system;
use crate::util::time::SystemTime;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveMigrationError {
    #[error("enter a description for the converted save")]
    InvalidName,
    #[error("the generated save filename is already in use")]
    NameExists,
    #[error("the selected slot is the original save")]
    SourceTarget,
    #[error("could not read the original save: {0}")]
    Read(#[source] io::Error),
    #[error("this save is outside the repository or has an unsupported layout")]
    Unsupported,
    #[error("could not read the original header: {0}")]
    Header(#[from] SnapshotError),
    #[error("could not write the save backup: {0}")]
    Backup(#[source] io::Error),
    #[error(transparent)]
    Convert(#[from] MigrationError),
    #[error("could not write the converted save: {0}")]
    Write(#[source] io::Error),
}

impl From<super::SaveWriteError> for SaveMigrationError {
    fn from(error: super::SaveWriteError) -> Self {
        error.source.into()
    }
}

impl From<io::Error> for SaveMigrationError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Self::NameExists
        } else {
            Self::Write(error)
        }
    }
}

impl SaveMigrationError {
    pub(crate) fn message_key(&self) -> &'static str {
        match self {
            Self::InvalidName => "VERA:SaveConversionInvalidName",
            Self::NameExists => "VERA:SaveConversionNameExists",
            Self::SourceTarget => "VERA:SaveConversionSourceTarget",
            Self::Read(_) => "VERA:SaveConversionReadFailed",
            Self::Backup(_) => "VERA:SaveConversionBackupFailed",
            Self::Convert(MigrationError::InvalidTunnelTransit(_)) => "VERA:SaveConversionTunnel",
            Self::Write(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData | io::ErrorKind::NotFound
                ) =>
            {
                "VERA:SaveConversionSlotHelp"
            }
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

    pub(crate) fn migrate_to_slot(
        &self,
        source: &Path,
        description: &str,
        overwrite: Option<&Path>,
    ) -> Result<PathBuf, SaveMigrationError> {
        if !self.contains_save_path(source)
            || overwrite.is_some_and(|target| !self.contains_save_path(target))
        {
            return Err(SaveMigrationError::Unsupported);
        }
        let filename_part =
            Self::filename_part(description).ok_or(SaveMigrationError::InvalidName)?;
        if overwrite == Some(source) {
            return Err(SaveMigrationError::SourceTarget);
        }
        let bytes = self.read(source).map_err(SaveMigrationError::Read)?;
        let header = GameSnapshot::read_metadata(&bytes)?;
        if !Self::can_migrate(&header) {
            return Err(SaveMigrationError::Unsupported);
        }
        // Explicit overwrite is still unavailable for aliases of the source.
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let Some(target) = overwrite
            && same_file::is_same_file(source, target).map_err(SaveMigrationError::Write)?
        {
            return Err(SaveMigrationError::SourceTarget);
        }

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let backups = self.directory.join("backups");
        let id = format!("migrated-{}-{}", std::process::id(), now.as_nanos());
        let backup = backups.join(format!("{id}.sav"));
        // Persist the exact bytes used by the converter before it mutates them.
        file_system::create_dir_all(&backups).map_err(SaveMigrationError::Backup)?;
        file_system::write_new(&backup, &bytes).map_err(SaveMigrationError::Backup)?;

        let converted = migration::migrate_named(bytes, description)?;
        let destination = match overwrite {
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            Some(target) => {
                file_system::write_staged(&self.directory, &converted, |stage| {
                    Self::replace_converted_slot(target, &backups.join("replaced"), &id, || {
                        file_system::persist_noclobber(stage, target)
                    })
                })?;
                target.to_owned()
            }
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            Some(_) => return Err(SaveMigrationError::Unsupported),
            None => self.write_player_slot(
                None,
                Some(&filename_part),
                header.tick,
                now.as_secs(),
                &converted,
            )?,
        };
        log::info!(
            "Converted save {} to {}; original backup: {}",
            source.display(),
            destination.display(),
            backup.display(),
        );
        Ok(destination)
    }

    /// Claim the actual occupant by moving it to its own fresh backup directory.
    /// Both publication and rollback leave a competing writer's slot alone.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    pub(super) fn replace_converted_slot(
        target: &Path,
        replaced: &Path,
        id: &str,
        publish: impl FnOnce() -> io::Result<()>,
    ) -> Result<(), SaveMigrationError> {
        let name = target.file_name().ok_or(SaveMigrationError::Unsupported)?;
        require_regular_slot(target)?;
        let directory = replaced.join(id);
        std::fs::create_dir_all(replaced).map_err(SaveMigrationError::Backup)?;
        std::fs::create_dir(&directory).map_err(SaveMigrationError::Backup)?;
        let backup = directory.join(name);
        if let Err(error) = std::fs::rename(target, &backup) {
            let _ = std::fs::remove_dir(&directory);
            return Err(if error.kind() == io::ErrorKind::NotFound {
                SaveMigrationError::Write(error)
            } else {
                SaveMigrationError::Backup(error)
            });
        }
        let result = (|| -> io::Result<()> {
            require_regular_slot(&backup)?;
            if !Self::file_header(&backup)?.is_compatible() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the selected save is no longer compatible",
                ));
            }
            publish()
        })();
        if result.is_err() {
            match file_system::persist_noclobber(&backup, target) {
                Ok(()) => {
                    let _ = std::fs::remove_dir(&directory);
                }
                Err(error) => log::warn!(
                    "Could not restore {}: {error}. Previous slot retained at {}",
                    target.display(),
                    backup.display()
                ),
            }
        }
        result.map_err(Into::into)
    }
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn require_regular_slot(path: &Path) -> io::Result<()> {
    // Moving a relative symlink into a backup directory changes its referent.
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the selected slot is not a regular file",
        ));
    }
    Ok(())
}
