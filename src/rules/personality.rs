//! Commander personalities: a named view over `[AITriggerTypes]` plus optional
//! skirmish feedback entries.
//!
//! Product extension modelled on Red Alert 3's `AIPersonalityDefinition`
//! (`Schemas/xsd/SkirmishAI/AssetTypeAIPersonalityDefinition.xsd` in the RA3
//! MOD SDK). It carries no gamemd evidence: gamemd has neither personalities nor
//! these videos, so nothing here may cite a native address or claim restoration.
//!
//! A personality is data only. It records which AI triggers it suppresses, how
//! much the remaining ones weigh against each other, and — separately — which
//! short feedback clips it may show. Reading it changes nothing; the skirmish
//! house projection ([`crate::sim::house_state::HouseState`]) and the AI team
//! selector apply it, and an empty catalog leaves every path at its native
//! value.
//!
//! ```ini
//! [AIPersonalities]
//! 0=AlliedBalanced
//! 1=SovietArmor
//!
//! [AlliedBalanced]
//! Country=Americans,British,French,Germans,Alliance
//! TriggerDeny=0CA2F7C0-G
//! TriggerWeight.0CA2F7C0-G=150
//! Entry0.Key=StartGame
//! Entry0.Video=AWAGR1
//! Entry0.To=ALLIES
//! ```
//!
//! Assignment is opt-in. A house with no requested commander keeps `None` and
//! behaves exactly as retail's AI does; [`PersonalityCatalog::draw_for`] exists
//! for a selector's "random" button, not for the launch path.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::ini_parser::{IniFile, IniSection};

/// Every `[<Personality>]` section `[AIPersonalities]` names, in registration
/// order. Which house a personality is *eligible* for is a per-definition
/// question ([`PersonalityDef::admits`]); the catalog keeps no country table, so
/// a country that appears only in a map or a mod still matches its own name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonalityCatalog {
    definitions: Vec<PersonalityDef>,
}

impl PersonalityCatalog {
    /// The reserved personality every rules set carries: no country filter, no
    /// trigger deny, no trigger weights and no clips. A house given it plays
    /// exactly the AI retail would and never shows a talking head.
    ///
    /// It is never drawn: [`Self::candidates_for`] skips it, so a match
    /// never hands it to an opponent by chance. It exists to be *named* — by a
    /// future selector, by `spawn.ini`, or by a map that ships preset AI and
    /// has no business playing commander clips (tower defence, for one).
    pub const STANDARD: &'static str = "Standard";

    /// No `[AIPersonalities]` in any rules layer: the whole feature is absent,
    /// and skirmish AI behaves exactly as it does without this module. The
    /// reserved entry does not count — a catalog holding only it is as good as
    /// no catalog.
    pub fn is_empty(&self) -> bool {
        !self
            .definitions
            .iter()
            .any(|definition| !definition.reserved)
    }

    /// Personalities the draw may hand out. Reserved entries are excluded; see
    /// [`Self::STANDARD`].
    pub fn len(&self) -> usize {
        self.definitions
            .iter()
            .filter(|definition| !definition.reserved)
            .count()
    }

    /// Registration order is the stable identity a save file stores.
    pub fn get(&self, index: u16) -> Option<&PersonalityDef> {
        self.definitions.get(usize::from(index))
    }

    /// The personalities a house of `country` (a `[Countries]` key) may be
    /// assigned, in registration order. A definition with no `Country=` is a
    /// candidate for every country.
    ///
    /// A selector's dropdown is this same list — the seam is the function, not
    /// a second API that could drift from the draw's eligibility.
    pub fn candidates_for(&self, country: &str) -> Vec<u16> {
        self.definitions
            .iter()
            .enumerate()
            .filter(|(_index, definition)| !definition.reserved && definition.admits(country))
            .filter_map(|(index, _definition)| u16::try_from(index).ok())
            .collect()
    }

    /// The personality registered under `name`, spelled in any case. This is
    /// the handle an override uses; [`Self::draw_for`] does not.
    pub fn index_of_name(&self, name: &str) -> Option<u16> {
        let index = self
            .definitions
            .iter()
            .position(|definition| definition.name.eq_ignore_ascii_case(name))?;
        u16::try_from(index).ok()
    }

    /// The reserved no-clip personality, present in every catalog.
    pub fn standard(&self) -> Option<u16> {
        self.index_of_name(Self::STANDARD)
    }

    /// A personality for a house of `country`, chosen from
    /// [`Self::candidates_for`] by `key`.
    ///
    /// This is a selector's "random" button, not the launch path: a house that
    /// requested nothing gets nothing. `key` must be stable across every client
    /// and every replay of one match — a slot index and its country, never a
    /// frame counter or a random draw. It deliberately consumes no RNG:
    /// assignment happens while the scenario is built, and drawing from the
    /// shared stream there would shift every later AI decision and invalidate
    /// existing replays. A caller packing one is describing facts the shell
    /// already fixed; country, colour and house index are the three.
    pub fn draw_for(&self, country: &str, key: u64) -> Option<u16> {
        let candidates = self.candidates_for(country);
        if candidates.is_empty() {
            return None;
        }
        let pick = (fnv1a64(key) % candidates.len() as u64) as usize;
        Some(candidates[pick])
    }

    fn push(&mut self, definition: PersonalityDef) {
        debug_assert!(
            u16::try_from(self.definitions.len()).is_ok(),
            "[AIPersonalities] exceeds u16 identity space"
        );
        self.definitions.push(definition);
    }
}

/// One `[<Personality>]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalityDef {
    /// The registry ID that named this section, as spelled in
    /// `[AIPersonalities]`.
    pub name: String,
    /// Uppercase `[Countries]` keys that may use this personality, written the
    /// way `Owner=` is written in `rules.ini`. `None` (`Country=` absent, or
    /// `*` / `All`) accepts every country.
    pub countries: Option<BTreeSet<String>>,
    /// Localized-name key for a future selector; unused by the simulation.
    pub display_name: Option<String>,
    /// Localized one-line description for a future selector; unused by the
    /// simulation. CSF key or plain text, exactly like `DisplayName=`.
    pub display_desc: Option<String>,
    /// `TriggerRatio=` — the house's `+0x565C` starting value, the percent
    /// chance a team-creation pass picks an AI trigger at all. `100` is native.
    /// A map's `RatioAITriggerTeam=` and trigger op 76 still win.
    pub trigger_ratio: i32,
    /// `TeamDelayPercent=` — scale on `[General] TeamDelays=`. `100` is native;
    /// below is a faster, above a slower opponent.
    pub team_delay_percent: u16,
    /// True for a personality the draw must skip: it is reachable only by name
    /// ([`PersonalityCatalog::index_of_name`]). See
    /// [`PersonalityCatalog::STANDARD`].
    pub reserved: bool,
    /// Uppercase `AITriggerTypes` IDs this personality never picks.
    pub deny: BTreeSet<String>,
    /// Uppercase `AITriggerTypes` ID -> percent of that trigger's own weight.
    /// Absent means `100`: the trigger keeps the weight the trigger table gives
    /// it, including whatever decay the running record has accumulated.
    pub trigger_weight_percent: BTreeMap<String, u16>,
    /// Ordered feedback entries (the optional clip layer).
    pub feedback: Vec<FeedbackEntry>,
}

impl Default for PersonalityDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            countries: None,
            display_name: None,
            display_desc: None,
            trigger_ratio: NATIVE_TRIGGER_RATIO,
            team_delay_percent: NEUTRAL_WEIGHT_PERCENT,
            reserved: false,
            deny: BTreeSet::new(),
            trigger_weight_percent: BTreeMap::new(),
            feedback: Vec::new(),
        }
    }
}

impl PersonalityDef {
    /// Whether a house of `country` (a `[Countries]` key) may use this
    /// personality. `Country=` is the owner-list rule from `rules.ini`: an exact
    /// key match, no side expansion, no parent-country inheritance. A
    /// personality that writes no `Country=` accepts every country.
    pub fn admits(&self, country: &str) -> bool {
        self.countries
            .as_ref()
            .is_none_or(|countries| countries.contains(&country.to_ascii_uppercase()))
    }

    /// The percent this personality applies to `trigger_id` (`AITriggerTypes`
    /// section name, any case), or `None` to keep the native weight.
    pub fn weight_percent(&self, trigger_id: &str) -> Option<u16> {
        self.trigger_weight_percent
            .get(&trigger_id.to_ascii_uppercase())
            .copied()
    }

    /// Whether this personality suppresses `trigger_id` outright.
    pub fn denies(&self, trigger_id: &str) -> bool {
        !self.deny.is_empty() && self.deny.contains(&trigger_id.to_ascii_uppercase())
    }

    /// The weight the trigger table's `native` weight takes under this
    /// personality, or `None` when the personality drops the trigger.
    ///
    /// `priority_weight` is the AI selector's "clear the candidates" sentinel
    /// (`0x006F0C72`, 5000). It marks precedence rather than magnitude, so a
    /// percentage would destroy its meaning and the sentinel passes through
    /// unchanged.
    pub fn effective_weight(
        &self,
        trigger_id: &str,
        native: i32,
        priority_weight: i32,
    ) -> Option<i32> {
        if self.denies(trigger_id) {
            return None;
        }
        if native == priority_weight {
            return Some(native);
        }
        Some(
            self.weight_percent(trigger_id)
                .map_or(native, |percent| scale_weight(native, percent)),
        )
    }

    /// Feedback entries for `event`, in declaration order.
    pub fn feedback_for(&self, event: FeedbackEvent) -> impl Iterator<Item = &FeedbackEntry> {
        self.feedback
            .iter()
            .filter(move |entry| entry.event == event)
    }
}

/// One `EntryN.*` group: when to show a clip, to whom, and how often.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackEntry {
    pub event: FeedbackEvent,
    /// Video ID resolved through the video catalog (`[Videos]`), e.g. `AWAGR1`.
    /// `None` keeps the entry for its text/audio feedback only.
    pub video: Option<String>,
    pub audience: FeedbackAudience,
    /// Seconds before this entry may fire again; `None` means no limit.
    pub timeout_seconds: Option<u32>,
}

/// The six skirmish moments a personality may comment on. This is the whole set
/// the RA3 schema defines — there is no seventh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeedbackEvent {
    /// Match start, before the first order.
    StartGame,
    /// The house is losing: its base is being overrun.
    InTrouble,
    /// The local player first met this commander in the field.
    FirstSeen,
    /// This house defeated the local player.
    DefeatedLocalPlayer,
    /// The local player defeated this house.
    DefeatedByLocalPlayer,
    /// This house was eliminated.
    Lost,
}

impl FeedbackEvent {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_uppercase().as_str() {
            "STARTGAME" => Some(Self::StartGame),
            "INTROUBLE" => Some(Self::InTrouble),
            "FIRSTSEEN" => Some(Self::FirstSeen),
            "DEFEATEDLOCALPLAYER" => Some(Self::DefeatedLocalPlayer),
            "DEFEATEDBYLOCALPLAYER" => Some(Self::DefeatedByLocalPlayer),
            "LOST" => Some(Self::Lost),
            _ => None,
        }
    }
}

/// Who may see a feedback entry. RA3's `FeedbackTo=` default is
/// `ALLIES ENEMIES NEUTRAL`; retail personalities only ever use the first two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackAudience(u8);

impl FeedbackAudience {
    pub const ALLIES: Self = Self(1);
    pub const ENEMIES: Self = Self(2);
    pub const NEUTRAL: Self = Self(4);
    pub const ALL: Self = Self(7);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl Default for FeedbackAudience {
    fn default() -> Self {
        Self::ALL
    }
}

/// `TriggerWeight.X=180` means "180% of X's own weight". `100` is the identity,
/// so a personality that names no triggers reproduces the native draw exactly.
pub const NEUTRAL_WEIGHT_PERCENT: u16 = 100;

/// `HouseTeamCreation`'s constructor value for `+0x565C` (`0x004F5BDD`): every
/// team-creation pass may pick a trigger.
pub const NATIVE_TRIGGER_RATIO: i32 = 100;

/// `native` scaled by `percent`, truncating toward zero like the rest of the AI
/// weight path. `NEUTRAL_WEIGHT_PERCENT` returns `native` unchanged, which is
/// what keeps an all-default personality byte-identical to no personality.
pub fn scale_weight(native: i32, percent: u16) -> i32 {
    if percent == NEUTRAL_WEIGHT_PERCENT {
        return native;
    }
    let scaled = i64::from(native) * i64::from(percent) / i64::from(NEUTRAL_WEIGHT_PERCENT);
    i32::try_from(scaled).unwrap_or(if scaled < 0 { i32::MIN } else { i32::MAX })
}

/// `delay` frames scaled by a `TeamDelayPercent=`. `100` is the identity; the
/// result never drops below one frame, because a zero-frame restart would let a
/// house create a team every pass.
pub fn scale_team_delay(delay: i32, percent: u16) -> i32 {
    if percent == NEUTRAL_WEIGHT_PERCENT {
        return delay;
    }
    let scaled = i64::from(delay) * i64::from(percent) / i64::from(NEUTRAL_WEIGHT_PERCENT);
    i32::try_from(scaled).unwrap_or(i32::MAX).max(1)
}

/// `[CommanderFeedback]`: the commander feature's own switches.
///
/// Read from the same rules layers as the personalities, so a mod — or a map
/// that ships its own scripted AI and has no business playing talking heads —
/// can switch them off without a UI and without touching the player's options.
/// Defaults keep the layer on and sensitive: one building lost is enough.
///
/// The two switches are independent, and the difference between them is what a
/// house stops doing:
///
/// * `Enabled=no` — no clips, but every computer house keeps its commander and
///   therefore its `TriggerDeny=` / `TriggerWeight.` strategy tilt.
/// * `Personalities=no` — no house draws a commander at all: the AI is
///   retail's own, trigger weights untouched, exactly as if this module did not
///   exist.
///
/// `Assign.<house>=<personality>` names a commander for one launch slot
/// (`Computer1`..`ComputerN`, the same names the startup log prints). It is the
/// way to test a personality before a selector exists, and it loses to the
/// slot's own `commander` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommanderFeedbackRules {
    /// `Enabled=no` switches the whole clip layer off for every house.
    pub enabled: bool,
    /// `Personalities=no` stops the launch from assigning a commander to any
    /// computer house: native skirmish AI, no draw, no clips.
    pub personalities: bool,
    /// Buildings a house must lose from its own high-water mark before it is
    /// in trouble.
    pub trouble_buildings_lost: i32,
    /// Seconds a house stays quiet after an `InTrouble` clip whose entry names
    /// no `EntryN.Timeout=`.
    pub trouble_quiet_seconds: u32,
    /// Uppercase launch-slot name (`Computer1`) -> `[AIPersonalities]` name.
    pub assign: BTreeMap<String, String>,
}

impl CommanderFeedbackRules {
    /// The commander `Assign.` names for `house` (a launch slot's owner name),
    /// in any case.
    pub fn assigned(&self, house: &str) -> Option<&str> {
        self.assign
            .get(&house.to_ascii_uppercase())
            .map(String::as_str)
    }
}

/// The values `[CommanderFeedback]` uses when a key is absent: on, one building
/// lost is enough, three minutes of quiet, nobody assigned by name.
impl Default for CommanderFeedbackRules {
    fn default() -> Self {
        Self {
            enabled: true,
            personalities: true,
            trouble_buildings_lost: 1,
            trouble_quiet_seconds: 180,
            assign: BTreeMap::new(),
        }
    }
}

pub(crate) fn parse_commander_feedback(ini: &IniFile) -> CommanderFeedbackRules {
    let section = ini.section_or_empty("CommanderFeedback");
    let buildings = section.read_int("TroubleBuildingsLost", 1);
    let quiet = section.read_int("TroubleQuiet", 180);
    let mut assign = BTreeMap::new();
    const ASSIGN_PREFIX: &str = "Assign.";
    for key in section.keys() {
        let Some(house) = key
            .get(..ASSIGN_PREFIX.len())
            .filter(|prefix| prefix.eq_ignore_ascii_case(ASSIGN_PREFIX))
            .map(|_prefix| &key[ASSIGN_PREFIX.len()..])
        else {
            continue;
        };
        let Some(personality) = section.read_name(key, 256) else {
            continue;
        };
        if house.is_empty() || personality.is_empty() {
            continue;
        }
        assign.insert(house.to_ascii_uppercase(), personality.to_owned());
    }
    CommanderFeedbackRules {
        enabled: section.read_bool("Enabled", true),
        personalities: section.read_bool("Personalities", true),
        trouble_buildings_lost: buildings.max(1),
        trouble_quiet_seconds: u32::try_from(quiet.max(0)).unwrap_or(u32::MAX),
        assign,
    }
}

/// Parse `[AIPersonalities]` and each section it names.
///
/// `ids` is the registry walk (see `parse_registry` in `ruleset`); the caller
/// owns it so the stored-ID projection stays in one place. Nothing here needs
/// the finished `RuleSet`: eligibility is a string comparison against the
/// house's own `[Countries]` key.
pub(crate) fn parse_personalities(ini: &IniFile, ids: &[String]) -> PersonalityCatalog {
    let mut catalog = PersonalityCatalog::default();
    for id in ids {
        let Some(section) = ini.section(id) else {
            log::warn!("[AIPersonalities] names {id}, but that section is missing");
            continue;
        };
        catalog.push(parse_personality_def(id, section));
    }
    // The reserved no-clip personality, last so its identity cannot shift the
    // IDs a save file already stores.
    if catalog
        .index_of_name(PersonalityCatalog::STANDARD)
        .is_none()
    {
        catalog.push(PersonalityDef {
            name: PersonalityCatalog::STANDARD.to_owned(),
            reserved: true,
            ..PersonalityDef::default()
        });
    }
    if !catalog.is_empty() {
        // How many personalities actually tilt the AI. Zero is the normal
        // state of a rules set that only writes clips, and it is worth one
        // number: the strategy half of this module is invisible otherwise.
        let tilted = catalog
            .definitions
            .iter()
            .filter(|definition| {
                !definition.reserved
                    && (!definition.deny.is_empty()
                        || !definition.trigger_weight_percent.is_empty()
                        || definition.trigger_ratio != NATIVE_TRIGGER_RATIO
                        || definition.team_delay_percent != NEUTRAL_WEIGHT_PERCENT)
            })
            .count();
        let bound = catalog
            .definitions
            .iter()
            .filter(|definition| !definition.reserved && definition.countries.is_some())
            .count();
        log::info!(
            "AIPersonalities: {} loaded ({bound} with Country=, {} any country), \
             {tilted} with strategy entries",
            catalog.len(),
            catalog.len().saturating_sub(bound),
        );
    }
    catalog
}

fn parse_personality_def(id: &str, section: &IniSection) -> PersonalityDef {
    // `Country=` is the `Owner=`-style list: exact `[Countries]` keys, comma or
    // space separated, any case. `*` and `All` mean "no restriction", which is
    // also what an absent key means.
    let countries = section.read_name("Country", 1024).and_then(|list| {
        let mut keys = BTreeSet::new();
        for token in list.split([',', ' ']).filter(|token| !token.is_empty()) {
            if token == "*" || token.eq_ignore_ascii_case("all") {
                return None;
            }
            keys.insert(token.to_ascii_uppercase());
        }
        (!keys.is_empty()).then_some(keys)
    });

    // `Side=` was the first schema's filter and is gone: it restated what a
    // `[Countries]` key already says, and two filters for one question invite
    // data that disagrees with itself. A section that still writes it would
    // otherwise be read as "every country", so say so once.
    if section.read_name("Side", 64).is_some() {
        log::warn!(
            "[{id}] Side= is not supported; it is ignored. Write Country= with the \
             [Countries] keys this personality may be offered to"
        );
    }

    let mut deny = BTreeSet::new();
    if let Some(list) = section.read_name("TriggerDeny", 1024) {
        for token in list.split([',', ' ']).filter(|token| !token.is_empty()) {
            deny.insert(token.to_ascii_uppercase());
        }
    }

    // `TriggerWeight.<AITriggerTypes ID>=<percent>` — the section name is part
    // of the key, so the map is the only place the target trigger appears.
    // The prefix match is case-insensitive as a whole; matching only two
    // spellings silently dropped `TRIGGERWEIGHT.`.
    const WEIGHT_PREFIX: &str = "TriggerWeight.";
    let mut trigger_weight_percent = BTreeMap::new();
    for key in section.keys() {
        let Some(trigger_id) = key
            .get(..WEIGHT_PREFIX.len())
            .filter(|prefix| prefix.eq_ignore_ascii_case(WEIGHT_PREFIX))
            .map(|_prefix| &key[WEIGHT_PREFIX.len()..])
        else {
            continue;
        };
        if trigger_id.is_empty() {
            continue;
        }
        let percent = section.read_int(key, i32::from(NEUTRAL_WEIGHT_PERCENT));
        let percent = u16::try_from(percent).unwrap_or_else(|_| {
            log::warn!("[{id}] TriggerWeight.{trigger_id}={percent} is out of range; ignored");
            NEUTRAL_WEIGHT_PERCENT
        });
        trigger_weight_percent.insert(trigger_id.to_ascii_uppercase(), percent);
    }

    let trigger_ratio = section.read_int("TriggerRatio", NATIVE_TRIGGER_RATIO);
    let trigger_ratio = if (0..=NATIVE_TRIGGER_RATIO).contains(&trigger_ratio) {
        trigger_ratio
    } else {
        log::warn!(
            "[{id}] TriggerRatio={trigger_ratio} is outside 0..=100; using {NATIVE_TRIGGER_RATIO}"
        );
        NATIVE_TRIGGER_RATIO
    };

    let team_delay_percent = section.read_int("TeamDelayPercent", 100);
    let team_delay_percent = if (1..=1000).contains(&team_delay_percent) {
        u16::try_from(team_delay_percent).unwrap_or(NEUTRAL_WEIGHT_PERCENT)
    } else {
        log::warn!("[{id}] TeamDelayPercent={team_delay_percent} is outside 1..=1000; using 100");
        NEUTRAL_WEIGHT_PERCENT
    };

    PersonalityDef {
        name: id.to_owned(),
        countries,
        display_name: section.read_name("DisplayName", 256).map(str::to_owned),
        display_desc: section.read_name("DisplayDesc", 1024).map(str::to_owned),
        trigger_ratio,
        team_delay_percent,
        reserved: false,
        deny,
        trigger_weight_percent,
        feedback: parse_feedback_entries(id, section),
    }
}

/// The commander a computer house gets.
///
/// Assignment is opt-in: nothing is drawn by default, so a launch that asks for
/// no commander keeps retail's own AI. `requested` decides, in the launch's own
/// order of precedence (slot field, then `[CommanderFeedback] Assign.`):
///
/// * `None` — no commander: native behaviour, no clips;
/// * `Some("none")` (or `off` / `silent`) — the same, spelled out, for a map
///   that switches the feature off per house;
/// * `Some(name)` — that personality, [`PersonalityCatalog::STANDARD`] among
///   them. An unknown name warns and leaves the house with no commander rather
///   than silently giving it something it did not ask for.
///
/// A name the house's country is not eligible for still takes effect, with a
/// warning: an explicit choice (a map, a test, a future selector) outranks the
/// `Country=` filter, which only limits what the draw may offer.
pub fn personality_for_house(
    catalog: &PersonalityCatalog,
    country: &str,
    requested: Option<&str>,
) -> Option<u16> {
    let Some(requested) = requested else {
        return None;
    };
    if ["none", "off", "silent"]
        .iter()
        .any(|token| requested.eq_ignore_ascii_case(token))
    {
        return None;
    }
    match catalog.index_of_name(requested) {
        Some(index) => {
            if let Some(definition) = catalog.get(index) {
                if !definition.reserved && !definition.admits(country) {
                    log::warn!(
                        "Commander={requested} does not list {country} in Country=; \
                         assigned anyway because the name was requested explicitly"
                    );
                }
            }
            Some(index)
        }
        None => {
            log::warn!(
                "Commander={requested} names no [AIPersonalities] entry; the house gets none"
            );
            None
        }
    }
}

/// `Entry<N>.Key` / `.Video` / `.To` / `.Timeout` / `.Text`, grouped by `N` and
/// returned in ascending index order. An entry without `Key=` cannot fire and
/// is dropped.
fn parse_feedback_entries(id: &str, section: &IniSection) -> Vec<FeedbackEntry> {
    type Slot = (
        Option<FeedbackEvent>,
        Option<String>,
        Option<String>,
        Option<u32>,
    );
    let mut groups: BTreeMap<usize, Slot> = BTreeMap::new();
    for key in section.keys() {
        let Some(rest) = key
            .strip_prefix("Entry")
            .or_else(|| key.strip_prefix("entry"))
        else {
            continue;
        };
        let Some((index, field)) = rest.split_once('.') else {
            continue;
        };
        let Ok(index) = index.parse::<usize>() else {
            continue;
        };
        let slot = groups.entry(index).or_default();
        match field.to_ascii_lowercase().as_str() {
            "key" => {
                let token = section.read_string(key, "", 64);
                match FeedbackEvent::parse(&token) {
                    Some(event) => slot.0 = Some(event),
                    None if !token.is_empty() => {
                        log::warn!(
                            "[{id}] Entry{index}.Key={token} is not a skirmish feedback event"
                        )
                    }
                    None => {}
                }
            }
            "video" => slot.1 = section.read_name(key, 64).map(str::to_owned),
            "to" => slot.2 = section.read_name(key, 256).map(str::to_owned),
            "timeout" => {
                let seconds = section.read_int(key, -1);
                slot.3 = u32::try_from(seconds).ok();
            }
            _ => {}
        }
    }
    groups
        .into_iter()
        .filter_map(|(_index, (event, video, audience, timeout))| {
            let event = event?;
            Some(FeedbackEntry {
                event,
                video,
                audience: audience.as_deref().map(parse_audience).unwrap_or_default(),
                timeout_seconds: timeout,
            })
        })
        .collect()
}

fn parse_audience(list: &str) -> FeedbackAudience {
    let mut audience = FeedbackAudience(0);
    for token in list.split([',', ' ']).filter(|token| !token.is_empty()) {
        let bit = match token.to_ascii_uppercase().as_str() {
            "ALLIES" => FeedbackAudience::ALLIES,
            "ENEMIES" => FeedbackAudience::ENEMIES,
            "NEUTRAL" => FeedbackAudience::NEUTRAL,
            _ => {
                log::warn!("Entry.To={token} is not ALLIES, ENEMIES or NEUTRAL");
                continue;
            }
        };
        audience = FeedbackAudience(audience.0 | bit.0);
    }
    if audience.0 == 0 {
        // An unreadable audience list must not silently mute the entry.
        FeedbackAudience::default()
    } else {
        audience
    }
}

/// FNV-1a. Deliberately not `DefaultHasher`: assignment has to give the same
/// answer on every platform and every Rust release, because it is part of the
/// scenario's AI setup and therefore of what a replay expects.
fn fnv1a64(value: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
#[path = "personality_tests.rs"]
mod personality_tests;
