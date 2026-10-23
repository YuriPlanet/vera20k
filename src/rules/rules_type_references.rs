//! The RulesClass Type pointers and pointer lists the ordered Rules processor
//! keeps, and the `RuleSet` fields that publish them.
//!
//! A pointer key is a ReadString into 0x80 bytes (`0x00528A10`) whose empty
//! read keeps the field and whose value goes through its family's
//! FindOrAllocate, which answers null for `none` and `<none>`. A list key is
//! the same ReadString, whose empty read keeps the vector and whose value
//! `strtok`s into a new vector of the tokens' non-null Types. The RulesClass
//! constructor (`0x00665650`) leaves each pointer null and each vector empty,
//! and RulesClass lives for the whole process: a pass or a later Process call
//! that omits a key keeps the earlier value. Full_Init's reset (`0x006686C0`)
//! deletes the Types and leaves these fields alone; VERA clears them there
//! instead (`NativeRulesRegistryState::destructive_reset`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::{AnimRef, RuleSet};

/// The pointers and lists the ordered Rules processor keeps, by INI key, as
/// the stored IDs of the Types their factories answered. It keeps the keys of
/// [`POINTERS`], [`DEFAULT_PARTICLE_SYSTEMS`] and [`LISTS`]; it allocates the
/// Types of the other RulesClass keys and drops the pointers, as VERA ports
/// no reader of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct RulesTypeReferences {
    /// Non-null pointers; a key without an entry holds null.
    pointers: BTreeMap<String, String>,
    /// Lists; a key without an entry holds the constructor's empty vector.
    lists: BTreeMap<String, Vec<String>>,
}

impl RulesTypeReferences {
    /// One pointer read. `None` is an empty read, which keeps the field;
    /// `Some(None)` is the factory's null.
    pub(crate) fn read_pointer(&mut self, key: &str, resolved: Option<Option<String>>) {
        let Some(resolved) = resolved else {
            return;
        };
        if !POINTERS
            .iter()
            .chain(&DEFAULT_PARTICLE_SYSTEMS)
            .any(|&(kept, _)| kept == key)
        {
            return;
        }
        match resolved {
            Some(id) => {
                self.pointers.insert(key.to_owned(), id);
            }
            None => {
                self.pointers.remove(key);
            }
        }
    }

    /// One list read. `None` is an empty read, which keeps the vector.
    pub(crate) fn read_list(&mut self, key: &str, resolved: Option<Vec<String>>) {
        if let Some(list) = resolved
            && LISTS.iter().any(|&(kept, _)| kept == key)
        {
            self.lists.insert(key.to_owned(), list);
        }
    }

    pub(crate) fn list(&self, key: &str) -> &[String] {
        self.lists.get(key).map_or(&[], Vec::as_slice)
    }

    /// Publish each kept pointer and list; a null pointer is the empty name.
    pub(crate) fn apply_to(&self, rules: &mut RuleSet) {
        for &(key, publish) in POINTERS.iter().chain(&DEFAULT_PARTICLE_SYSTEMS) {
            publish(rules, self.pointers.get(key).map_or("", String::as_str));
        }
        for &(key, publish) in LISTS {
            publish(rules, self.list(key));
        }
    }
}

/// Where `RuleSet` publishes a pointer: the stored ID, empty for null.
type PublishPointer = fn(&mut RuleSet, &str);
/// Where `RuleSet` publishes a list of stored IDs.
type PublishList = fn(&mut RuleSet, &[String]);

fn named(id: &str) -> Option<String> {
    (!id.is_empty()).then(|| id.to_owned())
}

/// The kept pointers, by reader. Each Rules offset is the constructor's null
/// store and the read's store.
const POINTERS: &[(&str, PublishPointer)] = &[
    // ReadGeneral already resolves this pointer; EMP is its first consumer.
    ("EMPulseSparkles", |rules, id| {
        rules.bind_emp_sparkles(named(id))
    }),
    // ReadCombatDamage (`0x0066BBB0`): FlameDamage2 +F88 (constructor
    //666B22, read66C2D4..66C2FE); C4Warhead +FA8 (constructor
    // `0x00666B52`, read `0x0066C31F`), CrushWarhead +FAC (`0x00666B58`,
    // `0x0066C35D`), IvanWarhead +FC8 (`0x00666B82`, `0x0066C50C`), WarheadTypes;
    // DrainAnimationType +31C, ControlledAnimationType +320 and
    // PermaControlledAnimationType +324 (`0x00665AD8..0x00665AE4`,
    // `0x0066C94F..0x0066CA4F`), AnimTypes.
    ("FlameDamage2", |rules, id| {
        rules.combat_damage.bind_flame_damage2(named(id));
    }),
    ("C4Warhead", |rules, id| {
        rules.bridge_warheads.c4_name = id.to_owned();
    }),
    ("CrushWarhead", |rules, id| {
        rules.bridge_warheads.crush_name = id.to_owned();
    }),
    ("IvanWarhead", |rules, id| {
        rules.combat_damage.ivan_warhead = named(id);
    }),
    ("DrainAnimationType", |rules, id| {
        rules.combat_damage.drain_animation_type = named(id);
    }),
    ("ControlledAnimationType", |rules, id| {
        rules.mind_control.controlled_anim = named(id);
    }),
    ("PermaControlledAnimationType", |rules, id| {
        rules.mind_control.perma_controlled_anim = named(id);
    }),
    // ReadRadiation: RadSiteWarhead +1834 (`0x00667801`,
    // `0x0066D0F6..0x0066D127`).
    ("RadSiteWarhead", |rules, id| {
        rules.radiation.site_warhead = id.to_owned();
    }),
    // ReadSpecialWeapons: MutateWarhead +F98 (`0x00666B3A`, `0x006690B3`) and
    // MutateExplosionWarhead +F9C (`0x00666B40`, `0x006690F2`).
    ("MutateWarhead", |rules, id| {
        rules.general.mutate_warhead = id.to_owned();
    }),
    ("MutateExplosionWarhead", |rules, id| {
        rules.general.mutate_explosion_warhead = id.to_owned();
    }),
    // ReadSpecialWeapons @ 0x00668FB0: EMPulseWarhead +FA0. Constructor
    // null is at `0x00666B46`; ReadString `0x00528A10` (call `0x00669138`)
    // starts with an empty buffer and preserves the prior pointer on empty;
    // names resolve through FindOrAllocate `0x0075E3B0` (call `0x00669145`).
    ("EMPulseWarhead", |rules, id| {
        rules.general.empulse_warhead = named(id);
    }),
    // ReadGeneral (`0x0066D530`), AnimTypes through `0x00428B80` unless
    // noted: OreTwinkle +1870 (`0x0066788E`, `0x0066D67B..0x0066D6A2`),
    // BarrelParticle +74, a ParticleSystemType through `0x00644890`
    // (`0x006656EB`, `0x0066D7ED..0x0066D80B`), NukeTakeOff +98 (`0x00665735`,
    // `0x0066D829..0x0066D850`), Wake +94 (`0x0066572F`,
    // `0x0066D867..0x0066D88E`).
    ("OreTwinkle", |rules, id| {
        rules.general.ore_twinkle = named(id);
    }),
    // RulesCtor6656D1; ReadGeneral66D6A8..66D6D5.
    ("BarrelExplode", |rules, id| {
        rules.general.barrel_explode = named(id);
    }),
    ("BarrelParticle", |rules, id| {
        rules.general.barrel_particle = named(id);
    }),
    ("NukeTakeOff", |rules, id| {
        rules.general.nuke_take_off = id.to_owned();
    }),
    ("Wake", |rules, id| rules.general.wake = anim(id)),
    // DominatorWarhead +2F8, a WarheadType through `0x0075E3B0`
    // (`0x00665A91`, `0x0066DF72..0x0066DF90`), DominatorFirstAnim +2FC
    // (`0x00665A97`, `0x0066DFB1..0x0066DFD8`), DominatorSecondAnim +300
    // (`0x00665AA2`, `0x0066DFEF..0x0066E00D`), ChronoPlacement +330
    // (`0x00665AF6`, `0x0066E08D..0x0066E0AB`), ChronoBlast +328 (`0x00665AEA`,
    // `0x0066E10A..0x0066E131`), ChronoBlastDest +32C (`0x00665AF0`,
    // `0x0066E149..0x0066E167`).
    ("DominatorWarhead", |rules, id| {
        rules.general.dominator_warhead = id.to_owned();
    }),
    ("DominatorFirstAnim", |rules, id| {
        rules.general.dominator_first_anim = id.to_owned();
    }),
    ("DominatorSecondAnim", |rules, id| {
        rules.general.dominator_second_anim = id.to_owned();
    }),
    ("ChronoPlacement", |rules, id| {
        rules.general.chrono_placement_anim = id.to_owned();
    }),
    ("ChronoBlast", |rules, id| {
        rules.general.chrono_blast_anim = id.to_owned();
    }),
    ("ChronoBlastDest", |rules, id| {
        rules.general.chrono_blast_dest_anim = id.to_owned();
    }),
    // WarpIn +338, WarpOut +33C, WarpAway +340, ChronoSparkle1 +344,
    // IronCurtainInvokeAnim +348, ForceShieldInvokeAnim +34C (constructor
    // `0x00665B02..0x00665B20`, read `0x0066E188..0x0066E327`).
    ("WarpIn", |rules, id| rules.general.warp_in = anim(id)),
    ("WarpOut", |rules, id| rules.general.warp_out = anim(id)),
    ("WarpAway", |rules, id| rules.general.warp_away = anim(id)),
    ("ChronoSparkle1", |rules, id| {
        rules.general.chrono_sparkle1 = anim(id);
    }),
    ("IronCurtainInvokeAnim", |rules, id| {
        rules.general.iron_curtain_invoke_anim = id.to_owned();
    }),
    ("ForceShieldInvokeAnim", |rules, id| {
        rules.general.force_shield_invoke_anim = id.to_owned();
    }),
    // The infantry death anims +9C..+B4, indexed by Warhead `InfDeath`
    // (constructor `0x0066573B..0x0066575F`, read `0x0066E33E..0x0066E4DD`).
    ("InfantryExplode", |rules, id| {
        rules.general.infantry_death_anims[3] = named(id);
    }),
    ("FlamingInfantry", |rules, id| {
        rules.general.infantry_death_anims[4] = named(id);
    }),
    ("InfantryHeadPop", |rules, id| {
        rules.general.infantry_death_anims[6] = named(id);
    }),
    ("InfantryNuked", |rules, id| {
        rules.general.infantry_death_anims[7] = named(id);
    }),
    ("InfantryVirus", |rules, id| {
        rules.general.infantry_death_anims[8] = named(id);
    }),
    ("InfantryMutate", |rules, id| {
        rules.general.infantry_death_anims[9] = named(id);
    }),
    ("InfantryBrute", |rules, id| {
        rules.general.infantry_death_anims[10] = named(id);
    }),
    // MoveFlash +BB4 (`0x006665CF`, `0x0066E534..0x0066E55B`) and Parachute
    // +BBC (`0x006665E3`, `0x0066E572..0x0066E599`).
    ("MoveFlash", |rules, id| rules.general.move_flash = anim(id)),
    ("Parachute", |rules, id| {
        rules.general.parachute_shp = named(id).map(|id| id.to_ascii_uppercase());
    }),
    // InfantryTypes through the scalar reader `0x0067BAC0`, whose default is
    // the current field: Engineer +F70 (`0x0066FC94`), Technician +F6C
    // (`0x0066FCB3`), AlliedCrew +F78 (`0x0066FCF1`), SovietCrew +F7C
    // (`0x0066FD10`), ThirdCrew +F80 (`0x0066FD2F`); constructor
    // `0x00666AF8..0x00666B16`.
    ("Engineer", |rules, id| {
        rules.general.engineer_infantry = named(id);
    }),
    ("Technician", |rules, id| {
        rules.general.technician = named(id);
    }),
    ("AlliedCrew", |rules, id| {
        rules.general.allied_crew = named(id);
    }),
    ("SovietCrew", |rules, id| {
        rules.general.soviet_crew = named(id);
    }),
    ("ThirdCrew", |rules, id| {
        rules.general.third_crew = named(id);
    }),
];

/// ReadCombatDamage's nine `Default*System=` ParticleSystemTypes
/// (+1018..+1038, constructor from `0x00666C09`, read
/// `0x0066CACD..0x0066CCE0`), in read order.
pub(crate) const DEFAULT_PARTICLE_SYSTEMS: [(&str, PublishPointer); 9] = [
    ("DefaultLargeGreySmokeSystem", |rules, id| {
        rules.combat_damage.default_large_grey_smoke_system = named(id);
    }),
    ("DefaultSmallGreySmokeSystem", |rules, id| {
        rules.combat_damage.default_small_grey_smoke_system = named(id);
    }),
    ("DefaultSparkSystem", |rules, id| {
        rules.combat_damage.default_spark_system = named(id);
    }),
    ("DefaultLargeRedSmokeSystem", |rules, id| {
        rules.combat_damage.default_large_red_smoke_system = named(id);
    }),
    ("DefaultSmallRedSmokeSystem", |rules, id| {
        rules.combat_damage.default_small_red_smoke_system = named(id);
    }),
    ("DefaultDebrisSmokeSystem", |rules, id| {
        rules.combat_damage.default_debris_smoke_system = named(id);
    }),
    ("DefaultFireStreamSystem", |rules, id| {
        rules.combat_damage.default_fire_stream_system = named(id);
    }),
    ("DefaultTestParticleSystem", |rules, id| {
        rules.combat_damage.default_test_particle_system = named(id);
    }),
    ("DefaultRepairParticleSystem", |rules, id| {
        rules.combat_damage.default_repair_particle_system = named(id);
    }),
];

/// The kept lists. Each Rules offset is the constructor's empty vector and the
/// read's store; the empty read keeps the vector at the address given.
const LISTS: &[(&str, PublishList)] = &[
    // RulesCtor6656C0..6656E4; ReadGeneral66D6E2..66D7B1.
    ("BarrelDebris", |rules, ids| {
        rules.general.barrel_debris = ids.to_vec();
    }),
    // RepairBay +850: ReadGeneral66F362..66F3F8, through the shared
    // ReadString128/BuildingType4653C0 list reader. Omission/empty keeps
    // the existing vector; the Rules constructor leaves it empty.
    ("RepairBay", |rules, ids| {
        rules.repair_bay_types = ids.to_vec()
    }),
    // ReadGeneral: DamageFireTypes +2A0 (constructor `0x00665A1D`, read
    // `0x0066D573`, kept at `0x0066D609`) and DeadBodies +120 (`0x0066580B`,
    // `0x0066D9A2`, `0x0066DA38`), AnimTypes.
    ("DamageFireTypes", |rules, ids| {
        rules.general.damage_fire_types = ids
            .iter()
            .map(|id| AnimRef {
                name: id.to_ascii_uppercase(),
            })
            .collect();
    }),
    ("DeadBodies", |rules, ids| {
        rules.general.dead_bodies = ids.to_vec();
    }),
    // Shipyard +880, BuildingTypes through `0x004653C0` (`0x00666231`,
    // `0x0066F59E`, `0x0066F634`); BaseUnit +B20 and HarvesterUnit +B3C,
    // UnitTypes through `0x007480D0` (`0x00666515`/`0x00666522`,
    // `0x0066F7DA`/`0x0066F8DD`, `0x0066F870`/`0x0066F973`); PadAircraft +B58,
    // AircraftTypes through the list reader `0x0067B8F0` (`0x00666545`,
    // `0x0066F9D8`, `0x0067BA2E`).
    ("Shipyard", |rules, ids| rules.shipyard_types = ids.to_vec()),
    ("BaseUnit", |rules, ids| {
        rules.general.base_unit_types = ids.iter().map(|id| id.to_ascii_uppercase()).collect();
    }),
    ("HarvesterUnit", |rules, ids| {
        rules.harvester_unit_types = rules.ids_with_objects(ids, ObjectCategory::Vehicle);
    }),
    ("PadAircraft", |rules, ids| {
        rules.general.pad_aircraft_types = ids.iter().map(|id| id.to_ascii_uppercase()).collect();
    }),
    // ReadAI (`0x00672AE0`), BuildingTypes through `0x004653C0`: BuildConst
    // +8AC through ConcreteWalls +A50 (constructor `0x00666265..0x0066641B`,
    // read `0x00672B23..0x006737F6`).
    ("BuildConst", |rules, ids| rules.set_build_const_types(ids)),
    ("BuildPower", |rules, ids| {
        rules.build_power_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("BuildRefinery", |rules, ids| {
        rules.build_refinery_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("BuildBarracks", |rules, ids| {
        rules.build_barracks_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("BuildTech", |rules, ids| {
        rules.build_tech_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("BuildWeapons", |rules, ids| {
        rules.build_weapons_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("AlliedBaseDefenses", |rules, ids| {
        rules.allied_base_defense_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("SovietBaseDefenses", |rules, ids| {
        rules.soviet_base_defense_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("ThirdBaseDefenses", |rules, ids| {
        rules.third_base_defense_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("BuildRadar", |rules, ids| {
        rules.build_radar_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    ("ConcreteWalls", |rules, ids| {
        rules.concrete_wall_types = rules.ids_with_objects(ids, ObjectCategory::Building);
    }),
    // NeutralTechBuildings +ADC through the list reader `0x0067B550`
    // (`0x006664A7`, `0x00673926`): the random map generator reads it from
    // the processed rules, before any RuleSet exists
    // (`ProcessedRulesLayers::neutral_tech_buildings`).
    ("NeutralTechBuildings", |_, _| {}),
];

fn anim(id: &str) -> AnimRef {
    AnimRef {
        name: id.to_owned(),
    }
}
