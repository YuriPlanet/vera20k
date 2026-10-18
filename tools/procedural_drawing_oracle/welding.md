# Engineer IFV welding controls

The optional `--welding` mode of [electric_bolt.py](electric_bolt.py) reuses its
original-image reader/world, Spark continuation, RGB565 surface and light
owners. It writes the separately consumed `welding.json` and
`welding.meta.json`. The default electric-bolt corpus is independent.

From the repository root, with the original retail installation configured:

```sh
python tools/render_depth_fixture.py target/ordinary-ifv-welding.map --ifv-welding repair
python tools/render_depth_fixture.py target/ordinary-ifv-full-health.map --ifv-welding full-health
python -m tools.procedural_drawing_oracle.electric_bolt --welding \
  --welding-map target/ordinary-ifv-welding.map \
  --welding-map target/ordinary-ifv-full-health.map --check
```

`--write` deliberately regenerates those two references. The physical map
payload identities are in `type_inputs.profiles[].map`; each supplied map must
be unchanged scenario INI, not its MMX container. Hills is always read from
the original loose `Hills.mmx` through the existing MIX owner. Its
`HILLS.MAP` inner entry is `D5FE80AC`. RULESMD, optional LANGRULE and Battle
mode precede each selected map. ART is not a rules layer.

The existing `render_depth_fixture.py --ifv-welding` modes own these fixtures.
Their scenario rows place owned stock FV/ENGINEER/E1 and damaged/full MTNK without type or
timer overrides. The full-health variant changes only the damaged MTNK's
authored fraction to256. Compare regenerated map bytes with the recorded
`type_inputs.profiles[].map` payload identities before substituting them for
archived production inputs. This oracle neither creates a second map generator
nor supplies a gameplay health conversion. Existing `object_health.py` owns native authored-health
arithmetic; existing IFV boarding evidence owns passenger binding.

## Consumed groups

| Group | Original body and declared boundary |
| --- | --- |
| `type_inputs.profiles` | Whole original SystemType/ParticleType/Weapon constructors and readers consume exact physical selected strings, retaining constructor defaults and each layer's state. Fresh FV/ENGINEER IFVMode and fixed turret-table histories call the existing `ifv_turret_switching.Machine.read_layer` unchanged. Its narrow reader scope excludes Gunner, passenger limits and numbered weapon fields. |
| `constructor_priors` | Existing original Unit/Foot/Techno prefix and Drive constructor, real RTTI and observed IDs. All three actors retain their original Object-constructor expiry subscriptions. Full Unit construction, placement and Logic participation are excluded. |
| `constructor_cases` | Whole `62DC50` with real Entity/Cell/null target, owner/null, different/null owner TarCom, native zero fifth coordinate and one declared nonzero coordinate. Original Cell getter/RTTI and bridge adjustment run. Retail SmallGreySSys/SmallGreySmoke and SparkSys/Spark also use the same constructor. |
| `birth_cases` | FireAt `6FF1AD..6FF1FB`: flag/link gate, allocation, full constructor and retained+308 store. Muzzle and target are explicit caller priors; earlier Bullet/healing/FLH/full-shot execution is excluded. |
| `admission_cases` | Original FireAt live-link guard, GetFireError T37/T46 blocks, full GetROF and actual FireAt burst/GetROF/Berserk/rearm/IDIV tail. Entries use declared register/stack priors and stop at actual continuation or return. This does not execute the whole GetFireError caller family. |
| `lifecycle_cases` | Whole Techno/System PointerExpired with exact/different pointers and both removal controls; actual target setter; Stop's null-destination/null-target setter fragment. Full Stop Event admission/mission dispatch is excluded. |
| `live_case` | Uninterrupted existing native live Logic/System/Spark/child AI and original725C70 deferred drain after every Logic visit, bounded to600 visits. Retains spawn boundaries, last-live and final pre-drain System state, post-drain live rosters/pending/link/IDs/RNG and one real produced child. It never treats freed allocation bytes as a live System. This is an isolated flat-cell effect continuation, not whole Unit/Bullet scheduling. |
| `spark_draw_cases` | Whole `62CEC0`, actual FPS/detail/fog gates, projection/clip, full-u16 A, strict signed candidate/unsigned Z, native interpolation/modulation/packing and original BSurface pixel store. Valid physical color states and an originally produced Welding child are used. |
| `system_draw_cases` | Whole `62E280`: current count/cap clamp, PC53/chop multiply/ftol, temporary light construction, current F4 stage, whole `5FF850`, destruction. No persistent-light ageing is substituted. |
| `mixed_draw_cases` | Whole System and Particle bodies execute in both component orders on the same retained destination, with no intervening reset. Sparse packed pixels demonstrate order dependence. Native Display-loop/common Object Render admission is excluded. |
| `light_masks`, `stage_table_hex` | Original normal-bank mask-loop prefix and original80-byte stage table. Only indices actually reached by selected whole light draws are retained. Type16 is outside this corpus. |

The sidecar pins the original binary, commands and boundaries. It contains no
repository source hashes. Native results are outputs of the original image,
not hand-calculated values or Rust results.

Deferred cleanup reuses `native.initialize_empty_windows_seh` and the existing
IFV retirement transport for empty Windows FS exception-chain storage and
checked IsBadReadPtr/atomic imports. The original CRT RTTI and receiver bodies
execute; this supplies no class, expiry, timer or RNG result.

After `initialize_effect_registries` runs original `7252D0` and resets the
expiry-listener header, the existing `prepare_spark_capacity` owner supplies
empty `B0F720` capacity once before the first actor. Multi-actor preparation
asks `shroud._prepare_unit_rank` to reuse that capacity. Original Object
`5F3A76..5F3A8B`
appends each actor to `B0F720`; original `72593E..72595F` traverses that registry
independently of Logic. Each actor must remain registered exactly once after
all three constructors. The helper's default retains its existing fresh-registry
behavior for older callers.

## Schema details

Whole Draw uses ECX as receiver and two stack arguments, cleaned by RET8.
For ParticleDraw, its entry subtracts0x24 bytes and saves four registers;
therefore `62D06D`'s `[ESP+0x3C]` is caller argument2, and `62D075`
dereferences that bounds pointer. The selected Spark arm ignores argument1;
SystemDraw reads neither argument. Controls supply `(NULL,bounds)` without
changing the older oracle callers.

`constructor_cases[].state` records `position`, Location-minus-owner `offset`,
`target_coords` (native D4/D8/DC), `owner` (E0), `retained_target` (E4), native
ID, F0/F4/EC/E8, flags, Logic/Alive and child membership. Pointer roles are
`owner`, `target`, `other_target`, `cell`, `system` or null; native pointers are
not Rust entity IDs.

The supplied constructor target determines D4 via its actual virtual+48;
with no target the fifth coordinate supplies D4. With a Techno owner, E4 is
the owner's current TarCom. Without an owner E4 is the supplied target.
Original initializer `6F2A50` writes `[0,0,0]` to `B0EA90`; its bytes are
recorded in birth rows. A misleading historical NullCoord label is not a
different coordinate value.

`link` / `rearm` has `spark_link`, `target`, `burst`, stored `rof` and raw
three-dword `rearm`. The middle timer dword remains opaque supplied storage.
The+298 tail control is Berserk. Admission inputs declare the currently
queried weapon flag and actual retained native link independently. The native
link may outlive the weapon selection that created it.

`constructor_priors[].expiry_registered` and the live/logical
`expiry_registered_roles` observe the retained actor subscriptions. The final
`retirement_before_drain.pending_particles` contains at most five still-allocated
queued children, recording native ID, Alive/Limbo/on-map/Logic bytes, the Display
layer and membership, Object/Particle/expiry registry membership, parent-vector
membership and deletion mark. Later post-drain rows read live rosters only.

Spark `62ED0E..62ED33` walks marked children backward and calls their virtual
`+F8 = 5F65F0`. This UnInit announces expiry, calls Limbo, clears Alive and
queues the allocation. System PointerExpired `62FE90` removes the child from
its active vector during the announcement. Particle Ready `5F6690` returns
`!Alive`; the pending allocation still belongs to the Object/Particle/expiry
registries until scalar destructor `62D9A0` and Object destructor `5F3B80`
remove it. System scalar destructor `630230` delegates to `62E070`, whose
nonempty child loop calls scalar destructors directly and erases pending
entries; it does not call child UnInit. The selected natural continuation
reaches that destructor with an empty active child vector.

Default `RngTrace` case seed1 means Main1, Scenario2, Mapgen3. The boundary
resets follow the supplied mobile preparation; its constructor draws are
separate priors. Natural live snapshots retain the complete Scenario state
and cumulative raw/ranged request counts. Main/Mapgen invariants are explicit.

Drawing inputs contain raw u32 FPS/minimum/buffer values, signed world/camera
coordinates, signed i16 `z_origin_term` / `z_bottom_term`, and16-digit numeric
hexadecimal `*_f64_bits` strings. `a_default`/`z_default` and `[x,y,u16]`
patches retain the complete word domain. Physical A producers and the common
Object Render prefix have their existing independent owners.

`spark_draw_cases[].draws` records threshold/performance/detail facts, optional
raw fog result, projected screen, signed candidate, A/Z words, interpolated
and modulated RGB and the actual `write:{screen,packed_value}` or null.
Unreached observations remain null. Active-retail `5865E0` returns false;
that raw value means the fog call permits continuation. A nonzero damage
field may permit continuation even when `performance_passed` is false.

Point cases cover zero/low/threshold/high A (including256 and65535), strict
Z equality/sides, negative candidate, signed term wrapping/row origin,
viewport/clip, unsigned FPS hysteresis overflow, color transition, ordered
overlap, signed/overflowing planar products and a real produced child.

`system_draw_cases[].light` is null when SystemDraw never constructs one;
otherwise it records `base_size`, current `stage`, `flags`, actual
`surface_index` when reached, queried `threshold`, `performance_passed`,
projection/fog/admission facts. Offscreen rejection and FPS rejection remain
distinct. Native count0/1/6/12/15/30 and current F4 stages17/29/41 bound the
selected physical cap15/LightSize25 arithmetic. Masks are256×128 byte surfaces.

Each draw records sparse changed RGB565 words and asserts unchanged RNG,
native ID cursor, A/Z, destination guards and original executable text. A
temporary system light must leave no persistent registry entry. Malformed
types/color indices, invalid x87 values, unsupported mask indices, complete
Windows presentation, every particle family and full saved-load behavior
are outside these executable component comparisons.
