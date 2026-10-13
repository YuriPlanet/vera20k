# War Miner mission ownership

An ordinary stock `HARV` ordered to Attack must stop cutting ore, fire through
the Unit combat host, and use the Unit idle receiver after its target disappears.
On human-owned ore that receiver queues Harvest; on clear land it queues Guard.
Cargo is retained. This chain shares its mission and navigation owners with
movement, placement, capture and refinery/depot exits.

| Responsibility | Rust owner | Original identity |
| --- | --- | --- |
| Committed mission, cursor, queue, dispatch timer | `MissionCom` / mission authority | Mission `5B3060`, Queue `5B35E0`, Commence `5B3570` |
| Single Foot/Unit dispatch and timer epilogue | `world::techno_ai::mission_handlers::dispatch_foot_mission` | Unit Attack `7447A0` → Foot `4D4DC0`; Unit Harvest `73E5E0` |
| Unit idle base, selection, setters and deferred queue | `Simulation::unit_enter_idle_mode` in `movement/track_host.rs` | Foot `4D82B0` → Unit `738970` |
| Target setter and pointer-expiry hierarchy | Mission concrete effects / `world::lifecycle` | Target `6FCDB0`; Unit `7446E0` → Foot `4D9960` → Techno `7077C0` |
| Ore search, cargo and ore mutations | `miner_system`, `ore_scan`, `tiberium` | Search `4DCFE0`, ore tick `73D450` |
| Stage and harvesting latch | Entity Stage owner / Unit AI miner receiver | Techno `6FABB8..6FAC31`; Unit `7365BB..7365DF` |
| Ordered refinery selection and admission | `miner_system::find_docking_bay` → shared `radio::receive` | `4DF040`/`4DEE80` → CAN_LOAD `43C2D0` |
| Shared refinery/depot Enter | `mission::enter` with `MissionCom` cadence | Foot `4D9290`, current control `5B3A00` |
| Unit destination and per-cell docking | `movement::track_path` / `movement::per_cell` | Unit `741970` / `73A31F` |
| Unload, cargo payment and departure | `miner::refinery_dock` with shared House economy | Unit `73D630` → House `4F9610` |

The committed selector chooses one handler. A Miner component never admits
Harvest while Attack, Move or another mission is current. Harvest returns its
delay to the common dispatcher; committing its working snapshot does not write
a second timer. Unit idle calls the concrete Target(NULL) then Destination(NULL)
owners before its final queue gate. Promotion resets the shared handler cursor
and dispatch timer; changing a queued mission does not interpret an Attack
cursor as Harvest state.

Target expiry may draw Scenario range 4..8 before clearing Target, depending on
the passive timer. A due targetless Attack invokes idle before its own cadence
draw, even if idle queues Harvest. The later Unit checkpoint clears the retained
harvesting latch before firing and promotes the queue. Harvest's next dispatch
arms Stage; Techno advances Stage after dispatch, at most once per admitted visit.
These timer/RNG results come from original executable samples, not calculations
from the Rust implementation.

## Evidence and reproduction

The [native packet and reader receipts](../../../tools/spatial_oracle/harvest_attack_return.md)
save 53 original histories, including complete 250-word Scenario RNG states.
`world::harvest_attack_return_oracle_tests` compares 52 represented histories
step by step. Retained AttackMove is explicitly excluded from Rust replay.
The existing field, refinery and Chrono Miner corpora retain their own coverage.

Ten controls cover the shared non-harvester Unit idle branch. Armed Units keep
their target and burst; unarmed Units clear those fields before the Patrol/Unload
queue tail. The existing `combat_weapon::is_armed` owner resolves the weapon.
AreaGuard, Wait and Unload with a resolved DeploysInto retain their native early
returns. These controls execute original idle and predicate bodies on declared
scalar/pointer inputs; they do not establish native movement or combat.

`world::harvest_field_cycle_tests::retail_war_miner_attacks_without_cutting_ore_and_resumes_its_idle_mission`
loads physical RULESMD, fixed ARTMD, Battle and AnyTown INIs through the production
reader. It connects ordinary Attack to firing, damage, target removal and renewed
ore collection, and checks the clear-land Guard return. Its terrain/refineries
are the existing field fixture; it is Rust integration coverage.

The [ordinary map profile](../../../tools/map_observation.war-miner-attack.example.json)
deploys an MCV, builds power and a refinery, obtains its free War Miner, then
ForceAttacks a stationary friendly conscript. It observes real terrain and
actor missions through the synchronized command scheduler. Run it with the
existing [map observation tool](../../../tools/map_observation.md). Its numeric
type/actor handles belong to the saved launch; verify them if the population
changes. Runtime observation is separate from a native comparison.

The validation receipt (`tools/spatial_oracle/harvest_attack_return.validation.json`, in git at 1b1a8b43e)
binds the final Rust source, native output, test logs and release binary. In the
ordinary run, ore stayed at density 4 for all 226 Attack observations
(steps 2502..2727). Combat killed the target at 2725; Harvest returned at 2728
and collection resumed at 2748. These are Rust production observations.

## Refinery deposit and shared depot consumers

A full stock War Miner searches its owner's ordered Dock list and asks the
Building radio receiver for admission. The common Enter handler probes the
contact and uses the committed mission's cadence. On the pad, Unit per-cell
sends DOCK_NOW; Unload advances the shared Stage, pays the refinery owner and
returns to Harvest. Scanner queries and mission probes use the same admission
owner. Depot Enter also uses that handler, the Unit setter and per-cell owner;
it carries no duplicate Enter retry timer.

Building construction installs `max(NumberOfDocks, 1)` contact slots before
selection or HELLO. Admission queries the actual sparse slots through
`Contacts::has_free_or`; HELLO uses the radio owner without resizing the building.
The same construction setup serves refinery, depot and airfield consumers.

The docking owner retains native pending entry independently of an admitted
depot visit. Its pointer can survive approach/service, contributes to the world
hash and is saved. Building lethal damage captures contacts before Techno death
effects; the later Building wrapper sends RUN_AWAY and clears pending after the
receiver returns. Generic Destroy does not repeat that continuation.

The [refinery executable packet](../../../tools/spatial_oracle/refinery_dock.md)
retains the older 107 rows, additional shared controls and three complete
sampled deposit/retry/destruction-prefix histories. The continuation replay
compares represented boundaries and full Scenario RNG; a separate test binds
the native reader inputs to production retail rules and ART. The
packet also retains nine actual-slot admission controls and four constructor
controls, replayed through the shared owners and both construction routes. The
[ordinary release profile](../../../tools/map_observation.refinery-docking.example.json)
observes cargo, House payment, radio release and resumed harvesting through
the normal loader and synchronized commands. The packet records the exact
boundaries of its repair, movement, scheduling and rendering evidence.

The docking validation receipt (`tools/spatial_oracle/refinery_dock.validation.json`, in git at 1b1a8b43e)
binds the final Rust hashes, native comparisons and release run. That ordinary
run observed two 40-bale deposits paying 1,000 credits each, with radio release
and collection resuming after each deposit. The final GPU frame was inspected;
native whole-clock and pixel comparisons remain outside this evidence.

## Coverage limits

The native packet executes command, expiry, dispatch, idle, promotion and selected
Stage/latch bodies. Live Approach uses a declared external return seam; damage
supplies expiry in the packet. Actual firing and combat-produced death are tested
through Rust and the ordinary retail map, without certifying native combat or
whole-match scheduler/RNG parity.

Retained AttackMove's saved mission, planning advancement, linked idle objects,
non-Cell waypoint heads and other class locomotor unwind remain separate base
mechanisms. Ordinary Attack clears its saved-order/navigation inputs. The stock
HARV path uses Drive, no slave manager and an owned type from its retail `Dock=`
list. Shared depot entry retains the native Building destination through idle
and per-cell docking. Full depot repair cost/payment cadence (issue #675),
unusual Weeder/slave routes, broad AreaGuard, network delivery and rendered pixel
equivalence retain their existing owners and residuals.
