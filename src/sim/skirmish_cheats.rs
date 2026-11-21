//! VERA's one-shot commands for the local player in an offline skirmish.

use crate::map::cell_index::square_ring_cells;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{CellPassabilityArgs, CellRef, check_cell_passability};
use crate::sim::find_nearby_cell::RADIUS_HARD_CAP;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

pub const MAX_SPAWN_COUNT: u16 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkirmishCheat {
    Money(i32),
    SpySat,
    /// Saved before cursor placement. This form always uses the original base position.
    Spawn {
        type_id: String,
        count: u16,
    },
    /// Store the selected map cell so playback is independent of the camera and pointer.
    SpawnAt {
        type_id: String,
        count: u16,
        cell: (u16, u16),
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheatEffect {
    Money(i32),
    SpySat,
    Spawned {
        placed: u16,
        requested: u16,
        type_id: String,
    },
}

/// Chat validates the order before choosing a location. Placement and replay
/// resolve it against the same match rules when executing it.
pub fn skirmish_spawn_type<'a>(
    rules: &'a RuleSet,
    type_id: &str,
    count: u16,
) -> Result<&'a crate::rules::object_type::ObjectType, &'static str> {
    if !(1..=MAX_SPAWN_COUNT).contains(&count) {
        return Err("VERA:CheatUsage");
    }
    // A vehicle keeps its ID even if a mod also registers infantry under
    // that name. Existing commands and replays resolved vehicles first.
    rules
        .object_in_category(ObjectCategory::Vehicle, type_id)
        .or_else(|| rules.task_force_member_object(type_id))
        .ok_or("VERA:CheatUnit")
}

impl Simulation {
    pub fn apply_skirmish_cheat(
        &mut self,
        cheat: &SkirmishCheat,
        rules: &RuleSet,
    ) -> Result<CheatEffect, &'static str> {
        // Offline Skirmish only: no lockstep peers, and multiplayer rules
        // rather than a campaign's.
        if self.session.execution_topology().is_lockstep()
            || !self
                .session
                .rule_policy()
                .bootstrap()
                .uses_multiplayer_rules()
        {
            return Err("VERA:CheatUnavailable");
        }
        let owner = self
            .session
            .current_house()
            .ok_or("VERA:CheatUnavailable")?;
        let house = self.houses.get(&owner).ok_or("VERA:CheatUnavailable")?;
        if self.session.is_observer_house(owner)
            || house.is_defeated()
            || !house.is_controlled_by_human(self.session.rule_policy())
        {
            return Err("VERA:CheatUnavailable");
        }
        match cheat {
            SkirmishCheat::Money(amount) => {
                if *amount <= 0 {
                    return Err("VERA:CheatUsage");
                }
                let wallet = house.economy();
                if wallet
                    .credits()
                    .max(wallet.available_money())
                    .checked_add(*amount)
                    .is_none()
                {
                    return Err("VERA:CheatFunds");
                }
                self.note_revision();
                crate::sim::credit_income::add_credits(&mut self.houses, owner, *amount);
                Ok(CheatEffect::Money(*amount))
            }
            SkirmishCheat::SpySat => {
                self.note_revision();
                // The Reveal crate and the Spy Satellite use MapClass::Reveal @ 0x00577D90.
                self.reveal_map_for_owner(owner, rules);
                Ok(CheatEffect::SpySat)
            }
            SkirmishCheat::Spawn { type_id, count } => {
                self.spawn_skirmish_units(owner, type_id, *count, None, rules)
            }
            SkirmishCheat::SpawnAt {
                type_id,
                count,
                cell,
            } => self.spawn_skirmish_units(owner, type_id, *count, Some(*cell), rules),
        }
    }

    fn spawn_skirmish_units(
        &mut self,
        owner: InternedId,
        type_id: &str,
        count: u16,
        at: Option<(u16, u16)>,
        rules: &RuleSet,
    ) -> Result<CheatEffect, &'static str> {
        let object = skirmish_spawn_type(rules, type_id, count)?;
        let bounds = self.playfield_bounds().ok_or(if at.is_some() {
            "VERA:CheatPointAtMap"
        } else {
            "VERA:CheatNoBase"
        })?;
        let in_playfield = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
            bounds.contains_height_aware_packed(
                i32::from(cell.rx),
                i32::from(cell.ry),
                cell.level as i8,
                cell.slope_type,
            )
        };
        let (cx, cy) = match at {
            Some((x, y)) => {
                if !self
                    .resolved_terrain()
                    .and_then(|grid| grid.cell(x, y))
                    .is_some_and(in_playfield)
                {
                    return Err("VERA:CheatPointAtMap");
                }
                (x, y)
            }
            None => self
                .houses
                .get(&owner)
                .and_then(|house| house.base_center())
                .or_else(|| {
                    self.entities()
                        .values()
                        .find(|e| e.owner() == owner && e.is_live_on_map())
                        .map(|e| (e.position.rx(), e.position.ry()))
                })
                .ok_or("VERA:CheatNoBase")?,
        };
        let center = (i32::from(cx), i32::from(cy));
        let passability = CellPassabilityArgs {
            speed_type: object.speed_type,
            ignore_infantry: false,
            ignore_vehicles: false,
            zone: None,
            movement_zone: object.movement_zone,
            level: None,
            is_bridge: false,
        };
        self.note_revision();
        let mut placed = 0;
        let rings = (1..=i32::from(RADIUS_HARD_CAP)).flat_map(|r| square_ring_cells(center, r));
        for (x, y) in std::iter::once(center).chain(rings) {
            let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some(cell) = self.resolved_terrain.as_ref().and_then(|t| t.cell(rx, ry)) else {
                continue;
            };
            if !in_playfield(cell)
                || cell.bridge_facts.has_structural_bridge()
                || !self
                    .occupancy()
                    .is_empty_on_layer(rx, ry, MovementLayer::Ground)
                || self.terrain_objects.cell_index().contains_key(&(rx, ry))
                || self
                    .overlay_grid
                    .as_ref()
                    .and_then(|grid| grid.cell(rx, ry).overlay_id)
                    .is_some_and(|id| {
                        rules
                            .overlay_registry()
                            .flags(id)
                            .is_none_or(|flags| !flags.tiberium)
                    })
                || !check_cell_passability(
                    &CellRef::Real(cell),
                    passability,
                    &self.cell_passability_map(),
                )
                .admits()
            {
                continue;
            }
            let type_id = self.interner.intern(&object.id);
            if self
                .spawn_resolved_object_at_height(
                    object,
                    type_id,
                    owner,
                    rx,
                    ry,
                    0,
                    cell.level,
                    rules,
                    Some(rules.overlay_registry()),
                )
                .and_then(|id| self.entities().get(id))
                .is_some_and(|e| e.is_live_on_map())
            {
                placed += 1;
                if placed == count {
                    break;
                }
            }
        }
        // Even a refused Unlimbo can have consumed a constructor RNG draw and an ID.
        // Keep the attempt in the replay when the command placed fewer than requested.
        Ok(CheatEffect::Spawned {
            placed,
            requested: count,
            type_id: object.id.clone(),
        })
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use crate::sim::scenario_session::ScenarioGameMode;

    #[test]
    fn money_requires_a_living_local_skirmish_player_and_room_in_the_wallet() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Player");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 500, 10,
            ));
        sim.session.set_current_house_for_test(Some(owner));
        let rules = RuleSet::empty_for_test();
        for mode in [
            ScenarioGameMode::Lan,
            ScenarioGameMode::Wol,
            ScenarioGameMode::Campaign,
        ] {
            sim.install_session_mode(mode);
            assert_eq!(
                sim.apply_skirmish_cheat(&SkirmishCheat::Money(10000), rules),
                Err("VERA:CheatUnavailable")
            );
        }
        sim.install_session_mode(ScenarioGameMode::Skirmish);
        assert_eq!(
            sim.apply_skirmish_cheat(&SkirmishCheat::Money(i32::MAX), rules),
            Err("VERA:CheatFunds")
        );
        assert_eq!(sim.houses[&owner].economy().credits(), 500);
        sim.houses.get_mut(&owner).unwrap().mark_defeated();
        assert_eq!(
            sim.apply_skirmish_cheat(&SkirmishCheat::Money(10000), rules),
            Err("VERA:CheatUnavailable")
        );
    }

    #[test]
    fn spawn_keeps_the_vehicle_when_other_classes_share_its_rules_id() {
        let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut scene = crate::headless_scenario::load(&retail, "Hills.mmx", 17).unwrap();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=DUAL\n[InfantryTypes]\n0=DUAL\n[BuildingTypes]\n0=DUAL\n[DUAL]\nStrength=100\nSpeedType=Track\n",
        )).unwrap();
        let sim = &mut scene.runtime.simulation;
        let command = SkirmishCheat::Spawn {
            type_id: "dual".into(),
            count: 1,
        };
        assert_eq!(
            sim.apply_skirmish_cheat(&command, &rules),
            Ok(CheatEffect::Spawned {
                placed: 1,
                requested: 1,
                type_id: "DUAL".into()
            })
        );
        assert!(sim.entities().values().any(|e| e.is_live_on_map()
            && sim.resolve(e.type_ref()) == "DUAL"
            && e.category() == crate::map::entities::EntityCategory::Unit));
    }

    #[test]
    fn a_retail_skirmish_spawns_kirovs_aircraft_ships_and_infantry() {
        use crate::map::entities::EntityCategory;
        use crate::rules::terrain_rules::LandType;
        let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut scene = crate::headless_scenario::load(&retail, "Pacific.mmx", 17).unwrap();
        let sim = &mut scene.runtime.simulation;
        let owner = sim.session.current_house().unwrap();
        let before = sim
            .entities()
            .values()
            .map(|e| e.stable_id())
            .max()
            .unwrap_or(0);
        let units = [
            ("ZEP", 100, EntityCategory::Unit),
            ("ORCA", 1, EntityCategory::Aircraft),
            ("SUB", 2, EntityCategory::Unit),
            ("E1", 3, EntityCategory::Infantry),
        ];
        for (id, count, _) in units {
            let command = SkirmishCheat::Spawn {
                type_id: id.into(),
                count,
            };
            assert_eq!(
                sim.apply_skirmish_cheat(&command, &scene.runtime.resources.rules),
                Ok(CheatEffect::Spawned {
                    placed: count,
                    requested: count,
                    type_id: id.into()
                }),
                "{id}"
            );
        }
        scene
            .runtime
            .advance_frame(
                &[],
                67,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::empty(),
            )
            .unwrap();
        let sim = &scene.runtime.simulation;
        let terrain = sim.resolved_terrain().unwrap();
        for (id, count, category) in units {
            let members = sim.entities().values().filter(|e| {
                e.stable_id() > before
                    && e.owner() == owner
                    && e.is_live_on_map()
                    && e.category() == category
                    && sim.resolve(e.type_ref()) == id
            });
            let cells: std::collections::BTreeSet<_> = members
                .map(|e| (e.position.rx(), e.position.ry()))
                .collect();
            assert_eq!(cells.len(), usize::from(count), "{id}");
            if id == "SUB" {
                assert!(
                    cells
                        .iter()
                        .all(|&(x, y)| terrain.cell(x, y).unwrap().land_type
                            == LandType::Water.as_index())
                );
            }
        }
    }

    #[test]
    fn a_retail_skirmish_grants_credits_reveals_the_map_and_places_200_rhinos() {
        let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut scene = crate::headless_scenario::load(&retail, "Hills.mmx", 17).unwrap();
        use crate::sim::replay::{REPLAY_VERSION, ReplayHeader, ReplayLog, ReplayRunner};
        let mut playback = crate::sim::runtime::SimRuntime {
            simulation: scene.runtime.simulation.clone(),
            resources: scene.runtime.resources.clone(),
        };
        let mut log = ReplayLog::new(ReplayHeader {
            version: REPLAY_VERSION,
            tick_hz: 15,
            seed: 17,
            rules_hash: scene.runtime.resources.rules.simulation_config_hash(),
            map_name: scene.sim().session().map_name().into(),
            pixel_conversion_bounds: scene.sim().session().pixel_conversion_bounds(),
        });
        let sim = &mut scene.runtime.simulation;
        let rules = &scene.runtime.resources.rules;
        let owner = sim.session.current_house().unwrap();
        let credits = sim.houses[&owner].economy().credits();
        assert_eq!(
            log.apply_skirmish_cheat(sim, rules, SkirmishCheat::Money(10000)),
            Ok(CheatEffect::Money(10000))
        );
        assert_eq!(sim.houses[&owner].economy().credits(), credits + 10000);
        let mut frame = scene
            .runtime
            .advance_frame(
                &[],
                67,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::empty(),
            )
            .unwrap();
        let admitted = frame.take_admitted_commands();
        log.record_tick(frame.tick.tick, admitted, frame.tick.state_hash);
        let sim = &mut scene.runtime.simulation;
        let rules = &scene.runtime.resources.rules;
        assert_eq!(
            log.apply_skirmish_cheat(sim, rules, SkirmishCheat::SpySat),
            Ok(CheatEffect::SpySat)
        );
        assert!(sim.houses[&owner].map_is_clear());
        let before = sim
            .entities()
            .values()
            .map(|e| e.stable_id())
            .max()
            .unwrap_or(0);
        assert_eq!(
            log.apply_skirmish_cheat(
                sim,
                rules,
                SkirmishCheat::Spawn {
                    type_id: "htnk".into(),
                    count: 200
                }
            ),
            Ok(CheatEffect::Spawned {
                placed: 200,
                requested: 200,
                type_id: "HTNK".into()
            }),
        );
        let cells: std::collections::BTreeSet<_> = sim
            .entities()
            .values()
            .filter(|e| {
                e.stable_id() > before
                    && e.owner() == owner
                    && sim.resolve(e.type_ref()) == "HTNK"
                    && e.is_live_on_map()
            })
            .map(|e| (e.position.rx(), e.position.ry()))
            .collect();
        assert_eq!(cells.len(), 200);
        // The recorded legacy spawn above still starts at the base. Aim across
        // Hills at another player's unused starting cell and replay both orders.
        let base = sim.houses[&owner].base_center().unwrap();
        let target = *sim
            .session
            .mp_start_waypoints()
            .values()
            .find(|&&(x, y)| x.abs_diff(base.0).max(y.abs_diff(base.1)) > RADIUS_HARD_CAP)
            .unwrap();
        let spawn_at = |cell| SkirmishCheat::SpawnAt {
            type_id: "HTNK".into(),
            count: 1,
            cell,
        };
        let unchanged = sim.state_hash();
        assert_eq!(
            log.apply_skirmish_cheat(sim, rules, spawn_at((u16::MAX, u16::MAX))),
            Err("VERA:CheatPointAtMap")
        );
        assert_eq!(sim.state_hash(), unchanged);
        assert_eq!(
            log.apply_skirmish_cheat(sim, rules, spawn_at(target)),
            Ok(CheatEffect::Spawned {
                placed: 1,
                requested: 1,
                type_id: "HTNK".into()
            })
        );
        assert!(sim.entities().values().any(|e| e.owner() == owner
            && e.is_live_on_map()
            && sim.resolve(e.type_ref()) == "HTNK"
            && (e.position.rx(), e.position.ry()) == target));
        let saved = serde_json::from_slice(&serde_json::to_vec(&log).unwrap()).unwrap();
        ReplayRunner::run_runtime(&mut playback, &saved, 67).unwrap();
        assert_eq!(playback.simulation.state_hash(), sim.state_hash());
    }
}
