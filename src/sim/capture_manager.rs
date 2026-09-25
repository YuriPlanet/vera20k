//! `CaptureManagerClass`: mind control (Yuri, Yuri Prime, Psychic Commando,
//! Mastermind, Psychic Tower).
//!
//! Native owner: one manager per Techno whose weapon 0 carries a
//! `MindControl=` warhead (`TechnoClass::Init_Managers @ 0x006F4090..
//! 0x006F40F2`, `TechnoClass+0x2BC`; ctor `0x004717D0(owner, Damage,
//! InfiniteMindControl)`). The firer's Inviso `[PsychicControl]` shot reaches
//! the MindControl arm of `BulletClass::DetonateAtCoord` (`0x0046920B..
//! 0x0046933E`), which calls [`Simulation::capture_unit`] on the bullet's own
//! target. The victim holds only the back-link [`MindControlLink`]
//! (`TechnoClass+0x2C0` MindControlledBy, `+0x2C8` its ring anim); the
//! controller owns the ordered nodes and every state change.
//!
//! Releases (`FreeUnit @ 0x00471FF0`, through `FreeAll @ 0x00472140`): the
//! controller's death arm (`TechnoClass::ReceiveDamage @ 0x00702112`; the
//! later `BuildingClass::ReceiveDamage` FreeAll at `0x004424F9` finds the
//! manager already empty), Foot UnInit (`0x004DE5DD`), a drain link
//! installed on it (`0x0070FDBD`), a Psychic Tower going offline (the
//! `0x004549B0` off edge, FreeAll at `0x00454B47`), which a sale reaches
//! because Selling ends Is_Operational (`0x004555D0`) before the tower is
//! removed, CaptureUnit's single-victim replacement (`0x00471D98`), a Chrono
//! beam's warp start on the controller (`TemporalClass::InitiateWarp @
//! 0x0071AF48`), and a captive boarding a building or transport (the
//! PerCellProcess sites `0x0051A2DA`, `0x0051A438`, `0x0073A2CD`,
//! `0x0073A72B`). A victim's own
//! death only drops its node (`TechnoClass::PointerExpired @ 0x00707B14` ->
//! `0x00471F90`): no owner change, no sound, no draw. The victim's
//! `+0x2C0` also bars deploying an MCV (`0x00700ED0`) and repacking a
//! Construction Yard (`0x00449C15`, `0x0044F614`).
//!
//! Scenario draws: DecideUnitFate takes one `RandomRanged(1, 100)` when the
//! object's current house is not human (`0x004724F2`); a Mastermind's damaging
//! overload check takes five pairs of `RandomRanged(-200, 200)` and, while it
//! survives above the first row, one `RandomRanged(0, 100)`.
//!
//! Evidence: control flow and ordering read from the disassembly at the
//! addresses cited here, with Ghidra plates on the functions. Native
//! execution (`tools/spatial_oracle/`, compared in the `native_` tests):
//! `capture_decide_fate.py` runs the original DecideUnitFate (reasons and
//! their boundaries, the draw, the choice walk; every row compared);
//! `capture_overload_update.py` the original constructor and overload Update
//! frame by frame (the frames that check, the damage amount, the voice
//! latch and where it plays, the spark offsets and draws, the lean draw;
//! the ReceiveDamage arguments are recorded natively and mirrored at the
//! call site, not observed on the Rust side); `capture_ring_height.py` the
//! static initializer behind a building ring's height. Retail data: `[Controller]`,
//! `[ControllerBuilding]`, `[PsychicControl]` (Inviso), `[CombatDamage]
//! Overload*=`, `[General] AICapture*=`.
//!
//! RESIDUALS:
//! - The Psychic Dominator (`PsychicDominator::MindControlArea @ 0x0053B080`)
//!   and its permanent byte `+0x2C4` are not ported; `is_mind_controlled`
//!   reads `+0x2C0` alone. Trigger: the Dominator superweapon. Effect: no
//!   permanent capture. Frequency: per Dominator strike.
//! - DecideUnitFate's Team arms: VERA has no Team membership, so a captive
//!   leaves no team (`0x006EA870`), an "Add To Team" roll and the TeamType's
//!   MindControlDecision override (`Type+0xC0`) never apply, and "Add To Team"
//!   falls back to Hunt as native does for a teamless controller. The
//!   "Put in Grinder"/"Put in Bio Reactor" arms (`0x004DFA70`/`0x004DFB70`)
//!   walk the house's grinder and absorber lists, which VERA does not keep,
//!   so they fall back to Hunt. Trigger: an AI-owned controller (or an AI
//!   house getting a unit back). Effect: the unit hunts where native may send
//!   it into a Grinder or Bio Reactor. Frequency: AI Yuri games with those
//!   buildings; no skirmish AI yet.
//! - The link line (`DrawLinks @ 0x00472160`, the node timer from
//!   `MindControlAttackLineFrames=`), the overload flash counter (`+0x44`)
//!   and the Mastermind's `PipScale=MindControl` pips (`0x0070A15C` ->
//!   `0x00708C30` case 5 -> Count `0x004722D0`) feed only rendering and are
//!   not kept.
//! - The Mastermind's overload rocking (`+0x330`, sideways lean 0.015/0.03)
//!   has no VERA rocking producer; its Scenario draw is kept.
//! - An expired ring anim clears the victim's `+0x2C8` natively
//!   (`TechnoClass::PointerExpired @ 0x00707946`, `0x0071043D`); VERA clears
//!   the link only in FreeUnit, so a ring that ran out of loops leaves a
//!   stale anim id in the persisted and hashed link. Harmless while stable
//!   ids are never reused.
//! - A type's `MindClearedSound=` naming an unknown sound is -1 natively and
//!   FreeUnit falls back to the global (`0x00472057`); VERA keeps the name
//!   and plays nothing. No stock type authors the key.
//! - `AICaptureWoundedMark=` is read natively as a double and stored with
//!   `FSTP float` under the process chop control word (`0x00670504..
//!   0x00670509`); VERA parses to the nearest `f32`. Equal for stock `.25`;
//!   an authored mark can differ by one ulp.
//! - A crushed controller frees its victims at the crush call (`0x007418E5`,
//!   before Record_The_Kill); VERA frees them in the crushed object's UnInit
//!   a moment later in the same frame.

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::mind_control_rules::CaptureReason;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimId, AnimWorldCoord};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{SimSoundEvent, Simulation};

/// The manager constructor's overload check countdown (`+0x4C = 30`).
const OVERLOAD_FIRST_CHECK_FRAMES: i32 = 30;
/// `AnimClass(type, coord, 0, 1, 0x600, 0, 0)` at `0x00471F1D`.
const RING_ANIM_DRAW_FLAGS: u32 = 0x600;
/// A building victim's ring ZAdjust (`anim+0x100 = -1024`, `0x00471F66`).
const BUILDING_RING_Z_ADJUST: i32 = -1024;

/// One `ControlNode` (`operator new(0x14)`): the victim and the house it had
/// before the capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlNode {
    /// `+0x0` the controlled Techno.
    victim: u64,
    /// `+0x4` its house before the capture; FreeUnit restores it.
    original_owner: InternedId,
}

/// Retained `CaptureManagerClass` fields; the owner (`+0x48`) is the entity
/// storing this value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CaptureManagerState {
    /// `+0x3C` weapon 0's signed `Damage=`: the link limit.
    max_control: i32,
    /// `+0x40` `InfiniteMindControl=`: no capacity gate; only such a manager
    /// overloads.
    infinite: bool,
    /// `+0x24` the ControlNode vector, in native order.
    nodes: Vec<ControlNode>,
    /// `+0x41` the overload voice played this overload episode.
    overload_sound_played: bool,
    /// `+0x4C` frames until the next overload check.
    overload_countdown: i32,
}

impl CaptureManagerState {
    fn constructed(max_control: i32, infinite: bool) -> Self {
        Self {
            max_control,
            infinite,
            nodes: Vec::new(),
            overload_sound_played: false,
            overload_countdown: OVERLOAD_FIRST_CHECK_FRAMES,
        }
    }

    /// `IsFull @ 0x004722A0`: a finite manager at its limit. Gates
    /// `ShouldRetaliate` (`0x0070882F`) and `CanAcquireTarget` (`0x00709230`).
    pub fn is_full(&self) -> bool {
        !self.infinite && self.count() >= self.max_control
    }

    /// `HasAny @ 0x004722C0`.
    pub(crate) fn has_any(&self) -> bool {
        !self.nodes.is_empty()
    }

    fn count(&self) -> i32 {
        i32::try_from(self.nodes.len()).unwrap_or(i32::MAX)
    }

    /// CanCapture's capacity gate (`0x00471C90`, gate 8): a finite manager
    /// below its limit, or one limited to a single victim, which CaptureUnit
    /// replaces.
    fn admits_another(&self) -> bool {
        self.infinite || self.count() < self.max_control || self.max_control == 1
    }

    /// The controlled objects, in node order.
    pub(crate) fn victims(&self) -> impl DoubleEndedIterator<Item = u64> + '_ {
        self.nodes.iter().map(|node| node.victim)
    }

    /// `GetOriginalOwner @ 0x004722F0`: reverse node scan, `None` (native
    /// null) without a node. Read through `TechnoClass 0x0070F820` by the
    /// HouseClass defeat blow-up (`house_defeat`); FreeUnit reads its node
    /// directly.
    pub(crate) fn original_owner(&self, victim: u64) -> Option<InternedId> {
        self.nodes
            .iter()
            .rev()
            .find(|node| node.victim == victim)
            .map(|node| node.original_owner)
    }

    /// The rewrite half of `SetOriginalOwnerToCivilian @ 0x00472330`: every
    /// node controlling `victim` (reverse scan, all matches) now returns it
    /// to `house`. The caller found the Civilian-side house.
    pub(crate) fn set_original_owner(&mut self, victim: u64, house: InternedId) {
        for node in self.nodes.iter_mut().rev() {
            if node.victim == victim {
                node.original_owner = house;
            }
        }
    }

    /// `RemoveNode @ 0x00471F90`, the pointer-expiry listener: a dead
    /// victim's node goes silently.
    pub(crate) fn pointer_expired(&mut self, stable_id: u64) {
        self.nodes.retain(|node| node.victim != stable_id);
    }

    #[cfg(test)]
    pub(crate) fn with_victims_for_test(max_control: i32, infinite: bool, victims: &[u64]) -> Self {
        let mut state = Self::constructed(max_control, infinite);
        state.nodes = victims
            .iter()
            .map(|&victim| ControlNode {
                victim,
                original_owner: InternedId::default(),
            })
            .collect();
        state
    }

    /// The pre-196 hash projection: limit, infinity and the victim ids.
    pub(crate) fn hash_before_mind_control(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.max_control.hash(hasher);
        self.infinite.hash(hasher);
        self.victims().collect::<Vec<_>>().hash(hasher);
    }

    #[cfg(test)]
    pub(crate) fn reverse_nodes_for_test(&mut self) {
        self.nodes.reverse();
    }

    /// A manager holding these `(victim, original owner)` nodes in order.
    #[cfg(test)]
    pub(crate) fn with_nodes_for_test(nodes: &[(u64, InternedId)]) -> Self {
        let mut state = Self::constructed(3, false);
        state.nodes = nodes
            .iter()
            .map(|&(victim, original_owner)| ControlNode {
                victim,
                original_owner,
            })
            .collect();
        state
    }
}

/// The victim side: `TechnoClass+0x2C0` MindControlledBy and `+0x2C8` the
/// ring anim. Written only by this module.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MindControlLink {
    controller: Option<u64>,
    ring_anim: Option<AnimId>,
}

impl MindControlLink {
    /// `TechnoClass::IsMindControlled @ 0x007105E0` (`+0x2C0 || +0x2C4`;
    /// the Dominator's `+0x2C4` is a module residual).
    pub fn is_mind_controlled(&self) -> bool {
        self.controller.is_some()
    }

    pub(crate) fn controller(&self) -> Option<u64> {
        self.controller
    }

    #[cfg(test)]
    pub(crate) fn controlled_by_for_test(controller: u64) -> Self {
        Self {
            controller: Some(controller),
            ring_anim: None,
        }
    }
}

/// CanCapture's victim-side gates (`0x00471C90`): gates 3-7 and 9 read the
/// target alone; gate 2 compares its house with the controller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureVictimFacts {
    owner: InternedId,
    capturable: bool,
}

impl CaptureVictimFacts {
    /// An ordinary capturable target of house zero, for fixtures that do not
    /// model mind control.
    #[cfg(test)]
    pub(crate) fn capturable_for_test() -> Self {
        Self {
            owner: InternedId::from_index(u32::MAX),
            capturable: true,
        }
    }

    /// `frame` evaluates gate 7 (vt+0x160, an active Iron Curtain or Force
    /// Shield); the weapon ladder has no frame and leaves that gate to fire
    /// admission.
    pub(crate) fn of(target: &GameEntity, target_obj: &ObjectType, frame: Option<u32>) -> Self {
        // 3: `ImmuneToPsionics=` (`+0xD35`); 4: a Unit in a tank bunker
        // (`+0x2E4`); 5: already controlled; 6: a trigger transfer (`+0x2CC`,
        // no VERA producer); 7: the Iron Curtain; 9: Selling or Construction.
        let refused = target_obj.immune_to_psionics
            || (target.category == EntityCategory::Unit
                && target.bunker_link.installed_in().is_some())
            || target.mind_control.is_mind_controlled()
            || frame.is_some_and(|frame| {
                crate::sim::superweapon::invulnerability::is_invulnerable(
                    target.invulnerability.as_ref(),
                    frame,
                )
            })
            || constructing_or_selling(target);
        Self {
            owner: target.owner(),
            capturable: !refused,
        }
    }
}

/// CanCapture's gate 9 (`0x00471D1E..0x00471D2C`) and the reset's skip: the
/// current mission is Construction (0x12) or Selling (0x13). VERA keeps a
/// building's build-up and its build-down (the Construction Yard repack) in
/// `building_up`/`building_down` without publishing those missions, as
/// `building_operational_state` also reads.
fn constructing_or_selling(target: &GameEntity) -> bool {
    target.building_up.is_some()
        || target.building_down.is_some()
        || matches!(
            target.mission.current().known(),
            Some(MissionType::Selling | MissionType::Construction)
        )
}

/// CanCapture's manager side: the controller's house and whether its
/// capacity admits another victim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureControllerFacts {
    owner: InternedId,
    admits_another: bool,
}

impl CaptureControllerFacts {
    /// `None` without a manager. GetFireError dereferences the manager
    /// unconditionally for a MindControl warhead (`0x006FCB3B`); a type whose
    /// weapon 0 is not MindControl but whose other weapon is would crash
    /// there, which no stock type authors.
    pub(crate) fn of(controller: &GameEntity) -> Option<Self> {
        controller.capture_manager.as_ref().map(|manager| Self {
            owner: controller.owner(),
            admits_another: manager.admits_another(),
        })
    }
}

/// `CaptureManagerClass::CanCapture @ 0x00471C90` (gate 1, a null target, is
/// the caller's). Allies are not refused: only the controller's own house is.
pub(crate) fn can_capture(controller: CaptureControllerFacts, victim: CaptureVictimFacts) -> bool {
    victim.owner != controller.owner && victim.capturable && controller.admits_another
}

/// `TechnoClass::Init_Managers @ 0x006F3F40` (`0x006F4090..0x006F40F2`): a
/// manager for every class (buildings too) whose `GetWeapon(0)` warhead is
/// MindControl. It runs from the constructors and InitFromType
/// (`0x007355F1`, `0x00517CC4`, `0x00442C43`, ...), before a map reader sets
/// veterancy (`0x0051FB00` -> `0x007500E0`), so the weapon is always the
/// rookie one: an elite Psychic Commando keeps one link.
pub(crate) fn init_capture_manager(
    object_type: &ObjectType,
    rules: &RuleSet,
) -> Option<CaptureManagerState> {
    let (weapon_name, _) = crate::sim::combat::combat_weapon::weapon_for_index(object_type, 0, 0)?;
    let weapon = rules.weapon(weapon_name)?;
    let warhead = rules.warhead(weapon.warhead.as_deref()?)?;
    warhead
        .mind_control
        .then(|| CaptureManagerState::constructed(weapon.damage, weapon.infinite_mind_control))
}

impl Simulation {
    /// CanCapture for the live pair.
    pub(crate) fn can_capture(&self, controller_id: u64, target_id: u64, rules: &RuleSet) -> bool {
        let Some(controller) = self
            .substrate
            .entities
            .get(controller_id)
            .and_then(CaptureControllerFacts::of)
        else {
            return false;
        };
        let Some(target) = self.substrate.entities.get(target_id) else {
            return false;
        };
        let Some(target_obj) = self.object_type(target.type_ref(), rules) else {
            return false;
        };
        can_capture(
            controller,
            CaptureVictimFacts::of(target, target_obj, Some(self.session.binary_frame)),
        )
    }

    /// `CaptureManagerClass::CaptureUnit @ 0x00471D40`.
    pub(crate) fn capture_unit(
        &mut self,
        controller_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> bool {
        if !self.can_capture(controller_id, target_id, rules) {
            return false;
        }
        // 0x00471D8A: a single-victim manager releases its victim first.
        let released: Vec<u64> = self
            .substrate
            .entities
            .get(controller_id)
            .and_then(|controller| controller.capture_manager.as_ref())
            .filter(|manager| manager.max_control == 1)
            .map(|manager| manager.victims().rev().collect())
            .unwrap_or_default();
        for victim in released {
            self.free_unit(controller_id, victim, rules);
        }
        let (Some(controller_owner), Some(original_owner)) = (
            self.substrate
                .entities
                .get(controller_id)
                .map(GameEntity::owner),
            self.substrate
                .entities
                .get(target_id)
                .map(GameEntity::owner),
        ) else {
            return false;
        };
        // 0x00471DB8: ChangeOwner(controller's house, 1). It refuses only an
        // unchanged house, which CanCapture already excluded.
        self.change_owner_with_rules(target_id, controller_owner, rules);
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(controller_id)
            .and_then(|controller| controller.capture_manager.as_mut())
        {
            manager.nodes.push(ControlNode {
                victim: target_id,
                original_owner,
            });
        }
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            target.mind_control.controller = Some(controller_id);
        }
        self.reset_captured_orders(target_id, rules);
        self.decide_unit_fate(controller_id, target_id, rules);
        self.attach_capture_ring(target_id, rules);
        true
    }

    /// The MindControl arm of `BulletClass::DetonateAtCoord` (`0x0046920B..
    /// 0x0046933E`): no firer or no manager ends it; otherwise the firer's
    /// manager captures the bullet's own target (a cell or none fails), and a
    /// success plays `YuriMindControlSound=` at the target when the firer's
    /// house or the target's house before the capture passes
    /// `HouseClass::IsHumanPlayer @ 0x0050B6F0` (the local player in skirmish;
    /// the app resolves it from the two houses on the event).
    /// RESIDUAL: the two object-tag events raised before the attempt (6 and
    /// 0x2C, `0x0046929D`/`0x004692B8`) need object Tag attachment, which VERA
    /// does not have.
    pub(crate) fn mind_control_detonation(
        &mut self,
        firer_id: u64,
        target: Option<u64>,
        rules: &RuleSet,
    ) {
        let Some(firer) = self.substrate.entities.get(firer_id) else {
            return;
        };
        if firer.capture_manager.is_none() {
            return;
        }
        let firer_owner = firer.owner();
        let Some(target_id) = target else {
            return;
        };
        let Some(target_owner) = self
            .substrate
            .entities
            .get(target_id)
            .map(GameEntity::owner)
        else {
            return;
        };
        if !self.capture_unit(firer_id, target_id, rules) {
            return;
        }
        let Some(sound) = rules.mind_control.mind_control_sound.clone() else {
            return;
        };
        if let Some(target) = self.substrate.entities.get(target_id) {
            let event = SimSoundEvent::voc_at_for(
                sound,
                Some([firer_owner, target_owner]),
                &target.position,
            );
            self.sound_events.push(event);
        }
    }

    /// `0x00471E3A..0x00471E73` then `vt+0x3D0` (`0x0070F850`, no class
    /// override): unless a Simple Deployer is deploying (Unload) or the
    /// object is Selling or under Construction, the captive drops its
    /// destination (`vt+0x480(0, 1)` at `0x0070F859`), target and archive and
    /// is assigned Guard.
    fn reset_captured_orders(&mut self, target_id: u64, rules: &RuleSet) {
        let Some(target) = self.substrate.entities.get(target_id) else {
            return;
        };
        let simple_deployer_unloading = target.category == EntityCategory::Unit
            && target.mission.current().known() == Some(MissionType::Unload)
            && self
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| object.is_simple_deployer);
        if simple_deployer_unloading || constructing_or_selling(target) {
            return;
        }
        let now = self.session.binary_frame;
        self.assign_null_destination(target_id, Some(rules));
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            target.movement_target = None;
            crate::sim::mission::concrete_effects::represented_assign_target(target, None);
            target.set_archive_target(None);
        }
        let _ =
            self.mission_assign_exact(target_id, MissionId::from_known(MissionType::Guard), now);
    }

    /// The ring (`0x00471EB8..0x00471F6D`): `ControlledAnimationType=` at the
    /// victim's GetCoords (`vt+0x48`, the foundation centre for a building)
    /// raised by its type's `MindControlRingOffset=`, or for a building by
    /// its `Height=` in levels (`BuildingType+0xEF4 * [0x0089E178]`, 104 once
    /// the file's static initializer `0x00471610` has run:
    /// `tools/spatial_oracle/capture_ring_height.py`), attached to the victim.
    fn attach_capture_ring(&mut self, target_id: u64, rules: &RuleSet) {
        let Some(anim_name) = rules.mind_control.controlled_anim.as_deref() else {
            return;
        };
        let Some(target) = self.substrate.entities.get(target_id) else {
            return;
        };
        let Some(object) = self.object_type(target.type_ref(), rules) else {
            return;
        };
        let building = target.category == EntityCategory::Structure;
        let offset = if building {
            // Without an art section the BuildingTypeClass constructor's 2
            // stands.
            let height = rules
                .art_registry
                .get(&object.image)
                .or_else(|| rules.art_registry.get(&object.id))
                .map_or(2, |art| art.height);
            height.wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
        } else {
            object.mind_control_ring_offset
        };
        let Some(centre) = self.anim_owner_coords(target_id) else {
            return;
        };
        let coord = AnimWorldCoord {
            x: centre.x,
            y: centre.y,
            z: centre.z.wrapping_add(offset),
        };
        let type_id = self.interner.intern(anim_name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: RING_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_id, rx, ry, sub_x, sub_y, z,
            )
        };
        let anim_id = match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim_id) => anim_id,
            Err(error) => {
                log::debug!("capture ring [{anim_name}] did not construct: {error}");
                return;
            }
        };
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            target.mind_control.ring_anim = Some(anim_id);
        }
        self.set_anim_owner_object(anim_id, Some(target_id), rules);
        if building {
            self.set_anim_z_adjust(anim_id, BUILDING_RING_Z_ADJUST);
        }
    }

    /// `CaptureManagerClass::FreeUnit @ 0x00471FF0`: every node of `victim`,
    /// newest first. Returns false for a missing controller or manager.
    pub(crate) fn free_unit(
        &mut self,
        controller_id: u64,
        victim_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(manager) = self
            .substrate
            .entities
            .get(controller_id)
            .and_then(|controller| controller.capture_manager.as_ref())
        else {
            return false;
        };
        let matches: Vec<(usize, InternedId)> = manager
            .nodes
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, node)| node.victim == victim_id)
            .map(|(index, node)| (index, node.original_owner))
            .collect();
        for (index, original_owner) in matches {
            // The ring goes (`vt+0xF8` UnInit), then MindClearedSound: the
            // type's own, else the global.
            if let Some(ring) = self
                .substrate
                .entities
                .get_mut(victim_id)
                .and_then(|victim| victim.mind_control.ring_anim.take())
            {
                self.destroy_anim(ring, rules);
            }
            if let Some(victim) = self.substrate.entities.get(victim_id) {
                let sound = self
                    .object_type(victim.type_ref(), rules)
                    .and_then(|object| object.mind_cleared_sound.clone())
                    .or_else(|| rules.mind_control.mind_cleared_sound.clone());
                if let Some(sound) = sound {
                    let event = SimSoundEvent::voc_at(sound, &victim.position);
                    self.sound_events.push(event);
                }
            }
            self.change_owner_with_rules(victim_id, original_owner, rules);
            // DecideUnitFate runs while `+0x2C0` still names the controller.
            self.decide_unit_fate(controller_id, victim_id, rules);
            if let Some(victim) = self.substrate.entities.get_mut(victim_id) {
                victim.mind_control.controller = None;
            }
            if let Some(manager) = self
                .substrate
                .entities
                .get_mut(controller_id)
                .and_then(|controller| controller.capture_manager.as_mut())
                && index < manager.nodes.len()
            {
                manager.nodes.remove(index);
            }
        }
        true
    }

    /// `CaptureManagerClass::FreeAll @ 0x00472140`: every victim, newest node
    /// first.
    pub(crate) fn free_all_captures(&mut self, controller_id: u64, rules: &RuleSet) {
        let victims: Vec<u64> = self
            .substrate
            .entities
            .get(controller_id)
            .and_then(|controller| controller.capture_manager.as_ref())
            .map(|manager| manager.victims().rev().collect())
            .unwrap_or_default();
        for victim in victims {
            self.free_unit(controller_id, victim, rules);
        }
    }

    /// `CaptureManagerClass::DecideUnitFate @ 0x004723B0`, called with the
    /// unit's house already changed (to the controller's on capture, back to
    /// the original on release). Team, Grinder and Bio Reactor arms are module
    /// residuals.
    fn decide_unit_fate(&mut self, controller_id: u64, unit_id: u64, rules: &RuleSet) {
        if !self.substrate.entities.contains(unit_id) {
            return;
        }
        // 0x004723E4..0x004723F3: a unit holding a Temporal target lets go.
        self.temporal_release_if_warping(unit_id);
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let Some(object) = self.object_type(unit.type_ref(), rules) else {
            return;
        };
        // 0x00472405: an OpenTopped unit's passengers drop their targets
        // (`0x00710550(unit, 0)`).
        if object.open_topped {
            let passengers: Vec<u64> = unit
                .passenger_role
                .cargo()
                .map(|cargo| cargo.passengers.clone())
                .unwrap_or_default();
            for passenger in passengers {
                if let Some(passenger) = self.substrate.entities.get_mut(passenger) {
                    crate::sim::mission::concrete_effects::represented_assign_target(
                        passenger, None,
                    );
                }
            }
        }
        let Some(unit) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let (unit_owner, health, strength) = (unit.owner(), unit.health, object.strength);
        // 0x0047242A: an object of a human house keeps its orders; no draw.
        if self
            .houses
            .get(&unit_owner)
            .is_none_or(|house| house.is_human)
        {
            return;
        }
        let Some(controller_owner) = self
            .substrate
            .entities
            .get(controller_id)
            .map(GameEntity::owner)
        else {
            return;
        };
        let reason = self.capture_reason(controller_owner, health, strength, rules);
        // 0x004724E3: `RandomRanged(1, 100)` on the Scenario stream.
        let roll = self.scenario_rng.next_range_i32_inclusive(1, 100);
        let Some(choice) = capture_decision(rules.mind_control.ai_capture_table(reason), roll)
        else {
            return;
        };
        // 1 "Add To Team", 2 "Put in Grinder", 3 "Put in Bio Reactor" fall
        // back to Hunt here (module residual); 5 "Do Nothing" keeps orders.
        if choice == 5 {
            return;
        }
        let now = self.session.binary_frame;
        let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
        let _ = self.mission_queue_exact(
            unit_id,
            MissionId::from_known(MissionType::Hunt),
            0,
            now,
            &readiness,
        );
    }

    /// DecideUnitFate's reason (`0x00472438..0x004724D5`), read from the
    /// CONTROLLER's house, on release too: money below
    /// `AICaptureLowMoneyMark=`, then a power ratio (`GetPowerRatio
    /// @ 0x004FCE30`) below one, then the unit's Health/Strength below
    /// `AICaptureWoundedMark=`.
    fn capture_reason(
        &self,
        controller_owner: InternedId,
        health: crate::sim::components::Health,
        strength: i32,
        rules: &RuleSet,
    ) -> CaptureReason {
        let mind_control = &rules.mind_control;
        if crate::sim::credit_income::available_money(self, controller_owner)
            < mind_control.ai_capture_low_money_mark
        {
            return CaptureReason::LowMoney;
        }
        // A house without a power state has produced 0 of 0 drained: ratio 1.
        if self
            .power_states
            .get(&controller_owner)
            .is_some_and(|power| !power.has_full_power())
        {
            return CaptureReason::LowPower;
        }
        // `FILD Health; FSTP float; FILD Strength; FDIVR float; FCOMP mark;
        // TEST AH,1` (`0x00472497..0x004724C3`): GetHealthRatio's PC53/chop
        // quotient (Health passes through a float store, exact up to 2^24),
        // and C0 is set for less OR unordered, so 0/0 is wounded and x/0
        // (+inf) is not.
        use crate::util::native_x87::MaskedX87Ordering;
        match health.compare_ratio(strength, f64::from(mind_control.ai_capture_wounded_mark)) {
            MaskedX87Ordering::Less | MaskedX87Ordering::Unordered => CaptureReason::Wounded,
            MaskedX87Ordering::Equal | MaskedX87Ordering::Greater => CaptureReason::Normal,
        }
    }
}

impl Simulation {
    /// `CaptureManagerClass::Update @ 0x00471A50`, the Mastermind overload,
    /// from `TechnoClass::AI_Update` (`0x006FA730`, after the drain block and
    /// before the IsAlive gate at `0x006FA735`). Only an infinite manager
    /// overloads. The captive count picks the `Overload*=` row; a damaging row
    /// hits the controller with `C4Warhead=`, plays the overload voice once
    /// per episode and throws five spark systems.
    pub(crate) fn capture_manager_update(
        &mut self,
        controller_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let table = &rules.mind_control;
        let Some(entity) = self.substrate.entities.get_mut(controller_id) else {
            return;
        };
        let Some(manager) = entity.capture_manager.as_mut() else {
            return;
        };
        if !manager.infinite {
            return;
        }
        if manager.overload_countdown > 0 {
            manager.overload_countdown -= 1;
            return;
        }
        // `0x00471A82..0x00471AB1` reads `OverloadCount[0]` unguarded; stock
        // authors the table.
        let Some(&first) = table.overload_count.first() else {
            return;
        };
        let count = manager.count();
        let mut index = 0;
        if count > first {
            while index + 1 < table.overload_count.len() {
                index += 1;
                if count <= table.overload_count[index] {
                    break;
                }
            }
        }
        manager.overload_countdown = table.overload_frames.get(index).copied().unwrap_or(0);
        let damage = table.overload_damage.get(index).copied().unwrap_or(0);
        if damage <= 0 {
            // `0x00471C83`: leaving overload re-arms the voice.
            manager.overload_sound_played = false;
            return;
        }
        let sound_played = manager.overload_sound_played;
        let position = entity.position.clone();
        let location = crate::sim::movement::ground_pose::position_world_coord(&position);
        // 0x00471B04: `ReceiveDamage(&damage, 0, C4Warhead, NULL, 0, 0, NULL)`.
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            controller_id,
            damage,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        self.commit_direct_damage_receiver(rules, overlay_registry, event);
        if !sound_played {
            if let Some(sound) = rules.mind_control.overload_sound.clone() {
                self.sound_events
                    .push(SimSoundEvent::voc_at(sound, &position));
            }
            if let Some(manager) = self
                .substrate
                .entities
                .get_mut(controller_id)
                .and_then(|controller| controller.capture_manager.as_mut())
            {
                manager.overload_sound_played = true;
            }
        }
        // 0x00471B48..0x00471C23: five `DefaultSparkSystem=` systems, each at
        // the controller's Location offset by (second draw, first draw, +100).
        let spark_type = rules
            .combat_damage
            .default_spark_system
            .as_deref()
            .and_then(|name| rules.ps_type_id_by_name(name));
        for _ in 0..5 {
            let dy = self.scenario_rng.next_range_i32_inclusive(-200, 200);
            let dx = self.scenario_rng.next_range_i32_inclusive(-200, 200);
            if let Some(system_type) = spark_type {
                self.spawn_particle_system(
                    system_type,
                    glam::IVec3::new(
                        location.x.wrapping_add(dx),
                        location.y.wrapping_add(dy),
                        location.z.wrapping_add(100),
                    ),
                    None,
                    None,
                    glam::IVec3::ZERO,
                    None,
                    rules,
                );
            }
        }
        // 0x00471C29..0x00471C62: above the first row and still alive, the
        // sideways lean's sign (module residual) takes one draw.
        let alive = self
            .substrate
            .entities
            .get(controller_id)
            .is_some_and(|controller| controller.lifecycle.object_alive);
        if index > 0 && alive {
            let _lean_left = self.scenario_rng.next_range_i32_inclusive(0, 100) < 50;
        }
    }
}

/// The choice walk of DecideUnitFate (`0x004724FD..0x00472537`): the running
/// sum of the table's weights until it reaches the roll. `None` when the
/// table runs out first (or at choice 6), which returns without an order.
fn capture_decision(table: &[i32], roll: i32) -> Option<u8> {
    let mut choice = 0_u8;
    let mut sum = 0_i32;
    for &weight in table {
        if choice == 6 {
            return None;
        }
        choice += 1;
        sum = sum.wrapping_add(weight);
        if roll <= sum {
            return Some(choice);
        }
    }
    None
}

#[cfg(test)]
#[path = "capture_manager_tests.rs"]
mod tests;
