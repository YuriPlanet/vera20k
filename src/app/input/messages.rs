//! Chat/system message surface driver (study §3.1): anchors the
//! `ui::messages::MessageList` to the tactical viewport,
//! posts system messages (insert sound = [AudioVisual] IncomingMessage),
//! expires rows on Main_Tick against the advancing process wall clock,
//! and builds the text instances drawn between sidebar text and tooltip.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::ui::game_screen::GameScreen;

/// Fallback while match color schemes are unavailable.
const MESSAGE_RGB_SYSTEM: [f32; 3] = [1.0, 1.0, 1.0];
/// `0xF0` native 16 ms timer buckets.
const TYPE_SELECT_MESSAGE_TIMEOUT_MS: u64 = 3_840;
/// The Supers' lines' `0x96` native 16 ms timer buckets.
const SUPER_WEAPON_MESSAGE_TIMEOUT_MS: u64 = 0x96 * 16;
/// PlaceBeacon's help lines' `0xE1` buckets (`0x00430D01`, `0x00430D38`).
const BEACON_HELP_TIMEOUT_MS: u64 = 0xE1 * 16;
/// Native falls back to runtime color-scheme 3. Rust stores the undoubled
/// `[Colors]` entry index, so that scheme is entry 1.
const TYPE_SELECT_FALLBACK_SCHEME_ENTRY: crate::rules::house_colors::HouseColorIndex =
    crate::rules::house_colors::HouseColorIndex(3 / 2);

/// Add_Message5D3DD5 and Manage5D4449 read process timer887338/887340.
/// Menu/Scenario pause stops polling, not this clock. EnterPause683EB0 and
/// ExitPause683FB0 only stop/rearm the separate Scenario614 score clock.
/// Source: original gamemd.exe instructions and message-pause native controls.
pub(crate) fn message_now_ms(state: &AppState) -> u64 {
    crate::app::input::tooltips::now_ms(state)
}

/// Original TAction11 6DE0B0..6DE122 resolves its CSF label, selects the
/// scenario-side priority through69A310 and adds a non-silent4046 row.
/// MessageDelay's ftol result is in WallClock16ms buckets, not sim frames.
pub(crate) fn post_trigger_message(state: &mut AppState, csf_key: &str) {
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return;
    };
    // The selected campaign opening keeps its startup player/country. The
    // existing House owner supplies that side; a later scenario-side change
    // independent of the current House remains a separate retained-field gap.
    // Ares `TActionClass_Execute_MessageColor @ 0x1002FA80` reads the saved
    // Scenario.PlayerSideIndex instead.
    #[cfg(feature = "ares")]
    let side = runtime.simulation.session().scenario_side();
    #[cfg(not(feature = "ares"))]
    let side = runtime
        .simulation
        .session()
        .current_house()
        .and_then(|owner| runtime.simulation.houses().get(&owner))
        .map_or(0, |house| i32::from(house.side_index()));
    let rules = &runtime.resources.rules;
    #[cfg(feature = "ares")]
    let rgb = ares_message_text_rgb(rules, side).unwrap_or_else(|| {
        trigger_message_rgb(&rules.color_schemes, side, rules.skirmish_colors())
    });
    #[cfg(not(feature = "ares"))]
    let rgb = trigger_message_rgb(&rules.color_schemes, side, rules.skirmish_colors());
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

/// Ares `[Side] MessageTextColor=`: the stored runtime ColorScheme index (two
/// schemes per `[Colors]` entry) resolves to that entry. `-1`/an
/// out-of-range entry leaves the stock side-priority path unchanged.
#[cfg(feature = "ares")]
fn ares_message_text_rgb(rules: &crate::rules::ruleset::RuleSet, side: i32) -> Option<[f32; 3]> {
    let index = rules.side_message_text_color_index(side)?;
    if index < 0 {
        return None;
    }
    let entry = usize::try_from(index).ok()? / 2;
    crate::rules::color_scheme::scheme_hsv_by_entry(&rules.color_schemes, entry)
        .map(crate::rules::color_scheme::hsv_to_rgb)
        .map(|rgb| rgb.map(|channel| f32::from(channel) / 255.0))
}

fn trigger_message_rgb(
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
    side: i32,
    skirmish_colors: crate::rules::skirmish_colors::SkirmishColorMode,
) -> [f32; 3] {
    let priority = match side {
        0 => 2,
        1 => 1,
        _ => 5,
    };
    let entry = skirmish_colors.entry_for_priority(priority);
    crate::rules::color_scheme::scheme_hsv_by_entry(schemes, entry)
        .map(crate::rules::color_scheme::hsv_to_rgb)
        .unwrap_or([255; 3])
        .map(|channel| f32::from(channel) / 255.0)
}

/// Post the localized, silent HUD result of one executed TypeSelect tap.
pub(crate) fn post_type_select_feedback(state: &mut AppState, csf_key: &str) {
    post_silent_house_feedback(state, csf_key, TYPE_SELECT_MESSAGE_TIMEOUT_MS);
}

/// PlanMgr's refusals (63A0D0/639FD0/639130/638120/637270) and help
/// (63A120/63A1B0) use the same silent730A90 HUD owner with duration1E0.
/// A refusal's separate ScoldSound belongs to the planning input owner.
pub(crate) fn post_planning_message(state: &mut AppState, csf_key: &str) {
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
    /// The configured label/style is resolved from its type at presentation.
    #[cfg(feature = "ares")]
    AresSuperWeaponRefused(crate::sim::intern::InternedId),
    /// SWTypeExt::PrintMessage10033BD0 of one `Message.*` label, with the
    /// House its Message.FirerColor would use (None: Gold).
    #[cfg(feature = "ares")]
    AresSuperWeaponMessage {
        sw_type: crate::sim::intern::InternedId,
        message: crate::rules::superweapon_type::AresSuperWeaponMessage,
        firer: Option<crate::sim::intern::InternedId>,
    },
    /// A captured tech building's `Message.Lost`/`Message.Capture` label
    /// (Ares `BuildingClass_ChangeOwnership_Tech @ 10013E40`'s PrintMessage
    /// lambda): PlayerPtr's colour, MessageDelay through the CRT `_ftol2`
    /// (`100772B0`), silent 0.
    #[cfg(feature = "ares")]
    AresTechCaptureLabel(crate::sim::intern::InternedId),
    /// MPlayer_Defeated's line for a non-observer House (`0x004FC271`,
    /// `0x004FC34F`).
    PlayerDefeated(crate::sim::intern::InternedId),
    /// MakeAlly's `TXT_HAS_ALLIED` or MakeEnemy's `TXT_AT_WAR` (owner,
    /// other) in the owner's HouseType colour (`0x004F9D4E..0x004F9DFC`,
    /// `0x004FA0E4..0x004FA175`).
    Alliance {
        owner: crate::sim::intern::InternedId,
        other: crate::sim::intern::InternedId,
        formed: bool,
    },
    /// PlaceBeacon's `TXT_BEACON_HELP1`/`2` for the local House's beacon
    /// (`0x00430CF9..0x00430D69`): PlayerPtr's scheme `+0x16054`, style
    /// 4046, `0xE1` buckets, silent 0.
    BeaconHelp(&'static str),
    /// VERA's optional detailed EVA text (`[VERA] DetailedEvaText=`, default
    /// off): one line naming the object an EVA voice spoke about without
    /// naming it. The audio dispatch arm that admitted the voice collects it,
    /// so text and voice share every gate. `label` selects the VERA template;
    /// `type_id` is the sim's own type, never a lookup or a guess.
    EvaObjectLine {
        label: &'static str,
        type_id: crate::sim::intern::InternedId,
    },
}

impl HudMessage {
    fn csf_key(self) -> Option<&'static str> {
        Some(match self {
            #[cfg(feature = "ares")]
            Self::AresSuperWeaponRefused(_)
            | Self::AresSuperWeaponMessage { .. }
            | Self::AresTechCaptureLabel(_) => return None,
            Self::LowPower => "TXT_LOW_POWER", // original House4F8D4A
            Self::SuperWeapon(key) | Self::BeaconHelp(key) => key,
            Self::PlayerDefeated(_) => "TXT_PLAYER_DEFEATED",
            Self::Alliance { formed: true, .. } => "TXT_HAS_ALLIED",
            Self::Alliance { formed: false, .. } => "TXT_AT_WAR",
            Self::EvaObjectLine { .. } => return None,
        })
    }

    fn timeout_ms(self, rules: &crate::rules::ruleset::RuleSet) -> Option<u64> {
        match self {
            // House4F8D20..31 uses the same MessageDelay*900/ftol as
            // TAction11; these are 16 ms clock buckets, never speed normalized.
            Self::LowPower | Self::PlayerDefeated(_) | Self::Alliance { .. } => {
                message_delay_timeout_ms(rules.general.message_delay_minutes)
            }
            Self::SuperWeapon(_) => Some(SUPER_WEAPON_MESSAGE_TIMEOUT_MS),
            #[cfg(feature = "ares")]
            Self::AresSuperWeaponRefused(_)
            | Self::AresSuperWeaponMessage { .. }
            | Self::AresTechCaptureLabel(_) => {
                ares_message_delay_timeout_ms(rules.general.message_delay_minutes)
            }
            Self::BeaconHelp(_) => Some(BEACON_HELP_TIMEOUT_MS),
            // `post_hud_message` posts these through `post_eva_text`.
            Self::EvaObjectLine { .. } => None,
        }
    }

    fn post(
        self,
        rules: &crate::rules::ruleset::RuleSet,
        rgb: [f32; 3],
    ) -> Option<crate::ui::messages::MessagePost<'static>> {
        Some(crate::ui::messages::MessagePost {
            prefix: None,
            text: self.csf_key()?,
            rgb,
            timeout_ms: self.timeout_ms(rules),
            silent: false,
        })
    }
}

fn message_delay_timeout_ms(delay: crate::util::native_x87::NativeF64Bits) -> Option<u64> {
    let buckets = message_delay_buckets(delay);
    message_timeout_ms_from_buckets(buckets)
}

fn message_timeout_ms_from_buckets(buckets: i32) -> Option<u64> {
    (buckets != -1).then(|| u64::try_from(buckets).unwrap_or(0) * 16)
}

/// Ares PrintMessage10033BD0 uses CRT100772B0's signed32 conversion,
/// unlike the stock callers' Math7C5F00 low dword. Reuse the numeric owner.
#[cfg(feature = "ares")]
fn ares_message_delay_timeout_ms(delay: crate::util::native_x87::NativeF64Bits) -> Option<u64> {
    use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
    let scaled = X::mul(
        X::load_f64(delay),
        X::load_f64(NativeF64Bits::from_bits(0x408c_2000_0000_0000)),
    );
    message_timeout_ms_from_buckets(X::fistp_i32_masked(scaled))
}

/// Installed Ares PrintMessage10033BD0: retain the caller's label, choose
/// firer/configured/player colour (Gold only for a null house), MessageDelay
/// in900 wall-clock buckets, no prefix and silent0. The existing message list
/// owns CSF lookup, empty-text admission, repeated rows, expiry and insert audio.
#[cfg(feature = "ares")]
fn ares_message_post<'a>(
    settings: &'a crate::rules::superweapon_type::AresSuperWeaponMessages,
    message: crate::rules::superweapon_type::AresSuperWeaponMessage,
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
    colors: &crate::map::houses::HouseColorMap,
    firer: Option<&str>,
    player: Option<&str>,
    delay: crate::util::native_x87::NativeF64Bits,
) -> Option<crate::ui::messages::MessagePost<'a>> {
    let text = settings.label(message);
    if text.is_empty() {
        return None;
    }
    let house = if settings.firer_color() {
        firer
    } else {
        player
    };
    let rgb = if !settings.firer_color() && settings.color_scheme() >= 0 {
        country_message_rgb(settings.color_scheme(), schemes)
    } else if let Some(house) = house {
        house_message_rgb(Some(house), colors, Some(schemes))
    } else {
        // PrintMessage10033BD0 calls ColorScheme::Find("Gold", 1): the even member.
        let gold = crate::rules::color_scheme::scheme_entry_by_name(schemes, "Gold")
            .map_or(-1, |entry| (entry as i32).wrapping_mul(2));
        country_message_rgb(gold, schemes)
    };
    Some(crate::ui::messages::MessagePost {
        prefix: None,
        text,
        rgb,
        timeout_ms: ares_message_delay_timeout_ms(delay),
        silent: false,
    })
}

/// A cameo press refusal's line, with the local player as the firer: the
/// select hook 10034DA0 prints only the funds and CannotFire labels.
#[cfg(feature = "ares")]
pub(crate) fn post_ares_superweapon_refusal(
    state: &mut AppState,
    section: &str,
    refusal: crate::sim::superweapon::AresCameoRefusal,
) {
    use crate::rules::superweapon_type::AresSuperWeaponMessage as M;
    use crate::sim::superweapon::AresCameoRefusal as R;
    let message = match refusal {
        R::CannotFire => M::CannotFire,
        R::InsufficientFunds => M::InsufficientFunds,
        R::NotReady | R::ManualFireDisabled | R::Unstoppable => return,
    };
    let owner = crate::app::input::commands::preferred_local_owner_name(state);
    post_ares_superweapon_message(state, section, message, owner.as_deref());
}

/// SWTypeExt::PrintMessage10033BD0 of one label of `section`, for `firer`
/// (the House whose colour Message.FirerColor picks; None prints Gold).
#[cfg(feature = "ares")]
pub(crate) fn post_ares_superweapon_message(
    state: &mut AppState,
    section: &str,
    message: crate::rules::superweapon_type::AresSuperWeaponMessage,
    firer: Option<&str>,
) {
    let Some(settings) = state
        .rules()
        .and_then(|rules| rules.super_weapon(section))
        .map(|sw| sw.ares_messages().clone())
    else {
        return;
    };
    let player = crate::app::input::commands::preferred_local_owner_name(state);
    let Some(rules) = state.rules() else {
        return;
    };
    let Some(post) = ares_message_post(
        &settings,
        message,
        &rules.color_schemes,
        &state.match_state.match_presentation.house_color_map,
        firer,
        player.as_deref(),
        rules.general.message_delay_minutes,
    ) else {
        return;
    };
    post_localized_message(state, &post);
}

/// House4F8D5C and the Super print callers use MessageList5D3BA0 with
/// PlayerPtr's color, no sender, and silent0. Keep the existing list/clock,
/// localization and IncomingMessage owners; no warning timer/guard lives here.
/// The callers' text flags still share the existing bitmap-text lane; this
/// transport does not establish native glyph/style raster equivalence.
pub(crate) fn post_hud_message(state: &mut AppState, message: HudMessage) {
    #[cfg(feature = "ares")]
    if let HudMessage::AresSuperWeaponRefused(sw_type) = message {
        let section = state.match_state.sim_runtime.runtime().and_then(|runtime| {
            runtime
                .simulation
                .interner()
                .try_resolve(sw_type)
                .map(str::to_owned)
        });
        if let Some(section) = section {
            post_ares_superweapon_refusal(
                state,
                &section,
                crate::sim::superweapon::AresCameoRefusal::CannotFire,
            );
        }
        return;
    }
    #[cfg(feature = "ares")]
    if let HudMessage::AresSuperWeaponMessage {
        sw_type,
        message,
        firer,
    } = message
    {
        let names = state.match_state.sim_runtime.runtime().and_then(|runtime| {
            let interner = runtime.simulation.interner();
            Some((
                interner.try_resolve(sw_type)?.to_owned(),
                firer.and_then(|firer| interner.try_resolve(firer).map(str::to_owned)),
            ))
        });
        if let Some((section, firer)) = names {
            post_ares_superweapon_message(state, &section, message, firer.as_deref());
        }
        return;
    }
    #[cfg(feature = "ares")]
    if let HudMessage::AresTechCaptureLabel(label) = message {
        let Some(label) = state.match_state.sim_runtime.runtime().and_then(|runtime| {
            runtime
                .simulation
                .interner()
                .try_resolve(label)
                .map(str::to_owned)
        }) else {
            return;
        };
        let Some(rules) = state.rules() else {
            return;
        };
        let post = crate::ui::messages::MessagePost {
            prefix: None,
            text: &label,
            rgb: house_message_rgb(
                crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
                &state.match_state.match_presentation.house_color_map,
                Some(&rules.color_schemes),
            ),
            timeout_ms: message.timeout_ms(rules),
            silent: false,
        };
        post_localized_message(state, &post);
        return;
    }
    if let HudMessage::EvaObjectLine { label, type_id } = message {
        if let Some(name) = eva_text_product_name(state, type_id) {
            post_eva_text(state, label, &name);
        }
        return;
    }
    let Some(key) = message.csf_key() else {
        return;
    };
    if let HudMessage::PlayerDefeated(house) = message {
        post_house_line(state, key, house, HouseLineColor::Player);
        return;
    }
    if let HudMessage::Alliance { owner, other, .. } = message {
        post_house_names_line(state, key, &[owner, other], HouseLineColor::Country);
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
    if let Some(post) = message.post(rules, rgb) {
        post_localized_message(state, &post);
    }
}

/// The visible name of a sim type, through the owner every tooltip uses
/// ([`crate::assets::csf_file::type_ui_name`]); a type with no `UIName=`
/// keeps its rules key (e.g. `HTNK`).
fn eva_text_product_name(
    state: &AppState,
    type_id: crate::sim::intern::InternedId,
) -> Option<String> {
    let section = state
        .match_state
        .sim_runtime
        .runtime()?
        .simulation
        .interner()
        .resolve(type_id);
    eva_text_name(section, state.rules()?, state.process_assets.csf.as_ref())
}

fn eva_text_name(
    section: &str,
    rules: &crate::rules::ruleset::RuleSet,
    csf: Option<&crate::assets::csf_file::CsfFile>,
) -> Option<String> {
    let object = rules.object(section)?;
    let name = crate::assets::csf_file::type_ui_name(object.ui_name.as_deref(), csf);
    Some(if name.is_empty() {
        section.to_owned()
    } else {
        name
    })
}

/// The sidebar speaks `EVA_Building` / `EVA_Training` without naming the
/// object, while the clicked cameo carries its type key. Only these two lines
/// get a row: hold/resume arms speak the same line and name the resumed type,
/// while on-hold, unable-to-comply and select-target arms speak other lines.
/// The cancel line has its own row; see [`post_eva_cancel`].
pub(crate) fn post_eva_production_start(state: &mut AppState, eva_line: &str, type_key: &str) {
    let label = match eva_line {
        crate::app::input::sidebar_eva::EVA_BUILDING => "VERA:EvaBuildingStarted",
        crate::app::input::sidebar_eva::EVA_TRAINING => "VERA:EvaTrainingStarted",
        _ => return,
    };
    post_eva_type_line(state, label, type_key);
}

/// `EVA_Canceled` (`SelectClass::Action @ 0x006AAD00`, `0x006AAE39`) names
/// nothing either, and a single `ABANDON` removed exactly the clicked cameo's
/// type. `structure` comes from the strip that owns the click.
pub(crate) fn post_eva_cancel(state: &mut AppState, type_key: &str, structure: bool) {
    let label = if structure {
        "VERA:EvaBuildingCanceled"
    } else {
        "VERA:EvaTrainingCanceled"
    };
    post_eva_type_line(state, label, type_key);
}

fn post_eva_type_line(state: &mut AppState, label: &str, type_key: &str) {
    if !state.persistence.options_profile.detailed_eva_text {
        return;
    }
    let Some(name) = state
        .rules()
        .and_then(|rules| eva_text_name(type_key, rules, state.process_assets.csf.as_ref()))
    else {
        return;
    };
    post_eva_text(state, label, &name);
}

/// Post one detailed EVA text row from the VERA template `label`, with the
/// object's `{name}` filled in. The rows are subtitles, not chat: `silent`
/// skips the `IncomingMessage` insert sound and the typing reveal, so the
/// EVA voice stays the only sound.
fn post_eva_text(state: &mut AppState, label: &str, name: &str) {
    if !state.persistence.options_profile.detailed_eva_text {
        return;
    }
    let Some(template) = crate::i18n::find(label) else {
        return;
    };
    let text = crate::i18n::fill(&template, &[("name", &name)]);
    let Some(rules) = state.rules() else {
        return;
    };
    let timeout_ms = message_delay_timeout_ms(rules.general.message_delay_minutes);
    let rgb = house_message_rgb(
        crate::app::input::commands::preferred_local_owner_name(state).as_deref(),
        &state.match_state.match_presentation.house_color_map,
        Some(&rules.color_schemes),
    );
    post_resolved_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text: &text,
            rgb,
            timeout_ms,
            silent: true,
        },
    );
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
    post_house_names_line(state, key, &[house], color);
}

/// [`post_house_line`] naming player `name` instead of the House: VERA
/// shared control names which of a shared House's players left.
pub(crate) fn post_player_house_line(
    state: &mut AppState,
    key: &str,
    house: crate::sim::intern::InternedId,
    name: &str,
    color: HouseLineColor,
) {
    post_named_house_line(state, key, house, &[name.to_owned()], color);
}

/// [`post_house_line`] with one `%s` per House, coloured by the first.
fn post_house_names_line(
    state: &mut AppState,
    key: &str,
    houses: &[crate::sim::intern::InternedId],
    color: HouseLineColor,
) {
    let Some(names) = houses
        .iter()
        .map(|&house| crate::app::presentation::house_ui::house_ui_name(state, house))
        .collect::<Option<Vec<String>>>()
    else {
        return;
    };
    let Some(&house) = houses.first() else {
        return;
    };
    post_named_house_line(state, key, house, &names, color);
}

/// The line itself: the template with `names`, coloured by `house`.
fn post_named_house_line(
    state: &mut AppState,
    key: &str,
    house: crate::sim::intern::InternedId,
    names: &[String],
    color: HouseLineColor,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .runtime()
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
    let args: Vec<crate::assets::csf_file::CsfArg<'_>> = names
        .iter()
        .map(|name| crate::assets::csf_file::CsfArg::Str(name))
        .collect();
    let text = crate::assets::csf_file::format_csf(&template, &args);
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
        .runtime()
        .map(|runtime| runtime.simulation.interner().resolve(owner).to_string());
    house_message_rgb(
        owner.as_deref(),
        &state.match_state.match_presentation.house_color_map,
        state.rules().map(|rules| rules.color_schemes.as_slice()),
    )
}

/// Network packet colour is a Session priority, converted by69A310 before
/// TextLabel72A4A0. Keep this in the same message-colour owner as local echoes.
pub(crate) fn packet_player_line_rgb(state: &AppState, priority: i32) -> [f32; 3] {
    let Some(rules) = state.rules() else {
        return MESSAGE_RGB_SYSTEM;
    };
    country_message_rgb(
        rules.skirmish_colors().scheme_index_for_priority(priority) as i32,
        &rules.color_schemes,
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

/// Radar subtitle callback `0x005BF360`: literal text, scheme 0 and native
/// MessageList duration buckets; cue Position/RGB belong to full-screen movies.
pub(crate) fn post_movie_subtitle(state: &mut AppState, text: &str, duration: u32) {
    let rgb = state.rules().map_or(MESSAGE_RGB_SYSTEM, |rules| {
        country_message_rgb(0, &rules.color_schemes)
    });
    post_resolved_message(state, &movie_subtitle_post(text, duration, rgb));
}

fn movie_subtitle_post(
    text: &str,
    duration: u32,
    rgb: [f32; 3],
) -> crate::ui::messages::MessagePost<'_> {
    crate::ui::messages::MessagePost {
        prefix: None,
        text,
        rgb,
        // 0x005BF360 passes the cue duration as Add_Message's signed timeout.
        timeout_ms: message_timeout_ms_from_buckets(duration as i32),
        silent: false,
    }
}

/// The observer gate's silent `TXT_OBSERVER_CANT_SEND` row in the "Grey"
/// scheme (`0x004E12D0` with `0x00822728`), flags 0x16, MessageDelay*900
/// (`0x0055EED0..0x0055EF2C`).
pub(crate) fn post_observer_refusal(state: &mut AppState) {
    post_grey_row(state, "TXT_OBSERVER_CANT_SEND", true, true);
}

/// The WOL page sinks in a match (`0x007A92F9..0x007A93B4`): the page line
/// and, from the ANSI sink, `TXT_HOW_TO_REPLY` under it, each a Grey row
/// with flags 0x16, `MessageDelay * 900` and silent 0.
pub(crate) fn post_wol_page_rows(state: &mut AppState, line: &str, reply_hint: bool) {
    post_grey_row(state, line, false, false);
    if reply_hint {
        post_grey_row(state, "TXT_HOW_TO_REPLY", true, false);
    }
}

/// A WOL find or page-result row in a match with no window to take it
/// (`0x007AA5D5..`, `0x007A9800`): the same Grey row, silent 0.
pub(crate) fn post_wol_result_row(state: &mut AppState, text: &str) {
    post_grey_row(state, text, false, false);
}

/// The WOL NetStatus sink's lost connection in a match (`0x007AA7C0..`):
/// one `TXT_WOL_CONNLOST` row in the same scheme, flags and timeout, with
/// its sound.
pub(crate) fn post_wol_connection_lost(state: &mut AppState) {
    post_grey_row(state, "TXT_WOL_CONNLOST", true, false);
}

/// Cooperative `+0xA0`'s save result (`0x005C37B7..0x005C381F`):
/// `TXT_COOP_SAVE_SUCCESS` or `TXT_COOP_SAVE_FAILED`, a Grey row with its
/// sound.
pub(crate) fn post_cooperative_save_result(state: &mut AppState, saved: bool) {
    let key = if saved {
        "TXT_COOP_SAVE_SUCCESS"
    } else {
        "TXT_COOP_SAVE_FAILED"
    };
    post_grey_row(state, key, true, false);
}

/// A row in `GadgetClass::GetColorScheme` (`0x004E12D0`, "Grey"), flags
/// 0x16, `MessageDelay * 900`; `text` is a CSF key when `localize`.
fn post_grey_row(state: &mut AppState, text: &str, localize: bool, silent: bool) {
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
    post_message(
        state,
        &crate::ui::messages::MessagePost {
            prefix: None,
            text,
            rgb,
            timeout_ms,
            silent,
        },
        localize,
    );
}

/// A relay speed change (`network_match::post_speed_changes`). gamemd has
/// no such line, so `text` is VERA's own, not a CSF key.
pub(crate) fn post_match_speed(state: &mut AppState, text: &str) {
    post_grey_row(state, text, false, false);
}

/// VERA: the answer to a `/pause` or `/resume` chat line
/// (`pause_menu::chat_command`); `text` is VERA's own, not a CSF key.
pub(crate) fn post_chat_command_answer(state: &mut AppState, text: &str) {
    post_grey_row(state, text, false, false);
}

/// VERA: a match whose `veraui.ini` had lines the game could not use says so
/// once, naming the first; the log lists them all.
pub(crate) fn post_layout_problem(state: &mut AppState) {
    let Some(place) = state
        .match_state
        .match_presentation
        .match_layouts
        .problems()
        .first()
        .map(crate::ui::match_layout::LayoutProblem::place)
    else {
        return;
    };
    let text = crate::i18n::tr_fill("VERA:LayoutFileProblem", &[("where", &place)]);
    post_grey_row(state, &text, false, false);
}

/// Tell the player the layout edit mode is on and how to use it, or that it
/// is off and what they moved is kept.
pub(crate) fn post_layout_edit(state: &mut AppState, on: bool) {
    let key = if on {
        "VERA:LayoutEditHint"
    } else {
        "VERA:LayoutEditDone"
    };
    post_grey_row(state, &crate::i18n::tr(key), false, false);
}

/// VERA formation lock (`context_order::toggle_formation_lock`): say that
/// the selection's units were locked, and how many, or unlocked. gamemd has
/// no such line, so the text is VERA's own.
pub(crate) fn post_formation_lock(state: &mut AppState, locked: bool, units: usize) {
    let text = if locked {
        crate::i18n::tr_fill("VERA:FormationLocked", &[("count", &units)])
    } else {
        crate::i18n::tr("VERA:FormationUnlocked")
    };
    post_grey_row(state, &text, false, true);
}

/// VERA primary rotation (`input::primary_rotation`): say how many factories
/// now take turns, or that the selected ones stopped. gamemd has no such
/// line, so the text is VERA's own.
pub(crate) fn post_primary_rotation(state: &mut AppState, factories: usize) {
    let text = if factories > 0 {
        crate::i18n::tr_fill("VERA:PrimaryRotationOn", &[("count", &factories)])
    } else {
        crate::i18n::tr("VERA:PrimaryRotationOff")
    };
    post_grey_row(state, &text, false, true);
}

pub(super) fn post_command_backpressure(state: &mut AppState) {
    post_grey_row(
        state,
        &crate::i18n::tr("VERA:CommandsStillSending"),
        false,
        true,
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

/// Main_Tick55DDA5 polls Manage only when its offline menu/Scenario holds
/// permit it. On the first resumed pass, deadlines include the entire hold.
pub(crate) fn update(state: &mut AppState, main_tick_admitted: bool) -> Option<u64> {
    if state.frontend.screen != GameScreen::InGame
        || !poll_is_admitted(main_tick_admitted, state.match_state.main_tick_frozen())
    {
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

/// A hold created by an already-admitted Logic cannot revoke this pass's
/// Manage call. An expired hold is released before runtime admission.
fn poll_is_admitted(main_tick_admitted: bool, currently_frozen: bool) -> bool {
    main_tick_admitted || !currently_frozen
}

fn sync_view(state: &mut AppState) {
    let typing = state
        .frontend
        .multiplayer_shell_runtime
        .scroll_text(state.match_state.session_mode());
    state
        .match_state
        .match_presentation
        .message_list
        .set_typing(typing);
    // The list spans its box: the tactical viewport, the frame beside the
    // sidebar, unless the side's layout gives it another.
    // VERA's performance panel holds the viewport's top left while it shows, and the
    // list and its edit line start below it.
    let [x, y, width, _] = crate::app::presentation::field_parts::rect(
        state,
        crate::ui::field_layout::FieldPart::Messages,
    );
    let (_, tactical_y, _, _) = crate::app::input::camera::tactical_viewport_px(state);
    let top = state.renderer.performance_overlay.message_top();
    state.match_state.match_presentation.message_list.set_view(
        x,
        y.max(tactical_y as i32 + top),
        width,
    );
}

/// Prepare one successful offline TextLabel paint, using raw wall time for
/// typing and the existing sound owner for every MessageCharTyped request.
/// Expiry uses the same wall clock, but is polled only by admitted Main_Tick passes.
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
    // A layout that leaves the messages out only takes them off the screen.
    if !crate::app::presentation::field_parts::shows(
        state,
        crate::ui::field_layout::FieldPart::Messages,
    ) {
        return (Vec::new(), Vec::new());
    }
    message_instances(
        &state.renderer.bit_font,
        &state.match_state.match_presentation.message_list,
        state
            .platform
            .ime
            .composition(crate::app::input::ime::ImeTarget::InGameChat)
            .map(|(units, _)| units),
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    )
}

/// The open edit line in window pixels, where the input method shows its
/// candidates; `None` while no edit is open.
pub(crate) fn ime_edit_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    if state.frontend.screen != GameScreen::InGame {
        return None;
    }
    let list = &state.match_state.match_presentation.message_list;
    let edit = list.edit_line()?;
    let font = &state.renderer.bit_font;
    // The message rows are frame pixels, which a zoomed match shows scaled
    // and centred in the window.
    let (x, y) = (list.x() as f32, edit.y as f32);
    let ((x, y), scale) = state.tactical_zoom().map_or(((x, y), 1.0), |zoom| {
        (zoom.source_to_window(x, y), zoom.scale())
    });
    Some(crate::ui::shell::geom::RectPx::new(
        x as i32,
        y as i32,
        (font.text_width(&edit.text) as f32 * scale) as i32,
        (font.cell_height() * scale) as i32,
    ))
}

fn message_instances(
    font: &crate::render::bit_font::BitFont,
    list: &crate::ui::messages::MessageList,
    composition: Option<&[u16]>,
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
    // The open composition follows the typed text (`app::input::ime`).
    let composition = edit.as_ref().zip(composition).map(|(row, units)| {
        let mut row = row.clone();
        row.text = String::from_utf16_lossy(units);
        row.rgb = crate::app::input::ime::COMPOSITION_RGB.map(|c| f32::from(c) / 255.0);
        row
    });
    let composition_x = edit
        .as_ref()
        .map_or(x, |row| x + font.text_width(&row.text) as f32);
    let rows = edit
        .iter()
        .map(|row| (x, row))
        .chain(composition.iter().map(|row| (composition_x, row)))
        .chain(list.messages().iter().map(|row| (x, row)));
    for (x, m) in rows {
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

    #[test]
    fn movie_subtitle_rows_keep_native_duration_and_literal_text() {
        let rgb = [0.2, 0.3, 0.4];
        let post = movie_subtitle_post("test subtitle", 180, rgb);
        assert_eq!(post.text, "test subtitle");
        assert_eq!(post.rgb, rgb);
        assert!(post.prefix.is_none());
        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 600);
        let result = list.add_message(&post, 1_000, &|s| s.len() as i32);
        assert!(result.play_sound, "native callback uses silent=0");
        assert_eq!(list.messages()[0].deadline_ms, Some(3_880));
        list.manage(3_879);
        assert_eq!(list.messages().len(), 1);
        list.manage(3_880);
        assert_eq!(
            list.messages().len(),
            1,
            "MessageList expires strictly after its deadline"
        );
        list.manage(3_881);
        assert!(list.messages().is_empty());
        assert!(
            movie_subtitle_post("persistent", u32::MAX, rgb)
                .timeout_ms
                .is_none()
        );
        assert_eq!(movie_subtitle_post("zero", 0, rgb).timeout_ms, Some(0));
    }

    #[cfg(feature = "ares")]
    #[test]
    fn ares_refusal_post_uses_localized_message_owner_colour_and_expiry() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::superweapon_type::AresSuperWeaponMessage;
        let ini = IniFile::from_str(
            "[AudioVisual]\nMessageDelay=1\n[Colors]\nGold=85,255,255\nOwner=0,255,255\nPlayer=170,255,255\n\
             [SuperWeaponTypes]\n0=Strike\n1=Configured\n2=Firer\n\
             [Strike]\nType=GenericWarhead\nMessage.CannotFire=MSG:Denied\nMessage.InsufficientFunds=MSG:Funds\n\
             [Configured]\nType=GenericWarhead\nMessage.CannotFire=MSG:Denied\nMessage.Color=Owner\n\
             [Firer]\nType=GenericWarhead\nMessage.CannotFire=MSG:Denied\nMessage.FirerColor=yes\nMessage.Color=Player\n",
        );
        let processed = RulesLayerStack::new(ini)
            .process_with_fixed_art(&IniFile::empty())
            .unwrap();
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        let mut colors = crate::map::houses::HouseColorMap::new();
        colors.insert(
            "FirerHouse".into(),
            crate::rules::house_colors::HouseColorIndex(1),
        );
        colors.insert(
            "PlayerHouse".into(),
            crate::rules::house_colors::HouseColorIndex(2),
        );
        let build = |id: &str, firer| {
            ares_message_post(
                rules.super_weapon(id).unwrap().ares_messages(),
                AresSuperWeaponMessage::CannotFire,
                &rules.color_schemes,
                &colors,
                firer,
                Some("PlayerHouse"),
                rules.general.message_delay_minutes,
            )
            .unwrap()
        };
        let post = build("Strike", Some("FirerHouse"));
        assert_eq!(post.rgb, [0.0, 0.0, 1.0]);
        assert_eq!(post.timeout_ms, Some(14_400));
        assert!(post.prefix.is_none() && !post.silent);
        assert_eq!(build("Configured", Some("FirerHouse")).rgb, [1.0, 0.0, 0.0]);
        assert_eq!(build("Firer", Some("FirerHouse")).rgb, [1.0, 0.0, 0.0]);
        assert_eq!(
            build("Firer", None).rgb,
            [0.0, 1.0, 0.0],
            "null firer uses Gold"
        );
        let csf = test_csf("MSG:Denied", "No eligible target");
        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 600);
        assert!(
            add_localized_message(&mut list, Some(&csf), &post, 100, &|s| s.len() as i32)
                .play_sound
        );
        assert_eq!(list.messages()[0].text, "No eligible target");
        assert_eq!(list.messages()[0].deadline_ms, Some(14_500));
        assert!(
            add_localized_message(&mut list, Some(&csf), &post, 101, &|s| s.len() as i32)
                .play_sound
        );
        assert_eq!(
            list.messages().len(),
            2,
            "retain native repeated rows; no new debounce"
        );
        let empty = test_csf("MSG:Denied", "");
        assert!(
            !add_localized_message(&mut list, Some(&empty), &post, 102, &|s| s.len() as i32)
                .play_sound
        );
        assert_eq!(list.messages().len(), 2);
        list.manage(14_502);
        assert!(list.messages().is_empty());
    }

    #[test]
    fn timed_release_admission_survives_a_new_pause_in_the_same_logic_pass() {
        use crate::app::match_runtime::session::SessionMode;
        use crate::app::match_runtime::sim_tick::{RuntimePassInputs, decide_runtime_pass};
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        let decide = |paused| {
            decide_runtime_pass(RuntimePassInputs {
                exact_step: false,
                window_active: true,
                focus_frozen: false,
                startup_admitted: true,
                frame_stepping: false,
                paused: false,
                scenario_paused: paused,
                menu_open: false,
                session_mode: SessionMode::Campaign,
                pacer_timing_admits: true,
                network_wait: false,
                network_dialog_open: false,
            })
        };
        let blocked = |sim: &crate::sim::world::Simulation| {
            crate::app::match_runtime::scenario_pause::blocks_main_tick(
                SessionMode::Campaign,
                sim.session().scenario_paused(),
            )
        };
        sim.pause_scenario_for_test(1);
        assert!(!poll_is_admitted(decide(true).run_sim, blocked(&sim)));
        // Main_Tick releases the timed credit before its admission test.
        sim.expire_scenario_pause_for_test();
        let admitted = decide(sim.session().scenario_paused()).run_sim;
        assert!(admitted);
        // Action110 can acquire another credit during this admitted Logic.
        sim.pause_scenario_for_test(1);
        assert!(blocked(&sim));
        assert!(poll_is_admitted(admitted, blocked(&sim)));
        let mut list = crate::ui::messages::MessageList::new(0, 0, 6, 600);
        list.add_message(
            &crate::ui::messages::MessagePost {
                prefix: None,
                text: "objective",
                rgb: [1.0; 3],
                timeout_ms: Some(1_000),
                silent: true,
            },
            0,
            &|text| text.len() as i32,
        );
        if poll_is_admitted(admitted, blocked(&sim)) {
            assert!(list.manage(1_001));
        }
        assert!(list.messages().is_empty());
    }

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
                trigger_message_rgb(
                    &rules.color_schemes,
                    row["side"].as_i64().unwrap() as i32,
                    crate::rules::skirmish_colors::SkirmishColorMode::VANILLA,
                ),
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
    fn house_low_power_native_post_reaches_retail_text_and_wall_clock_expiry() {
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
        let post = HudMessage::LowPower.post(&retail.rules, rgb).unwrap();
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
        let now = 1_200;
        let outcome = add_localized_message(&mut list, Some(&csf), &post, now, &|text| {
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
        assert_eq!(message_instances(&font, &list, None, [0.0; 2]).1.len(), 1);
        assert_eq!(list.advance_reveal(now as u32 + 16), 1);
        assert_eq!(message_instances(&font, &list, None, [0.0; 2]).1.len(), 2);
        // Offline menus skip Manage. Equality still retains the row; a
        // first resumed poll past the deadline removes it immediately.
        assert!(!list.manage(deadline));
        let resumed_wall_ms = deadline + 20_000;
        assert!(list.manage(resumed_wall_ms));
        assert!(list.messages().is_empty());

        // Admission failure cannot play the message-insertion sound.
        list.set_view(0, 0, 0);
        let rejected =
            add_localized_message(&mut list, Some(&csf), &post, resumed_wall_ms + 2, &|_| 8);
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
    fn open_composition_paints_after_the_typed_text_in_its_colour() {
        use crate::render::bit_font::tests::make_test_font;

        let font = make_test_font(&[(b'a' as u16, 6), (0x4E2D, 15)], 4);
        let mut list = crate::ui::messages::MessageList::new(3, 0, 6, 1_000);
        assert!(list.add_edit("a", [1.0, 1.0, 1.0], true));
        let (fills, texts) = message_instances(&font, &list, Some(&[0x4E2D]), [0.0; 2]);
        let expected = font.build_text_label(
            "\u{4E2D}",
            3.0 + font.text_width("a") as f32,
            list.edit_line().unwrap().y as f32,
            0.00022,
            [0.0; 2],
            crate::app::input::ime::COMPOSITION_RGB,
            0,
        );
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[1].position, expected[0].position);
        assert_eq!(texts[1].tint, expected[0].tint);
        assert_eq!(fills.len(), 2, "the composition has its own backing box");
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

        let at_origin = message_instances(&font, &list, None, [0.0, 0.0]).1;
        let ui_camera = [640.0_f32, 480.0_f32];
        let panned = message_instances(&font, &list, None, ui_camera).1;

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

    /// An EVA row names the type by its `UIName=` text, as tooltips do; a
    /// type with no `UIName=` keeps its rules key, and an unknown type posts
    /// nothing.
    #[test]
    fn eva_text_names_a_type_by_its_ui_name() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=GAPOWR\n1=GAREFN\n[GAPOWR]\nUIName=Name:GAPOWR\n[GAREFN]\n[AudioVisual]\nBaseUnderAttackSound=Siren\n",
            ),
        )
        .unwrap();
        let csf = test_csf("Name:GAPOWR", "發電廠");
        assert_eq!(
            eva_text_name("GAPOWR", &rules, Some(&csf)).as_deref(),
            Some("發電廠")
        );
        assert_eq!(
            eva_text_name("GAREFN", &rules, Some(&csf)).as_deref(),
            Some("GAREFN")
        );
        assert_eq!(eva_text_name("UNKNOWN", &rules, Some(&csf)), None);
    }
}

#[cfg(all(test, feature = "ares"))]
mod ares_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn fourth_side_message_text_color_resolves_to_its_colors_entry() {
        let mut colors = String::from("[Colors]\n");
        for index in 0..11 {
            colors.push_str(&format!("C{index}={},{},{}\n", index, index, index));
        }
        colors.push_str("DarkRed=0,230,255\n");
        let ini = IniFile::from_str(&format!(
            "{colors}\
             [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
             [Fourth]\nMessageTextColor=DarkRed\n"
        ));
        let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
            .process()
            .expect("message color rules process");
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed)
            .expect("message color projection");
        let entry =
            crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, "DarkRed")
                .expect("DarkRed entry");
        assert_eq!(
            ares_message_text_rgb(&rules, 3),
            Some(
                crate::rules::color_scheme::hsv_to_rgb(rules.color_schemes[entry].hsv)
                    .map(|channel| f32::from(channel) / 255.0)
            )
        );
    }
}
