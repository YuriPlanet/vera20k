# SIDEBAR_SOVIET_SHP_LOAD_PATH_FUN_006D02B0_GHIDRA_REPORT

Date: 2026-05-27

## Target question

What exact sidebar SHP filenames does active YR load through `FUN_006D02B0` and its immediate load-path callees for the Soviet sidebar, in what order, and is Soviet art selected by explicit Soviet filenames or by the active side MIX search state?

## Non-goals

- Do not inspect unrelated sidebar drawing, placement, or cameo insertion behavior.
- Do not inspect the full MIX resolver beyond the filename-to-CDFile load proof needed here.
- Do not inspect retail MIX archive contents; this is a Ghidra load-path report.
- Do not change Rust, INI, published sibling docs, or Ghidra state.

## Evidence needed to mark COMPLETE

- `FUN_006D02B0` decompile plus assembly/disassembly evidence for its caller and loop bounds.
- `SidebarClass__LoadSHPs` decompile plus assembly/disassembly evidence for filename constants and order.
- Immediate callee evidence for `PowerClass__Init_IO` / `RadarClass__Init_For_House` because they execute before the main sidebar chrome loads.
- `CDFileClass__Constructor` / `LoadFileFromMIX` evidence sufficient to prove these calls pass generic filenames into the normal file resolver, not side-specific filename branches.
- Active in YR labels for all material findings.

## Stop conditions

- Stop if Ghidra MCP read-only access is unavailable.
- Stop if the function boundary is missing and would require creating a function.
- Stop if the target expands into unrelated sidebar systems.
- Stop after writing this one report and updating only the swarm claims file.

## Verified binary findings

### 1. `FUN_006D02B0` is active from side-MIX initialization

Active in YR: Yes.

`FUN_006D02B0` is called from `InitSideMixFiles` at `0x00535347` after side MIX file setup, palette setup, sidebar text color setup, and `UIMD.INI` command-bar read. Ghidra xrefs show `From 00535347 in InitSideMixFiles [UNCONDITIONAL_CALL]`; the caller decompile and assembly place the call after `RulesClass__ReadCommandBar`.

For Soviet/Yuri, the parent-established caller fact remains material: `InitSideMixFiles` maps side `2` to side `1` for side MIX filenames before formatting `SIDEC%02dMD.MIX`, `SIDEC%02d.MIX`, and `SIDENC%02d.MIX`. This report does not re-prove the full side-MIX substitution beyond confirming the live caller relation.

### 2. `FUN_006D02B0` load order after `SidebarClass__LoadSHPs`

Active in YR: Yes.

`FUN_006D02B0` first calls `SidebarClass__LoadSHPs` at `0x006D02BB`. It then loops over `Button%02d.SHP` at string `0x00842828`, with `ESI` starting at `0`, global destination pointer `EDI = 0x00B0C148`, and loop end `EDI < 0x00B0C1AC`. The pointer range is `0x64` bytes of 4-byte entries, so the loop loads 25 button SHPs: `Button00.SHP` through `Button24.SHP`.

Evidence: `FUN_006D02B0` decompile; disassembly `0x006D02BB` call to `0x006A5840`, `0x006D02D1` pushes `0x842828`, `0x006D02FB` increments `ESI`, and `0x006D02FC..0x006D0302` compares `EDI` against `0xB0C1AC`. `inspect_memory_content 0x00842828` detects `Button%02d.SHP`.

### 3. `SidebarClass__LoadSHPs` starts with radar/utility-button and power-bar loads

Active in YR: Yes.

`SidebarClass__LoadSHPs` at `0x006A5840` begins by calling `PowerClass__Init_IO` at `0x006A5849`. That callee calls `RadarClass__Init_For_House`, then loads `POWERP.SHP`.

`RadarClass__Init_For_House` loads two utility button SHPs using pointers stored at globals `0x008391F4` and `0x008391F8`. At inspection time those globals point to `OPTBTN.SHP` (`0x0083927C`) and `DIPLOBTN.SHP` (`0x00839288`). No `RADAR.SHP` or `RADARY.SHP` load is present in this immediate path; those strings have only data-table xrefs in this scope.

Evidence: `PowerClass__Init_IO` decompile and assembly at `0x0063F7C0..0x0063F7D9`; `inspect_memory_content 0x00836D9C` detects `POWERP.SHP`; `RadarClass__Init_For_House` decompile and assembly at `0x00652F44..0x00652F75`; `inspect_memory_content 0x008391F4`, `0x0083927C`, and `0x00839288`.

### 4. Main chrome loads are generic filenames, in fixed order

Active in YR: Yes.

After palette setup, `SidebarClass__LoadSHPs` issues `CDFileClass__Constructor` calls with fixed filename constants. The order is:

1. `GCLOCK2.SHP`
2. `SELL.SHP`
3. `REPAIR.SHP`
4. `TAB00.SHP`
5. `TAB01.SHP`
6. `TAB02.SHP`
7. `TAB03.SHP`
8. `R-DN.SHP`
9. `R-UP.SHP`
10. `SIDE1.SHP`
11. `SIDE2.SHP`
12. `SIDE3.SHP`
13. `ADDON.SHP`

The tab loop starts with `EDI = 0`, formats `TAB%02d.SHP` from string `0x0083FA34`, increments once per load, and runs while the gadget pointer moves from `0x00B07C48` to `< 0x00B07DC8` in `0x60`-byte steps: exactly four iterations.

Evidence: `SidebarClass__LoadSHPs` decompile; disassembly and assembly context at `0x006A58BF`, `0x006A58CF`, `0x006A5907`, `0x006A5948`, `0x006A5994`, `0x006A59CA`, `0x006A59F8`, `0x006A5A0C`, `0x006A5A20`, `0x006A5A34`; `inspect_memory_content` at `0x0083FA58`, `0x0083FA4C`, `0x0083FA40`, `0x0083FA28`, `0x0083FA1C`, `0x0083FA10`, `0x0083FA04`, `0x0083F9F8`, `0x0083F9EC`.

### 5. Country/observer icon SHPs load after the second ConvertClass

Active in YR: Yes.

`SidebarClass__LoadSHPs` then builds a second `ConvertClass` at `DAT_0087F6D0` and loads the sidebar country/observer icon SHPs in this order:

1. `OBSALLI.SHP`
2. `OBSSOVI.SHP`
3. `OBSYURI.SHP`
4. `RANI.SHP`
5. `OBSI.SHP`
6. `USAI.SHP`
7. `JAPI.SHP`
8. `FRAI.SHP`
9. `GERI.SHP`
10. `GBRI.SHP`
11. `DJBI.SHP`
12. `ARBI.SHP`
13. `LATI.SHP`
14. `RUSI.SHP`
15. `YRII.SHP`

Evidence: `SidebarClass__LoadSHPs` decompile and disassembly `0x006A5AB2..0x006A5BD9`; string block inspection at `0x0083F938..0x0083F9EC`, including direct checks for `OBSALLI.SHP`, `OBSSOVI.SHP`, and `OBSYURI.SHP`.

### 6. Art selection is not an explicit Soviet filename branch in this path

Active in YR: Yes.

Within `FUN_006D02B0`, `SidebarClass__LoadSHPs`, `PowerClass__Init_IO`, and `RadarClass__Init_For_House`, no branch tests side, country, house, or scenario to choose Soviet-suffixed chrome names. The filenames are generic constants (`SIDE1.SHP`, `SIDE2.SHP`, `SIDE3.SHP`, `TAB%02d.SHP`, `REPAIR.SHP`, `SELL.SHP`, `POWERP.SHP`, etc.) or preselected globals for `OPTBTN.SHP` / `DIPLOBTN.SHP`.

`CDFileClass__Constructor` first calls `LoadFileFromMIX` with the passed filename; `LoadFileFromMIX` uppercases/canonicalizes the filename, hashes it, consults the global file cache, and falls back through normal file construction if needed. There is no Soviet-specific filename switch inside this constructor path.

Therefore Soviet art selection in this path is by the active MIX/search state established before the call, not by explicit Soviet filenames inside `FUN_006D02B0` or `SidebarClass__LoadSHPs`.

Evidence: `CDFileClass__Constructor` decompile and assembly at `0x004A38D0..0x004A3966`; `LoadFileFromMIX` decompile and assembly at `0x005B40B0..0x005B4262`; filename callsites listed above.

## Inference

Because `InitSideMixFiles` sets up the side MIX files before `FUN_006D02B0`, and because the actual SHP load calls pass generic filenames into `CDFileClass__Constructor`, the Soviet-vs-Allied physical asset decision should be modeled as resolver/MIX precedence plus current side setup. The binary evidence in this slot does not prove which retail archive contains each named file; that is an asset-content question.

## Implementation Handoff

1. Verified behavior: Soviet sidebar chrome load path uses generic filenames after side MIX setup, not explicit Soviet filenames. Rust delta: make `SidebarChromeSet` construction able to model gamemd's active side MIX resolver for `SIDE1/2/3`, `TAB00..03`, `REPAIR`, `SELL`, `GCLOCK2`, `POWERP`, and utility button art instead of hardcoding the idea that every named piece is selected only from one theme archive. Affected surface: `src/render/sidebar_chrome.rs`. Acceptance scenario: after initializing Soviet side, loading `REPAIR.SHP` / `SELL.SHP` follows the same resolver precedence as other generic CDFile loads. Proposed test: `test_soviet_sidebar_generic_chrome_names_resolve_through_side_mix_order`. Risk: high screenshot drift if the current per-theme direct archive lookup disagrees with gamemd resolver precedence.

2. Verified behavior: `POWERP.SHP` loads through `PowerClass__Init_IO` before main chrome, not from a `power.shp`/`powerp.shp` pair inside `SidebarClass__LoadSHPs`. Rust delta: keep `powerp.shp` as the meter source, but do not treat `power.shp` as a binary-proven part of this load path unless another report proves it. Affected surface: `src/render/sidebar_chrome.rs`, `src/app_sidebar_build.rs`. Acceptance scenario: sidebar atlas can omit `power.shp` without blocking binary-proven `POWERP.SHP` meter frames. Proposed test: `test_sidebar_power_meter_uses_powerp_without_requiring_power_shp`. Risk: medium; wrong required asset can disable otherwise valid Soviet chrome.

3. Verified behavior: `FUN_006D02B0` loads exactly `Button00.SHP` through `Button24.SHP` after `SidebarClass__LoadSHPs`. Rust delta: command-bar button art should be represented as a 25-entry post-chrome load table if/when command bar art is implemented. Affected surface: future sidebar command-bar atlas, likely near `src/render/sidebar_chrome.rs`. Acceptance scenario: loader formats `Button%02d.SHP` for indices `0..25` exclusive and preserves order. Proposed test: `test_sidebar_command_button_loads_button_00_through_24_in_order`. Risk: low until command-button art is rendered; high for command-bar parity once visible.

## Negative Facts / Do Not Do

- Do not implement Soviet sidebar chrome by changing filenames to Soviet-prefixed names such as `NASIDE1.SHP`; no such branch exists in this load path. Evidence: fixed string callsites in `SidebarClass__LoadSHPs` at `0x006A59F8..0x006A5A3E`.
- Do not treat `RADAR.SHP` / `RADARY.SHP` as loaded by `FUN_006D02B0` or `SidebarClass__LoadSHPs`; this slot only found `OPTBTN.SHP`, `DIPLOBTN.SHP`, and `POWERP.SHP` in the initial immediate callee chain. Evidence: `RadarClass__Init_For_House` at `0x00652F44..0x00652F75`.
- Do not use `TABS.SHP` as a substitute for the four tab button SHPs in this load path. The binary formats `TAB%02d.SHP` exactly four times. Evidence: loop at `0x006A5943..0x006A598C`.
- Do not require `power.shp` for the binary-proven power meter path from this target. The proven filename is `POWERP.SHP` at `0x00836D9C`. Evidence: `PowerClass__Init_IO` at `0x0063F7CA`.
- Do not conclude retail physical archive ownership from this Ghidra report alone. The binary proves generic resolver calls and side-MIX setup, not which MIX file contains each SHP.

## Remaining Uncertainty

- Exact retail archive membership for `REPAIR.SHP`, `SELL.SHP`, `POWERP.SHP`, `TAB00..03.SHP`, and `SIDE1..3.SHP` was not checked because this was a Ghidra-only load-path slot.
- The exact global MIX precedence inside `LoadFileFromMIX` was not fully expanded in this report; the relevant binary handoff is that the path uses the normal resolver after `InitSideMixFiles`, not explicit Soviet filenames.
- `RADAR.SHP` / `RADARY.SHP` load path remains outside this target, despite sibling docs covering radar positioning and palette selection.

## Stale-doc replacement wording

Suggested replacement for [docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md) section 7.3:

> Current Rust builds separate Allied/Soviet/Yuri sidebar atlases from `sidec01.mix`, `sidec02.mix`, and `sidec02md.mix`. Fresh `FUN_006D02B0` / `SidebarClass__LoadSHPs` proof shows gamemd does not explicitly reload `REPAIR.SHP` or `SELL.SHP` through a side branch; it calls the generic CDFile/MIX resolver after `InitSideMixFiles` has installed the current side MIX state. Treat current Rust as potentially resolver-order-aware only if its direct per-theme lookup matches gamemd's active MIX search order for the same side; otherwise it is still more side-directed than the binary path.

## Status

COMPLETE.
