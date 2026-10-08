//! The Psychic Dominator: `SuperClass::Launch @ 0x006CC390` case 7
//! (`0x006CCDBD..0x006CCE61`), `PsyDom::Start @ 0x0053AE50`,
//! `PsychicDominator::Process @ 0x0053AF40` and
//! `PsyDom::MindControlArea @ 0x0053B080`.
//!
//! One Dominator runs at a time (the globals `0x00A9FA48..0x00A9FAC8`), and
//! ClickFire refuses another while it does (`fire.rs`). The launch raises
//! `[General] DominatorFirstAnim=` 750 leptons over the target and tints the
//! scenario with the map's `Dominator*=` lighting (`light_sources`). Once the
//! anim has played `DominatorFireAtPercentage=` of its frames, MindControlArea
//! strikes: `DominatorSecondAnim=` on the cell, `DominatorDamage=` with
//! `DominatorWarhead=` around it, and every eligible object of the cells
//! within `DominatorCaptureRange=` permanently joins the launcher's house
//! (`capture_manager`). Near the second anim's end the tint fades back, and
//! the Dominator is over when the ambient has returned.
//!
//! Process runs inside `LightningStorm::Process @ 0x0053A6C0` (`0x0053A742`),
//! after its nuke-flash step and before the storm's own work, from the
//! pre-object superweapon slot ([`super::tick_active_superweapon_effects`]).
//! A computer house aims its own through `AI_Fire_PsyDom` (`super::ai_fire`),
//! and its All_To_Hunt destroys the `Insignificant=` objects it holds
//! (`sim::house_strategy`).
//!
//! Evidence: instruction reading at the addresses cited here. Native
//! execution (`tools/superweapon_oracle.py`, replayed in
//! `psychic_dominator_tests.rs`): Process's statuses and status 2's firing
//! stage for every percent 0..100 and anim frame count 1..64, Start's writes
//! and anim, UpdateLighting's selections, the ambient fade's intervals and
//! ClickFire's refusal; the map's `Dominator*=` conversions; and the
//! Ground/Level a full cell relight adds in each lighting state (`relight`,
//! replayed through the match's lighting in `app::loading::init`'s tests).
//!
//! Scenario draws: none of its own; the area damage draws through its
//! receivers, and a released captive's DecideUnitFate through FreeUnit
//! (`capture_manager`). Timer writes: Start restarts the scenario's ambient
//! fade timer (`ScenarioClass+0x1248`, `[frame, 1]`). Detach calls: FreeUnit
//! for a captive a controller held, and the perma ring's SetOwnerObject.
//!
//! RESIDUALS:
//! - MindControlArea's `IonBlastClass` at the cell (`0x0053B089..0x0053B0D4`,
//!   flag `+0x10` set, so its update applies no forces) only draws a screen
//!   ripple for 79 frames at high detail (`IonBlastClass::DrawAll @
//!   0x0053D850`); VERA draws none. Trigger: every strike. Effect: no ripple.
//! - Launch's EVA line is skipped natively while `0x00A8B538` is set (a
//!   defeated client); VERA always plays it.
//! - The anim Process follows (`0x00A9FAC4`) has no pointer expiry natively;
//!   VERA treats a missing anim as finished. Dormant: the retail anims stay
//!   on their last stage for three frames (`Rate=300`), longer than Process
//!   needs to let go.
//! - A negative `DominatorCaptureRange=` indexes before the count table
//!   (`0x0053B186`); VERA walks only the centre cell. No retail data sets
//!   one.
//! - The refusal message for the player (`PsyDom::PrintMessage @ 0x0053B410`,
//!   `Msg:DominatorActive`) is not posted.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimId;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};

/// Leptons Start raises the first anim over the cell (`ADD EAX,0x2EE` at
/// `0x0053AEBB`).
const FIRST_ANIM_LIFT: i32 = 750;
/// Status 3 ends once the anim is this few frames from its last
/// (`0x0053AFE1`).
const ENDING_FRAMES: i32 = 10;
/// Status 4 ends at the anim's last frame (`0x0053B012`).
const LAST_FRAMES: i32 = 1;
/// MindControlArea's cap on `DominatorCaptureRange=` (`0x0053B17C`).
const MAX_CAPTURE_BAND: i32 = 10;
/// The double 0.01 status 2 multiplies the percentage by (`[0x007E3808]`).
const PERCENT_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3f84_7ae1_47ae_147b);
/// `ScenarioClass+0x1248`'s duration after Start (`0x0053AF24`).
const START_FADE_FRAMES: i32 = 1;

/// The Dominator's status, `0x00A9FAC0`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
enum Status {
    /// 0: none.
    #[default]
    Idle,
    /// 1: Start ran; the next Process moves on.
    Started,
    /// 2: the first anim rises until the firing percentage.
    Rising,
    /// 3: struck; the second anim plays.
    Struck,
    /// 4: the second anim nears its end.
    Ending,
    /// 5: the tint fades back to the scenario's ambient.
    FadingBack,
}

/// The Dominator's globals. Owned here; the world hash folds the fields in
/// declaration order.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct PsychicDominatorState {
    /// `0x00A9FAC0`.
    status: Status,
    /// `0x00A9FA48`: the target cell; the end of the strike writes the
    /// empty cell `0x00A9F9F8`, `(0, 0)`.
    cell: (i16, i16),
    /// `0x00A9FAC8`: the launching house. Only Start writes it.
    owner: Option<InternedId>,
    /// `0x00A9FAC4`: the anim Process follows, the first one and from the
    /// strike the second.
    anim: Option<AnimId>,
}

/// `PsyDom::Active @ 0x0053B400`: any status but 0, the fade back included.
pub(crate) fn active(sim: &Simulation) -> bool {
    sim.psychic_dominator.status != Status::Idle
}

/// UpdateLighting's Dominator test (`0x0053C313..0x0053C31F`): a status
/// neither 0 nor 5 tints the scenario.
pub(crate) fn tints_scenario(sim: &Simulation) -> bool {
    !matches!(
        sim.psychic_dominator.status,
        Status::Idle | Status::FadingBack
    )
}

/// Launch case 7 for `owner`'s Super of `sw_type` at `cell`: a charged Super
/// (`+0x6F`, which ClickFire's launch always is) raises a type-13 radar event
/// at the cell on every client (`0x006CCDD7`) and starts the Dominator
/// ([`start`]). The launch event carries the rest, which the app plays:
/// `EVA_PsychicDominatorActivated` (`0x006CCDFA`), `[AudioVisual]
/// PsychicDominatorActivateSound=` at the cell's GetCoords (`0x006CCE28`) and,
/// for the local player, the dropped selection and queued
/// `EVA_PsychicDominatorReady` (`0x006CCE41..0x006CCE52`).
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    (rx, ry): (u16, u16),
) -> bool {
    let charged = sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type))
        .is_some_and(|instance| instance.is_ready);
    if !charged {
        return false;
    }
    sim.sound_events.push(SimSoundEvent::SuperWeaponRadarEvent {
        radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
    });
    start(sim, rules, owner, (rx as i16, ry as i16));
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx,
        ry,
    });
    true
}

/// `PsyDom::Start @ 0x0053AE50`: unless either Dominator anim is unset
/// (`0x0053AE58..0x0053AE6E`), the owner and cell are stored, the first anim
/// rises [`FIRST_ANIM_LIFT`] over the cell's GetCoords with the superweapon
/// row and becomes the followed anim, the status becomes 1, the ambient fade
/// timer restarts for one frame and UpdateLighting runs. Nothing tests a
/// Dominator already running.
fn start(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, cell: (i16, i16)) {
    let general = &rules.general;
    if general.dominator_first_anim.trim().is_empty()
        || general.dominator_second_anim.trim().is_empty()
    {
        return;
    }
    sim.psychic_dominator.owner = Some(owner);
    sim.psychic_dominator.cell = cell;
    let [x, y, z] = super::fire::cell_coords(sim, (cell.0 as u16, cell.1 as u16));
    let anim = super::spawn_super_anim(
        sim,
        rules,
        &general.dominator_first_anim,
        [x, y, z.wrapping_add(FIRST_ANIM_LIFT)],
    );
    sim.psychic_dominator.anim = anim;
    sim.psychic_dominator.status = Status::Started;
    let frame = sim.session.binary_frame as i32;
    sim.session
        .lighting
        .restart_transition_timer(frame, START_FADE_FRAMES);
    sim.update_lighting();
}

/// `PsychicDominator::Process @ 0x0053AF40` for one frame. Returns whether
/// the strike changed a bridge.
pub(super) fn process(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    match sim.psychic_dominator.status {
        Status::Idle => {}
        Status::Started => sim.psychic_dominator.status = Status::Rising,
        Status::Rising => {
            let (stage, frames) = anim_progress(sim, rules);
            if fires(stage, frames, rules.general.dominator_fire_at_percentage) {
                sim.psychic_dominator.status = Status::Struck;
                return mind_control_area(sim, rules, overlay_registry);
            }
        }
        Status::Struck => {
            let (stage, frames) = anim_progress(sim, rules);
            if frames.wrapping_sub(stage) <= ENDING_FRAMES {
                sim.psychic_dominator.status = Status::Ending;
            }
        }
        Status::Ending => {
            let (stage, frames) = anim_progress(sim, rules);
            if frames.wrapping_sub(stage) <= LAST_FRAMES {
                let state = &mut sim.psychic_dominator;
                state.status = Status::FadingBack;
                state.cell = (0, 0);
                state.anim = None;
                sim.update_lighting();
            }
        }
        Status::FadingBack => {
            // `ScenarioClass+0x3530 == +0x352C` (`0x0053B03F..0x0053B054`).
            let lighting = &sim.session.lighting;
            if lighting.current_ambient == lighting.target_ambient {
                sim.psychic_dominator.status = Status::Idle;
                // Later relights leave PsyDom::Active's arm; no cell refreshes.
                sim.publish_relight_profile();
            }
        }
    }
    false
}

/// Status 2's test (`0x0053AF64..0x0053AFAA`), under the process control
/// word (53-bit chop): the strike fires once `percent * 0.01` is at most
/// `stage / frames`. A zero frame count (a masked divide by zero) gives +inf
/// for a positive stage and NaN for a zero one, which fire (an unordered
/// compare sets C0 and C3), and -inf for a negative one, which does not.
fn fires(stage: i32, frames: i32, percent: i32) -> bool {
    let Ok(ratio) = X87Chop53::div(X87Chop53::load_i32(stage), X87Chop53::load_i32(frames)) else {
        return stage >= 0;
    };
    let threshold = X87Chop53::mul(
        X87Chop53::load_i32(percent),
        X87Chop53::load_f64(PERCENT_SCALE).expect("finite constant"),
    );
    X87Chop53::compare(threshold, ratio) != X87Ordering::Greater
}

/// The followed anim's stage and its image's frame count
/// ([`Simulation::anim_stage_and_frames`]). A missing anim reads as finished
/// (module residual).
fn anim_progress(sim: &Simulation, rules: &RuleSet) -> (i32, i32) {
    sim.psychic_dominator
        .anim
        .and_then(|anim| sim.anim_stage_and_frames(anim, rules))
        .unwrap_or((0, 0))
}

/// `PsyDom::MindControlArea @ 0x0053B080`:
/// 1. `DominatorSecondAnim=` at the cell's GetCoords becomes the followed
///    anim (`0x0053B0DB..0x0053B145`);
/// 2. Apply_area_damage there with `DominatorDamage=` and `DominatorWarhead=`,
///    no source object and the Lightning Storm's house `0x00A9FACC` as the
///    source house (`0x0053B14B..0x0053B16B`): a storm's owner while one
///    counts down or rages, else none;
/// 3. for the first `count[min(DominatorCaptureRange=, 10)]` cells of the
///    cell-spread sweep around the cell (`0x0053B170..0x0053B391`): from the
///    ground list's Techno nearest the cell's (0, 0) sub-point
///    (`CellClass::Find_Nearest_Object @ 0x0047C3D0`; the ones listed before
///    it are passed over) through its successors
///    while they are Technos (`+0x14 & 1`, `0x0053B364..0x0053B37F`), each
///    that [`Simulation::can_be_perma_mind_controlled`] passes is captured
///    ([`Simulation::perma_capture`]) and kept;
/// 4. unless the owner is controlled by a human (`HouseClass::
///    IsControlledByHuman @ 0x0050B730`), the kept objects queue Hunt, the
///    last first (`0x0053B3A6..0x0053B3C4`).
///
/// Returns whether the area damage changed a bridge.
fn mind_control_area(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    let general = &rules.general;
    let centre = sim.psychic_dominator.cell;
    let coords = super::fire::cell_coords(sim, (centre.0 as u16, centre.1 as u16));
    sim.psychic_dominator.anim =
        super::spawn_super_anim(sim, rules, &general.dominator_second_anim, coords);

    let mut bridge_changed = false;
    if let Some(warhead) = rules.warhead(&general.dominator_warhead) {
        let warhead_id = sim.interner.intern(&general.dominator_warhead);
        let storm_house = sim.lightning_storm.owner();
        let [x, y, z] = coords;
        bridge_changed = crate::sim::combat::world_receiver::apply_area_damage(
            sim,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(x, y, z),
            general.dominator_damage,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, storm_house, warhead_id),
        );
    }

    let Some(owner) = sim.psychic_dominator.owner else {
        return bridge_changed;
    };
    let band = general.dominator_capture_range.clamp(0, MAX_CAPTURE_BAND) as usize;
    let mut captured = Vec::new();
    for &(dx, dy) in crate::sim::combat::cell_spread::sweep(band) {
        let x = centre.0.wrapping_add(dx);
        let y = centre.1.wrapping_add(dy);
        // `MapClass::operator[]`'s cell; Find_Nearest_Object's alt argument 0
        // reads its ground list whatever the bridge flag.
        let Some((cell, _)) = super::cell_grid::selected_cell_list(sim, x, y) else {
            continue;
        };
        let mut current = sim.nearest_cell_object(cell, MovementLayer::Ground, None);
        while let Some(id) = current {
            if sim.can_be_perma_mind_controlled(id, rules) {
                sim.perma_capture(id, owner, rules, overlay_registry);
                captured.push(id);
            }
            // Object+0x30 after the capture: a Terrain object ends the walk.
            current = match sim.next_cell_object(CellObjectMember::Entity(id)) {
                Some(CellObjectMember::Entity(next)) => Some(next),
                _ => None,
            };
        }
    }

    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let human = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
    if !human {
        let frame = sim.session.binary_frame;
        let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
        for id in captured.into_iter().rev() {
            let _ = sim.mission_queue_exact(
                id,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Hunt),
                0,
                frame,
                &readiness,
            );
        }
    }
    bridge_changed
}

#[cfg(test)]
impl PsychicDominatorState {
    /// A Dominator in `status` (0..5) at `cell` following `anim`.
    pub(crate) fn for_test(
        status: u8,
        cell: (i16, i16),
        owner: Option<InternedId>,
        anim: Option<AnimId>,
    ) -> Self {
        let status = match status {
            0 => Status::Idle,
            1 => Status::Started,
            2 => Status::Rising,
            3 => Status::Struck,
            4 => Status::Ending,
            5 => Status::FadingBack,
            other => panic!("status {other}"),
        };
        Self {
            status,
            cell,
            owner,
            anim,
        }
    }

    /// The status as native numbers it (`0x00A9FAC0`).
    pub(crate) fn status_number(&self) -> u8 {
        self.status as u8
    }

    pub(crate) fn cell(&self) -> (i16, i16) {
        self.cell
    }

    pub(crate) fn anim(&self) -> Option<AnimId> {
        self.anim
    }

    pub(crate) fn owner(&self) -> Option<InternedId> {
        self.owner
    }
}

#[cfg(test)]
pub(crate) fn fires_for_test(stage: i32, frames: i32, percent: i32) -> bool {
    fires(stage, frames, percent)
}
