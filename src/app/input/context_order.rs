//! Context-sensitive order resolution — translates a screen click into game commands.
//!
//! Given a click position and the current selection, determines what command to issue:
//! move, attack, garrison, deploy, harvest, rally point, etc. This is the decision tree
//! that maps player intent to `Command` envelopes.
//!
//! Split from the input dispatcher to separate order resolution from raw input handling.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner;
use crate::app::input::dispatch::{
    is_alt_held, is_ctrl_held, is_shift_held, selected_stable_ids_in_order,
};
use crate::app::input::entity_pick::{HoverObject, HoverTargetKindWithId, hover_object_at_point};
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;

/// The verb a Ctrl / Shift / Alt chord selects for a tactical click.
///
/// Retail contract, read from `TechnoClass::What_Action_OnCell`,
/// `TechnoClass::What_Action_OnObject` and `TechnoClass::Player_Send_Command`:
///
/// * **Ctrl alone → force fire.** The cell path takes the attack branch with no
///   enemy under the cursor; the object path drops the ally guard.
/// * **Alt alone → force move.** The object path returns the plain Move action
///   instead of whatever context action the object would resolve, and the cell
///   path returns Move without running its occupancy probe.
/// * **Ctrl+Shift → attack move.** `What_Action_OnCell` cancels Shift and Ctrl
///   against each other, so the action itself resolves as an ordinary
///   Move/Attack; `Player_Send_Command` then promotes a committed Move or Attack
///   mission to attack-move whenever the chord test passes. That test reads the
///   raw key state, so the chord still fires when Alt is also held — and because
///   the cancel has already cleared Ctrl, the Ctrl+Alt guard-area gate cannot.
/// * **Ctrl+Alt → guard area** (patrol when the cell carries a waypoint).
/// * **Shift alone.** On an object it returns the
///   add-to-selection action, which the object click handler has no case for and
///   therefore sends no mission; on a cell it returns the plain Move action, an
///   ordinary immediate move that every Foot's Cell resolver receives. Retail
///   Planning Mode uses its existing shared path owner and separate event opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderModifier {
    /// No modifier held — the object/cell context action stands.
    Normal,
    /// Ctrl — force fire.
    ForceFire,
    /// Alt — force move.
    ForceMove,
    /// Ctrl+Shift — attack move.
    AttackMove,
    /// Ctrl+Alt — guard area.
    GuardArea,
    /// Shift — over a Cell, What_Action answers Move before Can_Enter_Cell
    /// (`0x00700B03`).
    Shift,
}

/// Resolve the retail modifier verb from the three held-key states.
///
/// Ordering mirrors the binary: the Ctrl+Shift chord is tested against the raw
/// key state before anything else, then the Ctrl+Alt guard-area gate, then the
/// single-modifier branches. Shift outranks Alt because the cell path returns on
/// Shift before it ever reaches the Alt test, and the object path returns the
/// add-to-selection action before the Alt branch.
pub(crate) fn resolve_order_modifiers(ctrl: bool, shift: bool, alt: bool) -> OrderModifier {
    if ctrl && shift {
        OrderModifier::AttackMove
    } else if ctrl && alt {
        OrderModifier::GuardArea
    } else if ctrl {
        OrderModifier::ForceFire
    } else if shift {
        OrderModifier::Shift
    } else if alt {
        OrderModifier::ForceMove
    } else {
        OrderModifier::Normal
    }
}

/// The action [`attack_action`] answers, shared by cursor and click. When it
/// is the action of the object that owns the cursor, a click dispatches
/// Attack and NoMove to every receiver; Select, and 0 over an object the
/// player may select and has not, select that object instead
/// (`DisplayClass::BandBox_LeftUp` `0x004ABDD3..0x004ABE11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttackAction {
    None,
    Select,
    Attack,
    NoMove,
    Rally,
    /// Move (1) over an object: an Aircraft's own Cell action under it
    /// (`0x00417D7C`), which its ClickedAction sends to Foot's Move arm.
    Move,
    /// AttackSupport (0x3B): a player's healing Unit's Attack on a Cell
    /// (`0x007404FD..0x00740522`). SetCursorFromAction (`0x004AAE90`) has no
    /// case for it, so it shows the arrow (row 0), and Foot's ClickedAction_Cell
    /// (`0x004D7D50`) takes its default case (`0x004D8237`): the click is
    /// consumed and sends nothing.
    AttackSupport,
}

/// The allied-object part of Techno6FFEC0's unforced attack admission,
/// 700314..7003D4. All consumers ask this through the existing action owner.
/// IsAlly reads the real owner and the asking House's directional mask;
/// display friendliness alone cannot decide whether to attack or select.
fn friendly_attack_is_admitted(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    use crate::sim::combat::combat_weapon;

    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(target) = sim.entities().get(target_id) else {
        // House4F9A90 sees Terrain's null owner (Object4104A0), so
        // the allied-only restrictions do not apply to a real Terrain.
        return crate::sim::movement::ground_pose::target_get_coords(
            TargetKind::Entity(target_id),
            sim,
            None,
        )
        .is_some();
    };
    if !combat_weapon::is_ally_by_object(
        Some(sim.house_alliances()),
        sim.interner(),
        actor.owner(),
        target.owner(),
    ) {
        return true;
    }
    let Some(kind) = sim.interner().object_type(actor.type_ref(), rules) else {
        return false;
    };
    if kind.attacks_friendlies() {
        return true;
    }
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let weapon = combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        kind,
        &combat_weapon::attacker_facts(actor, kind),
        Some(&TargetKind::Entity(target_id)),
        sim,
        sim.interner(),
        cells.as_ref(),
        Some(sim.house_alliances()),
    )
    .and_then(|(_, weapon)| weapon);
    let ivan_transport = weapon
        .as_ref()
        .is_some_and(|weapon| weapon.warhead.ivan_bomb)
        && sim
            .interner()
            .object_type(target.type_ref(), rules)
            .is_some_and(|target| target.passengers > 0);
    (kind.attack_cursor_on_friendlies && !ivan_transport)
        || (weapon.as_ref().is_some_and(|weapon| weapon.warhead.sonic)
            && target.category() != EntityCategory::Structure
            && target.parasite.is_infested())
}

/// What_Action_OnObject's attack rung without Ctrl (`0x00700400..0x0070043E`)
/// needs the object's type (vt+0x88) to be `LegalTarget=` (`+0x231`,
/// `0x0070042E`) or TechnoClass vt+0x398 (`0x0070EF00`, asked at
/// `0x007002EE`) to answer yes. For a Terrain object
/// that is its TerrainType's `LegalTarget=` (retail: only `VEINHOLE`), else
/// `[CombatDamage] TreeTargeting=` (`Rules+0x17E9`, retail no) or the
/// `TerrainFire=` (`+0x138`) of the weapon SelectWeapon picks for it
/// (`0x0070EF0C..0x0070EF4B`). A Techno target passes here (#605).
fn terrain_target_is_legal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    use crate::sim::combat::combat_weapon;

    let Some(terrain) = sim.terrain_objects().get(target_id) else {
        return true;
    };
    if rules
        .terrain_object_type_case_insensitive(sim.interner().resolve(terrain.type_ref()))
        .is_some_and(|terrain| terrain.legal_target)
    {
        return true;
    }
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(kind) = sim.interner().object_type(actor.type_ref(), rules) else {
        return false;
    };
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        kind,
        &combat_weapon::attacker_facts(actor, kind),
        Some(&TargetKind::Entity(target_id)),
        sim,
        sim.interner(),
        cells.as_ref(),
        Some(sim.house_alliances()),
    )
    .and_then(|(_, weapon)| weapon)
    .is_some_and(|weapon| rules.general.tree_targeting || weapon.weapon.terrain_fire)
}

/// The attack rung of the actor's What_Action and the tail it falls to.
///
/// Object target (`TechnoClass::What_Action_OnObject @ 0x006FFEC0`): the
/// rung is a human player's (`0x007003ED`), for an armed actor (`Is_Armed`,
/// vt+0x2AC) or an `Infiltrate=` infantryman (`0x00700444..0x00700474`), and
/// asks its control capability (vt+0xA0) or its selected weapon's range
/// (`0x0070047A..0x0070049A`). Unless `forced`, a shot ILLEGAL to the actor
/// leaves the rung as well ([`shot_is_illegal`]); `forced` is the force-fire
/// key, which What_Action reads itself (`0x00700048..0x00700072`): Ctrl,
/// alone or in the Ctrl+Alt chord. Off the rung, the tail
/// (`0x0070056C..0x007005E6`) answers 0 when an armed human player's object
/// points at another house's object, or for a loaner (`+0x3D4`) the player
/// owns (`+0x41A`); otherwise Select for an object the player may select
/// (vt+0x138) that is neither selected (`+0x83`) nor warped out (vt+0x1D4),
/// and 0 for any other.
///
/// Cell target (`TechnoClass::What_Action_OnCell @ 0x00700600`): the rung is
/// a human player's too (`0x007008A6..0x007008B3`), and the caller has asked
/// the weapon ([`armed_action`]); control or range answers Attack
/// (`0x007009F3..0x00700A44`). A rejected rung continues at the Cell
/// movement tail (`0x00700AB7`), which callers resolve through
/// `cell_move_action` and `ordinary_cell_move_goal`. Any other house's object
/// answers 0 there (`0x00700AB7..0x00700AC4`).
///
/// This asks no ROF or reload. The class post-processing follows below. See
/// the original DetermineAction/Unit/Foot/Techno and Display action corpus
/// in tools/spatial_oracle/bunker_attack_action.json.
///
/// The real-owner ally gate and its shared weapon/type grants are handled by
/// [`friendly_attack_is_admitted`]. A different allied House therefore takes
/// the identity-sensitive tail, then the existing class postprocessing.
///
/// Unforced, a Terrain object must pass [`terrain_target_is_legal`].
///
/// RESIDUAL (#605): the rest of admission (`0x00700255..0x0070043E`) is not
/// ported as one gate. Unforced, native also leaves the rung for:
/// - an object disguised to the actor's house (vt+0xC8), unless that grant
///   holds, `[Rules+0xD6C]` is set, its type is `PermaDisguise=` (`+0xD30`),
///   its `+0x1EC`/`+0x1F4` timer runs, or it wears no house (vt+0xD0) and the
///   weapon may fire at terrain (vt+0x398, `0x0070EF00`, `TerrainFire=`);
/// - a Techno type that is not `LegalTarget=` (`+0x231`), unless that
///   terrain test passes. No retail Techno type clears `LegalTarget=`.
///
/// Active YR Battle authors AttackCursorOnDisguise=yes, and SPY also authors
/// PermaDisguise=yes: an ordinary stock Spy/Mirage is not evidence of a missed
/// rejection. A layer disabling those grants or a non-LegalTarget type still
/// requires the remaining admission. Those conditional paths remain open.
pub(crate) fn attack_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> AttackAction {
    let Some(actor) = sim.entities().get(actor_id) else {
        return AttackAction::None;
    };
    let Some(kind) = sim.interner().object_type(actor.type_ref(), rules) else {
        return AttackAction::None;
    };
    let human = sim.house_is_human_player(actor.owner());
    let armed = crate::sim::combat::combat_weapon::is_armed(actor, kind);
    let infiltrator = actor.category() == EntityCategory::Infantry && kind.infiltrate;
    let admitted = match target {
        TargetKind::Entity(target_id) => {
            human
                && (armed || infiltrator)
                && (forced
                    || (friendly_attack_is_admitted(sim, rules, actor_id, *target_id)
                        && terrain_target_is_legal(sim, rules, actor_id, *target_id)))
        }
        TargetKind::Cell(..) => human,
    };
    // TechnoClass::What_Action_OnObject's tail (`0x0070056C..0x007005E6`):
    // Select over an object the player may select that is not selected,
    // unless an armed player's object points at another House's, or at its
    // own loaner. The warp arm (`0x006FFF03..0x006FFF8A`) asks the same
    // without the target's own warp test (`0x007005CD`).
    let selects = |target_id: u64, warp_test: bool| {
        sim.entities().get(target_id).is_some_and(|object| {
            !(armed && human && object.owner() != actor.owner())
                && !(actor.is_mission_only() && actor.discovery.owned_by_current_house())
                && sim.object_is_selectable(target_id, rules)
                && !object.selection.is_selected()
                && !(warp_test && object.is_warped_out())
        })
    };
    // An object being warped out (vt+0x1D4) answers before the attack rung:
    // 0 on a Cell (`0x00700731`; warping in too, `0x00700743`), and over an
    // object the tail's Select, else 0. A berserk one answers 0 before that
    // (`0x006FFEC9`). The class tails below still run on the answer; none
    // turns its 0 into NoMove, as the object is not controllable (vt+0xA0,
    // `0x00700C40`), but a Building's Cell tail still answers its rally.
    let warping =
        actor.is_warped_out() || (actor.is_warping_in() && matches!(target, TargetKind::Cell(..)));
    let mut action = if warping {
        match *target {
            TargetKind::Entity(target_id)
                if !actor.berserk.is_active() && selects(target_id, false) =>
            {
                AttackAction::Select
            }
            _ => AttackAction::None,
        }
    } else if admitted
        && (sim.techno_player_controllable(actor_id, rules)
            || resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain(),
                overlay_registry,
                Some(target),
            ))
        && (forced || !shot_is_illegal(sim, rules, actor_id, target, overlay_registry))
        // Ares3.0 hooks700536/7008D4 ->1004A460/1004A430 leave
        // this rung for NoManualFire. Keep the original selection/Foot
        // fallback below; a Unit's rejected object attack becomes NoMove,
        // whose click still moves near the object. Do not gate CanFire or
        // received simulation missions: auto-targeting/scripts bypass UI.
        // RESIDUAL: Infantry's C4 and infiltration arms need this Attack
        // (`cmp ebp,5` at 0x0051E9F1 and 0x0051EE62), but `infantry_c4_action`
        // and `infiltrator_capture_orders` decide without it, so a
        // NoManualFire Tanya or Spy still plants C4 or infiltrates. Only Ares
        // builds read the key; retail rules never set it.
        && !kind.no_manual_fire()
    {
        AttackAction::Attack
    } else if let TargetKind::Entity(target_id) = *target
        && selects(target_id, true)
    {
        AttackAction::Select
    } else {
        AttackAction::None
    };
    let healing_unit = actor.category() == EntityCategory::Unit
        && human
        && crate::sim::combat::combat_weapon::weapon_damage_value(
            actor,
            kind,
            sim.entities(),
            rules,
            sim.interner(),
        ) < 0;
    // UnitClass::What_Action_OnCell 0x007404FD..0x00740522: a player's Unit
    // whose GetWeaponDamageValue(-1) (0x006F3970) is negative, an IFV with an
    // Engineer, turns Foot's Attack on a Cell into AttackSupport (0x3B), which
    // every later Unit rung returns unchanged (0x0074072B..0x0074075F).
    if action == AttackAction::Attack && healing_unit && matches!(target, TargetKind::Cell(..)) {
        action = AttackAction::AttackSupport;
    }
    // UnitClass::What_Action_OnObject's healer arm (0x0073FD7C)
    // asks Is_Ally_ByObject (0x004F9A90), which refuses a Terrain object's
    // null House, so the answer is 0 (0x00740115) and the Foot tail below
    // makes it NoMove. A Techno target's healer arm is
    // `Simulation::unit_repair_action`.
    if healing_unit
        && let TargetKind::Entity(target_id) = *target
        && sim.terrain_objects().get(target_id).is_some()
    {
        action = AttackAction::None;
    }
    // UnitClass::What_Action_OnObject 0x0073FE48: a Unit that is not IsActive
    // (0x007010D0; a Robot Tank without its control) turns Foot's Attack over
    // an object into Move where it may crush it (a crusher, and
    // Is_Crushable_By 0x005F6CD0), otherwise Select (0x0073FE3B).
    if action == AttackAction::Attack
        && actor.category() == EntityCategory::Unit
        && let TargetKind::Entity(target_id) = *target
        && !sim.techno_is_active(actor_id, rules)
    {
        action = if crate::sim::combat::veterancy::is_crusher(actor.veterancy.rank_kind(), kind)
            && sim.object_crushable_by_unit(actor_id, target_id, rules)
        {
            AttackAction::Move
        } else {
            AttackAction::Select
        };
    }
    // Unit74077F..7407E5 /740389..7403E8: the fully deployed
    // SimpleDeployer repeats SelectWeapon(NULL), then range. This returns
    // Action2, unlike the Techno control/range Action0 branch above.
    if action == AttackAction::Attack
        && actor.category() == EntityCategory::Unit
        && kind.is_simple_deployer
        && actor.is_fully_deployed()
        && (!kind.deploy_fire
            || !resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain(),
                overlay_registry,
                None,
            ))
    {
        action = AttackAction::NoMove;
    }
    // A player's Deployer= infantryman in Doing 27..30 (Type+0xEC8,
    // +0x6C4) keeps Attack only for a DeployFire= type (+0x6AC) whose
    // SelectWeapon(NULL) weapon reaches the target, else NoMove: over an
    // object CanFireAt (vt+0x3A8, Infantry51EB81..51EC11), on a Cell
    // IsCloseEnough3D (vt+0x164 0x6F7970, which asks CanFireAt on the Cell
    // under the Cell's centre; Infantry51F8BB..51F94E).
    if action == AttackAction::Attack
        && actor.category() == EntityCategory::Infantry
        && human
        && kind.deployer
        && actor.infantry_deploy_doing()
        && (!kind.deploy_fire
            || !resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain(),
                overlay_registry,
                None,
            ))
    {
        action = AttackAction::NoMove;
    }
    // Infantry51EE3B..51EEEF: a player's Infiltrate= infantryman's Attack is
    // Capture over a building it may infiltrate, which its producers ask
    // first (Simulation::infiltrator_building_action); otherwise it is 0
    // while he is unarmed (51F04E..51F05C).
    if action == AttackAction::Attack
        && infiltrator
        && human
        && !armed
        && let TargetKind::Entity(target_id) = *target
        && !sim.infiltrator_building_action(actor_id, target_id, rules)
    {
        action = AttackAction::None;
    }
    // Over an object, the Foot class tails turn 0 into another action: NoMove
    // for a player's Unit or Infantry it may control, and an Aircraft's own
    // Cell action under the object. The player's own object: its owner views.
    if action == AttackAction::None
        && let TargetKind::Entity(target_id) = *target
    {
        action =
            if aircraft_cell_attack_under_object(sim, rules, actor_id, target_id, overlay_registry)
            {
                AttackAction::Attack
            } else {
                match sim.foot_object_class_action(
                    actor.owner(),
                    actor_id,
                    target_id,
                    crate::sim::world::MoveAction::None,
                    rules,
                    overlay_registry,
                ) {
                    // Harvest is a Unit's Cell answer, never an object's.
                    crate::sim::world::MoveAction::None
                    | crate::sim::world::MoveAction::Harvest => AttackAction::None,
                    crate::sim::world::MoveAction::Move => AttackAction::Move,
                    crate::sim::world::MoveAction::NoMove => AttackAction::NoMove,
                }
            };
    }
    // Attack over an Immune= type (vt+0x88 +0x233) is NoMove at the end of
    // the Unit (74041B..74049A) and Aircraft (417F4A..417F63) object
    // actions. An Aircraft's ClickedAction refuses it (417C12); a Unit's
    // moves to the object or a Cell near it (Foot4D74E0,
    // Simulation::object_move_click). The Infantry tail (51F160..51F179)
    // follows its C4, weapon-cursor and Ivan arms, so it is the last arm of
    // infantry_attack_action.
    // RESIDUAL: the Unit's Attack reaches 740486 only when vt+0x310
    // (0x00700D10, 74041B..74042E) admits it, and is 0 otherwise. That test
    // refuses a Unit in a tube (Foot +0x684 >= 0) whose vt+0x4E4
    // (0x00736D50) answers yes, and a Unit whose vt+0x37C (0x00746C90)
    // answers yes: one under EMP (+0x504 > 0) or one counting down art
    // `DeathFrames=` (+0x6D8 != -1, set at 0x00737DBF only when UnitType
    // +0xE20 > 0). Not ported. EMP is UNREACHABLE: EMPulseClass::Apply
    // (0x004C5718) is reached only from the unreferenced constructor
    // 0x004C52B0. Neither `DeathFrames=` nor `DeployToFire=` appears in
    // retail ARTMD/RULESMD, and for a human's Unit CanDeployNow needs
    // DeployToFire= (its TickTank branch is computer-only), so both other
    // arms are DORMANT.
    if action == AttackAction::Attack
        && matches!(
            actor.category(),
            EntityCategory::Unit | EntityCategory::Aircraft
        )
        && target_is_immune(sim, rules, target)
    {
        action = AttackAction::NoMove;
    }
    if actor.category() == EntityCategory::Structure {
        // Building447366..4473C7 /4476F3..447749 inspect GetWeapon(0),
        // irrespective of SelectWeapon. These are class post-processing of
        // Attack, not a replacement range owner.
        if action == AttackAction::Attack
            && let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
                actor,
                kind,
                0,
                sim.entities(),
                rules,
                sim.interner(),
            )
            && (weapon
                .projectile
                .as_deref()
                .and_then(|name| rules.projectile(name))
                .is_some_and(|projectile| !projectile.ag)
                || kind.emp_pulse_cannon
                || actor.mission.current().known()
                    == Some(crate::sim::mission::MissionType::Selling))
        {
            action = AttackAction::None;
        }
        if action == AttackAction::Attack && target_is_immune(sim, rules, target) {
            // Object447512..447527; the Cell wrapper has no such target.
            action = AttackAction::NoMove;
        }
        if action == AttackAction::None
            && matches!(target, TargetKind::Cell(..))
            && kind.has_rally_line()
            && sim.house_is_human_player(actor.owner())
        {
            // Cell44774B..447768; existing factory destination owner commits it.
            action = AttackAction::Rally;
        }
    }
    action
}

/// Whether an Aircraft's 0 over the object `target_id` becomes Attack. The
/// Aircraft wrapper asks its own What_Action on the Cell under the object
/// (`0x00417D7C..0x00417DCB`, [`Simulation::object_coords_cell`]):
/// Aircraft `0x00417F80`, a human player's only, through Foot's shroud wrapper
/// (`0x004DDDE0`) to Techno `0x00700600`, whose unforced Attack rung
/// (`0x007008A6..0x007009F1`, [`Simulation::cell_attack_rung_admits`])
/// answers over a destroyable cliff or another house's Wall its `Wall=`
/// warhead may hit (retail Harrier and Black Eagle warheads have it), with
/// the forced shot's tail at the Cell. A shrouded Cell takes the Attack back
/// (`0x004DDE64..0x004DDEBE`). Aircraft `0x00417FB1` and `0x00417DCD` keep
/// it only with a weapon in slot 0 (vt+0x3F8). The Immune rung then
/// tests the object (`0x00417F4A`), and the click sends the Attack to Foot
/// `0x004D74E0` as an attack on the object ([`object_attack_click_command`]).
///
/// RESIDUAL: the slot-0 test stands on the rung's current weapon. Trigger: an
/// aircraft type with a Secondary but no Primary weapon (none in retail, whose
/// aircraft all carry a Primary). Effect: Attack where native answers 0.
///
/// [`Simulation::object_coords_cell`]: crate::sim::world::Simulation::object_coords_cell
/// [`Simulation::cell_attack_rung_admits`]: crate::sim::world::Simulation::cell_attack_rung_admits
fn aircraft_cell_attack_under_object(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let Some(actor) = sim
        .entities()
        .get(actor_id)
        .filter(|actor| actor.category() == EntityCategory::Aircraft)
    else {
        return false;
    };
    let Some(cell) = sim.object_coords_cell(target_id) else {
        return false;
    };
    sim.clicked_cell_is_shrouded(actor.owner(), cell) == Ok(false)
        && sim.cell_attack_rung_admits(actor_id, cell, rules, overlay_registry)
        && attack_action(
            sim,
            rules,
            actor_id,
            &TargetKind::Cell(cell.0, cell.1),
            true,
            overlay_registry,
        ) == AttackAction::Attack
}

/// Whether the cursor owner `actor_id`'s own What_Action decides a click or
/// hover over the friendly object `target_id` ([`attack_action`]), rather
/// than the friendly selection route. Every class's does. Over an own object
/// that is not selected the Techno tail answers Select (7) as the selection
/// route would. An armed human player's object over another house's object,
/// an ally's too, answers 0 there (`0x0070056C..0x0070059B`), as it does
/// over a selected object (`0x007005BF`). A Unit (`0x007403EB`) or an
/// Infantryman (`0x0051F05E..0x0051F08B`) it may control turns that 0 into
/// NoMove, an Aircraft into its Cell action (`0x00417D7C`), and a Building
/// keeps it (`0x0044750D`), the arrow. A lone selected Aircraft over itself
/// is Techno's Self (`0x007000B9..0x007000DF`), which an Aircraft carrying no
/// passengers turns into 0 (`0x00417DCD..0x00417DF4`), so it keeps the
/// selection route.
pub(super) fn object_action_answers_friendly(
    sim: &crate::sim::world::Simulation,
    selected: &[u64],
    actor_id: u64,
    target_id: u64,
) -> bool {
    sim.entities().get(actor_id).is_some_and(|actor| {
        actor.category() != EntityCategory::Aircraft || selected != [target_id]
    })
}

/// The target is an object whose type (vt+0x88) is `Immune=` (`+0x233`),
/// which every class's object action turns from Attack into NoMove: a
/// Techno's, or a Terrain object's TerrainType.
fn target_is_immune(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    target: &TargetKind,
) -> bool {
    let TargetKind::Entity(id) = *target else {
        return false;
    };
    if let Some(target) = sim.entities().get(id) {
        return sim
            .interner()
            .object_type(target.type_ref(), rules)
            .is_some_and(|target| target.immune);
    }
    sim.terrain_objects()
        .get(id)
        .and_then(|terrain| {
            rules.terrain_object_type_case_insensitive(sim.interner().resolve(terrain.type_ref()))
        })
        .is_some_and(|terrain| terrain.immune)
}

/// [`attack_action`] for an armed actor: the Ctrl branch (`forced`), and a
/// building's unforced one. An unarmed receiver answers `None` and keeps its
/// ordinary action paths. Cell7008BD reads GetCurrentWeapon, which also
/// resolves an occupied building's firing occupant; Object700468 asks
/// virtual IsArmed.
pub(crate) fn armed_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<AttackAction> {
    let actor = sim.entities().get(actor_id)?;
    let object = sim.interner().object_type(actor.type_ref(), rules)?;
    let armed = match target {
        TargetKind::Cell(..) => crate::sim::combat::combat_weapon::current_weapon(
            actor,
            object,
            sim.entities(),
            rules,
            sim.interner(),
        )
        .is_some(),
        TargetKind::Entity(_) => crate::sim::combat::combat_weapon::is_armed(actor, object),
    };
    armed.then(|| attack_action(sim, rules, actor_id, target, forced, overlay_registry))
}

/// Display4AB51B asks CanFireAtTarget6F7780: ONE SelectWeapon result, not
/// either slot's longest range. The query domain is private to input, across
/// target flight, fire source and InRange, so nested misses never stamp the
/// simulation's canonical Dummy.
pub(crate) fn resolved_unit_in_range(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target: &TargetKind,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    selection_target: Option<&TargetKind>,
) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    if let Some(terrain) = terrain {
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        return crate::sim::combat::can_fire_at_target_in_query(
            sim,
            rules,
            sim.interner(),
            actor_id,
            target,
            &cells,
            Some(sim.house_alliances()),
            &crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: sim.overlay_grid(),
                overlay_registry,
                alliances: Some(sim.house_alliances()),
            },
            selection_target,
        );
    }
    // A world without resolved terrain takes the shared mapless answer;
    // production maps take the native owner above.
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(object) = sim.interner().object_type(actor.type_ref(), rules) else {
        return false;
    };
    let Some((index, _)) = crate::sim::combat::combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        object,
        &crate::sim::combat::combat_weapon::attacker_facts(actor, object),
        selection_target,
        sim,
        sim.interner(),
        None,
        Some(sim.house_alliances()),
    ) else {
        return false;
    };
    let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
        actor,
        object,
        index,
        sim.entities(),
        rules,
        sim.interner(),
    ) else {
        return false;
    };
    let techno = match *target {
        TargetKind::Entity(id) => sim.entities().get(id),
        TargetKind::Cell(..) => None,
    };
    let target = match techno {
        Some(entity) => crate::sim::combat::MaplessTarget::Techno(entity),
        None => match crate::sim::movement::ground_pose::target_get_coords(*target, sim, None) {
            Some(point) => crate::sim::combat::MaplessTarget::Point(point),
            None => return false,
        },
    };
    crate::sim::combat::mapless_in_range(actor, target, weapon, rules, sim.interner())
}

/// Resolved attack receiver batch. These are ephemeral input decisions;
/// source state and mutation authority remain with Simulation.
struct AttackDispatch {
    receivers: std::collections::BTreeSet<u64>,
    commands: Vec<Command>,
    consumed: bool,
    select_fallback: bool,
}

/// Unit73FD7C's post-processing precedes the generic friendly selection and
/// Ctrl attack paths. Selection/keyboard and mission promotion stay with their
/// existing owners. Shift and one selected object's self action precede
/// the native repair arm. Saved caller/output controls: bunker_attack_action
/// repair_rows; Unit738890 converts Repair29 to the ordinary Attack mission.
/// The self gate and DetermineAction read `current_objects`, the whole
/// selection; only `receivers` (`click_order_receivers`) get orders.
fn unit_repair_branch_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    receivers: &[u64],
    target: &HoverTargetKindWithId,
    issuing_house: InternedId,
    modifier: OrderModifier,
    order_mode: OrderMode,
    force_move_key: bool,
) -> AttackDispatch {
    use crate::sim::world::UnitRepairAction;
    let mut output = AttackDispatch {
        receivers: Default::default(),
        commands: Vec::new(),
        consumed: false,
        select_fallback: false,
    };
    if modifier == OrderModifier::Shift
        || current_objects == [target.stable_id]
        || target.kind == HoverTargetKind::HiddenEnemy
    {
        return output;
    }
    let friendly = matches!(
        target.kind,
        HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
    );
    // DetermineAction's one chosen object owns a normal friendly click. A
    // repair-capable member must not turn the whole group's Select into Repair.
    if friendly
        && matches!(modifier, OrderModifier::Normal | OrderModifier::AttackMove)
        && !cursor_owner(sim, rules, current_objects, target.stable_id).is_some_and(|id| {
            sim.unit_repair_action(id, target.stable_id, issuing_house, rules)
                == Some(UnitRepairAction::Repair)
        })
    {
        return output;
    }
    for &id in receivers {
        let Some(action) = sim.unit_repair_action(id, target.stable_id, issuing_house, rules)
        else {
            continue;
        };
        output.receivers.insert(id);
        let payload = match action {
            UnitRepairAction::Select => {
                output.select_fallback = true;
                continue;
            }
            UnitRepairAction::Repair => {
                output.consumed = true;
                // Repair29 replaced Ctrl+Alt's base action before the mission
                // producer; it sends Attack1, not the legacy Guard adapter.
                let attack_modifier = if modifier == OrderModifier::GuardArea {
                    OrderModifier::Normal
                } else {
                    modifier
                };
                object_click_payload(
                    order_mode,
                    attack_modifier,
                    entity_can_attack_move(sim, Some(rules), id),
                    id,
                    target.stable_id,
                )
            }
            UnitRepairAction::Move => {
                output.consumed = true;
                let Some(destination) = crate::app::input::commands::object_move_destination(
                    sim,
                    rules,
                    issuing_house,
                    id,
                    target.stable_id,
                    crate::sim::world::MoveAction::NoMove,
                    force_move_key,
                ) else {
                    continue;
                };
                move_click_command(sim, rules, order_mode, id, destination)
            }
        };
        output.commands.push(payload);
    }
    output
}

fn attack_branch_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    attack_target: TargetKind,
    force_fire: bool,
    order_mode: OrderMode,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> AttackDispatch {
    let mut consumed_attack_action = false;
    let mut queued = Vec::new();
    let mut attack_receivers = std::collections::BTreeSet::new();
    let mut select_fallback = false;
    // Over a Terrain object every receiver answers its own What_Action on
    // the object, armed or not: there is no other object route to take.
    let terrain =
        matches!(attack_target, TargetKind::Entity(id) if sim.terrain_objects().get(id).is_some());
    for &id in selected_ids {
        let is_structure = sim
            .entities()
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure);
        if !(force_fire || is_structure || terrain) {
            continue;
        }
        let action = if terrain {
            Some(attack_action(
                sim,
                rules,
                id,
                &attack_target,
                force_fire,
                Some(overlay_registry),
            ))
        } else {
            armed_action(
                sim,
                rules,
                id,
                &attack_target,
                force_fire,
                Some(overlay_registry),
            )
        };
        let Some(action) = action else {
            continue;
        };
        if action == AttackAction::Rally {
            continue;
        }
        // NoIvanBomb (`0x0051EB24`) is only reached from an Attack answer
        // (`0x0051EA6F`, `0x0051EB33`); it orders nothing.
        if action == AttackAction::Attack
            && let TargetKind::Entity(target_id) = attack_target
            && ivan_cannot_bomb(sim, rules, id, target_id)
        {
            attack_receivers.insert(id);
            consumed_attack_action = true;
            continue;
        }
        if action == AttackAction::None && matches!(attack_target, TargetKind::Cell(..)) {
            // Rejection of the attack rung still reaches Techno's Cell
            // movement tail700AB7..700C08. Leave this receiver to the
            // existing cell-move/harvest path below, including under Ctrl.
            continue;
        }
        attack_receivers.insert(id);
        // Unit 0x738890, Infantry 0x51F190 and Aircraft 0x417BD0
        // ClickedAction_Object hand an object click's 1 or 2 to Foot
        // 0x4D74E0, which moves near the object, as the unforced click does
        // (`object_attack_click_command`).
        if let Some(move_action) = match action {
            AttackAction::NoMove => Some(crate::sim::world::MoveAction::NoMove),
            AttackAction::Move => Some(crate::sim::world::MoveAction::Move),
            _ => None,
        } && !is_structure
            && let TargetKind::Entity(target_id) = attack_target
        {
            consumed_attack_action = true;
            queued.extend(move_near_object(
                sim,
                rules,
                order_mode,
                id,
                target_id,
                move_action,
            ));
            continue;
        }
        match action {
            AttackAction::None
            | AttackAction::NoMove
            | AttackAction::Move
            | AttackAction::AttackSupport => consumed_attack_action = true,
            AttackAction::Rally => unreachable!("rally remains with its existing receiver"),
            AttackAction::Select => select_fallback = true,
            AttackAction::Attack => {
                consumed_attack_action = true;
                let payload = match attack_target {
                    // Under the attack-move chord or mode ClickedMission
                    // promotes this Attack whatever its target
                    // (`0x006FFC09..0x006FFC1E`): an attack-move with the
                    // Cell as its target and no destination, the tokens Foot
                    // ClickedAction_Cell sends (`0x004D80E9..0x004D80FF`).
                    TargetKind::Cell(target_rx, target_ry)
                        if order_mode == OrderMode::AttackMove
                            && entity_can_attack_move(sim, Some(rules), id) =>
                    {
                        Command::AttackMove {
                            entity_id: id,
                            target: Some(TargetKind::Cell(target_rx, target_ry)),
                            destination: None,
                        }
                    }
                    TargetKind::Cell(target_rx, target_ry) => Command::ForceAttackCell {
                        attacker_id: id,
                        target_rx,
                        target_ry,
                    },
                    TargetKind::Entity(target_id) => {
                        match infantry_attack_action(sim, rules, id, target_id) {
                            Some(InfantryAttackAction::NoMove) => {
                                let Some(command) = move_near_object(
                                    sim,
                                    rules,
                                    order_mode,
                                    id,
                                    target_id,
                                    crate::sim::world::MoveAction::NoMove,
                                ) else {
                                    continue;
                                };
                                command
                            }
                            Some(InfantryAttackAction::Infiltrate) => Command::CaptureBuilding {
                                engineer_id: id,
                                target_building_id: target_id,
                            },
                            Some(InfantryAttackAction::Sabotage) => Command::PlantC4 {
                                attacker_id: id,
                                target_building_id: target_id,
                            },
                            Some(InfantryAttackAction::Select) => {
                                select_fallback = true;
                                continue;
                            }
                            _ => object_click_payload(
                                order_mode,
                                OrderModifier::Normal,
                                entity_can_attack_move(sim, Some(rules), id),
                                id,
                                target_id,
                            ),
                        }
                    }
                };
                queued.push(payload);
            }
        }
    }
    AttackDispatch {
        receivers: attack_receivers,
        commands: queued,
        consumed: consumed_attack_action,
        select_fallback,
    }
}

/// Foot `0x004D74E0` case 1 or 2 (`action`) for a forced object click: a
/// Move near the object.
fn move_near_object(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    id: u64,
    target_id: u64,
    action: crate::sim::world::MoveAction,
) -> Option<Command> {
    let owner = sim.entities().get(id)?.owner();
    let destination = crate::app::input::commands::object_move_destination(
        sim, rules, owner, id, target_id, action, false,
    )?;
    Some(move_click_command(sim, rules, order_mode, id, destination))
}

/// INI voice key for the mission a command commits.
///
/// `TechnoClass::Player_Send_Command` dispatches the order-ack line by mission
/// number: Harvest, Attack, Move (shared with attack-move), Enter, Capture and
/// Unload each have their own slot, and every other mission falls to a default
/// branch that draws a random entry from the type's `VoiceSpecialAttack` list.
///
/// Capture has its **own** slot, and it is wired here now. The dispatcher at
/// `0x00708DC0` reads the type's `VoiceCapture=` (`TechnoTypeClass+0x55C`) and
/// queues it (`0x00708DE8 CALL [EDI+0x354]` = `TechnoClass::Queue_Voice @
/// 0x00708D90`); only when that slot is the `-1` sentinel (`0x00708DCB CMP
/// [EAX+0x55C],-1`) does it fall through to the Enter-voice method
/// (`0x00708DF5 CALL [EDX+0x358]` = `0x00709020`, which reads
/// `VoiceEnter=` at `+0x558` and itself falls through to `[+0x368]` when that
/// is absent). Every stock engineer ships `VoiceCapture=`, so before this arm
/// existed every engineer capture spoke the move line (Allied `EngAllMove`
/// instead of `EngAllAttackCommand`, Soviet `EngSovMove` instead of
/// `EngSovAttackCommand`; Yuri's two keys hold the same sound, so Yuri was
/// unaffected).
///
/// A building pack-up speaks at Selling stage0, so UndeployBuilding must not
/// queue a second input acknowledgement here.
///
/// The self-click halt is silent in retail as well: it builds a detonate event
/// directly instead of sending a mission, so the voice dispatch never runs.
fn order_voice_key(command: &Command) -> Option<&'static str> {
    match command {
        Command::Move { .. } | Command::MoveToObject { .. } | Command::AttackMove { .. } => {
            Some("VoiceMove")
        }
        Command::Attack { .. } | Command::ForceAttackCell { .. } => Some("VoiceAttack"),
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Harvest,
            ..
        } => Some("VoiceHarvest"),
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Enter,
            ..
        } => Some("VoiceEnter"),
        // Eaten9 takes QueueMegaMission6FFD42's default voice branch.
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Eaten,
            ..
        } => Some("VoiceSpecialAttack"),
        Command::CaptureBuilding { .. } => Some("VoiceCapture"),
        // Sabotage and area guard have no dedicated slot, so retail takes the
        // default branch and speaks a VoiceSpecialAttack line.
        Command::PlantC4 { .. } | Command::Guard { .. } => Some("VoiceSpecialAttack"),
        // A self-click's deploy, unload or deploy-fire is the Unload mission
        // (`FootClass::ClickedAction_Object @ 0x004D74E0` actions 4/0x34/0x38;
        // `AircraftClass @ 0x00417BD0` action 4; a Building's garrison or
        // absorber arm `0x004434F2..0x00443562`), whose ClickedMission arm
        // speaks VoiceDeploy (vt+0x36C, `0x006FFD3A`). EjectBunker and the
        // factory primary toggle are not mapped.
        Command::DeployMcv { .. }
        | Command::ToggleInfantryDeploy { .. }
        | Command::UnloadPassengers { .. } => Some("VoiceDeploy"),
        _ => None,
    }
}

/// The entity a command acts on, when the command targets exactly one.
///
/// Used to find which queued order belongs to the speaking object.
fn command_actor_id(command: &Command) -> Option<u64> {
    match command {
        Command::Move { entity_id, .. }
        | Command::MegaMission { entity_id, .. }
        | Command::MoveToObject { entity_id, .. }
        | Command::AttackMove { entity_id, .. }
        | Command::Guard { entity_id, .. }
        | Command::ToggleInfantryDeploy { entity_id }
        | Command::DeployMcv { entity_id }
        | Command::UndeployBuilding { entity_id }
        | Command::ArchiveObject { entity_id, .. }
        | Command::Stop { entity_id } => Some(*entity_id),
        Command::Attack { attacker_id, .. }
        | Command::ForceAttackCell { attacker_id, .. }
        | Command::PlantC4 { attacker_id, .. } => Some(*attacker_id),
        Command::CaptureBuilding { engineer_id, .. } => Some(*engineer_id),
        Command::UnloadPassengers { transport_id } => Some(*transport_id),
        Command::EjectBunker { bunker_id } => Some(*bunker_id),
        Command::SetRally { producer_ids, .. } if producer_ids.len() == 1 => Some(producer_ids[0]),
        _ => None,
    }
}

/// Selection4AE750 object loop4AE829..4AE85A and ground loop4AE95B..4AE990
/// resolve each object's action in selection-array order.
/// Capability batches may remove handled actors, but must not reorder their
/// single-object events, including each prepared factory rally. Legacy aggregate
/// commands cannot be placed at one actor's index and keep dispatcher order.
/// This index derives from input selection. A MoveFlash belongs to the first
/// receiver and stays ahead of its order ([`push_move_flash`]).
fn restore_selection_dispatch_order(queued: &mut [CommandEnvelope], selected: &[u64]) {
    if queued.iter().any(|envelope| {
        command_actor_id(&envelope.payload).is_none()
            && !matches!(envelope.payload, Command::MoveFlash { .. })
    }) {
        return;
    }
    let positions: std::collections::HashMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(index, &id)| (id, index))
        .collect();
    queued.sort_by_key(|envelope| match envelope.payload {
        Command::MoveFlash { .. } => (0, false),
        _ => (
            command_actor_id(&envelope.payload)
                .and_then(|id| positions.get(&id).copied())
                .unwrap_or(usize::MAX),
            true,
        ),
    });
}

/// Play the single order-ack line for a batch of freshly queued orders.
///
/// Retail's dispatch loop clears the voice-enable flag at the end of *every*
/// iteration and restores it once after the loop, so exactly one object speaks —
/// the first entry of the selection array — and it speaks the line for the
/// mission *it* resolved, not an order-wide line. If that first object resolved
/// no order (its action mapped to a cursor-only code) nothing is spoken.
fn emit_resolved_order_voice(state: &mut AppState, speaker_id: u64, queued: &[CommandEnvelope]) {
    let Some(envelope) = queued
        .iter()
        .find(|env| command_actor_id(&env.payload) == Some(speaker_id))
    else {
        return;
    };
    let Some(voice_field) = order_voice_key(&envelope.payload) else {
        return;
    };
    //6FFCEA..6FFCEF passes the committed Attack argument to VoiceAttack,
    // before event encoding. A Move destination and the actor's old TarCom
    // are different owners; neither supplies this argument.
    let target = match &envelope.payload {
        Command::Attack { target_id, .. } => Some(TargetKind::Entity(*target_id)),
        Command::ForceAttackCell {
            target_rx,
            target_ry,
            ..
        } => Some(TargetKind::Cell(*target_rx, *target_ry)),
        _ => None,
    };
    emit_entity_order_voice(state, speaker_id, voice_field, target);
}

/// Play one entity's voice line for the given `Voice*` INI key.
///
/// The superseded app-layer order-voice helper always spoke the lowest-`stable_id` selected
/// entity; retail speaks the object that resolved the order, so order resolution
/// needs to name the speaker explicitly.
fn emit_entity_order_voice(
    state: &mut AppState,
    speaker_id: u64,
    voice_field: &str,
    target: Option<TargetKind>,
) {
    if voice_field == "VoiceDeploy" {
        if let Some(runtime) = state.match_state.sim_runtime.as_mut()
            && let Some(event) = crate::app::match_runtime::sound_dispatch::deploy_order_voice_event(
                &mut runtime.simulation,
                &runtime.resources.rules,
                speaker_id,
                state.match_state.input.selection_voice_enabled,
            )
        {
            state.match_state.match_audio.sound_events.push(event);
        }
        return;
    }
    if voice_field == "VoiceSpecialAttack" {
        if let Some(runtime) = state.match_state.sim_runtime.as_mut()
            && let Some(event) =
                crate::app::match_runtime::sound_dispatch::default_order_voice_event(
                    &mut runtime.simulation,
                    &runtime.resources.rules,
                    speaker_id,
                    state.match_state.input.selection_voice_enabled,
                )
        {
            state.match_state.match_audio.sound_events.push(event);
        }
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let Some(id) = runtime.simulation.order_voice_request(
        &runtime.resources.rules,
        speaker_id,
        voice_field,
        target,
        state.match_state.input.selection_voice_enabled,
    ) else {
        return;
    };

    let event = if voice_field == "VoiceAttack" {
        crate::audio::events::GameSoundEvent::UnitAttackOrder {
            speaker_id,
            sound_id: id.to_owned(),
        }
    } else {
        crate::audio::events::GameSoundEvent::UnitMoveOrder {
            speaker_id,
            sound_id: id.to_owned(),
        }
    };
    state.match_state.match_audio.sound_events.push(event);
}

/// Commit a resolved context dispatch: one voice line, the queue, then its timer.
///
/// Every exit from order resolution goes through here so the single-speaker rule
/// holds for the capability branches (garrison, C4, capture, depot, bunker,
/// deploy) as well as for the move/attack tail.
/// Display4ABFA9 calls Selection4AE750, then unconditionally starts the global
/// action-line timer at4ABFAE, including Move1/NoMove2 with no admitted event.
/// Both take the common4AC294 exit and RET4AC2A7, without visiting Select7.
/// The return value means that dispatch consumed the click, not that it
/// admitted an event; an empty batch must not fall through to selection clear.
/// Friendly click-selection and an empty selection never enter this boundary.
/// Evidence: tools/procedural_drawing_oracle/action_lines.
fn finish_order(
    state: &mut AppState,
    queued: Vec<CommandEnvelope>,
    speaker_id: Option<u64>,
) -> bool {
    let admitted = if state.match_state.input.planning.active() {
        let mut queued = queued;
        let selected = selected_stable_ids_in_order(
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.rules(),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        restore_selection_dispatch_order(&mut queued, &selected);
        crate::app::input::planning_mode::queue_orders(state, &selected, queued)
    } else {
        let mut queued = queued;
        restore_selection_dispatch_order(
            &mut queued,
            &selected_stable_ids_in_order(
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                state.rules(),
                &state.match_state.input.selection_order,
                state.match_state.input.selection_order_pending,
            ),
        );
        // The local acknowledgement reads the resolved input order before
        // encoding. Only the native record enters the gameplay queue.
        let (encoded, feedback): (Vec<_>, Vec<_>) = if let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        {
            queued
                .into_iter()
                .filter_map(|envelope| {
                    crate::app::input::commands::encode_ordinary_local_command(
                        sim,
                        envelope.clone(),
                    )
                    .map(|encoded| (encoded, envelope))
                })
                .unzip()
        } else {
            (queued.clone(), queued)
        };
        if !encoded.is_empty() {
            if let Some(speaker_id) = speaker_id {
                emit_resolved_order_voice(state, speaker_id, &feedback);
            }
            if let Some(sim) = state
                .match_state
                .sim_runtime
                .as_mut()
                .map(|rt| &mut rt.simulation)
            {
                sim.issue_local_commands(encoded);
            }
        }
        true
    };
    //4ABF94's admitted planning route joins the same70D150 suffix at
    //4ABFAE;4ABF8E's refusal exits4AC294 without restarting this timer.
    if admitted
        && let Some(frame) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| rt.simulation.session().binary_frame())
    {
        state
            .match_state
            .match_presentation
            .target_lines
            .start_timer(frame);
    }
    true
}

/// Does this object accept an attack-move order? Its `Can_Attack_Move`
/// ([`crate::sim::world::Simulation::techno_can_attack_move`]).
fn entity_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    stable_id: u64,
) -> bool {
    rules.is_some_and(|rules| sim.techno_can_attack_move(stable_id, rules))
}

/// Every selected object has to accept an attack-move order for the chord to fire.
///
/// The chord test walks the *whole* current selection — buildings and aircraft
/// included, not just the mobiles that would receive the order — and fails
/// outright the moment one member answers no.
pub(super) fn selection_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selected_ids: &[u64],
) -> bool {
    if selected_ids.is_empty() {
        return false;
    }
    selected_ids
        .iter()
        .all(|&sid| entity_can_attack_move(sim, rules, sid))
}

/// What Techno What_Action_OnObject's key rungs answer
/// (`0x00700006..0x00700217`), for the cursor and the click alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectKeyAction {
    /// Shift's ToggleSelect (8, `0x00700107`). Over a Terrain object, which
    /// cannot be selected, the release selects nothing and keeps the
    /// selection.
    ToggleSelect,
    /// Ctrl+Alt's GuardArea (0x1A, `0x007001DD`, [`guard_area_admitted`]).
    GuardArea,
    /// Alt's Move (`0x007001E9..0x00700217`), in any chord. Foot
    /// `0x004D74E0` moves the receiver to the object or near it.
    Move,
}

/// The key rungs of `actor_id`'s What_Action over an object: `target` is a
/// Techno, or None for a Terrain object, which is no ally for the guard rung
/// (`0x004F9A90` reads its vt+0x3C, TerrainClass's null house). They follow
/// the berserk exit and warp arm (`0x006FFEC9`, `0x006FFF03`). Shift without
/// Ctrl answers ToggleSelect for a player's object that is not mission-only
/// (`0x007000EB..0x00700107`); Alt, Ctrl+Alt and Ctrl+Shift+Alt answer Move
/// for a player's controllable object (vt+0xA0) once the guard rung refused.
/// None leaves the later rungs (for a Terrain object, plain, Ctrl and
/// Ctrl+Shift: [`crate::app::input::cursor::terrain_object_feedback`]). A
/// selected building's Move over a Terrain object then asks its own factory
/// rungs (`0x004473F4..0x0044750B`, [`building_object_orders`]).
pub(super) fn object_key_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: Option<u64>,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<ObjectKeyAction> {
    let actor = sim.entities().get(actor_id)?;
    if actor.berserk.is_active() || actor.is_warped_out() {
        return None;
    }
    let human = sim.house_is_human_player(actor.owner());
    if modifier == OrderModifier::Shift {
        return (human && !actor.is_mission_only()).then_some(ObjectKeyAction::ToggleSelect);
    }
    if modifier == OrderModifier::GuardArea
        && target.is_some()
        && guard_area_admitted(sim, rules, actor_id, target)
    {
        return Some(ObjectKeyAction::GuardArea);
    }
    let alt = matches!(
        modifier,
        OrderModifier::ForceMove | OrderModifier::GuardArea
    ) || (modifier == OrderModifier::AttackMove && force_move_key);
    (alt && human && sim.techno_player_controllable(actor_id, rules))
        .then_some(ObjectKeyAction::Move)
}

/// Whether What_Action's later rungs over a Terrain object see Ctrl: it sets
/// the forced flag even with Alt held (`0x00700048..0x00700072`), so Ctrl
/// and Ctrl+Alt force, while Ctrl+Shift clears it for the Shift test
/// (`0x007000AC..0x007000B4`).
pub(super) fn terrain_forced(modifier: OrderModifier) -> bool {
    matches!(
        modifier,
        OrderModifier::ForceFire | OrderModifier::GuardArea
    )
}

/// The object under the click as [`object_key_action`] takes it: a Techno
/// the player may see (`hover`; an enemy the player cannot see is no object
/// to click) or a Terrain object, with the Techno's id as its target.
fn object_key_target(
    object: HoverObject,
    hover: Option<&HoverTargetKindWithId>,
) -> Option<(u64, Option<u64>)> {
    match object {
        HoverObject::Terrain(id) => Some((id, None)),
        HoverObject::Techno(_) => hover
            .filter(|target| target.kind != HoverTargetKind::HiddenEnemy)
            .map(|target| (target.stable_id, Some(target.stable_id))),
    }
}

/// Techno object700191..700217 / Cell700830..700894. The keyboard chord
/// alone is not a Guard action. Object targets must be allied and the actor
/// no Aircraft (RTTI 2, 7001B4..7001BA); bare ground requires IsActive7010D0. Waypoint/patrol input is not exposed by VERA.
/// A berserk object answers 0 before the rung (object `0x006FFEC9`, Cell
/// `0x00700609`). Shared by the hover and click producers; no simulation
/// state is changed.
pub(super) fn guard_area_admitted(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: Option<u64>,
) -> bool {
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    if actor.berserk.is_active()
        || !sim.house_is_human_player(actor.owner())
        || !sim.techno_player_controllable(actor_id, rules)
    {
        return false;
    }
    match target_id {
        Some(target_id) => {
            actor_id != target_id
                && actor.category() != EntityCategory::Aircraft
                && sim.entities().get(target_id).is_some_and(|target| {
                    crate::map::houses::is_allied_with(
                        sim.house_alliances(),
                        sim.interner().resolve(actor.owner()),
                        sim.interner().resolve(target.owner()),
                    )
                })
        }
        None => sim.techno_is_active(actor_id, rules),
    }
}

/// Techno What_Action on a Cell's Ctrl+Alt rung (`0x00700815..0x007008A3`):
/// whether `actor_id` answers GuardArea (0x1A) on `cell`, ahead of the move
/// rungs, as `viewer` sees it. The class wrappers then keep it, except:
/// - over a shrouded Cell centre a Foot's wrapper turns it into Move or
///   NoMove (`0x004DDE64..0x004DDEBE`, the bridge-raised test of
///   [`Simulation::clicked_cell_is_shrouded`]), as the move rungs answer there;
/// - a Building with a rally line turns it into Move (`0x0044774B`); any
///   other Building's wrapper (`0x00447540`) keeps it, shroud or not.
///
/// The hover, the Foot click and the Building click all ask this.
///
/// [`Simulation::clicked_cell_is_shrouded`]: crate::sim::world::Simulation::clicked_cell_is_shrouded
pub(super) fn cell_guard_area_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    viewer: Option<InternedId>,
    cell: (u16, u16),
) -> bool {
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let rewritten = if actor.category() == EntityCategory::Structure {
        sim.interner()
            .object_type(actor.type_ref(), rules)
            .is_some_and(|object| object.has_rally_line())
    } else {
        viewer.is_some_and(|viewer| sim.clicked_cell_is_shrouded(viewer, cell) == Ok(true))
    };
    !rewritten && guard_area_admitted(sim, rules, actor_id, None)
}

/// The order one receiver commits for a Move click's destination.
/// ClickedMission (`0x006FFBE0`) promotes Move to attack-move (mission 29)
/// while the attack-move chord or toggle is on (`0x00731BF0`) and the
/// receiver can attack-move (vt+0x4C0), and the event keeps the click's
/// destination, a Cell or the object (`0x006FFDA5..0x006FFDE6`). The
/// promotion is per object: a unit whose type refuses it keeps the plain Move
/// it committed, even when the rest of the group attack-moves.
fn move_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    entity_id: u64,
    destination: TargetKind,
) -> Command {
    let attack_move =
        order_mode == OrderMode::AttackMove && entity_can_attack_move(sim, Some(rules), entity_id);
    match destination {
        _ if attack_move => Command::AttackMove {
            entity_id,
            target: None,
            destination: Some(destination),
        },
        TargetKind::Cell(target_rx, target_ry) => Command::Move {
            entity_id,
            target_rx,
            target_ry,
        },
        TargetKind::Entity(target_id) => Command::MoveToObject {
            entity_id,
            target_id,
        },
    }
}

/// The command one selected unit commits for a tactical click on an object.
///
/// ClickedMission (`0x006FFBE0`) promotes the committed **Attack** to
/// attack-move as readily as a committed Move, gated per object on its own
/// type predicate, and the event keeps the clicked object as its target
/// (`0x004D76B1..0x004D76C1`). `Assign_Target_Command` stores it as MegaTarget
/// and answers Attack, so the unit attacks it as an attack-mover; a member
/// whose type refuses attack-move commits the plain attack. A forced
/// Attack is promoted too: in the attack-move mode (`0x00B0FE58`, set by
/// `0x00731AF0`) `Is_Attack_Move_Chord_Active @ 0x00731BF0` passes before
/// it reads any key, so Ctrl there attack-moves onto the object.
pub(crate) fn object_click_payload(
    order_mode: OrderMode,
    modifier: OrderModifier,
    can_attack_move: bool,
    attacker_id: u64,
    target_id: u64,
) -> Command {
    if modifier == OrderModifier::GuardArea {
        return Command::Guard {
            entity_id: attacker_id,
            target: Some(TargetKind::Entity(target_id)),
        };
    }
    match order_mode {
        OrderMode::AttackMove if can_attack_move => Command::AttackMove {
            entity_id: attacker_id,
            target: Some(TargetKind::Entity(target_id)),
            destination: None,
        },
        _ => Command::Attack {
            attacker_id,
            target_id,
        },
    }
}

/// The order one receiver commits for a click on another house's object:
/// its own What_Action there, which the selection's click loop hands to its
/// ClickedAction (`0x004AE829..0x004AE844`).
///
/// Unforced, that is [`attack_action`]'s answer. NoMove moves the receiver
/// to the object or a Cell near it (Foot `0x004D74E0`), as an attack-move
/// under the chord ([`Simulation::object_move_click`],
/// [`move_click_command`]); Select and 0 order nothing.
///
/// Attack orders [`object_click_payload`]. What_Action decides Attack before
/// ClickedMission promotes it to attack-move, so an unforced shot ILLEGAL to
/// the receiver leaves no Attack to promote: its tail answers 0 or Select
/// there ([`attack_action`]). A Crazy Ivan's Attack over an object he cannot
/// bomb is NoIvanBomb (`0x0051EB24`), which orders nothing.
///
/// Ctrl bypasses friendly/ILLEGAL rejection in the existing action owner,
/// but still requires armed or Infiltrate admission (`700444..700474`).
/// Its Select/NoMove tail remains the ordinary selection/movement path.
/// Original controls: allied_object_action.json unarmed_object_controls.
///
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
fn object_attack_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    issuing_house: InternedId,
    order_mode: OrderMode,
    modifier: OrderModifier,
    force_move_key: bool,
    attacker_id: u64,
    target_id: u64,
) -> Option<Command> {
    let move_near = |action| {
        let destination = crate::app::input::commands::object_move_destination(
            sim,
            rules,
            issuing_house,
            attacker_id,
            target_id,
            action,
            force_move_key,
        )?;
        Some(move_click_command(
            sim,
            rules,
            order_mode,
            attacker_id,
            destination,
        ))
    };
    if modifier == OrderModifier::GuardArea {
        return if guard_area_admitted(sim, rules, attacker_id, Some(target_id)) {
            Some(Command::Guard {
                entity_id: attacker_id,
                target: Some(TargetKind::Entity(target_id)),
            })
        } else {
            move_near(crate::sim::world::MoveAction::Move)
        };
    }
    // Guard was classified above with hover. Every other receiver asks its
    // own action, including Ctrl; forcing does not invent Attack admission.
    match attack_action(
        sim,
        rules,
        attacker_id,
        &TargetKind::Entity(target_id),
        modifier == OrderModifier::ForceFire,
        Some(overlay_registry),
    ) {
        // NoIvanBomb (`0x0051EB24`) orders nothing.
        AttackAction::Attack if ivan_cannot_bomb(sim, rules, attacker_id, target_id) => {
            return None;
        }
        AttackAction::Attack => {}
        AttackAction::NoMove => {
            return move_near(crate::sim::world::MoveAction::NoMove);
        }
        AttackAction::Move => return move_near(crate::sim::world::MoveAction::Move),
        AttackAction::Select
        | AttackAction::None
        | AttackAction::Rally
        | AttackAction::AttackSupport => return None,
    }
    match infantry_attack_action(sim, rules, attacker_id, target_id) {
        Some(InfantryAttackAction::NoMove) => {
            return move_near(crate::sim::world::MoveAction::NoMove);
        }
        Some(InfantryAttackAction::Select) => return None,
        Some(InfantryAttackAction::Infiltrate) => {
            return Some(Command::CaptureBuilding {
                engineer_id: attacker_id,
                target_building_id: target_id,
            });
        }
        Some(InfantryAttackAction::Sabotage) => {
            return Some(Command::PlantC4 {
                attacker_id,
                target_building_id: target_id,
            });
        }
        _ => {}
    }
    Some(object_click_payload(
        order_mode,
        modifier,
        entity_can_attack_move(sim, Some(rules), attacker_id),
        attacker_id,
        target_id,
    ))
}

/// What `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` does with the action
/// DetermineAction answered ([`left_release`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftRelease {
    /// Every selected object gets the action (`0x004ABE88`); one whose
    /// ClickedAction refuses sends nothing, and the action-line timer still
    /// starts (`0x004ABFA9`).
    Dispatch,
    /// Nothing (`0x004AC294`).
    Nothing,
    /// The ordinary selection click on the object under the cursor
    /// (`0x004ABE18`).
    Select,
}

/// The part of DetermineAction's action that [`left_release`] tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseAction {
    Select,
    Zero,
    Other,
}

impl From<AttackAction> for ReleaseAction {
    fn from(action: AttackAction) -> Self {
        match action {
            AttackAction::Select => Self::Select,
            AttackAction::None => Self::Zero,
            AttackAction::Attack
            | AttackAction::NoMove
            | AttackAction::Rally
            | AttackAction::Move
            | AttackAction::AttackSupport => Self::Other,
        }
    }
}

/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` for `action`, with `target`
/// the object under the cursor: Select selects it (`0x004ABDD3..0x004ABDDA`),
/// and 0 selects an object the player may select (vt+0x138) and has not
/// (`+0x83`) (`0x004ABDE1..0x004ABE11`); otherwise both do nothing. Every
/// other action dispatches.
fn left_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    action: ReleaseAction,
    target: Option<u64>,
) -> LeftRelease {
    let selects = match action {
        ReleaseAction::Other => return LeftRelease::Dispatch,
        ReleaseAction::Select => target.is_some(),
        ReleaseAction::Zero => target.is_some_and(|id| {
            sim.object_is_selectable(id, rules)
                && sim
                    .entities()
                    .get(id)
                    .is_some_and(|object| !object.selection.is_selected())
        }),
    };
    if selects {
        LeftRelease::Select
    } else {
        LeftRelease::Nothing
    }
}

/// The left release for a selection whose every object is berserk.
/// DetermineAction's best object is then berserk too (rank 0, `0x00535435`),
/// and its What_Action base answers 0 whatever the modifiers (`0x006FFEC9`,
/// cell `0x00700609`). The class rungs decide the rest:
/// - Infantry: NoMove for a human owner (`0x0051F05E`), on the ground too
///   unless `JumpJet=` (`0x0051FAAB`).
/// - Unit: NoMove over an object (`0x007403EB`), 0 on the ground
///   (`0x007404B0`). A healer's repair arm still runs (`0x0073FD7C`), so a
///   berserk repair IFV may answer Select or Repair.
/// - Aircraft: 0, over an object through its cell action (`0x00417D7C`).
///
/// [`left_release`] then decides what the release does.
fn berserk_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    cell: (u16, u16),
    issuing_house: InternedId,
) -> LeftRelease {
    use crate::app::input::cursor::ActionDistanceTarget;
    let target = hover.map_or(ActionDistanceTarget::CellCentre(cell.0, cell.1), |target| {
        ActionDistanceTarget::Object(target.stable_id)
    });
    let Some((best_id, best)) = crate::app::input::cursor::select_best_for_action(
        sim,
        current_objects,
        target,
        Some(rules),
    )
    .and_then(|id| Some((id, sim.entities().get(id)?))) else {
        return LeftRelease::Dispatch;
    };
    let action_zero = match best.category() {
        EntityCategory::Aircraft => true,
        EntityCategory::Unit => hover.is_none(),
        EntityCategory::Infantry => {
            hover.is_none()
                && sim
                    .interner()
                    .object_type(best.type_ref(), rules)
                    .is_some_and(|object| object.jumpjet)
        }
        EntityCategory::Structure => false,
    };
    let action = if action_zero {
        ReleaseAction::Zero
    } else if hover.is_some_and(|target| {
        sim.unit_repair_action(best_id, target.stable_id, issuing_house, rules)
            == Some(crate::sim::world::UnitRepairAction::Select)
    }) {
        ReleaseAction::Select
    } else {
        ReleaseAction::Other
    };
    left_release(sim, rules, action, hover.map(|target| target.stable_id))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ContextOrderTarget {
    ScreenPoint(f32, f32),
    Radar(crate::render::radar::NativeRadarInputTarget),
}

/// Attempt to issue a context-sensitive order at the given input target.
///
/// Returns `true` when context dispatch consumed the click, including a refused
/// event, and `false` when the click should fall through to selection handling.
///
/// When `select_friendly_clicks` is true, clicks on friendly units/structures
/// return `false` so the caller can treat them as selection clicks instead.
pub(crate) fn try_queue_context_order(
    state: &mut AppState,
    target: ContextOrderTarget,
    select_friendly_clicks: bool,
) -> bool {
    if state.match_state.sim_runtime.is_none() {
        return false;
    }
    let (target_rx, target_ry) = match target {
        ContextOrderTarget::ScreenPoint(x, y) => {
            crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, x, y)
        }
        ContextOrderTarget::Radar(target) => target.cell,
    };
    let world_point = match target {
        ContextOrderTarget::ScreenPoint(x, y) => Some(
            crate::app::match_runtime::sim_tick::screen_point_to_world(state, x, y),
        ),
        ContextOrderTarget::Radar(_) => None,
    };
    // Retail modifier map: Ctrl = force fire, Alt = force move,
    // Ctrl+Shift = attack move, Ctrl+Alt = guard area. Shift alone only makes a
    // Cell's What_Action answer Move — see `OrderModifier` for the full
    // derivation.
    let mut modifier = resolve_order_modifiers(
        is_ctrl_held(state),
        is_shift_held(state),
        is_alt_held(state),
    );
    // The object receiver 4D74E0 reads the force-move key itself
    // (`0x004D79EC..0x004D7A14`), whatever chord it is part of.
    let force_move_key = is_alt_held(state);
    let order_mode = state.match_state.input.queued_order_mode;
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    let owner_id: InternedId = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(|s| s.interner().get(&owner))
        .unwrap_or_default();

    let mut queued: Vec<CommandEnvelope> = Vec::new();
    // The one object that speaks the order-ack line. Retail lets only the first
    // entry of the selection array speak.
    let mut speaker_id: Option<u64> = None;
    let selected_ids = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    // `EVA_NewRallyPointEstablished` is spoken by the click handler itself,
    // after the sim borrow below ends.
    let mut rally_announce = false;
    let mut consumed_attack_action = false;
    let planning = state.match_state.input.planning.active();

    if let Some(rt) = state.match_state.sim_runtime.as_mut() {
        let resources = &rt.resources;
        let sim = &mut rt.simulation;
        let execute_tick = sim.session().tick();
        if selected_ids.is_empty() {
            return false;
        }
        speaker_id = selected_ids.first().copied();
        // `CurrentObjects`: DetermineAction and the gates that read the
        // selection's size see every selected object. Orders go only to the
        // objects whose ClickedAction can still send one.
        let current_objects = selected_ids;
        let selected_ids = sim.click_order_receivers(&current_objects);

        let mut selected_units: Vec<u64> = Vec::new();

        for &sid in &selected_ids {
            let Some(entity) = sim.entities().get(sid) else {
                continue;
            };
            if entity.category() != EntityCategory::Structure {
                selected_units.push(sid);
            }
        }
        let hover_object = match target {
            ContextOrderTarget::ScreenPoint(..) => {
                let (world_x, world_y) = world_point.expect("tactical input has a world point");
                hover_object_at_point(
                    crate::app::input::entity_pick::PresentedWorld {
                        sim,
                        interpolation: &state.match_state.match_presentation.interpolation,
                    },
                    world_x,
                    world_y,
                    &owner,
                    state.match_state.sandbox_full_visibility,
                )
            }
            // Radar653C6A asks What_Action for the tracker identity returned
            // by656750, never the tactical object at the camera-clamped cell.
            ContextOrderTarget::Radar(target) => target
                .tracker_hit
                .and_then(|id| crate::app::input::entity_pick::hover_object_by_id(sim, id, &owner)),
        };
        let hover = hover_object.and_then(HoverObject::techno);
        // Techno700082 -> Action8 precedes Infantry/Unit class actions.
        // Let the selection consumer handle Shift over a visible object;
        // bare ground still reaches its immediate Move. Berserk-only groups
        // keep their native base-action-zero route below.
        if modifier == OrderModifier::Shift
            && !selected_ids.is_empty()
            && hover_object
                .and_then(|object| object_key_target(object, hover.as_ref()))
                .is_some_and(|(object, target)| {
                    cursor_owner(sim, &resources.rules, &current_objects, object).is_some_and(
                        |owner| {
                            object_key_action(
                                sim,
                                &resources.rules,
                                owner,
                                target,
                                modifier,
                                force_move_key,
                            ) == Some(ObjectKeyAction::ToggleSelect)
                        },
                    )
                })
        {
            return false;
        }
        // Selection__DispatchMultiUnitOrder clamps the shared Cell first
        // (`0x004AE77E`, `Clamp_To_Playfield @ 0x00586E50`); every later
        // receiver's What_Action and ClickedAction see it. Object hit-testing
        // above keeps the raw screen-derived world point.
        let (target_rx, target_ry) = sim.clamped_click_cell((target_rx, target_ry));
        let cell_is_shrouded = crate::app::presentation::instances::cell_visibility_for_local_owner(
            Some(owner_id),
            Some(sim.fog()),
            target_rx,
            target_ry,
            state.match_state.sandbox_full_visibility,
            sim.resolved_terrain(),
        )
            == crate::app::presentation::instances::CellVisibilityState::Shrouded;
        // The chord test fails — and the order resolves normally — unless every
        // selected object can accept an attack-move order. The walk covers the
        // whole selection, so a selected building or aircraft kills the chord.
        if modifier == OrderModifier::AttackMove
            && !selection_can_attack_move(sim, Some(&resources.rules), &current_objects)
        {
            modifier = if force_move_key {
                OrderModifier::ForceMove
            } else {
                OrderModifier::Normal
            };
        }
        // Techno What_Action answers Self for the lone selected object clicking
        // itself (`0x007000B9..0x007000E8`) before Alt's force-move rung
        // (`0x007001E9..0x00700217`), so Alt leaves that click a plain one.
        if modifier != OrderModifier::Shift
            && hover
                .as_ref()
                .is_some_and(|target| current_objects == [target.stable_id])
        {
            modifier = OrderModifier::Normal;
        }
        let shift_click: bool = modifier == OrderModifier::Shift;
        let force_fire: bool = modifier == OrderModifier::ForceFire;
        let force_move: bool = modifier == OrderModifier::ForceMove
            || (modifier == OrderModifier::AttackMove && force_move_key);
        // Legacy capability batches below run for ordinary input. Engineer
        // overrides run in their native prefix for every chord except Shift;
        // forced C4/infiltration use the shared admitted-Attack suffix.
        let context_actions_enabled: bool = matches!(
            modifier,
            OrderModifier::Normal | OrderModifier::Shift | OrderModifier::AttackMove
        );
        // A held chord overrides the sticky sidebar order mode for this click.
        let order_mode = match modifier {
            OrderModifier::AttackMove => OrderMode::AttackMove,
            _ => order_mode,
        };

        if selected_ids.is_empty() {
            return match berserk_selection_click(
                sim,
                &resources.rules,
                &current_objects,
                hover.as_ref(),
                (target_rx, target_ry),
                owner_id,
            ) {
                LeftRelease::Dispatch => finish_order(state, queued, speaker_id),
                LeftRelease::Nothing => true,
                LeftRelease::Select => false,
            };
        }

        let mut selected_ids = selected_ids;
        let repair_dispatch = hover.as_ref().map(|target| {
            unit_repair_branch_orders(
                sim,
                &resources.rules,
                &current_objects,
                &selected_ids,
                target,
                owner_id,
                modifier,
                order_mode,
                force_move_key,
            )
        });
        if let Some(repair_dispatch) = repair_dispatch {
            consumed_attack_action |= repair_dispatch.consumed;
            queued.extend(
                repair_dispatch
                    .commands
                    .into_iter()
                    .map(|payload| CommandEnvelope::new(owner_id, execute_tick, payload)),
            );
            selected_units.retain(|id| !repair_dispatch.receivers.contains(id));
            selected_ids.retain(|id| !repair_dispatch.receivers.contains(id));
            if selected_ids.is_empty() {
                let sent = finish_order(state, queued, speaker_id);
                return sent || (consumed_attack_action && !repair_dispatch.select_fallback);
            }
        }

        // An Engineer's DisarmBomb is its first action over a bombed object.
        // Every selected object takes its own action
        // (`Selection__DispatchMultiUnitOrder` 0x004AE750, object loop
        // 0x004AE829..0x004AE85A): the Engineers defuse, and over an enemy the rest go on to
        // their own orders below. Over a friend the rest have nothing to do,
        // and the dispatch happens only when the object that owns the cursor
        // is one of the Engineers; otherwise the click selects.
        if modifier != OrderModifier::Shift
            && let Some(target) = hover.as_ref()
        {
            let friendly = matches!(
                target.kind,
                HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
            );
            let engineers = disarm_bomb_engineers(
                sim,
                &resources.rules,
                &selected_units,
                target.stable_id,
                owner_id,
            );
            let dispatched = !engineers.is_empty()
                && (!friendly
                    || cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                        .is_some_and(|best| engineers.contains(&best)));
            if dispatched {
                for &attacker_id in &engineers {
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::Attack {
                            attacker_id,
                            target_id: target.stable_id,
                        },
                    ));
                }
                selected_units.retain(|id| !engineers.contains(id));
                selected_ids.retain(|id| !engineers.contains(id));
                if friendly || selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id);
                }
            }
        }

        // Infantry51E49E's Engineer building override precedes Ctrl/Alt's
        // base action. Reuse the existing admission/command producer here;
        // later legacy capability batches must not reclassify these actors.
        if modifier != OrderModifier::Shift
            && let Some(target) = hover.as_ref()
        {
            if let Some(orders) =
                engineer_capture_orders(sim, &resources.rules, &mut selected_units, target)
            {
                consumed_attack_action = true;
                queued.extend(
                    orders
                        .into_iter()
                        .map(|payload| CommandEnvelope::new(owner_id, execute_tick, payload)),
                );
                let remaining: std::collections::BTreeSet<_> =
                    selected_units.iter().copied().collect();
                selected_ids.retain(|id| {
                    remaining.contains(id)
                        || sim
                            .entities()
                            .get(*id)
                            .is_some_and(|actor| actor.category() == EntityCategory::Structure)
                });
                if selected_ids.is_empty() {
                    return finish_order(state, queued, speaker_id) || consumed_attack_action;
                }
            }
        }

        // Attack-capable Ctrl receivers, including fixed/occupied buildings,
        // resolve the same action as the cursor before the legacy rally/mobile
        // split. Non-attack capabilities keep their existing handlers below.
        // Over a Terrain object the cursor owner's key rungs answer first
        // ([`object_key_action`]): Alt's Move dispatches each receiver's own
        // (a building's are its own orders below). When they refuse, as for
        // a plain, Ctrl or Ctrl+Shift click, each receiver takes its
        // What_Action on the object (`terrain_object_feedback` for the
        // cursor).
        let hover_terrain = match hover_object {
            Some(HoverObject::Terrain(id)) => Some(id),
            _ => None,
        };
        let terrain_target = hover_terrain.filter(|&terrain| {
            cursor_owner(sim, &resources.rules, &current_objects, terrain).is_none_or(|owner| {
                object_key_action(sim, &resources.rules, owner, None, modifier, force_move_key)
                    .is_none()
            })
        });
        // Ctrl+Alt over a Terrain object whose key rungs refused still holds
        // Ctrl for the later rungs ([`terrain_forced`]).
        let object_forced = force_fire || (terrain_target.is_some() && terrain_forced(modifier));
        let attack_target = match (hover_object, terrain_target) {
            (Some(HoverObject::Techno(target)), _) => TargetKind::Entity(target.stable_id),
            (_, Some(id)) => TargetKind::Entity(id),
            _ => TargetKind::Cell(target_rx, target_ry),
        };
        let enemy_object = hover.as_ref().is_some_and(|target| {
            matches!(
                target.kind,
                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
            )
        });
        let cell_hidden =
            force_fire && matches!(attack_target, TargetKind::Cell(..)) && cell_is_shrouded;
        let attack_dispatch = if force_fire
            || terrain_target.is_some()
            || (enemy_object && context_actions_enabled)
        {
            // Foot's shroud wrapper (0x004DDE64..0x004DDEBE) takes a
            // Foot's forced shot at a shrouded Cell back; the Building
            // wrapper keeps it (0x00447540 passes the Attack through).
            let receivers: Vec<u64> = selected_ids
                .iter()
                .copied()
                .filter(|&id| {
                    !cell_hidden
                        || sim
                            .entities()
                            .get(id)
                            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
                })
                .collect();
            attack_branch_orders(
                sim,
                &resources.rules,
                &receivers,
                attack_target,
                object_forced,
                order_mode,
                &resources.overlay_registry,
            )
        } else if let TargetKind::Cell(rx, ry) = attack_target
            && !force_fire
            && modifier != OrderModifier::GuardArea
        {
            // Each selected object's What_Action on the Cell reaches its
            // unforced Attack rung on its own (0x004AE750's object loop);
            // its tail is the forced shot's (Attack, or a healing Unit's
            // AttackSupport), and any other answer keeps the object on its
            // Move rungs below. Foot's shroud wrapper
            // (0x004DDE64..0x004DDEBE) takes a Foot's Attack back over a
            // shrouded Cell; the Building wrapper keeps it.
            let rung: Vec<u64> = selected_ids
                .iter()
                .copied()
                .filter(|&id| {
                    (!cell_is_shrouded
                        || sim
                            .entities()
                            .get(id)
                            .is_some_and(|entity| entity.category() == EntityCategory::Structure))
                        && sim.cell_attack_rung_admits(
                            id,
                            (rx, ry),
                            &resources.rules,
                            Some(&resources.overlay_registry),
                        )
                        && matches!(
                            armed_action(
                                sim,
                                &resources.rules,
                                id,
                                &attack_target,
                                true,
                                Some(&resources.overlay_registry),
                            ),
                            Some(AttackAction::Attack | AttackAction::AttackSupport)
                        )
                })
                .collect();
            attack_branch_orders(
                sim,
                &resources.rules,
                &rung,
                attack_target,
                true,
                order_mode,
                &resources.overlay_registry,
            )
        } else {
            AttackDispatch {
                receivers: std::collections::BTreeSet::new(),
                commands: Vec::new(),
                consumed: false,
                select_fallback: false,
            }
        };
        consumed_attack_action |= attack_dispatch.consumed;
        queued.extend(
            attack_dispatch
                .commands
                .into_iter()
                .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
        );
        let attack_receivers = attack_dispatch.receivers;
        let select_fallback = attack_dispatch.select_fallback;
        if let TargetKind::Entity(target_id) = attack_target {
            let receivers: Vec<u64> = attack_receivers.iter().copied().collect();
            match object_click_release(
                sim,
                &resources.rules,
                &current_objects,
                &receivers,
                target_id,
                &resources.overlay_registry,
                object_forced,
            ) {
                LeftRelease::Select => return false,
                LeftRelease::Nothing => return true,
                LeftRelease::Dispatch => {}
            }
        }
        // A blocked stationary actor cannot suppress another selected actor's
        // accepted attack. An owner this branch took has decided the release
        // above; otherwise, with every receiver taken, Action7 alone allows
        // click-selection and Action0 consumes the click.
        let selected_ids: Vec<_> = selected_ids
            .into_iter()
            .filter(|id| !attack_receivers.contains(id))
            .collect();
        if selected_ids.is_empty() {
            let sent = finish_order(state, queued, speaker_id);
            return sent || (consumed_attack_action && !select_fallback);
        }
        selected_units.retain(|id| !attack_receivers.contains(id));
        let mobile_count = selected_units.len();
        let structure_selected = selected_ids.iter().any(|&id| {
            sim.entities()
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        });

        // Each selected building answers its own What_Action in the click
        // loops (`0x004AE829..0x004AE85A` over an object, `0x004AE95B..0x004AE990`
        // on a Cell). Beside mobile receivers, theirs decide the release below.
        if structure_selected && mobile_count > 0 {
            let (orders, announce) = building_click_orders(
                sim,
                &resources.rules,
                Some(&resources.overlay_registry),
                &selected_ids,
                owner_id,
                hover.as_ref(),
                hover_terrain,
                (target_rx, target_ry),
                modifier,
                force_move_key,
            );
            queued.extend(
                orders
                    .into_iter()
                    .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
            );
            rally_announce |= announce;
        }

        if structure_selected && mobile_count == 0 {
            match building_selection_click(
                sim,
                &resources.rules,
                &resources.overlay_registry,
                &current_objects,
                &selected_ids,
                owner_id,
                hover.as_ref(),
                hover_terrain,
                (target_rx, target_ry),
                modifier,
                force_move_key,
            ) {
                BuildingClick::Select => return false,
                BuildingClick::Nothing => return true,
                BuildingClick::Orders(orders, announce) => {
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                    rally_announce |= announce;
                }
            }
        } else {
            // Unforced late infiltration follows the garrison/C4 prefixes.
            // Forced Attack uses the same suffix in attack_branch_orders.
            let mut consumed_building_action = false;
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                if let Some(orders) =
                    infiltrator_capture_orders(sim, &resources.rules, &mut selected_units, target)
                {
                    consumed_building_action = true;
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                }
                if consumed_building_action && selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id);
                }
            }

            // The passenger rows take Alt's Move too, and Ctrl's tail for an
            // unarmed receiver; the objects they leave take the force-move or
            // the forced click below.
            let mut consumed_entry_action = false;
            if (context_actions_enabled || force_move || force_fire)
                && let Some(target) = hover.as_ref()
                && let Some(orders) = entry_orders(
                    sim,
                    &resources.rules,
                    &current_objects,
                    &mut selected_units,
                    target,
                    modifier,
                    force_move_key,
                    Some(&resources.overlay_registry),
                )
            {
                consumed_building_action = true;
                consumed_entry_action = true;
                queued.extend(
                    orders
                        .into_iter()
                        .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                );
                if selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id);
                }
            }

            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let orders =
                    garrison_entry_orders(sim, &resources.rules, &selected_units, target.stable_id);
                if !orders.is_empty() {
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                    return finish_order(state, queued, speaker_id);
                }
            }

            // C4 precedes the late infiltration action (51E9DA before51EE54).
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let c4_attackers: Vec<_> = selected_units
                    .iter()
                    .copied()
                    .filter(|&id| {
                        infantry_c4_action(sim, &resources.rules, id, target, false)
                            == Some(InfantryC4Action::Sabotage)
                    })
                    .collect();
                if !c4_attackers.is_empty() {
                    for attacker_id in c4_attackers {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::PlantC4 {
                                attacker_id,
                                target_building_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id);
                }
            }

            let clicked_friendly = hover.as_ref().is_some_and(|target| {
                matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            });
            if context_actions_enabled
                && let Some(cmd) = self_click_command(
                    sim,
                    &resources.rules,
                    Some(&resources.overlay_registry),
                    owner_id,
                    &current_objects,
                    hover.as_ref(),
                    planning,
                )
            {
                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                return finish_order(state, queued, speaker_id);
            }
            if context_actions_enabled
                && hover.as_ref().is_some_and(|target| {
                    self_answers_no_deploy(
                        sim,
                        &resources.rules,
                        Some(&resources.overlay_registry),
                        owner_id,
                        &current_objects,
                        target,
                        planning,
                    )
                })
            {
                return finish_order(state, queued, speaker_id);
            }
            if context_actions_enabled
                && let Some(target) = hover.as_ref()
                && let Some(release) = lone_self_foot_release(
                    sim,
                    &resources.rules,
                    owner_id,
                    &current_objects,
                    target.stable_id,
                    order_mode,
                    force_move_key,
                )
            {
                return match release {
                    (LeftRelease::Dispatch, order) => {
                        queued.extend(
                            order.map(|order| CommandEnvelope::new(owner_id, execute_tick, order)),
                        );
                        finish_order(state, queued, speaker_id)
                    }
                    _ => true,
                };
            }
            // A Crazy Ivan bombs a friend by a plain click, when the object that
            // owns the cursor is one of the bombing Ivans.
            if clicked_friendly
                && context_actions_enabled
                && !force_move
                && let Some(target) = hover.as_ref()
            {
                let ivans = friendly_bomb_ivans(
                    sim,
                    &resources.rules,
                    &selected_units,
                    current_objects.len(),
                    target.stable_id,
                    Some(&resources.overlay_registry),
                );
                if cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                    .is_some_and(|best| ivans.contains(&best))
                {
                    for attacker_id in ivans {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Attack {
                                attacker_id,
                                target_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id);
                }
            }
            // The cursor owner's What_Action distinguishes a different allied
            // House from an own object. Preserve Shift's earlier toggle-select
            // answer and the capability routes above. The shared release
            // below, not display friendliness, decides Select versus dispatch.
            let friendly_unit_action = clicked_friendly
                && hover.as_ref().is_some_and(|target| {
                    cursor_owner(sim, &resources.rules, &current_objects, target.stable_id)
                        .is_some_and(|id| {
                            object_action_answers_friendly(
                                sim,
                                &current_objects,
                                id,
                                target.stable_id,
                            )
                        })
                });
            if select_friendly_clicks
                && clicked_friendly
                && context_actions_enabled
                && (!friendly_unit_action
                    || modifier == OrderModifier::Shift
                    || consumed_building_action
                    || consumed_attack_action)
            {
                return if consumed_building_action || consumed_attack_action {
                    finish_order(state, queued, speaker_id)
                } else {
                    false
                };
            }

            // Alt over an object: Techno's force-move rung answers Move for a
            // player's controllable receiver (`0x007001E9..0x00700217`), so
            // nothing under the cursor is a target, and the object receiver
            // 4D74E0 orders the object or a Cell near it
            // ([`Simulation::object_move_click`]). Rungs that answer first
            // (Self) or rewrite it (the passenger rows, [`entry_orders`]) are
            // handled above. Others are not asked here (#605): the class
            // rungs noted above, and Foot's shroud rung
            // (`0x004DDF45..0x004DDF82`), which only GameMode 0 reaches. An
            // enemy the player cannot see is no object to click, as for the
            // cursor: the Cell under it takes the click.
            let force_move_target = hover_object
                .and_then(|object| object_key_target(object, hover.as_ref()))
                .filter(|&(object, target)| {
                    // A Techno's Ctrl+Alt Move goes through its guard click.
                    (force_move || target.is_none())
                        && cursor_owner(sim, &resources.rules, &current_objects, object)
                            .is_some_and(|owner| {
                                object_key_action(
                                    sim,
                                    &resources.rules,
                                    owner,
                                    target,
                                    modifier,
                                    force_move_key,
                                ) == Some(ObjectKeyAction::Move)
                            })
                })
                .map(|(object, _)| object);

            // Scroll693271..69328B gives the same picked object to action
            // and release. Re-picking with an empty viewer for Ctrl lost the
            // local shroud and sensor policy; retain the original pick.
            let attack_target = hover
                .as_ref()
                .filter(|target| {
                    !force_move
                        && (force_fire
                            || (select_friendly_clicks
                                && friendly_unit_action
                                && matches!(
                                    modifier,
                                    OrderModifier::Normal | OrderModifier::AttackMove
                                ))
                            || modifier == OrderModifier::GuardArea
                            || matches!(
                                target.kind,
                                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
                            ))
                })
                .map(|target| target.stable_id);
            // Foot4DDDE0's hidden-cell wrapper rewrites a nonzero action
            // except the planning action 0x33 to Move when MoveToShroud and the playfield
            // admit it, otherwise NoMove (4DDE8F..4DDEB0). It does not preserve
            // Attack5. The ordinary MTNK fall-through below takes Move; other
            // class/shroud combinations remain part of the shared-cell audit.
            if let Some(target_id) = attack_target {
                // Entries already dispatched the release.
                if !consumed_entry_action
                    && matches!(
                        modifier,
                        OrderModifier::Normal
                            | OrderModifier::AttackMove
                            | OrderModifier::ForceFire
                    )
                {
                    // A selected building is a receiver as well: when it
                    // owns the cursor, its own answer decides the release.
                    match object_click_release(
                        sim,
                        &resources.rules,
                        &current_objects,
                        &selected_ids,
                        target_id,
                        &resources.overlay_registry,
                        modifier == OrderModifier::ForceFire,
                    ) {
                        LeftRelease::Select => return false,
                        LeftRelease::Nothing => return true,
                        LeftRelease::Dispatch => {}
                    }
                }
                // A dispatched release selects nothing, whatever each
                // receiver sends (`0x004ABE88`).
                consumed_attack_action = true;
            }
            for stable_id in selected_units {
                let payload = if let Some(target_id) = attack_target {
                    let Some(payload) = object_attack_click_command(
                        sim,
                        &resources.rules,
                        &resources.overlay_registry,
                        owner_id,
                        order_mode,
                        modifier,
                        force_move_key,
                        stable_id,
                        target_id,
                    ) else {
                        continue;
                    };
                    payload
                } else if modifier == OrderModifier::GuardArea
                    && force_move_target.is_none()
                    && cell_guard_area_action(
                        sim,
                        &resources.rules,
                        stable_id,
                        Some(owner_id),
                        (target_rx, target_ry),
                    )
                {
                    // Ctrl+Alt over bare ground uses the same rung as hover;
                    // a Foot over shroud takes the Move path below.
                    Command::Guard {
                        entity_id: stable_id,
                        target: Some(TargetKind::Cell(target_rx, target_ry)),
                    }
                } else if force_move_target.is_none()
                    && let Some(order) = cell_harvest_order(
                        sim,
                        &resources.rules,
                        Some(&resources.overlay_registry),
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    )
                {
                    // The receiver's own Cell action; under Ctrl too once it
                    // has no weapon to force-fire (`0x007008C5`).
                    order
                } else if force_fire && !cell_is_shrouded {
                    // Unarmed Cell-click fall-through resolves the original
                    // clicked Cell before committing its Move event.
                    push_move_flash(
                        &mut queued,
                        sim,
                        &resources.rules,
                        Some(&resources.overlay_registry),
                        owner_id,
                        execute_tick,
                        &current_objects,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    );
                    let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                        sim,
                        &resources.rules,
                        Some(&resources.overlay_registry),
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    ) else {
                        continue;
                    };
                    Command::Move {
                        entity_id: stable_id,
                        target_rx: goal.0,
                        target_ry: goal.1,
                    }
                } else {
                    match order_mode {
                        OrderMode::Move | OrderMode::AttackMove => {
                            if force_move_target.is_none() {
                                push_move_flash(
                                    &mut queued,
                                    sim,
                                    &resources.rules,
                                    Some(&resources.overlay_registry),
                                    owner_id,
                                    execute_tick,
                                    &current_objects,
                                    stable_id,
                                    (target_rx, target_ry),
                                    shift_click,
                                );
                            }
                            let destination = match force_move_target {
                                Some(target_id) => {
                                    crate::app::input::commands::object_move_destination(
                                        sim,
                                        &resources.rules,
                                        owner_id,
                                        stable_id,
                                        target_id,
                                        crate::sim::world::MoveAction::Move,
                                        force_move_key,
                                    )
                                }
                                None => crate::app::input::commands::ordinary_cell_move_goal(
                                    sim,
                                    &resources.rules,
                                    Some(&resources.overlay_registry),
                                    owner_id,
                                    stable_id,
                                    (target_rx, target_ry),
                                    shift_click,
                                )
                                .map(|(rx, ry)| TargetKind::Cell(rx, ry)),
                            };
                            let Some(destination) = destination else {
                                continue;
                            };
                            move_click_command(
                                sim,
                                &resources.rules,
                                order_mode,
                                stable_id,
                                destination,
                            )
                        }
                    }
                };
                queued.push(CommandEnvelope::new(owner_id, execute_tick, payload));
            }
        }
    }

    if rally_announce && !state.match_state.input.planning.active() {
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::match_runtime::eva_producers::EVA_NEW_RALLY_POINT_ESTABLISHED,
        );
    }
    finish_order(state, queued, speaker_id) || consumed_attack_action
}

/// A receiver's order when its ClickedAction on a Cell re-asks its What_Action
/// with mouse-over 0 (`0x004D7DA1`) and gets Harvest
/// ([`crate::sim::world::Simulation::cell_move_action`]): case 6 sends
/// ClickedMission Harvest with the Cell as Destination and no Target
/// (`0x004D7E8E`), before the MoveFlash. The class wrapper answers 0 for a
/// House that is not the player's (`0x007404BF`); the MEGAMISSION event
/// itself tests no owner (`0x004C71F6..0x004C721F`).
fn cell_harvest_order(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    viewer: InternedId,
    receiver: u64,
    cell: (u16, u16),
    shift: bool,
) -> Option<Command> {
    let query = crate::sim::world::CellActionQuery {
        mouse_over: false,
        shift,
    };
    (sim.cell_move_action(viewer, receiver, cell, query, rules, registry)
        == Ok(crate::sim::world::MoveAction::Harvest))
    .then(|| {
        Command::clicked_mission(
            receiver,
            crate::sim::mission::MissionType::Harvest,
            TargetKind::Cell(cell.0, cell.1),
        )
    })
}

/// `FootClass::ClickedAction_Cell`'s MoveFlash (`0x004D7EB5..0x004D8065`)
/// for one receiver of a Cell click: only the selection's first object asks
/// ([`crate::sim::world::Simulation::cell_click_flashes_move`]), and its
/// flash is queued before its own order, as native constructs it before the
/// receiver's destination resolves. Planning Mode clicks reach the same
/// receiver loop (`0x004ABF94`), so they flash too; a plan's execution clears
/// the latch around its own ClickedAction (`0x004DC96F..0x004DC984`).
#[allow(clippy::too_many_arguments)]
fn push_move_flash(
    queued: &mut Vec<CommandEnvelope>,
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    owner_id: InternedId,
    execute_tick: u64,
    current_objects: &[u64],
    receiver: u64,
    cell: (u16, u16),
    shift: bool,
) {
    if current_objects.first() != Some(&receiver) {
        return;
    }
    let flashes = sim
        .cell_click_flashes_move(owner_id, receiver, cell, shift, rules, registry)
        .unwrap_or_else(|cause| {
            log::warn!("MoveFlash click input: {cause}");
            false
        });
    if flashes {
        queued.push(CommandEnvelope::new(
            owner_id,
            execute_tick,
            Command::MoveFlash {
                rx: cell.0,
                ry: cell.1,
            },
        ));
    }
}

/// The data capability shared by self-click, self-hover and the deploy key.
/// `UnitClass::What_Action_OnObject @ 0x0074000B` admits `IsSimpleDeployer`
/// to the deploy action; `CanDeploySlashUnload @ 0x00700E81` reads that same
/// UnitType flag. Deploy/undeploy transitions do not remove the capability.
/// The simulation owns command admission and the deployment lifecycle.
pub(super) fn is_simple_deploy_unit(
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    entity.category() == EntityCategory::Unit && object.is_simple_deployer
}

/// Infantry51EC14..51EC9F blocks Doing27..31 only when Type+6C4
/// (UndeployDelay) > -1; Type+6A8 (DeployFireWeapon) is unrelated.
/// Unit740015..74003F blocks an empty mobile transport outside a tank bunker
/// (`+0x2E4`, `0x00740023`). Both answer NoDeploy30 rather than a self
/// action. The D-key's receiver is separate.
fn self_deploy_is_blocked(
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    (entity.category() == EntityCategory::Infantry
        && object.deployer
        && object.undeploy_delay > -1
        && entity
            .infantry_sprite_pose()
            .is_some_and(|(doing, _)| (27..=31).contains(&doing)))
        || (entity.category() == EntityCategory::Unit
            && object.passengers > 0
            && !object.is_simple_deployer
            && entity.bunker.link().installed_in().is_none()
            && entity
                .passenger_role
                .cargo()
                .is_none_or(|cargo| cargo.is_empty()))
}

/// The command a click on the one selected object issues when it clicks
/// itself. `TechnoClass::What_Action_OnObject` returns ACTION_SELF only when
/// the target is the acting object, exactly one object is selected
/// (`[0xA8ECC8] == 1`, `0x007000C1`) and its owner is the current player
/// (`0x007000B9..0x007000DF`). With more selected, a click on one of them
/// resolves to ACTION_SELECT, which the left-up handler turns into a fresh
/// selection of the clicked object (`0x004ABE18..`); the caller's friendly-click
/// path does that.
///
/// Shared self-action admission for click and hover. Transport cargo,
/// factory primary and simple deployment keep their existing owners; the
/// blocked Infantry/empty-transport suffix is self_deploy_is_blocked.
///
/// A Unit's self action answers NoDeploy where Unit What_Action's site
/// tests refuse it (`Simulation::unit_self_deploy_site_refused`: a
/// DeploysInto unit's placement, a loaded transport's first-passenger land
/// and height) or CanDeploySlashUnload does (`0x0073FFEA`)
/// ([`self_deploy_is_refused`]). In Planning Mode (`0x00AC4CF4`) a Unit's or
/// Infantryman's every self action answers NoDeploy
/// (`Planning__AllowsAction 0x00637DB0`, called at `0x0073FEA0`,
/// `0x0051EC1B`, `0x0051ECA4`); a building's or aircraft's is unaffected.
/// NoDeploy orders nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn self_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    planning: bool,
) -> Option<Command> {
    let target = hover?;
    if planning_refuses_self(sim, local_owner, selected_ids, target, planning)
        || self_is_warped_out(sim, target)
    {
        return None;
    }
    self_action(sim, rules, local_owner, selected_ids, target)
        .filter(|_| !unit_refuses_deploy(sim, rules, registry, target.stable_id))
}

/// The lone selected object's NoDeploy (0x1E) over itself: a refused self
/// action ([`self_deploy_is_refused`]), or one blocked before it
/// ([`self_deploy_is_blocked`], [`is_simple_deploy_unit`]). Cursor and click
/// share it. The release dispatches NoDeploy, so a click on it orders and
/// selects nothing (`0x004ABE88`).
#[allow(clippy::too_many_arguments)]
pub(super) fn self_answers_no_deploy(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    !self_is_warped_out(sim, target)
        && (self_deploy_is_refused(
            sim,
            rules,
            registry,
            local_owner,
            selected_ids,
            target,
            planning,
        ) || (selected_ids == [target.stable_id]
            && sim.entities().get(target.stable_id).is_some_and(|entity| {
                entity.owner() == local_owner
                    && sim
                        .interner()
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| {
                            self_deploy_is_blocked(entity, object)
                                || is_simple_deploy_unit(entity, object)
                        })
            })))
}

/// Techno's warp arm (`0x006FFF03..0x006FFF8A`) precedes the self rung
/// (`0x007000B9`): an object being warped out (a Chronosphere's warp-out,
/// which leaves it selected) answers 0 over itself, as it is selected, so it
/// neither deploys nor answers NoDeploy.
fn self_is_warped_out(sim: &crate::sim::world::Simulation, target: &HoverTargetKindWithId) -> bool {
    sim.entities()
        .get(target.stable_id)
        .is_some_and(|entity| entity.is_warped_out())
}

/// The release of the lone selected Unit or Infantryman `target` clicked by
/// its owner `owner`, once its self action has been neither sent
/// ([`self_click_command`]) nor refused ([`self_answers_no_deploy`]); `None`
/// for any other click. Techno What_Action's self rung
/// (`0x007000B9..0x007000DF`) answers before the attack rung, so no attack
/// grant applies: a Crazy Ivan clicking himself bombs nothing. What is left
/// of the class's self arm is 0 (Unit `0x00740115`, Infantry `0x0051ECFC`),
/// which the class tail turns into NoMove for an object the player may order
/// ([`Simulation::foot_object_class_action`]). NoMove dispatches
/// ([`LeftRelease::Dispatch`]): the receiver's ClickedAction asks the same and sends the
/// order [`Simulation::object_move_click`] gives, which a Unit refuses over
/// itself (`0x007388D5..0x007388E8`) and an Infantryman does not. A 0 over
/// the selected object itself does nothing ([`LeftRelease::Nothing`]).
///
/// [`Simulation::foot_object_class_action`]: crate::sim::world::Simulation::foot_object_class_action
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
fn lone_self_foot_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: InternedId,
    selected_ids: &[u64],
    target: u64,
    order_mode: OrderMode,
    force_move_key: bool,
) -> Option<(LeftRelease, Option<Command>)> {
    let entity = sim.entities().get(target).filter(|entity| {
        selected_ids == [target]
            && entity.owner() == owner
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            )
    })?;
    let action = sim.foot_object_class_action(
        entity.owner(),
        target,
        target,
        crate::sim::world::MoveAction::None,
        rules,
        None,
    );
    if action == crate::sim::world::MoveAction::None {
        return Some((LeftRelease::Nothing, None));
    }
    Some((
        LeftRelease::Dispatch,
        crate::app::input::commands::object_move_destination(
            sim,
            rules,
            owner,
            target,
            target,
            action,
            force_move_key,
        )
        .map(|destination| move_click_command(sim, rules, order_mode, target, destination)),
    ))
}

/// Unit What_Action's NoDeploy answer (`0x0073FFF4`) for a self action its
/// site tests or CanDeploySlashUnload refuse, and Planning Mode's for any
/// Unit or Infantry self action.
fn self_deploy_is_refused(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    planning_refuses_self(sim, local_owner, selected_ids, target, planning)
        || (unit_refuses_deploy(sim, rules, registry, target.stable_id)
            && self_action(sim, rules, local_owner, selected_ids, target).is_some())
}

/// `Planning__AllowsAction(4)` (`0x00637DB0`) refuses a Unit's or
/// Infantryman's own Self action while Planning Mode is on.
fn planning_refuses_self(
    sim: &crate::sim::world::Simulation,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
    planning: bool,
) -> bool {
    planning
        && selected_ids == [target.stable_id]
        && sim.entities().get(target.stable_id).is_some_and(|entity| {
            entity.owner() == local_owner
                && matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry
                )
        })
}

fn unit_refuses_deploy(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> bool {
    sim.entities()
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        && (sim.unit_self_deploy_site_refused(id, rules, registry)
            || !sim.can_deploy_slash_unload(id, rules))
}

fn self_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    local_owner: InternedId,
    selected_ids: &[u64],
    target: &HoverTargetKindWithId,
) -> Option<Command> {
    if selected_ids != [target.stable_id] {
        return None;
    }
    let entity = sim
        .entities()
        .get(target.stable_id)
        .filter(|entity| entity.owner() == local_owner)?;
    let obj = sim.interner().object_type(entity.type_ref(), rules);
    if obj.is_some_and(|object| self_deploy_is_blocked(entity, object)) {
        return None;
    }
    // Unit740023/740107: a unit in a tank bunker skips the transport arm
    // and offers Deploy whenever CanDeploySlashUnload admits it; its Unload
    // leaves the bunker (the DEPLOY event, `deploy::issue_order`).
    if entity.category() == EntityCategory::Unit && entity.bunker.link().installed_in().is_some() {
        return Some(Command::DeployMcv {
            entity_id: target.stable_id,
        });
    }
    // Building447286..4472E7's gap rung precedes its cargo rungs; its Self
    // click queues Unload443514, which toggles the super-charge. Residual:
    // a type both GapGenerator= and a factory would also take the primary
    // tail4472EC and Event17 (44345F); no retail type is both.
    if let Some(offered) = sim.gap_generator_self_action(target.stable_id, rules) {
        return offered.then_some(Command::UnloadPassengers {
            transport_id: target.stable_id,
        });
    }
    if let Some(command) = super::transport_orders::unload_command(entity, obj, Some(local_owner)) {
        return Some(command);
    }
    if entity.category() == EntityCategory::Structure {
        // Building44731C..447358 keeps self action for cargo/garrisons or
        // factory primary selection. Repacking is a cell-targeted order.
        return if entity.bunker.occupant().is_some() {
            Some(Command::EjectBunker {
                bunker_id: target.stable_id,
            })
        } else if crate::sim::production::can_select_factory_primary(sim, rules, target.stable_id) {
            // Building4472EC preserves Action4 only for a nonprimary
            // factory; Click443450..443494 queues this target's Event17.
            Some(Command::TogglePrimaryFactory {
                entity_id: target.stable_id,
            })
        } else {
            None
        };
    }
    if entity.category() == EntityCategory::Infantry && obj.is_some_and(|o| o.deploy_fire) {
        return Some(Command::ToggleInfantryDeploy {
            entity_id: target.stable_id,
        });
    }
    // Unit What_Action offers its self action to a simple deployer, a
    // DeploysInto unit or a `DeployFire=` one (`0x0074000B`, `0x007400F0`).
    obj.is_some_and(|o| {
        o.deploys_into.is_some()
            || if entity.category() == EntityCategory::Unit {
                o.is_simple_deployer || o.deploy_fire
            } else {
                o.deployer
            }
    })
    .then_some(Command::DeployMcv {
        entity_id: target.stable_id,
    })
}

/// Garrison orders use the same admission owner as the hover cursor.
/// Infantry What_Action51E668..51E6B1 answers action9 for an admitted
/// garrison; Foot ClickedAction4D7716..4D773A sends Capture8 with NULL
/// Target and the building as Destination. Actual arrival belongs to the
/// existing Capture/PerCell owners, as for computer occupiers (#1400).
/// Passenger carriers take [`entry_orders`].
fn garrison_entry_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    building_id: u64,
) -> Vec<Command> {
    let garrison = sim
        .entities()
        .get(building_id)
        .filter(|target| target.category() == EntityCategory::Structure)
        .and_then(|target| sim.interner().object_type(target.type_ref(), rules))
        .is_some_and(|object| object.can_be_occupied);
    if !garrison {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| crate::sim::passenger::can_enter_transport(sim, rules, id, building_id))
        .map(|engineer_id| Command::CaptureBuilding {
            engineer_id,
            target_building_id: building_id,
        })
        .collect()
}

/// [`Simulation::entry_click_action`] under the base action the modifier and
/// [`attack_action`] give:
/// - Shift's ToggleSelect (8) returns first (`0x0051E453`), and Ctrl+Alt's
///   GuardArea (0x1A) skips every entry row;
/// - Alt answers Move (1) first (`0x007001E9`), in the Ctrl+Shift+Alt chord
///   too, which only the passenger rows take. A player's deployed `Deployer=`
///   infantryman (Doing 27..30) answers NoMove (2) for it before them
///   (`0x0051EB81..0x0051EBD7`);
/// - otherwise [`attack_action`], forced under Ctrl: Select (7) takes every
///   row, and Attack (5) none. Every other answer takes only the passenger
///   rows: an armed player's object over another house's object (0,
///   `0x0070056C`) or a selected one. Ctrl leaves the attack rung to the
///   tail for an actor neither armed nor `Infiltrate=`
///   (`0x00700444..0x00700474`), so an Engineer's Ctrl over a carrier still
///   asks it.
///
/// [`Simulation::entry_click_action`]: crate::sim::world::Simulation::entry_click_action
pub(super) fn entry_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<crate::sim::world::EntryClickAction> {
    use crate::sim::world::EntryBaseAction;
    let base = match modifier {
        OrderModifier::Shift | OrderModifier::GuardArea => return None,
        OrderModifier::ForceMove => EntryBaseAction::Other,
        OrderModifier::AttackMove if force_move_key => EntryBaseAction::Other,
        OrderModifier::Normal | OrderModifier::AttackMove | OrderModifier::ForceFire => {
            match attack_action(
                sim,
                rules,
                actor_id,
                &TargetKind::Entity(target_id),
                modifier == OrderModifier::ForceFire,
                overlay_registry,
            ) {
                AttackAction::Select => EntryBaseAction::Select,
                AttackAction::Attack => return None,
                _ => EntryBaseAction::Other,
            }
        }
    };
    let alt = modifier == OrderModifier::ForceMove
        || (modifier == OrderModifier::AttackMove && force_move_key);
    if alt
        && sim.entities().get(actor_id).is_some_and(|actor| {
            actor.category() == EntityCategory::Infantry
                && sim.house_is_human_player(actor.owner())
                && actor.infantry_deploy_doing()
                && sim
                    .interner()
                    .object_type(actor.type_ref(), rules)
                    .is_some_and(|object| object.deployer)
        })
    {
        return None;
    }
    sim.entry_click_action(actor_id, target_id, base, rules)
}

/// The selected objects whose action over a friendly object is Enter, Grind
/// or NoEnter ([`entry_action`]) leave `selected`, each on its own
/// What_Action as `Selection__DispatchMultiUnitOrder`'s object loop
/// (`0x004AE829..0x004AE85A`) asks it. Foot ClickedAction (`0x004D74E0`)
/// sends a controllable (vt+0xA0) one mission Enter (case 3) or Eaten (case
/// 0xB) with NULL Target and the object as Destination; NoEnter has no case
/// and orders nothing. Without Alt the loop runs only when the action of the
/// object that owns the cursor ([`cursor_owner`] of `current_objects`) is an
/// order: one of these, or its [`attack_action`] when the [`left_release`]
/// of that dispatches. For its Select, or a 0 over an object the player may
/// select, BandBox_LeftUp selects the object instead (`0x004AB9B0` ->
/// `0x004ABE18`) and never reaches the dispatcher. A selected carrier that
/// owns the cursor over itself answers NoMove (its rung skips itself at
/// `0x00740312`, and `0x007403EB` turns the 0 into NoMove), so the passengers
/// selected with it still enter. Alt's Move is an order, so the loop always
/// runs, and the objects it leaves take the force-move. None when no
/// selected object takes an entry action.
fn entry_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::FriendlyUnit
    ) {
        return None;
    }
    let action = |id| {
        entry_action(
            sim,
            rules,
            id,
            hover.stable_id,
            modifier,
            force_move_key,
            overlay_registry,
        )
    };
    let force_move = modifier == OrderModifier::ForceMove
        || (modifier == OrderModifier::AttackMove && force_move_key);
    if !force_move {
        let owner = cursor_owner(sim, rules, current_objects, hover.stable_id)?;
        if action(owner).is_none()
            && left_release(
                sim,
                rules,
                attack_action(
                    sim,
                    rules,
                    owner,
                    &TargetKind::Entity(hover.stable_id),
                    modifier == OrderModifier::ForceFire,
                    overlay_registry,
                )
                .into(),
                Some(hover.stable_id),
            ) != LeftRelease::Dispatch
        {
            return None;
        }
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    selected.retain(|&id| {
        let Some(action) = action(id) else {
            return true;
        };
        consumed = true;
        let mission = match action {
            crate::sim::world::EntryClickAction::Enter => crate::sim::mission::MissionType::Enter,
            crate::sim::world::EntryClickAction::Grind => crate::sim::mission::MissionType::Eaten,
            crate::sim::world::EntryClickAction::NoEnter => return false,
        };
        if sim.techno_player_controllable(id, rules) {
            orders.push(Command::MegaMission {
                entity_id: id,
                mission,
                target: None,
                destination: Some(TargetKind::Entity(hover.stable_id)),
            });
        }
        false
    });
    consumed.then_some(orders)
}

/// Engineer object actions before event enqueue. No duplicate geometry query.
/// Some(empty) is a consumed no-order hut click; None allows other actions.
fn engineer_capture_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
    ) {
        return None;
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    selected.retain(|&id| {
        let Some(actor) = sim.entities().get(id) else {
            return true;
        };
        if actor.category() != EntityCategory::Infantry
            || !sim
                .interner()
                .object_type(actor.type_ref(), rules)
                .is_some_and(|o| o.engineer)
        {
            return true;
        }
        let Some(action) = sim.engineer_building_action(id, hover.stable_id, rules) else {
            return true;
        };
        use crate::sim::world::EngineerBuildingAction;
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if sim.techno_player_controllable(id, rules) {
            match action {
                EngineerBuildingAction::Capture
                | EngineerBuildingAction::Damage
                | EngineerBuildingAction::Repair(true) => orders.push(Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: hover.stable_id,
                }),
                // Foot clicked actions3/11, 4D76F6/4D7761: NULL Target,
                // building Destination. Event4 and Event36 share these tokens.
                EngineerBuildingAction::EnterHospital | EngineerBuildingAction::EnterGrinder => {
                    orders.push(Command::MegaMission {
                        entity_id: id,
                        mission: if action == EngineerBuildingAction::EnterHospital {
                            crate::sim::mission::MissionType::Enter
                        } else {
                            crate::sim::mission::MissionType::Eaten
                        },
                        target: None,
                        destination: Some(TargetKind::Entity(hover.stable_id)),
                    });
                }
                EngineerBuildingAction::Repair(false) => {}
            }
        }
        false
    });
    consumed.then_some(orders)
}

/// Infantry51E9C1..51EB21 post-processes an admitted Attack5. Weapon
/// cursor actions64/71 remain Attack commands; C4 action16 plants a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InfantryAttackAction {
    Attack,
    Sabotage,
    WeaponDemolish,
    AirStrike,
    Select,
    Infiltrate,
    NoMove,
}

pub(super) fn infantry_attack_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> Option<InfantryAttackAction> {
    use crate::sim::combat::combat_weapon;
    let actor = sim.entities().get(actor_id)?;
    if actor.category() != EntityCategory::Infantry {
        return None;
    }
    let object = sim.interner().object_type(actor.type_ref(), rules)?;
    // Infantry51E6B4..51E6D5 refuses any Attack left after the Engineer
    // building/disarm prefixes. Ctrl cannot turn DefuseKit into a gun.
    if object.engineer {
        return Some(InfantryAttackAction::NoMove);
    }
    // The target is a Techno or, under Ctrl, a Terrain object, which the C4
    // and weapon-cursor arms below read as no building and no Unit.
    let target = sim.entities().get(target_id);
    if target.is_none() && sim.terrain_objects().get(target_id).is_none() {
        return None;
    }
    if let Some(target) = target {
        let hover = HoverTargetKindWithId {
            stable_id: target_id,
            kind: if target.category() == EntityCategory::Structure {
                HoverTargetKind::EnemyStructure
            } else {
                HoverTargetKind::EnemyUnit
            },
        };
        if let Some(c4) = infantry_c4_action(sim, rules, actor_id, &hover, true) {
            return Some(match c4 {
                InfantryC4Action::Sabotage => InfantryAttackAction::Sabotage,
                InfantryC4Action::Attack => InfantryAttackAction::Attack,
            });
        }
    }
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let (_, weapon) = combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        object,
        &combat_weapon::attacker_facts(actor, object),
        Some(&TargetKind::Entity(target_id)),
        sim,
        sim.interner(),
        cells.as_ref(),
        Some(sim.house_alliances()),
    )?;
    let weapon = weapon?.weapon;
    if weapon.sabotage_cursor {
        return Some(
            if target.is_some_and(|target| {
                target.category() == EntityCategory::Unit
                    && target.bunker.link().installed_in().is_some()
            }) {
                InfantryAttackAction::Select
            } else {
                InfantryAttackAction::WeaponDemolish
            },
        );
    }
    // Infantry51EAD2..51EB21: anything but a C4-able building keeps Attack
    // and returns before the tail below.
    if weapon.mig_attack_cursor {
        let air_strike = target.is_some_and(|target| {
            target.category() == EntityCategory::Structure
                && sim
                    .interner()
                    .object_type(target.type_ref(), rules)
                    .is_some_and(|building| building.can_c4 && !building.invisible_in_game)
        });
        return Some(if air_strike {
            InfantryAttackAction::AirStrike
        } else {
            InfantryAttackAction::Attack
        });
    }
    // Infantry51EE54..51EEEF only rewrites an admitted Attack. The
    // simulation already owns CanInfiltrate and building admission.
    if sim.infiltrator_building_action(actor_id, target_id, rules) {
        return Some(InfantryAttackAction::Infiltrate);
    }
    // Infantry51F160..51F179: the Attack left is NoMove over an Immune=
    // type. A player's Crazy Ivan has returned at his bomb arm
    // (51EB24..51EB7E) before it.
    if !(object.ivan && sim.house_is_human_player(actor.owner()))
        && target_is_immune(sim, rules, &TargetKind::Entity(target_id))
    {
        return Some(InfantryAttackAction::NoMove);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InfantryC4Action {
    Attack,
    Sabotage,
}

/// The type-C4 arm of Infantry What_Action51E9C1..51EB21, after the
/// ordinary enemy-building hover admission. A non-vehicle building
/// terminates this arm even when CanC4/InvisibleInGame leave action Attack;
/// it must not fall through to the later infiltration arm (51EE54).
/// Shared by click dispatch and cursor feedback. Rank-granted C4 remains
/// outside the existing type-C4 input path.
pub(super) fn infantry_c4_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    hover: &HoverTargetKindWithId,
    admitted_attack: bool,
) -> Option<InfantryC4Action> {
    if !admitted_attack && hover.kind != HoverTargetKind::EnemyStructure {
        return None;
    }
    let actor = sim.entities().get(actor_id)?;
    let target = sim.entities().get(hover.stable_id)?;
    if actor.category() != EntityCategory::Infantry
        || target.category() != EntityCategory::Structure
        || !sim.house_is_human_player(actor.owner())
        || !sim.interner().object_type(actor.type_ref(), rules)?.c4
    {
        return None;
    }
    let building = sim.interner().object_type(target.type_ref(), rules)?;
    // Building virtual+80: 457620 -> BuildingType465D40, not invulnerability.
    if building.is_1x1_with_undeploy() {
        return None;
    }
    Some(if building.can_c4 && !building.invisible_in_game {
        InfantryC4Action::Sabotage
    } else {
        InfantryC4Action::Attack
    })
}

/// The selected spies whose action over `hover` is Capture
/// (`Simulation::infiltrator_building_action`): `ClickedAction_Object`
/// (0x0051F190) keeps action 9, and `FootClass::ClickedAction_Object`
/// (0x004D74E0) queues it as it does an Engineer's capture. None when no
/// selected object takes the arm.
fn infiltrator_capture_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
    ) {
        return None;
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    selected.retain(|&id| {
        if infantry_c4_action(sim, rules, id, hover, false).is_some()
            || crate::sim::passenger::can_enter_transport(sim, rules, id, hover.stable_id)
            || !sim.infiltrator_building_action(id, hover.stable_id, rules)
        {
            return true;
        }
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if sim.techno_player_controllable(id, rules) {
            orders.push(Command::CaptureBuilding {
                engineer_id: id,
                target_building_id: hover.stable_id,
            });
        }
        false
    });
    consumed.then_some(orders)
}

/// The selected Engineers whose click on `target` is DisarmBomb
/// (`InfantryClass::What_Action_OnObject`, 0x0051E462): their first action
/// over an object carrying a bomb their player sees, whoever owns it.
/// `InfantryClass::ClickedAction_Object` (0x0051F190) sends it as Attack, so
/// the DefuseKit shot defuses the bomb; it refuses the Engineer's own body.
fn disarm_bomb_engineers(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
    player: InternedId,
) -> Vec<u64> {
    let bombed = sim
        .entities()
        .get(target)
        .is_some_and(|entity| entity.bomb.is_some());
    if !bombed || !sim.bomb_seen_by(target, player) {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| {
            id != target
                && sim.entities().get(id).is_some_and(|e| {
                    e.category() == EntityCategory::Infantry
                        && sim
                            .interner()
                            .object_type(e.type_ref(), rules)
                            .is_some_and(|o| o.engineer)
                })
        })
        .collect()
}

/// The object whose action owns the cursor over `target` for the selection
/// (gamemd's DetermineAction, shared with the cursor): the click dispatches
/// its orders only when that object's action is one.
fn cursor_owner(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
) -> Option<u64> {
    crate::app::input::cursor::select_best_for_action(
        sim,
        selected,
        crate::app::input::cursor::ActionDistanceTarget::Object(target),
        Some(rules),
    )
}

/// The left release over another house's object or a Terrain object:
/// [`left_release`] of the [`attack_action`] of the object that owns the
/// cursor ([`cursor_owner`]), when it is one of the `receivers`; any other
/// owner answered in an earlier arm, whose release dispatches. The click asks it twice: for the attack
/// branch's receivers right after that branch, where an armed owner's 0 over
/// another house's object out of its range (Techno `0x0070056C..0x007005E6`,
/// kept by Building `0x0044750D`) selects the object and sends nothing, the
/// other receivers' orders included (`0x004ABE18`, `0x004ABE88`); then for
/// the receivers left.
///
/// Over a Terrain object the owner decides the same way: its 0 (an unarmed
/// building's, or an armed one's out of range) does nothing
/// (`0x004AC294`), since no retail TerrainType is selectable (vt+0x138 at
/// `0x004ABDF5` reads `+0x230`, which the constructor clears at
/// `0x0071DB18`), and the other receivers' orders are dropped.
///
/// DORMANT: a `Selectable=yes` TerrainType (ObjectType ReadINI `0x005F948C`)
/// would let that 0 select the tree; VERA never selects a Terrain object.
/// Neither RULESMD nor any of the 177 stock skirmish maps sets the key on a
/// TerrainType (census through the production map loader).
///
/// A release that selects hands the click to the selection's click pick,
/// which asks the same `Tactical::PickObjectAtScreenPoint` port
/// ([`crate::app::input::entity_pick::pick_object_at_point`]) with the same
/// Display order, presented positions, fog and viewer, and nothing changes
/// between the two calls. So it selects the object DetermineAction saw, as
/// native's one pick does (`0x00692515` → `0x00693271` / `0x0069328B`);
/// `hover_and_click_match_original_weighted_pick_and_encounter_ties` pins
/// both against the native tie corpus (spatial_oracle/pointer_pick.json).
fn object_click_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    receivers: &[u64],
    target_id: u64,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    forced: bool,
) -> LeftRelease {
    let Some(best) = cursor_owner(sim, rules, current_objects, target_id)
        .filter(|best| receivers.contains(best))
    else {
        return LeftRelease::Dispatch;
    };
    let mut action = attack_action(
        sim,
        rules,
        best,
        &TargetKind::Entity(target_id),
        forced,
        Some(overlay_registry),
    );
    if action == AttackAction::Attack
        && infantry_attack_action(sim, rules, best, target_id) == Some(InfantryAttackAction::Select)
    {
        action = AttackAction::Select;
    }
    left_release(sim, rules, action.into(), Some(target_id))
}

/// The selected Crazy Ivans whose click on the own or allied `target` plants a
/// bomb. The canonical Techno grant and ILLEGAL test must already answer
/// Attack ([`attack_action`], which the cursor asks too). Infantry51EB24..7E
/// consumes that answer without repeating its type/weapon grants; the Ivan makes it
/// IvanBomb ([`ivan_bomb_refused`], 0x0051EB24), and
/// `FootClass::ClickedAction_Object` (0x004D74E0) sends that as Attack. A
/// bombed friend stays a selection click. A lone selected Ivan over himself
/// has the self action (4) instead; in a larger selection nothing stops him
/// bombing himself, as natively. The count is the whole selection's
/// (`[0xA8ECC8]`), berserk objects included.
fn friendly_bomb_ivans(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    selection_count: usize,
    target: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Vec<u64> {
    let bombable = sim.entities().get(target).is_some_and(|entity| {
        let obj = sim.interner().object_type(entity.type_ref(), rules);
        !ivan_bomb_refused(entity, obj)
    });
    if !bombable {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| {
            (id != target || selection_count > 1)
                && sim.entities().get(id).is_some_and(|e| {
                    sim.interner()
                        .object_type(e.type_ref(), rules)
                        .is_some_and(|o| o.ivan)
                })
                && attack_action(
                    sim,
                    rules,
                    id,
                    &TargetKind::Entity(target),
                    false,
                    overlay_registry,
                ) == AttackAction::Attack
        })
        .collect()
}

/// `InfantryClass::What_Action_OnObject`'s Ivan arm
/// (`0x0051EB24..0x0051EB7E`): a player's Crazy Ivan (`Ivan=`, `+0xEAE`)
/// whose action is Attack answers NoIvanBomb over an object that carries a
/// bomb (`+0x38`) or is not `Bombable=` (`+0x22E`), else IvanBomb.
pub(crate) fn ivan_bomb_refused(
    target: &crate::sim::game_entity::GameEntity,
    target_obj: Option<&crate::rules::object_type::ObjectType>,
) -> bool {
    target.bomb.is_some() || !target_obj.is_some_and(|o| o.bombable)
}

/// [`ivan_bomb_refused`] for any object under the cursor, a Terrain object
/// included: one never carries a bomb, since BombListClass::Attach gets a
/// null target for anything that is not a Techno (AbstractFlags bit 1,
/// `0x0046934D..0x00469361`), so only its type's `Bombable=` refuses it.
/// The Ivan still answers IvanBomb, and his shot at a tree plants nothing.
pub(crate) fn ivan_bomb_object_refused(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    target_id: u64,
) -> bool {
    if let Some(target) = sim.entities().get(target_id) {
        return ivan_bomb_refused(target, sim.interner().object_type(target.type_ref(), rules));
    }
    !sim.terrain_objects().get(target_id).is_some_and(|terrain| {
        rules
            .terrain_object_type_case_insensitive(sim.interner().resolve(terrain.type_ref()))
            .is_some_and(|terrain| terrain.bombable)
    })
}

/// Whether `actor_id` is a Crazy Ivan whose Attack over `target_id` is
/// NoIvanBomb ([`ivan_bomb_refused`]), which
/// `FootClass::ClickedAction_Object` (0x004D74E0) ignores. An unforced click
/// on a bombed target never gets that far: its GetFireError is ILLEGAL
/// (`0x006FCBAD`, [`shot_is_illegal`]), so the Ivan's What_Action answers 0
/// or Select there ([`attack_action`]).
fn ivan_cannot_bomb(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    let is_ivan = sim
        .entities()
        .get(actor_id)
        .and_then(|e| sim.interner().object_type(e.type_ref(), rules))
        .is_some_and(|o| o.ivan);
    is_ivan && ivan_bomb_object_refused(sim, rules, target_id)
}

/// The ILLEGAL test of `TechnoClass::What_Action_OnObject`'s attack rung
/// (`0x00700536..0x0070055B`): GetFireError (vt+0x3C0, with the selected
/// weapon and range asked; [`Simulation::selected_weapon_fire_error`])
/// answers ILLEGAL (5) for the object. The Infiltrate arm
/// (`0x007004A0..0x00700531`) keeps the rung for an `Infiltrate=`
/// infantryman over a building that is `Capturable=` when he is not
/// `Agent=`, `Spyable=` when he is, or `CanC4=` when he is `C4=`. A Cell
/// target asks no such test.
///
/// Input asks it with the Dummy's coordinate word detached
/// ([`SharedCellDummy::detached`]), so GetFireError's missing-cell lookups
/// stamp a private copy and a cursor or click leaves the simulation's
/// retained Dummy as it was. [`resolved_unit_in_range`] isolates its query
/// for the same reason.
///
/// [`Simulation::selected_weapon_fire_error`]: crate::sim::world::Simulation::selected_weapon_fire_error
/// [`SharedCellDummy::detached`]: crate::map::resolved_terrain::SharedCellDummy::detached
fn shot_is_illegal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let TargetKind::Entity(target_id) = *target else {
        return false;
    };
    let infiltrate_arm = sim
        .entities()
        .get(actor_id)
        .filter(|actor| actor.category() == EntityCategory::Infantry)
        .and_then(|actor| sim.interner().object_type(actor.type_ref(), rules))
        .filter(|infantry| infantry.infiltrate)
        .zip(
            sim.entities()
                .get(target_id)
                .filter(|object| object.category() == EntityCategory::Structure)
                .and_then(|object| sim.interner().object_type(object.type_ref(), rules)),
        )
        .is_some_and(|(infantry, building)| {
            (if infantry.agent {
                building.spyable
            } else {
                building.capturable
            }) || (infantry.c4 && building.can_c4)
        });
    !infiltrate_arm
        && sim.effective_shared_cell_dummy().detached(|| {
            sim.selected_weapon_fire_error(rules, actor_id, *target, true, overlay_registry)
        }) == crate::sim::combat::fire_error::FireError::Illegal
}

/// Building4436F0(1/6, cell): prepare archive event1E, then SELL16 for an
/// UndeploysInto type. A failed FNPC emits no archive but still queues SELL,
/// retaining an earlier archive, except that a Construction Yard archives its
/// own Cell ([`crate::sim::world::Simulation::factory_rally_cell_input`]). A
/// factory announces its rally click whether or not the search found a Cell
/// (`0x00443A7B..0x00443A89` jumps to the announcement at `0x00443A2B`).
///
/// The click loop clamps the Cell to the playfield first
/// ([`crate::sim::world::Simulation::clamped_click_cell`]), then asks each
/// building's own What_Action on it (`0x004AE974`,
/// [`crate::sim::world::Simulation::cell_move_action`]). Active_Click_With_Cell
/// has no arm for NoMove or for the Ctrl+Alt guard rung's 0x1A
/// (`0x0044372F..0x00443737`): a Construction Yard over a Cell it may not
/// stand on neither archives nor packs up, nor does an active building
/// without a rally line under `guard_area` ([`cell_guard_area_action`]).
/// A query the sim cannot answer (no map cells) keeps Move: VERA-internal, as
/// a live gamemd session always has a map.
#[allow(clippy::too_many_arguments)]
fn building_cell_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    selected: &[u64],
    owner: InternedId,
    cell: (u16, u16),
    shift: bool,
    guard_area: bool,
) -> (Vec<Command>, bool) {
    let cell = sim.clamped_click_cell(cell);
    let query = crate::sim::world::CellActionQuery {
        mouse_over: false,
        shift,
    };
    let mut orders = Vec::new();
    let mut announce = false;
    for id in selected_rally_producer_ids(sim, rules, selected, owner) {
        let Some(object) = sim
            .entities()
            .get(id)
            .and_then(|entity| sim.interner().object_type(entity.type_ref(), rules))
        else {
            continue;
        };
        if guard_area && cell_guard_area_action(sim, rules, id, Some(owner), cell) {
            continue;
        }
        let action = sim
            .cell_move_action(owner, id, cell, query, rules, registry)
            .unwrap_or(crate::sim::world::MoveAction::Move);
        if action != crate::sim::world::MoveAction::Move {
            continue;
        }
        match sim.factory_rally_cell_input(id, cell, rules) {
            Ok(found) => {
                if let Some((rx, ry)) = found {
                    orders.push(Command::SetRally {
                        rx,
                        ry,
                        producer_ids: vec![id],
                    });
                }
                // A missed search still reaches the announcement
                // (`0x00443977` -> `0x00443A7B` -> `0x00443A2B`).
                announce |= crate::app::match_runtime::eva_producers::rally_point_announces(object);
            }
            Err(error) => log::warn!("building cell input: {error}"),
        }
        if object.undeploys_into.is_some() {
            orders.push(Command::SellBuilding { entity_id: id });
        }
    }
    (orders, announce)
}

/// What a left click does for a selection of buildings only.
#[derive(Debug, Clone, PartialEq)]
enum BuildingClick {
    /// The ordinary selection click on the object under the cursor.
    Select,
    /// The click is consumed with no order.
    Nothing,
    /// The click dispatches these orders; `true` announces a new rally point.
    Orders(Vec<Command>, bool),
}

/// A left click for a selection of buildings only, after the attack
/// receivers have been taken out of `receivers`: a lone building's self
/// action, the release over an object ([`building_object_release`]), then
/// each building's own orders ([`building_click_orders`]). Over the player's
/// own or an allied object the cursor owner's What_Action decides the
/// release too ([`object_action_answers_friendly`]): Select or 0 selects an
/// object the player may select and has not, and nothing else happens.
/// Shift over an object never reaches here: the caller hands it to the
/// selection consumer first.
#[allow(clippy::too_many_arguments)]
fn building_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    current_objects: &[u64],
    receivers: &[u64],
    owner: InternedId,
    hover: Option<&HoverTargetKindWithId>,
    terrain: Option<u64>,
    cell: (u16, u16),
    modifier: OrderModifier,
    force_move_key: bool,
) -> BuildingClick {
    let context_actions_enabled = matches!(
        modifier,
        OrderModifier::Normal | OrderModifier::Shift | OrderModifier::AttackMove
    );
    if context_actions_enabled
        // Building What_Action's self rows have no planning test.
        && let Some(command) = self_click_command(
            sim,
            rules,
            Some(overlay_registry),
            owner,
            current_objects,
            hover,
            false,
        )
    {
        return BuildingClick::Orders(vec![command], false);
    }
    if let Some(target) = hover.filter(|target| target.kind != HoverTargetKind::HiddenEnemy) {
        match building_object_release(
            sim,
            rules,
            overlay_registry,
            current_objects,
            receivers,
            target.stable_id,
            modifier == OrderModifier::ForceFire,
            force_move_key,
        ) {
            LeftRelease::Select => return BuildingClick::Select,
            LeftRelease::Nothing => return BuildingClick::Nothing,
            LeftRelease::Dispatch => {}
        }
    }
    let (orders, announce) = building_click_orders(
        sim,
        rules,
        Some(overlay_registry),
        receivers,
        owner,
        hover,
        terrain,
        cell,
        modifier,
        force_move_key,
    );
    // Over a Terrain object a building that does not answer Move answers 0,
    // which selects nothing (a tree is no object to select).
    if terrain.is_some() && orders.is_empty() {
        return BuildingClick::Nothing;
    }
    BuildingClick::Orders(orders, announce)
}

/// The selected buildings' own orders for a left click: over a visible
/// object or a Terrain object (`terrain`) ([`building_object_orders`]; an
/// enemy the player cannot see is no object, and its Cell takes the click)
/// or on a Cell ([`building_cell_orders`]). Ctrl+Alt over the player's own
/// or an allied object answers 0x1A, which has no Building ClickedAction
/// arm; a Terrain object is no ally, so there it answers Alt's Move
/// ([`object_key_action`]).
#[allow(clippy::too_many_arguments)]
fn building_click_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    selected: &[u64],
    owner: InternedId,
    hover: Option<&HoverTargetKindWithId>,
    terrain: Option<u64>,
    cell: (u16, u16),
    modifier: OrderModifier,
    force_move_key: bool,
) -> (Vec<Command>, bool) {
    let guard_area = modifier == OrderModifier::GuardArea;
    match hover.filter(|target| target.kind != HoverTargetKind::HiddenEnemy) {
        Some(target)
            if guard_area
                && matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                ) =>
        {
            (Vec::new(), false)
        }
        Some(target) => {
            building_object_orders(sim, rules, selected, target.stable_id, force_move_key)
        }
        None if let Some(terrain) = terrain => {
            building_object_orders(sim, rules, selected, terrain, force_move_key)
        }
        None => building_cell_orders(
            sim,
            rules,
            registry,
            selected,
            owner,
            cell,
            modifier == OrderModifier::Shift,
            guard_area,
        ),
    }
}

/// Whether Techno What_Action over an object (`0x006FFEC0`) answers on its
/// force-move rungs for the selected building `id`: the force-move key held
/// (`0x0070000E..0x0070003D`), a human player's building, and one the player
/// may order (vt+0xA0). Over the player's own or an allied object Ctrl+Alt
/// answers 0x1A (`0x00700191..0x007001E7`), otherwise Move
/// (`0x007001E9..0x00700217`). Without them Building What_Action_OnObject
/// (`0x00447210`) is left with the attack rung or the Techno tail's Select
/// or 0 ([`attack_action`]), so a plain click over an object never rallies
/// or packs a building up: Building ClickedAction_Object (`0x00443410`)
/// sends neither for those.
fn building_moves_over_object(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    force_move_key: bool,
) -> bool {
    force_move_key
        && sim
            .entities()
            .get(id)
            .is_some_and(|building| sim.house_is_human_player(building.owner()))
        && sim.techno_player_controllable(id, rules)
}

/// The left release over an object for a selection of buildings only: the
/// action of the object that owns the cursor ([`cursor_owner`]). A force-move
/// rung dispatches ([`building_moves_over_object`]); any other answer is
/// [`object_click_release`]'s, so an unarmed factory or Construction Yard
/// selects the object as DisplayClass does for Select (`0x004ABDD3`).
#[allow(clippy::too_many_arguments)]
fn building_object_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    current_objects: &[u64],
    receivers: &[u64],
    target_id: u64,
    forced: bool,
    force_move_key: bool,
) -> LeftRelease {
    if cursor_owner(sim, rules, current_objects, target_id).is_some_and(|best| {
        receivers.contains(&best) && building_moves_over_object(sim, rules, best, force_move_key)
    }) {
        return LeftRelease::Dispatch;
    }
    object_click_release(
        sim,
        rules,
        current_objects,
        receivers,
        target_id,
        overlay_registry,
        forced,
    )
}

/// Building ClickedAction_Object (`0x00443410`) for each selected building
/// whose action over `target` is Move: the force-move rung
/// ([`building_moves_over_object`]) and the building's own factory rungs
/// ([`Simulation::building_object_move_action`]); a NoMove sends nothing. An
/// Infantry, Unit or Aircraft factory (Type+0xEB8, `0x0044361B..0x0044363A`)
/// sets its rally point from the Cell under the object's GetCoords
/// (`0x0044368D..0x004436D8`, Building443860, [`Simulation::factory_rally_cell_input`]).
/// Any other building, a Construction Yard among them, archives the object
/// itself (event 0x1E, `0x0044363C..0x00443688`, [`Command::ArchiveObject`])
/// and never packs up: a depot then releases repaired units toward it,
/// Cloning Vats hand it to their clones, and a yard's later pack-up that does
/// not archive again sends its MCV or Slave Miner to the object.
///
/// [`Simulation::factory_rally_cell_input`]: crate::sim::world::Simulation::factory_rally_cell_input
/// [`Simulation::building_object_move_action`]: crate::sim::world::Simulation::building_object_move_action
fn building_object_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
    force_move_key: bool,
) -> (Vec<Command>, bool) {
    use crate::rules::object_type::FactoryType;
    let Some(cell) = sim.object_coords_cell(target) else {
        return (Vec::new(), false);
    };
    let mut orders = Vec::new();
    let mut announce = false;
    for &id in selected {
        let Some(object) = sim
            .entities()
            .get(id)
            .filter(|building| building.category() == EntityCategory::Structure)
            .and_then(|building| sim.interner().object_type(building.type_ref(), rules))
        else {
            continue;
        };
        if !building_moves_over_object(sim, rules, id, force_move_key) {
            continue;
        }
        if !matches!(
            object.factory_type(),
            Some(FactoryType::InfantryType | FactoryType::UnitType | FactoryType::AircraftType)
        ) {
            orders.push(Command::ArchiveObject {
                entity_id: id,
                target_id: target,
            });
            continue;
        }
        match sim.building_object_move_action(id, target, rules) {
            Ok(crate::sim::world::MoveAction::Move) => {}
            Ok(_) => continue,
            Err(error) => {
                log::warn!("building object input: {error}");
                continue;
            }
        }
        match sim.factory_rally_cell_input(id, cell, rules) {
            Ok(found) => {
                if let Some((rx, ry)) = found {
                    orders.push(Command::SetRally {
                        rx,
                        ry,
                        producer_ids: vec![id],
                    });
                }
                // A missed search still reaches the announcement
                // (`0x00443977` -> `0x00443A7B` -> `0x00443A2B`).
                announce |= crate::app::match_runtime::eva_producers::rally_point_announces(object);
            }
            Err(error) => log::warn!("building object input: {error}"),
        }
    }
    (orders, announce)
}

fn selected_rally_producer_ids(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    owner: InternedId,
) -> Vec<u64> {
    let owner = sim.interner().resolve(owner);
    let mut seen = std::collections::HashSet::with_capacity(selected_ids.len());
    selected_ids
        .iter()
        .copied()
        .filter(|&stable_id| {
            seen.insert(stable_id) && sim.takes_rally_point(stable_id, owner, rules)
        })
        .collect()
}

#[cfg(test)]
#[path = "building_redeploy_tests.rs"]
mod building_redeploy_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    /// Without resolved terrain the attack cursor asks the shared mapless
    /// range (`sim::combat::mapless_in_range`), as the fire gate and the
    /// threat scan do: `Range=-2` reaches across the map, and MinimumRange
    /// refuses a target inside it.
    #[test]
    fn a_mapless_attack_cursor_takes_the_shared_range_answer() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n0=ASW\n1=ART\n2=MTNK\n\
                 [AircraftTypes]\n[BuildingTypes]\n\
                 [ASW]\nStrength=300\nPrimary=Depth\n[ART]\nStrength=300\nPrimary=Shell\n\
                 [MTNK]\nStrength=300\n\
                 [Depth]\nDamage=10\nROF=30\nRange=-2\nWarhead=WH\n\
                 [Shell]\nDamage=10\nROF=30\nRange=8\nMinimumRange=3\nWarhead=WH\n\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let mut spawn = |kind: &str, owner: &str, rx: u16, ry: u16| {
            sim.spawn_object(kind, owner, rx, ry, 0, &rules).unwrap()
        };
        let asw = spawn("ASW", "Americans", 5, 5);
        let art = spawn("ART", "Americans", 5, 7);
        let near = spawn("MTNK", "Russians", 6, 7);
        let mid = spawn("MTNK", "Russians", 10, 7);
        let far = spawn("MTNK", "Russians", 40, 5);
        assert!(sim.resolved_terrain().is_none());
        let in_range = |actor, target| {
            resolved_unit_in_range(
                &sim,
                actor,
                &TargetKind::Entity(target),
                Some(&rules),
                None,
                None,
                None,
            )
        };
        assert!(in_range(asw, far), "35 cells away");
        assert!(!in_range(art, near), "inside MinimumRange");
        assert!(in_range(art, mid), "5 cells away");
        assert!(!in_range(art, far), "beyond Range");
    }

    /// A Cell click asks each receiver's own What_Action ([`cell_harvest_order`]).
    /// Over revealed ore the player's War Miner sends MEGAMISSION Harvest with
    /// the Cell as Destination. Another player's War Miner, though selected,
    /// answers None and sends nothing: the event itself would run it, testing
    /// no owner. A Grizzly has no harvest rung. Where a tank holds the ore
    /// Cell, the player's War Miner answers NoMove, and its ClickedAction
    /// resolves a Cell to move to instead (`0x004D8065`).
    #[test]
    fn retail_ore_click_harvests_where_the_receivers_cell_action_does() {
        use crate::sim::house_state::HouseState;
        use crate::sim::world::{CellActionQuery, MoveAction};

        let Some(battle) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = &battle.rules;
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &battle.processed_rules,
            Some(&battle.fixed_art),
        );
        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        for name in ["Americans", "Russians"] {
            let owner = sim.intern(name);
            let side = rules
                .country_side_index(name)
                .expect("stock country Side")
                .0;
            sim.houses_mut_for_test().register(HouseState::new(
                owner,
                side,
                Some(owner),
                true,
                0,
                10,
            ));
        }
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        let (open, taken) = ((12, 14), (16, 14));
        let ore = registry.id_for_name("TIB01").unwrap();
        for cell in [open, taken] {
            sim.place_overlay_for_test(cell, ore, 6, &registry);
        }
        let own = sim
            .spawn_object("HARV", "Americans", 10, 10, 0, rules)
            .expect("the player's War Miner");
        let other = sim
            .spawn_object("HARV", "Russians", 14, 10, 0, rules)
            .expect("another player's War Miner");
        let tank = sim
            .spawn_object_with_overlay_registry(
                "MTNK",
                "Americans",
                taken.0,
                taken.1,
                0,
                rules,
                &registry,
            )
            .expect("the player's Grizzly on the ore");
        sim.resolve_type_handles(rules);

        let harvest =
            |id, cell| cell_harvest_order(&sim, rules, Some(&registry), owner, id, cell, false);
        assert_eq!(
            harvest(own, open),
            Some(Command::clicked_mission(
                own,
                crate::sim::mission::MissionType::Harvest,
                TargetKind::Cell(open.0, open.1),
            ))
        );
        assert_eq!(harvest(other, open), None);
        assert_eq!(harvest(tank, open), None);

        let click = CellActionQuery {
            mouse_over: false,
            shift: false,
        };
        assert_eq!(
            sim.cell_move_action(owner, own, taken, click, rules, Some(&registry)),
            Ok(MoveAction::NoMove)
        );
        assert_eq!(harvest(own, taken), None);
        assert!(
            crate::app::input::commands::ordinary_cell_move_goal(
                &sim,
                rules,
                Some(&registry),
                owner,
                own,
                taken,
                false,
            )
            .is_some()
        );
    }

    /// The native selection dispatcher clamps its shared Cell before asking
    /// receivers' What_Action and ClickedAction (`0x004AE77E`), then each Foot
    /// resolves from that same Cell (`0x004AE8BA`, `0x004AE921`, `0x004AE976`).
    /// At these retail edge clicks the raw Cell answers NoMove while the
    /// clamped one answers the receiver's real action; an interior Cell is
    /// unchanged. The click path's call site itself is not driven here.
    #[test]
    fn mobile_cell_dispatch_clamps_before_action_and_goal_resolution() {
        use crate::sim::world::{CellActionQuery, MoveAction};

        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let tank = sim
            .spawn_object("MTNK", "Local", 12, 4, 0, &rules)
            .expect("retail tank spawns");
        sim.resolve_type_handles(&rules);
        let action = |cell| {
            sim.cell_move_action(
                owner,
                tank,
                cell,
                CellActionQuery {
                    mouse_over: false,
                    shift: false,
                },
                &rules,
                None,
            )
            .unwrap()
        };

        for (raw, expected_action) in [((7, 0), MoveAction::NoMove), ((16, 0), MoveAction::Move)] {
            let clamped = sim.clamped_click_cell(raw);
            assert_ne!(clamped, raw, "fixed edge input {raw:?} must clamp");
            assert_eq!(action(raw), MoveAction::NoMove, "raw edge {raw:?}");
            assert_eq!(action(clamped), expected_action, "clamped edge {raw:?}");
        }
        assert_eq!(sim.clamped_click_cell((9, 5)), (9, 5));
    }

    /// An all-berserk selection's left release follows the best object's
    /// class rung: Infantry NoMove (`0x0051F05E`, cell `0x0051FAAB`), Unit
    /// NoMove over an object (`0x007403EB`) but 0 on the ground
    /// (`0x007404B0`), JumpJet infantry 0 on the ground, and Aircraft 0
    /// (`0x00417D7C`). The best is the nearest berserk object.
    #[test]
    fn an_all_berserk_click_follows_the_best_objects_class() {
        use LeftRelease::{Dispatch, Nothing, Select};
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=GI\n1=JUMPJET\n[VehicleTypes]\n0=MTNK\n\
                 [AircraftTypes]\n0=ORCA\n[GI]\nStrength=125\n\
                 [JUMPJET]\nStrength=125\nJumpJet=yes\n[MTNK]\nStrength=300\n\
                 [ORCA]\nStrength=200\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let mut spawn = |kind: &str, rx: u16, berserk: bool| {
            let id = sim
                .spawn_object(kind, "Americans", rx, 5, 0, &rules)
                .unwrap();
            sim.entities_mut_for_test()
                .get_mut(id)
                .unwrap()
                .berserk
                .set_active_for_test(berserk);
            id
        };
        let gi = spawn("GI", 3, true);
        let jumpjet = spawn("JUMPJET", 4, true);
        let tank = spawn("MTNK", 6, true);
        let orca = spawn("ORCA", 8, true);
        let friend = spawn("GI", 10, false);
        let over_friend = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: friend,
        };
        let click = |selection: &[u64], hover: Option<&HoverTargetKindWithId>, cell| {
            berserk_selection_click(&sim, &rules, selection, hover, cell, owner)
        };
        assert_eq!(click(&[gi], None, (3, 5)), Dispatch);
        assert_eq!(click(&[gi], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[jumpjet], None, (3, 5)), Nothing);
        assert_eq!(click(&[jumpjet], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[tank], None, (3, 5)), Nothing);
        assert_eq!(click(&[tank], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[orca], None, (3, 5)), Nothing);
        assert_eq!(click(&[orca], Some(&over_friend), (3, 5)), Select);
        // Equal rank 0: the object nearer the clicked cell decides.
        assert_eq!(click(&[tank, gi], None, (3, 5)), Dispatch);
        assert_eq!(click(&[gi, tank], None, (6, 5)), Nothing);
        // 0 over an object already selected does nothing (`0x004ABE0B`).
        sim.entities_mut_for_test()
            .get_mut(friend)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        assert_eq!(
            berserk_selection_click(&sim, &rules, &[orca], Some(&over_friend), (3, 5), owner),
            Nothing
        );
    }

    /// The Ivan's self-bomb gate counts the whole selection (`[0xA8ECC8]`):
    /// a calm Ivan clicked with a berserk escort still bombs himself, though
    /// he is the only object that can send the order.
    #[test]
    fn the_ivan_self_bomb_gate_counts_the_whole_selection() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n[IVAN]\nStrength=100\nIvan=yes\n\
                 AttackCursorOnFriendlies=yes\nBombable=yes\nPrimary=IvanBomber\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Invisible]\nInviso=yes\n[IvanBomb]\nIvanBomb=yes\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let americans = sim.intern("Americans");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                americans, 0, None, false, 0, 10,
            ));
        // The rung is a human player's (`0x007003ED`).
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(americans));
        let ivan = sim
            .spawn_object("IVAN", "Americans", 3, 5, 0, &rules)
            .unwrap();
        assert!(friendly_bomb_ivans(&sim, &rules, &[ivan], 1, ivan, None).is_empty());
        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &[ivan], 2, ivan, None),
            [ivan]
        );
    }

    /// Role-derived Infiltrate must preserve native C4-before-Capture order,
    /// including terminal ordinary Attack when the building refuses C4.
    #[test]
    fn specialist_infiltration_keeps_c4_click_and_cursor_priority() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=GHOST\n1=SPY\n[BuildingTypes]\n0=PLANT\n1=BUNK\n2=FOLD\n[VehicleTypes]\n0=TRUCK\n\
                 [GHOST]\nStrength=100\nC4=yes\nPrimary=Rifle\n\
                 [SPY]\nStrength=100\nAgent=yes\n\
                 [PLANT]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=yes\n\
                 [BUNK]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=no\n\
                 [FOLD]\nStrength=750\nCapturable=yes\nSpyable=yes\nUndeploysInto=TRUCK\n\
                 [TRUCK]\nStrength=100\n\
                 [Rifle]\nDamage=10\nRange=8\nProjectile=Invisible\nWarhead=AP\n\
                 [Invisible]\nInviso=yes\n\
                 [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ),
        ).unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (name, human) in [("Americans", true), ("Soviets", false)] {
            let owner = sim.intern(name);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, human, 0, 10,
                ));
        }
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
        let seal = sim
            .spawn_object("GHOST", "Americans", 3, 5, 0, &rules)
            .unwrap();
        let spy = sim
            .spawn_object("SPY", "Americans", 4, 5, 0, &rules)
            .unwrap();
        let plant = sim
            .spawn_object("PLANT", "Soviets", 6, 5, 0, &rules)
            .unwrap();
        let bunker = sim
            .spawn_object("BUNK", "Soviets", 8, 5, 0, &rules)
            .unwrap();
        let fold = sim
            .spawn_object("FOLD", "Soviets", 10, 5, 0, &rules)
            .unwrap();
        assert!(rules.object("GHOST").unwrap().infiltrate);
        assert!(rules.object("SPY").unwrap().infiltrate);
        for (target, expected_action, expected_cursor) in [
            (
                plant,
                InfantryC4Action::Sabotage,
                crate::app::types::CursorFeedbackKind::Demolish,
            ),
            (
                bunker,
                InfantryC4Action::Attack,
                crate::app::types::CursorFeedbackKind::EnemyStructure,
            ),
        ] {
            let hover = HoverTargetKindWithId {
                kind: HoverTargetKind::EnemyStructure,
                stable_id: target,
            };
            assert!(sim.infiltrator_building_action(seal, target, &rules));
            let mut selected = vec![seal, spy];
            assert_eq!(
                infiltrator_capture_orders(&sim, &rules, &mut selected, &hover),
                Some(vec![Command::CaptureBuilding {
                    engineer_id: spy,
                    target_building_id: target
                }])
            );
            assert_eq!(selected, [seal]);
            assert_eq!(
                infantry_c4_action(&sim, &rules, seal, &hover, false),
                Some(expected_action)
            );
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[seal],
                    Some(seal),
                    &hover,
                    Some(&rules),
                    None,
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
                ),
                expected_cursor
            );
        }
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyStructure,
            stable_id: fold,
        };
        assert!(rules.object("FOLD").unwrap().is_1x1_with_undeploy());
        assert_eq!(infantry_c4_action(&sim, &rules, seal, &hover, false), None);
        assert_eq!(
            infiltrator_capture_orders(&sim, &rules, &mut vec![seal], &hover),
            Some(vec![Command::CaptureBuilding {
                engineer_id: seal,
                target_building_id: fold
            }])
        );
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[seal],
                Some(seal),
                &hover,
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
            ),
            crate::app::types::CursorFeedbackKind::Enter
        );
    }

    /// `TechnoClass::What_Action_OnObject @ 0x00700548`: unforced, the
    /// attack rung answers Attack only while the actor's GetFireError for the
    /// object is not ILLEGAL; otherwise the tail answers, and a player's
    /// Infantry turns its 0 into NoMove. Force fire keeps the rung
    /// (`0x00700555`). Out of range is RANGE (T61, the last test), not
    /// ILLEGAL. A Crazy Ivan's bombed target is ILLEGAL at any distance (T55,
    /// `0x006FCBAD`, comes first). ClickedMission promotes only an Attack
    /// that passed, so an attack-move click needs it too.
    #[test]
    fn an_unforced_click_refuses_only_an_illegal_shot() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n1=GI\n[VehicleTypes]\n0=HTNK\n\
                 [CombatDamage]\nIvanTimedDelay=450\n\
                 [IVAN]\nStrength=125\nPrimary=IvanBomber\nIvan=yes\n\
                 [GI]\nStrength=125\nPrimary=Pea\n\
                 [HTNK]\nStrength=900\nArmor=heavy\nPrimary=120mm\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
                 [120mm]\nDamage=90\nRange=5.75\nProjectile=Invisible\nWarhead=AP\n\
                 [Invisible]\nInviso=yes\n\
                 [IvanBomb]\nIvanBomb=yes\n\
                 [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, false, 0, 10,
                ));
        }
        // The attack rung is a human player's (`0x007003ED`).
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let tank = spawn("HTNK", "Americans", 3);
        let gi = spawn("GI", "Americans", 4);
        let ivan = spawn("IVAN", "Americans", 5);
        let near = spawn("HTNK", "Soviets", 6);
        let far = spawn("HTNK", "Soviets", 20);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |sim: &Simulation, actor: u64, target: u64, forced: bool| {
            attack_action(
                sim,
                &rules,
                actor,
                &TargetKind::Entity(target),
                forced,
                Some(&overlays),
            )
        };

        assert_eq!(
            action(&sim, gi, near, false),
            AttackAction::NoMove,
            "0% Verses against heavy armour"
        );
        assert_eq!(action(&sim, gi, near, true), AttackAction::Attack);
        assert_eq!(action(&sim, tank, near, false), AttackAction::Attack);
        assert_eq!(
            action(&sim, tank, far, false),
            AttackAction::Attack,
            "out of range"
        );
        assert_eq!(action(&sim, ivan, near, false), AttackAction::Attack);
        let americans = sim.interner().get("Americans").unwrap();
        assert_eq!(
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                americans,
                OrderMode::AttackMove,
                OrderModifier::Normal,
                false,
                tank,
                near,
            ),
            Some(Command::AttackMove {
                entity_id: tank,
                target: Some(TargetKind::Entity(near)),
                destination: None,
            })
        );

        sim.bomb_attach_for_test(ivan, Some(near), &rules);
        sim.bomb_attach_for_test(ivan, Some(far), &rules);
        assert_eq!(
            action(&sim, ivan, near, false),
            AttackAction::NoMove,
            "a bombed target"
        );
        assert_eq!(
            action(&sim, ivan, far, false),
            AttackAction::NoMove,
            "the bomb gate precedes the range test"
        );
    }

    /// The Infiltrate arm keeps an unforced ILLEGAL shot on the rung
    /// (`0x007004A0..0x00700531`): an `Agent=` infantryman over a `Spyable=`
    /// building, another over a `Capturable=` one, and a `C4=` one over a
    /// building that is `CanC4=` (a building's default). Any other building
    /// takes the tail. The reader also enables Infiltrate for C4, Engineer
    /// and Agent types without an explicit Infiltrate key.
    #[test]
    fn the_infiltrate_arm_keeps_an_illegal_shot_on_the_rung() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=SPY\n1=ENGI\n2=SEAL\n\
                 [BuildingTypes]\n0=LAB\n1=DEPOT\n2=BUNK\n\
                 [SPY]\nStrength=100\nInfiltrate=yes\nAgent=yes\nPrimary=Pea\n\
                 [ENGI]\nStrength=100\nInfiltrate=yes\nPrimary=Pea\n\
                 [SEAL]\nStrength=100\nInfiltrate=yes\nC4=yes\nPrimary=Pea\n\
                 [LAB]\nStrength=750\nArmor=heavy\nSpyable=yes\n\
                 [DEPOT]\nStrength=750\nArmor=heavy\nCapturable=yes\n\
                 [BUNK]\nStrength=750\nArmor=heavy\nCanC4=no\n\
                 [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
                 [Invisible]\nInviso=yes\n\
                 [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (house, human) in [("Americans", true), ("Soviets", false)] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, human, 0, 10,
                ));
        }
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
        let mut spawn = |kind: &str, owner: &str, rx: u16, ry: u16| {
            sim.spawn_object(kind, owner, rx, ry, 0, &rules).unwrap()
        };
        let spy = spawn("SPY", "Americans", 4, 5);
        let engineer = spawn("ENGI", "Americans", 5, 5);
        let seal = spawn("SEAL", "Americans", 5, 8);
        let lab = spawn("LAB", "Soviets", 7, 5);
        let depot = spawn("DEPOT", "Soviets", 7, 8);
        let bunker = spawn("BUNK", "Soviets", 10, 8);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |actor, target| {
            attack_action(
                &sim,
                &rules,
                actor,
                &TargetKind::Entity(target),
                false,
                Some(&overlays),
            )
        };
        assert_eq!(action(spy, lab), AttackAction::Attack);
        assert_eq!(action(spy, depot), AttackAction::NoMove);
        assert_eq!(action(engineer, depot), AttackAction::Attack);
        assert_eq!(action(engineer, lab), AttackAction::NoMove);
        assert_eq!(action(seal, lab), AttackAction::Attack);
        assert_eq!(action(seal, bunker), AttackAction::NoMove);
    }

    /// A shot ILLEGAL to an armed receiver leaves its What_Action 0
    /// (`0x0070056C..0x007005E6`), which a player's Infantry or Unit it may
    /// control turns into NoMove (`0x0051F05E..0x0051F08B`,
    /// `0x007403EB..0x00740414`): the click moves it near the object, as an
    /// attack-move under the chord, and the cursor shows NoMove. A retail
    /// Crazy Ivan's bombed target is ILLEGAL to him (`0x006FCBAD`), and a
    /// retail Grizzly is ILLEGAL to an attack dog.
    #[test]
    fn a_refused_shot_moves_the_receiver_near_the_object() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let ivan = sim.spawn_object("IVAN", "Local", 6, 4, 0, &rules).unwrap();
        let dog = sim.spawn_object("DOG", "Local", 5, 4, 0, &rules).unwrap();
        let tank = sim.spawn_object("MTNK", "Enemy", 6, 6, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        sim.bomb_attach_for_test(ivan, Some(tank), &rules);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        for actor in [ivan, dog] {
            assert_eq!(
                attack_action(
                    &sim,
                    &rules,
                    actor,
                    &TargetKind::Entity(tank),
                    false,
                    Some(&overlays)
                ),
                AttackAction::NoMove
            );
            // The cursor shows that NoMove too.
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[actor],
                    Some(actor),
                    &HoverTargetKindWithId {
                        kind: HoverTargetKind::EnemyUnit,
                        stable_id: tank,
                    },
                    Some(&rules),
                    None,
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
                ),
                crate::app::types::CursorFeedbackKind::Invalid
            );
        }
        let click = |order_mode| {
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                order_mode,
                OrderModifier::Normal,
                false,
                ivan,
                tank,
            )
        };
        let near = crate::app::input::commands::object_move_destination(
            &sim,
            &rules,
            owner,
            ivan,
            tank,
            crate::sim::world::MoveAction::NoMove,
            false,
        );
        let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
            panic!("NoMove searches near the tank: {near:?}");
        };
        assert_eq!(
            click(OrderMode::Move),
            Some(Command::Move {
                entity_id: ivan,
                target_rx,
                target_ry,
            })
        );
        assert_eq!(
            click(OrderMode::AttackMove),
            Some(Command::AttackMove {
                entity_id: ivan,
                target: None,
                destination: near,
            })
        );
    }

    /// An unarmed object's What_Action over another house's object skips the
    /// attack rung (`0x00700464..0x00700474`) for the tail
    /// (`0x0070059D..0x007005E6`): Select over an object the player may
    /// select and has not, else 0, which a player's Unit it may control
    /// turns into NoMove (`0x007403EB`). A loaner the player owns answers 0.
    /// The click selects the object when the object that owns the cursor
    /// answers Select (`0x004ABDD3..0x004ABDDA`), and an armed object
    /// outranks unarmed ones (`0x00535435..0x0053549C`); NoMove moves the
    /// receiver near the object. Retail `[CMIN]` (`Primary=none`) and
    /// `[AMCV]` are unarmed. Another house's armed object asks no rung
    /// (`0x007003ED`) and no owner test, so it answers Select.
    #[test]
    fn an_unarmed_receiver_selects_another_houses_object() {
        use crate::sim::world::MoveAction;
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let miner = sim.spawn_object("CMIN", "Local", 2, 4, 0, &rules).unwrap();
        let mcv = sim.spawn_object("AMCV", "Local", 2, 6, 0, &rules).unwrap();
        let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
        let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |sim: &Simulation, actor| {
            attack_action(sim, &rules, actor, &TargetKind::Entity(enemy), false, None)
        };
        let release = |sim: &Simulation, selection: &[u64]| {
            object_click_release(sim, &rules, selection, selection, enemy, &overlays, false)
        };
        let click = |sim: &Simulation, actor| {
            object_attack_click_command(
                sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::Normal,
                false,
                actor,
                enemy,
            )
        };
        let cursor = |sim: &Simulation, actor| {
            crate::app::input::cursor::capability_cursor_for_hover(
                sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyUnit,
                    stable_id: enemy,
                },
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
            )
        };

        assert_eq!(action(&sim, miner), AttackAction::Select);
        assert_eq!(action(&sim, mcv), AttackAction::Select);
        assert_eq!(action(&sim, tank), AttackAction::Attack);
        assert_eq!(
            cursor(&sim, miner),
            crate::app::types::CursorFeedbackKind::FriendlyUnit
        );
        assert_eq!(release(&sim, &[miner, mcv]), LeftRelease::Select);
        assert_eq!(
            release(&sim, &[miner, tank]),
            LeftRelease::Dispatch,
            "the armed tank owns the cursor"
        );
        assert_eq!(
            object_click_release(&sim, &rules, &[mcv], &[], enemy, &overlays, false),
            LeftRelease::Dispatch,
            "a capability arm took the object that owns the cursor"
        );
        assert_eq!(click(&sim, mcv), None);

        sim.entities_mut_for_test()
            .get_mut(enemy)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        assert_eq!(action(&sim, mcv), AttackAction::NoMove);
        assert_eq!(
            cursor(&sim, mcv),
            crate::app::types::CursorFeedbackKind::Invalid
        );
        assert_eq!(release(&sim, &[mcv]), LeftRelease::Dispatch);
        let near = crate::app::input::commands::object_move_destination(
            &sim,
            &rules,
            owner,
            mcv,
            enemy,
            MoveAction::NoMove,
            false,
        );
        let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
            panic!("NoMove searches near the tank: {near:?}");
        };
        assert_eq!(
            click(&sim, mcv),
            Some(Command::Move {
                entity_id: mcv,
                target_rx,
                target_ry,
            })
        );
        sim.entities_mut_for_test()
            .get_mut(enemy)
            .unwrap()
            .selection
            .set_selected_for_test(false);

        let loaner = sim.entities_mut_for_test().get_mut(mcv).unwrap();
        loaner.mission_only.mark();
        loaner.discovery.set_owned_by_current_house_for_test(true);
        assert_eq!(action(&sim, mcv), AttackAction::NoMove);

        assert_eq!(
            attack_action(&sim, &rules, enemy, &TargetKind::Entity(tank), false, None),
            AttackAction::Select
        );
    }

    /// `AircraftClass::What_Action` over an object (`0x00417CC0`): the Foot
    /// base's 0 becomes the aircraft's own What_Action on the Cell under the
    /// object (`0x00417D7C..0x00417DCB`). A retail Harrier (`ORCA`) over an
    /// enemy Kirov in flight, which its `AirToGroundMissile` cannot hit, and
    /// over an allied Rocketeer, answers Move: the Move cursor, a dispatched
    /// release and a Move order towards the object. Attack over an `Immune=`
    /// type is NoMove (`0x00417F4A..0x00417F63`), which the click refuses
    /// (`0x00417C12`).
    #[test]
    fn a_harrier_moves_to_an_object_it_may_not_attack() {
        use crate::app::types::CursorFeedbackKind;
        let patch = crate::rules::ini_parser::IniFile::from_str("[MTNK]\nImmune=yes\n");
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        ) else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let harrier = sim.spawn_object("ORCA", "Local", 5, 4, 0, &rules).unwrap();
        let kirov = sim.spawn_object("ZEP", "Enemy", 7, 6, 0, &rules).unwrap();
        let rocketeer = sim
            .spawn_object("JUMPJET", "Ally", 6, 8, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Enemy", 12, 10, 0, &rules)
            .unwrap();
        let own_tank = sim.spawn_object("MTNK", "Local", 6, 4, 0, &rules).unwrap();
        let wingman = sim.spawn_object("ORCA", "Local", 6, 5, 0, &rules).unwrap();
        sim.entities_mut_for_test()
            .get_mut(wingman)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        for flier in [kirov, rocketeer] {
            sim.entities_mut_for_test()
                .get_mut(flier)
                .unwrap()
                .position
                .set_exact_z_leptons_for_test(Some(700));
        }
        for (asker, other) in [("LOCAL", "ALLY"), ("ALLY", "LOCAL")] {
            sim.house_alliances_mut_for_test()
                .entry(asker.to_string())
                .or_default()
                .insert(other.to_string());
        }
        sim.resolve_type_handles(&rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |target| {
            attack_action(
                &sim,
                &rules,
                harrier,
                &TargetKind::Entity(target),
                false,
                None,
            )
        };
        let cursor = |target, kind| {
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[harrier],
                Some(harrier),
                &HoverTargetKindWithId {
                    kind,
                    stable_id: target,
                },
                Some(&rules),
                None,
                OrderModifier::Normal,
                false,
                false,
            )
        };
        let click = |target| {
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::Normal,
                false,
                harrier,
                target,
            )
        };

        for (target, kind) in [
            (kirov, HoverTargetKind::EnemyUnit),
            (rocketeer, HoverTargetKind::FriendlyUnit),
        ] {
            assert_eq!(action(target), AttackAction::Move);
            assert_eq!(cursor(target, kind), CursorFeedbackKind::Move);
            assert_eq!(
                object_click_release(
                    &sim,
                    &rules,
                    &[harrier],
                    &[harrier],
                    target,
                    &overlays,
                    false
                ),
                LeftRelease::Dispatch
            );
            assert!(
                matches!(
                    click(target),
                    Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                        if entity_id == harrier
                ),
                "{:?}",
                click(target)
            );
        }
        assert_eq!(action(tank), AttackAction::NoMove);
        assert_eq!(
            cursor(tank, HoverTargetKind::EnemyUnit),
            CursorFeedbackKind::Invalid
        );
        assert_eq!(click(tank), None);

        // Own objects: one not selected is Techno's Select (7); a selected
        // one in a larger selection is 0, so the Cell action, Move; a lone
        // Harrier over itself is Self, which 417DCD turns into 0 (the arrow).
        assert_eq!(action(own_tank), AttackAction::Select);
        assert_eq!(
            cursor(own_tank, HoverTargetKind::FriendlyUnit),
            CursorFeedbackKind::FriendlyUnit
        );
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[harrier, wingman],
                Some(harrier),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::FriendlyUnit,
                    stable_id: wingman,
                },
                Some(&rules),
                None,
                OrderModifier::Normal,
                false,
                false,
            ),
            CursorFeedbackKind::Move
        );
        assert_eq!(
            cursor(harrier, HoverTargetKind::FriendlyUnit),
            CursorFeedbackKind::Default
        );
    }

    /// The Harrier's Cell query under the object (`0x00417D7C..0x00417DCB`)
    /// reaches Techno's unforced Cell Attack rung (`0x007008A6..0x007009F1`):
    /// over an enemy Kirov in flight above another house's GAWALL, which the
    /// Harrier's `ORCAAP` (`Wall=yes`) may hit, the answer is Attack, not
    /// Move: the attack cursor, and the click attacks the Kirov (Aircraft
    /// `0x00417BD0` -> Foot `0x004D74E0`). The allied Rocketeer over such a
    /// wall answers Attack too. Over the player's own wall it stays Move.
    #[test]
    fn a_harrier_over_a_flier_above_an_enemy_wall_answers_attack() {
        use crate::app::types::CursorFeedbackKind;
        let patch = crate::rules::ini_parser::IniFile::from_str("[DISK]\nImmune=yes\n");
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        ) else {
            return;
        };
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &retail.processed_rules,
            Some(retail.fixed_art.as_ref()),
        );
        let wall = overlays.id_for_name("GAWALL").expect("retail GAWALL");
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner().get("Local").unwrap();
        let harrier = sim.spawn_object("ORCA", "Local", 5, 4, 0, &rules).unwrap();
        let kirov = sim.spawn_object("ZEP", "Enemy", 7, 6, 0, &rules).unwrap();
        let rocketeer = sim
            .spawn_object("JUMPJET", "Ally", 6, 8, 0, &rules)
            .unwrap();
        let own_wall_kirov = sim.spawn_object("ZEP", "Enemy", 10, 6, 0, &rules).unwrap();
        let shrouded_kirov = sim.spawn_object("ZEP", "Enemy", 12, 6, 0, &rules).unwrap();
        let disc = sim.spawn_object("DISK", "Enemy", 9, 9, 0, &rules).unwrap();
        for flier in [kirov, rocketeer, own_wall_kirov, shrouded_kirov, disc] {
            sim.entities_mut_for_test()
                .get_mut(flier)
                .unwrap()
                .position
                .set_exact_z_leptons_for_test(Some(700));
        }
        for (asker, other) in [("LOCAL", "ALLY"), ("ALLY", "LOCAL")] {
            sim.house_alliances_mut_for_test()
                .entry(asker.to_string())
                .or_default()
                .insert(other.to_string());
        }
        let enemy = sim.interner().get("Enemy").unwrap();
        let (width, height) = {
            let terrain = sim.resolved_terrain().unwrap();
            (terrain.width(), terrain.height())
        };
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(width, height);
        for cell in [(7, 6), (6, 8), (12, 6), (9, 9)] {
            grid.place_owned_wall(cell.0, cell.1, wall, 0, enemy);
        }
        grid.place_owned_wall(10, 6, wall, 0, owner);
        *sim.overlay_grid_mut_for_test() = Some(grid);
        sim.resolve_type_handles(&rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test().reveal_cells_for_owner(
            owner,
            (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|&cell| cell != (12, 6)),
        );
        let action = |target| {
            attack_action(
                &sim,
                &rules,
                harrier,
                &TargetKind::Entity(target),
                false,
                Some(&overlays),
            )
        };
        let cursor = |target, kind| {
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[harrier],
                Some(harrier),
                &HoverTargetKindWithId {
                    kind,
                    stable_id: target,
                },
                Some(&rules),
                Some(&overlays),
                OrderModifier::Normal,
                false,
                false,
            )
        };
        let click = |target| {
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::Normal,
                false,
                harrier,
                target,
            )
        };
        // The ally's Rocketeer is a friend: without the wall it would be the
        // Move of `a_harrier_moves_to_an_object_it_may_not_attack`.
        for (target, kind) in [
            (kirov, HoverTargetKind::EnemyUnit),
            (rocketeer, HoverTargetKind::FriendlyUnit),
        ] {
            assert_eq!(action(target), AttackAction::Attack);
            assert!(
                matches!(
                    cursor(target, kind),
                    CursorFeedbackKind::EnemyUnit | CursorFeedbackKind::EnemyOutOfRange
                ),
                "{:?}",
                cursor(target, kind)
            );
            assert_eq!(
                object_click_release(
                    &sim,
                    &rules,
                    &[harrier],
                    &[harrier],
                    target,
                    &overlays,
                    false
                ),
                LeftRelease::Dispatch
            );
            assert_eq!(
                click(target),
                Some(Command::Attack {
                    attacker_id: harrier,
                    target_id: target,
                })
            );
        }
        // The player's own wall admits no rung; the shroud wrapper takes the
        // Attack back over a shrouded Cell; Attack over an `Immune=` object is
        // NoMove (`0x00417F4A`).
        assert_eq!(action(own_wall_kirov), AttackAction::Move);
        assert_ne!(action(shrouded_kirov), AttackAction::Attack);
        assert_eq!(action(disc), AttackAction::NoMove);
    }

    /// Unit (`0x0074041B..0x0074049A`) and Infantry
    /// (`0x0051F160..0x0051F179`) What_Action over an object turn a final
    /// Attack over an `Immune=` type (`+0x233`) into NoMove, with or without
    /// Ctrl: the NoMove cursor, and the click is Foot ClickedAction's Move
    /// (`0x004D74E0`), not an Attack. Retail's `Immune=yes` object is the
    /// bridge repair hut (`CABHUT`); the patched Grizzly shows the same answer
    /// over a unit. A Rhino and a GI keep Attack over another enemy tank.
    /// Infantry's C4 (`0x0051E9C1..0x0051EA56`) and Crazy Ivan
    /// (`0x0051EB24..0x0051EB7E`) arms return before its Immune tail, so
    /// Tanya still sabotages the hut and an Ivan still bombs it.
    #[test]
    fn a_tank_and_a_gi_refuse_to_attack_an_immune_object() {
        use crate::app::types::CursorFeedbackKind;
        let patch = crate::rules::ini_parser::IniFile::from_str("[MTNK]\nImmune=yes\n");
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        ) else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        assert!(rules.object("CABHUT").is_some_and(|hut| hut.immune));
        let owner = sim.interner().get("Local").unwrap();
        let rhino = sim.spawn_object("HTNK", "Local", 5, 4, 0, &rules).unwrap();
        let gi = sim.spawn_object("E1", "Local", 5, 6, 0, &rules).unwrap();
        let grizzly = sim.spawn_object("MTNK", "Enemy", 4, 11, 0, &rules).unwrap();
        let enemy_rhino = sim.spawn_object("HTNK", "Enemy", 6, 8, 0, &rules).unwrap();
        let hut = sim
            .spawn_object("CABHUT", "Enemy", 13, 13, 0, &rules)
            .unwrap();
        let ivan = sim.spawn_object("IVAN", "Local", 4, 6, 0, &rules).unwrap();
        let tanya = sim.spawn_object("TANY", "Local", 4, 4, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
        sim.fog_mut_for_test()
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        for actor in [rhino, gi] {
            let action = |target, forced| {
                attack_action(
                    &sim,
                    &rules,
                    actor,
                    &TargetKind::Entity(target),
                    forced,
                    None,
                )
            };
            let cursor = |target, kind| {
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[actor],
                    Some(actor),
                    &HoverTargetKindWithId {
                        kind,
                        stable_id: target,
                    },
                    Some(&rules),
                    None,
                    OrderModifier::Normal,
                    false,
                    false,
                )
            };
            let click = |target| {
                object_attack_click_command(
                    &sim,
                    &rules,
                    &overlays,
                    owner,
                    OrderMode::Move,
                    OrderModifier::Normal,
                    false,
                    actor,
                    target,
                )
            };
            assert_eq!(action(enemy_rhino, false), AttackAction::Attack);
            assert_eq!(
                cursor(enemy_rhino, HoverTargetKind::EnemyUnit),
                CursorFeedbackKind::EnemyUnit
            );
            assert!(matches!(
                click(enemy_rhino),
                Some(Command::Attack { .. } | Command::AttackMove { .. })
            ));
            for (target, kind) in [
                (grizzly, HoverTargetKind::EnemyUnit),
                (hut, HoverTargetKind::EnemyStructure),
            ] {
                if actor == gi {
                    // The Infantry tail is infantry_attack_action's last arm.
                    assert_eq!(action(target, true), AttackAction::Attack, "{target}");
                    assert_eq!(
                        infantry_attack_action(&sim, &rules, gi, target),
                        Some(InfantryAttackAction::NoMove),
                        "{target}"
                    );
                } else {
                    assert_eq!(action(target, false), AttackAction::NoMove, "{target}");
                    assert_eq!(action(target, true), AttackAction::NoMove, "{target}");
                }
                assert_eq!(
                    cursor(target, kind),
                    CursorFeedbackKind::Invalid,
                    "{target}"
                );
                assert!(
                    matches!(
                        click(target),
                        Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                            if entity_id == actor
                    ),
                    "{target}: {:?}",
                    click(target)
                );
            }
        }

        assert_eq!(
            attack_action(&sim, &rules, ivan, &TargetKind::Entity(hut), false, None),
            AttackAction::Attack
        );
        assert_eq!(infantry_attack_action(&sim, &rules, ivan, hut), None);
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[ivan],
                Some(ivan),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyStructure,
                    stable_id: hut,
                },
                Some(&rules),
                None,
                OrderModifier::Normal,
                false,
                false,
            ),
            CursorFeedbackKind::IvanBomb
        );
        assert_eq!(
            infantry_attack_action(&sim, &rules, tanya, hut),
            Some(InfantryAttackAction::Sabotage)
        );
        assert!(matches!(
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::ForceFire,
                false,
                tanya,
                hut,
            ),
            Some(Command::PlantC4 { attacker_id, .. }) if attacker_id == tanya
        ));
    }

    /// What_Action_OnCell's force-fire rung is a human player's too
    /// (`0x007008A6..0x007008B3`): another house's armed object, which the
    /// player may select to inspect, answers 0 over a cell in its range
    /// (`0x00700AB7..0x00700AC4`), where the player's answers Attack.
    #[test]
    fn force_fire_on_a_cell_is_a_human_players() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
        let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        let over_cell =
            |actor| armed_action(&sim, &rules, actor, &TargetKind::Cell(5, 6), true, None);
        assert_eq!(over_cell(tank), Some(AttackAction::Attack));
        assert_eq!(over_cell(enemy), Some(AttackAction::None));
    }

    /// An unarmed `Infiltrate=` infantryman keeps the attack rung
    /// (`0x00700444..0x00700474`), but his class turns its Attack into 0 over
    /// an object he may not infiltrate (`0x0051F04E..0x0051F05C`), and its
    /// tail turns 0 into NoMove (`0x0051F05E..0x0051F08B`). Retail's SPY
    /// carries its MakeupKit, so only modded data reaches this.
    #[test]
    fn an_unarmed_infiltrator_moves_near_an_object_it_may_not_enter() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=AGENT\n[VehicleTypes]\n0=MTNK\n\
                 [AGENT]\nStrength=100\nInfiltrate=yes\nAgent=yes\n[MTNK]\nStrength=300\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (house, human) in [("Americans", true), ("Russians", false)] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, human, 0, 10,
                ));
        }
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = sim.interner().get("Americans");
        sim.session_mut_for_test().set_current_house_for_test(house);
        let agent = sim
            .spawn_object("AGENT", "Americans", 3, 5, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Russians", 6, 5, 0, &rules)
            .unwrap();
        assert_eq!(
            attack_action(&sim, &rules, agent, &TargetKind::Entity(tank), false, None),
            AttackAction::NoMove
        );
    }

    /// The rally click names the local player's selected buildings that take
    /// a rally: not their power plant, not another house's factory, not units.
    #[test]
    fn rally_click_names_only_the_players_rally_buildings() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=GAWEAP\n1=GAPOWR\n[VehicleTypes]\n0=MTNK\n\
                 [GAWEAP]\nFactory=UnitType\n[GAPOWR]\nPower=100\n[MTNK]\nStrength=300\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let other = sim.intern("Russians");
        let factory_type = sim.intern("GAWEAP");
        let power_type = sim.intern("GAPOWR");
        let tank_type = sim.intern("MTNK");
        for (id, rx, house, kind, category) in [
            (1, 10, owner, factory_type, EntityCategory::Structure),
            (2, 11, owner, tank_type, EntityCategory::Unit),
            (3, 12, owner, power_type, EntityCategory::Structure),
            (4, 13, other, factory_type, EntityCategory::Structure),
            (5, 14, owner, factory_type, EntityCategory::Structure),
        ] {
            sim.entities_mut_for_test()
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    rx,
                    10,
                    0,
                    0,
                    house,
                    Health::new(300),
                    kind,
                    category,
                    0,
                    5,
                    category == EntityCategory::Unit,
                ));
        }

        let producer_ids = selected_rally_producer_ids(&sim, &rules, &[5, 4, 3, 2, 1, 5], owner);

        assert_eq!(producer_ids, vec![5, 1]);
    }

    #[test]
    fn refinery_cell_click_archives_before_repacking_and_self_click_does_not_repack() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::sim::arena_fixture::clear_ground_ini(
                "[BuildingTypes]\n0=REFN\n[VehicleTypes]\n0=SMIN\n\
                 [REFN]\nStrength=1000\nUndeploysInto=SMIN\nResourceGatherer=yes\nResourceDestination=yes\n\
                 [SMIN]\nStrength=1000\nSpeed=3\n")).unwrap();
        let mut sim = Simulation::with_seed(6);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let refinery = sim
            .spawn_object("REFN", "Americans", 15, 15, 0, &rules)
            .unwrap();
        let enemy = sim
            .spawn_object("REFN", "Russians", 17, 15, 0, &rules)
            .unwrap();
        let owner = sim.interner().get("Americans").unwrap();
        let (orders, announce) = building_cell_orders(
            &sim,
            &rules,
            None,
            &[refinery, enemy, refinery],
            owner,
            (17, 16),
            false,
            false,
        );
        assert!(
            matches!(orders.as_slice(), [Command::SetRally { producer_ids, .. },
            Command::SellBuilding { entity_id }] if producer_ids == &[refinery] && *entity_id == refinery)
        );
        assert!(
            !announce,
            "ResourceDestination does not announce a factory rally"
        );
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: refinery,
        };
        assert_eq!(
            self_click_command(&sim, &rules, None, owner, &[refinery], Some(&hover), false),
            None
        );
        // A missing input map cannot produce an archive;4437AD still queues SELL.
        *sim.resolved_terrain_mut_for_test() = None;
        let (orders, _) = building_cell_orders(
            &sim,
            &rules,
            None,
            &[refinery],
            owner,
            (17, 16),
            false,
            false,
        );
        assert_eq!(
            orders,
            [Command::SellBuilding {
                entity_id: refinery
            }]
        );
    }

    /// The retail modifier map, one row per chord. Ctrl force-fires, Alt forces
    /// a move, Ctrl+Shift is attack move and Ctrl+Alt is guard area.
    #[test]
    fn modifier_map_matches_retail_chords() {
        // (ctrl, shift, alt) -> verb
        assert_eq!(
            resolve_order_modifiers(false, false, false),
            OrderModifier::Normal
        );
        assert_eq!(
            resolve_order_modifiers(true, false, false),
            OrderModifier::ForceFire
        );
        assert_eq!(
            resolve_order_modifiers(false, false, true),
            OrderModifier::ForceMove
        );
        assert_eq!(
            resolve_order_modifiers(true, true, false),
            OrderModifier::AttackMove
        );
        assert_eq!(
            resolve_order_modifiers(true, false, true),
            OrderModifier::GuardArea
        );
        assert_eq!(
            resolve_order_modifiers(false, true, false),
            OrderModifier::Shift
        );
        // The chord test reads the raw key state, so a held Alt does not defeat
        // attack move, and Shift outranks Alt without Ctrl.
        assert_eq!(
            resolve_order_modifiers(true, true, true),
            OrderModifier::AttackMove
        );
        assert_eq!(
            resolve_order_modifiers(false, true, true),
            OrderModifier::Shift
        );
    }

    /// Each order speaks the voice slot of the mission it commits, and missions
    /// with no dedicated slot fall to the VoiceSpecialAttack default branch.
    #[test]
    fn order_voice_key_follows_the_committed_mission() {
        // A self-click's deploy, unload or deploy-fire is the Unload mission.
        for command in [
            Command::DeployMcv { entity_id: 1 },
            Command::ToggleInfantryDeploy { entity_id: 1 },
            Command::UnloadPassengers { transport_id: 1 },
        ] {
            assert_eq!(order_voice_key(&command), Some("VoiceDeploy"));
        }
        assert_eq!(
            order_voice_key(&Command::Move {
                entity_id: 1,
                target_rx: 0,
                target_ry: 0,
            }),
            Some("VoiceMove")
        );
        // Attack-move commits through the Move voice slot, not the Attack one.
        assert_eq!(
            order_voice_key(&Command::AttackMove {
                entity_id: 1,
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Cell(0, 0)),
            }),
            Some("VoiceMove")
        );
        assert_eq!(
            order_voice_key(&Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }),
            Some("VoiceAttack")
        );
        assert_eq!(
            order_voice_key(&Command::clicked_mission(
                1,
                crate::sim::mission::MissionType::Harvest,
                TargetKind::Cell(0, 0)
            )),
            Some("VoiceHarvest")
        );
        assert_eq!(
            order_voice_key(&Command::clicked_mission(
                1,
                crate::sim::mission::MissionType::Enter,
                TargetKind::Entity(2)
            )),
            Some("VoiceEnter")
        );
        assert_eq!(
            order_voice_key(&Command::CaptureBuilding {
                engineer_id: 1,
                target_building_id: 2,
            }),
            Some("VoiceCapture")
        );
        // Area guard and sabotage have no slot of their own.
        assert_eq!(
            order_voice_key(&Command::Guard {
                entity_id: 1,
                target: None,
            }),
            Some("VoiceSpecialAttack")
        );
        assert_eq!(
            order_voice_key(&Command::PlantC4 {
                attacker_id: 1,
                target_building_id: 2,
            }),
            Some("VoiceSpecialAttack")
        );
    }

    fn chord_rules() -> crate::rules::ruleset::RuleSet {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             1=HARV\n\
             2=SECONLY\n\
             3=CMIN\n\
             4=SHAD\n\
             5=SREF\n\
             [AircraftTypes]\n\
             0=ORCA\n\
             [BuildingTypes]\n\
             0=GAWEAP\n\
             [MTNK]\n\
             Strength=300\n\
             Primary=105mm\n\
             [HARV]\n\
             Strength=1000\n\
             Harvester=yes\n\
             Primary=105mm\n\
             [CMIN]\n\
             Strength=1000\n\
             Harvester=yes\n\
             Primary=none\n\
             [SHAD]\n\
             Strength=200\n\
             Primary=105mm\n\
             PreventAttackMove=yes\n\
             [SECONLY]\n\
             Strength=200\n\
             Secondary=105mm\n\
             [SREF]\n\
             Strength=200\n\
             TurretCount=4\n\
             WeaponCount=1\n\
             Weapon1=105mm\n\
             [ORCA]\n\
             Strength=200\n\
             Primary=105mm\n\
             [GAWEAP]\n\
             Strength=1000\n\
             Primary=105mm\n\
             [WeaponTypes]\n\
             0=105mm\n\
             [105mm]\n\
             Damage=60\n\
             Range=5\n",
        );
        crate::rules::ruleset::RuleSet::from_ini(&ini).expect("chord rules")
    }

    /// Insert an entity of an explicit category, so the building and aircraft
    /// halves of the type rule can be exercised directly.
    fn insert_typed(
        sim: &mut Simulation,
        stable_id: u64,
        type_name: &str,
        category: EntityCategory,
    ) -> u64 {
        let owner = sim.intern("Americans");
        let type_ref = sim.intern(type_name);
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                stable_id,
                5,
                5,
                0,
                0,
                owner,
                Health::new(300),
                type_ref,
                category,
                0,
                5,
                true,
            ));
        stable_id
    }

    /// The retail per-type rule: a real `Primary=` and nothing else.
    ///
    /// `Secondary=` is not consulted, `Primary=none` counts as unarmed, and
    /// being a harvester is irrelevant — the armed War Miner accepts the order
    /// while the Chrono Miner is refused for having no primary weapon.
    #[test]
    fn attack_move_eligibility_follows_the_primary_weapon() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
            .expect("unarmed miner");
        let arty = sim
            .spawn_object("SECONLY", "Americans", 7, 5, 0, &rules)
            .expect("secondary-only unit");

        assert!(entity_can_attack_move(&sim, Some(&rules), tank));
        // An armed harvester is an ordinary member of a defended-expansion group.
        assert!(entity_can_attack_move(&sim, Some(&rules), war_miner));
        // `Primary=none` resolves to no weapon at all.
        assert!(!entity_can_attack_move(&sim, Some(&rules), chrono_miner));
        // A secondary-only type is refused: the rule reads Primary only.
        assert!(!entity_can_attack_move(&sim, Some(&rules), arty));
    }

    /// GSI-08.02 regression: a `TurretCount>0` type carries its weapon in the
    /// `Primary` field even though it authors no `Primary=` key, because
    /// `Weapon1=` and `Primary=` are the same storage
    /// (`TechnoTypeClass::ReadINI`: cursor `0x007128D6 LEA EDI,[EBP+0xA94]`,
    /// base store `0x0071294A MOV [EDI-0x1FC],EAX`, and `0xA94-0x1FC = 0x898`
    /// which is the `Primary` field `Can_Attack_Move @ 0x00711E90` reads).
    ///
    /// The group case is the one a player feels: `selection_can_attack_move` is
    /// an `.all(..)`, so a single Prism Tank answering "no" used to silently
    /// disable the attack-move chord for every other unit selected with it.
    #[test]
    fn gsi_08_02_turret_count_type_attack_moves_through_weapon_one() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let prism = sim
            .spawn_object("SREF", "Americans", 7, 6, 0, &rules)
            .expect("TurretCount type");
        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");

        assert!(entity_can_attack_move(&sim, Some(&rules), prism));
        assert!(selection_can_attack_move(
            &sim,
            Some(&rules),
            &[prism, tank]
        ));
    }

    /// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90` reads BOTH halves: a real
    /// `Primary=` and `PreventAttackMove=` off. The nine stock types that carry
    /// the key and are not aircraft — the three Engineers, the Spy, Boris and
    /// the four helicopters — all have a real primary, so the weapon half alone
    /// let every one of them through.
    #[test]
    fn gsi_07_34_prevent_attack_move_refuses_an_armed_type() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let nighthawk = sim
            .spawn_object("SHAD", "Americans", 5, 6, 0, &rules)
            .expect("helicopter carrying PreventAttackMove=yes");
        assert!(!entity_can_attack_move(&sim, Some(&rules), nighthawk));
    }

    /// Buildings and aircraft answer no unconditionally, whatever they are armed
    /// with.
    #[test]
    fn buildings_and_aircraft_never_attack_move() {
        let rules = chord_rules();
        let mut sim = Simulation::new();

        let factory = insert_typed(&mut sim, 1, "GAWEAP", EntityCategory::Structure);
        let orca = insert_typed(&mut sim, 2, "ORCA", EntityCategory::Aircraft);
        let tank = insert_typed(&mut sim, 3, "MTNK", EntityCategory::Unit);

        assert!(!entity_can_attack_move(&sim, Some(&rules), factory));
        assert!(!entity_can_attack_move(&sim, Some(&rules), orca));
        assert!(entity_can_attack_move(&sim, Some(&rules), tank));
    }

    /// The chord walks the whole selection — buildings included — and dies on
    /// the first member that refuses.
    #[test]
    fn attack_move_chord_requires_every_selected_object() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
            .expect("unarmed miner");
        let factory = insert_typed(&mut sim, 900, "GAWEAP", EntityCategory::Structure);

        assert!(selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, war_miner]
        ));
        assert!(!selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, chrono_miner]
        ));
        // A selected structure kills the chord for the whole group.
        assert!(!selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, factory]
        ));
        // An empty selection cannot attack-move either.
        assert!(!selection_can_attack_move(&sim, Some(&rules), &[]));
    }

    /// A chorded click on an enemy *object* attack-moves with the object as its
    /// target, because ClickedMission promotes a committed Attack just as it
    /// promotes a committed Move (`0x006FFC09..0x006FFC1E`). The promotion is
    /// per object: a member whose type refuses attack-move still commits the
    /// plain attack.
    #[test]
    fn chorded_click_on_an_enemy_object_attack_moves() {
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::Normal, true, 1, 2),
            Command::AttackMove {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
                destination: None,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::Normal, false, 1, 2),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        // Plain click and guard area are untouched by the promotion; force
        // fire is promoted only in the attack-move mode, whose chord test
        // passes before any key read (`0x00731BF0`).
        for modifier in [OrderModifier::Normal, OrderModifier::ForceFire] {
            assert_eq!(
                object_click_payload(OrderMode::Move, modifier, true, 1, 2),
                Command::Attack {
                    attacker_id: 1,
                    target_id: 2,
                }
            );
        }
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::ForceFire, true, 1, 2),
            Command::AttackMove {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
                destination: None,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::Move, OrderModifier::GuardArea, true, 1, 2),
            Command::Guard {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
            }
        );
    }

    /// The attack-move promotion keeps a Move click's destination token
    /// (`0x006FFDA5..0x006FFDE6`), so an object stays the destination; a
    /// receiver whose type refuses attack-move commits the plain Move.
    #[test]
    fn attack_move_keeps_the_move_click_destination() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
                 [E1]\nStrength=125\nPrimary=M60\n\
                 [AMCV]\nStrength=1000\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let gi = sim
            .spawn_object("E1", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let mcv = sim
            .spawn_object("AMCV", "Americans", 9, 5, 0, &rules)
            .unwrap();
        let command =
            |id, mode, destination| move_click_command(&sim, &rules, mode, id, destination);

        for destination in [TargetKind::Entity(mcv), TargetKind::Cell(12, 5)] {
            assert_eq!(
                command(gi, OrderMode::AttackMove, destination),
                Command::AttackMove {
                    entity_id: gi,
                    target: None,
                    destination: Some(destination),
                }
            );
        }
        assert_eq!(
            command(mcv, OrderMode::AttackMove, TargetKind::Entity(gi)),
            Command::MoveToObject {
                entity_id: mcv,
                target_id: gi,
            }
        );
        assert_eq!(
            command(gi, OrderMode::Move, TargetKind::Cell(12, 5)),
            Command::Move {
                entity_id: gi,
                target_rx: 12,
                target_ry: 5,
            }
        );
    }

    /// A click on the selected object itself is its self action only while it
    /// is the one selected object and the local player's
    /// (`TechnoClass::What_Action_OnObject`, `0x007000B9..0x007000DF`); a click
    /// on one of several selected objects is a plain selection click.
    #[test]
    fn self_click_needs_the_one_selected_own_object() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
                 [BuildingTypes]\n0=GACNST\n\
                 [E1]\nStrength=125\nDeployFire=yes\nDeployer=yes\n\
                 [AMCV]\nStrength=1000\nDeploysInto=GACNST\n\
                 [GACNST]\nStrength=1000\n[Clear]\nBuildable=yes\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        // Ground the MCV's yard may stand on (Unit What_Action's placement
        // test, `0x0073FF9C`).
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, false, 0, 10,
                ));
        }
        let americans = sim.interner().get("Americans").unwrap();
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let gi = spawn("E1", "Americans", 5);
        let other_gi = spawn("E1", "Americans", 7);
        let mcv = spawn("AMCV", "Americans", 9);
        let enemy_mcv = spawn("AMCV", "Soviets", 20);
        let hover = |stable_id| HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id,
        };
        let click = |selected: &[u64], target: u64| {
            self_click_command(
                &sim,
                &rules,
                None,
                americans,
                selected,
                Some(&hover(target)),
                false,
            )
        };

        assert_eq!(
            click(&[gi], gi),
            Some(Command::ToggleInfantryDeploy { entity_id: gi })
        );
        assert_eq!(
            click(&[mcv], mcv),
            Some(Command::DeployMcv { entity_id: mcv })
        );
        assert_eq!(click(&[gi, other_gi], gi), None, "a group click selects");
        assert_eq!(click(&[mcv, gi], mcv), None, "a group click selects");
        assert_eq!(click(&[gi], other_gi), None, "not the selected object");
        assert_eq!(click(&[enemy_mcv], enemy_mcv), None, "not the player's");
    }

    /// Building447210 keeps Action4 for a lone selected own nonprimary
    /// factory. Click443410 then queues TargetEvent17 for that same building;
    /// its receiver toggles Building448160. This covers the input boundary,
    /// with existing FindFactory proving the initial primary independently.
    #[test]
    fn retail_nonprimary_war_factory_self_click_queues_its_primary_selection() {
        use crate::rules::object_type::FactoryType;
        use crate::sim::house_state::HouseState;

        let Some(battle) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = &battle.rules;
        let factory = rules.object("GAWEAP").expect("stock Allied War Factory");
        assert_eq!(factory.factory_type(), Some(FactoryType::UnitType));
        assert!(!factory.naval && !factory.invisible_in_game);
        let mut sim = Simulation::with_seed(0x0044_8160);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        for name in ["Americans", "Russians"] {
            let owner = sim.intern(name);
            let side = rules
                .country_side_index(name)
                .expect("stock country Side")
                .0;
            let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
            house.project_country_mults(rules, sim.interner());
            sim.houses_mut_for_test().register(house);
        }
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        let first = sim
            .spawn_object("GAWEAP", "Americans", 6, 6, 0, rules)
            .expect("first own retail War Factory");
        let second = sim
            .spawn_object("GAWEAP", "Americans", 20, 6, 0, rules)
            .expect("second own retail War Factory");
        sim.spawn_object("GAWEAP", "Russians", 20, 20, 0, rules)
            .expect("foreign owner's initial primary War Factory");
        let foreign = sim
            .spawn_object("GAWEAP", "Russians", 6, 20, 0, rules)
            .expect("foreign owner's nonprimary War Factory");
        let product = sim
            .spawn_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, rules)
            .expect("actual retail tank for the existing producer query");
        assert_eq!(
            crate::sim::production::object_find_factory(&sim, rules, product, false, false),
            Some(first),
            "automatic initialization must preserve the first primary; without one FindFactory returns the last eligible building"
        );
        let hover = |stable_id| HoverTargetKindWithId {
            kind: if stable_id == foreign {
                HoverTargetKind::EnemyStructure
            } else {
                HoverTargetKind::FriendlyStructure
            },
            stable_id,
        };
        let click = |selected: &[u64], target| {
            self_click_command(
                &sim,
                rules,
                None,
                owner,
                selected,
                Some(&hover(target)),
                false,
            )
        };
        assert_eq!(click(&[first], first), None, "already primary is Action0");
        assert_eq!(click(&[first, second], second), None, "a group selects");
        assert_eq!(click(&[first], second), None, "not the selected object");
        assert_eq!(
            click(&[foreign], foreign),
            None,
            "not the player's building"
        );
        let command = click(&[second], second)
            .expect("native nonprimary GAWEAP self-click queues its target-bearing Event17");
        assert_eq!(command, Command::TogglePrimaryFactory { entity_id: second });
        assert_eq!(
            order_voice_key(&command),
            None,
            "Event17 has no input voice"
        );

        use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
        use crate::app::types::{CursorFeedbackKind, CursorId};

        let cursor = |selected: &[u64], target| {
            capability_cursor_for_hover(
                &sim,
                selected,
                selected.first().copied(),
                &hover(target),
                Some(rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
            )
        };
        let nonprimary_cursor = cursor(&[second], second);
        assert_eq!(nonprimary_cursor, CursorFeedbackKind::Deploy);
        assert_eq!(
            cursor_id_for_feedback(nonprimary_cursor),
            Some(CursorId::Deploy),
            "native main-map Action4 uses the existing row27"
        );
        let primary_cursor = cursor(&[first], first);
        assert_eq!(primary_cursor, CursorFeedbackKind::Default);
        assert_eq!(
            cursor_id_for_feedback(primary_cursor),
            Some(CursorId::Default),
            "already-primary Action0 uses row0"
        );
        assert_eq!(
            cursor_id_for_feedback(cursor(&[first], second)),
            Some(CursorId::Select),
            "the hovered factory must be the selected object"
        );
        assert_ne!(
            cursor(&[foreign], foreign),
            CursorFeedbackKind::Deploy,
            "a foreign nonprimary must not offer the local self action"
        );
        // DetermineAction692610 asks the best object alone; with two
        // selected, Techno's self rung (0x007000C1) is closed and its tail
        // answers 0 over a selected object (0x007005BF), which Building
        // keeps (0x0044750D).
        for id in [first, second] {
            sim.entities_mut_for_test()
                .get_mut(id)
                .unwrap()
                .selection
                .set_selected_for_test(true);
        }
        let group = capability_cursor_for_hover(
            &sim,
            &[first, second],
            Some(first),
            &hover(second),
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        );
        assert_eq!(
            cursor_id_for_feedback(group),
            Some(CursorId::Default),
            "a group hover over a selected factory is the arrow"
        );
    }

    #[test]
    fn a_loaded_bio_reactor_self_click_and_hover_cursor_offer_unload() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=YAPOWR\n[YAPOWR]\nStrength=1000\n\
                 InfantryAbsorb=yes\nPassengers=5\n",
            ))
            .expect("Bio Reactor input rules");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let reactor = sim
            .spawn_object_limbo_at_height("YAPOWR", "Americans", 10, 10, 0, 0, &rules)
            .expect("Bio Reactor builds");
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0.0);
        cargo.board_forced_chain(&[(99, 1.0)]);
        sim.entities_mut_for_test()
            .get_mut(reactor)
            .unwrap()
            .passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: reactor,
        };

        let command =
            self_click_command(&sim, &rules, None, owner, &[reactor], Some(&hover), false);
        let cursor = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[reactor],
            Some(reactor),
            &hover,
            Some(&rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        );

        assert_eq!(
            command,
            Some(Command::UnloadPassengers {
                transport_id: reactor
            })
        );
        assert_eq!(cursor, crate::app::types::CursorFeedbackKind::Deploy);
    }

    /// Unit What_Action asks CanDeploySlashUnload of a loaded transport's
    /// self action (`0x0073FFEA`); on a bridge it refuses and the cursor
    /// answers NoDeploy (`0x0073FFF4`).
    #[test]
    fn a_loaded_transport_on_a_bridge_offers_no_deploy() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=IFV\n[IFV]\nStrength=200\nPassengers=1\n",
            ))
            .expect("transport input rules");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let ifv = sim
            .spawn_object_limbo_at_height("IFV", "Americans", 10, 10, 0, 0, &rules)
            .expect("transport builds");
        let mut cargo = crate::sim::passenger::PassengerCargo::new(1, 0.0);
        cargo.board_forced_chain(&[(99, 1.0)]);
        sim.entities_mut_for_test()
            .get_mut(ifv)
            .unwrap()
            .passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: ifv,
        };
        let probe = |sim: &Simulation| {
            (
                self_click_command(sim, &rules, None, owner, &[ifv], Some(&hover), false),
                crate::app::input::cursor::capability_cursor_for_hover(
                    sim,
                    &[ifv],
                    Some(ifv),
                    &hover,
                    Some(&rules),
                    None,
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
                ),
            )
        };
        assert_eq!(
            probe(&sim),
            (
                Some(Command::UnloadPassengers { transport_id: ifv }),
                crate::app::types::CursorFeedbackKind::Deploy
            )
        );
        sim.entities_mut_for_test()
            .get_mut(ifv)
            .unwrap()
            .set_on_bridge(true);
        assert_eq!(
            probe(&sim),
            (None, crate::app::types::CursorFeedbackKind::NoDeploy)
        );
    }

    /// Retail SCHP has IsSimpleDeployer without DeploysInto/Deployer. Native
    /// Unit WhatAction74000B and CanDeploy700E81 still offer its deploy action,
    /// including while the deploy animation owns the visible body.
    #[test]
    fn retail_siege_chopper_self_click_and_cursor_offer_the_same_deploy_action() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &battle.rules;
        let object = rules.object("SCHP").expect("retail Siege Chopper");
        assert!(object.is_simple_deployer);
        assert!(object.deploys_into.is_none());
        assert!(!object.deployer);
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
            .expect("retail Siege Chopper builds");
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: id,
        };
        for (deployed, begin, reverse) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, false, true),
        ] {
            sim.entities_mut_for_test()
                .get_mut(id)
                .unwrap()
                .set_unit_simple_deploy_for_test(deployed, begin, reverse);
            assert_eq!(
                self_click_command(&sim, rules, None, owner, &[id], Some(&hover), false),
                Some(Command::DeployMcv { entity_id: id }),
            );
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[id],
                    Some(id),
                    &hover,
                    Some(rules),
                    None,
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
                ),
                crate::app::types::CursorFeedbackKind::Deploy,
            );
        }
        assert_eq!(
            self_click_command(&sim, rules, None, owner, &[id, id + 1], Some(&hover), false),
            None,
            "a group self-click remains selection"
        );
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .low_bridge_tube_state = Some(
            crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
                crate::map::tube_facts::TubeId(0),
                0,
                crate::sim::components::DriveCoord::cell(10, 10, 0),
            ),
        );
        assert_eq!(
            self_click_command(&sim, rules, None, owner, &[id], Some(&hover), false),
            None
        );
        let feedback = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[id],
            Some(id),
            &hover,
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        );
        assert_eq!(feedback, crate::app::types::CursorFeedbackKind::NoDeploy);
        assert_eq!(
            crate::app::input::cursor::cursor_id_for_feedback(feedback),
            Some(crate::app::types::CursorId::NoDeploy),
            "native action30 must display the blocked deploy cursor"
        );
    }

    /// A deploying or deployed receiver takes no Move click on a Cell,
    /// whatever its locomotor, so no Event clears its target. Retail SCHP is a
    /// JumpJet IsSimpleDeployer Unit: over an open Cell, Unit7404B0 makes Move
    /// action0 while 6E0, 6E1 or 6E2 is set (740709..740727). A retail E1
    /// Deployer in Doing28 answers NoMove (51F8ED..51F94E), which its Cell
    /// click refuses (51F25D..51F285). Over an object their What_Action turns
    /// Move into NoMove only for 6E0 or Doing27..30 (`0x0074037A`,
    /// `0x0051EB81`), and the object receiver 4D74E0 still sends NoMove, as a
    /// Cell near the object. A non-player House orders neither: its Cell
    /// What_Action answers 0, and 4D74E0 asks IsControllable (`0x004D77FF`).
    #[test]
    fn retail_deployment_answers_move_clicks_through_the_class_what_action() {
        use crate::app::input::commands::{object_move_destination, ordinary_cell_move_goal};
        use crate::sim::world::MoveAction;
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &battle.rules;
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        // Successful Unlimbo now publishes sight immediately; establish the
        // viewer-plane dimensions before creating any sources.
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        let chopper = sim
            .spawn_object("SCHP", "Americans", 10, 10, 0, rules)
            .expect("retail Siege Chopper spawns");
        let gi = sim
            .spawn_object("E1", "Americans", 12, 10, 0, rules)
            .expect("retail GI spawns");
        let tank = sim
            .spawn_object("MTNK", "Americans", 14, 10, 0, rules)
            .expect("retail tank spawns");
        let owner = sim.interner().get("Americans").unwrap();
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let goals = |sim: &Simulation, id| {
            (
                ordinary_cell_move_goal(sim, rules, None, owner, id, (14, 12), false),
                object_move_destination(sim, rules, owner, id, tank, MoveAction::Move, false),
            )
        };
        let moves = Some((14, 12));
        let onto_tank = Some(TargetKind::Entity(tank));
        assert_eq!(goals(&sim, chopper), (moves, onto_tank));
        assert_eq!(goals(&sim, gi), (moves, onto_tank));
        for (deployed, begin, reverse) in [
            (false, true, false),
            (true, false, false),
            (true, false, true),
        ] {
            sim.entities_mut_for_test()
                .get_mut(chopper)
                .unwrap()
                .set_unit_simple_deploy_for_test(deployed, begin, reverse);
            let (on_cell, over_object) = goals(&sim, chopper);
            let state = format!("6E0={deployed} 6E1={begin} 6E2={reverse}");
            assert_eq!(on_cell, None, "{state}");
            if deployed {
                assert!(
                    matches!(over_object, Some(TargetKind::Cell(..))),
                    "{state}: {over_object:?}"
                );
            } else {
                assert_eq!(over_object, onto_tank, "{state}");
            }
        }
        assert!(rules.object("E1").unwrap().deployer);
        sim.entities_mut_for_test()
            .get_mut(gi)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(28)
            .unwrap();
        let (on_cell, over_object) = goals(&sim, gi);
        assert_eq!(on_cell, None);
        assert!(
            matches!(over_object, Some(TargetKind::Cell(..))),
            "{over_object:?}"
        );
        sim.entities_mut_for_test()
            .get_mut(gi)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(0)
            .unwrap();
        sim.session_mut_for_test().set_current_house_for_test(None);
        assert_eq!(goals(&sim, gi), (None, None));
    }

    /// The click side of the bomb actions: an Engineer's DisarmBomb and a
    /// Crazy Ivan's IvanBomb on a friend are Attack orders. An Ivan never
    /// attacks a bombed target, nor a
    /// friend his shot cannot hit (the rung's ILLEGAL test), where the cursor
    /// shows Select too.
    #[test]
    fn bomb_click_orders() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n1=ENGINEER\n[VehicleTypes]\n0=HTNK\n1=HVY\n\
                 [CombatDamage]\nIvanTimedDelay=450\n\
                 [IVAN]\nStrength=125\nIvan=yes\nAttackCursorOnFriendlies=yes\n\
                 Primary=IvanBomber\n\
                 [ENGINEER]\nStrength=75\nEngineer=yes\nBombSight=4\n\
                 [HTNK]\nStrength=900\n[HVY]\nStrength=900\nArmor=heavy\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Invisible]\nInviso=yes\n\
                 [IvanBomb]\nIvanBomb=yes\n\
                 Verses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.intern(house);
            sim.houses_mut_for_test()
                .register(crate::sim::house_state::HouseState::new(
                    id, 0, None, false, 0, 10,
                ));
        }
        let americans = sim.interner().get("Americans").unwrap();
        // The rung is a human player's (`0x007003ED`).
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(americans));
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let ivan = spawn("IVAN", "Americans", 5);
        let engineer = spawn("ENGINEER", "Americans", 7);
        let own_tank = spawn("HTNK", "Americans", 9);
        let spare_tank = spawn("HTNK", "Americans", 11);
        let armoured = spawn("HVY", "Americans", 13);
        let enemy_ivan = spawn("IVAN", "Soviets", 20);
        let far_tank = spawn("HTNK", "Soviets", 22);
        let selected = [ivan, engineer];

        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), own_tank, None),
            vec![ivan]
        );
        // 0% Verses: ILLEGAL to the Ivan, so the friend stays a selection
        // click, as the cursor shows it.
        assert!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), armoured, None).is_empty()
        );
        {
            let cursor = |target| {
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[ivan],
                    Some(ivan),
                    &HoverTargetKindWithId {
                        kind: HoverTargetKind::FriendlyUnit,
                        stable_id: target,
                    },
                    Some(&rules),
                    None,
                    crate::app::input::context_order::OrderModifier::Normal,
                    false,
                    false,
                )
            };
            assert_eq!(
                cursor(armoured),
                crate::app::types::CursorFeedbackKind::FriendlyUnit
            );
            assert_eq!(
                cursor(spare_tank),
                crate::app::types::CursorFeedbackKind::IvanBomb
            );
        }
        assert!(disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans).is_empty());
        assert!(!ivan_cannot_bomb(&sim, &rules, ivan, own_tank));

        sim.bomb_attach_for_test(ivan, Some(own_tank), &rules);
        sim.bomb_attach_for_test(enemy_ivan, Some(far_tank), &rules);
        sim.bomb_list_update_for_test(&rules);
        sim.bomb_list_update_for_test(&rules);

        assert_eq!(
            disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans),
            vec![engineer]
        );
        assert!(
            disarm_bomb_engineers(&sim, &rules, &selected, far_tank, americans).is_empty(),
            "an unseen bomb"
        );
        assert!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), own_tank, None).is_empty(),
            "a bombed friend stays a selection click"
        );
        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), spare_tank, None),
            vec![ivan]
        );
        assert!(ivan_cannot_bomb(&sim, &rules, ivan, own_tank));
        assert!(!ivan_cannot_bomb(&sim, &rules, engineer, own_tank));

        // The Engineer carrying a bomb itself is no target of its own click.
        sim.bomb_attach_for_test(ivan, Some(engineer), &rules);
        sim.bomb_list_update_for_test(&rules);
        sim.bomb_list_update_for_test(&rules);
        assert!(disarm_bomb_engineers(&sim, &rules, &[engineer], engineer, americans).is_empty());
    }
    /// Supplies the ordinary object-route prerequisites, through production
    /// object constructors and the existing terrain/type authorities.
    fn engineer_hut_click_fixture(
        relation: &str,
        collapsed: bool,
        repairable: bool,
        capturable: bool,
        extra_hut_flags: &str,
        extra_actor_flags: &str,
    ) -> (
        Simulation,
        crate::rules::ruleset::RuleSet,
        u64,
        HoverTargetKindWithId,
    ) {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[InfantryTypes]\n0=ENGINEER\n[VehicleTypes]\n0=TRUCK\n[BuildingTypes]\n0=CABHUT\n\
                 [TRUCK]\nStrength=100\nSpeed=4\n\
                 [ENGINEER]\nStrength=75\nEngineer=yes\n{}\n\
                 [CABHUT]\nStrength=200\nFoundation=1x1\nBridgeRepairHut=yes\nRepairable={}\nCapturable={}\n{}\n",
                extra_actor_flags,
                if repairable { "yes" } else { "no" },
                if capturable { "yes" } else { "no" },
                extra_hut_flags,
            )),
        ).unwrap();
        let mut sim = Simulation::with_seed(0x587410);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.intern("Americans"));
        sim.session_mut_for_test().set_current_house_for_test(house);
        let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, |rx, ry| {
            crate::map::resolved_terrain::ResolvedTerrainCell {
                speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
                base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
                ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
            }
        });
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        sim.install_resolved_terrain_for_new_map(terrain);
        for y in 9..=11 {
            sim.overlay_cell_mut_for_test(12, y).overlay_id =
                Some(if collapsed { 0xE7 } else { 0xD4 });
        }
        *sim.bridge_state_mut_for_test() =
            Some(crate::sim::bridge_state::BridgeRuntimeState::default());
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let owner = if relation == "self" {
            "Americans"
        } else {
            "Soviets"
        };
        let engineer = sim
            .spawn_object("ENGINEER", "Americans", 8, 10, 0, &rules)
            .unwrap();
        let hut = sim
            .spawn_object("CABHUT", owner, 10, 10, 0, &rules)
            .unwrap();
        sim.entities_mut_for_test()
            .get_mut(hut)
            .unwrap()
            .health
            .set_for_test(150);
        if relation == "allied" {
            sim.house_alliances_mut_for_test()
                .entry("AMERICANS".into())
                .or_default()
                .insert("SOVIETS".into());
        }
        let kind = if relation == "hostile" {
            HoverTargetKind::EnemyStructure
        } else {
            HoverTargetKind::FriendlyStructure
        };
        (
            sim,
            rules,
            engineer,
            HoverTargetKindWithId {
                kind,
                stable_id: hut,
            },
        )
    }

    /// Original object-click51F190 maps action29 to Capture with Destination
    /// hut; action32 consumes the click without queueing. Owner friendship is
    /// absent from the admitted hut arm, including damaged/capturable traps.
    #[test]
    fn engineer_hut_context_orders_match_native_terminal_action_decisions() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        for relation in ["hostile", "allied", "self"] {
            for collapsed in [false, true] {
                let (sim, rules, engineer, hover) =
                    engineer_hut_click_fixture(relation, collapsed, true, true, "", "");
                let before = sim.state_hash();
                let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
                let rng = (
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state(),
                );
                let commands = engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover)
                    .expect("an admitted hut arm always consumes the click, including false query");
                let row = native["object_clicks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["name"]
                            == format!("{relation}_{}", if collapsed { "True" } else { "False" })
                            && row["input"]["emp_blocked"] == false
                    })
                    .unwrap();
                assert_eq!(
                    commands.len() as u64,
                    row["output"]["queue_calls"].as_u64().unwrap()
                );
                let expected = if collapsed {
                    vec![Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id,
                    }]
                } else {
                    vec![]
                };
                assert_eq!(
                    commands, expected,
                    "relation={relation} collapsed={collapsed}"
                );
                assert_eq!(
                    sim.state_hash(),
                    before,
                    "input producer must not mutate gameplay"
                );
                assert_eq!(
                    serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                    actor
                );
                assert_eq!(
                    (
                        sim.scenario_rng().logical_state(),
                        sim.main_rng().logical_state(),
                        sim.mapgen_rng().logical_state()
                    ),
                    rng
                );
            }
        }
    }

    /// The native object route checks Repairable before entering the hut
    /// branch. Native repairable_false establishes exclusion/query_calls=0;
    /// its later ordinary WhatAction continuation is outside those goldens.
    #[test]
    fn nonrepairable_noncapturable_hut_does_not_produce_hut_capture_order() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        let row = native["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["input"]["route"] == "object" && row["input"]["name"] == "repairable_false"
            })
            .unwrap();
        assert_eq!(row["output"]["query_calls"], 0);
        for relation in ["hostile", "allied", "self"] {
            let (sim, rules, engineer, hover) =
                engineer_hut_click_fixture(relation, true, false, false, "", "");
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state(),
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                None
            );
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
                None
            );
            assert_eq!(sim.state_hash(), before);
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor
            );
            assert_eq!(
                (
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state()
                ),
                rng
            );
        }
    }
    /// Executed local_false and undeploy_True_foundation_0 object rows stop
    /// before the hut query. Both assertions are hut-arm exclusion, without
    /// claiming the later ordinary WhatAction continuation's final action.
    #[test]
    fn engineer_hut_object_prerequisites_exclude_nonlocal_and_undeploy_targets() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        for gate in ["nonlocal", "undeploy"] {
            let flags = if gate == "undeploy" {
                "UndeploysInto=TRUCK"
            } else {
                ""
            };
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", true, true, false, flags, "");
            if gate == "nonlocal" {
                let house = Some(sim.intern("Soviets"));
                sim.session_mut_for_test().set_current_house_for_test(house);
            } else {
                assert!(rules.object("CABHUT").unwrap().is_1x1_with_undeploy());
            }
            if gate == "nonlocal" {
                let row = native["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["route"] == "object" && row["input"]["name"] == "local_false"
                    })
                    .unwrap();
                assert_eq!(row["output"]["query_calls"], 0);
            }
            if gate == "undeploy" {
                let row = native["undeploy_controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["route"] == "object"
                            && row["input"]["name"] == "undeploy_True_foundation_0"
                    })
                    .unwrap();
                assert_eq!(row["output"]["query_calls"], 0);
            }
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state(),
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                None,
                "gate={gate}"
            );
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
                None,
                "gate={gate}"
            );
            assert_eq!(sim.state_hash(), before);
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor
            );
            assert_eq!(
                (
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state()
                ),
                rng
            );
        }
    }
    /// Original700C40 and its object-click caller are independently executed
    /// in each paired golden group. These adapters supply represented owner
    /// state; they do not assert that retail constructs missile-spawn Engineers
    /// or imports arbitrary native EMP/Robot latch bytes.
    #[test]
    fn represented_infantry_controllability_gates_match_native_hut_clicks() {
        use crate::sim::spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState};
        use crate::sim::timer::CdTimer;
        let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        let mut compared = 0;
        let mut unrepresented = Vec::new();
        for row in packet["controllability_controls"].as_array().unwrap() {
            let input = &row["native"]["input"];
            let name = input["name"].as_str().unwrap();
            if matches!(
                name,
                "robot_native_save_control" | "emp_native_save_positive" | "emp_negative"
            ) {
                unrepresented.push(name);
                continue;
            }
            let gates = &input["gates"];
            let spawned = gates["spawned"].as_bool().unwrap_or(false);
            let capacity = gates["spawns_number"].as_i64().unwrap_or(1);
            let missile_child = gates["spawn_slots"]
                .as_array()
                .is_some_and(|slots| slots.iter().any(|slot| slot["missile_spawn"] == true));
            let actor_flags = format!(
                "Spawned={}\nSpawnsNumber={capacity}\nMissileSpawn={}\n",
                if spawned { "yes" } else { "no" },
                if missile_child { "yes" } else { "no" }
            );
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", true, true, false, "", &actor_flags);
            sim.session_mut_for_test()
                .set_binary_frame_for_test(gates["frame"].as_u64().unwrap_or(0) as u32);
            if gates["bunker"] == true {
                // Typed existing owner state: positive native pointer control,
                // not an assertion of ordinary Infantry bunker installation.
                sim.entities_mut_for_test()
                    .get_mut(engineer)
                    .unwrap()
                    .bunker
                    .set_bunker_link_for_test(
                        crate::sim::docking::bunker_link::BunkerLink::Installed(hover.stable_id),
                    );
            }
            if gates["paralysis_start"].is_number() {
                sim.entities_mut_for_test()
                    .get_mut(engineer)
                    .unwrap()
                    .parasite
                    .set_paralysis_for_test(CdTimer::from_raw(
                        gates["paralysis_start"].as_i64().unwrap() as i32,
                        gates["paralysis_duration"].as_i64().unwrap() as i32,
                    ));
            }
            if gates["warp_out"] == true {
                sim.entities_mut_for_test()
                    .get_mut(engineer)
                    .unwrap()
                    .set_temporal_for_test(
                        crate::sim::temporal::TemporalState::warped_by_for_test(hover.stable_id),
                    );
            }
            if gates["warp_in"] == true {
                sim.entities_mut_for_test()
                    .get_mut(engineer)
                    .unwrap()
                    .install_teleport_state_for_test(Some(
                        crate::sim::movement::teleport_movement::TeleportState::for_test(
                            crate::sim::movement::teleport_movement::TeleportPhase::ChronoDelay,
                            8,
                            10,
                            1,
                        ),
                    ));
            }
            if gates["slave_owner"] == true {
                sim.entities_mut_for_test().get_mut(engineer).unwrap().slave =
                    crate::sim::slave_manager::SlaveLink::for_test(Some(hover.stable_id), vec![]);
            }
            if let Some(native_slots) = gates["spawn_slots"].as_array() {
                let mut slots = Vec::new();
                for (index, native_slot) in native_slots.iter().enumerate() {
                    let child = if native_slot["state"] == 2 && native_slot["child"] != false {
                        let child = sim
                            .spawn_object("ENGINEER", "Americans", 5 + index as u16, 12, 0, &rules)
                            .unwrap();
                        sim.entities_mut_for_test()
                            .get_mut(child)
                            .unwrap()
                            .lifecycle
                            .set_in_limbo_for_test(native_slot["limbo"] == true);
                        Some(child)
                    } else {
                        None
                    };
                    slots.push(SpawnSlot::for_test(
                        child,
                        match native_slot["state"].as_i64().unwrap() {
                            0 => SpawnSlotState::ReadyDocked,
                            1 => SpawnSlotState::KamikazeWait,
                            2 => SpawnSlotState::InFlight,
                            other => panic!("unrepresented native slot state {other}"),
                        },
                        CdTimer::default(),
                        false,
                    ));
                }
                let spawn_type = sim.intern("ENGINEER");
                sim.entities_mut_for_test()
                    .get_mut(engineer)
                    .unwrap()
                    .spawn_manager = Some(SpawnManagerState::for_test(spawn_type, slots));
            }
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state(),
            );
            assert_eq!(
                sim.techno_player_controllable(engineer, &rules),
                row["native"]["output"]["returned_al"] == 1,
                "{name}"
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                Some(crate::sim::world::EngineerBuildingAction::Repair(true)),
                "control admission must not change WhatAction: {name}"
            );
            let mut selected = vec![engineer];
            let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover)
                .expect("hut arm consumes blocked and allowed clicks");
            assert_eq!(
                commands.len() as u64,
                row["object_click"]["output"]["queue_calls"]
                    .as_u64()
                    .unwrap(),
                "{name}"
            );
            assert!(
                selected.is_empty(),
                "handled Engineer must not reach a later input branch: {name}"
            );
            if !commands.is_empty() {
                assert_eq!(
                    commands,
                    [Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id
                    }],
                    "{name}"
                );
            }
            assert_eq!(sim.state_hash(), before, "{name}");
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor,
                "{name}"
            );
            assert_eq!(
                (
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state()
                ),
                rng,
                "{name}"
            );
            compared += 1;
        }
        assert_eq!(compared, 21);
        assert_eq!(
            unrepresented,
            [
                "robot_native_save_control",
                "emp_native_save_positive",
                "emp_negative"
            ]
        );
    }

    /// A hut's terminal action consumes each handled Engineer, while other
    /// selected actors continue through their own original action routes.
    #[test]
    fn engineer_hut_context_keeps_unhandled_actors_in_selection_order() {
        for collapsed in [false, true] {
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", collapsed, true, true, "", "C4=yes");
            // Unit Unlimbo requires configured playfield bounds; the hut
            // action fixture otherwise only constructs Infantry/Buildings.
            *sim.playfield_bounds_mut_for_test() = Some(crate::map::playfield::PlayfieldBounds {
                base: 0,
                off_fc: -100,
                off_100: -100,
                off_104: 200,
                off_108: 200,
            });
            let first = sim
                .spawn_object("TRUCK", "Americans", 4, 10, 0, &rules)
                .unwrap();
            let last = sim
                .spawn_object("TRUCK", "Americans", 6, 10, 0, &rules)
                .unwrap();
            let second_engineer = sim
                .spawn_object("ENGINEER", "Americans", 7, 10, 0, &rules)
                .unwrap();
            let mut selected = vec![first, engineer, last, second_engineer];
            let before = sim.state_hash();
            let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover).unwrap();
            assert_eq!(selected, [first, last]);
            let expected = if collapsed {
                vec![
                    Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id,
                    },
                    Command::CaptureBuilding {
                        engineer_id: second_engineer,
                        target_building_id: hover.stable_id,
                    },
                ]
            } else {
                vec![]
            };
            assert_eq!(commands, expected);
            assert_eq!(sim.state_hash(), before);
            let before_selected = selected.clone();
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut selected, &hover),
                None
            );
            assert_eq!(
                selected, before_selected,
                "non-Engineers retain their next action route"
            );
        }
    }
}

#[cfg(test)]
#[path = "context_order_selection_tests.rs"]
mod selection_dispatch_tests;

#[cfg(test)]
#[path = "context_order_move_flash_tests.rs"]
mod move_flash_tests;

#[cfg(test)]
#[path = "context_order_retail_bridge_tests.rs"]
mod retail_bridge_tests;

#[cfg(test)]
#[path = "context_order_retail_engineer_tests.rs"]
mod retail_engineer_tests;

#[cfg(test)]
#[path = "context_order_retail_garrison_tests.rs"]
mod retail_garrison_tests;

#[cfg(test)]
#[path = "context_order_retail_entry_tests.rs"]
mod retail_entry_tests;

#[cfg(test)]
#[path = "context_order_retail_terrain_tests.rs"]
mod retail_terrain_tests;

#[cfg(test)]
#[path = "context_order_retail_inactive_unit_tests.rs"]
mod retail_inactive_unit_tests;

#[cfg(test)]
#[path = "context_order_retail_self_deploy_tests.rs"]
mod retail_self_deploy_tests;

#[cfg(test)]
#[path = "context_order_retail_ifv_repair_tests.rs"]
pub(in crate::app) mod retail_ifv_repair_tests;

#[cfg(test)]
mod stationary_attack_dispatch_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::world::Simulation;

    #[test]
    fn bunker_attack_batch_preserves_mobile_order_and_select_fallback() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            r#"
[VehicleTypes]
0=TANK
[TANK]
Strength=300
Primary=GUN
Speed=6
SpeedType=Track
MovementZone=Normal
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[GUN]
Damage=60
Range=5
Projectile=SHOT
Warhead=AP
[SHOT]
AG=yes
Inviso=yes
SubjectToElevation=no
[Warheads]
0=AP
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
"#,
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let stationary = sim
            .spawn_object("TANK", "Americans", 2, 2, 0, &rules)
            .unwrap();
        let mobile = sim
            .spawn_object("TANK", "Americans", 2, 4, 0, &rules)
            .unwrap();
        let friend = sim
            .spawn_object("TANK", "Americans", 20, 2, 0, &rules)
            .unwrap();
        let enemy = sim
            .spawn_object("TANK", "Soviets", 20, 4, 0, &rules)
            .unwrap();
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let house = Some(sim.entities().get(stationary).unwrap().owner());
        sim.session_mut_for_test().set_current_house_for_test(house);
        sim.entities_mut_for_test()
            .get_mut(stationary)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(99));
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ));
        let registry = OverlayTypeRegistry::empty();
        let mixed = attack_branch_orders(
            &sim,
            &rules,
            &[stationary, mobile],
            TargetKind::Cell(20, 2),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(mixed.consumed);
        assert!(!mixed.select_fallback);
        assert_eq!(mixed.receivers, [mobile].into_iter().collect());
        assert_eq!(
            crate::app::input::commands::ordinary_cell_move_goal(
                &sim,
                &rules,
                Some(&registry),
                house.unwrap(),
                stationary,
                (20, 2),
                false,
            ),
            None,
            "the remaining stationary receiver cannot commit a Cell move"
        );
        assert_eq!(mixed.commands.len(), 1);
        assert!(
            matches!(mixed.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==mobile)
        );
        let none = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(enemy),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(none.commands.is_empty());
        assert!(
            none.consumed && !none.select_fallback,
            "Action0 consumes without selection or a hidden attack event"
        );
        let select = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(friend),
            true,
            OrderMode::Move,
            &registry,
        );
        assert!(select.commands.is_empty());
        assert!(
            !select.consumed && select.select_fallback,
            "Action7 leaves the click for ordinary selection"
        );
        let near = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Cell(4, 2),
            true,
            OrderMode::Move,
            &registry,
        );
        assert_eq!(near.commands.len(), 1);
        assert!(
            matches!(near.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==stationary)
        );
    }
}

#[cfg(test)]
#[path = "allied_object_action_tests.rs"]
mod allied_object_action_tests;

#[cfg(test)]
#[path = "context_order_cursor_contract_tests.rs"]
mod cursor_contract_tests;

#[cfg(test)]
#[path = "engineer_special_entry_tests.rs"]
mod engineer_special_entry_tests;

#[cfg(test)]
#[path = "foot_unarmed_ctrl_tests.rs"]
mod foot_unarmed_ctrl_tests;
