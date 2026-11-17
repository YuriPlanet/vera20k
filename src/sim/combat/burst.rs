//! Retained TechnoClass firing indices, independent of Target and its orders.

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[cfg(feature = "phobos")]
use crate::rules::ruleset::RuleSet;
#[cfg(feature = "phobos")]
use crate::sim::world::Simulation;

/// Techno+3B8: ctor6F2F01 initializes zero; FireAt6FF274 increments before
/// GetROF and6FF2C5 stores the signed remainder afterwards. Target assignment
/// to another non-null target does not reset it. Assign_Target6FCF5B clears
/// it on a changed assignment to null (the same-target early exit does not).
/// A spawn manager's missile launch whose GetWeapon(0) has `Burst > 1` sets
/// it to the slot's parity around its GetFLH and stores 0 after the child's
/// Unlimbo (`0x006B73F6`, `0x006B7585`).
/// Unit ReceiveGunner/RemoveGunner increment the receiving owner's index
/// before GetROF without applying a remainder (`0x00746493`, `0x0074656C`).
/// This owner replaces the old target-owned remaining-shot count. FLH and
/// the next GetROF branch both read this retained index.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct WeaponBurst {
    index: i32,
    /// Techno+43C: constructor6F3046 initializes zero. FireAt6FEADA reads
    /// this independent fan position, then6FEBC5..6FEBDC increments/wraps
    /// it. Assign_Target and the ordinary burst's remainder do not reset it.
    #[serde(default)]
    radial_index: i32,
    /// Phobos `TechnoExt::ForceFullRearmDelay` (`Ext/Techno/Body.h:47`,
    /// constructor false at `Body.h:108`): the `Burst.FireWithinSequence`
    /// overrun arm sets it (`Ext/Infantry/Hooks.Firing.cpp:129`) so the current
    /// shot arms the full rearm (`0x6FD054` skips the mid-burst branch) and the
    /// shot's burst step clears it and resets the index (`0x6FF660` logic).
    #[cfg(feature = "phobos")]
    #[serde(default)]
    force_full_rearm: bool,
    /// Phobos `TechnoExt::LastRearmWasFullDelay` (`Ext/Techno/Body.h:48`,
    /// constructor false at `Body.h:109`, saved at `Body.cpp:1172`): the
    /// arm GetROF answered, written by the same `0x6FD054` hook that reads
    /// `force_full_rearm` — false on entry, true only on the ordinary
    /// full-rearm arm — and by `0x6FD0B5` on that arm. Above `0x6FD054` the
    /// early returns keep it. The charge-turret AttachEffect reader is
    /// [`crate::sim::game_entity::GameEntity::phobos_scale_charge_turret_delay`].
    #[cfg(feature = "phobos")]
    #[serde(default)]
    last_rearm_was_full_delay: bool,
}

impl Hash for WeaponBurst {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        // Preserve established hashes when the additional retained state is
        // zero; no fan history then distinguishes this object.
        if self.radial_index != 0 {
            b"radial-fire-index".hash(state);
            self.radial_index.hash(state);
        }
        #[cfg(feature = "phobos")]
        if self.force_full_rearm {
            b"force-full-rearm".hash(state);
        }
        #[cfg(feature = "phobos")]
        if self.last_rearm_was_full_delay {
            b"last-rearm-was-full-delay".hash(state);
        }
    }
}

impl WeaponBurst {
    pub(crate) fn index(self) -> i32 {
        self.index
    }

    pub(crate) fn next_index(self) -> i32 {
        self.index.wrapping_add(1)
    }

    /// Store the signed INC before GetROF. Gunner handover keeps this value;
    /// a completed FireAt also applies its weapon's remainder below.
    pub(crate) fn advance(&mut self) -> i32 {
        self.index = self.next_index();
        self.index
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn radial_index(self) -> i32 {
        self.radial_index
    }

    /// Consume the positive RadialFireSegments fan at6FEAC5..6FEBE6. This
    /// occurs before pitch/launch can fail, independently of complete_shot.
    /// Preserve signed INC and CMP, including out-of-range loaded counters.
    pub(crate) fn take_radial_index(&mut self, segments: i32) -> i32 {
        let index = self.radial_index;
        if segments > 0 {
            let next = index.wrapping_add(1);
            self.radial_index = if next >= segments { 0 } else { next };
        }
        index
    }

    /// Store 0: Assign_Target's null arm (`0x006FCF5B`) and a missile launch
    /// that set the index to its slot's parity (`SpawnManagerClass::AI`,
    /// `0x006B7585`).
    pub(crate) fn reset(&mut self) {
        self.index = 0;
    }

    /// A plain store of `TechnoClass+3B8`, without the increment: Phobos
    /// `AircraftExt::FireWeapon` writes `CurrentBurstIndex = bombDropCount % 2
    /// == 0` before a single-shot strafing shot
    /// (`Ext/Aircraft/Body.cpp:26-27`).
    pub(crate) fn set_index(&mut self, index: i32) {
        self.index = index;
    }

    pub(crate) fn complete_shot(&mut self, burst: i32) {
        // Native faults on Burst=0. Aircraft never calls FireAt for that
        // value; the ordinary caller retains its existing max(1) policy.
        self.index = self.advance().wrapping_rem(burst);
        // Phobos `TechnoClass_FireAt_LateLogic` (`0x6FF660`,
        // `Ext/Techno/Hooks.Firing.cpp:902-910`): a forced full rearm also
        // restarts the burst, so the next cycle begins at its first shot.
        #[cfg(feature = "phobos")]
        if self.force_full_rearm {
            self.force_full_rearm = false;
            self.index = 0;
        }
    }

    /// Phobos `TechnoExt::ForceFullRearmDelay`; read by GetROF's `0x6FD054`
    /// full-rearm bypass.
    #[cfg(feature = "phobos")]
    pub(crate) const fn force_full_rearm(self) -> bool {
        self.force_full_rearm
    }

    /// The `Burst.FireWithinSequence` overrun arm's set.
    #[cfg(feature = "phobos")]
    pub(crate) fn set_force_full_rearm(&mut self) {
        self.force_full_rearm = true;
    }

    /// Phobos `TechnoExt::LastRearmWasFullDelay`: the arm the last GetROF
    /// answered. Its writer is [`Self::record_rearm_arm`].
    #[cfg(feature = "phobos")]
    pub(crate) const fn last_rearm_was_full_delay(self) -> bool {
        self.last_rearm_was_full_delay
    }

    /// Store the flag write of the GetROF arm that answered
    /// (`Ext/Techno/Hooks.Firing.cpp:1124`, `:1139`). An arm above `0x6FD054`
    /// writes nothing, so [`RearmArm::Untouched`] keeps the previous value.
    /// The flag itself is Phobos state (`TechnoExt`), so a build without that
    /// feature records nothing.
    pub(crate) fn record_rearm_arm(&mut self, arm: crate::sim::combat::rof::RearmArm) {
        #[cfg(feature = "phobos")]
        if let Some(full) = arm.last_rearm_was_full_delay() {
            self.last_rearm_was_full_delay = full;
        }
        #[cfg(not(feature = "phobos"))]
        let _ = arm;
    }
}

/// Phobos `TechnoExt::UpdateRecountBurst` (`Ext/Techno/Body.Update.cpp:544-566`),
/// called from `OnEarlyUpdate` (`:22`): with `RecountBurst` enabled, no
/// target, a retained burst index and the last weapon's
/// `LastFireBulletFrame + max(ROF, 30)` elapsed, the index resets and a
/// positive remaining ROF re-arms the shot. No RNG, attachment or detach is
/// involved.
#[cfg(feature = "phobos")]
pub(crate) fn update_phobos_recount_burst(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let frame = sim.session.binary_frame() as i32;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    if !obj
        .phobos_recount_burst
        .unwrap_or(rules.phobos_globals.recount_burst)
    {
        return;
    }
    let index = entity.weapon_burst.index();
    if index == 0 || entity.attack_target.target().is_some() {
        return;
    }
    let Some(last_ref) = entity.phobos_last_weapon_type else {
        return;
    };
    let Some(weapon) = rules.weapon(sim.interner.resolve(last_ref)) else {
        return;
    };
    if weapon.burst == 0 {
        return;
    }
    let threshold = weapon.rof.max(30);
    if entity
        .last_fire_frame
        .frame()
        .wrapping_add(i64::from(threshold))
        > i64::from(frame)
    {
        return;
    }
    let ratio = f64::from(index) / f64::from(weapon.burst);
    let rof = (ratio * f64::from(weapon.rof) * entity.phobos_rof_multiplier()) as i32;
    let rof = rof.wrapping_sub(threshold);
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if rof > 0 {
        entity.rearm_after_fire(frame, rof);
    }
    entity.weapon_burst.reset();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::concrete_effects::represented_assign_target;

    #[test]
    fn radial_index_matches_original_signed_increment_without_changing_burst() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/radial_launch.json",
        ))
        .unwrap();
        for row in rows {
            let index = row["radial_index"].as_i64().unwrap() as i32;
            let mut state = WeaponBurst {
                index: 7,
                radial_index: index,
                #[cfg(feature = "phobos")]
                force_full_rearm: false,
            };
            assert_eq!(
                state.take_radial_index(row["radial_segments"].as_i64().unwrap() as i32),
                index
            );
            assert_eq!(
                state.radial_index(),
                row["radial_next"].as_i64().unwrap() as i32
            );
            assert_eq!(state.index(), 7);
        }
    }

    #[test]
    fn burst_index_matches_original_signed_increment_and_remainder() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/techno_burst_index.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 54);
        for row in rows {
            let mut burst = WeaponBurst {
                index: row["index"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            assert_eq!(
                burst.next_index(),
                row["get_rof_indices"][0].as_i64().unwrap() as i32
            );
            burst.complete_shot(row["burst"].as_i64().unwrap() as i32);
            assert_eq!(
                burst.index(),
                row["next_index"].as_i64().unwrap() as i32,
                "{row}"
            );
        }
    }

    #[test]
    fn player_orders_cancel_the_previous_burst_through_target_assignment() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::command::Command;
        use crate::sim::pathfinding::PathGrid;
        use crate::sim::world::Simulation;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=200\nSpeed=6\nPrimary=Gun\n\
             [Gun]\nDamage=10\nROF=100\nRange=8\nBurst=2\n",
        ))
        .unwrap();
        let grid = PathGrid::new(32, 32);
        for order in 0..4 {
            let mut sim = Simulation::with_seed(0);
            sim.install_fixture_path_grid(Some(&grid));
            let id = sim
                .spawn_object("TANK", "Americans", 4, 4, 64, &rules)
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            represented_assign_target(entity, Some(TargetKind::Cell(5, 4)));
            entity
                .passive_scan
                .set_passively_acquired_target_for_test(true);
            entity.weapon_burst.complete_shot(2);
            assert_eq!(entity.weapon_burst.index(), 1);
            let command = match order {
                0 => Command::Stop { entity_id: id },
                1 => Command::Move {
                    entity_id: id,
                    target_rx: 8,
                    target_ry: 4,
                },
                2 => Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(crate::sim::combat::TargetKind::Cell(8, 4)),
                },
                _ => Command::Guard {
                    entity_id: id,
                    target: None,
                },
            };
            assert!(sim.apply_command("Americans", &command, Some(&rules)));
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert!(entity.attack_target.target().is_none(), "{command:?}");
            assert!(!entity.passive_scan.acquired(), "{command:?}");
            // Ares TechnoClass_SetTarget_Burst (1005D320, hook 0x6FCF53)
            // keeps the index through the NULL assignment.
            let reset = if cfg!(feature = "ares") { 1 } else { 0 };
            assert_eq!(entity.weapon_burst.index(), reset, "{command:?}");
            // A later non-null assignment preserves the reset value, so FLH
            // and GetROF start the new burst at its first shot.
            represented_assign_target(entity, Some(TargetKind::Cell(6, 4)));
            assert_eq!(entity.weapon_burst.index(), reset, "{command:?}");
        }
    }

    #[test]
    fn special_orders_use_target_setter_after_admission() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::command::Command;
        use crate::sim::world::Simulation;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=APC\n\
             [InfantryTypes]\n0=TANY\n1=ENGINEER\n\
             [BuildingTypes]\n0=DEPOT\n1=ENEMY\n2=BUNKER\n\
             [TANK]\nStrength=200\nSpeed=6\nPrimary=Gun\nTurret=yes\n\
             [APC]\nStrength=200\nPassengers=5\nSizeLimit=2\n\
             Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
             [TANY]\nStrength=100\nSpeed=4\nC4=yes\nPrimary=Gun\n\
             [ENGINEER]\nStrength=100\nSpeed=4\nEngineer=yes\n\
             [DEPOT]\nStrength=1000\nUnitRepair=yes\n\
             [ENEMY]\nStrength=1000\nCapturable=yes\nCanC4=yes\n\
             [BUNKER]\nStrength=1000\nBunker=yes\n\
             [Gun]\nDamage=10\nROF=100\nRange=8\nBurst=2\n",
        ))
        .unwrap();
        for order in 0..5 {
            for had_target in [false, true] {
                let mut sim = Simulation::with_seed(0);
                let actor_type = match order {
                    1 | 2 => "TANY",
                    3 => "ENGINEER",
                    _ => "TANK",
                };
                let destination_type = match order {
                    0 => "DEPOT",
                    1 => "APC",
                    2 | 3 => "ENEMY",
                    _ => "BUNKER",
                };
                let id = sim
                    .spawn_object(actor_type, "Americans", 4, 4, 64, &rules)
                    .unwrap();
                let destination = sim
                    .spawn_object(
                        destination_type,
                        if matches!(order, 2 | 3) {
                            "Russians"
                        } else {
                            "Americans"
                        },
                        8,
                        4,
                        64,
                        &rules,
                    )
                    .unwrap();
                let actor = sim.substrate.entities.get_mut(id).unwrap();
                actor.health.set_for_test(actor.health.current() / 2); // Repair admission requires damage.
                if had_target {
                    represented_assign_target(actor, Some(TargetKind::Cell(6, 4)));
                }
                actor
                    .passive_scan
                    .set_passively_acquired_target_for_test(true);
                actor.weapon_burst.complete_shot(2);
                let command = match order {
                    2 => Command::PlantC4 {
                        attacker_id: id,
                        target_building_id: destination,
                    },
                    3 => Command::CaptureBuilding {
                        engineer_id: id,
                        target_building_id: destination,
                    },
                    // Into a depot, a transport or a bunker.
                    _ => Command::clicked_mission(
                        id,
                        crate::sim::mission::MissionType::Enter,
                        TargetKind::Entity(destination),
                    ),
                };
                // Rejected ownership cannot cancel a burst or passive
                // provenance. The MEGAMISSION Enter tests no owner
                // (`0x004C71F6..0x004C721F`), so only the typed orders do.
                if matches!(order, 2 | 3) {
                    assert!(!sim.apply_command("Russians", &command, Some(&rules)));
                    let actor = sim.substrate.entities.get(id).unwrap();
                    assert_eq!(
                        actor.attack_target.target().is_some(),
                        had_target,
                        "{command:?}"
                    );
                    assert!(actor.passive_scan.acquired(), "{command:?}");
                    assert_eq!(actor.weapon_burst.index(), 1, "{command:?}");
                }

                assert!(
                    sim.apply_command("Americans", &command, Some(&rules)),
                    "{command:?}"
                );
                let actor = sim.substrate.entities.get_mut(id).unwrap();
                assert!(actor.attack_target.target().is_none(), "{command:?}");
                assert!(!actor.passive_scan.acquired(), "{command:?}");
                // Ares 0x6FCF53 (1005D320) never resets the index.
                let kept = i32::from(!had_target || cfg!(feature = "ares"));
                assert_eq!(actor.weapon_burst.index(), kept, "{command:?}");
                // The next non-null assignment must retain the observed index.
                represented_assign_target(actor, Some(TargetKind::Cell(7, 4)));
                assert_eq!(actor.weapon_burst.index(), kept, "{command:?}");
            }
        }
    }

    #[test]
    fn target_burst_reset_matches_original_setter_and_callers() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/techno_target_burst.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 108);
        let target = |value: &serde_json::Value| match value.as_u64().unwrap() {
            0 => None,
            1 => Some(TargetKind::Cell(5, 4)),
            2 => Some(TargetKind::Cell(6, 4)),
            other => panic!("unexpected native target {other}"),
        };
        for row in rows {
            let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 4, 4);
            represented_assign_target(&mut entity, target(&row["previous"]));
            entity.weapon_burst.index = row["index"].as_i64().unwrap() as i32;
            entity
                .passive_scan
                .set_passively_acquired_target_for_test(row["passive"].as_u64().unwrap() != 0);
            represented_assign_target(&mut entity, target(&row["requested"]));
            // Ares 0x6FCF53 (1005D320) keeps the index on every assignment.
            let next_index = if cfg!(feature = "ares") {
                &row["index"]
            } else {
                &row["next_index"]
            };
            assert_eq!(
                entity.weapon_burst.index(),
                next_index.as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                entity.passive_scan.acquired(),
                row["next_passive"].as_u64().unwrap() != 0,
                "{row}"
            );
            assert_eq!(
                entity.attack_target.target(),
                target(&row["target"]),
                "{row}"
            );
        }
    }

    /// Phobos `TechnoExt::UpdateRecountBurst`
    /// (`Ext/Techno/Body.Update.cpp:544-566`): with `RecountBurst` on, a
    /// stale retained index with no target resets and a positive remaining
    /// ROF re-arms the shot.
    #[cfg(feature = "phobos")]
    #[test]
    fn recount_burst_resets_a_stale_index_and_rearms_the_shot() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::world::Simulation;
        let rules = |type_keys: &str, global: &str| {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[General]\n{global}\
                 [VehicleTypes]\n0=TANK\n[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [TANK]\nStrength=300\nPrimary=Gun\n{type_keys}\
                 [Gun]\nDamage=10\nROF=60\nBurst=1\nRange=6\nWarhead=WH\n\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
            )))
            .unwrap()
        };
        let recount = |rules: &RuleSet| {
            let mut entity = GameEntity::test_default(1, "TANK", "Americans", 5, 5);
            entity.weapon_burst.set_index(2);
            entity.last_fire_frame.set_for_test(-100);
            let mut sim = Simulation::new();
            sim.interner = crate::sim::intern::test_interner();
            entity.phobos_last_weapon_type = Some(sim.interner.intern("Gun"));
            sim.substrate.entities.insert(entity);
            super::update_phobos_recount_burst(&mut sim, 1, rules);
            let entity = sim.substrate.entities.get(1).unwrap();
            (entity.weapon_burst.index(), entity.rearm_timer.remaining(0))
        };
        // `Burst=1` with a retained index of 2: ratio 2 x ROF 60 - 60 = 60.
        assert_eq!(recount(&rules("RecountBurst=yes\n", "")), (0, 60));
        assert_eq!(recount(&rules("", "")), (2, 0), "default off");
        assert_eq!(
            recount(&rules("", "RecountBurst=yes\n")),
            (0, 60),
            "the global enables it"
        );
        assert_eq!(
            recount(&rules("RecountBurst=no\n", "RecountBurst=yes\n")),
            (2, 0),
            "the type key overrides the global"
        );
    }
}
