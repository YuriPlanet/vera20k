//! Unit selection system — tracks which entities the player has selected.
//!
//! Manages selection state: click-to-select single units, drag-to-select
//! groups, and the Selected marker component on entities. The render loop
//! reads Selected components to draw selection highlights.
//!
//! ## Design
//! - `Selected` is a zero-size marker component in sim/components.rs.
//! - `SelectionState` tracks the current drag rectangle (if any) and provides
//!   methods for selecting entities by screen-space rectangle or single click.
//! - Selection is player-side state, not part of the deterministic simulation.
//! - Drag-box excludes structures (RA2 convention: only mobile units).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components (Position, Owner, Selected, Category).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[cfg(test)]
use crate::sim::entity_store::EntityStore;

/// Drag phase tracks the two-stage selection state machine:
/// mouse-down sets `Pending`, reaching the effective 5px threshold activates
/// band-box.
#[derive(Debug, Clone, Copy)]
pub enum DragPhase {
    /// No drag in progress.
    None,
    /// Left mouse pressed but haven't moved far enough to activate band-box.
    Pending { start_x: f32, start_y: f32 },
    /// Band-box is active — rectangle is being drawn.
    BandBoxActive { start_x: f32, start_y: f32 },
}

/// Tracks the player's current selection drag state.
///
/// Two-phase state machine:
/// 1. `Pending` -- left mouse pressed, waiting for the effective 5px threshold.
/// 2. `BandBoxActive` -- threshold crossed, drawing the selection rectangle.
///
/// On release: `Pending` -> `Click`, `BandBoxActive` -> `BoxSelect`.
#[derive(Debug, Clone)]
pub struct SelectionState {
    /// Current drag phase.
    pub phase: DragPhase,
    /// Current mouse position during drag (updated on mouse move).
    /// Only meaningful when phase is `BandBoxActive`.
    pub drag_current: Option<(f32, f32)>,
}

/// Minimum effective drag distance (pixels) to activate band-box selection.
/// Native truncates the Euclidean distance and tests the integer result `> 4`,
/// so every distance below 5 stays pending and 5 activates.
const MIN_DRAG_DISTANCE: f32 = 5.0;

/// Returned by `update_drag` when the drag crosses the activation threshold.
///
/// Activation is presentation-only: the rectangle starts being drawn and
/// nothing else happens. The selection is replaced on the release, and only when
/// the rectangle caught something.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragTransition {
    /// No state change — still pending or already active.
    NoChange,
    /// Band-box just activated — the rectangle is now live.
    Activated,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionState {
    /// Create a new SelectionState with no active drag.
    pub fn new() -> Self {
        Self {
            phase: DragPhase::None,
            drag_current: None,
        }
    }

    /// Begin a selection drag at the given screen position.
    /// Sets phase to `Pending` — band-box won't activate until the mouse
    /// reaches 5 pixels from this point.
    pub fn begin_drag(&mut self, screen_x: f32, screen_y: f32) {
        self.phase = DragPhase::Pending {
            start_x: screen_x,
            start_y: screen_y,
        };
        self.drag_current = Some((screen_x, screen_y));
    }

    /// Update the current drag position (called on mouse move while dragging).
    ///
    /// If in `Pending` phase and the mouse has moved at least 5px from start,
    /// transitions to `BandBoxActive` and returns `DragTransition::Activated`
    /// so the caller knows the rectangle is now live.
    pub fn update_drag(&mut self, screen_x: f32, screen_y: f32) -> DragTransition {
        match self.phase {
            DragPhase::Pending { start_x, start_y } => {
                let dx = screen_x - start_x;
                let dy = screen_y - start_y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist >= MIN_DRAG_DISTANCE {
                    self.phase = DragPhase::BandBoxActive { start_x, start_y };
                    self.drag_current = Some((screen_x, screen_y));
                    DragTransition::Activated
                } else {
                    self.drag_current = Some((screen_x, screen_y));
                    DragTransition::NoChange
                }
            }
            DragPhase::BandBoxActive { .. } => {
                self.drag_current = Some((screen_x, screen_y));
                DragTransition::NoChange
            }
            DragPhase::None => DragTransition::NoChange,
        }
    }

    /// True if the player is currently in any drag state (pending or active).
    #[cfg(test)]
    pub fn is_dragging(&self) -> bool {
        !matches!(self.phase, DragPhase::None)
    }

    /// True if the band-box rectangle is actively being drawn.
    pub fn is_band_box_active(&self) -> bool {
        matches!(self.phase, DragPhase::BandBoxActive { .. })
    }

    /// Get the current drag rectangle in screen-space (min_x, min_y, max_x, max_y).
    /// Returns None if band-box is not active (only draws during BandBoxActive phase).
    pub fn drag_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let DragPhase::BandBoxActive { start_x, start_y } = self.phase else {
            return None;
        };
        let (cx, cy) = self.drag_current?;
        let min_x: f32 = start_x.min(cx);
        let min_y: f32 = start_y.min(cy);
        let max_x: f32 = start_x.max(cx);
        let max_y: f32 = start_y.max(cy);
        Some((min_x, min_y, max_x, max_y))
    }

    /// End the drag and determine if it was a box select or a click.
    /// `BandBoxActive` → `BoxSelect`, `Pending` → `Click`, `None` → `None`.
    pub fn end_drag(&mut self, screen_x: f32, screen_y: f32) -> SelectAction {
        let phase = self.phase;
        self.phase = DragPhase::None;
        self.drag_current = None;

        match phase {
            DragPhase::BandBoxActive { start_x, start_y } => {
                let min_x: f32 = start_x.min(screen_x);
                let min_y: f32 = start_y.min(screen_y);
                let max_x: f32 = start_x.max(screen_x);
                let max_y: f32 = start_y.max(screen_y);
                SelectAction::BoxSelect(min_x, min_y, max_x, max_y)
            }
            DragPhase::Pending { .. } => SelectAction::Click(screen_x, screen_y),
            DragPhase::None => SelectAction::None,
        }
    }

    /// Cancel any in-progress drag without selecting.
    pub fn cancel_drag(&mut self) {
        self.phase = DragPhase::None;
        self.drag_current = None;
    }
}

/// Result of ending a selection drag.
#[derive(Debug, Clone, Copy)]
pub enum SelectAction {
    /// No drag was active.
    None,
    /// Single click at screen position (x, y).
    Click(f32, f32),
    /// Box select with screen rectangle (min_x, min_y, max_x, max_y).
    BoxSelect(f32, f32, f32, f32),
}

/// Clear all Selected markers from the entity store.
///
/// Kept as the sim-side primitive for a wholesale unselect. The band-box path no
/// longer calls it: the native release replaces the selection through the
/// selection command instead, and an empty box clears nothing at all.
#[cfg(test)]
pub fn deselect_all(entities: &mut EntityStore) {
    let selected_ids: Vec<u64> = entities
        .values()
        .filter(|e| e.selected)
        .map(|e| e.stable_id())
        .collect();
    for id in selected_ids {
        if let Some(e) = entities.get_mut(id) {
            e.selected = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;

    fn spawn_unit(store: &mut EntityStore, id: u64, owner: &str) -> u64 {
        let mut interner = StringInterner::new();
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            interner.intern(owner),
            Health { current: 100 },
            interner.intern("E1"),
            EntityCategory::Unit,
            0,
            0,
            false,
        );
        store.insert(e);
        id
    }

    #[test]
    fn test_selection_state_click() {
        let mut sel: SelectionState = SelectionState::new();
        sel.begin_drag(100.0, 200.0);
        assert!(sel.is_dragging());
        let action: SelectAction = sel.end_drag(102.0, 201.0);
        assert!(matches!(action, SelectAction::Click(_, _)));
        assert!(!sel.is_dragging());
    }

    #[test]
    fn test_selection_state_box() {
        let mut sel: SelectionState = SelectionState::new();
        sel.begin_drag(100.0, 100.0);
        let transition = sel.update_drag(200.0, 200.0);
        assert_eq!(transition, DragTransition::Activated);
        assert!(sel.is_band_box_active());
        let action: SelectAction = sel.end_drag(200.0, 200.0);
        match action {
            SelectAction::BoxSelect(min_x, min_y, max_x, max_y) => {
                assert!((min_x - 100.0).abs() < 0.1);
                assert!((min_y - 100.0).abs() < 0.1);
                assert!((max_x - 200.0).abs() < 0.1);
                assert!((max_y - 200.0).abs() < 0.1);
            }
            _ => panic!("Expected BoxSelect"),
        }
    }

    #[test]
    fn test_drag_threshold_matches_native_truncated_distance() {
        let mut sel = SelectionState::new();
        sel.begin_drag(100.0, 100.0);
        let t = sel.update_drag(104.9, 100.0);
        assert_eq!(t, DragTransition::NoChange);
        assert!(!sel.is_band_box_active());
        let t = sel.update_drag(104.0, 103.0);
        assert_eq!(t, DragTransition::Activated);
        assert!(sel.is_band_box_active());
        let t = sel.update_drag(110.0, 110.0);
        assert_eq!(t, DragTransition::NoChange);
    }

    /// `MIN_DRAG_DISTANCE` claims the exact root and the native approximate one
    /// pick the same boundary. Prove it on the inputs that decide it rather than
    /// asserting it in prose.
    ///
    /// `DisplayClass__BandBox_MouseMove` 0x004AC380 activates on
    /// `ftol(Sqrt_Approx(dx² + dy²)) > 4`, and both deltas are integer mouse
    /// coordinates, so the sum is an integer. 25 is the only sum whose exact
    /// root is 5; its neighbours are 4.898 and 5.099, far outside the table's
    /// ~3e-5 relative error, so no integer sum can straddle the boundary
    /// differently under the two kernels.
    #[test]
    fn band_box_activation_boundary_agrees_with_the_native_truncated_root() {
        use crate::util::native_x87::distance_3d_leptons;

        // Exactly 25: the table returns 5.0 and ftol keeps 5, which passes `> 4`.
        for (dx, dy) in [(3, 4), (4, 3), (5, 0), (0, 5)] {
            assert_eq!(
                distance_3d_leptons([0, 0, 0], [dx, dy, 0]),
                5,
                "({dx},{dy}) must reach the activation threshold"
            );
        }
        // The integer sums immediately below stay at 4 and do not activate.
        for (dx, dy) in [(3, 3), (4, 2), (4, 0)] {
            assert!(
                distance_3d_leptons([0, 0, 0], [dx, dy, 0]) <= 4,
                "({dx},{dy}) must stay pending"
            );
        }

        // And the f32 form in `update_drag` agrees on the same inputs.
        let mut sel = SelectionState::new();
        sel.begin_drag(0.0, 0.0);
        assert_eq!(sel.update_drag(3.0, 4.0), DragTransition::Activated);
        let mut sel = SelectionState::new();
        sel.begin_drag(0.0, 0.0);
        assert_eq!(sel.update_drag(3.0, 3.0), DragTransition::NoChange);
    }

    #[test]
    fn test_drag_rect_only_when_active() {
        let mut sel = SelectionState::new();
        sel.begin_drag(100.0, 100.0);
        assert!(sel.drag_rect().is_none());
        sel.update_drag(200.0, 200.0);
        assert!(sel.drag_rect().is_some());
    }

    #[test]
    fn test_deselect_all() {
        let mut store = EntityStore::new();
        let e1 = spawn_unit(&mut store, 1, "Americans");
        store.get_mut(e1).unwrap().selected = true;
        assert!(store.get(e1).unwrap().selected);
        deselect_all(&mut store);
        assert!(!store.get(e1).unwrap().selected);
    }

    #[test]
    fn test_cancel_drag() {
        let mut sel: SelectionState = SelectionState::new();
        sel.begin_drag(100.0, 100.0);
        assert!(sel.is_dragging());
        sel.cancel_drag();
        assert!(!sel.is_dragging());
        let action: SelectAction = sel.end_drag(200.0, 200.0);
        assert!(matches!(action, SelectAction::None));
    }
}
