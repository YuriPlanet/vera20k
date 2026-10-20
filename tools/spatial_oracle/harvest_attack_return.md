# War Miner Attack return comparison

`harvest_attack_return.py` extends the existing `harvest_field` /
`refinery_dock` fixture. The 53 saved histories execute original command events,
pointer-expiry callbacks, Attack dispatch, Unit idle, Ready/Commence, selected
TechnoAI and UnitAI scalar prefixes, and Harvest states 0/1. Expectations come
from `gamemd.exe`, independently of Rust.

The accepted executable SHA-256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The validation receipt (`tools/spatial_oracle/harvest_attack_return.validation.json`, in git at 1b1a8b43e) binds Rust replay,
retail integration, source identity and the ordinary release observation.
Each history checks that the original image instruction region and Unit vtable
remain unchanged. The `.meta.json` pins Unicorn, entry points, assumptions,
substitutions, and the canonical payload hash. This is a bounded
native comparison, not a whole UnitAI, combat, scenario-loader, or match result.

## Reproduce

Use Python with the dependencies in `tools/requirements-test.txt` and the retail
YR installation. Extract the selected physical layers with the existing `asset`
tool; its `extract` command puts files beneath the output's `extract/` directory:

```sh
export RA2_DIR='/path/to/Command and Conquer Red Alert II'
asset extract RULESMD.INI --out target/asset/harvest-attack-return
asset extract MPBattleMD.ini --out target/asset/harvest-attack-return
asset extract XMP03T4.MAP --out target/asset/harvest-attack-return
# Extract LANGRULE.INI too if this installation contains that layer.
python -m tools.spatial_oracle.harvest_attack_return --check
```

`VERA20K_HARVEST_ATTACK_INPUTS` overrides
`target/asset/harvest-attack-return/extract`. The checked physical inputs are:

| Layer | Physical container | SHA-256 |
| --- | --- | --- |
| `RULESMD.INI` | `expandmd01.mix` | `3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef` |
| `LANGRULE.INI` | Absent in the checked installation | — |
| `MPBattleMD.ini` | `ra2md.mix` / `localmd.mix` | `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963` |
| `XMP03T4.MAP` | `multimd.mix` | `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e` |

The map supplies a final INI layer. Its terrain is not loaded into this fixture;
the ore VM retains the owner's declared 32×32 map. An installation with different
layer bytes will fail the saved reference comparison. `--write` deliberately
replaces both outputs and must be reviewed. `--help` does not execute readers or
require extracted inputs.

## Executed original boundaries

| Boundary | Original entry / caller | Saved observation |
| --- | --- | --- |
| Attack/Move event | Constructor `4C6860`, execute `4C6CB0`, Foot command setup `4DF0E0` | Event bytes, resolved actor/target/destination, setters and queued selectors |
| Stop event | Constructor `4C65E0`, execute `4C6CB0` | Actual event type 6; destination clears before target; current Harvest promotes Guard immediately |
| Mission Attack | Dispatcher `5B3060`, call at `5B3176`, Unit vtable `7F5C70+210 → 7447A0`, tail jump to `4D4DC0` | Current and queued mission, return delay, epilogue timer words, raw/ranged Scenario draws |
| Target expiry | Unit `7446E0 → Foot 4D9960 → Techno 7077C0 → Radio 65AAC0 → Object 5F5230` | Pointer clear/retention, target timer restart, optional ranged draw, setter ordering |
| Unit idle | Unit vtable `+484 → 738970`, Foot idle `4D82B0` | Human land decision, radio/NavCom/current/queued/saved-command gates, destination/target clear and queue |
| Linked-contact BREAK | Unit vtable `7F5C70+274 → 65ACB0`, radio `65A970` | Sender/receiver contact-slot clears, surviving NavCom and subsequent idle selection from Enter |
| Promotion | Unit Ready `744270`, conditional Commence `5B3570` | Ready/Commence return byte and committed selector/timer changes |
| Stage update | TechnoAI `6FABB8..6FAC31`, after dispatcher call at `6FA655` | Original RTTI, signed due timer, stage increment/changed flag, native timer restart |
| Harvesting latch | UnitAI `7365BB..7365DF`, before Fire at `7365E1` | Original alive/effective-mission gate and `Unit+6D2` clear |
| Harvest | Unit `73E5E0`, ore tick `73D450` | Harvest start, stage-9 ore consumption, cargo/cell mutation and non-harvester hold |

The live-target Attack call reaches its unchanged concrete `UnitApproach7414E0`
receiver, then stops **before** that body. A declared `RET4` outside the image
returns EAX 0 without target mutation, and the original caller resumes through its
cadence and mission epilogue. This seam establishes caller dispatch and cadence;
it does not establish Approach, path/range admission, a shot, or damage.

The `expiry` operation supplies the victim's health/alive change when `dead=true`
and executes the complete receiver callback hierarchy. It does not execute the
weapon/damage/destructor producer or whole-world `DetachAll` traversal. The corpus
records reached linked/world detach entries; no unexecuted detach call is inferred.

Stop's constructor and execute body run. The UI selection/ClickedEvent producers,
event queue insertion, network delivery and normal Logic scheduling are excluded.
The native token format is the body-established `Cell X + 1000*Y`, kind `0x0B`
(`6E6AB0`, resolved by `6E6E20`), with supplied sorted object-ID registry entries
101/202/303 and a House array. The original resolver and RTTI execute.

## Native retail readers

Physical strings enter the existing `Landing` owner's native CRC/index cache.
Original UnitType `7470D0`, Rules `665650`, MissionControl static construction
`4E7CF0`, the following selected reader blocks, and the complete MissionControl
table read loop `679C92..679CAF` execute in a separate VM. The saved receipts
include lexical source lines, pre/post values, reader arguments and field writes
for every layer in order.

| Exact key / section | Original reader block | Native constructor default | After checked layers |
| --- | --- | --- | --- |
| `[HARV] Harvester`, `Weeder` | `74769F..7476D3`, ReadBool `5295F0` | false / false | true / false |
| `[HARV] Storage` | `713129..713143`, ReadInt `5276D0` | 0 | 40 |
| `[HARV] MovementZone` | `71605E..716090`, converter `474E40` | 0 | Crusher, ordinal 1 |
| `[HARV] Dock` | `713171..713264`, ReadString `528A10`, strtok `7C9CC2`, BuildingType factory `4653C0`, list assignment `67B180` | Empty list | `NAREFN`, `GAREFN` in order |
| `[General] HarvesterLoadRate` | `670CE7..670D07`, ReadInt `5276D0` | 2 | 2; key absent |
| `[General] TiberiumShortScan`, `TiberiumLongScan` | `67028C..6702CB`, range parser `474620` | 1536 / 8192 leptons | 1536 / 12288 leptons |
| `[Attack] Rate`, `[Harvest] Rate` | MissionControl `5B3760`, ReadDouble `5283D0` | Binary64 `.016`, bits `3f90624dd2f1a9fc` | `0.01600000075995922`, the native parser's float widened to double |

The selected blocks preserve adjacent prior read results when their leading
instructions also store them. The Dock reader executes an original local vector
constructor and the actual native BuildingType factories over supplied empty
registries, yielding fixture ordinals 0/1. Full physical file loading, global type
population, other type keys/weapons, and whole Rules Process are excluded.

Final selected scalar fields and six MissionControl entries transfer to each ore
VM. `harvest_field` supplies one Dock type, ordinal 0, and its House owned-count
1. That bounded prestate satisfies original Harvest's Dock/owned-count predicate;
it corresponds to the first native Dock entry but does not establish complete
refinery type/House count initialization or lifecycle. Existing map, refinery,
ore/gem, cargo, queue, reachability and presentation boundaries remain documented
in `harvest_field` and `refinery_dock`. Their existing saved payloads are preserved.
The original mission-name table `816CAC` identifies Sleep as ordinal 0 and Stop
as ordinal 13. Both are named correctly in the six-entry receipt; the original
Stop-event controls execute event type 6 independently of these names.

## Outcomes and schema

The six Attack/expiry histories cross ore/clear land with cargo 0/12/40. They
execute an Attack command, promotion, a live-target caller visit, pointer expiry,
a visit before the deadline, the due targetless visit, latch cleanup and promotion.
On ore all three cargo states queue Harvest; clear land queues Guard. Empty and
partial cargo then run Harvest/Stage in original order through one ore tick.

In the seed-1 histories, the live Attack at frame 200 returns 15. Expiry does not
reset that dispatch deadline. At frame 215 the targetless Attack queues the idle
choice and returns 16. `GetMissionTimerEntry5B3A00` reads **current** `Mission+AC`,
so that visit still uses Attack Rate even though Harvest is queued. Commence
promotes the selector and starts a zero-duration dispatch timer. Harvest begins at
215, Stage 9 is published after the dispatch at 233, and the next Harvest visit at
234 consumes ore: cargo 0→1 or 12→13 and cell data 5→4. Those outcomes are native
outputs, not fixture-calculated expectations.

Additional controls cover exact/before/after/paused dispatch timers, seeds 0/1/31,
current or queued Harvest, installed NavCom, radio contact, saved MegaMission,
human/AI/forced idle, dead or zero-health source, expiry timer threshold and
sensor/owner retention, Stop on Attack or Harvest, and Move/Attack retasking after
Harvest was queued. A non-harvester assigned Harvest returns 450 without a draw.
The retained-latch row clears `6D2` while current Attack has queued Harvest, before
promotion; current Harvest retains the latch.

Six `depot_break_*` controls validate the shared idle owner's prerequisite for
the `4D92D0..4D92E8` exit caller. That original caller transmits BREAK through
`+274(3)` at `4D92D4`, then calls `+484(0,1)` at `4D92E2`, with no intervening
destination clear. The fixture supplies current Enter 7, linked contacts and
the already documented cleared saved command (`4DF1A0`) / legacy planning
slot -1. The rows execute the unchanged sender, idle and Ready/Commence bodies;
they exclude full FootEnter admission, depot repair/sale and later gameplay.

BREAK returns 1. At `65A9B8` it writes zero to the miner's contact slot before
the receiver clears the refinery's slot at `65A8A0`. Both contacts become null;
NavCom survives. For both a plain non-harvester and HARV with installed NavCom
`[17,15]`, idle writes queued Move 2 at `5B3614` and Commence promotes Move at
`5B357F`. The idle visit preserves the destination and reaches neither target nor
destination setters. This selects Move before the harvester branch. Without
NavCom, HARV human/ore, AI/ore and AI/clear queue Harvest 10; human/clear queues
Guard 5. Those idle visits call the null target setter, then null destination
setter, then queue. All six histories preserve the complete Scenario RNG state.

Ten `plain_idle_*` controls cover the armed/unarmed split at Unit `738A6E`.
They supply Harvester/Weeder false, target and burst 7, HasTurrets 0,
DefaultToGuardArea 0, house IQ 0 and GuardArea IQ threshold 1. The optional slot-0
weapon pointer and DeploysInto pointer are declared fixture data. Original
`IsArmed701120`, its weapon getters and the complete Unit idle body execute;
the saved receipt records the actual EAX and the Rust comparison uses its return
byte. Weapon bodies and broader AreaGuard/transport gameplay remain excluded.

Armed Move queues Guard while retaining target `0x200c2000` and burst 7; armed
Patrol and Sleep retain those fields without queuing. Unarmed Patrol clears
target and burst through Target(NULL), then Destination(NULL,1), before the
tail suppresses the queue. Unarmed Guard performs the setters but the mission
owner rejects the same-current queue; unarmed Sleep clears and queues Guard.
AreaGuard, Wait and Unload with DeploysInto return before the setters; plain
Unload clears before the queue tail. All ten controls preserve Scenario RNG.
These are receiver controls, not full native moving/firing histories.

`schema_version=1` contains `reader_receipts` and `rows`. Each row holds declared
`input`, `before`, `after`, and ordered `steps`. Each step has its input operation,
pre/post state, raw return, original entry/return events, field writes, and inherited
callback events. State keeps the `harvest_field` keys (`miner_mission`,
`miner_queued`, `miner_status`, `miner_nav`, `storage`, `cells`, `stage`, etc.), plus
target/lifecycle fields, dispatch/targeting raw timer words and complete Scenario
RNG state. `deadline` supplies a frame from the previous native start/duration;
it does not calculate an expected dispatch result.
`break_contact` invokes the asserted original Unit `+274` receiver with BREAK 3.
Its writer records additionally name the miner/refinery contact slots and retain
their absolute addresses, PCs and instruction order.
The ten plain-Unit controls additionally save `idle_control_receipt` and retained
`burst_index` in each state. The prior 43 histories and reader receipts are
unchanged.

RandomRanged `65C7E0` inlines its raw generator. Checkpoints at `65C87E` / `65C882`
and the actual continue/accept branch record raw values, masks, candidates,
rejections and post-draw cursors. For example seed 31 needs two raw draws for one
Attack range 0..2; the active-expiry range 4..8 in seed 1 needs three raw draws.
Full 250-word RNG state is retained per step. Timer middle words `+CC`, `+184`
and Stage `+104` include native stack-local copies: they are recorded raw and
are not assigned countdown meaning. Stage `+104` gets an explicitly zero supplied
stack local; Stage increment `+110=1` is inherited prior state.

Native replay success establishes these sampled outputs. Rust comparison and
connected production command/combat validation remain separate evidence levels.


## Area Guard handoff

The additive `--area-guard` mode records `harvest_area_guard.json` through the
same `AttackReturn` fixture, readers and original command/mission owners. It
leaves the 53 Attack-return histories unchanged:

```sh
VERA20K_HARVEST_ATTACK_INPUTS=/path/to/extracted/layers \
  python -m tools.spatial_oracle.harvest_attack_return --area-guard --check
```

The ordinary G producer (`730D60`) admits a selected Harvester and emits
mission 11 with its current Cell as Target. These six controls begin at that
body-established event payload: original constructor `4C6860` and execution
`4C6CB0` clear the Target and assign the same Cell to NavCom and ArchiveTarget.
Original Ready/Commence promotes AreaGuard, and dispatcher `5B3060` selects
Unit vtable `+220 -> 744100 -> Foot4D6AA0`. The keyboard, voice queue and
OutList delivery are outside this corpus.

The Foot body's Harvester branch (`4D6D1A..4D6D68`) tests Unit RTTI 1 and
UnitType `+E0E`, then calls QueueMission(10, 0), **unconditional** Commence,
and Scenario `RandomRanged(1,10) + 1`, in that order. Its earlier bunker,
building-entry, AI navigation-list and archived allied-building branches
execute over explicitly absent inputs. The allied-building relocation excludes
Harvester types. These controls do not certify those independent mechanisms.
No AreaGuard Rate, ordinary scan, idle action or approach runs on this exit.

The existing independent native reader receipt supplies the HARV flag from the
physical layers listed above: constructor false, original ReadBool at
`7476AE` true, retained through Battle and XMP03T4. The supplied map, registered
objects, no SlaveManager and other fixture boundaries remain those of
`harvest_field`/`refinery_dock`; this is a bounded War Miner handoff comparison.

Human/AI crossed with ore/clear controls use seeds 1, 4, 5 and 8. Seed 4's range
call rejects raw words 1020323421 and 425984382, accepts 808536008, and returns
9; the handler returns 10. Seed 5 takes two raw draws and returns handler delay
11; seed 8 reaches the minimum delay 2. Every raw acceptance and full Scenario
state comes from original execution. Main and MapGen are independently seeded
by original `65C6D0` with 31 and 42, and their complete states remain unchanged.

The retained-state control first asks original UnitReady with unloading `+6D1`
set; it refuses. The already-current AreaGuard handler still calls Commence
and installs Harvest, preserving the unload latch, Target, NavCom and archive.
That prior cross-state and the unload producer are supplied, not claimed as a
production history. A final control proves no handler draw before a retained
deadline and the handoff exactly when it expires.

Commence resets the handler cursor and its timer before the dispatcher writes
the returned delay. Target, NavCom, archive, cargo and harvesting/unloading
latches are otherwise retained, and no detach call is observed. The corpus
stops at this handoff; subsequent Harvest, driving, ore collection and refinery
return belong to their existing owners and production-frame integration tests.
