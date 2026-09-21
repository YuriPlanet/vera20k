# Retail shell acceptance

The requested scope is **all player-facing VERA20k shells**, starting with
Skirmish. The July [stock-skirmish design](2026-07-25-exact-stock-skirmish-shell-ui-design.md)
and `tools/exact_shell_ui_matrix` cover only a subset. They cannot certify this
whole task. Correct existing behavior is retained; missing or unproven required
behavior remains open. The 20,000-unit / 30-player scale exception in AGENTS.md
remains explicit; retail-size comparisons do not certify the extended roster.

Prioritize ordinary player journeys and frequently visible differences. The user's
September 12 continuation explicitly asks not to spend too much time on edge
cases. Record rare, unproven corners with their limits instead of expanding each
increment into exhaustive low-frequency emulation; they must not conceal a broken
ordinary route or justify a broader parity claim.

## Acceptance for every reachable route

1. Establish active retail callers, resource/control identities, inputs, state
   writers and exit/return paths. Recheck research against `gamemd.exe` and data;
   a resource or inherited function's existence does not prove reachability.
2. Compare geometry, composition order, assets/frames, palettes, font/CSF text,
   cursor, focus, disabled/pressed/hover states and help. Cover 640x480, 800x600,
   1024x768 and further distinct active sizing branches. Capture resolution,
   crop, scaling, color space, cursor and animation phase must be comparable.
3. Exercise mouse press/release/capture, dragging outside, keyboard default and
   cancel, tab order, editing/selection/caret, list/combo scrolling, and modal
   blocking. Check sound events and transitions as well as steady frames.
4. Validate defaults, persisted state, commit/cancel, errors, child return,
   repeated entry, teardown and handoff through the production path. Preserve
   one authoritative settings/selection/result owner across these operations.
5. Distinguish native behavior established, named Rust regressions tested, and
   parity demonstrated. Record native-derived comparisons with binary/data
   identity and bounded coverage. Tests or plausible art alone cannot close
   visual acceptance. Sampled comparisons do not certify unsampled states.

## Required family inventory

This is a discovery/acceptance map, not a completion ledger. Expand it when an
active caller exposes another route; exclude only with recorded evidence.

| Family | Required journey and children | Current implementation entry points |
| --- | --- | --- |
| Skirmish first | Main menu → Single Player → setup; roster/name/country/color/team/AI, settings, map/mode chooser, cooperative selection, validation, RMG and saved-seed children; Back/reentry; Start → loading → first tactical frame | `src/app/shell_skirmish.rs`, `shell_random_map.rs`, `src/ui/skirmish_shell/`, `src/app/frontend/skirmish_shell_render.rs` |
| Startup/main menu | Startup presentation, main menu, Single Player, navigation/help, website action, exit confirmation/shutdown | `src/app/shell_main_menu.rs`, `src/ui/main_menu_shell/`, `single_player_shell/` |
| Campaign | Choice/difficulty, launch, active briefing/media checks, progression and return | `src/ui/main_menu_dialogs.rs`, `src/app/shell_main_menu.rs` |
| Saved games | Single Player Load and in-game Load/Save/Delete; metadata, edit, confirmation/error, restoration and return | `src/app/persistence/save_load_panel.rs` |
| Launcher options | Parent settings, previews, persistence; Keyboard and Network children; child return | `src/app/shell_main_menu.rs`, `src/ui/main_menu_dialogs.rs` |
| Movies/credits | Submenu, movie list, playback, credits, cancellation and return | `src/ui/main_menu_dialogs.rs`, `src/app/shell_main_menu.rs` |
| LAN/network | Settings/identity, browse/host/join, lobby, map/options/readiness/chat, Start, disconnect/cancel/error and reentry | Main-menu dispatch; active native route inventory still required |
| Westwood Online | Reachable retail entry/settings/login, connection/error/cancel and subsequent active dialogs | Main-menu dispatch; native local behavior must be separated from external service availability |
| In-game shell | Resume, Abort/Leave/Restart/Observe by mode, Game Controls, Keyboard, Sound, save children, objectives/mission restatement/diplomacy where active | `src/ui/pause_menu.rs`, `src/app/input/in_game_options.rs`, `dispatch.rs` |
| Results/return | Skirmish score, applicable campaign/cooperative/network results, Continue, media/progression, shell reconstruction | `src/ui/score_shell.rs`, `src/app/frontend/score_shell_render.rs` |
| Common children | Reachable confirmations, errors, media checks and progress; mode/side composition, focus and default/cancel restoration | `src/ui/shell/` plus each owning route |

The initial independent read-only audit found log-only or presentation-only
actions in campaign, movies, network/online, website and options children, and
egui substitutes for several menus. Source must be rechecked when those families
are reached. Their presence in this inventory is not a parity claim.

## Delivery and final audit

Use coherent behavior increments with native evidence, implementation, production
integration and appropriate regressions. Refactor affected ownership when it
removes duplicate authority or makes a proven second consumer correct; avoid a
generic UI rewrite merely to replace a rendering toolkit.

After each increment a fresh independent read-only critic inspects original
evidence, design, diff and actual validation, with freedom to challenge omitted
scope. Correct confirmed findings until a scoped pass. Preserve independently
confirmed Ghidra labels/comments, save and read them back, update relevant docs,
then commit, publish and merge under the user's authority. Revalidate affected
neighbors after shared changes and integration conflicts.

Finish only after the complete reachable route graph has been checked against
this inventory, including error/cancel/return paths, whole-scope validation and
an independent omission/regression audit. Keep one current local checkpoint for
unfinished work; do not convert partial progress into a completion claim.
