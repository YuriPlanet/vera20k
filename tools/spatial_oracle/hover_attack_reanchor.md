# Grounded HoverAttack re-anchor

`hover_attack_reanchor.py` executes the original Unit Guard740810 or Foot
Attack4D4DC0 prefix through NearbyLocation703590, Unit741970, Foot4D94B0,
Jumpjet MoveTo54B1C0 and its Unit placement54D6D0. Four controls vary the two
missions and real versus missing cell `(0,0)`.

The input supplies a grounded Unit holding a Cell target, with the stock
SHAD/SCHP fields HoverAttack1, SpeedTypeHover3 and MovementZoneFly9. It reuses
the existing destination VM, original class vtables and Jumpjet constructor;
the linked owner and retained zone cache are supplied admitted prestates.
It does not execute physical type readers, map construction or touchdown.

Only the two FNPC results are supplied empty: NearbyLocation's search at
7036A1 and Jumpjet's own placement search at54B374. The original caller still
looks up the first empty cell, passes its nonnull Real/Dummy receiver to the
class setter, and reaches MoveTo with `(128,128,0)`. MoveTo keeps its own
empty-result placement, rewrites NavCom and sets moving. Phase0 remains0
until Process. The supplied flat, unbridged state never redirects through
7447B0; the control rejects such a redirect instead of substituting it.

Each control stops immediately after the concrete setter returns, at4D5225
for Guard or4D4E0D for Attack. The latter is after the call at4D4E07. These
are transient prefix boundaries, before later targeting and cadence work.
No Scenario RNG is consumed at this Unit placement boundary; infantry
subcell placement and full Jumpjet Process are separate mechanisms.

```sh
RA2_DIR=/path/to/retail python -m tools.spatial_oracle.hover_attack_reanchor --check
```

Use `--write` only to deliberately regenerate the corpus. The sidecar records
the supplied boundaries and replay command. No retail game assets are stored.

The Rust regression reads physical stock rules/ART for SCHP and SHAD and
executes the actual searches over a constructed terrain grid: a lone Clear
landing cell surrounded by stock Rock, whose Hover cost is zero. The landed
Unit's actual ground occupation bit rejects the centre, which FNPC also
visits. A rival Unit outside the search region keeps the continuation live.
This is a stock-type constructed-terrain witness, not a named retail map or
a measured native touchdown history.

The prefix comparisons stop at the same shared helper boundary as the native
controls. A further regression dispatches Guard through the production
mission owner, saves after the handoff, then compares independent restored
and uninterrupted worlds through ordinary frames and the resulting lift.
The two worlds have distinct shared fallback Cells, so their hashes do not
mask divergent Dummy state through aliasing.

The retained Cell getter dependency is included by the preceding transport
shore-search change. Broader bridge preflight, infantry piggyback and complete
native flight histories remain outside this control's evidence.
