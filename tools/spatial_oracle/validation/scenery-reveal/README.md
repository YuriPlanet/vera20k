# Local ordinary scenery first-reveal validation

The previous building repair still left whole-anchor exploration gates in the
ordinary cell-overlay and Terrain instance builder. This change removes those
two gates in `src/app/presentation/instances/overlays.rs`, including the private
overlay visibility input and its obsolete plumbing. Lifecycle, current overlay
identity, empty-frame, geometry and camera admission remain with their existing
owners. The existing framebuffer ABuffer/palette path clips the submitted art.
No simulation, input-selection, RNG, timer or detach logic changes.

Native admission evidence is separate from Rust production observation:

- [Static Ground/Terrain](../../terrain_reveal.md): original scan, rectangles,
  projection and draw arguments, with a recording raster terminal.
- [Ordinary cell overlays](../../bridge_shadow_render.shroud-admission.md):
  original rectangles and raster for prepared physical low/high wood bridges;
  parent Tactical iteration is read rather than executed.

Neither native fixture executes an entire scenario or supplies a composited
native frame for these Rust captures. No whole-frame pixel parity is claimed.

## Reproduction

Use the project's Cargo owner and a configured checkout with the retail assets:

```powershell
python -m tools.cargo_run --label scenery-shroud-reveal-20261007-v1 -- build -p vera20k --release --bin vera20k
Remove-Item Env:RA2_DIR -ErrorAction SilentlyContinue
$env:WGPU_BACKEND = 'dx12'
python -m tools.map_observation --build-label scenery-shroud-reveal-20261007-v1 --profile tools/map_observation.scenery-tree-25.example.json --contract src/app/diagnostics/tactical_capture/contract.v2.json --cwd . --output logs/scenery-tree-25
```

Use the matching tree-27, bridge-243, bridge-244, rock-94 and rock-95 example
profiles for the other endpoints. `config.toml` supplies the retail directory;
the capture wrapper rejects an ambient `RA2_DIR`. Labels above refer to the retained local
candidate; rebuild under a new owned label if that label already exists.
Validate a bundle without putting derived previews inside it:

```powershell
python -m tools.map_observation validate --run logs/scenery-tree-25 --output logs/scenery-tree-25-validation.json
```

The profiles select retail SNOW `XShrapnel.MAP`, start 0, America, seed
305419896 and a normal Move order for MTNK stable ID 1517, discovered from the
actual loaded roster. Recheck IDs if launch inputs or map data change. Both
before and after candidates use byte-identical profiles, config and contract.
Map payload and executable identities are in [receipt.json](receipt.json).
The before candidate is the retained building-fixed release
`building-shroud-reveal-20261006-v1`, also matching the user's recorded binary.

## Observed output

| Ordinary route | Anchor first revealed | Before anchor revelation | Next endpoint |
| --- | --- | --- | --- |
| TREE20 at (40,87), Move to (43,98), camera (40,88) | Step 27 | [Old step 25](tree-25-before.png), [fixed step 25](tree-25-after.png) | [Old step 27](tree-27-before.png), [fixed step 27](tree-27-after.png) |
| Low wood center at (54,105), Move to (60,105), camera (56,105) | Step 244 | [Old step 243](bridge-243-before.png), [fixed step 243](bridge-243-after.png) | [Old step 244](bridge-244-before.png), [fixed step 244](bridge-244-after.png) |
| SROCK05 overlay at (40,97), Move to (43,98), camera (41,97) | Step 95 | [Old step 94](rock-94-before.png), [fixed step 94](rock-94-after.png) | [Old step 95](rock-95-before.png), [fixed step 95](rock-95-after.png) |

The fixed step-25 image shows partial tree crowns while TREE20's anchor is
still unrevealed. The old image omits them completely. The fixed bridge-243
image already shows the exposed part of the deck while its center anchor is
unrevealed; the old image omits it. At the next endpoints the shared shroud
boundary exposes more of the same submitted art.

All seven before/after endpoint pairs, including the step-500 bridge route,
match initial/final simulation fingerprints and every retained observation,
presentation clock, camera, atlas summary and input identity. Observer-on/off
step-500 runs match BGRA bytes and fingerprints. Fifteen sealed bundles pass
final offline validation. These are bounded Rust integration results on DX12
with an RTX 3050 Laptop GPU; shroud is enabled and FogOfWar is disabled.

The existing physical wood-bridge GPU witness first failed before the repair
because the unexplored center emitted zero instances. After the repair both
wood and concrete witnesses pass, retaining damage/collapse/Engineer repair,
camera, empty-frame and every color/depth-pixel checks on Vulkan. Focused
instance tests pass 89 with 3 ignored. Strict-retail full lib tests pass 9704
with 239 ignored; Clippy exits 0 with 722 retained warnings. The simulation
field ratchet stays 2505/2505. All four native corpus checks pass, and both
historical default payloads remain byte-identical after actual Steam replay.

Frame wall means in the receipt include startup shader costs, simulation,
observation, rendering and pacing. They are not GPU time or gameplay FPS;
this capture does not certify scalability. Camera culling remains active.

The single [fresh read-only critic](review.md) found no confirmed defect or
blocker in the Terrain/cell-overlay gate and tree/bridge evidence. The rock
captures below were added by the owner afterward using the same unchanged
production source and binaries; they are outside that critic's capture review.

## Additional rock observation

The actual scenario's live overlay ID 172 at (40,97) is SROCK05. The existing
`asset ini-get` command exercises the production layered reader:

```powershell
asset ini-get OverlayTypes 172 --domain rules --reader raw --map XShrapnel.MAP --mode-id 1 --ra2-dir 'D:/steam/steamapps/common/Command & Conquer Red Alert II'
```

The [reader receipt](rock-overlay-production-reader.json) records the selected
rules, absent language layer, Battle mode and map identities. The processed
registry result is SROCK05; the authored numeric key 172 contains SROCK02.
Native overlay registration and the production `OverlayTypeRegistry` use list
declaration order rather than those numeric keys.

In the fixed step-94 image the gray/brown upper rock surface is already visible
between the trees while its anchor is unrevealed; the old image has bare ground
there. At step 95 the old renderer admits the rock abruptly. Nearby trees also
change because they use the repaired Terrain gate and partially occlude this
rock. These pixels are observed output, not isolated native raster goldens.

The [old 94](rock-94-before-detail.png), [fixed 94](rock-94-after-detail.png)
and [old 95](rock-95-before-detail.png) detail images crop the same 100x100
rectangle at (240,220), then enlarge it eight times with nearest sampling.
They change no colors and add no annotation; original full frames and both
hashes remain in the receipt. The central gray/brown rock is distinct from the
newly submitted pine silhouettes partly in front of it.

This is the same ordinary cell-overlay builder and native `6D6D10` ->
`47FB90` -> `47F6A0` route used by the bridge repair. The saved unmodified
[parent](native-6D6D10.json), [rectangle](native-47FB90.json),
[body](native-47F6A0.json) and [drawing lift](native-480110.json) instructions
pin the Steam executable. The parent calls map bounds at `6D6EBC`, cell lookup
at `6D6ED3`, body rectangle at `6D6EEE`, rectangle clip at `6D6F22`, and body
draw at `6D7001`. Original bodies plus Ghidra control-flow reading establish
geometry/frame admission without an anchor exploration predicate. Linear
disassembly alone does not prove absence of arbitrary aliases or callers.
The executable wood-bridge oracle remains
bounded to its prepared bridge types; it is not relabeled as a rock raster
oracle. No new source or simulation logic was needed for these rock captures.

[Retail SHP inspection](rock-retail-shp-info.json) records the SROCK05 body
frame rectangle (45,44,27,26) in its 120x120 canvas. Its standalone catalog
lookup warning means that inspection is not proof of runtime asset binding;
the actual scenario output is retained separately above. To reproduce static
reads use the existing [native inspection tool](../../../native_inspect.md),
for example `python -m tools.native_inspect disasm 0x47F6A0 --bytes 0x4F0`.

## Remaining coverage

Animated/crumbling Terrain uses another native route. Full FogOfWar/gap,
arbitrary overlays/heights and native full-scene pixels remain outside this
repair's evidence. These draw bodies introduce no simulation RNG draws,
timer writes or detach calls; existing animation/simulation owners are intact.

The exact gray shore cluster around 13 seconds in the user's video remains
unidentified. A cluster may contain separate TMP and overlay objects, so its
partially visible caps alone cannot clear a rock pop-in report. The TMP terrain
builder already has no exploration gate and loads its map atlas up front.
The actual SROCK05 witness establishes this overlay repair, not every reported
rock type, TMP detail or the identity of that video cluster.
