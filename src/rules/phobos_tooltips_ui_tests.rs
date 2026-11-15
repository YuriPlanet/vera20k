//! The `[ToolTips]`/`[LoadingScreen]` reads: defaults, authored values, `<none>`
//! handling and CSF label resolution, each against `Phobos.INI.cpp:135-170`.
use super::ToolTipsUi;
use crate::rules::ini_parser::IniFile;

fn uimd(text: &str) -> ToolTipsUi {
    ToolTipsUi::from_ini(&IniFile::from_str(text), &|label| {
        Some(
            (label == "UI:PHOBOS:Cost")
                .then(|| "CR".to_string())
                .unwrap_or_else(|| format!("MISSING:'{label}'")),
        )
    })
}

#[test]
fn an_absent_uimd_keeps_every_phobos_default() {
    let ui = ToolTipsUi::default();
    assert!(!ui.extended_tooltips);
    assert!(!ui.anchored_tooltips);
    assert_eq!(ui.max_width, 0);
    assert_eq!(ui.cost_label, "$");
    assert_eq!(ui.time_label, "\u{231A}");
    assert_eq!(ui.sw_shots_format, "Shots: %d");
    assert!(!ui.disable_empty_spawn_positions);

    let bare = uimd("[General]\n");
    assert_eq!(
        bare, ui,
        "a uimd file without either section keeps defaults"
    );
}

#[test]
fn authored_switches_and_labels_land_where_the_native_block_reads_them() {
    let ui = uimd(
        "[LoadingScreen]\nDisableEmptySpawnPositions=yes\n\
         [ToolTips]\nExtendedToolTips=yes\nAnchoredToolTips=yes\nMaxWidth=260\n\
         CostLabel=UI:PHOBOS:Cost\nTimeLabel=UI:PHOBOS:Time\nSWShotsFormat=UI:PHOBOS:Shots\n",
    );
    assert!(ui.extended_tooltips);
    assert!(ui.anchored_tooltips);
    assert_eq!(ui.max_width, 260);
    // A CSF label the test table lacks keeps the native MISSING text.
    assert_eq!(ui.cost_label, "CR");
    assert_eq!(ui.time_label, "MISSING:'UI:PHOBOS:Time'");
    assert_eq!(ui.sw_shots_format, "MISSING:'UI:PHOBOS:Shots'");
    assert!(ui.disable_empty_spawn_positions);
}

#[test]
fn blank_and_none_values_keep_the_literal_default_but_not_a_label() {
    // `IsValidString` rejects only an empty/blank value, so `<none>` (the
    // ReadString default) and `NONE` both fall back. Phobos compares with
    // `INIClass::IsBlank`, which folds case like the native parser.
    for value in ["", "<none>", "NONE", " <none> "] {
        let ui = uimd(&format!("[ToolTips]\nCostLabel={value}\n"));
        assert_eq!(ui.cost_label, "$", "CostLabel={value:?}");
    }
    let authored = uimd("[ToolTips]\nCostLabel=Credit\n");
    assert_eq!(
        authored.cost_label, "MISSING:'Credit'",
        "a real value is a CSF key, not a literal"
    );
}

#[test]
fn max_width_accepts_the_native_integer_forms_and_keeps_zero_off() {
    assert_eq!(uimd("[ToolTips]\nMaxWidth=0\n").max_width, 0);
    assert_eq!(uimd("[ToolTips]\nMaxWidth=-7\n").max_width, -7);
    assert_eq!(
        uimd("[ToolTips]\nMaxWidth=bad\n").max_width,
        0,
        "an unparsable value falls back to the reader default"
    );
}
