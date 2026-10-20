//! High-level asset loading API for the RA2 engine.
//!
//! The AssetManager registers top-level and explicitly named nested MIX
//! archives in the same construction order as active retail `gamemd.exe`.
//! Registration appends to the search list and the first matching archive
//! wins. Other known nested archives are catalogued for direct access and
//! on-demand registration, but do not silently enter the global search path.
//! The requested RA2 music extension additionally registers `theme.mix` behind
//! `thememd.mix`; native YR otherwise selects only the latter when present.
//! Audio selection and lifetime: [process audio owner](../../tools/audio_catalog_owner.md).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::assets::audio_bag::AudioIndex;
use crate::assets::error::AssetError;
use crate::assets::mix_archive::MixArchive;
use crate::assets::mix_hash::mix_hash;

/// A MIX archive with a human-readable name for logging and diagnostics.
#[derive(Clone)]
struct NamedArchive {
    /// Name for logs and tools, for example `ra2.mix` or `ra2.mix -> local.mix`.
    name: String,
    /// Parsed archive data.
    archive: MixArchive,
}

#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
struct LooseAsset {
    path: PathBuf,
    source_name: String,
    bytes: OnceLock<Option<Box<[u8]>>>,
}

impl LooseAsset {
    /// Borrowed asset APIs need stable storage, but loose files may be rewritten
    /// by profile/SED/preview owners. A retained mapping prevents Windows writes
    /// and cannot safely coexist with truncation on other platforms. Snapshot
    /// only requested payloads; root indexing must not copy entire MIX archives.
    /// This is Rust storage policy, not native raw-file cache equivalence.
    fn bytes(&self) -> Option<&[u8]> {
        self.bytes
            .get_or_init(|| match std::fs::read(&self.path) {
                Ok(bytes) => Some(bytes.into_boxed_slice()),
                Err(err) => {
                    log::debug!("Skipping loose {}: {}", self.path.display(), err);
                    None
                }
            })
            .as_deref()
    }
}

#[derive(Clone, Copy)]
struct AssetLocation {
    archive_index: usize,
    entry_id: i32,
}

#[derive(Default)]
struct WildcardMount {
    matched: bool,
    first_registered: bool,
}

/// Borrowed first-match resolution from the production archive stack.
///
/// This is observational only: callers receive the same bytes that `get_ref`
/// would return plus the archive-chain identity and hashed entry ID that
/// selected them. It must not be used to bypass normal lookup precedence.
#[derive(Clone, Copy, Debug)]
pub struct AssetResolutionRef<'a> {
    pub bytes: &'a [u8],
    pub source_archive: &'a str,
    pub entry_id: i32,
}

/// Identity of the bytes actually consumed by one side of the audio pair.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AudioSourceIdentity {
    pub source_archive: String,
    pub entry_id: i32,
    pub payload_len: usize,
    pub source_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AudioIndexSources {
    pub idx: AudioSourceIdentity,
    pub bag: AudioSourceIdentity,
}

pub struct LoadedAudioIndex {
    pub index: AudioIndex,
    pub sources: AudioIndexSources,
}

/// Parse a selected pair without making another source-selection decision.
/// Production and explicit diagnostic overrides share this boundary.
pub fn load_audio_index_pair(
    idx: AssetResolutionRef<'_>,
    bag: AssetResolutionRef<'_>,
) -> Result<LoadedAudioIndex, AssetError> {
    let identity = |source: AssetResolutionRef<'_>| AudioSourceIdentity {
        source_archive: source.source_archive.to_owned(),
        entry_id: source.entry_id,
        payload_len: source.bytes.len(),
        source_sha256: crate::util::sha256::sha256_hex(source.bytes),
    };
    let sources = AudioIndexSources {
        idx: identity(idx),
        bag: identity(bag),
    };
    let index = AudioIndex::from_idx_bag(idx.bytes, bag.bytes.to_vec()).ok_or_else(|| {
        AssetError::ParseError {
            format: "audio.idx".to_owned(),
            detail: format!("unsupported or truncated index from {}", idx.source_archive),
        }
    })?;
    Ok(LoadedAudioIndex { index, sources })
}

/// A frozen resolver winner, not another archive search list. Archive storage
/// stays on the manager; loose bytes stay in its existing OnceLock snapshot.
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
struct SelectedAudioFile {
    source_archive: String,
    entry_id: i32,
    loose_name: Option<String>,
}

#[derive(Default)]
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
struct SelectedAudioPair {
    idx: Option<SelectedAudioFile>,
    bag: Option<SelectedAudioFile>,
}

/// The media-pack branch selected by active `Init_Mix_Files @ 0x00530460`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaArchiveMode {
    /// Normal startup uses the current media index plus one in `%02d` names.
    Numbered { media_index: i32 },
    /// Retail's `-CD` switch selects the wildcard-enumeration branch.
    CdWildcard,
}

impl MediaArchiveMode {
    /// Stock digital YR uses media index 2, selecting the `03` archive family.
    /// Tools and capture callers select this explicitly; asset loading never
    /// examines process arguments.
    pub const STOCK_DIGITAL: Self = Self::Numbered { media_index: 2 };
}

/// An owned result from retail's cache-first `LoadFileFromMIX` path.
///
/// The payload is owned by the cache rather than an archive borrow, so the
/// first winner remains valid after theater archives are destroyed/remounted.
#[derive(Clone, Debug)]
pub struct MixFileLoad {
    pub bytes: Arc<[u8]>,
    pub source_archive: Arc<str>,
    #[cfg(test)]
    pub entry_id: i32,
}

/// Manages loaded MIX archives and provides name-based lookups.
///
/// Archives are searched in priority order. Earlier archives win.
///
/// ## Two lookup families (F11)
///
/// - **Current-resolution** — `get`, `get_ref`, `get_with_source`,
///   `get_with_source_ref`, `resolve_ref`, `contains`, `archive`: re-resolve
///   through the live archive list and `lookup_index`, so their answers CHANGE
///   when a theater activates or archives are (de)registered.
/// - **Process-sticky** — `load_file_from_mix`: the native
///   `LoadFileFromMIX @ 0x005B40B0` normalized-CRC first-winner cache. The
///   first resolution wins for the life of the process, deliberately
///   surviving theater swaps. Never invalidated, matching retail.
///
/// Pick by invalidation semantics, not convenience: provenance/evidence reads
/// and theater-dependent assets want current-resolution; anything mirroring
/// the native `LoadFileFromMIX` contract wants the sticky path.
pub struct AssetManager {
    /// Loaded MIX archives in search priority order.
    archives: Vec<NamedArchive>,
    /// Parsed, named nested MIX archives that retail has not registered yet.
    archive_catalog: Vec<NamedArchive>,
    /// Precomputed first-match lookup across all archives.
    lookup_index: HashMap<i32, AssetLocation>,
    /// `LoadFileFromMIX`'s process-lifetime, normalized-CRC first-winner cache.
    mix_file_cache: Mutex<HashMap<i32, MixFileLoad>>,
    /// Case-insensitive, non-recursive view of the retail executable directory.
    /// Payloads are owned snapshots at first lookup (including failed reads).
    /// Mutable profile/seed/preview consumers read the filesystem directly.
    loose_files: HashMap<String, LooseAsset>,
    /// Selected at AudioSystem::Init, before later bulk/theater mounts.
    audio_pair: SelectedAudioPair,
    /// Frozen root-file policy shared by Rules, Art, AI and tooling.
    ini_roots: crate::util::ini_roots::IniRoots,
    /// Currently registered theater identity and its archive names.
    active_theater: Option<String>,
    active_theater_archives: Vec<String>,
    /// Path to the RA2 installation directory.
    ra2_dir: PathBuf,
}

/// Test fixtures open the retail install once per test process and hand each
/// test its own copy (`rules::retail_ini_fixture`). A copy shares the archive
/// bytes and starts with the same registrations, lookups, caches and loose
/// snapshots as the original. Production code holds one manager and never
/// copies it.
#[cfg(any(test, feature = "test-support"))]
impl Clone for AssetManager {
    fn clone(&self) -> Self {
        Self {
            archives: self.archives.clone(),
            archive_catalog: self.archive_catalog.clone(),
            lookup_index: self.lookup_index.clone(),
            mix_file_cache: Mutex::new(
                self.mix_file_cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            ),
            loose_files: self.loose_files.clone(),
            audio_pair: self.audio_pair.clone(),
            ini_roots: self.ini_roots.clone(),
            active_theater: self.active_theater.clone(),
            active_theater_archives: self.active_theater_archives.clone(),
            ra2_dir: self.ra2_dir.clone(),
        }
    }
}

/// Reversible current-resolution authority for a fallible VERA save candidate.
/// Archive bytes remain shared through MixArchive's retained backing storage.
struct SavedLoadLookupAuthority {
    archives: Vec<NamedArchive>,
    active_theater: Option<String>,
    active_theater_archives: Vec<String>,
}

/// Named nested archives shipped by the active retail RA2/YR install.
///
/// Retail constructs archives by name. It does not scan arbitrary payloads
/// looking for plausible MIX headers; doing so misclassifies SHP and CSF data.
const KNOWN_NESTED_MIX_NAMES: &[&str] = &[
    "audio.mix",
    "audiomd.mix",
    "cache.mix",
    "cachemd.mix",
    "cameo.mix",
    "cameomd.mix",
    "cameos.mix",
    "conquer.mix",
    "conqmd.mix",
    "des.mix",
    "desert.mix",
    "generic.mix",
    "genermd.mix",
    "isodes.mix",
    "isodesmd.mix",
    "isogen.mix",
    "isogenmd.mix",
    "isolun.mix",
    "isolunmd.mix",
    "isosnomd.mix",
    "isosnow.mix",
    "isotemp.mix",
    "isotem.mix",
    "isotemmd.mix",
    "isourb.mix",
    "isourbmd.mix",
    "isoubn.mix",
    "isoubnmd.mix",
    "load.mix",
    "loadmd.mix",
    "local.mix",
    "localmd.mix",
    "lun.mix",
    "lunar.mix",
    "neutral.mix",
    "ntrlmd.mix",
    "sidec01.mix",
    "sidec01md.mix",
    "sidec02.mix",
    "sidec02md.mix",
    "sidenc01.mix",
    "sidenc02.mix",
    "sno.mix",
    "snow.mix",
    "snowmd.mix",
    "tem.mix",
    "temperat.mix",
    "ubn.mix",
    "urb.mix",
    "urban.mix",
    "urbann.mix",
];

impl AssetManager {
    /// Consume a newly assembled provider, then freeze one root choice before
    /// consumers receive it. This never mounts an archive or changes a winner.
    pub fn with_ini_roots(
        mut self,
        config: &crate::util::ini_roots::IniRootConfig,
    ) -> Result<Self, AssetError> {
        self.ini_roots = config
            .resolve(|name| {
                self.resolve_ref(name)
                    .is_some_and(|source| !source.bytes.is_empty())
            })
            .map_err(|detail| AssetError::ParseError {
                format: "INI root selection".into(),
                detail,
            })?;
        log::info!("Selected INI roots: {:?}", self.ini_roots);
        Ok(self)
    }

    /// Recovery preserves the already resolved names, not a fresh Auto probe.
    pub fn with_selected_ini_roots(mut self, roots: crate::util::ini_roots::IniRoots) -> Self {
        self.ini_roots = roots;
        self
    }

    pub fn ini_roots(&self) -> &crate::util::ini_roots::IniRoots {
        &self.ini_roots
    }

    /// Read-only INI tooling without a complete retail installation. No game
    /// archives are implicitly registered; the probe may explicitly use the
    /// existing load_all_disk_mixes widening. Not a gameplay constructor.
    pub fn from_ini_probe_root(ra2_dir: &Path) -> Result<Self, AssetError> {
        Ok(Self {
            archives: Vec::new(),
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: Self::open_loose_root(ra2_dir)?,
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: ra2_dir.to_path_buf(),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_loose_root_for_test(ra2_dir: &Path) -> Self {
        let mut manager = Self::from_ini_probe_root(ra2_dir).expect("open loose test root");
        manager.select_audio_pair();
        manager
    }

    /// Load the core runtime archive stack for one native media-selection mode.
    pub fn new(ra2_dir: &Path, media_mode: MediaArchiveMode) -> Result<Self, AssetError> {
        let mut manager = Self {
            archives: Vec::new(),
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: Self::open_loose_root(ra2_dir)?,
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: ra2_dir.to_path_buf(),
        };

        // WinMain registers the YR language archive before the RA2 fallback.
        manager.mount_named_archive("langmd.mix", false)?;
        manager.mount_named_archive("language.mix", false)?;

        // Init_Base_Mix_Files probes all expansion slots in descending order.
        for index in (0..=99).rev() {
            manager.mount_named_archive(&format!("expandmd{index:02}.mix"), false)?;
        }

        // Active Yuri's Revenge requires both masters, in this order.
        manager.mount_named_archive("ra2md.mix", true)?;
        manager.mount_named_archive("ra2.mix", true)?;

        for name in ["cachemd.mix", "cache.mix", "localmd.mix", "local.mix"] {
            manager.mount_named_archive(name, true)?;
        }

        // AudioSystem__Init uses exactly one archive: the MD archive when
        // present, otherwise the base archive.
        if !manager.mount_named_archive("audiomd.mix", false)? {
            manager.mount_named_archive("audio.mix", false)?;
        }

        manager.select_audio_pair();

        // Init_Mix_Files constructs only these named core archives. Optional
        // constructors are allowed to fail exactly where retail does not gate
        // startup on their result.
        manager.mount_named_archive("conqmd.mix", true)?;
        manager.mount_named_archive("genermd.mix", false)?;
        manager.mount_named_archive("generic.mix", false)?;
        manager.mount_named_archive("isogenmd.mix", false)?;
        manager.mount_named_archive("isogen.mix", false)?;
        manager.mount_named_archive("conquer.mix", true)?;
        manager.mount_named_archive("cameomd.mix", true)?;
        manager.mount_named_archive("cameo.mix", true)?;

        manager.mount_native_map_media(media_mode)?;
        manager.mount_named_archive("multimd.mix", true)?;
        manager.mount_named_archive("thememd.mix", false)?;
        // VERA presentation extension: native YR mounts THEME.MIX only when
        // THEMEMD.MIX is absent. Keep MD first, then make original RA2 music
        // available to the shared Theme catalog without displacing YR files.
        // Preserve this user-requested parity exception during future parity
        // work unless the user explicitly asks to change the music catalog.
        manager.mount_named_archive("theme.mix", false)?;
        manager.mount_native_movie_media(media_mode)?;

        // Loose YRO archives are deliberately absent here. Retail registers
        // each one during SessionClass scenario enumeration, immediately
        // before opening its embedded PKT, and retains that registered object.
        manager.catalog_named_nested_archives();
        log::info!(
            "Retail archive search order ({} registered archives, first match wins):",
            manager.archives.len()
        );
        for (i, named) in manager.archives.iter().enumerate() {
            log::info!(
                "  [{:2}] {} ({} entries)",
                i,
                named.name,
                named.archive.entry_count()
            );
        }

        Ok(manager)
    }

    /// Freeze the independent CCFile winners at native AudioIndex construction.
    /// `Init_Game @ 0x0052BBC7 -> AudioInit @ 0x00406B10 -> AudioIndex @ 0x004011C0` precedes bulk
    /// mounts at 0x0052C59C. `CCFile::Open @ 0x00473D10` chooses loose before registered
    /// MIXes; missing pair members must not become available from later mounts.
    fn select_audio_pair(&mut self) {
        let select = |name: &str| {
            let source = self.resolve_ref(name)?;
            Some(SelectedAudioFile {
                source_archive: source.source_archive.to_owned(),
                entry_id: source.entry_id,
                loose_name: self
                    .loose_asset(name)
                    .filter(|loose| loose.source_name == source.source_archive)
                    .map(|_| name.to_owned()),
            })
        };
        self.audio_pair = SelectedAudioPair {
            idx: select("audio.idx"),
            bag: select("audio.bag"),
        };
    }

    fn selected_audio_ref<'a>(
        &'a self,
        selected: &'a SelectedAudioFile,
    ) -> Option<AssetResolutionRef<'a>> {
        let bytes = if let Some(name) = selected.loose_name.as_deref() {
            self.loose_asset(name)?.bytes()?
        } else {
            self.archives
                .iter()
                .chain(self.archive_catalog.iter())
                .find(|archive| archive.name == selected.source_archive)?
                .archive
                .get_by_id(selected.entry_id)?
        };
        Some(AssetResolutionRef {
            bytes,
            source_archive: &selected.source_archive,
            entry_id: selected.entry_id,
        })
    }

    /// Load the one startup-selected audio index. Call only when launch policy
    /// enables the index: selection retains MIX handles without copying BAG
    /// data, and this method performs parsing and the index's owned BAG copy.
    /// Loose overrides already use the manager's existing byte snapshot.
    /// No second archive or per-sample fallback follows an absent/invalid pair.
    pub fn load_audio_index(&self) -> Result<Option<LoadedAudioIndex>, AssetError> {
        let (Some(idx), Some(bag)) = (&self.audio_pair.idx, &self.audio_pair.bag) else {
            return Ok(None);
        };
        let missing = |source: &SelectedAudioFile| AssetError::AssetNotFound {
            name: format!(
                "retained audio entry {:08X} in {}",
                source.entry_id, source.source_archive
            ),
        };
        let idx_bytes = self.selected_audio_ref(idx).ok_or_else(|| missing(idx))?;
        let bag_bytes = self.selected_audio_ref(bag).ok_or_else(|| missing(bag))?;
        load_audio_index_pair(idx_bytes, bag_bytes).map(Some)
    }

    /// Look up a file by name across all loaded archives.
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        if let Some((loose, bytes)) = self.loose_bytes(name) {
            log::trace!("Found loose '{}' at {}", name, loose.path.display());
            return Some(bytes.to_vec());
        }
        let (named, entry_id) = self.lookup_asset_entry(name)?;
        log::trace!("Found '{}' in {}", name, named.name);
        named.archive.get_by_id(entry_id).map(|data| data.to_vec())
    }

    /// Look up a file by name without copying the asset bytes.
    pub fn get_ref(&self, name: &str) -> Option<&[u8]> {
        if let Some((loose, bytes)) = self.loose_bytes(name) {
            log::trace!("Found loose '{}' at {}", name, loose.path.display());
            return Some(bytes);
        }
        let (named, entry_id) = self.lookup_asset_entry(name)?;
        log::trace!("Found '{}' in {}", name, named.name);
        named.archive.get_by_id(entry_id)
    }

    /// Registered archive fallback for mutable loose-file owners. These callers
    /// read current disk bytes themselves; cached startup loose snapshots must
    /// not resurrect a file they just deleted (e.g. Keyboard Reset All5FBA05).
    pub fn get_archive_ref(&self, name: &str) -> Option<&[u8]> {
        let (named, entry_id) = self.lookup_asset_entry(name)?;
        named.archive.get_by_id(entry_id)
    }

    /// Look up a file by name and return both the bytes and source archive name.
    pub fn get_with_source(&self, name: &str) -> Option<(Vec<u8>, String)> {
        if let Some((loose, bytes)) = self.loose_bytes(name) {
            return Some((bytes.to_vec(), loose.source_name.clone()));
        }
        let (named, entry_id) = self.lookup_asset_entry(name)?;
        named
            .archive
            .get_by_id(entry_id)
            .map(|data| (data.to_vec(), named.name.clone()))
    }

    /// Look up a file by name and return both the borrowed bytes and source archive name.
    pub fn get_with_source_ref(&self, name: &str) -> Option<(&[u8], &str)> {
        let resolved = self.resolve_ref(name)?;
        Some((resolved.bytes, resolved.source_archive))
    }

    /// Resolve one file through the normal first-match archive lookup.
    pub fn resolve_ref(&self, name: &str) -> Option<AssetResolutionRef<'_>> {
        if let Some((loose, bytes)) = self.loose_bytes(name) {
            return Some(AssetResolutionRef {
                bytes,
                source_archive: loose.source_name.as_str(),
                entry_id: mix_hash(name),
            });
        }
        let (named, entry_id) = self.lookup_asset_entry(name)?;
        named
            .archive
            .get_by_id(entry_id)
            .map(|bytes| AssetResolutionRef {
                bytes,
                source_archive: named.name.as_str(),
                entry_id,
            })
    }

    /// Load through retail's `LoadFileFromMIX @ 0x005B40B0` boundary.
    /// Retail provenance: Sticky CRC file cache — `LoadFileFromMIX` @ `0x005B40B0`.
    /// Retail provenance: Availability probing — `CCFileClass__IsAvailable_MixThenRaw` @ `0x00473C50`.
    /// Retail provenance: Raw-before-MIX size selection — `CCFileClass__GetSize_RawThenMix` @ `0x00473C00`.
    /// Retail provenance: Raw-before-MIX byte selection — `CCFileClass__Open_RawThenMix` @ `0x00473D10`.
    /// A cache miss takes loose bytes before registered MIX bytes; that first CRC winner is sticky.
    pub fn load_file_from_mix(&self, name: &str) -> Option<MixFileLoad> {
        let cache_key = mix_hash(name);
        if let Some(cached) = self
            .mix_file_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let candidate = if let Some((loose, bytes)) = self.loose_bytes(name) {
            MixFileLoad {
                bytes: Arc::from(bytes),
                source_archive: Arc::from(loose.source_name.as_str()),
                #[cfg(test)]
                entry_id: cache_key,
            }
        } else {
            let (named, entry_id) = self.lookup_asset_entry(name)?;
            MixFileLoad {
                bytes: Arc::from(named.archive.get_by_id(entry_id)?),
                source_archive: Arc::from(named.name.as_str()),
                #[cfg(test)]
                entry_id,
            }
        };

        // A concurrent first load wins exactly once for this CRC. Returning
        // the occupied value preserves the native cache's CRC-only identity.
        let mut cache = self
            .mix_file_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Some(cache.entry(cache_key).or_insert(candidate).clone())
    }

    /// Register the loading-screen archive pair at the scenario-load boundary.
    ///
    /// Retail constructs the MD archive first and returns immediately when
    /// either archive is unavailable.
    pub fn register_loading_archives(&mut self) -> Result<bool, AssetError> {
        if !self.mount_named_archive("loadmd.mix", false)? {
            return Ok(false);
        }
        if !self.mount_named_archive("load.mix", false)? {
            return Ok(false);
        }
        Ok(true)
    }

    /// Register the neutral shell archive pair at the shell-init boundary.
    ///
    /// `Shell_LoadNeutralMixArchives @ 0x00534E50` constructs `NTRLMD.MIX`
    /// first and returns immediately when it is unavailable, then constructs
    /// `NEUTRAL.MIX`. Their registration order makes MD duplicates win retail's
    /// first-match archive scan.
    pub fn register_neutral_archives(&mut self) -> Result<bool, AssetError> {
        if !self.mount_named_archive("ntrlmd.mix", false)? {
            return Ok(false);
        }
        if !self.mount_named_archive("neutral.mix", false)? {
            return Ok(false);
        }
        Ok(true)
    }

    /// Register an additional named archive at the tail of the search list.
    ///
    /// This matches `MixFileClass__Constructor_Registered @ 0x005B3C20`.
    /// Existing archives retain priority and an already registered name is a
    /// no-op.
    pub fn load_nested(&mut self, name: &str) -> Result<(), AssetError> {
        self.mount_named_archive(name, true).map(|_| ())
    }

    /// Retail provenance: theater archive-group lifecycle — `Init_Theater` @ `0x005349C0`.
    ///
    /// The previous group is destroyed, every optional slot is attempted in
    /// caller order, successful slots are retained, and an unchanged theater
    /// identity is a no-op.
    pub fn activate_theater_archives(
        &mut self,
        theater_name: &str,
        archive_names: &[&str],
    ) -> Result<(), AssetError> {
        if self
            .active_theater
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(theater_name))
        {
            return Ok(());
        }

        if !self.active_theater_archives.is_empty() {
            let old_names = std::mem::take(&mut self.active_theater_archives);
            let mut retained = Vec::with_capacity(self.archives.len());
            for archive in self.archives.drain(..) {
                if old_names
                    .iter()
                    .any(|name| archive_name_matches(&archive.name, name))
                {
                    self.archive_catalog.push(archive);
                } else {
                    retained.push(archive);
                }
            }
            self.archives = retained;
            self.rebuild_indexes();
        }

        for &name in archive_names {
            match self.mount_named_archive(name, false) {
                Ok(true) => self.active_theater_archives.push(name.to_string()),
                Ok(false) => {}
                Err(err) => {
                    log::warn!("Theater {theater_name}: skipping optional archive {name}: {err}");
                }
            }
        }
        self.active_theater = Some(theater_name.to_string());
        Ok(())
    }

    /// Prepare a replacement world without leaving the outgoing world's
    /// current-resolution search list pointed at another theater on error.
    /// Native Load67E440 is not globally atomic; this is VERA's candidate
    /// admission policy. Resident archive bytes and the explicitly process-
    /// sticky LoadFileFromMIX cache survive both outcomes.
    pub fn prepare_saved_load<T, E>(
        &mut self,
        prepare: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let previous = SavedLoadLookupAuthority {
            archives: self.archives.clone(),
            active_theater: self.active_theater.clone(),
            active_theater_archives: self.active_theater_archives.clone(),
        };
        let result = prepare(self);
        if result.is_err() {
            // Keep newly opened files resident but remove their registration.
            // Restored registrations use the exact prior bytes, even if an
            // attempted remount selected another backing file of the same name.
            for archive in std::mem::take(&mut self.archives) {
                if !previous.archives.iter().any(|old| old.name == archive.name)
                    && !self
                        .archive_catalog
                        .iter()
                        .any(|cached| cached.name == archive.name)
                {
                    self.archive_catalog.push(archive);
                }
            }
            self.archive_catalog
                .retain(|cached| !previous.archives.iter().any(|old| old.name == cached.name));
            self.archives = previous.archives;
            self.active_theater = previous.active_theater;
            self.active_theater_archives = previous.active_theater_archives;
            self.rebuild_indexes();
        }
        result
    }

    /// Load every additional top-level `.mix` file present in the RA2 directory.
    ///
    /// This is meant for tooling. Runtime code does not call it.
    pub fn load_all_disk_mixes(&mut self) -> Result<usize, AssetError> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&self.ra2_dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(OsStr::to_str)
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("mix"))
            })
            .collect();
        paths.sort_by(|a, b| {
            a.file_name()
                .unwrap_or_default()
                .cmp(b.file_name().unwrap_or_default())
        });

        let mut loaded_count = 0usize;
        for path in paths {
            let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if self.archive_is_known(file_name) {
                continue;
            }

            let archive = match MixArchive::load(&path) {
                Ok(archive) => archive,
                Err(err) => {
                    log::debug!("Skipping {}: {}", path.display(), err);
                    continue;
                }
            };
            log::info!(
                "Loaded extra disk archive {} ({} entries)",
                file_name,
                archive.entry_count()
            );
            self.append_registered_archive(NamedArchive {
                name: file_name.to_string(),
                archive,
            });
            loaded_count += 1;
        }
        if loaded_count > 0 {
            self.catalog_named_nested_archives();
        }

        Ok(loaded_count)
    }

    /// Check the registered MIX and startup loose-file catalogs without loading
    /// payloads. Catalog presence does not guarantee a subsequent disk read.
    /// Retail provenance: MIX-then-raw availability — `CCFileClass__IsAvailable_MixThenRaw` @ `0x00473C50`.
    pub fn contains(&self, name: &str) -> bool {
        self.lookup_location_for_name(name).is_some() || self.loose_asset(name).is_some()
    }

    /// Look up a loaded archive by its display/debug name.
    pub fn archive(&self, name: &str) -> Option<&MixArchive> {
        self.archive_with_source(name).map(|(_, archive)| archive)
    }

    /// Explicit archive lookup with its full consumed source identity.
    pub fn archive_with_source(&self, name: &str) -> Option<(&str, &MixArchive)> {
        self.archives
            .iter()
            .chain(self.archive_catalog.iter())
            .find(|archive| archive_name_matches(&archive.name, name))
            .map(|archive| (archive.name.as_str(), &archive.archive))
    }

    /// Visit all loaded archives in search order.
    pub fn visit_archives<F>(&self, mut visitor: F)
    where
        F: FnMut(&str, &MixArchive),
    {
        for archive in &self.archives {
            visitor(&archive.name, &archive.archive);
        }
        for archive in &self.archive_catalog {
            visitor(&archive.name, &archive.archive);
        }
    }

    /// Return the names of all loaded archives.
    pub fn loaded_archive_names(&self) -> Vec<String> {
        self.archives
            .iter()
            .chain(self.archive_catalog.iter())
            .map(|archive| archive.name.clone())
            .collect()
    }

    /// Return only the archives that participate in name lookup.
    ///
    /// [`Self::loaded_archive_names`] also reports the catalogued nested
    /// archives, whose contents are parsed and in memory but absent from the
    /// lookup index — `get`/`resolve_ref` cannot reach them. Tools that report
    /// where an asset lives need to tell the two apart, or they present
    /// unreachable bytes as if normal lookup would find them.
    pub fn registered_archive_names(&self) -> Vec<String> {
        self.archives
            .iter()
            .map(|archive| archive.name.clone())
            .collect()
    }

    /// Return the RA2 installation directory.
    pub fn ra2_dir(&self) -> &Path {
        &self.ra2_dir
    }

    /// `RawFile` on a relative path under the install directory, such as
    /// `taunts\tauam01.wav`: Windows opens it whatever the case of each
    /// component, so each one is matched exactly first, then without regard
    /// to ASCII case. Read from disk at each call; not part of the startup
    /// loose catalog, which holds top-level files only.
    pub fn read_install_file(&self, relative: &str) -> Option<Vec<u8>> {
        let mut path = self.ra2_dir.clone();
        for component in relative.split(['\\', '/']).filter(|part| !part.is_empty()) {
            let exact = path.join(component);
            path = if exact.exists() {
                exact
            } else {
                std::fs::read_dir(&path)
                    .ok()?
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .find(|candidate| {
                        candidate
                            .file_name()
                            .and_then(OsStr::to_str)
                            .is_some_and(|name| name.eq_ignore_ascii_case(component))
                    })?
            };
        }
        std::fs::read(&path).ok()
    }

    fn lookup_asset_entry(&self, name: &str) -> Option<(&NamedArchive, i32)> {
        let location = self.lookup_location_for_name(name)?;
        let archive = self.archives.get(location.archive_index)?;
        Some((archive, location.entry_id))
    }

    /// Retail provenance: First-match MIX filename resolution — `MixFileSystem__Resolve_First` @ `0x005B4430`.
    /// Retail provenance: Padded filename CRC — `CRCEngine__AddData` @ `0x004A1DE0`.
    fn lookup_location_for_name(&self, name: &str) -> Option<AssetLocation> {
        self.lookup_index.get(&mix_hash(name)).copied()
    }

    /// Retail provenance: Append-before-tail registration — `MixFileClass` registered constructor @ `0x005B3C20`.
    fn append_registered_archive(&mut self, named: NamedArchive) {
        let archive_index = self.archives.len();
        for entry in named.archive.entries() {
            self.lookup_index.entry(entry.id).or_insert(AssetLocation {
                archive_index,
                entry_id: entry.id,
            });
        }
        log::info!(
            "Registered {} at priority {} ({} entries)",
            named.name,
            archive_index,
            named.archive.entry_count()
        );
        self.archives.push(named);
    }

    fn mount_native_map_media(&mut self, mode: MediaArchiveMode) -> Result<(), AssetError> {
        match mode {
            MediaArchiveMode::Numbered { media_index } => {
                let number = media_index.wrapping_add(1);
                self.mount_named_archive(&format!("mapsmd{number:02}.mix"), true)?;
                self.mount_named_archive(&format!("maps{number:02}.mix"), false)?;
            }
            MediaArchiveMode::CdWildcard => {
                let md = self.mount_wildcard_archives(None, "mapsmd")?;
                if !md.matched {
                    // This is the native fallback branch, even though active
                    // YR still fails the later required-MD global check.
                    self.mount_wildcard_archives(None, "maps")?;
                }
                if !md.first_registered {
                    return Err(AssetError::AssetNotFound {
                        name: "MAPSMD*.MIX".to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn mount_native_movie_media(&mut self, mode: MediaArchiveMode) -> Result<(), AssetError> {
        match mode {
            MediaArchiveMode::Numbered { media_index } => {
                let number = media_index.wrapping_add(1);
                if !self.mount_named_archive(&format!("movmd{number:02}.mix"), false)? {
                    self.mount_named_archive(&format!("movies{number:02}.mix"), true)?;
                }
            }
            MediaArchiveMode::CdWildcard => {
                // Retail starts with root MOVMD, then each successful raw-file
                // probe overwrites the selected wildcard in this exact order.
                let mut directory = None;
                let mut family = "movmd";
                for (probe, selected_directory, selected_family) in [
                    ("MOVIES01.MIX", None, "movies"),
                    ("MIXFILES/MOVIES01.MIX", Some("MIXFILES"), "movies"),
                    ("MOVMD01.MIX", None, "movmd"),
                    ("MIXFILES/MOVMD01.MIX", Some("MIXFILES"), "movmd"),
                    ("MOVMD03.MIX", None, "movmd"),
                    ("MIXFILES/MOVMD03.MIX", Some("MIXFILES"), "movmd"),
                ] {
                    if self.raw_relative_file_available(probe) {
                        directory = selected_directory;
                        family = selected_family;
                    }
                }

                let mut mounted = self.mount_wildcard_archives(directory, family)?;
                if !mounted.matched {
                    mounted = self.mount_wildcard_archives(None, "movies")?;
                }
                if !mounted.first_registered {
                    return Err(AssetError::AssetNotFound {
                        name: if family == "movmd" {
                            "MOVMD*.MIX".to_string()
                        } else {
                            "MOVIES*.MIX".to_string()
                        },
                    });
                }
            }
        }
        Ok(())
    }

    fn mount_wildcard_archives(
        &mut self,
        relative_directory: Option<&str>,
        family_prefix: &str,
    ) -> Result<WildcardMount, AssetError> {
        let directory = relative_directory
            .map(|relative| self.ra2_dir.join(relative))
            .unwrap_or_else(|| self.ra2_dir.clone());
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WildcardMount::default());
            }
            Err(err) => return Err(err.into()),
        };

        let mut result = WildcardMount::default();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            if !retail_enumerated_file(&path) {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if !mix_wildcard_family_matches(file_name, family_prefix) {
                continue;
            }

            let first_match = !result.matched;
            result.matched = true;
            let display_name = relative_directory
                .map(|relative| format!("{relative}\\{file_name}"))
                .unwrap_or_else(|| file_name.to_string());
            let registered = self.try_register_disk_archive(&path, &display_name);
            if first_match {
                result.first_registered = registered;
            }
        }
        Ok(result)
    }

    fn try_register_disk_archive(&mut self, path: &Path, display_name: &str) -> bool {
        if self.archive_is_known(display_name) {
            return true;
        }
        let archive = match MixArchive::load(path) {
            Ok(archive) => archive,
            Err(err) => {
                log::debug!("Skipping media archive {}: {}", path.display(), err);
                return false;
            }
        };
        self.append_registered_archive(NamedArchive {
            name: display_name.to_string(),
            archive,
        });
        true
    }

    fn raw_relative_file_available(&self, relative: &str) -> bool {
        retail_enumerated_file(&self.ra2_dir.join(relative))
    }

    fn mount_named_archive(&mut self, name: &str, required: bool) -> Result<bool, AssetError> {
        if self
            .archives
            .iter()
            .any(|archive| archive_name_matches(&archive.name, name))
        {
            return Ok(true);
        }

        if let Some(disk_path) = self.loose_path(name).map(Path::to_path_buf) {
            let archive = MixArchive::load(&disk_path)?;
            if let Some(position) = self
                .archive_catalog
                .iter()
                .position(|catalogued| archive_name_matches(&catalogued.name, name))
            {
                self.archive_catalog.remove(position);
            }
            self.append_registered_archive(NamedArchive {
                name: name.to_string(),
                archive,
            });
            return Ok(true);
        }

        if let Some(position) = self
            .archive_catalog
            .iter()
            .position(|archive| archive_name_matches(&archive.name, name))
        {
            let archive = self.archive_catalog.remove(position);
            self.append_registered_archive(archive);
            return Ok(true);
        }

        if let Some((parent_index, entry_id)) = self
            .lookup_location_for_name(name)
            .map(|location| (location.archive_index, location.entry_id))
        {
            let parent_name = self.archives[parent_index].name.clone();
            let archive = self.archives[parent_index]
                .archive
                .nested_archive_by_id(entry_id)?
                .ok_or_else(|| AssetError::AssetNotFound {
                    name: name.to_string(),
                })?;
            self.append_registered_archive(NamedArchive {
                name: format!("{parent_name} -> {name}"),
                archive,
            });
            return Ok(true);
        }

        if required {
            Err(AssetError::AssetNotFound {
                name: name.to_string(),
            })
        } else {
            log::debug!("{name} not found (optional, skipping)");
            Ok(false)
        }
    }

    fn catalog_named_nested_archives(&mut self) {
        let mut discovered = Vec::new();

        for parent in &self.archives {
            for &nested_name in KNOWN_NESTED_MIX_NAMES {
                if self.archive_is_known(nested_name)
                    || discovered.iter().any(|archive: &NamedArchive| {
                        archive_name_matches(&archive.name, nested_name)
                    })
                {
                    continue;
                }

                let entry_id = mix_hash(nested_name);
                let Ok(Some(archive)) = parent.archive.nested_archive_by_id(entry_id) else {
                    continue;
                };
                if archive.entry_count() == 0 {
                    continue;
                }
                discovered.push(NamedArchive {
                    name: format!("{} -> {nested_name}", parent.name),
                    archive,
                });
            }
        }

        if !discovered.is_empty() {
            log::info!(
                "Catalogued {} unregistered named nested archives",
                discovered.len()
            );
            self.archive_catalog.extend(discovered);
        }
    }

    /// Register one loose YRO at its native scenario-enumeration point.
    ///
    /// `SessionClass__ScanMultiplayerMapFiles @ 0x00699980` performs this
    /// registration before deriving and opening the archive's embedded PKT.
    pub(crate) fn register_loose_yro_archive(&mut self, path: &Path) -> Result<bool, AssetError> {
        if !retail_enumerated_file(path)
            || !path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("yro"))
        {
            return Ok(false);
        }
        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            return Ok(false);
        };
        if self.archive_is_known(file_name) {
            return Ok(true);
        }

        let archive = MixArchive::load(path)?;
        self.append_registered_archive(NamedArchive {
            name: file_name.to_string(),
            archive,
        });
        Ok(true)
    }

    fn archive_is_known(&self, name: &str) -> bool {
        self.archives
            .iter()
            .chain(self.archive_catalog.iter())
            .any(|archive| archive_name_matches(&archive.name, name))
    }

    fn open_loose_root(ra2_dir: &Path) -> Result<HashMap<String, LooseAsset>, AssetError> {
        let mut files = HashMap::new();

        for entry in std::fs::read_dir(ra2_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            let key = file_name.to_ascii_lowercase();
            if files.contains_key(&key) {
                continue;
            }

            files.insert(
                key,
                LooseAsset {
                    path: path.clone(),
                    source_name: format!("loose:{}", path.display()),
                    bytes: OnceLock::new(),
                },
            );
        }

        Ok(files)
    }

    fn loose_asset(&self, name: &str) -> Option<&LooseAsset> {
        let key = loose_lookup_key(name)?;
        self.loose_files.get(&key)
    }

    /// Unreadable catalogued files must not hide a readable MIX fallback.
    fn loose_bytes(&self, name: &str) -> Option<(&LooseAsset, &[u8])> {
        let asset = self.loose_asset(name)?;
        Some((asset, asset.bytes()?))
    }

    fn loose_path(&self, name: &str) -> Option<&Path> {
        self.loose_asset(name).map(|asset| asset.path.as_path())
    }

    fn rebuild_indexes(&mut self) {
        self.lookup_index.clear();

        for (archive_index, named) in self.archives.iter().enumerate() {
            for entry in named.archive.entries() {
                self.lookup_index.entry(entry.id).or_insert(AssetLocation {
                    archive_index,
                    entry_id: entry.id,
                });
            }
        }
    }
}

fn archive_name_matches(stored_name: &str, requested_name: &str) -> bool {
    archive_lookup_keys(stored_name)
        .iter()
        .any(|key| key.eq_ignore_ascii_case(requested_name))
}

fn loose_lookup_key(name: &str) -> Option<String> {
    let path = Path::new(name);
    if path
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return None;
    }
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}

/// Whether a retail directory listing keeps `path`: a regular file without the
/// hidden, system, directory or temporary attribute (`0x116`). The asset search
/// and the scenario listings share it.
pub(crate) fn retail_enumerated_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x116 == 0
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn mix_wildcard_family_matches(file_name: &str, family_prefix: &str) -> bool {
    let path = Path::new(file_name);
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mix"))
        && path
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|stem| {
                stem.get(..family_prefix.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(family_prefix))
            })
}

fn archive_lookup_keys(name: &str) -> Vec<String> {
    let mut keys = Vec::with_capacity(3);
    keys.push(name.to_ascii_lowercase());

    if let Some(rest) = name.strip_prefix("nested:") {
        keys.push(rest.trim().to_ascii_lowercase());
    }
    if let Some((_, leaf)) = name.rsplit_once("->") {
        keys.push(leaf.trim().to_ascii_lowercase());
    }

    keys.sort();
    keys.dedup();
    keys
}

/// Install-directory fixtures, shared with client tests through `test-support`.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::assets::mix_hash::mix_hash;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    pub struct TestDirectory(PathBuf);

    impl TestDirectory {
        pub fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-asset-manager-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write_mix(&self, archive_name: &str, entry_name: &str, body: &[u8]) {
            let path = self.0.join(archive_name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create archive parent");
            }
            std::fs::write(path, make_new_format_mix_bytes(entry_name, body))
                .expect("write test MIX");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn make_new_format_mix_bytes(name: &str, body: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        let entry_id = mix_hash(name);

        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&entry_id.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(body);
        data
    }

    pub fn audio_test_install(label: &str) -> TestDirectory {
        let dir = TestDirectory::new(label);
        for name in [
            "ra2md.mix",
            "ra2.mix",
            "cachemd.mix",
            "cache.mix",
            "localmd.mix",
            "local.mix",
            "conqmd.mix",
            "conquer.mix",
            "cameomd.mix",
            "cameo.mix",
            "mapsmd03.mix",
            "multimd.mix",
            "movmd03.mix",
        ] {
            dir.write_mix(name, "unrelated.bin", b"fixture");
        }
        dir
    }

    pub fn write_audio_archive(dir: &TestDirectory, name: &str, idx: &[u8], bag: &[u8]) {
        std::fs::write(
            dir.path().join(name),
            crate::map::source::test_support::make_new_format_mix_bytes(&[
                ("audio.idx", idx),
                ("audio.bag", bag),
            ]),
        )
        .expect("write audio archive");
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        TestDirectory, audio_test_install, make_new_format_mix_bytes, write_audio_archive,
    };
    use super::*;

    fn empty_manager(ra2_dir: &Path) -> AssetManager {
        AssetManager {
            archives: Vec::new(),
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: AssetManager::open_loose_root(ra2_dir).expect("open loose test root"),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: ra2_dir.to_path_buf(),
        }
    }

    fn make_new_format_mix(name: &str, body: &[u8]) -> MixArchive {
        MixArchive::from_bytes(make_new_format_mix_bytes(name, body))
            .expect("new-format test mix should parse")
    }

    #[test]
    fn audio_pair_cannot_appear_from_a_later_bulk_mount() {
        let dir = audio_test_install("late-audio");
        let idx = crate::assets::audio_bag::test_support::build_idx(&[("late", 0, 2, 22050, 4)]);
        write_audio_archive(&dir, "conqmd.mix", &idx, &[1, 2]);
        let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        assert!(
            manager.resolve_ref("audio.idx").is_some(),
            "bulk mount is live"
        );
        assert!(
            manager.load_audio_index().unwrap().is_none(),
            "startup pair stays absent"
        );
    }

    #[test]
    fn ra2_music_archive_preserves_yr_winners_and_optional_fallback() {
        for (has_md, has_base) in [(true, true), (false, true), (true, false), (false, false)] {
            let dir = audio_test_install("theme-archives");
            if has_md {
                dir.write_mix("thememd.mix", "shared.wav", b"YR music");
            }
            if has_base {
                dir.write_mix("THEME.MIX", "shared.wav", b"RA2 music");
            }
            let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            let names = manager.registered_archive_names();
            assert_eq!(
                names.iter().filter(|name| *name == "thememd.mix").count(),
                usize::from(has_md)
            );
            assert_eq!(
                names.iter().filter(|name| *name == "theme.mix").count(),
                usize::from(has_base)
            );
            match manager.resolve_ref("shared.wav") {
                Some(source) if has_md => {
                    assert_eq!(source.source_archive, "thememd.mix");
                    assert_eq!(source.bytes, b"YR music");
                }
                Some(source) if has_base => {
                    assert_eq!(source.source_archive, "theme.mix");
                    assert_eq!(source.bytes, b"RA2 music");
                }
                None => assert!(!has_md && !has_base),
                _ => panic!("unexpected music source"),
            }
        }
    }

    #[test]
    fn audio_pair_members_select_independently_and_keep_consumed_loose_bytes() {
        for loose_idx in [false, true] {
            let dir = audio_test_install("split-audio");
            let idx =
                crate::assets::audio_bag::test_support::build_idx(&[("sample", 0, 2, 22050, 4)]);
            let bag = [1, 2];
            let (loose_name, loose_bytes, mixed_name, mixed_bytes): (&str, &[u8], &str, &[u8]) =
                if loose_idx {
                    ("audio.idx", &idx, "audio.bag", &bag)
                } else {
                    ("audio.bag", &bag, "audio.idx", &idx)
                };
            std::fs::write(dir.path().join(loose_name), loose_bytes).unwrap();
            dir.write_mix("audiomd.mix", mixed_name, mixed_bytes);
            let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            // The retained loose snapshot is the consumed startup source, even
            // if the physical file changes before the process requests parsing.
            std::fs::write(dir.path().join(loose_name), b"changed after startup").unwrap();
            let loaded = manager.load_audio_index().unwrap().unwrap();
            assert!(loaded.index.get("sample").is_some());
            assert_eq!(
                loaded.sources.idx.source_sha256,
                crate::util::sha256::sha256_hex(&idx)
            );
            assert_eq!(
                loaded.sources.bag.source_sha256,
                crate::util::sha256::sha256_hex(&bag)
            );
            assert_eq!(loaded.sources.idx.payload_len, idx.len());
            assert_eq!(loaded.sources.bag.payload_len, bag.len());
            assert_eq!(loaded.sources.idx.entry_id, mix_hash("audio.idx"));
            assert_eq!(loaded.sources.bag.entry_id, mix_hash("audio.bag"));
            let (loose, mixed) = if loose_idx {
                (&loaded.sources.idx, &loaded.sources.bag)
            } else {
                (&loaded.sources.bag, &loaded.sources.idx)
            };
            assert_eq!(
                loose.source_archive,
                format!("loose:{}", dir.path().join(loose_name).display())
            );
            assert_eq!(mixed.source_archive, "audiomd.mix");
        }
    }

    #[test]
    fn malformed_selected_audio_index_does_not_retry_base_archive() {
        let dir = audio_test_install("invalid-audio");
        let base =
            crate::assets::audio_bag::test_support::build_idx(&[("base_only", 0, 2, 22050, 4)]);
        write_audio_archive(&dir, "audiomd.mix", b"truncated", &[1, 2]);
        write_audio_archive(&dir, "audio.mix", &base, &[3, 4]);
        let manager = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        assert!(matches!(
            manager.load_audio_index(),
            Err(AssetError::ParseError { .. })
        ));
    }

    #[test]
    fn install_files_resolve_each_component_without_regard_to_case() {
        let dir = TestDirectory::new("install-subdirectory");
        std::fs::create_dir(dir.path().join("Taunts")).expect("subdirectory");
        std::fs::write(dir.path().join("Taunts").join("TAUAM01.WAV"), b"am").expect("file");
        std::fs::write(dir.path().join("Taunts").join("tauyu01.wav"), b"yu").expect("file");
        let manager = empty_manager(dir.path());
        assert_eq!(
            manager.read_install_file("taunts\\tauam01.wav").as_deref(),
            Some(&b"am"[..])
        );
        assert_eq!(
            manager.read_install_file("TAUNTS/TAUYU01.WAV").as_deref(),
            Some(&b"yu"[..])
        );
        assert_eq!(manager.read_install_file("taunts\\tauam02.wav"), None);
        assert!(
            !manager.contains("taunts\\tauam01.wav"),
            "the startup loose catalog stays top-level"
        );
    }

    #[test]
    fn loose_payloads_are_lazy_stable_and_do_not_lock_writable_files() {
        let dir = TestDirectory::new("mutable-loose");
        for name in ["SAVE0001.SED", "RandMap.Sed", "RandMap.img", "RandMap.PCX"] {
            std::fs::write(dir.path().join(name), b"old payload").expect("initial file");
        }
        std::fs::write(dir.path().join("empty.bin"), b"").expect("empty file");
        let manager = empty_manager(dir.path());
        assert!(manager.contains("RANDMAP.PCX"));
        assert!(
            manager
                .loose_files
                .values()
                .all(|file| file.bytes.get().is_none())
        );

        for name in ["SAVE0001.SED", "RandMap.Sed", "RandMap.img", "RandMap.PCX"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"first lookup").expect("write after catalog creation");
            let borrowed = manager.get_ref(name).expect("loose payload");
            assert_eq!(borrowed, b"first lookup");
            std::fs::write(&path, b"new").expect("truncate while borrowed payload lives");
            assert_eq!(std::fs::read(&path).expect("current disk bytes"), b"new");
            assert_eq!(borrowed, b"first lookup", "owned borrow remains stable");
            assert_eq!(manager.get_ref(name), Some(borrowed));
        }
        assert_eq!(manager.get_ref("empty.bin"), Some(&b""[..]));
    }

    #[test]
    fn unreadable_loose_snapshot_falls_through_all_byte_apis_to_mix() {
        let dir = TestDirectory::new("unreadable-loose");
        let path = dir.path().join("duplicate.shp");
        std::fs::write(&path, b"loose").expect("initial loose file");
        let mut manager = empty_manager(dir.path());
        manager.archives.push(NamedArchive {
            name: "first.mix".to_string(),
            archive: make_new_format_mix("duplicate.shp", b"archived"),
        });
        manager.rebuild_indexes();
        std::fs::remove_file(&path).expect("remove before first payload read");
        assert_eq!(
            manager.get("duplicate.shp").as_deref(),
            Some(&b"archived"[..])
        );
        assert_eq!(manager.get_ref("duplicate.shp"), Some(&b"archived"[..]));
        assert_eq!(
            manager.get_with_source("duplicate.shp"),
            Some((b"archived".to_vec(), "first.mix".to_string()))
        );
        assert_eq!(
            manager.get_with_source_ref("duplicate.shp"),
            Some((&b"archived"[..], "first.mix"))
        );
        let resolved = manager.resolve_ref("duplicate.shp").expect("MIX fallback");
        assert_eq!(resolved.bytes, b"archived");
        assert_eq!(resolved.source_archive, "first.mix");
        let cached = manager
            .load_file_from_mix("duplicate.shp")
            .expect("sticky fallback");
        assert_eq!(&*cached.bytes, b"archived");
        assert_eq!(&*cached.source_archive, "first.mix");
    }

    #[test]
    fn numbered_media_uses_media_plus_one_and_movie_family_fallback() {
        let directory = TestDirectory::new("numbered-media");
        directory.write_mix("MAPSMD03.MIX", "md-map.bin", b"md");
        directory.write_mix("MAPS03.MIX", "base-map.bin", b"base");
        directory.write_mix("MOVIES03.MIX", "movie.bin", b"movie");

        let mut manager = empty_manager(directory.path());
        let mode = MediaArchiveMode::Numbered { media_index: 2 };
        manager
            .mount_native_map_media(mode)
            .expect("numbered map media");
        manager
            .mount_native_movie_media(mode)
            .expect("numbered movie fallback");

        assert_eq!(
            manager.loaded_archive_names(),
            vec![
                "mapsmd03.mix".to_string(),
                "maps03.mix".to_string(),
                "movies03.mix".to_string(),
            ]
        );
    }

    #[test]
    fn cd_map_wildcard_uses_md_family_and_preserves_enumeration_order() {
        let directory = TestDirectory::new("cd-map-media");
        directory.write_mix("MAPSMD01.MIX", "one.bin", b"one");
        directory.write_mix("mapsmd03.mix", "three.bin", b"three");
        directory.write_mix("MAPS01.MIX", "base.bin", b"base");

        let expected: Vec<String> = std::fs::read_dir(directory.path())
            .expect("enumerate expected order")
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_name = entry.file_name().into_string().ok()?;
                mix_wildcard_family_matches(&file_name, "mapsmd").then_some(file_name)
            })
            .collect();

        let mut manager = empty_manager(directory.path());
        manager
            .mount_native_map_media(MediaArchiveMode::CdWildcard)
            .expect("CD wildcard map media");

        assert_eq!(manager.loaded_archive_names(), expected);
        assert!(
            manager
                .loaded_archive_names()
                .iter()
                .all(|name| name.to_ascii_lowercase().starts_with("mapsmd"))
        );
    }

    #[test]
    fn cd_movie_probe_order_selects_movmd_before_movies_family() {
        let directory = TestDirectory::new("cd-movie-media");
        directory.write_mix("MOVIES01.MIX", "base-movie.bin", b"base");
        directory.write_mix("MOVMD03.MIX", "md-three.bin", b"three");
        directory.write_mix("MOVMD04.MIX", "md-four.bin", b"four");

        let expected: Vec<String> = std::fs::read_dir(directory.path())
            .expect("enumerate expected order")
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_name = entry.file_name().into_string().ok()?;
                mix_wildcard_family_matches(&file_name, "movmd").then_some(file_name)
            })
            .collect();

        let mut manager = empty_manager(directory.path());
        manager
            .mount_native_movie_media(MediaArchiveMode::CdWildcard)
            .expect("CD wildcard movie media");

        assert_eq!(manager.loaded_archive_names(), expected);
    }

    #[test]
    fn loose_yro_is_absent_until_scenario_time_registration() {
        let directory = TestDirectory::new("scenario-yro");
        directory.write_mix("CUSTOM.YRO", "custom.map", b"[Basic]\nName=Custom");
        let yro_path = directory.path().join("CUSTOM.YRO");

        let mut manager = empty_manager(directory.path());
        assert!(manager.lookup_asset_entry("custom.map").is_none());

        assert!(
            manager
                .register_loose_yro_archive(&yro_path)
                .expect("register scenario YRO")
        );
        assert_eq!(
            manager.get_ref("custom.map"),
            Some(&b"[Basic]\nName=Custom"[..])
        );
    }

    #[test]
    fn rejected_saved_load_restores_lookup_authority_and_keeps_resident_bytes() {
        let directory = TestDirectory::new("saved-theater-rollback");
        directory.write_mix("temperat.mix", "tile.bin", b"temperate");
        directory.write_mix("snow.mix", "tile.bin", b"snow");
        let mut manager = empty_manager(directory.path());
        manager
            .activate_theater_archives("TEMPERATE", &["temperat.mix"])
            .unwrap();
        let before = manager.registered_archive_names();
        let result: Result<(), &'static str> = manager.prepare_saved_load(|candidate| {
            candidate
                .activate_theater_archives("SNOW", &["snow.mix"])
                .unwrap();
            assert_eq!(candidate.get_ref("tile.bin"), Some(&b"snow"[..]));
            Err("invalid restored object reference after theater binding")
        });
        assert!(result.is_err());
        assert_eq!(manager.registered_archive_names(), before);
        assert_eq!(manager.get_ref("tile.bin"), Some(&b"temperate"[..]));
        assert_eq!(manager.active_theater.as_deref(), Some("TEMPERATE"));
        assert!(
            manager
                .archive_catalog
                .iter()
                .any(|entry| archive_name_matches(&entry.name, "snow.mix"))
        );

        manager
            .prepare_saved_load(|candidate| {
                candidate.activate_theater_archives("SNOW", &["snow.mix"])
            })
            .unwrap();
        assert_eq!(manager.get_ref("tile.bin"), Some(&b"snow"[..]));
        assert_eq!(manager.active_theater.as_deref(), Some("SNOW"));
    }

    #[test]
    fn loading_archives_register_md_then_base_once_at_resource_setup() {
        let mut manager = AssetManager {
            archives: vec![NamedArchive {
                name: "base.mix".to_string(),
                archive: make_new_format_mix("base.bin", b"base"),
            }],
            // Deliberately reverse catalogue order: the setup function's named
            // construction order, not discovery order, controls registration.
            archive_catalog: vec![
                NamedArchive {
                    name: "master.mix -> load.mix".to_string(),
                    archive: make_new_format_mix("loading.bin", b"base-loading"),
                },
                NamedArchive {
                    name: "master.mix -> loadmd.mix".to_string(),
                    archive: make_new_format_mix("loading.bin", b"md-loading"),
                },
            ],
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        assert!(
            manager
                .register_loading_archives()
                .expect("register loading archives")
        );
        assert_eq!(
            manager.loaded_archive_names(),
            vec![
                "base.mix".to_string(),
                "master.mix -> loadmd.mix".to_string(),
                "master.mix -> load.mix".to_string(),
            ]
        );
        assert_eq!(manager.get_ref("loading.bin"), Some(&b"md-loading"[..]));

        assert!(
            manager
                .register_loading_archives()
                .expect("repeat loading setup")
        );
        assert_eq!(manager.archives.len(), 3);
    }

    #[test]
    fn loading_archive_setup_returns_at_each_missing_archive_boundary() {
        let directory = TestDirectory::new("loading-archive-failure");
        let mut missing_md = empty_manager(directory.path());
        missing_md.archive_catalog.push(NamedArchive {
            name: "master.mix -> load.mix".to_string(),
            archive: make_new_format_mix("base-loading.bin", b"base"),
        });
        assert!(
            !missing_md
                .register_loading_archives()
                .expect("missing MD archive is not an I/O failure")
        );
        assert!(missing_md.archives.is_empty());

        let mut missing_base = empty_manager(directory.path());
        missing_base.archive_catalog.push(NamedArchive {
            name: "master.mix -> loadmd.mix".to_string(),
            archive: make_new_format_mix("md-loading.bin", b"md"),
        });
        assert!(
            !missing_base
                .register_loading_archives()
                .expect("missing base archive is not an I/O failure")
        );
        assert_eq!(
            missing_base.loaded_archive_names(),
            vec!["master.mix -> loadmd.mix".to_string()]
        );
    }

    #[test]
    fn load_file_from_mix_cache_survives_theater_replacement_and_normalizes_case() {
        let mut manager = AssetManager {
            archives: vec![NamedArchive {
                name: "base.mix".to_string(),
                archive: make_new_format_mix("base.bin", b"base"),
            }],
            archive_catalog: vec![
                NamedArchive {
                    name: "master.mix -> snow.mix".to_string(),
                    archive: make_new_format_mix("weather.bin", b"snow"),
                },
                NamedArchive {
                    name: "master.mix -> temperat.mix".to_string(),
                    archive: make_new_format_mix("weather.bin", b"temperate"),
                },
            ],
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();
        manager
            .activate_theater_archives("SNOW", &["snow.mix"])
            .expect("activate snow");

        let first = manager
            .load_file_from_mix("Weather.Bin")
            .expect("first MIX load");
        assert_eq!(&*first.bytes, b"snow");

        manager
            .activate_theater_archives("TEMPERATE", &["temperat.mix"])
            .expect("replace theater");
        assert_eq!(
            manager
                .resolve_ref("WEATHER.BIN")
                .map(|result| result.bytes),
            Some(&b"temperate"[..])
        );

        let cached = manager
            .load_file_from_mix("WEATHER.BIN")
            .expect("cached MIX load");
        assert_eq!(&*cached.bytes, b"snow");
        assert!(Arc::ptr_eq(&first.bytes, &cached.bytes));
        assert_eq!(cached.source_archive.as_ref(), "master.mix -> snow.mix");
    }

    #[test]
    fn first_registered_crc_duplicate_wins() {
        let directory = TestDirectory::new("first-registered-crc-duplicate");
        let mut manager = empty_manager(directory.path());
        manager.append_registered_archive(NamedArchive {
            name: "first.mix".to_string(),
            archive: make_new_format_mix("audio.idx", b"first"),
        });
        manager.append_registered_archive(NamedArchive {
            name: "second.mix".to_string(),
            archive: make_new_format_mix("audio.idx", b"second"),
        });

        let (bytes, source) = manager
            .get_with_source("audio.idx")
            .expect("indexed lookup should find audio.idx");
        assert_eq!(bytes, b"first");
        assert_eq!(source, "first.mix");

        let resolved = manager
            .resolve_ref("audio.idx")
            .expect("observational resolution should preserve first match");
        assert_eq!(resolved.bytes, b"first");
        assert_eq!(resolved.source_archive, "first.mix");
        assert_eq!(resolved.entry_id, mix_hash("audio.idx"));
    }

    #[test]
    fn archive_lookup_matches_nested_leaf_aliases() {
        let mut manager = AssetManager {
            archives: vec![NamedArchive {
                name: "language.mix -> audio.mix".to_string(),
                archive: make_new_format_mix("audio.idx", b"test"),
            }],
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        assert!(manager.archive("audio.mix").is_some());
        assert!(manager.archive("language.mix -> audio.mix").is_some());
    }

    #[test]
    fn neutral_sidebar_mix_names_match_retail_hashes_and_catalog() {
        let expected = [("sidenc01.mix", 0x330A_4ADF), ("sidenc02.mix", 0x74AA_300F)];

        for (name, entry_id) in expected {
            assert_eq!(mix_hash(name), entry_id);
            assert!(KNOWN_NESTED_MIX_NAMES.contains(&name));
        }
    }

    #[test]
    fn registering_nested_archive_appends_without_displacing_first_winner() {
        let nested_bytes = make_new_format_mix_bytes("duplicate.bin", b"late");
        let mut manager = AssetManager {
            archives: vec![
                NamedArchive {
                    name: "early.mix".to_string(),
                    archive: make_new_format_mix("duplicate.bin", b"early"),
                },
                NamedArchive {
                    name: "master.mix".to_string(),
                    archive: make_new_format_mix("audio.mix", &nested_bytes),
                },
            ],
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();
        manager.catalog_named_nested_archives();

        manager
            .load_nested("audio.mix")
            .expect("named child archive should register");

        assert_eq!(manager.loaded_archive_names()[2], "master.mix -> audio.mix");
        let (bytes, source) = manager
            .get_with_source("duplicate.bin")
            .expect("duplicate should resolve");
        assert_eq!(bytes, b"early");
        assert_eq!(source, "early.mix");
    }

    #[test]
    fn neutral_archive_registration_preserves_md_first_winner() {
        let mut manager = AssetManager {
            archives: Vec::new(),
            archive_catalog: vec![
                NamedArchive {
                    name: "ra2md.mix -> ntrlmd.mix".to_string(),
                    archive: make_new_format_mix("duplicate.shp", b"yr"),
                },
                NamedArchive {
                    name: "ra2.mix -> neutral.mix".to_string(),
                    archive: make_new_format_mix("duplicate.shp", b"base"),
                },
            ],
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        assert!(
            manager
                .register_neutral_archives()
                .expect("register neutral pair")
        );
        assert_eq!(
            manager.loaded_archive_names(),
            [
                "ra2md.mix -> ntrlmd.mix".to_string(),
                "ra2.mix -> neutral.mix".to_string(),
            ]
        );

        let loaded = manager
            .load_file_from_mix("DUPLICATE.SHP")
            .expect("registered neutral asset");
        assert_eq!(&*loaded.bytes, b"yr");
        assert_eq!(loaded.source_archive.as_ref(), "ra2md.mix -> ntrlmd.mix");
    }

    #[test]
    fn neutral_archive_registration_stops_when_md_archive_is_missing() {
        let mut manager = AssetManager {
            archives: Vec::new(),
            archive_catalog: vec![NamedArchive {
                name: "ra2.mix -> neutral.mix".to_string(),
                archive: make_new_format_mix("base-only.shp", b"base"),
            }],
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        assert!(
            !manager
                .register_neutral_archives()
                .expect("missing MD archive is the native optional path")
        );
        assert!(manager.load_file_from_mix("BASE-ONLY.SHP").is_none());
    }

    #[test]
    fn load_file_from_mix_fills_from_loose_before_mix_and_stays_sticky() {
        let mut manager = AssetManager {
            archives: vec![NamedArchive {
                name: "first.mix".to_string(),
                archive: make_new_format_mix("ra2md.csf", b"archived"),
            }],
            archive_catalog: Vec::new(),
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::from([(
                "ra2md.csf".to_string(),
                LooseAsset {
                    path: PathBuf::from("RA2MD.CSF"),
                    source_name: "loose:RA2MD.CSF".to_string(),
                    bytes: OnceLock::from(Some(Box::from(&b"loose"[..]))),
                },
            )]),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        assert!(manager.contains("Ra2Md.CsF"));
        let (bytes, source) = manager
            .get_with_source("Ra2Md.CsF")
            .expect("case-insensitive loose file should resolve");
        assert_eq!(bytes, b"loose");
        assert_eq!(source, "loose:RA2MD.CSF");

        let first = manager
            .load_file_from_mix("RA2MD.CSF")
            .expect("loose file should fill cache before registered MIX");
        assert_eq!(&*first.bytes, b"loose");
        assert_eq!(first.source_archive.as_ref(), "loose:RA2MD.CSF");
        assert_eq!(first.entry_id, mix_hash("RA2MD.CSF"));

        manager.loose_files.clear();
        assert_eq!(
            manager.resolve_ref("RA2MD.CSF").map(|found| found.bytes),
            Some(&b"archived"[..])
        );
        let cached = manager
            .load_file_from_mix("ra2md.csf")
            .expect("cached loose winner");
        assert_eq!(&*cached.bytes, b"loose");
        assert!(Arc::ptr_eq(&first.bytes, &cached.bytes));
        assert_eq!(cached.source_archive.as_ref(), "loose:RA2MD.CSF");
    }

    #[test]
    fn theater_group_replaces_old_archives_and_repeated_identity_is_noop() {
        let mut manager = AssetManager {
            archives: vec![NamedArchive {
                name: "base.mix".to_string(),
                archive: make_new_format_mix("base.bin", b"base"),
            }],
            archive_catalog: vec![
                NamedArchive {
                    name: "master.mix -> snow.mix".to_string(),
                    archive: make_new_format_mix("snow.bin", b"snow"),
                },
                NamedArchive {
                    name: "master.mix -> temperat.mix".to_string(),
                    archive: make_new_format_mix("temperate.bin", b"temperate"),
                },
            ],
            lookup_index: HashMap::new(),
            mix_file_cache: Mutex::new(HashMap::new()),
            loose_files: HashMap::new(),
            audio_pair: SelectedAudioPair::default(),
            ini_roots: Default::default(),
            active_theater: None,
            active_theater_archives: Vec::new(),
            ra2_dir: PathBuf::new(),
        };
        manager.rebuild_indexes();

        manager
            .activate_theater_archives("SNOW", &["snow.mix"])
            .expect("snow group");
        assert!(manager.contains("snow.bin"));
        assert_eq!(manager.archives.len(), 2);

        manager
            .activate_theater_archives("snow", &["snow.mix"])
            .expect("same theater is a no-op");
        assert_eq!(manager.archives.len(), 2);

        manager
            .activate_theater_archives("TEMPERATE", &["temperat.mix"])
            .expect("temperate group");
        assert!(!manager.contains("snow.bin"));
        assert!(manager.contains("temperate.bin"));
        assert_eq!(manager.archives.len(), 2);
    }

    #[test]
    fn theater_group_skips_malformed_middle_slot_and_commits_new_identity() {
        let directory = TestDirectory::new("theater-middle-failure");
        directory.write_mix("snow.mix", "snow.bin", b"snow");
        directory.write_mix("temperat.mix", "temperate.bin", b"temperate");
        std::fs::write(directory.path().join("tem.mix"), b"not a MIX")
            .expect("write malformed middle archive");
        directory.write_mix("isotemmd.mix", "iso-md.bin", b"iso-md");

        let mut manager = empty_manager(directory.path());
        manager
            .activate_theater_archives("SNOW", &["snow.mix"])
            .expect("activate initial snow group");
        manager
            .activate_theater_archives(
                "TEMPERATE",
                &["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"],
            )
            .expect("malformed optional slot is skipped");

        assert_eq!(manager.active_theater.as_deref(), Some("TEMPERATE"));
        assert_eq!(
            manager.active_theater_archives,
            ["temperat.mix", "isotemmd.mix"]
        );
        assert_eq!(
            manager.registered_archive_names(),
            ["temperat.mix", "isotemmd.mix"]
        );
        assert!(!manager.contains("snow.bin"));
        assert_eq!(manager.get_ref("iso-md.bin"), Some(&b"iso-md"[..]));

        manager
            .activate_theater_archives(
                "temperate",
                &["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"],
            )
            .expect("same theater is a no-op");
        assert_eq!(
            manager.registered_archive_names(),
            ["temperat.mix", "isotemmd.mix"]
        );
    }
}
