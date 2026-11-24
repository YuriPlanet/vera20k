# Authored ore queue initialization

`python -m tools.spatial_oracle.ore_queue --authored-init --check` replays the
compact native control in `ore_authored_init.json`. Use `--write` to deliberately
replace it. The shared native-oracle environment and extracted input bundles are
described in [native_oracle.md](../native_oracle.md). The sidecar pins the original
executable, replay command and canonical payload hash; it pins no repository
source hashes.

The existing `anytown_damage.navigation_inputs.Inputs` owner reads physical
RULESMD, optional LANGRULE, Battle and XMP03T4 layers with its existing native
constructors and readers. The opt-in timer block executes `721A78..721AFA` in
each existing layer immediately before Value. The same physical TileSet loop
reads exact key `AllowTiberium` through `5295F0`, default false, and the existing
Navigation TMP owner publishes that result to IsoTileType+306. Original
`546187..54619B` reads the key; `54646D`/`546AB5` publish it for the original
primary/secondary tile construction paths.

The existing full-map Navigation VM stops after its original Cell constructors,
physical primary TMPs, first attribute recalculation and native Terrain
constructor/direct Place phase, before its final `568BB0` sweep. This is the
`Full_Init` boundary preceding growth init `687A85 → 722D00` then spread init
`687A8A → 722240`. Both full init bodies and their four per-type rebuild calls
execute unchanged. The shared queue snapshot follows actual member pointers,
native heap references and native bitmap ordinals; no host heap constructs the
expected queue.

`inputs.gates` comes from original SpecialFlags `6B8CA0` and Basic growth
`683848`/`68A571..68A58F`, using temporary declared Scenario member storage on
the existing reader VM. Original `687853 → 689E90 → 6B8CA0` precedes queue init;
the mode-dependent session overwrite at `687C23` follows it. The control copies
the earlier native-read values into the prepared map, rather than deriving
them from a Rust final-load snapshot.

The retained packet contains:

- `native_rect`, `capacity`, `state.classes`: native growth/spread entry arrays,
  heap indices, bitmap coordinates/indices and both raw timers per type.
- `inputs.manifest`, `type_readers`, `effective_types`: physical payload/archive
  identities, optional LANGRULE status and the actual ordered native reader
  outputs. LANGRULE absence refers to the extraction bundle; the production
  consumer separately checks its actual asset source.
- `inputs.native_iterator`, `source_admission_summary`, `flat_overlay_types`:
  compact source, iteration and native OverlayType CellAnim prerequisites.
- `timers_before`, `init_order`, `rng_before`, `rng_after_init`: constructor timer
  priors, actual rebuild call order and complete-state RNG hashes. Generation
  compares all bytes of all three RNG streams, not only their hashes.
- `final_sweep`: actual later `568BB0(0)` value and preservation of queues,
  timers, RNG and every resource record. Full cell planes remain unretained.
- `patch`: source 72,25 and targets 71,24 / 72,24 / 73,24 / 71,25 / 71,26,
  including native tile/read provenance, Buildable land, playfield and pure
  active-game `4838E0` admission outputs.

These inputs establish authored queue initialization and the selected target
prerequisites. They do not reproduce whole native scenario startup: registry
declaration-order transport, cached lexical INI input, prepared OverlayPack Cell
fields and native Terrain direct Place remain declared seams. Terrain Unlimbo,
discovery and complete startup object/Overlay serial lifetime are excluded.
RNG begins from the existing independently native-seeded map fixture; unchanged
init/final-sweep states do not establish native startup or whole-frame RNG.

The packet does not execute a new ordinary growth/spread history. The existing
[ore_queue](ore_queue.md) controls retain that owner's direction/variant draws,
timer writes, Overlay construction/retirement and deferred drains. Initialization
and the selected pure admission queries consume no RNG, reload no timers and
detach no objects. Other maps, saved/generated worlds, custom types/CellAnim and
full natural startup remain outside this control.
