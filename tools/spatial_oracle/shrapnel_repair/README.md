# Physical Shrapnel ordinary low repair

This packet executes original active-retail instructions over the stock Shrapnel Mountain broken low bridge. It is a bounded repair witness, not a native scenario loader or a whole-game comparison. Native instructions are never patched. CPU writes into `.text` fail and the complete mapped `.text` hash must remain unchanged.

## Inputs and limits

The physical `XShrapnel.MAP` bytes are from `multimd.mix`, entry `0x38D73A35`, SHA256 `c32e412e938e1b9a0b29f89ff9bc0e6f410f75bc76701fb4acef6581fd19b438`. It is SNOW, Size82,82, raw LocalSize5,6,71,70. Actual567230 normalization executes and leaves those bounds unchanged. Every supplied crop cell's tile, subtile, elevation, overlay and overlay-data byte is checked against decoded physical MAP packs. The MapGen, Main and Scenario logical states (disabled byte, two cursors and250 words; three native padding bytes supplied as zero), cell flags, crop land/class values and full navigation input planes are explicitly recorded production inputs. They do not prove native map-loading equivalence.

The native OverlayTypes registry/constructors execute over physical RULESMD strings. Original overlay scalar read blocks execute for types74..101, including `Land` and `NoUseTileLandType`; complete674000 land-speed reads and the original `CliffBack` reader execute in the declared layer order. Native constructor CliffBack is0; retail read produces2. Inputs are physical RULESMD, absent LANGRULE, active `MPBattleMD.ini`, then physical Shrapnel. The actual active mode resolves `ra2md.mix → localmd.mix`, entry `0xAF084179`,295bytes, SHA256 `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963`. It is byte-identical to the earlier separately extracted `MPBattle.INI`; its sole section is MultiplayerDialogSettings. Native cached INI objects are supplied from exact lexical source lines. No native physical INI parser or full Rules Process chronology is claimed.

The original56 theater-key reads and545CEF global projections run on physical SNOWMD. Each `TilesInSet` scalar uses original5276D0; ordinal iteration, cumulative loaded count and file admission remain supplied theater-loader boundaries. Required TMP files are physical `Water02.sno`, `Water03.sno`, `Water10.sno`, `Water11.sno`, and `Shore22.sno` from `ra2.mix/isosnow.mix`. Only runtime pointer offsets are relocated. Resident TMP head binding is supplied, with no lazy file load, animation or shadow receiver claim. Missing reached dependencies fail closed.

The original CRT precision tail and WinMain rounding block establish x87 state. The13 actual Cell initializer pointers at8129FC then derive LevelHeight104 and BridgeHeight416. Ground and alternate occupant heads are empty by construction. No Engineer admission, Engineer common tail, live object fallout, animation, audio, rendering or radar queue is executed here. The neighboring naval oracles and the production Engineer test cover separate continuations.

## Executed chain

`shrapnel_repair.py` retains two hut selectors:117,56 and113,62. Both enter570050→57F200→57FBC0. The controller stage records the Recalc boundary; the resident stage executes full47D2B0→47CA80→483C80 with physical TMP and native-read overlay/land/theater inputs. The resident stage also executes Recalc on all nine authored affected cells before repair, preserving actual land/class stores. The native middle101 row is Water2/class4; neighboring damaged90/91 rows are Road1/class0.

Both hut paths execute actual598030→65C780 against the frozen complete MapGen state. Each repair requests[0,3] three times, takes three raw draws and obtains1,1,2. Repaired y58/y59/y60 rows become84/84/85. All nine cells become Road1/class0; overlays preserve frame0/1/2, structural flag0 and physical tile/subtile/elevation. Empty487A10 and ground47B3A0 execute nine times. Original5868A0 enumerates rectangle114,58,3,3. A second repair on the same VM produces no mutations, RNG draws or downstream rebuild calls.

`zone_composition.py` adds actual56C510. It builds initial connectivity from the supplied full class/height planes, then executes the repair and actual connectivity suffix in the same VM. It injects no Rust base-zone IDs or movement rows. Both initial and repaired outputs match all26,896 production class/height/base-ID cells and all13 raw movement rows exactly; zone count is388. Empty endpoint records are justified by the selected map's lack of high bridge overlays/Tubes and checked against empty production bridge authority. Bucket construction follows original565800; original58AFF0 executes, followed by the constructor caller's vtable/growth stores.

`hierarchy_composition.py` executes original initial581F90 levels2,1,0, then actual586990→584550→42C1C0. All13,366 live allocated cells and their elevations exactly match physical MAP records; live slope bytes remain a supplied production boundary. No Rust graph IDs or edges initialize the native graph. All three initial and final graphs match production exactly, including26,896 cell IDs and329 padding IDs per level, parents, types and ordered edges. Initial record counts3587/1545/857 become3595/1556/885. Repair performs18 native Recalc calls (9 direct plus9 reverse batch),9 empty occupant controllers, four incremental patches at116,60;116,59;115,60;115,59 and four scratch refreshes. The second repair remains a no-op. The portable receipt and independent `--check` bind this claim to exact inputs and sources.

## Comparison boundaries

Production snapshots span the Engineer's30 ordinary approach frames. The complete native repair MapGen output equals production after repair, and Main is unchanged/equal. Native Scenario is unchanged inside570050, while production Scenario advances during ordinary frames outside this bounded call. That difference is recorded, not treated as a repair mismatch or claimed as full-tick RNG parity.

Successful bounded allocation and no-op free are supplied. Display rectangles return zero; screen and radar requests are recorded sinks. The controller/resident stages substitute connectivity/hierarchy until the named composed stages replace those exact seams. The resident bootstrap is a component derivation, not proof of native initial map-load order. Malformed cells, other bridge orientations/types, occupied spans and full scenario initialization remain outside this packet.

## Portable packet and retained evidence

`production_inputs.json.gz` contains only the before/after fields consumed here: supplied cell/RNG/bounds/cache/live-cell inputs and the separately captured production comparison targets. The full original `loaded`, `before_command` and `after_approach` capture SHA256 values remain in that projection. Only the latter two have retained field projections; the loaded capture is an identity record. Original captures, unrelated entity/debug text, duplicate pretty exports and logs are excluded.

The three native reference files use canonical JSON inside deterministic gzip. Every native value, trace, RNG word, cell result and ordered graph record from the frozen packet remains present. Only copied lexical inputs at `native_inputs.layers[].sections`, `.source_lines` and `theater.sets[].physical` are replaced with canonical JSON hashes. Native reader outputs remain unchanged. `promotion.json` records the original file/canonical-payload/metadata hashes and each published payload hash. The runners reject any changed published payload even under `--write`. The small `production_comparison.json` remains byte-for-byte equal to its frozen original.

`retail_manifest.json` pins all external inputs by filename, archive, entry, length and SHA256; it contains no retail file bytes. The legacy `MPBattle.INI` alias is no longer required: its prior direct byte comparison is retained by hash, and the actual `MPBattleMD.ini` remains mandatory. Original disassembly dumps are omitted because execution uses the verified executable and the runner addresses; they are not runtime dependencies. `receipt.json` names all owned files and imported repository helper sources by relative path and hash. The old external receipt hash is retained in `promotion.json`; its workstation paths are not republished.

`map_facts.py` retains the prior LCW decoder and obtains liblzo2 through system library discovery or `VERA20K_LZO2_LIBRARY`. This is raw MAP extraction, not a native gameplay oracle. The reusable MapGen startup owner is the repository's `tools.spatial_oracle.mapgen_range`, rather than a duplicated external helper. Other native readers and VM helpers remain with their existing repository owners and are listed in the receipt.

## Reproduction

Use Python 3.10 or newer, the Unicorn/Capstone dependencies described in [`tools/native_oracle.md`](../../native_oracle.md), and liblzo2. Set `VERA20K_GAMEMD_EXE` to the matching original executable (or use the shared oracle's configured `RA2_DIR` lookup). Place the exact extracted files named in `retail_manifest.json` in one flat external directory and set `VERA20K_SHRAPNEL_INPUTS` to it. `LANGRULE.INI` must be absent for this frozen witness. The default directory is `target/shrapnel-native-inputs/extract`. TMP extraction must explicitly bind the SNOW theater archive shown in the manifest.

Run from the repository root, using your configured Python interpreter:

```sh
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=.
export VERA20K_SHRAPNEL_INPUTS=target/shrapnel-native-inputs/extract
python -m tools.spatial_oracle.shrapnel_repair.validate_packet --write
python -m tools.spatial_oracle.shrapnel_repair.validate_packet --check
```

These are separate invocations. `--write` repeats all native executions, enforces the frozen publication hashes, writes the references and regenerates the portable receipt. `--check` independently repeats all four runners and compares outputs, provenance, physical input hashes and the complete source/artifact manifest without writing. Omitting a flag means check. Individual runners also support `--check` and `--write`, for example:

```sh
python -m tools.spatial_oracle.shrapnel_repair.hierarchy_composition --check
```

Inspect a reference with Python's `gzip.open` and `json.load`; compression does not truncate the graphs. No special absolute paths, adjacent research files or original full production exports are required.

The naval cleanup runner can reuse the same input and resident bindings:

```python
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri
from tools.spatial_oracle.shrapnel_repair import shrapnel_repair as sr

rules = ri.Rules()
theater = ri.theater()
case = sr.input_case([117, 56], theater)
machine = sr.ResidentRepair(case, rules, theater)
```

Both modules expose the same `ASSETS` directory. `ResidentRepair` performs the nine initial native Recalc calls; it does not perform the repair until `.run()`. Downstream oracles must state their own occupant/list/lifetime boundaries and cannot inherit a whole-scenario parity claim from these helpers.

## Current follow-up: CliffBack constructor/default

Native constructor `0x665F3B` stores zero at Rules+`0x664`. The reader `0x66F1CB` defaults to the retained signed byte; physical RULESMD explicitly reads two in this witness. The current production owner, [`GeneralRules`](../../../src/rules/ruleset.rs), initializes `cliff_back_impassability` to two and uses `unwrap_or(2)` for an absent key. An absent key or later layer without the key therefore needs separate constructor/retention closure, including affected Recalc consumers. This packet records the mismatch; it does not fix it. The retail Shrapnel witness is unaffected because its native and production value is explicitly two. Its successful comparison does not close that required follow-up.
