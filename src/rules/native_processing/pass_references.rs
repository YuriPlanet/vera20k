//! `RulesPassProcessor` reference allocation: AI, `[General]`, prerequisite,
//! CombatDamage, radiation, shield, insignia, display, AttachEffect and
//! audio-visual references.

use super::RulesPassProcessor;
use super::{ARES_GENERAL_LISTS, PARADROP_INFANTRY_KEYS, RulesTypeFamily};
use crate::rules::ini_parser::IniFile;
use crate::rules::missile_spawn::MissileFamily;
use crate::rules::rules_type_references::DEFAULT_PARTICLE_SYSTEMS;
use crate::rules::sound_ini::SoundRegistry;
use crate::util::native_x87::NativeF64Bits;

impl RulesPassProcessor {
    pub(super) fn allocate_ai_references(&mut self, pass: &IniFile) {
        const BUILDING_LISTS: &[&str] = &[
            "BuildConst",
            "BuildPower",
            "BuildRefinery",
            "BuildBarracks",
            "BuildTech",
            "BuildWeapons",
            "AlliedBaseDefenses",
            "SovietBaseDefenses",
            "ThirdBaseDefenses",
            "BuildDefense",
            "BuildPDefense",
            "BuildAA",
            "BuildHelipad",
            "BuildRadar",
            "ConcreteWalls",
            "NSGates",
            "EWGates",
            "BuildNavalYard",
            "BuildDummy",
            "NeutralTechBuildings",
        ];

        let Some(section) = pass.section("AI") else {
            return;
        };
        for key in BUILDING_LISTS {
            // Ares 3.0 Buf_AI (1005B830, hook 0x672B0E) replaces the original
            // 0x80 FindOrAllocate reads (skipped to 0x673950) with ParseList
            // (1005B340's BuildingType instantiation): the shared 0x800
            // buffer and Find-only, in this order; AIForcePredictionFudge is
            // RuleSet's int list.
            let resolved = if crate::rules::ares_switch::ares_way() {
                self.resolve_existing_list_from(section, key, RulesTypeFamily::Building, 0x800)
            } else {
                self.resolve_list_from(section, key, RulesTypeFamily::Building, 0x80)
            };
            self.rules_type_references.read_list(key, resolved);
        }
        // ReadAI672AE0 has its own AI-section gate.67399E..6739C5 reads
        // the current double as default, with no clamp, after the AI lists.
        // RulesClass outlives the destructive Type reset, so keep this scalar
        // in the same process-resident authority as the other retained rules.
        self.rules_patrol_scan_minutes =
            section.read_double_bits("PatrolScan", self.rules_patrol_scan_minutes);
        // ReadAI672AE0 reaches673CFD..673D1D after AirstripLimit and
        // before Paranoid. Rules+17E3 survives the6686C0 Type reset;
        // ReadBool5295F0 retains its current byte on absent/invalid input.
        self.rules_comp_easy_bonus = section.read_bool("CompEasyBonus", self.rules_comp_easy_bonus);
    }

    pub(super) fn allocate_general_references(&mut self, pass: &IniFile) {
        const SITES: &[(&str, RulesTypeFamily, bool)] = &[
            ("DamageFireTypes", RulesTypeFamily::Animation, true),
            ("OreTwinkle", RulesTypeFamily::Animation, false),
            ("BarrelExplode", RulesTypeFamily::Animation, false),
            ("BarrelDebris", RulesTypeFamily::VoxelAnimation, true),
            ("BarrelParticle", RulesTypeFamily::ParticleSystem, false),
            ("NukeTakeOff", RulesTypeFamily::Animation, false),
            ("Wake", RulesTypeFamily::Animation, false),
            ("DropPod", RulesTypeFamily::Animation, true),
            ("DeadBodies", RulesTypeFamily::Animation, true),
            ("MetallicDebris", RulesTypeFamily::Animation, true),
            ("BridgeExplosions", RulesTypeFamily::Animation, true),
            ("IonBlast", RulesTypeFamily::Animation, false),
            ("IonBeam", RulesTypeFamily::Animation, false),
            ("WeatherConClouds", RulesTypeFamily::Animation, true),
            ("WeatherConBolts", RulesTypeFamily::Animation, true),
            ("WeatherConBoltExplosion", RulesTypeFamily::Animation, false),
            ("DominatorWarhead", RulesTypeFamily::Warhead, false),
            ("DominatorFirstAnim", RulesTypeFamily::Animation, false),
            ("DominatorSecondAnim", RulesTypeFamily::Animation, false),
            ("ChronoPlacement", RulesTypeFamily::Animation, false),
            ("ChronoBeam", RulesTypeFamily::Animation, false),
            ("ChronoBlast", RulesTypeFamily::Animation, false),
            ("ChronoBlastDest", RulesTypeFamily::Animation, false),
            ("WarpIn", RulesTypeFamily::Animation, false),
            ("WarpOut", RulesTypeFamily::Animation, false),
            ("WarpAway", RulesTypeFamily::Animation, false),
            ("IronCurtainInvokeAnim", RulesTypeFamily::Animation, false),
            ("ForceShieldInvokeAnim", RulesTypeFamily::Animation, false),
            ("WeaponNullifyAnim", RulesTypeFamily::Animation, false),
            ("ChronoSparkle1", RulesTypeFamily::Animation, false),
            ("InfantryExplode", RulesTypeFamily::Animation, false),
            ("FlamingInfantry", RulesTypeFamily::Animation, false),
            ("InfantryHeadPop", RulesTypeFamily::Animation, false),
            ("InfantryNuked", RulesTypeFamily::Animation, false),
            ("InfantryVirus", RulesTypeFamily::Animation, false),
            ("InfantryBrute", RulesTypeFamily::Animation, false),
            ("InfantryMutate", RulesTypeFamily::Animation, false),
            ("Behind", RulesTypeFamily::Animation, false),
            ("MoveFlash", RulesTypeFamily::Animation, false),
            ("Parachute", RulesTypeFamily::Animation, false),
            ("BombParachute", RulesTypeFamily::Animation, false),
            ("DropZoneAnim", RulesTypeFamily::Animation, false),
            ("EMPulseSparkles", RulesTypeFamily::Animation, false),
            ("LargeVisceroid", RulesTypeFamily::Vehicle, false),
            ("SmallVisceroid", RulesTypeFamily::Vehicle, false),
            ("DropPodWeapon", RulesTypeFamily::Weapon, false),
            (
                "ExplosiveVoxelDebris",
                RulesTypeFamily::VoxelAnimation,
                true,
            ),
            ("TireVoxelDebris", RulesTypeFamily::VoxelAnimation, false),
            ("ScrapVoxelDebris", RulesTypeFamily::VoxelAnimation, false),
            ("RepairBay", RulesTypeFamily::Building, true),
            ("GDIGateOne", RulesTypeFamily::Building, false),
            ("GDIGateTwo", RulesTypeFamily::Building, false),
            ("NodGateOne", RulesTypeFamily::Building, false),
            ("NodGateTwo", RulesTypeFamily::Building, false),
            ("WallTower", RulesTypeFamily::Building, false),
            ("Shipyard", RulesTypeFamily::Building, true),
            ("GDIPowerPlant", RulesTypeFamily::Building, false),
            ("NodRegularPower", RulesTypeFamily::Building, false),
            ("NodAdvancedPower", RulesTypeFamily::Building, false),
            ("ThirdPowerPlant", RulesTypeFamily::Building, false),
            ("PrerequisiteProcAlternate", RulesTypeFamily::Vehicle, false),
            ("BaseUnit", RulesTypeFamily::Vehicle, true),
            ("HarvesterUnit", RulesTypeFamily::Vehicle, true),
            ("PadAircraft", RulesTypeFamily::Aircraft, true),
            ("Paratrooper", RulesTypeFamily::Infantry, false),
            ("SecretInfantry", RulesTypeFamily::Infantry, true),
            ("SecretUnits", RulesTypeFamily::Vehicle, true),
            ("SecretBuildings", RulesTypeFamily::Building, true),
            ("AlliedDisguise", RulesTypeFamily::Infantry, false),
            ("SovietDisguise", RulesTypeFamily::Infantry, false),
            ("ThirdDisguise", RulesTypeFamily::Infantry, false),
            ("Engineer", RulesTypeFamily::Infantry, false),
            ("Technician", RulesTypeFamily::Infantry, false),
            ("Pilot", RulesTypeFamily::Infantry, false),
            ("AlliedCrew", RulesTypeFamily::Infantry, false),
            ("SovietCrew", RulesTypeFamily::Infantry, false),
            ("ThirdCrew", RulesTypeFamily::Infantry, false),
            ("AmerParaDropInf", RulesTypeFamily::Infantry, true),
            ("AllyParaDropInf", RulesTypeFamily::Infantry, true),
            ("SovParaDropInf", RulesTypeFamily::Infantry, true),
            ("YuriParaDropInf", RulesTypeFamily::Infantry, true),
            ("AnimToInfantry", RulesTypeFamily::Infantry, true),
            ("LightningWarhead", RulesTypeFamily::Warhead, false),
            ("PrismType", RulesTypeFamily::Building, false),
            ("V3RocketType", RulesTypeFamily::Aircraft, false),
            ("DMislType", RulesTypeFamily::Aircraft, false),
            ("CMislType", RulesTypeFamily::Aircraft, false),
            ("VeinholeTypeClass", RulesTypeFamily::Terrain, false),
            ("DefaultMirageDisguises", RulesTypeFamily::Terrain, true),
            // NPSE extension read (`0xC30FF8..0xC3101F`): the canopy an
            // airdropped non-infantry object takes. The extension's General
            // reader runs after the stock reads, so its FindOrAllocate lands
            // after every vanilla General pointer; every build reads it now
            // (below), marked as an NPatch read.
            ("VehicleParachute", RulesTypeFamily::Animation, false),
        ];

        // Original RA2 keeps 36 moved references in [AudioVisual], and a
        // layered pass may carry only that section with no [General]. In that
        // case continue with an empty General view so the AV keys still read;
        // all General reads return their retained defaults. YR and ordinary
        // RA2 data (which have a General section) take the unchanged path.
        let section = if crate::util::ra2_launch::enabled()
            && pass.section("General").is_none()
            && pass.section("AudioVisual").is_some()
        {
            crate::rules::ini_parser::IniSection::empty()
        } else {
            let Some(section) = pass.section("General") else {
                return;
            };
            section
        };
        let ares_way = crate::rules::ares_switch::ares_way();
        // RulesClass::ReadGeneral66D530, original66EB03..66EB1C:
        // ReadBool5295F0 uses the current byte; Type reset6686C0 retains it.
        self.rules_allow_shrouded_subterannean_moves = section.read_bool(
            "AllowShroudedSubteranneanMoves",
            self.rules_allow_shrouded_subterannean_moves,
        );
        // ReadGeneral0066FDB1..0066FDD9 / ReadINI_IntVector00475D70:
        // exact key, 512-byte string, comma tokens through CRT atoi; an
        // absent/empty read keeps the prior vector. Counts are raw frames.
        if let Some(delays) = section.read_int_list("AIHateDelays") {
            self.rules_ai_hate_delays = delays;
        }
        // ReadGeneral66EBD4..66EBEE: exact-case signed ReadInt5276D0,
        // current Rules+580 as default, no clamp. Ctor665D92 stores1.
        self.rules_maximum_cheer_rate =
            section.read_int("MaximumCheerRate", self.rules_maximum_cheer_rate);
        // ReadGeneral66EC08: ReadDouble with the current Rules+588 as default.
        if ares_way {
            let forest_fire = &mut self.ares_global_controls.forest_fire;
            forest_fire.tree_flammability =
                section.read_double_bits("TreeFlammability", forest_fire.tree_flammability);
        }
        // General66EC37/66EC57 read current RulesClass values as defaults.
        // Both survive empty/missing later passes and destructive Type resets.
        self.rules_missile_rot_var =
            section.read_double("MissileROTVar", self.rules_missile_rot_var);
        self.rules_safety_altitude =
            section.read_int("MissileSafetyAltitude", self.rules_safety_altitude);
        // Rules665650 +7AC starts-1; ReadGeneral66F2AF..66F2CF uses
        // previous fullsigned WindDirection as default, no clamp.
        self.rules_wind_direction = section.read_int("WindDirection", self.rules_wind_direction);
        // ReadGeneral66F30E..66F350: ReadInt5276D0, exact-case keys,
        // current +7B8/+7BC as defaults, in this order, without clamps.
        // Original: tools.native_inspect disasm0x66F30E --bytes66;
        // ctor0x666163 --bytes20 (supported retail gamemd).
        self.rules_parachute_max_fall_rate =
            section.read_int("ParachuteMaxFallRate", self.rules_parachute_max_fall_rate);
        self.rules_no_parachute_max_fall_rate = section.read_int(
            "NoParachuteMaxFallRate",
            self.rules_no_parachute_max_fall_rate,
        );
        // ReadGeneral670C13..670C6C: ReadInt5276D0 in side order, current
        // signed values as defaults, no clamp. Full_Init6876AC ->6686C0
        // resets Types, not these RulesClass fields. Original gamemd.
        for (value, key) in self.rules_survivor_divisors.iter_mut().zip([
            "AlliedSurvivorDivisor",
            "SovietSurvivorDivisor",
            "ThirdSurvivorDivisor",
        ]) {
            *value = section.read_int(key, *value);
        }
        // ReadGeneral670DE9/670E30/670E57: ReadInt, then ReadDouble twice,
        // current Rules+16D8/+16E8/+16F0 defaults, exact keys, no read clamps.
        // Rules66751A/667540 constructor defaults come from GeneralRules.
        self.rules_infantry_repair_step =
            section.read_int("IRepairStep", self.rules_infantry_repair_step);
        self.rules_unit_repair_rate =
            section.read_double_bits("URepairRate", self.rules_unit_repair_rate);
        self.rules_infantry_repair_rate =
            section.read_double_bits("IRepairRate", self.rules_infantry_repair_rate);
        // ReadGeneral67183C..67185D, Rules+760. Shared binary32 scanner,
        // optional percent scaling, then retained binary64; no type reset.
        self.rules_charge_to_drain_ratio =
            section.read_double_bits("ChargeToDrainRatio", self.rules_charge_to_drain_ratio);
        // ReadGeneral's Prism block (`0x0067114F..0x006711B8`), current values
        // as defaults; RulesClass keeps them across Full_Init's reset.
        self.rules_prism_support = self.rules_prism_support.read_pass(section);
        self.rules_spotlight.read_pass(section);
        // ReadGeneral671AF6..671B97; Process668EE8 reaches General before
        // TypeData668EF0. Keep current defaults on each admitted General pass.
        self.rules_threat_coefficient_defaults
            .read_defaults(section);
        // RulesCtor666DF8 starts TreeStrength at25. ReadGeneral671DD2..
        //671DEB retains the current signed dword; Process668EE8 reaches this
        // before TypeData668EF0. This same boundary handles General prepasses,
        // and RulesClass survives the destructive Type reset6686C0.
        self.rules_tree_strength = section.read_int("TreeStrength", self.rules_tree_strength);
        self.rules_talk_bubble_time = section.read_talk_bubble_time(self.rules_talk_bubble_time);
        // RulesClass::ReadGeneral66D530, original66FC36..66FC50:
        // current byte through ReadBool5295F0; Type reset retains this scalar.
        self.rules_attack_cursor_on_disguise = section.read_bool(
            "AttackCursorOnDisguise",
            self.rules_attack_cursor_on_disguise,
        );
        // ReadGeneral's DropPodHeight, DropPodSpeed and DropPodAngle, which
        // only the Ares DropPod launch reaches (`DropPodLocomotorRules`).
        if ares_way {
            self.ares_global_controls
                .drop_pod
                .locomotor
                .read_general(section);
        }

        // Ares 3.0 Buf_General (1005BB00, hook 0x66D55E) reads these lists
        // at ReadGeneral's start through ParseList (1005B340 and its other
        // instantiations): ReadString into the shared 0x800 buffer, comma
        // tokens, Find-only lookups that drop unknown and none tokens. Its
        // jump hooks then skip the original 0x80 FindOrAllocate reads
        // (0x66DB93, 0x66DD13, 0x66F34B, 0x66F589, 0x66F7C0, 0x66FA13,
        // 0x67062F). DefaultMirageDisguises has no skip hook: the original
        // read at 0x671D3E still follows and replaces a present list.
        if ares_way {
            for (key, family) in ARES_GENERAL_LISTS {
                let resolved = self.resolve_existing_list_from(section, key, family, 0x800);
                self.store_general_list(key, resolved);
            }
        }

        // Original RA2 stores these 36 type references under [AudioVisual];
        // YR moved the same keys into [General]. Values are identical, only
        // the section differs. Select the section per key so RA2 reads them,
        // while every other key and the whole YR path stay on [General].
        // (Derived from rules.ini [AudioVisual] vs rulesmd.ini [General].)
        const RA2_AUDIOVISUAL_MOVED_KEYS: &[&str] = &[
            "BarrelDebris",
            "BarrelExplode",
            "BarrelParticle",
            "Behind",
            "BombParachute",
            "BridgeExplosions",
            "ChronoBeam",
            "ChronoBlast",
            "ChronoBlastDest",
            "ChronoPlacement",
            "ChronoSparkle1",
            "DamageFireTypes",
            "DeadBodies",
            "DropPod",
            "DropZoneAnim",
            "EMPulseSparkles",
            "FlamingInfantry",
            "InfantryExplode",
            "InfantryHeadPop",
            "InfantryNuked",
            "IonBeam",
            "IonBlast",
            "IronCurtainInvokeAnim",
            "MetallicDebris",
            "MoveFlash",
            "NukeTakeOff",
            "OreTwinkle",
            "Parachute",
            "Wake",
            "WarpAway",
            "WarpIn",
            "WarpOut",
            "WeaponNullifyAnim",
            "WeatherConBoltExplosion",
            "WeatherConBolts",
            "WeatherConClouds",
        ];
        let ra2_av = crate::util::ra2_launch::enabled()
            .then(|| pass.section("AudioVisual"))
            .flatten();
        for &(key, family, is_list) in SITES {
            // RA2 reads the moved keys from [AudioVisual]; absent there, fall
            // back to [General] so a later pass / map override still works and
            // YR (no AV section read) is byte-for-byte unchanged.
            let src = if ra2_av.is_some() && RA2_AUDIOVISUAL_MOVED_KEYS.contains(&key) {
                ra2_av.unwrap_or(section)
            } else {
                section
            };
            // NPSE's General reader runs after the stock reads, so its
            // FindOrAllocate lands after every vanilla General pointer. Every
            // build reads it now, marked as an NPatch read, so the key turns
            // the per-game switch on (`npatch_switch`).
            let _npatch = (key == "VehicleParachute").then(crate::rules::ares_switch::npatch_reads);
            if ares_way
                && key != "DefaultMirageDisguises"
                && ARES_GENERAL_LISTS.iter().any(|&(name, _)| name == key)
            {
                continue;
            }
            if is_list {
                // Keep actual FindOrAllocate identities, duplicate slots and
                // empty/missing-layer retention. Null factory tokens are
                // excluded by this shared native list reader.
                let resolved = self.resolve_list_from(src, key, family, 0x80);
                self.store_general_list(key, resolved);
            } else if let Some(side) = ["AlliedDisguise", "SovietDisguise", "ThirdDisguise"]
                .iter()
                .position(|&name| name == key)
            {
                // Absent/empty retains the prior pointer; a factory none
                // token clears it. Reuse the existing allocating scalar reader.
                if let Some(resolved) = self.resolve_scalar_from(src, key, family, 0x80) {
                    self.general_type_lists.infantry_disguises[side] = resolved;
                }
            } else if matches!(
                key,
                "LightningWarhead"
                    | "IonBlast"
                    | "IonBeam"
                    | "WeatherConBoltExplosion"
                    | "WeaponNullifyAnim"
                    | "PrismType"
                    | "DropZoneAnim"
                    | "Behind"
                    | "PrerequisiteProcAlternate"
                    | "GDIGateOne"
                    | "GDIGateTwo"
                    | "NodGateOne"
                    | "NodGateTwo"
                    | "WallTower"
                    | "GDIPowerPlant"
                    | "NodRegularPower"
                    | "NodAdvancedPower"
                    | "ThirdPowerPlant"
            ) {
                // General671053/66DF19/66E2AF: empty ReadString128 retains the
                // current pointer; exact none clears it through the factory.
                // PrismType's reader (`0x0067BCE0`, called at `0x00671144`),
                // the gate/WallTower reads (`0x0066F450..0x0066F583`) and the
                // power plant reads (`0x0066F692..0x0066F781`) do the same
                // through BuildingType's FindOrAllocate (`0x004653C0`), and
                // PrerequisiteProcAlternate (`0x0066F787..0x0066F7C9`) through
                // UnitType's (`0x007480D0`).
                if let Some(resolved) = self.resolve_scalar_from(src, key, family, 0x80) {
                    let resolved = resolved.unwrap_or_default();
                    match key {
                        "LightningWarhead" => self.select_anim.lightning_warhead = resolved,
                        //66DCC7..66DD0D: ReadString128/Anim428B80, current
                        //pointer retained for missing/empty; native none clears.
                        "IonBlast" => self.rules_ion_blast_anim = resolved,
                        "IonBeam" => self.rules_ion_beam_anim = resolved,
                        "WeatherConBoltExplosion" => {
                            self.select_anim.weather_con_bolt_explosion = resolved;
                        }
                        "WeaponNullifyAnim" => self.select_anim.weapon_nullify_anim = resolved,
                        "DropZoneAnim" => {
                            self.rules_drop_zone_anim = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "Behind" => {
                            self.rules_behind = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "PrismType" => {
                            self.rules_prism_type = Some(resolved).filter(|id| !id.is_empty());
                        }
                        "PrerequisiteProcAlternate" => {
                            self.rules_prerequisite_proc_alternate =
                                Some(resolved).filter(|id| !id.is_empty());
                        }
                        _ => {
                            let types = &mut self.rules_building_types;
                            let slot = match key {
                                "GDIGateOne" => &mut types.gdi_gate_one,
                                "GDIGateTwo" => &mut types.gdi_gate_two,
                                "NodGateOne" => &mut types.nod_gate_one,
                                "NodGateTwo" => &mut types.nod_gate_two,
                                "WallTower" => &mut types.wall_tower,
                                "GDIPowerPlant" => &mut types.gdi_power_plant,
                                "NodRegularPower" => &mut types.nod_regular_power,
                                "NodAdvancedPower" => &mut types.nod_advanced_power,
                                _ => &mut types.third_power_plant,
                            };
                            *slot = Some(resolved).filter(|id| !id.is_empty());
                        }
                    }
                }
            } else if let Some(missile) = [
                ("V3RocketType", MissileFamily::V3Rocket),
                ("DMislType", MissileFamily::DMisl),
                ("CMislType", MissileFamily::CMisl),
            ]
            .into_iter()
            .find_map(|(name, missile)| (name == key).then_some(missile))
            {
                // ReadGeneral's rocket blocks end with their type, read by
                // `0x0067BD30` (`0x006713B0`, `0x0067156A`, `0x00671723`):
                // ReadString128, AircraftType's factory, default the current
                // pointer.
                if let Some(resolved) = self.resolve_scalar_from(src, key, family, 0x80) {
                    self.missile_spawn.set_type(missile, resolved);
                }
            } else {
                let resolved = self.resolve_scalar_from(src, key, family, 0x80);
                // ReadGeneral's DropPodWeapon (Rules+0x5A4), for the Ares
                // DropPod locomotor (`DropPodLocomotorRules`).
                if ares_way && key == "DropPodWeapon" {
                    self.ares_global_controls
                        .drop_pod
                        .locomotor
                        .read_weapon(resolved.clone());
                }
                self.rules_type_references.read_pointer(key, resolved);
            }
        }
    }

    /// Store one `[General]` type list's read: `None` (absent key) keeps the
    /// retained vector, a present read replaces it.
    fn store_general_list(&mut self, key: &str, resolved: Option<Vec<String>>) {
        // ReadGeneral's DropPod= (Rules+0x108), the landed pod's anims, for
        // the Ares DropPod locomotor (`DropPodLocomotorRules`).
        if crate::rules::ares_switch::ares_way() && key == "DropPod" {
            self.ares_global_controls
                .drop_pod
                .locomotor
                .read_anims(resolved);
            return;
        }
        let lists = &mut self.general_type_lists;
        let slot = match key {
            "MetallicDebris" => &mut lists.metallic_debris,
            "BridgeExplosions" => &mut lists.bridge_explosions,
            "WeatherConClouds" => &mut lists.weather_con_clouds,
            "WeatherConBolts" => &mut lists.weather_con_bolts,
            "AnimToInfantry" => &mut lists.anim_to_infantry,
            "DefaultMirageDisguises" => &mut lists.default_mirage_disguises,
            "SecretInfantry" => &mut lists.secret_lab_choices[0],
            "SecretUnits" => &mut lists.secret_lab_choices[1],
            "SecretBuildings" => &mut lists.secret_lab_choices[2],
            _ => match PARADROP_INFANTRY_KEYS.iter().position(|&name| name == key) {
                Some(side) => &mut lists.paradrop_infantry[side],
                None => {
                    self.rules_type_references.read_list(key, resolved);
                    return;
                }
            },
        };
        if let Some(resolved) = resolved {
            *slot = resolved;
        }
    }
}

impl RulesPassProcessor {
    pub(super) fn allocate_combat_references(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("CombatDamage") else {
            return;
        };
        // RulesCtor665675=100; CombatDamage66BBDE..66BBF5 precedes IonCannonDamage.
        self.rules_ammo_crate_damage =
            section.read_int("AmmoCrateDamage", self.rules_ammo_crate_damage);
        // CombatDamage66BBF8..66BC12: exact-case signed ReadInt5276D0,
        // current Rules+490 default, constructor665CD4=700, no clamp.
        self.rules_ion_cannon_damage =
            section.read_int("IonCannonDamage", self.rules_ion_cannon_damage);
        // RulesClass::ReadCombatDamage @ 0x0066BBB0, original instructions
        //66BC37..66BC51: current signed Rules+568,
        // ReadInt5276D0, with no clamp; it precedes the Scorches factories.
        self.rules_tiberium_explosion_damage = section.read_int(
            "TiberiumExplosionDamage",
            self.rules_tiberium_explosion_damage,
        );
        // Ares 3.0 Buf_CombatDamage (1005BA50, hook 0x66BC71) reads the five
        // Scorches lists and SplashList in place through ParseList (shared
        // 0x800 buffer, Find-only) and skips to 0x66C287. A Find-only Scorches
        // read allocates nothing, and no VERA consumer reads the lists.
        let ares_way = crate::rules::ares_switch::ares_way();
        if !ares_way {
            for key in [
                "Scorches",
                "Scorches1",
                "Scorches2",
                "Scorches3",
                "Scorches4",
            ] {
                self.allocate_list_from(section, key, RulesTypeFamily::Smudge, 0x80);
            }
        }
        // CombatDamage66C184..66C287: ctor-empty vector, nonempty
        // ReadString128 replaces it using untrimmed strtok tokens/factories.
        let splash_list = if ares_way {
            self.resolve_existing_list_from(
                section,
                "SplashList",
                RulesTypeFamily::Animation,
                0x800,
            )
        } else {
            self.resolve_list_from(section, "SplashList", RulesTypeFamily::Animation, 0x80)
        };
        if let Some(list) = splash_list {
            self.select_anim.splash_list = list;
        }
        // ReadCombatDamage66C2A1..66C35D, in original read order.
        // RulesTypeReferences retains each consumed pointer; FlameDamage
        // still allocates its Type without a published consumer.
        for key in ["FlameDamage", "FlameDamage2", "C4Warhead", "CrushWarhead"] {
            let warhead = self.resolve_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80);
            self.rules_type_references.read_pointer(key, warhead);
        }
        // 0x0066C36A..0x0066C4F4: the six rocket warheads, in this order.
        let mut missile_spawn = std::mem::take(&mut self.missile_spawn);
        missile_spawn.read_warheads(|key| {
            self.resolve_scalar_from(section, key, RulesTypeFamily::Warhead, 0x80)
        });
        self.missile_spawn = missile_spawn;
        let warhead =
            self.resolve_scalar_from(section, "IvanWarhead", RulesTypeFamily::Warhead, 0x80);
        self.rules_type_references
            .read_pointer("IvanWarhead", warhead);
        if let Some(weapon) =
            self.resolve_scalar_from(section, "DeathWeapon", RulesTypeFamily::Weapon, 0x80)
        {
            self.rules_death_weapon = weapon;
        }
        //66C914..66C92E: current signed default, no clamp, before DrainAnimationType.
        self.rules_mind_control_attack_line_frames = section.read_int(
            "MindControlAttackLineFrames",
            self.rules_mind_control_attack_line_frames,
        );
        let anim = self.resolve_scalar_from(
            section,
            "DrainAnimationType",
            RulesTypeFamily::Animation,
            0x80,
        );
        self.rules_type_references
            .read_pointer("DrainAnimationType", anim);
        // RulesClass::ReadCombatDamage66BBB0, original66C973..66C9AC:
        // signed ReadInt5276D0, current defaults, no clamps, in this order.
        self.rules_drain_money_frame_delay =
            section.read_int("DrainMoneyFrameDelay", self.rules_drain_money_frame_delay);
        self.rules_drain_money_amount =
            section.read_int("DrainMoneyAmount", self.rules_drain_money_amount);
        for key in ["ControlledAnimationType", "PermaControlledAnimationType"] {
            let anim = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
            self.rules_type_references.read_pointer(key, anim);
        }
        //66CA96..66CACB retains the current Rules+FF0 pointer on an
        //empty read; the existing allocating scalar reader handles none.
        if let Some(resolved) =
            self.resolve_scalar_from(section, "IonCannonWarhead", RulesTypeFamily::Warhead, 0x80)
        {
            self.rules_ion_cannon_warhead = resolved.unwrap_or_default();
        }
        for (key, _) in DEFAULT_PARTICLE_SYSTEMS {
            let system =
                self.resolve_scalar_from(section, key, RulesTypeFamily::ParticleSystem, 0x80);
            self.rules_type_references.read_pointer(key, system);
        }
    }

    pub(super) fn allocate_radiation_references(&mut self, pass: &IniFile) {
        if let Some(section) = pass.section("Radiation") {
            let warhead =
                self.resolve_scalar_from(section, "RadSiteWarhead", RulesTypeFamily::Warhead, 0x80);
            self.rules_type_references
                .read_pointer("RadSiteWarhead", warhead);
        }
    }

    /// Phobos `RadTypeClass::LoadFromINIList` + each type's `LoadFromINI`,
    /// reached on every Rules pass: the pass's `[RadiationTypes]` list
    /// appends entries and every retained type re-reads its own section.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_radiation_types(&mut self, pass: &IniFile) {
        if let Some(catalog) = self.phobos_radiation_types.as_mut() {
            catalog.apply_pass(pass);
        }
    }

    /// Phobos `ShieldTypeClass::LoadFromINIList` + each type's `LoadFromINI`.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_shield_types(&mut self, pass: &IniFile) {
        if let Some(catalog) = self.phobos_shield_types.as_mut() {
            catalog.apply_pass(pass);
        }
    }

    /// Phobos `InsigniaTypeClass::LoadFromINIList` + each type's
    /// `LoadFromINI`, reached on every Rules pass.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_insignia_types(&mut self, pass: &IniFile) {
        if let Some(catalog) = self.phobos_insignia_types.as_mut() {
            catalog.apply_pass(pass);
        }
    }

    /// Phobos `DigitalDisplayTypeClass::LoadFromINIList` + each type's
    /// `LoadFromINI`, reached on every Rules pass.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_digital_display_types(&mut self, pass: &IniFile) {
        if let Some(catalog) = self.phobos_digital_display_types.as_mut() {
            catalog.apply_pass(pass);
        }
    }

    /// The `Read<true>` type factories of `AttachEffectTypeClass::LoadFromINI`
    /// (`ExpireWeapon`, `RevengeWeapon`, `ReflectDamage.Warhead`), run ahead of
    /// the type-body pass so those types are constructed and read this pass.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_attach_effect_references(&mut self, pass: &IniFile) {
        let Some(list) = pass.section("AttachEffectTypes") else {
            return;
        };
        for key in list.physical_keys() {
            let Some(name) = list.read_name(key, 2048) else {
                continue;
            };
            let section = pass.section(name).or_else(|| {
                pass.section_names()
                    .into_iter()
                    .find(|candidate| candidate.eq_ignore_ascii_case(name))
                    .and_then(|candidate| pass.section(candidate))
            });
            let Some(section) = section else {
                continue;
            };
            for weapon_key in ["ExpireWeapon", "RevengeWeapon"] {
                if let Some(weapon) = section.read_type_name(weapon_key, 0x100) {
                    self.find_or_allocate(RulesTypeFamily::Weapon, weapon);
                }
            }
            if let Some(warhead) = section.read_type_name("ReflectDamage.Warhead", 0x100) {
                self.find_or_allocate(RulesTypeFamily::Warhead, warhead);
            }
        }
    }

    /// Phobos `AttachEffectTypeClass::LoadFromINIList` + each type's
    /// `LoadFromINI`, reached on every Rules pass.
    #[cfg(feature = "phobos")]
    pub(super) fn allocate_attach_effect_types(&mut self, pass: &IniFile) {
        let Some(mut catalog) = self.phobos_attach_effect_types.take() else {
            return;
        };
        catalog.apply_pass(pass);
        // Phobos `Valueable<WeaponTypeClass*>::Read<true>` (`ExpireWeapon`,
        // `RevengeWeapon`) and `Nullable<WarheadTypeClass*>` (`ReflectDamage.
        // Warhead`) call their factories while the type's section is read, so
        // this pass's type-body reads reach types only these keys name.
        for entry in catalog.types() {
            for name in [
                entry.expire_weapon.as_deref(),
                entry.revenge_weapon.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                self.find_or_allocate(RulesTypeFamily::Weapon, name);
            }
            if let Some(name) = entry.reflect_damage_warhead.as_deref() {
                self.find_or_allocate(RulesTypeFamily::Warhead, name);
            }
        }
        self.phobos_attach_effect_types = Some(catalog);
    }

    pub(super) fn allocate_audio_visual_references(
        &mut self,
        pass: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        use crate::util::native_x87::MaskedX87Chop53 as X87;

        let Some(section) = pass.section("AudioVisual") else {
            return;
        };
        let ares_way = crate::rules::ares_switch::ares_way();
        // Original 66920D..669258 reads these first, including the cold
        // startup's same AudioVisual call before any complete Process.
        self.rules_detail = self.rules_detail.read_pass(section);
        self.rules_global_sounds.read_pass(pass, fixed_sounds);
        // gamemd.exe ReadAudioVisual66A68E/66A693: current byte default.
        self.rules_shroud_grow = section.read_bool("ShroudGrow", self.rules_shroud_grow);
        // AudioVisual66B385..66B39E: ReadRange474620, current +14BC default.
        // Rules6672FF constructs1024 leptons; literal -1 retains the prior.
        self.rules_drop_zone_radius =
            section.read_range("DropZoneRadius", self.rules_drop_zone_radius);
        // Full AudioVisual6691E0's 66B3C4 read uses the retained signed dword
        // default. Cold startup52D132 calls the same reader before Process.
        self.rules_gravity = section.read_int("Gravity", self.rules_gravity);
        // ReadAudioVisual66B4CA..66B4E4: retain the current double across
        // Process and Type resets; authored input is scanned as f32 then widened.
        self.rules_shroud_rate = section.read_double_bits("ShroudRate", self.rules_shroud_rate);
        self.rules_fog_rate = section.read_double_bits("FogRate", self.rules_fog_rate);
        // AudioVisual66B5D7..66B625: rate then step; both ReadDouble defaults
        // are the current Rules fields. Missing reads retain; malformed float
        // output is native undefined stack data, using ini_value's safe policy.
        // Original physical/retained controls: trigger_map_lighting.json (+meta).
        self.rules_ambient_change_rate =
            section.read_double_bits("AmbientChangeRate", self.rules_ambient_change_rate);
        self.rules_ambient_change_step =
            section.read_double_bits("AmbientChangeStep", self.rules_ambient_change_step);
        // Original ReadAudioVisual66B673..66B74E divides each current
        // signed integer by1000 BEFORE widening the ReadDouble default.
        // Every reached section performs this conversion, even if a key
        // is omitted. Math_ftol7C5F00 stores the low32 bits after *1000;
        // unlike the Building light reader, there is no added0.1 bias.
        for (key, current) in ["ExtraUnitLight", "ExtraInfantryLight", "ExtraAircraftLight"]
            .into_iter()
            .zip(&mut self.rules_class_extra_lights)
        {
            let value = section.read_double(key, f64::from(*current / 1000));
            *current = X87::ftol_i32_low_masked(X87::mul(
                X87::load_f64(NativeF64Bits::from_bits(value.to_bits())),
                X87::load_i32(1000),
            ));
        }
        // AudioVisual66B77D..66B7A7, after Gravity, retains the RGB default.
        self.rules_line_trail_override = section.read_color_rgb(
            "LineTrailColorOverride",
            self.rules_line_trail_override,
            None,
        );
        // AudioVisual66B7A7..66B7D2 reads this next, even without General.
        // Its default is the current unsigned RGB; complete scans narrow to
        // bytes. Incomplete scans retain ini_value's deterministic safe policy.
        self.rules_chrono_beam_color =
            section.read_color_rgb("ChronoBeamColor", self.rules_chrono_beam_color, None);
        self.rules_color_add_selectors.read_pass(section);
        for key in ["DropPodPuff", "VeinAttack", "Dig", "AtmosphereEntry"] {
            let resolved = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
            if key == "Dig" {
                self.rules_type_references
                    .read_pointer(key, resolved.clone());
            }
            // DropPodPuff (Rules+0x4C) and AtmosphereEntry (+0x354), for the
            // Ares DropPod locomotor (`DropPodLocomotorRules`).
            if ares_way {
                self.ares_global_controls
                    .drop_pod
                    .locomotor
                    .read_audio_visual(key, resolved);
            }
        }
        let tree_fire =
            self.resolve_list_from(section, "TreeFire", RulesTypeFamily::Animation, 0x80);
        if ares_way && let Some(tree_fire) = tree_fire {
            self.ares_global_controls.forest_fire.tree_fire = tree_fire;
        }
        // ReadAudioVisual66B0E4..66B1CF: ReadString128 and comma-only
        // strtok replace OnFire with the resolved Anim pointers. Empty
        // reads retain the vector; null tokens are omitted without padding.
        let on_fire = self.resolve_list_from(section, "OnFire", RulesTypeFamily::Animation, 0x80);
        self.rules_type_references.read_list("OnFire", on_fire);
        let _ = self.resolve_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        let _ = self.resolve_scalar_from(section, "Smoke", RulesTypeFamily::Animation, 0x80);
        for key in ["SmallFire", "LargeFire"] {
            let _ = self.resolve_scalar_from(section, key, RulesTypeFamily::Animation, 0x80);
        }
        // Firestorm 2.0's WebbedInfantry (`docs/ts-webs.md`): ReadString into
        // 0x80 bytes and AnimType FindOrMake (`0x005D39A0`), an empty value
        // keeping the pointer. gamemd has no such read, so VERA reads it after
        // every AudioVisual key gamemd reads: an INI without the key
        // allocates nothing.
        let webbed =
            self.resolve_scalar_from(section, "WebbedInfantry", RulesTypeFamily::Animation, 0x80);
        self.rules_type_references
            .read_pointer("WebbedInfantry", webbed);
    }

    pub(super) fn process_special_weapons(&mut self, pass: &IniFile) {
        let Some(section) = pass.section("SpecialWeapons") else {
            return;
        };
        for (key, family) in [
            ("NukeWarhead", RulesTypeFamily::Warhead),
            ("NukeProjectile", RulesTypeFamily::Projectile),
            ("NukeDown", RulesTypeFamily::Projectile),
            ("MutateWarhead", RulesTypeFamily::Warhead),
            ("MutateExplosionWarhead", RulesTypeFamily::Warhead),
            ("EMPulseWarhead", RulesTypeFamily::Warhead),
            ("EMPulseProjectile", RulesTypeFamily::Projectile),
        ] {
            // ReadSpecialWeapons @ 0x00668FB0 keeps EMPulseWarhead at +0xFA0;
            // retain that typed ID through RulesTypeReferences. NukeWarhead,
            // NukeProjectile, NukeDown and EMPulseProjectile remain
            // allocation-only; no gameplay effect is claimed for this state.
            let resolved = self.resolve_scalar_from(section, key, family, 0x80);
            self.rules_type_references.read_pointer(key, resolved);
        }
        self.process_warhead_family(pass);
    }
}
