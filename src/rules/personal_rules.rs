//! The player's own rule switches: Terrain Deformation and the Superweapon
//! Limit of the skirmish page's Game Options and the Options page's Building
//! Production Queue, which a network room's host shares. VERA-only.
//!
//! VERARULES.INI, a file of the player's own rule keys, was read here until
//! it was retired for New Mod (docs/modding.md); `persistence::rules_profile`
//! tells a player whose file still holds changes.
//!
//! Compose before the existing Rules readers rather than adding a Process pass:
//! another pass can reset omitted fields and repeat native postprocessing.

use std::borrow::Cow;

use crate::rules::ini_parser::IniFile;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalRules {
    /// The skirmish Game Options' Terrain Deformation box (`[VERA] TerrainDeformation=`
    /// in VERA.INI). Each new scenario's root reads it, so it needs no restart.
    terrain_deformation: bool,
    /// The skirmish Game Options' Superweapon Limit (`[VERA] SuperweaponLimit=` in
    /// VERA.INI), read as Terrain Deformation is: 0 while off, else
    /// [`admitted_superweapon_limit`]'s 2 to [`SUPERWEAPON_LIMIT_MAX`].
    superweapon_limit: u8,
    /// The Options page's Building Production Queue box (`[VERA]
    /// BuildingProductionQueue=` in VERA.INI). It sets Phobos' `[General]
    /// BuildingProductionQueue=` in the scenario root, so the simulation
    /// reads it from the rules every peer, save and replay reads. A network
    /// room shares its host's ([`HostRules`](crate::skirmish_launch::HostRules)).
    building_production_queue: bool,
}

impl PersonalRules {
    pub fn set_terrain_deformation(&mut self, on: bool) {
        self.terrain_deformation = on;
    }

    pub fn terrain_deformation(&self) -> bool {
        self.terrain_deformation
    }

    /// Below 2 turns it off and past [`SUPERWEAPON_LIMIT_MAX`] takes the
    /// most ([`admitted_superweapon_limit`]).
    pub fn set_superweapon_limit(&mut self, limit: i32) {
        self.superweapon_limit = admitted_superweapon_limit(limit);
    }

    pub fn superweapon_limit(&self) -> u8 {
        self.superweapon_limit
    }

    pub fn set_building_production_queue(&mut self, on: bool) {
        self.building_production_queue = on;
    }

    pub fn building_production_queue(&self) -> bool {
        self.building_production_queue
    }

    /// A switch changes the game: [`Self::scenario_root`] composes it and
    /// [`Self::compatibility_crc`] adds it to the network rules digest.
    pub fn in_effect(&self) -> bool {
        self.terrain_deformation || self.superweapon_limit != 0 || self.building_production_queue
    }

    /// The selected root as each scenario reads it: the switches' rules
    /// composed over it; unchanged while all are off. Composing here rather
    /// than before the cold start lets a process play another player's
    /// switches without rebuilding its one registry
    /// (`NativeRulesProcessOwner::use_personal_rules`). Terrain Deformation
    /// turns on
    /// `[CombatDamage] TerrainDeformation=` and gives the nuke's warhead each
    /// of `Deform=100%`, `DeformDepth=3` and `DeformRadius=10` the root does
    /// not set, so a nuke digs a crater three levels deep and 23 cells across,
    /// as wide as its retail `CellSpread=10` blast: gamemd names that warhead
    /// `NUKE` (0x0081AF98), and retail sets none of them. Superweapon
    /// Limit gives each building [`superweapon_limit`] picks its
    /// `BuildLimit=` and `SuperWeaponPerBuilding=yes`. Building Production
    /// Queue sets `[General] BuildingProductionQueue=yes`. Map and mode rules
    /// still read after it and can turn any of them off.
    pub(crate) fn scenario_root<'a>(&self, root: &'a IniFile) -> Cow<'a, IniFile> {
        if !self.in_effect() {
            return Cow::Borrowed(root);
        }
        let mut options = String::new();
        if self.superweapon_limit != 0 {
            options.push_str(&superweapon_limit(root, self.superweapon_limit));
        }
        if self.terrain_deformation {
            options.push_str(&terrain_deformation(root));
        }
        if self.building_production_queue {
            options.push_str("[General]\nBuildingProductionQueue=yes\n");
        }
        let mut root = root.clone();
        root.merge_source_sections(&IniFile::from_str(&options), true);
        Cow::Owned(root)
    }

    /// What the switches add to the network rules digest; `None` while all
    /// are off. The header keeps the words it had while VERARULES.INI was
    /// read, so a game with only the switches on digests as it did.
    pub fn compatibility_crc(&self) -> Option<u32> {
        if !self.in_effect() {
            return None;
        }
        let mut header = String::from("[VERA]\nPersonalRulesVersion=1\nOverrideMapRules=no\n");
        if self.terrain_deformation {
            header.push_str("TerrainDeformation=yes\n");
        }
        if self.superweapon_limit != 0 {
            header.push_str(&format!("SuperweaponLimit={}\n", self.superweapon_limit));
        }
        if self.building_production_queue {
            header.push_str("BuildingProductionQueue=yes\n");
        }
        Some(IniFile::from_str(&header).native_crc())
    }
}

/// Terrain Deformation's rules: the switch, and the crater keys `root`'s
/// `NUKE` warhead leaves unset.
fn terrain_deformation(root: &IniFile) -> String {
    let mut options = String::from("[CombatDamage]\nTerrainDeformation=yes\n");
    if let Some(nuke) = root.section("NUKE") {
        let crater: String = [
            ("Deform", "100%"),
            ("DeformDepth", "3"),
            ("DeformRadius", "10"),
        ]
        .into_iter()
        .filter(|(key, _)| !nuke.is_present(key))
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect();
        if !crater.is_empty() {
            options.push_str("[NUKE]\n");
            options.push_str(&crater);
        }
    }
    options
}

/// The most of each superweapon building Superweapon Limit allows.
pub const SUPERWEAPON_LIMIT_MAX: u8 = 20;

/// A Superweapon Limit from VERA.INI, a network room or a Game Options slider
/// as the game plays it: 0 (off) below 2, at most [`SUPERWEAPON_LIMIT_MAX`].
pub const fn admitted_superweapon_limit(limit: i32) -> u8 {
    if limit < 2 {
        0
    } else if limit > SUPERWEAPON_LIMIT_MAX as i32 {
        SUPERWEAPON_LIMIT_MAX
    } else {
        limit as u8
    }
}

/// Superweapon Limit's rules: each `[BuildingTypes]` entry of `root` whose
/// `SuperWeapon=` or `SuperWeapon2=` names a Super and whose positive
/// `BuildLimit=` is below `limit` (retail's six superweapons, each limited
/// to one) may be built `limit` times, each charging its own shot. A building with
/// no limit, such as a Battle Lab or a radar, keeps the house's one charge.
/// Names read as the Rules reader reads them: list entries and Supers into
/// 32 bytes (`RulesClass::Process @ 0x00668BF0`), `BuildLimit=` with its
/// constructor's default, no limit.
fn superweapon_limit(root: &IniFile, limit: u8) -> String {
    let list = root.section_or_empty("BuildingTypes");
    let mut options = String::new();
    for name in list.keys().filter_map(|key| list.read_name(key, 0x20)) {
        let Some(building) = root.section(name) else {
            continue;
        };
        let provides = ["SuperWeapon", "SuperWeapon2"].into_iter().any(|key| {
            building
                .read_name(key, 0x20)
                .is_some_and(|sw| !crate::rules::ini_parser::is_native_none_type_name(sw))
        });
        let own = building.read_int("BuildLimit", i32::MAX);
        if provides && (1..i32::from(limit)).contains(&own) {
            options.push_str(&format!(
                "[{name}]\nBuildLimit={limit}\nSuperWeaponPerBuilding=yes\n"
            ));
        }
    }
    options
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::assets::asset_manager::AssetManager;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::retail_sources::RetailRulesSources;

    fn sources(root: &[u8]) -> RetailRulesSources {
        let directory = TestDirectory::new("personal-rules");
        directory.write("rulesmd.ini", root);
        directory.write("artmd.ini", b"[TEST]\nImage=TEST\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        RetailRulesSources::select(&assets).unwrap()
    }

    #[test]
    fn a_room_plays_another_players_switches_and_keeps_what_the_shell_read() {
        let root = b"[Countries]\n0=Alpha\n[Orphan]\nMultiplay=yes\n[Warheads]\n0=NUKE\n[NUKE]\nVerses=100%\n";
        let mut owner = sources(root)
            .into_owner(Default::default(), Default::default())
            .unwrap();
        let settings = IniFile::from_str("[MultiPlayer]\nSide=Orphan\n");
        assert_eq!(owner.read_multiplayer_country(&settings, -1), 1);
        let deforms = |owner: &mut NativeRulesProcessOwner| {
            let loaded = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap();
            loaded.into_parts().0.combat_damage.terrain_deformation
        };
        // The host's switches replace the player's own, and the Country the
        // shell allocated stays for the match's loading to bind.
        let mut host = PersonalRules::default();
        host.set_terrain_deformation(true);
        owner.use_personal_rules(host);
        assert_eq!(owner.country_name(1), Some("Orphan"));
        assert!(deforms(&mut owner));
        owner.use_personal_rules(PersonalRules::default());
        assert!(!deforms(&mut owner));
    }

    #[test]
    fn terrain_deformation_box_reaches_the_next_scenario_and_network_agreement() {
        let scenario = |root: &[u8], on: bool| {
            let mut owner = sources(root)
                .into_owner(Default::default(), Default::default())
                .unwrap();
            let digests = owner.multiplayer_source_digests();
            owner.set_terrain_deformation(on);
            let (rules, _, _, _) = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            let nuke = rules.warhead("NUKE").unwrap();
            (
                rules.combat_damage.terrain_deformation,
                (nuke.deform_f64, nuke.deform_depth, nuke.deform_radius),
                digests != owner.multiplayer_source_digests(),
                owner.personal_rules_compatibility_crc().is_some(),
            )
        };
        let root = b"[Warheads]\n0=NUKE\n[NUKE]\nVerses=100%\n";
        assert_eq!(scenario(root, false), (false, (0.0, 1, 0), false, false));
        assert_eq!(scenario(root, true), (true, (1.0, 3, 10), true, true));
        // Keys the root already sets, from a mod, stay.
        let modded = b"[Warheads]\n0=NUKE\n[NUKE]\nDeform=25%\nDeformRadius=2\n";
        assert_eq!(scenario(modded, true).1, (0.25, 3, 2));
    }

    /// The Options box reaches the simulation through the scenario's rules,
    /// which every peer, save and replay reads, not through a per-client flag.
    #[test]
    fn building_production_queue_box_sets_the_scenarios_general_key() {
        let scenario = |on: bool| {
            let mut owner = sources(b"[General]\nMaximumQueuedObjects=5\n")
                .into_owner(Default::default(), Default::default())
                .unwrap();
            let mut rules = PersonalRules::default();
            rules.set_building_production_queue(on);
            owner.use_personal_rules(rules);
            let (rules, _, _, _) = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            (
                rules.general.building_production_queue,
                rules.general.maximum_queued_objects,
                owner.personal_rules_compatibility_crc().is_some(),
            )
        };
        assert_eq!(scenario(false), (false, 5, false));
        // A room shares it, so peers compare it.
        assert_eq!(scenario(true), (true, 5, true));
    }

    #[test]
    fn superweapon_limit_lifts_only_limited_superweapon_buildings() {
        let root = b"[BuildingTypes]\n0=NAMISL\n1=GATECH\n2=GAPOWR\n\
            [SuperWeaponTypes]\n0=NukeSpecial\n1=ForceShieldSpecial\n\
            [NAMISL]\nSuperWeapon=NukeSpecial\nBuildLimit=1\n\
            [GATECH]\nSuperWeapon=ForceShieldSpecial\n[GAPOWR]\nBuildLimit=1\n";
        let scenario = |limit: i32| {
            let mut owner = sources(root)
                .into_owner(Default::default(), Default::default())
                .unwrap();
            owner.set_superweapon_limit(limit);
            let (rules, _, _, _) = owner
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::empty(),
                )
                .unwrap()
                .into_parts();
            let building = |name| {
                let object = rules.object(name).unwrap();
                (object.build_limit, object.super_weapon_per_building)
            };
            (
                [building("NAMISL"), building("GATECH"), building("GAPOWR")],
                owner.personal_rules_compatibility_crc().is_some(),
            )
        };
        let off = ([(1, false), (i32::MAX, false), (1, false)], false);
        assert_eq!(scenario(0), off);
        assert_eq!(scenario(1), off);
        // The silo may be built that many times, each charging its own
        // shot; the Battle Lab keeps no limit and the house's one charge.
        assert_eq!(
            scenario(2),
            ([(2, true), (i32::MAX, false), (1, false)], true)
        );
        assert_eq!(
            scenario(99),
            ([(20, true), (i32::MAX, false), (1, false)], true)
        );
    }
}
