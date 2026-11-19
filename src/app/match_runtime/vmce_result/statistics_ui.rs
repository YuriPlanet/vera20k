//! The statistics table shared by the victory and defeat pages.

use crate::app::AppState;
use crate::app::frontend::score_overlay::{ScoreHeader, ScoreLayout};
use crate::i18n::tr;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::{PaintLabel, SHELL_TEXT_RGB_ENABLED};
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::{RectPx, ShellScale};

use super::SeatResult;

pub(super) const ROWS_PER_PAGE: usize = 4;

const HEADERS: [(&str, i32, i32); 6] = [
    ("VERA:VmcePlayers", 40, 150),
    ("VERA:VmceKills", 200, 70),
    ("VERA:VmceLosses", 270, 70),
    ("VERA:VmceBuilt", 340, 70),
    ("VERA:VmceScore", 410, 80),
    ("VERA:VmceIncome", 490, 90),
];
#[derive(Debug)]
pub(super) struct Statistics {
    time: String,
    par: String,
}

impl Statistics {
    pub(super) fn new(header: &ScoreHeader) -> Self {
        Self {
            time: header.your_time.clone(),
            par: header.par_time.clone(),
        }
    }

    pub(super) fn labels(&self, state: &AppState, rows: &[SeatResult]) -> Vec<PaintLabel<'static>> {
        let yellow = SHELL_TEXT_RGB_ENABLED;
        let center = ShellAlign::H_CENTER;
        let left = ShellAlign::NONE;
        let mut labels = vec![
            label(
                format!("{} {}", tr("VERA:VmceMatchTime"), self.time),
                RectPx::new(40, 56, 260, 17),
                left,
                yellow,
            ),
            label(
                format!("{} {}", tr("VERA:VmceParTime"), self.par),
                RectPx::new(340, 56, 260, 17),
                left,
                yellow,
            ),
        ];
        for (index, &(key, x, w)) in HEADERS.iter().enumerate() {
            let align = if index == 0 { left } else { center };
            labels.push(label(
                ellipsize(&state.renderer.bit_font, &tr(key), w),
                RectPx::new(x, 96, w, 17),
                align,
                yellow,
            ));
        }
        for (index, row) in rows.iter().enumerate() {
            let y = 122 + index as i32 * 34;
            let rgb = rgb(row.score.rgb);
            let (_, x, w) = HEADERS[0];
            labels.push(label(
                ellipsize(&state.renderer.bit_font, &row.score.name, w),
                RectPx::new(x, y, w, 24),
                left,
                rgb,
            ));
            let values = [
                row.score.kills.to_string(),
                row.score.losses.to_string(),
                row.score.built.to_string(),
                row.score.score.to_string(),
                row.income.to_string(),
            ];
            for (&(_, x, w), value) in HEADERS[1..].iter().zip(values) {
                labels.push(label(value, RectPx::new(x, y, w, 24), center, rgb));
            }
        }
        labels
    }
}

pub(super) fn body_scale(layout: &ScoreLayout) -> ShellScale {
    if layout.scale.is_retail() && layout.screen.w == 640 {
        ShellScale::from_screen(layout.screen.w, layout.screen.h)
    } else {
        layout.scale
    }
}

pub(super) fn body_rect(layout: &ScoreLayout, scale: ShellScale, rect: RectPx) -> RectPx {
    scale
        .scale_rect(rect)
        .translate(layout.origin.0, layout.origin.1)
}

pub(super) fn ellipsize(font: &BitFont, text: &str, max_w: i32) -> String {
    let max_w = max_w as u32;
    if font.text_width(text) <= max_w {
        return text.to_owned();
    }
    for (end, _) in text.char_indices().rev() {
        let candidate = format!("{}...", &text[..end]);
        if font.text_width(&candidate) <= max_w {
            return candidate;
        }
    }
    String::new()
}

pub(super) fn label(
    text: String,
    rect: RectPx,
    align: ShellAlign,
    rgb: [f32; 3],
) -> PaintLabel<'static> {
    PaintLabel {
        text: text.into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    }
}

pub(super) fn rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|value| f32::from(value) / 255.0)
}
