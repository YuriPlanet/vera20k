//! Entity picking, hover-target resolution, and selection snapshot computation.
//!
//! Handles click and box selection logic, enemy target picking for attack
//! commands, and hover-target classification (friendly/enemy/structure/unit).
//!
//! Split from the presentation render path to keep files under 400 lines.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::types::HoverTargetKind;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;

/// Elliptical distance threshold for picking objects at a screen point.
/// `dx*dx + 0.5*dy*dy < 200` with 0.5 Y-weight compensating for isometric projection.
/// Max horizontal reach: sqrt(200) ≈ 14px, max vertical reach: sqrt(400) = 20px.
const PICK_DISTANCE_THRESHOLD: f32 = 200.0;
type TacticalBridgeInverseMap =
    std::collections::BTreeMap<(u16, u16), crate::map::terrain::TacticalBridgeCell>;
/// The simulation's house table, as the picker needs it: owner → player state.
type HouseStates = std::collections::BTreeMap<InternedId, crate::sim::house_state::HouseState>;

/// Ordered calls into the native selection vector. `clear` runs first,
/// `deselect` removes exact members without disturbing survivors, and `select`
/// is consumed in source encounter order (the insertion policy is owned by the
/// app selection ledger).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SelectionMutation {
    pub(crate) clear: bool,
    pub(crate) deselect: Vec<u64>,
    pub(crate) select: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeSelectTapResult {
    pub(crate) mutation: SelectionMutation,
    pub(crate) outcome: crate::app::types::TypeSelectOutcome,
    pub(crate) across_map: bool,
}

/// EntityStore keys are monotonic object identities, hence creation order.
pub(crate) fn map_entity_creation_order(entities: &EntityStore) -> Vec<u64> {
    entities.values().map(|entity| entity.stable_id()).collect()
}

/// Y-axis weight in the elliptical pick distance formula.
/// Makes the hit zone taller than wide, matching the isometric projection where
/// units appear elongated vertically.
const PICK_Y_WEIGHT: f32 = 0.5;

/// Compute the elliptical pick distance for object selection.
/// Returns `dx² + 0.5 * dy²` — an ellipse wider vertically than horizontally.
fn pick_distance_sq(dx: f32, dy: f32) -> f32 {
    dx * dx + PICK_Y_WEIGHT * dy * dy
}

pub(crate) fn pick_enemy_target_stable_id(
    sim: &crate::sim::world::Simulation,
    world_x: f32,
    world_y: f32,
    friendly_owner: &str,
    ignore_visibility: bool,
    rules: Option<&RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
) -> Option<u64> {
    hover_target_at_point(
        sim,
        world_x,
        world_y,
        friendly_owner,
        ignore_visibility,
        rules,
        height_map,
        bridge_height_map,
    )
    .and_then(|hover| match hover.kind {
        HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure => Some(hover.stable_id),
        _ => None,
    })
}

/// Force-fire: pick any entity under cursor (including friendlies).
pub(crate) fn pick_any_target_stable_id(
    sim: &crate::sim::world::Simulation,
    world_x: f32,
    world_y: f32,
    ignore_visibility: bool,
    rules: Option<&RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
) -> Option<u64> {
    // Use empty owner so everything is considered "enemy" in hover logic.
    hover_target_at_point(
        sim,
        world_x,
        world_y,
        "",
        ignore_visibility,
        rules,
        height_map,
        bridge_height_map,
    )
    .filter(|hover| hover.kind != HoverTargetKind::HiddenEnemy)
    .map(|hover| hover.stable_id)
}

pub(crate) fn hover_target_at_point(
    sim: &crate::sim::world::Simulation,
    world_x: f32,
    world_y: f32,
    local_owner: &str,
    ignore_visibility: bool,
    rules: Option<&RuleSet>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
) -> Option<HoverTargetKindWithId> {
    let local_owner_id: InternedId = sim.interner.get(local_owner).unwrap_or_default();
    let mut best: Option<(u64, f32)> = None;
    for entity in sim.entities().values() {
        let is_structure = entity.category == EntityCategory::Structure;
        let type_str = sim.interner.resolve(entity.type_ref());
        let owner_str = sim.interner.resolve(entity.owner());
        let (sx, sy) = if is_structure {
            crate::render::locomotor_visual::screen_position(entity)
        } else {
            crate::app::presentation::instances::interpolated_screen_position_entity(entity)
        };
        // Hit test: structures use foundation cells, mobile units use elliptical distance.
        if is_structure {
            let foundation = rules
                .and_then(|r| r.object(type_str))
                .map(|o| o.foundation.as_str())
                .unwrap_or("1x1");
            if !click_hits_foundation(
                world_x,
                world_y,
                entity.position.rx,
                entity.position.ry,
                foundation,
                height_map,
                bridge_height_map,
            ) {
                continue;
            }
        } else {
            let dx = sx - world_x;
            let dy = sy - world_y;
            if pick_distance_sq(dx, dy) >= PICK_DISTANCE_THRESHOLD {
                continue;
            }
        }
        // Distance for tie-breaking (prefer closest to anchor point).
        let dx = sx - world_x;
        let dy = sy - world_y;
        let dist_sq = dx * dx + dy * dy;
        let is_friendly = sim.fog.is_friendly(local_owner, owner_str);
        let is_visible = ignore_visibility
            || (sim
                .fog
                .is_cell_revealed(local_owner_id, entity.position.rx, entity.position.ry)
                && !sim.fog.is_cell_gap_covered(
                    local_owner_id,
                    entity.position.rx,
                    entity.position.ry,
                ));
        let kind = if is_friendly {
            if is_structure {
                HoverTargetKind::FriendlyStructure
            } else {
                HoverTargetKind::FriendlyUnit
            }
        } else if !is_visible {
            HoverTargetKind::HiddenEnemy
        } else if is_structure {
            HoverTargetKind::EnemyStructure
        } else {
            HoverTargetKind::EnemyUnit
        };
        match best {
            Some((_, best_dist_sq)) if dist_sq >= best_dist_sq => {}
            _ => best = Some((encode_hover_kind_with_id(kind, entity.stable_id()), dist_sq)),
        }
    }
    best.map(|(encoded, _)| decode_hover_kind_with_id(encoded))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HoverTargetKindWithId {
    pub(crate) kind: HoverTargetKind,
    pub(crate) stable_id: u64,
}

fn encode_hover_kind_with_id(kind: HoverTargetKind, stable_id: u64) -> u64 {
    let kind_bits = match kind {
        HoverTargetKind::FriendlyUnit => 1u64,
        HoverTargetKind::FriendlyStructure => 2,
        HoverTargetKind::EnemyUnit => 3,
        HoverTargetKind::EnemyStructure => 4,
        HoverTargetKind::HiddenEnemy => 5,
    };
    (stable_id << 3) | kind_bits
}

fn decode_hover_kind_with_id(encoded: u64) -> HoverTargetKindWithId {
    let kind = match encoded & 0b111 {
        1 => HoverTargetKind::FriendlyUnit,
        2 => HoverTargetKind::FriendlyStructure,
        3 => HoverTargetKind::EnemyUnit,
        4 => HoverTargetKind::EnemyStructure,
        _ => HoverTargetKind::HiddenEnemy,
    };
    HoverTargetKindWithId {
        kind,
        stable_id: encoded >> 3,
    }
}

#[cfg(test)]
pub(crate) fn compute_click_selection_snapshot(
    entities: &EntityStore,
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    world_x: f32,
    world_y: f32,
    click_radius: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    houses: Option<&HouseStates>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<SelectionMutation> {
    compute_click_selection_snapshot_with_playfield(
        entities,
        encounter_order,
        current_selection,
        fog,
        local_owner,
        world_x,
        world_y,
        click_radius,
        additive,
        rules,
        houses,
        height_map,
        bridge_height_map,
        interner,
        false,
    )
}

pub(crate) fn compute_click_selection_snapshot_with_playfield(
    entities: &EntityStore,
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    world_x: f32,
    world_y: f32,
    click_radius: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    houses: Option<&HouseStates>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> Option<SelectionMutation> {
    let Some(picked_sid) = pick_entity_at_point(
        entities,
        encounter_order,
        fog,
        local_owner,
        world_x,
        world_y,
        click_radius,
        rules,
        houses,
        height_map,
        bridge_height_map,
        interner,
    ) else {
        // Non-shift click clears selection; shift-click with no hit keeps it.
        return if additive {
            None
        } else if current_selection.is_empty() {
            None
        } else {
            Some(SelectionMutation {
                clear: true,
                ..Default::default()
            })
        };
    };

    let picked = entities.get(picked_sid)?;
    let type_str = interner.map_or("", |i| i.resolve(picked.type_ref()));
    let admitted = static_selection_gate(picked, type_str, rules, require_playfield_membership);
    // A shift-click adds when the selection it is joining is the local player's,
    // whatever was clicked. The native ACTION_SELECT arm asks
    // `HouseClass__IsHumanPlayer(CurrentObjects[0]->Owner)` — 0x004ABD42 loads
    // `CurrentObjects[0]`, calls its owner virtual `+0x3C`, and 0x004ABD50 calls
    // 0x0050B6F0, whose real test is `house == g_PlayerPtr`; a false answer at
    // 0x004ABD57 jumps to the replace path. It is a test on what is ALREADY
    // selected, never on whether the clicked object matches it.
    //
    // Enemy clicks genuinely reach this arm. `TechnoClass__What_Action_OnObject`
    // 0x006FFEC0 short-circuits to ACTION_SELECT at 0x00700107 whenever the
    // Shift pair `DAT_00A8EC08`/`0C` is down and the Ctrl pair is not — gated on
    // `IsHumanPlayer` of the SELECTED object's owner `[ESI+0x21C]`, with no test
    // on the clicked object at all.
    //
    // That first test is only half the decision. The arm's add branch calls the
    // clicked object's vtable `+0x14C` Select (TechnoClass vtable base
    // 0x007F4960, slot 0x007F4AAC = `TechnoClass__Select` 0x006FBFA0), which
    // always reaches `ObjectClass__Select` 0x005F4520. With a non-empty
    // selection that body compares `IsHumanPlayer` of the NEW object against
    // `IsHumanPlayer` of `CurrentObjects[0]` (0x005F45D2/0x005F45DF): differing
    // answers fall to `Unselect_All_With_Mode_Reset` 0x0048DC90 at 0x005F45F8,
    // and equal answers keep the selection only when that answer was true
    // (0x005F45EF/0x005F45F6). So the selection survives a shift-click only when
    // BOTH sides are the local player's — shift-clicking an enemy while your own
    // army is selected drains the army and leaves the enemy alone selected.
    //
    // VERA-internal: gamemd always has a `g_PlayerPtr`, so it has no answer for
    // "the local house is unknown". Sandbox and test callers pass no owner, and
    // those keep the ordinary add/toggle rather than being forced down the
    // replace path. gamemd equivalent UNCHECKED because the state cannot arise —
    // both production call sites always pass a resolved owner and interner.
    let owner_is_local = |entity: &crate::sim::game_entity::GameEntity| -> bool {
        match (local_owner, interner) {
            (Some(owner), Some(names)) => names.resolve(entity.owner()).eq_ignore_ascii_case(owner),
            _ => true,
        }
    };
    let selection_is_local = current_selection
        .first()
        .and_then(|id| entities.get(*id))
        .is_some_and(|first| owner_is_local(first));
    let picked_is_local = owner_is_local(picked);

    if !additive || current_selection.is_empty() || !selection_is_local || !picked_is_local {
        return Some(SelectionMutation {
            clear: true,
            select: admitted.then_some(picked_sid).into_iter().collect(),
            ..Default::default()
        });
    }
    if current_selection.contains(&picked_sid) {
        Some(SelectionMutation {
            deselect: vec![picked_sid],
            ..Default::default()
        })
    } else {
        admitted.then(|| SelectionMutation {
            select: vec![picked_sid],
            ..Default::default()
        })
    }
}

/// Did the band box catch anything at all?
///
/// gamemd asks this before it clears the selection, and the question is far
/// looser than "which objects join the selection": it walks the drawn-object
/// list and accepts any live object inside the rectangle — no owner filter, no
/// `Selectable=`, no limbo or docked test. A rectangle holding nothing but enemy
/// units, or a single building, still counts as caught and still replaces the
/// selection. A rectangle holding nothing at all leaves the selection untouched
/// and the release falls through to the ordinary click/action path.
///
/// Limbo is the one exclusion, and it is a registration property rather than a
/// filter: an object without map presence is never in the drawn-object list, so
/// a passenger inside a transport cannot make a rectangle "non-empty".
///
/// The list is **partly** visibility-filtered, and the asymmetry is native:
/// mobile objects append themselves from inside their own draw routine, so an
/// object the shroud hides is never registered, while buildings are appended in
/// bulk from the whole building array with no visibility test at all. A building
/// under unexplored shroud therefore does make a rectangle non-empty, and an
/// enemy unit under it does not.
///
/// The registration helper has exactly four call sites, and every one of them is
/// the same virtual — the per-class "draw extras" override — reached once from
/// the vehicle version, twice from the infantry version (its two draw paths) and
/// once from the aircraft version. Nothing outside a draw routine registers an
/// object, so the shroud filter above is complete: there is no second, unfiltered
/// path by which a hidden enemy could enter the list.
pub(crate) fn band_rect_contains_drawn_object(
    entities: &EntityStore,
    encounter_order: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    let local_owner_id = local_owner.and_then(|o| interner.and_then(|i| i.get(o)));
    encounter_order.iter().any(|id| {
        let Some(entity) = entities.get(*id) else {
            return false;
        };
        if !entity.lifecycle.object_alive || entity.lifecycle.in_limbo {
            return false;
        }
        let (sx, sy) =
            crate::app::presentation::instances::interpolated_screen_position_entity(entity);
        if !(sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y) {
            return false;
        }
        if entity.category == EntityCategory::Structure {
            // Bulk-registered, shroud or no shroud.
            return true;
        }
        let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
        is_drawn_for_local_owner(fog, local_owner, owner_str, entity, local_owner_id)
    })
}

/// Would the local player see this object drawn right now?
///
/// Deliberately distinct from the eligibility gate's visibility tail: with no
/// fog state at all (the sandbox full-visibility switch) everything is drawn,
/// where the eligibility gate falls back to an owner test.
fn is_drawn_for_local_owner(
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    entity_owner: &str,
    entity: &crate::sim::game_entity::GameEntity,
    local_owner_id: Option<InternedId>,
) -> bool {
    let Some(local_owner) = local_owner else {
        return true;
    };
    let Some(fog) = fog else {
        // Sandbox full visibility: everything is drawn.
        return true;
    };
    if fog.is_friendly(local_owner, entity_owner) {
        return true;
    }
    let owner_id = local_owner_id.unwrap_or_default();
    let pos = &entity.position;
    fog.is_cell_revealed(owner_id, pos.rx, pos.ry)
        && !fog.is_cell_gap_covered(owner_id, pos.rx, pos.ry)
}

/// Resolve a band-box release into the selection snapshot to commit.
///
/// Three outcomes, matching the native release:
/// * `None` — the rectangle caught no drawn object (or a shift-drag added
///   nothing). The selection is left exactly as it was.
/// * `Some(list)` — the rectangle caught something. A plain drag replaces the
///   selection with the eligible members; a shift drag **adds** them.
///
/// A rectangle that caught something but nothing eligible — a box over an enemy
/// patrol, say — returns an empty list, because the native clear runs before the
/// per-object filter does.
#[cfg(test)]
pub(crate) fn compute_box_selection_snapshot(
    entities: &EntityStore,
    preflight_order: &[u64],
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    houses: Option<&HouseStates>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<SelectionMutation> {
    compute_box_selection_snapshot_with_playfield(
        entities,
        preflight_order,
        encounter_order,
        current_selection,
        fog,
        local_owner,
        min_x,
        min_y,
        max_x,
        max_y,
        additive,
        rules,
        houses,
        interner,
        false,
    )
}

pub(crate) fn compute_box_selection_snapshot_with_playfield(
    entities: &EntityStore,
    preflight_order: &[u64],
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    houses: Option<&HouseStates>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> Option<SelectionMutation> {
    if !band_rect_contains_drawn_object(
        entities,
        preflight_order,
        fog,
        local_owner,
        min_x,
        min_y,
        max_x,
        max_y,
        interner,
    ) {
        return None;
    }
    let candidates = entities_in_rect(
        entities,
        encounter_order,
        fog,
        local_owner,
        min_x,
        min_y,
        max_x,
        max_y,
        rules,
        houses,
        interner,
        require_playfield_membership,
    );
    if additive {
        // The native band callback only ever calls Select — there is no Deselect
        // anywhere on this path, and the shift branch merely skips the clear. So
        // a shift drag across units already in the group keeps them.
        let candidates: Vec<u64> = candidates
            .into_iter()
            .filter(|sid| !current_selection.contains(sid))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        return Some(SelectionMutation {
            select: candidates,
            ..Default::default()
        });
    }
    Some(SelectionMutation {
        clear: true,
        select: candidates,
        ..Default::default()
    })
}

/// Resolve one short TypeSelect release. Seeds retain current selection order
/// and expand each selected type through its deploy/undeploy links. Screen
/// escalation happens in the same invocation when no visible unselected match
/// exists, including the all-visible-already-selected case.
#[cfg(test)]
pub(crate) fn compute_type_select_tap(
    entities: &EntityStore,
    screen_order: &[u64],
    map_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    across_map: bool,
) -> TypeSelectTapResult {
    compute_type_select_tap_with_playfield(
        entities,
        screen_order,
        map_order,
        current_selection,
        fog,
        local_owner,
        rules,
        interner,
        across_map,
        false,
    )
}

pub(crate) fn compute_type_select_tap_with_playfield(
    entities: &EntityStore,
    screen_order: &[u64],
    map_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    across_map: bool,
    require_playfield_membership: bool,
) -> TypeSelectTapResult {
    let mut mutation = SelectionMutation::default();
    let mut selected = current_selection.to_vec();
    if selected.len() == 1 {
        let nonlocal = entities.get(selected[0]).is_some_and(|entity| {
            let owner = interner.map_or("", |i| i.resolve(entity.owner()));
            local_owner.is_some_and(|local| !owner.eq_ignore_ascii_case(local))
        });
        if nonlocal {
            mutation.deselect.push(selected[0]);
            selected.clear();
        }
    }

    let mut seeds: Vec<InternedId> = Vec::new();
    for id in selected.iter().copied() {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        let own = interner.map_or("", |i| i.resolve(entity.type_ref()));
        push_exact_seed(&mut seeds, entity.type_ref());
        if let Some(object) = rules.and_then(|r| r.object(own)) {
            if let Some(deploys_into) = object.deploys_into.as_deref() {
                if let Some(type_ref) = interner.and_then(|i| i.get(deploys_into)) {
                    push_exact_seed(&mut seeds, type_ref);
                }
            }
            if let Some(undeploys_into) = object.undeploys_into.as_deref() {
                if let Some(type_ref) = interner.and_then(|i| i.get(undeploys_into)) {
                    push_exact_seed(&mut seeds, type_ref);
                }
            }
        }
    }
    if seeds.is_empty() {
        return TypeSelectTapResult {
            mutation,
            outcome: crate::app::types::TypeSelectOutcome::Empty,
            across_map: false,
        };
    }

    if !across_map {
        let visible = type_select_candidates(
            entities,
            screen_order,
            &seeds,
            fog,
            local_owner,
            rules,
            interner,
            true,
            require_playfield_membership,
        );
        if visible.iter().any(|id| !selected.contains(id)) {
            mutation.select.extend(type_select_final_admissions(
                entities,
                &visible,
                rules,
                interner,
                require_playfield_membership,
            ));
            return TypeSelectTapResult {
                mutation,
                outcome: crate::app::types::TypeSelectOutcome::Screen,
                across_map: false,
            };
        }
    }

    let map_candidates = type_select_candidates(
        entities,
        map_order,
        &seeds,
        None,
        local_owner,
        rules,
        interner,
        false,
        require_playfield_membership,
    );
    mutation.select.extend(type_select_final_admissions(
        entities,
        &map_candidates,
        rules,
        interner,
        require_playfield_membership,
    ));
    TypeSelectTapResult {
        mutation,
        outcome: crate::app::types::TypeSelectOutcome::Map,
        across_map: true,
    }
}

fn push_exact_seed(seeds: &mut Vec<InternedId>, seed: InternedId) {
    if !seeds.contains(&seed) {
        seeds.push(seed);
    }
}

fn type_select_candidates(
    entities: &EntityStore,
    source_order: &[u64],
    seeds: &[InternedId],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    screen_only: bool,
    require_playfield_membership: bool,
) -> Vec<u64> {
    let local_owner_id = local_owner.and_then(|owner| interner.and_then(|i| i.get(owner)));
    source_order
        .iter()
        .filter_map(|id| {
            let entity = entities.get(*id)?;
            let owner = interner.map_or("", |i| i.resolve(entity.owner()));
            let type_id = interner.map_or("", |i| i.resolve(entity.type_ref()));
            if !entity.lifecycle.object_alive
                || local_owner.is_some_and(|local| !owner.eq_ignore_ascii_case(local))
                || !seeds.contains(&entity.type_ref())
                || (screen_only
                    && entity.category != EntityCategory::Structure
                    && !is_drawn_for_local_owner(fog, local_owner, owner, entity, local_owner_id))
            {
                return None;
            }
            let dynamic = can_be_selected_now(entity, entities, rules, interner)
                && type_is_selectable(type_id, rules)
                && (!require_playfield_membership || entity.in_playfield);
            let undeploying_building_fallback = entity.category == EntityCategory::Structure
                && rules
                    .and_then(|r| r.object(type_id))
                    .is_some_and(|object| object.undeploys_into.is_some());
            (dynamic || undeploying_building_fallback).then_some(*id)
        })
        .collect()
}

fn type_select_final_admissions(
    entities: &EntityStore,
    candidates: &[u64],
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> Vec<u64> {
    candidates
        .iter()
        .copied()
        .filter(|id| {
            entities.get(*id).is_some_and(|entity| {
                let type_id = interner.map_or("", |i| i.resolve(entity.type_ref()));
                static_selection_gate(entity, type_id, rules, require_playfield_membership)
            })
        })
        .collect()
}

/// Exact-type group mutation used while TypeSelect is held over one picked
/// object. Link types and the dynamic CanBeSelectedNow virtual are deliberately
/// absent from this path.
#[cfg(test)]
pub(crate) fn compute_type_select_click_mutation(
    entities: &EntityStore,
    scope_order: &[u64],
    current_selection: &[u64],
    clicked_id: u64,
    additive: bool,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> SelectionMutation {
    compute_type_select_click_mutation_with_playfield(
        entities,
        scope_order,
        current_selection,
        clicked_id,
        additive,
        local_owner,
        rules,
        interner,
        false,
    )
}

pub(crate) fn compute_type_select_click_mutation_with_playfield(
    entities: &EntityStore,
    scope_order: &[u64],
    current_selection: &[u64],
    clicked_id: u64,
    additive: bool,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> SelectionMutation {
    let Some(clicked) = entities.get(clicked_id) else {
        return SelectionMutation::default();
    };
    let clicked_type = clicked.type_ref();
    if additive && current_selection.contains(&clicked_id) {
        return SelectionMutation {
            deselect: exact_type_group(
                entities,
                scope_order,
                clicked_type,
                local_owner,
                rules,
                interner,
                false,
                require_playfield_membership,
            )
            .into_iter()
            .filter(|id| current_selection.contains(id))
            .collect(),
            ..Default::default()
        };
    }
    SelectionMutation {
        clear: !additive,
        select: exact_type_group(
            entities,
            scope_order,
            clicked_type,
            local_owner,
            rules,
            interner,
            true,
            require_playfield_membership,
        ),
        ..Default::default()
    }
}

/// Held TypeSelect bandbox: anchors inside the rectangle contribute their own
/// exact type IDs; those IDs are then expanded over the current screen/map
/// scope without deploy-link or dynamic-gate expansion.
#[cfg(test)]
pub(crate) fn compute_type_select_box_mutation(
    entities: &EntityStore,
    screen_order: &[u64],
    scope_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> SelectionMutation {
    compute_type_select_box_mutation_with_playfield(
        entities,
        screen_order,
        scope_order,
        current_selection,
        fog,
        local_owner,
        min_x,
        min_y,
        max_x,
        max_y,
        additive,
        rules,
        interner,
        false,
    )
}

pub(crate) fn compute_type_select_box_mutation_with_playfield(
    entities: &EntityStore,
    screen_order: &[u64],
    scope_order: &[u64],
    current_selection: &[u64],
    _fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> SelectionMutation {
    let mut represented_types = Vec::new();
    for &id in screen_order {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        let (sx, sy) =
            crate::app::presentation::instances::interpolated_screen_position_entity(entity);
        if !entity.lifecycle.object_alive || sx < min_x || sx > max_x || sy < min_y || sy > max_y {
            continue;
        }
        if !represented_types.contains(&entity.type_ref()) {
            represented_types.push(entity.type_ref());
        }
    }

    let mut select = Vec::new();
    for type_ref in represented_types {
        select.extend(exact_type_group(
            entities,
            scope_order,
            type_ref,
            local_owner,
            rules,
            interner,
            true,
            require_playfield_membership,
        ));
    }
    select.retain(|id| !additive || !current_selection.contains(id));
    SelectionMutation {
        clear: !additive,
        select,
        ..Default::default()
    }
}

fn exact_type_group(
    entities: &EntityStore,
    source_order: &[u64],
    type_ref: InternedId,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    selecting: bool,
    require_playfield_membership: bool,
) -> Vec<u64> {
    source_order
        .iter()
        .filter_map(|id| {
            let entity = entities.get(*id)?;
            let owner = interner.map_or("", |i| i.resolve(entity.owner()));
            let type_id = interner.map_or("", |i| i.resolve(entity.type_ref()));
            if !entity.lifecycle.object_alive
                || entity.lifecycle.in_limbo
                || entity.type_ref() != type_ref
                || local_owner.is_some_and(|local| !owner.eq_ignore_ascii_case(local))
                || (selecting
                    && !static_selection_gate(entity, type_id, rules, require_playfield_membership))
            {
                return None;
            }
            Some(*id)
        })
        .collect()
}

fn entities_in_rect(
    entities: &EntityStore,
    encounter_order: &[u64],
    _fog: Option<&FogState>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    rules: Option<&RuleSet>,
    _houses: Option<&HouseStates>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
) -> Vec<u64> {
    encounter_order
        .iter()
        .filter_map(|entity| {
            let entity = entities.get(*entity)?;
            // Rectangle first, the way the native walk does it: the per-object
            // filter only runs for objects the box actually covers.
            let (sx, sy) =
                crate::app::presentation::instances::interpolated_screen_position_entity(entity);
            if !(sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y) {
                return None;
            }
            let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
            let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
            if !is_local_band_candidate(
                local_owner,
                owner_str,
                entity,
                type_str,
                rules,
                require_playfield_membership,
            ) {
                return None;
            }
            if !can_be_selected_now(entity, entities, rules, interner) {
                return None;
            }
            Some(entity.stable_id())
        })
        .collect()
}

/// The band-box-only half of the eligibility chain.
///
/// A single click reaches an object through a shorter chain than a band box
/// does: only the band path asks `CanBeSelectedNow`, which refuses an object
/// that is enslaved, powered down, linked into a bunker, or radio-docked on a
/// building. The docked clause is the one players feel — it keeps the miner
/// unloading on the refinery pad out of a box dragged across the base, while a
/// direct click on that same miner still selects it.
///
/// The radio-dock clause is a **pair** of tests, and both halves matter: the
/// dock flag alone is not enough, the object also has to be standing on a
/// building. That second half is what keeps the rule narrow — the same flag is
/// raised on a vehicle leaving a war factory, and such a vehicle stops being
/// refused the moment it drives clear of the factory footprint, exactly as the
/// native cell lookup behaves.
///
/// Two native clauses are approximated rather than reproduced, because VERA has
/// no matching state:
/// * The native enslaved test reads a slave-owner pointer that every slave
///   carries. VERA's nearest state is the slave-harvester component, which only
///   the Slave Miner's slaves own; a mind-controlled unit is a different field
///   in the original and is deliberately not tested here.
/// * The native bunker test reads one link field. VERA splits the link into an
///   approach marker and an installed link, and only the installed link is
///   treated as linked.
///
/// The powered-down clause reads byte `+0x1C8`, and it is NOT a deploy-transition
/// flag. An exhaustive instruction search finds exactly three writers —
/// `TechnoClass__Constructor` 0x006F2C9A initialises it,
/// `TechnoClass__OnDeployBegin` 0x0070FD62 raises it, and
/// `TechnoClass__OnUndeployComplete` 0x0070FC6D/0x0070FC7A clear it — and
/// neither handler has a vtable or DATA reference. Their only callers are
/// `HouseClass__LostPoweredCenter`, `HouseClass__Removed_From_Game`,
/// `UnitClass__PerCellProcess` and `HouseClass__RobotTanksBackOnline`. So it is
/// a *latch* held for as long as a Robot Tank is offline for want of a Robot
/// Control Center — never raised by an infantry deploy toggle or an ordinary MCV
/// deploy. VERA previously refused every mid-deploy object here, which is a gate
/// gamemd does not have: it silently dropped GIs out of a box dragged over them
/// during their deploy animation. That gate is gone; the offline latch itself
/// waits on the Robot Control Center mechanism, which has no owner yet.
fn can_be_selected_now(
    entity: &crate::sim::game_entity::GameEntity,
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    // BuildingClass answers false on the ordinary dynamic selection virtual.
    // TypeSelect owns the verified UndeploysInto fallback separately.
    if entity.category == EntityCategory::Structure {
        return false;
    }
    if entity.slave_harvester.is_some() {
        return false;
    }
    if entity.bunker_link.installed_in().is_some() {
        return false;
    }
    if entity.dock_entered_with.is_some()
        && building_covers_cell(
            entities,
            rules,
            interner,
            entity.position.rx,
            entity.position.ry,
        )
    {
        return false;
    }
    true
}

/// Does a building's footprint cover this cell? The native lookup asks the cell
/// itself; VERA has no cell→occupier index on this path, so the building list is
/// walked instead. It only runs for an object that already carries the dock
/// flag, which is a handful of objects in a normal match.
///
/// A destroyed building is skipped as well as a limboed one: the native cell
/// occupier is cleared the moment the building leaves the map, so a dead
/// refinery must not keep the miner standing on its pad out of a band box.
///
/// The footprint is treated as the plain `Foundation=` rectangle. `Foundation=`
/// is not free-form — it resolves through a fixed table — and every value that
/// appears in stock `artmd.ini` is a plain `WxH` name, so the rectangle covers
/// stock play. The table also carries named entries the stock art never selects
/// (`3x3Refinery`); whether their native occupancy is the full rectangle is
/// UNCHECKED, so a mod that selects one is the residual here, not stock YR.
fn building_covers_cell(
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    rx: u16,
    ry: u16,
) -> bool {
    entities.values().any(|candidate| {
        if candidate.category != EntityCategory::Structure
            || candidate.lifecycle.in_limbo
            || !candidate.lifecycle.object_alive
        {
            return false;
        }
        let type_str = interner.map_or("", |i| i.resolve(candidate.type_ref()));
        let foundation = rules
            .and_then(|r| r.object(type_str))
            .map(|o| o.foundation.as_str())
            .unwrap_or("1x1");
        let (fw, fh) = crate::rules::foundation::foundation_dimensions(foundation);
        let brx = candidate.position.rx;
        let bry = candidate.position.ry;
        rx >= brx && rx < brx.saturating_add(fw) && ry >= bry && ry < bry.saturating_add(fh)
    })
}

/// Check if a world-space click point falls on a building's foundation cells.
/// The building occupies cells `(rx..rx+fw, ry..ry+fh)`. We convert the click
/// to cell coords and check containment. This matches the original engine which
/// uses the foundation footprint, not the visual sprite bounds.
fn click_hits_foundation(
    world_x: f32,
    world_y: f32,
    entity_rx: u16,
    entity_ry: u16,
    foundation: &str,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
) -> bool {
    let (fw, fh) = crate::rules::foundation::foundation_dimensions(foundation);
    let (click_rx, click_ry) = crate::app::match_runtime::sim_tick::world_point_to_cell(
        world_x,
        world_y,
        height_map,
        bridge_height_map,
    );
    let crx = click_rx as i32;
    let cry = click_ry as i32;
    let brx = entity_rx as i32;
    let bry = entity_ry as i32;
    crx >= brx && crx < brx + fw as i32 && cry >= bry && cry < bry + fh as i32
}

pub(crate) fn pick_entity_at_point(
    entities: &EntityStore,
    encounter_order: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    world_x: f32,
    world_y: f32,
    click_radius: f32,
    rules: Option<&RuleSet>,
    _houses: Option<&HouseStates>,
    height_map: &std::collections::BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&TacticalBridgeInverseMap>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> Option<u64> {
    // Elliptical pick distance: dx² + 0.5*dy² < 200, with the 0.5 Y-weight
    // compensating for isometric projection.
    // The click_radius parameter is kept for API compatibility but not used.
    let _ = click_radius;
    let local_owner_id = local_owner.and_then(|o| interner.and_then(|i| i.get(o)));
    if let (Some(fog), Some(owner_id)) = (fog, local_owner_id) {
        let (rx, ry) = crate::app::match_runtime::sim_tick::world_point_to_cell(
            world_x,
            world_y,
            height_map,
            bridge_height_map,
        );
        if !fog.is_cell_revealed(owner_id, rx, ry) || fog.is_cell_gap_covered(owner_id, rx, ry) {
            return None;
        }
    }
    let mut best: Option<(u64, i32)> = None;

    for &stable_id in encounter_order {
        let Some(entity) = entities.get(stable_id) else {
            continue;
        };
        if !entity.lifecycle.object_alive || entity.lifecycle.in_limbo {
            continue;
        }
        let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
        if entity.category != EntityCategory::Structure
            && !is_drawn_for_local_owner(fog, local_owner, owner_str, entity, local_owner_id)
        {
            continue;
        }
        // `Tactical::PickObjectAtScreenPoint @ 0x006DA380` admits a
        // render-tracked object only when
        //   `InLimbo == 0 && (+0x41A != 0 || CloakState != 2
        //                     || SensorCountForHouse(itsCell, PlayerPtr->ArrayIndex))`.
        // `+0x41A` is the owned-by-the-local-player byte — `TechnoClass::
        // PointerExpired @ 0x007077C0` clears it together with `pOwner` — so
        // being merely allied does NOT exempt a cloaked object. The same filter
        // guards `DisplayClass::DetermineAction @ 0x00692610` and the
        // object-name-under-cursor helper, which is why an unsensed submerged
        // submarine cannot be moused over at all and no explicit attack order
        // against it can be issued.
        if let (Some(fog_state), Some(owner_id)) = (fog, local_owner_id)
            && entity.owner() != owner_id
            && entity
                .cloak
                .as_ref()
                .is_some_and(|cloak| cloak.is_fully_cloaked())
            && !fog_state.has_sensor_for_house(owner_id, entity.position.rx, entity.position.ry)
        {
            continue;
        }
        let (sx, sy) =
            crate::app::presentation::instances::interpolated_screen_position_entity(entity);
        let distance = pick_distance_sq(sx - world_x, sy - world_y) as i32;
        if distance < PICK_DISTANCE_THRESHOLD as i32
            && best.is_none_or(|(_, best_distance)| distance < best_distance)
        {
            best = Some((entity.stable_id(), distance));
        }
    }

    if let Some((stable_id, _)) = best {
        return Some(stable_id);
    }

    // Cell/foundation occupancy is consulted only when the tracked-anchor pass
    // found nothing. Preserve encounter order when a malformed overlap exists.
    entities.values().find_map(|entity| {
        if entity.category != EntityCategory::Structure
            || !entity.lifecycle.object_alive
            || entity.lifecycle.in_limbo
        {
            return None;
        }
        let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
        let foundation = rules
            .and_then(|r| r.object(type_str))
            .map(|o| o.foundation.as_str())
            .unwrap_or("1x1");
        click_hits_foundation(
            world_x,
            world_y,
            entity.position.rx,
            entity.position.ry,
            foundation,
            height_map,
            bridge_height_map,
        )
        .then_some(entity.stable_id())
    })
}

/// Final object admission shared by selection callers. Ownership and the
/// band/TypeSelect dynamic virtual are caller gates: an ordinary discovered
/// nonlocal click may reach this gate, while band and TypeSelect collect only
/// exact-local candidates. Newly selected objects still have to be alive,
/// present, statically selectable, and outside active warp-out.
fn static_selection_gate(
    entity: &crate::sim::game_entity::GameEntity,
    entity_type: &str,
    rules: Option<&RuleSet>,
    require_playfield_membership: bool,
) -> bool {
    entity.lifecycle.object_alive
        && !entity.lifecycle.in_limbo
        && !entity
            .teleport_state
            .as_ref()
            .is_some_and(|teleport| teleport.warp_out_active())
        // `TechnoClass::Select @ 0x006F32D0` requires the stored +0x3D5
        // membership byte. The app supplies the live MapClass authority gate;
        // headless selection fixtures leave it disabled.
        && (!require_playfield_membership || entity.in_playfield)
        && type_is_selectable(entity_type, rules)
}

fn type_is_selectable(entity_type: &str, rules: Option<&RuleSet>) -> bool {
    rules.is_none_or(|r| r.object(entity_type).is_none_or(|obj| obj.selectable))
}

fn is_local_band_candidate(
    local_owner: Option<&str>,
    entity_owner: &str,
    entity: &crate::sim::game_entity::GameEntity,
    entity_type: &str,
    rules: Option<&RuleSet>,
    require_playfield_membership: bool,
) -> bool {
    local_owner.is_none_or(|local| entity_owner.eq_ignore_ascii_case(local))
        && static_selection_gate(entity, entity_type, rules, require_playfield_membership)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;

    fn store_with_structure(alive: bool, in_limbo: bool) -> (EntityStore, StringInterner) {
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let type_ref = interner.intern("NAREFN");
        let mut entities = EntityStore::new();
        let mut building = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health { current: 1000 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.lifecycle.object_alive = alive;
        building.lifecycle.in_limbo = in_limbo;
        entities.insert(building);
        (entities, interner)
    }

    /// A dead building no longer occupies its cell.
    ///
    /// The native own-cell lookup answers with whatever currently occupies the
    /// cell, and a destroyed building has already been taken off the map — so a
    /// miner left standing where a refinery used to be must be band-selectable
    /// again, exactly as it is when it drives clear of a live one.
    #[test]
    fn destroyed_and_limboed_buildings_stop_covering_their_cell() {
        let (entities, interner) = store_with_structure(true, false);
        assert!(building_covers_cell(
            &entities,
            None,
            Some(&interner),
            10,
            10
        ));
        // A cell outside the footprint is never covered.
        assert!(!building_covers_cell(
            &entities,
            None,
            Some(&interner),
            11,
            10
        ));

        let (dead, interner) = store_with_structure(false, false);
        assert!(!building_covers_cell(&dead, None, Some(&interner), 10, 10));

        let (limboed, interner) = store_with_structure(true, true);
        assert!(!building_covers_cell(
            &limboed,
            None,
            Some(&interner),
            10,
            10
        ));
    }

    fn item83_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n\n\
             [VehicleTypes]\n0=AMCV\n1=NOSEL\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n0=GACNST\n\n\
             [AMCV]\nStrength=1000\nArmor=heavy\nSpeed=4\nDeploysInto=GACNST\n\n\
             [GACNST]\nStrength=1000\nArmor=concrete\nUndeploysInto=AMCV\n\n\
             [E1]\nStrength=100\nArmor=none\nSpeed=4\n\n\
             [NOSEL]\nStrength=100\nArmor=light\nSpeed=4\nSelectable=no\n",
        ))
        .expect("item83 rules")
    }

    fn item83_entity(
        id: u64,
        rx: u16,
        ry: u16,
        owner: InternedId,
        type_ref: InternedId,
        category: EntityCategory,
        selected: bool,
    ) -> GameEntity {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            category,
            0,
            5,
            category == EntityCategory::Aircraft,
        );
        entity.lifecycle.object_alive = true;
        entity.lifecycle.in_limbo = false;
        entity.selected = selected;
        entity
    }

    /// `CanBeSelectedNow`'s byte `+0x1C8` is the Robot-Tank-offline latch, not a
    /// deploy-transition flag: its only writers are `TechnoClass__OnDeployBegin`
    /// 0x0070FD62 and `TechnoClass__OnUndeployComplete` 0x0070FC6D/0x0070FC7A,
    /// reached only from the powered-center and robot-tank paths. A GI part-way
    /// through its deploy toggle therefore stays band-selectable in gamemd, and
    /// must here too.
    #[test]
    fn item83_a_deploying_object_is_still_band_selectable() {
        use crate::sim::deploy::DeployPhase;
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let type_ref = interner.intern("GI");
        let mut entity = item83_entity(1, 5, 5, owner, type_ref, EntityCategory::Infantry, false);
        let entities = EntityStore::new();

        assert!(
            can_be_selected_now(&entity, &entities, Some(&rules), Some(&interner)),
            "an idle object is selectable"
        );
        for phase in [
            DeployPhase::Deploying { ticks_remaining: 3 },
            DeployPhase::Undeploying { ticks_remaining: 3 },
        ] {
            entity.deploy_state = Some(phase);
            assert!(
                can_be_selected_now(&entity, &entities, Some(&rules), Some(&interner)),
                "gamemd has no deploy-transition clause here"
            );
        }
    }

    #[test]
    fn item83_type_select_seeds_own_deploy_and_undeploy_then_escalates_same_call() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let gacnst = interner.intern("GACNST");
        let soviet = interner.intern("Soviet");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        entities.insert(item83_entity(
            2,
            11,
            10,
            owner,
            gacnst,
            EntityCategory::Structure,
            true,
        ));
        entities.insert(item83_entity(
            3,
            12,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            4,
            13,
            10,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        entities.insert(item83_entity(
            5,
            30,
            30,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            6,
            14,
            10,
            soviet,
            amcv,
            EntityCategory::Unit,
            false,
        ));

        let screen = [1, 2, 4, 6, 3];
        let map = [1, 2, 3, 4, 5, 6];
        let first = compute_type_select_tap(
            &entities,
            &screen,
            &map,
            &[1, 2],
            None,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
            false,
        );
        assert_eq!(first.outcome, crate::app::types::TypeSelectOutcome::Screen);
        assert_eq!(first.mutation.select, [1, 2, 4, 3]);
        assert!(!first.across_map);

        let second = compute_type_select_tap(
            &entities,
            &screen,
            &map,
            &[1, 2, 4, 3],
            None,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
            false,
        );
        assert_eq!(second.outcome, crate::app::types::TypeSelectOutcome::Map);
        assert_eq!(second.mutation.select, [1, 2, 3, 4, 5]);
        assert!(second.across_map);
    }

    #[test]
    fn item83_empty_outcome_precedes_an_existing_across_map_scope() {
        let entities = EntityStore::new();
        let tap = compute_type_select_tap(
            &entities,
            &[],
            &[],
            &[],
            None,
            Some("Americans"),
            None,
            None,
            true,
        );

        assert_eq!(tap.outcome, crate::app::types::TypeSelectOutcome::Empty);
        assert_eq!(tap.outcome.csf_key(), "MSG:NothingSelected");
    }

    #[test]
    fn item83_visible_warp_out_match_holds_screen_scope_before_final_admission() {
        use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        let mut warp_out = item83_entity(2, 11, 10, owner, amcv, EntityCategory::Unit, false);
        warp_out.teleport_state = Some(TeleportState {
            phase: TeleportPhase::Relocate,
            target_rx: 20,
            target_ry: 20,
            being_warped_ticks: 0,
        });
        entities.insert(warp_out);
        entities.insert(item83_entity(
            3,
            30,
            30,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));

        let tap = compute_type_select_tap(
            &entities,
            &[1, 2],
            &[1, 2, 3],
            &[1],
            None,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
            false,
        );

        assert_eq!(tap.outcome, crate::app::types::TypeSelectOutcome::Screen);
        assert!(!tap.across_map);
        assert_eq!(
            tap.mutation.select,
            [1],
            "warp-out holds screen scope but fails final admission; off-screen stays out"
        );
    }

    #[test]
    fn item83_held_click_uses_exact_type_without_deploy_link_and_deselects_group() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let gacnst = interner.intern("GACNST");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        entities.insert(item83_entity(
            2,
            11,
            10,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        entities.insert(item83_entity(
            3,
            12,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        let scope = [1, 2, 3];

        let replace = compute_type_select_click_mutation(
            &entities,
            &scope,
            &[1, 3],
            1,
            false,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
        );
        assert!(replace.clear);
        assert_eq!(replace.select, [1, 3]);
        assert!(!replace.select.contains(&2));

        let add = compute_type_select_click_mutation(
            &entities,
            &scope,
            &[2],
            1,
            true,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
        );
        assert!(!add.clear);
        assert_eq!(add.select, [1, 3]);

        let deselect = compute_type_select_click_mutation(
            &entities,
            &scope,
            &[1, 3],
            1,
            true,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
        );
        assert_eq!(deselect.deselect, [1, 3]);
    }

    #[test]
    fn item83_held_band_unions_each_represented_exact_type_in_scope() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let gacnst = interner.intern("GACNST");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            2,
            11,
            10,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        entities.insert(item83_entity(
            3,
            30,
            30,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            4,
            31,
            30,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        let (ax, ay) = crate::render::locomotor_visual::screen_position(entities.get(1).unwrap());
        let (bx, by) = crate::render::locomotor_visual::screen_position(entities.get(2).unwrap());
        let mutation = compute_type_select_box_mutation(
            &entities,
            &[1, 2],
            &[1, 2, 3, 4],
            &[],
            None,
            Some("Americans"),
            ax.min(bx) - 1.0,
            ay.min(by) - 1.0,
            ax.max(bx) + 1.0,
            ay.max(by) + 1.0,
            false,
            Some(&rules),
            Some(&interner),
        );
        assert!(mutation.clear);
        assert_eq!(mutation.select, [1, 3, 2, 4]);

        let additive = compute_type_select_box_mutation(
            &entities,
            &[1, 2],
            &[1, 2, 3, 4],
            &[1],
            None,
            Some("Americans"),
            ax.min(bx) - 1.0,
            ay.min(by) - 1.0,
            ax.max(bx) + 1.0,
            ay.max(by) + 1.0,
            true,
            Some(&rules),
            Some(&interner),
        );
        assert!(!additive.clear);
        assert_eq!(additive.select, [3, 2, 4]);
    }

    #[test]
    fn item83_held_band_enemy_anchor_seeds_local_exact_type_batch() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let local_owner = interner.intern("Americans");
        let enemy_owner = interner.intern("Soviet");
        let amcv = interner.intern("AMCV");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            enemy_owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            2,
            30,
            30,
            local_owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        let (x, y) = crate::app::presentation::instances::interpolated_screen_position_entity(
            entities.get(1).unwrap(),
        );

        let mutation = compute_type_select_box_mutation(
            &entities,
            &[1],
            &[1, 2],
            &[],
            None,
            Some("Americans"),
            x - 1.0,
            y - 1.0,
            x + 1.0,
            y + 1.0,
            false,
            Some(&rules),
            Some(&interner),
        );

        assert!(mutation.clear);
        assert_eq!(
            mutation.select,
            [2],
            "enemy anchor contributes only its type; the batch admits only the local match"
        );
    }

    #[test]
    fn item83_picker_uses_global_distance_ties_and_never_clicks_through_rejection() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let unit_type = interner.intern("AMCV");
        let building_type = interner.intern("GACNST");
        let noselect = interner.intern("NOSEL");
        let soviet = interner.intern("Soviet");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            unit_type,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            2,
            10,
            10,
            owner,
            building_type,
            EntityCategory::Structure,
            false,
        ));
        let (x, y) = crate::render::locomotor_visual::screen_position(entities.get(1).unwrap());
        let heights = std::collections::BTreeMap::new();
        assert_eq!(
            pick_entity_at_point(
                &entities,
                &[2, 1],
                None,
                Some("Americans"),
                x,
                y,
                30.0,
                Some(&rules),
                None,
                &heights,
                None,
                Some(&interner),
            ),
            Some(2),
            "an earlier structure wins an equal global tracked-anchor score"
        );

        entities.get_mut(2).unwrap().type_ref = noselect;
        let rejected = compute_click_selection_snapshot(
            &entities,
            &[2, 1],
            &[1],
            None,
            Some("Americans"),
            x,
            y,
            30.0,
            false,
            Some(&rules),
            None,
            &heights,
            None,
            Some(&interner),
        )
        .expect("the chosen target still owns the click");
        assert!(rejected.clear);
        assert!(
            rejected.select.is_empty(),
            "must not retry the valid unit behind it"
        );

        let nonlocal = entities.get_mut(2).unwrap();
        nonlocal.type_ref = building_type;
        nonlocal.owner = soviet;
        let nonlocal_click = compute_click_selection_snapshot(
            &entities,
            &[2, 1],
            &[1],
            None,
            Some("Americans"),
            x,
            y,
            30.0,
            true,
            Some(&rules),
            None,
            &heights,
            None,
            Some(&interner),
        )
        .expect("discovered nonlocal object owns the click");
        // ObjectClass__Select drains the selection whenever the two sides
        // disagree on IsHumanPlayer, so a shift-click on an enemy leaves the
        // enemy alone selected. See the `selection_is_local` comment.
        assert!(
            nonlocal_click.clear,
            "a shift-click on an enemy drains the local group"
        );
        assert_eq!(nonlocal_click.select, [2]);

        // Both sides non-local also drains: equal IsHumanPlayer answers keep the
        // selection only when that answer was true. This is the case the old
        // owner-match predicate got wrong -- it saw two Soviets and added.
        let enemy_group = compute_click_selection_snapshot(
            &entities,
            &[2, 1],
            &[2],
            None,
            Some("Americans"),
            x,
            y,
            30.0,
            true,
            Some(&rules),
            None,
            &heights,
            None,
            Some(&interner),
        )
        .expect("discovered nonlocal object owns the click");
        assert!(
            enemy_group.clear,
            "an enemy group is replaced rather than extended"
        );
    }

    /// The ordinary additive path, with a real local owner supplied — the
    /// existing coverage passes `None` for the owner and so routes around the
    /// `IsHumanPlayer` pair entirely.
    #[test]
    fn item83_shift_click_adds_only_while_both_sides_are_the_local_player() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let soviets = interner.intern("Soviet");
        let unit_type = interner.intern("GI");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            5,
            5,
            americans,
            unit_type,
            EntityCategory::Infantry,
            true,
        ));
        entities.insert(item83_entity(
            2,
            9,
            9,
            americans,
            unit_type,
            EntityCategory::Infantry,
            false,
        ));
        let heights: std::collections::BTreeMap<(u16, u16), u8> = std::collections::BTreeMap::new();
        let (x, y) = {
            let target = entities.get(2).unwrap();
            crate::render::locomotor_visual::screen_position(target)
        };

        // Both local: adds without clearing.
        let added = compute_click_selection_snapshot(
            &entities,
            &[2, 1],
            &[1],
            None,
            Some("Americans"),
            x,
            y,
            30.0,
            true,
            Some(&rules),
            None,
            &heights,
            None,
            Some(&interner),
        )
        .expect("the local object owns the click");
        assert!(!added.clear, "two local objects build one group");
        assert_eq!(added.select, [2]);

        // Flip the clicked object to an enemy: the same gesture now drains.
        entities.get_mut(2).unwrap().owner = soviets;
        let drained = compute_click_selection_snapshot(
            &entities,
            &[2, 1],
            &[1],
            None,
            Some("Americans"),
            x,
            y,
            30.0,
            true,
            Some(&rules),
            None,
            &heights,
            None,
            Some(&interner),
        )
        .expect("the enemy object owns the click");
        assert!(drained.clear, "an enemy joins alone");
        assert_eq!(drained.select, [2]);
    }

    #[test]
    fn item83_two_ordinary_clicks_never_enter_the_type_select_group_path() {
        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        entities.insert(item83_entity(
            2,
            14,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            false,
        ));
        let (x, y) = crate::render::locomotor_visual::screen_position(entities.get(1).unwrap());
        let heights = std::collections::BTreeMap::new();

        for current in [&[][..], &[1][..]] {
            let mutation = compute_click_selection_snapshot(
                &entities,
                &[1, 2],
                current,
                None,
                Some("Americans"),
                x,
                y,
                30.0,
                false,
                Some(&rules),
                None,
                &heights,
                None,
                Some(&interner),
            )
            .expect("ordinary click mutation");
            assert!(mutation.clear);
            assert_eq!(mutation.select, [1]);
            assert!(!mutation.select.contains(&2));
        }
    }

    #[test]
    fn item83_held_deselect_preserves_map_scope_until_short_release_tap() {
        use std::time::{Duration, Instant};
        use winit::keyboard::KeyCode;

        let rules = item83_rules();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let amcv = interner.intern("AMCV");
        let gacnst = interner.intern("GACNST");
        let mut entities = EntityStore::new();
        entities.insert(item83_entity(
            1,
            10,
            10,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        entities.insert(item83_entity(
            2,
            30,
            30,
            owner,
            amcv,
            EntityCategory::Unit,
            true,
        ));
        entities.insert(item83_entity(
            3,
            11,
            10,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        entities.insert(item83_entity(
            4,
            31,
            30,
            owner,
            gacnst,
            EntityCategory::Structure,
            false,
        ));
        let screen = [1, 3];
        let map = [1, 2, 3, 4];

        let mut input = crate::app::types::TypeSelectInputState::default();
        input.finish_tap(crate::app::types::TypeSelectOutcome::Map, true);
        let pressed_at = Instant::now();
        input.press(KeyCode::KeyT, pressed_at, false);

        let deselect = compute_type_select_click_mutation(
            &entities,
            &map,
            &[1, 2],
            1,
            true,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
        );
        assert_eq!(deselect.deselect, [1, 2]);
        input.note_successful_selection_mutation(false);
        assert!(
            input.across_map,
            "held deselect resets SelectionMode but preserves AcrossMap"
        );

        let second_scope = if input.across_map {
            map.as_slice()
        } else {
            screen.as_slice()
        };
        let held = compute_type_select_click_mutation(
            &entities,
            second_scope,
            &[],
            1,
            false,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
        );
        assert_eq!(
            held.select,
            [1, 2],
            "a second held action still consumes the preserved map scope"
        );
        input.note_successful_selection_mutation(false);
        assert!(input.release(KeyCode::KeyT, pressed_at + Duration::from_millis(500)));

        input.prepare_tap_scope();
        assert!(
            !input.across_map,
            "mode reset forces the release tap back to screen"
        );
        let tap = compute_type_select_tap(
            &entities,
            &screen,
            &map,
            &held.select,
            None,
            Some("Americans"),
            Some(&rules),
            Some(&interner),
            input.across_map,
        );
        assert_eq!(tap.outcome, crate::app::types::TypeSelectOutcome::Screen);
        assert_eq!(tap.mutation.select, [1, 3]);
        assert!(
            !tap.mutation.select.contains(&4),
            "off-screen linked type stays out"
        );
    }

    #[test]
    fn techno_playfield_stored_membership_gates_local_mobile_selection() {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.lifecycle.object_alive = true;
        entity.lifecycle.in_limbo = false;
        entity.in_playfield = false;
        assert!(!static_selection_gate(&entity, "MTNK", None, true));
        entity.in_playfield = true;
        assert!(static_selection_gate(&entity, "MTNK", None, true));
        entity.in_playfield = false;
        assert!(
            static_selection_gate(&entity, "MTNK", None, false),
            "headless fixtures without MapClass authority preserve prior admission"
        );
    }
}
