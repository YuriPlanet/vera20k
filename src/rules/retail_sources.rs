//! Selection of retail INI bytes at the production archive boundary.
//!
//! Selection transfers parsed snapshots to NativeRulesProcessOwner. It does not
//! own registry transitions or implement another layer/parser. Source identities
//! describe exactly the consumed buffer, including loose-file winners.
use std::sync::Arc;

use crate::assets::asset_manager::{AssetManager, AssetResolutionRef};
use crate::assets::error::AssetError;
use crate::assets::mix_archive::MixArchive;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::directives::{
    DirectiveDiagnostic, DirectiveOptions, include_identity, process_directives,
};
use crate::rules::personal_rules::PersonalRules;
use crate::rules::process_owner::{NativeLangRuleSource, NativeRulesProcessOwner};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use crate::util::sha256::sha256_hex;

const MAX_INCLUDED_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const INCLUDE_BYTES_EXCEEDED: &str = "INI included sources exceed the 32 MiB expansion budget";

#[derive(Clone, Debug, serde::Serialize)]
pub struct IniSourceIdentity {
    pub(crate) logical_name: String,
    pub source_archive: String,
    pub(crate) entry_id: i32,
    pub(crate) payload_len: usize,
    pub(crate) source_sha256: String,
}

#[derive(Debug)]
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct SelectedIni {
    pub ini: IniFile,
    /// Root bytes parsed before optional Ares include expansion. LANGRULE's
    /// native Digest covers this physical file, not the composed reader layer.
    pub(crate) physical_ini: IniFile,
    pub source: IniSourceIdentity,
    /// Physical child snapshots in traversal order (including repeated DAG
    /// references). Root identity still describes the original root bytes.
    pub included_sources: Vec<IniSourceIdentity>,
    pub directive_diagnostics: Vec<DirectiveDiagnostic>,
}

enum OptionalIniSelection {
    Absent,
    Selected(SelectedIni),
    SelectionFailed(String),
}

pub fn select_ini(assets: &AssetManager, name: &str) -> Result<SelectedIni, String> {
    select_game_ini(assets, name, assets.ships_ares())
}

/// [`select_ini`] for a game that ships `ares.mix` or not: such a game loads
/// its INIs with Ares' syntax and `[#include]` (`ares_switch::ares_way`), as
/// its launch does.
pub(crate) fn select_game_ini(
    assets: &AssetManager,
    name: &str,
    ships_ares: bool,
) -> Result<SelectedIni, String> {
    let _ares_way = crate::rules::ares_switch::read_ares_way(ships_ares);
    if expands_directives(ships_ares) {
        select_ini_with_options(assets, name, DirectiveOptions::production())
    } else {
        select_physical_ini(assets, name)
    }
}

/// Whether a game that ships `ares.mix` or not expands its INIs' directives.
fn expands_directives(ships_ares: bool) -> bool {
    ships_ares || crate::rules::PHOBOS_ENABLED
}

/// Explicit file-composition probe. Tool callers may inspect extension syntax
/// independently of the gameplay capability selected at compile time.
pub fn select_ini_with_options(
    assets: &AssetManager,
    name: &str,
    options: DirectiveOptions,
) -> Result<SelectedIni, String> {
    let mut selected = select_physical_ini(assets, name)?;
    let (expanded, included_sources) = expand_ini_with_assets(assets, selected.ini, name, options)?;
    selected.ini = expanded.ini;
    selected.included_sources = included_sources;
    selected.directive_diagnostics = expanded.diagnostics;
    Ok(selected)
}

/// Maps already own their decoded physical INI (including MMX wrappers).
/// They enter the same traversal/provider instead of selecting a second root.
pub(crate) fn expand_ini_with_assets(
    assets: &AssetManager,
    ini: IniFile,
    name: &str,
    options: DirectiveOptions,
) -> Result<
    (
        crate::rules::ini_parser::directives::ExpandedIni,
        Vec<IniSourceIdentity>,
    ),
    String,
> {
    let mut included_sources = Vec::new();
    let mut included_bytes = 0usize;
    let mut exceeded_budget = false;
    let expanded = process_directives(
        ini,
        name,
        |reference| {
            // All references share the production VFS root; do not rebase on the
            // including document's host directory or open arbitrary host paths.
            let logical = include_identity(reference);
            let remaining = MAX_INCLUDED_SOURCE_BYTES - included_bytes;
            let resolved = match assets.resolve_ref_bounded(&logical, remaining) {
                Ok(Some(resolved)) => resolved,
                Ok(None) => return Ok(None),
                Err(AssetError::ReadLimitExceeded { .. }) => {
                    exceeded_budget = true;
                    return Err(INCLUDE_BYTES_EXCEEDED.into());
                }
                Err(error) => return Err(error.to_string()),
            };
            // An existing zero-byte file is present, not missing: Phobos loads
            // it through `CCFileClass`/`ReadCCFile` and Ares also keeps the
            // read. Only an unresolved reference answers `None`.
            // Charge every physical read, including repeated DAG references,
            // before parsing. Resolution enforces the remaining budget before
            // reading or retaining the first loose snapshot, not afterwards.
            included_bytes += resolved.bytes.len();
            log::info!(
                "Including {logical} ({} bytes) from {}",
                resolved.bytes.len(),
                resolved.source_archive
            );
            included_sources.push(source_identity(&logical, &resolved));
            Ok(Some(resolved.bytes.to_vec()))
        },
        options,
    );
    if exceeded_budget {
        return Err(INCLUDE_BYTES_EXCEEDED.into());
    }
    let expanded = expanded.map_err(|error| error.to_string())?;
    for diagnostic in &expanded.diagnostics {
        log::warn!("INI directive: {diagnostic}");
    }
    Ok((expanded, included_sources))
}

fn select_physical_ini(assets: &AssetManager, name: &str) -> Result<SelectedIni, String> {
    let selected = assets
        .resolve_ref(name)
        .ok_or_else(|| format!("retail INI {name} is unavailable"))?;
    parse_physical_ini(name, selected)
}

fn parse_physical_ini(name: &str, selected: AssetResolutionRef<'_>) -> Result<SelectedIni, String> {
    log::info!(
        "Loading {name} ({} bytes) from {}",
        selected.bytes.len(),
        selected.source_archive
    );
    let ini = IniFile::from_bytes(selected.bytes)
        .map_err(|error| format!("parse {name} from {}: {error}", selected.source_archive))?;
    Ok(SelectedIni {
        ini: ini.clone(),
        physical_ini: ini,
        included_sources: Vec::new(),
        directive_diagnostics: Vec::new(),
        source: source_identity(name, &selected),
    })
}

fn source_identity(name: &str, selected: &AssetResolutionRef<'_>) -> IniSourceIdentity {
    IniSourceIdentity {
        logical_name: name.to_owned(),
        source_archive: selected.source_archive.to_owned(),
        entry_id: selected.entry_id,
        payload_len: selected.bytes.len(),
        source_sha256: sha256_hex(selected.bytes),
    }
}

/// Select the native mode object's INI, if there is a mode object.
/// Reset668B8E..668BAA gates on that object, then always processes the INI
/// returned by 5D67B0. A missing file or empty filename therefore still adds
/// an empty Process pass, including its fixed-ART reads and weapon postpass.
/// `None` means no selected object, not an unavailable file. Missing sources
/// have no fabricated archive identity. Present parse failures propagate;
/// native behavior for malformed data is not established here.
pub fn select_mode_ini(
    assets: &AssetManager,
    selected_name: Option<&str>,
) -> Result<Option<IniFile>, String> {
    let Some(name) = selected_name else {
        return Ok(None);
    };
    let name = name.trim();
    if name.is_empty() {
        return Ok(Some(IniFile::empty()));
    }
    if assets.resolve_ref(name).is_none() {
        log::warn!("Selected mode INI {name} is missing; processing an empty layer");
        return Ok(Some(IniFile::empty()));
    }
    select_ini(assets, name).map(|selected| Some(selected.ini))
}

/// Transient selected inputs, consumed when the process authority is constructed.
/// `langrule` contains a selected mod's optional source; with no mod selected,
/// VERA ignores it so localized installations share the original rules.
/// A physical absence and a selection/expansion failure are recorded separately.
/// Present empty and unverified files retain their physical byte count and parsed root
/// for the process owner's native Digest gate. ART is fixed and never enters
/// the scenario layer stack. Audio definitions are separately selected and
/// injected before Rules construction.
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct RetailRulesSources {
    pub rulesmd: SelectedIni,
    pub langrule: Option<SelectedIni>,
    langrule_selection_failure: Option<String>,
    pub artmd: SelectedIni,
    personal_rules: PersonalRules,
    /// The game is an Ares game (`AssetManager::ships_ares`): it mounted
    /// `ares.mix`, or the player's Ares setting is On.
    ships_ares: bool,
    /// The player's Ares setting is Off: no Ares key turns Ares on.
    ares_keys_off: bool,
    /// The player's Kratos setting (`AssetManager::kratos_setting`), which
    /// every game this owner loads carries into `RuleSet::kratos`.
    kratos_setting: crate::util::kratos_launch::KratosSetting,
}

impl RetailRulesSources {
    /// The game's startup sources, read as its asset manager says: an Ares
    /// game (`AssetManager::ships_ares`) or not, under the player's Ares
    /// setting (`AssetManager::ares_setting`).
    pub fn select(assets: &AssetManager) -> Result<Self, String> {
        Self::select_for_game(assets, assets.ships_ares())
    }

    /// [`Self::select`] for a game that ships `ares.mix` or not, whatever the
    /// archives `assets` mounted: a fixture that reads a stock install as an
    /// Ares game says so here.
    pub(crate) fn select_for_game(assets: &AssetManager, ships_ares: bool) -> Result<Self, String> {
        let rulesmd = select_game_ini(assets, assets.ini_roots().rules(), ships_ares)?;
        // VERA's Original mode deliberately omits the optional LANGRULE pass
        // of Load_Game_Rules (0x0052CD70) and Reset (0x00668B05). A selected
        // mod keeps it, including its native Digest validation and layering.
        let langrule = if assets.mod_selected() {
            select_optional_ini(assets, "langrule.ini", ships_ares)
        } else {
            OptionalIniSelection::Absent
        };
        let (langrule, langrule_selection_failure) = match langrule {
            OptionalIniSelection::Absent => (None, None),
            OptionalIniSelection::Selected(selected) => (Some(selected), None),
            OptionalIniSelection::SelectionFailed(error) => (None, Some(error)),
        };
        let artmd = select_game_ini(assets, assets.ini_roots().art(), ships_ares)?;
        Ok(Self {
            rulesmd,
            langrule,
            langrule_selection_failure,
            artmd,
            personal_rules: PersonalRules::default(),
            ships_ares,
            ares_keys_off: assets.ares_setting() == crate::util::ares_launch::AresSetting::Off,
            kratos_setting: assets.kratos_setting(),
        })
    }

    /// VERA-only source composition; physical source identities remain intact.
    /// No extra native Rules pass is introduced by a personal override: the
    /// owner composes it over each scenario's root and LANGRULE.
    pub fn with_personal_rules(mut self, personal_rules: PersonalRules) -> Self {
        self.personal_rules = personal_rules;
        self
    }

    /// The cold-started native Rules authority alone, without the shell
    /// compatibility projection [`Self::into_startup`] also derives.
    pub fn into_owner(
        self,
        sounds: Arc<SoundRegistry>,
        eva: Arc<EvaRegistry>,
    ) -> Result<NativeRulesProcessOwner, String> {
        let langrule = match (self.langrule, self.langrule_selection_failure) {
            (Some(source), _) => Some(NativeLangRuleSource::Selected(source)),
            (None, Some(error)) => Some(NativeLangRuleSource::SelectionFailed(error)),
            (None, None) => None,
        };
        let owner = NativeRulesProcessOwner::from_named_cold_start_sources(
            self.rulesmd.ini,
            langrule,
            self.artmd.ini,
            sounds,
            eva,
            [
                self.rulesmd.source.logical_name,
                self.artmd.source.logical_name,
            ],
            self.personal_rules,
            self.ships_ares,
            self.ares_keys_off,
            self.kratos_setting,
        )
        .map_err(|error| format!("Native rules cold startup failed: {error}"))?;
        Ok(owner)
    }

    pub fn into_startup(
        self,
        sounds: Arc<SoundRegistry>,
        eva: Arc<EvaRegistry>,
    ) -> Result<StartupRulesLoad, String> {
        let native_owner = self.into_owner(sounds, eva)?;
        let (compatibility_rules, compatibility_projection) =
            match native_owner.startup_compatibility_projection() {
                Ok(processed) => {
                    let mut compatibility_rules = RuleSet::from_processed_rules(&processed)
                        .map_err(|error| {
                            log::warn!("Failed to parse startup rules projection: {error}")
                        })
                        .ok();
                    if let Some(rules) = compatibility_rules.as_mut() {
                        native_owner.bind_type_audio_references(rules, &processed);
                    }
                    let compatibility_projection =
                        processed.into_projection_discarding_native_receipt();
                    (compatibility_rules, Some(compatibility_projection))
                }
                Err(error) => {
                    log::warn!("Failed to build startup rules projection: {error}");
                    (None, None)
                }
            };

        Ok(StartupRulesLoad {
            compatibility_rules,
            compatibility_projection,
            native_owner,
        })
    }
}

/// Optional LANGRULE selection uses the same physical parser and Ares include
/// expansion as the original selector. Probe availability first to retain a
/// physical absence separately from a selected file whose expansion fails.
fn select_optional_ini(
    assets: &AssetManager,
    name: &str,
    ships_ares: bool,
) -> OptionalIniSelection {
    if assets.resolve_ref(name).is_none() {
        return OptionalIniSelection::Absent;
    }
    match select_game_ini(assets, name, ships_ares) {
        Ok(selected) => OptionalIniSelection::Selected(selected),
        Err(error) => OptionalIniSelection::SelectionFailed(error),
    }
}

/// The archives a retail install ships, by outermost name and in lookup
/// order: the language archives (mounted ahead of the expansions), the
/// YR 1.001 patch `expandmd01.mix` (retail `rulesmd.ini` comes from it) and
/// the masters. Retail installs ship no other expansion.
const SHIPPED_ARCHIVES: [&str; 5] = [
    "langmd.mix",
    "language.mix",
    "expandmd01.mix",
    "ra2md.mix",
    "ra2.mix",
];
/// The master archive whose nested `localmd.mix` ships most INIs.
const SHIPPED_MASTER: &str = "ra2md.mix";
const SHIPPED_LOCAL: &str = "localmd.mix";

/// Whether a source (`AssetResolutionRef::source_archive`, an archive's name)
/// is a shipped archive or nested in one. A loose file never is.
pub fn shipped_source(source: &str) -> bool {
    !source.starts_with("loose:")
        && SHIPPED_ARCHIVES
            .iter()
            .any(|shipped| outermost_archive(source).eq_ignore_ascii_case(shipped))
}

/// The game-folder file a source comes from: a loose file's name, else the
/// archive's outermost name (`expandmd99.mix`).
pub fn game_folder_file(source: &str) -> String {
    match source.strip_prefix("loose:") {
        Some(path) => std::path::Path::new(path).file_name().map_or_else(
            || path.to_owned(),
            |file| file.to_string_lossy().into_owned(),
        ),
        None => outermost_archive(source).to_owned(),
    }
}

/// One archive a retail install searches ([`retail_archive`]): the name its
/// files report as their source, the game-folder source it stands in for,
/// and the archive.
pub(crate) type RetailArchive<'a> = (&'a str, Option<&'a str>, std::borrow::Cow<'a, MixArchive>);

/// The registered archives as a retail install searches them, in their
/// order ([`retail_archive`]).
pub(crate) fn registered_retail(asset_manager: &AssetManager) -> Vec<RetailArchive<'_>> {
    asset_manager
        .registered_archives()
        .filter_map(|(source, archive)| {
            let (replaces, retail) = retail_archive(asset_manager, source, Some(archive))?;
            Some((source, replaces, retail))
        })
        .collect()
}

/// What a retail install searches in place of one archive (`None` when it
/// does not open): the archive itself when it ships; else the shipped copy of
/// the archive of its name, with the source it stands in for; else nothing
/// (`expandmd99.mix`).
pub(crate) fn retail_archive<'a>(
    assets: &AssetManager,
    source: &'a str,
    archive: Option<&'a MixArchive>,
) -> Option<(Option<&'a str>, std::borrow::Cow<'a, MixArchive>)> {
    if let Some(archive) = archive.filter(|_| shipped_source(source)) {
        return Some((None, std::borrow::Cow::Borrowed(archive)));
    }
    let name = source.rsplit("->").next().unwrap_or(source).trim();
    shipped_nested_archive(assets, name).map(|copy| (Some(source), std::borrow::Cow::Owned(copy)))
}

/// VERA: a binder's reads of picture and model files, telling a file the
/// game folder supplies from the copy a retail install reads. The asset
/// binders read through it where a network start certifies what they take
/// (`RuleSet::file_inputs_hash`), so a refused start can name the files that
/// change it (`RuleSet::file_input_folder_files`).
pub(crate) struct FolderFiles<'a> {
    assets: &'a AssetManager,
    /// [`registered_retail`], found by the first read no shipped archive
    /// supplies; a game without such files never builds it.
    retail: std::cell::OnceCell<Vec<RetailArchive<'a>>>,
}

impl<'a> FolderFiles<'a> {
    pub(crate) fn new(assets: &'a AssetManager) -> Self {
        Self {
            assets,
            retail: std::cell::OnceCell::new(),
        }
    }

    /// `name` as the game reads it (`AssetManager::get_ref`'s bytes), with
    /// the game-folder file that supplies it ([`game_folder_file`]) when no
    /// shipped archive does ([`shipped_source`]).
    pub(crate) fn read(&self, name: &str) -> Option<(&'a [u8], Option<String>)> {
        let (bytes, source) = self.assets.get_with_source_ref(name)?;
        Some((
            bytes,
            (!shipped_source(source)).then(|| game_folder_file(source)),
        ))
    }

    /// The copy of `name` a retail install reads from the registered
    /// archives; `None` when it ships none.
    pub(crate) fn shipped(&self, name: &str) -> Option<&[u8]> {
        let id = crate::assets::mix_hash::mix_hash(name);
        self.retail
            .get_or_init(|| registered_retail(self.assets))
            .iter()
            .find(|(_, _, archive)| archive.contains_id(id))
            .and_then(|(_, _, archive)| archive.get_by_id(id))
    }

    /// [`Self::read`]'s bytes, noting the game-folder file in `changed` when
    /// `value` of them is not `value` of the shipped copy's: the file then
    /// changes what the binder takes. A name a retail install lacks differs.
    pub(crate) fn read_noting<V: PartialEq>(
        &self,
        name: &str,
        value: impl Fn(&[u8]) -> V,
        changed: &mut std::collections::BTreeSet<String>,
    ) -> Option<&'a [u8]> {
        let (bytes, folder_file) = self.read(name)?;
        if let Some(file) = folder_file
            && self.shipped(name).map(&value) != Some(value(bytes))
        {
            changed.insert(file);
        }
        Some(bytes)
    }

    /// VERA-only, experimental (docs/picture-model-art.md): notes in
    /// `changed` the picture or model `file` a binder read in place of an
    /// SHP or VXL when what it took from it, `taken` (None when the file
    /// gave nothing), is not what the copy a retail install reads of the
    /// first of `retail` it ships gives (`value`; None when it ships none).
    /// The file noted is the game-folder file that supplies `file`
    /// ([`game_folder_file`]), else `file` itself when nothing does; a
    /// shipped archive's is never noted. Equal values note nothing, so a
    /// pack that keeps the original's facts is never named while the
    /// retail file is there; where a repacked archive dropped it, the
    /// stand-in is named whatever its fact. Naming only: no hash may depend
    /// on this, as the retail file's presence is no fact the simulation
    /// reads. Whether the values differ, noted or not.
    pub(crate) fn note_stand_in<V: PartialEq, N: AsRef<str>>(
        &self,
        file: &str,
        taken: Option<V>,
        retail: impl IntoIterator<Item = N>,
        value: impl FnOnce(&[u8]) -> Option<V>,
        changed: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        let shipped = retail
            .into_iter()
            .find_map(|name| self.shipped(name.as_ref()))
            .and_then(value);
        if shipped == taken {
            return false;
        }
        match self.assets.get_with_source_ref(file) {
            Some((_, source)) if shipped_source(source) => {}
            Some((_, source)) => {
                changed.insert(game_folder_file(source));
            }
            None => {
                changed.insert(file.to_owned());
            }
        }
        true
    }
}

fn outermost_archive(source: &str) -> &str {
    source.split(" -> ").next().unwrap_or(source).trim()
}

/// The shipped copy of the nested archive `name` (`isosnomd.mix`): the
/// first shipped archive, in lookup order, that holds one.
pub(crate) fn shipped_nested_archive(assets: &AssetManager, name: &str) -> Option<MixArchive> {
    let id = crate::assets::mix_hash::mix_hash(name);
    SHIPPED_ARCHIVES.iter().find_map(|archive| {
        assets
            .archive_with_source(archive)?
            .1
            .nested_archive_by_id(id)
            .ok()
            .flatten()
    })
}

/// VERA: the game-folder files that change any of `names` from the copy the
/// retail game ships, for Quick Match, which plays the original game. Each
/// is a loose file's name or an archive's outermost name (`expandmd99.mix`),
/// once. A copy from a shipped archive is the original (a replaced retail
/// archive is not caught, and is never named). Any other copy is compared
/// with the shipped one: a shipped archive's own entry, else the one in
/// `ra2md.mix`'s own `localmd.mix`, read there because a mod's archive can
/// mount another `localmd.mix` first. The same entries
/// (`IniFile::multiplayer_digest`, without [`vera_drawing_keys`]) are
/// unchanged, and so is a name nothing supplies.
pub fn files_changing_shipped_inis(assets: &AssetManager, names: &[&str]) -> Vec<String> {
    let shipped = ShippedInis::new(assets);
    let mut files = Vec::new();
    for name in names {
        let Some(used) = shipped.used(name) else {
            continue;
        };
        if used.shipped == ShippedCopy::Same {
            continue;
        }
        let file = game_folder_file(used.source);
        if !files.contains(&file) {
            files.push(file);
        }
    }
    files
}

/// VERA: the copies of INIs the game reads held against the ones a retail
/// install ships, as [`files_changing_shipped_inis`] compares them.
pub struct ShippedInis<'a> {
    assets: &'a AssetManager,
    /// `ra2md.mix`'s own `localmd.mix`, opened by the first comparison that
    /// needs it.
    local: std::cell::OnceCell<Option<MixArchive>>,
}

/// One INI as the game reads it ([`ShippedInis::used`]).
pub struct UsedIni<'a> {
    /// Where it comes from (`AssetResolutionRef::source_archive`).
    pub source: &'a str,
    pub bytes: &'a [u8],
    pub shipped: ShippedCopy<'a>,
}

/// How an INI the game reads stands to the copy a retail install ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShippedCopy<'a> {
    /// The shipped copy, or one with the same entries.
    Same,
    /// Another copy: the shipped one's bytes, `None` when retail ships no
    /// such file.
    Changed(Option<&'a [u8]>),
}

impl<'a> UsedIni<'a> {
    /// The copy a retail install ships: this one when it is the same.
    pub fn shipped_bytes(&self) -> Option<&'a [u8]> {
        match self.shipped {
            ShippedCopy::Same => Some(self.bytes),
            ShippedCopy::Changed(shipped) => shipped,
        }
    }
}

impl<'a> ShippedInis<'a> {
    pub fn new(assets: &'a AssetManager) -> Self {
        Self {
            assets,
            local: std::cell::OnceCell::new(),
        }
    }

    /// The copy of `name` the game reads and how it stands to the shipped
    /// one; `None` when nothing supplies `name`.
    pub fn used(&self, name: &str) -> Option<UsedIni<'_>> {
        let name = name.trim();
        let (source, bytes) = (!name.is_empty())
            .then(|| self.assets.installed_file(name))
            .flatten()?;
        let mut found = UsedIni {
            source,
            bytes,
            shipped: ShippedCopy::Same,
        };
        if shipped_source(source) {
            return Some(found);
        }
        let shipped = self.shipped(name);
        let dropped = vera_drawing_keys(self.assets, name);
        if !shipped.is_some_and(|shipped| same_entries(shipped, bytes, dropped)) {
            found.shipped = ShippedCopy::Changed(shipped);
        }
        Some(found)
    }

    /// The copy of `name` a retail install ships: a shipped archive's own
    /// entry, else the one in `ra2md.mix`'s own `localmd.mix`.
    fn shipped(&self, name: &str) -> Option<&[u8]> {
        let local = self.local.get_or_init(|| {
            self.assets
                .archive_with_source(SHIPPED_MASTER)
                .and_then(|(_, master)| {
                    master
                        .nested_archive_by_id(crate::assets::mix_hash::mix_hash(SHIPPED_LOCAL))
                        .ok()
                        .flatten()
                })
        });
        SHIPPED_ARCHIVES
            .iter()
            .find_map(|archive| {
                self.assets
                    .archive_with_source(archive)
                    .and_then(|(_, archive)| archive.get_by_name(name))
            })
            .or_else(|| local.as_ref().and_then(|archive| archive.get_by_name(name)))
    }
}

/// VERA: which AI INI (`IniRoots::ai`) a match reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiFile {
    /// The one the game folder supplies, as every other INI root.
    GameFolder,
    /// The copy a retail install ships, whatever the game folder supplies.
    Original,
}

/// VERA: the AI INI a match reads. A network match of the original game
/// that seats no computer reads the original AI INI. No house in it runs
/// the computer's teams, and the teams a map's triggers create are then the
/// original game's for every player, so players whose game folders hold
/// other AI files load one world and their rules agreement compares the
/// original (the app's `ProcessAssets::multiplayer_match_rules_digest`).
/// `computers` is whether the match seats a house the computer runs
/// (`SkirmishLaunchSession::seats_computers`; a campaign always does).
/// Every other match reads the game folder's: one with a computer, a
/// selected mod's, whose AI file is its game, one whose AI root is not
/// YR's, and every match off the network.
pub fn match_ai_file(assets: &AssetManager, network: bool, computers: bool) -> AiFile {
    let stock_root = assets
        .ini_roots()
        .ai()
        .eq_ignore_ascii_case(crate::util::ini_roots::IniRoots::default().ai());
    if network && !computers && !assets.mod_selected() && stock_root {
        AiFile::Original
    } else {
        AiFile::GameFolder
    }
}

/// The AI root as `file` names it, read as [`select_ini`] reads it. The game
/// folder's too where a retail install ships no AI INI.
pub fn select_ai_ini(assets: &AssetManager, file: AiFile) -> Result<IniFile, String> {
    let name = assets.ini_roots().ai();
    let shipped = ShippedInis::new(assets);
    let from_folder = assets
        .resolve_ref(name)
        .is_some_and(|used| !shipped_source(used.source_archive));
    if file == AiFile::Original
        && from_folder
        && let Some(bytes) = shipped.shipped(name)
    {
        let ships_ares = assets.ships_ares();
        let _ares_way = crate::rules::ares_switch::read_ares_way(ships_ares);
        let ini = IniFile::from_bytes(bytes)
            .map_err(|error| format!("parse the original {name}: {error}"))?;
        if !expands_directives(ships_ares) {
            return Ok(ini);
        }
        return expand_ini_with_assets(assets, ini, name, DirectiveOptions::production())
            .map(|(expanded, _)| expanded.ini);
    }
    select_ini(assets, name).map(|selected| selected.ini)
}

/// Whether `used` holds `shipped`'s entries as the multiplayer digest reads
/// them, without the keys `dropped` ([`vera_drawing_keys`]).
fn same_entries(shipped: &[u8], used: &[u8], dropped: &[&str]) -> bool {
    shipped == used
        || match (IniFile::from_bytes(shipped), IniFile::from_bytes(used)) {
            (Ok(shipped), Ok(used)) => {
                shipped.multiplayer_digest_without(dropped)
                    == used.multiplayer_digest_without(dropped)
            }
            _ => false,
        }
}

/// VERA-only, experimental (docs/picture-model-art.md): the keys of the INI
/// `name` the game reads that only choose what is drawn, which every
/// agreement on that INI leaves out (`IniFile::multiplayer_digest_without`):
/// VERA's art keys in the Art root (`art_data::VERA_ART_KEYS`), the tile
/// pictures' key in a theater INI (`map::theater::VERA_THEATER_KEYS`), none
/// in any other. So an art pack's INIs that add only those keys are the
/// original game's to rooms, Quick Match and the rules parts. Retail INIs
/// hold none of them.
pub fn vera_drawing_keys(assets: &AssetManager, name: &str) -> &'static [&'static str] {
    let name = name.trim();
    if name.eq_ignore_ascii_case(assets.ini_roots().art()) {
        crate::rules::art_data::VERA_ART_KEYS
    } else if crate::map::theater::theater_ini_names(assets)
        .iter()
        .any(|theater| theater.eq_ignore_ascii_case(name))
    {
        crate::map::theater::VERA_THEATER_KEYS
    } else {
        &[]
    }
}

/// Cold-start native Rules authority plus the one shell compatibility
/// projection derived from the same selected INI snapshots.
pub struct StartupRulesLoad {
    compatibility_rules: Option<RuleSet>,
    compatibility_projection: Option<IniFile>,
    native_owner: NativeRulesProcessOwner,
}

impl StartupRulesLoad {
    pub fn into_parts(self) -> (Option<RuleSet>, Option<IniFile>, NativeRulesProcessOwner) {
        (
            self.compatibility_rules,
            self.compatibility_projection,
            self.native_owner,
        )
    }
}

/// Production cold-start selection. Failed compatibility projection does not
/// discard the native registry owner; only selection/cold construction can fail.
pub fn load_startup_rules(
    assets: &AssetManager,
    sounds: Arc<SoundRegistry>,
    eva: Arc<EvaRegistry>,
) -> Option<StartupRulesLoad> {
    RetailRulesSources::select(assets)
        .and_then(|sources| sources.into_startup(sounds, eva))
        .map_err(|error| log::warn!("{error}"))
        .ok()
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "ini_root_tests.rs"]
mod ini_root_tests;

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn include_byte_limit_refuses_sparse_source_before_retaining_a_snapshot() {
        let directory = TestDirectory::new("include-sparse-limit");
        directory.write("root.ini", b"[#include]\n0=child.ini\n");
        let child = directory.path().join("child.ini");
        let sparse = std::fs::File::create(&child).unwrap();
        sparse.set_len(32 * 1024 * 1024 + 1).unwrap();
        drop(sparse);
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let error = select_ini_with_options(&assets, "root.ini", DirectiveOptions::default())
            .err()
            .expect("even non-strict traversal must refuse an over-budget source");
        assert!(error.contains("32 MiB expansion budget"));

        // Refusal must leave the lazy source untouched. Retrying through the
        // same manager after shrinking the physical file consumes its new bytes.
        let bytes = b"[Unit]\nValue=bounded\n";
        std::fs::write(child, bytes).unwrap();
        let selected =
            select_ini_with_options(&assets, "root.ini", DirectiveOptions::default()).unwrap();
        assert_eq!(selected.included_sources.len(), 1);
        assert_eq!(selected.included_sources[0].payload_len, bytes.len());
        assert_eq!(
            selected.included_sources[0].source_sha256,
            sha256_hex(bytes)
        );
        assert_eq!(
            selected
                .ini
                .section_or_empty("Unit")
                .read_string("Value", "", 32),
            "bounded"
        );
    }

    #[test]
    fn selected_sources_hash_consumed_bytes_and_survive_disk_changes() {
        let directory = TestDirectory::new("rules-selection");
        let root = b"[General]\r\nBuildSpeed=.7\r\nFlightLevel=1500\r\n";
        directory.write("rulesmd.ini", root);
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = RetailRulesSources::select(&assets).expect("selected sources");
        assert_eq!(selected.rulesmd.source.source_sha256, sha256_hex(root));
        assert_eq!(selected.rulesmd.source.payload_len, root.len());
        assert!(selected.langrule.is_none());
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=9\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=8x8\n");
        let (_, _, mut owner) = selected
            .into_startup(Arc::default(), Default::default())
            .expect("cold startup")
            .into_parts();
        let (rules, projection, art, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .expect("scenario from retained startup")
            .into_parts();
        assert_eq!(
            projection
                .section("General")
                .unwrap()
                .get_for_test("BuildSpeed"),
            Some(".7")
        );
        assert_eq!(
            art.section("TECH").unwrap().get_for_test("Foundation"),
            Some("2x3")
        );
        assert_eq!(
            rules.production.build_speed.bits(),
            f64::from(0.7_f32).to_bits()
        );
        assert_eq!(
            owner
                .selected_rules_root()
                .section("General")
                .unwrap()
                .get_for_test("FlightLevel"),
            Some("1500")
        );
    }

    #[test]
    fn optional_langrule_selection_preserves_absent_empty_and_bad_digest_sources() {
        use crate::assets::asset_manager::{MediaArchiveMode, test_support::audio_test_install};

        let install = audio_test_install("langrule-mod-install");
        let directory = TestDirectory::new("optional-langrule-physical-source");
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");

        let open = || {
            AssetManager::with_mod(
                install.path(),
                Some(directory.path()),
                MediaArchiveMode::STOCK_DIGITAL,
            )
            .unwrap()
        };
        let absent_assets = open();
        assert!(
            RetailRulesSources::select(&absent_assets)
                .expect("required sources")
                .langrule
                .is_none(),
            "a missing physical source has no retained parsed layer"
        );

        directory.write("langrule.ini", b"");
        let empty_assets = open();
        let empty = RetailRulesSources::select(&empty_assets)
            .expect("required sources")
            .langrule
            .expect("present empty LANGRULE remains selected");
        assert_eq!(empty.source.payload_len, 0);
        assert!(empty.ini.is_empty());

        let invalid = b"[General]\nBuildSpeed=.58\n[Digest]\n1=AAAAAAAAAAAAAAAAAAAAAAAAAAA=\n";
        directory.write("langrule.ini", invalid);
        let invalid_assets = open();
        let selected = RetailRulesSources::select(&invalid_assets)
            .expect("required sources")
            .langrule
            .expect("a rejected digest is still a present physical source");
        assert_eq!(selected.source.payload_len, invalid.len());
        assert!(selected.ini.section("Digest").is_some());
    }

    /// A selected mod that mounts `ares.mix` (Load_Bootstrap_AresMIX, hook
    /// `0x0053029E`) selects its INIs with Ares' syntax. No mod ignores the
    /// installed archive and keeps gamemd's syntax. The game's asset manager
    /// gives that answer, whichever way the calling thread reads.
    #[test]
    fn a_game_with_ares_mix_selects_its_rules_with_ares_syntax() {
        use crate::assets::asset_manager::MediaArchiveMode;
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
            let directory = crate::assets::asset_manager::test_support::audio_test_install(
                "ares-mix-rules-syntax",
            );
            std::fs::write(
                directory.path().join("rulesmd.ini"),
                b"[BuildingTypes]\n+=GAPOWR\n",
            )
            .unwrap();
            std::fs::write(
                directory.path().join("artmd.ini"),
                b"[GAPOWR]\nFoundation=2x2\n",
            )
            .unwrap();
            let mod_dir = TestDirectory::new("ares-rules-mod");
            let keys = |selected_mod: bool| {
                let assets = AssetManager::with_mod(
                    directory.path(),
                    selected_mod.then_some(mod_dir.path()),
                    MediaArchiveMode::STOCK_DIGITAL,
                )
                .unwrap();
                let sources = RetailRulesSources::select(&assets).unwrap();
                let keys: Vec<String> = sources
                    .rulesmd
                    .ini
                    .section_or_empty("BuildingTypes")
                    .keys()
                    .map(str::to_owned)
                    .collect();
                (assets.ships_ares(), sources.ships_ares, keys)
            };
            assert_eq!(keys(false), (false, false, vec!["+".to_owned()]));
            directory.write_mix("ares.mix", "ares.txt", b"ares");
            assert_eq!(keys(false), (false, false, vec!["+".to_owned()]));
            assert_eq!(keys(true), (true, true, vec!["var_0".to_owned()]));
        }
    }

    #[test]
    fn langrule_selection_expands_ares_includes_and_keeps_physical_root_for_digest() {
        use crate::assets::asset_manager::{MediaArchiveMode, test_support::audio_test_install};

        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let install = audio_test_install("langrule-include-install");
        let directory = TestDirectory::new("langrule-ares-include");
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        directory.write(
            "langrule.ini",
            b"[#include]\n0=lang-extra.ini\n[General]\nBuildSpeed=.58\n",
        );
        directory.write(
            "lang-extra.ini",
            b"[General]\nBuildSpeed=.4\n[ColorAdd]\nLANG0=7,8,9\n",
        );
        // The selected mod mounts the installation's `ares.mix`, so its game
        // reads the Ares way.
        install.write_mix("ares.mix", "ares.txt", b"ares");
        let assets = AssetManager::with_mod(
            install.path(),
            Some(directory.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let selected = RetailRulesSources::select(&assets)
            .expect("required sources")
            .langrule
            .expect("physical LANGRULE");

        assert_eq!(selected.included_sources.len(), 1);
        assert_eq!(
            selected
                .ini
                .section_or_empty("General")
                .read_string("BuildSpeed", "", 16),
            ".4",
            "the Ares include wins in the composed LANGRULE layer"
        );
        assert_eq!(
            selected
                .physical_ini
                .section_or_empty("General")
                .read_string("BuildSpeed", "", 16),
            ".58",
            "Digest sees the selected physical root before include expansion"
        );
        assert!(selected.physical_ini.section("#include").is_some());
    }

    #[test]
    fn optional_mode_ini_missing_file_continues_with_an_empty_layer() {
        // Reset668BA0..668BAA processes the empty INI returned by 5D67B0.
        // ReadTypeData679A5D..679A82 must sweep fixed ART before the map
        // allocates MAP, including an animation first named by AudioVisual.
        let directory = TestDirectory::new("optional-mode-ini-missing");
        directory.write(
            "rulesmd.ini",
            b"[General]\nBuildSpeed=.7\n[AudioVisual]\nSmoke=LATE\n",
        );
        directory.write("artmd.ini", b"[LATE]\nNext=CHILD\n[CHILD]\nRate=1\n");
        directory.write("MPEmptyMD.ini", b"");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        assert!(
            select_ini(&assets, "MPMissingMD.ini").is_err(),
            "the required-INI selector keeps a missing file fatal (rulesmd policy)"
        );
        let load = |name| {
            let selected = select_mode_ini(&assets, Some(name)).expect("select mode");
            let (_, _, mut owner) = load_startup_rules(&assets, Arc::default(), Default::default())
                .expect("startup")
                .into_parts();
            let (rules, ini, _, receipt) = owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
                        selected.as_ref(),
                    ),
                    &IniFile::from_str("[Animations]\n0=MAP\n[General]\nFlightLevel=1500\n"),
                )
                .expect("scenario rebuild with selected mode")
                .into_parts();
            assert_eq!(
                rules.production.build_speed.bits(),
                f64::from(0.7_f32).to_bits()
            );
            assert_eq!(
                ini.section("General").unwrap().get_for_test("FlightLevel"),
                Some("1500")
            );
            let (pre, post) = receipt.into_parts();
            (rules.source_ini_hash(), pre.into_parts(), post.into_parts())
        };
        let missing = load("MPMissingMD.ini");
        let empty = load("MPEmptyMD.ini");
        assert_eq!(
            missing.2, empty.2,
            "a missing file must retain the empty Process pass"
        );
        assert_eq!(
            missing.1, empty.1,
            "the pre-reset constructor prefix stays identical"
        );
        assert_eq!(
            missing.0, empty.0,
            "selected empty sources have the same rules identity"
        );
        assert_eq!(
            load(""),
            empty,
            "an object with an empty filename still processes its INI"
        );
    }

    #[test]
    fn no_selected_mode_object_selects_no_layer() {
        let directory = TestDirectory::new("no-mode-ini");
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = select_mode_ini(&assets, None).expect("no selected mode");
        assert!(selected.is_none());
    }

    #[test]
    fn optional_mode_ini_present_file_overlays_rulesmd_under_the_map() {
        let directory = TestDirectory::new("optional-mode-ini-present");
        directory.write(
            "rulesmd.ini",
            b"[General]\nBuildSpeed=.7\nFlightLevel=1500\n",
        );
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        directory.write("MPBattleMD.ini", b"[General]\nFlightLevel=1200\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = select_mode_ini(&assets, Some("MPBattleMD.ini"))
            .expect("present optional INI selects")
            .expect("a present file yields a layer");
        assert_eq!(
            selected
                .section("General")
                .unwrap()
                .get_for_test("FlightLevel"),
            Some("1200")
        );
        let (_, _, mut owner) = load_startup_rules(&assets, Arc::default(), Default::default())
            .expect("startup")
            .into_parts();
        let (_, ini, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(
                    &selected,
                )),
                &IniFile::from_str("[General]\nBuildSpeed=1\n"),
            )
            .expect("scenario rebuild with the mode layer")
            .into_parts();
        let general = ini.section("General").unwrap();
        assert_eq!(
            general.get_for_test("FlightLevel"),
            Some("1200"),
            "the mode layer beats rulesmd"
        );
        assert_eq!(
            general.get_for_test("BuildSpeed"),
            Some("1"),
            "the map layer beats the mode layer"
        );
    }

    #[test]
    fn langrule_overlays_standalone_rulesmd_through_retained_owner() {
        // Migrated from the deleted app-local compose_rules_layers regression.
        let owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n"),
            Some(IniFile::from_str("[General]\nBuildSpeed=.58\n")),
            IniFile::empty(),
            Arc::default(),
            Default::default(),
        )
        .expect("cold startup");
        let processed = owner
            .startup_compatibility_projection()
            .expect("startup projection");
        let ini = processed.ini();
        assert_eq!(
            ini.section("General").unwrap().get_for_test("BuildSpeed"),
            Some(".58")
        );
        assert_eq!(
            ini.section("General").unwrap().get_for_test("FlightLevel"),
            Some("1500")
        );
        let rules = RuleSet::from_processed_rules(&processed).expect("processed rules");
        assert_eq!(
            rules.production.build_speed.bits(),
            f64::from(0.58_f32).to_bits()
        );
    }

    #[test]
    fn random_map_catalog_uses_retained_root_langrule_and_fixed_art() {
        use crate::assets::asset_manager::{MediaArchiveMode, test_support::audio_test_install};

        let install = audio_test_install("rmg-langrule-install");
        let directory = TestDirectory::new("rmg-retained-rules");
        // TECH2 is registered so that Ares's Find-only list read (Buf_AI
        // 1005B830, hook 0x672B0E) resolves the langrule list too.
        directory.write(
            "rulesmd.ini",
            b"[BuildingTypes]\n0=TECH2\n[AI]\nNeutralTechBuildings=TECH1\n",
        );
        // The original Reset's ReadCCFile4741F0 accepted this Digest for the
        // exact physical body through the existing RawFile/INI owners.
        directory.write(
            "langrule.ini",
            b"[AI]\nNeutralTechBuildings=TECH2\n[Digest]\n1=CGLFHoNX2UiC0RYwRTmgBnJVpec=\n",
        );
        directory.write("artmd.ini", b"[TECH2]\nFoundation=2x3\n");
        let assets = AssetManager::with_mod(
            install.path(),
            Some(directory.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let (_, _, owner) = load_startup_rules(&assets, Arc::default(), Default::default())
            .expect("startup")
            .into_parts();
        directory.write(
            "langrule.ini",
            b"[AI]\nNeutralTechBuildings=WRONG\n[Digest]\n1=CGLFHoNX2UiC0RYwRTmgBnJVpec=\n",
        );
        directory.write("artmd.ini", b"[TECH2]\nFoundation=1x1\n");
        let projected = owner
            .startup_compatibility_projection()
            .expect("read-only startup projection");
        let types = crate::map::rmg::tech_catalog::resolve(
            projected.neutral_tech_buildings(),
            owner.fixed_art(),
        );
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "TECH2");
        assert_eq!(
            types[0].footprint,
            vec![(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]
        );
        let again = owner
            .startup_compatibility_projection()
            .expect("repeated projection");
        assert_eq!(again.content_hash(), projected.content_hash());
    }
    /// Stock field regressions through retained startup, scenario passes and
    /// fixed ART. These are retail-data checks, not fresh native goldens.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn stock_hills_battle_fields_match_retail_contracts() {
        use crate::util::fixed_math::SimFixed;
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let gacnst = rules.object("GACNST").expect("retail GACNST");
        assert_eq!(gacnst.strength, 1000);
        assert_eq!(gacnst.armor, "concrete");
        assert_eq!(gacnst.adjacent, 2);
        assert_eq!(gacnst.power, 0);
        let gapowr = rules.object("GAPOWR").expect("retail GAPOWR");
        assert_eq!(gapowr.strength, 750);
        assert_eq!(gapowr.armor, "wood");
        assert_eq!(gapowr.adjacent, 2);
        assert_eq!(gapowr.power, 200);
        let amradr = rules.object("AMRADR").expect("retail AMRADR");
        assert_eq!(amradr.strength, 600);
        assert_eq!(amradr.armor, "steel");
        assert_eq!(amradr.adjacent, 2);
        assert_eq!(amradr.power, -50);
        assert!(amradr.radar);

        for id in ["GAWEAP", "NAWEAP", "GAYARD", "NAYARD", "YAWEAP", "YAYARD"] {
            assert!(
                rules
                    .object(id)
                    .is_some_and(|object| object.weapons_factory)
            );
        }
        for id in ["GAPILE", "GAPOWR"] {
            assert!(
                !rules
                    .object(id)
                    .is_some_and(|object| object.weapons_factory)
            );
        }

        assert_eq!(
            rules.object("APOC").expect("retail APOC").weight,
            SimFixed::lit("3.5")
        );
        assert_eq!(
            rules.object("MTNK").expect("retail MTNK").weight,
            SimFixed::ONE
        );
        assert_eq!(
            rules.object("DEST").expect("retail DEST").weight,
            SimFixed::lit("3")
        );
        assert_eq!(
            rules.general.direct_rocking_coefficient,
            SimFixed::lit("1.5")
        );
        assert_eq!(rules.general.fallback_coefficient, SimFixed::lit("0.1"));
        let v3wh = rules.warhead("V3WH").expect("processed retail V3WH");
        assert!(v3wh.rocker);
        assert!(!v3wh.direct_rocker);

        for id in ["GHOST", "TANY", "PTROOP"] {
            assert!(rules.object(id).is_some_and(|object| object.c4));
        }
        for id in ["CAMISC01", "CAMISC02", "CAMISC06", "AMMOCRAT"] {
            assert!(!rules.object(id).is_some_and(|object| object.can_c4));
        }
        let napowr = rules.object("NAPOWR").expect("retail NAPOWR");
        assert_eq!(napowr.strength, 750);
        assert!(napowr.can_c4);
        assert_eq!(rules.c4_delay_frames(), 26);
    }

    #[test]
    fn game_folder_files_that_change_shipped_inis_are_named() {
        use crate::assets::asset_manager::MediaArchiveMode;
        use crate::assets::asset_manager::test_support::audio_test_install;
        use crate::map::source::test_support::make_new_format_mix_bytes;
        let dir = audio_test_install("shipped-inis");
        let local = make_new_format_mix_bytes(&[("aimd.ini", b"[AI]\nValue=1\n")]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local)]),
        )
        .unwrap();
        dir.write_mix(
            "expandmd01.mix",
            "rulesmd.ini",
            b"[General]\nBuildSpeed=.7\n",
        );
        let changed = || {
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            files_changing_shipped_inis(&assets, &["rulesmd.ini", "aimd.ini", "langrule.ini"])
        };
        assert!(changed().is_empty());
        // A localized edition's own archive is the original too.
        dir.write_mix("langmd.mix", "langrule.ini", b"[General]\nName=x\n");
        assert!(changed().is_empty());
        let rules = dir.path().join("rulesmd.ini");
        std::fs::write(&rules, b"; same entries\r\n[General]\r\nBuildSpeed=.7\r\n").unwrap();
        assert!(changed().is_empty());
        std::fs::write(&rules, b"[General]\nBuildSpeed=.5\n").unwrap();
        dir.write_mix("expandmd99.mix", "aimd.ini", b"[AI]\nValue=2\n");
        assert_eq!(changed(), ["rulesmd.ini", "expandmd99.mix"]);
    }

    /// VERA (docs/picture-model-art.md): a loose art INI that only adds
    /// VERA's drawing keys, a section of only those among them, changes no
    /// game file (`vera_drawing_keys`); one that changes any other key does.
    /// The same key in the Rules root still counts.
    #[test]
    fn an_art_ini_that_only_adds_veras_drawing_keys_changes_no_game_file() {
        use crate::assets::asset_manager::MediaArchiveMode;
        use crate::assets::asset_manager::test_support::audio_test_install;
        use crate::map::source::test_support::make_new_format_mix_bytes;
        let dir = audio_test_install("shipped-art-pictures");
        let local = make_new_format_mix_bytes(&[
            ("artmd.ini", b"[GI]\nSequence=GISequence\n"),
            ("rulesmd.ini", b"[General]\nBuildSpeed=.7\n"),
        ]);
        std::fs::remove_file(dir.path().join("localmd.mix")).unwrap();
        std::fs::write(
            dir.path().join("ra2md.mix"),
            make_new_format_mix_bytes(&[("localmd.mix", &local)]),
        )
        .unwrap();
        let changed = || {
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            files_changing_shipped_inis(&assets, &["artmd.ini", "rulesmd.ini"])
        };
        let art = dir.path().join("artmd.ini");
        std::fs::write(
            &art,
            b"[GI]\nPicture=gi.png\nSequence=GISequence\nPictureSize=40,50\n\
              [APMUZZLE]\nPicture=apmuzzle.png\n",
        )
        .unwrap();
        assert!(changed().is_empty());
        std::fs::write(
            dir.path().join("rulesmd.ini"),
            b"[General]\nBuildSpeed=.7\nPicture=x.png\n",
        )
        .unwrap();
        assert_eq!(changed(), ["rulesmd.ini"]);
        std::fs::write(&art, b"[GI]\nPicture=gi.png\nSequence=Other\n").unwrap();
        assert_eq!(changed(), ["artmd.ini", "rulesmd.ini"]);
    }
}
