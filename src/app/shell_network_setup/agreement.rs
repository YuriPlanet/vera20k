//! One build/features and resolved selected-mode rules agreement for both
//! multiplayer adapters. ProcessAssets retains the actual source/digest owner.

use crate::app::AppState;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MultiplayerAgreement {
    pub version: String,
    pub rules_digest: String,
}

/// The build two games must share to play together: the package version and
/// build.rs's hash of the sources and enabled Cargo features
/// (docs/Networking.md, "Build check").
pub(crate) fn version() -> String {
    concat!(
        env!("CARGO_PKG_VERSION"),
        ":",
        env!("VERA20K_NETWORK_BUILD")
    )
    .to_owned()
}

pub(crate) fn local(state: &mut AppState) -> Result<MultiplayerAgreement> {
    let session = &state.match_state.session;
    let rules = session
        .network_personal_rules()
        .ok_or_else(|| anyhow!("no setup"))?
        .map_err(|error| anyhow!("the host's rules do not read: {error}"))?;
    state.process_assets.use_personal_rules(rules);
    // VERA: a match that seats no computer reads the original AI INI, as its
    // load does (`LoadingStartup::ai_file`).
    let seats_computers = session.network_seats_computers();
    if session.campaign_room().is_some() {
        let rules_digest = state
            .process_assets
            .multiplayer_match_rules_digest("", seats_computers)
            .map_err(anyhow::Error::msg)?;
        return Ok(MultiplayerAgreement {
            version: version(),
            rules_digest,
        });
    }
    let setup = session.network_setup().ok_or_else(|| anyhow!("no setup"))?;
    let file = setup
        .selected_map_file
        .as_deref()
        .ok_or_else(|| anyhow!("no selected map"))?;
    let mode = setup.mode.override_file.clone();
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
            .multiplayer_generated_rules_digest(&mode, options, seats_computers)
    } else {
        state
            .process_assets
            .multiplayer_match_rules_digest(&mode, seats_computers)
    }
    .map_err(anyhow::Error::msg)?;
    Ok(MultiplayerAgreement {
        version: version(),
        rules_digest: digest,
    })
}

/// VERA: log the match's rules in parts (`app::rules_parts`), so a refused
/// start can be read against another player's log.
pub(crate) fn log_rules_parts(state: &AppState) {
    let Some(mode) = state
        .match_state
        .session
        .network_setup()
        .map(|setup| setup.mode.override_file.as_str())
    else {
        return;
    };
    match state.process_assets.multiplayer_rules_parts(&[mode]) {
        Ok(parts) => {
            for parts in parts {
                log::info!("Rules parts: {}", parts.json());
            }
        }
        Err(error) => log::warn!("No rules parts: {error}"),
    }
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

/// One way this game changes the original game, which other players'
/// games must change the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginalGameChange<'a> {
    /// The mod in use, by its title.
    Mod(&'a str),
    /// RA2 Mode is on (`util::ra2_launch`, docs/ra2-mode.md).
    Ra2,
    /// `config.toml`'s INI roots that games compare (`IniRoots::agreed_files`)
    /// or its theater suffix are not YR's.
    IniRoots,
    /// The Ares setting where it decides which game this is
    /// (`ProcessAssets::agreed_ares_setting`), never Auto.
    Ares(crate::util::ares_launch::AresSetting),
    /// The game-folder files that change the original game.
    ChangedFiles(&'a [String]),
}

/// What this game changes from the original game: its mod, RA2 Mode or else
/// `config.toml`'s INI roots (RA2 Mode picks its own), the Ares setting where it decides which game this is and
/// changed game files, in that order. The rules-differ text names them all,
/// Quick Match refuses on the first. Ares comes before the files: On can
/// mount the installation's `ares.mix`, which counts as a changed file, and a
/// box naming the files would send the player to delete it. Empty when it
/// plays the original.
/// The Options switches never count: the room decides which its players play
/// (`MatchSession::network_personal_rules`). Nor does the Ares setting Off
/// without a mod whose `ares.mix` it leaves out, or On with one: each opens
/// the game Auto opens.
pub(crate) fn original_game_changes<'a>(
    mod_title: Option<&'a str>,
    assets: &'a crate::app::process_assets::ProcessAssets,
) -> Vec<OriginalGameChange<'a>> {
    let ares = assets.agreed_ares_setting();
    let files = assets.changed_game_files();
    let ra2 = crate::util::ra2_launch::enabled();
    [
        mod_title.map(OriginalGameChange::Mod),
        ra2.then_some(OriginalGameChange::Ra2),
        (!ra2 && !assets.stock_ini_roots()).then_some(OriginalGameChange::IniRoots),
        (ares != crate::util::ares_launch::AresSetting::Auto)
            .then_some(OriginalGameChange::Ares(ares)),
        (!files.is_empty()).then_some(OriginalGameChange::ChangedFiles(files)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// `changes` as the `Vera-Changes` sign-in header names them, so that a room
/// whose rules differ hears which of them only one game has
/// (docs/lobby-protocol.md "Sign-in headers"): percent-encoded JSON with
/// `mod`, `ini_roots: true`, `ares: "on"|"off"` and `files` with `count`, each
/// left out when unchanged, `{}` for the original game. Texts keep to the
/// lobby's 64 bytes of whole characters without control characters, and the
/// files to the lobby's first 32 names.
pub(crate) fn changes_header(changes: &[OriginalGameChange]) -> String {
    use crate::util::ares_launch::AresSetting;
    use serde_json::{Map, Value};
    const FILES: usize = 32;
    let text = |text: &str| {
        let mut kept = String::new();
        for c in text.chars().filter(|c| !c.is_control()) {
            if kept.len() + c.len_utf8() > 64 {
                break;
            }
            kept.push(c);
        }
        kept
    };
    let mut header = Map::new();
    for change in changes {
        match *change {
            OriginalGameChange::Mod(name) => {
                header.insert("mod".into(), text(name).into());
            }
            // The lobby names no RA2 Mode, and RA2 Mode does read other
            // INI roots, so lobbies and older games keep naming it so.
            OriginalGameChange::Ra2 | OriginalGameChange::IniRoots => {
                header.insert("ini_roots".into(), true.into());
            }
            OriginalGameChange::Ares(AresSetting::On) => {
                header.insert("ares".into(), "on".into());
            }
            OriginalGameChange::Ares(AresSetting::Off) => {
                header.insert("ares".into(), "off".into());
            }
            OriginalGameChange::Ares(AresSetting::Auto) => {}
            OriginalGameChange::ChangedFiles(files) => {
                let named: Vec<Value> = files
                    .iter()
                    .take(FILES)
                    .map(|file| text(file).into())
                    .collect();
                header.insert("files".into(), named.into());
                header.insert("count".into(), files.len().min(65535).into());
            }
        }
    }
    Value::Object(header)
        .to_string()
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The text naming each of this game's [`original_game_changes`]; `None`
/// when it plays the original.
pub(crate) fn own_changes_text(state: &AppState) -> Option<String> {
    changes_text(original_game_changes(
        state.frontend.mods.title(),
        &state.process_assets,
    ))
}

/// VERA: the rules a match's start certifies (`StartupAgreement::new`): the
/// room's digest, then what only the loaded match says. `terrain` names the
/// game folder's terrain files a match read whose map needs them
/// (`ProcessAssets::match_terrain_digest`). `rules` are the loaded match's,
/// for whether Ares plays in it (`RuleSet::ares`): with the same files that
/// differs only between a player whose Ares setting is Off and one it is
/// not, and only when the match's mode or map holds an Ares key, which the
/// room's digest cannot know (`ProcessAssets::agreed_ares_setting`). Both add
/// nothing to a match of the original game on its own terrain.
pub(crate) fn start_rules(
    mut rules_digest: String,
    terrain: Option<&str>,
    rules: &crate::rules::ruleset::RuleSet,
) -> String {
    if let Some(terrain) = terrain {
        rules_digest.push_str(" terrain ");
        rules_digest.push_str(terrain);
    }
    if rules.ares() {
        rules_digest.push_str(" ares");
    }
    rules_digest
}

/// VERA: whether the Ares setting Off is what keeps Ares from playing in the
/// loaded match (`RuleSet::ares_kept_off`): its rules or map hold an Ares key.
/// No room compares that and the rules parts leave it out; the match's start
/// does ([`start_rules`]).
pub(crate) fn match_keeps_ares_off(state: &AppState) -> bool {
    state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.resources.rules.ares_kept_off())
}

/// VERA: the game-folder files that change what the loaded match's
/// simulation reads from pictures and models
/// (`RuleSet::file_input_folder_files`), which its start certifies and no
/// room compares. A selected mod's own files are left out: the rules digest
/// compares them with the mod.
pub(crate) fn match_picture_files(state: &AppState) -> Vec<String> {
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return Vec::new();
    };
    let mut files = runtime.resources.rules.file_input_folder_files();
    if let Some(assets) = state.process_assets.manager() {
        files.retain(|file| !assets.from_mod_folder(file));
    }
    files
}

/// The files a refused start names: first what only this match read of the
/// game folder (`read`), which no room compared and the box's three names
/// should not push out, then the changed game files, each file once. `None`
/// when the match read none besides the changed files, named already.
fn start_files(changed: &[String], read: impl IntoIterator<Item = String>) -> Option<Vec<String>> {
    let mut files = Vec::new();
    for file in read {
        if !files.contains(&file) {
            files.push(file);
        }
    }
    if files.iter().all(|file| changed.contains(file)) {
        return None;
    }
    for file in changed {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    Some(files)
}

/// VERA: [`own_changes_text`] for a start the players' games refused. A match
/// whose map names a tile the original game lacks also read this game's
/// terrain files (`ProcessAssets::match_terrain_files`), and a game folder
/// can change what a match's simulation reads from pictures and models
/// ([`match_picture_files`]). No room compares either, so they are named
/// before the changed files. And where the Ares setting Off is what keeps
/// Ares from playing in the match (`RuleSet::ares_kept_off`), which no room
/// compares either, it is named as the setting.
pub(crate) fn start_changes_text(state: &AppState) -> Option<String> {
    let assets = &state.process_assets;
    let mut changes = original_game_changes(state.frontend.mods.title(), assets);
    if match_keeps_ares_off(state)
        && !changes
            .iter()
            .any(|change| matches!(change, OriginalGameChange::Ares(_)))
    {
        let before_files = changes
            .iter()
            .position(|change| matches!(change, OriginalGameChange::ChangedFiles(_)))
            .unwrap_or(changes.len());
        changes.insert(
            before_files,
            OriginalGameChange::Ares(crate::util::ares_launch::AresSetting::Off),
        );
    }
    let read = assets
        .match_terrain_files()
        .iter()
        .cloned()
        .chain(match_picture_files(state));
    let files = start_files(assets.changed_game_files(), read);
    if let Some(files) = &files {
        changes.retain(|change| !matches!(change, OriginalGameChange::ChangedFiles(_)));
        changes.push(OriginalGameChange::ChangedFiles(files));
    }
    changes_text(changes)
}

fn changes_text(changes: Vec<OriginalGameChange<'_>>) -> Option<String> {
    use crate::i18n::{tr, tr_fill};
    let named: Vec<String> = changes
        .into_iter()
        .map(|change| match change {
            OriginalGameChange::Mod(name) => tr_fill("VERA:RulesChangeMod", &[("name", &name)]),
            OriginalGameChange::Ra2 => tr("VERA:RulesChangeRa2"),
            OriginalGameChange::IniRoots => tr("VERA:RulesChangeIniRoots"),
            OriginalGameChange::Ares(setting) => tr_fill(
                "VERA:RulesChangeAres",
                &[(
                    "setting",
                    &crate::ui::main_menu_dialogs::options::ares_label(setting),
                )],
            ),
            OriginalGameChange::ChangedFiles(files) => changed_files_text(files),
        })
        .collect();
    (!named.is_empty())
        .then(|| tr_fill("VERA:RulesDifferChanges", &[("changes", &named.join(", "))]))
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::util::ares_launch::AresSetting;
    use serde_json::{Value, json};

    /// A start certifies the room's digest alone for a match of the original
    /// game on its own terrain. On a map with an Ares key, the player whose
    /// Ares setting is Off and the one set to Auto loaded other rules from
    /// the same files, and their starts differ; on a map without one they
    /// loaded the same and their starts agree. The terrain files a match read
    /// are named too.
    #[test]
    fn a_start_names_what_only_the_loaded_match_says() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::process_owner::NativeScenarioRulesPrefix;
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let loaded = |ares_off: bool, map: &str| {
            crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
                .with_ares_mix(false)
                .with_ares_keys_off(ares_off)
                .load_scenario(
                    NativeScenarioRulesPrefix::NonCampaign(None),
                    &IniFile::from_str(map),
                )
                .unwrap()
                .into_parts()
                .0
        };
        let start = |ares_off, map| start_rules("digest".to_owned(), None, &loaded(ares_off, map));
        assert_eq!(start(true, ""), "digest");
        assert_eq!(start(false, ""), "digest");
        let ares_key = "[HTNK]\nImmuneToAbduction=yes\n";
        assert_eq!(start(true, ares_key), "digest");
        assert_ne!(start(false, ares_key), "digest");
        let terrain = start_rules("digest".to_owned(), Some("files"), &loaded(true, ""));
        assert_ne!(terrain, "digest");
    }

    /// A refused start names the terrain and picture files its match read
    /// before the changed game files, each file once, and adds nothing when
    /// the match read no others.
    #[test]
    fn a_refused_start_names_what_its_match_read_first_and_once() {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|&name| name.to_owned())
                .collect::<Vec<_>>()
        };
        let changed = names(&["rulesmd.ini", "expandmd07.mix"]);
        assert_eq!(start_files(&changed, names(&[])), None);
        assert_eq!(start_files(&changed, names(&["expandmd07.mix"])), None);
        assert_eq!(
            start_files(
                &changed,
                names(&["clear01.sno", "expandmd07.mix", "gtpowrmk.shp"])
            ),
            Some(names(&[
                "clear01.sno",
                "expandmd07.mix",
                "gtpowrmk.shp",
                "rulesmd.ini"
            ]))
        );
        assert_eq!(
            start_files(&[], names(&["gtpowrmk.shp", "gtpowrmk.shp"])),
            Some(names(&["gtpowrmk.shp"]))
        );
    }

    /// The `Vera-Changes` header names each change in the JSON and bounds the
    /// lobby takes: Auto is no change, texts keep to 64 bytes of whole
    /// characters without control characters, and the files to the first 32
    /// of their count, all in the characters a header may carry.
    #[test]
    fn the_changes_header_names_what_the_lobby_compares() {
        let decoded = |header: String| -> Value {
            assert!(
                header
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"%-_.~".contains(&byte)),
                "{header}"
            );
            let mut bytes = Vec::new();
            let mut rest = header.as_str();
            while let Some((before, after)) = rest.split_once('%') {
                bytes.extend_from_slice(before.as_bytes());
                bytes.push(u8::from_str_radix(&after[..2], 16).unwrap());
                rest = &after[2..];
            }
            bytes.extend_from_slice(rest.as_bytes());
            serde_json::from_slice(&bytes).unwrap()
        };
        assert_eq!(decoded(changes_header(&[])), json!({}));
        assert_eq!(
            decoded(changes_header(&[OriginalGameChange::Ares(
                AresSetting::Auto
            )])),
            json!({})
        );
        let title = format!("Long\u{7}ModName {}", "é".repeat(40));
        let files: Vec<String> = (0..40).map(|n| format!("expandmd{n:02}.mix")).collect();
        let header = changes_header(&[
            OriginalGameChange::Mod(&title),
            OriginalGameChange::IniRoots,
            OriginalGameChange::Ares(AresSetting::Off),
            OriginalGameChange::ChangedFiles(&files),
        ]);
        assert_eq!(
            decoded(header),
            json!({
                "mod": format!("LongModName {}", "é".repeat(26)),
                "ini_roots": true,
                "ares": "off",
                "files": files[..32],
                "count": 40,
            })
        );
    }
}
