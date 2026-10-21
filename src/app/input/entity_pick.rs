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
use crate::sim::occupancy::CellObjectMember;
use crate::sim::vision::FogState;

/// Selection uses the last prepared presentation sample, not a temporary
/// mutation of authoritative entities. Native callers/tests retain raw geometry.
pub(crate) trait PickSource: Copy {
    fn entities(&self) -> &EntityStore;
    fn screen_position(&self, entity: &crate::sim::game_entity::GameEntity) -> (f32, f32) {
        crate::render::locomotor_visual::screen_position(entity)
    }
    /// The cell's ground-list head the point picker falls back to. A bare
    /// EntityStore has no cell lists.
    fn first_ground_cell_object(&self, _cell: (u16, u16)) -> Option<CellObjectMember> {
        None
    }
}
impl PickSource for &EntityStore {
    fn entities(&self) -> &EntityStore {
        self
    }
}

pub(crate) trait PickWorld: PickSource {
    fn simulation(&self) -> &crate::sim::world::Simulation;
}
impl PickSource for &crate::sim::world::Simulation {
    fn entities(&self) -> &EntityStore {
        crate::sim::world::Simulation::entities(self)
    }
    fn first_ground_cell_object(&self, cell: (u16, u16)) -> Option<CellObjectMember> {
        crate::sim::world::Simulation::first_ground_cell_object(self, cell)
    }
}
impl PickWorld for &crate::sim::world::Simulation {
    fn simulation(&self) -> &crate::sim::world::Simulation {
        self
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PresentedWorld<'a> {
    pub sim: &'a crate::sim::world::Simulation,
    pub interpolation: &'a crate::app::presentation::interpolation::RenderInterpolation,
}
impl PickSource for PresentedWorld<'_> {
    fn entities(&self) -> &EntityStore {
        self.sim.entities()
    }
    fn screen_position(&self, entity: &crate::sim::game_entity::GameEntity) -> (f32, f32) {
        let pose = self
            .interpolation
            .sample_entity(entity, self.sim.session().binary_frame());
        (pose.screen[0], pose.screen[1] - pose.visual_lift)
    }
    fn first_ground_cell_object(&self, cell: (u16, u16)) -> Option<CellObjectMember> {
        self.sim.first_ground_cell_object(cell)
    }
}
impl PickWorld for PresentedWorld<'_> {
    fn simulation(&self) -> &crate::sim::world::Simulation {
        self.sim
    }
}
pub(crate) fn presented<'a>(
    state: &'a crate::app::AppState,
    sim: &'a crate::sim::world::Simulation,
) -> PresentedWorld<'a> {
    PresentedWorld {
        sim,
        interpolation: &state.match_state.match_presentation.interpolation,
    }
}

/// Elliptical distance threshold for picking objects at a screen point.
/// `dx*dx + 0.5*dy*dy < 200` with 0.5 Y-weight compensating for isometric projection.
/// Max horizontal reach: sqrt(200) ≈ 14px, max vertical reach: sqrt(400) = 20px.
const PICK_DISTANCE_THRESHOLD: f32 = 200.0;
/// The simulation's house table, as the picker needs it: owner → player state.
type HouseStates = crate::sim::houses::Houses;

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

/// The object under the cursor that DetermineAction sees, for the cursor and
/// the click. ProcessClickCoords692515 calls TacticalPick6DA380 once before
/// passing that same identity to DetermineAction and LeftUp693271/69328B:
/// a Techno classified for the local player, or a Terrain object.
pub(crate) fn hover_object_at_point(
    source: impl PickWorld,
    world_x: f32,
    world_y: f32,
    local_owner: &str,
    ignore_visibility: bool,
) -> Option<HoverObject> {
    let sim = source.simulation();
    let fog = (!ignore_visibility).then_some(sim.fog());
    let stable_id = match pick_object_at_point(
        source,
        sim.display_layers().ordered_ids(),
        fog,
        sim.interner().get(local_owner),
        world_x,
        world_y,
        sim.resolved_terrain(),
    )? {
        CellObjectMember::Entity(id) => id,
        CellObjectMember::Terrain(id) => return Some(HoverObject::Terrain(id)),
    };
    hover_object_by_id(sim, stable_id, local_owner)
}

/// Classify an identity already picked by a retained input source (radar or
/// tactical). Do not perform another geometric pick at its coordinates.
pub(crate) fn hover_object_by_id(
    sim: &crate::sim::world::Simulation,
    stable_id: u64,
    local_owner: &str,
) -> Option<HoverObject> {
    let entity = sim.entities().get(stable_id)?;
    let is_structure = entity.category() == EntityCategory::Structure;
    // What_Action asks the actor's house, here the local player's
    // (HouseClass::Is_Ally_ByObject4F9A90 at 7003C4).
    let is_friendly = crate::map::houses::is_allied_with(
        sim.house_alliances(),
        local_owner,
        sim.interner().resolve(entity.owner()),
    );
    let kind = match (is_friendly, is_structure) {
        (true, true) => HoverTargetKind::FriendlyStructure,
        (true, false) => HoverTargetKind::FriendlyUnit,
        (false, true) => HoverTargetKind::EnemyStructure,
        (false, false) => HoverTargetKind::EnemyUnit,
    };
    Some(HoverObject::Techno(HoverTargetKindWithId {
        kind,
        stable_id,
    }))
}

/// The Techno [`hover_object_at_point`] finds, for consumers that answer a
/// Terrain object under the cursor as its cell.
pub(crate) fn hover_target_at_point(
    source: impl PickWorld,
    world_x: f32,
    world_y: f32,
    local_owner: &str,
    ignore_visibility: bool,
) -> Option<HoverTargetKindWithId> {
    hover_object_at_point(source, world_x, world_y, local_owner, ignore_visibility)?.techno()
}

/// What [`hover_object_at_point`] found under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoverObject {
    Techno(HoverTargetKindWithId),
    /// A Terrain object, reached only through the clicked cell's list.
    Terrain(u64),
}

impl HoverObject {
    pub(crate) fn techno(self) -> Option<HoverTargetKindWithId> {
        match self {
            Self::Techno(target) => Some(target),
            Self::Terrain(_) => None,
        }
    }

    /// The object's id in the shared runtime-object namespace.
    pub(crate) fn id(self) -> u64 {
        match self {
            Self::Techno(target) => target.stable_id,
            Self::Terrain(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HoverTargetKindWithId {
    pub(crate) kind: HoverTargetKind,
    pub(crate) stable_id: u64,
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
    additive: bool,
    rules: Option<&RuleSet>,
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
        additive,
        rules,
        interner,
        false,
        None,
    )
}

pub(crate) fn compute_click_selection_snapshot_with_playfield(
    source: impl PickSource,
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    world_x: f32,
    world_y: f32,
    additive: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    require_playfield_membership: bool,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<SelectionMutation> {
    let entities = source.entities();
    let Some(picked_sid) = pick_entity_at_point(
        source,
        encounter_order,
        fog,
        local_owner,
        world_x,
        world_y,
        interner,
        terrain,
    ) else {
        // Non-shift click clears selection; shift-click with no hit keeps it.
        return if additive || current_selection.is_empty() {
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
    // With no selection, DisplayDetermineAction69261B ->692762..692778
    // refuses ACTION_SELECT for retained Techno+3D4. A nonempty selection
    // delegates to WhatActionOnObject; its source/target and modifier gates
    // differ. Don't apply this leaf to that path or forced/type selection.
    let admitted = (!current_selection.is_empty() || !picked.is_mission_only())
        && static_selection_gate(picked, type_str, rules, require_playfield_membership);
    // A shift-click adds when the selection it is joining is the local player's,
    // whatever was clicked. The native toggle-select arm (action 8,
    // `cmp ebx, 8` at 0x004ABD24; the ordinary ACTION_SELECT arm at
    // 0x004ABDC5..0x004ABE4A is the one that clears first) asks
    // `HouseClass__IsHumanPlayer(CurrentObjects[0]->Owner)` — 0x004ABD42 loads
    // `CurrentObjects[0]`, calls its owner virtual `+0x3C`, and 0x004ABD50 calls
    // 0x0050B6F0, whose real test is `house == g_PlayerPtr`; a false answer at
    // 0x004ABD57 jumps to the replace path. It is a test on what is ALREADY
    // selected, never on whether the clicked object matches it.
    //
    // Enemy clicks genuinely reach this arm. `TechnoClass__What_Action_OnObject`
    // 0x006FFEC0 short-circuits to toggle-select (`mov eax, 8` at 0x00700107)
    // whenever the
    // Shift pair `DAT_00A8EC08`/`0C` is down and the Ctrl pair is not — gated on
    // `IsHumanPlayer` of the SELECTED object's owner `[ESI+0x21C]`, with no test
    // on the clicked object at all.
    //
    // That first test is only half the decision. The arm's add branch calls the
    // clicked object's vtable `+0x14C` Select (`TechnoClass__Select`
    // 0x006FBFA0, 0x004ABDBA), whose `ObjectClass__Select` 0x005F4520 runs
    // `Unselect_All_With_Mode_Reset` 0x0048DC90 (0x005F45AF..0x005F45F8) when
    // the new object is not the player's. The selection ledger owns that Select
    // (`resolve_selection_mutation`): shift-clicking an enemy while your own
    // army is selected drains the army and leaves the enemy alone selected,
    // and an enemy that Select refuses leaves the army selected.
    // Residual: a pick this gate refuses answers no snapshot, so it starts no
    // action-line timer; native starts it after the toggle arm (0x004ABFAE).
    // Visible only as a missing line flash on such a refused Shift-click.
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
        .is_some_and(owner_is_local);

    if !additive || current_selection.is_empty() || !selection_is_local {
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
    source: impl PickSource,
    encounter_order: &[u64],
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
    local_owner: Option<&str>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    let entities = source.entities();
    let local_owner_id = local_owner.and_then(|o| interner.and_then(|i| i.get(o)));
    encounter_order.iter().any(|id| {
        let Some(entity) = entities.get(*id) else {
            return false;
        };
        if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
            return false;
        }
        let (sx, sy) = source.screen_position(entity);
        if !(sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y) {
            return false;
        }
        if entity.category() == EntityCategory::Structure {
            // Bulk-registered, shroud or no shroud.
            return true;
        }
        let owner_str = interner.map_or("", |i| i.resolve(entity.owner()));
        is_drawn_for_local_owner(fog, local_owner, owner_str, entity, local_owner_id)
    })
}

/// Whether `Tactical::PickObjectAtScreenPoint @ 0x006DA380` considers a
/// Techno under the cursor: alive and out of limbo, and, unless the player
/// owns it (`+0x41A`), not cloaked (CloakState 2) or on a cell the player's
/// sensors cover. `+0x41A` is the owned-by-the-local-player byte
/// (`TechnoClass::PointerExpired @ 0x007077C0` clears it with `pOwner`), so
/// an ally's cloaked object is not exempt. `DisplayClass::ProcessClickCoords`
/// asks the cloak test again of the object it returns (`0x00692529..0x0069257F`),
/// and that one object is what both DetermineAction and BandBox_LeftUp get
/// (`ScrollClass::HandleMouseMessage`, `0x00693271` → `0x0069328B`). So an
/// unsensed submerged submarine is no object to hover, click or select.
///
/// Without fog (sandbox full visibility) or a known player, only the first
/// part applies. ProcessClickCoords also drops its chosen nonlocal object's
/// TypeInvisible (`+0xC9A`, `0x0069256B..0x0069257F`) after the scan; that
/// producer gate remains open. Building's InvisibleInGame can derive this byte
/// too. Display's tooltip owns its separately repeated postwinner gate.
pub(crate) fn pick_admits_object(
    entity: &crate::sim::game_entity::GameEntity,
    fog: Option<&FogState>,
    player: Option<InternedId>,
) -> bool {
    if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
        return false;
    }
    let (Some(fog), Some(player)) = (fog, player) else {
        return true;
    };
    //6DA3FF reads retained Techno+41A. Its owner classifies this at
    // InitManagers6F3F43 and ChangeOwner70173B; save/load preserves the byte.
    entity.discovery.owned_by_current_house()
        || !entity
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.is_fully_cloaked())
        || fog.has_sensor_for_house(player, entity.position.rx(), entity.position.ry())
}

/// Would the local player see this object drawn right now?
///
/// Deliberately distinct from the eligibility gate's visibility tail: with no
/// fog state at all (the sandbox full-visibility switch) everything is drawn,
/// where the eligibility gate falls back to an owner test.
fn is_drawn_for_local_owner(
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
    local_owner: Option<&str>,
    entity_owner: &str,
    entity: &crate::sim::game_entity::GameEntity,
    local_owner_id: Option<InternedId>,
) -> bool {
    let Some(local_owner) = local_owner else {
        return true;
    };
    let Some((fog, alliances)) = fog else {
        // Sandbox full visibility: everything is drawn.
        return true;
    };
    // VERA's policy, with no native ally test: the local player's allies are
    // drawn, as the local player's house counts them.
    if crate::map::houses::is_allied_with(alliances, local_owner, entity_owner) {
        return true;
    }
    let owner_id = local_owner_id.unwrap_or_default();
    let pos = &entity.position;
    fog.is_cell_revealed(owner_id, pos.rx(), pos.ry())
        && !fog.is_cell_gap_covered(owner_id, pos.rx(), pos.ry())
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
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
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
    source: impl PickSource,
    preflight_order: &[u64],
    encounter_order: &[u64],
    current_selection: &[u64],
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
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
        source,
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
        source,
        encounter_order,
        fog.map(|(fog, _)| fog),
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

pub(crate) fn compute_type_select_tap_with_playfield(
    entities: &EntityStore,
    screen_order: &[u64],
    map_order: &[u64],
    current_selection: &[u64],
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
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
            if let Some(deploys_into) = object.deploys_into.as_deref()
                && let Some(type_ref) = interner.and_then(|i| i.get(deploys_into))
            {
                push_exact_seed(&mut seeds, type_ref);
            }
            if let Some(undeploys_into) = object.undeploys_into.as_deref()
                && let Some(type_ref) = interner.and_then(|i| i.get(undeploys_into))
            {
                push_exact_seed(&mut seeds, type_ref);
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
    fog: Option<(&FogState, &crate::map::houses::HouseAllianceMap)>,
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
            if !entity.lifecycle.object_alive()
                || local_owner.is_some_and(|local| !owner.eq_ignore_ascii_case(local))
                || !seeds.contains(&entity.type_ref())
                || (screen_only
                    && entity.category() != EntityCategory::Structure
                    && !is_drawn_for_local_owner(fog, local_owner, owner, entity, local_owner_id))
            {
                return None;
            }
            let dynamic = selection_dynamic_prefix(entity, entities, rules, interner)
                && type_is_selectable(type_id, rules)
                && (!require_playfield_membership || entity.in_playfield());
            let undeploying_building_fallback = entity.category() == EntityCategory::Structure
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

pub(crate) fn compute_type_select_box_mutation_with_playfield(
    source: impl PickSource,
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
    let entities = source.entities();
    let mut represented_types = Vec::new();
    for &id in screen_order {
        let Some(entity) = entities.get(id) else {
            continue;
        };
        let (sx, sy) = source.screen_position(entity);
        if !entity.lifecycle.object_alive() || sx < min_x || sx > max_x || sy < min_y || sy > max_y
        {
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
            if !entity.lifecycle.object_alive()
                || entity.lifecycle.in_limbo()
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
    source: impl PickSource,
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
    let entities = source.entities();
    encounter_order
        .iter()
        .filter_map(|entity| {
            let entity = entities.get(*entity)?;
            // Rectangle first, the way the native walk does it: the per-object
            // filter only runs for objects the box actually covers.
            let (sx, sy) = source.screen_position(entity);
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
            if !selection_dynamic_prefix(entity, entities, rules, interner) {
                return None;
            }
            Some(entity.stable_id())
        })
        .collect()
}

/// Shared prefix of Techno6FC030, used by band, TypeSelect and N/M.
/// Its final virtual+138 call belongs to `Simulation::object_is_selectable`.
/// N/M invokes that owner during its candidate walk; every selection mutation
/// invokes it at final admission. The older band/type candidate builders lack
/// world context for the disguise branch and retain that candidate-tail gap
/// until their held migration resumes.
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
/// The enslaved test reads the slave-owner pointer (`TechnoClass+0x2DC`,
/// `SlaveLink::owner`), which a freed slave no longer carries; a
/// mind-controlled unit is a different field in the original and is
/// deliberately not tested here.
///
/// One native clause is approximated rather than reproduced, because VERA has
/// no matching state: the native bunker test reads one link field. VERA splits
/// the link into an approach marker and an installed link, and only the
/// installed link is treated as linked.
///
/// The powered-down clause reads byte `+0x1C8` (`0x006FC041`), and it is NOT a
/// deploy-transition flag. An exhaustive instruction search finds exactly three
/// writers — the `TechnoClass` constructor (`0x006F2C9A`), Deactivate
/// (`0x0070FD62`, Ghidra's `OnDeployBegin`) and Reactivate
/// (`0x0070FC6D`/`0x0070FC7A`, Ghidra's `OnUndeployComplete`), reached only from
/// `HouseClass::LostPoweredCenter`, `Removed_From_Game`,
/// `UnitClass::Per_Cell_Process` and `RobotTanksBackOnline`. So it is a *latch*
/// held for as long as a Robot Tank is offline for want of a Robot Control
/// Center (`GameEntity::deactivated`, owned by `sim::powered_unit`), never
/// raised by an infantry deploy toggle or an ordinary MCV deploy: a GI in its
/// deploy animation stays band-selectable.
pub(crate) fn selection_dynamic_prefix(
    entity: &crate::sim::game_entity::GameEntity,
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
) -> bool {
    // BuildingClass answers false on the ordinary dynamic selection virtual.
    // TypeSelect owns the verified UndeploysInto fallback separately.
    if entity.category() == EntityCategory::Structure {
        return false;
    }
    if entity.slave.owner().is_some() || entity.deactivated.is_offline() {
        return false;
    }
    if entity.bunker.link().installed_in().is_some() {
        return false;
    }
    if entity.tether.partner().is_some()
        && building_covers_cell(
            entities,
            rules,
            interner,
            entity.position.rx(),
            entity.position.ry(),
        )
    {
        return false;
    }
    true
}

/// Techno virtual+64, original6F32D0. Discovery is retained +41B, not the
/// current fog tile or viewport. House50B6F0 owns campaign/current-house policy.
/// This gate does not read Alive, Foot+6AD or Techno+270; later virtuals do.
pub(crate) fn is_local_player_selectable_object(
    sim: &crate::sim::world::Simulation,
    rules: Option<&RuleSet>,
    entity: &crate::sim::game_entity::GameEntity,
) -> bool {
    entity.health.current() > 0
        && !entity.lifecycle.in_limbo()
        && entity.in_playfield()
        && entity.category() != EntityCategory::Structure
        && entity.discovery.discovered_by_current_house()
        && type_is_selectable(sim.interner().resolve(entity.type_ref()), rules)
        && sim.house_is_human_player(entity.owner())
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
        if candidate.category() != EntityCategory::Structure
            || candidate.lifecycle.in_limbo()
            || !candidate.lifecycle.object_alive()
        {
            return false;
        }
        let type_str = interner.map_or("", |i| i.resolve(candidate.type_ref()));
        let foundation = rules
            .and_then(|r| r.object(type_str))
            .map(|o| o.foundation.as_str())
            .unwrap_or("1x1");
        let (fw, fh) = crate::rules::foundation::foundation_dimensions(foundation);
        let brx = candidate.position.rx();
        let bry = candidate.position.ry();
        rx >= brx && rx < brx.saturating_add(fw) && ry >= bry && ry < bry.saturating_add(fh)
    })
}

/// The entity [`pick_object_at_point`] finds; a Terrain object counts as no
/// entity.
pub(crate) fn pick_entity_at_point<'a>(
    source: impl PickSource,
    encounter_order: impl IntoIterator<Item = &'a u64>,
    fog: Option<&FogState>,
    local_owner: Option<&str>,
    world_x: f32,
    world_y: f32,
    interner: Option<&crate::sim::intern::StringInterner>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<u64> {
    let player = local_owner.and_then(|o| interner.and_then(|i| i.get(o)));
    match pick_object_at_point(
        source,
        encounter_order,
        fog,
        player,
        world_x,
        world_y,
        terrain,
    )? {
        CellObjectMember::Entity(id) => Some(id),
        CellObjectMember::Terrain(_) => None,
    }
}

/// `Tactical::PickObjectAtScreenPoint @ 0x006DA380`, with the cloak test
/// ProcessClickCoords repeats on its result (`0x00692529..0x0069257F`).
///
/// A shrouded clicked cell picks nothing. Otherwise the nearest admitted
/// registered object wins on `ftol(dx² + 0.5·dy²) < 200`, the first in
/// `encounter_order` on a tie. With none, the pick is the head of the clicked
/// cell's ground list (`CellClass+0xE4`, `0x006DA4B8..0x006DA4FB`): a
/// building over its whole foundation, a unit standing beyond the ellipse, or
/// a Terrain object. The bridge-deck list (`+0xE8`) is never read. An
/// unadmitted head picks nothing; the pick never looks behind it.
///
/// Residual: the anchor pass scores only entities. Native also scores a
/// registered Terrain object whose type has `IsVeinhole=` (`+0x2B4`);
/// VERA reaches one only through its cell.
pub(crate) fn pick_object_at_point<'a>(
    source: impl PickSource,
    encounter_order: impl IntoIterator<Item = &'a u64>,
    fog: Option<&FogState>,
    player: Option<InternedId>,
    world_x: f32,
    world_y: f32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<CellObjectMember> {
    let entities = source.entities();
    let cells = terrain.map(|terrain| terrain as &dyn crate::map::terrain::TacticalCellLookup);
    let click_cell =
        crate::app::match_runtime::sim_tick::world_point_to_cell(world_x, world_y, cells);
    if let (Some(fog), Some(player)) = (fog, player) {
        let (rx, ry) = click_cell;
        if crate::app::presentation::instances::cell_visibility_for_local_owner(
            Some(player),
            Some(fog),
            rx,
            ry,
            false,
            terrain,
        ) == crate::app::presentation::instances::CellVisibilityState::Shrouded
        {
            return None;
        }
    }
    let mut best: Option<(u64, i32)> = None;

    for &stable_id in encounter_order {
        let Some(entity) = entities.get(stable_id) else {
            continue;
        };
        if !pick_admits_object(entity, fog, player) {
            continue;
        }
        // Native6DA380 tests lifecycle/cloak after the clicked Cell gate;
        // it does not repeat a raw anchor-cell shroud or discovery test.
        let (sx, sy) = source.screen_position(entity);
        let distance = pick_distance_sq(sx - world_x, sy - world_y) as i32;
        if distance < PICK_DISTANCE_THRESHOLD as i32
            && best.is_none_or(|(_, best_distance)| distance < best_distance)
        {
            best = Some((entity.stable_id(), distance));
        }
    }

    if let Some((stable_id, _)) = best {
        return Some(CellObjectMember::Entity(stable_id));
    }
    match source.first_ground_cell_object(click_cell)? {
        CellObjectMember::Entity(id) => entities
            .get(id)
            .filter(|entity| pick_admits_object(entity, fog, player))
            .map(|_| CellObjectMember::Entity(id)),
        terrain => Some(terrain),
    }
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
    entity.lifecycle.object_alive()
        // `0x006F32D3`: no Health — a crashing aircraft falls alive at 0.
        && entity.health.current() > 0
        && !entity.lifecycle.in_limbo()
        && !entity.is_warped_out()
        // The mouse/type caller's aggregate admission includes +0x3D5.
        // This helper is not the complete virtual+64; see its owner above.
        // The app supplies the live MapClass authority gate;
        // headless selection fixtures leave it disabled.
        && (!require_playfield_membership || entity.in_playfield())
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
#[path = "entity_pick_reveal_tests.rs"]
mod reveal_tests;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;

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
            Health::new(100),
            type_ref,
            category,
            0,
            5,
            category == EntityCategory::Aircraft,
        );
        entity.lifecycle.set_object_alive_for_test(true);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.selection.set_selected_for_test(selected);
        entity
    }

    #[test]
    fn hover_and_click_match_original_weighted_pick_and_encounter_ties() {
        use crate::sim::world::Simulation;

        // These points are declared native registration inputs, not a second
        // projection implementation. The ordinary retail scene is exercised
        // separately by the bunker entry regression.
        #[derive(Clone, Copy)]
        struct Registrations<'a> {
            sim: &'a Simulation,
            points: [(f32, f32); 2],
        }
        impl PickSource for Registrations<'_> {
            fn entities(&self) -> &EntityStore {
                self.sim.entities()
            }
            fn screen_position(&self, entity: &GameEntity) -> (f32, f32) {
                self.points[entity.stable_id() as usize - 1]
            }
        }
        impl PickWorld for Registrations<'_> {
            fn simulation(&self) -> &Simulation {
                self.sim
            }
        }

        let corpus = crate::test_fixture::json("tools/spatial_oracle/pointer_pick.json");
        for row in corpus["controls"].as_array().unwrap() {
            let mut sim = Simulation::new();
            let viewer = sim.intern("viewer");
            let typ = sim.intern("INIT");
            for id in [1, 2] {
                sim.entities_mut_for_test().insert(item83_entity(
                    id,
                    10,
                    10,
                    viewer,
                    typ,
                    EntityCategory::Infantry,
                    false,
                ));
            }
            let order: Vec<_> = row["registration_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() + 1)
                .collect();
            for &id in &order {
                sim.resubmit_entity_display_for_test(id);
            }
            let point = |value: &serde_json::Value| {
                (
                    value[0].as_f64().unwrap() as f32,
                    value[1].as_f64().unwrap() as f32,
                )
            };
            let source = Registrations {
                sim: &sim,
                points: [point(&row["positions"][0]), point(&row["positions"][1])],
            };
            let (x, y) = point(&row["cursor"]);
            let expected = Some(row["picked"].as_u64().unwrap() + 1);
            assert_eq!(
                pick_entity_at_point(
                    source,
                    &order,
                    None,
                    Some("viewer"),
                    x,
                    y,
                    Some(sim.interner()),
                    None,
                ),
                expected,
                "click: {}",
                row["name"]
            );
            assert_eq!(
                hover_target_at_point(source, x, y, "viewer", true).map(|hover| hover.stable_id),
                expected,
                "hover: {}",
                row["name"]
            );
        }
    }

    #[test]
    fn cell_fallback_keeps_the_post_pick_cloak_gate() {
        use crate::sim::cloak_disguise::CloakRuntime;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let viewer = sim.intern("viewer");
        let other = sim.intern("other");
        let typ = sim.intern("GAPOWR");
        *sim.fog_mut_for_test() = FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(viewer);
        let mut building = item83_entity(1, 10, 10, other, typ, EntityCategory::Structure, false);
        building.cloak = Some(CloakRuntime::new(0));
        let (x, y) = crate::render::locomotor_visual::screen_position(&building);
        sim.entities_mut_for_test().insert(building);
        sim.occupancy_mut().add(
            10,
            10,
            1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        sim.resubmit_entity_display_for_test(1);
        // Outside the tracked-anchor ellipse but inside this foundation.
        // ProcessClickCoords692529..69257F repeats the cloak gate after the
        // chosen fallback; sharing the picker must preserve hover's refusal.
        for (cloak_state, expected) in [(0, Some(1)), (2, None)] {
            sim.entities_mut_for_test()
                .get_mut(1)
                .unwrap()
                .cloak
                .as_mut()
                .unwrap()
                .set_state_for_test(cloak_state);
            assert_eq!(
                pick_entity_at_point(
                    &sim,
                    &[1],
                    Some(sim.fog()),
                    Some("viewer"),
                    x + 20.,
                    y,
                    Some(sim.interner()),
                    None,
                ),
                expected
            );
            assert_eq!(
                hover_target_at_point(&sim, x + 20., y, "viewer", false)
                    .map(|hover| hover.stable_id),
                expected
            );
        }
    }

    /// With no registered object near the point, the pick is the head of the
    /// clicked cell's ground list (`0x006DA4B8..0x006DA4FB`): a unit standing
    /// beyond the ellipse, ahead of the building whose foundation it stands
    /// on (AddContent `0x0047E8A0` prepends it), or a Terrain object, which
    /// only the object pick returns.
    #[test]
    fn the_point_pick_falls_back_to_the_clicked_cells_ground_list_head() {
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::CellListInsertion;
        use crate::sim::terrain_object::TerrainObjectState;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let viewer = sim.intern("viewer");
        let typ = sim.intern("HTNK");
        let building_type = sim.intern("GAPOWR");
        let unit = item83_entity(1, 10, 10, viewer, typ, EntityCategory::Unit, false);
        let (x, y) = crate::render::locomotor_visual::screen_position(&unit);
        sim.entities_mut_for_test().insert(unit);
        sim.entities_mut_for_test().insert(item83_entity(
            2,
            9,
            9,
            viewer,
            building_type,
            EntityCategory::Structure,
            false,
        ));
        for (id, insertion) in [
            (2, CellListInsertion::AppendBuilding),
            (1, CellListInsertion::PrependNonBuilding),
        ] {
            sim.occupancy_mut()
                .add(10, 10, id, MovementLayer::Ground, None, insertion);
        }
        sim.resubmit_entity_display_for_test(1);
        // dx = 20 scores 400, outside the anchor ellipse's 200.
        let (x, y) = (x + 20., y);
        assert_eq!(
            crate::app::match_runtime::sim_tick::world_point_to_cell(x, y, None),
            (10, 10)
        );
        let pick = |sim: &Simulation, x, y| {
            pick_entity_at_point(
                sim,
                &[2, 1],
                None,
                Some("viewer"),
                x,
                y,
                Some(sim.interner()),
                None,
            )
        };
        assert_eq!(pick(&sim, x, y), Some(1));
        assert_eq!(
            hover_target_at_point(&sim, x, y, "viewer", true).map(|hover| hover.stable_id),
            Some(1)
        );

        let tree_type = sim.intern("TREE01");
        let terrain = sim.terrain_objects_mut_for_test();
        terrain.cell_index_mut_for_test().insert((12, 12), 50);
        terrain
            .objects_mut_for_test()
            .insert(50, TerrainObjectState::for_test(50, tree_type, 12, 12));
        let half = crate::util::fixed_math::SimFixed::from_num(128);
        let (x, y) = crate::util::lepton::lepton_to_screen(12, 12, half, half, 0);
        assert_eq!(
            hover_object_at_point(&sim, x, y, "viewer", true),
            Some(HoverObject::Terrain(50))
        );
        assert_eq!(hover_target_at_point(&sim, x, y, "viewer", true), None);
        assert_eq!(pick(&sim, x, y), None);
    }

    #[test]
    fn both_pointer_queries_use_retained_local_ownership_for_cloak_admission() {
        use crate::sim::cloak_disguise::CloakRuntime;
        use crate::sim::world::Simulation;

        //6DA3FF and69252D read retained Techno+41A, independently of the
        // current owner pointer. These explicit retained-byte controls cover
        // the reader, not a claim that each prior has a natural producer.
        for (same_owner, retained_owned, sensed, cloak_state, admitted) in [
            (false, true, false, 2, true),
            (true, false, false, 2, false),
            (false, false, true, 2, true),
            (false, false, false, 2, false),
            (false, false, false, 0, true),
        ] {
            let mut sim = Simulation::new();
            let viewer = sim.intern("viewer");
            let other = sim.intern("other");
            let typ = sim.intern("HTNK");
            sim.session_mut_for_test()
                .set_current_house_for_test(Some(viewer));
            *sim.fog_mut_for_test() = FogState::new(32, 32);
            sim.fog_mut_for_test()
                .reveal_cells_for_owner(viewer, [(10, 10)]);
            if sensed {
                sim.fog_mut_for_test().increment_sensor_at(viewer, 10, 10);
            }
            let owner = if same_owner { viewer } else { other };
            let mut entity = item83_entity(1, 10, 10, owner, typ, EntityCategory::Unit, false);
            entity
                .discovery
                .set_owned_by_current_house_for_test(retained_owned);
            let mut cloak = CloakRuntime::new(0);
            cloak.set_state_for_test(cloak_state);
            entity.cloak = Some(cloak);
            let world =
                crate::app::presentation::instances::interpolated_screen_position_entity(&entity);
            sim.entities_mut_for_test().insert(entity);
            sim.resubmit_entity_display_for_test(1);
            let order = crate::app::presentation::instances::tactical_entity_encounter_order(&sim);

            let expected = admitted.then_some(1);
            assert_eq!(
                pick_entity_at_point(
                    sim.entities(),
                    &order,
                    Some(sim.fog()),
                    Some("viewer"),
                    world.0,
                    world.1,
                    Some(sim.interner()),
                    None,
                ),
                expected,
                "pointer: owner equality{same_owner}, retained{retained_owned}, sensor{sensed}, cloak{cloak_state}"
            );
            assert_eq!(
                hover_target_at_point(&sim, world.0, world.1, "viewer", false)
                    .map(|target| target.stable_id),
                expected,
                "hover: owner equality{same_owner}, retained{retained_owned}, sensor{sensed}, cloak{cloak_state}"
            );
        }
    }

    #[test]
    fn mission_only_empty_selection_click_matches_native_flag_gate() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/aircraft_mission_only.json",
        ))
        .unwrap();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let type_ref = interner.intern("PLANE");
        for row in rows {
            let mut entities = EntityStore::new();
            let mut plane =
                item83_entity(1, 10, 10, owner, type_ref, EntityCategory::Aircraft, false);
            if row["mission_only"].as_bool().unwrap() {
                plane.mission_only.mark();
            }
            let (x, y) = crate::render::locomotor_visual::screen_position(&plane);
            entities.insert(plane);
            let result = compute_click_selection_snapshot(
                &entities,
                &[1],
                &[],
                None,
                Some("Americans"),
                x,
                y,
                false,
                None,
                Some(&interner),
            )
            .unwrap();
            assert_eq!(!result.select.is_empty(), row["click_action"] == 7, "{row}");
        }
    }

    #[test]
    fn native_bunker_install_head_allows_tank_click_and_force_attack() {
        use crate::sim::command::Command;
        use crate::sim::docking::bunker_link::BunkerLink;
        use crate::sim::world::Simulation;
        use crate::util::fixed_math::SimFixed;

        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_force_track.bunker_install.json",
        ))
        .unwrap();
        let row = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "same_raw_cell")
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let tank_type = sim.intern("LTNK");
        let bunker_type = sim.intern("NATBNK");
        let mut at = |id, type_ref, category, coords: &serde_json::Value| {
            let xyz: [i32; 3] = std::array::from_fn(|i| coords[i].as_i64().unwrap() as i32);
            let mut entity = item83_entity(
                id,
                (xyz[0] / 256) as u16,
                (xyz[1] / 256) as u16,
                owner,
                type_ref,
                category,
                false,
            );
            entity
                .position
                .set_sub_x_for_test(SimFixed::from_num(xyz[0] % 256));
            entity
                .position
                .set_sub_y_for_test(SimFixed::from_num(xyz[1] % 256));
            entity.position.set_exact_z_leptons_for_test(Some(xyz[2]));
            entity.set_in_playfield(true);
            sim.entities_mut_for_test().insert(entity);
        };
        at(
            2,
            bunker_type,
            EntityCategory::Structure,
            &row["input"]["building_location"],
        );
        at(
            1,
            tank_type,
            EntityCategory::Unit,
            &row["output"]["force_coordinate"],
        );
        at(
            3,
            tank_type,
            EntityCategory::Unit,
            &serde_json::json!([4352, 2816, 312]),
        );
        sim.entities_mut_for_test()
            .get_mut(1)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(2));
        sim.entities_mut_for_test()
            .get_mut(2)
            .unwrap()
            .bunker
            .set_bunker_occupant_for_test(Some(1));

        let pick = |sim: &Simulation, point: (f32, f32)| {
            pick_entity_at_point(
                sim.entities(),
                &[2, 1],
                None,
                Some("Americans"),
                point.0,
                point.1,
                Some(sim.interner()),
                sim.resolved_terrain(),
            )
        };
        // Original6D461A registers buildings before mobile drawing6D465F;
        // 6DA47C keeps the first equal score. Preserve that existing policy.
        // The native ForceTrack head is supplied as a parked pose here; this
        // checks its input consumers, not Drive Process completion/rendering.
        let native_position = sim.entities().get(1).unwrap().position;
        let point =
            crate::render::locomotor_visual::screen_position(sim.entities().get(1).unwrap());
        assert_eq!(pick(&sim, point), Some(1));
        sim.entities_mut_for_test().get_mut(1).unwrap().position =
            sim.entities().get(2).unwrap().position;
        let old_point =
            crate::render::locomotor_visual::screen_position(sim.entities().get(1).unwrap());
        assert_eq!(
            pick(&sim, old_point),
            Some(2),
            "raw-location overlap reproduces the wrong pick"
        );
        sim.entities_mut_for_test().get_mut(1).unwrap().position = native_position;
        let selection = compute_click_selection_snapshot_with_playfield(
            sim.entities(),
            &[2, 1],
            &[],
            None,
            Some("Americans"),
            point.0,
            point.1,
            false,
            Some(&retail.rules),
            Some(sim.interner()),
            true,
            sim.resolved_terrain(),
        )
        .unwrap();
        assert_eq!(selection.select, [1]);
        assert!(sim.apply_command(
            "Americans",
            &Command::Attack {
                attacker_id: selection.select[0],
                target_id: 3,
            },
            Some(&retail.rules)
        ));
        let tank = sim.entities().get(1).unwrap();
        assert_eq!(tank.bunker.link().installed_in(), Some(2));
        assert_eq!(
            tank.attack_target.target(),
            Some(crate::sim::combat::TargetKind::Entity(3))
        );
        assert_eq!(sim.entities().get(2).unwrap().bunker.occupant(), Some(1));
    }
}
