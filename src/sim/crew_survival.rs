//! Crew survival: the infantry that escape a destroyed building or vehicle.
//!
//! Owner of the two death-time producers and the pieces only they share:
//! - `BuildingClass::SpawnSurvivors @ 0x00442D90`, called by
//!   `BuildingClass::DestructionEffects` (`0x00441F1B`) while the building is
//!   still on the map: absorbed passengers leave first (Phase A), then each
//!   foundation cell gets one survivor roll followed by that cell's
//!   scorch/crater mark (Phase B);
//! - the crew block of `UnitClass::ReceiveDamage` (`0x007381BC..0x0073838A`);
//! - `BuildingClass::How_Many_Survivors @ 0x00451330`, the building crew pick
//!   `0x0044EB10` and `TechnoClass::GetCrew @ 0x00707D20`.
//!
//! Placement is `CellClass::PlaceInfantryInCell @ 0x00481180`
//! ([`bump_crush::place_infantry_in_cell`]); the exit is
//! `InfantryClass::Scatter(&EmptyCoord, 1, 0)` through the shared forced
//! Scatter arm. Every draw is on the Scenario stream. Aircraft never ask for
//! a crew: `Pilot=` has no gameplay reader and no aircraft caller reaches the
//! crew-type slot `vt+0x30C`.
//!
//! RESIDUALS:
//! - A second SpawnSurvivors after Limbo: DestructionEffects arms the death
//!   timer (`+0x528`) at 0 for an `Explodes=` type (TechnoType `+0xD15`) or a
//!   building killed while Selling (`0x00441C43..0x00441C8C`), so the building
//!   stays alive at 0 HP and `BuildingClass::Update` runs SpawnSurvivors again
//!   after its Limbo (`0x004400D4`), then UnInit. VERA UnInits at once and
//!   runs one round. Trigger: every NANRCT death (the only stock `Crewed=yes`
//!   `Explodes=yes` building) and any crewed building killed while sold.
//!   Effect: one survivor round and its per-cell marks instead of two.
//!   Frequency: every Soviet Nuclear Reactor death. Risk: survivor count,
//!   marks and the Scenario stream after them. Needs a breakpoint at
//!   `0x004400D4` (kill a NANRCT) to confirm Update reaches that arm.
//! - Sale crew (`Mission_Selling` Status 1, `0x0044A2EE`): a sale keeps the
//!   older VERA survivor adapter until the Mission_Selling port, which can
//!   reuse this count. Trigger: every sale of a crewed building. Effect:
//!   invented survivor count, type and cells.
//! - Passenger escape (`UnitClass::ReceiveDamage` `0x00737FB0..0x007381B6`):
//!   a dying unit that is not `Crashable=` (`+0xD95`) Unlimboes each passenger
//!   at its Location and Scatters it (computer passengers join a team or
//!   Hunt); IgnoreDefenses, a falling unit or a Can_Enter_Cell refusal give
//!   RecordKill and UnInit instead. VERA's fatal prelude purges all cargo.
//!   Trigger: every destroyed loaded transport (IFV, Flak Track, Battle
//!   Fortress, amphibious transport on land). Effect: the infantry inside die
//!   with it instead of stepping out. Frequency: common. Risk: unit counts and
//!   Scenario draws. Its own mechanism.
//! - Scatter's FNPC failure arm (the eight-neighbour fallback and
//!   QueueMission(Move) inside `InfantryClass::Scatter @ 0x0051D0D0`) is not
//!   ported: the crewman stays where it landed, with its mission still
//!   queued. Trigger: no passable cell at its height within the FNPC radius,
//!   e.g. a building straddling a cliff ledge. Frequency: rare (buildings
//!   stand on level ground). Risk: position only. Likewise a crewman on a
//!   cell the path grid marks unwalkable loses its Scatter destination in the
//!   immediate Process (the movement owner refuses a blocked start cell);
//!   only seen with a building placed partly on such ground.
//! - House IsToDie (`+0x1F6`, set by `0x004FC980`): VERA has no resign
//!   countdown, so it never suppresses the survivor roll. Frequency: only a
//!   resigning house's buildings.
//! - A survivor's Doing right after Unlimbo decides whether Scatter's table
//!   gate admits it (`0x0051D1AA`); VERA's fresh infantry passes it. Needs a
//!   breakpoint on `0x0051D0D0` with a survivor.
//! - The Unlimbo usable-area arm: a vehicle crewman whose cell lies outside
//!   the usable map area places with priority (no occupancy test, no draw;
//!   `0x0051E06B`, `0x00578460`); VERA always runs the ordinary placement.
//!   Trigger: a vehicle dying on the unusable map rim. Rare.
//! - HijackerType (Unit `+0x338`): VERA has no hijacking, so the always-exit
//!   hijacker arm is unreachable.
//! - The crewman takes the vehicle's selection (vt+0x14C, local player) and
//!   tag (`0x006E57C0`/`0x005F5B50`): selection is presentation and VERA has
//!   no per-object tags.
//! - Phase A bookkeeping: the House `+0x2F4` counter and the passenger
//!   `+0x438`/`+0x439` flags, and a refused passenger's kill credit to a
//!   Techno C4AppliedBy (vt+0xE0) before its UnInit. A UnitAbsorb passenger
//!   (no stock building) leaves without its Scatter.
//! - The Nominal survivor flag (Infantry `+0x6D9`, read by
//!   `HouseClass::Added_To_Game @ 0x00502C3C`) is not represented.
//! - A Bio Reactor holding more infantry than foundation cells reads its
//!   list's sentinel for the extra passenger and starts Phase B past the list
//!   (open native question); VERA puts the extra passenger's dead
//!   PlaceInfantryInCell draw on the origin cell and skips Phase B.

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::object_type::FactoryType;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation, UninitContext,
};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

/// Phase B's in-cell request `(cell.x*256+0x80, cell.y*256+0xA4)`: 36 leptons
/// south of the centre, inside the 60-lepton centre radius, so
/// PlaceInfantryInCell takes its centre row draw.
const SURVIVOR_REQUEST_X: i32 = 0x80;
const SURVIVOR_REQUEST_Y: i32 = 0xA4;

/// The building's foundation cells in `vt+0x108(0)` list order (the order
/// Phase B walks), from its origin cell.
pub(crate) fn foundation_cells(rx: u16, ry: u16, foundation: &str) -> Vec<(u16, u16)> {
    crate::rules::foundation::foundation_cell_offsets(foundation)
        .into_iter()
        .filter_map(|(dx, dy)| {
            let x = u16::try_from(i32::from(rx) + i32::from(dx)).ok()?;
            let y = u16::try_from(i32::from(ry) + i32::from(dy)).ok()?;
            Some((x, y))
        })
        .collect()
}

/// `r * 1/0x7FFFFFFE < CrewEscape` in the native x87 order
/// (`0x00738218..0x0073822D`: FILD, FMUL `0x007E3570`, FCOMP `Rules+0x5C0`,
/// then `TEST AH,1` on C0, which an unordered compare also sets). For the
/// stock 50% it is exactly `r < 0x40000000`.
fn crew_escapes(roll: u32, crew_escape: NativeF64Bits) -> bool {
    let scaled = X87::mul(
        X87::load_i32(roll as i32),
        X87::load_f64(crate::sim::rng::RANDOM_RANGED_UNIT_SCALE),
    );
    matches!(
        X87::compare(scaled, X87::load_f64(crew_escape)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
    )
}

/// Where a new crewman Unlimboes (`InfantryClass::Unlimbo @ 0x0051DFF0`).
enum CrewUnlimbo {
    /// PlaceInfantryInCell on the ground plane at `request` inside `cell`,
    /// then the chosen spot on the cell floor `z`.
    Place {
        cell: (u16, u16),
        z: u8,
        request: (SimFixed, SimFixed),
    },
    /// A coordinate above the floor (`0x0051E01B`): no placement and no draw;
    /// the crewman keeps the exact coordinate and the vehicle's OnBridge.
    Exact {
        rx: u16,
        ry: u16,
        z: u8,
        sub_x: SimFixed,
        sub_y: SimFixed,
        on_bridge: bool,
    },
}

impl Simulation {
    /// `TechnoClass::GetCrew @ 0x00707D20` for a Crewed type owned by a house
    /// of `side` (House `+0x1E8`): the side's crew, Technician for any other
    /// side, and a 15% Technician roll for an armed object. A country with no
    /// `Side=` (HouseType `+0xBC == -1`, none in stock) would return
    /// Technician before the roll; `side_index` cannot express it.
    fn techno_crew_type(&mut self, rules: &RuleSet, side: u8, armed: bool) -> Option<String> {
        let general = &rules.general;
        let crew = match side {
            0 => general.allied_crew.clone(),
            1 => general.soviet_crew.clone(),
            2 => general.third_crew.clone(),
            _ => general.technician.clone(),
        };
        if armed && self.scenario_rng.next_range_u32_inclusive(0, 99) < 15 {
            return general.technician.clone();
        }
        crew
    }

    /// Building vt+0x30C `0x0044EB10`: an uncaptured building always draws
    /// the Engineer roll, which only a `Factory=BuildingType` yard can win,
    /// then falls to GetCrew. A garrison's IsArmed arm (`0x00458DD0`) is
    /// false here: its occupants were ejected before the death effects.
    fn building_crew_type(&mut self, rules: &RuleSet, building_id: u64) -> Option<String> {
        let entity = self.substrate.entities.get(building_id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        if !object.crewed {
            return None;
        }
        let captured = entity.has_been_captured;
        let armed = crate::sim::combat::combat_weapon::is_armed(entity, object);
        let yard = object.factory == Some(FactoryType::BuildingType);
        let side = self.houses.get(&entity.owner())?.side_index;
        if !captured && self.scenario_rng.next_range_u32_inclusive(0, 99) < 25 && yard {
            return rules.general.engineer_infantry.clone();
        }
        self.techno_crew_type(rules, side, armed)
    }

    /// `BuildingClass::How_Many_Survivors @ 0x00451330`: zero when
    /// NoSurvivor (`+0x6E0`), uncrewed or owned by a house outside the three
    /// sides; otherwise the refund over the side's divisor (doubled once
    /// captured), clamped to 1..5.
    fn building_survivor_count(&self, rules: &RuleSet, building_id: u64, no_survivor: bool) -> i32 {
        let Some(entity) = self.substrate.entities.get(building_id) else {
            return 0;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return 0;
        };
        if no_survivor || !object.crewed {
            return 0;
        }
        let Some(house) = self.houses.get(&entity.owner()) else {
            return 0;
        };
        let divisor = match house.side_index {
            0 => rules.general.allied_survivor_divisor,
            1 => rules.general.soviet_survivor_divisor,
            2 => rules.general.third_survivor_divisor,
            _ => return 0,
        };
        if divisor == 0 {
            return 0;
        }
        let divisor = if entity.has_been_captured {
            divisor.wrapping_add(divisor)
        } else {
            divisor
        };
        let refund = crate::sim::production::building_type_refund(
            rules,
            object,
            house,
            self.session.game_mode_nonzero,
            false,
        );
        refund.checked_div(divisor).unwrap_or(0).clamp(1, 5)
    }

    /// `BuildingClass::SpawnSurvivors @ 0x00442D90` for a dying building
    /// still on the map. `no_survivor` is the killing ReceiveDamage's
    /// IgnoreDefenses, which DestructionEffects stores in `+0x6E0`
    /// (`0x00441EFC..0x00441F0B`); a C4 expiry passes it set (`0x00440345`).
    /// `commit_cell_smudge` places one foundation cell's scorch/crater mark
    /// through the smudge owner, after that cell's survivor roll.
    pub(crate) fn spawn_building_survivors(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        building_id: u64,
        no_survivor: bool,
        mut commit_cell_smudge: impl FnMut(&mut Simulation, (u16, u16)),
    ) {
        let Some(entity) = self.substrate.entities.get(building_id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let owner = entity.owner();
        let origin = (entity.position.rx, entity.position.ry);
        let captured = entity.has_been_captured;
        let absorbs = object.infantry_absorb || object.unit_absorb;
        let cells = foundation_cells(origin.0, origin.1, &object.foundation);
        // C4AppliedBy (+0x540); the pointer-expiry broadcast clears it when
        // the planter leaves play.
        let c4_source = entity
            .pending_c4_detonation
            .and_then(|pending| pending.source_entity_id);
        let chance_max = if c4_source.is_some() { 1 } else { 2 } + if captured { 6 } else { 0 };
        let mut count = self.building_survivor_count(rules, building_id, no_survivor);

        // Phase A (0x00442DF2..0x00443011): every absorbed passenger advances
        // the foundation cursor that Phase B then continues from.
        let mut cursor = 0;
        if absorbs {
            while let Some((passenger, _)) = self
                .substrate
                .entities
                .get_mut(building_id)
                .and_then(|building| building.passenger_role.cargo_mut())
                .and_then(|cargo| cargo.unload_first())
            {
                let cell = cells.get(cursor).copied().unwrap_or(origin);
                cursor += 1;
                self.eject_absorbed_passenger(
                    rules,
                    registry,
                    building_id,
                    passenger,
                    cell,
                    no_survivor,
                );
            }
        }

        // Phase B (0x00443017..0x004433F4): nothing at all, smudges
        // included, when no survivor is owed.
        if count == 0 {
            return;
        }
        for &cell in cells.iter().skip(cursor) {
            if count > 0
                && self.scenario_rng.next_range_u32_inclusive(0, chance_max) == 1
                && let Some(crew) = self.building_crew_type(rules, building_id)
                && self.spawn_building_survivor(rules, registry, &crew, owner, cell, c4_source)
            {
                count -= 1;
            }
            commit_cell_smudge(self, cell);
        }
    }

    /// One Phase B survivor: construct (the TechnoClass constructor's draw),
    /// place in the cell, Unlimbo, Health `RandomRanged(5, Strength)`,
    /// Scatter, then Attack an enemy C4 planter, else Move for a human owner
    /// or Hunt for the computer.
    fn spawn_building_survivor(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        crew: &str,
        owner: InternedId,
        cell: (u16, u16),
        c4_source: Option<u64>,
    ) -> bool {
        let z = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .map_or(0, |terrain_cell| terrain_cell.level);
        let request = (
            SimFixed::from_num(SURVIVOR_REQUEST_X),
            SimFixed::from_num(SURVIVOR_REQUEST_Y),
        );
        let Some(id) =
            self.construct_crew(rules, crew, owner, CrewUnlimbo::Place { cell, z, request })
        else {
            return false;
        };
        let strength = rules.object(crew).map_or(0, |object| object.strength);
        let health = self.scenario_rng.next_range_i32_inclusive(5, strength);
        self.set_crew_health(id, health);
        self.scatter_crew(rules, registry, id);

        // `HouseClass::IsAlliedWith(object) @ 0x004F9AF0` on the building owner.
        let owner_name = self.interner.resolve(owner);
        let enemy_planter = c4_source.filter(|&planter| {
            self.substrate.entities.get(planter).is_some_and(|planter| {
                !crate::map::houses::is_allied_with(
                    &self.house_alliances,
                    owner_name,
                    self.interner.resolve(planter.owner()),
                )
            })
        });
        let mission = if enemy_planter.is_some() {
            MissionType::Attack
        } else if self.house_is_human(owner) {
            MissionType::Move
        } else {
            MissionType::Hunt
        };
        self.queue_crew_mission(id, mission);
        if let Some(planter) = enemy_planter {
            // Infantry vt+0x3C8 (`0x0051B1F0`) over TechnoClass::Assign_Target.
            let target = Some(crate::sim::combat::TargetKind::Entity(planter));
            let commits = crate::sim::mission::concrete_effects::assign_target_commits(
                &self.substrate.entities,
                target,
            );
            if let Some(survivor) = self.substrate.entities.get_mut(id) {
                crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                    survivor, target, commits,
                );
            }
        }
        true
    }

    /// One Phase A passenger of an absorbing building. An infantryman spends
    /// the PlaceInfantryInCell draw for its foundation cell (the result is
    /// overwritten), then every passenger Unlimboes at the building Location
    /// in priority mode (no second draw), Scatters, and Hunts for a computer
    /// building owner. NoSurvivor or a refused Unlimbo UnInits it instead.
    fn eject_absorbed_passenger(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        building_id: u64,
        passenger: u64,
        cell: (u16, u16),
        no_survivor: bool,
    ) {
        let Some(infantry) = self
            .substrate
            .entities
            .get(passenger)
            .map(|entity| entity.category == EntityCategory::Infantry)
        else {
            return;
        };
        if infantry {
            let _overwritten = bump_crush::place_infantry_in_cell(
                &self.substrate.raw_cell_occupation,
                cell.0,
                cell.1,
                MovementLayer::Ground,
                SimFixed::from_num(SURVIVOR_REQUEST_X),
                SimFixed::from_num(SURVIVOR_REQUEST_Y),
                &mut self.scenario_rng,
            );
        }
        let Some(building) = self.substrate.entities.get(building_id) else {
            return;
        };
        let owner = building.owner();
        let on_bridge = building.on_bridge;
        let facing = building.facing;
        let position = building.position.clone();
        let sub_cell =
            infantry.then(|| bump_crush::priority_sub_cell(position.sub_x, position.sub_y));
        // The dying building's expiry broadcast already cleared the
        // passenger's transporter link; the cargo list still held it.
        if let Some(entity) = self.substrate.entities.get_mut(passenger) {
            entity.passenger_role = crate::sim::passenger::PassengerRole::None;
            entity.on_bridge = on_bridge;
            entity.facing = facing;
            entity.sub_cell = sub_cell;
        }
        let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(sub_cell);
        let revealed = !no_survivor
            && matches!(
                self.try_reveal_entity_with_context(
                    passenger,
                    RevealRequest {
                        position: RevealPosition {
                            rx: position.rx,
                            ry: position.ry,
                            z: position.z,
                            sub_x,
                            sub_y,
                        },
                        placement: PlacementEvidence::MarkSucceeded,
                        logic_eligible: true,
                    },
                    UninitContext::with_rules(rules),
                ),
                RevealOutcome::Revealed { .. }
            );
        if !revealed {
            self.uninit_with_rules(passenger, rules);
            return;
        }
        if infantry {
            self.scatter_crew(rules, registry, passenger);
        }
        if !self.house_is_human(owner) {
            self.queue_crew_mission(passenger, MissionType::Hunt);
        }
    }

    /// The crew block of `UnitClass::ReceiveDamage` (`0x007381BC..0x0073838A`),
    /// after the dying unit's Mark(UP). `prevent_escape` is ReceiveDamage's
    /// arg6. A crewed vehicle with no passenger capacity rolls CrewEscape; an
    /// escaped crewman leaves from the vehicle's own coordinate with Health
    /// `RandomRanged(5, Strength/2)`, and Guards for a human owner or Hunts
    /// for the computer.
    pub(crate) fn spawn_vehicle_crew(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        unit_id: u64,
        prevent_escape: bool,
    ) {
        if prevent_escape {
            return;
        }
        let Some(entity) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !object.crewed || object.passengers != 0 {
            return;
        }
        let owner = entity.owner();
        let armed = crate::sim::combat::combat_weapon::is_armed(entity, object);
        let on_bridge = entity.on_bridge;
        let position = entity.position.clone();
        let Some(side) = self.houses.get(&owner).map(|house| house.side_index) else {
            return;
        };
        // `InfantryClass::Unlimbo @ 0x0051DFF0` places through
        // PlaceInfantryInCell (ground plane, `0x0051E07F`) only when the
        // coordinate's Z is the floor at its XY (`0x0051E01B`): a vehicle on a
        // bridge deck, or lifted off the ground, leaves its crewman at its own
        // coordinate with no placement draw.
        let world_xy = crate::sim::movement::ground_pose::position_world_xy(&position);
        let floor = crate::sim::movement::ground_pose::ground_surface_z_at(
            world_xy,
            false,
            self.resolved_terrain.as_ref(),
            self.path_grid(),
        );
        let off_floor = on_bridge
            || position
                .exact_z_leptons
                .zip(floor)
                .is_some_and(|(z, floor)| z != floor);
        let unlimbo = if off_floor {
            CrewUnlimbo::Exact {
                rx: position.rx,
                ry: position.ry,
                z: position.z,
                sub_x: position.sub_x,
                sub_y: position.sub_y,
                on_bridge,
            }
        } else {
            CrewUnlimbo::Place {
                cell: (position.rx, position.ry),
                z: position.z,
                request: (position.sub_x, position.sub_y),
            }
        };

        let roll = self.scenario_rng.next_range_u32_inclusive(0, 0x7FFF_FFFE);
        if !crew_escapes(roll, rules.general.crew_escape) {
            return;
        }
        let Some(crew) = self.techno_crew_type(rules, side, armed) else {
            return;
        };
        let Some(id) = self.construct_crew(rules, &crew, owner, unlimbo) else {
            return;
        };
        // Signed Strength/2 (CDQ; SUB; SAR).
        let strength = rules.object(&crew).map_or(0, |object| object.strength);
        let health = self.scenario_rng.next_range_i32_inclusive(5, strength / 2);
        self.set_crew_health(id, health);
        self.scatter_crew(rules, registry, id);
        let mission = if self.house_is_human(owner) {
            MissionType::Guard
        } else {
            MissionType::Hunt
        };
        self.queue_crew_mission(id, mission);
    }

    /// `new InfantryClass(type, Owner)` (the TechnoClass constructor's
    /// Scenario draw), then Unlimbo as `unlimbo` says. A refused cell or
    /// Unlimbo deletes the new object; its constructor draw stays spent.
    fn construct_crew(
        &mut self,
        rules: &RuleSet,
        crew: &str,
        owner: InternedId,
        unlimbo: CrewUnlimbo,
    ) -> Option<u64> {
        let owner_name = self.interner.resolve(owner).to_string();
        let (rx, ry, z) = match unlimbo {
            CrewUnlimbo::Place { cell, z, .. } => (cell.0, cell.1, z),
            CrewUnlimbo::Exact { rx, ry, z, .. } => (rx, ry, z),
        };
        let id = self.construct_object_limbo_at_height(crew, &owner_name, rx, ry, 0, z, rules)?;
        let (spot, sub_x, sub_y, on_bridge) = match unlimbo {
            CrewUnlimbo::Place { cell, request, .. } => {
                let Some(spot) = bump_crush::place_infantry_in_cell(
                    &self.substrate.raw_cell_occupation,
                    cell.0,
                    cell.1,
                    MovementLayer::Ground,
                    request.0,
                    request.1,
                    &mut self.scenario_rng,
                ) else {
                    self.discard_constructed_limbo(id);
                    return None;
                };
                let (sub_x, sub_y) = crate::util::lepton::subcell_lepton_offset(Some(spot));
                (spot, sub_x, sub_y, false)
            }
            CrewUnlimbo::Exact {
                sub_x,
                sub_y,
                on_bridge,
                ..
            } => (
                bump_crush::priority_sub_cell(sub_x, sub_y),
                sub_x,
                sub_y,
                on_bridge,
            ),
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.sub_cell = Some(spot);
            entity.on_bridge = on_bridge;
        }
        let outcome = self.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition {
                    rx,
                    ry,
                    z,
                    sub_x,
                    sub_y,
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(rules),
        );
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            self.discard_constructed_limbo(id);
            return None;
        }
        Some(id)
    }

    fn set_crew_health(&mut self, id: u64, health: i32) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.health.current = health;
        }
    }

    /// `InfantryClass::Scatter(&EmptyCoord, 1, 0)` through the shared forced
    /// arm. An arm VERA does not port yet leaves the crewman where it landed
    /// (module residuals). The immediate Process's bridge-state flag, which
    /// the hut caller propagates, is dropped: a crewman's first walk step does
    /// not change bridge state.
    fn scatter_crew(&mut self, rules: &RuleSet, registry: Option<&OverlayTypeRegistry>, id: u64) {
        if let Err(cause) = self.scatter_infantry_forced_from_empty(id, rules, registry) {
            log::debug!("crew {id} did not scatter: {cause}");
        }
    }

    fn queue_crew_mission(&mut self, id: u64, mission: MissionType) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(mission),
            );
        }
    }

    /// `HouseClass::IsControlledByHuman @ 0x0050B730`.
    fn house_is_human(&self, owner: InternedId) -> bool {
        self.houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero))
    }
}

#[cfg(test)]
#[path = "crew_survival_tests.rs"]
mod tests;
