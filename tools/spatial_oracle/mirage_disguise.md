# Stationary Mirage disguise

`mirage_disguise.py` executes the original stationary retail MGTK disguise path
in the existing `anytown_damage.foot_missions.FootMissions` fixture. The JSON is
consumed by the production rules, simulation, damage, observer, radar and Unit
SHP-route tests. It is bounded evidence for this path, not whole MGTK, movement,
Spy, scenario startup, save-stream or rendered-scene parity.

The native VM, PE identity checks, map/type setup, locomotors, allocation and
platform transport remain with their existing owners. This producer adds original
calls and observations. It does not compute disguise, timer, RNG, frame-selection
or radar answers in Python. Native `.text` and inherited vtables are checked
unchanged after all experiments. The sidecar pins the imported tool-source
closure and the actual selected executable.

## Reproduction

From the repository root, select the supported retail executable and prepared
physical inputs described in [the shared workflow](../native_oracle.md),
[Foot missions](anytown_damage/foot_missions.md) and
[bridge inputs](../projectile_oracle/bridge_render_inputs.md):

```sh
export RA2_DIR='/path/to/retail-install'
export VERA20K_ANYTOWN_INPUTS='/path/to/anytown-inputs/extract'
export VERA20K_SHRAPNEL_INPUTS='/path/to/shrapnel-inputs/extract'
export VERA20K_PROJECTILE_RENDER_ASSETS='/path/to/projectile-inputs/extract'
export VERA20K_MIRAGE_ASSETS='/path/to/mirage-inputs/extract'
python -m tools.spatial_oracle.mirage_disguise --check
```

An explicit `VERA20K_GAMEMD_EXE` takes precedence over `RA2_DIR`. Keep the inherited
sparse input file sets intact: they are physical fixtures, not arbitrary merged
INI exports. The Mirage asset root contains `TREE01.TEM` through `TREE04.TEM`,
extracted from `ra2.mix/temperat.mix` by the existing asset extractor. The corpus
records their exact lengths, hashes and original file reads. No retail files are
distributed here. `--write` regenerates the JSON and its sidecar; `--check` reruns
original code and requires equality. Use `--output` for an isolated candidate.

## Trigger and ownership

Original UnitAI `7360C0` reaches FootAI and then `73647B..73649C` admits
`7468C0` only for `CanDisguise && !PermaDisguise`. The first two `histories` rows
execute complete UnitAI; canonical `FootMissions.navigation_snap` projections
bracket each call. `initialization` records the actual role/pointer mapping,
registries, actor state and current Cell before the first visit. Registry order
is not inferred from storage order. The unrelated enemy MTNK is removed by its
original Limbo call before these idle visits.

`7468C0` retains an existing stationary disguise. Fresh acquisition additionally
requires `DisguiseWhenStill`, no radio contact in slot zero, and the original
locomotor's nonmoving result. Slot one is a negative control. The moving controls
supply a Drive destination and execute its predicate; they do not certify Drive
Process or pathfinding. Acquisition executes Scenario ranged RNG `65C7E0`,
selects from Rules data `+FFC`/count `+1008`, stores the TerrainType and a NULL
disguise House, writes creation frame and dirties radar through `70CCF0`.

The adjacency branch runs when signed global-frame remainder by eight is nonzero.
It traverses the initialized eight-neighbor table and calls original
`47EC40`, which returns only the first Infantry on the selected linked layer.
A first allied Infantry can hide a later hostile one; a Unit is not this trigger.
The bridge selector is original `486750`. The modal global `A8E9A0` admits this
reader; ordinary PrepareSession sets it, but other active modal/shutdown paths
also write it. Its zero control is distinct from the live-game comparisons.
The physical adjacency history uses real Infantry construction, Unlimbo and
Limbo, with hostile owner pointers supplied after placement; it does not claim
native ownership transfer or complete hostile-House startup.

## Rules and retained fields

The reader section executes original constructors and selected exact reader
blocks over physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 layers.
The measured actor additionally executes full UnitType `747620` and its referenced
weapon/projectile/warhead/Terrain readers. Fixed ARTMD selects MGTK's `RTNK` image.
Authored missing, empty, wrong-case, malformed, duplicate, mixed-case and numeric
controls are explicitly separate from retail layers.

- TechnoType `710AF0` establishes false for `CanDisguise`, `PermaDisguise`,
  `DetectDisguise` and `DisguiseWhenStill`; exact readers are `714404..71446C`.
- Rules `665650` establishes the list and reveal-duration defaults; original
  `671D3E..671D92` reads `DefaultMirageDisguises` and
  `InfantryBlinkDisguiseTime`. Native allocation retains first-seen Terrain names.
- Poisoned full Unit construction `7353C0` establishes raw disguise false,
  creation zero, NULL type/House, and timer start at construction frame with zero
  duration. The middle timer word `+1E4` remains opaque. It is observed in raw
  reads/writes and raw load, without assigning it a Cell or gameplay meaning.
- UnitType `74719B` starts Facings at eight. ART FiringFrames is read at
  `74780F`, using the current signed-byte default and storing the low byte.
  `747930..747944` changes the current Facings default to one when that byte and
  Turret are both zero; `74795C` then reads exact `Facings` with the current
  default. The reached reader does not clamp authored Facings. `unit_art_controls`
  execute these original blocks, including byte truncation and retained defaults.

## Damage, lifetime and persistence

The damage rows execute original Unit/Foot/Techno/Object receivers with physical
AP warhead and pre-call HouseType armor, actor armor and veterancy readbacks.
Original `701FCB..702021` excludes results zero and four, gates Can/Perma, invokes
virtual clear `+470`, and writes reveal start plus twice the final modified damage
packet. Numeric outputs come from execution, including the defended packet and
admitted lethal cap. The lethal row does not certify every downstream death effect.

Clear controls distinguish Unit `746720`, Infantry `522780`, and base `41C030`.
Unit clears type and House and dirties radar; ordinary Infantry/base clear only
the raw flag. Authored permanent Infantry defaults are bounded controls, not the
full Spy lifecycle. Attached-ring rows supply an unconstructed Anim-sized buffer
and execute the reached `+19D` hidden-byte tail using original House `50B6F0`.
They do not establish capture-manager or Anim construction/lifetime parity.

Raw-load rows reuse `naval_lifetime_controls.raw_load_sound_reset` in the same VM:
original Abstract Load, Foot sound reset, Foot no-init constructor and Unit vtable
reconstruction execute. Saved object bytes and external pointer identity are
supplied. The existing original Drive is explicitly reattached for the subsequent
Update calls because no-init clears that interface. This proves the measured raw
field/timer continuation, not original Save or complete COM/vector/swizzle/scenario
reload. Whole-scenario RNG reseeding has a separate owner and comparison.

Cleanup executes Foot UnInit and the deferred Unit/Foot/Techno destructor chain,
including original token-vector startup `633900`. Registry readbacks qualify
removal. Freed raw bytes are not interpreted as live state. The selected object
has no attached Anim, bomb, target or planning group.

## Presentation boundaries

Observer controls execute `70EE30`, Unit type/House getters, `4DED70`, `70ED80`
and the admitted DrawIt selector. They include signed phase boundaries, enemy,
owner, allied viewer, sensor, selection and blink controls. `flags_arg256` is a
direct helper result; the actual drawing caller separately gates it by current
player control and raw disguise.

Original Terrain InitTheater `71DCA0` forms filenames and loads the complete
physical tree images through canonical CCFile/RawFile transport. Original
projection initializers `6D1830`, `6D18C0` and `6D1BB0` establish the height scale.
The drawing rows execute UnitSHP `73C5F0` and TechnoDraw `705E00`, recording body
and shadow `4AED70` arguments. Actual Unit ART layout, body counter, resolved
facing, locomotor predicate, world Z, above-ground height, Cell state and depth
coefficients are independent pre-call inputs. The image route remains UnitSHP,
not TerrainClass drawing. A supplied Cell Convert/light and the shape-argument
sink bound this comparison: no native pixel or GPU parity is claimed here.

Radar rows start at admitted `655F48` with flash inactive and reach the original
BSurface pixel write. The NULL Unit disguise-House path reads ColorScheme `+330`
directly; the corpus retains the final stored word, not an invented LightGrey RGB.
ColorScheme constructor index and registry/name prior are explicit. This does not
establish complete radar visibility, caching or surface scheduling.
