# Packed first-reveal material validation

This packet records bounded native execution and Rust production output. It does
not certify whole-scene geometry, cloak lifecycle timing, or ordinary 20k-unit FPS.
The [current-main submission report](submission.md) and
[receipt](submission.receipt.json) supersede the old-candidate readiness results
below. The [first-candidate receipt](receipt.json) pins the original candidate,
native corpus and authored-map inputs. The [corrected old-candidate receipt](after-critic.receipt.json)
pins that historical release, frozen compiled code, checks and 14 strict
production comparisons. Local repair is authorized; publication is not.

The canonical query is `Techno703860`, reached by the constructor-installed retail
Foot locomotors and Building. Rules owns the live signed stage count; discovery
owns the current-house byte. Source/caller evidence and the truncation argument
are in [native-controlflow](../native-controlflow/README.md). Native shadow and
`NoShadow` reader evidence is in [source-dependencies](../source-dependencies/README.md).
The original executable SHA256 is
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.

The existing [oracle](../../translucent_blitter_a.md) independently replayed20
selectors,6136 single-call pixel cases,54 three-parent replay controls,256 query
controls and14 offset controls. SHP and VXL source generation reuse their existing
projection/palette owners. `TerrainDrawRenderer` owns framebuffer/Z snapshot,
packed composition and commit. Its existing tile scheduler batches disjoint
pixel-local composites; overlapping or displaced reads retain required ordering.
These checks do not independently establish the input geometry of every unit.

## Executed checks

- Vulkan and DX12: all three packed GPU tests passed on an RTX 3050 Laptop GPU.
  The tests replay 6136+54 actual native outputs in both sRGB output formats,
  including source holes, Z rejection, write/read policies and signed neighbor
  feedback, plus the 4K distant-parent regression and synthetic workload.
- Ordinary native GPU controls:3 passed. Native producer CPU checks:8 passed.
  Map-observation Python suite:111 passed.
- Strict-retail full library run:9712 passed,2 failed,241 ignored. The two failing
  fixtures were corrected: snapshot version290→291, and the Walk oracle's explicit
  `Rules+628=10` input through the production reader. Subsequent focused checks
  passed1 snapshot assertion and both Walk-module tests. The full suite was not
  repeated after these assertion/fixture-only fixes, following AGENTS.md.
- Final DrawState focused checks: 10 passed. Strict-retail Clippy completed
  with exit 0 and 721 warnings. Final field ratchet: current 2501 versus 2505 at
  `origin/main`. Corrected release game and the earlier asset build passed.

The synthetic debug workload used1000/20000 small sources in a1280×720 viewport.
Both backends produced 4/24 waves and 16/96 passes. Final Vulkan encode/completed
wall times were 1.961/9.682 ms and 23.122/68.752 ms; DX12 was 14.021/20.006 ms
and 94.525/117.337 ms. These are single cold debug samples, including continuation
submissions; they are neither GPU timestamp measurements nor normal play FPS.

The single [independent review](review.md) confirmed a high-resolution dispatch
defect. Its new actual4K regression failed before the fix with129600 X groups
above the requested device's65535 limit. The owner has bounded dispatch on two
axes and restored the linear pixel/residue index from WGSL builtins. All three
packed GPU tests then passed on each backend; the 4K test checks both native
endpoint colors and the untouched middle in both output formats. The release
was rebuilt from frozen source and passed the production checks below. This is
owner validation following one critic pass. See [WGSL builtins](https://www.w3.org/TR/WGSL/#builtin-values)
and pinned [wgpu27.0.1 limits](https://github.com/gfx-rs/wgpu/blob/v27.0.1/wgpu-types/src/lib.rs).

## Production observations

The corrected `packed-shroud-reveal-20261008-v2` release was replayed against
the first packed candidate using identical profiles. All 14 comparisons are
strict **MATCH**, with no errors or differences: mobile110/111/hidden/clear,
tree25, bridge243, rock94, and the seven SUB endpoints described below. Each
new bundle is VALID; the unchanged comparer checks full frame bytes, inputs,
exact clocks, simulation fingerprints and observation transcripts, including
the new cloak fields. These are Rust regression comparisons, not original-game
whole-frame goldens. The exact executable SHA256 is
`592d26c19990652468dfffaa1bc5b37a33f95fb99c27d023b379a57ff25079ab`.

The earlier comparisons against the ordinary repaired release remain preserved:
Seven serial DX12 release controls replay mobile110/111/hidden/clear and the
prior tree25/bridge243/rock94 frontiers. Each bundle is VALID. Full frame bytes,
all original observation fields, inputs, exact clocks and simulation fingerprints
match the prior repaired release. The unchanged comparer reports MISMATCH solely
because the new additive `actor.cloak:null` field was absent from historical
receipts. The raw comparisons remain preserved; no expected field is removed
from the comparer and no MISMATCH is relabeled as an unqualified MATCH.

SUB integration uses Steam `expandmd01.mix` entry `c2s01md.map`,248872 bytes,
SHA256 `8b6722212fdfd111fc02da13cb97c653e40839ff8e5887521eb359340623bbb4`.
The first authored copy changes only Units row2's owner `Neutral` to
`VERA-OBSERVER`, preserving all other retail bytes. This is an explicit Rust
scenario fixture, not proof that native accepts that custom player name.
The normal loader observes SUB1930 at `(69,131)`, VXL=true, NoShadow=false,
stages9. Guard produces state1/progress0 at step1, state1/progress1 at2 and
state2/progress0 at6. Captures0/1/2/90 and Attack→Stop160 are VALID.

Stock `SubTorpedo` reads `DecloakToFire=no`; ordinary Attack90 damages HYD1928
while SUB stays cloaked. A second declared fixture appends only
`[SubTorpedo] DecloakToFire=yes` to the first copy. Normal Attack then produces
state3/progress8 at92 and state0/progress0 at100; damage and Stop are observed.
The state3 endpoint92 is also a VALID actual GPU capture. Neither variant
injects cloak state, invokes a debug spawn, or establishes native timer parity.

## Reproduce

Build using the Cargo owner; use the label in the receipt only for its exact
candidate. The [portable guard profile](../../../map_observation.packed-sub-guard.example.json)
expects the explicitly authored loose map in the checkout. Extract the stock
map with the existing asset owner, check the SHA above, and change the exact row:

```text
2=Neutral,SUB,256,69,131,64,Guard,0C2ADEAC,0,-1,0,-1,1,1
2=VERA-OBSERVER,SUB,256,69,131,64,Guard,0C2ADEAC,0,-1,0,-1,1,1
```

Do a byte replacement once, retain the source, and preserve the original line
endings. Place the copy at the profile's `selected_map_file`. Use the existing
map-observation wrapper with an absolute profile, contract, cwd and fresh output
directory as documented in [map_observation](../../../map_observation.md).
Keep the retail config and seed/roster fixed; inspect step0 before reusing IDs
if any input changes. Choose ticks1/2/90 for the guard endpoints. For the declared
decloak variant, append the rule override above and ordinary Attack1930→1928 at
step90; choose endpoint92 or add Stop1930 at130 and end at160.
Actual absolute profiles, sealed bundles and hashes are named in the receipt.

Resolve the final preserved game through the existing Cargo owner:

```powershell
python -m tools.cargo_run --resolve vera20k --profile release --from-label packed-shroud-reveal-20261008-v2
```

Run the capture from this checkout with its retail config; the returned binary
path alone does not select the correct working directory. The build manifest
records the pre-result-document source snapshot. The final receipt separately
pins the 1375 raw `src/` and Cargo file hashes checked against frozen release
source; later evidence-document edits do not alter that compiled code.

## Remaining evidence limits

The pre-existing SHP shadow companion chain (including DLPH), split turret/body
relative native source depth, nonzero Building+6ED writer/lifecycle, and enabled
FogOfWar Wave framebuffer distortion are separate missing or unproved mechanisms.
The literal scan of Techno+24C writers does not exhaust indirect aliases. These
limits prevent an exhaustive whole-engine or all-special-material parity claim.
They do not change the measured ordinary FogOfWar=false frontier controls above.
