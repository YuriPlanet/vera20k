//! Target/action and relationship line overlays.
//!
//! Selected action lines are short-lived app-layer feedback resolved from live
//! simulation movement/attack state. Factory rally lines are app-layer visuals
//! over deterministic per-producer rally target state.
//!
//! ## Dependency rules
//! - Part of the app layer - reads sim state but never mutates it.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::terrain;
use crate::render::batch::SpriteInstance;
use crate::rules::house_colors::{HouseColorRamps, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

/// How long selected action lines remain visible after a command is issued.
const DURATION_TICKS: u64 = 25;

/// One 6-bit VGA palette channel expanded to the 0..255 the renderer wants.
/// PALETTE.PAL stores `0xA8` for the full-intensity band these two lines use.
const PALETTE_CHANNEL_A8: f32 = 0xA8 as f32 / 255.0;

/// Attack (archive-target) line — PALETTE.PAL index 8, `#A80000` dark red.
/// The draw routine picks the palette index by branch: the archive-target arm
/// takes 8 and returns without falling through to the movement arm.
const ATTACK_COLOR: [f32; 3] = [PALETTE_CHANNEL_A8, 0.0, 0.0];
/// Move (navigation-target) line — PALETTE.PAL index 3, `#00A800` medium green.
const MOVE_COLOR: [f32; 3] = [0.0, PALETTE_CHANNEL_A8, 0.0];
/// Depth: above debug overlays (0.0004), below selection brackets (0.0006).
const LINE_DEPTH: f32 = 0.0005;
const ENDPOINT_BOX_RADIUS: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScreenPoint {
    x: f32,
    y: f32,
}

impl From<(f32, f32)> for ScreenPoint {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedLineKind {
    Move,
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SelectedActionLine {
    start: ScreenPoint,
    end: ScreenPoint,
    kind: SelectedLineKind,
}

/// Global selected action line state stored on `AppState`.
#[derive(Debug, Clone)]
pub(crate) struct TargetLineState {
    start_tick: Option<u64>,
    unit_action_lines_enabled: bool,
}

impl Default for TargetLineState {
    fn default() -> Self {
        Self {
            start_tick: None,
            unit_action_lines_enabled: true,
        }
    }
}

impl TargetLineState {
    pub(crate) fn is_selected_action_active(&self, current_tick: u64) -> bool {
        self.unit_action_lines_enabled
            && self
                .start_tick
                .is_some_and(|start| current_tick.saturating_sub(start) < DURATION_TICKS)
    }

    pub(crate) fn set_unit_action_lines_enabled(&mut self, enabled: bool) {
        self.unit_action_lines_enabled = enabled;
    }

    #[cfg(test)]
    pub(crate) fn unit_action_lines_enabled(&self) -> bool {
        self.unit_action_lines_enabled
    }

    /// Open the action-line window.
    ///
    /// The original's start-timer helper has four unconditional callers and
    /// three of them are selection events — band-box release, click-select and
    /// control-group recall — with order dispatch the fourth. So selecting a
    /// group flashes what it is already doing; the lines are not an order-only
    /// cue.
    pub(crate) fn start_timer(&mut self, current_tick: u64) {
        self.start_tick = Some(current_tick);
    }
}

/// Reset the selected action-line timer when action-producing commands are queued.
pub(crate) fn record_command_lines(
    state: &mut TargetLineState,
    commands: &[CommandEnvelope],
    current_tick: u64,
) {
    if commands.iter().any(|envelope| {
        matches!(
            envelope.payload,
            Command::Move { .. }
                | Command::AttackMove { .. }
                | Command::Attack { .. }
                | Command::ForceAttack { .. }
                | Command::ForceAttackCell { .. }
        )
    }) {
        state.start_tick = Some(current_tick);
    }
}

/// Build selected unit action-line instances from live simulation state.
pub(crate) fn build_target_line_instances(
    line_state: &TargetLineState,
    sim: Option<&Simulation>,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> Vec<SpriteInstance> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    if !line_state.is_selected_action_active(sim.session.tick) {
        return Vec::new();
    }

    let mut instances = Vec::new();
    for entity in sim.entities().values() {
        let Some(line) = selected_action_line_for_entity(entity, sim, height_map) else {
            continue;
        };
        let tint = match line.kind {
            SelectedLineKind::Attack => ATTACK_COLOR,
            SelectedLineKind::Move => MOVE_COLOR,
        };
        emit_selected_action_line(&mut instances, line.start, line.end, tint);
    }
    instances
}

/// Build selected local factory rally-line instances from per-producer state.
pub(crate) fn build_factory_rally_line_instances(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    height_map: &BTreeMap<(u16, u16), u8>,
    house_color_map: &HouseColorMap,
    local_owner: Option<&str>,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(rules), Some(local_owner)) = (sim, rules, local_owner) else {
        return Vec::new();
    };
    let mut instances = Vec::new();
    for entity in sim.entities().values() {
        if !entity.selected || entity.category != EntityCategory::Structure {
            continue;
        }
        let owner = sim.interner.resolve(entity.owner());
        if owner != local_owner {
            continue;
        }
        let Some((rx, ry)) = entity.rally_target else {
            continue;
        };
        let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
            continue;
        };
        if !obj.has_rally_line() {
            continue;
        }

        let (start_x, start_y) = crate::render::locomotor_visual::screen_position(entity);
        let start = ScreenPoint {
            x: start_x,
            y: start_y,
        };
        let end = project_cell_destination(rx, ry, height_map, None, Some(sim)).into();
        let tint = rally_tint_for_owner(owner, house_color_map, &rules.house_color_ramps);
        emit_rally_line(&mut instances, start, end, tint, sim.session.tick);
    }
    instances
}

fn selected_action_line_for_entity(
    entity: &GameEntity,
    sim: &Simulation,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> Option<SelectedActionLine> {
    if !entity.selected || entity.category == EntityCategory::Structure {
        return None;
    }
    let start = selected_action_line_source(entity);
    if let Some(attack) = &entity.attack_target {
        let end = resolve_attack_target_point(attack, sim, height_map)?;
        return Some(SelectedActionLine {
            start,
            end,
            kind: SelectedLineKind::Attack,
        });
    }

    let _nav_com = entity.navigation.nav_com?;
    let nav_target = entity
        .navigation
        .nav_queue
        .last()
        .copied()
        .or(entity.navigation.nav_com)?;
    Some(SelectedActionLine {
        start,
        end: resolve_navigation_target_point(nav_target, sim, height_map)?,
        kind: SelectedLineKind::Move,
    })
}

fn selected_action_line_source(entity: &GameEntity) -> ScreenPoint {
    let (x, y) = crate::render::locomotor_visual::screen_position(entity);
    ScreenPoint { x, y }
}

fn resolve_attack_target_point(
    attack: &AttackTarget,
    sim: &Simulation,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> Option<ScreenPoint> {
    match attack.target {
        TargetKind::Entity(target_id) => sim.entities().get(target_id).map(|target| ScreenPoint {
            x: crate::render::locomotor_visual::screen_position(target).0,
            y: crate::render::locomotor_visual::screen_position(target).1,
        }),
        TargetKind::Cell(rx, ry) => {
            Some(project_cell_destination(rx, ry, height_map, None, Some(sim)).into())
        }
    }
}

fn resolve_navigation_target_point(
    target: NavTargetRef,
    sim: &Simulation,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> Option<ScreenPoint> {
    match target {
        NavTargetRef::Cell { rx, ry } => {
            Some(project_cell_destination(rx, ry, height_map, None, Some(sim)).into())
        }
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => sim.entities().get(id).map(|target| ScreenPoint {
            x: crate::render::locomotor_visual::screen_position(target).0,
            y: crate::render::locomotor_visual::screen_position(target).1,
        }),
    }
}

fn project_cell_destination(
    rx: u16,
    ry: u16,
    height_map: &BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&BTreeMap<(u16, u16), u8>>,
    sim: Option<&Simulation>,
) -> (f32, f32) {
    let z = bridge_deck_height_for_cell(rx, ry, bridge_height_map, sim)
        .or_else(|| height_map.get(&(rx, ry)).copied())
        .unwrap_or(0);
    let (sx, sy) = terrain::iso_to_screen(rx, ry, z);
    (sx + 30.0, sy + 15.0)
}

fn bridge_deck_height_for_cell(
    rx: u16,
    ry: u16,
    bridge_height_map: Option<&BTreeMap<(u16, u16), u8>>,
    sim: Option<&Simulation>,
) -> Option<u8> {
    if let Some(deck_z) = bridge_height_map.and_then(|map| map.get(&(rx, ry)).copied()) {
        return Some(deck_z);
    }

    let cell = sim?.resolved_terrain.as_ref()?.cell(rx, ry)?;
    let is_low_bridge = cell
        .bridge_layer
        .as_ref()
        .is_some_and(|layer| layer.direction == crate::map::resolved_terrain::BridgeDirection::Low);
    (cell.has_bridge_deck && !is_low_bridge).then_some(cell.bridge_deck_level)
}

fn rally_tint_for_owner(
    owner: &str,
    house_color_map: &HouseColorMap,
    ramps: &HouseColorRamps,
) -> [f32; 3] {
    // Unknown owner → NO_REMAP, which ramp() resolves to the default scheme
    // (matching the producers' DEFAULT_SCHEME_ENTRY fallback), not entry 0.
    let index = house_color_map.get(owner).copied().unwrap_or(NO_REMAP);
    // Shade 0 = the scheme's brightest band (palette index 16) — gamemd's
    // radar/target-line color.
    let color = ramps.ramp(index)[0];
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    ]
}

fn push_line_pixel(instances: &mut Vec<SpriteInstance>, x: f32, y: f32, tint: [f32; 3]) {
    instances.push(SpriteInstance {
        position: [x.round(), y.round()],
        size: [1.0, 1.0],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        tint,
        alpha: 1.0,
        depth: LINE_DEPTH,
        ..Default::default()
    });
}

fn emit_endpoint_box(instances: &mut Vec<SpriteInstance>, point: ScreenPoint, tint: [f32; 3]) {
    for dy in -ENDPOINT_BOX_RADIUS..=ENDPOINT_BOX_RADIUS {
        for dx in -ENDPOINT_BOX_RADIUS..=ENDPOINT_BOX_RADIUS {
            push_line_pixel(instances, point.x + dx as f32, point.y + dy as f32, tint);
        }
    }
}

fn emit_solid_line(
    instances: &mut Vec<SpriteInstance>,
    start: ScreenPoint,
    end: ScreenPoint,
    tint: [f32; 3],
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let steps = dx.abs().max(dy.abs()).ceil() as i32;
    if steps <= 0 {
        return;
    }
    let step_x = dx / steps as f32;
    let step_y = dy / steps as f32;

    for i in 0..steps {
        push_line_pixel(
            instances,
            start.x + step_x * i as f32,
            start.y + step_y * i as f32,
            tint,
        );
    }
}

fn emit_selected_action_line(
    instances: &mut Vec<SpriteInstance>,
    start: ScreenPoint,
    end: ScreenPoint,
    tint: [f32; 3],
) {
    emit_endpoint_box(instances, start, tint);
    emit_endpoint_box(instances, end, tint);
    emit_solid_line(instances, start, end, tint);
}

fn emit_rally_line(
    instances: &mut Vec<SpriteInstance>,
    start: ScreenPoint,
    end: ScreenPoint,
    tint: [f32; 3],
    tick: u64,
) {
    let _phase = (0x7fff_ffffu64.saturating_sub(tick)) % 15;
    emit_solid_line(instances, start, end, tint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::house_colors::HouseColorIndex;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::components::MovementTarget;
    use crate::sim::game_entity::GameEntity;

    fn active_line_state_for_tick(tick: u64) -> TargetLineState {
        TargetLineState {
            start_tick: Some(tick),
            unit_action_lines_enabled: true,
        }
    }

    fn rules_with_factory_and_non_factory() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAWEAP\n\
             1=GAPOWR\n\
             [GAWEAP]\nFactory=UnitType\nStrength=1000\n\
             [GAPOWR]\nStrength=750\n",
        );
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn sim_with_selected_unit_that_has_attack_and_move() -> Simulation {
        let mut sim = Simulation::new();
        let mut unit = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        unit.selected = true;
        unit.navigation.nav_com = Some(NavTargetRef::cell(25, 25));
        unit.movement_target = Some(MovementTarget {
            final_goal: Some((25, 25)),
            path: vec![(10, 10), (25, 25)],
            ..Default::default()
        });
        unit.attack_target = Some(AttackTarget::new(2));
        let target = GameEntity::test_default(2, "HTNK", "Soviet", 14, 10);
        sim.entities_mut().insert(unit);
        sim.entities_mut().insert(target);
        sim
    }

    fn sim_with_selected_unit_navcom(nav_com: Option<(u16, u16)>) -> Simulation {
        let mut sim = Simulation::new();
        let mut unit = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        unit.selected = true;
        if let Some((rx, ry)) = nav_com {
            unit.navigation.nav_com = Some(NavTargetRef::cell(rx, ry));
        }
        sim.entities_mut().insert(unit);
        sim
    }

    fn sim_with_selected_factory_and_non_factory() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let factory_type = sim.interner.intern("GAWEAP");
        let power_type = sim.interner.intern("GAPOWR");
        let mut factory = GameEntity::new_at_frame_zero_for_test(
            10,
            10,
            10,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 1000 },
            factory_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        factory.selected = true;
        factory.rally_target = Some((16, 10));
        let mut power = factory.clone();
        power.stable_id = 11;
        power.type_ref = power_type;
        power.position.rx = 12;
        power.rally_target = Some((16, 10));
        sim.entities_mut().insert(factory);
        sim.entities_mut().insert(power);
        sim
    }

    fn test_house_colors() -> HouseColorMap {
        let mut colors = HouseColorMap::new();
        colors.insert("Americans".to_string(), HouseColorIndex(1));
        colors
    }

    #[test]
    fn bridge_height_entry_lifts_cell_destination_endpoint() {
        let rx = 10;
        let ry = 5;
        let deck_z = 4_u8;
        let height_map = BTreeMap::new();
        let mut bridge_height_map = BTreeMap::new();
        bridge_height_map.insert((rx, ry), deck_z);

        let ground = project_cell_destination(rx, ry, &height_map, None, None);
        let bridge = project_cell_destination(rx, ry, &height_map, Some(&bridge_height_map), None);

        assert_eq!(bridge.0, ground.0);
        assert!(
            (ground.1 - bridge.1 - deck_z as f32 * terrain::HEIGHT_STEP).abs() < f32::EPSILON,
            "bridge endpoint should lift by deck_z * HEIGHT_STEP"
        );
    }

    #[test]
    fn selected_action_timer_expires_at_25_ticks() {
        let state = active_line_state_for_tick(100);
        assert!(state.is_selected_action_active(124));
        assert!(!state.is_selected_action_active(125));
    }

    #[test]
    fn unit_action_lines_option_disables_selected_timer() {
        let mut state = active_line_state_for_tick(100);
        state.set_unit_action_lines_enabled(false);
        assert!(!state.is_selected_action_active(101));
    }

    /// The two line colours are palette entries, not eyeballed greens: the
    /// archive-target branch selects index 8 (`#A80000`) and the navigation
    /// branch index 3 (`#00A800`). PALETTE.PAL byte value for both is 0xA8.
    #[test]
    fn action_line_colors_are_palette_entries_eight_and_three() {
        let a8 = 168.0 / 255.0;
        assert_eq!(ATTACK_COLOR, [a8, 0.0, 0.0]);
        assert_eq!(MOVE_COLOR, [0.0, a8, 0.0]);
    }

    /// Selection alone opens the window — no command required.
    #[test]
    fn start_timer_opens_the_window_without_a_command() {
        let mut state = TargetLineState::default();
        assert!(!state.is_selected_action_active(100));
        state.start_timer(100);
        assert!(state.is_selected_action_active(100));
        assert!(state.is_selected_action_active(124));
        assert!(!state.is_selected_action_active(125));
    }

    #[test]
    fn selected_action_line_emits_endpoint_boxes() {
        let mut instances = Vec::new();
        emit_selected_action_line(
            &mut instances,
            ScreenPoint { x: 10.0, y: 10.0 },
            ScreenPoint { x: 20.0, y: 10.0 },
            MOVE_COLOR,
        );
        assert!(instances.len() >= 18);
    }

    #[test]
    fn selected_action_attack_target_wins_over_movement() {
        let sim = sim_with_selected_unit_that_has_attack_and_move();
        let target = sim.entities().get(2).unwrap();
        let lines = build_target_line_instances(
            &active_line_state_for_tick(sim.session.tick),
            Some(&sim),
            &BTreeMap::new(),
        );
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|instance| {
            instance.position
                == [
                    crate::render::locomotor_visual::screen_position(target)
                        .0
                        .round(),
                    crate::render::locomotor_visual::screen_position(target)
                        .1
                        .round(),
                ]
        }));
    }

    #[test]
    fn selected_action_line_uses_navcom_without_movement_target() {
        let sim = sim_with_selected_unit_navcom(Some((21, 22)));
        let unit = sim.entities().get(1).unwrap();
        let line = selected_action_line_for_entity(unit, &sim, &BTreeMap::new()).unwrap();
        assert_eq!(
            line.end,
            project_cell_destination(21, 22, &BTreeMap::new(), None, Some(&sim)).into()
        );
    }

    #[test]
    fn selected_action_line_uses_navqueue_last_when_navcom_exists() {
        let mut sim = sim_with_selected_unit_navcom(Some((21, 22)));
        let unit = sim.entities_mut().get_mut(1).unwrap();
        unit.navigation.nav_queue.push(NavTargetRef::cell(30, 31));
        unit.navigation.nav_queue.push(NavTargetRef::cell(32, 33));
        let unit = sim.entities().get(1).unwrap();
        let line = selected_action_line_for_entity(unit, &sim, &BTreeMap::new()).unwrap();
        assert_eq!(
            line.end,
            project_cell_destination(32, 33, &BTreeMap::new(), None, Some(&sim)).into()
        );
    }

    #[test]
    fn selected_action_line_navqueue_without_navcom_does_not_draw() {
        let mut sim = sim_with_selected_unit_navcom(None);
        sim.entities_mut()
            .get_mut(1)
            .unwrap()
            .navigation
            .nav_queue
            .push(NavTargetRef::cell(30, 31));
        let unit = sim.entities().get(1).unwrap();
        assert!(selected_action_line_for_entity(unit, &sim, &BTreeMap::new()).is_none());
    }

    #[test]
    fn factory_rally_builder_emits_only_selected_local_eligible_structures() {
        let sim = sim_with_selected_factory_and_non_factory();
        let rules = rules_with_factory_and_non_factory();
        let lines = build_factory_rally_line_instances(
            Some(&sim),
            Some(&rules),
            &BTreeMap::new(),
            &test_house_colors(),
            Some("Americans"),
        );
        assert!(!lines.is_empty());
    }

    #[test]
    fn disabling_unit_action_lines_does_not_disable_rally_lines() {
        let mut state = active_line_state_for_tick(0);
        state.set_unit_action_lines_enabled(false);
        let sim = sim_with_selected_factory_and_non_factory();
        let rules = rules_with_factory_and_non_factory();

        assert!(build_target_line_instances(&state, Some(&sim), &BTreeMap::new()).is_empty());
        assert!(
            !build_factory_rally_line_instances(
                Some(&sim),
                Some(&rules),
                &BTreeMap::new(),
                &test_house_colors(),
                Some("Americans"),
            )
            .is_empty()
        );
    }

    #[test]
    fn line_builders_skip_when_sim_or_rules_missing() {
        assert!(
            build_target_line_instances(&TargetLineState::default(), None, &BTreeMap::new())
                .is_empty()
        );
        assert!(
            build_factory_rally_line_instances(
                None,
                None,
                &BTreeMap::new(),
                &HouseColorMap::new(),
                Some("Americans")
            )
            .is_empty()
        );
    }
}
