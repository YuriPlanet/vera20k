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
    /// comparison; Snow is ordinal1. Unknown names return-1.
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

    /// Full_Init687633..687649 invokes ReadTheater475870 with default0
    /// before resetting Types. This reads the raw map, not the merged Rules.
    pub(crate) fn from_map(map: &IniFile) -> Self {
        let name = map.section_or_empty("Map").read_string("Theater", "", 0x80);
        if name.is_empty() {
            Self::Temperate
        } else {
            Self::from_name(&name)
        }
    }
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
}

impl ObjectImageState {
    /// ObjectTypeConstructor5F7090/5F7148/5F714E: ImageFile starts with
    /// stored ID; the alternate-art input and applied latch both start false.
    pub(crate) fn new(stored_id: &str) -> Self {
        Self {
            image: stored_id.chars().take(24).collect(),
            alternate_arctic_art: false,
            arctic_applied: false,
            theater: false,
            new_theater: false,
            voxel: false,
            shp_loads: Vec::new(),
            anim_bounds_loads: None,
            anim_theater_bounds_loads: None,
            voxel_load: None,
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
        let art = fixed_art.section(&self.image);
        self.read_art(art);
        self.load_for_read(kind, theater);
        art
    }

    pub(crate) fn read_input(&mut self, input: &IniSection) {
        self.image = input.read_string("Image", &self.image, 25);
        self.alternate_arctic_art =
            input.read_bool("AlternateArcticArt", self.alternate_arctic_art);
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

    pub(crate) fn load_for_read(&mut self, kind: ObjectImageKind, theater: ImageTheater) {
        if !self.voxel && kind != ObjectImageKind::Animation {
            self.load_unowned_shp(kind, theater);
        }
    }

    /// ObjectTypeLoadSHP5F9070..5F9119 mutates ImageFile once while snow,
    /// alternate art and an unowned entry shape admit it. Ordinary cached
    /// loads5F924E/5F926E never set ownership+A8; ObjectLoad5F985C clears it
    /// before every saved-family call here. This is the proven unowned
    /// read/saved boundary, not the lazy owned-shape lifecycle. A miss never
    /// undoes the name or latch.
    pub(crate) fn load_unowned_shp(&mut self, kind: ObjectImageKind, theater: ImageTheater) {
        if self.alternate_arctic_art && theater == ImageTheater::Snow {
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
            image: self.image.clone(),
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
            self.load_unowned_shp(ObjectImageKind::Animation, theater);
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
            image: self.image.clone(),
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
        self.voxel_load = Some(self.image.clone());
    }

    /// ObjectLoad5F9856..5F985C clears the saved resource pointer/ownership.
    pub(crate) fn clear_shp(&mut self) {
        self.shp_loads.clear();
    }

    pub(crate) fn replace_image(&mut self, input: &IniSection, default: &str) {
        self.image = input.read_string("Image", default, 25);
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
