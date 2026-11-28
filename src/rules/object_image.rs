//! Retained ObjectType image fields and reached image-load attempts.
//!
//! ProcessedType owns this state. Resource catalogs resolve the attempts;
//! constructing or projecting a RuleSet cannot execute another native read.
//! This covers the proven unowned Rules-read and saved-load boundary. Lazy
//! owned-shape transitions remain with their resource lifecycle callers.

use crate::rules::ini_parser::{IniFile, IniSection};

/// Scenario's theater at a Rules/resource phase, not another scenario owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageTheater {
    Uninitialized,
    Temperate,
    Snow,
    Urban,
    Desert,
    NewUrban,
    Lunar,
}

impl ImageTheater {
    /// FindTheater48DBE0 scans the six names at7E1B78 with case-insensitive
    /// comparison; Snow is ordinal1. Unknown names return-1. The map's
    /// theater data finds its theater through this too (`map::theater`).
    pub(crate) fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("SNOW") {
            Self::Snow
        } else if name.eq_ignore_ascii_case("TEMPERATE") {
            Self::Temperate
        } else {
            [
                ("URBAN", Self::Urban),
                ("DESERT", Self::Desert),
                ("NEWURBAN", Self::NewUrban),
                ("LUNAR", Self::Lunar),
            ]
            .into_iter()
            .find(|(candidate, _)| name.eq_ignore_ascii_case(candidate))
            .map_or(Self::Uninitialized, |(_, theater)| theater)
        }
    }

    /// The TheaterType ordinal FindTheater48DBE0 returns: Temperate0 to
    /// Lunar5, and -1 for an unknown name (Ares `Theater::FindIndex
    /// @ 1003DAE0` and `ScenarioClass+Theater`).
    pub(crate) const fn native_index(self) -> i32 {
        match self {
            Self::Uninitialized => -1,
            Self::Temperate => 0,
            Self::Snow => 1,
            Self::Urban => 2,
            Self::Desert => 3,
            Self::NewUrban => 4,
            Self::Lunar => 5,
        }
    }

    /// The theater's `TheaterData.Letter` string (`0x007E1BCE`, one per
    /// 0x70-byte entry from `0x007E1B78`): one letter, T A U D N L. None for
    /// an unknown theater, whose entry gamemd would read before the table.
    pub(crate) const fn letter(self) -> Option<char> {
        match self {
            Self::Uninitialized => None,
            Self::Temperate => Some('T'),
            Self::Snow => Some('A'),
            Self::Urban => Some('U'),
            Self::Desert => Some('D'),
            Self::NewUrban => Some('N'),
            Self::Lunar => Some('L'),
        }
    }

    /// Full_Init687633..687649 invokes ReadTheater475870 with default0
    /// before resetting Types. This reads the raw map, not the merged Rules,
    /// with the map header's read ([`read_map_theater`]).
    pub(crate) fn from_map(map: &IniFile) -> Self {
        Self::from_name(&read_map_theater(map))
    }
}

/// The map's `[Map] Theater=`: `ReadTheater @ 0x00475870` with default 0
/// (Temperate), as the map reader (0x004ACFE5) and Full_Init (0x0068763E)
/// both call it. The name its 0x80-byte ReadString copies, uppercased, or
/// `TEMPERATE` when the copy is empty. An unknown name stays as read; the
/// theater lookups match names without regard to case, as FindTheater
/// (0x0048DBE0) does. Rules reads it for the image theater, so it lives here
/// rather than in the map reader (rules must not import map).
pub(crate) fn read_map_theater(ini: &IniFile) -> String {
    ini.section_or_empty("Map")
        .read_name("Theater", 0x80)
        .unwrap_or("TEMPERATE")
        .to_uppercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectImageKind {
    Ordinary,
    Animation,
    Terrain,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnimImagePhase {
    ReadIni,
    Theater,
}

/// Inputs of one reached SHP load in a first-success request plan.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ObjectImageLoad {
    pub image: String,
    pub theater: bool,
    pub new_theater: bool,
    /// Generic LoadSHP retries G; direct Image.SHP readers load once.
    pub(crate) generic_retry: bool,
}

impl ObjectImageLoad {
    /// ObjectTypeLoadSHP5F9119..5F91DD chooses the extension, then applies
    /// NewTheater only without Theater. Failed loads5F9258/5F9276 retry with
    /// the second filename character forced to G, even if it is unchanged.
    /// Direct TechnoRead715820 and TerrainRead71DFC0 loads have no retry.
    pub fn shp_names(&self, theater_ext: &str, theater_name: &str) -> Vec<String> {
        let upper_image = self.image.to_ascii_uppercase();
        let extension = if self.theater {
            theater_ext.to_ascii_uppercase()
        } else {
            "SHP".to_string()
        };
        let primary = format!("{upper_image}.{extension}");
        let [substituted, generic_retry] =
            crate::rules::art_data::theater_shp_names(&primary, theater_name);
        let first = if !self.theater && self.new_theater {
            substituted
        } else {
            primary
        };
        if self.generic_retry {
            vec![first, generic_retry]
        } else {
            vec![first]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObjectImageState {
    image: String,
    alternate_arctic_art: bool,
    /// Ares TechnoTypeExt `AlternateTheaterArt` (+0x17A, Valueable<bool>,
    /// constructor false); only a TechnoType read the Ares way sets it.
    alternate_theater_art: bool,
    arctic_applied: bool,
    theater: bool,
    new_theater: bool,
    voxel: bool,
    /// Reached loads in first-success order. Unconditional loads replace
    /// this plan; a native Shape==null suffix appends to it.
    shp_loads: Vec<ObjectImageLoad>,
    /// Anim resolver427B50 writes numeric metadata before InitTheater427940
    /// can replace Shape without changing those numbers.
    anim_bounds_loads: Option<Vec<ObjectImageLoad>>,
    anim_theater_bounds_loads: Option<Vec<ObjectImageLoad>>,
    voxel_load: Option<String>,
    /// Phobos `[General] ArtImageSwap=true`: the own ART section's `Image=`
    /// names this TechnoType's visual files. It replaces only the file name;
    /// the section's flags still come from the type's own ART section.
    #[serde(default)]
    image_swap: Option<String>,
    #[serde(default)]
    pub(crate) model_slots: super::voxel_model_plan::ModelSlots,
}

impl ObjectImageState {
    /// ObjectTypeConstructor5F7090/5F7148/5F714E: ImageFile starts with
    /// stored ID; the alternate-art input and applied latch both start false.
    pub(crate) fn new(stored_id: &str) -> Self {
        Self {
            image: stored_id.chars().take(24).collect(),
            alternate_arctic_art: false,
            alternate_theater_art: false,
            arctic_applied: false,
            theater: false,
            new_theater: false,
            voxel: false,
            shp_loads: Vec::new(),
            anim_bounds_loads: None,
            anim_theater_bounds_loads: None,
            voxel_load: None,
            image_swap: None,
            model_slots: Default::default(),
        }
    }

    /// ObjectTypeReadINI5F933B/5F9460/5F95F4..5F964D. ART retains the
    /// pre-load section: its pointer cache can survive mutation of ImageFile.
    pub(crate) fn read_ini<'a>(
        &mut self,
        input: &IniSection,
        fixed_art: &'a IniFile,
        kind: ObjectImageKind,
        theater: ImageTheater,
    ) -> Option<&'a IniSection> {
        self.read_input(input);
        // A section of only VERA's drawing keys is none to the simulation.
        let art = crate::rules::art_data::sim_art_section(fixed_art, &self.image);
        self.read_art(art);
        self.load_for_read(kind, theater, Some(fixed_art));
        art
    }

    pub(crate) fn read_input(&mut self, input: &IniSection) {
        self.image = input.read_string("Image", &self.image, 25);
        self.image_swap = None;
        self.alternate_arctic_art =
            input.read_bool("AlternateArcticArt", self.alternate_arctic_art);
    }

    /// Ares ObjectTypeClass_LoadFromINI (hook `0x005F9634`, body `100413B0`):
    /// a TechnoType reads `AlternateTheaterArt` from the INI being read, in
    /// its ID's section, before ReadINI calls LoadSHP (`0x005F964D`).
    pub(crate) fn read_ares_alternate_theater_art(&mut self, input: &IniSection) {
        self.alternate_theater_art =
            input.read_ares_bool("AlternateTheaterArt", self.alternate_theater_art);
    }

    pub(crate) fn read_art(&mut self, art: Option<&IniSection>) {
        if let Some(art) = art {
            (self.theater, self.new_theater) = crate::rules::object_type::read_object_theater_flags(
                art,
                (self.theater, self.new_theater),
            );
            self.voxel = art.read_bool("Voxel", self.voxel);
        }
    }

    /// Phobos `[General] ArtImageSwap=true`: read the own ART section's
    /// `Image=` as the TechnoType's visual file name. Phobos reads no other
    /// key from the named section, so the retained flags stay those this
    /// section already read. Native art `Image=` remains building-only
    /// (`0x0045F945`); this path runs only while the Phobos key is on.
    pub(crate) fn apply_phobos_art_image_swap(&mut self, fixed_art: &IniFile) {
        let Some(section) = crate::rules::art_data::sim_art_section(fixed_art, &self.image) else {
            return;
        };
        if let Some(image) = section.read_name("Image", 0x40) {
            // The visual name still feeds ObjectTypeClass's 25-byte ImageFile;
            // keep the same 24-character ceiling a rules `Image=` gets.
            self.image_swap = Some(image.chars().take(24).collect());
        }
    }

    /// The name this object's SHP or VXL files load from: the Phobos
    /// ArtImageSwap name when on, otherwise the retained rules `Image=`.
    pub(crate) fn visual_image(&self) -> &str {
        self.image_swap.as_deref().unwrap_or(&self.image)
    }

    pub(crate) fn load_for_read(
        &mut self,
        kind: ObjectImageKind,
        theater: ImageTheater,
        art: Option<&IniFile>,
    ) {
        if !self.voxel && kind != ObjectImageKind::Animation {
            self.load_unowned_shp(kind, theater, art);
        }
    }

    /// ObjectTypeLoadSHP5F9070..5F9119 mutates ImageFile once while snow,
    /// alternate art and an unowned entry shape admit it. Ordinary cached
    /// loads5F924E/5F926E never set ownership+A8; ObjectLoad5F985C clears it
    /// before every saved-family call here. This is the proven unowned
    /// read/saved boundary, not the lazy owned-shape lifecycle. A miss never
    /// undoes the name or latch.
    ///
    /// Ares ObjectTypeClass_Load2DArt (hook `0x005F9070`, body `10041040`,
    /// resumes `0x005F92C3`) puts its own arm first: a TechnoType with
    /// `AlternateTheaterArt` takes ImageFile plus the theater letter once,
    /// and only when the art INI (`art`) holds a section of that name. That
    /// arm skips the Arctic one, its latch reset included. The rest of the
    /// hook is the stock load below. A saved-game restore passes no art: its
    /// latch and image were settled against the same art and theater.
    pub(crate) fn load_unowned_shp(
        &mut self,
        kind: ObjectImageKind,
        theater: ImageTheater,
        art: Option<&IniFile>,
    ) {
        if self.alternate_theater_art {
            if let (false, Some(letter), Some(art)) = (self.arctic_applied, theater.letter(), art) {
                // 10041098 formats into a 0x20-byte buffer and copies at most
                // 24 characters back into the 25-byte ImageFile.
                let name = format!("{}{letter}", self.image);
                if crate::rules::art_data::sim_art_section(art, &name).is_some() {
                    self.image = name.chars().take(24).collect();
                    self.arctic_applied = true;
                }
            }
        } else if self.alternate_arctic_art && theater == ImageTheater::Snow {
            if !self.arctic_applied {
                self.image = self.image.chars().chain(['A']).take(24).collect();
                self.arctic_applied = true;
            }
        } else {
            self.arctic_applied = false;
        }
        // 5F9203/5F920D clears the previous shape; among these callers,
        // Terrain's RTTI37 skips loading. Other callers try this ImageFile.
        self.shp_loads.clear();
        if kind != ObjectImageKind::Terrain {
            self.shp_loads.push(self.shp_request(true));
        }
    }

    /// TechnoRead715820/715825 and TerrainRead71DF9E..71DFC0 load
    /// Image.SHP once, without the alternate-art prefix or generic retry.
    /// A miss replaces the previous shape with null.
    pub(crate) fn load_plain_shp(&mut self) {
        self.shp_loads = vec![ObjectImageLoad {
            image: self.visual_image().to_owned(),
            theater: false,
            new_theater: false,
            generic_retry: false,
        }];
    }

    /// AnimTypeClass::LoadImageAndResolveFrameBounds427B50 calls generic
    /// LoadSHP only for Theater. Its other arm427B9F..427C0C formats the
    /// Image/ID name with the shared theater/G retry, without Arctic mutation.
    pub(crate) fn load_anim_shp(
        &mut self,
        stored_id: &str,
        theater: ImageTheater,
        phase: AnimImagePhase,
    ) {
        if self.theater {
            self.load_unowned_shp(ObjectImageKind::Animation, theater, None);
        } else {
            let mut request = self.shp_request(true);
            if request.image.is_empty() {
                request.image = stored_id.to_owned();
            }
            // ReadINI427DD9 passes-1 to427BDE, so no theater letter is
            // substituted there. InitTheater427A1B passes the real theater.
            request.new_theater = phase == AnimImagePhase::Theater;
            self.shp_loads = vec![request];
        }
        match phase {
            AnimImagePhase::ReadIni => {
                self.anim_bounds_loads = Some(self.shp_loads.clone());
                self.anim_theater_bounds_loads = None;
            }
            AnimImagePhase::Theater => {
                self.anim_theater_bounds_loads = Some(self.shp_loads.clone())
            }
        }
    }

    /// AnimTypeLoad4288E5..428929 replaces the generic image with the
    /// stored type ID and theater extension, without a G retry.
    pub(crate) fn load_saved_anim_theater_shp(&mut self, stored_id: &str) {
        self.shp_loads = vec![ObjectImageLoad {
            image: stored_id.to_owned(),
            theater: true,
            new_theater: false,
            generic_retry: false,
        }];
    }

    /// Read_Map4AD092 and MouseLoad5BE689 call the same AnimType array
    /// InitTheater427940 after type reads/loads. Its Theater arm4279A8..427A09
    /// replaces only Shape: stored ID + extension, then ID.SHP. It does not
    /// repeat the earlier427B50 numeric writes. Deferred/owned shapes remain
    /// the separate lifecycle boundary described above.
    pub(crate) fn init_anim_theater(&mut self, stored_id: &str, theater: ImageTheater) {
        if self.theater {
            self.load_saved_anim_theater_shp(stored_id);
            self.shp_loads.push(ObjectImageLoad {
                image: stored_id.to_owned(),
                theater: false,
                new_theater: false,
                generic_retry: false,
            });
        } else if self.new_theater {
            self.load_anim_shp(stored_id, theater, AnimImagePhase::Theater);
        }
    }

    fn shp_request(&self, generic_retry: bool) -> ObjectImageLoad {
        ObjectImageLoad {
            image: self.visual_image().to_owned(),
            theater: self.theater,
            new_theater: self.new_theater,
            generic_retry,
        }
    }

    /// OverlayType InitTheater5FE645..5FE719 replaces Shape for Theater
    /// or NewTheater, without replaying LoadSHP's Arctic prefix. The normal
    /// constructor's deferred+2AF is false; lazy owned shapes remain a separate
    /// resource lifecycle boundary, not inferred from an available file.
    pub(crate) fn init_overlay_theater(&mut self) {
        if self.theater || self.new_theater {
            self.shp_loads = vec![self.shp_request(true)];
        }
    }

    /// OverlayTypeLoad5FEB69..5FEBF1 loads this single suffix only while
    /// the preceding generic SHP attempts left Shape null. Non-theater art
    /// applies the theater letter even without NewTheater; it never retries G.
    pub(crate) fn append_saved_overlay_shp(&mut self) {
        let mut suffix = self.shp_request(false);
        suffix.new_theater = !suffix.theater;
        self.shp_loads.push(suffix);
    }

    /// LoadVoxel5F8110 formats the entry ImageFile independently of Voxel.
    pub(crate) fn load_voxel(&mut self) {
        self.voxel_load = Some(self.visual_image().to_owned());
    }

    /// ObjectLoad5F9856..5F985C clears the saved resource pointer/ownership.
    pub(crate) fn clear_shp(&mut self) {
        self.shp_loads.clear();
    }

    pub(crate) fn replace_image(&mut self, input: &IniSection, default: &str) {
        self.image = input.read_string("Image", default, 25);
        self.image_swap = None;
    }

    pub(crate) fn read_anim_theater_flags(&mut self, own_art: &IniSection) {
        if !self.image.is_empty() {
            (self.theater, self.new_theater) = crate::rules::object_type::read_object_theater_flags(
                own_art,
                (self.theater, self.new_theater),
            );
        }
    }

    /// ObjectType's Image25 read5F933B and arctic copy5F90DF..5F90EF keep
    /// at most24 source characters plus the terminating NUL. Load descriptors
    /// retain prior contents of that same buffer. Later reads can validly
    /// change Image without changing a prior descriptor or the arctic latch.
    pub(crate) fn validate_saved_state(&self) -> Result<(), crate::rules::error::RulesError> {
        let valid_name = |name: &str| name.chars().count() <= 24 && !name.contains('\0');
        if !valid_name(&self.image)
            || self.shp_loads.iter().any(|load| !valid_name(&load.image))
            || self.shp_loads.len() > 2
            || self.anim_bounds_loads.as_ref().is_some_and(|loads| {
                loads.len() > 2 || loads.iter().any(|load| !valid_name(&load.image))
            })
            || self
                .anim_theater_bounds_loads
                .as_ref()
                .is_some_and(|loads| {
                    loads.len() > 2 || loads.iter().any(|load| !valid_name(&load.image))
                })
            || self
                .voxel_load
                .as_deref()
                .is_some_and(|name| !valid_name(name))
            || self
                .image_swap
                .as_deref()
                .is_some_and(|name| !valid_name(name))
        {
            return Err(crate::rules::error::RulesError::InvalidSavedState(
                "invalid retained ObjectType image name",
            ));
        }
        Ok(())
    }

    pub(crate) fn image(&self) -> &str {
        &self.image
    }
    pub(crate) fn theater(&self) -> bool {
        self.theater
    }
    pub(crate) fn new_theater(&self) -> bool {
        self.new_theater
    }
    pub(crate) fn voxel(&self) -> bool {
        self.voxel
    }
    pub(crate) fn shp_loads(&self) -> &[ObjectImageLoad] {
        &self.shp_loads
    }
    pub(crate) fn anim_bounds_loads(&self) -> Option<&[ObjectImageLoad]> {
        self.anim_bounds_loads.as_deref()
    }
    pub(crate) fn anim_theater_bounds_loads(&self) -> Option<&[ObjectImageLoad]> {
        self.anim_theater_bounds_loads.as_deref()
    }
    pub(crate) fn voxel_load(&self) -> Option<&str> {
        self.voxel_load.as_deref()
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;

    /// Phobos `[General] ArtImageSwap=true` names the visual from the own
    /// ART `Image=`, while the image that names other ART reads and the
    /// retained `Voxel=` flag stay on the own section.
    #[test]
    fn phobos_art_image_swap_renames_visual_only() {
        let art = IniFile::from_str("[SAPCX]\nImage=SAPC\nVoxel=yes\nRemapable=yes\n");
        let section = art.section("SAPCX").expect("own ART section");
        let mut image = ObjectImageState::new("SAPCX");
        image.read_art(Some(section));
        image.apply_phobos_art_image_swap(&art);
        assert_eq!(image.image(), "SAPCX");
        assert_eq!(image.visual_image(), "SAPC");
        assert!(image.voxel());
        image.load_voxel();
        assert_eq!(image.voxel_load(), Some("SAPC"));
    }
}
