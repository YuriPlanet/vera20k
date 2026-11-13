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
//! strikes: a screen ripple (an IonBlast, `sim::world::ion_blast`) and
//! `DominatorSecondAnim=` on the cell, `DominatorDamage=` with
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
//! - Launch's EVA line is skipped natively while `0x00A8B538` is set (a
//!   defeated client); VERA always plays it.
//! - The anim Process follows (`0x00A9FAC4`) has no pointer expiry natively;
//!   VERA treats a missing anim as finished. Dormant: the retail anims stay
//!   on their last stage for three frames (`Rate=300`), longer than Process
//!   needs to let go.
//! - A negative `DominatorCaptureRange=` indexes before the count table
//!   (`0x0053B186`); VERA walks only the centre cell. No retail data sets
//!   one.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimId;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::FrameEffects;
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
pub struct PsychicDominatorState {
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
    /// Ares: SW_PsychicDominator::CurrentPsyDom's type (`100C1C88`), held
    /// from Activate until the state machine's end.
    #[cfg(feature = "ares")]
    #[serde(default)]
    ares_super: Option<InternedId>,
    /// Ares PsychicDominatorStateMachine's Deferment (`+0x1C`), and whether
    /// the machine is registered (its clock not yet zeroed).
    #[cfg(feature = "ares")]
    #[serde(default)]
    ares_deferment: i32,
    #[cfg(feature = "ares")]
    #[serde(default)]
    ares_machine: bool,
    /// CurrentPsyDom's `Light.*`, which UpdateLighting (hook 53C280) reads
    /// while the Dominator tints the scenario.
    #[cfg(feature = "ares")]
    #[serde(default)]
    ares_light: Option<crate::rules::superweapon_type::AresSuperWeaponLighting>,
}

#[cfg(feature = "ares")]
impl PsychicDominatorState {
    /// CurrentPsyDom's `Light.*`, none without one.
    pub(crate) fn ares_light(
        &self,
    ) -> Option<crate::rules::superweapon_type::AresSuperWeaponLighting> {
        self.ares_super.and(self.ares_light)
    }
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
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    sim.sound_events.emit(SimSoundEvent::RadarEvent {
        radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
    });
    start(sim, rules, owner, (rx as i16, ry as i16));
    // `0x006CCDFF..0x006CCE28`: the cell's GetCoords (`0x00486840`), no deck.
    let ground = crate::sim::projectile::cell_ground_coord(sim.resolved_terrain.as_ref(), rx, ry);
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world: ground.into(),
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
    let crate::sim::projectile::ProjectileCoord { x, y, z } =
        crate::sim::projectile::cell_ground_coord(
            sim.resolved_terrain.as_ref(),
            cell.0 as u16,
            cell.1 as u16,
        );
    let anim = super::spawn_super_anim(
        sim,
        rules,
        &general.dominator_first_anim,
        [x, y, z.wrapping_add(FIRST_ANIM_LIFT)],
    );
    sim.psychic_dominator.anim = anim;
    sim.psychic_dominator.status = Status::Started;
    let frame = sim.session.binary_frame() as i32;
    sim.session
        .lighting_mut()
        .restart_transition_timer(frame, START_FADE_FRAMES);
    sim.update_lighting();
}

/// `PsychicDominator::Process @ 0x0053AF40` for one frame. Returns whether
/// the strike changed a bridge.
pub(super) fn process(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    // Ares hook 53AF40 (10037B40) skips the stock Process; its state
    // machine (10070BA0) runs in its place. On the build like the Ares
    // launch and AbortFire in `fire.rs` that start this state.
    if cfg!(feature = "ares") {
        #[cfg(feature = "ares")]
        return ares_process(sim, rules, overlay_registry, frame_effects);
    }
    match sim.psychic_dominator.status {
        Status::Idle => {}
        Status::Started => sim.psychic_dominator.status = Status::Rising,
        Status::Rising => {
            let (stage, frames) = anim_progress(sim, rules);
            if fires(stage, frames, rules.general.dominator_fire_at_percentage) {
                sim.psychic_dominator.status = Status::Struck;
                return mind_control_area(sim, rules, overlay_registry, frame_effects);
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
            let lighting = &sim.session.lighting();
            if lighting.current_ambient() == lighting.target_ambient() {
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
/// 1. a flagged IonBlast at the cell's GetCoords (`0x0053B087..0x0053B0D4`,
///    [`Simulation::add_dominator_ion_blast`]), the screen ripple;
/// 2. `DominatorSecondAnim=` at the same coordinate becomes the followed anim
///    (`0x0053B0DB..0x0053B145`);
/// 3. Apply_area_damage there with `DominatorDamage=` and `DominatorWarhead=`,
///    no source object and the Lightning Storm's house `0x00A9FACC` as the
///    source house (`0x0053B14B..0x0053B16B`): a storm's owner while one
///    counts down or rages, else none;
/// 4. for the first `count[min(DominatorCaptureRange=, 10)]` cells of the
///    cell-spread sweep around the cell (`0x0053B170..0x0053B391`): from the
///    ground list's Techno nearest the cell's (0, 0) sub-point
///    (`CellClass::Find_Nearest_Object @ 0x0047C3D0`; the ones listed before
///    it are passed over) through its successors
///    while they are Technos (`+0x14 & 1`, `0x0053B364..0x0053B37F`), each
///    that [`Simulation::can_be_perma_mind_controlled`] passes is captured
///    ([`Simulation::perma_capture`]) and kept;
/// 5. unless the owner is controlled by a human (`HouseClass::
///    IsControlledByHuman @ 0x0050B730`), the kept objects queue Hunt, the
///    last first (`0x0053B3A6..0x0053B3C4`).
///
/// Returns whether the area damage changed a bridge.
fn mind_control_area(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let general = &rules.general;
    let centre = sim.psychic_dominator.cell;
    let ground = crate::sim::projectile::cell_ground_coord(
        sim.resolved_terrain.as_ref(),
        centre.0 as u16,
        centre.1 as u16,
    );
    let coords = [ground.x, ground.y, ground.z];
    sim.add_dominator_ion_blast(coords);
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
            true,
            (crate::sim::combat::RAD_NO_ATTACKER, storm_house, warhead_id),
            frame_effects,
        )
        .bridge_state_changed;
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
                sim.perma_capture(id, owner, rules, overlay_registry, frame_effects);
                captured.push(id);
            }
            // Object+0x30 after the capture: a Terrain object ends the walk.
            current = match sim.next_cell_object(CellObjectMember::Entity(id)) {
                Some(CellObjectMember::Entity(next)) => Some(next),
                _ => None,
            };
        }
    }

    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let human = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
    if !human {
        let frame = sim.session.binary_frame();
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

#[cfg(any(test, feature = "test-support"))]
impl PsychicDominatorState {
    /// A Dominator in `status` (0..5) at `cell` following `anim`.
    pub fn for_test(
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
            ..Self::default()
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

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn fires_for_test(stage: i32, frames: i32, percent: i32) -> bool {
    fires(stage, frames, percent)
}

/// SW_PsychicDominator::Activate (Ares 3.0 `100707C0`, hook 6CC390): a
/// charged Super becomes CurrentPsyDom and registers its state machine with
/// the type's `SW.Deferment` (else 0); PsyDom's status becomes 1, its owner
/// the Super's, its cell `cell` and its anim none. Reports activation even
/// when uncharged.
#[cfg(feature = "ares")]
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    (rx, ry): (u16, u16),
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let launch = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch());
    let deferment = launch.and_then(|launch| launch.deferment()).unwrap_or(0);
    let state = &mut sim.psychic_dominator;
    state.ares_light = launch.map(|launch| launch.lighting());
    state.ares_super = Some(sw_type);
    state.ares_deferment = deferment;
    state.ares_machine = true;
    state.status = Status::Started;
    state.owner = Some(owner);
    state.cell = (rx as i16, ry as i16);
    state.anim = None;
}

/// SW_PsychicDominator::AbortFire (Ares 3.0 `10070780`), from ClickFire
/// (10035130): a Dominator already active (PsyDom::Active) refuses the
/// launch; the owner's client prints `Message.Abort`.
#[cfg(feature = "ares")]
pub(super) fn ares_abort_fire(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
) -> bool {
    if !active(sim) {
        return false;
    }
    sim.sound_events
        .emit(SimSoundEvent::AresSuperWeaponAborted { owner, sw_type });
    true
}

/// PsychicDominatorStateMachine::Update (Ares 3.0 `10070BA0`) for one frame,
/// with CurrentPsyDom's SWTypeExt, after a positive Deferment counts down:
/// - 1: the type's `Dominator.FirstAnim` (else `[General]
///   DominatorFirstAnim=`) `Dominator.FirstAnimHeight` over the cell's
///   GetCoords becomes PsyDom's anim; `SW.ActivationSound` (else
///   `[AudioVisual] PsychicDominatorActivateSound=`) plays there, every
///   client prints `Message.Activate`; status 2, the ambient fade timer
///   restarts for one frame and UpdateLighting runs;
/// - 2: once the anim has shown `frames * Dominator.FireAtPercentage / 100`
///   (integer; else `DominatorFireAtPercentage=`) frames, PsyDom::Fire
///   ([`ares_fire`]) and status 3;
/// - 3: within 10 frames of the anim's last, status 4;
/// - 4: within 1 frame, status 5, the cell empty, no anim, UpdateLighting;
/// - 5: once the ambient reaches its target, no CurrentPsyDom, status 0,
///   UpdateLighting, and the machine ends.
///
/// A missing anim passes each wait. Returns whether Fire changed a bridge.
#[cfg(feature = "ares")]
fn ares_process(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if !sim.psychic_dominator.ares_machine {
        return false;
    }
    let state = &mut sim.psychic_dominator;
    if state.ares_deferment > 0 {
        state.ares_deferment -= 1;
        if state.ares_deferment != 0 {
            return false;
        }
    }
    let Some(sw) = state
        .ares_super
        .and_then(|typ| rules.super_weapon(sim.interner.resolve(typ)))
    else {
        return false;
    };
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return false;
    };
    let dominator = metadata.launch().dominator();
    let general = &rules.general;
    match sim.psychic_dominator.status {
        Status::Started => {
            let (rx, ry) = sim.psychic_dominator.cell;
            let ground = crate::sim::projectile::cell_ground_coord(
                sim.resolved_terrain.as_ref(),
                rx as u16,
                ry as u16,
            );
            let coords = [
                ground.x,
                ground.y,
                ground.z.wrapping_add(dominator.first_anim_height),
            ];
            let anim = match &dominator.first_anim {
                Some(anim) => anim.clone(),
                None => Some(general.dominator_first_anim.clone()),
            }
            .and_then(|name| super::spawn_super_anim(sim, rules, &name, coords));
            sim.psychic_dominator.anim = anim;
            let sound = match &sw.presentation.activation_sound {
                Some(sound) => sound.clone(),
                None => general.psychic_dominator_activate_sound.clone(),
            };
            if let Some(sound_id) = sound {
                sim.sound_events.emit(SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate: crate::sim::components::DriveCoord {
                        x: coords[0],
                        y: coords[1],
                        z: coords[2],
                    },
                    flags: 0,
                });
            }
            if let (Some(owner), Some(sw_type)) = (
                sim.psychic_dominator.owner,
                sim.psychic_dominator.ares_super,
            ) {
                sim.sound_events
                    .emit(SimSoundEvent::AresSuperWeaponActivated { owner, sw_type });
            }
            sim.psychic_dominator.status = Status::Rising;
            let frame = sim.session.binary_frame() as i32;
            sim.session
                .lighting_mut()
                .restart_transition_timer(frame, START_FADE_FRAMES);
            sim.update_lighting();
        }
        Status::Rising => {
            if sim.psychic_dominator.anim.is_some() {
                let (stage, frames) = anim_progress(sim, rules);
                let percent = dominator
                    .fire_at_percentage
                    .unwrap_or(general.dominator_fire_at_percentage);
                if frames.wrapping_mul(percent) / 100 > stage {
                    return false;
                }
            }
            let changed = ares_fire(sim, rules, sw, overlay_registry, frame_effects);
            sim.psychic_dominator.status = Status::Struck;
            return changed;
        }
        Status::Struck => {
            let (stage, frames) = anim_progress(sim, rules);
            if sim.psychic_dominator.anim.is_none() || frames.wrapping_sub(stage) <= ENDING_FRAMES {
                sim.psychic_dominator.status = Status::Ending;
            }
        }
        Status::Ending => {
            let (stage, frames) = anim_progress(sim, rules);
            if sim.psychic_dominator.anim.is_none() || frames.wrapping_sub(stage) <= LAST_FRAMES {
                let state = &mut sim.psychic_dominator;
                state.status = Status::FadingBack;
                state.cell = (0, 0);
                state.anim = None;
                sim.update_lighting();
            }
        }
        Status::FadingBack => {
            let lighting = sim.session.lighting();
            if lighting.current_ambient() == lighting.target_ambient() {
                let state = &mut sim.psychic_dominator;
                state.ares_super = None;
                state.status = Status::Idle;
                state.ares_machine = false;
                sim.update_lighting();
            }
        }
        Status::Idle => {}
    }
    false
}

/// The PsyDom_Fire hook (Ares 3.0 `10037840`, hook 53B080) that replaces
/// `PsyDom::MindControlArea`, at the cell's GetCoords with CurrentPsyDom's
/// SWTypeExt:
/// 1. `Dominator.Ripple`: a flagged IonBlast (no beam);
/// 2. `SW.CreateRadarEvent`: a type-13 radar event at the cell;
/// 3. `Dominator.SecondAnim` (else `[General] DominatorSecondAnim=`)
///    `Dominator.SecondAnimHeight` up becomes PsyDom's anim;
/// 4. a positive GetDamage (`SW.Damage`, else `DominatorDamage=`) with a
///    GetWarhead (`SW.Warhead`, else `DominatorWarhead=`): DamageArea there,
///    affecting Tiberium, no source, from PsyDom's owner;
/// 5. `Dominator.Capture`: each Techno on GetRange (`10070900`: `SW.Range`,
///    else the `DominatorCaptureRange=` spread) once, in UniqueID order,
///    through the Dominate lambda (`10035FE0`): not curtained, BalloonHover,
///    in the air or driverless; admitted by `SW.AffectsHouse`/
///    `SW.AffectsTarget`; a controlled one only with
///    `Dominator.CaptureMindControlled`, a permanently controlled one only
///    with `.CapturePermaMindControlled`, an `ImmuneToPsionics=` (or
///    PSIONICSIMMUNE) one only with `.CaptureImmuneToPsionics`; captured
///    ([`Simulation::ares_dominate`]); a computer owner then sends each captured
///    Foot to Hunt, first to last.
#[cfg(feature = "ares")]
fn ares_fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return false;
    };
    let launch = metadata.launch();
    let targeting = metadata.targeting();
    let dominator = launch.dominator().clone();
    let general = &rules.general;
    let owner = sim.psychic_dominator.owner;
    let centre = sim.psychic_dominator.cell;
    let ground = crate::sim::projectile::cell_ground_coord(
        sim.resolved_terrain.as_ref(),
        centre.0 as u16,
        centre.1 as u16,
    );
    let coords = [ground.x, ground.y, ground.z];
    if dominator.ripple {
        sim.add_dominator_ion_blast(coords);
    }
    if launch.radar_event() {
        sim.sound_events.emit(SimSoundEvent::RadarEvent {
            radar: RadarEventRequest::new(
                RadarEventType::ImpactSilent,
                centre.0 as u16,
                centre.1 as u16,
            ),
        });
    }
    sim.psychic_dominator.anim = None;
    let second = match &dominator.second_anim {
        Some(anim) => anim.clone(),
        None => Some(general.dominator_second_anim.clone()),
    };
    if let Some(second) = second {
        let at = [
            coords[0],
            coords[1],
            coords[2].wrapping_add(dominator.second_anim_height),
        ];
        sim.psychic_dominator.anim = super::spawn_super_anim(sim, rules, &second, at);
    }
    let mut bridge_changed = false;
    let damage = launch.damage().unwrap_or(general.dominator_damage);
    let warhead = match launch.warhead() {
        Some(warhead) => warhead.map(str::to_owned),
        None => Some(general.dominator_warhead.clone()),
    };
    if damage > 0
        && let Some(name) = warhead
        && let Some(warhead) = rules.warhead(&name)
    {
        let [x, y, z] = coords;
        let warhead_id = sim.interner.intern(&name);
        bridge_changed = crate::sim::combat::world_receiver::apply_area_damage(
            sim,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(x, y, z),
            damage,
            warhead,
            true,
            (crate::sim::combat::RAD_NO_ATTACKER, owner, warhead_id),
            frame_effects,
        )
        .bridge_state_changed;
    }
    let Some(owner) = owner.filter(|_| dominator.capture) else {
        return bridge_changed;
    };
    let range = match launch.range() {
        (width, height) if width < 0.0 && height < 0 => (general.dominator_capture_range as f32, 0),
        range => range,
    };
    let ring = match &dominator.control_anim {
        Some(anim) => anim.clone(),
        None => rules.mind_control.perma_controlled_anim.clone(),
    };
    let frame = sim.session.binary_frame();
    let mut minions = Vec::new();
    for id in super::protect::area_technos_in_spread(sim, range, (centre.0 as u16, centre.1 as u16))
    {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        let Some(object) = sim.object_type(entity.type_ref(), rules) else {
            continue;
        };
        let immune = object.immune_to_psionics
            || object
                .promotion_abilities()
                .is_some_and(|abilities| abilities.psionics_immune(entity.veterancy.raw()));
        let controlled = entity.mind_control.controller().is_some();
        if crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            frame,
        ) || object.balloon_hover
            || crate::sim::movement::air_movement::is_high_flying(
                entity,
                sim.resolved_terrain.as_ref(),
                Some((rules, &sim.interner)),
            )
            || entity.driver_killed()
            || !targeting
                .affects_house()
                .intersects(sim.ares_house_relation(owner, entity.owner()))
            || !super::protect::techno_affected(sim, entity, targeting.affects_target())
            || controlled && !dominator.capture_mind_controlled
            || entity.mind_control.permanent()
                && !controlled
                && !dominator.capture_perma_mind_controlled
            || immune && !dominator.capture_immune_to_psionics
        {
            continue;
        }
        let foot = entity.category() != crate::map::entities::EntityCategory::Structure;
        sim.ares_dominate(
            id,
            owner,
            dominator.permanent_capture,
            ring.as_deref(),
            rules,
            overlay_registry,
            frame_effects,
        );
        if foot {
            minions.push(id);
        }
    }
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    let human = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
    if !human {
        let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
        for id in minions {
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
