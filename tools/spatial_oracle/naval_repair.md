# Low wooden bridge repair and surface-ship sinking

The physical Shrapnel Mountain map (`XShrapnel.MAP`, SHA256
`c32e412e938e1b9a0b29f89ff9bc0e6f410f75bc76701fb4acef6581fd19b438`)
has a broken ground road at x114..116,y59, level2. Its overlay101 uses the
underlying Water terrain; repair changes it to a healthy Road overlay. This
does not create a raised deck or change the physical water outside the span.
This is the LOBRDG01..28 family (OverlayTypes74..101, wooden physical art),
distinct from LOBRDB01..28 (205..232, native Name `Concrete Low Bridge`).

The native-only corpora use original `gamemd.exe`:

- `naval_occupants` executes Cell487A10, Unit73F0A0/Foot4D9C10 admission,
  Ship6A3F50 AtCoord, and Unit737C90→Foot4D7330→Techno701900→Object5F5390.
  The 22 rows include a resident ship, moving-head neighbors, height-tolerance
  boundaries, and explicit type/receiver gate controls. A resident on repaired
  Road reaches Death_Explosion. A ship still on adjacent Water whose head
  matches the repaired cell returns Health1/Alive1/+3CD1. Main, Scenario and
  MapGen states remain unchanged during the measured repair receiver.
- `naval_sink_tick` executes 15 bounded UnitAI7364A1 visits after that receiver:
  raw Z drops by5, height below -400 calls a second RecordKill before UnInit,
  and nonterminal frames divisible by4 request Y then X Scenario jitter
  [-170,170] before WAKE1 construction at ground Z. Two range requests can
  consume three raw draws; complete states and original constructor arguments
  are retained. Terminal rows stop at FootUnInit entry, wake rows at the Anim
  constructor entry. Those boundaries do not prove final cleanup or playback.

The fixtures execute original constructors and selected native reader bodies
against physical exact-case lexical INI caches. Original archive/file loading
is excluded. Inputs include native-read AEGIS Strength800, Float/Water movement,
Weight4, Naval1, Underwater0, Organic0 and Crewed0; CombatDamage/C4Warhead is
Super and General/ShipSinkingWeight is3. The actual production mode filename is
MPBattleMD.ini; its295 bytes are identical to MPBattle.INI in this installation
(SHA256 `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963`).
It has no affected reader keys. LANGRULE.INI is absent in the recorded fixture.

Bounds of these two initial packets: the receiver rows supply finalized cell land and
object/list state. They do not execute original map spawn or route production.
Full-return rows retain constructor IsOnMap0; marked controls stop before the
fatal postlude. Do not infer preserved ground occupancy from those rows: native
Unit737F7A calls Mark(UP) after restoring Health1, while +3CD suppresses the later
UnInit. Sinking +3CD/+3CE is separate from crashing +425/+426. Any additional
marked-world, audio or production witness must state its own coverage.

To reproduce, extract RULESMD.INI, MPBattleMD.ini and XShrapnel.MAP from the
retail install with the asset tool, keeping its returned provenance. Put the
optional LANGRULE.INI in the same directory only if the installation has one.
`asset extract <NAME> --out target/shrapnel-native-inputs` writes under `extract/`.
With the native-oracle Python environment and original executable configured:

```sh
export VERA20K_SHRAPNEL_INPUTS=target/shrapnel-native-inputs/extract
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/naval_occupants.py --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/naval_sink_tick.py --check
```

Both default to read-only check; `--write` explicitly regenerates references.
Portable-path integration and correcting the mode filename changed no native
case output. The metadata preserves original byte slices, reader boundaries,
allocator/OS seams and complete input provenance. Rust tests bind these outputs
directly; hand calculations and Rust results do not generate the goldens.

## Connected lifetime and production evidence

The later packets close specific input and continuation boundaries above. Each
adjacent `.meta.json` gives its exact entry points, substitutions and limits.
These compositions do not claim native whole-scenario loading or frame timing.

| Packet | Executed coverage | Remaining boundary |
| --- | --- | --- |
| [shrapnel_repair](shrapnel_repair/README.md) | Both hut selectors; 570050→57F200→57FBC0; physical TMP Recalc; MapGen variants; all 26,896 base cells, 13 movement rows and the full three-level navigation hierarchy before/after | Initial loader chronology and occupied cells are separate |
| `naval_head_producer` | Ordinary Unit destination setter→Ship Process→Foot FindPath→real AStar; head enters repaired114,59 while hull remains on Water113,59 | Native hierarchy and physical cells are composed; no full scenario startup |
| `naval_lifetime_cleanup` | Four marked-world cases through Mark(UP), retained head/track and Logic/Display membership; terminal UnInit, Limbo, list removal and deferred self deletion | Sparse marked actor; full spawn and final destructor drain are excluded |
| `naval_lifetime_audio` | Constructor defaults, layered sound readers and nine Foot sound-edge cases, voice before attached sound/fallback | Seven reachable Boolean cases and two raw-byte controls; playback observed at call boundary, not audible capture |
| `naval_lifetime_controls` | Two selection controls, repeated direct fatal damage, two raw-load sound-reset controls | Original selection remains allowed; full scenario loading is excluded |
| `naval_sinking_clip` | Constructor; 28 clip entries including repeated sequences; 15 original Unit/Object projection-admission boundaries; three admitted AEGIS draw visits; signed waterline capture/clip; shadow and entire DrawExtras gates | Native viewport writer prefix executes; physical raster is composed from its separate packet; full Display iteration excluded |
| `naval_draw_bounds` | Physical AEGIS rules/ART reader, original Ship matrix and complete voxel raster at four cardinal facings, composed with waterline | Flat, no Rocking, one part, frame0; no whole-frame RGB claim |
| [naval_house_stats](naval_house_stats.md) | Full original House Save/Load: sinking loss1 survives, then RecordKill makes2; mixed kill/loss/score/built counters round-trip | Bounded services; existing Rust aggregate score representation is not newly certified |

`sim/world/sinking.rs` owns Techno+3CD/+3CE separately from crashing. The fatal
receiver restores Health1/Alive1, stuns again and calls Mark(UP), retaining the
head/track and Logic/Display lifetime while removing cell membership. Foot skips
locomotor Process and batched wakes; Unit AI lowers Z until terminal RecordKill
and shared UnInit. Sinking is illegal for firing/targeting, but selection remains
allowed. The exact-zero Object callback owns the initial kill before HP1 is
restored; later shared cleanup avoids duplicating that callback.

The existing House statistics owner now persists its six live Rust totals.
Dropping the initial loss on load changed terminal score and its later RNG draw.
Native House streams demonstrate retention. Snapshot 227 includes sinking,
statistics and a neutral presentation supplement; earlier hash projections omit
the additions and default-zero additions preserve the former folds.

`render/sinking.rs` owns signed Techno+3CA by stable ID, outside simulation and
its deterministic hash. First draw captures the native composite raster bottom
without clipping; subsequent positive rows clip even after the sinking flag
clears. Negative/zero rows recapture while sinking. Deletion prunes the cache;
new-world entry clears it; successful app load replaces it and failed load keeps
the old presentation. The shader preserves geometry, UVs, palette conversion and
depth while discarding below the world row. Sinking suppresses shadows and
DrawExtras, and the radar tracker consumes the authoritative sinking state.

The first waterline capture depends on the native draw admission: original
Unit73B0B0→Object5F4B10→CoordsToClient2 6D2140 admits X[-360,width+360],
Y[-180,height+180] of the tactical rectangle. Unit and Bullet drawing share that
admission owner in `src/app/presentation/instances/helpers.rs`; Aircraft retain
their existing admission. The original AEGIS sequence first admits client(840,200)
in a640×480 view and captures world row2593; at supplied later Z8, then after
panning X, the native cache still clips at client row228.

### RNG, timers, detach and loading

Repair requests MapGen [0,3] three times, yielding 1,1,2 in the saved state; the
four-way Rust owner compares complete native RNG states. The measured repair
receiver and repeated direct fatal callback leave all three RNG streams unchanged.
Nonterminal Unit AI visits lower Z by 5; frame&3==0 requests Y then X Scenario
jitter [-170,170], including rejection draws, then WAKE1 at ground Z with flags 0x600
and one loop. Terminal visits take no wake jitter. This is reached-visit cadence,
not a wall-clock timer. The second Stun cancels orders through the existing owner;
retained locomotor state stops advancing. Foot changes +3CE once and uses the
existing object-sound release owner. Raw load preserves +3CD/+3CE/+3CA while the
Foot load tail resets +544/+53C/+540. Terminal cleanup uses shared detach,
target-release, list-unregister and pending-delete owners. The physical Rust test
checks the final drain; native cleanup stops after deferred deletion is queued.

SOUNDMD and ARTMD are fixed unlayered catalogs. Type/general sinking-sound defaults
are None. The original layered reference reader retains a prior valid sound when
a later reference is absent, empty or invalid. Passenger/crew branches, malformed
inputs and other ship/bridge families are not certified by this packet.

### Production checks

[`bridge_low_repair_tests.rs`](../../src/sim/world/techno_ai/bridge_low_repair_tests.rs)
loads physical Shrapnel, issues ordinary Engineer Enter and vehicle Move commands,
and compares repaired surface/navigation fields with native evidence. FV crosses
the repaired Road at rawZ208 without a deck; two independently restored approach/
repaired futures also cross. The naval case produces the movement head, repairs,
checks retained and terminal lifetime fields over 150 frames, then compares two
independently restored 120-frame futures including complete RNG, hash and both
losses. Exact Rust frames 29/110 are regression observations, not native scene timing.

Focused tests bind native sink cadence/RNG, audio, repeat direct damage versus
area exclusion, firing/target legality, statistics and app cache persistence.
The ignored physical AEGIS test compares all four native bounds and complete
palette-byte hashes. The ignored GPU test sends all 16 Unit clip visits through
real batch lowering, upload and Metal shader, checking every 640×480 pixel and
unchanged depth. These checks do not close the whole-bridge goal.

### Reproduce the additional packets

Use the physical inputs in
[`shrapnel_repair/retail_manifest.json`](shrapnel_repair/retail_manifest.json), plus
SOUNDMD.INI, ARTMD.INI, aegis.vxl, aegis.hva and voxels.vpl, extracted with retained
asset provenance into the same flat external directory. Use the recorded SNOW
archive for TMP files. No retail asset bytes are committed.

```sh
export VERA20K_SHRAPNEL_INPUTS=target/shrapnel-native-inputs/extract
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=.
python -m tools.spatial_oracle.shrapnel_repair.validate_packet --check
for packet in naval_lifetime_audio naval_lifetime_cleanup naval_lifetime_controls \
  naval_sinking_clip naval_draw_bounds naval_house_stats naval_head_producer; do
  python "tools/spatial_oracle/$packet.py" --check || exit
done
```

The Shrapnel packet records a separate required CliffBack constructor/retention
follow-up. Other low/raised types and affected gameplay mechanisms remain work
for the whole-bridge audit.
