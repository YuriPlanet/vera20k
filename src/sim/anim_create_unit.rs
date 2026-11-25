//! Phobos `TechnoTypeExt::CreateUnit` (`Ext/TechnoType/Body.cpp:587-733`)
//! and the animation side of `CreateUnit.*`
//! (`Ext/Anim/Hooks.AnimCreateUnit.cpp:36-80`).
//!
//! The animation type's `CreateUnit=` reserves its cell's infantry
//! occupation bits at construction (`MarkAllOccupationBits`, the three
//! sub-cell bits `0x04|0x08|0x10` at `0x71C146`), then at its actual-effects
//! frame releases them and spawns one Techno through the shared constructor /
//! Unlimbo owner. The spawn refuses BuildingTypes, resolves the owner (the
//! anim's house, civilian side without `RequireOwner`), positions the unit by
//! `SpawnHeight` / `AlwaysSpawnOnGround` / `ConsiderPathfinding`, optionally
//! parachutes it in, then plays `SpawnAnim` and queues the mission.
//!
//! ## RNG draws
//! - `RandomFacing` (constructor default **yes**): one `RandomRanged(0,255)`.
//! - A `Random` `CreateUnit.Owner` draw happens at owner install, not here.
//! - `ConsiderPathfinding` uses the frame-counter search owner, which draws
//!   nothing.
//!
//! ## Dependency rules
//! - Part of sim/; uses the shared construction, movement and mission owners.

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::phobos_create_unit::CreateUnitSettings;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimId, AnimWorldCoord};
use crate::sim::cell_rect::{CellPassabilityArgs, check_cell_passability};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation, UninitContext};

impl Simulation {
    /// `AnimClass_AI_CreateUnit_ActualEffects @ 0x424932`'s body: unmark the
    /// cell, pick the facing, then `TechnoTypeExt::CreateUnit`.
    pub(crate) fn anim_create_unit(
        &mut self,
        id: AnimId,
        settings: &CreateUnitSettings,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let Some(location) = self.anim_absolute_coord(id) else {
            return false;
        };
        let owner_house = anim.owner_house();
        let invoker = anim.invoker();
        let invoker_house = anim.invoker_house();
        let from_death = anim.from_death_unit();
        let death_facing = anim.death_unit_facing();
        let death_has_turret = anim.death_unit_has_turret();
        let death_turret_facing = anim.death_unit_turret_facing();
        let frame = self.session.binary_frame();
        // A `CreateUnit=` naming no loaded unit type is null at load natively
        // (`Nullable<CreateUnitTypeClass*>::Read`, Find only), so it neither
        // marks the cell bits nor draws a facing.
        if rules.object(&settings.unit_type).is_none() {
            return false;
        }
        // `UnmarkAllOccupationBits(GetCell()->GetCoordsWithBridge())`.
        Self::anim_unmark_all_occupation_bits(
            &mut self.substrate,
            &self.resolved_terrain,
            location,
        );

        // `RandomFacing ? RandomRanged(0, 255) : Facing`, then the death
        // facing when `InheritDeathFacings` has one.
        let mut facing = if settings.random_facing {
            self.scenario_rng.next_range_i32_inclusive(0, 255) as u8
        } else {
            settings.facing
        };
        if settings.inherit_death_facings
            && from_death
            && let Some(death) = death_facing
        {
            facing = death;
        }
        let secondary = (settings.inherit_turret_facings && from_death && death_has_turret)
            .then_some(death_turret_facing)
            .flatten();

        let Some(object) = rules.object(&settings.unit_type) else {
            return false;
        };
        if object.category == ObjectCategory::Building {
            return false;
        }

        // Owner: the anim's house, else the civilian side unless
        // `RequireOwner`.
        let defeated = |sim: &Self, house: InternedId| {
            sim.houses
                .get(&house)
                .is_some_and(|state| state.is_defeated())
        };
        let mut decided = owner_house;
        if decided.is_none_or(|house| defeated(self, house)) {
            if settings.require_owner {
                return false;
            }
            decided = self.civilian_side_house(rules);
        }
        let Some(owner) = decided else {
            return false;
        };

        let (mut rx, mut ry, _, _, _) = location.to_cell_sub_z();
        let base_height = location.z;
        // `Unsorted::CellHeight * 2`.
        let in_air = base_height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS.wrapping_mul(2);

        let Some((mut placement, mut bridge)) =
            Self::create_unit_placement(&self.resolved_terrain, object, rx, ry, location, settings)
        else {
            return false;
        };
        let owner_name = self.interner.resolve(owner).to_owned();
        let Some(unit) = self.construct_resolved_object_limbo_at_height(
            object,
            &owner_name,
            rx,
            ry,
            facing,
            0,
            rules,
            frame_effects,
        ) else {
            return false;
        };

        if settings.consider_pathfinding {
            // RESIDUAL: the native pre-check reads the target cell through
            // `CellClass::IsClearToMove`; the nearby search runs on the
            // constructed limbo Techno's own type facts.
            let (speed_type, movement_zone) = Self::create_unit_speed_zone(object);
            let cell = crate::sim::cell_rect::get_cellclass_fallback(
                self.resolved_terrain.as_ref(),
                i32::from(rx),
                i32::from(ry),
            );
            let clear = {
                let map = self.cell_passability_map();
                check_cell_passability(
                    &cell,
                    CellPassabilityArgs {
                        speed_type,
                        ignore_infantry: false,
                        ignore_vehicles: false,
                        zone: None,
                        movement_zone,
                        level: None,
                        is_bridge: bridge,
                    },
                    &map,
                )
                .admits()
            };
            if !clear && let Some((nx, ny)) = self.techno_nearby_location(unit, None, rules) {
                rx = nx as u16;
                ry = ny as u16;
                let Some(moved) = Self::create_unit_placement(
                    &self.resolved_terrain,
                    object,
                    rx,
                    ry,
                    location,
                    settings,
                ) else {
                    return false;
                };
                (placement, bridge) = moved;
            }
        }

        // `pTechno->OnBridge = isBridge` right after `CreateObject`.
        if let Some(entity) = self.substrate.entities.get_mut(unit) {
            entity.on_bridge.store(bridge);
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.store_ground_layer(bridge);
            }
        }

        let aircraft = object.category == ObjectCategory::Aircraft;
        let parachuted = !aircraft
            && settings.spawn_parachuted_in_air
            && !settings.always_spawn_on_ground
            && in_air;
        let placed = if parachuted {
            crate::sim::aircraft::drop_payload::spawn_parachuted(
                self,
                rules,
                unit,
                DriveCoord {
                    x: placement.x,
                    y: placement.y,
                    z: placement.z,
                },
                0,
                frame_effects,
            )
            .is_ok()
        } else {
            self.reveal_constructed_object_at_coord_with_overlay_context(
                unit,
                DriveCoord {
                    x: placement.x,
                    y: placement.y,
                    z: placement.z,
                },
                facing,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                frame_effects,
            )
            .is_some()
        };
        if !placed {
            self.uninit_with_context(unit, UninitContext::new(rules).with_effects(frame_effects));
            return false;
        }
        if let Some(dir) = secondary
            && let Some(entity) = self.substrate.entities.get_mut(unit)
            && let Some(barrel) = entity.barrel_facing.as_mut()
        {
            barrel.set(u16::from(dir) << 8, frame);
        }

        // `CreateRandomAnim(SpawnAnim, location, pInvoker, pInvokerHouse, true)`:
        // the owner house is the invoker house, else the invoker's owner; the
        // invoker flag stores pInvoker, while `ownedObject` stays false.
        // RESIDUAL: native leaves the anim unowned when both are absent, while
        // the shared spawn owner requires a house and falls back to the
        // spawned unit's owner.
        let anim_house = invoker_house
            .or_else(|| {
                invoker.and_then(|object| {
                    self.substrate
                        .entities
                        .get(object)
                        .map(|entity| entity.owner())
                })
            })
            .unwrap_or(owner);
        self.spawn_random_owned_anim(
            rules,
            &settings.spawn_anim,
            AnimWorldCoord {
                x: placement.x,
                y: placement.y,
                z: placement.z,
            },
            anim_house,
            None,
            invoker,
        );

        // `Ext/TechnoType/Body.cpp:661-706`: after `Unlimbo` and the random
        // spawn anim, a Techno that is not in limbo takes one order per its
        // locomotor, unless `AlwaysSpawnOnGround`:
        //   Fly (`:674-683`)     re-stamp the placement, then `EnterIdleMode(false, true)`
        //                        when `pCell->GetContent()` or an `AirportBound`
        //                        `AircraftType`, else `Move_To(GetCoordsWithBridge())`;
        //   Jumpjet (`:684-700`) `LocomotionFacing.SetCurrent(DirStruct(facing)`,
        //                        `Move_To(location)` when `inAir`;
        //   other (`:702-706`)   `IsFallingDown` + `OnParachuted`.
        if !settings.always_spawn_on_ground {
            let facts = self.substrate.entities.get(unit).map(|entity| {
                let fly = entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.fly_runtime());
                let jumpjet = entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.jumpjet_runtime())
                    .is_some();
                (
                    entity.lifecycle.in_limbo(),
                    fly.is_some(),
                    fly.is_some_and(crate::sim::movement::fly_height::FlyRuntime::airport_bound)
                        && aircraft,
                    jumpjet,
                    crate::sim::movement::air_movement::is_high_flying(
                        entity,
                        self.resolved_terrain.as_ref(),
                        Some((rules, &self.interner)),
                    ),
                )
            });
            if let Some((in_limbo, fly, airport_bound, jumpjet, high_flying)) = facts
                && !in_limbo
            {
                // `pCell->GetContent()` (`0x0047E9xx`): the deck list when the
                // cell has a structural bridge, the ground list otherwise. The
                // spawned object itself is never content here — neither arm
                // places it on the cell list before this call.
                let content = self.native_cell_content((rx, ry)).any(|member| {
                    !matches!(
                        member,
                        crate::sim::occupancy::CellObjectMember::Entity(id) if id == unit
                    )
                });
                if fly && (content || airport_bound) {
                    crate::sim::aircraft::enter_idle_mode_for(self, unit, rules, frame_effects);
                } else if fly {
                    // `pFlyLoco->Move_To(pCell->GetCoordsWithBridge())`: the
                    // cell's centre and its structural deck when the cell has
                    // one (`NativeCellQuery::canonical` answers the same plane
                    // `GetCoordsWithBridge` does).
                    let cells = self
                        .resolved_terrain
                        .as_ref()
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    let target = crate::sim::movement::target_cell_coord(rx, ry, cells.as_ref());
                    Self::move_air_coordinate(
                        &mut self.interner,
                        &self.session,
                        &mut self.substrate,
                        &mut self.sound_events,
                        &self.resolved_terrain,
                        unit,
                        target,
                        None,
                        rules,
                    );
                } else if jumpjet {
                    // `pJJLoco->LocomotionFacing.SetCurrent(DirStruct(facing))`.
                    let now = self.session.binary_frame();
                    let location = DriveCoord {
                        x: placement.x,
                        y: placement.y,
                        z: placement.z,
                    };
                    if let Some(locomotor) = self
                        .substrate
                        .entities
                        .get_mut(unit)
                        .and_then(|entity| entity.locomotor.as_mut())
                        .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
                    {
                        locomotor.phobos_set_locomotion_facing(facing, now);
                        // `:689-695`: a BalloonHover jet is ordered to ascend
                        // straight up — `IsMoving = true`,
                        // `DestinationCoords = GetCoords()` written directly,
                        // plus the `JumpjetStraightAscend` latch its locomotion
                        // consumes (`Ext/Unit/Hooks.Jumpjet.cpp:394-437`).
                        if object.balloon_hover {
                            locomotor.phobos_straight_ascend_hold(location);
                        }
                    }
                    // RESIDUAL: `LastMapCoords = CellClass::Coord2Cell(location)`
                    // (`:687`) has no VERA owner — nothing reads a Foot last-cell
                    // latch.
                    if object.balloon_hover {
                        if let Some(entity) = self.substrate.entities.get_mut(unit) {
                            entity.mark_jumpjet_straight_ascend();
                        }
                    } else if high_flying {
                        // `:696-700`: a non-BalloonHover jet in air lands.
                        self.jumpjet_move_to(unit, location, rules);
                    }
                } else if high_flying && !parachuted {
                    if let Some(entity) = self.substrate.entities.get_mut(unit) {
                        entity.raise_falling_down();
                        entity.mark_on_parachuted();
                    }
                }
            }
        }

        // `QueueMission(UnitMission, false)` (AIMission for a computer owner).
        let mission = if !self.owner_is_human(owner) {
            settings.ai_unit_mission.unwrap_or(settings.unit_mission)
        } else {
            settings.unit_mission
        };
        if let Some(entity) = self.substrate.entities.get_mut(unit) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(mission),
            );
        }
        // `RecheckTechTree` unless the owner is MultiplayPassive.
        if !self
            .houses
            .get(&owner)
            .is_some_and(|house| house.multiplay_passive())
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house.request_tech_recheck();
        }
        true
    }

    /// The `pCell`/Z block of `TechnoTypeExt::CreateUnit` (`Body.cpp:614-628`):
    /// the cell floor (plus a structural bridge deck), `SpawnHeight` or
    /// `AlwaysSpawnOnGround`, and the coordinate the Unlimbo uses. `None`
    /// when the coordinate names no real cell (native's null `pCell`).
    fn create_unit_placement(
        resolved_terrain: &Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
        object: &crate::rules::object_type::ObjectType,
        rx: u16,
        ry: u16,
        location: AnimWorldCoord,
        settings: &CreateUnitSettings,
    ) -> Option<(DriveCoord, bool)> {
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            resolved_terrain.as_ref(),
            i32::from(rx),
            i32::from(ry),
        );
        let crate::sim::cell_rect::CellRef::Real(_) = cell else {
            return None;
        };
        // RESIDUAL: `allowBridges` is native `GroundType[Clear].Cost[SpeedType]
        // > 0`; VERA treats every non-Winged speed type as bridge-capable.
        let (speed_type, _) = Self::create_unit_speed_zone(object);
        let allows_bridges = speed_type != SpeedType::Winged;
        let is_bridge = allows_bridges && cell.bridge_flags_0x1180() & 0x100 != 0;
        let bridge_z = if is_bridge {
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        };
        let ground = cell.get_coords();
        let floor = ground.z.wrapping_add(bridge_z);
        let moved = i32::from(location.x / 256) != i32::from(rx)
            || i32::from(location.y / 256) != i32::from(ry);
        let (x, y) = if moved {
            (ground.x, ground.y)
        } else {
            (location.x, location.y)
        };
        let z = if !settings.always_spawn_on_ground && settings.spawn_height >= 0 {
            floor.wrapping_add(settings.spawn_height)
        } else if settings.always_spawn_on_ground {
            floor
        } else {
            floor.max(location.z)
        };
        Some((DriveCoord { x, y, z }, is_bridge))
    }

    /// `TechnoTypeExt::CreateUnit`: a non-Aircraft type uses its own
    /// `SpeedType`/`MovementZone`; an AircraftType uses Wheel/Normal
    /// (`Ext/TechnoType/Body.cpp:601-602`).
    fn create_unit_speed_zone(
        object: &crate::rules::object_type::ObjectType,
    ) -> (SpeedType, MovementZone) {
        if object.category == ObjectCategory::Aircraft {
            (SpeedType::Wheel, MovementZone::Normal)
        } else {
            (object.speed_type, object.movement_zone)
        }
    }
}

/// `CreateUnit`'s `MarkAllOccupationBits` / `UnmarkAllOccupationBits`: the
/// three infantry sub-cell bits `0x04|0x08|0x10` at the anim cell's
/// `GetCoordsWithBridge()` plane (`0x71C146`), the deck on a live structural
/// bridge cell and the ground plane otherwise.
impl Simulation {
    pub(crate) fn anim_mark_all_occupation_bits(
        substrate: &mut crate::sim::world::ObjectSubstrate,
        resolved_terrain: &Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
        location: AnimWorldCoord,
    ) {
        Self::apply_all_occupation_bits(
            substrate,
            resolved_terrain,
            location,
            crate::sim::anim_class::AnimOccupationOperation::Mark,
        );
    }

    pub(crate) fn anim_unmark_all_occupation_bits(
        substrate: &mut crate::sim::world::ObjectSubstrate,
        resolved_terrain: &Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
        location: AnimWorldCoord,
    ) {
        Self::apply_all_occupation_bits(
            substrate,
            resolved_terrain,
            location,
            crate::sim::anim_class::AnimOccupationOperation::Clear,
        );
    }

    fn apply_all_occupation_bits(
        substrate: &mut crate::sim::world::ObjectSubstrate,
        resolved_terrain: &Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
        location: AnimWorldCoord,
        operation: crate::sim::anim_class::AnimOccupationOperation,
    ) {
        // `0x71C146`: bits 0x04, 0x08 and 0x10 — every infantry sub-cell.
        const ALL_INFANTRY_BITS: u8 = 0x04 | 0x08 | 0x10;
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::sim::occupancy::RawCellKey;
        let (key, live_structural_bridge) = match resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(location.x, location.y);
                (
                    RawCellKey::from_native(terrain, cell),
                    cells.flags(cell) & 0x100 != 0,
                )
            }
            None => (
                RawCellKey::Real((location.x / 256) as u16, (location.y / 256) as u16),
                false,
            ),
        };
        crate::sim::anim_class::apply_anim_coords_occupation(
            &mut substrate.raw_cell_occupation,
            key,
            ALL_INFANTRY_BITS,
            live_structural_bridge,
            operation,
        );
    }
}

#[cfg(all(test, tests_sim_a_b))]
#[path = "anim_create_unit_tests.rs"]
mod tests;
