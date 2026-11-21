//! Declarative input for a series of authored campaign maps.
//!
//! This layer keeps map names, roles, branch edges, rule choices and carry
//! selections. Consumers resolve House identities against the constructed map.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::rules::ini_value::read_bool_decision;
use crate::rules::rule_policy::{
    AiBasePolicy, AllowWinTagsPolicy, AuthoredCratePolicy, BootstrapRuleProfile,
    ComputerParanoidPolicy, DoubleOwnedPolicy, RulePolicy, TeamLimitScope,
};
use crate::util::sha256::sha256_hex;

/// Limits both desktop streams and browser-imported manifests before parsing.
pub const MAX_CAMPAIGN_MANIFEST_BYTES: usize = 64 * 1024;

/// A campaign source index must fit in the nonnegative range of a signed byte.
pub const MAX_CAMPAIGN_PLAYERS: u8 = i8::MAX as u8 + 1;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{file}:{line} [{section}] {key}: {problem}. {hint}", file = .file.display())]
pub struct ManifestError {
    pub file: PathBuf,
    pub section: String,
    pub key: String,
    pub line: usize,
    pub problem: String,
    pub hint: String,
}

type Result<T> = std::result::Result<T, ManifestError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CampaignManifest {
    /// Lowercase SHA-256 of the exact bytes, including comments and line endings.
    pub digest: String,
    /// Declaration order supplies a stable zero-based stage index.
    pub stages: Vec<CampaignStage>,
    title: LocalizedTitle,
    series: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CampaignStage {
    pub id: String,
    pub map: String,
    pub map_sha256: Option<String>,
    /// Optional room admission bounds for playing connections.
    pub min_players: Option<u8>,
    pub max_players: Option<u8>,
    pub rule_policy: RulePolicy,
    /// Role declaration order supplies the stable zero-based source index.
    pub roles: Vec<CampaignRole>,
    pub grants: Vec<CampaignGrant>,
    /// Case-folded branch names and resolved destination stage indices.
    pub routes: BTreeMap<String, usize>,
    pub carry_over: StageCarrySettings,
    title: LocalizedTitle,
    file: Arc<Path>,
    map_location: Location,
    map_digest_location: Option<Location>,
    house_references: Vec<(String, Location)>,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignRole {
    pub name: String,
    pub house: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ManifestControlScope {
    FullControl,
    UnitsOnly,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignGrant {
    pub role: String,
    pub house: String,
    pub scope: ManifestControlScope,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StageCarrySettings {
    pub globals: bool,
    pub countdown: bool,
    pub map_stage: bool,
    pub houses: Vec<CarryHouse>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CarryHouse {
    pub house: String,
    pub credits: bool,
    pub difficulty: bool,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct LocalizedTitle {
    default: Option<String>,
    translations: BTreeMap<String, String>,
}

impl LocalizedTitle {
    fn get<'a>(&'a self, locale: &str, fallback: &'a str) -> &'a str {
        let locale = locale.to_ascii_lowercase();
        self.translations
            .get(&locale)
            .or_else(|| {
                locale
                    .split_once('-')
                    .and_then(|(language, _)| self.translations.get(language))
            })
            .map(String::as_str)
            .or(self.default.as_deref())
            .unwrap_or(fallback)
    }

    fn read(
        &mut self,
        key: &str,
        entry: Entry<'_>,
        section: &Section<'_>,
        path: &Path,
    ) -> Result<()> {
        if entry.value.is_empty() {
            return Err(section.error(path, entry, "empty title", "Supply visible title text"));
        }
        if key == "title" {
            self.default = Some(entry.value.to_owned());
            return Ok(());
        }
        let locale = key.strip_prefix("title.").unwrap_or_default();
        if !locale
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            return Err(section.error(
                path,
                entry,
                "invalid title locale or key",
                "Use Title or Title.<locale>, for example Title.zh-CN",
            ));
        }
        self.translations
            .insert(locale.to_owned(), entry.value.to_owned());
        Ok(())
    }
}

impl CampaignManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let input = std::fs::File::open(path);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let input = crate::util::file_system::read_shared(path).map(std::io::Cursor::new);
        let input = input.map_err(|error| {
            file_error(path, 1, error.to_string(), "Locate the campaign.ini file")
        })?;
        Self::read(path, input)
    }

    pub fn read(path: impl AsRef<Path>, input: impl Read) -> Result<Self> {
        let path = path.as_ref();
        let mut bytes = Vec::new();
        input
            .take(MAX_CAMPAIGN_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                file_error(path, 1, error.to_string(), "Check access to campaign.ini")
            })?;
        if bytes.len() > MAX_CAMPAIGN_MANIFEST_BYTES {
            return Err(file_error(
                path,
                line_after(&bytes[..MAX_CAMPAIGN_MANIFEST_BYTES]),
                "manifest exceeds 64 KiB",
                "Shorten campaign.ini or split the series",
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(|error| {
            file_error(
                path,
                line_after(&bytes[..error.valid_up_to()]),
                "invalid UTF-8",
                "Save campaign.ini as UTF-8",
            )
        })?;
        let digest = sha256_hex(&bytes);
        Self::parse(path, text, digest)
    }

    pub fn stage_index(&self, id: &str) -> Option<usize> {
        self.stages
            .iter()
            .position(|stage| stage.id.eq_ignore_ascii_case(id))
    }

    pub fn stage(&self, id: &str) -> Option<&CampaignStage> {
        self.stage_index(id)
            .and_then(|index| self.stages.get(index))
    }

    pub fn title(&self, locale: &str) -> &str {
        self.title.get(locale, &self.series)
    }

    pub fn stages_by_title(&self, locale: &str) -> Vec<&CampaignStage> {
        let mut stages = self.stages.iter().collect::<Vec<_>>();
        stages.sort_by_cached_key(|&stage| (stage.title(locale).to_lowercase(), stage.id.as_str()));
        stages
    }

    fn parse(path: &Path, text: &str, digest: String) -> Result<Self> {
        let sections = parse_sections(path, text.strip_prefix('\u{feff}').unwrap_or(text))?;
        let file: Arc<Path> = Arc::from(path);
        let mut stages = Vec::new();
        let mut indices = BTreeMap::new();
        let mut title = LocalizedTitle::default();

        for section in &sections {
            match section.kind {
                SectionKind::Campaign => {
                    for &entry in &section.entries {
                        title.read(&entry.key.to_ascii_lowercase(), entry, section, path)?;
                    }
                }
                SectionKind::Stage => {
                    indices.insert(section.id.to_ascii_lowercase(), stages.len());
                    stages.push(CampaignStage::read_header(&file, section)?);
                }
                _ => {}
            }
        }
        if stages.is_empty() {
            return Err(file_error(
                path,
                1,
                "no stages declared",
                "Add a [Stage <id>] section with a Map entry",
            ));
        }
        // Roles may appear after the Grants section. Resolve them first.
        for section in sections.iter().filter(|s| s.kind == SectionKind::Roles) {
            let stage = section.stage(path, &indices, &mut stages)?;
            for entry in &section.entries {
                if stage.roles.len() >= usize::from(MAX_CAMPAIGN_PLAYERS) {
                    return Err(section.error(
                        path,
                        *entry,
                        "too many roles",
                        "Keep at most 128 roles in a stage",
                    ));
                }
                if !identifier(entry.key) || !house_name(entry.value) {
                    return Err(section.error(
                        path,
                        *entry,
                        "invalid role or House name",
                        "Use RoleId=HouseId as named in the map",
                    ));
                }
                stage.roles.push(CampaignRole {
                    name: entry.key.to_owned(),
                    house: entry.value.to_owned(),
                });
                stage.track_house(section, *entry, entry.value);
            }
        }
        for section in sections.iter().filter(|s| {
            matches!(
                s.kind,
                SectionKind::Rules
                    | SectionKind::Grants
                    | SectionKind::Routes
                    | SectionKind::CarryOver
            )
        }) {
            let stage = section.stage(path, &indices, &mut stages)?;
            for &entry in &section.entries {
                match section.kind {
                    SectionKind::Rules => {
                        stage.rule_policy =
                            read_rule(stage.rule_policy, entry).map_err(|hint| {
                                section.error(path, entry, "invalid rule override", hint)
                            })?;
                    }
                    SectionKind::Grants => stage.read_grant(path, section, entry)?,
                    SectionKind::Routes => stage.read_route(path, section, entry, &indices)?,
                    SectionKind::CarryOver => stage.read_carry_over(path, section, entry)?,
                    _ => unreachable!("selected stage settings sections"),
                }
            }
        }
        let series = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("campaign")
            .to_owned();
        Ok(Self {
            digest,
            stages,
            title,
            series,
        })
    }
}

impl CampaignStage {
    pub fn title(&self, locale: &str) -> &str {
        self.title.get(locale, &self.id)
    }

    /// Validate the named Houses against the authored map before constructing
    /// its world. The first unresolved declaration retains its manifest site.
    pub fn validate_house_names<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Result<()> {
        let names = names
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();
        if let Some((house, location)) = self
            .house_references
            .iter()
            .filter(|(house, _)| !names.contains(&house.to_ascii_lowercase()))
            .min_by_key(|(_, location)| location.line)
        {
            return Err(location.error(
                &self.file,
                format!("House {house:?} is missing from map {:?}", self.map),
                "Use a House declared by the selected map",
            ));
        }
        Ok(())
    }

    pub fn map_error(&self, problem: impl Into<String>, hint: impl Into<String>) -> ManifestError {
        self.map_location.error(&self.file, problem, hint)
    }

    pub fn verify_map_digest(&self, actual: &str) -> Result<()> {
        if let Some(location) = self.map_digest_location.as_ref()
            && let Some(expected) = self.map_sha256.as_deref()
            && !expected.eq_ignore_ascii_case(actual)
        {
            return Err(location.error(
                &self.file,
                format!("map SHA-256 mismatch (expected {expected}, read {actual})"),
                "Restore the declared map or update MapSha256 for the intended map",
            ));
        }
        Ok(())
    }

    fn read_header(file: &Arc<Path>, section: &Section<'_>) -> Result<Self> {
        let path = file.as_ref();
        let mut stage = Self {
            id: section.id.to_owned(),
            map: String::new(),
            map_sha256: None,
            min_players: None,
            max_players: None,
            rule_policy: RulePolicy::stock_campaign().with_counts_player_control_as_human(false),
            roles: Vec::new(),
            grants: Vec::new(),
            routes: BTreeMap::new(),
            carry_over: StageCarrySettings::default(),
            title: LocalizedTitle::default(),
            file: Arc::clone(file),
            map_location: Location::new(
                section,
                Entry {
                    key: "Map",
                    value: "",
                    line: section.line,
                },
            ),
            map_digest_location: None,
            house_references: Vec::new(),
        };
        for &entry in &section.entries {
            let key = entry.key.to_ascii_lowercase();
            match key.as_str() {
                "map" if relative_map_name(entry.value) => {
                    stage.map = entry.value.to_owned();
                    stage.map_location = Location::new(section, entry);
                }
                "map" => {
                    return Err(section.error(
                        path,
                        entry,
                        "unsafe map path",
                        "Use a relative path below the series directory, with / separators",
                    ));
                }
                "mapsha256" => {
                    let digest = entry.value.to_ascii_lowercase();
                    if !crate::map::source::valid_source_sha256(&digest) {
                        return Err(section.error(
                            path,
                            entry,
                            "invalid SHA-256",
                            "Use the 64 hexadecimal characters of the authored map file's SHA-256",
                        ));
                    }
                    stage.map_sha256 = Some(digest);
                    stage.map_digest_location = Some(Location::new(section, entry));
                }
                "minplayers" | "maxplayers" => {
                    let count = entry
                        .value
                        .parse::<u8>()
                        .ok()
                        .filter(|&count| (1..=MAX_CAMPAIGN_PLAYERS).contains(&count))
                        .ok_or_else(|| {
                            section.error(
                                path,
                                entry,
                                "invalid player count",
                                "Enter a whole number between 1 and 128",
                            )
                        })?;
                    if key == "minplayers" {
                        stage.min_players = Some(count);
                    } else {
                        stage.max_players = Some(count);
                    }
                    if stage
                        .min_players
                        .zip(stage.max_players)
                        .is_some_and(|(min, max)| min > max)
                    {
                        return Err(section.error(
                            path,
                            entry,
                            "MinPlayers exceeds MaxPlayers",
                            "Set MinPlayers at or below MaxPlayers",
                        ));
                    }
                }
                "title" => stage.title.read(&key, entry, section, path)?,
                _ if key.starts_with("title.") => stage.title.read(&key, entry, section, path)?,
                _ => {
                    return Err(section.error(
                        path,
                        entry,
                        "unknown stage key",
                        "Use Map, MapSha256, MinPlayers, MaxPlayers, Title or Title.<locale>",
                    ));
                }
            }
        }
        if stage.map.is_empty() {
            return Err(section.header_error(
                path,
                "Map",
                "stage has no map",
                "Add Map=<relative path>",
            ));
        }
        Ok(stage)
    }

    fn track_house(&mut self, section: &Section<'_>, entry: Entry<'_>, house: &str) {
        self.house_references
            .push((house.to_owned(), Location::new(section, entry)));
    }

    fn read_grant(&mut self, path: &Path, section: &Section<'_>, entry: Entry<'_>) -> Result<()> {
        let (role, house) = entry.key.split_once('.').ok_or_else(|| {
            section.error(
                path,
                entry,
                "invalid grant key",
                "Use RoleId.HouseId=FullControl or UnitsOnly",
            )
        })?;
        let role = self
            .roles
            .iter()
            .find(|known| known.name.eq_ignore_ascii_case(role))
            .ok_or_else(|| {
                section.error(
                    path,
                    entry,
                    "grant names an undeclared role",
                    "Add this role to [Roles <stage>]",
                )
            })?;
        if !house_name(house) || role.house.eq_ignore_ascii_case(house) {
            return Err(section.error(
                path,
                entry,
                "grant does not name a distinct target House",
                "Name another House from the map. Control of the role's own House is automatic",
            ));
        }
        let scope = choice(
            entry.value,
            [
                ("FullControl", ManifestControlScope::FullControl),
                ("UnitsOnly", ManifestControlScope::UnitsOnly),
            ],
        )
        .ok_or_else(|| {
            section.error(
                path,
                entry,
                "invalid grant scope",
                "Choose FullControl or UnitsOnly",
            )
        })?;
        self.grants.push(CampaignGrant {
            role: role.name.clone(),
            house: house.to_owned(),
            scope,
        });
        self.track_house(section, entry, house);
        Ok(())
    }

    fn read_route(
        &mut self,
        path: &Path,
        section: &Section<'_>,
        entry: Entry<'_>,
        indices: &BTreeMap<String, usize>,
    ) -> Result<()> {
        if !identifier(entry.key) {
            return Err(section.error(
                path,
                entry,
                "invalid branch name",
                "Use an identifier such as Win or Retry",
            ));
        }
        if entry.key.eq_ignore_ascii_case("Retry") && !entry.value.eq_ignore_ascii_case(&self.id) {
            return Err(section.error(
                path,
                entry,
                "Retry must restart the current stage",
                "Name the current stage or use a different branch name",
            ));
        }
        let index = indices
            .get(&entry.value.to_ascii_lowercase())
            .ok_or_else(|| {
                section.error(
                    path,
                    entry,
                    "route targets an undeclared stage",
                    "Name an existing [Stage <id>] section",
                )
            })?;
        self.routes.insert(entry.key.to_ascii_lowercase(), *index);
        Ok(())
    }

    fn read_carry_over(
        &mut self,
        path: &Path,
        section: &Section<'_>,
        entry: Entry<'_>,
    ) -> Result<()> {
        let key = entry.key.to_ascii_lowercase();
        let setting = match key.as_str() {
            "globals" => &mut self.carry_over.globals,
            "countdown" => &mut self.carry_over.countdown,
            "mapstage" => &mut self.carry_over.map_stage,
            _ if key.starts_with("house.") => {
                let house = &entry.key["house.".len()..];
                if !house_name(house) {
                    return Err(section.error(
                        path,
                        entry,
                        "invalid House name",
                        "Use House.<HouseId> from the map",
                    ));
                }
                let mut carry = CarryHouse {
                    house: house.to_owned(),
                    credits: false,
                    difficulty: false,
                };
                for field in entry.value.split(',').map(str::trim) {
                    let enabled = match field.to_ascii_lowercase().as_str() {
                        "credits" => &mut carry.credits,
                        "difficulty" => &mut carry.difficulty,
                        _ => {
                            return Err(section.error(
                                path,
                                entry,
                                "invalid House carry field",
                                "Select Credits, Difficulty, or both",
                            ));
                        }
                    };
                    if *enabled {
                        return Err(section.error(
                            path,
                            entry,
                            "duplicate House carry field",
                            "List each selected field once",
                        ));
                    }
                    *enabled = true;
                }
                self.carry_over.houses.push(carry);
                self.track_house(section, entry, house);
                return Ok(());
            }
            _ => {
                return Err(section.error(
                    path,
                    entry,
                    "unknown carry setting",
                    "Use Globals, Countdown, MapStage, or House.<HouseId>",
                ));
            }
        };
        *setting = boolean(entry.value).ok_or_else(|| {
            section.error(path, entry, "invalid carry setting", "Choose Yes or No")
        })?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Location {
    section: String,
    key: String,
    line: usize,
}

impl Location {
    fn new(section: &Section<'_>, entry: Entry<'_>) -> Self {
        Self {
            section: section.name.to_owned(),
            key: entry.key.to_owned(),
            line: entry.line,
        }
    }

    fn error(
        &self,
        path: &Path,
        problem: impl Into<String>,
        hint: impl Into<String>,
    ) -> ManifestError {
        ManifestError {
            file: path.to_owned(),
            section: self.section.clone(),
            key: self.key.clone(),
            line: self.line,
            problem: problem.into(),
            hint: hint.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum SectionKind {
    Campaign,
    Stage,
    Roles,
    Rules,
    Grants,
    Routes,
    CarryOver,
}

struct Section<'a> {
    name: &'a str,
    kind: SectionKind,
    id: &'a str,
    line: usize,
    entries: Vec<Entry<'a>>,
}

#[derive(Clone, Copy)]
struct Entry<'a> {
    key: &'a str,
    value: &'a str,
    line: usize,
}

impl Section<'_> {
    fn error(
        &self,
        path: &Path,
        entry: Entry<'_>,
        problem: impl Into<String>,
        hint: &'static str,
    ) -> ManifestError {
        Location::new(self, entry).error(path, problem, hint)
    }

    fn header_error(
        &self,
        path: &Path,
        key: &str,
        problem: &'static str,
        hint: &'static str,
    ) -> ManifestError {
        self.error(
            path,
            Entry {
                key,
                value: "",
                line: self.line,
            },
            problem,
            hint,
        )
    }

    fn stage<'a>(
        &self,
        path: &Path,
        indices: &BTreeMap<String, usize>,
        stages: &'a mut [CampaignStage],
    ) -> Result<&'a mut CampaignStage> {
        indices
            .get(&self.id.to_ascii_lowercase())
            .and_then(|&index| stages.get_mut(index))
            .ok_or_else(|| {
                self.header_error(
                    path,
                    "<section>",
                    "section names an undeclared stage",
                    "Add the corresponding [Stage <id>] section",
                )
            })
    }
}

fn parse_sections<'a>(path: &Path, text: &'a str) -> Result<Vec<Section<'a>>> {
    let mut sections: Vec<Section<'_>> = Vec::new();
    let mut seen_sections = BTreeSet::new();
    let mut seen_entries = BTreeSet::new();
    for (number, raw) in text.lines().enumerate() {
        let line = number + 1;
        let text = raw.split(';').next().unwrap_or_default().trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if let Some(header) = text.strip_prefix('[') {
            let name = header.strip_suffix(']').unwrap_or(header).trim();
            let mut parts = name.split_whitespace();
            let word = parts.next().unwrap_or_default();
            let id = parts.next().unwrap_or_default();
            let kind = choice(
                word,
                [
                    ("Campaign", SectionKind::Campaign),
                    ("Stage", SectionKind::Stage),
                    ("Roles", SectionKind::Roles),
                    ("Rules", SectionKind::Rules),
                    ("Grants", SectionKind::Grants),
                    ("Routes", SectionKind::Routes),
                    ("CarryOver", SectionKind::CarryOver),
                ],
            );
            let invalid_header = || ManifestError {
                file: path.to_owned(),
                section: name.to_owned(),
                key: "<section>".into(),
                line,
                problem: "invalid section header".into(),
                hint: "Use [Campaign] or a stage section such as [Stage Opening]".into(),
            };
            let kind = kind.ok_or_else(invalid_header)?;
            if !text.ends_with(']')
                || parts.next().is_some()
                || (if kind == SectionKind::Campaign {
                    !id.is_empty()
                } else {
                    !identifier(id)
                })
            {
                return Err(invalid_header());
            }
            let section = Section {
                name,
                kind,
                id,
                line,
                entries: Vec::new(),
            };
            if !seen_sections.insert((kind, id.to_ascii_lowercase())) {
                return Err(section.header_error(
                    path,
                    "<section>",
                    "duplicate section",
                    "Combine the sections into a single declaration",
                ));
            }
            sections.push(section);
            seen_entries.clear();
            continue;
        }
        let section = sections.last_mut().ok_or_else(|| {
            file_error(
                path,
                line,
                "entry outside a section",
                "Declare a section header before writing entries",
            )
        })?;
        let (key, value) = text.split_once('=').ok_or_else(|| {
            section.error(
                path,
                Entry {
                    key: text,
                    value: "",
                    line,
                },
                "entry has no equals sign",
                "Write Key=Value",
            )
        })?;
        let entry = Entry {
            key: key.trim(),
            value: value.trim(),
            line,
        };
        if entry.key.is_empty() {
            return Err(section.error(
                path,
                entry,
                "empty key",
                "Write a key before the equals sign",
            ));
        }
        if !seen_entries.insert(entry.key.to_ascii_lowercase()) {
            return Err(section.error(path, entry, "duplicate key", "Keep one value for this key"));
        }
        section.entries.push(entry);
    }
    Ok(sections)
}

fn file_error(
    path: &Path,
    line: usize,
    problem: impl Into<String>,
    hint: &'static str,
) -> ManifestError {
    ManifestError {
        file: path.to_owned(),
        section: "<manifest>".into(),
        key: "<file>".into(),
        line,
        problem: problem.into(),
        hint: hint.into(),
    }
}

fn line_after(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte == b'\n').count() + 1
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn house_name(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '[' | ']' | ',' | '='))
}

fn relative_map_name(value: &str) -> bool {
    crate::map::source::is_logical_name(value)
        && !value.contains(['\\', '<', '>', '"', '|', '?', '*'])
        && value
            .split('/')
            .all(|part| !part.starts_with(' ') && !part.ends_with(['.', ' ']))
}

fn boolean(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "no" | "true" | "false" | "0" | "1" => read_bool_decision(value),
        _ => None,
    }
}

fn choice<T>(raw: &str, values: impl IntoIterator<Item = (&'static str, T)>) -> Option<T> {
    values
        .into_iter()
        .find_map(|(name, value)| raw.eq_ignore_ascii_case(name).then_some(value))
}

fn read_rule(
    policy: RulePolicy,
    entry: Entry<'_>,
) -> std::result::Result<RulePolicy, &'static str> {
    let raw = entry.value;
    let key = entry.key.to_ascii_lowercase();
    let setter: fn(RulePolicy, bool) -> RulePolicy = match key.as_str() {
        "bootstrap" => {
            return raw
                .eq_ignore_ascii_case("Campaign")
                .then(|| policy.with_bootstrap(BootstrapRuleProfile::Campaign))
                .ok_or("Use Campaign for an authored [Houses] roster");
        }
        "allowwintags" => {
            return choice(
                raw,
                [
                    ("RequireClear", AllowWinTagsPolicy::RequireClear),
                    ("Ignore", AllowWinTagsPolicy::Ignore),
                ],
            )
            .map(|value| policy.with_allow_win_tags(value))
            .ok_or("Choose RequireClear or Ignore");
        }
        "computerparanoid" => {
            return choice(
                raw,
                [
                    ("Disabled", ComputerParanoidPolicy::Disabled),
                    ("React", ComputerParanoidPolicy::React),
                ],
            )
            .map(|value| policy.with_computer_paranoid(value))
            .ok_or("Choose Disabled or React");
        }
        "crates" => {
            return choice(
                raw,
                [
                    ("Keep", AuthoredCratePolicy::Keep),
                    ("Skip", AuthoredCratePolicy::Skip),
                ],
            )
            .map(|value| policy.with_crates(value))
            .ok_or("Choose Keep or Skip");
        }
        "aibase" => {
            return choice(
                raw,
                [
                    ("RetainAuthoredNodes", AiBasePolicy::RetainAuthoredNodes),
                    (
                        "ReplaceUnavailableNodes",
                        AiBasePolicy::ReplaceUnavailableNodes,
                    ),
                ],
            )
            .map(|value| policy.with_ai_base(value))
            .ok_or("Choose RetainAuthoredNodes or ReplaceUnavailableNodes");
        }
        "doubleowned" => {
            return choice(
                raw,
                [
                    ("OwnerMask", DoubleOwnedPolicy::OwnerMask),
                    ("AllCountries", DoubleOwnedPolicy::AllCountries),
                ],
            )
            .map(|value| policy.with_double_owned(value))
            .ok_or("Choose OwnerMask or AllCountries");
        }
        "teamcreationlimitscope" => {
            return choice(
                raw,
                [
                    ("AllHouses", TeamLimitScope::AllHouses),
                    ("PerHouse", TeamLimitScope::PerHouse),
                ],
            )
            .map(|value| policy.with_team_creation_limit_scope(value))
            .ok_or("Choose AllHouses or PerHouse");
        }
        "allowsconstructionyardredeploy" => RulePolicy::with_allows_construction_yard_redeploy,
        "allowsidleobjectsleep" => RulePolicy::with_allows_idle_object_sleep,
        "allowsownerlessterrainlogic" => RulePolicy::with_allows_ownerless_terrain_logic,
        "countsplayercontrolashuman" => {
            return match boolean(raw) {
                Some(false) => Ok(policy.with_counts_player_control_as_human(false)),
                Some(true) => Err("Use No. Assign human-controlled Houses in [Roles <stage>]"),
                None => Err("Choose No for an authored multiplayer campaign"),
            };
        }
        "usescountrydifficultybiases" => RulePolicy::with_uses_country_difficulty_biases,
        "skipslocalplayerauthoredbuildings" => {
            RulePolicy::with_skips_local_player_authored_buildings
        }
        "usesmanagedaibase" => RulePolicy::with_uses_managed_ai_base,
        "usesaieconomy" => RulePolicy::with_uses_ai_economy,
        "runsaistrategy" => RulePolicy::with_runs_ai_strategy,
        "useshouseteamrules" => RulePolicy::with_uses_house_team_rules,
        "usescampaigndiscovery" => RulePolicy::with_uses_campaign_discovery,
        "usescampaignbasedefense" => RulePolicy::with_uses_campaign_base_defense,
        "usesdirectedhunterseekertargeting" => {
            RulePolicy::with_uses_directed_hunter_seeker_targeting
        }
        "usesmatchcrates" => RulePolicy::with_uses_match_crates,
        "useseliminationresults" => RulePolicy::with_uses_elimination_results,
        "usesroomoptions" => RulePolicy::with_uses_room_options,
        "usescampaigndiplomacy" => RulePolicy::with_uses_campaign_diplomacy,
        "allowsmanualalliances" => RulePolicy::with_allows_manual_alliances,
        "modmapadditions" => RulePolicy::with_mod_map_additions,
        "sharedalliedvictory" => RulePolicy::with_shared_allied_victory,
        "skipsnetworkscoredialog" => RulePolicy::with_skips_network_score_dialog,
        "paranoidignoresalliesallowed" => RulePolicy::with_paranoid_ignores_allies_allowed,
        "networkdiplomacybriefing" => RulePolicy::with_network_diplomacy_briefing,
        "startinghouserolealliances" => RulePolicy::with_starting_house_role_alliances,
        "arespsychicrevealexclusions" => RulePolicy::with_ares_psychic_reveal_exclusions,
        "dissolveunfilledteams" => RulePolicy::with_dissolve_unfilled_teams,
        _ => return Err("Choose a rule field listed in docs/campaign-manifest.md"),
    };
    boolean(raw)
        .map(|value| setter(policy, value))
        .ok_or("Choose Yes or No")
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;

    const FILE: &str = "campaigns/demo/campaign.ini";

    #[test]
    fn a_minimal_stage_inherits_campaign_rules_with_a_stable_digest_and_index() {
        let text = b"[Stage Opening]\nMap=maps/opening.map\n";
        let manifest = CampaignManifest::read(FILE, text.as_slice()).unwrap();
        assert_eq!(
            manifest.digest,
            "ab05505dbe932edb7087dd5e8f6c671dcc4599c52ef6fe2686af12c4e53a2ac2"
        );
        assert_eq!(manifest.stage_index("oPeNiNg"), Some(0));
        assert_eq!(manifest.stage_index("Closing"), None);
        let stage = manifest.stage("opening").unwrap();
        assert_eq!(stage.map, "maps/opening.map");
        assert_eq!(
            stage.rule_policy,
            RulePolicy::stock_campaign().with_counts_player_control_as_human(false)
        );
        assert_eq!((stage.min_players, stage.max_players), (None, None));
        assert_eq!(manifest.title("en"), "demo");
        assert_eq!(stage.title("en"), "Opening");
        assert!(stage.roles.is_empty() && stage.grants.is_empty() && stage.routes.is_empty());
        assert_eq!(stage.carry_over, StageCarrySettings::default());
    }

    #[test]
    fn staged_overrides_preserve_role_order_grants_routes_and_localized_titles() {
        let manifest = CampaignManifest::read(
            FILE,
            "[Campaign]\nTitle=Series\nTitle.de=Reihe\n\
             [Stage Opening]\nMap=open.map\nTitle=Zulu\nTitle.de=Anfang\nMinPlayers=1\nMaxPlayers=2\n\
             [Grants Opening]\nLEAD.Soviet=FullControl\nSupport.Soviet=UnitsOnly\nLead.Neutral=UnitsOnly\n\
             [Rules Opening]\nAllowWinTags=Ignore\nAiBase=ReplaceUnavailableNodes\nUsesMatchCrates=Yes\n\
             [Routes OPENING]\nWin=closing\nRetry=opening\n\
             [Roles Opening]\nLead=Allied\nSupport=Allied\n\
             [CarryOver Opening]\nGlobals=Yes\nCountdown=Yes\nHouse.Allied=Credits,Difficulty\nHouse.Soviet=Credits\n\
             [Stage Closing]\nMap=close.map\nTitle=Alpha\nTitle.de=Ziel\n"
                .as_bytes(),
        )
        .unwrap();
        let stage = manifest.stage("Opening").unwrap();
        assert_eq!(
            stage.rule_policy,
            RulePolicy::stock_campaign()
                .with_counts_player_control_as_human(false)
                .with_allow_win_tags(AllowWinTagsPolicy::Ignore)
                .with_ai_base(AiBasePolicy::ReplaceUnavailableNodes)
                .with_uses_match_crates(true)
        );
        assert_eq!((stage.min_players, stage.max_players), (Some(1), Some(2)));
        assert_eq!(
            stage
                .roles
                .iter()
                .map(|role| (role.name.as_str(), role.house.as_str()))
                .collect::<Vec<_>>(),
            [("Lead", "Allied"), ("Support", "Allied")]
        );
        assert_eq!(
            stage
                .grants
                .iter()
                .map(|grant| (grant.role.as_str(), grant.house.as_str(), grant.scope))
                .collect::<Vec<_>>(),
            [
                ("Lead", "Soviet", ManifestControlScope::FullControl),
                ("Support", "Soviet", ManifestControlScope::UnitsOnly),
                ("Lead", "Neutral", ManifestControlScope::UnitsOnly),
            ]
        );
        assert_eq!(stage.routes.get("win"), Some(&1));
        assert_eq!(stage.routes.get("retry"), Some(&0));
        assert!(stage.carry_over.globals && stage.carry_over.countdown);
        assert_eq!(
            stage.carry_over.houses,
            [
                CarryHouse {
                    house: "Allied".into(),
                    credits: true,
                    difficulty: true
                },
                CarryHouse {
                    house: "Soviet".into(),
                    credits: true,
                    difficulty: false
                },
            ]
        );
        assert_eq!(manifest.title("DE-at"), "Reihe");
        assert_eq!(manifest.stages_by_title("de-AT")[0].id, "Opening");
        assert_eq!(manifest.stages_by_title("en")[0].id, "Closing");
    }

    #[test]
    fn retry_cannot_point_to_another_stage() {
        let text = b"[Stage Opening]\nMap=open.map\n[Stage Closing]\nMap=close.map\n[Routes Opening]\nRetry=closing\n";
        let error = CampaignManifest::read(FILE, text.as_slice()).unwrap_err();
        assert_eq!(
            (error.section.as_str(), error.key.as_str(), error.line),
            ("Routes Opening", "Retry", 6)
        );
        assert!(error.problem.contains("current stage"));
    }

    #[test]
    fn oversized_reader_stops_at_the_first_byte_over_the_shared_limit() {
        let total = MAX_CAMPAIGN_MANIFEST_BYTES as u64 * 2;
        let mut input = std::io::repeat(b' ').take(total);
        let error = CampaignManifest::read(FILE, &mut input).unwrap_err();
        assert_eq!(
            input.limit(),
            total - MAX_CAMPAIGN_MANIFEST_BYTES as u64 - 1
        );
        assert_eq!(error.file, Path::new(FILE));
        assert_eq!(error.line, 1);
        assert_eq!(error.problem, "manifest exceeds 64 KiB");
    }

    #[test]
    fn invalid_declarations_report_the_original_section_key_and_line() {
        let error = CampaignManifest::read(
            FILE,
            b"[Stage Opening]\r\nMap=open.map\r\n\r\n[Grants Opening]\r\nLead.Soviet=UnitsOnly\r\n"
                .as_slice(),
        )
        .unwrap_err();
        assert_eq!(error.file, Path::new(FILE));
        assert_eq!(
            (error.section.as_str(), error.key.as_str(), error.line),
            ("Grants Opening", "Lead.Soviet", 5)
        );
        assert_eq!(error.hint, "Add this role to [Roles <stage>]");

        let overlong = format!("{}.map", "a".repeat(1021));
        for unsafe_path in [
            "../open.map",
            "/open.map",
            "C:/open.map",
            "maps/CON.map",
            "maps/CONIN$.map",
            "maps/COM0.map",
            "maps/LPT0.map",
            &overlong,
        ] {
            let text = format!("[Stage Opening]\nMap={unsafe_path}\n");
            let error = CampaignManifest::read(FILE, text.as_bytes()).unwrap_err();
            assert_eq!(
                (error.section.as_str(), error.key.as_str(), error.line),
                ("Stage Opening", "Map", 2)
            );
            assert_eq!(error.problem, "unsafe map path");
        }
    }

    #[test]
    fn authored_campaign_roles_cannot_enable_map_player_control() {
        let prefix = "[Stage Opening]\nMap=open.map\n[Rules Opening]\n";
        let refused = format!("{prefix}CountsPlayerControlAsHuman=Yes\n");
        let error = CampaignManifest::read(FILE, refused.as_bytes()).unwrap_err();
        assert_eq!(
            (error.section.as_str(), error.key.as_str(), error.line),
            ("Rules Opening", "CountsPlayerControlAsHuman", 4)
        );
        assert!(error.hint.contains("[Roles <stage>]"));

        let allowed = format!("{prefix}CountsPlayerControlAsHuman=No\n");
        let manifest = CampaignManifest::read(FILE, allowed.as_bytes()).unwrap();
        assert!(
            !manifest.stages[0]
                .rule_policy
                .counts_player_control_as_human()
        );
    }

    #[test]
    fn player_bounds_fail_at_the_declaration_that_invalidates_them() {
        let error = CampaignManifest::read(
            FILE,
            b"[Stage Opening]\nMap=open.map\nMaxPlayers=2\nMinPlayers=3\n".as_slice(),
        )
        .unwrap_err();
        assert_eq!((error.key.as_str(), error.line), ("MinPlayers", 4));
        assert_eq!(error.hint, "Set MinPlayers at or below MaxPlayers");
    }

    #[test]
    fn a_map_digest_refusal_points_to_its_declaration() {
        let supplied = "AB".repeat(32);
        let text = format!("[Stage Opening]\nMap=open.map\nMapSha256={supplied}\n");
        let manifest = CampaignManifest::read(FILE, text.as_bytes()).unwrap();
        let stage = manifest.stage("opening").unwrap();
        let normalized = supplied.to_ascii_lowercase();
        assert_eq!(stage.map_sha256.as_deref(), Some(normalized.as_str()));
        stage.verify_map_digest(&normalized).unwrap();
        let error = stage.verify_map_digest(&"00".repeat(32)).unwrap_err();
        assert_eq!(
            (
                error.file.as_path(),
                error.section.as_str(),
                error.key.as_str(),
                error.line
            ),
            (Path::new(FILE), "Stage Opening", "MapSha256", 3)
        );
        assert!(error.hint.contains("Restore the declared map"));
    }

    #[test]
    fn unknown_map_house_and_map_load_errors_point_to_the_manifest_entries() {
        let manifest = CampaignManifest::read(
            FILE,
            b"[Stage Opening]\nTitle=First\nMap=open.map\n\
              [Roles Opening]\nLead=Americans\n\
              [Grants Opening]\nLead.Russians=UnitsOnly\n\
              [CarryOver Opening]\nHouse.Brtiish=Credits\n"
                .as_slice(),
        )
        .unwrap();
        let stage = manifest.stage("Opening").unwrap();
        let error = stage
            .validate_house_names(["AMERICANS", "Russians", "British"])
            .unwrap_err();
        assert_eq!(
            (
                error.file.as_path(),
                error.section.as_str(),
                error.key.as_str(),
                error.line
            ),
            (Path::new(FILE), "CarryOver Opening", "House.Brtiish", 9),
        );
        assert!(error.hint.contains("House declared"));
        stage
            .validate_house_names(["Americans", "russians", "brtiish"])
            .unwrap();

        let error = stage.map_error("map could not be opened", "Check the map file");
        assert_eq!(
            (
                error.file.as_path(),
                error.section.as_str(),
                error.key.as_str(),
                error.line
            ),
            (Path::new(FILE), "Stage Opening", "Map", 3),
        );
    }
}
