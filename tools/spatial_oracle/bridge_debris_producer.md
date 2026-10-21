# Bridge fallout animation producer

`bridge_debris_producer.py` executes the original CellClass bridge fallout body
at `0x0047DD70`, AnimType constructors at `0x00427530`, the image metadata suffix
at `0x00427C12`, complete AnimClass constructors at `0x00421EA0`, immediate
`Start` at `0x00424CE0`, and Bounce initialization at `0x004397E0`.

Run from the repository root with Unicorn 2.1.4 and the retail executable selected
as described in [the native comparison workflow](../native_oracle.md):

```sh
python -m tools.spatial_oracle.bridge_debris_producer --check
```

The 266 producer rows cover 256 seeds, all 15 selected metallic entries, signed
cell coordinates and level bytes, empty/negative explosion counts, editor mode,
allocation refusal, and two supplied ground-object lists. The 12 `death_loop`
rows execute the original `0x007024E0..0x00702572` loop on the same types to protect
the shared constructor consumer. These are native outputs, not Rust-generated
expectations. The sidecar records all substitutions.

The lists are the actual native General reader results: 15 metallic entries,
ending with `D` because its 128-byte input buffer truncates the authored list,
and four BridgeExplosions. `bridge-retail-anim-inputs.json` retains the input
export from the production Hills loader on merged PR552. Its fourteen DBRIS SHPs
have 15 frames; TWLT026/036/050 have 17 and TWLT070 has 26. The production export
then lacked `D`; the oracle constructs that native type and retains its defaults
because its ART section and image are absent. The new Rust reader regression
requires the corresponding retained constructor state.

`bridge_retail_anim_inputs.rs` is the standalone exporter source. Compile it
against the release `vera20k` and `serde_json` rlibs from the same Cargo build,
with `-L dependency=<target>/release/deps` and `--extern` paths to those rlibs;
set `RA2_DIR` and run the exporter. Record the built commit before replacing
inputs. The supplied fields are production-reader and asset-binding inputs;
this fixture does not execute native INI or asset loading. The separate General
reader oracle establishes the truncated list. Do not replace these inputs with
the earlier supplied 20-entry/16-frame characterization fixture.

`src/sim/world/bridge_debris_tests.rs` compares the production readers and the
successful allocation, running-game producer domain: object order, exact signed
world coordinates, delay, draw flags, Bounce position/velocity/physics bits, and
complete Scenario RNG state. It also checks that Main and MapGen RNG are unchanged.
Allocation refusal, editor mode and ground receiver rows characterize native
branches that this test does not recreate. The ground receiver has a separate
`terrain_debris_receiver` comparison. This producer corpus stops before flight,
landing, delayed sibling Start/sounds/smudges and rendering; it does not prove
those consumers or a whole bridge collapse.
