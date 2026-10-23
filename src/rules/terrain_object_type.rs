//! TerrainTypeClass data read from layered RULESMD sections.
//!
//! Distinct from `terrain_rules` (which parses LAND types like Clear/Rough/Water).
//! These are per-object-type definitions for terrain decorations — currently
//! all terrain objects contribute their theater-selected occupation byte to
//! resolved terrain. Animation fields drive retained Terrain AI, including
//! the TIBTRE (Tiberium Tree) midpoint's forced ore spread.

use crate::rules::foundation;
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF32Bits;

/// Type-class data for a terrain object (e.g. `[TIBTRE01]`).
///
/// Fields consumed by terrain occupation, damage and animation. Other terrain
/// mechanisms retain their existing gaps; this is not the complete native type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainObjectType {
    /// Section name, e.g. "TIBTRE01".
    pub name: String,
    /// ObjectType ImageFile+1F8 and Theater+22C, retained by the process reader.
    image_file: String,
    theater: bool,
    /// Inherited `ObjectTypeClass::ReadINI @ 0x005F92D0` AlphaImage buffer
    /// (`+0x213`, `char[25]`), separate from TerrainType art metadata.
    pub(crate) alpha_image: String,
    /// Inherited IsLogic, initialized by TerrainType ctor71DB1E and cleared
    /// by the reached IsVeinhole post-read71DF05. It is not an INI key.
    is_logic: bool,
    /// `SpawnsTiberium=yes` — periodically spawns ore in adjacent cells.
    pub spawns_tiberium: bool,
    /// `IsAnimated=yes` — required gate for SpawnsTiberium logic.
    pub is_animated: bool,
    /// Signed logic frames per animation step; native ReadInt stores without clamps.
    pub animation_rate: i32,
    /// Native ReadDouble/FSTP-float result, preserved without micros quantization.
    pub animation_probability: NativeF32Bits,
    /// Inherited `Armor=`. TerrainTypeClass constructor defaults to Wood.
    /// Kept as authored text for diagnostics; gameplay uses `armor_id()`.
    pub armor: String,
    armor_id: crate::rules::armor::ArmorId,
    /// Inherited `Strength=`. The constructor value and explicit -1 resolve
    /// through `[General] TreeStrength` after successful ObjectType ReadINI.
    pub strength: i32,
    /// Inherited `Immune=`. Stock TIBTRE sets this, blocking normal terrain damage.
    pub immune: bool,
    /// Inherited `LegalTarget=`. `IsVeinhole=yes` force-enables it in gamemd.
    pub legal_target: bool,
    /// `IsFlammable`: `TerrainTypeClass::ReadINI @ 0x0071DEA0` passes the
    /// current `+0x2B2` byte to `CCINIClass::ReadBool @ 0x005295F0` at
    /// `0x0071DF56`; `TerrainTypeClass::Constructor @ 0x0071DA80` defaults it
    /// to false.
    pub is_flammable: bool,
    /// Inherited `Bombable=` (`+0x22E`): ObjectTypeClass::ReadINI's
    /// ReadBool over the constructor's 1 (`0x005F9417..0x005F9431`). No
    /// retail TerrainType sets it, so a Crazy Ivan may bomb every one.
    pub bombable: bool,
    /// Inherited `Insignificant=`. TerrainTypeClass constructor defaults true.
    pub insignificant: bool,
    /// Inherited `RadarInvisible=`. TerrainTypeClass constructor defaults true.
    pub radar_invisible: bool,
    /// TerrainTypeClass constructor `0x0071DA80` clears +0x29C..+0x29E.
    /// Each successful TerrainTypeClass::ReadINI pass `0x0071DEA0` replaces
    /// this with Image frame 0's SHP header RGB before reading `RadarColor`.
    /// No player-visible consumer is asserted by this retained type state.
    pub radar_color: [u8; 3],
    /// Ordered per-pass inputs projected from NativeRulesProcessOwner until
    /// the terrain asset binder applies the deferred SHP-derived defaults.
    /// `radar_color` remains the only final typed value.
    #[serde(skip)]
    pub(crate) radar_color_read_passes: Vec<TerrainRadarColorReadPass>,
    /// `WaterBound=`.
    pub water_bound: bool,
    /// `IsVeinhole=`; when true, gamemd also forces `LegalTarget=true`.
    pub is_veinhole: bool,
    /// Raw `TemperateOccupationBits=` ReadInt, including signed/high bits.
    temperate_occupation_bits: i32,
    /// Raw `SnowOccupationBits=` ReadInt, including signed/high bits.
    snow_occupation_bits: i32,
    /// Art `Foundation=`. Defaults to 1x1 until art.ini merge patches it.
    pub foundation: String,
}

impl TerrainObjectType {
    #[cfg(test)]
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        Self::from_ini_section_with_tree_strength(
            name,
            section,
            crate::rules::ruleset::GeneralRules::default().tree_strength,
        )
    }

    pub fn from_ini_section_with_tree_strength(
        name: &str,
        section: &IniSection,
        tree_strength: i32,
    ) -> Self {
        let armor = section
            .read_name("Armor", 0x80)
            .unwrap_or("wood")
            .to_ascii_lowercase();
        // ReadDouble -> `FSTP dword [ESI+0x2A4]` (`0x0071E073`) over the
        // constructor's zero (`0x0071DACD`).
        let animation_probability =
            section.read_double_to_float("AnimationProbability", NativeF32Bits::POSITIVE_ZERO);
        let mut read_state = TerrainTypeReadState::new(name);
        read_state.read_pass(section, &crate::rules::ini_parser::IniFile::empty());
        // Terrain ctor71DBAC initializes Strength=-1. ObjectType5F94D3 reads
        // with that current default; Terrain71DEC8..71DEDC substitutes
        // Rules+1144 when the stored result is exactly -1, including an
        // authored -1. Original execution: tools/spatial_oracle/terrain_strength.
        let strength = section.read_int("Strength", -1);

        Self {
            name: name.to_string(),
            image_file: read_state.image.clone(),
            theater: read_state.theater,
            alpha_image: section.read_object_alpha_image(""),
            is_logic: read_state.is_logic,
            spawns_tiberium: section.read_bool("SpawnsTiberium", false),
            is_animated: section.read_bool("IsAnimated", false),
            animation_rate: section.read_int("AnimationRate", 0),
            animation_probability,
            // ObjectType `Armor=` (`0x005F94C8`): ReadString 0x80 ahead of the
            // armor-name lookup, over the constructor's Wood (`0x0071DBB6`).
            armor_id: crate::rules::armor::armor_index(&armor),
            armor,
            strength: if strength == -1 {
                tree_strength
            } else {
                strength
            },
            immune: section.read_bool("Immune", false),
            legal_target: read_state.legal_target,
            is_flammable: section.read_bool("IsFlammable", false),
            bombable: section.read_bool("Bombable", true),
            insignificant: section.read_bool("Insignificant", true),
            radar_invisible: section.read_bool("RadarInvisible", true),
            radar_color: [0; 3],
            radar_color_read_passes: Vec::new(),
            water_bound: section.read_bool("WaterBound", false),
            is_veinhole: read_state.is_veinhole,
            // TerrainTypeCtor71DAB7/71DABD starts both dwords at7.
            // TerrainTypeReadINI71E079..71E0A0 stores ReadInt directly,
            // without clamps; the shared reader folds prior rule passes.
            temperate_occupation_bits: section.read_int("TemperateOccupationBits", 7),
            snow_occupation_bits: section.read_int("SnowOccupationBits", 7),
            foundation: foundation::FOUNDATION_TABLE[usize::from(read_state.foundation)]
                .name
                .to_string(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn merge_art_foundation(&mut self, foundation_name: &str) {
        self.foundation = foundation::foundation_name(foundation_name).to_string();
    }

    /// Terrain InitTheater71DCD8..71DD3B loads its stored ID and extension.
    /// The fresh non-theater reader71DF9E..71DFC0 loads only ImageFile.SHP;
    /// neither branch follows ART Image redirects or alternate candidates.
    pub(crate) fn shape_filename(&self, theater_ext: &str) -> String {
        if self.theater {
            format!("{}.{}", self.name, theater_ext)
        } else {
            format!("{}.SHP", self.image_file)
        }
    }

    pub(crate) fn uses_theater_shape(&self) -> bool {
        self.theater
    }

    pub fn armor_id(&self) -> crate::rules::armor::ArmorId {
        self.armor_id
    }

    pub fn raw_occupation_bits(&self, snow_theater: bool) -> i32 {
        if snow_theater {
            self.snow_occupation_bits
        } else {
            self.temperate_occupation_bits
        }
    }

    pub(crate) fn is_logic(&self) -> bool {
        self.is_logic
    }

    pub(crate) fn bind_armor(&mut self, armor: crate::rules::armor::ArmorId) {
        self.armor_id = armor;
    }
}

/// The two relevant native inputs in one admitted TerrainTypeClass::ReadINI
/// pass. Image is the inherited ObjectTypeClass `+0x1F8` buffer; its asset
/// color is deferred until the existing terrain asset binding phase.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TerrainRadarColorReadPass {
    pub(crate) image: Option<String>,
    pub(crate) radar_color: Option<String>,
}

/// Process-owned TerrainType state that cannot be reconstructed from a merged
/// section: inherited Image, ordered color inputs and the IsVeinhole post-read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TerrainTypeReadState {
    #[serde(default)]
    image: String,
    theater: bool,
    new_theater: bool,
    foundation: u8,
    #[serde(default)]
    reads: Vec<TerrainRadarColorReadPass>,
    is_logic: bool,
    is_veinhole: bool,
    legal_target: bool,
}

impl TerrainTypeReadState {
    /// ObjectTypeClass::Constructor @0x005F7090 copies the stored type ID into
    /// the inherited ImageFile buffer at +0x1F8 before the first ReadINI.
    pub(crate) fn new(type_id: &str) -> Self {
        Self {
            image: type_id.to_string(),
            theater: false,
            new_theater: false,
            foundation: foundation::DEFAULT_FOUNDATION_ID,
            reads: Vec::new(),
            is_logic: true,
            is_veinhole: false,
            legal_target: false,
        }
    }

    /// TerrainTypeClass::ReadINI @0x0071DEA0 first calls inherited
    /// ObjectTypeClass::ReadINI @0x005F92D0, whose current Image buffer is
    /// the default to ReadString("Image", ..., 0x19). The derived reader then
    /// loads frame 0 and reads RadarColor with that current RGB as default.
    pub(crate) fn read_pass(
        &mut self,
        section: &crate::rules::ini_parser::IniSection,
        fixed_art: &crate::rules::ini_parser::IniFile,
    ) {
        self.image = section.read_string("Image", &self.image, 0x19);
        let image_art = fixed_art.section_or_empty(&self.image);
        (self.theater, self.new_theater) = crate::rules::object_type::read_object_theater_flags(
            image_art,
            (self.theater, self.new_theater),
        );
        // TerrainType ReadINI71DF61..71DF96 reads this same retained Image,
        // using the current Foundation as default through shared ReadFoundation.
        self.foundation = foundation::read_foundation(image_art, self.foundation);
        // Original instructions: ObjectTypeClass::ReadINI5F9499..5F94C2
        // reads LegalTarget with its current value. TerrainTypeClass::ReadINI
        //71DEEA..71DF0C then reads IsVeinhole and, when true, clears IsLogic
        // and forces LegalTarget. A later false never restores IsLogic.
        self.legal_target = section.read_bool("LegalTarget", self.legal_target);
        self.is_veinhole = section.read_bool("IsVeinhole", self.is_veinhole);
        if self.is_veinhole {
            self.is_logic = false;
            self.legal_target = true;
        }
        self.reads.push(TerrainRadarColorReadPass {
            // Object LoadSHP5F9203/5F923E clears then skips TerrainType.
            // Only fresh non-theater71DF9C..71DFC0 reloads ImageFile.SHP
            // before the optional GetColor71DFD0..71DFF5.
            image: (!self.theater).then(|| self.image.clone()),
            radar_color: section.raw_value("RadarColor").map(str::to_string),
        });
    }

    pub(crate) fn project_into(&self, terrain: &mut TerrainObjectType) {
        terrain.is_logic = self.is_logic;
        terrain.is_veinhole = self.is_veinhole;
        terrain.legal_target = self.legal_target;
        terrain.image_file.clone_from(&self.image);
        terrain.theater = self.theater;
        terrain.foundation = foundation::FOUNDATION_TABLE[usize::from(self.foundation)]
            .name
            .to_string();
        terrain.radar_color_read_passes.clone_from(&self.reads);
    }

    pub(crate) fn validate_saved_state(&self) -> Result<(), crate::rules::error::RulesError> {
        if usize::from(self.foundation) >= foundation::FOUNDATION_TABLE.len() {
            return Err(crate::rules::error::RulesError::InvalidSavedState(
                "Terrain Foundation is outside its native table",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn parse_tibtre_section_defaults() {
        let ini = IniFile::from_str(
            "[TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
             AnimationRate=3\nAnimationProbability=.003\n",
        );
        let section = ini.section("TIBTRE01").expect("section");
        let t = TerrainObjectType::from_ini_section("TIBTRE01", section);
        assert_eq!(t.name, "TIBTRE01");
        assert!(t.spawns_tiberium);
        assert!(t.is_animated);
        assert_eq!(t.animation_rate, 3);
        assert_eq!(t.animation_probability.bits(), 0x3b44_9ba6);
        assert_eq!(t.armor, "wood");
        assert_eq!(
            t.strength,
            crate::rules::ruleset::GeneralRules::default().tree_strength
        );
        assert!(!t.immune);
        assert!(!t.legal_target);
        assert!(!t.is_flammable);
        assert!(t.insignificant);
        assert!(t.radar_invisible);
        assert!(!t.water_bound);
        assert!(!t.is_veinhole);
        assert_eq!(t.temperate_occupation_bits, 7);
        assert_eq!(t.snow_occupation_bits, 7);
        assert_eq!(t.foundation, "1x1");
    }

    #[test]
    fn parse_tibtre_lifecycle_fields_and_tree_strength_fallback() {
        let ini = IniFile::from_str(
            "[TIBTRE03]\nSpawnsTiberium=yes\nIsAnimated=yes\nImmune=yes\n\
             Armor=None\nIsVeinhole=true\nTemperateOccupationBits=4\nSnowOccupationBits=7\n",
        );
        let section = ini.section("TIBTRE03").expect("section");
        let t = TerrainObjectType::from_ini_section_with_tree_strength("TIBTRE03", section, 375);

        assert_eq!(t.strength, 375);
        assert_eq!(t.armor, "none");
        assert!(t.immune);
        assert!(t.is_veinhole);
        assert!(t.legal_target);
        assert_eq!(t.temperate_occupation_bits, 4);
        assert_eq!(t.snow_occupation_bits, 7);
    }

    #[test]
    fn bridge_tree_strength_reader_matches_original_constructor_and_fallback() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/terrain_strength.json",
        ))
        .unwrap();
        for row in corpus["cases"].as_array().unwrap() {
            // This test supplies a present Terrain section, so ObjectType's
            // absent-section refusal is native characterization only.
            if row["read_success"] == false {
                continue;
            }
            let raw = row["raw"].as_str();
            let strength = raw
                .map(|raw| format!("Strength={raw}\n"))
                .unwrap_or_default();
            let ini = IniFile::from_str(&format!(
                "[General]\nTreeStrength={}\n[TerrainTypes]\n0=TREE01\n[TREE01]\nFixture=1\n{strength}",
                row["tree_strength"].as_i64().unwrap()
            ));
            let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
            let terrain = rules
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap();
            assert_eq!(
                i64::from(terrain.strength),
                row["strength"].as_i64().unwrap(),
                "{row}"
            );
        }

        let Some(retail) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules =
            crate::rules::retail_ini_fixture::retail_rulesmd_rules().expect("retail rulesmd.ini");
        let general = retail
            .section("General")
            .unwrap()
            .read_int("TreeStrength", 0);
        let raw = retail.section("TREE01").unwrap().get_for_test("Strength");
        let row = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["read_success"] == true
                    && row["tree_strength"] == general
                    && row["raw"].as_str() == raw
            })
            .expect("the native corpus must include the actual retail TREE01 inputs");
        assert_eq!(
            i64::from(
                rules
                    .terrain_object_type_case_insensitive("TREE01")
                    .unwrap()
                    .strength
            ),
            row["strength"].as_i64().unwrap()
        );
    }

    #[test]
    fn terrain_flammability_retains_stock_battle_tree01_value() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let tree = battle
            .rules
            .terrain_object_type_case_insensitive("TREE01")
            .expect("stock Battle rules register TREE01");

        assert!(!tree.is_flammable);
    }

    #[test]
    fn terrain_flammability_retains_native_bool_across_rule_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[TREE01]\nIsFlammable=yes\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TREE01]\nIsFlammable=invalid\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TREE01]\nSpawnsTiberium=yes\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("production layered rules parse");
        let tree = rules
            .terrain_object_type_case_insensitive("TREE01")
            .expect("retail root registers TREE01");

        assert!(tree.is_flammable);
    }

    #[test]
    fn occupation_reads_retain_signed_values_across_rule_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[TREE01]\nTemperateOccupationBits=8\nSnowOccupationBits=-1\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[TREE01]\nTemperateOccupationBits=$invalid\nSnowOccupationBits=$invalid\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let tree = rules
            .terrain_object_type_case_insensitive("TREE01")
            .unwrap();
        // TerrainTypeReadINI71E079..71E0A0 stores each ReadInt over its
        // current dword, without clamping. ReadInt527856..527874 leaves
        // that current value when a later hexadecimal scan fails.
        assert_eq!(tree.raw_occupation_bits(false), 8);
        assert_eq!(tree.raw_occupation_bits(true), -1);
    }

    #[test]
    fn terrain_veinhole_logic_disable_survives_later_rule_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use crate::sim::world::Simulation;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[TREE01]\nIsVeinhole=yes\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TREE01]\nIsVeinhole=no\nSpawnsTiberium=yes\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).expect("layered retail types");
        let tree = rules
            .terrain_object_type_case_insensitive("TREE01")
            .unwrap();
        assert!(!tree.is_veinhole);
        assert!(
            tree.legal_target,
            "the earlier derived-reader write persists"
        );
        let mut sim = Simulation::new();
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[crate::map::overlay::TerrainObject {
                rx: 3,
                ry: 4,
                name: "TREE01".to_owned(),
            }],
            &rules,
            false,
        );
        assert!(sim.live_object_order_snapshot().is_empty());
    }

    #[test]
    fn retained_occupation_defaults_change_configuration_identity() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let continued = |initial: i32| {
            let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
            layers.push(
                RulesLayerKind::GameMode,
                IniFile::from_str(&format!("[TREE01]\nTemperateOccupationBits={initial}\n")),
            );
            let first = layers.process_with_fixed_art(&battle.fixed_art).unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let next = RulesLayerStack::new(IniFile::from_str(
                "[TREE01]\nTemperateOccupationBits=$invalid\n",
            ))
            .process_with_fixed_art_and_registry_state(
                &battle.fixed_art,
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
            RuleSet::from_processed_rules(&next).unwrap()
        };
        let zero_mask = continued(8);
        let one_mask = continued(9);
        assert_eq!(zero_mask.source_ini_hash(), one_mask.source_ini_hash());
        assert_eq!(
            zero_mask
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .raw_occupation_bits(false),
            8
        );
        assert_eq!(
            one_mask
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .raw_occupation_bits(false),
            9
        );
        assert_ne!(
            zero_mask.simulation_config_hash(),
            one_mask.simulation_config_hash()
        );
    }

    #[test]
    fn retained_terrain_logic_changes_configuration_identity() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let continued = |initial: &str| {
            let mut layers = RulesLayerStack::new((*battle.authored_rules).clone());
            layers.push(
                RulesLayerKind::GameMode,
                IniFile::from_str(&format!("[TREE01]\nIsVeinhole={initial}\n")),
            );
            let first = layers.process_with_fixed_art(&battle.fixed_art).unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let next = RulesLayerStack::new(IniFile::from_str(
                "[TREE01]\nIsVeinhole=no\nLegalTarget=no\nSpawnsTiberium=yes\n",
            ))
            .process_with_fixed_art_and_registry_state(
                &battle.fixed_art,
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
            RuleSet::from_processed_rules(&next).unwrap()
        };
        let enabled = continued("no");
        let disabled = continued("yes");
        assert!(
            enabled
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .is_logic()
        );
        assert!(
            !disabled
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap()
                .is_logic()
        );
        assert_eq!(enabled.source_ini_hash(), disabled.source_ini_hash());
        assert_ne!(
            enabled.simulation_config_hash(),
            disabled.simulation_config_hash()
        );
    }
}
