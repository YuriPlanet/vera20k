//! One build/features and resolved selected-mode rules agreement for both
//! multiplayer adapters. ProcessAssets retains the actual source/digest owner.

use crate::app::AppState;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MultiplayerAgreement {
    pub version: String,
    pub rules_digest: String,
}

pub(crate) fn version() -> String {
    // The `0` held the retired `ares` build feature; every game now has Ares
    // (`RuleSet::ares`), and the slot keeps a released game's value.
    format!(
        "{}:{}:0{}{}{}",
        env!("CARGO_PKG_VERSION"),
        env!("VERA20K_NETWORK_BUILD"),
        u8::from(crate::rules::PHOBOS_ENABLED),
        u8::from(crate::rules::NPATCH_ENABLED),
        u8::from(crate::rules::NPBUG_ENABLED)
    )
}

pub(crate) fn local(state: &mut AppState) -> Result<MultiplayerAgreement> {
    let setup = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?;
    let file = setup
        .selected_map_file
        .as_deref()
        .ok_or_else(|| anyhow!("no selected map"))?;
    let mode = setup.mode.override_file.clone();
    // VERA: every player plays the room's rules, the host's, and agrees on
    // them; a Quick Match room has none (`live_service` drops what its host
    // sends), a launcher's room each player's own.
    let room_rules = setup
        .options
        .host_rules
        .personal_rules()
        .map_err(|error| anyhow!("the host's rules do not read: {error}"))?;
    state.process_assets.use_personal_rules(room_rules);
    let seed_options = if crate::map::rmg::is_seed_selection(file) {
        let root = &state
            .platform
            .game_config
            .as_ref()
            .ok_or_else(|| anyhow!("no asset directory"))?
            .paths
            .ra2_dir;
        let source_file = state
            .match_state
            .session
            .network_local_map_source()
            .and_then(|source| match source {
                crate::map::source::LoadedMapSource::Loose { path, .. } => path.to_str(),
                _ => None,
            })
            .unwrap_or(file);
        let assets = state
            .process_assets
            .manager()
            .ok_or_else(|| anyhow!("assets missing"))?;
        let ares = state.process_assets.reads_ares_way();
        crate::map::source::load_network_map_ini(root, source_file, assets)
            .ok()
            .map(|loaded| {
                let mut options = crate::map::rmg::RmgOptions::default();
                options.apply_sed(&loaded.ini, ares);
                options.normalize();
                options
            })
    } else {
        None
    };
    let digest = if let Some(options) = seed_options.as_ref() {
        state
            .process_assets
            .multiplayer_generated_rules_digest(&mode, options)
    } else {
        state.process_assets.multiplayer_rules_digest(&mode)
    }
    .map_err(anyhow::Error::msg)?;
    Ok(MultiplayerAgreement {
        version: version(),
        rules_digest: digest,
    })
}

/// `{files}` in a box naming the changed game files: the body is a few
/// lines, so it names the first three files and counts the rest, which the
/// startup log lists, as `(+2)`, which needs no translation.
pub(crate) fn changed_files_text(files: &[String]) -> String {
    const NAMED: usize = 3;
    let named = files[..files.len().min(NAMED)].join(", ");
    match files.len().saturating_sub(NAMED) {
        0 => named,
        more => format!("{named} (+{more})"),
    }
}

/// VERA: what a refused join or start says when the players' rules differ.
/// Each game knows only its own files, so it names what it changes from the
/// original game ([`own_changes_text`]), or says it plays the original, so
/// the change is in another player's.
pub(crate) fn rules_differ_text(state: &AppState) -> String {
    let own =
        own_changes_text(state).unwrap_or_else(|| crate::i18n::tr("VERA:RulesDifferOriginal"));
    format!("{} {own}", crate::i18n::tr("VERA:RulesDiffer"))
}

/// What this game changes from the original game, which other players'
/// games must change the same way: its mod, `config.toml`'s INI roots, the
/// Ares setting when not Auto and changed game files. `None` when it plays the original. The Options
/// switches never count: a room plays its host's
/// (`HostRules`).
pub(crate) fn own_changes_text(state: &AppState) -> Option<String> {
    use crate::i18n::{tr, tr_fill};
    let mut changes = Vec::new();
    if let Some(active) = &state.frontend.mods.active {
        changes.push(tr_fill("VERA:RulesChangeMod", &[("name", &active.title)]));
    }
    if !state.process_assets.stock_ini_roots() {
        changes.push(tr("VERA:RulesChangeIniRoots"));
    }
    let ares = state.process_assets.ares_setting();
    if ares != crate::util::ares_launch::AresSetting::Auto {
        changes.push(tr_fill(
            "VERA:RulesChangeAres",
            &[(
                "setting",
                &crate::ui::main_menu_dialogs::options::ares_label(ares),
            )],
        ));
    }
    let files = state.process_assets.changed_game_files();
    if !files.is_empty() {
        changes.push(changed_files_text(files));
    }
    (!changes.is_empty()).then(|| {
        tr_fill(
            "VERA:RulesDifferChanges",
            &[("changes", &changes.join(", "))],
        )
    })
}
