# Phobos `ShowBriefing*` keys (task 52, Section D subset)

Three keys from `EXTERNAL-TASK-phobos-52.md` Section D. This page records
their native reads, the VERA owners that now hold them, and — importantly —
why the consumer pass is **escalated rather than faked**.

## Native evidence

| Key | Read site | Default | Consumer |
|---|---|---|---|
| `ShowBriefing` | `src/Phobos.INI.cpp:107` `[Phobos] ReadBool` (static `Phobos.h:108`, `:68`); also `Ext/Scenario/Body.cpp:167` `[Basic]` and `Ext/Scenario/Hooks.cpp:21` (scenario section, `Body.h:62` default `false`) | client `yes`, scenario `no` | `DEFINE_HOOK(0x683E41, ScenarioClass_Start_ShowBriefing, 0x6)`, `src/Misc/Hooks.UI.cpp:345-373` |
| `ShowBriefingResumeButtonLabel` | `src/Phobos.INI.cpp:246` `uimd.ini [UISettings] ReadString` | `GUI:Resume` | `DEFINE_HOOK(0x65F764, BriefingDialog_ShowBriefing, 0x5)`, `Hooks.UI.cpp:423-433` |
| `ShowBriefingResumeButtonStatusLabel` | `src/Phobos.INI.cpp:249` `uimd.ini [UISettings] ReadString` | `STT:BriefingButtonReturn` | `DEFINE_HOOK(0x604985, GetDialogUIStatusLabels_ShowBriefing, 0x5)`, `Hooks.UI.cpp:437-446` |

`0x683E41` refuses the dialog unless **all** hold: the client
`Phobos::Config::ShowBriefing`, `ScenarioExt::Global()->ShowBriefing`, the
caller's `showBriefing` stack argument and `SessionClass::IsCampaign()`.
It then sets `BriefingTemp::ShowBriefing`, queues `BriefingTheme`, and returns
to `SkipGameCode = 0x683E6B`. `0x48CE85` and `0x55D14F` call `ShowBriefing()`
to open `Game::SpecialDialog = 9`; `0x683F66` / `0x685D95` skip a redraw while
it is pending; the two label hooks rewrite dialog item `1059` (`0x423`) and
the status line only while that flag is set.

## VERA implementation

- `src/app/persistence/phobos_settings.rs` — `PhobosClientSettings::show_briefing`,
  read with the exact literal `"ShowBriefing"` and the native `true` default,
  alongside `SaveGameOnScenarioStart` (`Phobos.INI.cpp:106`).
- `src/rules/phobos_briefing_ui.rs` — `BriefingDialogUi::from_uimd_bytes` /
  `from_ini` / `from_section` read `uimd.ini [UISettings]` with
  `IniSection::read_string` (the native `INI_EX::ReadString`: default only on
  an absent key, an authored empty value stays empty). The button label uses
  the shared 2048-byte buffer (`Phobos.h:26`), the status label native's
  `char[0x20]`.

Both land in `rules`/`app` as part of every build; a game without the
Phobos reads is unchanged. All three canonical keys were already in
`G:/20k/scratch/phobos_read_keys.json`, so no registry edit was needed.

## Scenario-start pass (wired)

VERA's `InGameMenuState::MissionBriefing` **is** the native 0x10C dialog
(`app/input/restate.rs` calls it "in-scenario state 9"; Phobos opens
`Game::SpecialDialog = 9`, item 1059 = 0x423). Before this pass VERA only
opened it through the F2 / sidebar-gadget path. Phobos arms the same dialog at
scenario start and, while `BriefingTemp::ShowBriefing` is set, replaces the
0x423 caption and the status label. The pass keeps the two paths distinct
instead of renaming the stock "Return" button:

1. **Scenario gate** — `map/basic.rs::BasicSection::show_briefing` reads
   `[Basic] ShowBriefing=` with `read_bool_value` (`Ext/Scenario/Body.cpp:167`,
   constructor false at `Body.h:62`); an absent key stays `None`.
2. **Admission** — `app/briefing_start.rs::admits` combines the client
   `[Phobos] ShowBriefing`, the scenario flag and
   `ScenarioGameMode::Campaign` (`SessionClass::IsCampaign()`). It runs from
   `install_world_presentation`'s `Main_Game 0x0048CE7E` branch, VERA's
   counterpart of `MainGame_ShowBriefing @ 0x48CE85` /
   `AuxLoop_ShowBriefing @ 0x55D14F`. On admission it calls
   `input::restate::open` and sets `MatchPresentationState::briefing_at_start`
   (the `BriefingTemp::ShowBriefing` equivalent) only if the dialog really
   opened; `restate::open` clears that flag for the F2 path.
3. **uimd chain** — `app/loading/init_helpers.rs` loads
   `BriefingDialogUi::from_uimd_bytes` from the same `uimd_bytes` as
   `PauseMenuUi` / `SidebarIndicatorsUi`; `PresentationLoadAssets` carries it
   into `MatchPresentationState` through `loading/transitions.rs`.
4. **Labels** — `render_restate_shell` uses
   `BriefingDialogUi::button_label_key` / `status_label_key`, which return the
   authored CSF keys only when `briefing_at_start` is set. The F2 path keeps
   `GUI:Return` and `STT:BriefingButtonReturn`.

All new state fields and the `briefing_start` module are Phobos-only in
play; a game without the reads uses `GUI:Return` and never opens the start
dialog.

## Tests (production readers)

- `phobos_briefing_ui.rs::tests`:
  `an_absent_uisettings_section_keeps_the_phobos_reader_defaults`,
  `authored_uisettings_labels_replace_only_their_own_defaults`,
  `an_authored_empty_status_label_stays_empty_like_read_string`.
- `phobos_settings.rs::tests`:
  `show_briefing_defaults_to_yes_and_reads_an_explicit_no`, plus the extended
  `imported_switches_are_read` and `missing_profile_or_section_keeps_the_source_defaults`.
- `phobos_briefing_ui.rs::tests`:
  `the_authored_labels_apply_only_to_the_scenario_start_path`.
- `map/basic.rs::tests`:
  `the_basic_show_briefing_flag_defaults_to_absent_and_reads_both_spellings`.
- `app/briefing_start.rs::tests`:
  `the_modelled_gates_admit_only_a_campaign_with_both_switches_on`.

## Residuals

- The fourth native gate, `ScenarioClass::Start`'s `showBriefing` stack
  argument (`Misc/Hooks.UI.cpp:352`), has no VERA owner; `admits` treats it as
  true. A loaded save or non-fresh launch therefore also admits when the two
  switches and campaign mode hold.
- `Ext/Scenario/Hooks.cpp:21` also reads `ShowBriefing` under the scenario's
  own MISSIONMD section; only the `[Basic]` layer is ported here (the
  `campaign_loading.rs` owner belongs to another in-flight branch).
- `BriefingTheme` (`ScenarioExt`, `SideExt::BriefingTheme`,
  `Misc/Hooks.UI.cpp:359-371`) and the `0x683F66` / `0x685D95` redraw skips
  are not ported; `audio_runtime::bind_match_theme_context` owns the map theme.
- `0x604985` returns a dialog-level status label, while VERA shows the status
  line only while a control is hovered; the authored label therefore appears
  on hover of the resume button.
- The button label is a CSF key resolved through `i18n::label` at draw time.
