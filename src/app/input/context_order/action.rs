//! What one selected object's click means: the Ctrl/Shift/Alt chord, the attack,
//! armed and healer actions over an object or cell, and the range and immunity
//! gates those actions ask (TechnoClass::What_Action_OnObject / _OnCell).

use super::{ObjectKeyAction, context_actor_viewer, object_key_action_with, shot_is_illegal};
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

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
/// * **Ctrl+Alt → guard area.** Native's patrol answer for a cell holding a
///   legacy House waypoint never fires in YR, which creates none.
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
    /// GuardArea (0x1A): a player's negative-damage Infantry turns Attack on
    /// a Cell into this action (Infantry51F838..51F85B).
    GuardArea,
    /// AttackSupport (0x3B): a player's healing Unit's Attack on a Cell
    /// (`0x007404FD..0x00740522`). SetCursorFromAction (`0x004AAE90`) has no
    /// case for it, so it shows the arrow (row 0), and Foot's ClickedAction_Cell
    /// (`0x004D7D50`) takes its default case (`0x004D8237`): the click is
    /// consumed and sends nothing.
    AttackSupport,
}

/// Techno6FFEC0's disguise and real-owner admission,70025D..70041E.
/// Native friendly-attack grants also override a disguise refusal.
/// All consumers ask this through the existing action owner.
/// IsAlly reads the real owner and the asking House's directional mask;
/// display friendliness alone cannot decide whether to attack or select.
pub(super) fn object_attack_is_admitted(
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
        // Terrain has no House (Object4104A0) and IsDisguisedTo426430
        // returns false. Neither restriction applies to a real Terrain.
        return crate::sim::movement::ground_pose::target_get_coords(
            TargetKind::Entity(target_id),
            sim,
            None,
        )
        .is_some();
    };
    let allied = combat_weapon::is_ally_by_object(
        Some(sim.house_alliances()),
        sim.interner(),
        actor.owner(),
        target.owner(),
    );
    //70025D..70030F: this is the firing blink, not the reacquisition timer.
    let disguise_admitted =
        rules.general.attack_cursor_on_disguise
            || sim
                .interner()
                .object_type(target.type_ref(), rules)
                .is_some_and(|target_type| target_type.perma_disguise)
            || target.seldom.disguise.as_ref().is_some_and(|disguise| {
                disguise.blink_remaining(sim.session().binary_frame()) != 0
            })
            || !target.is_disguised_to(
                actor.owner(),
                Some(sim.fog()),
                Some(sim.house_alliances()),
                sim.interner(),
            )
            || (target
                .disguise_house(true, sim.session().current_house(), false)
                .is_none()
                && sim.fires_at_terrain(
                    actor_id,
                    &TargetKind::Entity(target_id),
                    rules,
                    Some(rules.overlay_registry()),
                ));
    if !allied && disguise_admitted {
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
/// `0x0070042E`) or TechnoClass vt+0x398 (asked at `0x007002EE`,
/// [`Simulation::fires_at_terrain`]) to answer yes. For a Terrain object the
/// type is its TerrainType (retail: only `VEINHOLE` is `LegalTarget=`); a
/// Techno uses its own ObjectType byte.
///
/// [`Simulation::fires_at_terrain`]: crate::sim::world::Simulation::fires_at_terrain
fn object_target_is_legal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    let legal_target = match sim.terrain_objects().get(target_id) {
        Some(terrain) => rules
            .terrain_object_type_case_insensitive(sim.interner().resolve(terrain.type_ref()))
            .is_some_and(|terrain| terrain.legal_target),
        None => sim.entities().get(target_id).is_some_and(|target| {
            sim.interner()
                .object_type_in_category(target.type_ref(), target.category().into(), rules)
                .is_some_and(|kind| kind.legal_target)
        }),
    };
    legal_target
        || sim.fires_at_terrain(
            actor_id,
            &TargetKind::Entity(target_id),
            rules,
            Some(rules.overlay_registry()),
        )
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
/// Before that tail, a second rung (`0x00700A46..0x00700AA8`) answers Attack
/// when vt+0x398 accepts the cell (an `IsARock=` overlay under a
/// `TerrainFire=` weapon) and control or range holds; the unforced callers ask
/// it with the first through [`Simulation::cell_attack_rung_admits`].
///
/// This asks no ROF or reload. Over an object a Foot's shroud rung comes
/// next ([`shrouded_object_action`]); so far it is [`base_action`]. Then
/// the class post-processing below. See
/// the original DetermineAction/Unit/Foot/Techno and Display action corpus
/// in tools/spatial_oracle/bunker_attack_action.json.
///
/// The real-owner ally gate and its shared weapon/type grants are handled by
/// [`object_attack_is_admitted`]. A different allied House therefore takes
/// the identity-sensitive tail, then the existing class postprocessing.
///
/// Unforced, an object target must pass [`object_target_is_legal`] and
/// [`object_attack_is_admitted`]. Both cursor and click use that answer.
///
/// [`Simulation::cell_attack_rung_admits`]: crate::sim::world::Simulation::cell_attack_rung_admits
pub(crate) fn attack_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> AttackAction {
    attack_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        target,
        forced,
        overlay_registry,
    )
}

pub(super) fn attack_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> AttackAction {
    let mut action = base_action(
        sim,
        control,
        rules,
        actor_id,
        target,
        forced,
        overlay_registry,
    );
    let Some(actor) = sim.entities().get(actor_id) else {
        return action;
    };
    let Some(kind) = sim.interner().object_type(actor.type_ref(), rules) else {
        return action;
    };
    // InfantryClass::What_Action_OnObject's healer arm replaces Foot's
    // answer after the Engineer's refusal of an Attack (0x0051E6B4..0x0051E6D5,
    // [`infantry_attack_action`]): GuardArea over itself (0x0051E748), whose
    // click is `object_guard_command`, and AttackSupport over a Terrain
    // object (0x0051E797), whose click guards the healer
    // (`attack_support_command`). Its rows over a Techno are
    // [`healer_object_action`]'s.
    if !(kind.engineer && action == AttackAction::Attack)
        && let TargetKind::Entity(target_id) = *target
    {
        if target_id == actor_id && sim.infantry_healer_guards_itself(control, actor_id, rules) {
            return AttackAction::GuardArea;
        }
        if healer_supports_over_terrain(sim, control, rules, actor_id, target_id) {
            return AttackAction::AttackSupport;
        }
    }
    let human = control.can_order(actor.owner());
    let armed = crate::sim::combat::combat_weapon::is_armed(
        actor,
        kind,
        sim.entities(),
        rules,
        sim.interner(),
    );
    let infiltrator = actor.category() == EntityCategory::Infantry && kind.infiltrate;
    let healing_weapon = human
        && matches!(
            actor.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        )
        && crate::sim::combat::combat_weapon::weapon_damage_value(
            actor,
            kind,
            sim.entities(),
            rules,
            sim.interner(),
        ) < 0;
    let healing_unit = actor.category() == EntityCategory::Unit && healing_weapon;
    // UnitClass::What_Action_OnCell 0x007404FD..0x00740522: a player's Unit
    // whose GetWeaponDamageValue(-1) (0x006F3970) is negative, an IFV with an
    // Engineer, turns Foot's Attack on a Cell into AttackSupport (0x3B), which
    // every later Unit rung returns unchanged (0x0074072B..0x0074075F).
    if action == AttackAction::Attack && healing_unit && matches!(target, TargetKind::Cell(..)) {
        action = AttackAction::AttackSupport;
    }
    // gamemd.exe InfantryClass::What_Action_OnCell 0x0051F838..0x0051F85B:
    // the same negative GetWeaponDamageValue(-1) instead answers GuardArea,
    // before the deploy-fire rungs. Object healing has its own class ladder.
    if action == AttackAction::Attack
        && actor.category() == EntityCategory::Infantry
        && healing_weapon
        && matches!(target, TargetKind::Cell(..))
    {
        action = AttackAction::GuardArea;
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
        && !sim.local_techno_is_active_with(control, actor_id, rules)
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
        && !sim.infiltrator_building_action_with(control, actor_id, target_id, rules)
    {
        action = AttackAction::None;
    }
    // Over an object, the Foot class tails turn 0 into another action: NoMove
    // for a player's Unit or Infantry it may control, and an Aircraft's own
    // Cell action under the object, using the tactical viewer for shared control.
    if action == AttackAction::None
        && let TargetKind::Entity(target_id) = *target
    {
        action = if aircraft_cell_attack_under_object(
            sim,
            control,
            rules,
            actor_id,
            target_id,
            overlay_registry,
        ) {
            AttackAction::Attack
        } else if let Some(viewer) = context_actor_viewer(sim, actor.owner()) {
            match sim.foot_object_class_action_with(
                control,
                viewer,
                actor_id,
                target_id,
                crate::sim::world::MoveAction::None,
                rules,
            ) {
                // Harvest is a Unit's Cell answer, never an object's.
                crate::sim::world::MoveAction::None
                | crate::sim::world::MoveAction::Harvest
                | crate::sim::world::MoveAction::EnterTunnel => AttackAction::None,
                crate::sim::world::MoveAction::Move => AttackAction::Move,
                crate::sim::world::MoveAction::NoMove => AttackAction::NoMove,
            }
        } else {
            AttackAction::None
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
    // answers yes (`GameEntity::is_under_emp`): one under EMP or one counting
    // down art `DeathFrames=`. Not ported. Stock EMP is UNREACHABLE:
    // EMPulseClass::Apply (0x004C5718) is reached only from the unreferenced
    // constructor 0x004C52B0. A death-counting Unit has Health 0 and cannot be
    // selected (0x006F32D3). `DeployToFire=` is not in retail RULESMD, and
    // for a human's Unit CanDeployNow needs it (its TickTank branch is
    // computer-only), so that arm is DORMANT.
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
        // Attack, not a replacement range owner. A Tiberian Sun limpet mine
        // takes no attack order either: Firestorm 2.0 `BuildingClass::
        // What_Action` tests `IsLimpetMine=` in the same list (object
        // `0x0042ECC6`, cell `0x0042EF65`; `sim::limpet`).
        if action == AttackAction::Attack
            && let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
                actor,
                kind,
                0,
                sim.entities(),
                rules,
                sim.interner(),
            )
            && (rules
                .weapon_projectile(weapon)
                .is_some_and(|projectile| !projectile.ag)
                || kind.emp_pulse_cannon
                || kind.is_limpet_mine
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
            && human
        {
            // Cell44774B..447768; existing factory destination owner commits it.
            action = AttackAction::Rally;
        }
    }
    action
}

/// [`attack_action`] before the class rungs: Techno's rung and tail and,
/// over an object, Foot's shroud rung. It is the answer each class
/// What_Action over an object starts from (Unit `0x0073FD62`, Infantry
/// `0x0051E44C`, Aircraft `0x00417CD3`), which Infantry's C4 and
/// infiltration arms test for Attack (`cmp ebp,5` at `0x0051E9F1` and
/// `0x0051EE62`, [`infantry_arm_attacks`]).
fn base_action(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> AttackAction {
    let Some(actor) = sim.entities().get(actor_id) else {
        return AttackAction::None;
    };
    // gamemd.exe TechnoClass::What_Action_OnCell @0x00700600 (700609)
    // and What_Action_OnObject @0x006FFEC0 (6FFEC9) refuse berserk before
    // modifier and weapon checks. The class wrappers own action 0's fallback.
    if actor.berserk.is_active() {
        return AttackAction::None;
    }
    let Some(kind) = sim.interner().object_type(actor.type_ref(), rules) else {
        return AttackAction::None;
    };
    let human = control.can_order(actor.owner());
    let armed = crate::sim::combat::combat_weapon::is_armed(
        actor,
        kind,
        sim.entities(),
        rules,
        sim.interner(),
    );
    let infiltrator = actor.category() == EntityCategory::Infantry && kind.infiltrate;
    let admitted = match target {
        TargetKind::Entity(target_id) => {
            human
                && (armed || infiltrator)
                && (forced
                    || (object_attack_is_admitted(sim, rules, actor_id, *target_id)
                        && object_target_is_legal(sim, rules, actor_id, *target_id)))
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
                && !(actor.is_mission_only() && actor.seldom.discovery.owned_by_current_house())
                && sim.object_is_selectable(target_id, rules)
                && !object.seldom.selection.is_selected()
                && !(warp_test && object.is_warped_out())
        })
    };
    // An object being warped out (vt+0x1D4) answers before the attack rung:
    // 0 on a Cell (`0x00700731`; warping in too, `0x00700743`), and over an
    // object the tail's Select, else 0. The class tails below still run on
    // the answer; none turns its 0 into NoMove, as the object is not
    // controllable (vt+0xA0, `0x00700C40`), but a Building's Cell tail still
    // answers its rally.
    let warping =
        actor.is_warped_out() || (actor.is_warping_in() && matches!(target, TargetKind::Cell(..)));
    let mut action = if warping {
        match *target {
            TargetKind::Entity(target_id) if selects(target_id, false) => AttackAction::Select,
            _ => AttackAction::None,
        }
    } else if admitted
        && (sim.local_techno_controllable_with(control, actor_id, rules)
            || resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain(),
                overlay_registry,
                Some(target),
            ))
        && (forced || !shot_is_illegal(sim, rules, actor_id, target))
        // Ares3.0 hooks700536/7008D4 ->1004A460/1004A430 leave
        // this rung for NoManualFire. Keep the original selection/Foot
        // fallback below; a Unit's rejected object attack becomes NoMove,
        // whose click still moves near the object. Do not gate CanFire or
        // received simulation missions: auto-targeting/scripts bypass UI.
        // Infantry's C4 and infiltration arms ask this Attack too
        // ([`infantry_arm_attacks`]).
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
    if action != AttackAction::None
        && let TargetKind::Entity(target_id) = *target
        && let Some(shrouded) = shrouded_object_action(sim, control, rules, actor_id, target_id)
    {
        action = match shrouded {
            crate::sim::world::MoveAction::Move => AttackAction::Move,
            crate::sim::world::MoveAction::NoMove => AttackAction::NoMove,
            crate::sim::world::MoveAction::None
            | crate::sim::world::MoveAction::Harvest
            | crate::sim::world::MoveAction::EnterTunnel => AttackAction::None,
        };
    }
    action
}

/// Foot's shroud rung (`0x004DDED0`) for a nonzero answer over `object`, a
/// Techno or Terrain object: in a campaign, where the local player's map
/// shrouds the object's coordinate (the Map's own shroud, whoever owns the
/// actor), Move or NoMove ([`Simulation::foot_object_shroud_action`]), which
/// the Move-action class rungs then take
/// ([`Simulation::foot_object_class_action`]). None where it does not apply.
/// Techno's key answers ([`object_key_action`]) reach it as its other
/// answers do ([`base_action`]).
///
/// [`Simulation::foot_object_shroud_action`]: crate::sim::world::Simulation::foot_object_shroud_action
/// [`Simulation::foot_object_class_action`]: crate::sim::world::Simulation::foot_object_class_action
pub(super) fn shrouded_object_action(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    object: u64,
) -> Option<crate::sim::world::MoveAction> {
    let player = sim.session().current_house()?;
    let shrouded = sim.foot_object_shroud_action(player, actor_id, object, rules)?;
    let viewer = context_actor_viewer(sim, sim.entities().get(actor_id)?.owner())?;
    Some(sim.foot_object_class_action_with(control, viewer, actor_id, object, shrouded, rules))
}

/// Whether Infantry's C4 and infiltration arms see Attack over `target_id`
/// (`cmp ebp,5` at `0x0051E9F1` and `0x0051EE62`): no key rung answers
/// ([`object_key_action`]; Ctrl+Shift+Alt's Move, as Shift clears Ctrl at
/// `0x007000AC..0x007000B4`), and the unforced [`base_action`] is Attack, so
/// they refuse where Techno's rung does (an ILLEGAL shot without the
/// Infiltrate grant, `NoManualFire=`) or Foot's shroud rung answers Move or
/// NoMove. Cursor and click ask it alike.
pub(in crate::app::input) fn infantry_arm_attacks(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    infantry_arm_attacks_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        target_id,
        modifier,
        force_move_key,
        overlay_registry,
    )
}

pub(super) fn infantry_arm_attacks_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    object_key_action_with(
        sim,
        control,
        rules,
        actor_id,
        target_id,
        Some(target_id),
        modifier,
        force_move_key,
    )
    .is_none()
        && base_action(
            sim,
            control,
            rules,
            actor_id,
            &TargetKind::Entity(target_id),
            false,
            overlay_registry,
        ) == AttackAction::Attack
}

/// Whether an Aircraft's 0 over the object `target_id` becomes Attack. The
/// Aircraft wrapper asks its own What_Action on the Cell under the object
/// (`0x00417D7C..0x00417DCB`, [`Simulation::object_coords_cell`]): Aircraft
/// `0x00417F80`, a human player's only, through Foot's shroud wrapper
/// (`0x004DDDE0`) to Techno `0x00700600`, whose unforced Attack rung
/// (`0x007008A6..0x007009F1`, [`Simulation::cell_attack_rung_admits`]) answers
/// over a destroyable cliff or another house's Wall its `Wall=` warhead may hit
/// (retail Harrier and Black Eagle warheads have it), and whose TerrainFire
/// rung (`0x00700A46`) answers over an `IsARock=` overlay for a `TerrainFire=`
/// weapon, each with the forced shot's tail at the Cell. A shrouded Cell takes
/// the Attack back (`0x004DDE64..0x004DDEBE`). Aircraft `0x00417FB1` and
/// `0x00417DCD` keep it only with a weapon in slot 0 (vt+0x3F8). The Immune
/// rung then tests the object (`0x00417F4A`), and the click sends the Attack to
/// Foot `0x004D74E0` as an attack on the object
/// ([`object_attack_click_command`]).
///
/// RESIDUAL: the slot-0 test stands on the rung's current weapon. Trigger: an
/// aircraft type with a Secondary but no Primary weapon (none in retail, whose
/// aircraft all carry a Primary). Effect: Attack where native answers 0.
///
/// [`Simulation::object_coords_cell`]: crate::sim::world::Simulation::object_coords_cell
/// [`Simulation::cell_attack_rung_admits`]: crate::sim::world::Simulation::cell_attack_rung_admits
fn aircraft_cell_attack_under_object(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
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
    let Some(viewer) = context_actor_viewer(sim, actor.owner()) else {
        return false;
    };
    sim.clicked_cell_is_shrouded(viewer, cell) == Ok(false)
        && sim.cell_attack_rung_admits_with(control, actor_id, cell, rules, overlay_registry)
        && attack_action_with(
            sim,
            control,
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
pub(in crate::app::input) fn object_action_answers_friendly(
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
pub(super) fn target_is_immune(
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
    armed_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        target,
        forced,
        overlay_registry,
    )
}

pub(super) fn armed_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
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
        TargetKind::Entity(_) => crate::sim::combat::combat_weapon::is_armed(
            actor,
            object,
            sim.entities(),
            rules,
            sim.interner(),
        ),
    };
    armed.then(|| {
        attack_action_with(
            sim,
            control,
            rules,
            actor_id,
            target,
            forced,
            overlay_registry,
        )
    })
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
                buildings: Some(crate::sim::combat::line_of_fire::CellBuildings::of(sim)),
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
    crate::sim::combat::mapless_in_range(
        actor,
        target,
        weapon,
        rules,
        sim.interner(),
        Some(sim.entities()),
    )
}

/// Resolved attack receiver batch. These are ephemeral input decisions;
/// source state and mutation authority remain with Simulation.
pub(super) struct AttackDispatch {
    pub(super) receivers: std::collections::BTreeSet<u64>,
    pub(super) commands: Vec<Command>,
    pub(super) consumed: bool,
    pub(super) select_fallback: bool,
    /// DetermineAction's object answered Select, so BandBox_LeftUp selects the
    /// target and dispatches nothing (`0x004ABDD3`).
    pub(super) selects: bool,
}

/// The keys a healer's object action reads ([`crate::sim::world::HealArmKeys`]):
/// Alt, the ForceMove binding (held for Ctrl+Alt too), and the Foot answer
/// the infantry tail keeps (`0x0051E776..0x0051E782`): Ctrl+Alt's admitted
/// GuardArea or a Move, which Alt's key rung ([`object_key_action`]) or, in
/// a campaign, Foot's shroud rung over a shrouded object gives
/// (`shroud_moves`, [`base_action`]).
fn heal_arm_keys(
    key_action: Option<ObjectKeyAction>,
    modifier: OrderModifier,
    force_move_key: bool,
    shroud_moves: bool,
) -> crate::sim::world::HealArmKeys {
    crate::sim::world::HealArmKeys {
        force_move: matches!(
            modifier,
            OrderModifier::ForceMove | OrderModifier::GuardArea
        ) || (modifier == OrderModifier::AttackMove && force_move_key),
        guard_area: key_action == Some(ObjectKeyAction::GuardArea),
        moves: key_action == Some(ObjectKeyAction::Move) || shroud_moves,
    }
}

/// Ctrl forcing Techno What_Action_OnObject's attack rung (`0x0070021A`):
/// Ctrl, or Ctrl+Alt whose key rungs refused (`0x00700191..0x00700217`).
pub(in crate::app::input) fn ctrl_forces_attack_rung(
    modifier: OrderModifier,
    key_action: Option<ObjectKeyAction>,
) -> bool {
    key_action.is_none()
        && matches!(
            modifier,
            OrderModifier::ForceFire | OrderModifier::GuardArea
        )
}

/// Infantry What_Action's healer self arm (`0x0051E748`) as the cursor and the
/// click ask it: [`attack_action`]'s GuardArea over the actor itself, with
/// `forced` when Ctrl forces the attack rung ([`ctrl_forces_attack_rung`]),
/// which a healing Engineer refuses first.
pub(in crate::app::input) fn healer_guards_itself(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    forced: bool,
) -> bool {
    attack_action_with(
        sim,
        control,
        rules,
        actor,
        &TargetKind::Entity(actor),
        forced,
        None,
    ) == AttackAction::GuardArea
}

/// Infantry What_Action's healer arm over a Terrain object (`0x0051E797`):
/// the object has no House, so the ally test (`0x004F9A90`) refuses it and a
/// human player's healer answers AttackSupport. The arm comes after Foot's
/// What_Action, so it replaces the key rungs' Move, NoMove or GuardArea
/// too; only Shift's ToggleSelect returns before it (`0x0051E453`), and
/// the Engineer's refusal (`0x0051E6B4`) tests only an Attack answer, which
/// no key rung gives. The cursor and the click share it.
pub(in crate::app::input) fn healer_supports_over_terrain(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    terrain: u64,
) -> bool {
    sim.terrain_objects().get(terrain).is_some()
        && sim.entities().get(actor).is_some_and(|entity| {
            sim.infantry_heal_action_with(
                control,
                actor,
                terrain,
                entity.owner(),
                Default::default(),
                rules,
            ) == Some(crate::sim::world::InfantryHealAction::AttackSupport)
        })
}

/// A healer's action over another Techno, which the cursor and the click
/// share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app::input) enum HealerAction {
    /// A Unit's repair arm ([`crate::sim::world::Simulation::unit_repair_action`]).
    Unit(crate::sim::world::UnitRepairAction),
    /// An Infantryman's heal arm
    /// ([`crate::sim::world::Simulation::infantry_heal_action`]).
    Infantry(crate::sim::world::InfantryHealAction),
}

/// A healer's object action: a Unit's repair arm or an Infantryman's heal
/// arm, which InfantryClass::What_Action_OnObject reaches only after the
/// rows before it ([`infantry_rows_precede_heal_arm`]).
pub(in crate::app::input) fn healer_object_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    target: u64,
    issuing_house: InternedId,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<HealerAction> {
    healer_object_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor,
        target,
        issuing_house,
        modifier,
        force_move_key,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn healer_object_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    target: u64,
    issuing_house: InternedId,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<HealerAction> {
    let key_action = object_key_action_with(
        sim,
        control,
        rules,
        actor,
        target,
        Some(target),
        modifier,
        force_move_key,
    );
    // Without a key answer Foot's is the base action, which the shroud rung
    // makes Move or NoMove (`0x004DDED0`).
    let shroud_moves = key_action.is_none()
        && base_action(
            sim,
            control,
            rules,
            actor,
            &TargetKind::Entity(target),
            ctrl_forces_attack_rung(modifier, key_action),
            None,
        ) == AttackAction::Move;
    let keys = heal_arm_keys(key_action, modifier, force_move_key, shroud_moves);
    if sim.entities().get(actor)?.category() != EntityCategory::Infantry {
        return sim
            .unit_repair_action_with(control, actor, target, issuing_house, keys, rules)
            .map(HealerAction::Unit);
    }
    // The arm answers nothing for an infantryman that does not heal, so ask
    // it before the rows that precede it.
    let action =
        sim.infantry_heal_action_with(control, actor, target, issuing_house, keys, rules)?;
    (!infantry_rows_precede_heal_arm(
        sim,
        control,
        rules,
        actor,
        target,
        modifier,
        force_move_key,
        ctrl_forces_attack_rung(modifier, key_action),
    ))
    .then_some(HealerAction::Infantry(action))
}

/// The rows InfantryClass::What_Action_OnObject answers over `target` before
/// its healer arm: an Engineer's DisarmBomb (`0x0051E462..0x0051E49B`) and
/// building rows (`0x0051E49E..0x0051E665`), the garrison rung
/// (`0x0051E668..0x0051E6A5`) and the Engineer's refusal of an Attack base
/// action (`0x0051E6B4..0x0051E6D5`), `forced` when Ctrl forces it.
#[allow(clippy::too_many_arguments)]
fn infantry_rows_precede_heal_arm(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor: u64,
    target: u64,
    modifier: OrderModifier,
    force_move_key: bool,
    forced: bool,
) -> bool {
    let Some(entity) = sim.entities().get(actor) else {
        return false;
    };
    let engineer = sim
        .interner()
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.engineer);
    (engineer
        && (!super::disarm_bomb_engineers(sim, rules, &[actor], target, entity.owner()).is_empty()
            || sim
                .engineer_building_action_with(control, actor, target, rules)
                .is_some()
            || base_action(
                sim,
                control,
                rules,
                actor,
                &TargetKind::Entity(target),
                forced,
                None,
            ) == AttackAction::Attack))
        || super::infantry_garrisons_with(
            sim,
            control,
            rules,
            actor,
            target,
            modifier,
            force_move_key,
        )
}
