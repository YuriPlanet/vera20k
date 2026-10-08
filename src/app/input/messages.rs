//! Chat/system message surface driver (study §3.1): anchors the
//! `ui::messages::MessageList` to the tactical viewport (x+3 / y / w−14),
//! posts system messages (insert sound = [AudioVisual] IncomingMessage),
//! expires rows per frame against a pause-FROZEN clock (contract §4.2 step 8
//! / §4.3: deadlines resume with remaining lifetime intact after a pause),
//! and builds the text instances drawn between the sidebar text and the
//! tooltip (study O10: chat before tooltip).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::ui::game_screen::GameScreen;

/// Native Init anchors: x = tactical_x + 3, y = tactical_y, w = tactical_w − 14.
const MESSAGE_X_INSET: i32 = 3;
const MESSAGE_WIDTH_INSET: i32 = 14;
/// Interim system-message color (native rows use a color-scheme index whose
/// mapping is a plan deferred item).
const MESSAGE_RGB_SYSTEM: [f32; 3] = [1.0, 1.0, 1.0];
/// Mission/trigger text lifetime — preserves the pre-A5 banner's 4 s
/// (the native trigger-text timeout is untraced; deferred item).
const MISSION_TEXT_TIMEOUT_MS: u64 = 4_000;
/// `0xF0` native 16 ms timer buckets.
const TYPE_SELECT_MESSAGE_TIMEOUT_MS: u64 = 3_840;
/// The Lightning Storm's `0x96` native 16 ms timer buckets.
const LIGHTNING_STORM_MESSAGE_TIMEOUT_MS: u64 = 0x96 * 16;
/// Native falls back to runtime color-scheme 3. Rust stores the undoubled
/// `[Colors]` entry index, so that scheme is entry 1.
const TYPE_SELECT_FALLBACK_SCHEME_ENTRY: crate::rules::house_colors::HouseColorIndex =
    crate::rules::house_colors::HouseColorIndex(3 / 2);

/// Pause-adjusted message `now` (contract §4.2 step 8 / §4.3): the wall clock
/// minus every paused span. ALL message deadlines and expiry checks use this
/// clock — never the raw `tooltips::now_ms` — so a pause freezes the
/// remaining lifetime of every visible row.
pub(crate) fn message_now_ms(state: &AppState) -> u64 {
    state.match_state.match_presentation.message_clock.now(crate::app::input::tooltips::now_ms(state))
}

/// Post a system message (mission/trigger text, future house notifications).
pub(crate) fn post_system_message(state: &mut AppState, text: &str) {
    sync_view(state);
    let now = message_now_ms(state);
    let font = &state.renderer.bit_font;
    let measure = |s: &str| font.text_width(s) as i32;
    let outcome = state.match_state.match_presentation.message_list.add_message(
        &crate::ui::messages::MessagePost {
            prefix: None,
            text,
            rgb: MESSAGE_RGB_SYSTEM,
            timeout_ms: Some(MISSION_TEXT_TIMEOUT_MS),
            silent: false,
        },
        now,
        &measure,
    );
    if outcome.play_sound {
        let sound = state.rules()
            .and_then(|r| r.general.incoming_message_sound.clone());
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
}

/// Post the localized, silent HUD result of one executed TypeSelect tap.
pub(crate) fn post_type_select_feedback(state: &mut AppState, csf_key: &str) {
    sync_view(state);
    let now = message_now_ms(state);
    let rgb = type_select_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| &rules.house_color_ramps),
    );
    let font = &state.renderer.bit_font;
    let measure = |s: &str| font.text_width(s) as i32;
    let outcome = add_type_select_feedback(
        &mut state.match_state.match_presentation.message_list,
        state.process_assets.csf.as_ref(),
        csf_key,
        rgb,
        now,
        &measure,
    );
    debug_assert!(
        !outcome.play_sound,
        "TypeSelect feedback is a silent message add"
    );
}

fn add_type_select_feedback(
    list: &mut crate::ui::messages::MessageList,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    csf_key: &str,
    rgb: [f32; 3],
    now_ms: u64,
    measure: &dyn Fn(&str) -> i32,
) -> crate::ui::messages::AddOutcome {
    let text = csf
        .map(|table| table.text(csf_key))
        .unwrap_or(std::borrow::Cow::Borrowed(csf_key));
    list.add_message(
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: text.as_ref(),
            rgb,
            timeout_ms: Some(TYPE_SELECT_MESSAGE_TIMEOUT_MS),
            silent: true,
        },
        now_ms,
        measure,
    )
}

/// HealthNav uses the same silent, house-colored 0xF0-bucket message add as
/// TypeSelect, after formatting MSG:UnitsWorth/NoUnitsSel/NavEmpty (733569).
pub(crate) fn post_selection_navigation_text(state: &mut AppState, text: &str) {
    sync_view(state);
    let now = message_now_ms(state);
    let rgb = type_select_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| &rules.house_color_ramps),
    );
    let font = &state.renderer.bit_font;
    state.match_state.match_presentation.message_list.add_message(
        &crate::ui::messages::MessagePost {
            prefix: None, text, rgb,
            timeout_ms: Some(TYPE_SELECT_MESSAGE_TIMEOUT_MS), silent: true,
        },
        now, &|s| font.text_width(s) as i32,
    );
}

/// Post one of the Lightning Storm's lines (`LightningStorm::Start @
/// 0x0053A076`, `Process @ 0x0053AB40`): `MessageListClass::AddMessage(0, 0,
/// text, PlayerPtr's colour scheme or 3, style, 0x96, not silent)` on every
/// client, so the line takes the player's colours as TypeSelect's does and
/// plays IncomingMessage. Its print styles (`0x4046`, `0x46`) are not
/// represented.
pub(crate) fn post_lightning_storm_message(state: &mut AppState, csf_key: &str) {
    sync_view(state);
    let now = message_now_ms(state);
    let rgb = type_select_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| &rules.house_color_ramps),
    );
    let text = state
        .process_assets
        .csf
        .as_ref()
        .map_or_else(|| csf_key.to_string(), |table| table.text(csf_key).into_owned());
    let font = &state.renderer.bit_font;
    let outcome = state.match_state.match_presentation.message_list.add_message(
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: &text,
            rgb,
            timeout_ms: Some(LIGHTNING_STORM_MESSAGE_TIMEOUT_MS),
            silent: false,
        },
        now,
        &|s| font.text_width(s) as i32,
    );
    if outcome.play_sound {
        let sound = state
            .rules()
            .and_then(|r| r.general.incoming_message_sound.clone());
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
}

fn type_select_message_rgb(
    local_owner: Option<&str>,
    house_colors: &crate::map::houses::HouseColorMap,
    ramps: Option<&crate::rules::house_colors::HouseColorRamps>,
) -> [f32; 3] {
    let Some(ramps) = ramps else {
        return MESSAGE_RGB_SYSTEM;
    };
    let scheme = local_owner
        .and_then(|owner| house_colors.get(owner).copied())
        .unwrap_or(TYPE_SELECT_FALLBACK_SCHEME_ENTRY);
    let color = ramps.ramp(scheme)[0];
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    ]
}

/// Per-frame: feed the pause edge into the clock, then (unpaused, in-game)
/// re-anchor to the live viewport and expire rows against the FROZEN clock.
/// While paused the clock accumulates the span and `manage` is skipped — both
/// halves are required: skipping alone would let wall-time deadlines expire
/// the instant the game unpauses.
pub(crate) fn update(state: &mut AppState) -> Option<u64> {
    if state.frontend.screen != GameScreen::InGame {
        return None;
    }
    let wall = crate::app::input::tooltips::now_ms(state);
    state.match_state.match_presentation.message_clock.set_paused(state.match_state.paused(), wall);
    if state.match_state.paused() {
        return None;
    }
    sync_view(state);
    let now = message_now_ms(state);
    state
        .match_state
        .match_presentation
        .message_list
        .manage(now);
    Some(now)
}

fn sync_view(state: &mut AppState) {
    // Tactical viewport = render area minus the sidebar panel width.
    let tactical_w =
        state.render_width() as i32 - state.match_state.match_presentation.sidebar_layout_spec.sidebar_width.round() as i32;
    state.match_state.match_presentation.message_list.set_view(
        MESSAGE_X_INSET,
        0,
        (tactical_w - MESSAGE_WIDTH_INSET).max(0),
    );
}

/// Text instances for the "message_text" pooled buffer (GAME.FNT atlas).
pub(crate) fn build_message_text_instances(state: &AppState) -> Vec<SpriteInstance> {
    if state.frontend.screen != GameScreen::InGame {
        return Vec::new();
    }
    message_text_instances(
        &state.renderer.bit_font,
        &state.match_state.match_presentation.message_list,
        [state.match_state.input.camera_x, state.match_state.input.camera_y],
    )
}

fn message_text_instances(
    font: &crate::render::bit_font::BitFont,
    list: &crate::ui::messages::MessageList,
    camera_offset: [f32; 2],
) -> Vec<SpriteInstance> {
    let x = list.x() as f32;
    list.messages()
        .iter()
        .flat_map(|m| {
            // Message rows are screen-space, but `draw_pooled_ui` binds the
            // live world camera and the shader subtracts it. Add that camera
            // back here so scrolling cannot displace the HUD text.
            crate::render::sidebar_text::build_text(
                font,
                &m.text,
                x,
                m.y as f32,
                1.0,
                0.00022,
                m.rgb,
                camera_offset,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_csf(label: &str, value: &str) -> crate::assets::csf_file::CsfFile {
        let encoded_value: Vec<u8> = value
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .map(|byte| !byte)
            .collect();
        let mut data = Vec::new();
        data.extend_from_slice(&0x4353_4620u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0x4C42_4C20u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&(label.len() as u32).to_le_bytes());
        data.extend_from_slice(label.as_bytes());
        data.extend_from_slice(&0x5354_5220u32.to_le_bytes());
        data.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        data.extend_from_slice(&encoded_value);
        crate::assets::csf_file::CsfFile::from_bytes(&data).expect("item83 CSF fixture")
    }

    #[test]
    fn item83_type_select_feedback_resolves_csf_into_one_real_silent_message_row() {
        let csf = test_csf("msg:selacrossscreen", "Lokalisierte Bildschirmauswahl");
        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 1_000);
        let rgb = [0.25, 0.5, 0.75];
        let now_ms = 1_200;
        let measure = |text: &str| text.chars().count() as i32 * 8;

        let outcome = add_type_select_feedback(
            &mut list,
            Some(&csf),
            "MSG:SelAcrossScreen",
            rgb,
            now_ms,
            &measure,
        );

        assert_eq!(outcome.added, 1);
        assert!(!outcome.play_sound, "silent=1 suppresses IncomingMessage");
        assert_eq!(list.messages().len(), 1);
        let row = &list.messages()[0];
        assert_eq!(row.text, "Lokalisierte Bildschirmauswahl");
        assert_eq!(row.rgb, rgb);
        assert_eq!(row.deadline_ms, Some(now_ms + 3_840));
    }

    #[test]
    fn item83_type_select_feedback_uses_local_scheme_color_zero_and_runtime_three_fallback() {
        use crate::rules::color_scheme::ColorSchemeEntry;
        use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};

        let ramps = HouseColorRamps::from_schemes(&[
            ColorSchemeEntry {
                name: "LightGold".into(),
                hsv: [25, 255, 255],
            },
            ColorSchemeEntry {
                name: "Gold".into(),
                hsv: [43, 239, 255],
            },
            ColorSchemeEntry {
                name: "DarkBlue".into(),
                hsv: [153, 214, 212],
            },
        ]);
        let mut house_colors = crate::map::houses::HouseColorMap::new();
        house_colors.insert("Americans".into(), HouseColorIndex(2));
        let normalized = |index| {
            let color = ramps.ramp(index)[0];
            [
                color.r as f32 / 255.0,
                color.g as f32 / 255.0,
                color.b as f32 / 255.0,
            ]
        };

        assert_eq!(
            type_select_message_rgb(Some("Americans"), &house_colors, Some(&ramps)),
            normalized(HouseColorIndex(2))
        );
        assert_eq!(
            type_select_message_rgb(None, &house_colors, Some(&ramps)),
            normalized(HouseColorIndex(1)),
            "runtime scheme 3 addresses undoubled Colors entry 1"
        );
    }

    #[test]
    fn gsi_02_14_message_glyphs_compensate_for_ui_camera() {
        use crate::render::bit_font::tests::make_test_font;

        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 1_000);
        list.add_message(
            &crate::ui::messages::MessagePost {
                prefix: None,
                text: "xxx",
                rgb: [1.0, 1.0, 1.0],
                timeout_ms: None,
                silent: true,
            },
            0,
            &|text| font.text_width(text) as i32,
        );

        let at_origin = message_text_instances(&font, &list, [0.0, 0.0]);
        let ui_camera = [640.0_f32, 480.0_f32];
        let panned = message_text_instances(&font, &list, ui_camera);

        assert_eq!(at_origin.len(), 3);
        assert_eq!(panned.len(), at_origin.len());
        for (origin, shifted) in at_origin.iter().zip(&panned) {
            let after_shader_subtraction = [
                shifted.position[0] - ui_camera[0].round(),
                shifted.position[1] - ui_camera[1].round(),
            ];
            assert_eq!(after_shader_subtraction, origin.position);
        }
    }
}
