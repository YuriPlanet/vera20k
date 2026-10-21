# AI sale after Engineer capture

`captured_building_sale.py` executes the original capture-to-sale handoff using
the existing `EngineerJoinedFixture`. The reference ends when capture returns
with the original sale order admitted or refused. The existing
[`building_sale.py`](building_sale.py) corpus owns later Selling timing,
refund and destruction; this fixture does not claim a complete native match.

## Reproduction

Use the environment in [the native oracle guide](../native_oracle.md), the
supported original executable, and the exact flat retail inputs listed in
[`engineer_repair_joined.md`](engineer_repair_joined.md). No proprietary files
are included. Set `VERA20K_CAPTURED_BUILDING_SALE_ASSETS` to that directory:

```sh
python -m tools.spatial_oracle.captured_building_sale --check
```

`--check` and the default mode compare without writing. `--write` deliberately
records new original executions and their provenance. Import and `--help` do
not open the executable or retail inputs. The payload records retail input
hashes and reader-derived type values; the sidecar records the payload hash
and inherited platform boundaries. The shared publisher checks source
stability during generation without persisting repository-source hashes.

## Joined and cropped boundaries

The six joined histories use original Building constructor `43B740`, including
Techno constructor's `6F3241` store of its House argument into `Techno+14C`.
The shared already-admitted damaged `GAPOWR` map prior follows construction;
it supplies physical placement, health and House bookkeeping. This is not an
additional claim of complete native Building Unlimbo/startup.

The supplied issued-order boundary is Capture mission 8 with both Target and
Destination naming the building. Original `6FFBE0`, event delivery `4C6CB0`
and `5B3570` Commence execute. This is the input shape produced by Engineer
Hunt's capture arms at `4D53BA..4D53F4` and `4D548F..4D54B9`; a human convenience
order with a null Target is not the same AI input. The fixture does not run
autonomous Hunt or a complete AI Team script.

Walk starts 16 leptons before an already-paid head, as in the existing
Engineer comparison. Original Walk `75AEC0`, Infantry PerCell `519630`,
Building ChangeOwner `448260` and Techno ChangeOwner `7014A0` run unchanged.
The histories cover computer capture, human capture, campaign, recapture by
the constructor owner, passive constructor owner, and a third current owner.
Recapture distinguishes the constructor-owner gate from a last-owner gate;
the third-owner control retains another distinct ownership history. Every row
observes that the original constructor field remains unchanged during this chain.

The seven tail controls enter `4492D7` and stop at `44936E`, with ESI naming
the building, EBP naming its new owner, and the current-owner field already
transferred. This explicit boundary isolates C4, missing Buildup, existing
Selling, local-player versus human control, and upgrade refunds. It does not
repeat upstream ownership bookkeeping.

The original predicate skips campaign mode 0, a human new owner, the same
constructor owner, or a passive constructor owner's Country. On admission,
positive signed `UpgradeLevel` values select the top installed type. Original
`GetRefund(new_owner, true)` at `711F60`, `AddCredits` at `4F9950` and
`RemoveLastUpgrade` at `451690` run before the next upgrade and before sale.
Installed GAPOWR/GAPILE slots are explicit synthetic controls using the
original reader's type prices, not a claim that stock power plants accept
those upgrades.

The tail calls virtual `Sell_Back(1)` at `449368`. Literal 1 is the computer
sale variant: `447110` still refuses an already-Selling building or planted
C4, and an ordinary building without Buildup. Upgrade refunds happen before
those refusals. There is no temporary upgrade object construction in this
tail. Calls, credits, slots, mission state, ID allocation and RNG continuation
are observed from the original execution, not calculated expected outputs.

Only observation hooks are added. The inherited fixture supplies its
documented file, allocator, platform and presentation transport. All selected
capture, refund, removal and mission bodies and class virtual dispatch remain
original. Complete executable sections and the selected Building/Infantry
vtables must remain unchanged.

## Initial-owner lifecycle evidence

The supported executable's whole-image literal-displacement census for the
four-byte region at offset `14C` returned 179 candidates. Static class/caller
inspection identifies the Techno constructor store at `6F3241`; the other
write candidates belong to Sound, Bullet, Convert/blitter, House, Country,
Map, Warhead, Wave, Weapon types, or stack locals. This is a literal scan,
not proof that alias-based or bulk writes cannot exist.

Persistence separately supports retaining the field. Techno Save `70C250`
delegates to Radio Save `65AC40`, which calls Abstract Save `410320`: the
original writes the receiver token and its virtual-size raw object block.
Techno Load `70BF50` subsequently passes the address of `this+14C` to the
Swizzle manager at `70BFA0..70BFAC`. This is static save/load evidence, not a
new executed original save-game round trip.

The inspected Techno PointerExpired body `7077C0` clears matching current
owner at `707A86..707A94`; it does not clear the constructor-owner field.
House destruction dispatches pointer expiry before teardown. These observations
do not establish arbitrary House destruction during a live match, and the
fixture keeps every referenced House alive. Initial owner is not the separate
captured InfantryType index at Techno `+338` or mind-control ownership state.

## Stock reachability and Rust comparisons

Physical AIMD defines the Soviet Engineer teams `0C143EFC-G` and `06BC2DEC-G`,
TaskForce `08A2824C-G` with one HTK and five SENGINEER, and capture scripts
`0A88524C-G`/`0A8856FC-G`. AI triggers `0D53430C-G`, `0ECCFCAC-G`, `0ECCFA7C-G`
and `0C8EEEDC-G` target Allied/Soviet construction yards at medium/hard
difficulty. This is static stock reachability. The smaller GAPOWR test shares
the capture/sale mechanism; it does not claim to replay those scripts' yard
target selection or the transport/team rollout.

Rust tests
`captured_building_sale_matches_original_joined_owner_transitions` and
`captured_building_sale_matches_original_tail_gates_and_upgrade_refunds` in
[`captured_building_sale_tests.rs`](../../src/sim/production/captured_building_sale_tests.rs)
consume the compact state through Building ChangeOwner and the post-owner-change
sale helper, respectively.

The separate physical-rules tests
`stock_computer_capture_walks_then_sells_and_refunds_the_new_owner` and
`stock_computer_recapture_after_load_keeps_its_original_building` in
[`ai_captured_building_sale_tests.rs`](../../src/sim/world/ai_captured_building_sale_tests.rs)
supply the ordinary AI order and exercise Rust movement, capture, Selling and
removal, or retained constructor ownership across save/load. Neither Rust
continuation is described as full native travel or native save/load execution.

A manual gameplay check can let a computer-controlled Engineer capture an
enemy construction yard in skirmish and observe the captured building start
selling. Human capture, a neutral tech structure and recapture by the original
owner provide contrasting controls. This interactive check has not been run
as part of the corpus.
