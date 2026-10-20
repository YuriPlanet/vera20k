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
/// Fallback while match color schemes are unavailable.
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
        .session()
        .current_house()
        .and_then(|owner| runtime.simulation.houses().get(&owner))
        .map_or(0, |house| i32::from(house.side_index()));
    let rules = &runtime.resources.rules;
    let rgb = trigger_message_rgb(&rules.color_schemes, side);
    let timeout = message_delay_timeout_ms(rules.general.message_delay_minutes);
    post_localized_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: csf_key,
            rgb,
            timeout_ms: timeout,
            silent: false,
        },
    );
}

fn message_delay_buckets(delay: crate::util::native_x87::NativeF64Bits) -> i32 {
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
    post_silent_house_feedback(state, csf_key, TYPE_SELECT_MESSAGE_TIMEOUT_MS);
}

///63A0D0/639FD0/639130 use the same silent730A90 HUD owner with duration1E0.
/// Their separate ScoldSound belongs to the planning input owner.
pub(crate) fn post_planning_refusal(state: &mut AppState, csf_key: &str) {
    post_silent_house_feedback(state, csf_key, 0x1E0 * 16);
}

fn post_silent_house_feedback(state: &mut AppState, csf_key: &str, timeout_ms: u64) {
    sync_view(state);
    let now = message_now_ms(state);
    let rgb = house_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| rules.color_schemes.as_slice()),
    );
    let font = &state.renderer.bit_font;
    let measure = |s: &str| font.text_width(s) as i32;
    let outcome = add_localized_message(
        &mut state.match_state.match_presentation.message_list,
        state.process_assets.csf.as_ref(),
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: csf_key,
            rgb,
            timeout_ms: Some(timeout_ms),
            silent: true,
        },
        now,
        &measure,
    );
    debug_assert!(
        !outcome.play_sound,
        "TypeSelect feedback is a silent message add"
    );
}

#[cfg(test)]
fn add_type_select_feedback(
    list: &mut crate::ui::messages::MessageList,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    csf_key: &str,
    rgb: [f32; 3],
    now_ms: u64,
    measure: &dyn Fn(&str) -> i32,
) -> crate::ui::messages::AddOutcome {
    add_localized_message(
        list,
        csf,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: csf_key,
            rgb,
            timeout_ms: Some(TYPE_SELECT_MESSAGE_TIMEOUT_MS),
            silent: true,
        },
        now_ms,
        measure,
    )
}

fn add_localized_message(
    list: &mut crate::ui::messages::MessageList,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    post: &crate::ui::messages::MessagePost<'_>,
    now_ms: u64,
    measure: &dyn Fn(&str) -> i32,
) -> crate::ui::messages::AddOutcome {
    let text = csf
        .map(|table| table.text(post.text))
        .unwrap_or(std::borrow::Cow::Borrowed(post.text));
    list.add_message(
        &crate::ui::messages::MessagePost {
            text: text.as_ref(),
            ..*post
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
    let rgb = house_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| rules.color_schemes.as_slice()),
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

/// Local message facts share one ordered projection with the Supers' lines.
/// Admission/shortfall guard belongs to House, not to this presentation enum.
/// Native caller arguments: tools/spatial_oracle/house_power_consumers.py;
/// shared MessageDelay controls: trigger_action_values.json/localized_message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudMessage {
    LowPower,
    SuperWeapon(&'static str),
    /// MPlayer_Defeated's line for a non-observer House (`0x004FC271`,
    /// `0x004FC34F`).
    PlayerDefeated(crate::sim::intern::InternedId),
}

impl HudMessage {
    fn csf_key(self) -> &'static str {
        match self {
            Self::LowPower => "TXT_LOW_POWER", // original House4F8D4A
            Self::SuperWeapon(key) => key,
            Self::PlayerDefeated(_) => "TXT_PLAYER_DEFEATED",
        }
    }

    fn timeout_ms(self, rules: &crate::rules::ruleset::RuleSet) -> Option<u64> {
        match self {
            // House4F8D20..31 uses the same MessageDelay*900/ftol as
            // TAction11; these are 16 ms clock buckets, never speed normalized.
            Self::LowPower | Self::PlayerDefeated(_) => {
                message_delay_timeout_ms(rules.general.message_delay_minutes)
            }
            Self::SuperWeapon(_) => Some(SUPER_WEAPON_MESSAGE_TIMEOUT_MS),
        }
    }

    fn post(
        self,
        rules: &crate::rules::ruleset::RuleSet,
        rgb: [f32; 3],
    ) -> crate::ui::messages::MessagePost<'static> {
        crate::ui::messages::MessagePost {
            prefix: None,
            text: self.csf_key(),
            rgb,
            timeout_ms: self.timeout_ms(rules),
            silent: false,
        }
    }
}

fn message_delay_timeout_ms(delay: crate::util::native_x87::NativeF64Bits) -> Option<u64> {
    let buckets = message_delay_buckets(delay);
    (buckets != -1).then(|| u64::try_from(buckets).unwrap_or(0) * 16)
}

/// House4F8D5C and the Super print callers use MessageList5D3BA0 with
/// PlayerPtr's color, no sender, and silent0. Keep the existing list/clock,
/// localization and IncomingMessage owners; no warning timer/guard lives here.
/// The callers' text flags still share the existing bitmap-text lane; this
/// transport does not establish native glyph/style raster equivalence.
pub(crate) fn post_hud_message(state: &mut AppState, message: HudMessage) {
    if let HudMessage::PlayerDefeated(house) = message {
        post_house_line(state, message.csf_key(), house, HouseLineColor::Player);
        return;
    }
    let Some(rules) = state.rules() else {
        return;
    };
    let rgb = house_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        Some(&rules.color_schemes),
    );
    let post = message.post(rules, rgb);
    post_localized_message(state, &post);
}

/// The colour of a House-named line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HouseLineColor {
    /// The player's colour scheme: MPlayer_Defeated (House+0x16054 remote,
    /// Session+0x15C local, which VERA reads as the House's own scheme).
    Player,
    /// The HouseType's `Color=` scheme `[[House+0x34]+0xC0]`:
    /// DestroyConnection (`0x005DA827..0x005DA834`).
    Country,
}

/// A House-named line: the CSF template formatted with House+0x1602A, for
/// MessageDelay*900 buckets, silent 0. DestroyConnection5DA750 posts
/// TXT_LEFT_GAME/TXT_CONNECTION_LOST (name read at 5DA772); MPlayer_Defeated
/// posts TXT_PLAYER_DEFEATED (4FC271..4FC2DB local, 4FC34F..4FC3AD remote).
pub(crate) fn post_house_line(
    state: &mut AppState,
    key: &str,
    house: crate::sim::intern::InternedId,
    color: HouseLineColor,
) {
    let Some(name) = crate::app::presentation::house_ui::house_ui_name(state, house) else {
        return;
    };
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| &runtime.simulation)
    else {
        return;
    };
    let owner = sim.interner().resolve(house).to_string();
    let country = sim
        .houses()
        .get(&house)
        .and_then(|state| state.country())
        .map(|country| sim.interner().resolve(country).to_string());
    let Some(rules) = state.rules() else {
        return;
    };
    let rgb = match color {
        HouseLineColor::Player => house_message_rgb(
            Some(&owner),
            &state.match_state.match_presentation.house_color_map,
            Some(&rules.color_schemes),
        ),
        HouseLineColor::Country => {
            // HouseType+0xC0 holds 2*entry+1 (ReadColor 0x474A90).
            let scheme = country.map_or(0, |country| rules.country_color_scheme(&country));
            country_message_rgb(scheme, &rules.color_schemes)
        }
    };
    let timeout_ms = message_delay_timeout_ms(rules.general.message_delay_minutes);
    let template = crate::app::frontend::shell_pass::resolve_csf(state, key).into_owned();
    let text = crate::assets::csf_file::format_csf(
        &template,
        &[crate::assets::csf_file::CsfArg::Str(&name)],
    );
    post_resolved_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: &text,
            rgb,
            timeout_ms,
            silent: false,
        },
    );
}

/// MapPlayerColorToScheme `0x0069A310` of a player's Session colour: VERA
/// reads it as the House's own scheme, as MPlayer_Defeated's lines do.
pub(crate) fn player_line_rgb(state: &AppState, owner: crate::sim::intern::InternedId) -> [f32; 3] {
    let owner = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| runtime.simulation.interner().resolve(owner).to_string());
    house_message_rgb(
        owner.as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| rules.color_schemes.as_slice()),
    )
}

/// A chat line: Add_Message with the sender's name as prefix, style 4046,
/// MessageDelay*900 buckets and silent 0, so IncomingMessage plays once
/// (`0x0055F0A8..0x0055F104` echo, `0x0048D93A..0x0048D98D` receive).
pub(crate) fn post_chat_line(state: &mut AppState, name: &str, text: &str, rgb: [f32; 3]) {
    let Some(rules) = state.rules() else {
        return;
    };
    let timeout_ms = message_delay_timeout_ms(rules.general.message_delay_minutes);
    post_resolved_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: Some(name),
            text,
            rgb,
            timeout_ms,
            silent: false,
        },
    );
}

/// The observer gate's silent `TXT_OBSERVER_CANT_SEND` row in the "Grey"
/// scheme (`0x004E12D0` with `0x00822728`), flags 0x16, MessageDelay*900
/// (`0x0055EED0..0x0055EF2C`).
pub(crate) fn post_observer_refusal(state: &mut AppState) {
    let Some(rules) = state.rules() else {
        return;
    };
    let timeout_ms = message_delay_timeout_ms(rules.general.message_delay_minutes);
    let rgb = crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, "Grey")
        .and_then(|entry| u8::try_from(entry).ok())
        .map_or(MESSAGE_RGB_SYSTEM, |entry| {
            crate::rules::house_colors::house_text_rgb(
                &rules.color_schemes,
                crate::rules::house_colors::HouseColorIndex(entry),
            )
            .map(|channel| f32::from(channel) / 255.0)
        });
    post_localized_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: "TXT_OBSERVER_CANT_SEND",
            rgb,
            timeout_ms,
            silent: true,
        },
    );
}

/// Post a row whose `text` is a CSF key.
fn post_localized_message(state: &mut AppState, post: &crate::ui::messages::MessagePost<'_>) {
    post_message(state, post, true);
}

/// Post a row whose `text` is already formatted from its CSF template.
fn post_resolved_message(state: &mut AppState, post: &crate::ui::messages::MessagePost<'_>) {
    post_message(state, post, false);
}

fn post_message(state: &mut AppState, post: &crate::ui::messages::MessagePost<'_>, localize: bool) {
    sync_view(state);
    let now = message_now_ms(state);
    let font = &state.renderer.bit_font;
    let outcome = add_localized_message(
        &mut state.match_state.match_presentation.message_list,
        state.process_assets.csf.as_ref().filter(|_| localize),
        post,
        now,
        &|s| font.text_width(s) as i32,
    );
    if outcome.play_sound {
        let sound = state
            .rules()
            .and_then(|r| r.general.incoming_message_sound().map(str::to_owned));
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
}

/// TextLabel72A4A0 reads scheme+308's original HSV, unlike the unit
/// palette ramp. Shared by TypeSelect732BF0, HUD730AC3, HealthNav7335A7,
/// Super prints53AE42/53B452 and House4F8D5C. Native caller-null fallback3
/// is [Colors] entry1; TextLabel72A4D6's invalid-index fallback is scheme0.
/// Source: tools/procedural_drawing_oracle/house_color.md, message consumer.
/// A runtime scheme index through TextLabel's invalid-index fallback to
/// scheme 0 (`0x0072A4D6`).
fn country_message_rgb(
    scheme: i32,
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
) -> [f32; 3] {
    let entry = u8::try_from(scheme / 2)
        .ok()
        .filter(|entry| usize::from(*entry) < schemes.len())
        .unwrap_or(0);
    crate::rules::house_colors::house_text_rgb(
        schemes,
        crate::rules::house_colors::HouseColorIndex(entry),
    )
    .map(|channel| f32::from(channel) / 255.0)
}

fn house_message_rgb(
    local_owner: Option<&str>,
    house_colors: &crate::map::houses::HouseColorMap,
    schemes: Option<&[crate::rules::color_scheme::ColorSchemeEntry]>,
) -> [f32; 3] {
    let Some(schemes) = schemes else {
        return MESSAGE_RGB_SYSTEM;
    };
    let mut scheme = local_owner
        .and_then(|owner| house_colors.get(owner).copied())
        .unwrap_or(TYPE_SELECT_FALLBACK_SCHEME_ENTRY);
    if usize::from(scheme.0) >= schemes.len() {
        scheme = crate::rules::house_colors::HouseColorIndex(0);
    }
    crate::rules::house_colors::house_text_rgb(schemes, scheme)
        .map(|channel| f32::from(channel) / 255.0)
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
    let typing = state
        .frontend
        .multiplayer_shell_runtime
        .scroll_text(state.match_state.session.mode());
    state
        .match_state
        .match_presentation
        .message_list
        .set_typing(typing);
    // The list spans the tactical viewport: the frame left of the sidebar.
    let tactical_w = crate::app::input::camera::tactical_viewport_size(state).0 as i32;
    state.match_state.match_presentation.message_list.set_view(
        MESSAGE_X_INSET,
        0,
        (tactical_w - MESSAGE_WIDTH_INSET).max(0),
    );
}

/// Prepare one successful offline TextLabel paint, using raw wall time for
/// typing and the existing sound owner for every MessageCharTyped request.
/// Expiry remains on the separate pause-aware clock in update().
pub(crate) fn build_message_instances(
    state: &mut AppState,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    if state.frontend.screen != GameScreen::InGame {
        return (Vec::new(), Vec::new());
    }
    let wall_ms = crate::app::input::tooltips::now_ms(state) as u32;
    let requests = state
        .match_state
        .match_presentation
        .message_list
        .advance_reveal(wall_ms);
    if requests > 0 {
        let sound = state
            .rules()
            .and_then(|rules| rules.general.message_char_typed_sound().map(str::to_owned));
        for _ in 0..requests {
            crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
        }
    }
    message_instances(
        &state.renderer.bit_font,
        &state.match_state.match_presentation.message_list,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    )
}

fn message_instances(
    font: &crate::render::bit_font::BitFont,
    list: &crate::ui::messages::MessageList,
    camera_offset: [f32; 2],
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let x = list.x() as f32;
    let mut fills = Vec::new();
    let mut texts = Vec::new();
    // Draw 0x005D49A0 paints the edit label first, whole: its reveal flag
    // is never set and the chat cursor character 0 draws no caret.
    let edit = list
        .edit_line()
        .map(|edit| crate::ui::messages::Message::edit_row(&edit));
    for m in edit.iter().chain(list.messages()) {
        // Message rows are screen-space, but `draw_pooled_ui` binds the
        // live world camera and the shader subtracts it. Add that camera
        // back here so scrolling cannot displace the HUD text.
        let text = font.build_text_label(
            &m.text,
            x,
            m.y as f32,
            0.00022,
            camera_offset,
            m.rgb.map(|channel| (channel * 255.0).round() as u8),
            m.reveal_count(),
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
                i64::from(message_delay_buckets(delay)),
                message["duration"].as_i64().unwrap(),
                "{row} native ftol duration"
            );
            let buckets = message["duration"].as_i64().unwrap();
            assert_eq!(
                HudMessage::LowPower.timeout_ms(&rules),
                (buckets != -1).then(|| u64::try_from(buckets).unwrap_or(0) * 16),
                "House and Action11 share the native conversion"
            );
        }
        // The stock `.6` spelling and the native control's `0.6` both pass
        // through the same production ReadDouble owner. Constructor omission
        // has distinct observed bits and is covered above.
        let Some(stock) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let recorded = rows.iter().find(|row| row["input_delay"] == "0.6").unwrap();
        assert_eq!(
            stock.general.message_delay_minutes.bits(),
            original_delay_bits(recorded)
        );
        assert_eq!(
            i64::from(message_delay_buckets(stock.general.message_delay_minutes)),
            recorded["calls"].as_array().unwrap().last().unwrap()["duration"]
                .as_i64()
                .unwrap()
        );
    }

    #[test]
    fn trigger_message_scenario_side_selects_original_runtime_scheme_in_stock_colors() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
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
    fn house_low_power_native_post_reaches_retail_text_and_pause_aware_expiry() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let bytes = assets
            .load_file_from_mix("ra2md.csf")
            .expect("active-YR CSF");
        let csf = crate::assets::csf_file::CsfFile::from_bytes(&bytes.bytes).unwrap();
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_power_consumers.json",
        ))
        .unwrap();
        let native = corpus["advice_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "local_short_first")
            .unwrap();
        let presentation = native["receipt"]["presentation"].as_array().unwrap();
        let lookup = presentation
            .iter()
            .find(|call| call["kind"] == "text_lookup")
            .unwrap();
        // Original file-backed label at824714 is TXT_LOW_POWER, not the
        // synthetic UTF16 pointer the native harness supplies to MessageList.
        assert_eq!(lookup["key"], 0x824714);
        let message = presentation
            .iter()
            .find(|call| call["kind"] == "message")
            .unwrap();
        let args = message["args"].as_array().unwrap();
        assert_eq!((args[0].as_i64(), args[1].as_i64()), (Some(0), Some(0)));
        assert_eq!(args[4], 0x4046);
        let mut colors = crate::map::houses::HouseColorMap::new();
        colors.insert(
            "Americans".into(),
            crate::rules::house_colors::HouseColorIndex(0),
        );
        let rgb = house_message_rgb(
            Some("Americans"),
            &colors,
            Some(&retail.rules.color_schemes),
        );
        let post = HudMessage::LowPower.post(&retail.rules, rgb);
        assert!(post.prefix.is_none());
        assert_eq!(post.text, "TXT_LOW_POWER");
        assert_eq!(post.silent, args[6] != 0);
        assert_eq!(post.timeout_ms, Some(args[5].as_u64().unwrap() * 16));
        assert_ne!(
            csf.text(post.text).as_ref(),
            post.text,
            "physical localized label exists"
        );

        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 1_000);
        let mut clock = crate::ui::messages::PauseAwareClock::default();
        let now = 1_200;
        let outcome =
            add_localized_message(&mut list, Some(&csf), &post, clock.now(now), &|text| {
                text.chars().count() as i32 * 8
            });
        assert_eq!(outcome.added, 1);
        assert!(
            outcome.play_sound,
            "native silent0 requests IncomingMessage"
        );
        assert_eq!(list.messages()[0].text, csf.text(post.text));
        assert_eq!(list.messages()[0].rgb, rgb);
        let deadline = now + post.timeout_ms.unwrap();
        assert_eq!(list.messages()[0].deadline_ms, Some(deadline));
        let font = crate::render::bit_font::tests::make_test_font(
            &[(b'L' as u16, 6), (b'o' as u16, 6)],
            4,
        );
        assert_eq!(list.advance_reveal(now as u32), 1);
        assert_eq!(message_instances(&font, &list, [0.0; 2]).1.len(), 1);
        assert_eq!(list.advance_reveal(now as u32 + 16), 1);
        assert_eq!(message_instances(&font, &list, [0.0; 2]).1.len(), 2);
        clock.set_paused(true, now + 100);
        assert!(!list.manage(clock.now(deadline + 20_000)));
        clock.set_paused(false, deadline + 20_000);
        let resumed_deadline = deadline + 20_000 + post.timeout_ms.unwrap() - 100;
        assert!(
            !list.manage(clock.now(resumed_deadline)),
            "deadline equality stays visible"
        );
        assert!(list.manage(clock.now(resumed_deadline + 1)));
        assert!(list.messages().is_empty());

        // Admission failure cannot play the message-insertion sound.
        list.set_view(3, 0, 0);
        let rejected = add_localized_message(
            &mut list,
            Some(&csf),
            &post,
            clock.now(resumed_deadline + 2),
            &|_| 8,
        );
        assert_eq!(rejected.added, 0);
        assert!(!rejected.play_sound);
    }

    #[test]
    fn item83_type_select_feedback_uses_local_scheme_color_zero_and_runtime_three_fallback() {
        use crate::rules::color_scheme::ColorSchemeEntry;
        use crate::rules::house_colors::HouseColorIndex;
        let schemes = [25, 43, 153].map(|hue| ColorSchemeEntry {
            name: format!("Hue{hue}"),
            hsv: [hue, 255, 255],
        });
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/color_oracle/hsv_to_rgb.json",
        ))
        .unwrap();
        let row = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["saturation"] == 255 && row["value"] == 255)
            .unwrap();
        let rgb = row["rgb_by_hue"].as_str().unwrap();
        let expected = |hue: usize| {
            std::array::from_fn(|channel| {
                let at = hue * 6 + channel * 2;
                f32::from(u8::from_str_radix(&rgb[at..at + 2], 16).unwrap()) / 255.0
            })
        };
        let mut house_colors = crate::map::houses::HouseColorMap::new();
        house_colors.insert("Americans".into(), HouseColorIndex(2));
        assert_eq!(
            house_message_rgb(Some("Americans"), &house_colors, Some(&schemes)),
            expected(153)
        );
        assert_eq!(
            house_message_rgb(None, &house_colors, Some(&schemes)),
            expected(43),
            "a null House uses runtime3/Colors entry1"
        );
        house_colors.insert("Americans".into(), HouseColorIndex(255));
        assert_eq!(
            house_message_rgb(Some("Americans"), &house_colors, Some(&schemes)),
            expected(25),
            "TextLabel falls back to runtime0 for an invalid scheme"
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
