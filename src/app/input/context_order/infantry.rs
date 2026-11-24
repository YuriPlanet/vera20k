//! Infantry object actions: attack post-processing, C4, infiltration, bomb
//! disarming and Crazy Ivan bombs, plus the cursor owner and object-click release
//! the click shares with the cursor.

use super::{
    AttackAction, LeftRelease, OrderModifier, attack_action, attack_action_with,
    infantry_arm_attacks_with, left_release, target_is_immune,
};
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::types::HoverTargetKind;
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// Infantry51E9C1..51EB21 post-processes an admitted Attack5. Weapon
/// cursor actions64/71 remain Attack commands; C4 action16 plants a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app::input) enum InfantryAttackAction {
    Attack,
    Sabotage,
    WeaponDemolish,
    AirStrike,
    Select,
    Infiltrate,
    NoMove,
}

pub(in crate::app::input) fn infantry_attack_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> Option<InfantryAttackAction> {
    infantry_attack_action_with(sim, &sim.control_projection(), rules, actor_id, target_id)
}

pub(super) fn infantry_attack_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
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
        if let Some(c4) = infantry_c4_action_with(sim, control, rules, actor_id, &hover, true) {
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
    // Infantry51EAD2..51EB21: anything the airstrike arm accepts keeps Attack
    // and returns before the tail below. Phobos
    // `TechnoClass_WhatAction_AllowAirstrike` (`0x51EAE0`) replaces the
    // building-only `CanC4` test: `AllowAirstrike` decides, defaulting to
    // `CanC4` for a building and to targetable for every other kind, while
    // `InvisibleInGame` still refuses a building.
    if weapon.mig_attack_cursor {
        #[cfg(feature = "phobos")]
        let air_strike = target.is_some_and(|target| {
            sim.interner()
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| {
                    let building = target.category() == EntityCategory::Structure;
                    object.allows_airstrike(building) && !(building && object.invisible_in_game)
                })
        });
        #[cfg(not(feature = "phobos"))]
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
    if sim.infiltrator_building_action_with(control, actor_id, target_id, rules) {
        return Some(InfantryAttackAction::Infiltrate);
    }
    // Infantry51F160..51F179: the Attack left is NoMove over an Immune=
    // type. A player's Crazy Ivan has returned at his bomb arm
    // (51EB24..51EB7E) before it.
    if !(object.ivan && control.can_order(actor.owner()))
        && target_is_immune(sim, rules, &TargetKind::Entity(target_id))
    {
        return Some(InfantryAttackAction::NoMove);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app::input) enum InfantryC4Action {
    Attack,
    Sabotage,
}

/// The type-C4 arm of Infantry What_Action51E9C1..51EB21, which needs the
/// Attack it is handed (`admitted_attack`, `cmp ebp,5` at `0x0051E9F1`):
/// unforced, [`infantry_arm_attacks`]. A non-vehicle building
/// terminates this arm even when CanC4/InvisibleInGame leave action Attack;
/// it must not fall through to the later infiltration arm (51EE54).
/// Shared by click dispatch and cursor feedback. Rank-granted C4 remains
/// outside the existing type-C4 input path.
pub(in crate::app::input) fn infantry_c4_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    hover: &HoverTargetKindWithId,
    admitted_attack: bool,
) -> Option<InfantryC4Action> {
    infantry_c4_action_with(
        sim,
        &sim.control_projection(),
        rules,
        actor_id,
        hover,
        admitted_attack,
    )
}

pub(super) fn infantry_c4_action_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    hover: &HoverTargetKindWithId,
    admitted_attack: bool,
) -> Option<InfantryC4Action> {
    if !admitted_attack {
        return None;
    }
    let actor = sim.entities().get(actor_id)?;
    let target = sim.entities().get(hover.stable_id)?;
    if actor.category() != EntityCategory::Infantry
        || target.category() != EntityCategory::Structure
        || !control.can_order(actor.owner())
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
/// (`Simulation::infiltrator_building_action`), which needs their Attack
/// there ([`infantry_arm_attacks`]): `ClickedAction_Object`
/// (0x0051F190) keeps action 9, and `FootClass::ClickedAction_Object`
/// (0x004D74E0) queues it as it does an Engineer's capture. None when no
/// selected object takes the arm.
pub(super) fn infiltrator_capture_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
    ) {
        return None;
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    let control = sim.control_projection();
    selected.retain(|&id| {
        let attacks = infantry_arm_attacks_with(
            sim,
            &control,
            rules,
            id,
            hover.stable_id,
            modifier,
            force_move_key,
            overlay_registry,
        );
        if !attacks
            || infantry_c4_action_with(sim, &control, rules, id, hover, attacks).is_some()
            || crate::sim::passenger::can_enter_transport(sim, rules, id, hover.stable_id)
            || !sim.infiltrator_building_action_with(&control, id, hover.stable_id, rules)
        {
            return true;
        }
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if sim.local_techno_controllable_with(&control, id, rules) {
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
pub(super) fn disarm_bomb_engineers(
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
    // Ares InfantryClass_GetActionOnObject2 (10056C00, hook 0x51E488) skips
    // a bomb that is not `IvanBomb.Detachable`.
    if !bombed || !sim.bomb_detachable(target, rules) || !sim.bomb_seen_by(target, player) {
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
pub(super) fn cursor_owner(
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
pub(super) fn object_click_release(
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

/// What a selected Crazy Ivan's click on an own or allied object does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FriendlyIvanAction {
    /// IvanBomb, sent as Attack.
    Bomb,
    /// Ares' Grind over an allied Grinder ([`ares_ivan_grinds`]), sent as
    /// mission Eaten.
    Grind,
}

/// The selected Crazy Ivans whose click on the own or allied `target` plants a
/// bomb or, with Ares, walks into it as a Grinder ([`ares_ivan_grinds`],
/// which answers ahead of the bomb test). The canonical Techno grant and
/// ILLEGAL test must already answer Attack ([`attack_action`], which the
/// cursor asks too). Infantry51EB24..7E consumes that answer without
/// repeating its type/weapon grants; the Ivan makes it IvanBomb
/// ([`ivan_bomb_refused`], 0x0051EB24), and `FootClass::ClickedAction_Object`
/// (0x004D74E0) sends that as Attack. A bombed friend stays a selection
/// click. A lone selected Ivan over himself has the self action (4) instead;
/// in a larger selection nothing stops him bombing himself, as natively. The
/// count is the whole selection's (`[0xA8ECC8]`), berserk objects included.
pub(super) fn friendly_ivans(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    selection_count: usize,
    target: u64,
    force_fire_key: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Vec<(u64, FriendlyIvanAction)> {
    let bombable = sim.entities().get(target).is_some_and(|entity| {
        let obj = sim.interner().object_type(entity.type_ref(), rules);
        !ivan_bomb_refused(entity, obj)
    });
    let control = sim.control_projection();
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
                && attack_action_with(
                    sim,
                    &control,
                    rules,
                    id,
                    &TargetKind::Entity(target),
                    false,
                    overlay_registry,
                ) == AttackAction::Attack
        })
        .filter_map(|id| {
            if ares_ivan_grinds(sim, rules, id, target, force_fire_key) {
                Some((id, FriendlyIvanAction::Grind))
            } else {
                bombable.then_some((id, FriendlyIvanAction::Bomb))
            }
        })
        .collect()
}

/// Ares 3.0 InfantryClass_GetActionOnObject_IvanGrinder (Ares.dll
/// `100254B0`, hook `0x0051EB48`, `Ext/Infantry/Hooks.cpp`), at the head of
/// the Ivan arm, ahead of its bomb test: a Crazy Ivan (`+0xEAE`) over a
/// `Grinding=` building (Type `+0x16AD`) of a House allied with his own
/// (the same House or his ally bits, `+0x5788`) answers Grind (0xB) instead
/// of IvanBomb or NoIvanBomb, unless the force-fire key is down
/// (`InputManagerClass::IsForceFireKeyPressed`: the keys at `0x00A8EC00`
/// and `0x00A8EC04`, which Shift does not cancel here as it does in the
/// stock rows). Ares' stub (`10025410`) returns the answer from
/// What_Action at once, and Foot ClickedAction (`0x004D74E0`) sends it as
/// mission Eaten with the Grinder as Destination. The player's own Grinder
/// is allied too.
pub(in crate::app::input) fn ares_ivan_grinds(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    ivan_id: u64,
    target_id: u64,
    force_fire_key: bool,
) -> bool {
    if !rules.ares() || force_fire_key {
        return false;
    }
    let (Some(ivan), Some(target)) = (sim.entities().get(ivan_id), sim.entities().get(target_id))
    else {
        return false;
    };
    sim.interner()
        .object_type(ivan.type_ref(), rules)
        .is_some_and(|object| object.ivan)
        && target.category() == EntityCategory::Structure
        && sim
            .interner()
            .object_type(target.type_ref(), rules)
            .is_some_and(|object| object.grinding)
        && crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(sim.house_alliances()),
            sim.interner(),
            ivan.owner(),
            target.owner(),
        )
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
pub(super) fn ivan_cannot_bomb(
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
pub(super) fn shot_is_illegal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
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
            // Ares 3.0 100261F0 (hook 0x007004AD): any action of the shared
            // classification keeps the rung.
            if rules.ares() {
                sim.infiltrate_action(actor_id, target_id, rules)
                    != crate::sim::world::InfiltrateAction::None
            } else {
                (if infantry.agent {
                    building.spyable
                } else {
                    building.capturable
                }) || (infantry.c4 && building.can_c4)
            }
        });
    !infiltrate_arm
        && sim
            .effective_shared_cell_dummy()
            .detached(|| sim.selected_weapon_fire_error(rules, actor_id, *target, true))
            == crate::sim::combat::fire_error::FireError::Illegal
}
