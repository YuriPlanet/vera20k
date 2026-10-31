//! Phobos `[InsigniaTypes]` catalog.
//!
//! Native identity: Phobos `InsigniaTypeClass`
//! (`src/New/Type/InsigniaTypeClass.{h,cpp}`), an
//! `Enumerable<InsigniaTypeClass>` whose main section is `[InsigniaTypes]`.
//! `InsigniaTypeClass::LoadFromINI` reads `Insignia=` (a
//! `Promotable<SHPStruct*>`, so the common key sets all three ranks and the
//! `.Rookie`/`.Veteran`/`.Elite` keys override one each) and `InsigniaFrame=`
//! (a `Promotable<int>` with the same shape). A techno type referring to an
//! entry through `InsigniaType=` copies these two fields verbatim.
//!
//! Asset loading is deferred: the catalog retains the file names with Phobos'
//! `.shp` suffix rule, and presentation resolves them once per match.

use crate::rules::ini_parser::IniFile;
#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniSection;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::parse_ares_int;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::truncate_native_bytes;

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`.
#[cfg(feature = "phobos")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// `Promotable<T>` rank order: Rookie, Veteran, Elite.
#[cfg(feature = "phobos")]
pub(crate) const RANKS: [&str; 3] = ["Rookie", "Veteran", "Elite"];

/// Phobos `InsigniaTypeClass`: one retained `[InsigniaTypes]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct InsigniaType {
    pub name: String,
    /// `Promotable<SHPStruct*> Insignia`: every applied file name per rank,
    /// oldest first. Phobos keeps the last SHP that loaded, so presentation
    /// tries these newest first and a missing file falls through.
    pub images: [Vec<String>; 3],
    /// `Promotable<int> InsigniaFrame`: per-rank frame index, -1 = default.
    pub frames: [i32; 3],
}

impl InsigniaType {
    #[cfg(feature = "phobos")]
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            images: Default::default(),
            frames: [-1, -1, -1],
        }
    }
}

/// Ordered Phobos `Enumerable<InsigniaTypeClass>::Array`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct InsigniaTypeCatalog {
    types: Vec<InsigniaType>,
}

impl InsigniaTypeCatalog {
    pub fn entry(&self, name: &str) -> Option<&InsigniaType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn entries(&self) -> &[InsigniaType] {
        &self.types
    }

    /// Read one reached Rules pass: the pass's `[InsigniaTypes]` list appends
    /// entries, then every retained entry re-reads its own section.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("InsigniaTypes") {
            // Phobos Enumerable::LoadFromINIList reads each physical key
            // through ReadString with Phobos::readBuffer[2048] before lookup.
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types.push(InsigniaType::named(&truncate_native_bytes(
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
            read_insignia_images(section, &mut entry.images, "Insignia");
            entry.frames = read_insignia_frames(section, entry.frames, "InsigniaFrame");
        }
    }
}

/// Phobos `RulesExt` drawing settings: `[General] EnemyInsignia` /
/// `DisguiseBlinkingVisibility` and the `[AudioVisual] DrawInsignia.*`
/// defaults. `TechnoExt::DrawInsignia` consumes all of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhobosDrawGlobals {
    pub enemy_insignia: bool,
    /// `[General] DisguiseBlinkingVisibility` (`AffectedHouse`, default
    /// `Owner`): who can see through a blinking disguise. DrawInsignia asks
    /// it with the viewer as source and the disguised Techno's owner as
    /// target.
    pub disguise_blinking_visibility: crate::rules::phobos_shields::AffectedHouse,
    pub only_on_selected: bool,
    pub use_pixel_selection_bracket_delta: bool,
    pub adjust_infantry: [i32; 2],
    pub adjust_units: [i32; 2],
    pub adjust_buildings: [i32; 2],
    /// Phobos `Nullable<BuildingSelectBracketPosition>`: unset adds the
    /// building offset to the object's own location instead of anchoring to
    /// the selection bracket.
    pub buildings_anchor:
        Option<crate::rules::building_select_bracket::BuildingSelectBracketPosition>,
}

impl Default for PhobosDrawGlobals {
    /// The `RulesExt` constructor values.
    fn default() -> Self {
        Self {
            enemy_insignia: true,
            disguise_blinking_visibility: crate::rules::phobos_shields::AffectedHouse::OWNER,
            only_on_selected: false,
            use_pixel_selection_bracket_delta: false,
            adjust_infantry: [5, 2],
            adjust_units: [10, 6],
            adjust_buildings: [10, 6],
            buildings_anchor: None,
        }
    }
}

impl PhobosDrawGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut globals = Self::default();
        let general = ini.section_or_empty("General");
        globals.enemy_insignia = general.read_ares_bool("EnemyInsignia", globals.enemy_insignia);
        globals.disguise_blinking_visibility = general.read_string_with(
            "DisguiseBlinkingVisibility",
            0x40,
            globals.disguise_blinking_visibility,
            |current, raw| {
                crate::rules::phobos_shields::AffectedHouse::parse(raw).unwrap_or(current)
            },
        );
        let audio_visual = ini.section_or_empty("AudioVisual");
        globals.only_on_selected =
            audio_visual.read_ares_bool("DrawInsignia.OnlyOnSelected", globals.only_on_selected);
        globals.use_pixel_selection_bracket_delta = audio_visual.read_ares_bool(
            "DrawInsignia.UsePixelSelectionBracketDelta",
            globals.use_pixel_selection_bracket_delta,
        );
        globals.adjust_infantry = audio_visual
            .read_phobos_int_pair("DrawInsignia.AdjustPos.Infantry", globals.adjust_infantry);
        globals.adjust_units =
            audio_visual.read_phobos_int_pair("DrawInsignia.AdjustPos.Units", globals.adjust_units);
        globals.adjust_buildings = audio_visual
            .read_phobos_int_pair("DrawInsignia.AdjustPos.Buildings", globals.adjust_buildings);
        globals.buildings_anchor = audio_visual.read_phobos_nullable_with(
            "DrawInsignia.AdjustPos.BuildingsAnchor",
            globals.buildings_anchor,
            crate::rules::building_select_bracket::BuildingSelectBracketPosition::parse,
        );
        globals
    }
}

/// One `Promotable<SHPStruct*>/Promotable<int>/Vector3D<int>` slot set. The
/// image slot keeps every applied file name oldest-first: a load tries newest
/// first, so a file that fails keeps the earlier pass's loaded art, as
/// Phobos' retained `SHPStruct*` does. A resolved `InsigniaType` replaces the
/// whole slot (a null pointer falls back to stock PIPS, not to older names).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct InsigniaVariant {
    pub images: [Vec<String>; 3],
    pub frames: [i32; 3],
    pub frame_lists: [i32; 3],
}

impl Default for InsigniaVariant {
    fn default() -> Self {
        Self {
            images: [Vec::new(), Vec::new(), Vec::new()],
            frames: [-1, -1, -1],
            frame_lists: [-1, -1, -1],
        }
    }
}

#[cfg(feature = "phobos")]
impl InsigniaVariant {
    /// One Rules pass of the directly authored keys, in Phobos'
    /// `Promotable::Read` order: the common key appends to all three ranks,
    /// then each rank key appends to one.
    fn apply_direct_pass(
        &mut self,
        raw: &IniSection,
        image_base: &str,
        frame_base: &str,
        list_base: &str,
    ) {
        read_insignia_images(raw, &mut self.images, image_base);
        self.frames = read_insignia_frames(raw, self.frames, frame_base);
        self.frame_lists = raw.read_int_fields::<3>(list_base, 2048, self.frame_lists);
    }

    /// A resolved `InsigniaType` assigns the entry's Promotable verbatim and
    /// clears `InsigniaFrames`; a missing rank image is a null pointer.
    fn apply_type(&mut self, entry: &InsigniaType) {
        self.images = entry.images.clone();
        self.frames = entry.frames;
        self.frame_lists = [-1, -1, -1];
    }
}

/// Phobos `TechnoTypeExt`'s insignia block, accumulated pass by pass like
/// `TechnoTypeExt::LoadFromINIFile`: a later layer resolves `InsigniaType`
/// first and skips the direct keys, and each direct key read retains what the
/// earlier layers set.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct PhobosInsignia {
    pub base: InsigniaVariant,
    pub weapons: Vec<InsigniaVariant>,
    pub passengers: Vec<InsigniaVariant>,
    /// `Insignia.ShowEnemy=`; `None` inherits `[General] EnemyInsignia`.
    pub show_enemy: Option<bool>,
    /// The type's `Passengers=` at the last pass, sizing the vector.
    #[serde(default)]
    pub passengers_capacity: i32,
}

/// One draw's image candidates: the base, the passenger variant and the
/// weapon variant, in DrawInsignia's override order. The frame fields are
/// already merged, since they do not depend on an asset load.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InsigniaSelection {
    pub layers: Vec<[Vec<String>; 3]>,
    pub frames: [i32; 3],
    pub frame_lists: [i32; 3],
}

impl PhobosInsignia {
    /// Apply one reached Rules pass. `catalog` is the cumulative
    /// `[InsigniaTypes]` state after this pass's list read, as Phobos'
    /// `TechnoTypeExt::LoadFromINIFile` sees it; `weapon_count` and `gunner`
    /// are the type's `WeaponCount=` and `Gunner=` after the native reads on
    /// this pass.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(
        &mut self,
        raw: &IniSection,
        catalog: Option<&InsigniaTypeCatalog>,
        weapon_count: i32,
        gunner: bool,
    ) {
        self.show_enemy = raw.read_phobos_nullable_bool("Insignia.ShowEnemy", self.show_enemy);
        // Nullable<InsigniaTypeClass*>::Read: a blank value resets, and any
        // other value is an `Enumerable::Find` (plain case-insensitive name
        // match, so an entry actually named `none` still resolves). Anything
        // unresolved falls back to the direct keys this pass.
        let type_name = |key: &str| {
            raw.read_name(key, 2048)
                .and_then(|name| catalog.and_then(|catalog| catalog.entry(name)))
        };
        match type_name("InsigniaType") {
            Some(entry) => self.base.apply_type(entry),
            None => self
                .base
                .apply_direct_pass(raw, "Insignia", "InsigniaFrame", "InsigniaFrames"),
        }
        if gunner {
            let count = weapon_count.max(0) as usize;
            self.weapons.resize(count, InsigniaVariant::default());
            for index in 0..count {
                let number = index + 1;
                match type_name(&format!("InsigniaType.Weapon{number}")) {
                    Some(entry) => self.weapons[index].apply_type(entry),
                    None => self.weapons[index].apply_direct_pass(
                        raw,
                        &format!("Insignia.Weapon{number}"),
                        &format!("InsigniaFrame.Weapon{number}"),
                        &format!("InsigniaFrames.Weapon{number}"),
                    ),
                }
            }
        }
        self.passengers_capacity = raw.read_int("Passengers", self.passengers_capacity);
        if self.passengers_capacity > 0 {
            let count = self.passengers_capacity as usize + 1;
            self.passengers.resize(count, InsigniaVariant::default());
            for slot in 0..count {
                match type_name(&format!("InsigniaType.Passengers{slot}")) {
                    Some(entry) => self.passengers[slot].apply_type(entry),
                    None => self.passengers[slot].apply_direct_pass(
                        raw,
                        &format!("Insignia.Passengers{slot}"),
                        &format!("InsigniaFrame.Passengers{slot}"),
                        &format!("InsigniaFrames.Passengers{slot}"),
                    ),
                }
            }
        }
    }

    /// The draw's variants in override order. A null shape on a later variant
    /// leaves the previous shape in place, so the caller resolves each
    /// layer's images against the loaded strips in this order.
    pub fn select(&self, weapon: Option<usize>, passengers: Option<usize>) -> InsigniaSelection {
        let mut selection = InsigniaSelection {
            layers: vec![self.base.images.clone()],
            frames: self.base.frames,
            frame_lists: self.base.frame_lists,
        };
        if let Some(variant) = passengers.and_then(|index| self.passengers.get(index)) {
            selection.merge(variant);
        }
        if let Some(variant) = weapon.and_then(|index| self.weapons.get(index)) {
            selection.merge(variant);
        }
        selection
    }
}

impl InsigniaSelection {
    fn merge(&mut self, variant: &InsigniaVariant) {
        self.layers.push(variant.images.clone());
        for rank in 0..3 {
            if variant.frames[rank] != -1 {
                self.frames[rank] = variant.frames[rank];
            }
        }
        if variant.frame_lists != [-1, -1, -1] {
            self.frame_lists = variant.frame_lists;
        }
    }

    /// The frame `DrawInsignia` finally draws for `rank`: the promotable
    /// single frame, else that rank's `InsigniaFrames` entry, else the
    /// custom-shape default 0 or the stock PIPS frame (14 Veteran, 15 Elite;
    /// Rookie draws nothing without an authored frame). `None` means no draw.
    pub fn frame(&self, rank: usize, custom: bool) -> Option<i32> {
        let mut frame = self.frames[rank];
        let mut default_frame = if custom { 0 } else { -1 };
        match rank {
            1 => {
                if !custom {
                    default_frame = 14;
                }
                if frame == -1 {
                    frame = self.frame_lists[1];
                }
            }
            2 => {
                if !custom {
                    default_frame = 15;
                }
                if frame == -1 {
                    frame = self.frame_lists[2];
                }
            }
            _ => {
                if frame == -1 {
                    frame = self.frame_lists[0];
                }
            }
        }
        if frame == -1 {
            frame = default_frame;
        }
        (frame >= 0).then_some(frame)
    }
}

/// Phobos `Promotable<SHPStruct*>::Read` with the base flag `base` (`%s` is
/// the rank): the common key applies to all three ranks, then each
/// `.Rookie`/`.Veteran`/`.Elite` key to one. The rank reader assigns its
/// pointed SHP only when the file loads, so each applied name is appended and
/// presentation falls back newest-to-oldest. A name without a `.shp` suffix
/// gains one, as the Phobos SHP reader does; a literal `none` is an ordinary
/// file name (`none.shp`) there.
#[cfg(feature = "phobos")]
pub(crate) fn read_insignia_images(
    section: &IniSection,
    current: &mut [Vec<String>; 3],
    base: &str,
) {
    if let Some(all) = section.read_name(base, 2048) {
        let name = normalize_insignia_image(all);
        for history in current.iter_mut() {
            history.push(name.clone());
        }
    }
    for (slot, rank) in RANKS.iter().enumerate() {
        if let Some(value) = section.read_name(&format!("{base}.{rank}"), 2048) {
            current[slot].push(normalize_insignia_image(value));
        }
    }
}

/// Phobos `Promotable<int>::Read`: the common key's successful parse sets all
/// three ranks, then the rank keys replace one; a rejected value keeps its
/// slot.
#[cfg(feature = "phobos")]
pub(crate) fn read_insignia_frames(
    section: &IniSection,
    mut current: [i32; 3],
    base: &str,
) -> [i32; 3] {
    if let Some(value) = read_int_key(section, base) {
        current = [value; 3];
    }
    for (slot, rank) in RANKS.iter().enumerate() {
        if let Some(value) = read_int_key(section, &format!("{base}.{rank}")) {
            current[slot] = value;
        }
    }
    current
}

/// Phobos' SHP name rule: append `.shp` unless the name already ends with it,
/// case-insensitively (`TemplateDef.h`'s `read<SHPStruct*>`).
#[cfg(feature = "phobos")]
pub(crate) fn normalize_insignia_image(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".shp") {
        name.to_owned()
    } else {
        format!("{name}.shp")
    }
}

#[cfg(feature = "phobos")]
fn read_int_key(section: &IniSection, key: &str) -> Option<i32> {
    section.read_name(key, 2048).and_then(parse_ares_int)
}

#[cfg(all(test, feature = "phobos"))]
#[path = "insignia_type_tests.rs"]
mod tests;
