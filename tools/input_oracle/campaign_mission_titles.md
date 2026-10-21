# Campaign mission names and initial-save arguments

`campaign_start.py --mission-titles` extends the existing `CampaignReader`.
It retains `campaign_mission_titles.json` for the production mission-metadata
reader and initial-save admission tests. Other campaign corpus modes are
unchanged.

```sh
VERA20K_GAMEMD_EXE='<original-gamemd.exe>' \
  python -m tools.input_oracle.campaign_start --mission-titles --check
```

The stock input is the selected `MISSIONMD.INI` from the existing local MIX
reader and Name/Sav strings from the existing CSF reader. Three rows use
ALL01UMD, ALL02UMD and SOV01UMD sections. Declared synthetic controls cover a
missing file, missing/empty/wrong-case sections and keys, comma-hex `Name`,
45-unit legacy names, 31-byte UIName keys, missing CSF entries, the exact
uppercase `MISSING:` fallback and UTF-16 truncation through a surrogate pair.

The file-existence result is an explicit boundary input at686D64, immediately
after `Exists473C50`. The original TEST/JZ reaches6873AB for a missing file,
retaining prior names. For a present file, the fixture executes the original
legacy name block686D7F..686DEC and localized block686EAB..687000, excluding
the unrelated Briefing block between them. This proves neither physical INI
parsing nor successful whole-scenario initialization.

`Name` resides at Scenario+1360, the ANSI key at+13BA and Sav at+13DA. Legacy
Name reads45 UTF-16 units; localized names and Sav write an explicit terminator
at unit44. A present file with no usable UIName retains prior Sav. A missing
file bypasses both name blocks. UTF-16 output arrays preserve native code units
without silently repairing a split surrogate; they end at the first NUL.

Initial-save rows execute55DBCD until the actual reached67CEF0 save entry or
55DC99 bypass. Inputs are the logical scenario filename, original-produced
save title, mode and process latch. The fixture captures ECX filename, EDX
UTF-16 description and stack argument0. It does not execute save IO, supply a
successful return, or claim a complete MainTick. Path controls observe the
last-dot operation over the entire input string, including directories.

Every row compares all three full0x3F4 RNG buffers, and verifies that original
executable sections remain unchanged. The shared before/after buffers are
retained once. These bounded name/admission slices have no timer or detach
calls. Whole loading, modal timing, the Windows save-filename conversion and
actual native save/restore are outside this corpus.

The original process latch ABCE08 is armed by campaign emblem selection
52F28B and successful ordinary MapSelect5AE126, consumed before save55DBEE,
and set by successful campaign load67F7C1. Restart and defeat retry preserve
it. Those surrounding call paths are instruction evidence; this corpus does
not emulate the Windows dialogs or replace their results.
