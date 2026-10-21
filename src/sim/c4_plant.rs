//! A demolition infantryman's C4 plant on a building.
//!
//! The player's Sabotage order (the Event's MegaMission, `Command::PlantC4`)
//! and the demolition arms of Attack, Hunt, Guard and AreaGuard put a
//! `C4=` infantryman on Sabotage (mission 17). Guard and AreaGuard promote
//! a computer infantryman's building Target (`FootClass` `0x004D523A..0x004D52A9`
//! and `0x004D6DBE..0x004D6E44`, `techno_ai::mission_handlers`), so a Tanya
//! holding that Target walks in to plant. Sabotage dispatches to
//! `FootClass::Mission_Capture` (`sim::mission::capture`), which heads for a
//! building Target again once it has no destination. The walk enters the
//! building's cells (`InfantryClass::Can_Enter_Cell`, `world::object_entry`),
//! and on arrival `InfantryClass::Per_Cell_Process` plants the charge
//! ([`Simulation::infantry_per_cell_c4`]). `BuildingClass::Update` sets it
//! off when its C4Timer runs out (`Simulation::tick_pending_building_detonation`,
//! which also collapses a bridge repair hut's bridge).
//!
//! The arm draws once from the Scenario stream for the planter's `GetROF(1)`
//! (`RandomRanged(3, 5)` for a retail Sapper, none for an empty slot). Its
//! Scatter draws (`RandomRanged(0, 4)`) only when Scatter admits it. A Walk
//! arrival runs the arm before the Walk lets its destination go
//! (`0x0075BE3C`, then `0x0075BEF0..0x0075BF38`), so the Walk is still
//! moving: the Scatter loses `forced` (`0x0051D172`) and refuses a planter
//! that is not `Fraidycat=`. A second planter's null destination stops the
//! Walk first, so its Scatter goes through. The arm writes the building's
//! C4Timer and the planter's rearm timer, and detaches nothing.
//!
//! After a plant the planter has no NavCom, and other owners carry it on.
//! Its Walk still heads for the building's centre; once the next cell is
//! the building's, `Can_Enter_Cell` no longer admits it, and the Walk's
//! blocked arm (`Simulation::mission_override_movement_blocker`) puts the
//! planter on Attack against the building, whose C4 arm sends it back in on
//! Sabotage to take the second-planter branch. A planter whose walk ends
//! where it planted idles in Mission_Capture, which sends a computer's back
//! in for its Target and scatters a player's out of the building
//! (`Scatter(NULL, 1, 0)`, `0x004D4BC7..0x004D4C0E`). Each Sabotage dispatch
//! draws its `RandomRanged(0, 2)` delay, and each Scatter that goes through
//! its own.
//!
//! The arm reads the type's `C4=` (`+0xEC2`) only, where Mission_Attack's
//! arm also takes weapon ability 14: a planter that holds C4 only by rank
//! walks in and plants nothing, natively as here. No retail type lists the
//! ability.
//!
//! Every arrival at the NavCom building first tells the building's Tag it
//! was entered (event 1, `0x0051A507..0x0051A51C`), before the refusal
//! tests, as the Engineer and spy arms do.
//!
//! The cell arm that follows (`0x0051A667..0x0051A7F2`,
//! [`Simulation::infantry_per_cell_c4_cell`]) takes the same Sabotage `C4=`
//! arrival when the entry cell has no building or one that is not the
//! NavCom, and fires when the NavCom is the cell the infantryman stands in.
//! The player's producer is dead: Foot ClickedAction_Cell (`0x004D7D50`)
//! assigns Sabotage with the cell as destination only for action 16
//! (`0x004D8182`), and every caller of the cell Active_Click_With slot
//! (vt+0x140: `0x004AE8C4`, `0x004AE927`, `0x004AE97C`, and
//! `FootClass::ActivatePlanningWaypoint` `0x004DC8C0` at `0x004DCDCF`,
//! `0x004DCDFF`) passes the object's own What_Action(cell), whose chain
//! (Infantry `0x0051F800`, Foot `0x004DDDE0`, Techno `0x00700600`) never
//! answers 16; Infantry ClickedAction_Cell (`0x0051F250`) passes it on
//! unchanged. The computer reaches it instead. Guard's and AreaGuard's
//! demolition arms queue Sabotage without touching the NavCom
//! (`0x004D529F`, `0x004D6E2E`), and a computer `C4=` infantryman takes
//! buildings as targets (threat mask `0x20`, `0x0051E34D..0x0051E379`), so
//! one already walking to a cell keeps walking: AreaGuard's leash walk back
//! to its post (`0x004D6ECB`, the post being `Map[Location]` archived at
//! `0x004D6DB9`) and its Approach (`0x004D6F32`, a ring cell) leave a cell
//! NavCom, and the next scan or retaliation can hand it a building.
//! Queue_Mission (`0x005B35E0`) and Commence (`0x005B3570`) leave the
//! NavCom alone,
//! `TechnoClass::AI`'s illegal-target drop spares Sabotage
//! (`0x006FA472..0x006FA4CB`), and Mission_Capture keeps an existing NavCom
//! (`0x004D4B57..0x004D4B5F`, `0x004D4BC7..0x004D4BCF`). The Walk runs
//! Per_Cell_Process(2) on reaching that cell before it lets the destination
//! go (`0x0075BE3C`, then `0x0075BEF0..0x0075BF38`), so the arm fires there.
//! A moving infantryman is not ready to commence (`0x00521B60`) unless on
//! Guard, Hunt or a targetless Attack, so AreaGuard's queued Sabotage
//! commits only in Per_Cell_Process's tail Commence (`0x0051A8C5`), after
//! the C4 arms: queued during the last step, it commits in the NavCom
//! cell's own visit, the Walk then drops the NavCom (`0x0075BF29`) and the
//! building route takes over.

use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation, UninitContext};

/// Shared per-building C4 / PostMortem detonation timer.
///
/// Native BuildingClass uses one latch and timer triple for both an infantry
/// C4 plant and a qualifying `CausesDelayKill` fatal hit. Once elapsed, the
/// building's own Update fires a forced C4Warhead receiver packet with damage
/// equal to its current HP.
///
/// IronCurtain/ForceShield entry cancels this state. Normal targets keep an
/// expired latch if the forced receiver unexpectedly leaves them alive;
/// BridgeRepairHut owns the separate consume-and-clear branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingC4Detonation {
    /// Native Building timer (`+0x528` start, `+0x530` duration), signed and
    /// unclamped; its remaining time drives the shorten test, the Building
    /// Update expiry and the checksum.
    timer: CdTimer,
    /// Retained source-object identity (`+0x540`). Fresh PostMortem arms leave
    /// this null; shortening an infantry C4 timer preserves its source.
    source_entity_id: Option<u64>,
}

impl PendingC4Detonation {
    /// A latch with `timer` and C4AppliedBy `source_entity_id`.
    pub(crate) fn new(timer: CdTimer, source_entity_id: Option<u64>) -> Self {
        Self {
            timer,
            source_entity_id,
        }
    }

    /// The fuse (`+0x528` start, `+0x530` duration).
    pub(crate) fn timer(&self) -> CdTimer {
        self.timer
    }

    /// C4AppliedBy (`+0x540`).
    pub(crate) fn source_entity_id(&self) -> Option<u64> {
        self.source_entity_id
    }
}

/// The building's C4/PostMortem latch: `None` until a C4 plant
/// ([`Self::plant`]) or a qualifying `CausesDelayKill` fatal hit
/// ([`Self::arm_post_mortem`]) arms it. IronCurtain/ForceShield entry and the
/// BridgeRepairHut's consume-and-clear branch drop it ([`Self::cancel`]); a
/// pointer expiry or a load that cannot resolve its source drops only the
/// source ([`Self::forget_source`]).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub(crate) struct C4Detonation {
    pending: Option<PendingC4Detonation>,
}

impl C4Detonation {
    /// The armed latch.
    pub(crate) fn pending(self) -> Option<PendingC4Detonation> {
        self.pending
    }

    /// The C4 arm's write (`0x0051A5A7..0x0051A608`): the fuse starts at
    /// `frame` with the planter as C4AppliedBy.
    pub(crate) fn plant(&mut self, frame: i32, fuse: i32, planter: u64) {
        self.pending = Some(PendingC4Detonation::new(
            CdTimer::started(frame, fuse),
            Some(planter),
        ));
    }

    /// PostMortem's arm in `TechnoClass::ReceiveDamage`
    /// (`0x00701F0E..0x00701F5D`): a fresh latch, or a shorter fuse. It never
    /// writes C4AppliedBy, so a shortened plant keeps its planter.
    pub(crate) fn arm_post_mortem(&mut self, current_frame: i32, duration_frames: i32) {
        let replace = self
            .pending
            .is_none_or(|pending| duration_frames < pending.timer.remaining(current_frame));
        if replace {
            let retained_source = self.pending.and_then(|pending| pending.source_entity_id);
            self.pending = Some(PendingC4Detonation::new(
                CdTimer::started(current_frame, duration_frames),
                retained_source,
            ));
        }
    }

    /// DestructionEffects' write (`0x00441C43..0x00441C8C`): the fuse
    /// restarts at `frame` for `duration_frames`, 0 for an `Explodes=` type
    /// or a building dying while Selling, else 8. C4AppliedBy (`+0x540`) is
    /// kept for the survivors. The latch lives in the fuse here, so a building
    /// with no plant gains one; the Health 0 it dies at keeps the C4 expiry
    /// from firing it (`Simulation::tick_pending_building_detonation`).
    /// RESIDUAL: native writes only the fuse and leaves the latch (`+0x6DF`)
    /// clear. Trigger: a map trigger's sell order on an `Explodes=` or
    /// selling building in the frame it waits at Health 0. Effect: VERA's
    /// computer Sell_Back refuses it as C4-latched where native queues the
    /// sale; the building leaves at that Update either way. Frequency: a
    /// scripted sell landing in that one frame.
    pub(crate) fn arm_destruction(&mut self, frame: i32, duration_frames: i32) {
        let retained_source = self.pending.and_then(|pending| pending.source_entity_id);
        self.pending = Some(PendingC4Detonation::new(
            CdTimer::started(frame, duration_frames),
            retained_source,
        ));
    }

    /// ReceiveDamage case 4's gate (`0x0044266B..0x00442690`): it UnInits a
    /// building, and runs AfterDestruction, only while the fuse has time left.
    pub(crate) fn has_time_left(self, frame: i32) -> bool {
        self.pending
            .is_some_and(|pending| pending.timer.remaining(frame) > 0)
    }

    /// `BuildingClass::IronCurtain @ 0x00457C90` (IronCurtain and ForceShield
    /// entry) and the BridgeRepairHut's expiry branch
    /// (`0x00440213..0x00440331`) drop the latch and C4AppliedBy.
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
    }

    /// `BuildingClass::PointerExpired @ 0x0044E8F0` nulls an expiring
    /// C4AppliedBy and keeps the fuse; a load whose source is gone does the
    /// same.
    pub(crate) fn forget_source(&mut self) {
        if let Some(pending) = self.pending.as_mut() {
            pending.source_entity_id = None;
        }
    }

    /// Install a fixture's latch.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, pending: Option<PendingC4Detonation>) {
        self.pending = pending;
    }
}

/// What `InfantryClass::Per_Cell_Process`'s C4 arms leave it to do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct C4Arrival {
    /// Per_Cell_Process returns here: after the building arm (`0x0051A65F`),
    /// or once the cell arm leaves the infantryman dead (`0x0051A7E0` to
    /// `0x0051AA01`). Otherwise it goes on at `0x0051A7F8`.
    pub(crate) returns: bool,
    /// The cell arm's area damage changed a bridge.
    pub(crate) bridge_state_changed: bool,
}

/// The building arm's answer: Per_Cell_Process returns.
const PLANTED: C4Arrival = C4Arrival {
    returns: true,
    bridge_state_changed: false,
};

impl Simulation {
    /// `InfantryClass::Per_Cell_Process`'s C4 arm (`0x0051A4BF..0x0051A664`),
    /// on a reason-2 arrival. An infantryman on Sabotage whose type is `C4=`,
    /// standing on a cell whose first ground building (`0x0047C520`) is its
    /// NavCom:
    /// - When that building is being sold or is Iron Curtained (vt+0x184,
    ///   vt+0x160), nothing is planted.
    /// - When it already holds a charge (`+0x6DF`, which a PostMortem also
    ///   sets), the planter drops its destination (vt+0x480(NULL, 1)), rearms
    ///   and scatters (`0x0051A550..0x0051A5A4`).
    /// - Otherwise the building flashes for [`RuleSet::c4_flash_frames`]
    ///   (vt+0x148), and its C4Timer starts for [`RuleSet::c4_delay_frames`]
    ///   with the planter as C4AppliedBy (`+0x540`; `0x0051A5A7..0x0051A608`).
    ///
    /// Unless it scattered for a charge already there, the planter then stops
    /// (`0x004DF0D0`), uncloaks (vt+0x45C(0)), rearms and scatters
    /// (`0x0051A60B..0x0051A657`). Its rearm is [`Self::rearm_c4_planter`].
    /// Its Scatter, from the building's coordinate
    /// ([`Self::scatter_from_building`]), does not Process, and on a Walk
    /// arrival refuses a planter that is not `Fraidycat=` (module doc). This
    /// arm returns before the Foot body.
    ///
    /// With no building there, or one that is not the NavCom, the cell arm
    /// runs instead ([`Self::infantry_per_cell_c4_cell`]).
    pub(crate) fn infantry_per_cell_c4(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<C4Arrival, FrameAdvanceError> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(C4Arrival::default());
        };
        if infantry.mission.effective().known() != Some(MissionType::Sabotage)
            || !self
                .object_type(infantry.type_ref(), rules)
                .is_some_and(|object| object.c4)
        {
            return Ok(C4Arrival::default());
        }
        let heading_for = infantry
            .navigation
            .nav_com()
            .and_then(NavTargetRef::object_id);
        let Some(building) = self
            .substrate
            .occupancy
            .first_building_on_layer(
                infantry.position.rx(),
                infantry.position.ry(),
                MovementLayer::Ground,
            )
            .filter(|&building| heading_for == Some(building))
        else {
            return self.infantry_per_cell_c4_cell(id, rules, registry, effects);
        };
        // 0x0051A507..0x0051A51C: the building's Tag hears event 1 with the
        // planter as actor on every arrival, before the refusals below.
        // Synchronous actions may change either object; both are reloaded.
        let context = UninitContext::new(rules, registry).with_effects(effects);
        self.dispatch_object_tag_event(building, 1, Some(id), None, context);
        let frame = self.session.binary_frame();
        let Some(target) = self.substrate.entities.get(building) else {
            return Ok(C4Arrival::default());
        };
        let refused = target.mission.effective().known() == Some(MissionType::Selling)
            || crate::sim::superweapon::invulnerability::is_invulnerable(
                target.invulnerability.as_ref(),
                frame,
            );
        if !refused {
            if target.pending_c4_detonation.pending().is_some() {
                self.assign_null_destination(id, rules, registry, effects);
                Self::rearm_c4_planter(
                    &self.interner,
                    &self.session,
                    &mut self.scenario_rng,
                    &mut self.substrate,
                    &self.houses,
                    id,
                    building,
                    rules,
                );
                self.scatter_from_building(id, building, rules, registry, effects)?;
                return Ok(PLANTED);
            }
            let (flash, fuse) = (rules.c4_flash_frames(), rules.c4_delay_frames());
            self.substrate.flash_building(building, flash, frame);
            if let Some(target) = self.substrate.entities.get_mut(building) {
                target.pending_c4_detonation.plant(frame as i32, fuse, id);
            }
        }
        if let Some(infantry) = self.substrate.entities.get_mut(id) {
            infantry.navigation.stop_moving();
        }
        crate::sim::combat::world_fire::start_uncloaking_to_fire(self, rules, id);
        Self::rearm_c4_planter(
            &self.interner,
            &self.session,
            &mut self.scenario_rng,
            &mut self.substrate,
            &self.houses,
            id,
            building,
            rules,
        );
        self.scatter_from_building(id, building, rules, registry, effects)?;
        Ok(PLANTED)
    }

    /// The C4 cell arm (`0x0051A667..0x0051A7F2`): when the cell at the
    /// infantryman's own coordinate (`Map[GetCoords]`, `0x00565730`) is its
    /// NavCom, in order:
    /// 1. area damage at its Location (`+0x9C`) of `[CombatDamage]
    ///    BridgeStrength=` (`Rules+0x1740`) with `C4Warhead=`
    ///    (`Rules+0xFA8`), itself as the source and no house (`0x00489280`,
    ///    [`Self::c4_cell_blast`]);
    /// 2. Stop_Driver (vt+0x500, [`Self::foot_stop_driver`]);
    /// 3. Scatter(coord, 1, 1) (vt+0x174, [`Self::scatter`]) away from its
    ///    Location stepped one cell along its body facing's octant
    ///    (`((Current >> 12) + 1) >> 1 & 7` of `+0x388`, the lepton table
    ///    `0x0089F6D8`), Z unchanged;
    /// 4. Queue_Mission(Move, 0) (vt+0x1E8);
    /// 5. Mark(DOWN) (vt+0x124(1), [`Self::foot_mark_put`]) when it then
    ///    has no NavCom or its NavCom's cell is Water (`+0xEC == 2`), so
    ///    the next two blasts find it in its cell's list;
    /// 6. the same area damage twice with no source;
    /// 7. Mark(DOWN) again if it is still alive (`+0x90`).
    ///
    /// Per_Cell_Process goes on at `0x0051A7F8` when it lives, or when its
    /// cell is not its NavCom, and returns once it is no longer IsAlive,
    /// which a killed infantryman stays until its UnInit. On a Walk arrival
    /// Stop_Driver has stopped the Walk, so the Scatter keeps `forced` and
    /// goes through: the NavCom becomes a neighbouring cell, step 5 is
    /// skipped on dry land, and the infantryman, lifted out of its cell's
    /// list by the Walk, survives all three blasts and leaves on Move,
    /// dropping its Sabotage. Retail `C4Warhead=Super` sets no `CellSpread=`,
    /// so the blasts reach only receivers at the point itself (the shared
    /// area owner's zero-spread rule), the cell's bridge among them.
    ///
    /// RNG: whatever the three area damage calls draw (their receivers, and
    /// `RandomRanged(1, BridgeStrength)` per bridge hit), and Scatter's own
    /// draws when it goes through. Timers: Stop_Driver's and the queued
    /// mission's. No detach of its own; the area damage's deaths detach
    /// through their receivers.
    pub(crate) fn infantry_per_cell_c4_cell(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<C4Arrival, FrameAdvanceError> {
        let frame_error = |sim: &Simulation, cause: String| {
            FrameAdvanceError::new(sim.session.tick(), sim.session.binary_frame(), id, cause)
        };
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(C4Arrival::default());
        };
        let location = self
            .foot_current_coordinate(id)
            .map_err(|cause| frame_error(self, cause))?;
        let own_cell = self.resolved_terrain.as_ref().map(|terrain| {
            let cells = NativeCellQuery::canonical(terrain);
            cells.coord(cells.lookup_world(location.x, location.y))
        });
        let at_nav_com = matches!(
            (infantry.navigation.nav_com(), own_cell),
            (Some(NavTargetRef::Cell { rx, ry }), Some(cell)) if (rx as i16, ry as i16) == cell
        );
        if !at_nav_com {
            return Ok(C4Arrival::default());
        }
        let mut bridge_state_changed =
            self.c4_cell_blast(Some(id), location, rules, registry, effects);
        self.foot_stop_driver(id, rules, registry, effects)
            .map_err(|cause| frame_error(self, cause))?;
        // Native re-reads Location (+0x9C) and the facing (+0x388) after the
        // first blast and Stop_Driver (`0x0051A6D0..0x0051A71D`), and again
        // before each sourceless blast (`0x0051A76C`, `0x0051A7A3`).
        let location = self
            .foot_current_coordinate(id)
            .map_err(|cause| frame_error(self, cause))?;
        let frame = self.session.binary_frame();
        let octant = crate::util::direction_tables::dir_from_facing16(
            self.substrate
                .entities
                .get(id)
                .map_or(0, |infantry| infantry.body_facing_current(frame)),
        );
        let (dx, dy) = crate::util::direction_tables::LEPTON_DELTAS[usize::from(octant)];
        let away_from = DriveCoord {
            x: location.x.wrapping_add(dx),
            y: location.y.wrapping_add(dy),
            z: location.z,
        };
        bridge_state_changed |= self
            .scatter(
                id,
                Some(away_from),
                ScatterFlags::new(true, true),
                rules,
                registry,
                effects,
            )
            .map_err(|cause| frame_error(self, cause))?;
        let now = self.session.binary_frame();
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Move),
            0,
            now,
            &LiveReadyInputProvider { rules },
        );
        let nav_com = self
            .substrate
            .entities
            .get(id)
            .and_then(|infantry| infantry.navigation.nav_com());
        let marks = match nav_com {
            None => true,
            Some(nav_com) => self.nav_com_cell_is_water(nav_com),
        };
        if marks {
            self.foot_mark_put(id, rules, registry, effects);
        }
        for _ in 0..2 {
            let location = self
                .foot_current_coordinate(id)
                .map_err(|cause| frame_error(self, cause))?;
            bridge_state_changed |= self.c4_cell_blast(None, location, rules, registry, effects);
        }
        let alive = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|infantry| infantry.lifecycle.object_alive());
        if alive {
            self.foot_mark_put(id, rules, registry, effects);
        }
        Ok(C4Arrival {
            returns: !alive,
            bridge_state_changed,
        })
    }

    /// The cell arm's `DamageArea(&Location, BridgeStrength, source,
    /// C4Warhead, 1, NULL)` (`0x00489280`, the shared area damage owner).
    /// A missing `C4Warhead=` makes it return at once (`0x004892D7`).
    /// Answers whether a bridge changed.
    fn c4_cell_blast(
        &mut self,
        source: Option<u64>,
        at: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(warhead) = rules.warhead(&rules.bridge_warheads.c4_name) else {
            return false;
        };
        let warhead_ref = self.interner.intern(&warhead.id);
        crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            registry,
            ProjectileCoord::new(at.x, at.y, at.z),
            rules.bridge_rules.strength,
            warhead,
            (
                source.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                None,
                warhead_ref,
            ),
            effects,
        )
    }

    /// `0x0051A738..0x0051A75E`: the land of the NavCom's cell
    /// (`Map[NavCom->GetCoords()]`, `+0xEC`) is Water.
    fn nav_com_cell_is_water(&self, nav_com: NavTargetRef) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let target = match nav_com {
            NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
            NavTargetRef::Object { id } => TargetKind::Entity(id),
        };
        crate::sim::movement::ground_pose::target_get_coords(target, self, Some(&cells))
            .is_some_and(|coord| {
                cells.land_type(cells.lookup_world(coord.x, coord.y))
                    == i32::from(LandType::Water.as_index())
            })
    }

    /// The planter's `RearmTimer.Start(GetROF(1))` (vt+0x318;
    /// `0x0051A55E..0x0051A587`, `0x0051A61E..0x0051A647`): its slot-1
    /// weapon at its own burst index (`+0x3B8`). Outside FireAt that index is
    /// below the retail Sapper's `Burst=`, so GetROF takes its mid-burst arm
    /// and draws `RandomRanged(3, 5)`; PTROOP's empty slot answers 1 without
    /// a draw.
    ///
    /// GetROF's Spark arm reads the same actual +308 link as FireAt.
    /// RESIDUAL: Fire/Railgun retained links remain unported. No retail
    /// planter's slot-1 weapon has those flags.
    fn rearm_c4_planter(
        interner: &StringInterner,
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        id: u64,
        building: u64,
        rules: &RuleSet,
    ) {
        let Some(planter) = substrate.entities.get(id) else {
            return;
        };
        let Some(obj) = interner.object_type(planter.type_ref(), rules) else {
            return;
        };
        let weapon = crate::sim::combat::combat_weapon::get_weapon(
            planter,
            obj,
            1,
            &substrate.entities,
            rules,
            interner,
        );
        let snap = crate::sim::combat::build_attacker_snapshot(
            planter,
            TargetKind::Entity(building),
            None,
        );
        let burst_index = planter.weapon_burst.index();
        let live = crate::sim::combat::rof::LiveParticles {
            spark: planter.spark_particle_system.system().is_some(),
            ..Default::default()
        };
        let rof = crate::sim::combat::world_fire::snapshot_get_rof(
            scenario_rng,
            &*substrate,
            houses,
            rules,
            &snap,
            obj,
            weapon,
            live,
            burst_index,
        );
        let frame = session.binary_frame() as i32;
        if let Some(planter) = substrate.entities.get_mut(id) {
            planter.rearm_timer.start(frame, rof);
        }
    }
}

#[cfg(test)]
#[path = "c4_plant_tests.rs"]
mod tests;
