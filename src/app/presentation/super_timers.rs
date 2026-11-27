//! The timer lines `TacticalClass::Draw` writes at the bottom right of the
//! tactical view after the PixelFX sparkles (`0x006D4941..0x006D4B25`): the
//! Scenario timer, the Super timer list, then each house's blackout, each by
//! `TacticalClass::DrawTimer @ 0x006D4B50`.
//!
//! Native comparisons: the `tactical_timers` and `timer_lines` sections of
//! `tools/superweapon_oracle.py`.
//!
//! Text is drawn in the scheme's 8-bit colour, not packed to the native
//! RGB565 surface's.

use std::collections::HashMap;

use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::super_timer_groups::{TeamKey, TeamSuperRow, grouped_super_rows};
use crate::app::presentation::text_hud::{self, TextHud};
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::assets::csf_file::type_ui_name;
use crate::map::houses::is_allied_with;
use crate::render::batch::SpriteInstance;
use crate::rules::house_colors::{
    HouseColorIndex, NO_REMAP, RELATION_ALLY, RELATION_ENEMY, RELATION_SELF, house_text_rgb,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperTimerView;
use crate::ui::game_screen::GameScreen;

/// Frames to a timer line's second (`0x88888889`, `sar 3`: a truncating
/// divide).
pub(crate) const FRAMES_PER_SECOND: i32 = 15;
/// The blackout lines' label, read through the string table (`0x00842A50`,
/// `0x00734E60`).
const BLACKOUT_LABEL: &str = "MSG:BlackoutTimer";
/// The time ends this far in from the view's right (`0x006D4D1A`,
/// `0x006D4D75`).
const RIGHT_MARGIN: i32 = 3;
/// The blinking time's colour scheme, ColorScheme::Array[5] (`0x006D4CD9`);
/// runtime scheme R is `[Colors]` entry R / 2.
const BLINK_SCHEME: HouseColorIndex = HouseColorIndex(5 / 2);
/// How long the blink keeps each state, in clock ms (`0x006D4CAE`).
const BLINK_MS: u64 = 1000;

/// What a timer line counts down, which also picks its label, its colour
/// scheme and whether it blinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerSource {
    /// The Scenario timer (`+0x11E8`), labelled by `+0x11F4`, in the
    /// player's scheme.
    Scenario,
    /// A Super of the timer list, labelled by its type's `UIName=`
    /// (`Type+0x60`), in its owner's scheme; its line steps its blink
    /// (`+0x40`, `+0x48`).
    Super(SuperTimerView),
    /// A house's blackout (`+0x2A4`), labelled `MSG:BlackoutTimer`, in the
    /// house's scheme.
    Blackout(InternedId),
    /// VERA's `[VERA] GroupedSuperTimers=` line in place of the Super lines:
    /// one team's Supers of one type, labelled `<team> <UIName> ready/total`
    /// in the team's first house's scheme; it blinks like a Super's line.
    Team(TeamSuperRow),
}

/// One timer line; its index is its place in [`timer_lines`]' answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerLine {
    pub(crate) source: TimerSource,
    /// Whole seconds left: the frames left over 15, truncated.
    pub(crate) seconds: i32,
}

/// TacticalClass::Draw's timer lines from the bottom up
/// (`0x006D4941..0x006D4B25`): the Scenario timer while it runs
/// (`scenario_left`, None while its start is -1), each Super of the list in
/// its order, then each house with blackout time left, in `HouseClass::Array`
/// order. In GameMode 0 (`campaign`) a held Super whose timer has all of
/// `GetRechargeTime` left takes no line (`0x006D49D1..0x006D4A06`). With
/// Ares, `supers` holds only the Supers whose `SW.TimerVisibility` admits
/// the player (hook `0x6D49D1`, filtered by the caller).
pub(crate) fn timer_lines(
    scenario_left: Option<i32>,
    supers: &[SuperTimerView],
    blackouts: impl IntoIterator<Item = (InternedId, i32)>,
    campaign: bool,
) -> Vec<TimerLine> {
    let line = |source, frames: i32| TimerLine {
        source,
        seconds: frames / FRAMES_PER_SECOND,
    };
    let scenario = scenario_left.map(|left| line(TimerSource::Scenario, left));
    let supers = supers
        .iter()
        .filter(|view| {
            !(campaign && view.on_hold() && view.frames_left() == view.recharge_frames())
        })
        .map(|view| line(TimerSource::Super(*view), view.frames_left()));
    let blackouts = blackouts
        .into_iter()
        .filter(|&(_, left)| left > 0)
        .map(|(house, left)| line(TimerSource::Blackout(house), left));
    scenario
        .into_iter()
        .chain(supers)
        .chain(blackouts)
        .collect()
}

/// `0x006D4B50`'s time: `%02d:%02d`, or `%d:%02d:%02d` with hours
/// (`0x00842A80`, `0x00842AB0`).
pub(crate) fn time_text(seconds: i32) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours != 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// `0x006D4B50`'s label: `%s  ` (`0x00842ACC`).
pub(crate) fn label_text(label: &str) -> String {
    format!("{label}  ")
}

/// Where `0x006D4B50` prints one of a line's texts, in the tactical view's
/// pixels: its top left, and the box `0x004A5EB0` fills black behind it
/// first (x, y, width, height).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextPlace {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) fill: [i32; 4],
}

/// `0x006D4CE2..0x006D4D9A`: line `index` sits `index + 1` font heights plus
/// 2 above the view's bottom; the time ends 3 pixels in from the view's right
/// and the label ends where the time starts. Both print right-aligned, which
/// moves each left by its width, over a black box one pixel out on every side
/// (`0x004A5EB0`). Returns the label's place and the time's.
pub(crate) fn place_line(
    index: usize,
    view: [i32; 2],
    font_height: i32,
    label_width: i32,
    time_width: i32,
) -> (TextPlace, TextPlace) {
    let y = view[1] - (font_height + 2) * (index as i32 + 1);
    let place = |right: i32, width: i32| {
        let x = right - width;
        TextPlace {
            x,
            y,
            fill: [x - 1, y - 1, width + 2, font_height + 2],
        }
    };
    let time_right = view[0] - RIGHT_MARGIN;
    (
        place(time_right - time_width, label_width),
        place(time_right, time_width),
    )
}

/// A Super's timer blink (`+0x40`, `+0x48`). The app keeps it, by owner and
/// type, because it runs on the wall clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TimerBlink {
    /// `+0x40`: the time prints in the blink colour.
    on: bool,
    /// `+0x48`: the clock ms (`0x004093B0`) the state next flips at.
    deadline_ms: u64,
    /// The Super's list place. A new place is a new grant, and Grant clears
    /// `+0x40` (`0x006CB580`).
    place: u32,
}

impl TimerBlink {
    /// The Super's blink. A new grant, which a new list place shows, clears
    /// it (`+0x40`, Grant `0x006CB580`); the deadline stays.
    fn of<'a>(blinks: &'a mut TimerBlinks, view: &SuperTimerView) -> &'a mut TimerBlink {
        Self::of_key(blinks, view.owner(), view.sw_type(), view.place())
    }

    /// The blink of a grouped team line, by its first house and type. It has
    /// no grant place, so it keeps place 0 and a regrouping never clears it.
    fn of_team<'a>(blinks: &'a mut TimerBlinks, row: &TeamSuperRow) -> &'a mut TimerBlink {
        Self::of_key(blinks, row.representative, row.sw_type, 0)
    }

    fn of_key<'a>(
        blinks: &'a mut TimerBlinks,
        owner: InternedId,
        sw_type: InternedId,
        place: u32,
    ) -> &'a mut TimerBlink {
        let blink = blinks.entry((owner, sw_type)).or_insert(TimerBlink {
            place,
            ..TimerBlink::default()
        });
        if blink.place != place {
            blink.on = false;
            blink.place = place;
        }
        blink
    }

    /// `0x006D4C75..0x006D4CDF`: a Super's line with no seconds left steps
    /// the blink; once its deadline is reached the state flips and the next
    /// deadline is a second on. Returns whether the time prints in the blink
    /// colour.
    fn step(&mut self, seconds: i32, now_ms: u64) -> bool {
        if seconds != 0 {
            return false;
        }
        if self.deadline_ms <= now_ms {
            self.deadline_ms = now_ms.wrapping_add(BLINK_MS);
            self.on = !self.on;
        }
        self.on
    }
}

/// The Supers' timer blinks, by owner and type.
pub(crate) type TimerBlinks = HashMap<(InternedId, InternedId), TimerBlink>;

/// VERA presentation extension, on with `[VERA] CompactSuperTimers=`
/// (Options). No timer, grant, visibility or command state belongs here: this
/// only chooses which existing DrawTimer rows are shown. Off, every line
/// stacks up from the bottom as `0x006D4941..0x006D4B25` draws it.
#[derive(Debug, Default)]
pub(crate) struct TimerDock {
    expanded: bool,
    offset: usize,
    max_offset: usize,
    toggle_rect: Option<crate::ui::shell::geom::RectPx>,
    scroll_rows: Vec<crate::ui::shell::geom::RectPx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockRow {
    Line(usize),
    Toggle,
}

impl TimerDock {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    fn clear_hits(&mut self) {
        self.toggle_rect = None;
        self.scroll_rows.clear();
    }

    pub(crate) fn toggle_rect(&self) -> Option<crate::ui::shell::geom::RectPx> {
        self.toggle_rect
    }

    pub(crate) fn toggle(&mut self) {
        self.expanded = !self.expanded;
        self.offset = 0;
    }

    pub(crate) fn over_toggle(&self, x: i32, y: i32) -> bool {
        self.toggle_rect.is_some_and(|rect| rect.contains(x, y))
    }

    pub(crate) fn wheel(&mut self, x: i32, y: i32, delta: f32) -> bool {
        if !self.expanded
            || !delta.is_finite()
            || delta == 0.0
            || !self.scroll_rows.iter().any(|rect| rect.contains(x, y))
        {
            return false;
        }
        self.offset = if delta > 0.0 {
            self.offset.saturating_sub(1)
        } else {
            self.offset.saturating_add(1).min(self.max_offset)
        };
        true
    }

    /// Native line order and placement remain untouched while `compact` is
    /// off, and through six Supers. Beyond that, keep local Supers first and
    /// page the remaining ready-first rows. Scenario/blackout lines retain
    /// their original relative order.
    fn rows(
        &mut self,
        lines: &[TimerLine],
        local: Option<InternedId>,
        view: [i32; 2],
        font_height: i32,
        compact: bool,
    ) -> Vec<DockRow> {
        self.clear_hits();
        let supers: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| matches!(line.source, TimerSource::Super(_)).then_some(i))
            .collect();
        if !compact || supers.len() <= 6 {
            self.expanded = false;
            self.offset = 0;
            self.max_offset = 0;
            return (0..lines.len()).map(DockRow::Line).collect();
        }
        let available = (view[1].max(0) / (font_height + 2).max(1)) as usize;
        let room = available.saturating_sub(lines.len() - supers.len() + 1);
        let limit = room.min(if self.expanded { 10 } else { 6 });
        let mut local_rows: Vec<_> = supers
            .iter()
            .copied()
            .filter(
                |&i| matches!(lines[i].source, TimerSource::Super(sw) if Some(sw.owner()) == local),
            )
            .collect();
        local_rows.sort_by_key(|&i| (lines[i].seconds, i));
        local_rows.truncate(limit.min(6));
        local_rows.sort_unstable();
        let mut others: Vec<_> = supers
            .into_iter()
            .filter(|i| !local_rows.contains(i))
            .collect();
        others.sort_by_key(|&i| (lines[i].seconds, i));
        let page_size = limit.saturating_sub(local_rows.len());
        self.max_offset = others.len().saturating_sub(page_size);
        self.offset = if self.expanded {
            self.offset.min(self.max_offset)
        } else {
            0
        };
        let mut block: Vec<_> = others
            .iter()
            .skip(self.offset)
            .take(page_size)
            .rev()
            .map(|&i| DockRow::Line(i))
            .chain(local_rows.iter().rev().map(|&i| DockRow::Line(i)))
            .collect();
        if self.expanded {
            block.insert(0, DockRow::Toggle);
        } else {
            block.push(DockRow::Toggle);
        }
        let mut rows = Vec::new();
        let mut inserted = false;
        for (i, line) in lines.iter().enumerate() {
            if matches!(line.source, TimerSource::Super(_)) {
                if !inserted {
                    rows.append(&mut block);
                    inserted = true;
                }
            } else {
                rows.push(DockRow::Line(i));
            }
        }
        if let Some(index) = rows.iter().position(|row| *row == DockRow::Toggle) {
            let y = place_line(index, view, font_height, 0, 0).1.y;
            self.toggle_rect = Some(crate::ui::shell::geom::RectPx::new(
                view[0] - RIGHT_MARGIN - 16,
                y - 1,
                16,
                font_height + 2,
            ));
        }
        rows
    }
}

/// The cursor in the pixels of the timers' box, where the dock keeps its hit
/// areas: the battlefield's own pixels in the original interface.
pub(crate) fn cursor_in_view(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> (i32, i32) {
    let (x, y) = match_state.input.cursor_px();
    let [left, top, _, _] = crate::app::presentation::field_parts::rect(
        platform,
        renderer,
        screen,
        &match_state.match_presentation,
        persistence,
        crate::ui::field_layout::FieldPart::Timers,
    );
    (x - left, y - top)
}

/// Consume wheel messages only over the expanded timer ink, before world zoom
/// or sidebar scrolling. Empty space around the rows stays interactive terrain.
pub(crate) fn wheel(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    delta: f32,
) -> bool {
    let (x, y) = cursor_in_view(platform, renderer, screen, match_state, persistence);
    match_state
        .match_presentation
        .super_timer_dock
        .wheel(x, y, delta)
}

/// The colour a blinking time prints in: [`BLINK_SCHEME`], or with Phobos
/// `ColorScheme::Array[TimerBlinkColorScheme]` (`PrintTimerOnTactical_BlinkColor`
/// at `0x006D4CD9`). A `none` (-1) keeps the native scheme where Phobos would
/// read `Array[-1]`.
fn blink_rgb(rules: &RuleSet) -> [u8; 3] {
    #[cfg(feature = "phobos")]
    let scheme = usize::try_from(rules.general.timer_blink_color_scheme)
        .ok()
        .and_then(|scheme| u8::try_from(scheme / 2).ok())
        .map_or(BLINK_SCHEME, HouseColorIndex);
    #[cfg(not(feature = "phobos"))]
    let scheme = BLINK_SCHEME;
    house_text_rgb(&rules.color_schemes, scheme)
}

/// What the timer lines show this frame, worked out before they are drawn
/// ([`update_super_timers`]): each shown text in drawing order, and the
/// dock's toggle while it has one.
#[derive(Clone, Debug, Default)]
pub(crate) struct TimerFrame {
    /// A line's label, then its time: the text, where it prints and its
    /// colour.
    pub(crate) texts: Vec<(String, TextPlace, [u8; 3])>,
    /// The dock's toggle and whether the dock is open.
    pub(crate) toggle: Option<(crate::ui::shell::geom::RectPx, bool)>,
}

/// Each drawn frame's timer step, before its timers are drawn: the lines,
/// their places and colours ([`TimerFrame`]), the dock's rows and hit areas,
/// and each Super's blink, which steps on the wall clock. The drawing then
/// only reads what this left ([`build_super_timer_instances`]).
pub(crate) fn update_super_timers(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) {
    match_state
        .match_presentation
        .super_timer_frame
        .texts
        .clear();
    match_state.match_presentation.super_timer_frame.toggle = None;
    match_state.match_presentation.super_timer_dock.clear_hits();
    match_state.match_presentation.super_timer_next_flip_ms = None;
    use crate::app::presentation::field_parts;
    use crate::ui::field_layout::FieldPart;
    // A layout that leaves the timers out draws none and keeps no hit area.
    if frontend.screen != GameScreen::InGame
        || !field_parts::shows(&match_state.match_presentation, FieldPart::Timers)
    {
        return;
    }
    let (Some(runtime), Some(rules)) = (
        match_state.sim_runtime.presentation(),
        crate::app::state::active_rules(frontend, match_state),
    ) else {
        return;
    };
    let sim = &runtime.simulation;
    let frame = sim.session().binary_frame();
    let supers = crate::sim::superweapon::super_timer_views(sim, rules);
    // When the Rules were read the Ares way (`RuleSet::ares_form`), Ares hook
    // 0x6D49D1 (10035A80) asks SW.TimerVisibility at the head of each Super's
    // line test; a hidden Super takes no line.
    let supers: Vec<_> = if rules.ares_form() {
        supers
            .into_iter()
            .filter(|view| crate::sim::superweapon::ares_timer_visible(sim, rules, *view))
            .collect()
    } else {
        supers
    };
    let blackouts = sim.houses().order().iter().map(|&house| {
        let left = sim
            .power_states()
            .get(&house)
            .map_or(0, |power| power.blackout_remaining(frame));
        (house, left)
    });
    let scenario = sim.session().scenario_timer_view();
    let grouped = persistence.options_profile.grouped_super_timers;
    let local = sim.session().current_house();
    // Teams are the match's REAL launch teams, kept for the whole game in the
    // app-owned SkirmishLaunchSession. Do not infer them from
    // sim.house_alliances(): MakeAlly/MakeEnemy, triggers and the Paranoid AI
    // rewrite that graph mid-match, splitting houses that launched on one team
    // (the reported "enemy in D, me in -, both team A" bug).
    // normalized_launch_slots' owner_name is exactly the name the sim interner
    // registered for that house (asserted at launch).
    //
    // One entry per launched House: its real team, and whether it is a player
    // seat (a human-controlled or AI House, not an observer). The per-team
    // player count is how many such houses sit on that team; it is the "N" in
    // the no-team row's `(N)` caption.
    let launch = match_state
        .session
        .network_setup()
        .or_else(|| match_state.startup.offline_launch());
    let mut team_by_name: HashMap<String, Option<u8>> = HashMap::new();
    let mut players_by_team: HashMap<Option<u8>, usize> = HashMap::new();
    if let Some(launch) = launch {
        for slot in crate::sim::scenario_bootstrap::normalized_launch_slots(launch) {
            let team = match slot.team() {
                crate::skirmish_launch::LaunchTeam::Team(team) => Some(team),
                crate::skirmish_launch::LaunchTeam::None => None,
            };
            if !slot.observer {
                *players_by_team.entry(team).or_insert(0) += 1;
            }
            team_by_name.insert(slot.owner_name, team);
        }
    }
    let local_team = local
        .and_then(|house| team_by_name.get(sim.interner().resolve(house)))
        .copied()
        .flatten();
    let mut lines = timer_lines(
        scenario.map(|(left, _)| left),
        if grouped { &[] } else { &supers },
        blackouts,
        !sim.session().game_mode_nonzero(),
    );
    if grouped {
        // The team lines take the Super lines' place: after the Scenario
        // line, before the blackouts.
        let teams = grouped_super_rows(
            sim.houses().order(),
            |house| {
                team_by_name
                    .get(sim.interner().resolve(house))
                    .copied()
                    .flatten()
            },
            &supers,
            local,
            |team| players_by_team.get(&team).copied().unwrap_or(0),
        );
        let at = usize::from(scenario.is_some());
        lines.splice(
            at..at,
            teams.into_iter().map(|row| TimerLine {
                source: TimerSource::Team(row),
                seconds: row.nearest_seconds,
            }),
        );
    }
    if lines.is_empty() {
        return;
    }

    let csf = process_assets.csf.as_ref();
    let schemes = rules.color_schemes.as_slice();
    let house_rgb = |house: Option<InternedId>| {
        let color = house
            .and_then(|house| {
                match_state
                    .match_presentation
                    .effective_house_color_map()
                    .get(sim.interner().resolve(house))
            })
            .copied()
            .unwrap_or(NO_REMAP);
        house_text_rgb(schemes, color)
    };
    let super_name = |sw_type: InternedId| {
        type_ui_name(
            rules
                .super_weapon(sim.interner().resolve(sw_type))
                .and_then(|sw| sw.ui_name.as_deref()),
            csf,
        )
    };
    // Grouped rows use fixed relation bands rather than a house's map colour:
    // your own team is blue, a team you are allied with is green, the rest are
    // red. Same-team is decided by the launch team (stable all match); the
    // green check uses the live alliance graph, which is the right source for
    // "are we allies right now" even though it must never decide grouping.
    // Under the team-colours toggle an enemy team takes the colour the map
    // gives it, so the timers agree with the units and the radar.
    let team_colours = match_state.match_presentation.team_colors;
    let alliances = sim.house_alliances();
    let local_name = local.map(|house| sim.interner().resolve(house));
    let team_rgb = |row: &TeamSuperRow| {
        let same_team = match (row.team, local_team) {
            (TeamKey::Named(team), Some(own)) => team == own,
            (TeamKey::Unteamed, None) => true,
            _ => false,
        };
        if team_colours && row.team == TeamKey::Unteamed {
            return house_text_rgb(schemes, RELATION_ENEMY);
        }
        if same_team {
            return house_text_rgb(schemes, RELATION_SELF);
        }
        if local_name.is_some_and(|name| {
            is_allied_with(
                alliances.as_map(),
                name,
                sim.interner().resolve(row.representative),
            )
        }) {
            return house_text_rgb(schemes, RELATION_ALLY);
        }
        if team_colours && matches!(row.team, TeamKey::Named(_)) {
            return house_rgb(Some(row.representative));
        }
        house_text_rgb(schemes, RELATION_ENEMY)
    };
    let lines: Vec<(TimerLine, String, [u8; 3])> = lines
        .into_iter()
        .map(|line| {
            let (house, label, team_color) = match line.source {
                TimerSource::Scenario => (
                    sim.session().current_house(),
                    scenario
                        .filter(|(_, caption)| !caption.is_empty())
                        .map_or_else(String::new, |(_, caption)| {
                            crate::i18n::label(csf, caption, caption)
                        }),
                    None,
                ),
                TimerSource::Super(view) => (Some(view.owner()), super_name(view.sw_type()), None),
                TimerSource::Blackout(house) => (
                    Some(house),
                    crate::i18n::label(csf, BLACKOUT_LABEL, BLACKOUT_LABEL),
                    None,
                ),
                TimerSource::Team(row) => (
                    Some(row.representative),
                    format!(
                        "{} {} {}/{}",
                        row.team.label(row.players),
                        super_name(row.sw_type),
                        row.ready,
                        row.total
                    ),
                    Some(team_rgb(&row)),
                ),
            };
            let rgb = team_color.unwrap_or_else(|| house_rgb(house));
            (line, label, rgb)
        })
        .collect();
    let blink_rgb = blink_rgb(rules);
    let now_ms = crate::app::input::tooltips::now_ms(diag, match_state);
    // The dock stacks up from the bottom right of its box, the battlefield
    // in the original interface, and works in the box's own pixels.
    let [_, _, view_w, view_h] = field_parts::rect(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
        FieldPart::Timers,
    );
    let font = &renderer.bit_font;
    let timer_lines: Vec<_> = lines.iter().map(|(line, _, _)| *line).collect();
    // The grouped lines are few; they replace the compact dock's paging.
    let compact = persistence.options_profile.compact_super_timers && !grouped;
    let dock = &mut match_state.match_presentation.super_timer_dock;
    let rows = dock.rows(
        &timer_lines,
        local,
        [view_w as i32, view_h as i32],
        font.cell_height() as i32,
        compact,
    );
    let mut positions = vec![None; lines.len()];
    for (position, row) in rows.iter().enumerate() {
        if let DockRow::Line(i) = row {
            positions[*i] = Some(position);
        }
    }
    let blinks = &mut match_state.match_presentation.super_timer_blinks;
    let mut shown = Vec::with_capacity(2 * lines.len());
    let mut next_flip_ms: Option<u64> = None;
    for (index, (line, label, rgb)) in lines.iter().enumerate() {
        let blink = match line.source {
            TimerSource::Super(view) => Some(TimerBlink::of(blinks, &view)),
            TimerSource::Team(row) => Some(TimerBlink::of_team(blinks, &row)),
            TimerSource::Scenario | TimerSource::Blackout(_) => None,
        };
        let blinking = match blink {
            Some(blink) => {
                let on = blink.step(line.seconds, now_ms);
                if line.seconds == 0 {
                    next_flip_ms = Some(
                        next_flip_ms.map_or(blink.deadline_ms, |next| next.min(blink.deadline_ms)),
                    );
                }
                on
            }
            None => false,
        };
        // Hidden rows still step the existing wall-clock blink owner.
        let Some(position) = positions[index] else {
            continue;
        };
        let label = label_text(label);
        let time = time_text(line.seconds);
        let (label_at, time_at) = place_line(
            position,
            [view_w as i32, view_h as i32],
            font.cell_height() as i32,
            font.text_width(&label) as i32,
            font.text_width(&time) as i32,
        );
        let time_rgb = if blinking { blink_rgb } else { *rgb };
        if dock.expanded && matches!(line.source, TimerSource::Super(_)) {
            dock.scroll_rows.push(crate::ui::shell::geom::RectPx::new(
                label_at.fill[0],
                label_at.fill[1],
                time_at.fill[0] + time_at.fill[2] - label_at.fill[0],
                label_at.fill[3],
            ));
        }
        shown.push((label, label_at, *rgb));
        shown.push((time, time_at, time_rgb));
    }
    let toggle = dock.toggle_rect.map(|rect| (rect, dock.expanded));
    let presentation = &mut match_state.match_presentation;
    presentation.super_timer_next_flip_ms = next_flip_ms;
    presentation.super_timer_frame.texts = shown;
    presentation.super_timer_frame.toggle = toggle;
}

/// The timer lines as UI sprites, from what [`update_super_timers`] worked
/// out this frame: a box and a text node of `hud` for each text, the boxes
/// drawn first, then the text. Returns the boxes and the glyphs.
pub(crate) fn build_super_timer_instances(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
    hud: &mut TextHud,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    use crate::app::presentation::field_parts;
    use crate::ui::field_layout::FieldPart;
    let frame = &match_state.match_presentation.super_timer_frame;
    // Screen-space lanes add back the world camera the UI shader subtracts,
    // and the box's corner.
    let camera = if frame.texts.is_empty() {
        [0.0; 2]
    } else {
        field_parts::lane_camera(
            platform,
            renderer,
            screen,
            match_state,
            persistence,
            FieldPart::Timers,
        )
    };
    draw_timer_frame(frame, camera, &renderer.bit_font, hud)
}

/// [`build_super_timer_instances`] for a given frame and lane camera.
fn draw_timer_frame(
    frame: &TimerFrame,
    camera: [f32; 2],
    font: &crate::render::bit_font::BitFont,
    hud: &mut TextHud,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    hud.begin();
    if !frame.texts.is_empty() {
        let black = text_hud::rgb([0; 3]);
        let fill = |hud: &mut TextHud, [x, y, w, h]: [i32; 4]| {
            hud.fill(
                vera20k_gui::Rect::new(x, y, w, h),
                [x as f32 + camera[0], y as f32 + camera[1]],
                [w as f32, h as f32],
                black,
                0.0,
            );
        };
        for (text, at, rgb) in &frame.texts {
            fill(hud, at.fill);
            hud.text(
                vera20k_gui::Rect::new(at.x, at.y, at.fill[2], at.fill[3]),
                text,
                [at.x as f32 + camera[0], at.y as f32 + camera[1]],
                text_hud::rgb(*rgb),
                None,
            );
        }
        if let Some((rect, expanded)) = frame.toggle {
            // Solid 7x4 pixel triangles, no font substitution, frame or backing box.
            for row in 0..4 {
                let width = if expanded { 7 - row * 2 } else { 1 + row * 2 };
                fill(
                    hud,
                    [
                        rect.x + (rect.w - width) / 2,
                        rect.y + (rect.h - 4) / 2 + row,
                        width,
                        1,
                    ],
                );
            }
        }
    }
    hud.finish(font);
    (
        hud.boxes().copied().collect(),
        hud.glyphs().copied().collect(),
    )
}

/// Whether a 0 s Super line's blink flips on the next timer draw
/// (`0x006D4C75..0x006D4CDF`): its deadline has passed on the same clock.
pub(crate) fn blink_due(diag: &DiagnosticsState, match_state: &MatchState) -> bool {
    match_state
        .match_presentation
        .super_timer_next_flip_ms
        .is_some_and(|next| crate::app::input::tooltips::now_ms(diag, match_state) >= next)
}

#[cfg(all(test, tests_app_presentation))]
#[path = "super_timers_tests.rs"]
mod tests;
