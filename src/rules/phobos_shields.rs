//! Phobos Shields, retained by the Rules registry.
//!
//! Native identity: Phobos `ShieldTypeClass`
//! (`src/New/Type/ShieldTypeClass.{h,cpp}`), an `Enumerable<ShieldTypeClass>`
//! whose main section is `ShieldTypes` and whose entries default to an
//! inactive `Strength=0`. `ShieldTypeClass::LoadFromINI` reads each type's own
//! section; `TechnoTypeExt::ShieldType` (`ShieldType=`) binds a techno, and
//! `TechnoExt` constructs a `ShieldClass` only when the bound type's strength is
//! above zero (`Hooks.cpp:297..301`).
//!
//! The catalog is read by the ordered Rules Process owner, exactly as the
//! `[RadiationTypes]` catalog is, so a later layer replaces or retains each
//! field independently. `RuleSet` publishes an immutable projection and the
//! simulation resolves shields through [`crate::rules::ruleset::RuleSet::shield_type`].
//! Damage absorption itself lives in `sim::shield`, at the
//! `TechnoClass::ReceiveDamage @ 0x00701900` entry
//! (installed hook `0x701900+0x6`, `src/Ext/Techno/Hooks.ReceiveDamage.cpp`).
//!
//! Read but not consumed here: `Pips*`, `IdleAnim*`, `HitFlash*`, `AllowTransfer*`
//! and the warhead-side `Shield.AttachTypes`/`RemoveTypes`/`Break`/`HitAnim`
//! keys. Those are separate presentation, transfer and warhead-effect chains.

#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniFile;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::truncate_native_bytes;

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`.
#[cfg(feature = "phobos")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// Phobos `AffectedHouse` flags (`Utilities/Enum.h:127..141`), the target of
/// `Valueable<AffectedHouse>::Read` (`Utilities/TemplateDef.h:824..873`): the
/// value starts at `None` and ORs one flag per comma-separated token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AffectedHouse(u8);

impl AffectedHouse {
    pub const NONE: Self = Self(0x0);
    pub const OWNER: Self = Self(0x1);
    pub const ALLIES: Self = Self(0x2);
    pub const ENEMIES: Self = Self(0x4);
    pub const NEUTRAL: Self = Self(0x8);
    pub const ALL: Self = Self(Self::OWNER.0 | Self::ALLIES.0 | Self::ENEMIES.0 | Self::NEUTRAL.0);

    pub fn bits(self) -> u8 {
        self.0
    }

    /// The `flags & AffectedHouse::X != AffectedHouse::None` test.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `Read<AffectedHouse>`: every token must match a known spelling, or the
    /// whole read fails and the caller retains its current value.
    #[cfg(feature = "phobos")]
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim_matches(|c| matches!(c, ' ' | '\t' | '\r'));
            let flag = match token.to_ascii_lowercase().as_str() {
                "owner" | "self" => Self::OWNER,
                "allies" | "ally" => Self::ALLIES,
                "enemies" | "enemy" => Self::ENEMIES,
                "neutral" => Self::NEUTRAL,
                // Team = Owner | Allies, Others = NotOwner.
                "team" => Self(Self::OWNER.0 | Self::ALLIES.0),
                "others" => Self(Self::ALLIES.0 | Self::ENEMIES.0),
                "all" => Self::ALL,
                "none" => Self::NONE,
                _ => return None,
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }
}

/// `ShieldTypeClass`'s constructor default (`Tint_VisibleToHouses { AffectedHouse::All }`).
impl Default for AffectedHouse {
    fn default() -> Self {
        Self::ALL
    }
}

/// `ShieldTypeClass`'s constructor values. Every field keeps the current value
/// when its key is absent, so one `apply_pass` may replace or retain each value.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ShieldType {
    name: String,
    strength: i32,
    initial_strength: Option<i32>,
    condition_yellow: Option<f64>,
    condition_red: Option<f64>,
    armor: String,
    inherit_armor_from_techno: bool,
    inherit_armor_allowed: Vec<String>,
    inherit_armor_disallowed: Vec<String>,
    apply_armor_mult: Option<bool>,
    powered: bool,
    respawn: f64,
    respawn_rate: i32,
    respawn_restart_in_combat: bool,
    respawn_restart_in_combat_delay: i32,
    respawn_anim: Vec<String>,
    respawn_weapon: Option<String>,
    self_healing: f64,
    self_healing_rate: i32,
    self_healing_restart_in_combat: bool,
    self_healing_restart_in_combat_delay: i32,
    self_healing_enabled_by: Vec<String>,
    absorb_over_damage: bool,
    immune_to_berserk: bool,
    bracket_delta: i32,
    break_anim: Vec<String>,
    hit_anim: Vec<String>,
    hit_flash: bool,
    hit_flash_fixed_size: Option<i32>,
    hit_flash_red: bool,
    hit_flash_green: bool,
    hit_flash_blue: bool,
    hit_flash_black: bool,
    break_weapon: Option<String>,
    absorb_percent: f64,
    pass_percent: f64,
    received_damage_minimum: i32,
    received_damage_maximum: i32,
    pips: [i32; 3],
    pips_building: [i32; 3],
    pips_background: Option<String>,
    pips_building_empty: Option<i32>,
    pips_hide_if_no_strength: bool,
    tint_color: Option<(u8, u8, u8)>,
    tint_intensity: f64,
    tint_visible_to_houses: AffectedHouse,
}

impl ShieldType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            armor: "none".to_owned(),
            // ShieldTypeClass's constructor:
            //   Respawn/SelfHealing 0, rates 0, RestartInCombat true, delays 0,
            //   HitFlash false, channels true, AbsorbPercent 1.0, PassPercent 0,
            //   ReceivedDamage min/max INT32_MIN/INT32_MAX.
            respawn_restart_in_combat: true,
            self_healing_restart_in_combat: true,
            hit_flash_red: true,
            hit_flash_green: true,
            hit_flash_blue: true,
            absorb_percent: 1.0,
            received_damage_minimum: i32::MIN,
            received_damage_maximum: i32::MAX,
            pips: [-1, -1, -1],
            pips_building: [-1, -1, -1],
            ..Self::default()
        }
    }

    /// `InitialStrength.Get(Strength)`: the constructor's nullable default.
    pub(crate) fn initial_strength(&self) -> i32 {
        self.initial_strength.unwrap_or(self.strength)
    }

    pub fn strength(&self) -> i32 {
        self.strength
    }

    pub fn condition_yellow(&self, global: f64) -> f64 {
        self.condition_yellow.unwrap_or(global)
    }

    pub fn condition_red(&self, global: f64) -> f64 {
        self.condition_red.unwrap_or(global)
    }

    pub fn has_tint(&self) -> bool {
        self.tint_color.is_some() || self.tint_intensity != 0.0
    }

    pub(crate) fn armor(&self) -> &str {
        &self.armor
    }

    pub(crate) fn inherit_armor_from_techno(&self) -> bool {
        self.inherit_armor_from_techno
    }

    pub(crate) fn inherit_armor_allowed(&self) -> &[String] {
        &self.inherit_armor_allowed
    }

    pub(crate) fn inherit_armor_disallowed(&self) -> &[String] {
        &self.inherit_armor_disallowed
    }

    pub(crate) fn apply_armor_mult(&self, global: bool) -> bool {
        self.apply_armor_mult.unwrap_or(global)
    }

    pub(crate) fn powered(&self) -> bool {
        self.powered
    }

    pub(crate) fn respawn(&self) -> f64 {
        self.respawn
    }

    pub(crate) fn respawn_rate(&self) -> i32 {
        self.respawn_rate
    }

    pub(crate) fn respawn_restart_in_combat(&self) -> bool {
        self.respawn_restart_in_combat
    }

    pub(crate) fn respawn_restart_in_combat_delay(&self) -> i32 {
        self.respawn_restart_in_combat_delay
    }

    pub(crate) fn respawn_weapon(&self) -> Option<&str> {
        self.respawn_weapon.as_deref()
    }

    pub(crate) fn self_healing(&self) -> f64 {
        self.self_healing
    }

    pub(crate) fn self_healing_rate(&self) -> i32 {
        self.self_healing_rate
    }

    pub(crate) fn self_healing_restart_in_combat(&self) -> bool {
        self.self_healing_restart_in_combat
    }

    pub(crate) fn self_healing_restart_in_combat_delay(&self) -> i32 {
        self.self_healing_restart_in_combat_delay
    }

    pub(crate) fn absorb_over_damage(&self) -> bool {
        self.absorb_over_damage
    }

    pub(crate) fn immune_to_berserk(&self) -> bool {
        self.immune_to_berserk
    }

    pub(crate) fn absorb_percent(&self) -> f64 {
        self.absorb_percent
    }

    pub(crate) fn pass_percent(&self) -> f64 {
        self.pass_percent
    }

    pub(crate) fn received_damage_minimum(&self) -> i32 {
        self.received_damage_minimum
    }

    pub(crate) fn received_damage_maximum(&self) -> i32 {
        self.received_damage_maximum
    }

    /// `Pips`/`Pips.Building`: the green/yellow/red PIPS.SHP frames, each
    /// `-1` when unset. The global `[AudioVisual] Pips.Shield*` list covers an
    /// entry whose own X is `-1`.
    pub fn pips(&self, building: bool) -> [i32; 3] {
        if building {
            self.pips_building
        } else {
            self.pips
        }
    }

    pub fn pips_background(&self) -> Option<&str> {
        self.pips_background.as_deref()
    }

    pub fn pips_building_empty(&self) -> Option<i32> {
        self.pips_building_empty
    }

    pub fn pips_hide_if_no_strength(&self) -> bool {
        self.pips_hide_if_no_strength
    }

    pub fn bracket_delta(&self) -> i32 {
        self.bracket_delta
    }

    pub fn tint_color(&self) -> Option<(u8, u8, u8)> {
        self.tint_color
    }

    pub fn tint_intensity(&self) -> f64 {
        self.tint_intensity
    }

    pub fn tint_visible_to_houses(&self) -> AffectedHouse {
        self.tint_visible_to_houses
    }
}

/// Float fields hash by bits so equal entries hash equal.
impl std::hash::Hash for ShieldType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.strength.hash(state);
        self.initial_strength.hash(state);
        self.condition_yellow.map(f64::to_bits).hash(state);
        self.condition_red.map(f64::to_bits).hash(state);
        self.armor.hash(state);
        self.inherit_armor_from_techno.hash(state);
        self.inherit_armor_allowed.hash(state);
        self.inherit_armor_disallowed.hash(state);
        self.apply_armor_mult.hash(state);
        self.powered.hash(state);
        self.respawn.to_bits().hash(state);
        self.respawn_rate.hash(state);
        self.respawn_restart_in_combat.hash(state);
        self.respawn_restart_in_combat_delay.hash(state);
        self.respawn_anim.hash(state);
        self.respawn_weapon.hash(state);
        self.self_healing.to_bits().hash(state);
        self.self_healing_rate.hash(state);
        self.self_healing_restart_in_combat.hash(state);
        self.self_healing_restart_in_combat_delay.hash(state);
        self.self_healing_enabled_by.hash(state);
        self.absorb_over_damage.hash(state);
        self.immune_to_berserk.hash(state);
        self.bracket_delta.hash(state);
        self.break_anim.hash(state);
        self.hit_anim.hash(state);
        self.hit_flash.hash(state);
        self.hit_flash_fixed_size.hash(state);
        self.hit_flash_red.hash(state);
        self.hit_flash_green.hash(state);
        self.hit_flash_blue.hash(state);
        self.hit_flash_black.hash(state);
        self.break_weapon.hash(state);
        self.absorb_percent.to_bits().hash(state);
        self.pass_percent.to_bits().hash(state);
        self.received_damage_minimum.hash(state);
        self.received_damage_maximum.hash(state);
        self.pips.hash(state);
        self.pips_building.hash(state);
        self.pips_background.hash(state);
        self.pips_building_empty.hash(state);
        self.pips_hide_if_no_strength.hash(state);
        self.tint_color.hash(state);
        self.tint_intensity.to_bits().hash(state);
        self.tint_visible_to_houses.hash(state);
    }
}

/// Ordered Phobos `Enumerable<ShieldTypeClass>::Array`. Unlike the radiation
/// catalog it has no seeded default: `ShieldTypeClass` defines no
/// `AddDefaults`, and a techno without `ShieldType=` has no shield at all.
#[derive(Debug, Clone, PartialEq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct ShieldTypeCatalog {
    types: Vec<ShieldType>,
}

impl ShieldTypeCatalog {
    #[cfg(feature = "phobos")]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|entry| entry.name.as_str())
    }

    #[cfg(feature = "phobos")]
    pub fn entry(&self, name: &str) -> Option<&ShieldType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[ShieldTypes]` list appends
    /// entries, then every retained entry re-reads its own section.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("ShieldTypes") {
            // Phobos Enumerable::LoadFromINIList reads each physical key
            // through ReadString with Phobos::readBuffer[2048] before lookup.
            // Source: Phobos b90ab4bc, src/Utilities/Enumerable.h:49-60,
            // src/Phobos.h:23-24; CCINIClass::ReadString @0x00528A10.
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types.push(ShieldType::named(&truncate_native_bytes(
                        name,
                        NAME_PAYLOAD_BYTES,
                    )));
                }
            }
        }
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            let Some(section) = pass.section(&name) else {
                continue;
            };
            let entry = &mut self.types[index];
            entry.strength = section.read_int("Strength", entry.strength);
            entry.initial_strength =
                section.read_phobos_nullable_int("InitialStrength", entry.initial_strength);
            entry.condition_yellow =
                section.read_phobos_nullable_double("ConditionYellow", entry.condition_yellow);
            entry.condition_red =
                section.read_phobos_nullable_double("ConditionRed", entry.condition_red);
            entry.armor = section.read_string("Armor", &entry.armor, 0x80);
            entry.inherit_armor_from_techno =
                section.read_bool("InheritArmorFromTechno", entry.inherit_armor_from_techno);
            if let Some(tokens) = section.read_list("InheritArmor.Allowed", 0x80) {
                entry.inherit_armor_allowed = tokens.into_iter().map(str::to_owned).collect();
            }
            if let Some(tokens) = section.read_list("InheritArmor.Disallowed", 0x80) {
                entry.inherit_armor_disallowed = tokens.into_iter().map(str::to_owned).collect();
            }
            entry.apply_armor_mult =
                section.read_phobos_nullable_bool("ApplyArmorMult", entry.apply_armor_mult);
            entry.powered = section.read_bool("Powered", entry.powered);
            entry.respawn = section.read_double("Respawn", entry.respawn);
            // ShieldTypeClass::LoadFromINI uses a fresh Nullable<double>
            // scratch per pass; unset/reset/invalid rates retain stored frames.
            if let Some(minutes) = section.read_phobos_nullable_double("Respawn.Rate", None) {
                entry.respawn_rate = (minutes * 900.0) as i32;
            }
            entry.respawn_restart_in_combat =
                section.read_bool("Respawn.RestartInCombat", entry.respawn_restart_in_combat);
            entry.respawn_restart_in_combat_delay = section.read_int(
                "Respawn.RestartInCombatDelay",
                entry.respawn_restart_in_combat_delay,
            );
            if let Some(tokens) = section.read_list("Respawn.Anim", 0x80) {
                entry.respawn_anim = tokens.into_iter().map(str::to_owned).collect();
            }
            if section.is_present("Respawn.Weapon") {
                entry.respawn_weapon = section
                    .read_name("Respawn.Weapon", 0x80)
                    .filter(|name| !is_none_name(name))
                    .map(str::to_owned);
            }
            entry.self_healing = section.read_double("SelfHealing", entry.self_healing);
            if let Some(minutes) = section.read_phobos_nullable_double("SelfHealing.Rate", None) {
                entry.self_healing_rate = (minutes * 900.0) as i32;
            }
            entry.self_healing_restart_in_combat = section.read_bool(
                "SelfHealing.RestartInCombat",
                entry.self_healing_restart_in_combat,
            );
            entry.self_healing_restart_in_combat_delay = section.read_int(
                "SelfHealing.RestartInCombatDelay",
                entry.self_healing_restart_in_combat_delay,
            );
            if let Some(tokens) = section.read_list("SelfHealing.EnabledBy", 0x80) {
                entry.self_healing_enabled_by = tokens.into_iter().map(str::to_owned).collect();
            }
            entry.absorb_over_damage =
                section.read_bool("AbsorbOverDamage", entry.absorb_over_damage);
            entry.immune_to_berserk = section.read_bool("ImmuneToBerserk", entry.immune_to_berserk);
            entry.bracket_delta = section.read_int("BracketDelta", entry.bracket_delta);
            entry.break_anim = section
                .read_list("BreakAnim", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_else(|| entry.break_anim.clone());
            entry.hit_anim = section
                .read_list("HitAnim", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_else(|| entry.hit_anim.clone());
            entry.hit_flash = section.read_bool("HitFlash", entry.hit_flash);
            entry.hit_flash_fixed_size =
                section.read_phobos_nullable_int("HitFlash.FixedSize", entry.hit_flash_fixed_size);
            entry.hit_flash_red = section.read_bool("HitFlash.Red", entry.hit_flash_red);
            entry.hit_flash_green = section.read_bool("HitFlash.Green", entry.hit_flash_green);
            entry.hit_flash_blue = section.read_bool("HitFlash.Blue", entry.hit_flash_blue);
            entry.hit_flash_black = section.read_bool("HitFlash.Black", entry.hit_flash_black);
            if section.is_present("BreakWeapon") {
                entry.break_weapon = section
                    .read_name("BreakWeapon", 0x80)
                    .filter(|name| !is_none_name(name))
                    .map(str::to_owned);
            }
            entry.absorb_percent = section.read_double("AbsorbPercent", entry.absorb_percent);
            entry.pass_percent = section.read_double("PassPercent", entry.pass_percent);
            entry.received_damage_minimum =
                section.read_int("ReceivedDamage.Minimum", entry.received_damage_minimum);
            entry.received_damage_maximum =
                section.read_int("ReceivedDamage.Maximum", entry.received_damage_maximum);
            entry.pips = read_pip_triplet(section, "Pips", entry.pips);
            entry.pips_building = read_pip_triplet(section, "Pips.Building", entry.pips_building);
            entry.pips_background = section.read_phobos_nullable_with(
                "Pips.Background",
                entry.pips_background.take(),
                |name| Some(name.to_owned()),
            );
            entry.pips_building_empty =
                section.read_phobos_nullable_int("Pips.Building.Empty", entry.pips_building_empty);
            entry.pips_hide_if_no_strength =
                section.read_bool("Pips.HideIfNoStrength", entry.pips_hide_if_no_strength);
            if section.is_present("Tint.Color") {
                let [red, green, blue] = section.read_color_rgb("Tint.Color", [0, 0, 0], None);
                entry.tint_color = Some((red, green, blue));
            }
            entry.tint_intensity = section.read_double("Tint.Intensity", entry.tint_intensity);
            if section.is_present("Tint.VisibleToHouses") {
                let raw = section.read_string("Tint.VisibleToHouses", "", 0x100);
                if let Some(houses) = AffectedHouse::parse(&raw) {
                    entry.tint_visible_to_houses = houses;
                }
            }
        }
    }
}

/// The warhead-side shield overrides (`WarheadTypeExt` shield fields,
/// `src/Ext/WarheadType/Body.cpp:246..260`). Read on the merged warhead section;
/// `None` floats/ints retain the shield type's value at use, exactly as
/// `Nullable.Get(typeValue)` resolves them.
#[derive(Debug, Clone, PartialEq)]
pub struct ShieldWarheadRules {
    pub(crate) penetrate: bool,
    /// `Shield.AffectTypes`: empty means every shield type.
    pub(crate) affect_types: Vec<String>,
    /// `Shield.Penetrate.Types`: when non-empty, a shield type outside it is
    /// never penetrated.
    pub(crate) penetrate_types: Vec<String>,
    pub(crate) absorb_percent: Option<f64>,
    pub(crate) pass_percent: Option<f64>,
    pub(crate) received_minimum: Option<i32>,
    pub(crate) received_maximum: Option<i32>,
    pub(crate) received_min_multiplier: f64,
    pub(crate) received_max_multiplier: f64,
}

impl Default for ShieldWarheadRules {
    fn default() -> Self {
        Self {
            penetrate: false,
            affect_types: Vec::new(),
            penetrate_types: Vec::new(),
            absorb_percent: None,
            pass_percent: None,
            received_minimum: None,
            received_maximum: None,
            received_min_multiplier: 1.0,
            received_max_multiplier: 1.0,
        }
    }
}

impl ShieldWarheadRules {
    /// `WarheadTypeClass::ReadINI`'s shield block, in native order.
    pub(crate) fn from_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        let mut rules = Self::default();
        rules.penetrate = section.read_bool("Shield.Penetrate", rules.penetrate);
        rules.affect_types = section
            .read_list("Shield.AffectTypes", 0x80)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        rules.penetrate_types = section
            .read_list("Shield.Penetrate.Types", 0x80)
            .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        rules.absorb_percent =
            section.read_phobos_nullable_double("Shield.AbsorbPercent", rules.absorb_percent);
        rules.pass_percent =
            section.read_phobos_nullable_double("Shield.PassPercent", rules.pass_percent);
        rules.received_minimum = section
            .read_phobos_nullable_int("Shield.ReceivedDamage.Minimum", rules.received_minimum);
        rules.received_maximum = section
            .read_phobos_nullable_int("Shield.ReceivedDamage.Maximum", rules.received_maximum);
        rules.received_min_multiplier = section.read_double(
            "Shield.ReceivedDamage.MinMultiplier",
            rules.received_min_multiplier,
        );
        rules.received_max_multiplier = section.read_double(
            "Shield.ReceivedDamage.MaxMultiplier",
            rules.received_max_multiplier,
        );
        rules
    }

    /// `Shield.AffectTypes`: whether this warhead reaches the given shield
    /// type. An empty list affects every type.
    pub(crate) fn affects(&self, shield_type: &str) -> bool {
        self.affect_types.is_empty()
            || self
                .affect_types
                .iter()
                .any(|name| name.eq_ignore_ascii_case(shield_type))
    }

    /// `CanBePenetrated`'s shield-type gate: `Shield.Penetrate.Types` falls
    /// back to `Shield.AffectTypes` and restricts only when non-empty.
    pub(crate) fn penetration_allowed(&self, shield_type: &str) -> bool {
        let list = if self.penetrate_types.is_empty() {
            &self.affect_types
        } else {
            &self.penetrate_types
        };
        list.is_empty()
            || list
                .iter()
                .any(|name| name.eq_ignore_ascii_case(shield_type))
    }
}

fn is_none_name(name: &str) -> bool {
    name.is_empty() || name.eq_ignore_ascii_case("none") || name.eq_ignore_ascii_case("<none>")
}

/// `Vector3D<int>` read: the first three tokens replace the defaults; an
/// absent key or short list keeps them.
fn read_pip_triplet(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
    default: [i32; 3],
) -> [i32; 3] {
    let Some(values) = section.read_int_list(key) else {
        return default;
    };
    let mut out = default;
    for (slot, value) in out.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    out
}

/// `[CombatDamage] ShieldApplyArmorMult` (default false) and
/// `[AudioVisual] Shield.ConditionYellow`/`ConditionRed`, which fall back to
/// the ordinary `[AudioVisual] ConditionYellow`/`ConditionRed`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShieldGlobals {
    pub(crate) apply_armor_mult: bool,
    pub(crate) condition_yellow: f64,
    pub(crate) condition_red: f64,
    pub(crate) pips: [i32; 3],
    pub(crate) pips_building: [i32; 3],
    pub(crate) pips_background: Option<String>,
    pub(crate) pips_building_empty: Option<i32>,
}

impl Default for ShieldGlobals {
    fn default() -> Self {
        Self {
            apply_armor_mult: false,
            condition_yellow: 0.5,
            condition_red: 0.25,
            pips: [-1, -1, -1],
            pips_building: [-1, -1, -1],
            pips_background: None,
            pips_building_empty: None,
        }
    }
}

impl ShieldGlobals {
    pub(crate) fn from_ini(ini: &crate::rules::ini_parser::IniFile) -> Self {
        let defaults = Self::default();
        let combat = ini.section_or_empty("CombatDamage");
        let audio = ini.section_or_empty("AudioVisual");
        let condition_yellow = audio.read_double("ConditionYellow", defaults.condition_yellow);
        let condition_red = audio.read_double("ConditionRed", defaults.condition_red);
        Self {
            apply_armor_mult: combat.read_bool("ShieldApplyArmorMult", defaults.apply_armor_mult),
            condition_yellow: audio
                .read_phobos_nullable_double("Shield.ConditionYellow", None)
                .unwrap_or(condition_yellow),
            condition_red: audio
                .read_phobos_nullable_double("Shield.ConditionRed", None)
                .unwrap_or(condition_red),
            pips: read_pip_triplet(&audio, "Pips.Shield", defaults.pips),
            pips_building: read_pip_triplet(&audio, "Pips.Shield.Building", defaults.pips_building),
            pips_background: audio.read_phobos_nullable_with(
                "Pips.Shield.Background",
                defaults.pips_background,
                |name| Some(name.to_owned()),
            ),
            pips_building_empty: audio.read_phobos_nullable_int(
                "Pips.Shield.Building.Empty",
                defaults.pips_building_empty,
            ),
        }
    }

    pub fn apply_armor_mult(&self) -> bool {
        self.apply_armor_mult
    }

    pub fn condition_yellow(&self) -> f64 {
        self.condition_yellow
    }

    pub fn condition_red(&self) -> f64 {
        self.condition_red
    }

    /// `[AudioVisual] Pips.Shield`: green/yellow/red PIPS.SHP frames.
    pub fn pips(&self, building: bool) -> [i32; 3] {
        if building {
            self.pips_building
        } else {
            self.pips
        }
    }

    pub fn pips_background(&self) -> Option<&str> {
        self.pips_background.as_deref()
    }

    pub fn pips_building_empty(&self) -> Option<i32> {
        self.pips_building_empty
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "phobos_shields_tests.rs"]
mod tests;
