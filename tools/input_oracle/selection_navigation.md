# Keyboard selection navigation

Run from the checkout with the supported retail executable selected:

```sh
VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_COMBATANT_INPUTS=/path/to/physical/selection/layers \
VERA20K_SELECTION_SOUND_INPUTS=/path/to/physical/fixed/sound \
python -m tools.input_oracle.selection_navigation --check
```

Explicit `--write` regenerates the JSON and identity/coverage sidecar. The shared
native runner verifies executable identity, checked return boundaries and unchanged
original code. No executable is distributed. Inputs are initialized memory, not a
native scenario load or whole constructor/lifecycle proof.

The physical selection directory contains `RULESMD.INI`, `MPBATTLEMD.INI` and
`XMP03T4.MAP`, plus `LANGRULE.INI` when present. Filename case may follow the
existing extraction tool. Extract these bytes through the production asset owner;
the fixed sound directory contains `SOUNDMD.INI` with its native filename.
The appended corpus records their SHA-256 identities. The existing search,
N/M command, voice, P and type-reader groups retain their inputs and outputs;
cleanup and Y/Health/initialization controls are separate families. New
navigation inputs add explicit raw veterancy and cost fields; historical actor
inputs and every historical family retain their observations.

The fixture reuses `KeyboardFixture` to execute ordinary command registration,
the input oracle's scalar/sink helpers, `NativeCallTrace` for nested return
observation, and `gen_rng_vectors` for original seeding and raw-draw continuation.
There is no Python comparator, candidate scan, selection mutation or RNG algorithm.

## Native route

Registered NextObject/PreviousObject execute536610/536A80. They disable power,
planning, repair and sell in that order; capture the first selected pointer; call
4AA2B0/4AA380; and, when a candidate exists, call UnselectAll48DC90, candidate
Select+14C, CenterOnSelection4AE290 and Redraw4F42F0(1). The Select result is ignored.

Search reads the actual820030 table `[2,3,4,4,3,2]`: Ground/Air/Top forwards for
Next and Top/Air/Ground backwards for Previous. It preserves each layer's stored
order. A candidate passes actual6F32D0 and6FC030 before it can count as the anchor.
An absent or ineligible anchor wraps to the first eligible member. Nulls and the
other two display layers do not participate.

Real Unit7F5C70 and Infantry7EB058 vtables dispatch those gates. The primary gate
checks positive health, non-limbo, in-playfield, non-Building, discovered-current,
Selectable, and House50B6F0. Dynamic selection rejects slave, robot-offline,
bunker and dock-on-building state, then reaches Foot4DFA50/Object5F6C30. Foot+6AD
is the **locomotor-swap** flag; it must not be confused with virtual+1D4/+270
(warp refusal) checked later by ObjectSelect5F4520. The supplied disguise+1D8 is
zero; these controls do not establish disguised-object visibility.

ObjectDeselect5F44A0 clears native follow when deselecting the followed object.
Singleton wrap therefore clears follow despite selecting the same identity again.
No candidate leaves selection/follow/camera unchanged after mode cancellation.
A found candidate refused by pending placement clears selection/follow, preserves
placement and armed modes, does not forward a camera coordinate and still redraws.
Mission-only+3D4 and virtual+1D4 refusal controls also distinguish search admission
from final Select admission. Ordinary Unit+6D8 is explicitly supplied as-1;
zero-filled Unit memory would incorrectly report a control blocker in746C90 and
hide the mission-only refusal.

## Corpus and execution boundaries

- `search_cases`: explicit actors, five layer vectors, direction, anchor and
  observed candidate/ordered gate visits. Includes building exclusion, dock cell
  queries, campaign human/control ownership and the separate Alive bit control.
- `command_histories`: original consecutive Execute calls retain the native
  selection array. Each step records selection/selected bits, follow, modes,
  placement, type-selection mode, calls, forwarded coordinate, redraw, queued
  voices and raw draws. This is the native reference for Rust's pending selection
  integration; no Rust scheduling assumption is supplied to the fixture.
- `voice_histories`: original708EB0/708D90 over empty/single/multiple numeric
  lists, seeds0/1/1234, both ordinary classes, special list branches and queue
  rejection controls. Each history retains full RNG bytes and the next four
  original Main draws. Sound IDs are fixture-relative, not stock registry IDs.
  The appended `unit_multiple_seed1_after248_main_raw` first executes248 raw
  draws on the same Main886B88. Its `main_prefix` records those draws and full
  before/after state; `rng_before_hex.main` is that advanced state. Four voices
  then cross the250-word cursor wrap before `main_next_four` records the next
  original raw continuation. The prior28 histories retain their fields/results.

The corpus contains94 searches,20 command histories/31 calls and29 voice
histories/112 calls, plus the248-call Main prefix. The prefix supplies prior
consumer activity; it does not execute terrain loading, MoveSound, Gattling or
death. It distinguishes a retained shared Main cursor from a freshly seeded
audio-only cursor without introducing a fixture RNG algorithm.

Mouse cursor entries5BDA80/5BDAA0, final Tactical camera application6D6070 and
Redraw4F42F0 are observed presentation boundaries. All search, candidate, selection,
follow cleanup, mode bodies and camera selection reduction execute unchanged.
The planning cursor backup is empty. Tags are null, planning-group propagation is
off, and selected-control-group metadata is empty. No graphical window, bandbox
geometry, Fly movement, display membership producer, authored Selected trigger,
save stream, audio playback or whole-game parity is claimed.

## Voice draw and admission distinction

For any nonempty selected voice list, VoiceSelect708EB0 spends one raw Main
Random65C780 draw, then chooses `draw % count` and calls QueueVoice708D90. This
includes a one-item list. An empty list preserves the queued ID and RNG state.
QueueVoice may subsequently refuse a disabled voice, nonlocal owner, or-1 sound;
the draw has already occurred. Direct receiver controls pin that ordering.

The outer TechnoSelect6FBFA0 checks successful ObjectSelect, owner/HouseType
admission and global822CF2 **before** calling VoiceSelect. Thus the complete
`voice_disabled` command control spends no draw. These are different call paths.
Ordinary PickCallback4AC2B0 clears822CF2 at4AC2FB only after a successful Select;
Tactical rectangle selection similarly clears it at6DA6F5 and restores it at
6DA71D/6DA730. That caller ordering is instruction-backed, not rectangle execution
in this fixture. A first successful object with an empty voice list still closes
the ordinary first-success voice allowance.

The measured selection path does not change Scenario or MapGen RNG. Main886B88
is one shared native stream, including callers outside audio; retaining its
exact fixture state does not establish whole-engine determinism. No action-line
timer start or Detach is traversed in this
bounded untagged path. Arbitrary Tag actions and later audio/renderer consumers
remain separate behavior.

## CombatantSelect P and retained selection modes

`combatant_histories` executes the originally registered CombatantSelect
`5367F0`, TypeSelect `5368B0` (press and release), and HealthNav `536950`.
P and Health pass `!(key >> 8) & 1`; ordinary P clears selection, Shift+P adds.
TypeSelect uses its actual key-edge/tap receiver, with a supplied 100 ms
`timeGetTime` interval. No command, filter, selection, comparator or RNG algorithm
is implemented in Python.

P `732280` first rejects the nonzero command guard `A8B538`, then invokes
`6DA770` to drop a lone nonowned selection. Outside campaign, owner admission is
exact current-house identity through `50B6F0`; campaign uses owner `+1ED` directly.
Human `+1EC` alone does not pass that campaign branch. `Deselect5F44A0` clears
follow where appropriate and retains the current selection mode; dropping a
lone nonlocal selection therefore retains an existing P map scope.
The `shift_P_lone_nonlocal_campaign0_human0` and
`shift_P_lone_nonlocal_campaign1_human1` histories start with mode1/maptrue,
selected/follow20 and Shift held. Both remove the nonlocal20, clear follow,
retain map scope and end with only local40 selected. Shift's retained selection
policy therefore still requires this explicit deselection to reach committed
membership; the campaign control has human1EC true and player-control1ED false.

The source gate `732580` tests pointer, Alive `+90` and that owner admission.
It does not use the stricter N/M health/limbo/in-playfield/discovered gate
`6F32D0`. The supplied undisguised controls distinguish those inputs. P screen
collection executes `6DC420/6DC430` on initialized Tactical selectable records,
then `7342C0` checks only nonnull and abstract flag `+14 & 1`. Map collection
walks the supplied native Techno vector in stored order and applies `732580`.

Escalation checks the collected screen candidates for non-Building, type `+DBC`,
dynamic virtual `+13C`, and not already selected. With none, or all already
selected, it expands to the map **in the same call**, before clearing selection.
P mode `1` with map byte `B0FE64` already true starts directly from the map.
Final predicate `7325C0` repeats class/type/dynamic admission, then calls original
Select `+14C`, whose additional refusals remain authoritative. Thus limbo,
mission-only, warp and pending placement can produce NothingSelected while P
still has screen scope; the preflight did find an unselected dynamic candidate.

ObjectSelect `5F4520` prepends positive-Primary type `+C9C` objects and appends
others. Every history records source order, Select/voice call order and resulting
CurrentObjects order independently. P voices every eligible newly selected
object while the global voice allowance stays enabled. Replacement reselects
and voices again; Shift's already-selected refusals spend no draw. Empty and
minus-one voice controls preserve the native distinction between list draw and
queue admission. Every step replays the observed Main draws through the original
RNG owner, checks all Scenario/MapGen bytes, and preserves full history boundary
states plus the next four original Main draws.

P finishes with mode `1`, T with mode `2`, and Health with mode `3` when its
snapshot is nonempty or mode `0` when empty. These modes share **one** `B0FE64`
scope byte. Original Health leaves that byte unchanged; a later P or T restarts
scope according to its mode check. The `T_map_P_restarts_screen` history exposes
T map selection followed by P screen selection. Direct ordinary TechnoSelect
and UnselectAll witnesses execute actual mode reset, retain the map byte, then
show the next P restarting the screen preflight. They do not execute a mouse
gesture or a full native scenario replacement.
`ordinary_deselect_retains_P_map_scope` separately executes Deselect5F44A0 on
local20 and then P. Deselect clears membership/follow but preserves mode1/maptrue,
so the following P selects the map candidates. This direct boundary distinguishes
Deselect from successful Select and UnselectAll mode reset. The older output field
`selection_across_map` remains its historical `B0FE58` submode observation;
the new `across_map` and `submode` fields separately name `B0FE64/B0FE58`.

P preserves armed repair/sell/power/planning state, makes no camera/redraw request,
and does not start the action-line timer or enter observed world/abstract Detach.
Timer bytes and writes are observed for active and inactive timers. Follow
clears through actual Deselect during replacement and remains for retained
Shift selection, except the lone-nonowned cleanup described above. MessageList
is a declared UI sink after original copying or
formatting; the fixture supplies actual strings from physical
`langmd.mix/ra2md.csf` through the existing CSF reader and records the original
seven message arguments. It does not execute rendering, device playback or
message timer scheduling.

The fixture supplies slave/offline/bunker/dock/locomotor-swap bits to execute
refusal controls. Their full lifecycle, robot power/voice transitions, Tag
Selected actions, disguise, display membership production, world replacement,
saved streams and held mouse/movement/Fly paths remain outside this corpus.

## Cleanup retention and local keyboard ordering

`combatant_cleanup_histories` contains six completed histories, fifteen semantic
steps, six registered P executions and eight direct cleanup boundaries. Each row
uses the existing flattened actor/layer/house/seed inputs plus `screen_order`,
`map_order`, `selected`, `follow`, `selection_mode`, `across_map`, `submode`,
`retained_navigation`, `action_timer`, `setup_cleanup` and `cleanup_world_size`.
Every row starts selected/follow20, mode1/maptrue/submode1, screen40 and map
order50/40/20; all three streams use original seed1. Actor inputs explicitly
declare unmarked state and native display layer2. Only40 is onscreen, so50
distinguishes retained map scope from a fresh screen preflight.

| History | Original cleanup before P | Following P order |
| --- | --- | --- |
| `direct_deselect_then_P` | Deselect5F44A0 | [20,40,50] |
| `alivefalse_deselect_expiry_then_P` | Alivefalse20; Deselect then733160 | [40,50] |
| `absent_registry_after_deselect_expiry_then_P` | Same leaves; explicit registry omission20 | [40,50] |
| `initialized_object_detach_all_true_then_P` | WholeObjectDetach5F5280(true) | [20,40,50] |
| `initialized_alivefalse_object_detach_all_then_P` | Alivefalse20; wholeObjectDetach(true) | [40,50] |
| `initialized_object_conceal_then_P` | WholeObjectConceal5F4D30 | [40,50] |

The existing `combatant_history` executor handles `ordinary_deselect`,
`pointer_expiry`, `object_detach_all`, `object_conceal`, `supply_selection_sources`
and the registered `combatant` command. Registry changes name their new layer,
screen and map inputs explicitly; mapped retired storage remains available.
Original732050 constructs the retained selected snapshot before its pointer is
assigned toB0FE6C as an earlier Health/Y prestate surviving while P owns mode1.
This does not execute VeterancyNav or introduce another selection owner.

Cleanup steps record both the existing selection snapshot and `actor_state`,
`retained_navigation`, actual layer/screen/map order, full `native_calls` with
mapped actor IDs, `scope_writes`, all three full RNG states and the next four
original Main draws. Existing `detach_calls` now includes actual Object/Foot
Detach entries. The previously unreached7258D0 trace declaration is corrected
to its actual register-based arguments and zero stack purge. It executes its
original body; no native return is substituted.

Deselect removes CurrentObjects membership and clears matching Follow, retaining
mode/scope. Direct733160 removes a matching retained pointer, retaining mode/scope.
WholeObjectDetach(true) reaches Deselect, AnnounceExpiredPointer7258D0, Bomb439150,
Kamikaze54E590,733160, Tactical6DA560 and Logic55B880. WholeConceal reaches
FootDetach4D9720, empty-slot Radio65ACB0, ObjectDetach and actual DisplayRemove4A9770;
its original tail sets Limbo true and display layer-1 while Alive remains true.
Type+234 is false, so this control does not enter Conceal's LogicRemove branch.

Every completed cleanup boundary leaves mode1/maptrue, makes zero writes to
B0FE54..B0FE67, spends zero draws from every RNG stream and preserves all action
timer bytes. No70D4A0 world-detach is reached. Subsequent P includes offscreen50
and emits physical-CSF "Selected across MAP". Two additions draw Main
2025287381/660436142; three also draw3663968561. Scenario/MapGen remain unchanged.
Successful P still traverses its actual UnselectAll/Select mode reset;731D00
writes modeB0FE54 and submodeB0FE58, leaving map byteB0FE64 unchanged.

Whole-producer setup reuses `ifv_impact.initialize_effect_world` for the original
40B540..40B5AB,725850..725886 and4E6D60..4E6D96 startup regions, with explicit
64x64 dimensions and seed1. Original4A8630..4A8672 initializes display vectors;
65A758..65A798 runs the Radio constructor's vector allocation/empty-slot region
over supplied actors. These regions stop before CRT atexit registration or
unrelated base-constructor state. Tactical's original6D1D54 vptr literal7F4348
is supplied and its actual+28 expiry body executes. Services/listeners,
Bomb/Kamikaze/Temporal/Team, linked radio contacts, Tags and spawner state are empty.

These are initialized unmarked Unit controls. Alivefalse is an input, rather
than an executed death writer; absence is supplied post-retirement registry
membership. No fatal AI, marked-cell removal, whole concrete-class Limbo/UnInit,
destructor/free/deferred-drain,
nonempty ancillary cleanup or scenario load is demonstrated. Rust's pending
selection bookkeeping has no corresponding native flag. Its membership and
scope reconciliation must be tested separately against these native boundaries.

Original local keyboard ordering supplies another boundary: MainTick55D360 calls
GScreenInput4F4320 at55D8AB, then ProcessCommand55DEE0 at55D8B4, before
Logic55AFB0 at55DC9E. ProcessCommand's Execute+20 call at55E015 reaches registered
P5367F0, whose5367FC call enters732280 synchronously. P's Select+14C call at7324A4
reaches6FBFA0/5F4520; ObjectSelect commits selected+83 at5F46AE/5F4718 before return.
Thus a later live Logic cleanup occurs after the actual local selection command;
there is no delayed second Select in this native Execute-to-Select chain.
This is instruction/caller/data evidence, not executed full fatal-frame parity
or an assertion about EventClass's separate dispatch phase.

Original executable bytes for Infantry vtable7EB058 pin +DC/+14C/+150 to
4D9720/6FBFA0/5F44A0, and registered P's Execute slot7EB9AC to5367F0. These source
identities and ordering addresses use the sidecar's original gamemd.exe identity;
Ghidra reads explicitly select `gamemd.exe`. No Ghidra annotation is the source of
the expected selection/RNG values: those come from the unchanged original bodies.

## IsSelectableCombatant type inputs

`combatant_type_histories` composes the existing BulletReader/type/INI owners.
Full InfantryType `5236A0`, UnitType `7470D0`, BuildingType `45DD90` and base
TechnoType `710AF0` constructors overwrite an `A5`-poisoned allocation. The
observed field writer `71164B` sets `+DBC` to false for each selected type.

For every physical layer, original AbstractType section admission `410A60` runs.
An admitted section then executes the unchanged field block
`71574E..71576F`: current-field default, type-name accessor `524EC0`, exact
literal `IsSelectableCombatant` at `843414`, ReadBool `5295F0`, and final store.
The physical RULESMD/LANGRULE/mode/map history and explicit missing-section,
missing-key, wrong-case, empty, malformed, numeric and retained-value histories
pin native outputs. There is no clamp or independent reset between retained
passes. Physical E1/E2/MTNK/ROBO become true; ENGINEER/HARV/CMIN/GACNST remain
false for the selected stock layers.

These controls execute whole constructors and the original selected field
reader. They do not execute unrelated intervening type reads, full Rules
scenario processing, physical INI file loading, asset binding or actor
constructors. All original `.text` bytes are checked after the type history.

## VeterancyNav Y and shared Health snapshot

Registered VeterancyNav Execute5369F0 calls7336C0 at5369FC with
`!(key >> 8) & 1`. Fresh mode other than4 deletes the prior shared snapshot,
constructs a selected Techno snapshot through732050, or a screen snapshot through
731F70/7335F0 when no selected Techno exists. It clears selection even for Shift
and starts category0 (elite). Continuing mode4 preserves that snapshot and cycles
elite0, veteran1, rookie2; ordinary Y clears selection and Shift+Y retains it.
There is no map expansion.

The screen source gate requires Alive+90, native local ownership, and non-Building.
Campaign admission uses owner+1ED directly; human+1EC alone is insufficient.
Selected-source732050 only filters null/non-Techno pointers. Its retained
members can therefore include an other-house dead Building, and continuing
navigation does not reapply the screen gate. Original750030 rechecks raw+150 on
every category scan. Actual Select+14C remains authoritative for placement,
limbo, mission-only, warp and other final refusals.

Health mode3 and Y mode4 use **one** B0FE6C vector, with separate signed category
globals845560/845564. Original file bytes initialize both to-1; forged continuing
mode with-1 starts category0. A fresh Health or Y replaces the same vector.
Nonempty retained state keeps the command's mode even when final selection is
empty. Empty state ends mode0. Both commands leave the P/T map byte B0FE64
unchanged; their final B0FE58 submode is zero. A nonzero A8B538 guard preserves
selection, follow, vector, categories, mode, map byte, timer and RNG.

Actual final membership drives category/mixed feedback and pricing through
731D90 and the original cost virtuals. Snapshot traversal order and final
CurrentObjects order are separate: ObjectSelect prepends positive-Primary
objects, so these inputs often finish in reverse traversal order. Pricing
does not sort the retained vector. Every eligible successful new selection
spends the actual VoiceSelect Main draw; already-selected or refused admissions
spend none. Original CRT copying/formatting executes, using the existing
infantry_deploy_action single-thread Interlocked OS transport. Physical CSF
strings and final MessageList transport have the same declared boundaries as P.

The new families contain38 Y histories/73 semantic steps,14 Health histories/24
steps and13 cleanup histories/41 steps. They make108 registered Execute calls
(including both T edges),24 direct native boundaries and7 supplied state changes.
Every new row uses the existing flat actor/layer/house/seed/screen/map/selected
history schema. Actors explicitly carry `veterancy_raw_bits` and `cost`;
`cost_factors_bits` supplies all-one country and plant slots. Each step has
`before`, `retained_navigation`, signed `health_category/veterancy_category`,
`actor_state`, layer/screen/map order, scope/category/action-timer writes,
native/rank calls, all three full RNG states and original Main next4. Commands
`supply_veterancy_raw_bits`, `supply_actor_state`, `supply_command_guard` and
`supply_selection_sources` explicitly name input seams, rather than native
rank/death/guard/registry producers.

Deselect and Alive=false alone retain the snapshot. Original733160 scans from
the end and stable-erases **one** exact pointer, retaining mode/map/categories.
No-match and repeated expiry are inert. The duplicate-pointer control supplies
an earlier vector through actual732050 and removes its last equal entry first;
it does not prove a normal CurrentObjects duplicate producer. Initialized whole
Detach/Conceal histories reuse the earlier empty-service/startup/Radio bounds.
They demonstrate Alive-preserving expiry and shared Health/Y cleanup, without
executing fatal death, destructors or nonempty ancillary cleanup.

Actual7258D0 traverses represented listeners, Bomb439150, Kamikaze54E590, Houses,
navigation733160, Tactical6DA560 and Logic55B880. Its navigation call is gated
by the supplied actor's Techno abstract bit, not the announce-control byte.
The historical P trace label `temporal_pointer_expiry` at54E590 is retained
byte-for-byte in old observations; the new opt-in label correctly names
Kamikaze. Mode reset731D00 and subsequent fresh navigation are executed, but
whole scenario replacement/persistence and non-Techno cleanup are not.

Ordinary world replacement has a separate instruction-level caller chain:
ClearScene6853D8..685408 invokes scalar destructors; Unit735892,
Infantry517E75 and Building43BD67 dispatch7258D0, whose7259B1 calls733160.
Fresh Full_Init and saved LoadContent both reach this cleanup. Unique outgoing
candidates therefore retire before incoming objects reuse their stable IDs.
The `all_pointers_expired_empty_snapshot` history executes the resulting
empty-source boundary: its first three expiry steps preserve mode4, map scope
and both last categories; only the following Y changes them. This combines
original caller reading with executed cleanup leaves, not whole native loading.

World replacement preserves categories845560/845564, modeB0FE54 and scopeB0FE64.
Saved CurrentObjects are restored by raw pointer-vector read67FA2A..67FAEB,
without Select's mode reset. Type heldB0FE65 and clockB0FE68 likewise have only
the ordinary press732CA9/732CB6 and release732CC9 writers; load invents no key
edge. Double-tap reset734210 is separate and changes only its two click-clock
pairs. Physical focus/key histories and arbitrary destructor listeners are
outside this bounded comparison.

## Raw rank and map percent prerequisites

`veterancy_rank_cases` executes750030 on16 raw binary32 boundary, sign,
infinity, NaN and subnormal inputs. The outputs are native return values;
there is no Python rank classifier.

`map_veterancy_seed_cases` has90 flat rows,30 per Unit/Infantry/Aircraft caller.
The original token-present/NULL slices are743495..7434AE,51FD54..51FD6D and
41B308..41B321. Unit and Aircraft consume map field8; Infantry consumes field9.
The supplied pointer begins after strtok. Empty string is a helper boundary
control because original strtok does not emit empty tokens. NULL cases start
at raw0, raw1.0 or an explicitly supplied1/3; only raw0 represents the ordinary
constructor comparison. The Aircraft slice uses shared Techno+150 storage and
executes no Aircraft constructor, virtual, Unlimbo or flight behavior.

Each present token executes original CRT atoi7C9BFD and7500E0. Atoi reads a
signed decimal prefix, returns0 for a malformed prefix, and wraps32-bit
overflow;2147483648 becomes-2147483648 and4294967295 becomes-1. Setter7500E0
loads signed i32 with FILD, multiplies by the original binary64 literal
7B14AE47E17A843F at7E3808, then stores binary32 with FSTP under FPCW0E7F
(53-bit precision/chop). It neither clamps nor quantizes rank:1 and2 remain
rookie,100 stores1.0,150 stores1.5,200 stores2.0,250 stores2.5 and-1 stores a
negative raw value. Every row records parsed native percent, exact raw bits,
actual750030 rank, full three RNG states/next4 and timer observations.

The rank writes precede actual map caller Unlimbo+D8 at7435D0 (Unit),51FE52
(Infantry) and41B39B (Aircraft). Thus the raw value belongs before admission
and downstream cloak/sensor/mission consumers. Whole map tokenization, class
construction/Unlimbo and SpecialFlags.InitialVeteran processing remain outside
these caller-slice comparisons.

`veterancy_initialization_cases` has18 reproducible rows. Full5113F0 on
A5-poisoned Americans/YuriCountry storage creates empty VeteranInfantry+14C,
VeteranUnits+168 and VeteranAircraft+184 vectors. Original Techno caller
6F2BDC..6F2BED invokes74FF30 and initializes raw+150 to0. Original House
constructor XOR4F54B2 and writers4F5855..4F5873 initialize Barracks+2BF and
WarFactory+2C0 infiltration false. This executes the named regions, not a whole
House constructor.

Eleven Unit735604..735678/Infantry517CE7..517D51 controls execute actual
constructor rank gates. A matching country list sets veteran1.0 independently
of Trainable. Infiltration requires Trainable+C8E, and Unit additionally
requires non-Naval+CCE. Fresh flags and empty constructor lists retain raw0.
The original current-default country list reader511D16..51208C executes its
exact-case VeteranInfantry/Units/Aircraft reads on the physical RULESMD,
optional LANGRULE, MPBATTLEMD and XMP03T4 layers for Americans/YuriCountry.
The native runtime empty default889F64 and missing reads retain empty lists.
All six present passes preserve three RNG streams and timer bytes.

These initialization results bound ordinary fresh retail preplaced E1/MTNK.
Authored country lists, spy infiltration lifecycle, whole scenario chronology,
full fatal cleanup, graphics/audio playback, other classes and whole-engine
determinism remain separate required mechanisms.

## Enslaved selection voice prerequisite

`enslaved_voice_histories` adds28 histories/76 steps:68 registered Y calls,
four bounded freed-slave writer regions and four original mode resets.
Only these new actor inputs declare `enslaved_voice_list`; the historical six
families and the earlier Y/Health/rank/initialization observations are retained.
Normal `voice_list` supplies type+414; the additive list supplies type+430.
The original actor's nonnull SlaveOwner+2DC chooses the latter at708EB5..708EFD.
Its empty count+440 returns at708FAD without falling back to normal or spending
Main. A nonempty list spends one original65C780 Main draw, including singleton
and deliberately supplied-1 numeric slots. Scenario and MapGen remain unchanged.
No Python voice choice or RNG algorithm supplies the expected result.

These rows record attempted `voice_requests` separately from
`accepted_voice_requests`. Acceptance comes from observed original708DB5
writes to actor+4F0, with `voice_writes` recording their identity, sound ID and
instruction. Each before/after actor state includes `slave_owner_present` and
the signed `queued_voice`. The outer Select caller rejects disabled voices,
unadmitted ownership and final-selection failures before VoiceSelect. A
MultiplayPassive+1A6 nonhuman-house control reaches VoiceSelect and spends its
draw, then QueueVoice rejects it without writing+4F0. `house.other_passive` is
an explicit new fixture input. Fresh/continuing Shift, placement, guard, repeated
category cycles, seeds0/1/1234 and mixed ordinary/enslaved actor order are executed.

Command `free_slave_owner_writer` runs the original outside-Limbo FreeSlaves
region6B0B6D..6B0B98, including the actual+2DC zero write at6B0B73. It stops
before owner transfer, mission reset and cheer. Subsequent original731D00 and
registered Y reach the normal list on the same Main continuation. This is an
executed writer boundary, not a whole SlaveManager FreeSlaves/death proof.
The existing slave-link lifecycle owns the production field. Offline Robot
selection voice and Robot lifecycle remain held.

`enslaved_voice_type_histories` composes the existing BulletReader, source-order
Sound cache/registry and NativeCallTrace owners. Four full selected type
constructors (E1, MTNK, SLAV and generic VOICE_CONTROL) overwrite A5-poisoned
normal/enslaved vectors with empty state. TechnoType710D87..710DBD invokes the
vector constructor and sets enslaved count0, increment10 and vptr7E4DD8.
The generic constructor has explicit SpeedType0 as its second argument;
original Infantry/Unit wrappers supply their own argument.

The admitted original reader blocks712B1D..712BF1 execute normal `VoiceSelect`
first, then exact-case `VoiceSelectEnslaved` at8442A0 through712B87..712BF1.
The latter precedes the held offline list reader. Both invoke ReadSoundList
525430 with a copied current-vector default, ReadString528A10 capacity128
(127 payload bytes), comma-only strtok817F70, sound pointer751520 and
index7515C0. Missing/empty/whitespace reads retain the vector. A nonempty read
replaces it even if no name resolves; unresolved tokens are skipped,
case-insensitive resolved sound names and duplicates are retained, and spaces
inside comma-delimited tokens are not removed. Fifteen consecutive authored
passes pin those outputs, wrong-case key/section rejection, the127-byte suffix
cut and independent normal-list changes. `before/after` are enslaved bindings;
`normal_before/normal_after` separately expose normal bindings. A binding has
`count`, native `ids` and native-retained `names`.

The fixed physical SOUNDMD source is expandmd01.mix, SHA-256
`0a8e85381aef1a0f97074c953bfe99504da00c6220fae1a023a1afd857023232`.
The four selected SoundList entries retain their physical source keys/order:
GISelect67, GenAllVehicleSelect228, SlaveFreedSelect695, SlaveWorkerSelect704.
Original7510D0/750440 registration and native sound lookup execute. Returned
list IDs0..3 are fixture-relative; retained names identify the sounds.
AudioIndex sample lookup uses the existing local-index transport; full catalog,
sample decoding, audio service and playback are outside this comparison.

Three retained retail histories execute nine present RULESMD/MPBATTLEMD/
XMP03T4 layer passes; LANGRULE is absent. Their physical source hashes are
recorded and match production mode1 AnyTown source selection. E1 resolves
normal GISelect and empty enslaved; MTNK normal GenAllVehicleSelect and empty
enslaved; SLAV normal SlaveFreedSelect and enslaved SlaveWorkerSelect. Every
constructor and reader row records complete three RNG states, original Main
next4, timer writes and nested native calls. No type reader or selected voice
control traverses action-timer start or detach. Whole TechnoType readers,
scenario chronology, physical INI file loading and full slave lifecycle are
excluded explicitly.
