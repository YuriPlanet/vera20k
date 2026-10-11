//! Foot4D5690, reached through Unit7414E0 by Cell Attack, and Infantry522340
//! by ordinary Teleport infantry object Attack. Native execution:
//! tools/spatial_oracle/fv_cell_attack. The shared range,
//! Cell admission, zone-cost, target and destination owners retain their work.

use super::foot_path::{cell_centre, chebyshev, coord_cell};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{self, CellRect, CellRectPassabilityContext};
use crate::sim::combat::{self, TargetKind, in_range};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::MissionType;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction_tables::{cell_delta_unchecked, facing16_between};
use crate::util::native_trig::facing_step_world_xy;
use crate::util::native_x87::distance_3d_leptons;

const OFFSETS: [i16; 25] = [
    0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6, -6, 8, -8, 16, -16, 24, -24, 32, -32, 48, -48, 64, -64,
]; // original signed table8224DC

#[cfg(test)]
#[path = "foot_approach_tests.rs"]
mod tests;

impl Simulation {
    /// Unit 0x7414E0's BalloonHover arm (0x741599..0x741600), as the Foot
    /// missions call Approach (vt+0x53C) with no-move 0
    /// ([`Self::approach_balloon_target`]). A `BalloonHover=` type (+0xD6A) whose GetWeapon(SelectWeapon(Target))
    /// (vt+0x2E4, vt+0x3F8) has a `Vertical=` (+0x2C0) `Projectile=` (+0xA0)
    /// takes its Target as its destination (vt+0x480 with 1) and returns.
    /// Any other weapon, or none, goes on to the next arm (0x741603). This
    /// answers that destination, or None for the next arm.
    ///
    /// Residuals:
    /// - The head's computer crush arm (0x7414E4..0x741593) runs first. A
    ///   computer's Unit with `Crusher=` (+0xD28) or the CRUSHER ability, no
    ///   NavCom and a crushable Techno Target within reach steers onto it
    ///   whatever its weapon. No retail `BalloonHover=` type (ZEP, DISK) has
    ///   either, so in retail it never pre-empts this arm.
    /// - With the no-move flag set, native returns the Target without the
    ///   destination. Every mission passes 0, and no VERA caller passes 1.
    /// - Mission_Patrol calls Approach too (`0x004D473A`), and VERA has no
    ///   Foot Patrol body; the pursuit pass (`tick_attack_pursuit`) stands in.
    ///   Trigger: a Kirov or a Floating Disc on Patrol holding a target.
    ///   Effect: the pass sends it to the target's cell, and its in-range halt
    ///   admits no Patrol (`range_stop_admits`), so it flies on to that cell
    ///   and stays on Patrol, roughly where the arm sends a vertical weapon;
    ///   a Disc on its laser does not stop at range. Frequency: nothing in
    ///   VERA puts a Unit on Patrol today; native producers were not surveyed.
    ///
    /// Native comparison: tools/spatial_oracle/balloon_hover.json `approach`
    /// rows.
    pub(crate) fn balloon_hover_approach(&self, id: u64, rules: &RuleSet) -> Option<NavTargetRef> {
        let actor = self.substrate.entities.get(id)?;
        let target = actor.attack_target.as_ref()?.target;
        if actor.category != EntityCategory::Unit
            || !self.object_type(actor.type_ref(), rules)?.balloon_hover
        {
            return None;
        }
        let selected = combat::pursuit_selection(
            actor,
            &target,
            &self.substrate.entities,
            rules,
            &self.interner,
            self.resolved_terrain.as_ref(),
            Some(&self.house_alliances),
        );
        balloon_hover_destination(selected.map(|selected| selected.weapon), target, rules)
    }

    /// Approach (vt+0x53C) with no-move 0, as Mission_Attack (`0x004D4E6A`),
    /// Mission_Hunt (`0x004D54DD`), Mission_AreaGuard (`0x004D6F2D`) and
    /// Mission_Rescue (`0x004DDFEB`) call it, for the BalloonHover arm
    /// ([`Self::balloon_hover_approach`]): its Target becomes the destination
    /// (vt+0x480 with 1). Answers whether the arm took the call; otherwise
    /// Mission_Attack's Cell approach or the pursuit pass stands in for the
    /// rest of Approach.
    pub(crate) fn approach_balloon_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(destination) = self.balloon_hover_approach(id, rules) else {
            return false;
        };
        let _ = self.assign_destination_represented(
            id,
            Some(destination),
            Some(rules),
            registry,
            frame_effects,
        );
        true
    }

    /// Whether the current mission's own Approach takes `id`'s approach, so the
    /// pursuit pass (`tick_attack_pursuit`) leaves it alone: Mission_Attack's
    /// common Foot approach ([`Self::owns_foot_approach`]), or the BalloonHover
    /// arm on each mission that calls [`Self::approach_balloon_target`]
    /// (Attack, Hunt, AreaGuard and Rescue). Keep it in step with those calls.
    pub(crate) fn mission_owns_approach(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(mission) = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.mission.current().known())
        else {
            return false;
        };
        (mission == MissionType::Attack && self.owns_foot_approach(id, rules))
            || (matches!(
                mission,
                MissionType::Attack
                    | MissionType::Hunt
                    | MissionType::AreaGuard
                    | MissionType::Rescue
            ) && self.balloon_hover_approach(id, rules).is_some())
    }

    /// Ownership boundary for the absorbed common Unit/Infantry approach. CloseRange,
    /// the BalloonHover arm ([`Self::balloon_hover_approach`]), non-Cell
    /// queued receivers and DestroyableCliffs redirect have separate native
    /// branches; their old adapter remains explicit.
    /// A mapless fixture cannot execute native cell/zone queries.
    pub(crate) fn owns_foot_approach(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let Some(target) = actor.attack_target.as_ref().map(|a| a.target) else {
            return false;
        };
        let represented = match (actor.category, target) {
            (EntityCategory::Unit, TargetKind::Cell(rx, ry)) => {
                actor.locomotor.as_ref().is_some_and(|l| {
                    matches!(l.active_kind(), LocomotorKind::Drive | LocomotorKind::Ship)
                }) && !terrain.is_destroyable_cliff(rx, ry)
            }
            (EntityCategory::Infantry, TargetKind::Entity(target)) => {
                // Infantry522340's ordinary tail calls Foot4D5690(false).
                // Doing27..30 and CloseRange use different override arms.
                actor
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.active_kind() == LocomotorKind::Teleport)
                    && actor
                        .mission_leaf
                        .as_infantry()
                        .is_some_and(|leaf| !matches!(leaf.doing(), 27..=30))
                    && self.substrate.entities.contains(target)
            }
            _ => false,
        };
        represented
            && self.zone_grid.is_some()
            && self
                .object_type(actor.type_ref(), rules)
                .is_some_and(|t| !t.close_range)
            && self.balloon_hover_approach(id, rules).is_none()
            && actor
                .navigation
                .nav_queue
                .iter()
                .all(|target| matches!(target, NavTargetRef::Cell { .. }))
    }

    /// Common Foot4D5690 body (Unit741689 and Infantry52236C pass false).
    /// No RNG belongs
    /// to Approach itself; MissionAttack draws its cadence after this returns.
    pub(crate) fn approach_foot_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let Some(target) = actor.attack_target.as_ref().map(|a| a.target) else {
            return Ok(None);
        };
        if actor
            .navigation
            .nav_com
            .is_some_and(|nav| match (nav, target) {
                (NavTargetRef::Cell { rx, ry }, TargetKind::Cell(x, y)) => (rx, ry) == (x, y),
                (
                    NavTargetRef::Entity { id }
                    | NavTargetRef::Object { id }
                    | NavTargetRef::Building { id },
                    TargetKind::Entity(target),
                ) => id == target,
                _ => false,
            })
        {
            //4D56A1..56B1: an existing NavCom identical to TarCom returns.
            return Ok(None);
        }
        let Some(selected) = combat::pursuit_selection(
            actor,
            &target,
            &self.substrate.entities,
            rules,
            &self.interner,
            self.resolved_terrain.as_ref(),
            Some(&self.house_alliances),
        ) else {
            return Ok(None);
        };
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach requires mover type")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Approach requires map cells")?;
        let identity = target.cell_identity(terrain);
        let target_reference = match target {
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
            TargetKind::Entity(id) => NavTargetRef::object(id),
        };
        let cells = NativeCellQuery::canonical(terrain);
        let mut range = combat::combat_weapon::weapon_range(
            actor,
            object,
            selected.index,
            &self.substrate.entities,
            rules,
            &self.interner,
        );
        if range >= 512 {
            range -= 128;
        }
        let los = combat::line_of_fire::LineOfFireInputs {
            overlay_grid: self.overlay_grid.as_ref(),
            overlay_registry: registry,
            alliances: Some(&self.fog.alliances),
        };
        let can_fire = if let Some(identity) = identity {
            in_range::cell_target_in_range(
                actor,
                identity,
                selected.weapon,
                rules,
                &self.interner,
                &self.substrate.entities,
                terrain,
                &los,
            )
            .ok_or("Approach CanFireAt coordinate failed")?
        } else {
            combat::can_fire_at_target_in_query(
                &self.substrate.entities,
                rules,
                &self.interner,
                id,
                &target,
                &cells,
                Some(&self.house_alliances),
                &los,
                Some(&target),
            )
        };
        let mission = actor.mission.current().known();
        let human = self
            .houses
            .get(&actor.owner())
            .is_none_or(|h| h.is_controlled_by_human(self.session.game_mode_nonzero));
        let may_approach = object.can_approach_target
            && actor.drain_target.is_none()
            && actor.bunker_link.installed_in().is_none()
            && !actor.passenger_role.in_open_transport();
        if !can_fire
            && (mission == Some(MissionType::Sticky)
                || !may_approach
                    && !(mission == Some(MissionType::Attack)
                        || human && mission == Some(MissionType::AreaGuard)
                        || !human && mission == Some(MissionType::Hunt)))
        {
            self.assign_target_represented(id, None, Some(rules))
                .map_err(|e| format!("Approach target clear: {e}"))?;
            self.assign_null_destination(id, Some(rules), registry, frame_effects);
            return Ok(None);
        }

        // Read target+48 before NavCom+48. Neither retained Cell receiver is a
        // fresh coord-to-Cell query: a Dummy may already have been restamped.
        let cells = NativeCellQuery::canonical(terrain);
        let mut clear_nav = false;
        if let Some(nav) = actor.navigation.nav_com
            && object.can_recalc_approach_target
            && !can_fire
        {
            let own = self.approach_nav_center(target_reference, &cells)?;
            let nav = self.approach_nav_center(nav, &cells)?;
            clear_nav = distance_3d_leptons([own.x, own.y, own.z], [nav.x, nav.y, nav.z])
                > range.wrapping_mul(rules.general.approach_target_reset_multiplier);
        }
        if clear_nav {
            //4D59FB/5A01 are precisely FootStopMoving's two direct writes.
            // The independently retained locomotor head/destination stay live.
            super::navcom::foot_stop_moving(self.substrate.entities.get_mut(id).unwrap());
        }
        let actor = self.substrate.entities.get(id).unwrap();
        let terrain = self.resolved_terrain.as_ref().unwrap();
        let cells = NativeCellQuery::canonical(terrain);
        if actor.navigation.nav_com.is_some()
            && !super::air_movement::is_high_flying_in_query(
                actor,
                &cells,
                Some((rules, &self.interner)),
            )
            || can_fire && actor.in_playfield
        {
            return Ok(None);
        }
        if let Some(NavTargetRef::Cell { rx, ry }) = actor.navigation.nav_queue.first().copied() {
            if actor.category == EntityCategory::Unit {
                self.set_unit_destination(
                    id,
                    NavTargetRef::cell(rx, ry),
                    rules,
                    false,
                    frame_effects,
                );
            } else {
                //4D5A64 passes flag0. Infantry51AA40 forwards the flag,
                // but Foot4D94B0 never reads it; original ordinary CLEG
                // Cell-setter flag0/1 controls have equal state and RNG.
                self.assign_destination_represented(
                    id,
                    Some(NavTargetRef::cell(rx, ry)),
                    Some(rules),
                    registry,
                    frame_effects,
                )
                .map_err(|e| format!("Approach queued destination: {e}"))?;
            }
            let queue = &mut self
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .nav_queue;
            if !queue.is_empty() {
                queue.remove(0);
            }
            return Ok(Some((rx, ry)));
        }

        let foundation = match target {
            TargetKind::Entity(id) => self
                .substrate
                .entities
                .get(id)
                .filter(|t| t.category == EntityCategory::Structure)
                .map(|t| {
                    let (width, height) =
                        crate::rules::foundation::foundation_dimensions(&t.foundation);
                    (i32::from(width), i32::from(height))
                }),
            TargetKind::Cell(..) => None,
        };
        let mut radius = approach_search_radius(range, actor.category, foundation);
        let own = self.approach_nav_center(target_reference, &cells)?;
        let target_cell = ((own.x / 256) as i16, (own.y / 256) as i16);
        let source = super::ground_pose::position_world_coord(&actor.position);
        let facing = facing16_between([own.x, own.y], [source.x, source.y]);
        let facing_byte = ((u32::from(facing) + 128) >> 8) as u8;
        //4D5CBC..5CFE: Cell uses its flag; a Foot uses OnBridge. A
        // building has no Foot flag, even when its coordinate lies on a bridge.
        let target_bridge = match target {
            TargetKind::Cell(..) => cells.flags(identity.unwrap()) & 0x100 != 0,
            TargetKind::Entity(id) => self.substrate.entities.get(id).is_some_and(|target| {
                target.category != EntityCategory::Structure && target.on_bridge
            }),
        };
        //4D5F6F target+50 may resample ground and flags before any candidate.
        let _ = if let Some(identity) = identity {
            in_range::native_cell_range_coords(identity, &cells)
        } else {
            in_range::resolve_target_coords_3d(
                &target,
                &self.substrate.entities,
                rules,
                &self.interner,
                &cells,
            )
        }
        .ok_or("Approach target surface failed")?;
        let mut retained_candidate = target_cell;
        let mut chosen = None;
        'rings: while radius > 204 {
            for offset in OFFSETS {
                let direction = (i16::from(facing_byte).wrapping_add(offset) as u16) << 8;
                let [x, y] = facing_step_world_xy([own.x, own.y], direction, radius);
                let radial = ((x / 256) as i16, (y / 256) as i16);
                let mut point = cell_centre(radial);
                point.z = in_range::native_ground_target_z(point.x, point.y, &cells)
                    .ok_or("Approach candidate surface failed")? as i32;
                let in_range = |point: DriveCoord| {
                    in_range::compute_in_range_in_query(
                        actor,
                        (i64::from(point.x), i64::from(point.y), i64::from(point.z)),
                        &target,
                        selected.weapon,
                        rules,
                        &self.interner,
                        &self.substrate.entities,
                        &cells,
                        &combat::line_of_fire::LineOfFireInputs {
                            overlay_grid: self.overlay_grid.as_ref(),
                            overlay_registry: registry,
                            alliances: Some(&self.fog.alliances),
                        },
                    )
                };
                if !in_range(point) {
                    continue;
                }
                retained_candidate = radial;
                if !cell_rect::cell_is_in_playfield_height_aware_in_query(
                    (i32::from(radial.0), i32::from(radial.1)),
                    self.playfield_bounds,
                    Some(terrain),
                    Some(&cells),
                ) {
                    continue;
                }
                let mut candidate = radial;
                let mut admitted = self.approach_cell_is_clear(id, candidate, rules, &cells)?;
                if !admitted {
                    for direction in 0..8 {
                        let (dx, dy) = cell_delta_unchecked(direction);
                        candidate = (
                            candidate.0.wrapping_add(dx as i16),
                            candidate.1.wrapping_add(dy as i16),
                        );
                        //4D643D..671E: cumulative neighbors, Z0, no new bounds
                        // or ground lookup. Destination setting resolves Z later.
                        if in_range(cell_centre(candidate))
                            && self.approach_cell_is_clear(id, candidate, rules, &cells)?
                        {
                            admitted = true;
                            retained_candidate = candidate;
                            break;
                        }
                    }
                }
                if !admitted {
                    continue;
                }
                let candidate_bridge = cells.flags(cells.lookup(candidate)) & 0x100 != 0;
                if self.estimate_zone_cost(
                    id,
                    candidate,
                    target_cell,
                    candidate_bridge,
                    target_bridge,
                    rules,
                )? <= chebyshev(candidate, target_cell) + 8
                {
                    chosen = Some(candidate);
                    break 'rings;
                }
                let source_nav = self.foot_navigation_coordinate(id)?;
                let start = coord_cell(source_nav);
                let source_bridge = super::ground_pose::navigation_should_be_on_bridge(
                    &cells,
                    source_nav,
                    source,
                    actor.on_bridge,
                    actor.low_bridge_tube_state.is_some(),
                )?;
                if self.estimate_zone_cost(
                    id,
                    start,
                    candidate,
                    source_bridge,
                    candidate_bridge,
                    rules,
                )? <= chebyshev(start, candidate) + 8
                {
                    chosen = Some(candidate);
                    break 'rings;
                }
            }
            radius -= 256;
        }
        if let Some(cell) = chosen {
            self.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(cell.0 as u16, cell.1 as u16)),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|e| format!("Approach destination: {e}"))?;
            //4D68B8 and4D68C7 perform separate lookups for setter and return.
            self.resolved_terrain
                .as_ref()
                .unwrap()
                .native_cell_identity(cell);
            return Ok(Some((cell.0 as u16, cell.1 as u16)));
        }
        if self
            .object_type(actor.type_ref(), rules)
            .unwrap()
            .hunter_seeker
        {
            let cell = (target_cell.0 as u16, target_cell.1 as u16);
            self.set_unit_destination(
                id,
                NavTargetRef::cell(cell.0, cell.1),
                rules,
                true,
                frame_effects,
            );
            return Ok(Some(cell));
        }
        let fallback = self.approach_fallback(id, retained_candidate, rules, &cells)?;
        self.assign_target_represented(id, None, Some(rules))
            .map_err(|e| format!("Approach exhausted target clear: {e}"))?;
        if let Some(cell) = fallback {
            self.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(cell.0, cell.1)),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|e| format!("Approach fallback destination: {e}"))?;
        } else {
            self.assign_null_destination(id, Some(rules), registry, frame_effects);
        }
        Ok(None)
    }

    fn approach_nav_center(
        &self,
        target: NavTargetRef,
        cells: &NativeCellQuery<'_>,
    ) -> Result<DriveCoord, String> {
        let id = match target {
            NavTargetRef::Cell { rx, ry } => {
                let cell = TargetKind::Cell(rx, ry)
                    .cell_identity(cells.terrain())
                    .unwrap();
                let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(cell, cells)
                    .ok_or("NavCom Cell coordinate failed")?;
                return Ok(DriveCoord {
                    x: x as i32,
                    y: y as i32,
                    z: z as i32,
                });
            }
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => id,
        };
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Approach NavCom")?;
        Ok(super::ground_pose::object_get_coords(
            entity,
            Some(cells.terrain()),
        ))
    }

    fn approach_cell_is_clear(
        &self,
        id: u64,
        candidate: (i16, i16),
        rules: &RuleSet,
        cells: &NativeCellQuery<'_>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Approach actor retired")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach mover type absent")?;
        let zones = self.zone_grid.as_ref().ok_or("Approach requires zones")?;
        let source = coord_cell(self.foot_navigation_coordinate(id)?);
        let zone = zones
            .get_zone_id_native_in_query(
                cells.terrain(),
                (source.0 as u16, source.1 as u16),
                object.movement_zone,
                actor.on_bridge,
                Some(cells),
            )
            .ok_or("Approach source zone absent")?;
        Ok(cell_rect::check_cell_passability(
            &CellRectPassabilityContext {
                native_cells: Some(cells),
                rect: CellRect {
                    x: i32::from(candidate.0),
                    y: i32::from(candidate.1),
                    width: 1,
                    height: 1,
                },
                speed_type: object.speed_type,
                required_zone_id: (zone != u32::MAX).then_some(zone),
                movement_zone: object.movement_zone,
                required_height_or_level: None,
                bridge_aware_zone: true,
                reject_any_overlay: false,
                path_grid: self.path_grid.as_deref(),
                resolved_terrain: Some(cells.terrain()),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                zone_grid: Some(zones),
            },
            Some(&self.substrate.raw_cell_occupation),
            i32::from(candidate.0),
            i32::from(candidate.1),
        ))
    }

    fn approach_fallback(
        &self,
        id: u64,
        retained: (i16, i16),
        rules: &RuleSet,
        cells: &NativeCellQuery<'_>,
    ) -> Result<Option<(u16, u16)>, String> {
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs,
            find_nearby_passable_cell, map_owned_radius_cap,
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Approach actor retired")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach mover type absent")?;
        let source = self.foot_navigation_coordinate(id)?;
        //4D6994 reads the retained candidate before sourceShouldBeOnBridge.
        let bridge_aware = cells.flags(cells.lookup(retained)) & 0x100 != 0;
        let should_bridge = super::ground_pose::navigation_should_be_on_bridge(
            cells,
            source,
            super::ground_pose::position_world_coord(&actor.position),
            actor.on_bridge,
            actor.low_bridge_tube_state.is_some(),
        )?;
        let source = coord_cell(source);
        let zone = self
            .zone_grid
            .as_ref()
            .ok_or("Approach requires zones")?
            .get_zone_id_native_in_query(
                cells.terrain(),
                (source.0 as u16, source.1 as u16),
                object.movement_zone,
                should_bridge,
                Some(cells),
            )
            .ok_or("Approach source zone absent")?;
        let (bounds, height) = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .ok_or("Approach requires map dimensions")?;
        Ok(find_nearby_passable_cell(
            (i32::from(retained.0), i32::from(retained.1)),
            &NearbyQuery {
                native_cells: Some(cells),
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id: (zone != u32::MAX).then_some(zone),
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: bridge_aware,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: false,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(bounds.base, height),
                target_cell: None,
                path_grid: self.path_grid.as_deref(),
                resolved_terrain: Some(cells.terrain()),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: Some(bounds),
            },
            self.session.binary_frame,
        )
        .filter(|cell| *cell != (0, 0))
        .map(|(x, y)| (x as u16, y as u16)))
    }
}

/// Foot4D5B97..4D5BEE: building dimensions expand the search radius before
/// Infantry RTTI15 subtracts281.6; the other represented class subtracts179.2.
/// Integral inputs permit integer-only truncation toward zero, preserving
/// CRT ftol's consumed low i32 and the wrapping foundation addition.
/// Executed controls: fv_cell_attack/cleg_approach_vectors.json (original CRT,
/// both ambient precision controls); no host floating-point math is needed.
fn approach_search_radius(
    range: i32,
    class: EntityCategory,
    foundation: Option<(i32, i32)>,
) -> i32 {
    let range = foundation.map_or(range, |(width, height)| {
        range.wrapping_add(width.wrapping_add(height).wrapping_mul(64))
    });
    let whole = if class == EntityCategory::Infantry {
        281
    } else {
        179
    };
    // Original ftol truncates toward zero into i64, then consumes its low
    // i32. For a negative intermediate the fractional part truncates up;
    // the low word can wrap at i32::MIN (native corpus boundary controls).
    let subtraction = whole + i32::from(range > whole);
    let radius = range.wrapping_sub(subtraction).max(0);
    if radius > 0 { radius.max(256) } else { 0 }
}

/// The BalloonHover arm once SelectWeapon has answered
/// (`0x007415BA..0x007415F8`): a selected weapon whose `Projectile=` (+0xA0)
/// is `Vertical=` (+0x2C0) makes the Target the destination; no weapon, no
/// projectile or a level one goes on to the next arm.
fn balloon_hover_destination(
    weapon: Option<&crate::rules::weapon_type::WeaponType>,
    target: TargetKind,
    rules: &RuleSet,
) -> Option<NavTargetRef> {
    weapon?
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
        .is_some_and(|projectile| projectile.vertical)
        .then_some(match target {
            TargetKind::Entity(id) => NavTargetRef::object(id),
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
        })
}
