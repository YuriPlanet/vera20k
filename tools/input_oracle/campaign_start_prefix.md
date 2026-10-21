# Explicit campaign filename and record selection

`python -m tools.input_oracle.campaign_start --start-prefix --check` replays six
synthetic controls through the existing CampaignReader/BulletReader VM. Set
`VERA20K_GAMEMD_EXE` to the retail executable being compared. No
campaign asset directory, mod resources, game installation writes or Windows
process is needed. `--write` explicitly regenerates the corpus and metadata.

The existing shared runner checks source stability during generation, and every
control checks all executable PE sections against their original bytes. Binary
identity belongs in the local execution receipt under the current evidence
retention policy; the public corpus carries computed results and scope.

## Executed boundary

Original Scenario ctor `6832C0`, movie registry `674550`, Campaign ctor/list/read
`46CB60/46CE10/46CCD0`, and StartScenario `683AB0` through `683BC9` execute.
The fixture supplies two synthetic catalog records with distinct Scenario, CD and
FinalMovie values. The native filename copy and uppercase execute; no expected
filename, index, CD, lookup result or accepted StartScenario result is injected.

An explicit filename is independent of the caller's campaign index: OTHER.MAP
with index 0 retains record FIRST and CD 2; even SECOND.MAP with index 0 still
retains FIRST/CD 2. OTHER.MAP with index 1 retains SECOND/CD 1. A null or empty
filename instead uses the selected record's Scenario. The actual index write at
`683B0A` and CD lookup at `683B86` are required execution points.

This matches the external call shape in official
[yrpp-spawner Spawner.cpp](https://github.com/CnCNet/yrpp-spawner/blob/de64f070815b5c9d0fdd13e721072da5d6a87933/src/Spawner/Spawner.cpp#L304):
its campaign route passes an explicit filename, action flag 1 and index 0.
The [client selector](https://github.com/CnCNet/xna-cncnet-client/blob/3a5c4c6acd641dabc4c74be0367fc0e08daf3ec9/DXMainClient/DXGUI/Campaign/CampaignSelector.cs#L416-L419)
writes its own selected Scenario; its catalog identity is not an engine record
index. These pinned sources are a launch adapter contract, not evidence about
any installed legacy mod's spawner DLL.

## Substitutions and limits

Existing source-order INI cache storage, allocator, CRT TLS/OS atomic imports and
clock transport are reused. DebugOnly descriptions avoid physical CSF lookup.
Only diagnostic `4068E0` returns a supplied zero with cdecl caller cleanup; its
return is unused. Original `683B65` supplies media -1 and original `4790B0`
executes before the selected-record CD boundary. Execution stops BEFORE the
selected CD operation at `683BC9`; it does not claim successful file admission,
map loading, movie playback or complete StartScenario execution.

Native-seeded Scenario/Main/MapGen RNG storage stays unchanged. Constructor clock
0 calls and prefix clock calls are recorded separately; the prefix makes none.
Live timers, entropy sampling and detach/lifecycle calls after this prefix are
outside coverage. FinalMovie is observed retained record state, not an executed
end-of-mission consumer. Invalid indexes and absent records are not executed;
VERA rejects an absent first record before reading a seed rather than claiming
unchecked native pointers provide a valid fallback.

`cncnet_campaign_spawn_native_start_prefix_keeps_filename_index_and_cd_independent`
consumes these native outputs through the one Rust startup constructor. Other
focused adapter controls preserve the first record through explicit loose-file
startup, restart and continuation, retain named menu selection, reject an empty
catalog, and keep compile-time Side admission. This does not certify every
campaign, mod or downstream trigger.
