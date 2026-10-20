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
//! RESIDUAL:
//! - The arm's Tag event 1 ("entered by", `0x0051A507..0x0051A51C`) is not
//!   raised; the Engineer and spy arms leave the same event unraised. It
//!   fires on every arrival at the NavCom building, before the refusal
//!   tests, when the building holds a map Tag (`world_spawn` attaches them).
//!   Trigger: a C4 infantryman reaching a tagged map building; how often
//!   retail skirmish maps tag buildings is not measured. Effect: a trigger
//!   waiting on that event never springs.
//! - The cell arm that follows (`0x0051A667..0x0051A7F2`): a C4 infantryman
//!   on Sabotage standing in its NavCom cell damages the area around it
//!   with `C4Warhead=`, scatters and queues mission 2. Its producer, a
//!   player's Sabotage order on a cell (a bridge), has no VERA order.
//!   Trigger: a player's C4 click on a bridge. Effect: a bridge cannot be
//!   demolished that way, only through its repair hut.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::mission::MissionType;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation};

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
    /// arrival refuses a planter that is not `Fraidycat=` (module doc).
    ///
    /// Answers whether the arm ran; it returns before the Foot body.
    pub(crate) fn infantry_per_cell_c4(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        if infantry.mission.effective().known() != Some(MissionType::Sabotage)
            || !self
                .object_type(infantry.type_ref(), rules)
                .is_some_and(|object| object.c4)
        {
            return Ok(false);
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
            return Ok(false);
        };
        let frame = self.session.binary_frame();
        let Some(target) = self.substrate.entities.get(building) else {
            return Ok(false);
        };
        let refused = target.mission.effective().known() == Some(MissionType::Selling)
            || crate::sim::superweapon::invulnerability::is_invulnerable(
                target.invulnerability.as_ref(),
                frame,
            );
        if !refused {
            if target.pending_c4_detonation.pending().is_some() {
                self.assign_null_destination(id, Some(rules), registry, effects);
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
                return Ok(true);
            }
            let (flash, fuse) = (rules.c4_flash_frames(), rules.c4_delay_frames());
            if let Some(target) = self.substrate.entities.get_mut(building) {
                target.flash_base(flash);
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
        Ok(true)
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
