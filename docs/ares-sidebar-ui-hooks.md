# Ares sidebar, string-table and interface hooks

Build with `--features ares`. These 72 Ares 3.0 hooks come from
`Ext/Rules/Hooks.CameoList.cpp`, `Ext/Rules/Hooks.Selects.cpp`, `Misc/Gadgets.cpp`,
`Misc/CSFLoader.cpp`, `Misc/Interface.cpp` and `Misc/Interface.PlayerColors.cpp`.
Most of them change how gamemd's own sidebar, gadgets and Win32 dialogs behave.
VERA20K draws its own sidebar and shell, so each hook is listed with what it
changes for a player and where the engine does that. Default builds keep the
vanilla behaviour in every row marked Ported.

## Ported

- **No cameo limit and Ares' own cameo list** (`SidebarClass_AddCameo_SkipSizeCheck`
  0x6A63B7, `StripClass_AddCameo_ReplaceItAll` 0x6A8710, `SidebarClass_FlashCameo`
  0x6AC6D9, `StripClass_RecheckCameos` 0x6AA600): an `ares` build never rejects a
  cameo for its strip's size (`strip_rejects_entry`), scans the whole strip when a
  trigger flashes a cameo (`flash_scan_limit`), inserts with `std::lower_bound`
  (an equal key enters after the existing cameo, `cameo_lower_bound`) and
  recalculates the top row by looking the old top cameo up again instead of the
  native `TopRow` adjustment (`CameoStrips::retain`). All in
  `src/ui/sidebar/cameo_order.rs`; the saved-order bound is in
  `src/app/sidebar_projection.rs`.
- **Shift-click queues five** (`SelectClass_ProcessInput_ProduceUnsuspended` 0x6AB773):
  `produce_copies` in `src/app/input/sidebar_eva.rs`, used by `sidebar_cameo_press`
  in `src/app/input/dispatch.rs`. The Options page's Shift-Click Batch Queue keeps
  its own count when ticked; a held build, a finished build and the structure strips
  keep their single event.

## Residuals

- **R1, Ares campaign menu configuration.** Trigger: uimd.ini `[UISettings]`
  `CampaignList`, `CampaignListSize`, `ShowSummary`, `ShowDebugCampaigns`,
  `Campaign1` to `Campaign4` with `.Image`, `.Palette`, `.Subline`, `.Tooltip`, and the
  rules campaign keys `DebugOnly`, `HoverSound`, `Summary`. Effect: up to four
  emblems with their own art, captions and campaigns (the Ares defaults are Allied and
  Soviet, as in the engine, with the emblem hit areas four fifths as wide), or a list
  of every campaign with a summary, hover sound and a Play button. Why open:
  it needs a uimd.ini `[UISettings]` reader, a campaign extension, a shell state for
  four emblems (`CampaignSide` has two fixed variants through seven files, including
  the capture diagnostics) and a list page; none exists yet.
- **R2, unlimited player colours.** Trigger: uimd.ini `[Colors]` `Count` above 8 (up to
  16), `Slot%d.ColorScheme`, `.Tooltip`, `.DisplayColor` and the `Observer.*` keys.
  Effect: the lobby offers `Count` colours chosen by scheme name, random colours are
  drawn from `Count`, the observer takes colour `Count`, and `SessionClass` maps a
  colour slot to a `[Colors]` scheme by name. Why open: no uimd.ini reader exists, and
  the engine's own list differs from Ares' numbering. VERA's lobbies list up to thirty
  colours from the rules' `[SkirmishColors]` ([lobby player colours](skirmish-colors.md)):
  the first eight keep priorities 0..=7, the observer keeps 8, the others take 9..=30,
  and a random colour stays in the first eight while one is free. Ares numbers its
  slots 0..`Count`-1, gives the observer `Count` and draws a random colour from all of
  them. With no uimd.ini `[Colors]` section Ares' defaults equal the engine's first
  eight colours and every colour row below holds for them.

## Dispositions

| Hook | Ares.dll | Name | Source | Disposition | Reason or engine place |
|---|---|---|---|---|---|
| `0x6A4EA5` | `1002C1C0` | SidebarClass_CTOR_InitCameosList | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `CameoStrips::default()` is the constructor state (6A4E60); Ares only resets its vectors |
| `0x6A4FD8` | `1002C1C0` | SidebarClass_Load_InitCameosList | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | a load replaces the strips (`prepare_saved_order`, `src/app/sidebar_projection.rs`); Ares only resets its vectors |
| `0x6A61B1` | `1002C230` | SidebarClass_SetFactoryForObject | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `SidebarClass::LinkFactory` 6A6140 finds the retained entry by identity (`src/app/sidebar_projection.rs`) |
| `0x6A63B7` | `1002C160` | SidebarClass_AddCameo_SkipSizeCheck | `Ext/Rules/Hooks.CameoList.cpp` | Ported | `strip_rejects_entry`, `src/ui/sidebar/cameo_order.rs` (no 75-entry limit); the saved-order bound in `src/app/sidebar_projection.rs` follows it |
| `0x6A8D1C` | `1002C5C0` | StripClass_MouseMove_GetCameos1 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::AI` 0x6A8B30 runs over the retained strip (`CameoStrips`, `src/ui/sidebar/cameo_order.rs`), whatever its size |
| `0x6A8DB5` | `1002C5F0` | StripClass_MouseMove_GetCameos2 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::AI` 0x6A8B30 runs over the retained strip (`CameoStrips`, `src/ui/sidebar/cameo_order.rs`), whatever its size |
| `0x6A8F6C` | `1002C620` | StripClass_MouseMove_GetCameos3 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::AI` 0x6A8B30 runs over the retained strip (`CameoStrips`, `src/ui/sidebar/cameo_order.rs`), whatever its size |
| `0x6A9304` | `1002C590` | StripClass_GetTip_NoLimit | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::GetCameoTooltip` 6A92E0 reads the hovered view item (`cameo_tip_text`, `src/app/input/tooltips.rs`), no bound |
| `0x6A9747` | `1002C450` | StripClass_Draw_GetCameo1 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::Draw` 0x6A9540 reads each retained entry (`src/ui/sidebar/sidebar_view.rs`, `src/app/presentation/sidebar_build.rs`), no slot array or bound |
| `0x6A9866` | `1002C4C0` | StripClass_Draw_Status_1 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::Draw` 0x6A9540 reads each retained entry (`src/ui/sidebar/sidebar_view.rs`, `src/app/presentation/sidebar_build.rs`), no slot array or bound |
| `0x6A9886` | `1002C4F0` | StripClass_Draw_Status_2 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::Draw` 0x6A9540 reads each retained entry (`src/ui/sidebar/sidebar_view.rs`, `src/app/presentation/sidebar_build.rs`), no slot array or bound |
| `0x6A99BE` | `1002C430` | StripClass_Draw_BreakDrawLoop | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | draw-loop exit inside gamemd's `StripClass::Draw`; the engine draws the visible items |
| `0x6A9B4F` | `1002C550` | StripClass_Draw_TestFlashFrame | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::Draw` 0x6A9540 reads each retained entry (`src/ui/sidebar/sidebar_view.rs`, `src/app/presentation/sidebar_build.rs`), no slot array or bound |
| `0x6A9EBA` | `1002C520` | StripClass_Draw_Status_3 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `StripClass::Draw` 0x6A9540 reads each retained entry (`src/ui/sidebar/sidebar_view.rs`, `src/app/presentation/sidebar_build.rs`), no slot array or bound |
| `0x6AAD2F` | `1002C0E0` | SelectClass_ProcessInput_LoadCameoData1 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | a click on an empty slot does nothing, otherwise the entry decides (`sync_cameos`, `cameo_click`) |
| `0x6AB0B0` | `1002C0B0` | SelectClass_ProcessInput_LoadCameo2 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | the right-press request state is a retained record (`CameoRequestState`, `src/ui/sidebar/cameo_order.rs`) |
| `0x6AB49D` | `1002BF30` | SelectClass_ProcessInput_FixOffset1 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6AB4E8` | `1002BF50` | SelectClass_ProcessInput_FixOffset2 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6AB577` | `1002BF70` | SelectClass_ProcessInput_FixOffset3 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | request state and predicted progress: `pending_production_smoothed_stage`, `src/ui/sidebar/cameo_order.rs` |
| `0x6AB620` | `1002BFF0` | SelectClass_ProcessInput_FixOffset4 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6AB741` | `1002C010` | SelectClass_ProcessInput_FixOffset5 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6AB802` | `1002C030` | SelectClass_ProcessInput_FixOffset6 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | the Started request is a retained record (`CameoRequestState::Started`) |
| `0x6AB825` | `1002C060` | SelectClass_ProcessInput_FixOffset7 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6AB92F` | `1002C0A0` | SelectClass_ProcessInput_FixOffset9 | `Ext/Rules/Hooks.CameoList.cpp` | Not applicable | offset into gamemd's per-tab record array; the engine keys the click by cameo identity (`sidebar_eva::cameo_click`) |
| `0x6ABBCB` | `1002C2A0` | StripClass_AbandonCameosFromFactory_GetPointer1 | `Ext/Rules/Hooks.CameoList.cpp` | Equivalent | `Sidebar6ABAD0` -> `Strip6ABB60` clears the retained factory links (`src/app/sidebar_projection.rs`) |
| `0x6AC6D9` | `1002C1D0` | SidebarClass_FlashCameo | `Ext/Rules/Hooks.CameoList.cpp` | Ported | `flash_scan_limit`, `CameoStrips::flash_type_index`, `src/ui/sidebar/cameo_order.rs`: the match covers the whole strip |
| `0x6A64C9` | `1002CDE0` | SidebarClass_AddCameo_Strip | `Ext/Rules/Hooks.Selects.cpp` | Equivalent | adding a cameo to an empty sidebar selects its tab (`CameoTransitions::active_tab_destination`, `src/ui/sidebar/cameo_order.rs`) |
| `0x6A75B9` | `1002CE00` | SidebarClass_SetActiveTab_Strip1 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A7619` | `1002CE30` | SidebarClass_SetActiveTab_Strip2 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A793F` | `1002CE60` | SidebarClass_Update_Strip1 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A79A0` | `1002CE90` | SidebarClass_Update_Strip2 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A7EEE` | `1002D080` | sub_6A7D70_Strip1 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A801C` | `1002D0B0` | sub_6A7D70_Strip2 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6A96D9` | `1002CF30` | StripClass_Draw_Strip | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | index of the shared select button drawn for a slot; the engine draws and hit-tests the same item list |
| `0x6ABF44` | `1002D0E0` | sub_6ABD30_Strip1 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6ABFB2` | `1002D100` | sub_6ABD30_Strip2 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | gamemd gadget-list plumbing for the shared SelectClass buttons; the engine pools one hit region per visible cameo, grown on demand (`sync_cameos`, `src/app/input/gadget_input.rs`) |
| `0x6AC02F` | `1002D130` | sub_6ABD30_Strip3 | `Ext/Rules/Hooks.Selects.cpp` | Not applicable | tooltip registration for the select buttons; the engine builds tooltip regions from the visible items (`in_game_regions`, `src/app/input/tooltips.rs`) |
| `0x7346D0` | `10060DD0` | CSF_LoadBaseFile | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x734823` | `10060D60` | CSF_AllocateMemory | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x7349CF` | `10060F20` | StringTable_ParseFile_Buffer | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x734A97` | `10060EE0` | CSF_SetIndex | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x734E83` | `10060E80` | CSF_LoadString_1 | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x734EC2` | `10060EC0` | CSF_LoadString_2 | `Misc/CSFLoader.cpp` | Deferred | Deferred: CSF loading is handled separately |
| `0x6AB773` | `10064520` | SelectClass_ProcessInput_ProduceUnsuspended | `Misc/Gadgets.cpp` | Ported | `produce_copies`, `src/app/input/sidebar_eva.rs`, called by `sidebar_cameo_press`, `src/app/input/dispatch.rs`: Shift queues five PRODUCE events |
| `0x4E42A0` | `10067000` | GameSetup_GetColorTooltip | `Misc/Interface.PlayerColors.cpp` | Equivalent | tooltips for the eight colours and the observer are `STT:PlayerColor*` (`src/ui/skirmish_shell/state/hit_test.rs`); `Slot%d.Tooltip` is residual R2 |
| `0x4E4556` | `10067200` | hWnd_GetSlotColorIndex | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E4580` | `10067250` | hWnd_IsAvailableColor | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E46BB` | `10067270` | hWnd_PopulateWithColors | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E4A41` | `10067340` | hWnd_SetPlayerColor_A | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E4B47` | `10067380` | hWnd_SetPlayerColor_B | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E4C9D` | `100673A0` | hWnd_UpdatePlayerColors_A | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x4E4D67` | `100673E0` | hWnd_UpdatePlayerColors_B | `Misc/Interface.PlayerColors.cpp` | Not applicable | Win32 colour combo-box bookkeeping; the shell tracks each row's claim itself (`available_color_choices`, `color_claimed_by_other_row`, `src/ui/skirmish_shell/state/combos.rs`) |
| `0x69B69B` | `10066FC0` | GameModeClass_PickRandomColor_Unlimited | `Misc/Interface.PlayerColors.cpp` | Equivalent | `ColorCount` is 8 unless uimd.ini says more, so the draw is `RandomRanged(0, 7)` as `draw_collision_free_color`, `src/skirmish_launch.rs`; Count > 8 is residual R2 |
| `0x69B7FF` | `100671D0` | Session_SetColor_Unlimited | `Misc/Interface.PlayerColors.cpp` | Equivalent | same draw, `resolve_local_color` -> `draw_collision_free_color`, `src/skirmish_launch.rs`; Count > 8 is residual R2 |
| `0x69B949` | `10067090` | Game_ProcessRandomPlayers_ColorsA | `Misc/Interface.PlayerColors.cpp` | Equivalent | same draw for a human row, `resolve_human_color`, `src/skirmish_launch.rs`; Count > 8 is residual R2 |
| `0x69B97D` | `100670F0` | Game_ProcessRandomPlayers_ObserverColor | `Misc/Interface.PlayerColors.cpp` | Equivalent | the observer's colour is 8 = `ColorCount` (`human.slot.color_index = 8`, `src/skirmish_launch.rs`); Count > 8 is residual R2 |
| `0x69BA13` | `100670C0` | Game_ProcessRandomPlayers_ColorsB | `Misc/Interface.PlayerColors.cpp` | Equivalent | same draw for an AI row, `src/skirmish_launch.rs`; Count > 8 is residual R2 |
| `0x52EC18` | `10066CF0` | CampaignMenu_hDlg_PreHandleGeneral | `Misc/Interface.cpp` | Residual | R1: campaign list mode ([UISettings] CampaignList), selection change and summary text |
| `0x52ED21` | `10066BA0` | CampaignMenu_hDlg_ClickedPlay | `Misc/Interface.cpp` | Residual | R1: campaign list mode, Play starts the selected list item |
| `0x52EE04` | `10066DE0` | CampaignMenu_hDlg_SelectHoverSound | `Misc/Interface.cpp` | Equivalent | hover voice: `CampaignSide::voice` equals the ALL1/SOV1 `HoverSound` defaults; configured campaigns and `HoverSound=` are R1 |
| `0x52EF39` | `10066BE0` | CampaignMenu_hDlg_ImageMouseDown | `Misc/Interface.cpp` | Equivalent | emblem press is the shell's press/release handling (`CampaignShellState`); only the 3rd and 4th emblem are R1 |
| `0x52F00B` | `10066C40` | CampaignMenu_hDlg_PopulateCampaignList | `Misc/Interface.cpp` | Residual | R1: campaign list mode fills a list box from the rules' campaigns |
| `0x52F191` | `100669E0` | CampaignMenu_InitializeMoreButtons | `Misc/Interface.cpp` | Equivalent | default [UISettings] gives Allied + Soviet like `CampaignSide` (`src/ui/campaign_shell/mod.rs`); configured campaigns are residual R1 |
| `0x52F232` | `10066EA0` | CampaignMenu_hDlg_StartCampaign | `Misc/Interface.cpp` | Equivalent | default Battle names ALL1 and SOV1 are `CampaignSide::campaign_name`; configured Battle names are R1 |
| `0x60357E` | `10066B20` | CampaignMenu_SetAnimationDuration | `Misc/Interface.cpp` | Residual | R1: animation timer for the 3rd and 4th emblem |
| `0x60378B` | `10066970` | CampaignMenu_ChooseButtonPalette | `Misc/Interface.cpp` | Residual | R1: `CampaignN.Palette` for the emblems (defaults FSALG.PAL, FSSLG.PAL, FSBCLG.PAL) |
| `0x603A2E` | `10066910` | CampaignMenu_ChooseButtonImage | `Misc/Interface.cpp` | Residual | R1: `CampaignN.Image` for the emblems (defaults FSALG.SHP, FSSLG.SHP) |
| `0x6041ED` | `10066F20` | DialogFunc_SubText_CampaignIconA | `Misc/Interface.cpp` | Equivalent | emblem tooltip defaults to its Subline, as `CampaignSide::caption_key`; `CampaignN.Tooltip` is R1 |
| `0x6041F5` | `10066F10` | DialogFunc_CampaignMenu_CampaignIconB | `Misc/Interface.cpp` | Not applicable | skips gamemd's own campaign-icon tooltip after 6041ED; no engine counterpart |
| `0x60A90A` | `10066B60` | CampaignMenu_StaticButtonImage | `Misc/Interface.cpp` | Residual | R1: background image for every campaign button |
| `0x62267F` | `10066F90` | Dialog_Show_GetTemplate | `Misc/Interface.cpp` | Not applicable | Win32 dialog template id cache; the engine opens its own shell pages |
| `0x6226EE` | `10066FA0` | Dialog_Show_UpdateControls | `Misc/Interface.cpp` | Not applicable | Win32 `updateMenu` after dialog creation (control moves and hides); the campaign layout part is R1, the main-menu button actions belong to Interface.Buttons |
