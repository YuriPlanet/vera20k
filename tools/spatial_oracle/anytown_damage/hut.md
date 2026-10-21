# Anytown concrete hut damage caller evidence

Six bounded executions of original active-retail `gamemd.exe` cover the two
authored Anytown huts, (85,58) and (89,51), with a healthy, first-damaged or
collapsed middle row. The original `anytown_hut.py --write` and a separate `--check` process
both exited 0. The canonical native payload SHA256 is
`51a58c909583e7f362c6237257905df5148b4a850ffb03de4fd411f5230bbaa3`.

This establishes original `574000 → 5749C0 → 575BA0 → 57CCF0` lookup,
selection, extent, sweep, retries, primitive effects and animation requests for
this physical Y-axis span. It does not execute the complete Bomb/Building hut
lifecycle or demonstrate Rust/production parity. The X-axis `575870` and the
fallback route below are instruction-established only.

## Ownership and inputs

The harness reuses `tools/spatial_oracle/anytown_damage/anytown_resident.py`
and its physical geometry, overlay readers, original Recalc and empty occupant
consumer. It does not copy or replace those owners. The existing
`tools/rules_oracle/bridge_anim_lists.py` executes the original BridgeExplosions
constructor/read blocks and native AnimType constructors. The selected physical
RULESMD, absent LANGRULE, MPBattleMD and XMP03T4 layers yield
`TWLT026,TWLT036,TWLT050,TWLT070`; constructor default is an empty vector.

The inherited inputs are the stock `XMP03T4.MAP` from `multimd.mix`, entry
`0xCD0DDEF2`, SHA256
`7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`.
The selected span occupies x86..88, y51..57, authored level 4. The shared owner
supplies a 180-cell physical crop, pristine TMP heads, initial zone planes and
empty object heads, then executes original Recalc on the 21 span cells and two
adjacent Water cells. Its other initialization and callback boundaries remain
unchanged. No native scenario/map/theater load is claimed.

First-damaged and collapsed prerequisites are produced by one or two original
`57CCF0` calls at (87,54), recorded separately in every case. No damaged overlay
is manufactured. All three RNG streams begin with original seed 0. Frame 1000
and a zeroed TacticalDisplay object are supplied. Original map startup
`561710/5617A0/5617C0/5617E0`, after the inherited CRT/WinMain FPU setup,
derives `ABDE88=104`; animation Z is the signed cell level times that value.

Every measured call asserts the entire mapped original `.text` remains SHA256
`4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc`.
No instruction is patched. The payload pins all 24 physical input files plus
absent LANGRULE.

## Native outcomes

`574000` searches a 5×5 square in X-major/Y-minor order. It selects the first
overlay in 205..232 and immediately calls `5749C0`, which canonicalizes the
width member before dispatching the axis walker. Each extent includes its
first failed off-band probe. The common extra count cancels in the bias and
direction comparison. Both huts bias to (87,54), then process four axial cells.

| Hut | First selected cell | Walker entry | Negative/positive extents | Four sweep Y values | Final x87 overlays, Y51..57 |
| --- | --- | --- | --- | --- | --- |
| (85,58) | (86,56), lookup 16 | (87,56) | 6 / 2 | 54,53,52,51 | 228,232,232,232,222,215,229 |
| (89,51) | (87,51), lookup 3 | (87,51) | 1 / 7 | 54,55,56,57 | 227,214,221,232,232,232,230 |

Those selection/extents/sweep/final overlay values hold for all three initial
states. Each retry rereads and mutates the live native cells. The healthy
middle row first changes `216→220` while returning AL=0, so the walker retries
and gets `220→232`, AL=1. A pre-collapsed middle row returns false on all three
attempts. The final endpoint (228 or 230 after earlier propagation) also
returns false on all three attempts.

| Initial state at (87,54) | Primitive calls | Changed cells during hut call | Animation requests | Scenario range requests | Raw Scenario words |
| --- | --- | --- | --- | --- | --- |
| Healthy | 7 | 15 | 12 | 48 | 55 |
| First-damaged | 6 | 15 | 12 | 48 | 55 |
| Collapsed | 8 | 9 | 9 | 36 | 41 |

These counts apply separately to either hut. The collapsed row's overlay 232
skips the three animation requests for that sweep step but still takes the
three primitive attempts. The other steps request three animations across the
width before primitive damage. Each animation uses Scenario `(0,2147483646)`
for X jitter, then the same range for Y, then `(1,5)` delay, then `(0,3)` type
slot. Native x87 arithmetic and FTOL execute. Rejection draws are retained.

The first three healthy/first-damaged requests for either hut are TWLT070 at
(22147,13968,416), delay 4; TWLT036 at (22412,13972,416), delay 5; TWLT050 at
(22649,13947,416), delay 1. Every request uses loops 1, flags `0x600`, Z adjust
0 and reverse 0. The full request sequence is in the payload and summary.

Main and MapGen RNG remain unchanged. Frame remains 1000 and the original
walker writes TacticalDisplay+`0xD7C` to 1 after the connectivity callback.
Root EAX/AL are mechanically recorded as zero under that callback seam; they
are not asserted to be a Boolean success contract for `574000` or the walker.

## Original callers and fallback

The frozen external `caller_startup.asm` contains original instruction slices,
without relying on the current Ghidra names. [`hut_callsite_bytes.json`](hut_callsite_bytes.json)
checks 15 critical call targets directly against the original PE. The concrete caller addresses are **Bomb `438982`**
and **Building update `44031B`**, both calling `574000`. The nearby `43896A`
and `440301` call **wood/low `574C20`**, not the concrete driver.

The Bomb branch tests target WhatAmI=6 and BuildingType+`0x16B6`
(BridgeRepairHut), then searches a 5×5 square in Y-major/X-minor order for a
wood/low family. The Building update branch checks its +`0x6DF` flag and
timer fields +`0x528/+0x530`, then the same type flag and low-family selector.
That selector chooses low if a tile lies in WoodBridgeSet base..base+15 or an
overlay lies in 74..101. Otherwise it calls `574000`. After the call, Building
clears +`0x6DF` at `440320` and +`0x540` at `440327`. These prefixes/suffixes
are instruction-established; no Bomb or full Building update is emulated here.

The bounded walkers call the shared concrete primitive at `575B10` (X) and
`575E42` (Y), returning at `575B15` and `575E47`. The current physical span
selects only Y. The frozen external `native_575870.asm` preserves the original X alternative.

The existing fallback can reach `57CCF0`: when `574000` finds no primary overlay
seed, it enters its flags/ramp path at `57409D`. That path calls `587180` at
`574483` and `57459A`, with bounded retries. At `5871F9..587213`, `587180`
tests the selected overlay in **205..230**, calls **`57CCF0` at `58720A`**, and
retains its AL result. Terminal 231/232 do not take that branch. The saved full
`574000`/`587180` bodies establish this instruction path. None of the six stock
cases reaches the fallback, and no full structural/ramp/hut lifecycle or
fallback dynamic coverage is claimed.

## Seams and retained evidence

Original lookup, selector, extents, sweeps, primitive bodies, propagation,
Recalc, empty occupant scans, endpoint notification, RNG and FTOL execute.
Inherited connectivity `56C510`, hierarchy `586990`, display and radar sinks
remain recording callbacks; heap allocation is bounded and successful.
`Anim421EA0` records all seven native arguments and returns its already
allocated storage. It does not execute the instance constructor, ART, lifetime,
audio, random-rate or spawned-effect work. Those wider chains may consume
additional RNG; these counts are exact only at the declared request boundary.
No objects, Engineer, full navigation graph, renderer or Rust comparison runs.

[`hut.json.gz`](hut.json.gz) retains the full ordered event trace and visit
numbers, all 180 before/after cell snapshots, cell writes, primitive return
registers, preparatory damage traces, frame/dirty state, complete three-stream
RNG states, raw words and requests. [`hut_summary.json.gz`](hut_summary.json.gz)
is the original direct projection for review; the full payload is authoritative.
The six disassemblies remain frozen externally and are pinned by
[`hut_promotion.json`](hut_promotion.json), along with every original artifact.
No external research Python or disassembly is required to replay.

The promoted harness changes only imports, output location and source-path
identity handling. The original unprojected payload hash must match even on
`--write`. The existing shared compressed-reference owner publishes deterministic
canonical JSON/gzip; copied `animation_inputs.layers[*].source_lines` become
hashes, while all native reader results, traces and state remain exact.
[`hut_receipt.json`](hut_receipt.json) records this promotion's independent
write/check and its artifact identities; the common package validator
also checks that receipt.

[`hut_test_vectors.json`](hut_test_vectors.json) is the uncompressed Rust test
projection. Each case preserves `hut`, `starting_stage`, `result.rng_before`,
`result.rng_after`, `result.final_bridge_cells` (21 rows sorted Y then X), and
`result.animation_requests` (type, position, delay, flags, loops, Z adjustment
and reverse in native order). The source payload is hash-checked before this
projection is written or checked by the existing shared native-oracle owner:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.anytown_damage.hut_test_vectors --check
```

## Replay

Use the Python/Unicorn environment described in [native_oracle.md](../../native_oracle.md), with
Unicorn 2.1.4 and liblzo2. From a VERA20k repository checkout, provide the
original retail executable and the two separate local
asset directories described by
[retail_manifest.json](retail_manifest.json). Keep LANGRULE absent.
No proprietary map, TMP, SHP or INI body is duplicated into this packet.

```sh
cd /path/to/vera20k
export VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe
export VERA20K_SHRAPNEL_INPUTS=/path/to/bootstrap-assets
export VERA20K_ANYTOWN_INPUTS=/path/to/anytown-assets
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.anytown_damage.hut --check
```

`--check` re-executes all six native cases and compares the complete published
payload and provenance without writing. `--write` republishes the pinned native
reference and rejects a changed original payload. Every original external
packet remains unchanged.
This is evidence packaging; neither the replay nor the package receipt
establishes a Rust, production or complete hut-lifecycle comparison.
