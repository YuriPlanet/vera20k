# House economic recovery predicate

Run `python -m tools.spatial_oracle.house_can_make_money --check` with
`VERA20K_GAMEMD_EXE` pointing to the local retail executable. `--write`
explicitly regenerates these outputs. No original executable bytes, identity
hashes or retail asset contents are published.

Eighty-two controls execute complete `HouseClass::Can_Make_Money` at `004F6540`
through its original RET:60 retail-shaped,11 custom-arithmetic and11 identity
controls. Three additional controls execute complete
`AI_Check_Build_Need 004FD9A0`, including its nested CanMakeMoney call.
No relevant return or original instruction is replaced. Real bodies include:

| Dependency | Original entry |
|---|---|
| Country ID/Name lookup and ASCII stricmp | `005117D0`, `007C8D20` |
| Preallocated active Unit counter | `0049FAE0` |
| Empty-storage AvailableMoney | `004F6990`, `006C9600`, `007C5F00` |
| FirstBuildableFromArray | `005051E0` |
| Building and generic type CostOf | `0045EDD0`, `00711F00` |
| Building actual cost / raw type cost | `0045ED50`, `00711EB0` |
| Country / FactoryPlant factor getters | `0050BDF0`, `0050BEB0` |

Type vtables are original Building `007E4570`, Unit `007F6218` and Aircraft
`007E2868`. CostOf and actual-cost virtual slots are checked before execution;
the IHouse money slot is original `007EA834+18`. The PadAircraft/Dock shell
is supplied even with SeparateAircraft=true: both real Building cost bodies
read the first aircraft's first Dock before checking that option. A refinery
with Cost2000/FreeUnit1400 and Unit plant factor.75 costs1650, with the
harvester priced1050; these prices come from native calls in the joined path.

`rows` contains the direct predicate controls. Each input records signed
cash/counts, the Country identity/parent/side, five raw country and plant
factor words, raw income, ordered lists and every supplied type field. Type
`array_index` is explicit within its class. The top-level country registry is
PARENT index0 and CHILD index1, with Name aliases. Owner uses ParentCountry;
RequiredHouses/ForbiddenHouses use actual Country index. Controls exercise
those distinctions, side admission and failing candidates before passing ones.

Output `al` is the actual low-byte return contract: native true paths need
not clear EAX's high bits. `can_make_money` and optional `check_build_need`
identify their respective native returns. Selected types retain the actual
FirstBuildable query sequence, rather than assuming the repeated refinery
query happened. `prices` contains returned predicate prices, or null when
the branch made no corresponding cost call. Full nested call/return records
include original PCs, supplied arguments, receiver addresses and EAX receipts.
EAX from x87 factor-returning helpers is an incidental receipt, not a float
value. Raw factors remain explicit inputs.

The direct threshold controls distinguish the strict no-destination test
`money > refinery_cost` at `004F680F..004F6811`, inclusive harvester and
Yuri-shaped no-HarvesterUnit recovery thresholds, yard versus active MCV,
and destination+gatherer early success. Industrial Plant controls execute
the FreeUnit cost split and the actual factor product. `custom_arithmetic`
rows explicitly cover missing buildable refinery, signed price-add wrap,
negative active-MCV/destination counters and nonneutral country factors;
these are component-domain controls, not claims of stock occurrence.

The three `check_build_need_rows` have human flags0, game mode1, no resource
destination, choices-1, an empty Factory registry and a valid two-entry
refinery list. At money1999/2000/2001 the native predicate is respectively
false/false/true while native recovery admission is true/false/false. This
separately observes the equality point that CheckBuildNeed alone would hide.
No held-factory balance-owed or strategic sale/rebuild history is executed.

Every measured call preserves the whole supplied arena, including all House,
Country, Rules, type, counter and list bytes. The active counter has capacity8
and an in-range BaseUnit index; the query never grows it. Yard-vector lengths
are0/1. Original `0065C6D0` seeds complete1012-byte Main, Scenario and MapGen
buffers before measurement. All three buffers, the action-timer sentinel,
registry/locale globals, x87 control `0E7F`, stack cleanup and every executable
PE section are checked. Counter growth, locale slow path, allocation or a
random draw causes failure rather than a supplied return.

These are data-shaped component controls. Synthetic names, raw masks,
whole-count priors and prices are supplied without native House/Rules/type
constructors or readers, physical INI/MIX loading, world admission or a save
history. `retail_shaped` identifies ordinary2000/1400/FreeUnit and.75-plant
shapes, not actual retail type discovery. Existing separate country-reader
and cost corpora establish their own readers/arithmetic; this corpus does
not claim a new actual-retail identity/type-price reader join. Nonempty House
storage, invalid null BaseUnit/war-factory lists, growing counters, other
locales and whole AI/scheduler parity remain outside its evidence.

The Rust consumers in
[`ai_can_make_money_oracle_tests.rs`](../../src/sim/ai_can_make_money_oracle_tests.rs)
build the supplied types and country layers through `IniFile` and `RuleSet`,
project the raw country factors onto a House, and admit ordinary counts through
actual object spawns. A live FactoryPlant supplies the nonneutral plant product.
Only explicitly labeled custom counter priors use test setters. The comparisons
check the selected types, queried prices, money, predicate results and retained
simulation hash. Run them with
`cargo test --locked -p vera20k-core --lib native_money_tests`.

These controls verify the existing predicate. They do not change production
behavior or establish a complete miner-loss/replacement history. The separate
ignored stock Soviet recovery test in `ai_build_need_retail_tests.rs` exercises
that longer Rust history using ordinary lobby settings and local Move/Attack
commands. Its preparation waits for a visible original miner, a live war factory,
and a specific unfinished miner whose actual Factory debt exceeds the wallet.
Difficulty, the predicate and retained counters are then asserted; the setup
does not admit on a fixed frame or inject those states. Each history must reach
preparation within its unchanged 10,000-frame bound. The attack history requires
loss, due strategy admission, sale/abandonment, a new production identity,
harvesting, unloading and wallet income. The healthy history requires no
emergency rebuild. These are Rust world assertions, not original world/timing
goldens or a complete native recovery comparison.
