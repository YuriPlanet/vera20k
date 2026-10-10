//! Weapon type definitions parsed from rules.ini.
//!
//! Each weapon in RA2 has its own `[WeaponName]` section in rules.ini,
//! defining damage, range, rate of fire, and references to a projectile
//! type and warhead type. Units reference weapons via their `Primary=`
//! and `Secondary=` keys.
//!
//! ## rules.ini format
//! ```ini
//! [105mm]
//! Damage=65
//! ROF=50
//! Range=5.75
//! Speed=40
//! Projectile=InvisibleLow
//! Warhead=AP
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::util::fixed_math::{SimFixed, sim_from_f32};

/// A weapon definition parsed from a rules.ini section.
///
/// Weapons are the bridge between units and damage. A unit fires a weapon,
/// which spawns a projectile carrying a warhead. The warhead determines
/// actual damage based on the target's armor type.
///
/// All 63 fields from the original WeaponTypeClass (verified from
/// decompilation of WeaponTypeClass::ReadINI at 0x772080).
#[derive(Debug, Clone)]
pub struct WeaponType {
    // ── Core fields ──────────────────────────────────────────────────
    /// Section name in rules.ini (e.g., "105mm", "Vulcan", "RedEye2").
    pub id: String,
    /// Base damage per hit.
    pub damage: i32,
    /// Maximum range in cells (fixed-point for deterministic range checks).
    pub range: SimFixed,
    /// Maximum range in LEPTONS — the value gamemd actually stores.
    ///
    /// `WeaponTypeClass::ReadINI` 0x00772080 fills `+0xB4` through
    /// `CCINIClass::ReadRange` 0x00474620 at 0x00772336, and that reader
    /// multiplies the INI cell count by `256.0` (`FMUL double ptr
    /// [0x007E1710]` at 0x0047464C) before `Math__ftol` chops it. Every native
    /// consumer — `TechnoClass::InRange` 0x006F7220 included — reads leptons.
    ///
    /// `range` above is the cell-valued form the older call sites still read;
    /// scaling it with `to_num::<i64>() * 256` throws the fraction away, which
    /// is what stranded 60 of the 256 stock weapons that carry a `Range=` key
    /// (21 of them at `Range=1.5`) short of their authored reach — up to 212
    /// leptons, and a third of it at `1.5`. Use this field for any range
    /// compare.
    ///
    /// The `-2` "infinite" sentinel survives the conversion as `-512`, which
    /// is the constant 0x006F724E tests.
    pub range_leptons: i32,
    /// Rate of fire: frames between consecutive shots (lower = faster).
    pub rof: i32,
    /// Native stored travel speed in leptons/frame, after ReadSpeed and the
    /// per-pass ballistic postpass. Authored Speed=40 reads as 102. Zero alone
    /// does not designate hitscan; projectile type and launch determine motion.
    pub speed: i32,
    /// Projectile type ID (references a [ProjectileName] section).
    pub projectile: Option<String>,
    /// Warhead type ID (references a [WarheadName] section).
    pub warhead: Option<String>,
    /// `Report=` firing sounds (references [SoundName] sections in
    /// soundmd.ini). Played each time this weapon fires.
    pub report: Vec<String>,
    /// Number of rapid shots per attack cycle (default 1).
    /// After the full burst, the ROF cooldown begins.
    pub burst: i32,
    /// `RevealOnFire=` (`+0x137`, default yes): a launched shot reveals the
    /// firer to a human victim (FireAt `0x006FF66C`). Retail turns it off on
    /// the sniper, Virus, disguise and Mirage weapons.
    pub reveal_on_fire: bool,

    // ── Int/fixed-point fields ───────────────────────────────────────
    /// Ambient damage dealt to units standing near the weapon's owner (+0x98).
    pub ambient_damage: i32,
    /// Minimum firing range in cells (+0xb8). Weapon won't fire if target
    /// is closer than this distance.
    pub minimum_range: SimFixed,
    /// Minimum firing range in LEPTONS — `+0xB8` through the same
    /// `CCINIClass::ReadRange` 0x00474620 call at 0x00772350.
    pub minimum_range_leptons: i32,
    /// Blink time for disguise fake when firing while disguised (+0x13c).
    pub disguise_fake_blink_time: i32,
    /// Signed byte duration of the laser effect, sign-extended for its timer
    /// consumer (+0x14E; constructor10, ReadInt7727DE, MOVSX6FD21D).
    pub laser_duration: i32,
    /// Radiation level emitted on impact (+0x158).
    pub rad_level: i32,

    // ── String/reference fields ──────────────────────────────────────
    /// `DownReport=` sounds, played when the weapon fires downward (e.g.,
    /// from a building).
    pub down_report: Vec<String>,
    /// Animation played during assault (garrison clearing).
    pub assault_anim: Option<String>,
    /// Animation played by occupants when firing from a building.
    pub occupant_anim: Option<String>,
    /// Animation played by units firing from an open-topped transport.
    pub open_topped_anim: Option<String>,
    /// Particle system spawned when this weapon fires.
    pub attached_particle_system: Option<String>,
    /// Comma-separated list of impact animations, indexed by damage magnitude.
    pub anim: Vec<String>,

    // ── Color fields (3 bytes each, "R,G,B" format) ──────────────────
    /// Inner color of laser beam (+0x120).
    pub laser_inner_color: [u8; 3],
    /// Outer color of laser beam (+0x123).
    pub laser_outer_color: [u8; 3],
    /// Outer spread/glow color of laser beam (+0x126).
    pub laser_outer_spread: [u8; 3],

    // ── Bool fields ──────────────────────────────────────────────────
    /// Weapon uses fire particle effects (+0x129).
    pub use_fire_particles: bool,
    /// Weapon uses spark particle effects (+0x12a).
    pub use_spark_particles: bool,
    /// Fires in all directions regardless of facing (+0x12b).
    pub omni_fire: bool,
    /// Distributes weapon fire across multiple targets (+0x12c).
    pub distributed_weapon_fire: bool,
    /// Weapon is a railgun (special projectile visual) (+0x12d).
    pub is_railgun: bool,
    /// Projectile follows a lobbed arc trajectory (+0x12e).
    pub lobber: bool,
    /// Weapon flash is extra bright (+0x12f).
    pub bright: bool,
    /// Weapon deals sonic/disruptor damage (+0x130).
    pub is_sonic: bool,
    /// Weapon spawns aircraft (like aircraft carriers) (+0x131).
    pub spawner: bool,
    /// `LimboLaunch=` (+0x132, ReadINI `0x00772114`): TechnoClass::Fire
    /// `0x006FF751` puts the FIRER in limbo after launching the bullet, which
    /// carries it (attack dog, Terror Drone and Giant Squid jumps).
    pub limbo_launch: bool,
    /// Unit must decloak before firing this weapon (+0x133).
    pub decloak_to_fire: bool,
    /// Uses cell-center rangefinding instead of edge-to-edge (+0x134).
    pub cell_rangefinding: bool,
    /// Weapon fires only once then is consumed (+0x135).
    pub fire_once: bool,
    /// Weapon is never automatically selected for use (+0x136).
    pub never_use: bool,
    /// Weapon can fire at terrain/ground (+0x138).
    pub terrain_fire: bool,
    /// Uses the sabotage cursor when this weapon is active (+0x139).
    pub sabotage_cursor: bool,
    /// Uses the MiG attack cursor (+0x13a).
    pub mig_attack_cursor: bool,
    /// Can only fire while the unit is disguised (+0x13b).
    pub disguise_fire_only: bool,
    /// Mind control effect has no unit limit (+0x140).
    pub infinite_mind_control: bool,
    /// Unit can fire this weapon while moving (+0x141). The constructor sets
    /// it (`0x00771DE6`) and `ReadINI` keeps that as its default
    /// (`0x0077221E..0x00772232`); stock only DiskDrain says no.
    pub fire_while_moving: bool,
    /// Weapon drains target's health to heal the firer (+0x142).
    pub drain_weapon: bool,
    /// `FireInTransport=` (+0x143): an open-topped passenger may fire it.
    /// The constructor sets it (`0x00771DF3`) and `ReadINI` keeps that as its
    /// default (ReadBool at `0x00772252`); stock opts only melee and special
    /// weapons out. GetFireError refuses it from an open-topped transport
    /// (`0x006FC57D`).
    pub fire_in_transport: bool,
    /// Firing this weapon kills the attacker (+0x144).
    pub suicide: bool,
    /// Grants speed boost on hit (+0x145).
    pub turbo_boost: bool,
    /// Suppresses target (Westwood's misspelling preserved) (+0x146).
    pub supress: bool,
    /// Weapon spawns a camera/reveal at impact point (+0x147).
    pub camera: bool,
    /// Weapon has limited charges (+0x148).
    pub charges: bool,
    /// Weapon fires a laser beam visual (+0x149).
    pub is_laser: bool,
    /// Weapon fires a disk-shaped laser (Vortex) (+0x14a).
    pub disk_laser: bool,
    /// Weapon draws a visible line to target (+0x14b).
    pub is_line: bool,
    /// Weapon fires a thicker laser beam (+0x14c).
    pub is_big_laser: bool,
    /// Laser color matches house/player color (+0x14d).
    pub is_house_color: bool,
    /// Weapon is affected by ion storms (+0x14f).
    pub ion_sensitive: bool,
    /// Weapon fires at area/ground instead of specific target (+0x150).
    pub area_fire: bool,
    /// Weapon fires an electric bolt visual (Tesla) (+0x151).
    pub is_electric_bolt: bool,
    /// Draw electric bolt using laser rendering (+0x152).
    pub draw_bolt_as_laser: bool,
    /// Laser uses alternate (darker) color scheme (+0x153).
    pub is_alternate_color: bool,
    /// Weapon fires a radiation beam visual (+0x154).
    pub is_rad_beam: bool,
    /// Weapon triggers a radiation eruption effect (+0x155).
    pub is_rad_eruption: bool,
    /// Weapon fires a magnetron beam (+0x15c).
    pub is_mag_beam: bool,
}

impl WeaponType {
    /// `Report.Count` (`WeaponTypeClass+0xCC`).
    pub fn report_count(&self) -> i32 {
        self.report.len() as i32
    }

    /// `Report.Items[index]`.
    pub fn report_item(&self, index: usize) -> Option<&str> {
        self.report.get(index).map(String::as_str)
    }

    /// Parse a WeaponType from a rules.ini section.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        Self {
            id: id.to_string(),
            damage: section.read_int("Damage", 0),
            range: sim_from_f32(section.read_double("Range", 0.0) as f32),
            // `WeaponTypeClass::Constructor` 0x00771C70 zeroes +0xB4 at
            // 0x00771CB6, so an absent (or literal `-1`) key leaves 0.
            range_leptons: section.read_range("Range", 0),
            rof: section.read_int("ROF", 0),
            // Weapon771C70 initializes +A8 to zero; ReadINI7722FD calls
            // ReadSpeed474810. Ordered Rules processing applies the later
            // Weapon7729F0 postpass to the live retained value.
            speed: section.read_speed("Speed", 0),
            // ReadString 0x80 ahead of each type lookup (`0x00772998`,
            // `0x0077295F`).
            projectile: section.read_name("Projectile", 0x80).map(str::to_string),
            warhead: section.read_name("Warhead", 0x80).map(str::to_string),
            report: sound_list(section, "Report"),
            burst: section.read_int("Burst", 1),
            // Constructor 1 (`0x00771DBB`); ReadINI passes it as the default
            // (`0x00772182..0x00772196`).
            reveal_on_fire: section.read_bool("RevealOnFire", true),

            // Int/fixed-point fields
            ambient_damage: section.read_int("AmbientDamage", 0),
            minimum_range: sim_from_f32(section.read_double("MinimumRange", 0.0) as f32),
            // +0xB8 is zeroed alongside +0xB4 at 0x00771CBF.
            minimum_range_leptons: section.read_range("MinimumRange", 0),
            disguise_fake_blink_time: section.read_int("DisguiseFakeBlinkTime", 0),
            // ReadINI stores only AL; SpawnLaser6FD21D sign-extends the
            // retained byte. ReadInt uses its default only for absent keys,
            // so narrowing after the projected pass history is equivalent.
            laser_duration: i32::from(section.read_int("LaserDuration", 10) as i8),
            rad_level: section.read_int("RadLevel", 0),

            // String/reference fields
            down_report: sound_list(section, "DownReport"),
            // ReadString 0x80 at `0x0077257C`, `0x007725B5`, `0x007725EE`.
            assault_anim: section.read_name("AssaultAnim", 0x80).map(str::to_string),
            occupant_anim: section.read_name("OccupantAnim", 0x80).map(str::to_string),
            open_topped_anim: section
                .read_name("OpenToppedAnim", 0x80)
                .map(str::to_string),
            // A 0x14-byte buffer (`0x00772920`).
            attached_particle_system: section
                .read_name("AttachedParticleSystem", 0x14)
                .map(str::to_string),
            // `0x00772479`: ReadString 0x80, then `strtok(",")`, keeping each
            // token `0x00428B80` resolves (`0x007724B8`). RESIDUAL: VERA keeps
            // unresolved names too, which shifts the damage-indexed pick past
            // one; every retail `Anim=` name resolves.
            anim: section
                .read_list("Anim", 0x80)
                .unwrap_or_default()
                .into_iter()
                .map(str::to_string)
                .collect(),

            // Color fields: ReadColorRGB over the constructor's zero
            // (`0x00771D30`-`0x00771D60`), at `0x00772770`, `0x00772796` and
            // `0x007727BC`.
            laser_inner_color: section.read_color_rgb("LaserInnerColor", [0; 3]),
            laser_outer_color: section.read_color_rgb("LaserOuterColor", [0; 3]),
            laser_outer_spread: section.read_color_rgb("LaserOuterSpread", [0; 3]),

            // Bool fields
            use_fire_particles: section.read_bool("UseFireParticles", false),
            use_spark_particles: section.read_bool("UseSparkParticles", false),
            omni_fire: section.read_bool("OmniFire", false),
            distributed_weapon_fire: section.read_bool("DistributedWeaponFire", false),
            is_railgun: section.read_bool("IsRailgun", false),
            lobber: section.read_bool("Lobber", false),
            bright: section.read_bool("Bright", false),
            is_sonic: section.read_bool("IsSonic", false),
            spawner: section.read_bool("Spawner", false),
            limbo_launch: section.read_bool("LimboLaunch", false),
            // WeaponTypeClass constructor / ReadINI consumed by
            // TechnoClass::GetFireError @ 0x006FC0B0: omitted
            // DecloakToFire= is YES. Stock CruiseLauncher relies on the
            // constructor default; BoomerTorpedo explicitly opts out.
            decloak_to_fire: section.read_bool("DecloakToFire", true),
            cell_rangefinding: section.read_bool("CellRangefinding", false),
            fire_once: section.read_bool("FireOnce", false),
            never_use: section.read_bool("NeverUse", false),
            terrain_fire: section.read_bool("TerrainFire", false),
            sabotage_cursor: section.read_bool("SabotageCursor", false),
            mig_attack_cursor: section.read_bool("MigAttackCursor", false),
            disguise_fire_only: section.read_bool("DisguiseFireOnly", false),
            infinite_mind_control: section.read_bool("InfiniteMindControl", false),
            fire_while_moving: section.read_bool("FireWhileMoving", true),
            drain_weapon: section.read_bool("DrainWeapon", false),
            fire_in_transport: section.read_bool("FireInTransport", true),
            suicide: section.read_bool("Suicide", false),
            turbo_boost: section.read_bool("TurboBoost", false),
            supress: section.read_bool("Supress", false),
            camera: section.read_bool("Camera", false),
            charges: section.read_bool("Charges", false),
            is_laser: section.read_bool("IsLaser", false),
            disk_laser: section.read_bool("DiskLaser", false),
            is_line: section.read_bool("IsLine", false),
            is_big_laser: section.read_bool("IsBigLaser", false),
            is_house_color: section.read_bool("IsHouseColor", false),
            ion_sensitive: section.read_bool("IonSensitive", false),
            area_fire: section.read_bool("AreaFire", false),
            is_electric_bolt: section.read_bool("IsElectricBolt", false),
            draw_bolt_as_laser: section.read_bool("DrawBoltAsLaser", false),
            is_alternate_color: section.read_bool("IsAlternateColor", false),
            is_rad_beam: section.read_bool("IsRadBeam", false),
            is_rad_eruption: section.read_bool("IsRadEruption", false),
            is_mag_beam: section.read_bool("IsMagBeam", false),
        }
    }
}

/// `Report=`/`DownReport=` through ReadSoundList (`0x00772394`,
/// `0x0077241A`); absent leaves the constructor's empty list.
fn sound_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_sound_list(key)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    /// Retail `RevealOnFire=` through the production reader: the constructor's
    /// yes stands unless a section says no (the sniper and disguise weapons).
    #[test]
    fn retail_reveal_on_fire_defaults_to_yes() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("retail rules parse");
        assert!(rules.weapon("M60").unwrap().reveal_on_fire);
        assert!(rules.weapon("105mm").unwrap().reveal_on_fire);
        assert!(!rules.weapon("AWP").unwrap().reveal_on_fire);
    }

    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::util::fixed_math::SIM_ZERO;

    fn laser_corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/weapon_laser.json",
        ))
        .unwrap()
    }

    fn laser_state(weapon: &WeaponType) -> serde_json::Value {
        serde_json::json!({
            "is_laser": weapon.is_laser,
            "is_house_color": weapon.is_house_color,
            "is_big_laser": weapon.is_big_laser,
            "duration": weapon.laser_duration,
            "inner": weapon.laser_inner_color,
            "outer": weapon.laser_outer_color,
            "spread": weapon.laser_outer_spread,
        })
    }

    fn cached_laser_sections(sections: &serde_json::Value) -> IniFile {
        let mut ini = IniFile::empty();
        for (name, keys) in sections.as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            for (key, value) in keys.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        ini
    }

    #[test]
    fn laser_constructor_and_signed_duration_match_original_reader() {
        let native = laser_corpus();
        let controls = &native["controls"];
        let defaults = WeaponType::from_ini_section("LaserProbe", IniSection::empty());
        assert_eq!(laser_state(&defaults), controls["constructor"]);
        let rows = controls["duration_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 24);
        for row in rows {
            let ini = cached_laser_sections(&row["sections"]);
            let weapon =
                WeaponType::from_ini_section("LaserProbe", ini.section("LaserProbe").unwrap());
            assert_eq!(laser_state(&weapon), row["state"], "{}", row["raw"]);
        }
    }

    #[test]
    fn laser_fields_retain_native_defaults_through_production_rules_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let native = laser_corpus();
        let rows = native["controls"]["retained_history"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n",
        ));
        for (index, row) in rows.iter().enumerate() {
            // Preserve the native cache's explicit empty values; the physical
            // INI loader omits them, so this is a reader-component control.
            layers.push(
                RulesLayerKind::Scenario,
                cached_laser_sections(&row["sections"]),
            );
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("LaserProbe").unwrap()),
                row["state"],
                "pass {index}"
            );
        }
    }

    #[test]
    fn retail_prism_laser_fields_match_original_layered_readers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let native = laser_corpus();
        let rows = native["physical_prism"].as_array().unwrap();
        let mut layers = RulesLayerStack::new(ini);
        for row in rows {
            if row["absent"] == true {
                continue;
            }
            if row["file"] != "RULESMD.INI" {
                layers.push(
                    RulesLayerKind::Scenario,
                    cached_laser_sections(&row["sections"]),
                );
            }
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("PrismShot").unwrap()),
                row["state"],
                "{}",
                row["file"]
            );
        }
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .general
                .prism_support
                .duration,
            15
        );
    }

    #[test]
    fn retail_prism_laser_and_detail_use_complete_production_source_chain() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let native = laser_corpus();
        assert_eq!(
            laser_state(retail.rules.weapon("PrismShot").unwrap()),
            native["physical_prism"].as_array().unwrap().last().unwrap()["state"]
        );
        let detail = retail.rules.general.detail;
        assert_eq!(
            serde_json::json!({
                "min_frame_rate_normal": detail.min_frame_rate_normal,
                "min_frame_rate_movie": detail.min_frame_rate_movie,
                "buffer_zone_width": detail.buffer_zone_width,
            }),
            native["physical_detail"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["state"]
        );
        assert_eq!(retail.rules.general.prism_support.duration, 15);
    }

    #[test]
    fn retained_laser_fields_participate_in_configuration_identity() {
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::ruleset::RuleSet;

        let common = RulesLayerStack::new(IniFile::from_str(
            "[LaserProbe]\nIsLaser=junk\nIsHouseColor=junk\nIsBigLaser=junk\n",
        ));
        let mut identities = Vec::new();
        for initial in ["yes", "no"] {
            let first = RulesLayerStack::new(IniFile::from_str(&format!(
                "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n\
                 [LaserProbe]\nIsLaser={initial}\nIsHouseColor={initial}\nIsBigLaser={initial}\n",
            )))
            .process()
            .unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let processed = common
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            assert_eq!(
                rules.weapon("LaserProbe").unwrap().is_laser,
                initial == "yes"
            );
            identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
        }
        assert_eq!(
            identities[0].0, identities[1].0,
            "same current rules source"
        );
        assert_ne!(
            identities[0].1, identities[1].1,
            "different retained laser admission"
        );
    }

    #[test]
    fn test_parse_weapon() {
        let ini: IniFile = IniFile::from_str(
            "[105mm]\nDamage=65\nROF=50\nRange=5.75\nSpeed=40\n\
             Projectile=InvisibleLow\nWarhead=AP\n",
        );
        let section: &IniSection = ini.section("105mm").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("105mm", section);

        assert_eq!(weapon.id, "105mm");
        assert_eq!(weapon.damage, 65);
        assert_eq!(weapon.range, sim_from_f32(5.75));
        assert_eq!(weapon.range_leptons, 1472, "5.75 cells * 256");
        assert_eq!(weapon.rof, 50);
        assert_eq!(weapon.speed, 102);
        assert_eq!(weapon.projectile, Some("InvisibleLow".to_string()));
        assert_eq!(weapon.warhead, Some("AP".to_string()));
    }

    #[test]
    fn test_weapon_defaults() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("Empty").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("Empty", section);

        assert_eq!(weapon.damage, 0);
        assert_eq!(weapon.range, SIM_ZERO);
        assert_eq!(weapon.rof, 0);
        assert_eq!(weapon.projectile, None);
        assert_eq!(weapon.warhead, None);
        // New fields should all default correctly
        assert_eq!(weapon.ambient_damage, 0);
        assert_eq!(weapon.minimum_range, SIM_ZERO);
        assert_eq!(weapon.laser_duration, 10);
        assert_eq!(weapon.rad_level, 0);
        assert!(!weapon.is_sonic);
        assert!(!weapon.is_laser);
        assert!(!weapon.is_electric_bolt);
        assert!(weapon.anim.is_empty());
        assert_eq!(weapon.laser_inner_color, [0, 0, 0]);
        assert!(weapon.down_report.is_empty());
        assert!(weapon.decloak_to_fire, "native omitted-key default is yes");
    }

    #[test]
    fn test_parse_bool_fields() {
        let ini: IniFile = IniFile::from_str(
            "[TestWeapon]\nDamage=100\nIsSonic=yes\nSpawner=yes\n\
             FireOnce=yes\nIsLaser=yes\nIsElectricBolt=no\n\
             Lobber=true\nSuicide=yes\nAreaFire=yes\n",
        );
        let section: &IniSection = ini.section("TestWeapon").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("TestWeapon", section);

        assert!(weapon.is_sonic);
        assert!(weapon.spawner);
        assert!(weapon.fire_once);
        assert!(weapon.is_laser);
        assert!(!weapon.is_electric_bolt);
        assert!(weapon.lobber);
        assert!(weapon.suicide);
        assert!(weapon.area_fire);
        // Unset bools remain false
        assert!(!weapon.bright);
        assert!(!weapon.camera);
    }

    #[test]
    fn decloak_to_fire_default_yes_and_explicit_no_match_stock_boomer_weapons() {
        let ini = IniFile::from_str(
            "[CruiseLauncher]\nDamage=25\n\
             [BoomerTorpedo]\nDamage=40\nDecloakToFire=no\n",
        );
        let cruise =
            WeaponType::from_ini_section("CruiseLauncher", ini.section("CruiseLauncher").unwrap());
        let torpedo =
            WeaponType::from_ini_section("BoomerTorpedo", ini.section("BoomerTorpedo").unwrap());
        assert!(cruise.decloak_to_fire);
        assert!(!torpedo.decloak_to_fire);
    }

    #[test]
    fn test_parse_laser_colors() {
        let ini: IniFile = IniFile::from_str(
            "[PrismWeapon]\nDamage=100\nIsLaser=yes\n\
             LaserInnerColor=255,0,0\nLaserOuterColor=128,64,32\n\
             LaserOuterSpread=200,200,200\n",
        );
        let section: &IniSection = ini.section("PrismWeapon").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("PrismWeapon", section);

        assert_eq!(weapon.laser_inner_color, [255, 0, 0]);
        assert_eq!(weapon.laser_outer_color, [128, 64, 32]);
        assert_eq!(weapon.laser_outer_spread, [200, 200, 200]);
    }

    #[test]
    fn test_parse_anim_list() {
        let ini: IniFile =
            IniFile::from_str("[TestWeapon]\nDamage=50\nAnim=YOURFIRE,YOUREXPL,YOURBOOM\n");
        let section: &IniSection = ini.section("TestWeapon").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("TestWeapon", section);

        assert_eq!(weapon.anim.len(), 3);
        assert_eq!(weapon.anim[0], "YOURFIRE");
        assert_eq!(weapon.anim[1], "YOUREXPL");
        assert_eq!(weapon.anim[2], "YOURBOOM");
    }

    /// `CCINIClass::ReadRange` 0x00474620 multiplies by `256.0` and THEN chops,
    /// so a fractional `Range=` keeps its fraction. 60 of the 256 stock
    /// `Range=` weapons carry one and 21 sit at exactly `1.5`, the stock
    /// `[BlimpBomb]` among them.
    #[test]
    fn fractional_range_keeps_its_fraction_in_leptons() {
        let ini = IniFile::from_str(
            "[BlimpBomb]\nDamage=250\nROF=50\nRange=1.5\nCellRangefinding=yes\n\
             [Nuke]\nRange=-2\n\
             [Quarter]\nRange=0.25\n\
             [NoKey]\nDamage=1\n",
        );
        let blimp = WeaponType::from_ini_section("BlimpBomb", ini.section("BlimpBomb").unwrap());
        assert_eq!(
            blimp.range_leptons, 384,
            "1.5 cells is 384 leptons, not 256"
        );
        assert!(blimp.cell_rangefinding);

        let nuke = WeaponType::from_ini_section("Nuke", ini.section("Nuke").unwrap());
        assert_eq!(
            nuke.range_leptons, -512,
            "the -2 'infinite' sentinel scales to the -512 that 0x006F724E tests"
        );

        let quarter = WeaponType::from_ini_section("Quarter", ini.section("Quarter").unwrap());
        assert_eq!(quarter.range_leptons, 64);

        let none = WeaponType::from_ini_section("NoKey", ini.section("NoKey").unwrap());
        assert_eq!(
            none.range_leptons, 0,
            "absent key keeps the 0x00771CB6 constructor zero"
        );
        assert_eq!(none.minimum_range_leptons, 0);
    }

    #[test]
    fn test_parse_minimum_range() {
        let ini: IniFile = IniFile::from_str("[TestWeapon]\nDamage=50\nMinimumRange=3.5\n");
        let section: &IniSection = ini.section("TestWeapon").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("TestWeapon", section);

        assert_eq!(weapon.minimum_range, sim_from_f32(3.5));
        assert_eq!(weapon.minimum_range_leptons, 896, "3.5 cells * 256");
    }
}
