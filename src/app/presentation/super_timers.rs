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

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::app::AppState;
use crate::assets::csf_file::{CsfFile, type_ui_name};
use crate::render::batch::SpriteInstance;
use crate::rules::house_colors::{HouseColorIndex, NO_REMAP, house_text_rgb};
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
///
/// Retained for the native `tactical_timers`/`timer_lines` oracle comparison;
/// the in-game panel now uses the grouped rows in [`super_timer_groups`].
#[allow(dead_code)]
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
}

/// One timer line; its index is its place in [`timer_lines`]' answer.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    /// it (`+0x40`, Grant `0x006CB580`); the deadline stays. Retained for the
    /// native parity test; the grouped panel uses [`Self::of_key`].
    #[allow(dead_code)]
    fn of<'a>(blinks: &'a mut TimerBlinks, view: &SuperTimerView) -> &'a mut TimerBlink {
        Self::of_key(blinks, view.owner(), view.sw_type(), view.place())
    }

    /// Blink identity for a grouped panel row, which has no native grant
    /// place. `place = 0` is stable across frames so a countdown's blink is
    /// not reset by a reframe; only an owner/type change picks a new blink.
    fn of_key<'a>(
        blinks: &'a mut TimerBlinks,
        owner: InternedId,
        sw_type: InternedId,
        place: u32,
    ) -> &'a mut TimerBlink {
        let blink = blinks
            .entry((owner, sw_type))
            .or_insert(TimerBlink {
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
    toggle_rect: Option<crate::ui::gadget::GadgetRect>,
    scroll_rows: Vec<crate::ui::gadget::GadgetRect>,
}

#[allow(dead_code)]
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

    pub(crate) fn toggle_rect(&self) -> Option<crate::ui::gadget::GadgetRect> {
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
    #[allow(dead_code)]
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
            self.toggle_rect = Some(crate::ui::gadget::GadgetRect::new(
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
pub(crate) fn cursor_in_view(state: &AppState) -> (i32, i32) {
    let (x, y) = state.match_state.input.cursor_px();
    let [left, top, _, _] = crate::app::presentation::field_parts::rect(
        state,
        crate::ui::field_layout::FieldPart::Timers,
    );
    (x - left, y - top)
}

/// Consume wheel messages only over the expanded timer ink, before world zoom
/// or sidebar scrolling. Empty space around the rows stays interactive terrain.
pub(crate) fn wheel(state: &mut AppState, delta: f32) -> bool {
    let (x, y) = cursor_in_view(state);
    state
        .match_state
        .match_presentation
        .super_timer_dock
        .wheel(x, y, delta)
}

/// The string table's text for `key` (`0x00734E60`); the key itself only
/// without a string table (an assetless run).
fn localized(csf: Option<&CsfFile>, key: &str) -> String {
    csf.map_or_else(|| key.to_string(), |csf| csf.text(key).into_owned())
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

/// The timer lines as UI sprites: the black boxes, drawn first, and the text.
/// The tactical view starts at the screen's origin.
/// One printable line of the grouped panel. Kept local because none of the
/// native [`TimerSource`] variants can carry a `小队 TYPE r/t` label; the
/// native `timer_lines`/oracle path stays untouched for parity evidence.
struct PanelRow {
    label: String,
    seconds: i32,
    rgb: [u8; 3],
    /// Grouped lines blink on the representative's wall clock, like a native
    /// Super. `None` for the Scenario and blackout lines.
    blink: Option<(InternedId, InternedId)>,
}

/// House names in the alliance graph are stored trimmed + uppercase.
fn normalized(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// Build the right-hand timer panel: the Scenario timer, one aggregated row
/// per (team, Super type) for the opponents, then each house's blackout. The
/// local player's own Supers are hidden (they read them on the sidebar); each
/// row reads `小队 TYPE ready/total` with the shortest pending countdown.
///
/// The former native one-line-per-Super list and its compact/fold dock are
/// replaced by these grouped rows. Scenario/blackout placement, colours and
/// blink cadence follow `TacticalClass::DrawTimer` as before.
pub(crate) fn build_super_timer_instances(
    state: &mut AppState,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    // Grouped rows have no fold/scroll hit areas; clear any left from a frame
    // that used the old native path (e.g. after a hot reload in dev).
    state
        .match_state
        .match_presentation
        .super_timer_dock
        .clear_hits();
    state
        .match_state
        .match_presentation
        .super_timer_next_flip_ms = None;
    use crate::app::presentation::field_parts;
    use crate::ui::field_layout::FieldPart;
    // A layout that leaves the timers out draws none and keeps no hit area.
    if state.frontend.screen != GameScreen::InGame || !field_parts::shows(state, FieldPart::Timers)
    {
        return (Vec::new(), Vec::new());
    }
    let (Some(runtime), Some(rules)) =
        (state.match_state.sim_runtime.presentation(), state.rules())
    else {
        return (Vec::new(), Vec::new());
    };
    let sim = &runtime.simulation;
    let frame = sim.session().binary_frame();
    let csf = state.process_assets.csf.as_ref();
    let schemes = rules.color_schemes.as_slice();
    let house_rgb = |house: Option<InternedId>| {
        let color = house
            .and_then(|house| {
                state
                    .match_state
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

    let mut rows: Vec<PanelRow> = Vec::new();

    // The Scenario timer, unchanged (its caption is the line's label).
    let scenario = sim.session().scenario_timer_view();
    if let Some((left, _)) = scenario {
        let caption = scenario
            .filter(|(_, caption)| !caption.is_empty())
            .map_or_else(String::new, |(_, caption)| localized(csf, caption));
        rows.push(PanelRow {
            label: caption,
            seconds: left / FRAMES_PER_SECOND,
            rgb: house_rgb(sim.session().current_house()),
            blink: None,
        });
    }

    // Grouped team/type lines replace the native Super timer list. Ares hook
    // 0x6D49D1 still hides any Super the local player must not see before it
    // reaches the panel: filter the ready views by the same TimerVisibility.
    let mut views = crate::sim::superweapon::super_ready_views(sim);
    if rules.ares_form() {
        let visible: HashSet<(InternedId, InternedId)> =
            crate::sim::superweapon::super_timer_views(sim, rules)
                .into_iter()
                .filter(|view| crate::sim::superweapon::ares_timer_visible(sim, rules, *view))
                .map(|view| (view.owner(), view.sw_type()))
                .collect();
        views.retain(|view| visible.contains(&(view.owner(), view.sw_type())));
    }
    let houses = sim.houses().order();
    let interner = sim.interner();
    let alliances = sim.house_alliances();
    let mut by_key: HashMap<String, InternedId> = HashMap::with_capacity(houses.len());
    for &house in houses {
        by_key.insert(normalized(interner.resolve(house)), house);
    }
    let allies: Vec<BTreeSet<InternedId>> = houses
        .iter()
        .map(|&house| {
            alliances
                .as_map()
                .get(&normalized(interner.resolve(house)))
                .into_iter()
                .flatten()
                .filter_map(|name| by_key.get(&normalized(name)).copied())
                .collect()
        })
        .collect();
    let grouped = crate::app::presentation::super_timer_groups::grouped_super_rows(
        houses,
        &allies,
        &views,
        sim.session().current_house(),
    );
    for group in grouped {
        let name = super_name(group.sw_type);
        let label = format!("{} {} {}/{}", group.team.label(), name, group.ready, group.total);
        rows.push(PanelRow {
            label,
            seconds: group.nearest_seconds,
            rgb: house_rgb(Some(group.representative)),
            blink: Some((group.representative, group.sw_type)),
        });
    }

    // Each house's blackout, unchanged and after the grouped Super lines.
    for &house in houses {
        let left = sim
            .power_states()
            .get(&house)
            .map_or(0, |power| power.blackout_remaining(frame));
        if left > 0 {
            rows.push(PanelRow {
                label: localized(csf, BLACKOUT_LABEL),
                seconds: left / FRAMES_PER_SECOND,
                rgb: house_rgb(Some(house)),
                blink: None,
            });
        }
    }

    if rows.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let blink_rgb = blink_rgb(rules);
    let now_ms = crate::app::input::tooltips::now_ms(state);
    let [_, _, view_w, view_h] = field_parts::rect(state, FieldPart::Timers);
    let camera = field_parts::lane_camera(state, FieldPart::Timers);

    let blinks = &mut state.match_state.match_presentation.super_timer_blinks;
    let font = &state.renderer.bit_font;
    let tint = |rgb: [u8; 3]| rgb.map(|channel| f32::from(channel) / 255.0);
    let fill = |[x, y, w, h]: [i32; 4]| SpriteInstance {
        position: [x as f32 + camera[0], y as f32 + camera[1]],
        size: [w as f32, h as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        tint: [0.0; 3],
        alpha: 1.0,
        ..Default::default()
    };
    let mut fills = Vec::with_capacity(2 * rows.len());
    let mut texts = Vec::new();
    let mut next_flip_ms: Option<u64> = None;
    for (index, row) in rows.iter().enumerate() {
        let blinking = match row.blink {
            Some((owner, sw_type)) => {
                let blink = TimerBlink::of_key(blinks, owner, sw_type, 0);
                let on = blink.step(row.seconds, now_ms);
                if row.seconds == 0 {
                    next_flip_ms = Some(
                        next_flip_ms.map_or(blink.deadline_ms, |next| next.min(blink.deadline_ms)),
                    );
                }
                on
            }
            None => false,
        };
        let label = label_text(&row.label);
        let time = time_text(row.seconds);
        let (label_at, time_at) = place_line(
            index,
            [view_w as i32, view_h as i32],
            font.cell_height() as i32,
            font.text_width(&label) as i32,
            font.text_width(&time) as i32,
        );
        let time_rgb = if blinking { blink_rgb } else { row.rgb };
        for (text, at, rgb) in [(&label, label_at, row.rgb), (&time, time_at, time_rgb)] {
            fills.push(fill(at.fill));
            texts.extend(font.build_text(
                text,
                at.x as f32,
                at.y as f32,
                1.0,
                0.0,
                tint(rgb),
                camera,
            ));
        }
    }
    state
        .match_state
        .match_presentation
        .super_timer_next_flip_ms = next_flip_ms;
    (fills, texts)
}

/// Whether a 0 s Super line's blink flips on the next timer draw
/// (`0x006D4C75..0x006D4CDF`): its deadline has passed on the same clock.
pub(crate) fn blink_due(state: &AppState) -> bool {
    state
        .match_state
        .match_presentation
        .super_timer_next_flip_ms
        .is_some_and(|next| crate::app::input::tooltips::now_ms(state) >= next)
}

#[cfg(all(test, tests_app_presentation))]
#[path = "super_timers_tests.rs"]
mod tests;
