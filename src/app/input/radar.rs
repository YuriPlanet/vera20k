//! The radar's mouse input: `RadarClass::Action @ 0x006539D0`.
//!
//! The handler takes every left or right event over the online radar's image
//! (`0x006539E6..0x00653A83`) except a held left button (flags `0x22`; the
//! map gadget's mask `0xDF` drops RIGHTHELD, `0x00652D79`). It picks
//! the radar pixel with `GetObjectAtRadarPixel @ 0x00656750`
//! ([`crate::render::radar::NativeRadarInputTarget`]) and does nothing for
//! the empty Cell (0,0) that pick starts from (`0x00653AC6..0x00653AD6`).
//!
//! A left event then asks one action ([`action`]): the armed superweapon's,
//! by its `[SuperWeaponTypes]` index (`[0x008809A0]`,
//! `0x00653B3A..0x00653C63`), or else, with something selected, the
//! selection's best object's What_Action on the picked Techno or Cell
//! (`0x00653C6A..0x00653CA1`). A keep table drops most actions
//! ([`RadarAction`]), and in Planning Mode a selection that may not take
//! waypoints has none (`0x00653CC0..0x00653CD1`).
//!
//! With an action, hovering draws its cursor ([`cursor`], `0x00653CFF`), a
//! press does nothing, and the release hands the pick and the action to
//! `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` (`0x00653D58`), the tactical
//! click's port ([`crate::app::input::dispatch::band_box_left_up`]), without
//! moving the view. Without one the cursor is the plain arrow mini
//! (`0x00653D6A`) and a press recentres the view on the pick
//! (`0x00653D7A..0x00653EA0`). A right press always recentres; a right
//! release does nothing (`0x00653B84..0x00653B8B`, `0x00653D6F`).
//!
//! RESIDUAL: the release is not refused in a frame whose band-box release or
//! cancelled drag set `[0x00A8ED9D]` (`0x00653D0D`; set at `0x004ABD0E` and
//! `0x004AEAF0`, cleared by `Main_Tick` at `0x0055D903`). VERA routes a release
//! to the one gadget region holding the press, and a cancelled gesture clears
//! the radar press it would need, so no release reaches both. Player effect:
//! none known. Downstream risk: none.

use crate::app::AppState;
use crate::app::input::commands::PlaceAction;
use crate::app::input::cursor::ActionQuery;
use crate::app::input::dispatch::LeftUp;
use crate::app::input::entity_pick::HoverObject;
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::render::radar::NativeRadarInputTarget;
use winit::event::MouseButton;

/// An action `RadarClass::Action` keeps. Its keep table (`0x00653F04`,
/// indexed by action - 1, `0x00653CA3..0x00653CBC`) keeps Move, NoMove,
/// Enter, Attack, Harvest, Capture, Sabotage, Nuke, GuardArea and the
/// LightningStorm, ParaDrop, AmerParaDrop, PsychicDominator, SpyPlane and
/// PsychicReveal targeting actions, and drops every other one like Action0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadarAction {
    /// Move (1).
    Move,
    /// Move or Attack while the attack-move chord or mode is active, which
    /// the cursor draws as AttackMoveNav/AttackMoveTar (0x3E/0x3F,
    /// `0x0070F0B0`).
    AttackMove,
    /// NoMove (2).
    NoMove,
    /// Enter (3) or Capture (9).
    Enter,
    /// Attack (5).
    Attack,
    /// Harvest (6).
    Harvest,
    /// Sabotage (0x10).
    Sabotage,
    /// GuardArea (0x1A).
    GuardArea,
    /// The armed superweapon's.
    SuperWeapon(RadarSuperWeapon),
    /// Ares: an action whose cursor override `0x653CA6` let through, with the
    /// cursors 10057F10 draws over a mapped and a shrouded Cell
    /// ([`crate::app::input::ares_cursor::radar_action`]).
    Ares { main: i32, shrouded: i32 },
    /// Ares: the armed superweapon's `Cursor` or `NoCursor` (hook `0x653B3A`),
    /// which launches only where it may fire.
    AresSuperWeapon { cursor: i32, launch: bool },
}

impl RadarAction {
    /// The keep table for the selection's action as the cursor's What_Action
    /// port answers it ([`ActionQuery::selection_action`]).
    pub(crate) fn kept(kind: CursorFeedbackKind) -> Option<Self> {
        use CursorFeedbackKind as Kind;
        Some(match kind {
            Kind::Move => Self::Move,
            Kind::AttackMove => Self::AttackMove,
            Kind::Invalid => Self::NoMove,
            Kind::Enter => Self::Enter,
            Kind::EnemyUnit | Kind::EnemyStructure | Kind::EnemyOutOfRange => Self::Attack,
            Kind::Harvest => Self::Harvest,
            Kind::Demolish => Self::Sabotage,
            Kind::GuardArea => Self::GuardArea,
            // Action0 and AttackSupport (0x3B), Self (4), Select (7),
            // ToggleSelect (8), Grind (0xB), Tote (0x11), Damage (0x1C), GRepair (0x1D),
            // NoDeploy (0x1E), NoEnter (0x1F), NoGRepair (0x20), IvanBomb
            // (0x35), NoIvanBomb (0x36), Detonate (0x37), DisarmBomb (0x39),
            // Demolish (0x40) and Airstrike (0x47).
            Kind::Default
            | Kind::Deploy
            | Kind::Select
            | Kind::FriendlyUnit
            | Kind::FriendlyStructure
            | Kind::Grind
            | Kind::EngineerDamage
            | Kind::RepairAction(_)
            | Kind::NoDeploy
            | Kind::NoEnter
            | Kind::Tote
            | Kind::IvanBomb
            | Kind::NoIvanBomb
            | Kind::DisarmBomb
            | Kind::Detonate
            | Kind::WeaponDemolish
            | Kind::AirStrike => return None,
            // The cursor's own modes, never a What_Action answer.
            Kind::PlaceValid
            | Kind::PlaceInvalid
            | Kind::Scroll(_)
            | Kind::ScrollBlocked(_)
            | Kind::Pan
            | Kind::PanBlocked(_)
            | Kind::Minimap(_)
            | Kind::RepairMode(_)
            | Kind::SellMode(_)
            | Kind::PlaceBeacon
            | Kind::SuperWeaponTarget(_)
            | Kind::AresAction(_)
            | Kind::SellUnit => return None,
        })
    }

    /// `DisplayClass::SetCursorFromAction @ 0x004AAE90` with `mini = 1`
    /// (`0x00653CF0`): the action's row in the main switch (jump table
    /// `0x004AB884`, `tools/mouse_oracle/cursor_table.json`) or, over a Cell
    /// the player has not mapped, the shrouded one (byte table `0x004AB838`,
    /// jump table `0x004AB7D0`, entered at `0x004AB35A`), drawn with the row's
    /// mini set. Row 52 has none and keeps its main set (`0x005BDCA3`).
    /// `lead_moves_to_shroud` is whether the first selected object's type has
    /// MoveToShroud (`0x004AB3EC..0x004AB42B`).
    pub(crate) fn cursor(self, shrouded: bool, lead_moves_to_shroud: bool) -> CursorId {
        match self {
            // Row 18 in both switches.
            Self::Move => CursorId::MinimapMove,
            // 0x3E/0x3F take row 71 (`0x00731CB0`) in both; its mini set is
            // rows 20/21's.
            Self::AttackMove => CursorId::MinimapAttack,
            Self::NoMove if shrouded && lead_moves_to_shroud => CursorId::MinimapMove,
            Self::NoMove => CursorId::MinimapNoMove,
            // Rows 20/21; row 18 shrouded.
            Self::Attack if shrouded => CursorId::MinimapMove,
            Self::Attack => CursorId::MinimapAttack,
            // Rows 21, 25 and 52; row 0 shrouded.
            Self::Harvest | Self::Enter | Self::Sabotage if shrouded => CursorId::DefaultMini,
            Self::Harvest => CursorId::MinimapAttack,
            Self::Enter => CursorId::MinimapEnter,
            Self::Sabotage => CursorId::Demolish,
            // Row 22 (`0x00731CC0`) in both.
            Self::GuardArea => CursorId::MinimapGuardArea,
            Self::SuperWeapon(super_weapon) => super_weapon.cursor(),
            Self::Ares {
                main,
                shrouded: under_shroud,
            } => crate::app::input::ares_cursor::cursor_id(
                if shrouded { under_shroud } else { main },
                true,
            ),
            Self::AresSuperWeapon { cursor, .. } => {
                crate::app::input::ares_cursor::cursor_id(cursor, true)
            }
        }
    }
}

/// A superweapon targeting action the radar keeps, by the armed type's
/// `[SuperWeaponTypes]` index ([`armed_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadarSuperWeapon {
    /// Nuke (0x14), index 0.
    Nuke,
    /// LightningStorm (0x26), index 2.
    LightningStorm,
    /// ParaDrop (0x29), index 5.
    ParaDrop,
    /// AmerParaDrop (0x41), index 6.
    AmerParaDrop,
    /// PsychicDominator (0x42), index 7.
    PsychicDominator,
    /// SpyPlane (0x43), index 8.
    SpyPlane,
    /// PsychicReveal (0x48), index 11.
    PsychicReveal,
}

impl RadarSuperWeapon {
    /// The action's name in the ActionType table (`0x007E4C50`), which
    /// `CCINIClass::ReadAction @ 0x00474EE0` matches `Action=` against.
    pub(crate) const fn action_name(self) -> &'static str {
        match self {
            Self::Nuke => "Nuke",
            Self::LightningStorm => "LightningStorm",
            Self::ParaDrop => "ParaDrop",
            Self::AmerParaDrop => "AmerParaDrop",
            Self::PsychicDominator => "PsychicDominator",
            Self::SpyPlane => "SpyPlane",
            Self::PsychicReveal => "PsychicReveal",
        }
    }

    /// The targeting row's mini set: rows 53, 50, 47, 83, 85 and 84 in both
    /// switches.
    const fn cursor(self) -> CursorId {
        match self {
            Self::Nuke => CursorId::NukeMini,
            Self::LightningStorm => CursorId::LightningStormMini,
            Self::ParaDrop | Self::AmerParaDrop => CursorId::ParadropMini,
            Self::PsychicDominator => CursorId::PsychicDominatorMini,
            Self::SpyPlane => CursorId::SpyPlaneMini,
            Self::PsychicReveal => CursorId::PsychicRevealMini,
        }
    }
}

/// What the armed superweapon's index answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Armed {
    Kept(RadarSuperWeapon),
    /// IronCurtain (0x25, index 1) and GeneticConverter (0x44, index 9),
    /// which the keep table drops.
    Dropped,
    /// An index the map does not list (ChronoSphere 3, ChronoWarp 4,
    /// ForceShield 10 and any later one): the selection answers instead.
    Unlisted,
}

/// RadarClass::Action's fixed map from the armed `[SuperWeaponTypes]` index
/// to an action (`0x00653B3A..0x00653C63`). It reads the index, not the
/// type's `Action=`.
const fn armed_action(index: usize) -> Armed {
    match index {
        0 => Armed::Kept(RadarSuperWeapon::Nuke),
        2 => Armed::Kept(RadarSuperWeapon::LightningStorm),
        5 => Armed::Kept(RadarSuperWeapon::ParaDrop),
        6 => Armed::Kept(RadarSuperWeapon::AmerParaDrop),
        7 => Armed::Kept(RadarSuperWeapon::PsychicDominator),
        8 => Armed::Kept(RadarSuperWeapon::SpyPlane),
        11 => Armed::Kept(RadarSuperWeapon::PsychicReveal),
        1 | 9 => Armed::Dropped,
        _ => Armed::Unlisted,
    }
}

/// RadarClass::Action's action at the picked `cell` and `object`, with
/// `armed` the armed superweapon's index; `None` when it has none.
///
/// Without a listed superweapon the selection's best object answers
/// (`SelectBestObjectForAction @ 0x005353D0`): its What_Action on the picked
/// Techno (vt+0x74) or on the Cell (vt+0x70, mouse-over 0), shrouded or not:
/// with Ctrl an armed building's Attack stays over shroud (`0x00447540`),
/// which the shrouded switch draws with the Move mini (`0x004AB35A`).
pub(crate) fn action(
    query: &ActionQuery<'_>,
    armed: Option<usize>,
    cell: (u16, u16),
    object: Option<HoverObject>,
) -> Option<RadarAction> {
    // Ares RadarClass_GetMouseAction_CustomSWAction 10034D00 (hook
    // 0x653B3A) asks a handled superweapon before the index map.
    let ares_super_weapon = armed.and_then(|armed| {
        let vanilla = match armed_action(armed) {
            Armed::Kept(super_weapon) => Some(RadarAction::SuperWeapon(super_weapon)),
            Armed::Dropped | Armed::Unlisted => None,
        };
        crate::app::input::ares_cursor::radar_super_weapon(query, armed, cell, vanilla)
    });
    let action = match armed.map_or(Armed::Unlisted, armed_action) {
        _ if ares_super_weapon.is_some() => ares_super_weapon.flatten(),
        Armed::Kept(super_weapon) => Some(RadarAction::SuperWeapon(super_weapon)),
        Armed::Dropped => None,
        Armed::Unlisted if query.selected.is_empty() => None,
        Armed::Unlisted => query.selection_action(cell, object).and_then(|kind| {
            // Ares RadarClass_GetMouseAction_AllowMinimap 100583F0 (hook
            // 0x653CA6).
            crate::app::input::ares_cursor::radar_action(query, kind, RadarAction::kept(kind))
        }),
    };
    // Planning Mode (`0x00637AA0`) wants every selected object to take
    // waypoints (`0x00639DA0`).
    if query.planning
        && query
            .sim
            .planning_selection_refusal(query.selected)
            .is_some()
    {
        return None;
    }
    action
}

/// The radar action for the pixel under the cursor, as [`begin_context_press`],
/// [`finish_order_press`] and [`cursor`] ask it.
struct Hover {
    target: NativeRadarInputTarget,
    action: Option<RadarAction>,
    /// Whether the player has not mapped the pick's Cell
    /// (`MapClass::IsShrouded @ 0x00586360`, `0x00653B1E`).
    shrouded: bool,
    lead_moves_to_shroud: bool,
}

/// The radar pixel under the cursor and its action; `None` off the radar
/// image, with the radar offline, or on the empty Cell (0,0).
fn hover(state: &AppState) -> Option<Hover> {
    if !crate::app::presentation::sidebar_render::is_cursor_over_minimap(state) {
        return None;
    }
    let target = crate::app::presentation::sidebar_render::minimap_input_target(state)?;
    if target.cell == (0, 0) {
        return None;
    }
    let sim = &state.match_state.sim_runtime.runtime()?.simulation;
    let owner = crate::app::input::commands::preferred_local_owner_name(state)?;
    let object = target
        .tracker_hit
        .and_then(|id| crate::app::input::entity_pick::hover_object_by_id(sim, id, &owner));
    let selected = crate::app::input::dispatch::selected_stable_ids_in_order(
        Some(sim),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    // The Cell's What_Action without mouse-over (`0x00653C9A`).
    let query = ActionQuery::new(state, sim, &selected, false);
    let armed = state
        .armed_super_weapon_type()
        .and_then(|section| state.rules()?.super_weapon_index(section));
    Some(Hover {
        target,
        action: action(&query, armed, target.cell, object),
        shrouded: query.cell_shrouded(target.cell),
        lead_moves_to_shroud: query.lead_moves_to_shroud(),
    })
}

/// What `RadarClass::Action @ 0x006539D0` writes to the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadarCursor {
    /// It writes none, and the last cursor stays: nothing else's region
    /// covers the map gadget (`0x00B04A10`).
    Kept,
    /// The normal arrow off the image (`SetCursor(0, false)`, `0x00653EE2`).
    Arrow,
    /// The action's row mini set on LEFTUP (`0x00653CFF`), or row 0's without
    /// one (`SetDefaultCursor(0, true)`, `0x00653D6A`).
    Mini(CursorId),
}

/// The radar's cursor, or `None` off its map gadget (the aperture). A press
/// never sticks to the gadget (`GadgetClass::Action` gets flags 0,
/// `0x00653ECB`), so a press dragged off it leaves the cursor to what lies
/// there.
pub(crate) fn cursor(state: &AppState) -> Option<RadarCursor> {
    use crate::app::presentation::sidebar_render as sidebar;
    if !sidebar::is_cursor_over_radar_gadget(state) {
        return None;
    }
    Some(written_cursor(
        state.match_state.input.tactical_mouse.left_held(),
        sidebar::minimap_shown(state),
        sidebar::is_cursor_over_minimap(state),
        || {
            hover(state).map(|hover| {
                hover.action.map_or(CursorId::DefaultMini, |action| {
                    action.cursor(hover.shrouded, hover.lead_moves_to_shroud)
                })
            })
        },
    ))
}

/// The gates of `RadarClass::Action` before its cursor write, in its order:
/// a held left button (`0x006539D8`), the radar not online
/// (`0x006539E6..0x00653A0A`), the image rect (`0x00653A53..0x00653A83`),
/// then the pick, whose empty Cell (0,0) writes nothing (`0x00653AC6`).
/// `pick` answers the cursor for the pixel, `None` on the empty Cell.
fn written_cursor(
    left_held: bool,
    online: bool,
    over_image: bool,
    pick: impl FnOnce() -> Option<CursorId>,
) -> RadarCursor {
    if left_held || !online {
        return RadarCursor::Kept;
    }
    if !over_image {
        return RadarCursor::Arrow;
    }
    pick().map_or(RadarCursor::Kept, RadarCursor::Mini)
}

/// Retain an order press on the online radar image until the same button
/// releases. Hit admission is Radar6539E6..653A83; MatchInputState owns the
/// pending button and selection-drag cancellation.
pub(crate) fn begin_order_press(state: &mut AppState, button: MouseButton) -> bool {
    if !crate::app::presentation::sidebar_render::is_cursor_over_minimap(state) {
        return false;
    }
    state.match_state.input.begin_minimap_press(button)
}

/// Begin the retail left-button context action, centring the camera on press
/// when there is no action. An admitted action waits for release.
pub(crate) fn begin_context_press(state: &mut AppState) {
    if begin_order_press(state, MouseButton::Left)
        && hover(state).is_none_or(|hover| hover.action.is_none())
    {
        centre_view_on_pick(state);
    }
}

/// Release the button owning a radar order: with an action,
/// `DisplayClass::BandBox_LeftUp` for the pick (`0x00653D1A..0x00653D58`).
/// Retail uses Left; VERA's alternative scheme also enters here with Right.
///
/// For a superweapon action its SPECIAL_PLACE names the first type whose
/// `Action=` is the listed index's action, not the armed type; with the Ares
/// superweapons on, the armed type for an Ares action or its own `Action=`,
/// else none ([`crate::app::input::commands::special_place_type`]).
pub(crate) fn finish_order_press(state: &mut AppState, button: MouseButton) {
    if !state.match_state.input.finish_minimap_press(button) {
        return;
    }
    let Some(Hover {
        target,
        action: Some(action),
        ..
    }) = hover(state)
    else {
        return;
    };
    // An Ares superweapon's NoCursor action (0x7E) reaches each selected
    // Foot's ClickedAction, which takes it and does nothing (hook 0x4D7524,
    // 10058390).
    if matches!(action, RadarAction::AresSuperWeapon { launch: false, .. }) {
        return;
    }
    let super_weapon = match action {
        RadarAction::SuperWeapon(super_weapon) => {
            Some(PlaceAction::Stock(super_weapon.action_name()))
        }
        RadarAction::AresSuperWeapon { .. } => Some(PlaceAction::Ares),
        _ => None,
    };
    crate::app::input::dispatch::band_box_left_up(
        state,
        LeftUp::Radar {
            target,
            super_weapon,
        },
    );
}

/// Centre the camera immediately on the radar pixel under the cursor,
/// independent of the selection's action. Retail binds this to right press.
pub(crate) fn centre_view_at_cursor(state: &mut AppState) {
    if crate::app::presentation::sidebar_render::is_cursor_over_minimap(state) {
        centre_view_on_pick(state);
    }
}

/// The view centres on the pick's clamped Cell (`0x00653D7A..0x00653EA0`),
/// except on the empty Cell (0,0) (`0x00653D84..0x00653D8C`).
fn centre_view_on_pick(state: &mut AppState) {
    if crate::app::presentation::sidebar_render::minimap_input_target(state)
        .is_some_and(|target| target.cell == (0, 0))
    {
        return;
    }
    crate::app::presentation::sidebar_render::update_camera_from_minimap_cursor(state);
}

#[cfg(test)]
#[path = "radar_tests.rs"]
mod tests;
