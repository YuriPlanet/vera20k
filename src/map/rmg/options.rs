//! `[RandomMap]` seed/options model, the normalizer, and `.SED` read/write.
//!
//! Field order mirrors the original record so the clamp table and the `.SED`
//! key order stay auditable side by side.
//!
//! `IniFile` lookups here are section-scoped: `ini.section(name)?.read_int(key, current)`.
//! There is no `IniFile::get(section, key)` and no `IniFile::parse`; construct
//! with `IniFile::from_bytes` or `IniFile::from_str`.

use super::description::{SeedDescription, read_description};
use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::set_ini_values;

/// Section a random-map seed file stores its options under.
const SECTION: &str = "RandomMap";
/// VERA's seed key for [`RmgOptions::urban_areas`]; native seeds lack it.
/// Written and read only when Ares.dll is loaded (the game is read the Ares
/// way).
const URBAN_AREAS_KEY: &str = "UrbanAreas";

/// One random-map configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmgOptions {
    pub theater: i32,
    pub map_type: i32,
    pub resources: i32,
    pub ruggedness: i32,
    pub time: i32,
    pub water_amount: i32,
    pub num_players: i32,
    pub tiberium: i32,
    pub tiberium_layout: i32,
    pub vegetation: i32,
    pub urban_presence: i32,
    /// VERA extension: the player set Urban Presence (the Advanced Buildings
    /// slider), so the urban stage places towns for it. The original never
    /// reads the presence; without this flag neither does VERA, so every
    /// original seed generates exactly as before. Saved as `VeraUrban=yes`
    /// only when set.
    pub urban_enabled: bool,
    /// VERA extension: Fair Starts. The generator tries a few seeds from this
    /// one, keeps the map whose starts are best spread and best matched in
    /// ore and building room, and evens the ore near each start to the
    /// median start's (`fairness`). Saved as `VeraFair=yes` only when set, so
    /// every original seed generates exactly as before.
    pub fair_starts: bool,
    /// VERA extension: the Extras view's choices (`extras`), all off by
    /// default. Each is saved under its own key only when on.
    pub extras: super::extras::RmgExtras,
    pub width: i32,
    pub height: i32,
    pub accessibility: i32,
    pub region_size: i32,
    pub seed: i32,
    /// Display description. Stored in `.SED` as comma-separated hex UTF-16 code
    /// units, and used as the random-map row's displayed name.
    pub description: SeedDescription,
    /// Ares' `RMG::UrbanAreas` (Ares.dll DAT_100c2150): the Urban Areas box
    /// of the dialog, read by MapSeedClass_Generate_UrbanAreas (1006ABD0,
    /// hook 0x598FB8). Native Ares keeps it process-global and writes no copy;
    /// VERA stores it in the seed as `UrbanAreas=` so the launch regeneration
    /// and every network peer generate the same map. The record digest
    /// ([`Self::native_digest`]) leaves it out. Only code that runs when
    /// Ares.dll is loaded (the game is read the Ares way) sets or reads it.
    pub urban_areas: bool,
    /// VERA extension: neutral tech-building amount, 0 = the original's own
    /// random count. Above 0 it is a 0..100 slider that steers how many neutral
    /// tech buildings the generator seats.
    pub tech_building_amount: i32,
    /// VERA extension: derelict combat buildings, 0 = none. A 0..100 slider
    /// that scatters neutral, unpowered military buildings (`[Extras]
    /// Derelicts=`, else the Rules' `[AI]` construction yard, barracks, battle
    /// lab and war factory lists) over open ground.
    pub derelict_amount: i32,
    /// VERA extension: damage level, 0 = leave building health as generated.
    /// A 0..100 slider that randomly damages every neutral building on the map
    /// for a war-torn look; stronger than the War Damage Extra wins.
    pub damage_amount: i32,
    /// VERA extension: extra civilians, 0 = only the original urban foots. A
    /// 0..100 slider that scatters civilian infantry and vehicles (the same
    /// kinds a town already places) over open ground, all on Neutral.
    pub civilian_amount: i32,
    /// VERA extension: animals, 0 = none. A 0..100 slider that scatters the
    /// rules' `Name=Animal …` infantry and vehicles on Neutral ground.
    pub animal_amount: i32,
    /// VERA extension: neutral defenders, 0 = none. A 0..100 slider that
    /// scatters player-buildable infantry and vehicles (E1, GI, tanks, …) on
    /// the Neutral house, so they defend themselves without a player house.
    pub wild_amount: i32,
}

impl RmgOptions {
    /// Whether the urban pass (`0x005A5020`) runs: Ares' `RMG::UrbanAreas`
    /// when Ares.dll is loaded (`ares`: the game is read the Ares way), or
    /// VERA's opted-in Urban Presence.
    pub fn urban_pass_runs(&self, ares: bool) -> bool {
        if ares && self.urban_areas {
            return true;
        }
        self.urban_enabled
    }

    /// VERA extension: whether a generation reads VERA's catalogue of
    /// civilian buildings (`RmgSettings::vera_urban`), which the app then
    /// resolves: the opted-in Buildings slider, or an Extra that places
    /// buildings from it.
    pub fn wants_vera_catalog(&self) -> bool {
        self.urban_enabled
            || self.derelict_amount > 0
            || self.civilian_amount > 0
            || self.animal_amount > 0
            || self.wild_amount > 0
            || super::extras::Extra::ALL
                .iter()
                .any(|&extra| extra.needs_catalog() && self.extras.get(extra) != 0)
    }
}

/// VERA's `.SED` key for [`RmgOptions::urban_enabled`].
const VERA_URBAN_KEY: &str = "VeraUrban";
/// VERA's `.SED` key for [`RmgOptions::fair_starts`].
const VERA_FAIR_KEY: &str = "VeraFair";
/// VERA's `.SED` key for [`RmgOptions::tech_building_amount`].
const VERA_TECH_AMOUNT_KEY: &str = "VeraTechBuildings";
/// VERA's `.SED` key for [`RmgOptions::derelict_amount`].
const VERA_DERELICT_KEY: &str = "VeraDerelicts";
/// VERA's `.SED` key for [`RmgOptions::damage_amount`].
const VERA_DAMAGE_KEY: &str = "VeraDamage";
/// VERA's `.SED` key for [`RmgOptions::civilian_amount`].
const VERA_CIVILIAN_KEY: &str = "VeraCivilians";
/// VERA's `.SED` key for [`RmgOptions::animal_amount`].
const VERA_ANIMAL_KEY: &str = "VeraAnimals";
/// VERA's `.SED` key for [`RmgOptions::wild_amount`].
const VERA_WILD_KEY: &str = "VeraWild";

impl Default for RmgOptions {
    /// The original constructor's defaults (`MapSeedClass` ctor `0x00595680`,
    /// fields +0x38..+0x74). Note `seed = -1`, which the
    /// normalizer later clamps to 0 unless a `.SED` supplies a real seed.
    fn default() -> Self {
        Self {
            theater: 0,
            map_type: 1,
            resources: 1,
            ruggedness: 0,
            time: 1,
            water_amount: 0,
            num_players: 2,
            tiberium: 0,
            tiberium_layout: 0,
            vegetation: 0,
            urban_presence: 0,
            urban_enabled: false,
            fair_starts: false,
            extras: super::extras::RmgExtras::default(),
            width: 0,
            height: 0,
            accessibility: 0,
            region_size: 0,
            seed: -1,
            description: SeedDescription::default(),
            urban_areas: false,
            tech_building_amount: 0,
            derelict_amount: 0,
            damage_amount: 0,
            civilian_amount: 0,
            animal_amount: 0,
            wild_amount: 0,
        }
    }
}

/// Encode a description the way the original writes it: each UTF-16 code unit
/// as lowercase hex followed by a comma, including a trailing one.
fn encode_description(text: &SeedDescription) -> String {
    crate::rules::ini_value::encode_comma_hex_utf16(text.units())
}

impl RmgOptions {
    /// Clamp every field to its accepted range (`MapSeedClass::ClampFields
    /// @ 0x005975E0`).
    ///
    /// Deliberately does **not** touch `theater`: the original normalizer has
    /// no code path that writes that field, so an out-of-range theater survives
    /// and is resolved later by the theater-name lookup.
    pub fn normalize(&mut self) {
        self.resources = self.resources.clamp(0, 3);
        self.map_type = self.map_type.clamp(0, 4);
        self.time = self.time.clamp(0, 3);
        self.ruggedness = self.ruggedness.clamp(0, 100);
        self.water_amount = self.water_amount.clamp(0, 100);
        // VERA extension: the original clamps to 2..=8.
        self.num_players = self.num_players.clamp(2, super::MAX_PLAYERS);
        self.tiberium = self.tiberium.clamp(1, 100);
        self.tiberium_layout = self.tiberium_layout.clamp(0, 100);
        self.vegetation = self.vegetation.clamp(0, 100);
        self.urban_presence = self.urban_presence.clamp(0, 100);
        // VERA extension: options past 3 ("Very Large") are larger maps.
        self.width = self.width.clamp(0, super::MAX_SIZE_OPTION);
        self.height = self.height.clamp(0, super::MAX_SIZE_OPTION);
        self.accessibility = self.accessibility.clamp(0, 100);
        self.region_size = self.region_size.clamp(0, 100);
        self.seed = self.seed.clamp(0, 0xFFFF);
        self.tech_building_amount = self.tech_building_amount.clamp(0, 100);
        self.derelict_amount = self.derelict_amount.clamp(0, 100);
        self.damage_amount = self.damage_amount.clamp(0, 100);
        self.civilian_amount = self.civilian_amount.clamp(0, 100);
        self.animal_amount = self.animal_amount.clamp(0, 100);
        self.wild_amount = self.wild_amount.clamp(0, 100);
    }

    /// Seed in the form the generator's RNG consumes. Normalizing first
    /// guarantees the value fits.
    pub fn seed_u16(&self) -> u16 {
        self.seed.clamp(0, 0xFFFF) as u16
    }

    /// Apply a `.SED`'s `[RandomMap]` keys over `self`.
    ///
    /// A missing key leaves the existing field alone because the original
    /// passes that field as the reader default. A present malformed value uses
    /// native `atoi` semantics and therefore becomes zero. Does not normalize.
    /// `ares`: Ares.dll is loaded (the game is read the Ares way), so the
    /// Urban Areas box is read too.
    pub fn apply_sed(&mut self, ini: &IniFile, ares: bool) {
        let Some(section) = ini.section(SECTION) else {
            return;
        };
        // MapSeed Load `0x00597A30`: ReadCommaHexUTF16, then ReadInt over each
        // field.
        self.description = read_description(section, &self.description);
        let read = |key: &str, field: &mut i32| *field = section.read_int(key, *field);
        read("Width", &mut self.width);
        read("Height", &mut self.height);
        read("NumPlayers", &mut self.num_players);
        read("Seed", &mut self.seed);
        read("MapType", &mut self.map_type);
        read("Theater", &mut self.theater);
        read("Time", &mut self.time);
        read("RegionSize", &mut self.region_size);
        read("Ruggedness", &mut self.ruggedness);
        read("Accessibility", &mut self.accessibility);
        read("WaterAmount", &mut self.water_amount);
        read("Tiberium", &mut self.tiberium);
        read("TiberiumLayout", &mut self.tiberium_layout);
        read("Vegetation", &mut self.vegetation);
        read("UrbanPresence", &mut self.urban_presence);
        read("Resources", &mut self.resources);
        // VERA: written only when set, so a seed without the key has no towns,
        // whatever `self` held.
        self.urban_enabled = section.read_bool(VERA_URBAN_KEY, false);
        self.fair_starts = section.read_bool(VERA_FAIR_KEY, false);
        // VERA: the Advanced sliders; absent means the original's behaviour.
        read(VERA_TECH_AMOUNT_KEY, &mut self.tech_building_amount);
        read(VERA_DERELICT_KEY, &mut self.derelict_amount);
        read(VERA_DAMAGE_KEY, &mut self.damage_amount);
        read(VERA_CIVILIAN_KEY, &mut self.civilian_amount);
        read(VERA_ANIMAL_KEY, &mut self.animal_amount);
        read(VERA_WILD_KEY, &mut self.wild_amount);
        for extra in super::extras::Extra::ALL {
            let value = section.read_int(extra.key(), 0).clamp(0, 255);
            self.extras.set(extra, value as u8);
        }
        if ares {
            self.urban_areas = section.read_bool(URBAN_AREAS_KEY, self.urban_areas);
        }
    }

    /// Serialize to `.SED` bytes, writing every key in the original's order.
    /// `ares`: Ares.dll is loaded (the game is read the Ares way), so the
    /// Urban Areas box is written too.
    pub fn to_sed_bytes(&self, ares: bool) -> Vec<u8> {
        let values: Vec<(&str, String)> = vec![
            // The original emits Description first, ahead of the 16 integers.
            ("Description", encode_description(&self.description)),
            ("Width", self.width.to_string()),
            ("Height", self.height.to_string()),
            ("NumPlayers", self.num_players.to_string()),
            ("Seed", self.seed.to_string()),
            ("MapType", self.map_type.to_string()),
            ("Theater", self.theater.to_string()),
            ("Time", self.time.to_string()),
            ("RegionSize", self.region_size.to_string()),
            ("Ruggedness", self.ruggedness.to_string()),
            ("Accessibility", self.accessibility.to_string()),
            ("WaterAmount", self.water_amount.to_string()),
            ("Tiberium", self.tiberium.to_string()),
            ("TiberiumLayout", self.tiberium_layout.to_string()),
            ("Vegetation", self.vegetation.to_string()),
            ("UrbanPresence", self.urban_presence.to_string()),
            ("Resources", self.resources.to_string()),
        ];
        // VERA: written only when set, so an original seed's bytes are unchanged.
        let mut values = values;
        if self.urban_enabled {
            values.push((VERA_URBAN_KEY, "yes".to_string()));
        }
        if self.fair_starts {
            values.push((VERA_FAIR_KEY, "yes".to_string()));
        }
        // VERA: the Advanced sliders are written only when set, so an original
        // seed's bytes stay unchanged.
        if self.tech_building_amount != 0 {
            values.push((VERA_TECH_AMOUNT_KEY, self.tech_building_amount.to_string()));
        }
        if self.derelict_amount != 0 {
            values.push((VERA_DERELICT_KEY, self.derelict_amount.to_string()));
        }
        if self.damage_amount != 0 {
            values.push((VERA_DAMAGE_KEY, self.damage_amount.to_string()));
        }
        if self.civilian_amount != 0 {
            values.push((VERA_CIVILIAN_KEY, self.civilian_amount.to_string()));
        }
        if self.animal_amount != 0 {
            values.push((VERA_ANIMAL_KEY, self.animal_amount.to_string()));
        }
        if self.wild_amount != 0 {
            values.push((VERA_WILD_KEY, self.wild_amount.to_string()));
        }
        for extra in super::extras::Extra::ALL {
            let value = self.extras.get(extra);
            if value != 0 {
                values.push((extra.key(), value.to_string()));
            }
        }
        if ares {
            values.push((
                URBAN_AREAS_KEY,
                if self.urban_areas { "yes" } else { "no" }.to_string(),
            ));
        }
        let pairs: Vec<(&str, &str)> = values
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        set_ini_values(b"", SECTION, &pairs)
    }

    /// The random-map record's digest, `0x005E84D0`: the 0x140-byte option
    /// block `0xABE010..0xABE14F` (the 16 integers in field order at
    /// MapSeed `+0x38..+0x74`, `0x00597760`, then the 0x100-byte description
    /// at `+0x78`, zeroed for the fold) folded as `crc = ROL1(crc) + dword`,
    /// printed `%08X` (`0x00817828`).
    pub fn native_digest(&self) -> String {
        let fields = [
            self.theater,
            self.map_type,
            self.resources,
            self.ruggedness,
            self.time,
            self.water_amount,
            self.num_players,
            self.tiberium,
            self.tiberium_layout,
            self.vegetation,
            self.urban_presence,
            self.width,
            self.height,
            self.accessibility,
            self.region_size,
            self.seed,
        ];
        const DESCRIPTION_DWORDS: usize = 0x100 / 4;
        let crc = fields
            .into_iter()
            .map(|field| field as u32)
            .chain(std::iter::repeat_n(0, DESCRIPTION_DWORDS))
            .fold(0u32, |crc, dword| crc.rotate_left(1).wrapping_add(dword));
        format!("{crc:08X}")
    }

    /// VERA extension: the options as a short text a player can send to
    /// another, who reads it back with [`Self::from_share_code`] and
    /// generates the same map. It carries this build's version, every
    /// option, VERA's switches and the Extras, not the description: `VR1-`
    /// then the bytes below and a check byte, in Crockford base 32, five
    /// characters to a group. `ares`: Ares.dll is loaded (the game is read the
    /// Ares way), so the Urban Areas box is carried too.
    pub fn share_code(&self, ares: bool) -> String {
        self.share_code_made_by(ShareVersion::this_build(), ares)
    }

    /// [`Self::share_code`] as the VERA version `made_by` writes it.
    fn share_code_made_by(&self, made_by: ShareVersion, ares: bool) -> String {
        let mut options = self.clone();
        options.normalize();
        let byte = |value: i32| value.clamp(0, 255) as u8;
        let mut flags = u8::from(options.urban_enabled) | u8::from(options.fair_starts) << 1;
        if ares {
            flags |= u8::from(options.urban_areas) << 2;
        }
        let building_sliders = [
            byte(options.tech_building_amount),
            byte(options.derelict_amount),
            byte(options.damage_amount),
        ];
        let population_sliders = [
            byte(options.civilian_amount),
            byte(options.animal_amount),
            byte(options.wild_amount),
        ];
        if population_sliders != [0; 3] {
            flags |= SHARE_BUILDING_SLIDERS | SHARE_POPULATION_SLIDERS;
        } else if building_sliders != [0; 3] {
            flags |= SHARE_BUILDING_SLIDERS;
        }
        let seed = options.seed_u16().to_le_bytes();
        let [major, minor, patch] = made_by.0;
        let mut bytes = vec![
            major,
            minor,
            patch,
            byte(options.theater),
            byte(options.map_type),
            byte(options.resources),
            byte(options.time),
            byte(options.num_players),
            byte(options.width),
            byte(options.height),
            byte(options.ruggedness),
            byte(options.water_amount),
            byte(options.tiberium),
            byte(options.tiberium_layout),
            byte(options.vegetation),
            byte(options.urban_presence),
            byte(options.accessibility),
            byte(options.region_size),
            flags,
            seed[0],
            seed[1],
        ];
        // The Extras last, without the trailing ones that are off, so a code
        // from before an Extra existed reads as that Extra off.
        let extras: Vec<u8> = super::extras::Extra::ALL
            .iter()
            .map(|&extra| options.extras.get(extra))
            .collect();
        let used = extras
            .iter()
            .rposition(|&value| value != 0)
            .map_or(0, |at| at + 1);
        bytes.extend_from_slice(&extras[..used]);
        if flags & SHARE_BUILDING_SLIDERS != 0 {
            bytes.extend_from_slice(&building_sliders);
        }
        if flags & SHARE_POPULATION_SLIDERS != 0 {
            bytes.extend_from_slice(&population_sliders);
        }
        bytes.push(share_check(&bytes));

        let mut code = String::from(SHARE_PREFIX);
        let (mut bits, mut held, mut written) = (0u32, 0u32, 0usize);
        let mut push = |code: &mut String, digit: u32| {
            if written % 5 == 0 {
                code.push('-');
            }
            code.push(SHARE_DIGITS[digit as usize] as char);
            written += 1;
        };
        for byte in bytes {
            bits = bits << 8 | u32::from(byte);
            held += 8;
            while held >= 5 {
                held -= 5;
                push(&mut code, bits >> held & 31);
            }
        }
        if held > 0 {
            push(&mut code, bits << (5 - held) & 31);
        }
        code
    }

    /// Read a [`Self::share_code`] back over `self`, keeping the description,
    /// with the VERA version that made it: another version's generator may
    /// build another map from the same options. Letters may be in either
    /// case, `I`/`L` read as `1` and `O` as `0`, and dashes and spaces are
    /// skipped. `None` for a text that is no code, is cut short or fails its
    /// check; `self` is then unchanged. `ares`: Ares.dll is loaded (the game
    /// is read the Ares way), so the Urban Areas box is read too.
    pub fn from_share_code(&self, code: &str, ares: bool) -> Option<(Self, ShareVersion)> {
        let text = code.trim();
        let body = text
            .get(..SHARE_PREFIX.len())
            .filter(|prefix| prefix.eq_ignore_ascii_case(SHARE_PREFIX))?;
        let body = &text[body.len()..];
        let (mut bits, mut held) = (0u32, 0u32);
        let mut bytes = Vec::new();
        for character in body.chars() {
            let digit = match character.to_ascii_uppercase() {
                '-' | ' ' => continue,
                'I' | 'L' => 1,
                'O' => 0,
                upper => SHARE_DIGITS
                    .iter()
                    .position(|&digit| digit as char == upper)? as u32,
            };
            bits = bits << 5 | digit;
            held += 5;
            if held >= 8 {
                held -= 8;
                bytes.push((bits >> held & 0xFF) as u8);
            }
        }
        let (&check, bytes) = bytes.split_last()?;
        if bytes.len() < SHARE_VERSION_BYTES + SHARE_OPTION_BYTES || check != share_check(bytes) {
            return None;
        }
        let (version, bytes) = bytes.split_at(SHARE_VERSION_BYTES);
        let made_by = ShareVersion([version[0], version[1], version[2]]);
        let (extras, sliders) = share_extras_and_sliders(bytes[15], &bytes[SHARE_OPTION_BYTES..])?;
        let field = |at: usize| i32::from(bytes[at]);
        let mut options = Self {
            theater: field(0),
            map_type: field(1),
            resources: field(2),
            time: field(3),
            num_players: field(4),
            width: field(5),
            height: field(6),
            ruggedness: field(7),
            water_amount: field(8),
            tiberium: field(9),
            tiberium_layout: field(10),
            vegetation: field(11),
            urban_presence: field(12),
            accessibility: field(13),
            region_size: field(14),
            urban_enabled: bytes[15] & 1 != 0,
            fair_starts: bytes[15] & 2 != 0,
            seed: i32::from(u16::from_le_bytes([bytes[16], bytes[17]])),
            tech_building_amount: sliders.first().copied().map_or(0, i32::from),
            derelict_amount: sliders.get(1).copied().map_or(0, i32::from),
            damage_amount: sliders.get(2).copied().map_or(0, i32::from),
            civilian_amount: sliders.get(3).copied().map_or(0, i32::from),
            animal_amount: sliders.get(4).copied().map_or(0, i32::from),
            wild_amount: sliders.get(5).copied().map_or(0, i32::from),
            ..self.clone()
        };
        if ares {
            options.urban_areas = bytes[15] & 4 != 0;
        }
        options.extras = super::extras::RmgExtras::default();
        for (&extra, &value) in super::extras::Extra::ALL.iter().zip(extras) {
            options.extras.set(extra, value);
        }
        options.normalize();
        Some((options, made_by))
    }
}

/// The VERA version that made a share code: major, minor and patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareVersion(pub [u8; 3]);

impl ShareVersion {
    /// This build's package version, each part held to a byte.
    pub fn this_build() -> Self {
        let mut parts = env!("CARGO_PKG_VERSION")
            .split(['.', '-', '+'])
            .map(|part| part.parse::<u32>().map_or(0, |part| part.min(255) as u8));
        Self(std::array::from_fn(|_| parts.next().unwrap_or(0)))
    }
}

impl std::fmt::Display for ShareVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
    }
}

/// What a share code starts with: VERA random map, layout 1.
const SHARE_PREFIX: &str = "VR1";
/// Crockford's base 32 digits: no `I`, `L`, `O` or `U`.
const SHARE_DIGITS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// The bytes of the version a share code opens with.
const SHARE_VERSION_BYTES: usize = 3;
/// The option bytes every share code holds between its version and Extras.
const SHARE_OPTION_BYTES: usize = 18;

const SHARE_BUILDING_SLIDERS: u8 = 1 << 3;
const SHARE_POPULATION_SLIDERS: u8 = 1 << 4;

/// The original layout trims trailing zero Extras, then appends either no
/// sliders or three building sliders. A development build also wrote six
/// sliders with the three-slider flag. Prefer the original interpretation
/// whenever both could have been emitted; new six-slider codes set both bits.
fn share_extras_and_sliders(flags: u8, tail: &[u8]) -> Option<(&[u8], &[u8])> {
    let count = match flags & (SHARE_BUILDING_SLIDERS | SHARE_POPULATION_SLIDERS) {
        0 => 0,
        SHARE_BUILDING_SLIDERS => {
            let (extras, sliders) = tail.split_last_chunk::<3>()?;
            let could_be_three = extras.len() <= super::extras::Extra::ALL.len()
                && extras.last() != Some(&0)
                && super::extras::Extra::ALL
                    .iter()
                    .zip(extras)
                    .all(|(extra, &v)| v < extra.choices())
                && sliders.iter().any(|&value| value != 0);
            if could_be_three { 3 } else { 6 }
        }
        SHARE_POPULATION_SLIDERS => return None,
        _ => 6,
    };
    let at = tail.len().checked_sub(count)?;
    Some(tail.split_at(at))
}

/// A share code's check byte over its other bytes.
fn share_check(bytes: &[u8]) -> u8 {
    bytes
        .iter()
        .fold(0x5Au8, |check, &byte| check.rotate_left(3) ^ byte)
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;

    fn sed_description(raw: &str) -> SeedDescription {
        let ini = IniFile::from_str(&format!("[RandomMap]\nDescription={raw}\n"));
        read_description(ini.section_or_empty(SECTION), &SeedDescription::default())
    }

    /// VERA's towns key is written only when set, so a seed without it turns
    /// the towns off whatever options it is read over.
    #[test]
    fn a_seed_without_the_towns_key_leaves_the_towns_off() {
        let seed = |urban_enabled| {
            let options = RmgOptions {
                seed: 7,
                urban_enabled,
                ..RmgOptions::default()
            };
            IniFile::from_bytes(&options.to_sed_bytes(false)).expect("seed INI")
        };
        let mut read = RmgOptions {
            urban_enabled: true,
            ..RmgOptions::default()
        };
        read.apply_sed(&seed(false), false);
        assert!(!read.urban_enabled);
        read.apply_sed(&seed(true), false);
        assert!(read.urban_enabled);
    }

    /// VERA's Fair Starts key follows the towns key: written only when set.
    #[test]
    fn fair_starts_are_saved_only_when_set() {
        let plain = RmgOptions::default().to_sed_bytes(false);
        assert!(!String::from_utf8_lossy(&plain).contains("VeraFair"));
        let fair = RmgOptions {
            fair_starts: true,
            ..RmgOptions::default()
        };
        let mut read = RmgOptions::default();
        read.apply_sed(
            &IniFile::from_bytes(&fair.to_sed_bytes(false)).expect("seed INI"),
            false,
        );
        assert!(read.fair_starts);
        read.apply_sed(&IniFile::from_bytes(&plain).expect("seed INI"), false);
        assert!(!read.fair_starts);
    }

    /// A share code carries every option, VERA's switches and the Extras,
    /// reads back however it is typed, and refuses a damaged one.
    #[test]
    fn a_share_code_reads_back_the_same_options() {
        use crate::map::rmg::extras::Extra;
        let mut sent = RmgOptions {
            theater: 2,
            map_type: 3,
            num_players: 30,
            width: 7,
            height: 7,
            water_amount: 5,
            urban_presence: 100,
            urban_enabled: true,
            fair_starts: true,
            seed: 54321,
            description: "mine".into(),
            // These are also valid Extras, so decoding this round trip
            // depends on the explicit flag for the population sliders.
            tech_building_amount: 2,
            derelict_amount: 1,
            damage_amount: 1,
            civilian_amount: 40,
            animal_amount: 25,
            wild_amount: 55,
            ..RmgOptions::default()
        };
        sent.extras.set(Extra::TownVariant, 4);
        sent.normalize();
        let code = sent.share_code(false);
        assert!(code.starts_with("VR1-"), "{code}");

        let receiver = RmgOptions {
            description: "theirs".into(),
            ..RmgOptions::default()
        };
        let (read, made_by) = receiver
            .from_share_code(&code, false)
            .expect("a whole code reads");
        assert_eq!(made_by, ShareVersion::this_build());
        assert_eq!(
            read,
            RmgOptions {
                description: "theirs".into(),
                ..sent.clone()
            }
        );
        // Lower case, spaces for dashes and stray blanks still read.
        let typed = format!("  {}  ", code.to_ascii_lowercase().replace('-', " "));
        assert_eq!(
            receiver.from_share_code(&typed, false),
            Some((read, made_by))
        );

        // One changed character fails the check; other texts are no code.
        // Change the first data digit after the prefix — the last digit can
        // land in encoder padding and not affect any decoded byte.
        let prefix = "VR1-";
        let first = code.as_bytes()[prefix.len()] as char;
        let swapped = if first == '0' { '1' } else { '0' };
        let damaged = format!("{prefix}{swapped}{}", &code[prefix.len() + 1..]);
        assert_eq!(receiver.from_share_code(&damaged, false), None);
        assert_eq!(receiver.from_share_code("hello", false), None);
        assert_eq!(receiver.from_share_code("VR1-", false), None);

        // Options without Extras give a shorter code that reads as none.
        let plain = RmgOptions::default();
        assert!(plain.share_code(false).len() < code.len());
        let (back, _) = sent
            .from_share_code(&plain.share_code(false), false)
            .expect("reads");
        assert!(back.extras.is_default() && !back.fair_starts);
    }

    #[test]
    fn legacy_slider_codes_keep_their_extras() {
        use crate::map::rmg::extras::Extra;

        // Version 0.3.6, seed 42, Ore/Town/Mood Extras, then three building
        // sliders. A development build also wrote six under the same flag.
        let legacy_three = "VR1-001GC-00104-0G400-00000-20000-00002-1A001-0808S-695TR";
        let legacy_six = "VR1-001GC-00104-0G400-00000-20000-00002-1A001-0808S-695GA-2GF8W";
        for (code, population) in [(legacy_three, [0, 0, 0]), (legacy_six, [5, 10, 15])] {
            let (read, version) = RmgOptions::default().from_share_code(code, false).unwrap();
            assert_eq!(version, ShareVersion([0, 3, 6]));
            assert_eq!(read.seed, 42);
            assert_eq!(read.extras.get(Extra::Mood), 1);
            assert_eq!(
                [
                    read.tech_building_amount,
                    read.derelict_amount,
                    read.damage_amount
                ],
                [25, 50, 75]
            );
            assert_eq!(
                [read.civilian_amount, read.animal_amount, read.wild_amount],
                population
            );
        }
    }

    /// A code made before the three Advanced sliders existed has neither the
    /// flag bit nor the trailing bytes. Such a code is byte-for-byte what a
    /// current build writes with the sliders at 0, and reads back as 0 — so an
    /// old friend's code never gains derelicts or damage it never asked for.
    #[test]
    fn a_share_code_without_sliders_reads_as_original_behaviour() {
        let old_style = RmgOptions {
            seed: 2468,
            water_amount: 44,
            num_players: 12,
            urban_enabled: true,
            fair_starts: true,
            description: "old map".into(),
            tech_building_amount: 0,
            derelict_amount: 0,
            damage_amount: 0,
            ..RmgOptions::default()
        };
        let code = old_style.share_code(false);
        let (read, _) = RmgOptions::default()
            .from_share_code(&code, false)
            .expect("an old-style code reads");
        assert_eq!(read.tech_building_amount, 0);
        assert_eq!(read.derelict_amount, 0);
        assert_eq!(read.damage_amount, 0);
        // The pre-slider options still round-trip unchanged.
        assert_eq!(read.seed, old_style.seed);
        assert_eq!(read.water_amount, old_style.water_amount);
        assert_eq!(read.num_players, old_style.num_players);
        assert_eq!(read.urban_enabled, old_style.urban_enabled);
        assert_eq!(read.fair_starts, old_style.fair_starts);
    }

    /// The Urban Areas box rides flag bit 2 only when Ares.dll is loaded:
    /// without it the bit is neither written nor read.
    #[test]
    fn a_share_code_carries_the_urban_areas_box_only_with_ares() {
        let sent = RmgOptions {
            seed: 7,
            urban_areas: true,
            ..RmgOptions::default()
        };
        let read = |code: String, ares| {
            RmgOptions::default()
                .from_share_code(&code, ares)
                .expect("a whole code reads")
                .0
                .urban_areas
        };
        assert!(read(sent.share_code(true), true));
        assert!(!read(sent.share_code(false), true));
        assert!(!read(sent.share_code(true), false));
    }

    /// A code from another VERA version still reads, and names that version,
    /// whose generator may build another map from the same options.
    #[test]
    fn a_share_code_names_the_version_that_made_it() {
        let this = ShareVersion::this_build();
        assert!(env!("CARGO_PKG_VERSION").starts_with(&this.to_string()));
        let older = ShareVersion([0, 1, 9]);
        assert_ne!(older, this);
        let sent = RmgOptions {
            seed: 99,
            ..RmgOptions::default()
        };
        let code = sent.share_code_made_by(older, false);
        assert_ne!(
            code,
            sent.share_code(false),
            "the version is part of the code"
        );
        let (read, made_by) = RmgOptions::default()
            .from_share_code(&code, false)
            .expect("another version's code reads");
        assert_eq!((read.seed, made_by.to_string()), (99, "0.1.9".to_string()));
    }

    /// An Extra is saved under its own key only when on.
    #[test]
    fn extras_are_saved_only_when_set() {
        use crate::map::rmg::extras::Extra;
        let plain = RmgOptions::default().to_sed_bytes(false);
        assert!(!String::from_utf8_lossy(&plain).contains("VeraOreVariant"));
        let mut varied = RmgOptions::default();
        varied.extras.set(Extra::OreVariant, 3);
        let mut read = RmgOptions::default();
        read.apply_sed(
            &IniFile::from_bytes(&varied.to_sed_bytes(false)).expect("seed INI"),
            false,
        );
        assert_eq!(read.extras.get(Extra::OreVariant), 3);
        read.apply_sed(&IniFile::from_bytes(&plain).expect("seed INI"), false);
        assert!(read.extras.is_default());
    }

    #[test]
    fn the_digest_hashes_the_options_and_not_the_description() {
        let options = RmgOptions {
            seed: 1234,
            ..RmgOptions::default()
        };
        let digest = options.native_digest();
        assert_eq!(digest.len(), 8);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        );
        let described = RmgOptions {
            description: sed_description("41,"),
            ..options.clone()
        };
        assert_eq!(described.native_digest(), digest);
        let reseeded = RmgOptions {
            seed: 1235,
            ..options.clone()
        };
        assert_ne!(reseeded.native_digest(), digest);
    }

    #[test]
    fn theater_is_never_clamped() {
        let mut high = RmgOptions {
            theater: 99,
            ..Default::default()
        };
        high.normalize();
        assert_eq!(high.theater, 99, "the normalizer never writes theater");

        let mut negative = RmgOptions {
            theater: -5,
            ..Default::default()
        };
        negative.normalize();
        assert_eq!(negative.theater, -5);
    }

    #[test]
    fn clamp_bounds_match_the_original() {
        let mut options = RmgOptions {
            theater: 0,
            resources: 9,
            map_type: 9,
            time: 9,
            ruggedness: 500,
            water_amount: -1,
            num_players: 1,
            tiberium: 0,
            tiberium_layout: 500,
            vegetation: -3,
            urban_presence: 900,
            urban_enabled: false,
            fair_starts: false,
            extras: Default::default(),
            width: 9,
            height: 9,
            accessibility: 101,
            region_size: -1,
            seed: 0x1_0000,
            description: SeedDescription::default(),
            urban_areas: false,
            tech_building_amount: 500,
            derelict_amount: -3,
            damage_amount: 200,
            civilian_amount: 500,
            animal_amount: -9,
            wild_amount: 200,
        };
        options.normalize();

        assert_eq!(
            (options.resources, options.map_type, options.time),
            (3, 4, 3)
        );
        assert_eq!((options.ruggedness, options.water_amount), (100, 0));
        assert_eq!(options.num_players, 2, "player floor is 2, not 0");
        assert_eq!(options.tiberium, 1, "tiberium floor is 1, not 0");
        assert_eq!(
            (
                options.tiberium_layout,
                options.vegetation,
                options.urban_presence
            ),
            (100, 0, 100)
        );
        assert_eq!((options.width, options.height), (7, 7));
        assert_eq!((options.accessibility, options.region_size), (100, 0));
        assert_eq!(options.seed, 0xFFFF);
        assert_eq!(
            (
                options.tech_building_amount,
                options.derelict_amount,
                options.damage_amount,
                options.civilian_amount,
                options.animal_amount,
                options.wild_amount
            ),
            (100, 0, 100, 100, 0, 100)
        );
    }

    #[test]
    fn defaults_match_the_original_constructor() {
        let options = RmgOptions::default();
        assert_eq!(options.map_type, 1);
        assert_eq!(options.resources, 1);
        assert_eq!(options.time, 1);
        assert_eq!(options.num_players, 2);
        assert_eq!(options.theater, 0);
        assert_eq!(options.seed, -1, "unset seed is -1 before normalizing");
    }

    #[test]
    fn missing_keys_carry_existing_values() {
        let mut options = RmgOptions {
            ruggedness: 42,
            num_players: 6,
            ..Default::default()
        };
        options.apply_sed(&IniFile::from_str("[RandomMap]\nSeed=7\n"), false);

        assert_eq!(options.seed, 7, "present key applies");
        assert_eq!(options.ruggedness, 42, "absent key carries, never zeroes");
        assert_eq!(options.num_players, 6);
    }

    #[test]
    fn malformed_value_uses_native_atoi_zero() {
        let mut options = RmgOptions {
            tiberium: 55,
            ..Default::default()
        };
        options.apply_sed(&IniFile::from_str("[RandomMap]\nTiberium=abc\n"), false);
        assert_eq!(options.tiberium, 0);
    }

    #[test]
    fn sed_round_trips() {
        // The seed writes and reads `UrbanAreas=` only when Ares.dll is loaded.
        for ares in [false, true] {
            let mut original = RmgOptions {
                theater: 2,
                map_type: 3,
                resources: 2,
                ruggedness: 40,
                time: 1,
                water_amount: 55,
                num_players: 6,
                tiberium: 30,
                tiberium_layout: 20,
                vegetation: 70,
                urban_presence: 10,
                urban_enabled: false,
                fair_starts: false,
                extras: Default::default(),
                width: 2,
                height: 3,
                accessibility: 60,
                region_size: 45,
                seed: 4321,
                description: "Round Trip".into(),
                urban_areas: ares,
                tech_building_amount: 60,
                derelict_amount: 35,
                damage_amount: 80,
                civilian_amount: 40,
                animal_amount: 25,
                wild_amount: 55,
            };
            original.normalize();

            let bytes = original.to_sed_bytes(ares);
            let mut parsed = RmgOptions::default();
            parsed.apply_sed(&IniFile::from_bytes(&bytes).unwrap(), ares);
            parsed.normalize();

            assert_eq!(parsed, original);
        }
    }

    #[test]
    fn seed_u16_survives_the_unset_sentinel() {
        let options = RmgOptions::default();
        assert_eq!(options.seed, -1);
        assert_eq!(options.seed_u16(), 0, "-1 must not wrap to 0xFFFF");
    }

    #[test]
    fn description_encodes_to_the_native_comma_hex_form() {
        // The trailing comma is not a typo: the original appends the delimiter
        // after every code unit, including the last.
        assert_eq!(
            encode_description(&"Random Map".into()),
            "52,61,6e,64,6f,6d,20,4d,61,70,"
        );
    }

    #[test]
    fn description_is_the_first_emitted_key() {
        let bytes = RmgOptions::default().to_sed_bytes(false);
        let text = String::from_utf8(bytes).expect("ini is utf-8");
        let body = text.split("[RandomMap]").nth(1).expect("section present");
        let first = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        assert!(first.starts_with("Description"), "got {first:?}");
    }

    #[test]
    fn malformed_description_tokens_repeat_the_previous_conversion() {
        assert_eq!(sed_description("52,zz,61,"), "RRa");
    }

    #[test]
    fn seed_persistence_preserves_unpaired_utf16_units() {
        // NewEdit Delete/Backspace works in UTF-16 units and can leave a high
        // surrogate without its low half. Display conversion is not storage.
        let original = RmgOptions {
            description: SeedDescription::from_units([0xd83d, 0x41]),
            ..RmgOptions::default()
        };
        assert_eq!(original.description.display_text(), "\u{fffd}A");
        let bytes = original.to_sed_bytes(false);
        let mut loaded = RmgOptions::default();
        loaded.apply_sed(&IniFile::from_bytes(&bytes).unwrap(), false);
        assert_eq!(loaded.description.units(), [0xd83d, 0x41]);
        assert_eq!(loaded.description, original.description);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("Description=d83d,41,")
        );
    }
}
