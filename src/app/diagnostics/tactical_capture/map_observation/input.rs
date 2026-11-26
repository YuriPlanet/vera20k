//! Input observations and gesture receipts: command and gesture receipts,
//! selection, placement, sidebar and command-bar state seen around each gesture.

use super::*;
use crate::app::persistence::PersistenceState;

#[derive(Debug, Serialize)]
pub(super) struct MapCommandReceipt {
    pub(super) ordinal: usize,
    pub(super) issue_after_step: u32,
    pub(super) issued_simulation_tick: u64,
    pub(super) envelope_execute_tick: u64,
    pub(super) owner: String,
    pub(super) payload: Command,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) bindings: Option<std::collections::BTreeMap<String, Value>>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapGestureCommandReceipt {
    pub(super) owner: String,
    pub(super) execute_tick: u64,
    pub(super) payload: Command,
}

#[derive(Debug, Serialize)]
pub(super) struct MapKeyboardBinding {
    pub(super) command: &'static str,
    pub(super) first_key: Option<u16>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapInputObservation {
    /// The existing ordered input ledger, including an optimistic selection
    /// that the next ordinary command drain has not committed yet.
    pub(super) selected_ids: Vec<u64>,
    pub(super) selection_pending: bool,
    pub(super) target_line_remaining: i32,
    pub(super) target_line_active: bool,
    pub(super) planning_active: bool,
    pub(super) planning_paths: Vec<MapPlanningPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) local_input: Option<MapLocalInputObservation>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapPlanningPath {
    actor_id: u64,
    pub(super) cells: Vec<(u16, u16)>,
    loop_start: Option<usize>,
    editing: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct MapSelectionVoiceRequest {
    pub(super) speaker_id: u64,
    pub(super) sound_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct MapTargetingObservation {
    kind: &'static str,
    pub(super) type_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct MapBuildingPlacementTargetObservation {
    pub(super) type_id: String,
    pub(super) product_id: Option<u64>,
    issuer: String,
}

/// Read-only fields from one production preview cell.
#[derive(Debug, Serialize)]
pub(super) struct MapPlacementCellObservation {
    pub(super) coord: (u16, u16),
    pub(super) valid: bool,
    pub(super) level: i8,
    pub(super) slope: u8,
}

#[derive(Debug, Serialize)]
struct MapPlacementPreviewObservation {
    pub(super) origin: [u16; 2],
    foundation: [u16; 2],
    pub(super) valid: bool,
    pub(super) in_build_area: bool,
    pub(super) foundation_visible: bool,
    pub(super) reason: Option<&'static str>,
    pub(super) cells: Vec<MapPlacementCellObservation>,
    pub(super) wall_autofill_cells: Vec<(u16, u16)>,
}

/// Borrow the current input owner's references without retaining a second
/// placement transaction. Issuers resolve against this observation's world.
#[derive(Debug, Serialize)]
pub(super) struct MapBuildingPlacementObservation {
    pub(super) current: Option<MapBuildingPlacementTargetObservation>,
    pub(super) saved_retry: Option<MapBuildingPlacementTargetObservation>,
    current_preview_present: bool,
    saved_preview_present: bool,
    /// Read the active preview, never recompute placement in diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_preview: Option<MapPlacementPreviewObservation>,
}

impl MapBuildingPlacementObservation {
    pub(super) fn capture(
        input: &crate::app::input::state::MatchInputState,
        interner: &crate::sim::intern::StringInterner,
    ) -> Self {
        let view = input.building_placement_view();
        let target = |target: &crate::app::input::state::BuildingPlacementTarget| {
            MapBuildingPlacementTargetObservation {
                type_id: target.type_id().to_owned(),
                product_id: target.product_id(),
                issuer: interner.resolve(target.owner()).to_owned(),
            }
        };
        Self {
            current: view.current.map(target),
            saved_retry: view.saved_retry.map(target),
            current_preview: input.building_placement_preview.as_ref().map(|preview| {
                MapPlacementPreviewObservation {
                    origin: [preview.rx, preview.ry],
                    foundation: [preview.width, preview.height],
                    valid: preview.valid,
                    in_build_area: preview.in_build_area,
                    foundation_visible: preview.foundation_visible,
                    reason: preview.reason.as_ref().map(|reason| reason.label()),
                    cells: preview
                        .cells
                        .iter()
                        .map(|cell| MapPlacementCellObservation {
                            coord: cell.coord,
                            valid: cell.valid,
                            level: cell.level,
                            slope: cell.slope,
                        })
                        .collect(),
                    wall_autofill_cells: preview
                        .wall_autofill_cells
                        .iter()
                        .map(|cell| cell.coord)
                        .collect(),
                }
            }),
            current_preview_present: view.current_preview_present,
            saved_preview_present: view.saved_preview_present,
        }
    }

    pub(super) fn sample_count(&self) -> usize {
        1 + usize::from(self.current.is_some())
            + usize::from(self.saved_retry.is_some())
            + self.current_preview.as_ref().map_or(0, |preview| {
                1 + preview.cells.len() + preview.wall_autofill_cells.len()
            })
    }
}

/// Read-only projection of the shared selection mode and Health/Y snapshot.
#[derive(Debug, Serialize)]
pub(super) struct MapSelectionScopeObservation {
    pub(super) mode: &'static str,
    across_map: bool,
    last_outcome_key: Option<&'static str>,
    pub(super) retained_navigation: Vec<u64>,
    health_category: i32,
    veterancy_category: i32,
}

impl MapSelectionScopeObservation {
    pub(super) fn capture(
        scope: &crate::app::types::TypeSelectInputState,
        navigation: &crate::app::input::dispatch::selection_navigation::CategoryNavigation,
    ) -> Self {
        let (mode, across_map, last_outcome_key) = scope.selection_scope_view();
        let (candidates, health, veterancy) = navigation.navigation_view();
        Self {
            mode,
            across_map,
            last_outcome_key,
            retained_navigation: candidates.to_vec(),
            health_category: health.map_or(-1, i32::from),
            veterancy_category: veterancy.map_or(-1, i32::from),
        }
    }
}

/// Read-only values from the current input, sidebar, simulation and audio owners.
/// Pending voice requests are observed before playback, not evidence of audible output.
#[derive(Debug, Serialize)]
pub(super) struct MapLocalInputObservation {
    pub(super) camera_top_left: [f32; 2],
    pub(super) camera_zoom: f32,
    pub(super) follow_target: Option<u64>,
    pub(super) repair_mode: bool,
    pub(super) sell_mode: bool,
    pub(super) targeting: Option<MapTargetingObservation>,
    /// Historical receipts omit this projection; an observed empty owner
    /// serializes a present object whose current/saved targets are null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) building_placement: Option<MapBuildingPlacementObservation>,
    pub(super) main_rng_cursor: [i32; 2],
    pub(super) selection_voice_enabled: bool,
    pub(super) selection_voice_requests: Vec<MapSelectionVoiceRequest>,
    pub(super) selection_scope: MapSelectionScopeObservation,
    /// Actual localized rows in insertion order; observing never posts/expires a row.
    pub(super) hud_messages: Vec<String>,
}

impl MapLocalInputObservation {
    pub(super) fn capture(
        frontend: &FrontendState,
        match_state: &MatchState,
        sim: &crate::sim::world::Simulation,
    ) -> Self {
        use crate::app::types::TargetingMode;
        let input = &match_state.input;
        let gadgets = &match_state.match_presentation.sidebar_gadget_state;
        let main_rng = sim.rng_views().main;
        Self {
            camera_top_left: [input.camera_x, input.camera_y],
            camera_zoom: input.zoom_level,
            follow_target: input.follow_target,
            repair_mode: gadgets.repair_mode_on,
            sell_mode: gadgets.sell_mode_on,
            targeting: input.targeting_mode.as_ref().map(|mode| {
                let (kind, type_id) = match mode {
                    TargetingMode::BuildingPlacement(target) => {
                        ("building_placement", target.type_id())
                    }
                    TargetingMode::SuperWeapon(name) => ("super_weapon", name.as_str()),
                };
                MapTargetingObservation {
                    kind,
                    type_id: type_id.to_string(),
                }
            }),
            building_placement: Some(MapBuildingPlacementObservation::capture(
                input,
                sim.interner(),
            )),
            main_rng_cursor: [main_rng.index_a, main_rng.index_b],
            selection_voice_enabled: input.selection_voice_enabled,
            selection_voice_requests: match_state
                .match_audio
                .sound_events
                .iter()
                .filter_map(|event| match event {
                    crate::audio::events::GameSoundEvent::UnitSelected { speaker_id, .. } => {
                        Some(MapSelectionVoiceRequest {
                            speaker_id: *speaker_id,
                            sound_id: event
                                .sound_id(
                                    crate::app::state::active_rules(frontend, match_state)
                                        .map(|rules| rules.sound_names()),
                                )
                                .to_string(),
                        })
                    }
                    _ => None,
                })
                .collect(),
            selection_scope: MapSelectionScopeObservation::capture(
                &input.type_select,
                &input.category_navigation,
            ),
            hud_messages: match_state
                .match_presentation
                .message_list
                .messages()
                .iter()
                .map(|message| message.text.clone())
                .collect(),
        }
    }
}

impl MapInputObservation {
    pub(super) fn capture(
        frontend: &FrontendState,
        match_state: &MatchState,
        local_input: bool,
    ) -> Result<Self> {
        let sim = &match_state
            .sim_runtime
            .runtime()
            .context("gesture observation requires a simulation")?
            .simulation;
        let lines = &match_state.match_presentation.target_lines;
        Ok(Self {
            selected_ids: crate::app::input::dispatch::selected_stable_ids_in_order(
                Some(sim),
                crate::app::state::active_rules(frontend, match_state),
                &match_state.input.selection_order,
                match_state.input.selection_order_pending,
            ),
            selection_pending: match_state.input.selection_order_pending,
            target_line_remaining: lines.remaining_frames(sim.session().binary_frame()),
            target_line_active: lines.is_selected_action_active(sim.session().binary_frame()),
            planning_active: match_state.input.planning.active(),
            planning_paths: sim
                .session()
                .current_house()
                .map_or_else(Vec::new, |owner| {
                    sim.planning_paths(owner)
                        .into_iter()
                        .map(|path| MapPlanningPath {
                            actor_id: path.actor_id,
                            cells: path.cells,
                            loop_start: path.loop_start,
                            editing: path.editing,
                        })
                        .collect()
                }),
            local_input: local_input
                .then(|| MapLocalInputObservation::capture(frontend, match_state, sim)),
        })
    }

    pub(super) fn sample_count(&self) -> usize {
        self.selected_ids.len()
            + self.planning_paths.len()
            + self
                .planning_paths
                .iter()
                .map(|path| path.cells.len())
                .sum::<usize>()
            + self.local_input.as_ref().map_or(0, |input| {
                input.selection_voice_requests.len()
                    + 1
                    + input.selection_scope.retained_navigation.len()
                    + input.hud_messages.len()
                    + input
                        .building_placement
                        .as_ref()
                        .map_or(0, MapBuildingPlacementObservation::sample_count)
            })
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub(super) struct MapSidebarControl {
    pub(super) rect: [f32; 4],
    pub(super) disabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(super) struct MapSidebarToggle {
    pub(super) rect: [f32; 4],
    pub(super) disabled: bool,
    pub(super) active: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub(super) struct MapSidebarTab {
    pub(super) tab: &'static str,
    pub(super) rect: [f32; 4],
    pub(super) active: bool,
    pub(super) disabled: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub(super) struct MapSidebarItem {
    pub(super) slot: usize,
    pub(super) type_id: String,
    pub(super) super_weapon_section: Option<String>,
    pub(super) display_name: String,
    pub(super) rect: [f32; 4],
    pub(super) cost: Option<i32>,
    pub(super) queue_category: String,
    pub(super) enabled: bool,
    pub(super) progress: f32,
    pub(super) queued_count: usize,
    pub(super) is_building_this_type: bool,
    pub(super) is_ready: bool,
    pub(super) is_on_hold: bool,
    pub(super) is_armed: bool,
    pub(super) is_superweapon: bool,
}

impl MapSidebarItem {
    fn identity(&self) -> &str {
        self.super_weapon_section
            .as_deref()
            .unwrap_or(&self.type_id)
    }
}

/// Only copies the retained projection consumed by rendering and hit-testing.
/// It never rebuilds options, clamps scrolling or advances any UI state.
#[derive(Debug, Serialize, PartialEq)]
pub(super) struct MapSidebarObservation {
    pub(super) active_tab: &'static str,
    pub(super) scroll_rows: usize,
    pub(super) max_scroll_rows: usize,
    pub(super) tabs: Vec<MapSidebarTab>,
    pub(super) items: Vec<MapSidebarItem>,
    pub(super) scroll_up: MapSidebarControl,
    pub(super) scroll_down: MapSidebarControl,
    // Button targets need their actual retained hit rectangles. Preserve the
    // historical frame/strip receipt; serialize these only for a toggle gesture.
    #[serde(skip_serializing)]
    pub(super) mode_buttons: [MapSidebarToggle; 2],
}

impl MapSidebarObservation {
    pub(super) fn capture(
        presentation: &crate::app::presentation::state::MatchPresentationState,
    ) -> Result<Self> {
        Self::from_view(
            crate::app::presentation::sidebar_render::current_sidebar_view(presentation)
                .context("sidebar observation requires the retained view")?,
        )
    }

    pub(super) fn from_view(view: &crate::ui::sidebar::SidebarView) -> Result<Self> {
        let rect = |rect: crate::ui::sidebar::Rect| [rect.x, rect.y, rect.w, rect.h];
        let active: Vec<_> = view.tabs.iter().filter(|tab| tab.active).collect();
        ensure!(
            active.len() == 1,
            "sidebar projection must have one active tab"
        );
        ensure!(
            view.items.iter().all(|item| item.progress.is_finite()),
            "sidebar progress is not finite"
        );
        Ok(Self {
            active_tab: sidebar_tab_name(active[0].tab),
            scroll_rows: view.scroll_rows,
            max_scroll_rows: view.max_scroll_rows,
            tabs: view
                .tabs
                .iter()
                .map(|tab| MapSidebarTab {
                    tab: sidebar_tab_name(tab.tab),
                    rect: rect(tab.rect),
                    active: tab.active,
                    disabled: tab.disabled,
                })
                .collect(),
            items: view
                .items
                .iter()
                .enumerate()
                .map(|(slot, item)| MapSidebarItem {
                    slot,
                    type_id: item.type_id.clone(),
                    super_weapon_section: item.super_weapon_section.clone(),
                    display_name: item.display_name.clone(),
                    rect: rect(item.rect),
                    cost: item.cost,
                    queue_category: format!("{:?}", item.queue_category),
                    enabled: item.enabled,
                    progress: item.progress,
                    queued_count: item.queued_count,
                    is_building_this_type: item.is_building_this_type,
                    is_ready: item.is_ready,
                    is_on_hold: item.is_on_hold,
                    is_armed: item.is_armed,
                    is_superweapon: item.is_superweapon,
                })
                .collect(),
            scroll_up: MapSidebarControl {
                rect: rect(view.scroll_up_button.rect),
                disabled: view.scroll_up_button.disabled,
            },
            scroll_down: MapSidebarControl {
                rect: rect(view.scroll_down_button.rect),
                disabled: view.scroll_down_button.disabled,
            },
            // Native never disables Repair/Sell; the receipt keeps the field.
            mode_buttons: [&view.repair_button, &view.sell_button].map(|button| MapSidebarToggle {
                rect: rect(button.rect),
                disabled: false,
                active: button.active,
            }),
        })
    }

    pub(super) fn sample_count(&self) -> usize {
        self.items.len() + self.tabs.len() + 2
    }

    pub(super) fn toggle(&self, target: &MapSidebarTarget) -> Option<&MapSidebarToggle> {
        match target {
            MapSidebarTarget::Repair {} => Some(&self.mode_buttons[0]),
            MapSidebarTarget::Sell {} => Some(&self.mode_buttons[1]),
            _ => None,
        }
    }

    pub(super) fn resolve(
        &self,
        target: &MapSidebarTarget,
        extent: [u32; 2],
    ) -> Result<([u32; 2], usize)> {
        let (rect, disabled, slot) = match target {
            MapSidebarTarget::Tab { tab } => {
                let (slot, found) = self
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(_, entry)| entry.tab == tab.as_str())
                    .context("requested sidebar tab is absent")?;
                (found.rect, found.disabled, slot)
            }
            MapSidebarTarget::Cameo { type_id } => {
                let matches: Vec<_> = self
                    .items
                    .iter()
                    .filter(|item| item.identity() == type_id.as_str())
                    .collect();
                ensure!(
                    matches.len() == 1,
                    "requested cameo {type_id:?} must occur once in the visible strip"
                );
                let item = matches[0];
                // A grey build-limit cameo remains a live native gadget.
                (item.rect, false, item.slot)
            }
            MapSidebarTarget::ScrollUp {} => (self.scroll_up.rect, self.scroll_up.disabled, 0),
            MapSidebarTarget::ScrollDown {} => {
                (self.scroll_down.rect, self.scroll_down.disabled, 0)
            }
            MapSidebarTarget::Repair {} | MapSidebarTarget::Sell {} => {
                let toggle = self.toggle(target).expect("mode toggle target");
                (toggle.rect, toggle.disabled, 0)
            }
        };
        Ok((control_center(rect, disabled, extent)?, slot))
    }
}

fn control_center(rect: [f32; 4], disabled: bool, extent: [u32; 2]) -> Result<[u32; 2]> {
    ensure!(!disabled, "requested control is disabled");
    ensure!(
        rect.iter().all(|part| part.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0,
        "requested control has no finite hit area"
    );
    let center = [
        (rect[0] + rect[2] / 2.0).floor(),
        (rect[1] + rect[3] / 2.0).floor(),
    ];
    ensure!(
        center
            .into_iter()
            .zip(extent)
            .all(|(point, limit)| limit > 1 && point > 0.0 && point < (limit - 1) as f32),
        "requested control center is outside the capture"
    );
    Ok(center.map(|point| point as u32))
}

pub(super) fn command_bar_index(command: &str) -> Result<usize> {
    if command == "Thumb" {
        return Ok(11);
    }
    crate::ui::sidebar::command_bar::COMMAND_NAMES
        .iter()
        .position(|name| *name == command)
        .context("unknown command-bar command")
}

#[derive(Debug, Serialize)]
pub(super) struct MapCommandBarGestureReceipt {
    pub(super) command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) slot: Option<usize>,
    pub(super) gadget_id: u16,
    pub(super) rect: [f32; 4],
    pub(super) resolved_position: [u32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) thumb: Option<MapCommandBarThumbReceipt>,
}

/// Passive snapshots of the existing owners, only for a requested thumb
/// gesture. No observation writes House, bar, input or driver state.
#[derive(Debug, Serialize)]
pub(super) struct MapCommandBarThumbState {
    closed: bool,
    pub(super) pressed: bool,
    pub(super) sticky: bool,
    pub(super) left_held: bool,
    pub(super) gadget_id: u16,
    pub(super) rect: [i32; 4],
    pub(super) disabled: bool,
    pub(super) current_house: Option<String>,
    side_index: Option<i32>,
    local_player_defeated: bool,
    pub(super) input_locked: bool,
    has_lost: Option<bool>,
    exit_ready: Option<bool>,
}

impl MapCommandBarThumbState {
    pub(super) fn capture(match_state: &MatchState, handle: vera20k_gui::NodeId) -> Result<Self> {
        let presentation = &match_state.match_presentation;
        let (gui, nodes) = crate::app::input::chrome_input::chrome(&match_state.match_presentation)
            .context("thumb control disappeared")?;
        let control = ChromeControl::read(gui, handle).context("thumb control disappeared")?;
        let sim = &match_state
            .sim_runtime
            .runtime()
            .context("thumb observation requires a simulation")?
            .simulation;
        let owner = sim.session().current_house();
        let house = owner.and_then(|owner| sim.houses().get(&owner));
        Ok(Self {
            closed: presentation.sidebar_gadget_state.command_bar_closed,
            pressed: gui.shows_pressed_in(nodes.layer, handle),
            sticky: gui.pressed_in(nodes.layer) == Some(handle),
            left_held: match_state.input.tactical_mouse.left_held(),
            gadget_id: control.id,
            rect: [
                control.rect.x,
                control.rect.y,
                control.rect.w,
                control.rect.h,
            ],
            disabled: control.disabled,
            current_house: owner.map(|owner| sim.interner().resolve(owner).to_owned()),
            side_index: house.map(|house| house.side_index()),
            local_player_defeated: sim.local_player_defeated(),
            input_locked: sim.session().input_locked(),
            has_lost: house.map(|house| house.has_lost()),
            exit_ready: owner
                .zip(house)
                .map(|(owner, _)| sim.ready_outcome_for_owner(owner).is_some()),
        })
    }
}

/// What the chrome did with the edge at this control: it took the edge, and
/// fired it (by the right button or not).
#[derive(Debug, Serialize)]
struct MapCommandBarDispatchObservation {
    fired: bool,
    right: bool,
}

/// A chrome control as the observation reads it.
struct ChromeControl {
    id: u16,
    rect: vera20k_gui::Rect,
    disabled: bool,
}

impl ChromeControl {
    fn read(gui: &vera20k_gui::Gui, node: vera20k_gui::NodeId) -> Option<Self> {
        use vera20k_gui::{Prop, Value};
        // Where the cursor hits it; its `Rect` is where its picture draws.
        let rect = match gui.get(node, Prop::HitRect).ok()? {
            Some(Value::Rect(rect)) => *rect,
            _ => vera20k_gui::Rect::default(),
        };
        let disabled = matches!(gui.get(node, Prop::Enabled), Ok(Some(Value::Bool(false))));
        Some(Self {
            id: u16::try_from(gui.id_of(node)?).ok()?,
            rect,
            disabled,
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapCommandBarThumbEdge {
    pub(super) state: MapCommandBarThumbState,
    dispatch: Option<MapCommandBarDispatchObservation>,
}

impl MapCommandBarThumbEdge {
    pub(super) fn capture(match_state: &MatchState, handle: vera20k_gui::NodeId) -> Result<Self> {
        let snapshot = MapCommandBarThumbState::capture(match_state, handle)?;
        // Both ordinary input ingress and the chrome return before routing
        // while Scenario input is locked. Do not mistake the record of an
        // earlier edge for a dispatch of this blocked edge.
        let chrome = &match_state.match_presentation.in_game_chrome;
        let dispatch = (!snapshot.input_locked && chrome.last_route == Some(handle)).then(|| {
            let fired = chrome.last_fired.filter(|(node, _)| *node == handle);
            MapCommandBarDispatchObservation {
                fired: fired.is_some(),
                right: fired.is_some_and(|(_, right)| right),
            }
        });
        Ok(Self {
            state: snapshot,
            dispatch,
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapCommandBarThumbReceipt {
    pub(super) before: MapCommandBarThumbState,
    pub(super) press: Option<MapCommandBarThumbEdge>,
    pub(super) release: Option<MapCommandBarThumbEdge>,
    pub(super) after: Option<MapCommandBarThumbState>,
}

impl MapCommandBarGestureReceipt {
    pub(super) fn sample_count(&self) -> usize {
        1 + self.thumb.as_ref().map_or(0, |thumb| {
            1 + usize::from(thumb.press.is_some())
                + usize::from(thumb.release.is_some())
                + usize::from(thumb.after.is_some())
                + usize::from(
                    thumb
                        .press
                        .as_ref()
                        .is_some_and(|edge| edge.dispatch.is_some()),
                )
                + usize::from(
                    thumb
                        .release
                        .as_ref()
                        .is_some_and(|edge| edge.dispatch.is_some()),
                )
        })
    }

    pub(super) fn capture(
        platform: &PlatformState,
        renderer: &RendererState,
        frontend: &FrontendState,
        match_state: &MatchState,
        persistence: &PersistenceState,
        command: &str,
    ) -> Result<(Self, vera20k_gui::NodeId)> {
        let (layout, slots) =
            crate::app::presentation::sidebar_build::command_bar::layout_and_slots(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
            )
            .context("command-bar layout is absent")?;
        let (gui, nodes) = crate::app::input::chrome_input::chrome(&match_state.match_presentation)
            .context("command-bar controls are absent")?;
        let (mut receipt, handle) = Self::resolve(
            command,
            layout,
            &slots,
            (gui, nodes.command_bar),
            [
                renderer.render_width(&frontend.screen),
                renderer.render_height(&frontend.screen),
            ],
        )?;
        if command == "Thumb" {
            ensure!(
                frontend.screen == crate::ui::game_screen::GameScreen::InGame
                    && !match_state.paused()
                    && frontend.keyboard_dialog.is_none(),
                "thumb gesture requires ordinary unpaused in-game input"
            );
            receipt.thumb = Some(MapCommandBarThumbReceipt {
                before: MapCommandBarThumbState::capture(match_state, handle)?,
                press: None,
                release: None,
                after: None,
            });
        }
        Ok((receipt, handle))
    }

    pub(super) fn resolve(
        command: &str,
        layout: crate::ui::sidebar::command_bar::CommandBarLayout,
        slots: &[Option<usize>],
        (gui, bar): (&vera20k_gui::Gui, [vera20k_gui::NodeId; 12]),
        extent: [u32; 2],
    ) -> Result<(Self, vera20k_gui::NodeId)> {
        let command_index = command_bar_index(command)?;
        let slot = if command_index == 11 {
            None
        } else {
            Some(
                slots
                    .iter()
                    .position(|entry| *entry == Some(command_index))
                    .context("requested command is absent from the loaded command bar")?,
            )
        };
        let layout_rect = match slot {
            Some(slot) => layout
                .slot(slot)
                .context("requested command-bar slot is closed or clipped")?,
            None => layout.left_cap,
        };
        let handle = bar[command_index];
        let control =
            ChromeControl::read(gui, handle).context("requested command-bar control is absent")?;
        let rect = [
            control.rect.x,
            control.rect.y,
            control.rect.w,
            control.rect.h,
        ]
        .map(|v| v as f32);
        ensure!(
            rect == [layout_rect.x, layout_rect.y, layout_rect.w, layout_rect.h],
            "command-bar control differs from its current layout"
        );
        let resolved_position = control_center(rect, control.disabled, extent)?;
        Ok((
            Self {
                command: command.to_owned(),
                slot,
                gadget_id: control.id,
                rect,
                resolved_position,
                thumb: None,
            },
            handle,
        ))
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapSidebarGestureReceipt {
    pub(super) resolved_position: [u32; 2],
    pub(super) before: MapSidebarObservation,
    pub(super) after: MapSidebarObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) toggle: Option<MapSidebarToggleReceipt>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapSidebarToggleReceipt {
    pub(super) before: MapSidebarToggle,
    pub(super) after: MapSidebarToggle,
}

#[derive(Debug, Serialize)]
pub(super) struct MapKeyboardGestureReceipt {
    /// The production logical-key translator's encoded key and loaded table
    /// resolution. The diagnostic never chooses a semantic command to execute.
    encoded_key: u16,
    binding_command: Option<&'static str>,
    press_held: bool,
    release_cleared: bool,
    planning_before: bool,
    planning_after: bool,
}

impl MapKeyboardGestureReceipt {
    pub(super) fn dispatch(
        state: &mut AppState,
        key: &str,
        modifiers: &[String],
        key_edge: Option<MapKeyEdge>,
    ) -> Result<Self> {
        use crate::app::input::hotkeys::{self, HotkeyResolution};
        use crate::app::input::keyboard::{InGameKeyEdge, in_game_key_edge};
        use winit::event::ElementState;
        use winit::keyboard::KeyLocation;
        ensure!(
            state.frontend.screen == crate::ui::game_screen::GameScreen::InGame
                && !state.match_state.paused()
                && state.frontend.keyboard_dialog.is_none(),
            "key gesture requires ordinary unpaused in-game input"
        );
        let (physical, logical) = literal_key(key)?;
        let planning_before = state.match_state.input.planning.active();
        let modifier_keys = literal_modifiers(modifiers)?;
        literal_modifier_edges(
            &mut state.platform,
            &mut state.match_state.input,
            &modifier_keys,
            true,
        );
        let location = KeyLocation::Standard;
        let encoded_key = hotkeys::logical_virtual_key(&logical, location)
            .context("literal key has no production virtual-key identity")?;
        let resolution = state.match_state.input.hotkey_bindings.resolve_event(
            &logical,
            location,
            state.match_state.input.hotkey_modifiers,
        );
        if key_edge.is_some() {
            ensure!(
                resolution == HotkeyResolution::Command(hotkeys::HotkeyCommand::PlanningMode)
                    || state.match_state.input.planning.owns_key(physical),
                "separate key edges require the production planning binding or its owned release"
            );
        }
        let binding_command = match resolution {
            HotkeyResolution::Command(command) => hotkeys::catalog::registered_commands()
                .iter()
                .find(|metadata| metadata.command == command)
                .map(|metadata| metadata.ini_name),
            _ => None,
        };
        let edge = |state: &mut AppState, pressed, repeat| {
            in_game_key_edge(
                state,
                InGameKeyEdge {
                    physical,
                    logical: &logical,
                    unmodified: &logical,
                    location,
                    state: pressed,
                    repeat,
                    text: None,
                },
                false,
            )
        };
        if key_edge != Some(MapKeyEdge::Released) {
            edge(
                state,
                ElementState::Pressed,
                key_edge == Some(MapKeyEdge::Repeat),
            );
        }
        let press_held = state.match_state.input.keys_held.contains(&physical);
        if key_edge.is_none() || key_edge == Some(MapKeyEdge::Released) {
            edge(state, ElementState::Released, false);
        }
        let release_cleared = !state.match_state.input.keys_held.contains(&physical);
        literal_modifier_edges(
            &mut state.platform,
            &mut state.match_state.input,
            &modifier_keys,
            false,
        );
        ensure!(
            (key_edge == Some(MapKeyEdge::Released) || press_held)
                && (matches!(key_edge, Some(MapKeyEdge::Pressed | MapKeyEdge::Repeat))
                    || release_cleared)
                && state.match_state.input.hotkey_modifiers.is_empty(),
            "key gesture did not traverse ordinary held-key edges"
        );
        ensure!(
            state.frontend.screen == crate::ui::game_screen::GameScreen::InGame
                && !state.match_state.paused()
                && state.frontend.keyboard_dialog.is_none(),
            "key gesture left ordinary unpaused in-game input"
        );
        Ok(Self {
            encoded_key,
            binding_command,
            press_held,
            release_cleared,
            planning_before,
            planning_after: state.match_state.input.planning.active(),
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapSidebarFrame {
    pub(super) completed_steps: u64,
    /// L0 is retained-only when a nonzero capture starts at its first step.
    pub(super) rendered: bool,
    pub(super) sidebar: MapSidebarObservation,
}

#[derive(Debug, Serialize)]
pub(super) struct MapMinimapGestureReceipt {
    pub(super) resolved_position: [u32; 2],
    pub(super) tracker_hit: Option<u64>,
    pub(super) cell: [u16; 2],
    pub(super) world_leptons: [i32; 3],
}

#[derive(Debug, Serialize)]
pub(super) struct MapGestureReceipt {
    pub(super) ordinal: usize,
    pub(super) issue_after_step: u32,
    pub(super) issued_simulation_tick: u64,
    pub(super) issued_binary_frame: u32,
    pub(super) gesture: MapGesture,
    pub(super) before: MapInputObservation,
    pub(super) after: MapInputObservation,
    pub(super) left_press_captured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) right_press_captured: Option<bool>,
    pub(super) band_box_before_release: bool,
    pub(super) neutral_input_restored: bool,
    pub(super) queued_commands: Vec<MapGestureCommandReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sidebar: Option<MapSidebarGestureReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) keyboard: Option<MapKeyboardGestureReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) command_bar: Option<MapCommandBarGestureReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) minimap: Option<MapMinimapGestureReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) held_cursor: Option<MapHeldCursorReceipt>,
}

/// Cursors a drag's draws pick while its button is held: one at the press
/// point, one at the drag end before release. Each is the production draw's
/// own selection (`ui_overlays::shown_cursor_id`), which also notes it as
/// the last shown cursor, as a drawn frame does.
#[derive(Debug, Serialize)]
pub(super) struct MapHeldCursorReceipt {
    pub(super) press: Option<String>,
    pub(super) before_release: Option<String>,
}
