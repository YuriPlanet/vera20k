//! The `action` host import (RFC 0001 section 6b, appendix section 5):
//! stock actions 0..149 and atomic actions from 1000.
//!
//! [`run_action`] is the one entry point, for direct `action` calls and for
//! the atomic and stock actions of module triggers. Each action calls the
//! state owner the game already has; none writes another owner's state.
//!
//! # Atomic actions (5.3, numbered from 1000 in table order)
//!
//! | ID | Name | Owner called |
//! | --- | --- | --- |
//! | 1000 | `FireWeapon` | `Simulation::commit_fire_visit` with `FireVisit::Direct`, the FireAt entry a mission calls once GetFireError (`0x006FC0B0`, range checked) answered Ok, as the deploy fire does; any other answer is `Refused`. Slot `value` 0 or 1, or the slot holding weapon `id`; aim at `cell`, else `other`, else the current target |
//! | 1001 | `LaunchSuperWeapon` | `Simulation::fire_super_weapon` (HouseClass::Fire_SW `0x004FAE50`), manual target |
//! | 1002 | `SpawnAnim` | `Simulation::trigger_play_anim_at_cell`, stock action 41's owner (PlayAnimAtWP `0x006E2290`) at a cell; `id` is the AnimType array index stock 41 takes, `house` optional |
//! | 1003 | `AttachAnim` | `Simulation::spawn_anim_at_world` with the object as owner (SetOwnerObject `0x00424B50`), as team script action 51 (`0x006EF610`) attaches one: delay 0, one pass of the type's loops, draw flags `0x600`, at the object's GetCoords; `id` the AnimType array index. `value2` other than 0 holds its loops (`hold_loops`, as a Phobos AttachEffect's animation) for that many frames, or until `DetachAnim` when negative (`held::ActionHeld`) |
//! | 1004 | `Remove` | `Simulation::uninit_with_context` |
//! | 1005 | `Kill` | `Simulation::commit_direct_damage_receiver`, damage = health, `C4Warhead=`, defenses ignored (as stock action 32, `0x006E2050`) |
//! | 1006 | `Damage` | `Simulation::commit_direct_damage_receiver` with the warhead, armor applied |
//! | 1007 | `Heal` | the `Health` write `set` uses (no credit, clamped to Strength) at health + `value` |
//! | 1008 | `CreateObject` | `construct_object_limbo_at_height`, `reveal_constructed_object_at_coord_with_overlay_context` (Unlimbo), `grand_opening` for buildings, `queue_entity_mission_deferred` |
//! | 1009 | `SetMission` | the `Mission` write `set` uses: the MEGAMISSION event a player's order sends, through the command receiver (`EventClass::Execute` `0x004C71F6`); `cell`, else `other`, is the mission's target or destination |
//! | 1010 | `Teleport` | `Simulation::foot_chrono_warp_to` (Foot4DF7F0 / Infantry522FE0) at the coordinate House50D6D0 computes (`chrono_warp_coord`); `Nearby` leaves a blocked landing to the Teleport's own nearby search, `Fail` asks Can_Enter_Cell first; ground Units and Infantry only; ends the object's `Push` |
//! | 1011 | `ClearTarget` | `Simulation::assign_target_represented(None)` (Assign_Target `0x006FCDB0`) |
//! | 1012 | `ClearDisguise` | `DisguiseRuntime::clear`, the class ClearDisguise (Unit `0x00746720`) |
//! | 1013 | `ClearMindControl` | `Simulation::free_unit` on the controller (CaptureManagerClass::FreeUnit `0x00471FF0`) |
//! | 1014 | `AddExperience` | `Veterancy::award_kill` (VeterancyClass::Add `0x0074FF50`) as for a kill worth `value`, at the object's own Cost (no house cost factor), only for a `Trainable=` type |
//! | 1015 | `AddMoney` | `credit_income::add_credits` |
//! | 1016 | `AllowType` | the house's forbidden types (`held::ActionHeld`), which CanBuild (`production::can_build`, `0x004F7870`) refuses like a room ban; 1 lifts the module's forbid |
//! | 1017 | `SetOwner` | `Simulation::change_owner_with_rules` |
//! | 1018 | `AddAmmo` | the `Ammo` write `set` uses at ammo + `value` |
//! | 1019 | `AddPassenger` | `passenger::can_enter_transport` (the CAN_LOAD answer and alliance), then `passenger::board_passenger`, the PerCellProcess transport arm's boarding (Infantry `0x0051A3FC`, Unit `0x0073A6E8`), with the passenger anywhere on the map; a Unit or Aircraft transport only |
//! | 1021 | `SetProperty` | `SimHost::set`, the one property writer |
//! | 1022 | `Modify` | `SimHost::modify`, the modifier store's one entry |
//! | 1023 | `RemoveModifiers` | `ModifierStore::remove_group` for the calling module's origin |
//! | 1024 | `SetVar` | `Simulation::write_module_var`, the writer `var_set` uses: global scope ignores the target, house scope takes the `House` target (0 the trigger house), object scope the target object |
//! | 1025 | `DetonateWeapon` | `Simulation::trigger_explosion_at_cell`, stock action 42's owner (DoExplosionAt `0x006E2390`) at a cell, with the optional source for credit |
//! | 1026 | `DetonateWarhead` | `world_receiver::apply_area_damage` (Apply_area_damage `0x00489280`) at the cell center |
//! | 1030 | `EndFor` | `Simulation::flag_house_to_win` / `flag_house_to_lose` (`0x004FC9E0` / `0x004FCBD0`) |
//! | 1031 | `SetScore` | `Economy::set_score` (`+0x54E8`) |
//! | 1032 | `AddScore` | `Economy::add_score`, the kill and capture awards' writer |
//! | 1033 | `Log` | the module log |
//! | 1042 | `RaiseEvent` | `Simulation::raise_module_event` for an object event, `offer_module_event` to each global trigger for a global one (`ini_names`) |
//! | 1034 | `Convert` | `Simulation::convert_techno_type` (Ares ConvertToType `0x10043650`); a positive `value2` converts back through it after that many frames (`held::ActionHeld`); ends the object's `Push` |
//! | 1035 | `Push` | `Simulation::start_module_push`, the forced-motion owner `motion` keeps: `value` leptons toward (negative: away from) `cell` over `value2` frames, each frame a module step at the object's locomotor Process; Units and Infantry on Drive, Ship, Walk, Mech or Hover, not warping |
//! | 1046 | `DetachAnim` | `Simulation::destroy_anim_with_context` on the animations this origin's `AttachAnim` keeps on the object (`held::ActionHeld`), of AnimType `id` or every one for -1 |
//! | 1045 | `SetFacing` | `Simulation::module_set_facing`: the active locomotor's Do_Turn (ILocomotion `+0x4C`, `movement::locomotor_do_turn`) toward body facing `value` (0..255) |
//!
//! Every other atomic action answers `(0, Unsupported)` and `has_kind`
//! reports 0:
//! - `RemovePassenger`: unloading is the transport's Unload mission
//!   (`UnitClass::Mission_Unload @ 0x0073D630`, `transport_unload`): it
//!   turns, searches an exit cell and ejects the cargo head one per 14..16
//!   frames. No native operation takes a named passenger out; `SetMission`
//!   with `Unload` orders the mission.
//! - `CreateProjectile`, `DetonateProjectile`, `RemoveProjectile`: not
//!   wired; projectile references are read (`ext_modules::projectiles`) and
//!   a `Fire` decision's plan launches bullets, but no action does yet.
//! - The team actions (`CreateTeamUnlimited`, `DisbandTeam`,
//!   `AddTeamMember`, `RemoveTeamMember`): teams are numbered by the team
//!   VM's own counter, not the shared creation counter, so a team has no
//!   reference kind yet (appendix "References" and 7.1 `Team` first).
//! - `Module`, which only a module trigger runs (it queues an `on_events`
//!   record; `ext_modules::triggers`).
//!
//! Also unsupported: `FireWeapon` with a weapon `id` neither of the
//! shooter's two slots holds at its rank (FireAt fires its own slots), or a
//! slot other than 0 and 1;
//! `AddPassenger` into a
//! building (garrisons and absorbers enter through their own arms); and
//! `ClearMindControl` of the Psychic Dominator's permanent control (no
//! controller to free it; `Refused`). `FireWeapon` and `AddPassenger` run
//! their owners at once, wherever the action runs (module phase or an event
//! site), not in the shooter's or passenger's own turn.
//!
//! # Stock actions (5.2)
//!
//! Stock actions run through the map trigger executor
//! (`Simulation::run_untriggered_stock_action`, TActionClass::Execute
//! `0x006DD8B0`) with no live trigger; the operands are mapped to what
//! `TActionClass::Read @ 0x006DD5B0` would have stored:
//!
//! | Kinds | Mapping |
//! | --- | --- |
//! | 0 | no operation |
//! | 1, 2 | `text` country name → `+0x90` country index (`NotFound` for an unknown country) |
//! | 11, 103 | `text` → inline CSF label (parameter type 4) |
//! | 20 | `text` → theme name (parameter type 8) |
//! | 12, 22, 53, 54 / 70 | `text` trigger / tag ID → definition reference (`NotFound` if absent) |
//! | 4, 5, 7, 104, 105 | `id` TeamType (map definition ordinal, `lookup(TeamType)`) → team reference |
//! | 80, 107 | as 4, plus the waypoint |
//! | 8, 89, 90, 94, 95, 96, 109, 112, 116 | waypoint → `+0x44` |
//! | 17, 18, 59, 63, 64..66, 101 | waypoint → `+0x90` |
//! | 41, 42, 43, 88 | waypoint, `id` (animation, weapon, voxel animation, particle system array index) → `+0x90`; 41 also takes `house` |
//! | 48, 55, 58, 108 | waypoint, `value` → `+0x90` |
//! | 102, 125 | waypoint and `house` (required); 125 takes `id` TechnoType as its inline type |
//! | 10, 23..27, 31, 44..47, 67..69, 71..73, 84..87, 97, 100, 110, 114, 117, 118, 127, 142..145 | `value` → `+0x90` |
//! | 28, 29 / 56, 57 | `value` 0..49 / 0..99 (out of range is invalid) |
//! | 106, 115 | `id` TechnoType → inline type name, `value` → `+0x90` |
//! | 30, 35, 51, 76..79, 82, 83, 139 | `House` target → the trigger house, `value` → `+0x90` |
//! | 33, 34, 133, 134 | `House` target, `id` superweapon (`[SuperWeaponTypes]` order) → `+0x90` |
//! | 129, 132 | as 33, `value` → `+0x48` (parameter type 11) |
//! | 128, 135, 137, 140 / 136, 138, 141 | `House` target, waypoint in `cell` / none |
//! | 131 | `House` target, `id` TechnoType inline name, `value` duration |
//! | 92, 93 / 148 / 149 | Ares: `House` target / `value` / object target as the Spring actor |
//!
//! A waypoint comes from the target or, for a `House` target, `cell`, and
//! must be a waypoint reference: the executor addresses waypoints, so a cell
//! or object reference answers `Unsupported`.
//!
//! These stock actions call the executor's owners directly for an object
//! target, since the executor walks the live trigger's tagged objects:
//! 14 (`SetOwner`), 32 (`Kill`), 60 (`production::sell_back`), 61 and 62
//! (`go_building_offline` / `go_building_online`). 21 plays the EVA name
//! through `emit_trigger_sound`, the network form of the executor's case 21
//! (the simulation holds no EVA catalog to resolve an index).
//!
//! Unsupported stock actions:
//! - a live trigger selects the house (`6E45E0` refuses a null trigger) or
//!   the objects (its tag): 3, 6, 9, 13, 36..38, 52, 74, 75, 81, 91, 111,
//!   113, 119..124, 126, 130 and Ares 146, 147;
//! - sound IDs (no `Sound` name kind yet): 19, 98, 99;
//! - 40 (`SetVisibleMapArea`, its four raw dwords are not mapped yet);
//! - 15 (a trigger flag), 39, 49, 50 (unavailable);
//! - Ares actions without Ares.

use vera20k_wasm_ext::{ActionParams, Invalid, SimHost, Status, VarScope, property_name};

use super::modifiers::Stat;
use super::{Host, ModuleSource};

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{RAD_NO_ATTACKER, TargetKind};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::trigger_runtime::{StockActionCall, StockReference, StockReferenceKind};
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation, UninitContext};

/// Atomic action IDs (appendix 5.3 order from 1000; open question 5).
pub const FIRE_WEAPON: i32 = 1000;
pub const LAUNCH_SUPER_WEAPON: i32 = 1001;
pub const SPAWN_ANIM: i32 = 1002;
pub const ATTACH_ANIM: i32 = 1003;
pub const REMOVE: i32 = 1004;
pub const KILL: i32 = 1005;
pub const DAMAGE: i32 = 1006;
pub const HEAL: i32 = 1007;
pub const CREATE_OBJECT: i32 = 1008;
pub const SET_MISSION: i32 = 1009;
pub const TELEPORT: i32 = 1010;
pub const CLEAR_TARGET: i32 = 1011;
pub const CLEAR_DISGUISE: i32 = 1012;
pub const CLEAR_MIND_CONTROL: i32 = 1013;
pub const ADD_EXPERIENCE: i32 = 1014;
pub const ADD_MONEY: i32 = 1015;
pub const ALLOW_TYPE: i32 = 1016;
pub const SET_OWNER: i32 = 1017;
pub const ADD_AMMO: i32 = 1018;
pub const ADD_PASSENGER: i32 = 1019;
pub const SET_PROPERTY: i32 = 1021;
pub const MODIFY: i32 = 1022;
pub const REMOVE_MODIFIERS: i32 = 1023;
pub const SET_VAR: i32 = vera20k_wasm_ext::triggers::ACTION_SET_VAR;
pub const DETONATE_WEAPON: i32 = 1025;
pub const DETONATE_WARHEAD: i32 = 1026;
pub const END_FOR: i32 = 1030;
pub const SET_SCORE: i32 = 1031;
pub const ADD_SCORE: i32 = 1032;
pub const LOG: i32 = 1033;
pub const CONVERT: i32 = 1034;
pub const PUSH: i32 = 1035;
pub const SET_FACING: i32 = 1045;
pub const DETACH_ANIM: i32 = 1046;
pub const RAISE_EVENT: i32 = vera20k_wasm_ext::triggers::ACTION_RAISE_EVENT;

const ATOMIC: [i32; 35] = [
    FIRE_WEAPON,
    LAUNCH_SUPER_WEAPON,
    SPAWN_ANIM,
    ATTACH_ANIM,
    REMOVE,
    KILL,
    DAMAGE,
    HEAL,
    CREATE_OBJECT,
    SET_MISSION,
    TELEPORT,
    CLEAR_TARGET,
    CLEAR_DISGUISE,
    CLEAR_MIND_CONTROL,
    ADD_EXPERIENCE,
    ADD_MONEY,
    ALLOW_TYPE,
    SET_OWNER,
    ADD_AMMO,
    ADD_PASSENGER,
    SET_PROPERTY,
    MODIFY,
    REMOVE_MODIFIERS,
    SET_VAR,
    DETONATE_WEAPON,
    DETONATE_WARHEAD,
    END_FOR,
    SET_SCORE,
    ADD_SCORE,
    LOG,
    CONVERT,
    PUSH,
    SET_FACING,
    DETACH_ANIM,
    RAISE_EVENT,
];

/// `Mission` "none" for `CreateObject`: keep the constructor's mission.
const MISSION_DEFAULT: i32 = 255;

/// What an action runs with besides its parameters.
pub struct ActionContext<'a, 'f> {
    pub rules: &'a RuleSet,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub frame_effects: &'a mut FrameEffects<'f>,
    /// `ActionParams.text`, already read.
    pub text: &'a str,
    /// The firing trigger's house inside a trigger, where a house of 0
    /// selects it. `None` for a direct call, where a house of 0 is invalid.
    pub trigger_house: Option<InternedId>,
    /// The calling module (or the trigger's): the origin of the modifiers
    /// `Modify` adds and `RemoveModifiers` removes.
    pub source: ModuleSource,
}

/// Runs action `kind` against `target` (a reference; a trigger resolves its
/// `ActionTarget` first and puts the event's other object in
/// `params.other`). Returns the created object's handle or 0, and the
/// status; `Err` means invalid arguments, which disable the module.
pub fn run_action(
    sim: &mut Simulation,
    kind: i32,
    target: i64,
    params: &ActionParams,
    ctx: ActionContext<'_, '_>,
) -> Result<(i64, Status), Invalid> {
    let mut run = Run { sim, params, ctx };
    let result = if kind >= vera20k_wasm_ext::FIRST_ATOMIC_ACTION {
        run.atomic(kind, target)
    } else {
        run.stock(kind, target)
    };
    match result {
        Ok(handle) => Ok((handle, Status::Ok)),
        Err(Stop::Status(status)) => Ok((0, status)),
        Err(Stop::Invalid(invalid)) => Err(invalid),
    }
}

/// Whether action `kind` runs in this game (`has_kind(ActionKind, id)`).
pub(crate) fn has_action(kind: i32, rules: &RuleSet) -> bool {
    if kind >= vera20k_wasm_ext::FIRST_ATOMIC_ACTION {
        return ATOMIC.contains(&kind);
    }
    match stock_operands(kind) {
        Some(Operands::Ares(_)) => rules.ares_form(),
        Some(_) => true,
        None => false,
    }
}

/// Why an action stopped early.
enum Stop {
    Status(Status),
    Invalid(Invalid),
}

impl From<Invalid> for Stop {
    fn from(invalid: Invalid) -> Self {
        Self::Invalid(invalid)
    }
}

type Act<T> = Result<T, Stop>;

fn invalid<T>(why: String) -> Act<T> {
    Err(Stop::Invalid(Invalid(why)))
}

fn status<T>(status: Status) -> Act<T> {
    Err(Stop::Status(status))
}

/// An owner's answer as the action's: 0 for `Ok`, else the status.
fn answer(answer: Status) -> Act<i64> {
    match answer {
        Status::Ok => Ok(0),
        other => status(other),
    }
}

/// A decoded target or parameter reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    None,
    Object(u64),
    House(usize),
    Cell(u16, u16),
    Waypoint(i32),
    /// Type, superweapon and global rules references: never an action target.
    Other,
}

fn decode(reference: i64) -> Act<Target> {
    Ok(match super::decode_ref(reference)? {
        None => Target::None,
        Some(super::Ref::Object(id)) => Target::Object(id),
        Some(super::Ref::House(index)) => Target::House(index),
        Some(super::Ref::Cell { x, y }) => Target::Cell(x, y),
        Some(super::Ref::Waypoint(number)) => Target::Waypoint(number),
        Some(super::Ref::Type { .. } | super::Ref::SuperWeapon { .. } | super::Ref::Global) => {
            Target::Other
        }
    })
}

/// The stock operand shape of a kind (module doc table); `None` for an
/// unsupported kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operands {
    Nothing,
    Value,
    Variable { count: i64 },
    Country,
    Label,
    Theme,
    Speech,
    Trigger,
    Tag,
    Team { waypoint: bool },
    Waypoint,
    WaypointValue,
    WaypointId { house: bool },
    WaypointOperand,
    WaypointHouse { type_name: bool },
    TypeValue,
    House,
    HouseSuper { auxiliary: bool },
    HouseWaypoint,
    HouseType,
    Object,
    Ares(AresOperands),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AresOperands {
    House,
    Value,
    Object,
}

fn stock_operands(kind: i32) -> Option<Operands> {
    use Operands::*;
    Some(match kind {
        0 => Nothing,
        1 | 2 => Country,
        4 | 5 | 7 | 104 | 105 => Team { waypoint: false },
        80 | 107 => Team { waypoint: true },
        8 | 89 | 90 | 94 | 95 | 96 | 109 | 112 | 116 => Waypoint,
        10
        | 16
        | 23..=27
        | 31
        | 44..=47
        | 67..=69
        | 71..=73
        | 84..=87
        | 97
        | 100
        | 110
        | 114
        | 117
        | 118
        | 127
        | 142..=145 => Value,
        11 | 103 => Label,
        12 | 22 | 53 | 54 => Trigger,
        14 | 32 | 60..=62 => Object,
        17 | 18 | 59 | 63..=66 | 101 => WaypointValue,
        20 => Theme,
        21 => Speech,
        28 | 29 => Variable { count: 50 },
        56 | 57 => Variable { count: 100 },
        30 | 35 | 51 | 76..=79 | 82 | 83 | 139 => House,
        33 | 34 | 133 | 134 => HouseSuper { auxiliary: false },
        129 | 132 => HouseSuper { auxiliary: true },
        41 => WaypointId { house: true },
        42 | 43 | 88 => WaypointId { house: false },
        48 | 55 | 58 | 108 => WaypointOperand,
        70 => Tag,
        102 => WaypointHouse { type_name: false },
        125 => WaypointHouse { type_name: true },
        106 | 115 => TypeValue,
        128 | 135 | 136..=138 | 140 | 141 => HouseWaypoint,
        131 => HouseType,
        92 | 93 => Ares(AresOperands::House),
        148 => Ares(AresOperands::Value),
        149 => Ares(AresOperands::Object),
        _ => return None,
    })
}

/// A TechnoType ID's type (the rules registry ordinal).
fn techno_type(rules: &RuleSet, id: i32) -> Act<&crate::rules::object_type::ObjectType> {
    match usize::try_from(id)
        .ok()
        .and_then(|id| rules.all_objects().nth(id))
    {
        Some(object) => Ok(object),
        None => invalid(format!("no TechnoType {id}")),
    }
}

struct Run<'s, 'p, 'a, 'f> {
    sim: &'s mut Simulation,
    params: &'p ActionParams,
    ctx: ActionContext<'a, 'f>,
}

impl Run<'_, '_, '_, '_> {
    fn rules(&self) -> &RuleSet {
        self.ctx.rules
    }

    /// An object target that exists; 0 or a removed object is `Gone`.
    /// `RaiseEvent` (RFC 0001 section 6h): a registered
    /// module event, on the target object (object event) or for every
    /// global trigger with it (global event).
    fn raise_event(&mut self, target: i64) -> Act<i64> {
        use super::triggers::ModuleEvent;
        use vera20k_wasm_ext::triggers::EVENT_MODULE;
        let id = self.params.id;
        let Some(global) = self.sim.ext_modules.ini.event_is_global(id) else {
            return invalid(format!("no module event {id}"));
        };
        let subject = if global { 0 } else { self.object(target)? };
        let other = self.source()?.unwrap_or(0);
        let house = match decode(self.params.house)? {
            Target::None => None,
            Target::House(index) if index < self.sim.houses.order().len() => Some(index as u32),
            other => return invalid(format!("{other:?} is not a house")),
        };
        if !self.sim.trigger_runtime.module_listens(EVENT_MODULE) {
            return Ok(0);
        }
        let event = ModuleEvent {
            other,
            value: self.params.value,
            value2: self.params.value2,
            id,
            house,
            ..ModuleEvent::new(EVENT_MODULE, subject)
        };
        let rules = self.ctx.rules;
        if global {
            for handle in self
                .sim
                .trigger_runtime
                .module_triggers()
                .global(EVENT_MODULE)
            {
                self.sim
                    .offer_module_event(rules, self.ctx.frame_effects, handle, 0, &event);
            }
        } else {
            self.sim
                .raise_module_event(rules, self.ctx.frame_effects, event);
        }
        Ok(0)
    }

    fn object(&self, reference: i64) -> Act<u64> {
        match decode(reference)? {
            Target::None => status(Status::Gone),
            Target::Object(id) if self.sim.entities().get(id).is_some() => Ok(id),
            Target::Object(_) => status(Status::Gone),
            other => invalid(format!("{other:?} is not an object target")),
        }
    }

    /// An optional second object (a damage source); a removed one counts
    /// as none.
    fn source(&self) -> Act<Option<u64>> {
        match decode(self.params.other)? {
            Target::None => Ok(None),
            Target::Object(id) => Ok(self.sim.entities().get(id).map(|_| id)),
            other => invalid(format!("{other:?} is not a source object")),
        }
    }

    /// A house reference; 0 selects the trigger house inside a trigger and
    /// is invalid in a direct call.
    fn house(&self, reference: i64) -> Act<InternedId> {
        match decode(reference)? {
            Target::None => match self.ctx.trigger_house {
                Some(house) => Ok(house),
                None => invalid("house 0 in a direct action call".into()),
            },
            Target::House(index) => match self.sim.houses.order().get(index) {
                Some(&house) => Ok(house),
                None => status(Status::Gone),
            },
            other => invalid(format!("{other:?} is not a house")),
        }
    }

    /// A cell: a cell reference, a waypoint's cell or an object's cell.
    fn cell(&self, reference: i64) -> Act<(u16, u16)> {
        match decode(reference)? {
            Target::Cell(x, y) => Ok((x, y)),
            Target::Waypoint(number) => match self.sim.session.waypoint_cell(number) {
                Some((x, y)) => Ok((x as u16, y as u16)),
                None => status(Status::NotFound),
            },
            Target::Object(id) => match self.sim.entities().get(id) {
                Some(entity) => Ok((entity.position.rx(), entity.position.ry())),
                None => status(Status::Gone),
            },
            Target::None => invalid("an action needs a cell".into()),
            other => invalid(format!("{other:?} is not a cell")),
        }
    }

    /// A stock waypoint operand: a waypoint reference that names a waypoint.
    fn waypoint(&self, reference: i64) -> Act<i32> {
        match decode(reference)? {
            Target::Waypoint(number) if self.sim.session.waypoint_cell(number).is_some() => {
                Ok(number)
            }
            Target::Waypoint(_) => status(Status::NotFound),
            // The executor addresses waypoints; a plain cell has none.
            Target::Cell(..) | Target::Object(_) => status(Status::Unsupported),
            Target::None => invalid("a stock action needs a waypoint".into()),
            other => invalid(format!("{other:?} is not a waypoint")),
        }
    }

    /// The coordinate of a cell's center on the ground.
    fn cell_center(&self, (x, y): (u16, u16)) -> Act<DriveCoord> {
        let terrain = self.sim.resolved_terrain.as_ref();
        let coord = terrain.and_then(|terrain| {
            let cells = NativeCellQuery::canonical(terrain);
            let cell = cells.lookup((x as i16, y as i16));
            crate::sim::cell_kernel::native_cell_own_coords(cell, &cells)
        });
        match coord {
            Some((x, y, z)) => Ok(DriveCoord {
                x: x as i32,
                y: y as i32,
                z: z as i32,
            }),
            None => status(Status::Refused),
        }
    }

    fn value_i32(&self, what: &str) -> Act<i32> {
        i32::try_from(self.params.value)
            .or_else(|_| invalid(format!("{what} {} is out of range", self.params.value)))
    }

    /// A superweapon ID: its `[SuperWeaponTypes]` position, as stock
    /// actions 33 and 34 number it.
    fn super_weapon(&self, id: i32) -> Act<usize> {
        match usize::try_from(id) {
            Ok(index) if index < self.rules().super_weapon_order.len() => Ok(index),
            _ => invalid(format!("no superweapon {id}")),
        }
    }

    /// A property ID's name; an unknown ID is invalid.
    fn property(&self, id: i32) -> Act<i32> {
        match property_name(id) {
            Some(_) => Ok(id),
            None => invalid(format!("unknown property {id}")),
        }
    }

    /// A cell on the map, from a cell, waypoint or object reference.
    fn map_cell(&self, reference: i64) -> Act<(i16, i16)> {
        let (x, y) = self.cell(reference)?;
        let on_map = self
            .sim
            .resolved_terrain()
            .is_some_and(|terrain| terrain.cell(x, y).is_some());
        match (on_map, i16::try_from(x), i16::try_from(y)) {
            (true, Ok(x), Ok(y)) => Ok((x, y)),
            _ => status(Status::NotFound),
        }
    }

    /// Runs `f` on the host the `set` and `modify` imports use, lent this
    /// action's context. Neither draws from a stream nor queues an order.
    fn host<R>(&mut self, f: impl FnOnce(&mut Host<'_, '_>) -> R) -> R {
        let mut rng = SimRng::new(0);
        let mut orders = Vec::new();
        f(&mut Host {
            sim: &mut *self.sim,
            rules: self.ctx.rules,
            overlay_registry: self.ctx.overlay_registry,
            frame_effects: &mut *self.ctx.frame_effects,
            rng: &mut rng,
            source: self.ctx.source,
            orders: &mut orders,
            rules_ini: None,
        })
    }

    fn atomic(&mut self, kind: i32, target: i64) -> Act<i64> {
        match kind {
            RAISE_EVENT => self.raise_event(target),
            ADD_MONEY => {
                let owner = self.house(target)?;
                let amount = self.value_i32("AddMoney amount")?;
                crate::sim::credit_income::add_credits(&mut self.sim.houses, owner, amount);
                Ok(0)
            }
            KILL => {
                let id = self.object(target)?;
                self.kill(id)
            }
            DAMAGE => self.damage(target),
            REMOVE => {
                let id = self.object(target)?;
                let context =
                    UninitContext::new(self.ctx.rules).with_effects(self.ctx.frame_effects);
                self.sim.uninit_with_context(id, context);
                Ok(0)
            }
            CLEAR_TARGET => {
                let id = self.object(target)?;
                match self.sim.assign_target_represented(
                    id,
                    None,
                    self.ctx.rules,
                    self.ctx.frame_effects,
                ) {
                    Ok(()) => Ok(0),
                    Err(_) => status(Status::Refused),
                }
            }
            SET_OWNER => {
                let id = self.object(target)?;
                self.set_owner(id)
            }
            CREATE_OBJECT => self.create_object(target),
            LAUNCH_SUPER_WEAPON => {
                let owner = self.house(target)?;
                let index = self.super_weapon(self.params.id)?;
                let cell = self.cell(self.params.cell)?;
                let rules = self.ctx.rules;
                let Some(type_id) = rules
                    .super_weapon_order
                    .get(index)
                    .and_then(|name| self.sim.interner.get(name))
                else {
                    return status(Status::Refused);
                };
                let fired = self.sim.fire_super_weapon(
                    rules,
                    owner,
                    type_id,
                    cell,
                    self.ctx.overlay_registry,
                    self.ctx.frame_effects,
                    false,
                );
                if fired {
                    Ok(0)
                } else {
                    status(Status::Refused)
                }
            }
            DETONATE_WARHEAD => self.detonate_warhead(target),
            END_FOR => {
                let owner = self.house(target)?;
                let rules = self.ctx.rules;
                match self.params.arg {
                    0 => self
                        .sim
                        .flag_house_to_win(owner, rules, false, self.ctx.frame_effects),
                    1 => self
                        .sim
                        .flag_house_to_lose(owner, rules, false, self.ctx.frame_effects),
                    arg => return invalid(format!("unknown Outcome {arg}")),
                };
                Ok(0)
            }
            SET_VAR => self.set_var(target),
            LOG => {
                log::info!(target: "ext_module", "{}", self.ctx.text);
                self.sim.ext_modules.log.push(self.ctx.text.to_owned());
                Ok(0)
            }
            SET_PROPERTY => {
                let prop = self.property(self.params.id)?;
                let value = self.params.value;
                answer(self.host(|host| host.set(target, prop, value))?)
            }
            MODIFY => self.modify(target),
            REMOVE_MODIFIERS => self.remove_modifiers(target),
            HEAL => {
                let id = self.object(target)?;
                if self.params.value < 0 {
                    return invalid(format!("negative Heal {}", self.params.value));
                }
                let health = self
                    .sim
                    .entities()
                    .get(id)
                    .map_or(0, |entity| entity.health.current());
                let health = i64::from(health).saturating_add(self.params.value);
                answer(self.host(|host| host.write_health(id, health)))
            }
            SET_MISSION => self.set_mission(target),
            ADD_AMMO => {
                let id = self.object(target)?;
                let Some(ammo) = self.sim.entities().get(id).and_then(|e| e.ammo()) else {
                    return status(Status::Refused);
                };
                let count = i64::from(ammo.current()).saturating_add(self.params.value);
                answer(self.host(|host| host.write_ammo(id, count)))
            }
            ADD_EXPERIENCE => self.add_experience(target),
            SPAWN_ANIM => self.spawn_anim(target),
            DETONATE_WEAPON => self.detonate_weapon(target),
            TELEPORT => self.teleport(target),
            CLEAR_DISGUISE => {
                let id = self.object(target)?;
                let sim = &mut *self.sim;
                let disguised = sim.substrate.entities.get(id).and_then(|entity| {
                    let disguise = entity.seldom.disguise.as_ref()?;
                    disguise.is_disguised().then_some(entity.category())
                });
                if let Some(category) = disguised {
                    // A worn look ends: a new look for the atlas check.
                    sim.substrate.entities.note_look(id);
                    if let Some(disguise) = sim
                        .substrate
                        .entities
                        .get_mut(id)
                        .and_then(|entity| entity.seldom.disguise.as_mut())
                    {
                        disguise.clear(category);
                    }
                }
                Ok(0)
            }
            CLEAR_MIND_CONTROL => {
                let id = self.object(target)?;
                let link = self
                    .sim
                    .entities()
                    .get(id)
                    .map(|entity| &entity.seldom.mind_control);
                let controller = link.and_then(|link| link.controller());
                if controller.is_none() {
                    return match link.is_some_and(|link| link.permanent()) {
                        true => status(Status::Refused),
                        false => Ok(0),
                    };
                }
                let freed = controller.is_some_and(|controller| {
                    self.sim.free_unit(
                        controller,
                        id,
                        self.ctx.rules,
                        self.ctx.overlay_registry,
                        self.ctx.frame_effects,
                    )
                });
                if freed {
                    Ok(0)
                } else {
                    status(Status::Refused)
                }
            }
            CONVERT => self.convert(target),
            PUSH => {
                let id = self.object(target)?;
                let cell = self.map_cell(self.params.cell)?;
                answer(
                    self.sim
                        .start_module_push(id, cell, self.params.value, self.params.value2),
                )
            }
            SET_FACING => {
                let id = self.object(target)?;
                let Ok(facing) = u8::try_from(self.params.value) else {
                    return invalid(format!("SetFacing facing {}", self.params.value));
                };
                answer(self.sim.module_set_facing(id, facing))
            }
            FIRE_WEAPON => self.fire_weapon(target),
            ATTACH_ANIM => self.attach_anim(target),
            DETACH_ANIM => {
                let id = self.object(target)?;
                let anim_type = self.params.id;
                if anim_type != -1 && self.ctx.rules.anim_type_runtime_key_by_index(anim_type).is_none() {
                    return invalid(format!("no animation {anim_type}"));
                }
                let taken = self
                    .sim
                    .ext_modules
                    .held
                    .take_anims_of(id, anim_type, self.ctx.source);
                for anim in taken {
                    self.sim.destroy_held_anim(anim, self.ctx.rules);
                }
                Ok(0)
            }
            ALLOW_TYPE => {
                let owner = self.house(target)?;
                techno_type(self.ctx.rules, self.params.id)?;
                let allow = match self.params.value {
                    0 => false,
                    1 => true,
                    value => return invalid(format!("AllowType value {value}")),
                };
                let Some(house) = self
                    .sim
                    .houses
                    .index_of(owner)
                    .and_then(|index| u32::try_from(index).ok())
                else {
                    return status(Status::Gone);
                };
                // `techno_type` checked the ID is a registry position.
                let type_id = self.params.id as u32;
                self.sim.ext_modules.held.allow_type(house, type_id, allow);
                Ok(0)
            }
            ADD_PASSENGER => self.add_passenger(target),
            SET_SCORE | ADD_SCORE => {
                let owner = self.house(target)?;
                let score = self.value_i32("score")?;
                if let Some(house) = self.sim.houses.get_mut(&owner) {
                    let economy = house.economy_mut();
                    match kind {
                        SET_SCORE => economy.set_score(score),
                        _ => economy.add_score(score),
                    }
                }
                Ok(0)
            }
            _ if vera20k_wasm_ext::action_kind_name(kind).is_some() => status(Status::Unsupported),
            _ => invalid(format!("unknown action kind {kind}")),
        }
    }

    /// Modify: `modify` with the action's operands (appendix 5.3).
    fn modify(&mut self, target: i64) -> Act<i64> {
        let prop = self.property(self.params.id)?;
        let op = self.params.arg;
        if !(0..=2).contains(&op) {
            return invalid(format!("unknown ModifierOp {op}"));
        }
        let Ok(frames) = i32::try_from(self.params.value2) else {
            return invalid(format!("{} modifier frames", self.params.value2));
        };
        let (value, source, group) = (self.params.value, self.params.other, self.params.arg2);
        match self.host(|host| host.modify(target, prop, op, value, frames, source, group))? {
            (handle, Status::Ok) => Ok(handle),
            (_, other) => status(other),
        }
    }

    /// RemoveModifiers: this origin's modifiers on the object in group
    /// `arg2`, of property `id` unless it is −1. Houses take no modifiers.
    fn remove_modifiers(&mut self, target: i64) -> Act<i64> {
        let stat = match self.params.id {
            -1 => None,
            id => {
                let name = property_name(self.property(id)?).unwrap_or_default();
                match Stat::wired_property(name) {
                    Some(stat) => Some(stat),
                    // A modifiable property no modifier is wired to holds none.
                    None if super::property_id_range(super::MODIFIABLE).contains(&id) => {
                        return Ok(0);
                    }
                    None => return status(Status::Refused),
                }
            }
        };
        let id = match decode(target)? {
            Target::House(_) => return status(Status::Unsupported),
            _ => self.object(target)?,
        };
        let sim = &mut *self.sim;
        let removed = sim.ext_modules.modifiers.remove_group(
            &mut sim.substrate.entities,
            id,
            self.ctx.source,
            self.params.arg2,
            stat,
        );
        sim.raise_modifiers_expired(self.ctx.rules, self.ctx.frame_effects, &removed);
        Ok(0)
    }

    /// Convert: the type through Ares ConvertToType; a positive `value2`
    /// converts back after that many frames (`held::ActionHeld`).
    fn convert(&mut self, target: i64) -> Act<i64> {
        let id = self.object(target)?;
        let Ok(frames) = u32::try_from(self.params.value2) else {
            return invalid(format!("Convert frames {}", self.params.value2));
        };
        let rules = self.ctx.rules;
        let object = techno_type(rules, self.params.id)?;
        let destination = crate::rules::superweapon_type::TechnoTypeReference {
            category: object.category,
            id: object.id.clone(),
        };
        let original = self
            .sim
            .entities()
            .get(id)
            .and_then(|entity| self.sim.entity_object_type(entity, rules))
            .map(
                |object| crate::rules::superweapon_type::TechnoTypeReference {
                    category: object.category,
                    id: object.id.clone(),
                },
            );
        let Some(original) = original else {
            return status(Status::Refused);
        };
        if !self
            .sim
            .convert_techno_type(id, &destination, rules, self.ctx.frame_effects)
        {
            return status(Status::Refused);
        }
        let frame = self.sim.session.binary_frame();
        self.sim
            .ext_modules
            .held
            .note_conversion(id, original, frames, frame);
        self.sim.ext_modules.motions.end_push(id);
        Ok(0)
    }

    /// FireWeapon: the shooter's slot `value` (0 primary, 1 secondary), or
    /// the slot holding weapon `id` at its rank (GetWeapon), at `cell`, else
    /// `other`, else its current target, through FireAt's direct entry once
    /// GetFireError answers Ok.
    fn fire_weapon(&mut self, target: i64) -> Act<i64> {
        use crate::sim::combat::combat_weapon::resolve_weapon_index;
        use crate::sim::combat::fire_error::FireError;
        use crate::sim::combat::fire_error_world::FireSubject;
        let id = self.object(target)?;
        let rules = self.ctx.rules;
        let weapon_index = if self.params.id == -1 {
            match self.params.value {
                0 => 0,
                1 => 1,
                _ => return status(Status::Unsupported),
            }
        } else {
            let Some(weapon) = u32::try_from(self.params.id)
                .ok()
                .and_then(|index| rules.weapon_at(index))
            else {
                return invalid(format!("no weapon {}", self.params.id));
            };
            let sim = &*self.sim;
            let slot = sim.substrate.entities.get(id).and_then(|actor| {
                let obj = sim.object_type(actor.type_ref(), rules)?;
                [0, 1].into_iter().find(|&slot| {
                    resolve_weapon_index(
                        rules,
                        actor,
                        obj,
                        slot,
                        &sim.substrate.entities,
                        &sim.interner,
                    )
                    .is_some_and(|selected| selected.weapon_id.eq_ignore_ascii_case(&weapon.id))
                })
            });
            match slot {
                Some(slot) => slot,
                // FireAt fires only the shooter's own weapon slots.
                None => return status(Status::Unsupported),
            }
        };
        let aim = if self.params.cell != 0 {
            let (x, y) = self.map_cell(self.params.cell)?;
            TargetKind::Cell(x as u16, y as u16)
        } else {
            match decode(self.params.other)? {
                Target::Object(other) if self.sim.entities().get(other).is_some() => {
                    TargetKind::Entity(other)
                }
                Target::Object(_) => return status(Status::Gone),
                Target::None => {
                    let held = self
                        .sim
                        .entities()
                        .get(id)
                        .and_then(|entity| entity.attack_target.target());
                    match held {
                        Some(held) => held,
                        None => return status(Status::Gone),
                    }
                }
                other => return invalid(format!("{other:?} is not a FireWeapon target")),
            }
        };
        let sim = &*self.sim;
        let ready = sim.substrate.entities.get(id).is_some_and(|firer| {
            firer.lifecycle.object_alive()
                && !firer.lifecycle.in_limbo()
                && sim.object_type(firer.type_ref(), rules).is_some_and(|obj| {
                    FireSubject {
                        world: sim,
                        rules,
                        fog: None,
                        firer,
                        obj,
                        target: Some(aim),
                        weapon_index,
                    }
                    .fire_error(true)
                        == FireError::Ok
                })
        });
        if !ready {
            return status(Status::Refused);
        }
        let receipt = self.sim.commit_fire_visit(
            crate::sim::combat::world_fire::FireVisit::Direct {
                id,
                target: aim,
                weapon_index,
            },
            rules,
            self.ctx.overlay_registry,
            self.ctx.frame_effects,
        );
        if receipt.bridge_state_changed {
            self.sim.publish_trigger_effects([
                crate::sim::trigger_runtime::TriggerEffect::BridgeStateChanged,
            ]);
        }
        Ok(0)
    }

    /// AttachAnim: one pass of animation `id` (its array index, as
    /// `SpawnAnim` takes) attached to the object, as team script action 51
    /// attaches one to each member. `value2` other than 0 keeps it looping
    /// for that many frames, or until `DetachAnim` when negative.
    fn attach_anim(&mut self, target: i64) -> Act<i64> {
        let id = self.object(target)?;
        let rules = self.ctx.rules;
        let index = self.params.id;
        if rules.anim_type_runtime_key_by_index(index).is_none() {
            return invalid(format!("no animation {index}"));
        }
        let frames = self.params.value2;
        let target = self.sim.entities().get(id).filter(|entity| entity.lifecycle.object_alive());
        let Some(target) = target else {
            return status(Status::Refused);
        };
        let held = frames != 0;
        if held && !self.sim.ext_modules.held.can_hold_anim() {
            return status(Status::Refused);
        }
        let anim = if held && super::held::held_anim_hidden(target) {
            None
        } else {
            let Some(anim) = self.sim.spawn_attached_anim(rules, id, index, held) else {
                return status(Status::Refused);
            };
            Some(anim)
        };
        if held {
            let frame = self.sim.session.binary_frame();
            self.sim
                .ext_modules
                .held
                .hold_anim(anim, id, index, self.ctx.source, frames, frame);
        }
        Ok(0)
    }

    /// AddPassenger: the passenger (`other`) boards the transport (the
    /// target) as the PerCellProcess transport arm boards one, once the
    /// transport answers CAN_LOAD for it.
    fn add_passenger(&mut self, target: i64) -> Act<i64> {
        use crate::sim::passenger::{BoardingArm, board_passenger, can_enter_transport};
        let carrier = self.object(target)?;
        let passenger = self.object(self.params.other)?;
        let rules = self.ctx.rules;
        let sim = &*self.sim;
        let placed = move |id: u64| {
            sim.entities()
                .get(id)
                .filter(|entity| entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo())
        };
        let (Some(transport), Some(rider)) = (placed(carrier), placed(passenger)) else {
            return status(Status::Refused);
        };
        if transport.category() == EntityCategory::Structure {
            return status(Status::Unsupported);
        }
        let arm = match rider.category() {
            EntityCategory::Infantry => BoardingArm::InfantryTransport,
            EntityCategory::Unit => BoardingArm::UnitTransport,
            _ => return status(Status::Refused),
        };
        if carrier == passenger || !can_enter_transport(sim, rules, passenger, carrier) {
            return status(Status::Refused);
        }
        if board_passenger(
            self.sim,
            rules,
            passenger,
            carrier,
            arm,
            self.ctx.overlay_registry,
            self.ctx.frame_effects,
        ) {
            Ok(0)
        } else {
            status(Status::Refused)
        }
    }

    /// SetMission: `cell`, else `other`, is the mission's target or
    /// destination; neither orders it untargeted.
    fn set_mission(&mut self, target: i64) -> Act<i64> {
        let id = self.object(target)?;
        let token = if self.params.cell != 0 {
            let (x, y) = self.cell(self.params.cell)?;
            Some(TargetKind::Cell(x, y))
        } else {
            match decode(self.params.other)? {
                Target::None => None,
                Target::Object(other) => match self.sim.entities().get(other) {
                    Some(_) => Some(TargetKind::Entity(other)),
                    None => return status(Status::Gone),
                },
                other => return invalid(format!("{other:?} is not a mission target")),
            }
        };
        let mission = i64::from(self.params.arg);
        answer(self.host(|host| host.order_mission(id, mission, token))?)
    }

    /// AddExperience: `VeterancyClass::Add` (`0x0074FF50`) as a kill worth
    /// `value` would add it, at the object's own Cost.
    fn add_experience(&mut self, target: i64) -> Act<i64> {
        let id = self.object(target)?;
        let points = self.value_i32("AddExperience points")?;
        if points < 0 {
            return invalid(format!("negative experience {points}"));
        }
        let rules = self.ctx.rules;
        let sim = &mut *self.sim;
        let object = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| sim.entity_object_type(entity, rules));
        let Some(object) = object.filter(|object| object.trainable) else {
            return status(Status::Refused);
        };
        let cost = rules.cost_of(object, None);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.veterancy.award_kill(
                cost,
                points,
                true,
                rules.general.veteran_ratio,
                rules.general.veteran_cap,
            );
        }
        Ok(0)
    }

    /// SpawnAnim: stock action 41's owner at a cell.
    fn spawn_anim(&mut self, target: i64) -> Act<i64> {
        let cell = self.map_cell(target)?;
        let rules = self.ctx.rules;
        let index = self.params.id;
        if rules.anim_type_runtime_key_by_index(index).is_none() {
            return invalid(format!("no animation {index}"));
        }
        let house = match self.params.house {
            0 => None,
            house => Some(self.house(house)?),
        };
        if self
            .sim
            .trigger_play_anim_at_cell(rules, index, cell, house, self.ctx.frame_effects)
        {
            Ok(0)
        } else {
            status(Status::Refused)
        }
    }

    /// DetonateWeapon: stock action 42's owner at a cell, crediting the
    /// source.
    fn detonate_weapon(&mut self, target: i64) -> Act<i64> {
        let cell = self.map_cell(target)?;
        let rules = self.ctx.rules;
        let Some(weapon) = u32::try_from(self.params.id)
            .ok()
            .and_then(|id| rules.weapon_at(id))
        else {
            return invalid(format!("no weapon {}", self.params.id));
        };
        let source = self.source()?;
        let source_house = source.and_then(|s| self.sim.entities().get(s).map(|e| e.owner()));
        let changed = self.sim.trigger_explosion_at_cell(
            rules,
            weapon,
            cell,
            (source.unwrap_or(RAD_NO_ATTACKER), source_house),
            self.ctx.overlay_registry,
            self.ctx.frame_effects,
        );
        match changed {
            Some(changed) => {
                if changed {
                    self.sim.publish_trigger_effects([
                        crate::sim::trigger_runtime::TriggerEffect::BridgeStateChanged,
                    ]);
                }
                Ok(0)
            }
            None => status(Status::Refused),
        }
    }

    /// Teleport: a Chrono warp of a ground Unit or Infantry to `cell`.
    /// `Blocked` (`arg`) 0 `Nearby` lets the Teleport's own blocked landing
    /// choose a nearby cell; 1 `Fail` refuses a cell it cannot enter.
    fn teleport(&mut self, target: i64) -> Act<i64> {
        use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
        let id = self.object(target)?;
        let cell = self.map_cell(self.params.cell)?;
        let fail = match self.params.arg {
            0 => false,
            1 => true,
            arg => return invalid(format!("unknown Blocked {arg}")),
        };
        let rules = self.ctx.rules;
        let infantry = match self.sim.entities().get(id) {
            Some(entity)
                if entity.lifecycle.object_alive()
                    && !entity.lifecycle.in_limbo()
                    && entity.passenger_role().transporter().is_none() =>
            {
                match entity.category() {
                    EntityCategory::Infantry => true,
                    EntityCategory::Unit => false,
                    _ => return status(Status::Refused),
                }
            }
            _ => return status(Status::Refused),
        };
        if fail {
            let entry = self.sim.mover_can_enter(
                id,
                cell,
                InfantryEntryArgs::REPAIR,
                EntryQueryMode::CheckLocomotor,
                rules,
            );
            if entry != Ok(0) {
                return status(Status::Refused);
            }
        }
        let warped = self
            .sim
            .chrono_warp_coord(cell, infantry, rules)
            .and_then(|coord| {
                self.sim.foot_chrono_warp_to(
                    id,
                    coord,
                    rules,
                    self.ctx.overlay_registry,
                    self.ctx.frame_effects,
                )
            });
        match warped {
            Ok(true) => {
                self.sim.ext_modules.motions.end_push(id);
                Ok(0)
            }
            Ok(false) => status(Status::Refused),
            Err(why) => {
                log::info!(target: "ext_module", "Teleport of {id} refused: {why}");
                status(Status::Refused)
            }
        }
    }

    /// SetVar: the variable's declared scope picks the holder (appendix 5.3).
    fn set_var(&mut self, target: i64) -> Act<i64> {
        let var = self.params.id;
        let scope = match self.sim.ext_modules.vars.scope(var) {
            // Red Alert 2 variables are global.
            None | Some(VarScope::Global) => 0,
            Some(VarScope::House) => {
                let house = self.house(target)?;
                match self.sim.houses.index_of(house) {
                    Some(index) => super::house_ref(index),
                    None => return status(Status::Gone),
                }
            }
            Some(VarScope::Object) => target,
        };
        match self.sim.write_module_var(scope, var, self.params.value)? {
            Status::Ok => Ok(0),
            other => status(other),
        }
    }

    /// Kill: health as damage through the receiver, defenses ignored, with
    /// `C4Warhead=` (stock action 32, `0x006E2050`), crediting the source.
    fn kill(&mut self, id: u64) -> Act<i64> {
        let rules = self.ctx.rules;
        let Some(health) = self.sim.entities().get(id).and_then(|entity| {
            (entity.health.current() > 0
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo())
            .then_some(entity.health.current())
        }) else {
            return status(Status::Refused);
        };
        let Some(c4) = rules.warhead(&rules.bridge_warheads.c4_name) else {
            return status(Status::Refused);
        };
        let warhead = self.sim.interner.intern(&c4.id);
        self.receive_damage(id, health, warhead, true)
    }

    /// Damage: `value` through the receiver with the warhead's armor rules,
    /// crediting the source.
    fn damage(&mut self, target: i64) -> Act<i64> {
        let id = self.object(target)?;
        let rules = self.ctx.rules;
        let Some(warhead) = u32::try_from(self.params.id)
            .ok()
            .and_then(|id| rules.warhead_at(id))
        else {
            return invalid(format!("no warhead {}", self.params.id));
        };
        let damage = self.value_i32("Damage amount")?;
        if damage < 0 {
            return invalid(format!("negative damage {damage}"));
        }
        let alive = self.sim.entities().get(id).is_some_and(|entity| {
            entity.health.current() > 0
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
        });
        if !alive {
            return status(Status::Refused);
        }
        let warhead = self.sim.interner.intern(&warhead.id);
        self.receive_damage(id, damage, warhead, false)
    }

    fn receive_damage(
        &mut self,
        id: u64,
        damage: i32,
        warhead: InternedId,
        ignore_defenses: bool,
    ) -> Act<i64> {
        let source = self.source()?;
        let source_house = source.and_then(|s| self.sim.entities().get(s).map(|e| e.owner()));
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            id,
            damage,
            0,
            source.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            source_house,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses,
                arg6: false,
            },
        );
        self.sim.commit_direct_damage_receiver(
            self.ctx.rules,
            self.ctx.overlay_registry,
            event,
            self.ctx.frame_effects,
        );
        Ok(0)
    }

    /// SetOwner and stock 14: the shared ChangeOwner owner, for a live
    /// object.
    fn set_owner(&mut self, id: u64) -> Act<i64> {
        let new_owner = self.house(self.params.house)?;
        let placed =
            self.sim.entities().get(id).is_some_and(|entity| {
                entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo()
            });
        if !placed {
            return status(Status::Refused);
        }
        self.sim.change_owner_with_rules(
            id,
            new_owner,
            self.ctx.rules,
            self.ctx.overlay_registry,
            self.ctx.frame_effects,
        );
        Ok(0)
    }

    /// CreateObject at a cell: the constructor, then Unlimbo at the cell
    /// center, which refuses an occupied cell. A factory target, a copied
    /// tag (`other`) and the factory exit are not supported yet.
    fn create_object(&mut self, target: i64) -> Act<i64> {
        if let Target::Object(_) = decode(target)? {
            return status(Status::Unsupported);
        }
        let cell = self.cell(target)?;
        let owner = self.house(self.params.house)?;
        let mission = self.params.arg;
        if mission != MISSION_DEFAULT && !(0..=31).contains(&mission) {
            return invalid(format!("unknown Mission {mission}"));
        }
        if self.params.other != 0 {
            return status(Status::Unsupported);
        }
        let rules = self.ctx.rules;
        let object = techno_type(self.ctx.rules, self.params.id)?;
        let building = object.category == crate::rules::object_type::ObjectCategory::Building;
        let coord = self.cell_center(cell)?;
        let Some(created) = self.sim.construct_object_limbo_at_height(
            object.id.as_str(),
            owner,
            0,
            0,
            0,
            0,
            rules,
            self.ctx.frame_effects,
        ) else {
            return status(Status::Refused);
        };
        let placed = self
            .sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                created,
                coord,
                0,
                PlacementEvidence::EvaluateMark,
                rules,
                self.ctx.overlay_registry,
                self.ctx.frame_effects,
            );
        let Some(placed) = placed else {
            let context = UninitContext::new(rules).with_effects(self.ctx.frame_effects);
            self.sim.uninit_with_context(created, context);
            return status(Status::Refused);
        };
        if building {
            self.sim.grand_opening(
                placed,
                false,
                false,
                rules,
                self.ctx.overlay_registry,
                self.ctx.frame_effects,
            );
        }
        if mission != MISSION_DEFAULT
            && let Some(entity) = self.sim.substrate.entities.get_mut(placed)
        {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_raw(mission),
            );
        }
        // Wasm module `Birth` (appendix 4.3) for the object it made.
        self.sim
            .raise_module_birth(rules, self.ctx.frame_effects, placed, None);
        Ok(placed as i64)
    }

    /// DetonateWarhead: Apply_area_damage at the cell center.
    fn detonate_warhead(&mut self, target: i64) -> Act<i64> {
        let cell = self.cell(target)?;
        let rules = self.ctx.rules;
        let Some(warhead) = u32::try_from(self.params.id)
            .ok()
            .and_then(|id| rules.warhead_at(id))
        else {
            return invalid(format!("no warhead {}", self.params.id));
        };
        let damage = self.value_i32("DetonateWarhead damage")?;
        let source = self.source()?;
        let source_house = source.and_then(|s| self.sim.entities().get(s).map(|e| e.owner()));
        let coord = self.cell_center(cell)?;
        let warhead_ref = self.sim.interner.intern(&warhead.id);
        let applied = crate::sim::combat::world_receiver::apply_area_damage(
            self.sim,
            rules,
            self.ctx.overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            damage,
            warhead,
            true,
            (
                source.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                source_house,
                warhead_ref,
            ),
            self.ctx.frame_effects,
        );
        if applied.bridge_state_changed {
            self.sim.publish_trigger_effects([
                crate::sim::trigger_runtime::TriggerEffect::BridgeStateChanged,
            ]);
        }
        Ok(0)
    }

    fn stock(&mut self, kind: i32, target: i64) -> Act<i64> {
        let Some(operands) = stock_operands(kind) else {
            return match vera20k_wasm_ext::action_kind_name(kind) {
                Some(_) => status(Status::Unsupported),
                None => invalid(format!("unknown action kind {kind}")),
            };
        };
        let params = self.params;
        let mut call = StockActionCall {
            kind,
            ..StockActionCall::default()
        };
        match operands {
            Operands::Nothing => return Ok(0),
            Operands::Value => call.value = Some(self.value_i32("value")?),
            Operands::Variable { count } => {
                if !(0..count).contains(&params.value) {
                    return invalid(format!("variable {} is out of range", params.value));
                }
                call.value = Some(params.value as i32);
            }
            Operands::Country => match self.rules().country_index(self.ctx.text) {
                Some(country) => call.value = Some(i32::from(country.0)),
                None => return status(Status::NotFound),
            },
            Operands::Label => call.params = vec!["4".into(), self.ctx.text.to_owned()],
            Operands::Theme => call.params = vec!["8".into(), self.ctx.text.to_owned()],
            Operands::Speech => {
                // Case 21's network form: the EVA name reaches the client's
                // announcer, which resolves it.
                let reference = crate::rules::sound_ini::SoundReference::Name(
                    self.ctx.text.to_ascii_uppercase(),
                );
                self.sim
                    .emit_trigger_sound(&reference, crate::sim::world::TriggerSoundTarget::Eva);
                self.ctx.frame_effects.flush_sound(self.sim, self.ctx.rules);
                return Ok(0);
            }
            Operands::Trigger | Operands::Tag => {
                if params.other != 0 {
                    // Trigger and tag handles belong to module triggers.
                    return status(Status::Unsupported);
                }
                let (kind, wrap): (_, fn(usize) -> StockReference) = match operands {
                    Operands::Trigger => (StockReferenceKind::Trigger, StockReference::Trigger),
                    _ => (StockReferenceKind::Tag, StockReference::Tag),
                };
                match self.sim.stock_definition(kind, self.ctx.text) {
                    Some(index) => call.reference = Some(wrap(index)),
                    None => return status(Status::NotFound),
                }
            }
            Operands::Team { waypoint } => {
                // 105 TalkBubble: −1 names no team, which clears the bubble.
                if !(kind == 105 && params.id == -1) {
                    let team = usize::try_from(params.id)
                        .ok()
                        .filter(|&id| self.sim.stock_team_type_name(id).is_some());
                    let Some(team) = team else {
                        return invalid(format!("no TeamType {}", params.id));
                    };
                    call.reference = Some(StockReference::Team(team));
                }
                if waypoint {
                    call.waypoint = Some(self.waypoint(self.place(target))? as u32);
                }
                call.value = Some(self.value_i32("value")?);
            }
            Operands::Waypoint => call.waypoint = Some(self.waypoint(self.place(target))? as u32),
            Operands::WaypointValue => call.value = Some(self.waypoint(self.place(target))?),
            Operands::WaypointId { house } => {
                call.waypoint = Some(self.waypoint(self.place(target))? as u32);
                call.value = Some(params.id);
                if house && params.house != 0 {
                    call.house = Some(self.house(params.house)?);
                }
            }
            Operands::WaypointOperand => {
                call.waypoint = Some(self.waypoint(self.place(target))? as u32);
                call.value = Some(self.value_i32("value")?);
            }
            Operands::WaypointHouse { type_name } => {
                call.waypoint = Some(self.waypoint(self.place(target))? as u32);
                call.house = Some(self.house(params.house)?);
                if type_name {
                    let name = techno_type(self.ctx.rules, params.id)?.id.clone();
                    call.params = vec!["10".into(), name];
                }
            }
            Operands::TypeValue => {
                let name = techno_type(self.ctx.rules, params.id)?.id.clone();
                call.params = vec!["10".into(), name];
                call.value = Some(self.value_i32("value")?);
            }
            Operands::House => {
                call.house = Some(self.house(target)?);
                call.value = Some(self.value_i32("value")?);
            }
            Operands::HouseSuper { auxiliary } => {
                call.house = Some(self.house(target)?);
                call.value = Some(self.super_weapon(params.id)? as i32);
                if auxiliary {
                    // Parameter type 11 keeps token 8 at +0x48.
                    let value = self.value_i32("value")?.to_string();
                    call.params = ["11", "0", "0", "0", "0", "0"]
                        .into_iter()
                        .map(String::from)
                        .chain([value])
                        .collect();
                }
            }
            Operands::HouseWaypoint => {
                call.house = Some(self.house(target)?);
                if matches!(kind, 128 | 135 | 137 | 140) {
                    call.waypoint = Some(self.waypoint(params.cell)? as u32);
                }
            }
            Operands::HouseType => {
                call.house = Some(self.house(target)?);
                let name = techno_type(self.ctx.rules, params.id)?.id.clone();
                call.params = vec!["10".into(), name];
                call.value = Some(self.value_i32("value")?);
            }
            Operands::Object => {
                let id = self.object(target)?;
                return self.stock_object(kind, id);
            }
            Operands::Ares(ares) => {
                if !self.rules().ares_form() {
                    return status(Status::Unsupported);
                }
                match ares {
                    AresOperands::House => call.house = Some(self.house(target)?),
                    AresOperands::Value => call.value = Some(self.value_i32("value")?),
                    AresOperands::Object => {
                        call.actor = Some(self.object(target)?);
                        call.value = Some(self.value_i32("value")?);
                    }
                }
            }
        }
        let handled = self.sim.run_untriggered_stock_action(
            call,
            self.ctx.rules,
            self.ctx.overlay_registry,
            self.ctx.frame_effects,
        );
        if handled {
            Ok(0)
        } else {
            status(Status::Refused)
        }
    }

    /// The place of a stock action with an optional cell target: the target,
    /// else `cell`.
    fn place(&self, target: i64) -> i64 {
        if target != 0 {
            target
        } else {
            self.params.cell
        }
    }

    /// Stock actions on the tag holder, called on the target object through
    /// the owners the executor calls for each tagged object.
    fn stock_object(&mut self, kind: i32, id: u64) -> Act<i64> {
        let rules = self.ctx.rules;
        let building = self.sim.entities().get(id).is_some_and(|entity| {
            entity.category() == EntityCategory::Structure
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
        });
        let done = match kind {
            14 => return self.set_owner(id),
            32 => return self.kill(id),
            60 if building => crate::sim::production::sell_back(
                self.sim,
                rules,
                id,
                crate::sim::production::SellOrder::Computer,
                self.ctx.overlay_registry,
                self.ctx.frame_effects,
            ),
            61 if building => self
                .sim
                .go_building_offline(id, rules, self.ctx.frame_effects),
            62 if building => self
                .sim
                .go_building_online(id, rules, self.ctx.frame_effects),
            _ => false,
        };
        if done { Ok(0) } else { status(Status::Refused) }
    }
}
