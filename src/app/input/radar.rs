//! The radar's mouse input: `RadarClass::Action @ 0x006539D0`.
//!
//! The handler takes every left or right event over the online radar's image
//! (`0x006539E6..0x00653A83`) except a held button (flags `0x22`). It picks
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
use crate::app::input::cursor::ActionQuery;
use crate::app::input::dispatch::LeftUp;
use crate::app::input::entity_pick::HoverObject;
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::render::radar::NativeRadarInputTarget;

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
            // ToggleSelect (8), Grind (0xB), Damage (0x1C), GRepair (0x1D),
            // NoDeploy (0x1E), NoEnter (0x1F), NoGRepair (0x20), IvanBomb
            // (0x35), NoIvanBomb (0x36), DisarmBomb (0x39), Demolish (0x40)
            // and Airstrike (0x47).
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
            | Kind::IvanBomb
            | Kind::NoIvanBomb
            | Kind::DisarmBomb
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
            | Kind::SuperWeaponTarget(_) => return None,
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
    #[cfg(test)]
    const fn action_name(self) -> &'static str {
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
/// Techno (vt+0x74) or on the Cell (vt+0x70, mouse-over 0). A Cell the player
/// does not see offers no object to the cursor's port, which answers its
/// empty-cell ladder there ([`ActionQuery::hidden_cell`]).
pub(crate) fn action(
    query: &ActionQuery<'_>,
    armed: Option<usize>,
    cell: (u16, u16),
    object: Option<HoverObject>,
) -> Option<RadarAction> {
    let action = match armed.map_or(Armed::Unlisted, armed_action) {
        Armed::Kept(super_weapon) => Some(RadarAction::SuperWeapon(super_weapon)),
        Armed::Dropped => None,
        Armed::Unlisted if query.selected.is_empty() => None,
        Armed::Unlisted => match object {
            Some(object) => query.selection_action(cell, Some(object)),
            None if query.cell_visible(cell) => query.selection_action(cell, None),
            None => Some(query.hidden_cell(cell)),
        }
        .and_then(RadarAction::kept),
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

/// The radar action for the pixel under the cursor, as [`left_press`],
/// [`left_release`] and [`cursor`] ask it.
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
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let owner = crate::app::input::commands::preferred_local_owner_name(state)
        .unwrap_or_else(|| "Americans".to_string());
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
    let shrouded = !query.full_visibility
        && query
            .viewer
            .is_some_and(|viewer| sim.clicked_cell_is_shrouded(viewer, target.cell) == Ok(true));
    let lead_moves_to_shroud = selected.first().is_some_and(|&id| {
        sim.entities().get(id).is_some_and(|entity| {
            state
                .rules()
                .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
                .is_some_and(|object| object.move_to_shroud)
        })
    });
    Some(Hover {
        target,
        action: action(&query, armed, target.cell, object),
        shrouded,
        lead_moves_to_shroud,
    })
}

/// The radar's cursor: its action's row mini set, or row 0's (`0x00653D6A`).
///
/// RESIDUAL: gamemd sets it only on a left-up event, so a held press keeps
/// the cursor of the hover before it (held events exit at `0x006539D8`), and
/// the empty pick (0,0) leaves the previous cursor (`0x00653AC6`). VERA
/// redraws it from the pixel under the cursor and draws row 0's mini on
/// (0,0). Trigger: a held press dragged across the radar, or hovering its
/// off-map corners. Effect: the cursor shape only. Downstream risk: none.
pub(crate) fn cursor(state: &AppState) -> CursorId {
    hover(state)
        .and_then(|hover| {
            let action = hover.action?;
            Some(action.cursor(hover.shrouded, hover.lead_moves_to_shroud))
        })
        .unwrap_or(CursorId::DefaultMini)
}

/// A left press on the radar. It holds the gesture for its release and,
/// without an action, recentres the view.
pub(crate) fn left_press(state: &mut AppState) {
    if crate::app::presentation::sidebar_render::try_begin_minimap_drag(state)
        && hover(state).is_none_or(|hover| hover.action.is_none())
    {
        recentre(state);
    }
}

/// The left release of a radar press: with an action,
/// `DisplayClass::BandBox_LeftUp` for the pick (`0x00653D1A..0x00653D58`).
///
/// For a superweapon action its SPECIAL_PLACE names the first type whose
/// `Action=` is that action (`SuperWeaponTypeClass::FindFirstOfAction @
/// 0x006CEEB0`). Every listed retail index's type is that first type, so
/// VERA launches the armed one. RESIDUAL: a mod whose listed index holds a
/// type of another action launches that type here, where gamemd launches the
/// first type of the listed action, or nothing. Frequency: mods only.
/// Downstream risk: none in retail.
pub(crate) fn left_release(state: &mut AppState) {
    if !std::mem::take(&mut state.match_state.input.minimap_dragging) {
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
    crate::app::input::dispatch::band_box_left_up(
        state,
        LeftUp::Radar {
            target,
            super_weapon: matches!(action, RadarAction::SuperWeapon(_)),
        },
    );
}

/// A right press on the radar recentres the view, whatever the action.
pub(crate) fn right_press(state: &mut AppState) {
    if crate::app::presentation::sidebar_render::is_cursor_over_minimap(state) {
        recentre(state);
    }
}

/// The view centres on the pick's clamped Cell (`0x00653D7A..0x00653EA0`),
/// except on the empty Cell (0,0) (`0x00653D84..0x00653D8C`).
fn recentre(state: &mut AppState) {
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
