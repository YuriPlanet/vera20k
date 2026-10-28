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
const FRAMES_PER_SECOND: i32 = 15;
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
/// `GetRechargeTime` left takes no line (`0x006D49D1..0x006D4A06`).
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
        let blink = blinks
            .entry((view.owner(), view.sw_type()))
            .or_insert(TimerBlink {
                place: view.place(),
                ..TimerBlink::default()
            });
        if blink.place != view.place() {
            blink.on = false;
            blink.place = view.place();
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
pub(crate) fn build_super_timer_instances(
    state: &mut AppState,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    if state.frontend.screen != GameScreen::InGame {
        return (Vec::new(), Vec::new());
    }
    let (Some(runtime), Some(rules)) = (state.match_state.sim_runtime.as_ref(), state.rules())
    else {
        return (Vec::new(), Vec::new());
    };
    let sim = &runtime.simulation;
    let frame = sim.session().binary_frame();
    let supers = crate::sim::superweapon::super_timer_views(sim, rules);
    let blackouts = sim.houses().order().iter().map(|&house| {
        let left = sim
            .power_states()
            .get(&house)
            .map_or(0, |power| power.blackout_remaining(frame));
        (house, left)
    });
    let scenario = sim.session().scenario_timer_view();
    let lines = timer_lines(
        scenario.map(|(left, _)| left),
        &supers,
        blackouts,
        !sim.session().game_mode_nonzero(),
    );
    if lines.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let csf = state.process_assets.csf.as_ref();
    let schemes = rules.color_schemes.as_slice();
    let house_rgb = |house: Option<InternedId>| {
        let color = house
            .and_then(|house| {
                state
                    .match_state
                    .match_presentation
                    .house_color_map
                    .get(sim.interner().resolve(house))
            })
            .copied()
            .unwrap_or(NO_REMAP);
        house_text_rgb(schemes, color)
    };
    let lines: Vec<(TimerLine, String, [u8; 3])> = lines
        .into_iter()
        .map(|line| {
            let (house, label) = match line.source {
                TimerSource::Scenario => (
                    sim.session().current_house(),
                    scenario
                        .filter(|(_, caption)| !caption.is_empty())
                        .map_or_else(String::new, |(_, caption)| localized(csf, caption)),
                ),
                TimerSource::Super(view) => (
                    Some(view.owner()),
                    type_ui_name(
                        rules
                            .super_weapon(sim.interner().resolve(view.sw_type()))
                            .and_then(|sw| sw.ui_name.as_deref()),
                        csf,
                    ),
                ),
                TimerSource::Blackout(house) => (Some(house), localized(csf, BLACKOUT_LABEL)),
            };
            (line, label, house_rgb(house))
        })
        .collect();
    let blink_rgb = blink_rgb(rules);
    let now_ms = crate::app::input::tooltips::now_ms(state);
    let (view_w, view_h) = crate::app::input::camera::tactical_viewport_size(state);
    // Screen-space lanes add back the world camera the UI shader subtracts.
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];

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
    let mut fills = Vec::with_capacity(2 * lines.len());
    let mut texts = Vec::new();
    for (index, (line, label, rgb)) in lines.iter().enumerate() {
        let blinking = match line.source {
            TimerSource::Super(view) => TimerBlink::of(blinks, &view).step(line.seconds, now_ms),
            TimerSource::Scenario | TimerSource::Blackout(_) => false,
        };
        let label = label_text(label);
        let time = time_text(line.seconds);
        let (label_at, time_at) = place_line(
            index,
            [view_w as i32, view_h as i32],
            font.cell_height() as i32,
            font.text_width(&label) as i32,
            font.text_width(&time) as i32,
        );
        let time_rgb = if blinking { blink_rgb } else { *rgb };
        for (text, at, rgb) in [(&label, label_at, *rgb), (&time, time_at, time_rgb)] {
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
    (fills, texts)
}

#[cfg(test)]
#[path = "super_timers_tests.rs"]
mod tests;
