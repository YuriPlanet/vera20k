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
/// `0xF0` native 16 ms timer buckets.
const TYPE_SELECT_MESSAGE_TIMEOUT_MS: u64 = 3_840;
/// The Supers' lines' `0x96` native 16 ms timer buckets.
const SUPER_WEAPON_MESSAGE_TIMEOUT_MS: u64 = 0x96 * 16;
/// Native falls back to runtime color-scheme 3. Rust stores the undoubled
/// `[Colors]` entry index, so that scheme is entry 1.
const TYPE_SELECT_FALLBACK_SCHEME_ENTRY: crate::rules::house_colors::HouseColorIndex =
    crate::rules::house_colors::HouseColorIndex(3 / 2);

/// Pause-adjusted message `now` (contract §4.2 step 8 / §4.3): the wall clock
/// minus every paused span. ALL message deadlines and expiry checks use this
/// clock — never the raw `tooltips::now_ms` — so a pause freezes the
/// remaining lifetime of every visible row.
pub(crate) fn message_now_ms(state: &AppState) -> u64 {
    state
        .match_state
        .match_presentation
        .message_clock
        .now(crate::app::input::tooltips::now_ms(state))
}

/// Original TAction11 6DE0B0..6DE122 resolves its CSF label, selects the
/// scenario-side priority through69A310 and adds a non-silent4046 row.
/// MessageDelay's ftol result is in WallClock16ms buckets, not sim frames.
pub(crate) fn post_trigger_message(state: &mut AppState, csf_key: &str) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    // The selected campaign opening keeps its startup player/country. The
    // existing House owner supplies that side; a later scenario-side change
    // independent of the current House remains a separate retained-field gap.
    let side = runtime
        .simulation
        .session
        .current_house()
        .and_then(|owner| runtime.simulation.houses.get(&owner))
        .map_or(0, |house| i32::from(house.side_index));
    let rules = &runtime.resources.rules;
    let rgb = trigger_message_rgb(&rules.color_schemes, side);
    let buckets = trigger_message_buckets(rules.general.message_delay_minutes);
    let timeout = (buckets != -1).then(|| u64::try_from(buckets).unwrap_or(0) * 16);
    post_localized_message(state, csf_key, rgb, timeout);
}

fn trigger_message_buckets(delay: crate::util::native_x87::NativeF64Bits) -> i32 {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53};
    X87Chop53::load_f64(delay)
        .map(|delay| {
            X87Chop53::mul(
                delay,
                X87Chop53::load_f64(NativeF64Bits::from_bits(0x408C_2000_0000_0000))
                    .expect("native900 constant is finite"),
            )
        })
        .map_or(0, X87Chop53::ftol_i32_low_masked)
}

fn trigger_message_rgb(
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
    side: i32,
) -> [f32; 3] {
    let priority = match side {
        0 => 2,
        1 => 1,
        _ => 5,
    };
    let entry = crate::rules::color_scheme::scheme_entry_for_priority(priority);
    crate::rules::color_scheme::scheme_hsv_by_entry(schemes, entry)
        .map(crate::rules::color_scheme::hsv_to_rgb)
        .unwrap_or([255; 3])
        .map(|channel| f32::from(channel) / 255.0)
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
    state
        .match_state
        .match_presentation
        .message_list
        .add_message(
            &crate::ui::messages::MessagePost {
                prefix: None,
                text,
                rgb,
                timeout_ms: Some(TYPE_SELECT_MESSAGE_TIMEOUT_MS),
                silent: true,
            },
            now,
            &|s| font.text_width(s) as i32,
        );
}

/// Post one of the Supers' lines (`LightningStorm::Start @ 0x0053A076`,
/// `Process @ 0x0053AB40`, `LightningStorm::PrintMessage @ 0x0053AE00`,
/// `PsyDom::PrintMessage @ 0x0053B410`): `MessageListClass::AddMessage(0, 0,
/// text, PlayerPtr's colour scheme or 3, style, 0x96, not silent)`, so the
/// line takes the player's colours as TypeSelect's does and plays
/// IncomingMessage. Its print styles (`0x4046`, `0x46`) are not represented.
pub(crate) fn post_super_weapon_message(state: &mut AppState, csf_key: &str) {
    let rgb = type_select_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| &rules.house_color_ramps),
    );
    post_localized_message(state, csf_key, rgb, Some(SUPER_WEAPON_MESSAGE_TIMEOUT_MS));
}

fn post_localized_message(
    state: &mut AppState,
    csf_key: &str,
    rgb: [f32; 3],
    timeout_ms: Option<u64>,
) {
    sync_view(state);
    let now = message_now_ms(state);
    let text = state.process_assets.csf.as_ref().map_or_else(
        || csf_key.to_string(),
        |table| table.text(csf_key).into_owned(),
    );
    let font = &state.renderer.bit_font;
    let outcome = state
        .match_state
        .match_presentation
        .message_list
        .add_message(
            &crate::ui::messages::MessagePost {
                prefix: None,
                text: &text,
                rgb,
                timeout_ms,
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
    state
        .match_state
        .match_presentation
        .message_clock
        .set_paused(state.match_state.paused(), wall);
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
    // The list spans the tactical viewport: the frame left of the sidebar.
    let tactical_w = crate::app::input::camera::tactical_viewport_size(state).0 as i32;
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
    message_instances(
        &state.renderer.bit_font,
        &state.match_state.match_presentation.message_list,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    )
    .1
}

/// Solid black boxes for the "message_fill" pooled buffer, drawn behind the
/// message rows. Native `MessageListEntry::Draw` (`0x488520`, entry vtable
/// `0x7F5B44` + `0x38`) renders every row through the shared text engine with
/// the row's opaque-background style, which fills a black box one pixel out on
/// every side of the text rect (`0x4A5EB0` @ `0x4A5F61`: `[x-1, y-1, w+2,
/// h+2]`, colour 0) before blitting the glyphs — the same box the sidebar
/// super-weapon timers get.
pub(crate) fn build_message_fill_instances(state: &AppState) -> Vec<SpriteInstance> {
    if state.frontend.screen != GameScreen::InGame {
        return Vec::new();
    }
    message_instances(
        &state.renderer.bit_font,
        &state.match_state.match_presentation.message_list,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    )
    .0
}

fn message_instances(
    font: &crate::render::bit_font::BitFont,
    list: &crate::ui::messages::MessageList,
    camera_offset: [f32; 2],
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let x = list.x() as f32;
    let mut fills = Vec::new();
    let mut texts = Vec::new();
    for m in list.messages() {
        // Message rows are screen-space, but `draw_pooled_ui` binds the
        // live world camera and the shader subtracts it. Add that camera
        // back here so scrolling cannot displace the HUD text.
        let text = crate::render::sidebar_text::build_text(
            font,
            &m.text,
            x,
            m.y as f32,
            1.0,
            0.00022,
            m.rgb,
            camera_offset,
        );
        let width = font.text_width(&m.text) as f32;
        let height = font.cell_height() as f32;
        fills.push(SpriteInstance {
            position: [
                x - 1.0 + camera_offset[0],
                m.y as f32 - 1.0 + camera_offset[1],
            ],
            size: [width + 2.0, height + 2.0],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            // The UI pass depth-tests: the box must sit BEHIND its own glyphs
            // (text depth 0.00022), mirroring the tooltip lane's ordering
            // (fill 0.00021 > border 0.000205 > text).
            depth: 0.00023,
            tint: [0.0; 3],
            alpha: 1.0,
            ..Default::default()
        });
        texts.extend(text);
    }
    (fills, texts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original_trigger_messages() -> serde_json::Value {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_action_values.json",
        ))
        .unwrap();
        rows.into_iter()
            .find(|row| row["case_kind"] == "localized_message")
            .expect("original action11 MessageDelay/color/CSF controls")
    }

    fn original_delay_bits(row: &serde_json::Value) -> u64 {
        let hex = row["delay_f64_le_hex"].as_str().unwrap();
        assert_eq!(hex.len(), 16);
        u64::from_le_bytes(std::array::from_fn(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap()
        }))
    }

    #[test]
    fn trigger_message_rules_reader_and_bucket_conversion_match_original_binary64_controls() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        let native = original_trigger_messages();
        let rows = native["histories"].as_array().unwrap();
        assert_eq!(rows.len(), 21);
        for row in rows {
            let ini = if let Some(delay) = row["input_delay"].as_str() {
                IniFile::from_str(&format!("[AudioVisual]\nMessageDelay={delay}\n"))
            } else {
                IniFile::from_str("[AudioVisual]\n")
            };
            // GeneralRules::from_ini is private to its existing owner. Use
            // its production RuleSet entry, without exposing another reader.
            let rules = RuleSet::from_ini(&ini).unwrap();
            let delay = rules.general.message_delay_minutes;
            assert_eq!(delay.bits(), original_delay_bits(row), "{row} parsed bits");
            let message = row["calls"].as_array().unwrap().last().unwrap();
            assert_eq!(message["call"], "message_add");
            assert_eq!(
                i64::from(trigger_message_buckets(delay)),
                message["duration"].as_i64().unwrap(),
                "{row} native ftol duration"
            );
        }
        // The stock `.6` spelling and the native control's `0.6` both pass
        // through the same production ReadDouble owner. Constructor omission
        // has distinct observed bits and is covered above.
        let Some(retail) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let stock = RuleSet::from_ini(&retail).unwrap();
        let recorded = rows.iter().find(|row| row["input_delay"] == "0.6").unwrap();
        assert_eq!(
            stock.general.message_delay_minutes.bits(),
            original_delay_bits(recorded)
        );
        assert_eq!(
            i64::from(trigger_message_buckets(stock.general.message_delay_minutes)),
            recorded["calls"].as_array().unwrap().last().unwrap()["duration"]
                .as_i64()
                .unwrap()
        );
    }

    #[test]
    fn trigger_message_scenario_side_selects_original_runtime_scheme_in_stock_colors() {
        use crate::rules::ruleset::RuleSet;

        let Some(retail) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&retail).unwrap();
        let native = original_trigger_messages();
        for row in native["histories"].as_array().unwrap() {
            let color = row["calls"].as_array().unwrap().last().unwrap()["color"]
                .as_u64()
                .unwrap() as usize;
            // Original69A310's output indexes the doubled runtime scheme
            // list. The shared color owner handles HSV conversion; this
            // comparison establishes priority selection, not GPU pixels.
            let scheme = &rules.color_schemes[color / 2];
            let expected = crate::rules::color_scheme::hsv_to_rgb(scheme.hsv)
                .map(|channel| f32::from(channel) / 255.0);
            assert_eq!(
                trigger_message_rgb(&rules.color_schemes, row["side"].as_i64().unwrap() as i32),
                expected,
                "{row}"
            );
        }
    }

    #[test]
    fn trigger_message_csf_labels_match_original_lookup_through_retail_archive_reader() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let selected = assets
            .load_file_from_mix("ra2md.csf")
            .expect("active-YR retail CSF");
        let native = original_trigger_messages();
        assert_eq!(
            crate::util::sha256::sha256_hex(&selected.bytes),
            native["csf"]["source_sha256"].as_str().unwrap(),
            "the corpus and production loader must select the same physical CSF"
        );
        let table = crate::assets::csf_file::CsfFile::from_bytes(&selected.bytes).unwrap();
        let rows = native["histories"]
            .as_array()
            .unwrap()
            .iter()
            .chain(native["label_controls"].as_array().unwrap());
        let mut compared = 0;
        for row in rows {
            let Some(message) = row["calls"].as_array().unwrap().last() else {
                continue;
            };
            assert_eq!(message["call"], "message_add");
            let label = row["label"].as_str().unwrap();
            assert_eq!(
                table.text(label).as_ref(),
                message["text"].as_str().unwrap(),
                "{label}"
            );
            compared += 1;
        }
        assert_eq!(compared, 27);
        // Archive selection, CSF parsing, case-insensitive lookup and missing
        // labels are compared. Full AppState posting, wrapping, expiration
        // and the rendered message surface remain separate validation.
    }

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

        let at_origin = message_instances(&font, &list, [0.0, 0.0]).1;
        let ui_camera = [640.0_f32, 480.0_f32];
        let panned = message_instances(&font, &list, ui_camera).1;

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
