//! Crew survival: the infantry that leave a building or vehicle as it dies
//! or is sold.
//!
//! Owner of the three crew producers and the pieces they share:
//! - `BuildingClass::SpawnSurvivors @ 0x00442D90`, called by
//!   `BuildingClass::DestructionEffects` (`0x00441F1B`) while the building is
//!   still on the map: absorbed passengers leave first (Phase A), then each
//!   foundation cell gets one survivor roll followed by that cell's
//!   scorch/crater mark (Phase B);
//! - `BuildingClass::Mission_Selling`'s stage 1 (`0x0044A2EE`, driven by
//!   `production::production_sell`): absorbed passengers, then the survivor
//!   count's crew, each on a random foundation cell;
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
//! Evidence: `tools/spatial_oracle/building_sale.json` `crew` rows (Sell's
//! stage 1, each crewman's Scatter setter and first Walk Process run
//! natively; `Find_Path` answered with the one-step route), replayed by
//! `building_sale_oracle_tests`.
//!
//! Escapes retain one call-scoped DWORD A8E7AC owner through Unlimbo,
//! Scatter and synchronous Walk: fatal crew443141..443288, absorbed
//! passengers442EDE..442FFC, dying unit passengers738030..7381A8. Sale
//! crew instead lowers it44A74E before Scatter44A768 and raises it only
//! for QueueMove44A76E..44A794. First building placement has explicit
//! priority=false; Unlimbo reads the live counter for its second placement.
//! The shared entry owner preserves the independent raw-owner and earlier
//! Building/wall gates. Original comparisons are in
//! `tools/spatial_oracle/infantry_scatter_entry_priority.json` and the joined
//! fatal `building_death_anims_joined.json`; the latter includes original
//! CRT48E480 initialization of the five89E9F0 subcell offsets.
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
//! - A sale's crew Unlimboes at the Z its request carries, 0
//!   (`0x0044A6C6`); VERA's survivor request carries the cell's level.
//!   Infantry51E01B compares that raw input before the shared type clamp
//!   (`0x005247D0`) raises a below-floor coordinate. Trigger: selling a
//!   crewed building on raised ground. Effect: the class placement arm and
//!   its occupation result may differ; final below-floor Z is clamped in
//!   both engines. Not executed (the sale oracle map is flat).
//! - A dying unit's passengers ([`Simulation::release_dying_unit_passengers`]):
//!   - IsABomb (`+0x8F`) kills every passenger (`0x007380AF`). Only
//!     `ObjectClass::DropAsBomb @ 0x005F4160` sets it: the deck pass of
//!     `CellClass::BlowUpBridge` (`0x0047DDC9`), a Foot whose deck vanished
//!     (`0x004D8D53`) and a locomotor below its floor (`0x00514C0C`). The
//!     object then falls and, once landed, takes its Strength as
//!     C4Warhead with IgnoreDefenses (`ObjectClass::AI` `0x005F4021`),
//!     which kills its passengers too. VERA's DropIn
//!     (`drop_in_bridge_member`) sets no byte and lands the object alive.
//!     Trigger: a loaded transport on a collapsing bridge. Effect: the
//!     transport and its passengers live on, and escape if it dies later.
//!     Frequency: rare. Risk: unit counts. Part of the bridge-fall
//!     mechanism.
//!   - A computer passenger of a unit in a Team joins it (`TeamClass::
//!     Add_Member @ 0x006EA500`) instead of Hunting, and KillPassengers
//!     takes each passenger out of its own Team (`0x006EA870`). VERA skips
//!     the Hunt but makes neither call. Production creates no teams, so
//!     both arms are dormant.
//!   - A vehicle passenger (in `SizeLimit=6` amphibious transports) is not
//!     Scattered (`UnitClass::Scatter @ 0x00743A50`), and its Unlimbo
//!     (`0x00737BA0`) still leaves `+0x220` unchanged. Secondary facing now
//!     follows its shared lifecycle writer at `0x00737BD2`. Trigger: a
//!     transport carrying vehicles destroyed ashore.
//!     Effect: the first vehicle stays on the cell with no NavCom, so the
//!     next one's Can_Enter_Cell sees a stationary unit (code 6) and it
//!     dies where native lets it out. Frequency: uncommon. Risk: unit
//!     counts and the Scenario stream. The next path of this mechanism.
//!   - The selection hand-over reads `HouseClass::IsHumanPlayer
//!     @ 0x0050B6F0` as "owned by the local player". That is its skirmish
//!     arm. In a campaign it admits any house with IsHuman (`+0x1EC`) or
//!     PlayerControl (`+0x1ED`). Trigger: a selected unit of a
//!     player-controlled allied campaign house dying. Effect: its escapees
//!     and crewman stay unselected. Frequency: rare.
//!   - VERA's Select sets the flag only. `TechnoClass::Select @ 0x006FBFA0`
//!     also runs ObjectClass::Select's gates (`0x005F4520`), the object's
//!     tag event 0x21 and the selection list add (`0x00637840`). For the
//!     local player's objects it also plays the select response (vtable
//!     `+0x360`, behind `[0x00822CF2]`). Trigger: every selected escapee
//!     and crewman. Effect: no select voice, and no tag event (VERA has no
//!     object tags). Frequency: whenever the player's selected transport or
//!     vehicle dies. Risk: presentation only.
//!   - A foreign open-topped escapee's Assign_Target(NULL) runs
//!     `InfantryClass::Assign_Target`'s head (`0x0051B203..0x0051B24F`).
//!     The head clears the fire latch `+0x68D`, then forces Deployed,
//!     Prone or Ready by its Doing. VERA clears the target only. Trigger:
//!     an open-topped transport dying with a passenger of another house.
//!     Frequency: rare. Risk: that escapee's Doing.
//!
//!   Each escapee spends Scatter's RandomRanged(0,4). Immediate Walk can
//!   place a center request ordinarily; a raised A8E7AC suppresses that draw.
//!   Initialized48E480 offsets and explicit center/corner controls are
//!   retained in the crew-priority evidence; cold offsets are superseded.
//! - A crewman on a cell the path grid marks unwalkable loses its Scatter
//!   destination in the immediate Process (the movement owner refuses a
//!   blocked start cell); only seen with a building placed partly on such
//!   ground.
//! - House IsToDie (`+0x1F6`, set by `0x004FC980`): VERA has no resign
//!   countdown, so it never suppresses the survivor roll. Frequency: only a
//!   resigning house's buildings.
//! - Original whole GAPOWR fatal/517A50/51DFF0 controls establish fresh E1
//!   Doing=-1 and Scatter admission at51D1AA. Different preexisting passenger
//!   Doing states remain bounded by their own Scatter controls.
//! - HijackerType (Unit `+0x338`): VERA has no hijacking, so the always-exit
//!   hijacker arm is unreachable.
//! - The crewman takes the vehicle's tag (`0x006E57C0`/`0x005F5B50`): VERA
//!   has no per-object tags.
//! - Absorbed-passenger bookkeeping: the House `+0x2F4` counter and the
//!   passenger `+0x438`/`+0x439` flags, and a refused passenger's kill credit
//!   to a Techno C4AppliedBy (vt+0xE0) before its UnInit. A UnitAbsorb
//!   passenger (no stock building) leaves without its Scatter in Phase A and
//!   with it in a sale; VERA scatters neither.
//! - The Nominal survivor flag (Infantry `+0x6D9`, set from the crew type's
//!   `+0xC9E`; a sale sets it at `0x0044A747`; read by
//!   `HouseClass::Added_To_Game @ 0x00502C3C`) is not represented.
//! - The off-map cell a passenger past the occupy list places in keeps the
//!   coordinate its lookup stamps (`MapClass::operator[]`); VERA's absorbed
//!   exit reads the off-map cell's bytes without stamping it. After Phase A
//!   such a Bio Reactor (more infantry than foundation cells) starts Phase B
//!   past the list (open native question); VERA skips Phase B's cells.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::FactoryType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation,
    UninitContext,
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

/// Where a crewman or an escaping passenger Unlimboes
/// (`InfantryClass::Unlimbo @ 0x0051DFF0`, `UnitClass::Unlimbo @ 0x00737BA0`).
#[derive(Clone, Copy)]
enum CrewUnlimbo {
    /// An infantryman on the floor: PlaceInfantryInCell on the ground plane at
    /// `request` inside `cell`, then the chosen spot on the cell floor `z`.
    /// A building caller places first with explicit priority=false. Unlimbo
    /// then places the returned coordinate using the live A8E7AC counter;
    /// that second priority placement spends no draw. Vehicle crew and
    /// passengers enter directly through Unlimbo instead.
    Place {
        cell: (u16, u16),
        z: u8,
        raw_z: Option<i32>,
        request: (SimFixed, SimFixed),
        caller_places_first: bool,
    },
    /// A coordinate above the floor (`0x0051E01B`), or any Unit's: no
    /// placement and no draw; the object keeps the exact coordinate and the
    /// given OnBridge.
    Exact {
        rx: u16,
        ry: u16,
        z: u8,
        raw_z: Option<i32>,
        sub_x: SimFixed,
        sub_y: SimFixed,
        on_bridge: bool,
    },
}

impl CrewUnlimbo {
    /// The cell and level the object is revealed at.
    fn cell_level(self) -> (u16, u16, u8) {
        match self {
            Self::Place { cell, z, .. } => (cell.0, cell.1, z),
            Self::Exact { rx, ry, z, .. } => (rx, ry, z),
        }
    }
}

/// What `UnitClass::ReceiveDamage` holds for a dying transport's passenger
/// block: the killing call's attacker (arg4, credited for each passenger that
/// dies) and IgnoreDefenses (arg5), and whether the unit was selected by the
/// local player on entry (`0x00737C98..0x00737CB6`: IsSelected `+0x83` and
/// `HouseClass::IsHumanPlayer @ 0x0050B6F0`), before the kill's Destroy
/// callback (`ObjectClass::Detach_All @ 0x005F5280`) deselected it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DyingTransport {
    pub(crate) attacker: Option<u64>,
    pub(crate) ignore_defenses: bool,
    pub(crate) selected_by_player: bool,
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
    pub(crate) fn building_survivor_count(
        &self,
        rules: &RuleSet,
        building_id: u64,
        no_survivor: bool,
    ) -> i32 {
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
        let Some(factors) = self.house_cost_factors(entity.owner()) else {
            return 0;
        };
        let refund = crate::sim::production::type_refund(
            rules,
            object,
            house,
            &factors,
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
        effects: FrameEffects<'_>,
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
        let cursor = self.eject_absorbed_passengers(
            rules,
            registry,
            building_id,
            &cells,
            no_survivor,
            effects,
        );

        // Phase B (0x00443017..0x004433F4): nothing at all, smudges
        // included, when no survivor is owed.
        if count == 0 {
            return;
        }
        for &cell in cells.iter().skip(cursor) {
            if count > 0
                && self.scenario_rng.next_range_u32_inclusive(0, chance_max) == 1
                && let Some(crew) = self.building_crew_type(rules, building_id)
                && self.spawn_building_survivor(
                    rules, registry, &crew, owner, cell, c4_source, effects,
                )
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
        effects: FrameEffects<'_>,
    ) -> bool {
        let unlimbo = self.survivor_unlimbo(cell);
        let (_, _, z) = unlimbo.cell_level();
        let Some(id) = self.construct_crew_limbo(rules, crew, owner, cell, z) else {
            return false;
        };
        //443141 raises A8E7AC after the constructor and before the caller's
        //ordinary481180. It remains raised through Scatter/Walk and the
        //mission queue, and is decremented at443288, including failed exits.
        self.with_object_placement_scope(|sim| {
            if !sim.unlimbo_crew(rules, id, unlimbo, None, registry, effects) {
                sim.discard_constructed_limbo(id, Some(rules), effects);
                return false;
            }
            let strength = rules.object(crew).map_or(0, |object| object.strength);
            let health = sim.scenario_rng.next_range_i32_inclusive(5, strength);
            sim.set_crew_health(id, health);
            sim.scatter_crew(rules, registry, id, effects);

            // `HouseClass::IsAlliedWith(object) @ 0x004F9AF0` on the building owner.
            let owner_name = sim.interner.resolve(owner);
            let enemy_planter = c4_source.filter(|&planter| {
                sim.substrate.entities.get(planter).is_some_and(|planter| {
                    !crate::map::houses::is_allied_with(
                        &sim.house_alliances,
                        owner_name,
                        sim.interner.resolve(planter.owner()),
                    )
                })
            });
            let mission = if enemy_planter.is_some() {
                MissionType::Attack
            } else if sim.owner_is_human(owner) {
                MissionType::Move
            } else {
                MissionType::Hunt
            };
            sim.queue_crew_mission(id, mission);
            if let Some(planter) = enemy_planter {
                // Infantry vt+0x3C8 (`0x0051B1F0`) over TechnoClass::Assign_Target.
                let target = Some(crate::sim::combat::TargetKind::Entity(planter));
                let commits = crate::sim::mission::concrete_effects::assign_target_commits(
                    &sim.substrate.entities,
                    target,
                );
                if let Some(survivor) = sim.substrate.entities.get_mut(id) {
                    crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                        survivor, target, commits,
                    );
                }
            }
            true
        })
    }

    /// The absorbed passengers' exit, shared by SpawnSurvivors' Phase A
    /// (`0x00442DF2..0x00443011`) and Sell's stage 1
    /// (`0x0044A389..0x0044A59E`): an `InfantryAbsorb=`/`UnitAbsorb=`
    /// building's passengers leave in cargo order, each taking the next cell
    /// of its occupy list `cells`. The cursor is unbounded: the passenger
    /// after the last cell reads the list's `0x7FFF` terminator, whose cell
    /// the Map lookup resolves to MapClass's off-map cell
    /// (`tools/spatial_oracle/building_sale.json` `c_absorbed_five`, a full
    /// Bio Reactor). Returns the cursor.
    pub(crate) fn eject_absorbed_passengers(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        building_id: u64,
        cells: &[(u16, u16)],
        no_survivor: bool,
        effects: FrameEffects<'_>,
    ) -> usize {
        let absorbs = self
            .substrate
            .entities
            .get(building_id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.infantry_absorb || object.unit_absorb);
        let mut cursor = 0;
        if !absorbs {
            return cursor;
        }
        while let Some((passenger, _)) = self
            .substrate
            .entities
            .get_mut(building_id)
            .and_then(|building| building.passenger_role.cargo_mut())
            .and_then(|cargo| cargo.unload_first())
        {
            let cell = cells.get(cursor).copied();
            cursor += 1;
            self.eject_absorbed_passenger(
                rules,
                registry,
                building_id,
                passenger,
                cell,
                no_survivor,
                effects,
            );
        }
        cursor
    }

    /// One absorbed passenger. An infantryman spends the PlaceInfantryInCell
    /// draw for its occupy-list cell (`None`: past the list, the off-map
    /// cell; the result is overwritten), then every passenger Unlimboes at
    /// the building Location in priority mode (no second draw), Scatters, and
    /// Hunts for a computer building owner. NoSurvivor or a refused Unlimbo
    /// UnInits it instead.
    fn eject_absorbed_passenger(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        building_id: u64,
        passenger: u64,
        cell: Option<(u16, u16)>,
        no_survivor: bool,
        effects: FrameEffects<'_>,
    ) {
        let Some(infantry) = self
            .substrate
            .entities
            .get(passenger)
            .map(|entity| entity.category == EntityCategory::Infantry)
        else {
            return;
        };
        //SpawnSurvivors442EDE..442FFC/Selling44A47A..44A589: the bracket includes
        //the caller's placement, Unlimbo, Scatter/Walk and failed deletion.
        self.with_object_placement_scope(|sim| {
            if infantry {
                let cell = cell.map_or(RawCellKey::Dummy, |(x, y)| RawCellKey::Real(x, y));
                let _overwritten = bump_crush::place_infantry_in_native_cell(
                    &sim.substrate.raw_cell_occupation,
                    cell,
                    MovementLayer::Ground,
                    crate::sim::components::DriveCoord {
                        x: SURVIVOR_REQUEST_X,
                        y: SURVIVOR_REQUEST_Y,
                        z: 0,
                    },
                    false,
                    false,
                    &mut sim.scenario_rng,
                );
            }
            let Some(building) = sim.substrate.entities.get(building_id) else {
                return;
            };
            let owner = building.owner();
            let on_bridge = building.on_bridge;
            // `0x00442F3C..0x00442F63`: the building's `+0x388` Current() as a
            // rounded DirType is the Unlimbo direction.
            let frame = sim.session.binary_frame;
            let facing = building.body_facing_dir(frame);
            let requested = building.position;
            let positioned = if infantry && !no_survivor {
                sim.infantry_unlimbo_position(requested, Some(rules))
            } else {
                Some(requested)
            };
            let sub_cell = infantry.then(|| {
                let position = positioned.unwrap_or(requested);
                bump_crush::priority_sub_cell(position.sub_x, position.sub_y)
            });
            // The dying building's expiry broadcast already cleared the
            // passenger's transporter link; the cargo list still held it.
            if let Some(entity) = sim.substrate.entities.get_mut(passenger) {
                entity.passenger_role.clear_carrier_relation();
                entity.on_bridge = on_bridge;
                entity.sub_cell = sub_cell;
            }
            let revealed = !no_survivor
                && positioned.is_some_and(|position| {
                    matches!(
                        sim.try_reveal_entity_with_context(
                            passenger,
                            RevealRequest {
                                position,
                                placement: PlacementEvidence::MarkSucceeded,
                                logic_eligible: true,
                            },
                            UninitContext::new(Some(rules), registry)
                                .with_effects(effects)
                                .with_unlimbo_facing(Some(facing)),
                        ),
                        RevealOutcome::Revealed { .. }
                    )
                });
            if !revealed {
                sim.uninit_with_context(
                    passenger,
                    UninitContext::new(Some(rules), registry).with_effects(effects),
                );
                return;
            }
            if infantry {
                sim.scatter_crew(rules, registry, passenger, effects);
            }
            if !sim.owner_is_human(owner) {
                sim.queue_crew_mission(passenger, MissionType::Hunt);
            }
        });
    }

    /// The crew block of `UnitClass::ReceiveDamage` (`0x007381BC..0x0073838A`),
    /// after the dying unit's Mark(UP). `prevent_escape` is ReceiveDamage's
    /// arg6. A crewed vehicle with no passenger capacity rolls CrewEscape; an
    /// escaped crewman leaves from the vehicle's own coordinate with Health
    /// `RandomRanged(5, Strength/2)`, Guards for a human owner or Hunts for
    /// the computer, and is selected if the vehicle was
    /// ([`DyingTransport::selected_by_player`], `0x00738352..0x0073835E`).
    pub(crate) fn spawn_vehicle_crew(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        unit_id: u64,
        prevent_escape: bool,
        selected_by_player: bool,
        effects: FrameEffects<'_>,
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
        let position = entity.position;
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
                raw_z: position.exact_z_leptons,
                sub_x: position.sub_x,
                sub_y: position.sub_y,
                on_bridge,
            }
        } else {
            CrewUnlimbo::Place {
                cell: (position.rx, position.ry),
                z: position.z,
                raw_z: position.exact_z_leptons,
                request: (position.sub_x, position.sub_y),
                caller_places_first: false,
            }
        };

        let roll = self.scenario_rng.next_range_u32_inclusive(0, 0x7FFF_FFFE);
        if !crew_escapes(roll, rules.general.crew_escape) {
            return;
        }
        let Some(crew) = self.techno_crew_type(rules, side, armed) else {
            return;
        };
        let Some(id) = self.construct_crew(rules, &crew, owner, unlimbo, registry, effects) else {
            return;
        };
        // Signed Strength/2 (CDQ; SUB; SAR).
        let strength = rules.object(&crew).map_or(0, |object| object.strength);
        let health = self.scenario_rng.next_range_i32_inclusive(5, strength / 2);
        self.set_crew_health(id, health);
        self.scatter_crew(rules, registry, id, effects);
        let mission = if self.owner_is_human(owner) {
            MissionType::Guard
        } else {
            MissionType::Hunt
        };
        self.queue_crew_mission(id, mission);
        if selected_by_player && let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.selected = true;
        }
    }

    /// The passenger block of `UnitClass::ReceiveDamage`
    /// (`0x00737F80..0x007381B6`), after the dying unit's Mark(UP) and
    /// before its crew roll. Above 0xD0 leptons (`0x00737F97..0x00737FAB`)
    /// KillPassengers kills them all, crediting the attacker; a `Crashable=`
    /// type (`+0xD95`) stops there (`0x00737FB0`), so lower down its Crash
    /// kills them uncredited. Every other passenger, in cargo order, steps
    /// out onto the unit's cell or dies. An `OpenTopped=` unit first clears
    /// each passenger's InOpenTransport (`0x007104C0`, `+0x82`), which VERA
    /// derives from the transporter link each escape clears.
    pub(crate) fn release_dying_unit_passengers(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        unit_id: u64,
        dying: DyingTransport,
        effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let crashable = self
            .object_type(unit.type_ref(), rules)
            .is_some_and(|object| object.crashable);
        if crate::sim::movement::air_movement::current_fly_height(
            unit,
            self.resolved_terrain.as_ref(),
        ) > 0xD0
        {
            self.kill_passengers(unit_id, dying.attacker, rules, registry, effects);
        }
        if crashable {
            return;
        }
        // `FootClass::RemoveFirstPassenger @ 0x004DE710` (`0x00737FD4`) until
        // the cargo is empty.
        while crate::sim::passenger::depart_cargo_head(
            self,
            rules,
            registry,
            unit_id,
            crate::sim::passenger::DepartureRoute::DeathEscape,
            |sim, passenger| {
                sim.escape_dying_unit(rules, registry, unit_id, passenger, dying, effects);
                Ok(())
            },
            effects,
        )
        .is_ok()
        {}
    }

    /// One passenger of the loop (`0x00737FE3..0x007381A1`), already popped.
    /// It must be able to enter the unit's cell (`Can_Enter_Cell` 0 or 2), and
    /// IgnoreDefenses must be clear; otherwise, or if its Unlimbo fails, it
    /// records its kill and is UnInit. It Unlimboes on the cell's floor at the
    /// unit's XY (on a bridge, at the unit's own coordinate) facing the unit's
    /// body facing, inside the `[0x00A8E7AC]` bracket (`0x00738030`,
    /// `0x007381A1`) that makes an infantryman's placement the priority arm.
    /// It then drops a target an `OpenTopped=` unit of another house gave it,
    /// Scatters, Hunts for a computer house, and is selected if the unit was.
    fn escape_dying_unit(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        unit_id: u64,
        passenger: u64,
        dying: DyingTransport,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::movement::ground_pose::{ground_surface_z_at, position_world_coord};
        // The transporter link (`+0x11C`) is cleared after a successful
        // Unlimbo (`0x007380FA`); VERA's link also keeps the passenger off its
        // cell's occupancy, so it goes first. Nothing between reads it.
        let Some(entity) = self.substrate.entities.get_mut(passenger) else {
            return;
        };
        if entity.passenger_role.inside_transport_id() == Some(unit_id) {
            entity.passenger_role.clear_carrier_relation();
        }
        let infantry = entity.category == EntityCategory::Infantry;
        let passenger_owner = entity.owner();
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let frame = self.session.binary_frame;
        let unit_owner = unit.owner();
        let on_bridge = unit.on_bridge;
        let (rx, ry, unit_z) = (unit.position.rx, unit.position.ry, unit.position.z);
        let (sub_x, sub_y) = (unit.position.sub_x, unit.position.sub_y);
        let location = position_world_coord(&unit.position);
        // `0x007380C5..0x007380E6`: FacingClass::Current (`0x004C93D0`) as a
        // rounded DirType.
        let facing = unit.body_facing_dir(frame);
        let open_topped = self
            .object_type(unit.type_ref(), rules)
            .is_some_and(|object| object.open_topped);

        // `0x00737FE3..0x0073802E`: the passenger's Can_Enter_Cell (vtable
        // `+0x1AC`) for the cell under the unit's Location, direction and
        // level -1, no previous cell.
        let code = match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cell = terrain.native_cell_identity((rx as i16, ry as i16));
                self.foot_can_enter(
                    passenger,
                    cell,
                    crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                    rules,
                    registry,
                )
            }
            None => Err("no map cells".into()),
        };
        let admitted = match code {
            Ok(code) => matches!(code, 0 | 2),
            Err(cause) => {
                log::debug!("passenger {passenger} of dying unit {unit_id} cannot enter: {cause}");
                false
            }
        };
        // `0x0073803B`: the unit's OnBridge.
        if let Some(entity) = self.substrate.entities.get_mut(passenger) {
            entity.on_bridge = on_bridge;
        }
        // `0x007380A3..0x007380BF`. The unit's IsABomb (`+0x8F`) kills too;
        // VERA has no such byte (module residuals).
        if dying.ignore_defenses || !admitted {
            self.record_kill_and_uninit(passenger, dying.attacker, rules, registry, effects);
            return;
        }

        //738030..7381A8: preserve the counter through every successful or
        //failed Unlimbo exit, including Scatter's immediate Walk Process.
        self.with_object_placement_scope(|sim| {
            // `0x00738047..0x0073809F`: the unit's XY at the Z of its cell's
            // CellClass::GetCoords (`0x00486840`, the floor at the cell centre),
            // or on a bridge the unit's own coordinate.
            let terrain = sim.resolved_terrain.as_ref();
            let path_grid = sim.path_grid();
            let level = terrain
                .and_then(|terrain| terrain.cell(rx, ry))
                .map_or(0, |cell| cell.level);
            let coord_z = if on_bridge {
                location.z
            } else {
                let centre = [i32::from(rx) * 256 + 128, i32::from(ry) * 256 + 128];
                ground_surface_z_at(centre, false, terrain, path_grid).unwrap_or(location.z)
            };
            let floor = ground_surface_z_at([location.x, location.y], false, terrain, path_grid);
            let unlimbo = if infantry && floor == Some(coord_z) {
                CrewUnlimbo::Place {
                    cell: (rx, ry),
                    z: level,
                    raw_z: Some(coord_z),
                    request: (sub_x, sub_y),
                    caller_places_first: false,
                }
            } else {
                CrewUnlimbo::Exact {
                    rx,
                    ry,
                    z: if on_bridge { unit_z } else { level },
                    raw_z: Some(coord_z),
                    sub_x,
                    sub_y,
                    on_bridge,
                }
            };
            if let Some(locomotor) = sim
                .substrate
                .entities
                .get_mut(passenger)
                .and_then(|entity| entity.locomotor.as_mut())
            {
                locomotor.layer = MovementLayer::Ground;
            }
            // `0x007380EC..0x007380F4`: a refused Unlimbo kills.
            if !sim.unlimbo_crew(rules, passenger, unlimbo, Some(facing), registry, effects) {
                sim.record_kill_and_uninit(passenger, dying.attacker, rules, registry, effects);
                return;
            }

            // `0x00738104..0x0073812A`: Assign_Target(NULL).
            if open_topped
                && passenger_owner != unit_owner
                && let Some(entity) = sim.substrate.entities.get_mut(passenger)
            {
                crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            }
            // `0x00738130..0x0073813D`: Scatter(&EmptyCoord, 1, 0).
            if infantry {
                sim.scatter_crew(rules, registry, passenger, effects);
            }
            // `0x00738143..0x0073816E`: a computer passenger joins the unit's Team
            // (`TeamClass::Add_Member @ 0x006EA500`), or Hunts without one.
            if !sim.owner_is_human(passenger_owner) {
                match sim.team_script_vm.team_for_member(unit_id) {
                    Some((team_id, _)) => {
                        sim.team_add_member(team_id, passenger, false, rules, registry, effects);
                    }
                    None => sim.queue_crew_mission(passenger, MissionType::Hunt),
                }
            }
            // `0x00738174..0x00738180`: Select (vtable `+0x14C`).
            if dying.selected_by_player
                && let Some(entity) = sim.substrate.entities.get_mut(passenger)
            {
                entity.selected = true;
            }
        });
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
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Option<u64> {
        let (rx, ry, z) = unlimbo.cell_level();
        let id = self.construct_crew_limbo(rules, crew, owner, (rx, ry), z)?;
        if !self.unlimbo_crew(rules, id, unlimbo, None, registry, effects) {
            self.discard_constructed_limbo(id, Some(rules), effects);
            return None;
        }
        Some(id)
    }

    /// `new InfantryClass(type, Owner)`: the TechnoClass constructor's
    /// Scenario draw. The crewman waits in limbo at `cell` until its Unlimbo.
    fn construct_crew_limbo(
        &mut self,
        rules: &RuleSet,
        crew: &str,
        owner: InternedId,
        cell: (u16, u16),
        z: u8,
    ) -> Option<u64> {
        let owner_name = self.interner.resolve(owner).to_string();
        self.construct_object_limbo_at_height(crew, &owner_name, cell.0, cell.1, 0, z, rules)
    }

    /// Unlimbo a Foot in limbo as `unlimbo` says; an infantryman takes the
    /// spot it lands on. `facing` is Unlimbo's direction, which
    /// `TechnoClass::Unlimbo` commits to the body facing (`0x006F6DAA`); a new
    /// crewman keeps its constructed facing. False leaves the object in limbo:
    /// the ordinary PlaceInfantryInCell arm found no spot, or the reveal was
    /// refused.
    fn unlimbo_crew(
        &mut self,
        rules: &RuleSet,
        id: u64,
        unlimbo: CrewUnlimbo,
        facing: Option<u8>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let infantry = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category == EntityCategory::Infantry);
        let (rx, ry, z) = unlimbo.cell_level();
        let requested_z = match unlimbo {
            CrewUnlimbo::Place { raw_z, .. } | CrewUnlimbo::Exact { raw_z, .. } => raw_z,
        };
        let (sub_cell, sub_x, sub_y, on_bridge) = match unlimbo {
            CrewUnlimbo::Place {
                cell,
                mut request,
                caller_places_first,
                ..
            } => {
                debug_assert!(infantry, "only an infantryman places in a cell");
                if caller_places_first {
                    let Some(spot) = bump_crush::place_infantry_in_cell(
                        &self.substrate.raw_cell_occupation,
                        cell.0,
                        cell.1,
                        MovementLayer::Ground,
                        request.0,
                        request.1,
                        &mut self.scenario_rng,
                    ) else {
                        return false;
                    };
                    request = crate::util::lepton::subcell_lepton_offset(Some(spot));
                }
                // The distinct caller placement above remains here; the
                // class51DFF0 floor/priority/placement owner is shared with
                // ordinary constructors, factory output and slaves.
                let Some(position) = self.infantry_unlimbo_position(
                    RevealPosition {
                        exact_z_leptons: requested_z,
                        rx: cell.0,
                        ry: cell.1,
                        z,
                        sub_x: request.0,
                        sub_y: request.1,
                    },
                    Some(rules),
                ) else {
                    return false;
                };
                (
                    Some(bump_crush::priority_sub_cell(
                        position.sub_x,
                        position.sub_y,
                    )),
                    position.sub_x,
                    position.sub_y,
                    false,
                )
            }
            CrewUnlimbo::Exact {
                sub_x,
                sub_y,
                on_bridge,
                ..
            } => (
                infantry.then(|| bump_crush::priority_sub_cell(sub_x, sub_y)),
                sub_x,
                sub_y,
                on_bridge,
            ),
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.sub_cell = sub_cell;
            entity.on_bridge = on_bridge;
        }
        let facing = facing.or_else(|| {
            self.substrate
                .entities
                .get(id)
                .map(|entity| entity.body_facing_dir(self.session.binary_frame))
        });
        let outcome = self.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition {
                    exact_z_leptons: requested_z,
                    rx,
                    ry,
                    z,
                    sub_x,
                    sub_y,
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::new(Some(rules), registry)
                .with_effects(effects)
                .with_unlimbo_facing(facing),
        );
        matches!(outcome, RevealOutcome::Revealed { .. })
    }

    /// A survivor's placement in a foundation cell: its request
    /// (`0x80`, `0xA4`) on the cell's floor.
    fn survivor_unlimbo(&self, cell: (u16, u16)) -> CrewUnlimbo {
        let z = self.terrain_cell_level(cell.0, cell.1).unwrap_or(0);
        CrewUnlimbo::Place {
            cell,
            z,
            raw_z: None,
            request: (
                SimFixed::from_num(SURVIVOR_REQUEST_X),
                SimFixed::from_num(SURVIVOR_REQUEST_Y),
            ),
            caller_places_first: true,
        }
    }

    /// Sell's stage-1 crew (`0x0044A5CF..0x0044A7A3`): `count` survivors
    /// (How_Many_Survivors, counted before the passengers left). Each takes
    /// the building's crew type (`vt+0x30C`: the Engineer roll, then
    /// GetCrew), picked again while it names an Engineer once one has been
    /// picked (`0x0044A5F0..0x0044A623`); is constructed (the TechnoClass
    /// constructor's draw); picks a cell of the occupy list `cells` with
    /// `RandomRanged(0, n - 1)` (`0x0044A67A`) and places at its request
    /// there (PlaceInfantryInCell's draw); Unlimboes in priority mode
    /// (ScenarioInit raised, no second draw); Scatters; and queues Move for
    /// any owner. A sale's crew keeps full strength. Returns whether one
    /// entered the map.
    pub(crate) fn spawn_sale_crew(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        building_id: u64,
        count: i32,
        cells: &[(u16, u16)],
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(owner) = self
            .substrate
            .entities
            .get(building_id)
            .map(|entity| entity.owner())
        else {
            return false;
        };
        // A crewed type always has cells (a 0x0 foundation would pick from
        // an empty list); no retail type is both.
        let Some(last) = i32::try_from(cells.len())
            .ok()
            .and_then(|count| count.checked_sub(1))
            .filter(|&last| last >= 0)
        else {
            return false;
        };
        let is_engineer = |crew: &str| rules.object(crew).is_some_and(|object| object.engineer);
        let mut engineer_picked = false;
        let mut spawned = false;
        for _ in 0..count {
            let Some(mut crew) = self.building_crew_type(rules, building_id) else {
                continue;
            };
            while engineer_picked && is_engineer(&crew) {
                match self.building_crew_type(rules, building_id) {
                    Some(next) => crew = next,
                    None => break,
                }
            }
            if is_engineer(&crew) {
                engineer_picked = true;
            }
            let Some(id) = self.construct_crew_limbo(rules, &crew, owner, cells[0], 0) else {
                continue;
            };
            //44A65E..44A74E: the cell pick, ordinary caller placement and
            //priority Unlimbo run raised. Sale crew Scatter44A768 is outside
            //that bracket; only QueueMove is rebracketed44A76E..44A794.
            let escaped = self.with_object_placement_scope(|sim| {
                let pick = sim.scenario_rng.next_range_i32_inclusive(0, last);
                let cell = cells[pick as usize];
                if !sim.unlimbo_crew(
                    rules,
                    id,
                    sim.survivor_unlimbo(cell),
                    None,
                    registry,
                    effects,
                ) {
                    sim.discard_constructed_limbo(id, Some(rules), effects);
                    return false;
                }
                true
            });
            if escaped {
                self.scatter_crew(rules, registry, id, effects);
                self.with_object_placement_scope(|sim| {
                    sim.queue_crew_mission(id, MissionType::Move)
                });
                spawned = true;
            }
        }
        spawned
    }

    fn set_crew_health(&mut self, id: u64, health: i32) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.health.current = health;
        }
    }

    /// `InfantryClass::Scatter(&EmptyCoord, 1, 0)`. A receiver error leaves
    /// the crewman where it landed. The immediate Process's bridge-state
    /// flag, which the hut caller propagates, is dropped: a crewman's first
    /// walk step does not change bridge state.
    fn scatter_crew(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        id: u64,
        effects: FrameEffects<'_>,
    ) {
        if let Err(cause) = self.scatter_null(
            id,
            crate::sim::movement::ScatterFlags::new(true, false),
            rules,
            registry,
            effects,
        ) {
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
}

#[cfg(test)]
#[path = "crew_survival_tests.rs"]
mod tests;
