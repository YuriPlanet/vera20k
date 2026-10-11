# Sidebar cameo ordering

`cameo_order.py` executes original `gamemd.exe` instructions through the
[shared checked runner](../native_oracle.md). It never computes expected
comparisons or ordered lists in Python. The JSON is a Rust-consumable corpus;
its `.meta.json` identifies the selected executable, source files, Unicorn
versions, scope and supplied boundaries. No executable or disassembly packet
is distributed.

From the checkout root, select a supported original executable and run:

```sh
VERA20K_GAMEMD_EXE=/absolute/path/gamemd.exe \
  VERA20K_READY_BUILDING_ASSETS=/absolute/path/extracted-retail-cache \
  python -B -m tools.sidebar_oracle.cameo_order --check
```

`RA2_DIR` is the alternative executable selector. `--write` explicitly
generates/replaces the JSON and sidecar; `--output` selects another path.
`--help` and imports do not read the executable. Original bytes and static
addresses can be inspected with `python -m tools.native_inspect`.

## Original comparator and retained owner

`StripClass::CompareItems` (`0x6A8420..0x6A870B`) receives
`(kind1, typeIndex1, kind2, typeIndex2)`. Its only caller is
`StripClass::InsertEntry` (`0x6A8710`, call at `0x6A8742`). The original
RTTI resolver `0x48DCD0` selects initialized arrays; their order is input to
the fixture. An empty right-hand kind returns true after those lookups.

For the ordinary type entries, native ordering compares:

1. Whether each `TechnoType +0x6D0` (`AIBasePlanningSide`) equals the current
   player's `HouseType +0xBC` side. The match comes first; two nonmatches
   continue without ordering their side numbers.
2. Movement groups. Only UnitType kind 40 and AircraftType kind 3 read
   `ConsideredAircraft +0xD96` and `Naval +0xCCE`. InfantryType kind 16 and
   BuildingType kind 7 do not use those flags for this comparison. With
   mutually exclusive flags, ground comes before considered-aircraft, which
   comes before naval. This is conditional native code, not a general sort
   rank: when both flags are set, an aircraft left operand precedes a naval
   right operand before comparing the remaining keys. The corpus includes
   all such pairs, including self-comparisons.
3. Signed `TechLevel +0x634`, ascending.
4. Each type's actual virtual `+0x84` `Cost_Of` for the current player,
   ascending. Both original virtual bodies execute: `0x711F00` and the
   BuildingType override `0x45EDD0`, using the shared
   [cost owner](../spatial_oracle/cost_of.py). Raw `Cost=` alone is insufficient;
   country, FactoryPlant, and BuildingType FreeUnit adjustments can change
   the order.
5. The localized UTF-16 string at AbstractType `+0x60`, compared by original
   `wcscmp` (`0x7CA5D3`). It compares unsigned 16-bit code units, case
   sensitively; this is neither locale collation nor Unicode scalar ordering.
   `SETLE AL` at `0x6A8703` makes equality return true.

Kinds 31, 32 and 57 take the superweapon branch. Supers precede ordinary
types without the side test. Between supers, native compares the signed
SuperWeaponType `+0xB0` recharge time, then the same `+0x60` name. It does
not read the individual SuperClass's custom recharge time or readiness.

`InsertEntry` increments count, scans the existing order and its empty tail,
then inserts before the first comparator-true entry. Later entries are copied
as complete 52-byte records. Consequently a new exact tie precedes an old
tie. It initializes the new entry's factory, request state, progress value,
rate and duration to zero; starts the progress timer at the current frame;
and clears the flash end frame. Kind 7 also records the BuildingType BuildCat.
Timer clock padding is not meaningful output. No RNG or detach runs in this
insertion path.

`SidebarClass::AddCameo` (`0x6A6300`) maps kinds to four strips: ordinary
buildings to 0, Combat BuildCat 5 buildings and supers to 1, infantry to 2,
and Unit/Aircraft to 3. It rejects an exact `(kind, typeIndex)` duplicate
before insertion. An accepted non-kind-31 entry calls the construction-options
EVA when the native nesting counter at `0xA8E7AC` is zero.

`StripClass::Recalculate` (`0x6AA600`) removes failed admissions and preserves
the order and full records of surviving entries. It never calls the
comparator, `Cost_Of` or `wcscmp`. It asks type virtual `+0x94` for a factory,
then that building's owner `CanBuild(type, false, true)`; a zero answer removes
the entry. A super survives only when its index is within the player's Supers
vector and its `SuperClass +0x6D` granted byte is set. Removing a linked
production factory or a type with a primary factory can enqueue abandonment
events; those production paths are excluded from this corpus. The removal
loop resets the vacated final record and its progress timer. The original
retained histories stop after this loop. Separate `recalculate_scroll`
controls execute the complete function, including its scroll adjustment.

## Scroll adjustment after pruning

The original prologue `0x6AA624..0x6AA711` calculates the visible capacity,
allocates a temporary array and copies complete records starting at
`2 * TopRow`. Here `TopRow` is strip `+0x44`; count is `+0x54`, and records
begin at `+0x58`, stride `0x34`. With signed division truncating toward zero:

```text
footer = Scenario +0x34B8 == 0 ? 26 : 18
visible_slots = 2 * trunc0((body_height + body_y - sidebar_top - 7 - footer) / 50)
```

The geometry fields come from `0x886F9C`, `0x886F94` and `0xB0B4F8`.
Each removed `(kind, typeIndex)` also clears its matching temporary record
at `0x6AA931..0x6AA99D`. The original tail `0x6AABC9..0x6AAC76` changes
the row only if an entry was removed. Let `i` be the first surviving slot
in the old visible snapshot and `j` that same identity's index in the
new strip:

```text
candidate = old_row - trunc0((i - j) / 2)
maximum = trunc0((new_count - visible_slots) / 2)
new_row = max(0, min(candidate, maximum))
```

No surviving snapshot identity, no match in the new strip, or an empty
strip gives row zero. With no removal, the row remains unchanged. The
native clamp uses truncating division, including odd counts; it does not
use the ceiling used for the last scrollable row. The adjustment adds to
the old row: with 20 entries, six visible slots and old row 1, removing
entry 6 or 19 produces row 2. With 12 entries, ten slots and old row 1,
removing the last entry produces row 0. These results are executed goldens,
not corrections to the original behavior.

The complete controls also observe the strip/full-redraw flags, original
return value, visible snapshot before and after filtering, and temporary
array cleanup. Original `0x69DCF0` constructs the tab gadgets, as the native
static initializer `0x6A4CE0` does. This permits the empty-strip disable
path to execute. `UpdateScrollButtons 0x6A6610` and `operator_delete
0x7C8B3D` are observed sinks; button appearance and allocator internals are
outside these controls. The allocator supplies zero-filled memory, including
unused snapshot slots. All other strips are empty, so switching to another
populated tab is not covered.

## Active caller chronology

Instruction reading establishes the current-player recheck path in
`HouseClass::Update` (`0x4F9265..0x4F9302`):

- Read and clear the House `+0x1FC` recheck flag, then walk its Buildings
  vector in increasing stored index (`0x4F9283..0x4F92DB`). Skip null,
  non-live, zero-health, Selling current/queued-mission and foreign-owner
  entries. Dispatch surviving Building virtual `+0x4E0` at `0x4F92D2`.
- The original Building vtable slot `0x7E439C` contains `0x4456D0`,
  `UpdateConstructionOptions`. It additionally requires local owner,
  non-limbo, discovered-by-current-player `+0x41B` and online `+0x660`.
  According to its `BuildingType Factory +0xEB8`, it walks one of the four
  original type arrays in increasing index and calls `AddCameo` for every
  nonzero `CanBuild(false, true)` answer (`0x445736..0x44583F`).
- Only after these additions, call `RecalculateStrips` at `0x4F92E2`.
  `0x6A7D20` visits the four strips in order.
- Then call `Update_Owned_Supers` (`0x50AF10`) and
  `Grant_Provided_Supers` (`0x50B1D0`) at `0x4F92F6/0x4F92FD`.
  The grant pass walks the House Supers vector in increasing index and
  calls `AddCameo(kind31, index)` after an admitted grant.

Thus ordinary additions precede pruning within this recheck; newly granted
supers follow pruning. Their order is not established by a set of visible
IDs alone. The whole House recheck, power/discovery producers, complete
CanBuild mechanism, superweapon grants and alternate crate/trigger admission
paths are not executed by this ordering fixture.

## Recharge input identity

The constructor's original store at `0x6CE5E9` initializes SuperWeaponType
`+0xB0` to 4500 frames. The reader at `0x6CED5C..0x6CED95` calls native
`ReadDouble` for exact-case `RechargeTime` with default 0.0. If the returned
value is nonzero, it multiplies by original 900.0 and calls `Math::ftol`
before replacing `+0xB0`. A missing or zero value keeps the previous field.
The corpus executes those stores/readers and retained histories, including
`0.01` becoming 8 frames under the game's FPCW `0x0E7F`. It supplies the
INI cache lookup; it does not execute physical file loading or complete
Rules layer processing. Malformed floating strings and their stale-stack
scan result are deliberately outside this corpus.

## Retained lifecycle and Rust ownership

Instruction reading also establishes save/load lifetime. `MouseClass::Save`
(`0x5BE6D0`, stream write `0x5BE715..0x5BE71C`) writes the entire `0x556C`
display object, including the four strips at `+0x1544`, stride `0xF94`.
`MouseClass::Load` (`0x5BDF70`, read `0x5BE089..0x5BE092`) restores those
bytes. `Tab NoInit 0x5BE9B0 -> Sidebar NoInit 0x6A4F20` resets only progress
fields through `0x6AC7C0`; identities, counts and order remain. Subsequent
`Tab Init 0x6D03A0 -> Sidebar Init 0x6A5310 -> Strip InitSelectZones 0x6A8220`
rebuilds geometry without clearing records. The local House pointer is read
and swizzled separately at `0x67F9F3..0x67FA0E`.

A new scenario clears the records through `ClearScene 0x6851F0` (call at
`0x685573`) -> `Mouse InitClear 0x5BDF50` -> `Sidebar InitClear 0x6A5030`
-> `Strip InitClear 0x6A81B0`. Save/load control flow here is established
by original instructions; the executable corpus above does not emulate
whole save streams.

Rust keeps ordered identities and each tab's scroll row in `ui::sidebar::cameo_order::CameoStrips`,
retained by the existing app sidebar projection. Canonical rule types,
`Simulation::cost_of`, `SuperWeaponType::recharge_time_frames` and the CSF
owner supply comparison fields. View construction resolves those identities
into presentation entries; rendering, tooltips, scrolling and hit testing
consume that same view. Rules/House changes refresh fields without sorting
survivors. Removal updates each retained row using the original snapshot-slot
adjustment; switching tabs selects that row without copying it to a second
active/parked owner. The snapshot envelope stores owner, ordered identities
and scroll rows;
load preparation validates them against the restored interner/rules and
rebuilds derived fields before committing. A new scenario clears the owner.

Admission remains with the existing production and superweapon owners.
The existing `production_tech::sidebar_state` residual around House `+0x1FC`,
factory power and discovery is not fixed by this ordering mechanism. Newly
admitted ordinary types currently follow the production roster's iteration;
full cross-factory admission scheduling, alternate grants and linked-factory
removal/cancellation need their own native production integration evidence.

## Corpus and limits

- `comparisons`: 33 declared types, 1,089 original pair comparisons. Each
  row records the boolean, original cost-call count and whether the name
  comparison was reached. Cases separate same-side preference, category
  flag gating/overlap, signed values, supers, exact ties and UTF-16 order.
- `cost_contexts`: four factor contexts over six types, 24 original effective
  costs and 144 comparisons. All five factor slots and a FreeUnit building
  execute through the existing cost owner.
- `histories`: three retained four-strip histories. They cover registration
  order, reverse/mixed admissions, duplicates, a FactoryPlant change without
  resorting, removal/reinsertion, super grant loss, and original strip reset.
  Effective native costs before and after the factor change are included.
- `capacity_history`: the original gate rejects count greater than 75.
  However, the constructor initializes only 75 entries in the `0xF94`-byte
  strip. Native admits the 76th record into adjacent storage. This fixture
  deliberately observes that admission, the following rejection at 76,
  pruning to 75 and a later accepted retry; it excludes adjacent-state
  integrity and UI execution. A safe Rust vector can preserve the admission
  behavior without reproducing native memory corruption. Pruning first
  would admit a new cameo a recheck earlier than the original sequence.
- `recharge_reader`: original constructor field initialization and ten
  selected retained reader calls.
- `recalculate_scroll`: complete original Recalculate calls over before,
  within and after viewport removals, multiple removals, all visible entries
  lost, all entries lost, odd/even counts, first/last scrollable rows and
  two/four/six/ten-slot viewports. Nonzero origins, both nonzero scenario
  sides, division boundaries and a zero-slot viewport are included.
  Inputs name unavailable IDs; all capacities, snapshots, final orders and
  row results come from original execution.

The whole comparator, RTTI resolver, type cost virtuals, name comparison,
insertion, strip constructor and clear execute unchanged. AddCameo stops
after insertion (`0x6A6423`) or before its rejection epilogue (`0x6A65FF`).
Recalculate starts at its entry. Retained/capacity histories stop after the
full filtering loop (`0x6AAAB3`); `recalculate_scroll` executes through its
return, including the original scroll arithmetic. Its visible-copy allocator
and availability queries are declared fixture boundaries, and EVA is an
observed sink. Original instructions are never replaced. These are bounded
ordering/scroll comparisons, not whole-sidebar or whole-engine parity.

## Paid ready-building retained placement

The additive `ready_building_histories` family uses the existing physical
[building reader](../spatial_oracle/building_construction.py),
[joined scene](../spatial_oracle/building_death_anims.py) and
[Engineer admission/audio VM](../spatial_oracle/engineer_repair_admission.py).
It keeps the earlier seven top-level payloads unchanged. The selected executable
is the original active-retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
No placement result, admission predicate or RNG result is computed in Python.

`VERA20K_READY_BUILDING_ASSETS` selects the extracted retail cache already
required by the joined owner. This family additionally needs `GGPILL.SHP`,
`PILLICON.SHP` and `GAPILLMK.SHP`. Obtain them through the existing asset
extractor; do not substitute authored images. The corpus records cached-file
hashes, selected ART sections, original sound names and physical RULESMD,
optional LANGRULE, Battle-mode and Hills-map layers. The cache is a declared
file-service input: these controls do not prove complete mounted MIX or
theater search precedence. Retail bytes are not distributed here.

The initializer executes original GACNST/GAPILE `0x43B740` constructors,
then reuses the joined owner's explicit admitted preplaced building/House/map
priors. The healthy yard is at cell `(14,14)` and the barracks at `(18,14)`.
House technology 10, initial credits 10000 and flat-clear cells are declared
inputs. Original Mark/Logic admission, House interface constructor region
`0x4F5DCC..0x4F5E02`, BuildingBuilt counter constructor `0x49F9B0` at
House `+0x55A0`, registry insertion and skirmish store execute. Whole preplaced
map loading and the whole House constructor are outside this initializer.
The original House constructor registration `0x4F5EB8..0x4F5F0B` inserts the
House into Factory-expiry listener vector `0xB0F640`. This is required by the
actual Factory scalar destructor's broadcast; other House constructor and
listener registration regions retain the declared bounds.

Whole BuildingType constructors/readers and original registration tail
`0x45E2ED..0x45E362` establish fixture GACNST/GAPOWR/GAPILE/GAPILL type
indices. `GUIBuildSound` (`0x6693A6..0x6693F1`) and `BuildingSlam`
(`0x66AA7F..0x66AACF`) execute original `ReadString 0x528A10` and
`VocClass::FindIndex 0x7514D0`, retaining constructor/current defaults over
applicable layers. Sound indices and pointers are fixture-relative; names
come from original unlayered SOUNDMD.

Original Factory, selection, foundation, Select-control and empty-cell CRT
owners initialize services. Display `0x4A8738..0x4A8847`, MouseClass
`0x5BDA48..0x5BDA74` and WWMouse `0x7B8730..0x7B8856` execute selected
constructor regions. The last stops before opaque platform wait/window calls.
Physical mouse `(320,240)`, selection capacity 64 and WinMM `timeGetTime=0`
are explicit transport inputs. Existing allocator, physical file, asset-cache,
DirectSound and presentation boundaries remain with the joined owner.

The first history creates and pays for GAPOWR and separate Defense-category
GAPILL factories through whole `House::BeginProduction 0x4FA350` and
`Factory::Start 0x4C9C70`. Whole `Factory::AI 0x4C9B20` visits increasing
frames until native stage 54. Ready stage, balance and product identity are
never supplied. Subsequent controls restore this executed VM/CPU/transport
baseline. Main and Scenario use original seed 2; MapGen uses original seed 31.

### Admission and repeat guards

Whole `SelectAction 0x6AAD00` queries the actual held product through
`ObjectFindFactory 0x5F5C20 -> TypeFindFactory 0x5F7900`, then enters
`ManualPlace 0x4FB840`. Both same-type and different-type ready-building
repeats refuse at the nonnull current-type guard `0x4FB852..0x4FB859`.
GUI sound and builder query precede that guard. Only fresh admission calls
`UnselectAll 0x48DC90`; repeated refusals retain selection and follow state.
The selected/follow prior uses whole `Object::Select 0x5F4520` and
`0x4AEB30`, with explicitly supplied prior navigation mode/scope.

Actual `ObjectFindFactory(0,0)` requires an eligible local builder; its common
TypeFindFactory owner skips Limbo, Selling current/queued missions, wrong
Factory, ownable and naval fields. It has no health test in these arguments.
Missing-builder cameo branch `0x6AB32A` precedes ManualPlace, emitting
Abandon and unavailable EVA even when a cursor is active. Builder-Limbo
controls explicitly mutate that input byte; they do not execute the Limbo
producer.

`ManualPlace` reads builder coordinate virtual `+0x1B8` (`0x41BEA0`): signed
leptons divided by 256 toward zero produce two signed 16-bit map-cell
coordinates. `0x4A91B0` changes the foundation preview anchor/offset and
foundation pointer; it does not move the tactical camera. Issuer `0x880994`
is House `+0x30` ArrayIndex, stored at `0x4FB8C7`; `0x880980` is its saved
counterpart. Neither field is a builder pointer. The receiver queries the
builder again.

### Producer, receiver and saved client order

Whole tactical left/right routines (`0x4AB9B0`, `0x4AAD30`), original
111-byte Place-event constructor `0x4C6AE0` and local event corridor
`0x6474C7..0x6474BF -> 0x64C380 -> 0x4C6CB0 -> 0x4FB0E0` execute.
OutList count/head/tail and event bytes are observed. Invalid local preview
retains the cursor without an event; an admitted gesture saves actual current
client product/type/issuer and clears the active cursor.

`House::PlaceObject 0x4FB0E0` requires the category factory, IsComplete,
nonnull held product and eligible builder before HELLO and whole Building
Unlimbo `0x440580`. Early cell guards compare original empty-cell sentinel
`(0,0)` and `(-1,-1)` at `0x4FB197..0x4FB1BE`. They reach alternate
`0x4FB520` and `ObjectFindFactory(0,1)`, returning false for these ordinary
building controls. Missing/incomplete factory, missing held product, missing
builder and sentinel cases do not perform saved-client retry. The incomplete
control uses a real new factory without AI visits, not a supplied stage.

Only an actual false Unlimbo result reaches `0x4FB35F` and restores the
latest saved client at `0x4FB395`. Success unconditionally clears saved
product/type/foundation at `0x4FB328..0x4FB333`. Both orders of two locally
admitted same-site GAPOWR/GAPILL events execute: the first succeeds and
clears the backup; the second's actual Unlimbo failure restores null. With
two failures, the first event restores the second event's saved product and
clears it; the second restores null. A retry carrying only the event's type
would invent a cursor in these controls.

The single-failure witness admits a gesture, then executes whole Building
constructor `0x43B740` and Unlimbo `0x440580` for an independent stationary
GAPILL at that site before receiving the queued command. This direct caller
is a declared input boundary, not a whole trigger or map-loader proof. Failed
placement restores the actual saved client and recomputes its preview from
the supplied physical mouse. Failure-then-Abandon and both same-site event
orders retain original receiver chronology.

Tactical right-click requires a nonnull current product at `0x4AAD34` and
AbstractFlags `+0x14` bit 0 at `0x4AAD3E..0x4AAD47` before clearing the cursor.
It preserves the paid product. A null current product leaves orphan type and
foundation intact. Own-factory cameo Right requires its retained Factory link
at `0x6AADC1`. Its prefix clears the active cursor only when current product
casts to Building (`0x40DD70`); the clear precedes GUI sound, EVA and Abandon.

`House::Abandon 0x4FAA10` first requires the category factory. A nonnull held
product must match the requested index in these controls. Null held product
with nonzero requested BuildCat admits through `0x4FABF6 -> 0x4FAB64` without
that match; the direct GACNST BuildCat 0 control refuses while GAPILE's
nonzero mismatched index and GAPILL's Defense category admit. Admitted local
building cancellation calls SidebarAbandon before its cursor prefix. Each
matching link calls FactoryAbandon at `0x6ABBDA` before unlink stores
`0x6ABBDF/0x6ABBE7`, so linked product refund/destructor/expiry precede the
House cursor clear. With no matching link, the House cursor prefix instead
precedes the core FactoryAbandon at `0x4FABA6`. The Factory is removed when
its queue is empty.
Null-held `Factory::Abandon 0x4C9FF0` immediately returns false without refund,
timer write or RNG draw. Nonnull cancellation reaches the original refund,
destructor and detach calls, without claiming every fatal path.

### Retained Factory associations and queue recheck

Each actual strip record begins at strip `+0x58` with type index and kind.
Record `+0x08` (strip `+0x60`) is BuildCat metadata; record `+0x0C` (strip
`+0x64`) is its Factory link. `BeginProduction 0x4FA6AA -> 0x6A6140` writes
only the matched entry's link at `0x6A61FA`. It does not clear older entries.
The executed null-held GAPOWR-to-GAPILE rearm reuses the same Factory and
preserves construction-array `0xA83E30` order. Both old GAPOWR and new GAPILE
cameos reference that live Factory. Right on the old GAPOWR can clear active
GAPILL before its requested-type mismatch is refused by the receiver.
Left on old GAPOWR instead executes `0x6AB2CC` GetObject and uses the actual
held GAPILE product for ObjectFindFactory and ManualPlace. The executed
`expired_product_same_category_rearm_retained_old_left` history previews,
saves and places GAPILE through the real tactical producer and local receiver;
the old requested cameo type does not become the current/saved placement type.

The drawing consumer is established by original instructions, rather than
executed drawing: `StripDraw 0x6A9540` loads each record's link at `0x6A9812`,
calls `IsComplete 0x4CA130` at `0x6A9822`, and resolves `TXT_READY` at
`0x6A983B`. It reads the same linked Factory's stage at `0x6A985D` and its
rate/suspension for `TXT_HOLD` at `0x6A9E9C..0x6A9EB0`; complete skips both
clock and Hold. The old cameo retains its own icon/name, while queue badges
separately query the House Factory for that requested type at
`0x6A9C48..0x6A9CC3`. Rust projects these drawing fields from the same
registry-validated references input reads, and keeps queue counts separate.
Full rendered native equivalence is not demonstrated by these instructions.
`IsComplete 0x4CA130` itself requires exactly stage54 and a nonnull held
product (or the dormant special-item arm); rate/suspension are caller guards.
Select checks them before IsComplete, whereas Draw checks completion first.

`HouseAbandon -> 0x6ABAD0 -> 0x6ABB60` clears all same-Factory-linked records
in the category strip at `0x6ABBDF` and their request state at `0x6ABBE7`.

The additive `rearmed_factory_sidebar_abandon_unlinks_all_retains_factory`
control isolates that owner while the Factory survives: original GAPOWR
product expiry and actual paid GAPILE rearm create two retained associations
through `0x6A6140`, then direct whole `0x6ABAD0`/`0x6ABB60` clears both links.
The target Factory keeps its live registry identity/order; the independent
GAPILL Factory/link remain unchanged. Original FactoryAbandon refunds GAPILE
once, writes its stage clock and destroys the limbo product; the second
matching-entry call has no held object. Full three RNG states, zero draws and
advances, timer writes and the reached destructor/expiry trace are recorded.
This is an initialized direct owner control, not whole House/mobile delivery
execution. Local HouseAbandon calls it at `0x4FAB74` before the building-only
cursor prefix and before successor promotion at `0x4FAC96`; queued-copy-only
removal bypasses it. Original mobile Place completion at `0x4FB649` reaches
HouseAbandon at `0x4FB663`; terminal nonbuilding Exit refusal reaches it at
`0x4FB5B5`. Those mobile caller bounds are instruction-supported.

The additive `local_unlinked_strip_clear_active_abandon_order` history uses
actual paid GAPOWR/GAPILL and real Select, then executes whole original
`StripClear 0x6A81B0` on tab0 before whole HouseAbandon. StripClear removes
that strip's records while leaving both live factories and the independent
Defense link intact. This is a declared direct initialized helper boundary,
not whole UI/scenario reset. At the zero-link House prefix `0x4FAB83`, current
GAPOWR product/type/foundation remain; the first product expiry `0x7258D0`
sees them cleared. Existing linked cancellation instead retains current
product/type/foundation at its first product expiry; Tactical expiry clears
only the product before the House prefix clears type/foundation. Full three
RNG states, zero abandon draws/advances, three Factory-clock stores and
reached lifecycle callbacks are recorded. StripClear's 75 slot clocks are
outside the existing stage/building/action timer-watch windows; no whole
reset/timer coverage is claimed. Raw EAX and timer padding are observations.

Link publication is an actual reached call, not a successful Begin return.
Instruction-level guards show no-builder, allocation failure, active kind-7
refusal and Start's false return bypass `0x4FA6AA`. A successful queued tail
with nonempty queue, flag argument false and no same-type shortcut returns
through `0x4FA5F8 -> 0x6A60A0`, without linking. Fresh/rearmed starts and the
same suspended held-type shortcut continue through `0x4FA61E`; only the local
House then calls `0x6A6140`. Fresh and rearmed calls execute in this corpus.
The queued-tail branch is instruction-supported; no building queue is forged
to reach it.

Whole `House::UpdateFactoryQueue 0x509140` executes at a declared direct
helper boundary with `(factory RTTI 7, naval false, BuildCat 0)` and separate
helper-only Defense-5 controls. Selected active stock GACNST Unlimbo, Limbo,
GoOnline/Offline, ChangeOwner and scenario-building callers push category 0;
their whole lifecycle and scheduling are not executed here. Supplied yard
Limbo makes the real builder query refuse. `0x50921C` calls
`FactoryAbandon 0x4C9FF0` directly, then `StartNextQueued 0x4CA5A0`; it has
no HouseAbandon client prefix. Empty final Factory cleanup at `0x5092A0`
executes scalar deletion `0x4CA770`, whose broadcast calls registered House
`0x4FB9B0`, clears the matching House slot, and removes the Factory from
the original live Factory and object-expiry arrays. Current type/foundation
remain orphaned while product expiry clears current/saved product as observed.
The raw cameo link can remain after this direct helper; snapshots mark it
`factory_live: false` and do not dereference it. Freed-pointer Right,
subsequent frame/sidebar recheck and every-frame queue scheduling are outside
these controls.

### Pointer expiry and input limits

Whole `DispatchPointerExpired 0x7258D0` executes against initialized services,
including Factory listener `0x4CA580` and Tactical listener `0x6DA560`.
Factory's exact-match store at `0x4CA58B` clears only held product `+0x58`;
stage 54 is retained. Current-product expiry clears only current product. It
retains current type/foundation/issuer and a following tactical gesture emits
no event. Current-type expiry clears current type and foundation while
retaining product and issuer. That direct unload-boundary control does not
attempt a gesture with null type and nonnull product.

Tactical compares saved product, not saved type. Saved-product expiry clears
saved product/type/issuer and calls `0x4A8D50(0)` for saved foundation;
saved-type expiry alone retains backup. Explicit Alive=false still allows
the local producer to save actual product; Alive is not pointer expiry.
Whole fatal death, every destructor class and scenario/load producers remain
outside these controls.

Escape is instruction-supported separately from this corpus. Main's chat
corridor (`0x55DEE0 -> 0x55E420 -> 0x5D4510`) can consume it; registered
Options `0x5372D0 -> 0x647040` and fallback `0x55E079` open Options.
Options Event `0x4C7929` sets game state before
`0x48C8B0 -> 0x683EB0 -> 0x6938C0 -> 0x4AEAD0` reaches platform/menu
handling. The local instruction route does not establish placement
cancellation. Full modal platform, resume, save and loading are excluded;
no executable Escape golden is asserted.

### New family schema

The 35 histories contain 161 steps. Each row has stable `id`, explicit `inputs`
and ordered `steps`. `paid_stock_ready` carries reader layers, hashes, type
indices, physical ART/sound inputs, initializer boundaries and seeds. Other
rows identify that executed baseline and supplied prior changes. Each step
has `command`, `inputs`, raw native `result`, `before` and `after`,
`native_calls`, `timer_writes`, `os_transport`, `rng_requests`,
`rng_advances`, `lifecycle_trace`, `runtime_arena_events` and
`native_code_unchanged`.

Snapshots include current/saved product, type, issuer, foundation and map-cell
preview; selected/follow/navigation state; credits; actual object/factory
states; action/mission/stage timers; command-ring bytes; and three complete
RNG states. `factory_registry` records original ordered live Factory identities;
`sidebar_entries` records actual record index/kind/category, Factory pointer,
live membership and live referenced stage/product/rate/suspended state.
Native calls include entry arguments, reached returns and placement
snapshots at original guard/retry boundaries. Timer padding, raw EAX for void
routines and pointers are observations rather than invented semantic outputs.
These are bounded ordinary retail paid-placement controls, not whole GUI
startup, full NativeMainTick, audible hardware output, complete save/load or
target-scale determinism proof.
