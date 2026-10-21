# Native refinery docking comparison

`refinery_dock.py` retains the original 107 rows, 57 continuation controls and
three War Miner histories. Separate additions contain nine actual-slot admission
controls and four Building contact-constructor controls.
Four further unload controls distinguish native slots 0/2 and 2/3, including
Vinifera and Aboreus alone. Each starts at a declared payout gate with integer
amounts; it executes the original slot selection, removal and payment. The
paired Rust retail test harvests Vinifera before Riparius from XMP17T6 overlay
identities, saves/loads the mixed hold, then reaches two production Unload gates.
Its flat world, pad pose and stage gates are supplied test boundaries; this is
not a full retail match or a float-storage comparison.
Unicorn 2.1.4 executes unchanged original text and Unit/Building vtables.
The JSON contains native outputs, not values calculated by Rust or the harness.
The sidecar records the payload hash.

## Reproduce

Use the shared asset extraction owner to place physical `RULESMD.INI`, optional
`LANGRULE.INI`, `MPBattleMD.ini` and `XMP03T4.MAP` in the input directory. Provide
`ARTMD.INI` there or use the checkout's retail `ini/ARTMD.INI`. The native reader
records every present file's bytes/hash and the absent LANGRULE layer.

```sh
export VERA20K_GAMEMD_EXE=/path/to/retail/gamemd.exe
export VERA20K_REFINERY_DOCK_INPUTS=/path/to/extracted/retail/inputs
python -m tools.spatial_oracle.refinery_dock --check
```

`--write` deliberately replaces native references. Review the payload and metadata
before accepting it. The shared `harvest_attack_return.reader_receipts` owns the
constructor VM, physical lexical input, original CRC indexes, layer order and
complete MissionControl table read. Its docking extension executes additional
selected original key/store blocks; it does not duplicate that reader.

The connected Rust consumers are
[`refinery_dock_oracle_tests.rs`](../../src/sim/world/refinery_dock_oracle_tests.rs)
for the retained controls and
[`refinery_dock_continuation_tests.rs`](../../src/sim/world/refinery_dock_continuation_tests.rs)
for the retained histories.
[`refinery_dock_retail_tests.rs`](../../src/sim/world/refinery_dock_retail_tests.rs)
binds these native selected data receipts to production Battle/XMP03T4 rules
processing and installed fixed ART, including the frame/ppm representations.
`actual_contact_slots_match_original_admission_receiver_and_scanner` replays the
nine slot controls through `Contacts`, CAN_LOAD and the narrow scanner.
`building_contact_constructor_matches_original_slots_before_hello` in
[`world_spawn/tests.rs`](../../src/sim/world/world_spawn/tests.rs) checks the
four constructor controls through the shared component prefix and authored/runtime
construction, then exercises the airfield HELLO consumer against initialized slots.
The production lethal-damage continuation is exercised separately by
`a_refinery_destroyed_mid_unload_hands_the_miner_to_harvest` in
[`refinery_dock_cycle_tests.rs`](../../src/sim/world/refinery_dock_cycle_tests.rs).
Python checks original execution. Rust comparison and runtime integration have
their own validation results; this document does not certify a whole native game.

The validation receipt (`tools/spatial_oracle/refinery_dock.validation.json`, in git at 1b1a8b43e) binds the 56 represented
shared controls, all three histories, selected retail inputs, strict Rust checks
and the release artifact. The ordinary 7,200-step retail run witnessed deposits
at steps 3778 and 5670: each drained 40 bales and paid 1,000 credits without
changing spent credits. Contact release and Harvest followed at 3794 and 5686;
collection resumed at 4229 and 6154. These timings are Rust production
observations, separate from the original-execution comparisons.

## Original instructions and inputs

| Responsibility | Original address/body |
| --- | --- |
| Selected HARV/Rules/BuildingType construction | `7470D0`, `665650`, `4653C0` |
| Strength | ObjectType read/store `5F94D3..5F94F3`, ReadInt `5276D0` |
| UnitRepair/DockUnload/Refinery | `460906..46092F`, `4609D6..4609F6`, `460A52..460A72`; ReadBool `5295F0` |
| NumberOfDocks | `46492E..46494B`, constructor 1 |
| DumpRate/TooFar/PurifierBonus | `670CC0..670CE7`, `66FFD7..670010`, `66FC56..66FC7B`; original defaults .016,5/50,.25f |
| Tiberium Value | constructor store `721713..721719`; original read/store `721AFA..721B12` |
| ART Foundation/QueueingCell | dual read/store `461225..46125D`; `4614EE..46152C` with native zero default |
| NADEPT ART DockingOffset0 | constructor allocation `45E2B5` and zero stores `45E4D2/45E4D6/45E4D9`; resize gate `46494B..46499D`; original sprintf/ReadCoord/slot stores `46499D..464A47`, reader `529CA0` |
| Building center/dock coordinate | `447AC0`; `447B20`, count1 offset-pointer read `447DB5` and components `447DC9/447D8A/447D8D` |
| MissionControl | constructor `4E7CF0`; complete original table reader `5B3760` through `679C92..679CAF` |
| Active building body mode | supplied constructor -1 fields, original BeginMode Idle prefix `447780..4477C7` |
| Ordered refinery scan | `4DF040`, `4DEE80`, actual CAN_LOAD through `65A970` and `43C2D0` |
| Actual contact-slot admission | `65ADF0`; narrow CAN_LOAD call `43C366`, narrow scanner call `4DEF0C` |
| Building contact count initialization | complete Radio constructor `65A750`; Building clamp/call prefix `43BCBD..43BCD5`, count setter `65AE60`, original vector resize `40B9A0` |
| Enter/current-mission cadence | `4D9290`, current control `5B3A00`, original ftol and Scenario `65C7E0` |
| Pending nearby destination | `70D8F0`, Nearby `703590`, GetZone `56D230`, FNPC `56DC20` |
| Pending depot admission | complete Foot `70D7E0..70D8E7`; success Queue `70D83C`, class setter `70D849`, pending clear `70D84F` |
| Dispatch/Stage/latch scheduling | `5B3060`; `6FABB8..6FAC31`; `7365BB..7365DF` |
| Harvest/unload/economy | `73E5E0`, `73D630`, House GiveTiberium `4F9610` |
| Unit destination/per-cell | `741970`; `73A31F` to `73A540` or `73A5EA`; release `73ACB3..73ADCA` |
| Depot power release | `44C35B..44C397`, PowerOn call `44C394` |
| Destroyed contact loop/pointer expiry | `442511..442608`, RUN_AWAY call `4425A4`, pending clear `4425AA`, Unit expiry `7446E0` |
| Fatal Building wrapper/Destroy | saved contacts `4422F9..44234A`, Techno call `442425`, result4 continuation `4424A2`; Building DetachAll `44EBF0` |
| Building ground occupation | Cell OccupyDown `47E8A0`, Building virtual+F0 `453D60`, bit0x80 store `453DAD..453DB0` |

The selected physical retail layers yield HARV Strength 1000 and Storage 40;
NAREFN/GAREFN Strength 1000 with DockUnload/Refinery, and NADEPT Strength 1200
with UnitRepair. All have one dock. Their original ART reads yield foundation
ordinal12 (4x3); NAREFN/GAREFN QueueingCell `(4,1)`, NADEPT `(0,0)`.
The separate `retail_inputs.art.docking_offsets.NADEPT` receipt executes the
original constructor's one-slot allocation and zero stores, then the selected
ART offset loop: `DockingOffset0=128,0,0`. It records original key formatting,
ReadCoord and all three slot stores. The Rust retail binding compares this
receipt with both the fixed ART entry and the installed Building pad owner.
The original input reader preserves constructor/prior-layer defaults and native
stored widths. Effective Image==typeID is a declared input for these types;
the surrounding full Image/type and asset-loading passes are excluded.

Rules+16F8 is a fixed double 1.0, not a `ConditionGreen` INI key. Original Rules
construction and AudioVisual stores `66B323/66B32D` establish it. Object health
radio 0x22 executes signed Health/Strength comparisons, including original masked
zero division. The general Section lookup `526810` returns a pointer in full EAX;
a valid aligned address with AL0 must not suppress a reader.

For independent static inspection, the shared indexed tool produces packets
with original identity/bytes and explicit decode coverage:

```sh
python -m tools.native_inspect disasm 0x4d9290 --bytes 0x200
python -m tools.native_inspect disasm 0x43c2d0 --bytes 0x7d0
python -m tools.native_inspect disasm 0x70d8f0 --bytes 0x90
python -m tools.native_inspect disasm 0x70d7e0 --bytes 0x107
python -m tools.native_inspect disasm 0x703590 --bytes 0x123
python -m tools.native_inspect disasm 0x73a31f --bytes 0x2cc
python -m tools.native_inspect disasm 0x44c35b --bytes 0x3c
python -m tools.native_inspect disasm 0x442511 --bytes 0xf7
python -m tools.native_inspect field 0x1618 --width 4 --start 0x45e890 --bytes 0x9000
python -m tools.native_inspect disasm 0x45e27f --bytes 0x4e
python -m tools.native_inspect disasm 0x45e4c6 --bytes 0x16
python -m tools.native_inspect disasm 0x46494b --bytes 0xfc
python -m tools.native_inspect disasm 0x447ac0 --bytes 0x54
python -m tools.native_inspect disasm 0x447d3d --bytes 0xbc
```

## Behaviors pinned by execution

The no-contact/no-archive Enter tail executes pending recovery, original Unit
idle, Commence and the **current** mission rate. Distinct supplied rates pin
plain/clear HARV Guard 27, ore/AI HARV Harvest 19, installed destination Move 37,
and linked offline refusal Enter 91 for the selected seed. They are cadence
controls, not retail rates. A live negative-health pending target remains valid;
invalid Techno/Alive state clears pending, while no nearby cell retains it.
Live pending controls execute original GetZone/Nearby/FNPC against supplied
uniform sector 0/zone 0 arrays. The no-cell control supplies only the FNPC result.

Nine additional pending-entry admission controls execute complete original
Foot `70D7E0` over the same prepared UnitRepair scene. Outside the foundation,
original HELLO2 establishes or retains the contact, CAN_LOAD0F accepts, and
Queue `5B35E0(Enter7,1)` executes actual Unit Ready `744270` and Commence `5B3570`.
The linked contact is present and its WeaponsFactory type byte is read during
Ready. Guard, Sleep and moving Move callers have current Enter7 before class
destination setter `741970` installs the Building NavCom. Foot then clears
pending+500 at `70D84F`.
This executes the Enter motion exception and world/contact readiness together;
no Ready answer is supplied.

Already on the foundation, the native CAN_LOAD denial instead queues
None `(-1,0)`, assigns NULL, clears pending and sends BREAK3. A busy target
refuses HELLO and retains pending, current mission and the existing destination.
Every new step records prepared field bytes, actual memory reads, radio replies,
mission/queue/Nav/pending/Drive state and full Scenario RNG. These sampled calls
draw no RNG. `returned.frame_matched_calls` pairs each return PC with its caller
stack frame so nested Building replies cannot overwrite outer radio results.
The earlier generic event return annotations are preserved; use the frame
receipts or inherited callback radio results for these new controls. Raw native
helper EAX/AL is recorded as non-semantic for the Rust void owner.

Every control with `input.rates` also carries `supplied_rate_frames` and
`supplied_rate_conversion_receipts`. Those fields come from a separate native
VM executing original `4D946C..4D9481`: current-control lookup `5B3A00`, the
original Rate load, multiplier at `7E27F8`, and `ftol7C5F00`. Execution stops
before the Scenario draw. Raw binary64 Rate/multiplier bytes, the inherited
supplied control word `0x0E7F`, signed frame result and unchanged native
text/vtables/full RNG are recorded independently of the expected Enter return.
The supplied rates yield Enter `.1` ->90, Guard `.03` ->26, Harvest `.02` ->18
and Move `.04` ->36 frames. Their table writes are binary64; INI ReadDouble
instead widens a parsed binary32, so parsing these controls as INI values would
change Harvest's supplied input. Rounding the Guard product to a host binary64
intermediate would also change its frame projection. The Rust test fixture
uses the saved frame inputs without adding another gameplay conversion port.

```sh
python -m tools.native_inspect disasm 0x4d946c --bytes 0x15
```

CAN_LOAD controls distinguish active mode 1 from construction mode 0, current and
queued effective Construction/Selling missions, narrow busy admission, wide
scanner without the receiver bypass and wide scanner with it. The busy history
executes Harvest's original temporary `A8E7AC` increment/decrement and the other
contact's original BREAK before admission succeeds.

The independent `actual_slot_controls` supply `input.actual_slots` in order and
signed `input.type_number_of_docks` separately. Native `65ADF0`, CAN_LOAD0F and
the narrow refinery scanner accept `[OTHER,NULL]` with type capacity1 and refuse
`[OTHER]` with type capacity3. An own contact and first/middle/last sparse NULL
slots also accept; an empty actual vector refuses. These are supplied-state
divergence controls, not evidence of a natural stock-play transition that changes
vector size. Every query retains the vector and full Scenario RNG. Prepared bytes,
actual slot reads, native AL, CAN_LOAD reply and scanner result are saved.

The Rust fixture deserializes actual Contacts from `input.actual_slots` through
their existing owner, preserving the supplied empty, duplicate and sparse vectors
independently of NumberOfDocks. Each new row has the normal full-state snapshots,
three steps (`has_free_or_own`, `radio` message15, `find_bay` narrow), and independent
`contact_vector_before`/`contact_vector_after` receipts on each step. Only AL is
the helper's semantic return; raw EAX may retain pointer bits above AL.

The four `building_contact_constructor_controls` execute full original Radio
`65A750`, including its one-slot allocation and NULL store, followed by the original
Building contact-count prefix `43BCBD..43BCD5`. Original `65AE60` and virtual vector
resize `40B9A0` execute. Signed NumberOfDocks `-4`, `0`, `1`, `3` produce actual
counts `1`, `1`, `1`, `3`, all NULL, with unchanged full Scenario RNG. The JSON saves
the supplied type bytes, state before/after Radio construction, state after the
Building prefix, and original calls/reads/writes. The existing native-reader heap
supplies allocation storage and ignores deallocation; no native text or vtable is
patched. EAX's BuildingType pointer and ESI's Radio-constructed object are supplied
entry registers. The surrounding full Building constructor and world registration
remain outside this bounded constructor comparison.

The single critic found the type-derived admission duplicate and lazy slot sizing.
All three focused regressions failed before correction. Rust now initializes slots
in `world_spawn::construction::install_techno_components`, queries them only through
`Contacts::has_free_or`, and sends refinery/depot/airfield HELLO through the radio
owner. The duplicate predicate and caller-specific HELLO wrappers were removed.
The corrected contact tests and full strict-retail suite pass; saved logs and source
hashes are bound by the validation receipt.

```sh
python -m tools.native_inspect disasm 0x65adf0 --bytes 0x40
python -m tools.native_inspect disasm 0x65a750 --bytes 0x8a
python -m tools.native_inspect disasm 0x43bcbd --bytes 0x18
python -m tools.native_inspect disasm 0x65ae60 --bytes 0x45
python -m tools.native_inspect disasm 0x40b9a0 --bytes 0x100
```

Depot DOCKING asks linked Object health22, then NEED_TO_MOVE 0x13. It returns a
Building payload and sends no MOVE_HERE 0x12. Unit per-cell accepts effective
Enter 7 or literal25, requires the contact's native dock-coordinate cell and
matching Building NavCom; exact world-coordinate equality is unnecessary.

Every UnitRepair control carries `supplied_dock_inputs`, queried
in a separate prepared VM before any mission/radio step. These historical sparse
types have NumberOfDocks 1 and a NULL items pointer at BuildingType+1788. Mapped
page0 supplies offset bytes `ffffffff0000000000000000`, or `offsets[0]=(-1,0,0)`.
Original `447AC0` returns center `(2048,2688,0)`; original `447B20` reads that
offset at `447DC9/447D8A/447D8D` and returns `(2047,2688,0)`, cell `(7,10)`.
This coordinate describes the supplied fixture, not stock 4x3 depot data. The
native input receipt saves count, pointer, offset bytes, center, dock coordinate
and exact reads, checking unchanged state, full RNG, original text and vtables.
The Rust fixture projects this supplied offset through its existing ART pad
owner. The independent retail reader retains NADEPT's stock `(128,0,0)` offset.

The missing-NavCom exception is **Hover**: original GetClassID `517070` returns
`4227584a3998d111b70900a024ddafd1`, the compared literal at `7E9A40`. Drive
`4B4830` returns the neighboring `41` identity. Successful per-cell docking
executes Foot per-cell once, DOCK_NOW 0x15 and **PowerOff55A910**, preserving
NavCom and Drive destination, then returns before Unit Ready/Commence and its
second Foot tail. Depot DOCK_NOW queues Building Repair 20 and Unit Sleep 0.
The release control executes original PowerOn before the exit-query boundary;
full repair health/payment production is outside that control.

The normal and busy histories start with 40 Riparius, execute admission and
unload, then resume original ore search/harvest to cargo 1. Native payout is 1000;
the observed shared score at House+54E8 becomes 200. Original GiveTiberium
`4F9628` stores `ftol(amountFloat * 5.0 + priorScore)` there, before the balance
store `4F965D`; kill feeder `70300F` and terminal score consumer `5C99EB` use
the same score field. Every step saves
full 250-word Scenario RNG, cursor state, return values, Stage/mission timer
writes, original radio sends, queues and destination changes. The original
Unload dump gate still pays when the linked refinery's online flag becomes
false; that gate does not re-check offline state.

## Scene and coverage boundaries

- The 107 legacy rows keep their historical supplied idle/readiness/bay/FNPC
  answers unchanged. The new corpus runs original idle, readiness and scanner;
  only the destruction history runs reached original Scatter bodies.
- Unit/House/Scenario/map constructors and world membership are supplied.
  Native parsed final type/rule bytes transfer into a 32x32 sparse scene. The
  selected Dock vector is represented by one eligible NAREFN pointer/ordinal 0
  and supplied House counters/ordered slots. The second radio Building has
  coordinates but is not foundation-placed. The main Building is on the twelve
  foundation object lists; its raw ground occupation words are zero, as pinned
  below. Object-list membership and the raw occupation plane are distinct inputs.
- Physical arrivals put the mover at its native Cell destination and terminate
  Drive destination/head vectors, then execute original setter/per-cell cleanup.
  Track traversal, full pathfinding and whole Logic/Scenario scheduling are
  excluded. CanReachZone uses the existing declared scene answer and retains
  its arguments. Uniform zone arrays are supplied state, not zone construction.
- Animation/smoke and tactical redraw sinks have no effect; ore occupancy-cache
  refresh uses the inherited fixture callback. These do not establish rendered
  output, generic occupied-world movement or complete ore-queue behavior.
- Destruction supplies captured contacts and the already dead Building, then
  executes original contact RUN_AWAY, both reached Scatter calls, pending clear
  and Unit pointer expiry. Cargo 40 survives and payment remains zero. This
  prefix leaves tether bits/building contact present; whole-world DetachAll,
  House roster/type-counter cleanup, foundation removal and destruction are
  excluded. The prefix is not evidence for final native cleanup.
- Stock integer cargo/payment is covered. Native four float cargo slots,
  non-stock IncomeMult 0.9 (the preserved legacy native 899/Rust 900 difference),
  Weeder/absorbers/naval/slave/Chrono branches, general repair payment cadence
  and unrelated Building Guard docking RNG remain separately bounded mechanisms.

These boundaries accompany the goldens and must accompany any parity claim.

## Fatal Building wrapper and pointer expiry

Original `Building442230` copies its sparse contacts at `4422F9..44234A`
before calling `Techno701900` at `442425`. The common receiver calls
`Object5F5390` at `701DF8`. The killing-hit Destroy callback reaches Building
DetachAll `44EBF0`: its factory prelude precedes BREAK broadcast `44EED4`,
then Object DetachAll `5F5280` at `44EEE2`. Object loads pointer-expiry control1
at `5F530B` and calls the global expiry dispatcher `7258D0` at `5F5311`.

Techno's synchronous death effects, including Stun and a nested DeathWeapon,
finish before the concrete Building receiver resumes. It checks ObjectAlive
at `44242C`; result4 selects `4424A2` through table `442C18`, then walks the
saved contacts at `442511`. The stock pad contact receives RUN_AWAY at `4425A4`,
and its private pending+500 is cleared at `4425AA` **after the receiver returns**,
even when pending names another Building. Building DestructionEffects at
`442665` and final UnInit/foundation removal follow this loop. This contact loop
belongs to the damage wrapper, not generic Destroy. The delayed-kill PostMortem
return is result5 at `701F6E`, so it does not enter the result4 loop. An admitted
Health0 re-entry is explicitly forced to result4 at `702035`.

The retained native history starts at the already-dead saved-contact loop; it
does not execute that whole killing hit. In that prefix, Unit RUN_AWAY clears
the unloading latch, calls Scatter `(NULL,1,0)`, queues Harvest and Commences,
then falls through to Foot RUN_AWAY's Scatter `(NULL,1,1)`. The first Scatter
is refused by the still-effective Unload mission; the second runs after Harvest
promotion. Both original calls must remain, even though only one changes NavCom.

Original Unit pointer expiry `7446E0` follows Foot/Techno/Radio/Object owners.
Radio `65AAF4` removes matching contact slots only for control1 and does not
write tether+418. Techno `707AE7..707AF5` likewise gates pending+500 clearing on
control1. These narrow receipts do not imply tether survives a whole killing
hit: the earlier Destroy BREAK is a separate native transition. The Rust damage
test captures pre-hit contacts once, preserves the wrapper boundary, and checks
expiry-before-RUN_AWAY-before-UnInit plus an unrelated pending-entry clear.
Its actual test result must be reported separately from this static evidence.

Reproduce the original body/vtable/dispatch evidence with the shared inspector:

```sh
python -m tools.native_inspect disasm 0x442230 --bytes 0x438
python -m tools.native_inspect read 0x442c18 --bytes 0x10
python -m tools.native_inspect read 0x7e3f98 --bytes 0x1a8
python -m tools.native_inspect disasm 0x701900 --bytes 0xa20
python -m tools.native_inspect disasm 0x5f5390 --bytes 0x494
python -m tools.native_inspect disasm 0x44ebf0 --bytes 0x398
python -m tools.native_inspect disasm 0x5f5280 --bytes 0x120
python -m tools.native_inspect disasm 0x7446e0 --bytes 0x100
python -m tools.native_inspect disasm 0x707ae7 --bytes 0x40
python -m tools.native_inspect disasm 0x65aac0 --bytes 0x50
```

## Scatter raw-occupation control

At frame265 of `stock_refinery_destroyed_while_unloading`, unchanged original
Unit Scatter `743A50` and FNPC `56DC20` choose cell `(9,10)`. The supplied main
foundation has the Building at every Cell+E4 list head and **zero Cell+124 raw
ground occupation**. Supplying only bit0x80 on those same twelve cells makes
the same original history choose `(10,11)`. Neither control draws RNG; both
retain both Scatter calls and unchanged original text/vtables. This isolates
a supplied-state difference, not a Scatter/FNPC algorithm defect.

The bit has an original producer: Cell OccupyDown `47E8A0` calls Building
virtual+F0 at `47EA43`; the original Building vtable resolves that to `453D60`.
That body loads Cell+124 at `453DA2`, ORs bit0x80 at `453DAD`, and stores the
DWORD at `453DB0`. The control below supplies the raw bit directly; it does not
execute foundation Mark production or certify whole destroyed-world placement.

```sh
python -m tools.native_inspect disasm 0x47e8a0 --bytes 0x250
python -m tools.native_inspect read 0x7e3fac --bytes 0x4
python -m tools.native_inspect disasm 0x453d60 --bytes 0x60
```

Run this read-only control from the checkout with the same retail executable
environment as `--check`. It reuses the existing retained-VM owner, does not
replace goldens, and prints `0x0 [9, 10]` then `0x80 [10, 11]`:

```python
from pathlib import Path
import json
from tools.spatial_oracle.refinery_dock import DockContinuation, BLD, NW, cell

payload = json.loads(Path("tools/spatial_oracle/refinery_dock.json").read_text())
history = next(row for row in payload["deposit_histories"]
               if row["input"]["name"] == "stock_refinery_destroyed_while_unloading")
for mask, expected in ((0, [9, 10]), (0x80, [10, 11])):
    scene = DockContinuation(history["input"], payload["retail_inputs"])
    for step in history["steps"]:
        operation = step["input"]
        if operation["op"] == "building_now_dead_contacts":
            for y in range(NW[1], NW[1] + 3):
                for x in range(NW[0], NW[0] + 4):
                    pointer = cell(x, y)
                    assert scene.read32(pointer + 0xE4) == BLD
                    assert scene.read32(pointer + 0x124) == 0
                    scene.u.mem_write(pointer + 0x124, mask.to_bytes(4, "little"))
            prior_rng = scene.state()["scenario_rng"]
            scene.step(operation)
            result = scene.state()["miner_nav"]
            assert result == expected, (mask, result)
            assert scene.state()["scenario_rng"] == prior_rng
            assert len([event for event in scene.steps[-1]["callback_events"]
                        if event[0] == "scatter"]) == 2
            scene.finish()
            print(hex(mask), result)
            break
        scene.step(operation)
    else:
        raise AssertionError("no saved-contact operation")
```
