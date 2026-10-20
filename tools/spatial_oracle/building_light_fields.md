# BuildingType radius-light field reads

Replay the existing Building body oracle's light controls with the supported
retail executable selected by `VERA20K_GAMEMD_EXE` or `RA2_DIR`:

```powershell
python -m tools.spatial_oracle.building_body_rules --lights --check
```

`--write` deliberately regenerates `building_light_fields.json` and its metadata
sidecar. The sidecar records the actual executable SHA-256 and coverage limits.
No executable bytes or game assets are distributed.

Original constructor stores `45DE12..45DE39` initialize visibility to5000,
intensity to0 and each tint to1,000,000. For every admitted BuildingType section,
`460C8C..460DCA` reads visibility and the four light scalars. Each double's default
first divides its retained integer by1000 **as a signed integer**, then converts
that quotient to double. The returned double is multiplied by1000, biased by.1
and passed through original `Math::ftol`.

Consequently, a stock `INGRNLMP` with intensity10 and tints10/1500/10 becomes
intensity0 and tints0/1000/0 when a later map section sets only `WorkingSound`.
Keeping the earlier lexical light values in a merged INI produces a different
result. The complete missing-section `45FE50` rejection control retains every
field, so this conversion must run only when a section was actually admitted.

The corpus executes original readers, division and conversion instructions on
the existing cached INI transport. Admitted sections use the five contiguous
scalar regions, not a whole BuildingType read. The missing-section case executes
the full rejection path. Native file loading, LightSource activation, sampling,
palettes, rendering and whole-map parity are outside this numerical corpus.
The selected reads pass no gameplay RNG, timer or detach operation.

Rust tests in `rules::building_light::tests` replay the records through the
production `RulesLayerStack` and `RuleSet` projection, check retained-state
serialization and exercise physical retail RULESMD/ARTMD/Battle inputs with
Storm's six sound-only map sections. Runtime GPU comparison is separate.
The Rust reader uses ordinary binary64 arithmetic and the existing shared ftol
conversion; arbitrary extreme mod-value precision is outside the sampled proof.

The mutation owner is `ProcessedType.building_light`; `ObjectType` receives an
immutable integer projection. Building allocation and test-only map collection
feed those words directly into the existing point-light constructor, avoiding a
second float conversion. Missing tint keys intentionally keep the surprising
native constructor defaults; they are not inferred from retail tuning values.
