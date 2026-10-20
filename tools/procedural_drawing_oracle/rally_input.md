# Factory rally input preparation

`rally_input.py/json/meta.json` preserve 27 original `Building443860`
executions, two separately bounded negative-frame controls and 28 direct
`Cell4834A0` prerequisite controls against the shared
`walk_move_admission.prepare_fixture` native Cell/zone grid.
The emulator version and payload
hash are in the sidecar. No gameplay leaf is replaced. Only imported
`timeGetTime` supplies zero for outgoing command wall timestamps.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.rally_input --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.walk_move_admission --check
```

Use the configured original executable and Python environment described in
[`native_oracle.md`](../native_oracle.md). `--write` deliberately regenerates
references; default/`--check` cannot change them.

## Executed caller contract

`443917` calls original `GetZone56D230` with physical Building `Location+9C`
divided by256 toward zero, the chosen movement zone, and **the clicked Cell's**
bridge flag. The factory's foundation center is not this source coordinate.
`443957` then calls original `FNPC56DC20` with:

```
(out, clicked, speed, source_zone, movement_zone, clicked_bridge,
 1, 1, 0, 0, 0, 1, &Cell(0,0), 0, 0)
```

Speed/zone default to raw0/0 (Foot/Normal), including UnitType factories.
Factory3 selects4/9 (Winged/Fly); Naval then overrides either to6/4
(Amphibious/AmphibiousCrusher). The corpus executes those alternate callers
without claiming their input-action admission. Full FNPC, raw passability,
bridge zone resolution, native projection, NetID packing, event constructor
`4C6780` and outgoing queue append all execute original instructions.

`cases[*].source_zone` and `.nearby` record actual arguments/returns;
`.constructed_events`, `.queue` and `.timestamps` record the output boundary.
The previous ArchiveTarget, actor/type/House bytes and clicked coordinate stay
unchanged. Event1E performs the later assignment; this corpus does not execute
the synchronized dispatcher. The reached original Random, RandomRanged and
SetArchive entry counts are zero.

## Coverage and bounds

The 27 controls cover clear and raw-blocked clicks, a fully impassable speed
table, an outside click, negative physical coordinate truncation, split zones,
eight binary-frame bit patterns, intact/missing bridge records, clicked-bridge
versus clicked-ground source-zone lookup, Infantry/Aircraft/Naval options,
full outgoing queue and queue-tail wrap. In this supplied grid:

- Raw occupancy `0x1F`, `0x20` and `0xFFFFFFFF` at11,5 redirect to11,4;
  `0x100` keeps11,5.
- Source9,5 on a supplied bridge resolves zone2 when the click has the bridge
  flag, but zone3 for a ground click; missing bridge records remain missing.
- Split-zone frame0 chooses7,9, frame1 chooses7,2, frame100 chooses7,5;
  `0xFFFFFFFF` returns0,0 and sends no event only with the fixture's zeroed prior
  stack. It is outside the general native parity domain described below.
- A zero speed table returns0,0 and leaves the old rally point intact; a full
  queue still constructs the event but does not append it.

Map dimensions8,8, LocalSize0,0,8,8, 16x16 prepared cells, raw zone labels2/3,
bridge records, speed floats, original vtables and object fields are supplied.
They do not establish native map loading/flood-fill, retail field readers,
full actor construction, action selection, selection ownership, EVA, ConYard
redeploy fallback or rendered output. The alternate factory controls establish
literal caller options only. The helper refactor leaves the old Foot payload
and metadata byte-identical (`walk_move_admission --check` passes).

## Real and Dummy passability prerequisite

`passability_cases` executes original `4834A0` directly. The whole caller above
reaches this same body through `FNPC56DC20` and `Rectangle56E7C0`; its zero-speed
case returns `(0,0)` after 136 passability calls and queues no event. The direct
matrix isolates that required input behavior without replacing the caller.

Original `48357D` reads the retained Cell `+0xEC` land type and `483583` reads
overlay `+0x44`. These fixtures have overlay `-1`, so the wall branch is skipped.
`4835D5..4835DE` loads `float[0x89EA40 + 4*(9*land + speed)]`. The following
comparison rejects a value equal to zero unless the selected movement plane is
the bridge deck. There is no exemption for the shared Dummy identity.

The 28 controls record:

- Real `(5,5)` and Dummy `(99,98)` with retained land types Clear `0` and
  Water `2`, Foot `0` or Amphibious `6`, and opposite Clear/Water zero/nonzero
  table rows. Both identities produce the same acceptance and actual loaded
  indices `0`, `6`, `18`, `24`. Changing Dummy's land changes its table row.
- Winged `4` with selected cost zero, ground/deck occupation `0xFF`, requested
  height `7` and mismatched requested zone `12345`. Native returns true at its
  early branch without reaching either the speed-table load or `GetZone56D230`.
- Flag `0x100`, base level `0` and no requested height select the empty deck
  and admit zero land cost even with ground occupation `0x40`. Deck occupation
  `0x40` rejects before the table load. Requested base height `0` with
  `bridge_aware=true` selects ground and rejects zero cost; requested height `4`
  selects the deck and admits zero cost, including retained Water land.

Each row has explicit `input` fields, the seven literal dword `args` and their
`args_hex`, `cell_before_hex` (Cell bytes `0..0x148`), `speed_table_hex` (90
little-endian floats), `table_loads` with actual native index/address/value,
`zone_queries`, and the original returned `accepted` boolean. Code, Cell and
table bytes are checked unchanged. The table has nine speed columns per land;
unused land rows contain `1.0`. The matrix is representative, not exhaustive.
It does not establish terrain INI parsing, arbitrary multipliers, wall overlays,
native map construction or the Dummy's wider lifecycle. Rust's retained table
must originate from its existing `TerrainRules` reader, with that reader and
the shared production passability owner tested separately.

Adding this matrix preserved every field of the previous 27 caller and two
boundary payloads. Canonical payload SHA256 values are respectively
`3947cab5131d7ffc10189f984c1973eb217b65682c97b79893cf52d09e50002f` and
`d6cc978256267ad3510fb0f56221001699d10f9402e1cb4dfd099376df0e9f41`.

## Signed frame selection boundary

The final null-reference selector is signed:56E6A8 loads binary frame,
56E6AF executes `CDQ`, and56E6B2/56E6D1 executes signed `IDIV` by the valid or
fallback count.56E6BF/56E6DE loads the selected Cell with that signed remainder;
there is no lower-bound guard. The fixture's `final_selection` records the
actual remainder and the memory value about to be read, not a Python selection.

Before the four register pops, the24-slot raw accepted array starts atESP+CC,
valid array atESP+12C, and fallback array atESP+18C. A negative valid index reads
raw-array tail storage. If fewer than24 cells were gathered, that storage can
be untouched. A negative fallback index reads preceding valid-array storage,
while that branch has no valid candidates. This is not a safe negative index.

Three original executions distinguish the behavior:

- Split-zone frameFFFFFFFF has8 accepted/8 valid and remainder-1. Raw slot23
  is untouched in this execution; a zeroed prior stack returns0,0/no event.
- The same inputs with prior stack filled with Cell6,6 return6,6 and enqueue
  event1E. The fixture is deliberately exposing dependence on prior stack data.
- Blocking the25 cells in x6..10/y6..10 around click8,8 forces24 accepted/24
  valid cells. FrameFFFFFFFF still has remainder-1, but raw slot23 is now
  initialized to11,10; native returns11,10 and enqueues. This is a bounded
  initialized-storage witness, not proof of intended negative-index semantics.

The existing frame80000000/split-zone control has8 valid cells and remainder0;
it safely selects7,9. It establishes only that zero-remainder control. General
Rust/native comparisons are bounded to binary frames0..7FFFFFFF. VERA retains
its deterministic unsigned frame selection outside this signed domain as an
explicit divergence; neither zeroed-stack nor poisoned-stack results are an
intended production rule. `native_boundary_cases` keeps the two added witnesses
separate from the ordinary27 input cases. This boundary does not close native
parity for later high-bit frames.

## Related native prerequisites

Original `HasRallyPoint455DA0` reads Factory+EB8 (40 or16), Cloning+16AC or
UnitRepair+16A9. BuildingType ctor45DD90 initializes these to0/false; reader
45FE50 reads exact-case `Factory` (46051A..460545), `UnitRepair`
(460906..460929) and `Cloning` (46094E..460977), retaining current defaults
between INI layers. `Factory` uses474FF0/40DCE0 RTTI-name lookup; the booleans
use5295F0. These constructor/reader claims are instruction evidence, not an
additional reader execution in this corpus.

Techno ctor6F2B40 zeroes ArchiveTarget+218 at6F2D15. Event1E at4C6DAA clears
planning tokens, requires a live decoded Techno, and calls SetArchive70C610
at4C6DDE. Building SetDestination455D50 clears/sets Archive for HasRallyPoint
or ConYard except while Selling; base TechnoSetDestination709A30 is empty.
Techno PointerExpired7077C0 clears an equal Archive only for nonzero control
(707AE7..707B03); Building delegates through44E8F0. Building ownership transfer
through7014A0 clears Archive at70151A. These are caller/body evidence; the Stop
and lifecycle Rust owners and their validation remain separate.

Rally RGB comes from House+56F9, initialized by50B840 using ColorScheme+330
(ctor68C710 sets16), converter+30C and packed-color table+174. RGB565 unpack
clears low channel bits without replication. Campaign500B40 repeats this
conversion after `Color=`. ComputeRemap50BA00 writes the separate laser RGB
atHouse+56FC. The separate [`house_color.md`](house_color.md) comparison now
executes the original ramp, lighting table and House/campaign conversion for
all21 physical stock colors. House uses N53 row26, which differs from directly
packing raw ramp[0] for four stock inputs.
