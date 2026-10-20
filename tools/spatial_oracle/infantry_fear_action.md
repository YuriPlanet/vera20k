# Original Infantry fear and stance requests

The selected `--fear` mode belongs to
[`infantry_movement_action.py`](infantry_movement_action.py). It reuses that
owner's Infantry/type/sequence/Walk machine. The default 198 rows remain
byte-identical: file SHA256
`acbab5fc41b7791309fa5d72835f7941ed8abcece1f093147b4edbdf26318ffb`,
canonical payload SHA256
`2f40b3be270bdce0070d59ac8efb6c02ab89a6ba68ad82d8ba93dada141cab27`.

Run from the checkout with the native-oracle environment configured:

```sh
python -m tools.spatial_oracle.infantry_movement_action --check
python -m tools.spatial_oracle.infantry_movement_action --fear --check
```

`--fear --write` deliberately regenerates only `infantry_fear_action.json`
and its provenance sidecar. It does not replace the default corpus.

## Native evidence

All 46 selected controls enter unchanged `Infantry5200B0`; 45 return and one
stops at the declared pre-Scatter boundary. Their
actual class requests run original `51D6F0`; original House `50B730`, COM Walk
`75AB30` and, for three controls, original `IsArmed701120` execute. One
Fraidycat row stops before the original Scatter entry rather than supplying its
answer. Original text and Infantry/Walk vtables are saved and checked unchanged.

The controls distinguish:

- Positive fear decay and the 50 stance threshold; initial zero fear does not
  ask a prone actor to get Up.
- Fearless skipping only decay while still allowing stance requests.
- Accepted/refused Up and Down, absent sequences, Crawls admission, retained
  clocks/prone on refusal, actual Doing27..30 exclusion and Undeploy31's
  separate class-action refusal.
- Human/PlayerControl/Session-mode admission, independent NavCom and actual
  Walk moving byte. A Rust movement order is not the native motion query.
- Fraidycat's strict fear greater than50, Doing/falling/motion/NavCom gates,
  and actual pushed `NullCoordA8F200, forced1, noKidding0` at `51D0D0`.
- Fear reaching0 with live Ammo0: no refill when unarmed; actual type-ammo
  refill when armed, including the unlimited value-1.

Native field writes and class call/return order are retained. All three complete
0x3F4 generators are seeded31 by original `65C6D0`; all before/after states are
unchanged and no observed Random/RandomRanged entry occurs in these bounded
controls. Up/Down stage restart is `(stage0, frame100, duration1, rate1)`,
retaining independent changed1/increment3. A refused Up leaves supplied
`(stage7, frame17, duration91, rate92, prone1)` intact.

## Production connection and bounds

[`Simulation::infantry_fear_turn`](../../src/sim/infantry.rs) is the sole
actor-local fear/stance decision owner. InfantryAI calls it at the original
`51BF0B` point after Foot Process, before FireAtTarget `51BF59` and sequencer
`51BF6A`. Retained death actions also receive fear decay; neither an adapter's
dying/deploy flag nor an animation substitutes for actual Doing. Requests use
the existing `infantry_do_action` owner; accepted actions own prone and the
private StageClass. Fraidycat calls the existing NULL Scatter dispatcher and
returns its immediate Process bridge-state-change result to the host.

The Rust tests `native_fear_up_refusal_retains_prone`,
`native_fear_receiver_matches_action_admission_clock_and_full_rng` and
`native_fear_down_uses_actual_walk_motion_not_movement_order` compare the
42 full unlimited-ammo controls through that production owner. The first was
run against the old helper before migration and failed: Rust cleared prone,
while native retained it. Exact Rust validation results belong to the owner
run; a saved native corpus alone is not a Rust pass or whole-mechanism parity.

`native_retail_e1_fear_inputs_and_receiver_match_ordinary_controls` reads
physical RULES/ART through the production owners, checks E1's relevant flags,
unlimited-ammo and Walk inputs, then compares 18 selected controls. The native
supplied frame count6 and positive physical Up/Down counts have the same
admission predicate at `51D70F`; their different counts do not prove sequence
advancement equivalence.

Infantry/type/House construction, complete retail loading, absolute stage
advancement, full InfantryAI/fire/projectile/world interleaving and persistence
are outside these selected controls. All42 sequence counts are supplied6
except explicit absent controls. Type flags, current Doing/prone/fear, falling,
House bytes, Session mode and the actual Walk+34 byte are declared prior-state
inputs, not producer proofs. Undeploy/current-Up/absent-action and altered
Fearless/Fraidycat controls do not claim ordinary stock E1 lifecycle reachability.
Weapon-slot presence in the three IsArmed controls is a comparison-only supplied
pointer; weapon reader/gameplay is excluded. No executable or gameplay result
is replaced.

The following required larger mechanisms remain open:

- **Ground TechnoAmmo:** `5200E1..520105` needs the live `Techno+2FC` owner and
  its initialization, fire/reload/persistence lifecycle. Current aircraft ammo
  storage is not a ground Infantry count. Stock E1 uses unlimited ammo; the
  receiver records this omission and does not infer live ammo from TypeAmmo or
  introduce an invented failure. The three executed zero-ammo controls are
  retained prerequisites for the shared ammo migration.
- **Complete Fraidycat transaction:** the boundary row establishes the fear
  caller's request, not the Scatter body, Scenario draws, destination/Process,
  bridge effects or enclosing return. Those use the existing Scatter owners
  and their separate native comparisons. This corpus does not certify their
  complete connection through fear.
- **Connected firing/Airstrike work:** no direct Airstrike operation occurs
  inside `5200B0`. The
  following InfantryAI `51BF10..51BF50` tests NavCom/prone and `+6DA`, reads
  `+6C8/+6D0` against absolute frame and may clear `+6DA`; then `5206B0` runs
  at `51BF59`. This receipt establishes those offsets and order without
  assigning an unproved identity to `+6DA`. Fire, designation and any
  Airstrike dependencies belong to their existing consumer owners; they are
  not invented parts of this fear receiver.

Timer+104 is copied stack auxiliary, excluded from logical timer comparison.
The full three RNG states are boundary evidence; no whole-match scheduling or
retail rendered bridge equivalence is claimed.
