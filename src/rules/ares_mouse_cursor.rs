//! Ares 3.0 named mouse cursors: the `[MouseCursors]` list and the type keys
//! that name one of its entries.
//!
//! - `MouseCursorTypes::InitDefaults @ 10007130`: the 86 vanilla cursors of
//!   gamemd's table at `0x0082D028`, under the names of Ares.dll `10094B20`,
//!   then nine Ares cursors. `YR_GameInit_Pre @ 1005D7F0` (hook `0x52BA78`)
//!   first gives six vanilla rows an interval of 4.
//! - `RulesExt::LoadBeforeTypeData @ 1002AEF0`: each Rules pass allocates its
//!   `[MouseCursors]` keys, then `MouseCursorTypeEntry::LoadFromINI @
//!   100075B0` reads every entry of the list.
//! - `detail::read<MouseCursorType> @ 1000FB50` (`Valueable<MouseCursorType>
//!   ::Read @ 1000EBB0`): a type key that names an entry, stored as its index.
//!
//! The list is presentation data. The client draws every software cursor
//! through it (`render::cursor_atlas`) and picks action cursors with it
//! (`app::input::ares_cursor`); no simulation state, RNG draw or hash reads it.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::parse_ares_int;

/// `MouseHotSpotX::Left`/`MouseHotSpotY::Top`.
pub const HOTSPOT_NEAR: i32 = 0;
/// `MouseHotSpotX::Center`/`MouseHotSpotY::Middle`.
pub const HOTSPOT_CENTRE: i32 = 12345;
/// `MouseHotSpotX::Right`/`MouseHotSpotY::Bottom`.
pub const HOTSPOT_FAR: i32 = 54321;

/// The indexes of the entries [`MouseCursors::default`] creates: the
/// vanilla `MouseCursorType` values and the nine Ares additions
/// (`AresMouseCursorType`, in `InitDefaults` order). A type key stores one of
/// these, the index of a `[MouseCursors]` entry, or -1 for `none`.
pub mod index {
    pub const DEFAULT: i32 = 0x00;
    pub const MOVE: i32 = 0x12;
    pub const NO_MOVE: i32 = 0x13;
    pub const ATTACK: i32 = 0x14;
    pub const ATTACK_OUT_OF_RANGE: i32 = 0x15;
    pub const ENTER: i32 = 0x19;
    pub const NO_ENTER: i32 = 0x1A;
    pub const DEPLOY: i32 = 0x1B;
    pub const NO_DEPLOY: i32 = 0x1C;
    pub const REPAIR: i32 = 0x21;
    pub const IVAN_BOMB: i32 = 0x26;
    pub const PARA_DROP: i32 = 0x2F;
    pub const LIGHTNING_STORM: i32 = 0x32;
    pub const DETONATE: i32 = 0x33;
    pub const NUKE: i32 = 0x35;
    pub const CURSOR_36: i32 = 0x36;
    pub const POWER: i32 = 0x37;
    pub const IRON_CURTAIN: i32 = 0x39;
    pub const CHRONOSPHERE: i32 = 0x3A;
    pub const DISALLOWED: i32 = 0x3C;
    pub const FORCE_SHIELD: i32 = 0x4F;
    pub const NO_FORCE_SHIELD: i32 = 0x50;
    pub const GENETIC_MUTATOR: i32 = 0x51;
    pub const PSYCHIC_DOMINATOR: i32 = 0x53;
    pub const PSYCHIC_REVEAL: i32 = 0x54;
    pub const SPY_PLANE: i32 = 0x55;
    pub const TOTE: i32 = 0x56;
    pub const ENGINEER_DAMAGE: i32 = 0x57;
    pub const TOGGLE_POWER: i32 = 0x58;
    pub const NO_TOGGLE_POWER: i32 = 0x59;
    pub const INFANTRY_HEAL: i32 = 0x5A;
    pub const UNIT_REPAIR: i32 = 0x5B;
    pub const TAKE_VEHICLE: i32 = 0x5C;
    pub const SABOTAGE: i32 = 0x5D;
    pub const REPAIR_TRENCH: i32 = 0x5E;
}

/// YRpp `MouseCursor`, 28 bytes: the first frame of `MOUSE.SHA`, the frame
/// count, the animation interval in 16 ms timer units, the radar's first
/// frame and count (-1 when the cursor has none) and the two hotspot
/// sentinels ([`HOTSPOT_NEAR`], [`HOTSPOT_CENTRE`], [`HOTSPOT_FAR`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MouseCursor {
    pub frame: i32,
    pub count: i32,
    pub interval: i32,
    pub mini_frame: i32,
    pub mini_count: i32,
    pub hot_x: i32,
    pub hot_y: i32,
}

const fn c(
    frame: i32,
    count: i32,
    interval: i32,
    mini_frame: i32,
    mini_count: i32,
    hot_x: i32,
    hot_y: i32,
) -> MouseCursor {
    MouseCursor {
        frame,
        count,
        interval,
        mini_frame,
        mini_count,
        hot_x,
        hot_y,
    }
}

const N: i32 = HOTSPOT_NEAR;
const C: i32 = HOTSPOT_CENTRE;
const F: i32 = HOTSPOT_FAR;

/// gamemd's compiled cursor table (`0x0082D028`, `MouseCursor::First`,
/// dumped to `tools/mouse_oracle/cursor_table.json`) under the names Ares
/// gives its rows (`10094B20`).
const VANILLA: [(&str, MouseCursor); 86] = [
    ("Default", c(0, 1, 0, 1, 1, N, N)),
    ("MoveN", c(2, 1, 0, -1, -1, C, N)),
    ("MoveNE", c(3, 1, 0, -1, -1, F, N)),
    ("MoveE", c(4, 1, 0, -1, -1, F, C)),
    ("MoveSE", c(5, 1, 0, -1, -1, F, F)),
    ("MoveS", c(6, 1, 0, -1, -1, C, F)),
    ("MoveSW", c(7, 1, 0, -1, -1, N, F)),
    ("MoveW", c(8, 1, 0, -1, -1, N, C)),
    ("MoveNW", c(9, 1, 0, -1, -1, N, N)),
    ("NoMoveN", c(10, 1, 0, -1, -1, C, N)),
    ("NoMoveNE", c(11, 1, 0, -1, -1, F, N)),
    ("NoMoveE", c(12, 1, 0, -1, -1, F, C)),
    ("NoMoveSE", c(13, 1, 0, -1, -1, F, F)),
    ("NoMoveS", c(14, 1, 0, -1, -1, C, F)),
    ("NoMoveSW", c(15, 1, 0, -1, -1, N, F)),
    ("NoMoveW", c(16, 1, 0, -1, -1, N, C)),
    ("NoMoveNW", c(17, 1, 0, -1, -1, N, N)),
    ("Select", c(18, 13, 4, -1, -1, C, C)),
    ("Move", c(31, 10, 4, 42, 10, C, C)),
    ("NoMove", c(41, 1, 0, 52, 1, C, C)),
    ("Attack", c(53, 5, 4, 63, 5, C, C)),
    ("AttackOutOfRange", c(58, 5, 4, 63, 5, C, C)),
    ("CUR_16", c(68, 5, 4, 73, 5, C, C)),
    ("DesolatorDeploy", c(78, 10, 4, -1, 10, C, C)),
    ("CUR_18", c(88, 1, 0, -1, 1, C, C)),
    ("Enter", c(89, 10, 4, 100, 10, C, C)),
    ("NoEnter", c(99, 1, 0, 63, 1, C, C)),
    ("Deploy", c(110, 9, 4, -1, -1, C, C)),
    ("NoDeploy", c(119, 1, 0, -1, -1, C, C)),
    ("CUR_1D", c(120, 9, 4, -1, -1, C, C)),
    ("Sell", c(129, 10, 4, -1, -1, C, C)),
    ("SellUnit", c(139, 10, 4, -1, -1, C, C)),
    ("NoSell", c(149, 1, 0, -1, -1, C, C)),
    ("Repair", c(150, 20, 4, -1, -1, C, C)),
    ("EngineerRepair", c(170, 20, 4, -1, -1, C, C)),
    ("NoRepair", c(190, 1, 0, -1, -1, C, C)),
    ("CUR_24", c(191, 7, 0, -1, -1, C, C)),
    ("Disguise", c(199, 5, 0, -1, -1, C, C)),
    ("IvanBomb", c(204, 5, 0, -1, -1, C, C)),
    ("MindControl", c(209, 5, 0, -1, -1, C, C)),
    ("RemoveSquid", c(214, 5, 0, -1, -1, C, C)),
    ("Crush", c(219, 5, 0, -1, -1, C, C)),
    ("SpyTech", c(224, 5, 0, -1, -1, C, C)),
    ("SpyPower", c(229, 5, 0, -1, -1, C, C)),
    ("CUR_2C", c(234, 5, 0, -1, -1, C, C)),
    ("GIDeploy", c(239, 10, 0, -1, -1, C, C)),
    ("CUR_2E", c(249, 10, 0, -1, -1, C, C)),
    ("Paradrop", c(259, 10, 0, 516, -1, C, C)),
    ("CUR_30", c(269, 10, 0, -1, -1, C, C)),
    ("CUR_31", c(356, 1, 0, -1, -1, C, C)),
    ("LightningStorm", c(279, 20, 4, 514, 1, C, C)),
    ("Detonate", c(299, 10, 0, -1, -1, C, C)),
    ("Demolish", c(309, 10, 4, -1, -1, C, C)),
    ("Nuke", c(319, 10, 4, 513, 1, C, C)),
    ("CUR_36", c(329, 10, 0, -1, -1, C, C)),
    ("Power", c(339, 6, 0, -1, -1, C, C)),
    ("CUR_38", c(345, 1, 0, -1, -1, C, C)),
    ("IronCurtain", c(346, 5, 0, -1, -1, C, C)),
    ("Chronosphere", c(357, 12, 0, -1, -1, C, C)),
    ("Disarm", c(369, 15, 0, -1, -1, C, C)),
    ("CUR_3C", c(384, 1, 0, -1, -1, C, C)),
    ("Scroll", c(385, 1, 0, -1, -1, C, C)),
    ("ScrollESW", c(386, 1, 0, -1, -1, C, C)),
    ("ScrollSW", c(387, 1, 0, -1, -1, C, C)),
    ("ScrollNSW", c(388, 1, 0, -1, -1, C, C)),
    ("ScrollNW", c(389, 1, 0, -1, -1, C, C)),
    ("ScrollNEW", c(390, 1, 0, -1, -1, C, C)),
    ("ScrollNE", c(391, 1, 0, -1, -1, C, C)),
    ("ScrollNES", c(392, 1, 0, -1, -1, C, C)),
    ("ScrollES", c(393, 1, 0, -1, -1, C, C)),
    ("CUR_46", c(394, 10, 4, -1, -1, C, C)),
    ("AttackMove", c(404, 9, 4, 63, 5, C, C)),
    ("CUR_48", c(413, 9, 4, -1, -1, C, C)),
    ("InfantryAbsorb", c(422, 9, 4, -1, -1, C, C)),
    ("NoMindControl", c(431, 1, 0, -1, -1, C, C)),
    ("CUR_4B", c(432, 1, 0, -1, -1, C, C)),
    ("CUR_4C", c(433, 1, 0, -1, -1, C, C)),
    ("CUR_4D", c(434, 1, 0, -1, -1, C, C)),
    ("Beacon", c(435, 15, 4, -1, -1, C, C)),
    ("ForceShield", c(450, 10, 4, -1, 1, C, C)),
    ("NoForceShield", c(460, 10, 4, -1, -1, C, C)),
    ("GeneticMutator", c(470, 10, 4, -1, -1, C, C)),
    ("AirStrike", c(480, 8, 4, -1, -1, C, C)),
    ("PsychicDominator", c(488, 8, 4, 516, -1, C, C)),
    ("PsychicReveal", c(496, 8, 4, 515, 1, C, C)),
    ("SpyPlane", c(504, 8, 4, 512, 1, C, C)),
];

/// The rows `YR_GameInit_Pre @ 1005D7F0` (hook `0x52BA78`) animates before
/// `InitDefaults` copies the table: ParaDrop, Chronosphere, IronCurtain,
/// Detonate, `0x36` and IvanBomb get `Interval = 4`.
const ANIMATED_BY_ARES: [i32; 6] = [
    index::PARA_DROP,
    index::CHRONOSPHERE,
    index::IRON_CURTAIN,
    index::DETONATE,
    index::CURSOR_36,
    index::IVAN_BOMB,
];

/// One named entry (`MouseCursorTypeEntry`, 0x3C bytes): `char Name[0x20]`
/// and its `MouseCursor`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct CursorTypeEntry {
    name: String,
    cursor: MouseCursor,
}

impl CursorTypeEntry {
    /// `MouseCursorTypeEntry @ 100070D0`: the name copied by `strCopy`
    /// (`strncpy_s(dest, 0x20, src, _TRUNCATE)`, 31 bytes) and the cursor
    /// `0, 1, 0, -1, -1, Left, Top`.
    fn new(name: &str) -> Self {
        Self {
            name: crate::rules::ini_value::truncate_native_bytes(name, 31).to_owned(),
            cursor: c(0, 1, 0, -1, -1, HOTSPOT_NEAR, HOTSPOT_NEAR),
        }
    }
}

/// `MouseCursorTypes::Array` (Ares.dll `100C18C0`): the named cursors, in
/// allocation order. A cursor index is a position in this list.
///
/// The list lives with the Rules registries: `Clear @ 100074C0` recreates
/// the defaults when a scenario clears its classes, ordinary Rules passes add
/// to it, and saves keep it (`SaveGlobals @ 10007890`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MouseCursors {
    entries: Vec<CursorTypeEntry>,
}

impl Default for MouseCursors {
    fn default() -> Self {
        let mut cursors = Self {
            entries: Vec::new(),
        };
        cursors.init_defaults();
        cursors
    }
}

impl MouseCursors {
    /// `MouseCursorTypes::InitDefaults @ 10007130`: the vanilla cursors get
    /// their names and the table's data, then nine Ares cursors follow, all
    /// but InfantryHeal copies of a vanilla row read before any of the nine
    /// is written. An existing entry keeps its place.
    fn init_defaults(&mut self) {
        for (row, &(name, mut cursor)) in VANILLA.iter().enumerate() {
            if ANIMATED_BY_ARES.contains(&(row as i32)) {
                cursor.interval = 4;
            }
            let entry = self.find_or_allocate(name);
            self.entries[entry].cursor = cursor;
        }
        let row = |index: i32| self.entries[index as usize].cursor;
        let ares = [
            (row(index::CURSOR_36), "Tote"),
            (row(index::DETONATE), "EngineerDamage"),
            (row(index::POWER), "TogglePower"),
            (row(index::DISALLOWED), "NoTogglePower"),
            (c(0x163, 1, 0, -1, -1, C, C), "InfantryHeal"),
            (row(index::REPAIR), "UnitRepair"),
            (row(index::ENTER), "TakeVehicle"),
            (row(index::ENTER), "Sabotage"),
            (row(index::REPAIR), "RepairTrench"),
        ];
        for (cursor, name) in ares {
            let entry = self.find_or_allocate(name);
            self.entries[entry].cursor = cursor;
        }
    }

    fn find_or_allocate(&mut self, name: &str) -> usize {
        if let Some(index) = self.position(name) {
            return index;
        }
        self.entries.push(CursorTypeEntry::new(name));
        self.entries.len() - 1
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// `MouseCursorTypes::Find @ 10007520`: the index of the entry named
    /// `name` (`strcmpi`), if any.
    pub fn find(&self, name: &str) -> Option<i32> {
        self.position(name).map(|index| index as i32)
    }

    /// `MouseCursorTypes::GetCursor @ 10007580` over the list. Ares returns
    /// the index itself as the pointer for one outside the list, which every
    /// reader then dereferences near address 0; VERA answers `None`.
    pub fn get(&self, index: i32) -> Option<&MouseCursor> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.entries.get(index))
            .map(|entry| &entry.cursor)
    }

    /// Every entry's index and cursor, in list order.
    pub fn iter(&self) -> impl Iterator<Item = (i32, &MouseCursor)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (index as i32, &entry.cursor))
    }

    /// `RulesExt::LoadBeforeTypeData @ 1002AEF0`'s `[MouseCursors]` block:
    /// every key of this pass not yet in the list is appended, then
    /// `MouseCursorTypeEntry::LoadFromINI @ 100075B0` reads each entry of the
    /// whole list, in list order. A value naming an entry copies that entry's
    /// current cursor; otherwise up to seven comma tokens (`Ares_strtok
    /// @ 10007920`, which skips empty ones) replace Frame, Count, Interval,
    /// MiniFrame, MiniCount (`Parser<int>::TryParse @ 10076A90`) and the
    /// hotspots (`left`/`center`/`right` @ 10076BC0, `top`/`middle`/`bottom`
    /// @ 10076C40); a token that does not parse keeps its field.
    pub(crate) fn read_pass(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let section = pass.section_or_empty("MouseCursors");
        for name in section.keys() {
            self.find_or_allocate(name);
        }
        for index in 0..self.entries.len() {
            let Some(value) = section.read_name(&self.entries[index].name, 0x800) else {
                continue;
            };
            if let Some(other) = self.position(value) {
                self.entries[index].cursor = self.entries[other].cursor;
                continue;
            }
            let cursor = &mut self.entries[index].cursor;
            let mut tokens = value.split(',').filter(|token| !token.is_empty());
            for field in [
                &mut cursor.frame,
                &mut cursor.count,
                &mut cursor.interval,
                &mut cursor.mini_frame,
                &mut cursor.mini_count,
            ] {
                let Some(token) = tokens.next() else {
                    break;
                };
                if let Some(value) = parse_ares_int(token) {
                    *field = value;
                }
            }
            let hotspot = |token: &str, near: &str, centre: &str, far: &str| {
                [
                    (near, HOTSPOT_NEAR),
                    (centre, HOTSPOT_CENTRE),
                    (far, HOTSPOT_FAR),
                ]
                .into_iter()
                .find_map(|(name, value)| token.eq_ignore_ascii_case(name).then_some(value))
            };
            if let Some(value) = tokens
                .next()
                .and_then(|token| hotspot(token, "left", "center", "right"))
            {
                cursor.hot_x = value;
            }
            if let Some(value) = tokens
                .next()
                .and_then(|token| hotspot(token, "top", "middle", "bottom"))
            {
                cursor.hot_y = value;
            }
        }
    }

    /// `detail::read<MouseCursorType> @ 1000FB50`: a nonempty value naming an
    /// entry stores its index, `none`/`<none>` (`IsBlank`) stores -1, and
    /// anything else, like an absent key, keeps `current`.
    pub(crate) fn read(&self, section: &IniSection, key: &str, current: i32) -> i32 {
        let _ares = crate::rules::ares_switch::ares_reads();
        section.read_ares_mouse_cursor(key, current, |name| self.find(name))
    }
}

/// TechnoTypeExt `Cursor.*` (+0x590..+0x5A4, read by `LoadFromINIFile @
/// 1003DFB0`) and BuildingTypeExt `Cursor.Spy` (+0x240, `LoadFromINIFile
/// @ 1000DD40`), with their constructor defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TechnoCursors {
    pub deploy: i32,
    pub no_deploy: i32,
    pub enter: i32,
    pub no_enter: i32,
    pub move_: i32,
    pub no_move: i32,
    /// A BuildingType's cursor for a spy that infiltrates it.
    pub spy: i32,
}

impl Default for TechnoCursors {
    fn default() -> Self {
        Self {
            deploy: index::DEPLOY,
            no_deploy: index::NO_DEPLOY,
            enter: index::ENTER,
            no_enter: index::NO_ENTER,
            move_: index::MOVE,
            no_move: index::NO_MOVE,
            spy: index::ENTER,
        }
    }
}

impl TechnoCursors {
    /// The six TechnoTypeExt reads, in `1003DFB0`'s order.
    pub(crate) fn read_techno_pass(&mut self, section: &IniSection, cursors: &MouseCursors) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.deploy = cursors.read(section, "Cursor.Deploy", self.deploy);
        self.no_deploy = cursors.read(section, "Cursor.NoDeploy", self.no_deploy);
        self.enter = cursors.read(section, "Cursor.Enter", self.enter);
        self.no_enter = cursors.read(section, "Cursor.NoEnter", self.no_enter);
        self.move_ = cursors.read(section, "Cursor.Move", self.move_);
        self.no_move = cursors.read(section, "Cursor.NoMove", self.no_move);
    }

    /// BuildingTypeExt `Cursor.Spy` (`1000DD40`).
    pub(crate) fn read_building_pass(&mut self, section: &IniSection, cursors: &MouseCursors) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.spy = cursors.read(section, "Cursor.Spy", self.spy);
    }
}

/// WeaponTypeExt `Cursor.Attack` and `Cursor.AttackOutOfRange` (+0xD4/+0xD8,
/// `LoadFromINIFile @ 10055520`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WeaponCursors {
    pub attack: i32,
    pub attack_out_of_range: i32,
}

impl Default for WeaponCursors {
    fn default() -> Self {
        Self {
            attack: index::ATTACK,
            attack_out_of_range: index::ATTACK_OUT_OF_RANGE,
        }
    }
}

impl WeaponCursors {
    pub(crate) fn read_pass(&mut self, section: &IniSection, cursors: &MouseCursors) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.attack = cursors.read(section, "Cursor.Attack", self.attack);
        self.attack_out_of_range =
            cursors.read(section, "Cursor.AttackOutOfRange", self.attack_out_of_range);
    }
}

/// SWTypeExt `Cursor` and `NoCursor` (+0x1EC/+0x1F0, `LoadFromINIFile @
/// 10033370`): the target cursor over a cell the superweapon may fire at and
/// over one it may not. Constructor `10031460`: Attack and Disallowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SuperWeaponCursors {
    pub cursor: i32,
    pub no_cursor: i32,
}

impl Default for SuperWeaponCursors {
    fn default() -> Self {
        Self {
            cursor: index::ATTACK,
            no_cursor: index::DISALLOWED,
        }
    }
}

impl SuperWeaponCursors {
    /// The handler's Initialize on the type's first read, by its `Type=`:
    /// SW_ChronoSphere `1006EF10`, SW_ChronoWarp `100700B0`,
    /// SW_PsychicDominator `10070990`, SW_EMPulse `10071440`,
    /// SW_GeneticMutator `10072090`, SW_LightningStorm `10072C50`,
    /// SW_NuclearMissile `10073360`, SW_ParaDrop `10073DC0`, SW_Protect
    /// `10074B60` (ForceShield cursors only for the stock ForceShield type),
    /// SW_Reveal `100752F0` and SW_SpyPlane `10075800`. The other handlers
    /// keep the constructor's cursors.
    pub(crate) fn initialize_for_kind(&mut self, kind: Option<&str>) {
        let is = |name: &str| kind.is_some_and(|kind| kind.eq_ignore_ascii_case(name));
        if is("ForceShield") {
            self.cursor = index::FORCE_SHIELD;
            self.no_cursor = index::NO_FORCE_SHIELD;
        } else if is("EMPulse") {
            self.cursor = index::ATTACK;
            self.no_cursor = index::ATTACK_OUT_OF_RANGE;
        } else if let Some(cursor) = [
            ("ChronoSphere", index::CHRONOSPHERE),
            ("ChronoWarp", index::CHRONOSPHERE),
            ("PsychicDominator", index::PSYCHIC_DOMINATOR),
            ("GeneticConverter", index::GENETIC_MUTATOR),
            ("LightningStorm", index::LIGHTNING_STORM),
            ("MultiMissile", index::NUKE),
            ("ParaDrop", index::PARA_DROP),
            ("AmerParaDrop", index::PARA_DROP),
            ("IronCurtain", index::IRON_CURTAIN),
            ("Protect", index::IRON_CURTAIN),
            ("PsychicReveal", index::PSYCHIC_REVEAL),
            ("SpyPlane", index::SPY_PLANE),
        ]
        .into_iter()
        .find_map(|(name, cursor)| is(name).then_some(cursor))
        {
            self.cursor = cursor;
        }
    }

    pub(crate) fn read_pass(&mut self, section: &IniSection, cursors: &MouseCursors) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.cursor = cursors.read(section, "Cursor", self.cursor);
        self.no_cursor = cursors.read(section, "NoCursor", self.no_cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(text: &str) -> IniFile {
        let _ares = crate::rules::ares_switch::ares_reads();
        IniFile::from_str(text)
    }

    /// The defaults reproduce the compiled table, with the six rows
    /// `0x52BA78` animates and the nine Ares cursors after it.
    #[test]
    fn defaults_copy_the_compiled_table_and_append_the_ares_cursors() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/mouse_oracle/cursor_table.json",
        ))
        .unwrap();
        let cursors = MouseCursors::default();
        for record in native["rows"].as_array().unwrap() {
            let row = record["row"].as_i64().unwrap() as i32;
            let field = |key: &str| record[key].as_i64().map_or(-1, |value| value as i32);
            let interval = if ANIMATED_BY_ARES.contains(&row) {
                4
            } else {
                field("rate")
            };
            assert_eq!(
                *cursors.get(row).unwrap(),
                c(
                    field("frame"),
                    field("count"),
                    interval,
                    field("mini_frame"),
                    field("mini_count"),
                    field("hotspot_x"),
                    field("hotspot_y"),
                ),
                "row {row}"
            );
        }
        assert_eq!(cursors.find("TakeVehicle"), Some(index::TAKE_VEHICLE));
        assert_eq!(cursors.find("repairtrench"), Some(index::REPAIR_TRENCH));
        // EngineerDamage copies Detonate after it was animated.
        assert_eq!(
            cursors.get(index::ENGINEER_DAMAGE),
            Some(&c(299, 10, 4, -1, -1, C, C))
        );
        assert_eq!(
            cursors.get(index::INFANTRY_HEAL),
            Some(&c(0x163, 1, 0, -1, -1, C, C))
        );
        assert_eq!(cursors.get(index::REPAIR_TRENCH + 1), None);
    }

    /// New keys append entries; each entry, in list order, copies a named
    /// entry's current cursor or reads tokens. The token list skips empty
    /// tokens, keeps a field whose token does not parse and matches hotspot
    /// names exactly.
    #[test]
    fn mouse_cursors_entries_parse_tokens_or_copy_a_named_entry() {
        let mut cursors = MouseCursors::default();
        cursors.read_pass(&pass(
            "[MouseCursors]\n\
             Spin=100,8,2,,x,5,right,bottom\n\
             Copy=Spin\n\
             Move=Spin\n\
             Short=7,3\n\
             Spaced=1,1,0,-1,-1, right,bottom\n",
        ));
        let spin = cursors.find("spin").unwrap();
        assert_eq!(spin, index::REPAIR_TRENCH + 1);
        assert_eq!(cursors.get(spin), Some(&c(100, 8, 2, -1, 5, F, F)));
        // Copy follows Spin in the list and copies its new cursor; Move comes
        // first and copies the one Spin had before this pass read it.
        assert_eq!(
            cursors.get(cursors.find("Copy").unwrap()),
            cursors.get(spin)
        );
        assert_eq!(cursors.get(index::MOVE), Some(&c(0, 1, 0, -1, -1, N, N)));
        let short = cursors.find("Short").unwrap();
        assert_eq!(cursors.get(short), Some(&c(7, 3, 0, -1, -1, N, N)));
        let spaced = cursors.find("Spaced").unwrap();
        assert_eq!(cursors.get(spaced), Some(&c(1, 1, 0, -1, -1, N, F)));

        // A later pass finds the entry (strcmpi) instead of appending a second
        // one, but reads its value under the stored name, an exact-case key.
        cursors.read_pass(&pass("[MouseCursors]\nSPIN=5\nSpin=6\n"));
        assert_eq!(cursors.find("Spaced"), Some(spaced));
        assert_eq!(cursors.get(spin).unwrap().frame, 6);
        cursors.read_pass(&pass("[MouseCursors]\nSPIN=5\n"));
        assert_eq!(cursors.find("SPIN"), Some(spin));
        assert_eq!(cursors.iter().count(), spaced as usize + 1);
        assert_eq!(cursors.get(spin).unwrap().frame, 6);
    }

    /// `1000FB50`: a known name stores its index, `none` stores -1, and an
    /// unknown name or an empty value keeps the current one.
    #[test]
    fn cursor_keys_store_an_entry_index_or_none() {
        let ini = pass(
            "[MouseCursors]\nSpin=1,1\n\
             [HTNK]\nCursor.Move=spin\nCursor.NoMove=none\nCursor.Enter=Unknown\nCursor.Deploy=\n\
             [GAPILE]\nCursor.Spy=Sabotage\n",
        );
        let mut cursors = MouseCursors::default();
        cursors.read_pass(&ini);
        let mut tank = TechnoCursors::default();
        tank.read_techno_pass(ini.section("HTNK").unwrap(), &cursors);
        assert_eq!(tank.move_, cursors.find("Spin").unwrap());
        assert_eq!(tank.no_move, -1);
        assert_eq!(tank.enter, index::ENTER);
        assert_eq!(tank.deploy, index::DEPLOY);
        let mut barracks = TechnoCursors::default();
        barracks.read_building_pass(ini.section("GAPILE").unwrap(), &cursors);
        assert_eq!(barracks.spy, index::SABOTAGE);
    }

    /// The ordered Rules reader allocates `[MouseCursors]` before the type
    /// bodies of the same pass and binds every type's keys; a SpyPlane's
    /// handler Initialize names its cursor.
    #[test]
    fn rules_bind_cursor_keys_to_their_types() {
        let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &pass(
                "[MouseCursors]\nSpin=100,8,2\n\
                 [InfantryTypes]\n0=E1\n[E1]\nPrimary=M60\nCursor.Move=Spin\n\
                 [BuildingTypes]\n0=GAPILE\n[GAPILE]\nCursor.Spy=Sabotage\n\
                 [M60]\nCursor.AttackOutOfRange=Spin\n\
                 [SuperWeaponTypes]\n0=Spy\n[Spy]\nType=SpyPlane\nNoCursor=Spin\n",
            ),
            &IniFile::empty(),
        )
        .unwrap();
        let spin = rules.ares_mouse_cursors().find("Spin").unwrap();
        assert_eq!(rules.object("E1").unwrap().ares_cursors().move_, spin);
        assert_eq!(
            rules.object("GAPILE").unwrap().ares_cursors().spy,
            index::SABOTAGE
        );
        assert_eq!(
            rules.weapon("M60").unwrap().ares_cursors,
            WeaponCursors {
                attack: index::ATTACK,
                attack_out_of_range: spin,
            }
        );
        assert_eq!(
            rules.ares_super_weapon_cursors("Spy"),
            Some(SuperWeaponCursors {
                cursor: index::SPY_PLANE,
                no_cursor: spin,
            })
        );
    }
}
