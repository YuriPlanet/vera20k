# Shrapnel wooden damage occupants

This packet executes the original active-retail MTNK/Drive admission, copied-Health C4 receiver, marked-world cleanup, list traversal and Cell-target Detach. It reuses `anytown_damage.anytown_occupants.Occupied`; the adapter supplies Shrapnel scene data and adds observations, without copying a VM or native gameplay implementation. The original `occupants` corpus stops at animation requests. The separately frozen `occupants_joined` corpus executes those constructors in the same VM and streams; it is the Rust comparison input.

The physical input is `XShrapnel.MAP`, SHA256 `c32e412e938e1b9a0b29f89ff9bc0e6f410f75bc76701fb4acef6581fd19b438`, SNOW, native Size82,82. The existing `shrapnel_repair.ResidentRepair` executes `570050 -> 57F200 -> 57FBC0` with actual TMP bodies, overlay/type readers and all nine affected `47D2B0` calls. This establishes the repaired low wooden span x114..116, y58..60 at ground level2/Z208, raw bridge flags0. No structural `47DD70` is admitted.

Each case then retains one VM across two calls to the **already-admitted** area branch `48A25A..48A26A`. Those instructions call the actual `57BAA0` controller and call `70D4A0` only after a true root return. The preceding strength roll, weapon firing and projectile impact are outside this packet. The actor/world/head fields and frame1000 are supplied active-state inputs, not native Unlimbo or movement-order proof. The actor starts with mission5; two explicit suspended-mission1 cases test restoration. This packet does not demonstrate a later ordinary Attack-to-Guard scheduler transition or absence of future shots.

## Native results

First damage changes the three rows to87/89/88. All nine occupant callbacks receive mode0 and Road; root returns AL0. Every case preserves its occupants, targets, timers and all three RNG states.

Collapse changes those rows to90/101/91. The y58/y60 callbacks remain mode0/Road; the y59 callbacks receive mode1/Water. Callback order is north centre/left/right, south centre/left/right, impact centre/left/right. Root returns AL1, then the area caller detaches the impact Cell.

| Case | Collapse result |
| --- | --- |
| `resident_center` | MTNK on115,59 receives copied Health300 through Super, dies and unlinks from cell/Logic/display. |
| `north_head_center` | MTNK on Road115,58 with a retained Drive head at115,59 receives the same damage and dies; its current cell remains Road. |
| `north_stationary` | MTNK survives, and Cell-target Detach clears its target. |
| `north_restore_replacement` | MTNK survives; the actual Restore chain retains replacement target115,57 and mission1. |
| `north_restore_same` | MTNK survives; the restored same Cell target is cleared, with mission1 retained. |
| `resident_two_members` | Both residents die: the resident pass caches `Object+30` before the callback. |
| `north_two_heads` | Only the first incoming tank dies: the neighbor pass reads `Object+30` after unlinking, obtains null, and leaves the second tank alive. |

In the request-boundary corpus, the first measured death requests Main Next, Scenario range0..1, Scenario range0..14, then Scenario Next. Seed0 returns1229352179,1,6,2424954917 respectively. It advances Main indices0/103 to1/104 and Scenario0/103 to3/106; MapGen stays unchanged. These values deliberately exclude animation construction and are not the joined production expectation. The two-death case retains its later actual draws and complete resulting states too. RadarCombatFlash writes are start1000/duration49 at +174/+17C; the observed +178 write is padding, not another timer value. Rearm +640 state remains unchanged in this composition.

## Joined constructor results

`occupants_joined.py` keeps the same physical repaired crop and seven supplied actor cases, then lets original `421EA0` execute instead of returning at the request seam. It reuses the existing `bridge_anim_inputs.Reader` for physical ART/SHP input and `anim_bouncer_launch.constructor_state` for state decoding. Original Bouncer4224D9..422648, BounceInit4397E0, Unlimbo5F4EC0, Logic registration55BAA0, Display submission4A9720 and Start424CE0 execute in the same VM. No native instruction, gameplay return or RNG result is replaced.

The original type factories/ART reader load the native MetallicDebris and MTNK Explosion candidates. `D` really has no ART section and retains constructor state. The selected physical SoundList reader extends the existing GenVehicleDie registry with the authored Report names, so the actual Start request executes. Sound indices remain fixture-relative; playback stops at7509E0. Original FlightLevel and DropZoneAnim constructor defaults and layered General readers execute. Game-speed index4 is supplied for normalization.

The DBRIS7LG constructor adds six Scenario draws. Its equal1..1 RandomRate call also executes but consumes no raw word. This changes the following explosion selection from the request-boundary corpus's S_BRNL58 to **TWLT070**. One lethal callback ends at Main1/104, Scenario9/112, MapGen0/103 and admits DBRIS7LG followed by TWLT070. Both enter native Logic and Display layer3 before the actor's removal completes. Native constructor IDs advance by1 then2 from the supplied pre-controller cursor. Timers start at1000 with duration1; loop counts are255 and1 respectively. The raw Bouncer position/velocity, binary64 parameters, quaternion bytes and constructor runtime fields are retained.

The two-resident case kills both tanks and produces DBRIS7LG, TWLT070, then S_TUMU60; Main ends2/105 and Scenario11/114. The two-incoming-head case leaves `mtnk2` before the two new animations in Logic. The three surviving single-tank cases create no animations and consume no words. Every case retains the full three streams, ordered Logic/Display/Anim registry and constructor requests/results, not only these summaries.

Joined setup corrects one supplied-boundary inconsistency from the preserved v1 packet: two actors now initialize both the per-type House item and native House+5574 total to2. The Rust projection compares the directly read House total with `HouseTracking` after subtracting unrelated scene units; it does not compare native inventory with `EntityStore`'s insert/remove storage count. Raw per-type and global vector counts stay in the full native packet. The projection compares ordered memberships instead of inferring them from Alive/Limbo, and subtracts the explicit pre-controller native ID cursor only to compare allocation order.

The existing Rust Bounce owner stores its presentation spin as axis/angle rather than the native quaternion block. The joined packet preserves those quaternion bytes, but the selected production comparison covers Bouncer position/velocity/elasticity/gravity/clamp and shared RNG, not quaternion rendering.

Native sound requests retain GenVehicleDie before Explosion09; existing Rust damage delivery publishes the Anim report before the queued death sound in the same frame. This presentation-order residual is recorded in `damage_consequence_tests.rs`; the joined comparison excludes sound ordering and playback.

## Reproduction

Use the repository's documented native-oracle Python environment. Configure the original executable with `VERA20K_GAMEMD_EXE` or `RA2_DIR`, and set `VERA20K_SHRAPNEL_INPUTS` to the flat extracted retail directory required by `shrapnel_repair`, including `SOUNDMD.INI`. The joined runner also needs physical `ARTMD.INI` and SHPs for DBRIS1LG through DBRIS9LG, DBRS10LG, DBRIS1SM through DBRIS4SM, TWLT070, S_BANG48, S_BRNL58, S_CLSN58 and S_TUMU60. The native requested filenames, complete byte hashes and SHP metadata are retained. No proprietary asset bytes are bundled.

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_test_vectors --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_test_vectors --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_joined --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_joined --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_joined_test_vectors --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_joined_test_vectors --check
```

The promotion identity prevents `--write` from accepting changed native values. Publication only replaces copied lexical INI fields with hashes. The uncompressed Rust projection reads the frozen native packet; it does not calculate gameplay outcomes. Shared-owner parameterization was separately replayed against the immutable original Anytown payload and its independent check passed.

## Coverage limits and production comparisons

The initial crop/scalar/zone input comes from the frozen supplied production boundary, checked against physical MAP bytes; this is not native full-map loading. All nine required initial and mutation Recalc calls execute. Connectivity and hierarchy callbacks are recorded return boundaries here; the separate navigation packet owns those bodies. MTNK/Drive constructors and selected original layered readers execute, but actor placement, House setup, cell/list/global registration and the incoming head are supplied.

In v1, the sound reader establishes a reduced GenVehicleDie registry at index0; animation construction and sound playback stop at request boundaries. The joined packet executes animation constructors and Report requests as described above, while sample IO/playback, later AnimAI/debris flight/contact/damage/smudge/expiry and their further RNG remain excluded. No deferred-delete drain or complete Logic scheduler runs. Main/Scenario/MapGen are independently original-seeded0 **after** actor setup, so the recorded repair-prefix RNG is not presented as a continuous loaded-world history. Untagged physical cells exclude trigger propagation.

Production comparisons should start from the ordinary repaired Shrapnel fixture and assert the mode/land/callback sequence; resident and incoming-head lifetime outcomes; retained second-member behavior; final cell/Logic/display ownership; target/restoration outcomes; and all full RNG states at a deliberately aligned callback boundary. The ordinary movement producer and ordinary firing/impact scheduler need their own joined production/native evidence. Physical retail explicitly supplies CliffBack2 and this witness retains that read.

## Selected lethal timer boundary

`occupants_timer.py` is a thin observer of this same native owner. Its separately reproduced four lethal cases record the original +174/+178/+17C stores, but no CPU read of that range and no `70D990` radar-update entry before the synchronous controller returns. The original terminal state has Alive0/Limbo1, no marked cell/Logic/Display membership, and deferred deletion. The second incoming-head actor survives and receives no timer store. These observations preserve the original case values from the external timer witness under an immutable digest.

The output also rereads original instruction bytes for the timer writer, reader, ordinary AI caller, tube-only caller, Limbo removal gate and alternate TriggerAction caller. Native Unit vtable7F5C70+4A0 points to70D990. Its owner-gated timer reader at70DBDE/70DC03 calls+49C→70CCF0→Map6562D0. Techno Limbo conditionally removes an existing radar entry through+498→70CCC0→Map655740 and clears+423. The supplied native actor boundary did not execute native radar registration, so this conditional armed-radar removal is instruction evidence only.

Rust does not retain RadarCombatFlash for these dead actors, and the seven-case Rust test does not claim timer or rendered-radar parity. A separate TriggerAction6E21E0 traverses global Units and can call+4A0 independently of Logic membership. No such trigger is executed in the selected lethal callback witness. Surviving ReceiveDamage flashes, trigger interleaving before deferred deletion, and rendered radar behavior remain outside this proof; there is no global radar-closure claim.

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_timer --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.occupants_timer --check
```
