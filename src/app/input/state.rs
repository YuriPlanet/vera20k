//! Match input owner (F12 `MatchInputState`): camera, zoom, cursor, held
//! keys, hotkey bindings, and the TypeSelect input machine.
//!
//! These are the app-side input/viewport facts for the running match (and the
//! shells that reuse the same cursor/key plumbing). Sim-authoritative input —
//! queued commands — lives in the simulation, never here.

use crate::sim::intern::InternedId;
use crate::sim::snapshot::saved_local_view::{
    CONTROL_GROUP_SLOTS, SavedCursorModes, SavedLocalView, SavedLocalViewError, SavedTargeting,
};
use std::collections::HashSet;
use winit::keyboard::{KeyCode, ModifiersState};

/// Client-side handles installed by ManualPlace4FB840. The House factory
/// remains the owner of the completed object; these are cursor references,
/// not a second ready-object registry. Type is independent of product pointer
/// lifetime in DispatchPointerExpired7258D0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildingPlacementTarget {
    type_id: String,
    product_id: Option<u64>,
    /// House+30 at4FB8C4 is captured as the existing command House identity.
    owner: InternedId,
}

impl BuildingPlacementTarget {
    pub(crate) fn type_id(&self) -> &str {
        &self.type_id
    }

    pub(crate) fn product_id(&self) -> Option<u64> {
        self.product_id
    }

    pub(crate) fn owner(&self) -> InternedId {
        self.owner
    }
}

/// Read-only current and deferred placement references at an input boundary.
/// Diagnostics borrow the existing owner; they keep no second transaction.
pub(crate) struct BuildingPlacementInputView<'a> {
    pub(crate) current: Option<&'a BuildingPlacementTarget>,
    pub(crate) saved_retry: Option<&'a BuildingPlacementTarget>,
    pub(crate) current_preview_present: bool,
    pub(crate) saved_preview_present: bool,
}

/// BandBoxLeftUp4ABBF5 saves current placement independently of its active
/// cursor. A later HousePlace4FB35F failure restores this transaction, rather
/// than inferring the target from the failing event's type.
struct SavedBuildingPlacement {
    target: BuildingPlacementTarget,
    preview: Option<crate::sim::production::BuildingPlacementPreview>,
}

/// A saved view whose identities and selection timeline were checked against
/// the prepared world. Commit performs no further admission or command work.
#[derive(Debug)]
pub(crate) struct PreparedSavedLocalView {
    saved: SavedLocalView,
    targeting: Option<crate::app::types::TargetingMode>,
}

impl PreparedSavedLocalView {
    /// Validate before replacing either the world or its input owner. Stable
    /// IDs are resolved only against the incoming world, never the outgoing one.
    pub(crate) fn new(
        saved: SavedLocalView,
        simulation: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<Self, SavedLocalViewError> {
        saved.validate_structure()?;
        let follow_target = saved.follow_target();
        for (field, references) in [
            ("selection", saved.selection_order()),
            ("Follow", follow_target.as_slice()),
        ] {
            for &object_id in references {
                if simulation.entities().get(object_id).is_none() {
                    return Err(SavedLocalViewError::MissingEntityReference { field, object_id });
                }
            }
        }
        for &object_id in saved.control_groups().iter().flatten() {
            if simulation.entities().get(object_id).is_none() {
                return Err(SavedLocalViewError::MissingEntityReference {
                    field: "control group",
                    object_id,
                });
            }
        }
        let (order, pending) = crate::app::input::dispatch::selection_order_for_world(
            simulation,
            Some(rules),
            saved.selection_order(),
            saved.selection_order_pending(),
        );
        if order != saved.selection_order() || pending != saved.selection_order_pending() {
            return Err(SavedLocalViewError::SelectionTimelineMismatch);
        }
        let unknown = |field, name: &str| SavedLocalViewError::UnknownName {
            field,
            name: name.to_owned(),
        };
        let targeting = match &saved.modes().targeting {
            None => None,
            Some(SavedTargeting::SuperWeapon(name)) => {
                rules
                    .super_weapon(name)
                    .ok_or_else(|| unknown("superweapon", name))?;
                Some(crate::app::types::TargetingMode::SuperWeapon(name.clone()))
            }
            Some(SavedTargeting::BuildingPlacement {
                type_id,
                product_id,
                owner,
            }) => {
                rules
                    .object(type_id)
                    .ok_or_else(|| unknown("placement type", type_id))?;
                if let Some(object_id) = *product_id
                    && simulation.entities().get(object_id).is_none()
                {
                    return Err(SavedLocalViewError::MissingEntityReference {
                        field: "placement",
                        object_id,
                    });
                }
                let owner = simulation
                    .interner
                    .get(owner)
                    .ok_or_else(|| unknown("placement House", owner))?;
                Some(crate::app::types::TargetingMode::BuildingPlacement(
                    BuildingPlacementTarget {
                        type_id: type_id.clone(),
                        product_id: *product_id,
                        owner,
                    },
                ))
            }
        };
        Ok(Self { saved, targeting })
    }

    /// Restore exactly the saved owners after navigation's world-replacement
    /// invalidation. The Tactical view has its own owner,
    /// camera::restore_tactical_view, applied once new geometry is installed.
    pub(crate) fn commit_into(
        self,
        input: &mut MatchInputState,
        sidebar: &mut crate::ui::sidebar::gadget_flash::SidebarGadgetState,
    ) {
        input
            .view_bookmarks
            .restore_saved_cells(self.saved.view_bookmarks());
        let modes = self.saved.modes();
        sidebar.repair_mode_on = modes.repair;
        sidebar.sell_mode_on = modes.sell;
        input.targeting_mode = self.targeting;
        // The preview is derived from the cursor; the next tactical frame
        // recomputes it. The BandBoxLeftUp retry copy (+1190..+1198) loads
        // raw but MouseLoad never swizzles it, so no retry survives a load.
        input.building_placement_preview = None;
        input.saved_building_placement = None;
        let (order, pending, groups, follow) = self.saved.into_local_input();
        input.selection_order = order;
        input.selection_order_pending = pending;
        input.control_groups = groups.into();
        input.follow_target = follow;
        // ClearScene685640 and PostLoad6851D2 call734210, which resets the
        // last team slot to -1 and rewinds both click clocks. None has the
        // same next-press behavior without persisting a wall-clock Instant.
        // Original reset witnesses: saved_game_layout.json/postload_clock_reset.
        input.last_control_group_press = None;
    }
}

pub(crate) struct MatchInputState {
    /// Presentation display depth; ScenarioSession owns the gameplay lock.
    pub(crate) cursor_visibility: crate::app::input::cursor::CursorVisibility,
    pub(crate) camera_x: f32,
    pub(crate) camera_y: f32,
    /// Camera-owned requests awaiting Tactical__AI's next single commit.
    pub(crate) pending_camera_scroll: crate::app::input::camera::PendingCameraScroll,
    /// Current zoom level for the game viewport. 1.0 = native pixel scale,
    /// above 1.0 = zoomed in (world appears larger), below 1.0 = zoomed out
    /// (see more map). Animated each frame toward `zoom_target`.
    pub(crate) zoom_level: f32,
    /// Target zoom level — mouse wheel sets this; `zoom_level` eases toward it.
    pub(crate) zoom_target: f32,
    /// World-space anchor point for zoom animation. The camera adjusts each frame
    /// so this world point stays at `zoom_anchor_screen` during the zoom ease.
    pub(crate) zoom_anchor_world: [f32; 2],
    /// Screen-space position of the zoom anchor (cursor position when wheel fired).
    pub(crate) zoom_anchor_screen: [f32; 2],
    /// Mouse edge auto-scroll ramp state (gamemd's CoastLevel and its 16 ms timer).
    pub(crate) edge_scroll: crate::app::input::camera::EdgeScrollState,
    /// Tactical mouse capture and right-drag pan anchor.
    pub(crate) tactical_mouse: crate::app::input::camera::TacticalMouseState,
    /// The four camera bookmarks (View1..4 / SetView1..4).
    pub(crate) view_bookmarks: crate::app::input::camera::ViewBookmarks,
    pub(crate) cursor_x: f32,
    pub(crate) cursor_y: f32,
    /// The match zoom `cursor_x`/`cursor_y` are in; `None` is window pixels.
    /// `AppState::sync_cursor_space` keeps it matching the current screen.
    pub(crate) cursor_zoom: Option<crate::render::upscale_pass::ZoomGeometry>,
    pub(crate) keys_held: HashSet<KeyCode>,
    pub(crate) hotkey_bindings: crate::app::input::hotkeys::HotkeyBindings,
    pub(crate) hotkey_modifiers: ModifiersState,
    /// Hybrid held/tap state for the retail TypeSelect command.
    pub(crate) type_select: crate::app::types::TypeSelectInputState,
    pub(crate) category_navigation:
        crate::app::input::dispatch::selection_navigation::CategoryNavigation,
    /// Original CursorCheat toggle A8F7D8, consumed by tactical tooltips.
    pub(crate) cursor_coordinates: bool,
    /// One-shot Shift+S request, consumed at the next render submission.
    pub(crate) retail_screenshot_requested: bool,
    /// True while left-dragging on minimap (camera pan mode).
    pub(crate) minimap_dragging: bool,
    /// Selection drag state — tracks mouse drag for box-select.
    pub(crate) selection_state: crate::sim::selection::SelectionState,
    /// Player-side `g_CurrentObjects` order. Selection commands update this
    /// immediately; the post-sim reconciliation removes lifecycle departures.
    pub(crate) selection_order: Vec<u64>,
    /// A queued selection command has not yet reached the simulation tick.
    pub(crate) selection_order_pending: bool,
    /// Existing selection paths speak by default; held TypeSelect batches
    /// temporarily suppress and restore this latch.
    pub(crate) selection_voice_enabled: bool,
    /// Pending order mode for the next right-click command.
    pub(crate) queued_order_mode: crate::app::types::OrderMode,
    /// Control group slots (0-9) storing stable entity ids.
    pub(crate) control_groups: Vec<Vec<u64>>,
    /// Slot and wall-clock instant of the last plain control-group recall, for
    /// the 800 ms double-tap that centres the camera. Wall clock, never sim
    /// state: the original stamps `timeGetTime()` here and only a recall writes
    /// it — assigning with Ctrl+digit never does.
    pub(crate) last_control_group_press: Option<(usize, std::time::Instant)>,
    /// The object the camera is following, i.e. `DisplayClass +0x11A0` behind
    /// its valid byte `+0x119C`. `None` is the cleared pair.
    pub(crate) follow_target: Option<u64>,
    /// Mutually-exclusive cursor-on-tactical-map targeting mode (building
    /// placement OR superweapon). Right-click clears; Options preserves it. Arming one
    /// kind clears the other.
    pub(crate) targeting_mode: Option<crate::app::types::TargetingMode>,
    /// Current placement preview for the armed building, if any.
    pub(crate) building_placement_preview: Option<crate::sim::production::BuildingPlacementPreview>,
    saved_building_placement: Option<SavedBuildingPlacement>,
}

#[cfg(test)]
#[path = "ready_building_placement_tests.rs"]
mod ready_building_placement_tests;

impl MatchInputState {
    /// Construct the match input owner with the ordinary startup defaults.
    pub(crate) fn new(hotkey_bindings: crate::app::input::hotkeys::HotkeyBindings) -> Self {
        Self {
            cursor_visibility: Default::default(),
            minimap_dragging: false,
            selection_state: crate::sim::selection::SelectionState::new(),
            selection_order: Vec::new(),
            selection_order_pending: false,
            selection_voice_enabled: true,
            queued_order_mode: crate::app::types::OrderMode::Move,
            control_groups: vec![Vec::new(); CONTROL_GROUP_SLOTS],
            last_control_group_press: None,
            follow_target: None,
            targeting_mode: None,
            building_placement_preview: None,
            saved_building_placement: None,
            camera_x: 0.0,
            camera_y: 0.0,
            pending_camera_scroll: Default::default(),
            zoom_level: 1.0,
            zoom_target: 1.0,
            zoom_anchor_world: [0.0, 0.0],
            zoom_anchor_screen: [0.0, 0.0],
            edge_scroll: crate::app::input::camera::EdgeScrollState::default(),
            tactical_mouse: crate::app::input::camera::TacticalMouseState::default(),
            view_bookmarks: crate::app::input::camera::ViewBookmarks::default(),
            cursor_x: 0.0,
            cursor_y: 0.0,
            cursor_zoom: None,
            keys_held: HashSet::new(),
            hotkey_bindings,
            hotkey_modifiers: ModifiersState::empty(),
            type_select: crate::app::types::TypeSelectInputState::default(),
            category_navigation: Default::default(),
            cursor_coordinates: false,
            retail_screenshot_requested: false,
        }
    }

    /// The cursor in whole window pixels, rounded to nearest: the position the
    /// shell screens and the gadget layer hit-test with.
    pub(crate) fn cursor_px(&self) -> (i32, i32) {
        (self.cursor_x.round() as i32, self.cursor_y.round() as i32)
    }

    /// HouseClass::Manual_Place4FB852..4FB859 refuses every new entry while
    /// a building placement is pending. SelectClass has already played the
    /// click sound and queried the actual held object's builder.
    /// Source: tools/sidebar_oracle/cameo_order.md, ready-building histories.
    /// The caller clears selection and repair/sell only on first entry.
    pub(crate) fn begin_building_placement(
        &mut self,
        type_id: &str,
        product_id: u64,
        owner: InternedId,
    ) -> bool {
        let armed = self
            .targeting_mode
            .as_ref()
            .and_then(crate::app::types::TargetingMode::as_building_placement)
            .is_some();
        if armed {
            false
        } else {
            self.targeting_mode = Some(crate::app::types::TargetingMode::BuildingPlacement(
                BuildingPlacementTarget {
                    type_id: type_id.to_string(),
                    product_id: Some(product_id),
                    owner,
                },
            ));
            self.building_placement_preview = None;
            true
        }
    }

    /// Display4AAD30's building rung and Select6AADC9's own-factory prefix.
    /// Neither consumes the House factory's completed object. A selected
    /// superweapon has its own input owner and is outside this building rung.
    pub(crate) fn cancel_building_placement(&mut self) -> bool {
        // Both native client rungs require current product, not current type.
        // An expired product can leave an orphan type/foundation pending.
        if self.building_placement_issuer().is_none() {
            return false;
        }
        self.clear_building_placement()
    }

    /// Explicit House placement/abandon tails clear the type/foundation even
    /// when pointer expiry has already cleared the current product.
    fn clear_building_placement(&mut self) -> bool {
        if self
            .targeting_mode
            .as_ref()
            .and_then(crate::app::types::TargetingMode::as_building_placement)
            .is_none()
        {
            return false;
        }
        self.targeting_mode = None;
        self.building_placement_preview = None;
        true
    }

    /// BandBoxLeftUp4AB9B0 requires the current product, independently of
    /// current type/foundation. Product-only expiry leaves those two intact.
    pub(crate) fn building_placement_issuer(&self) -> Option<InternedId> {
        let Some(crate::app::types::TargetingMode::BuildingPlacement(target)) =
            &self.targeting_mode
        else {
            return None;
        };
        target.product_id.map(|_| target.owner)
    }

    /// DispatchPointerExpired7258D0 clears a matching current product
    /// without dropping its type or active foundation. Alive/Health is not
    /// this notification. Source: cameo_order.md product-only expiry history.
    pub(crate) fn building_placement_pointer_expired(&mut self, stable_id: u64) {
        if let Some(crate::app::types::TargetingMode::BuildingPlacement(target)) =
            &mut self.targeting_mode
            && target.product_id == Some(stable_id)
        {
            target.product_id = None;
        }
        // TacticalPointerExpired6DA57D..6DA5A5 drops the complete saved
        // transaction when its product matches, independently of current.
        if self
            .saved_building_placement
            .as_ref()
            .is_some_and(|saved| saved.target.product_id == Some(stable_id))
        {
            self.saved_building_placement = None;
        }
    }

    /// BandBoxLeftUp4ABBF5..4ABC5D transfers current placement to the one
    /// saved transaction and drops the active cursor even if OutList is full.
    pub(crate) fn submit_building_placement(&mut self) {
        let mode = self.targeting_mode.take();
        match mode {
            Some(crate::app::types::TargetingMode::BuildingPlacement(target)) => {
                self.saved_building_placement = Some(SavedBuildingPlacement {
                    target,
                    preview: self.building_placement_preview.take(),
                });
            }
            other => self.targeting_mode = other,
        }
    }

    /// HousePlace4FB395..4FB49A restores the saved transaction, including an
    /// empty one left by an earlier success, and consumes the old backup.
    /// The caller recomputes the active preview from the actual cursor through
    /// the existing placement-preview owner after a nonempty restoration.
    pub(crate) fn retry_building_placement(&mut self) -> bool {
        self.clear_building_placement();
        let Some(saved) = self.saved_building_placement.take() else {
            return false;
        };
        self.targeting_mode = Some(crate::app::types::TargetingMode::BuildingPlacement(
            saved.target,
        ));
        self.building_placement_preview = saved.preview;
        true
    }

    /// HousePlace4FB328..4FB33F clears both current and saved building
    /// placement after successful Unlimbo and BuildingSlam.
    pub(crate) fn finish_building_placement(&mut self) {
        self.clear_building_placement();
        self.saved_building_placement = None;
    }

    /// Consume the ordered native lifecycle facts through this input owner.
    /// Returning true requests an actual-cursor preview refresh after retry.
    pub(crate) fn apply_building_placement_output(
        &mut self,
        output: &crate::sim::world::LifecycleOutput,
        local_house: Option<InternedId>,
    ) -> bool {
        use crate::sim::world::LifecycleOutput;
        match *output {
            LifecycleOutput::BuildingPlacementRetry { owner } if local_house == Some(owner) => {
                return self.retry_building_placement();
            }
            LifecycleOutput::BuildingPlacementSucceeded { owner } if local_house == Some(owner) => {
                self.finish_building_placement();
            }
            LifecycleOutput::BuildingProductionAbandoned { owner }
                if local_house == Some(owner) =>
            {
                self.clear_building_placement();
            }
            LifecycleOutput::ObjectPointerExpired { stable_id } => {
                self.building_placement_pointer_expired(stable_id);
            }
            _ => {}
        }
        false
    }

    /// New-scenario installation discards references into the outgoing world.
    pub(crate) fn reset_targeting_for_new_world(&mut self) {
        self.targeting_mode = None;
        self.building_placement_preview = None;
        self.saved_building_placement = None;
    }

    pub(crate) fn building_placement_view(&self) -> BuildingPlacementInputView<'_> {
        BuildingPlacementInputView {
            current: self.targeting_mode.as_ref().and_then(|mode| match mode {
                crate::app::types::TargetingMode::BuildingPlacement(target) => Some(target),
                _ => None,
            }),
            saved_retry: self
                .saved_building_placement
                .as_ref()
                .map(|saved| &saved.target),
            current_preview_present: self.building_placement_preview.is_some(),
            saved_preview_present: self
                .saved_building_placement
                .as_ref()
                .is_some_and(|saved| saved.preview.is_some()),
        }
    }

    /// Deactivate683EB0/4AEAD0's drag/capture teardown: the tactical capture
    /// byte, the band rectangle and the minimap gesture. Armed cursor modes
    /// (building placement, superweapon targeting, repair/sell) and physical
    /// held-button state survive this call — the pause overlay cancels a drag,
    /// never a pending placement.
    pub(crate) fn cancel_tactical_drag(&mut self) {
        self.minimap_dragging = false;
        self.selection_state.cancel_drag();
        self.tactical_mouse.cancel_mouse_drag();
    }

    /// TAction46 clears the active display modes before Lock684240 cancels
    /// the tactical drag through6938C0/4AEAD0/6DA160. Selection membership,
    /// camera requests/focus and physical held-key state survive this call.
    pub(crate) fn cancel_tactical_gesture_and_targeting(&mut self) {
        self.targeting_mode = None;
        self.building_placement_preview = None;
        self.cancel_tactical_drag();
    }

    /// KeyboardClear54F720 drains accepted input and Lock/Unlock clear the
    /// command latch. Edges are synchronous in this app, so no second ring
    /// exists; discard only app command latches. Physical key state remains
    /// owned by ordinary press/release delivery, not this bounded receipt.
    pub(crate) fn clear_command_latches(&mut self) {
        self.type_select.clear_held();
        self.queued_order_mode = crate::app::types::OrderMode::Move;
    }

    /// Capture the app-owned portion of a save at the same boundary as its
    /// Simulation. The existing selection owner decides whether its local
    /// order is provisional; this does not drain commands or alter selection.
    pub(crate) fn capture_saved_local_view(
        &self,
        simulation: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        sidebar: &crate::ui::sidebar::gadget_flash::SidebarGadgetState,
    ) -> Result<SavedLocalView, SavedLocalViewError> {
        let (order, pending) = crate::app::input::dispatch::selection_order_for_world(
            simulation,
            Some(rules),
            &self.selection_order,
            self.selection_order_pending,
        );
        assert_eq!(self.control_groups.len(), CONTROL_GROUP_SLOTS);
        // Native membership lives on each serialized Techno, not in a list
        // retaining already-deleted objects. Project the existing app lists
        // onto the same entity store being saved, preserving member order.
        let groups = std::array::from_fn(|slot| {
            self.control_groups[slot]
                .iter()
                .copied()
                .filter(|id| simulation.entities().get(*id).is_some())
                .collect()
        });
        let alive = |id: &u64| simulation.entities().get(*id).is_some();
        let targeting = self.targeting_mode.as_ref().map(|mode| match mode {
            crate::app::types::TargetingMode::BuildingPlacement(target) => {
                SavedTargeting::BuildingPlacement {
                    type_id: target.type_id.clone(),
                    product_id: target.product_id.filter(alive),
                    owner: simulation.interner.resolve(target.owner).to_owned(),
                }
            }
            crate::app::types::TargetingMode::SuperWeapon(name) => {
                SavedTargeting::SuperWeapon(name.clone())
            }
        });
        SavedLocalView::new(
            self.view_bookmarks.saved_cells(),
            order,
            pending,
            groups,
            self.follow_target.filter(alive),
            SavedCursorModes {
                repair: sidebar.repair_mode_on,
                sell: sidebar.sell_mode_on,
                targeting,
            },
        )
    }
}

#[cfg(test)]
#[path = "saved_local_view_tests.rs"]
mod saved_local_view_tests;
