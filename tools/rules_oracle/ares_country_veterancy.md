# Ares Country initial veterancy

`ares_country_veterancy.py` executes the enrolled Ares DLL and locally
configured retail executable through the existing public native VM. Inputs are
synthetic IDs, retained objects and lexical values. No mod resources or client
configuration are read by the producer.

Set `VERA20K_ARES_DLL` and `VERA20K_GAMEMD_EXE`, then run:

```sh
python -m tools.rules_oracle.ares_country_veterancy --check
```

`--write` deliberately regenerates the native corpus and metadata. The Ares
identity gate is shared with `ares_prerequisites.py`. Keep local executable
identities and execution receipts outside the repository.

## Native behavior established

The DLL's own hook table records the three-list replacement at `511D16`, the
successful and failed Country reader suffixes at `51214F`/`51215A`, and
`BuildingClass_Init_Academy` at `442D1B`. The last exported hook body is
`100236D0`. These records establish installed sites; the controls invoke hook
bodies with explicit ABI frames and do not emulate the installer.

Original `Country511850` still owns `ParentCountry`: current stored ID section,
current-ID default, capacity25 and a retained24-byte payload. The selected Ares
replacement `1005BE70` reads `VeteranInfantry`, `VeteranUnits`, then
`VeteranAircraft` using capacity2048 and Find-only binding. An omitted/empty
returned string retains the prior vector. A nonempty payload replaces it;
unknown IDs and sentinel tokens are skipped, ordered duplicates are retained,
and inner token spaces are significant. Later type registration does not bind
an earlier token without a nonempty reread. The tests include both sides of the
2047-byte payload boundary.

CountryExt literal constructor stores `1001B85F..1001B8DA` establish phase 0,
an empty `VeteranBuildings` vector and false parent-initialized flag. The
whole reader `1001C530` writes phase 1 before first common read, then phase 2;
phase 1/2 reread and negative/>2 phases skip. Its first-init side defaults do
not reset the retained veteran vector or one-shot flag. The common reader
`1001C5D0` also executes after a missing original Country section; the native
base failure and suffix are connected by a declared synthetic ABI frame.

Before common field reads, false flag + nonempty retained ParentCountry + a
case-insensitive non-self ID invokes `1001BDC0`. Actual Country Find
`1001BA90` matches the first stored `ID+24` case-insensitively, does not use
`Name+64`, and rejects `none`/`<none>` before registry lookup. Missing or
rejected lookup still consumes the one-shot flag. Empty/self skip without
consuming it. Inheritance copies the parent's current extension vector once;
later parent rereads do not change the child. This extension copy does not
copy the original three Country vectors.

`VeteranBuildings` is read by `10010850` from the current Country.ID in the
caller-supplied generic INI, capacity2048, empty default. It follows the same
nonempty replacement, Find-only, duplicate-preserving policy. It follows
AI.PowerPlants, parachute/paradrop fields, LoadScreenText.Color, selection
weights and ListIndex, and precedes Degrades/CanBeDriven/start-unit fields.
Only the fixed shared pass owner should select the physical rules layers.

Building initialization `100236D0` checks this list by type pointer and writes
veterancy bits `3F800000` on a match, independently of Trainable. It then has
separate spy and Academy branches. The corpus sets the spy gain false and
records Academy as a sink; it does not implement or certify those producers.
A matching country list writes the literal veteran value, including when the
supplied prior rank was greater; duplicates do not increment it.

Country-only alternate-cameo admission executes whole `1003D800`, with all
House/HouseExt spy flags false and an existing alternate SHP field. Country
matches admit untrainable Infantry/Unit/Aircraft/Building. Missing alternate
art rejects them. For an undeploying Building the branch compares the Building
self pointer in the original Unit list, rather than its UndeploysInto target
or VeteranBuildings. The positive self-pointer control explicitly marks this
as structurally supplied, not an ordinary UnitType-only INI-produced state.
No asset loading, PCX decoding or rendered cameo is certified here.

Vector helpers `1000F0F0`/`1000DAA0` serialize count then ordered pointer tokens,
including duplicates. Parent-flag save/load slices are
`1001B378..1001B388`/`1001B1C3..1001B1D3`. Actual ByteBuffer helpers produce a
17-byte payload and 21-byte framed IStream for three items plus one flag.
The shared `ScenarioFixture` owns byte transport; pointer-swizzle registration
is observed without resolving relocation. Whole save framing and failure
recovery remain outside this bounded evidence.

## Corpus schema and coverage limits

Schema 1 exposes `installed_hook_records`, `constructor`, `country_lists`,
`parent_find`, `extension_histories`, `building_creation`, `cameo_country`, and
`extension_stream`. Each history step retains its lexical input, ordered
resolved IDs, phase/one-shot state and observed reader/lookup diagnostics.
Rust checks should compare owner-produced histories, not parse the final merged
INI or re-resolve type IDs at spawn/cameo time.

`Probe` owns VM/image/arena/call setup. This producer disables its prerequisite
reader/CRT/category substitutions; the original comparisons, parent Find,
tokenizer, vectors and membership run. Declared boundaries are ReadString
lexical transport, diagnostics, missing-section admission, Academy, allocator
backing and shared IStream/swizzle transport. The selected paths reject observed
RNG/detach entry calls; this is not proof about unexecuted startup or unrelated
spy/Academy mechanisms. Original code is checked unchanged.

Physical INI ingestion, primary-Rules GUI fields, full constructors, startup
scheduling, nondefault CRT locales, whole House lifecycle, save relocation,
spy/Academy effects, promotion art, rendering and whole-match behavior require
their own integration evidence. This is bounded native execution evidence,
not a claim of complete Ares or MO compatibility.
