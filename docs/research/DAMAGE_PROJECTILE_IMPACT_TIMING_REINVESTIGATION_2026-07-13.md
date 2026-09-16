# Damage Projectile Impact Timing Reinvestigation

**Date:** 2026-07-13  
**Task:** Damage authoritative cutover plan, Task 3B  
**Investigation mode:** coverage-map  
**Overall status:** **PARTIAL**  
**Ordinary BulletClass route:** **VERIFIED for static implementation planning**  
**G2 implementation verdict:** **FAIL / BLOCKED on a separate projectile lifecycle design and implementation**  
**Evidence mode:** static, read-only Ghidra inspection of the active Yuri's Revenge gamemd.exe, current repo research, stock INI, and focused current Rust reads. No debugger, game process, retail capture, live input, Cargo command, or Rust edit was used.

## 1. Verdict first

Normal projectile damage is not applied inside <code>TechnoClass::Fire_At @ 0x006FDD50</code>. Fire creates and launches a <code>BulletClass</code>; <code>BulletClass::Fire @ 0x00468670</code> begins by revealing it, and reveal tail-appends it to the singleton live Logic vector at <code>0x0087F778</code>. The main object pass in <code>LogicClass::PerTickUpdate @ 0x0055AFB0</code> reloads that vector's live count after every object's <code>vtable+0x5C</code> call. Therefore a bullet created by an object already being visited can receive its first AI call later in the **same** main-object pass.

Damage is consequently deferred out of the fire call but is **not uniformly deferred to the next frame**. The first bullet AI can detonate in the firing frame, or it can leave the bullet registered and detonate on a later AI visit. The exact ordinary impact chain is:

    TechnoClass::Fire_At
      -> BulletClassAllocate / BulletClass::Init
      -> BulletClass::Fire
      -> ObjectClass::Reveal
      -> Logic vector tail insert
      -> later live vtable+0x5C dispatch
      -> BulletClass::AI
      -> BulletClass::BulletDetonation
      -> WarheadTypeClass::Detonate
      -> Apply_area_damage
      -> ordered fixed records
      -> per-record ReceiveDamage

The native scheduler owner is the **main live Logic vector and its forward live-count loop**, not a projectile-only phase, a snapshot, the current Rust combat batch, or a render projectile timer.

Current Rust already has a promising <code>LogicVector</code> and <code>Simulation::for_each_live_object</code> primitive, but production projectile creation and AI are not owned by that live pass. Combat still creates damage at fire time, homing/rocket movement consumes snapshots before combat, the returned detonation IDs are ignored, and neither projectile state retains the complete impact payload. G2 must remain failed.

The report is marked PARTIAL because it proves the ordinary BulletClass route and classifies the directly adjacent Wave and DiskLaser scheduler positions, but it does not exhaust every special effect producer's damage math. Those effect-specific routes must not be silently treated as the ordinary projectile adapter.

## 2. Scope and evidence discipline

### In scope

- <code>TechnoClass::Fire_At @ 0x006FDD50</code> ordinary bullet creation and launch.
- Active <code>BulletClass</code> vtable identity and relevant slots.
- reveal, live Logic insertion, iteration, self-removal, and same-pass consequences.
- first-AI same-frame eligibility and a delayed-impact branch trace.
- <code>BulletClass::BulletDetonation @ 0x00468D80</code> through <code>Apply_area_damage @ 0x00489280</code> and the concrete receiver.
- exact provenance of every planned <code>ProjectileImpactDamageCall</code> field.
- directly adjacent Fire_At effect routes only far enough to classify their scheduler position.
- current Rust drift and the G2 ownership boundary.

### Non-scope

- exact full trajectory math for every projectile type.
- every special weapon, particle, beam, radiation, death-weapon, or lightning producer.
- renderer cadence and projectile artwork.
- receiver-internal arithmetic already owned by Tasks 1 and 2.
- area collection details already finalized by Task 3A, except where required to prove the adapter boundary.
- Rust implementation or design.

### Confidence labels

- **VERIFIED:** rechecked in the current gamemd.exe body/assembly, or taken from the finalized Task 3A report and cold-checked at its load-bearing callsite.
- **INFERRED:** the evidence fixes the surrounding mechanism but does not uniquely prove the stated semantic label.
- **UNKNOWN / BLOCKED:** evidence is insufficient or implementation ownership does not exist.

## 3. Active BulletClass identity and slot bindings

The active vtable used for this route is <code>0x007E46E4</code>.

| Binding | Fresh evidence | Verdict |
|---|---|---|
| RTTI Complete Object Locator | pointer at <code>vtable-4 = 0x007E46E0</code> is <code>0x007FC7B0</code> | VERIFIED |
| RTTI type descriptor | COL field points to <code>0x0081AF70</code>, whose name is <code>.?AVBulletClass@@</code> | VERIFIED |
| <code>vtable+0x5C</code> | pointer at <code>0x007E4740</code> is <code>0x004666E0</code> | VERIFIED Bullet AI |
| <code>vtable+0xD4</code> | pointer at <code>0x007E47B8</code> is <code>0x005F4D30</code> | VERIFIED conceal/removal entry |
| <code>vtable+0xD8</code> | pointer at <code>0x007E47BC</code> is <code>0x005F4EC0</code> | VERIFIED reveal entry |
| <code>vtable+0xF8</code> | pointer at <code>0x007E47DC</code> is <code>0x005F65F0</code> | VERIFIED uninit entry |
| <code>vtable+0x1F0</code> | pointer at <code>0x007E48D4</code> is <code>0x00468670</code> | VERIFIED Bullet fire/reveal/launch |

Evidence: fresh <code>read_memory(program="gamemd.exe", address="0x007E46E0", length=16)</code>, slot reads at <code>0x007E4740</code>, <code>0x007E47B8</code>, <code>0x007E47DC</code>, and <code>0x007E48D4</code>, plus <code>read_memory(0x007FC7B0, 24)</code> and <code>inspect_memory_content(0x0081AF70, 64)</code>.

This RTTI walk is load-bearing because local Ghidra names are not treated as authority.

## 4. Fire_At creates the damage-carrying bullet

### 4.1 Launch construction

At <code>0x006FE53F..0x006FE55D</code>, Fire_At prepares the seven Bullet initialization arguments and calls <code>BulletClassAllocate @ 0x0046B050</code>. That allocator creates the COM-backed object and immediately calls <code>BulletClass::Init @ 0x004664C0</code>.

The ordinary initialization writes:

| Native Bullet field | Initialization source | Meaning used at impact | Verdict |
|---|---|---|---|
| <code>+0x10C</code> | Fire_At target argument | tracking/target pointer used by movement and detonation snap logic | VERIFIED |
| <code>+0xB0</code> | firing Techno pointer | source object retained to impact | VERIFIED |
| <code>+0x6C</code> | Fire_At's already-modified weapon damage | raw signed incoming damage before the impact scalar | VERIFIED |
| <code>+0x128</code> | <code>WeaponType+0xAC</code> | WarheadType pointer retained to impact | VERIFIED |
| <code>+0x110</code> | computed launch speed | projectile flight state | VERIFIED |
| <code>+0xE0</code> | <code>WeaponType+0x12F</code> byte | launch flag; not a G2 damage field | VERIFIED |
| <code>+0x150</code> | literal <code>0x100</code> in Init | signed fixed-point damage scalar, denominator 256 | VERIFIED |

Fresh evidence: <code>disassemble_bytes(0x006FE520..0x006FE59F)</code>, <code>decompile_function(0x0046B050)</code>, and <code>decompile_function(0x004664C0)</code>.

Fire_At then calls <code>0x0046B260</code> at <code>0x006FE573</code>. Despite its current local name <code>BulletClass__SetOwner</code>, the body only stores its argument at <code>Bullet+0x130</code>. The argument is the WeaponType pointer. It is **not** the impact source-object field; that field is <code>Bullet+0xB0</code> from Init. This is verified label drift.

### 4.2 Reveal and insertion happen inside BulletClass::Fire

The ordinary launch at <code>0x006FF014</code> calls <code>vtable+0x1F0</code>, which the RTTI-backed vtable read resolves to <code>BulletClass::Fire @ 0x00468670</code>. Its first action is <code>ObjectClass::Reveal @ 0x005F4EC0</code>, called at <code>0x00468684</code>. If reveal fails, launch returns false.

Bullet types are logic-enabled by default: <code>BulletTypeClass::Constructor @ 0x0046BBC0</code> writes byte <code>BulletType+0x234 = 1</code>. On the corresponding reveal path, assembly at <code>0x005F5038..0x005F5040</code> pushes the bullet and literal zero, loads <code>ECX=0x0087F778</code>, and calls <code>0x0055BAA0</code>.

<code>0x0055BAA0</code> is membership-guarded: if <code>Object+0x98</code> is clear, it calls <code>DynamicVector::Insert @ 0x005519B0</code>, then sets the byte. Insert writes the new pointer at <code>data[count]</code> and increments count. No sorting or next-frame staging occurs.

Fresh evidence:

- <code>disassemble_bytes(0x006FEFE0..0x006FF02F)</code>
- <code>search_instructions(function=0x00468670, CALL, 0x005F4EC0)</code>
- <code>disassemble_bytes(0x00468670..0x00468697)</code>
- <code>decompile_function(0x005F4EC0)</code>
- <code>disassemble_bytes(0x005F5028..0x005F504B)</code>
- <code>decompile_function(0x0055BAA0)</code>
- <code>decompile_function(0x005519B0)</code>
- <code>decompile_function(0x0046BBC0)</code>

## 5. Exact scheduler owner and mutation semantics

### 5.1 Owner

<code>Main_Tick @ 0x0055D360</code> loads <code>ECX=0x0087F778</code> at <code>0x0055DC99</code> and calls <code>LogicClass::PerTickUpdate @ 0x0055AFB0</code> at <code>0x0055DC9E</code>. The singleton vector's relevant layout is:

- <code>+0x04</code>: pointer array.
- <code>+0x10</code>: live count.

Fresh evidence: <code>get_function_callers(0x0055AFB0)</code> and <code>disassemble_bytes(0x0055DC70..0x0055DCAF)</code>.

### 5.2 Forward live-count iteration

The main object loop at <code>0x0055B608..0x0055B619</code> does:

    object = vector.data[index]
    object->vtable+0x5C()
    count = vector.count
    index += 1
    if index < count: continue

The count load at <code>0x0055B613</code> occurs **after** the AI call and on every iteration. This proves:

1. a tail-appended object can be visited later in the same pass;
2. a compacting removal shifts successors left;
3. the loop still increments its index, so the object shifted into the just-processed slot is skipped for the rest of that pass;
4. there is no cursor repair and no immutable snapshot.

Fresh evidence: <code>decompile_function(0x0055AFB0)</code> and <code>disassemble_bytes(0x0055B5F0..0x0055B624)</code>.

### 5.3 Bullet removal

When the ordinary terminal AI branch detonates, assembly at <code>0x00467F9B..0x00467FB4</code> calls <code>BulletClass::BulletDetonation @ 0x00468D80</code>, then calls <code>vtable+0xF8</code>. The vtable read resolves that slot to <code>ObjectClass::UnInit @ 0x005F65F0</code>. UnInit calls <code>vtable+0xD4</code>, which resolves to <code>ObjectClass::Conceal @ 0x005F4D30</code>. Conceal loads <code>ECX=0x0087F778</code> and calls compacting removal <code>0x0055BAE0</code> at <code>0x005F4DD3</code>.

Detonation therefore completes before the bullet leaves the live vector; removal then has the normal compact-and-skip consequence for the scheduler cursor.

Fresh evidence: <code>disassemble_bytes(0x00467F80..0x00467FBF)</code>, vtable reads above, <code>decompile_function(0x005F65F0)</code>, <code>decompile_function(0x005F4D30)</code>, and <code>disassemble_bytes(0x005F4DB0..0x005F4DE4)</code>.

## 6. Timing traces

These are mechanism fixtures. They prove the scheduler and branch boundaries; they do not claim one minimum-range latency for every stock projectile.

### 6.1 Same-frame appended-bullet trace

Fixture:

- the main live vector starts the object rung as <code>[attacker A, B, C]</code>;
- the cursor is visiting A;
- A's AI reaches Fire_At and creates projectile P;
- P is a stock-active ROT projectile using the ordinary BulletClass route;
- on P's first AI, the active HomingTrack close-impact result is inside the inclusive impact threshold and no delayed-nuke animation path diverts it.

Trace:

1. A calls Fire_At.
2. Fire_At initializes P and calls P's <code>vtable+0x1F0</code>.
3. P's Fire calls Reveal.
4. Reveal tail-appends P: <code>[A, B, C, P]</code>.
5. A's AI and Fire_At return.
6. The loop reloads count as four and visits B, C, then P.
7. P receives <code>BulletClass::AI @ 0x004666E0</code> in the same Main_Tick call.
8. For the exact ROT close-impact branch, HomingTrack returns a post-step distance at <code>0x00466D31..0x00466D40</code>; AI compares it inclusively with <code>current_speed * 0.5</code> at <code>0x00466DB1..0x00466DF4</code>.
9. Concrete one-lepton-boundary example: with speed magnitude 4, the threshold is 2 leptons. Returned distance 2 selects impact; returned distance 3 does not. This example assumes the other impact/height branches do not override the comparison.
10. The impact path reaches <code>0x00467FA2</code>, synchronously dispatches damage, then uninitializes P.

Result: real receiver calls can occur in the same game frame as fire, after A returns and when the tail-appended bullet reaches its AI position.

Evidence: fresh Bullet AI decompile and <code>disassemble_bytes(0x00466D20..0x00466E1F)</code>, plus the corrected exact-math result in <code>[AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md)</code>. The older <code>current_speed * 90</code> wording is not reused.

Important ordering consequence: P is appended after every object present at insertion time. In the simple fixture B and C run before P damages anything. “Same frame” does not mean “inside A's fire call” or “before every later pre-existing attacker.”

### 6.2 Delayed-impact trace

Use the same initial vector and launch, but on P's first AI:

- the HomingTrack returned distance is outside the close-impact threshold;
- bullet height and collision branches do not mark impact;
- proximity/arming does not mark impact;
- no special detonation state is pending.

Trace:

1. P is still appended and receives its first AI in the firing frame.
2. The non-impact branches reach the normal tail at <code>0x00467FBA</code> without calling <code>0x00468D80</code> or <code>vtable+0xF8</code>.
3. P remains registered in <code>0x0087F778</code> with its updated flight state.
4. The next Main_Tick's main object rung reaches P again in preserved live-vector order, subject to the native compact-removal skip rule.
5. When a later AI visit marks impact, the same synchronous detonation chain runs and P is removed.

Concrete boundary companion to the prior fixture: speed magnitude 4 plus returned distance 3 misses the 2-lepton close threshold on the first visit. If the next visit returns distance zero with no diverting special branch, it impacts on that later visit. This proves a one-or-more-visit delay without asserting a universal stock tick count.

### 6.3 What is and is not guaranteed

| Claim | Verdict |
|---|---|
| A bullet fired from a Techno already executing in the main object loop is eligible for first AI in that frame | VERIFIED |
| Every bullet detonates in its firing frame | FALSE |
| Every bullet waits until the next frame | FALSE |
| Fire_At itself applies HP | FALSE |
| Damage dispatch is synchronous once BulletDetonation calls the warhead/area path | VERIFIED |
| Exact tick count is a property of scheduler position plus projectile state/trajectory/target state | VERIFIED |

## 7. Detonation to receiver

### 7.1 BulletDetonation coordinate

<code>BulletClass::BulletDetonation @ 0x00468D80</code> begins by copying the signed 32-bit Cartesian coordinate triple at <code>Bullet+0x9C/+0xA0/+0xA4</code> into a stack-local <code>CoordStruct</code>. Its target and projectile-type branches may replace or adjust that local coordinate. The final local, not a render cell or screen coordinate, is passed by pointer to <code>WarheadTypeClass::Detonate @ 0x004690B0</code> at <code>0x00469033</code> or <code>0x004690A1</code>.

Fresh evidence: <code>decompile_function(0x00468D80)</code>, <code>disassemble_bytes(0x00468D80..0x00468DD4)</code>, and <code>disassemble_bytes(0x00468FD0..0x004690AB)</code>.

Implementation consequence: <code>impact_coord</code> must be the final signed native world-lepton local after the projectile's snap/airburst branches. It must not be reconstructed from <code>position.rx/ry</code>, isometric pixels, or the original fire target.

### 7.2 WarheadTypeClass::Detonate call to the area dispatcher

At <code>0x00469A56..0x00469A83</code>, with <code>ESI</code> still the Bullet:

    EDX = Bullet+0x150
    EDX = low_i32_signed_imul(EDX, Bullet+0x6C)
    EDX = arithmetic_shift_right(EDX, 8)
    EAX = Bullet+0xB0
    source_house = EAX ? *(EAX+0x21C) : null
    warhead = Bullet+0x128
    ECX = final CoordStruct*
    Apply_area_damage(
        coord=ECX,
        incoming_damage=EDX,
        source_object=EAX,
        warhead=warhead,
        producer_flag=true,
        source_house=source_house)

The multiplication is signed 32-bit two-operand <code>IMUL</code>, retaining the low 32 bits, followed by signed <code>SAR 8</code>. It does not saturate. With Init's default scalar <code>0x100</code>, the value is normally the raw bullet damage, but the scalar remains an authoritative impact-time field.

Fresh evidence: <code>decompile_function(0x004690B0)</code> and <code>disassemble_bytes(0x00469A35..0x00469A97)</code>.

### 7.3 Synchronous per-target receiver

Task 3A proved that <code>Apply_area_damage</code> first completes fixed-record capture and then calls receivers sequentially. Fresh cold inspection at <code>0x00489A97..0x00489AB6</code> confirms each eligible record calls <code>target->vtable+0x16C</code> with:

- a fresh local copy of the original signed incoming damage;
- the fixed record's signed lepton distance;
- unchanged warhead;
- unchanged source object;
- literal false;
- literal false, including <code>ignore_defenses=false</code>;
- unchanged source house.

Fresh evidence: <code>decompile_function(0x00489280)</code> and <code>disassemble_bytes(0x00489A78..0x00489AC4)</code>. Full collection/filter/order proof: <code>[DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md)</code>.

## 8. Exact ProjectileImpactDamageCall provenance

The planned DTO combines producer facts with one collector-record target. A native BulletDetonation call to <code>Apply_area_damage</code> itself has **no target argument**. Therefore <code>target_id</code> obtains native provenance only after Task 3A's fixed-record capture. The DTO is valid as a per-record receiver adapter (or as the record produced by a split impact/collection API); it is not valid as a single pre-collection producer record with a guessed tracked target.

| Planned field | Exact native provenance | Lifetime / boundary | Verdict |
|---|---|---|---|
| <code>target_id</code> | each ordered Task 3A fixed record's target pointer, converted to a safe stable/generational ID without dedup | exists only after area collection; it is not automatically <code>Bullet+0x10C</code> | VERIFIED |
| <code>source_object_id</code> | <code>Bullet+0xB0</code>, initialized from the firing Techno; passed as the area dispatcher source object | retained by projectile lifecycle and read at impact; nullable after native detach/lifetime handling | VERIFIED |
| <code>source_house</code> | if source object is non-null at <code>0x00469A6B</code>, load <code>source_object+0x21C</code>; else null | computed at impact, then passed unchanged to every receiver; not a launch-captured house ID | VERIFIED |
| <code>warhead_id</code> | <code>Bullet+0x128</code>, initialized from <code>WeaponType+0xAC</code> | retained from launch to impact and passed unchanged | VERIFIED |
| <code>incoming_damage</code> | signed low-32-bit <code>(Bullet+0x150 * Bullet+0x6C) >> 8</code> at <code>0x00469A56..0x00469A66</code> | computed once at producer impact; Task 3A copies the same original value fresh for every record | VERIFIED |
| <code>impact_coord</code> | final stack-local signed <code>CoordStruct</code> created from <code>Bullet+0x9C</code> and possibly adjusted by BulletDetonation branches | exact Cartesian X/Y/Z world leptons; unchanged into area collection | VERIFIED |
| <code>ignore_defenses</code> | literal false at receiver call <code>0x00489AA8</code> | per fixed record | VERIFIED |

Boundary rule: do not use the projectile's tracked <code>target_id</code> as the DTO's receiver <code>target_id</code>. Area damage may record zero, one, or multiple objects in native order, including objects other than the tracked target.

The native producer also passes literal <code>true</code> as the area dispatcher's fifth argument at <code>0x00469A7C</code>. That flag is outside the current DTO but remains part of the normal projectile area-dispatch contract and its non-HP behavior. It may be supplied as a route constant; it must not be silently discarded.

## 9. Adjacent Fire_At effect routes

The complete fresh direct-callee inventory for <code>0x006FDD50</code> includes Bullet allocation/init, <code>WaveClass::Constructor</code>, <code>DiskLaserClass::Constructor</code>, particle construction, laser/rad/electric effect spawners, trajectory math, RNG, animation, and sound. It contains no direct <code>Apply_area_damage</code>, <code>ReceiveDamage</code>, <code>WarheadTypeClass::Detonate</code>, or <code>BulletClass::BulletDetonation</code>.

Evidence: <code>get_function_callees(program="gamemd.exe", address="0x006FDD50", limit=200)</code>, plus the RTTI-backed resolution of the ordinary indirect Bullet fire slot.

| Effect route | Insertion/scheduler | Earliest damage position when created by a Techno in main rung N | Classification |
|---|---|---|---|
| ordinary BulletClass | reveal tail-appends to singleton Logic vector; same forward live-count rung N | later in the same rung if first AI impacts; otherwise a later visit | VERIFIED, this report's G2 route |
| DiskLaserClass | constructor appends to separate global DiskLaser array; that reverse AI rung G occurs before main rung N | next PerTick call at earliest, because rung G already ran | VERIFIED timing classification; separate damage adapter |
| WaveClass | constructor appends to separate Wave array; <code>FUN_0053D310</code> processes current count in rung P after main rung N | same frame at rung P if the created Wave survives construction and its splash path is active | VERIFIED timing classification; separate effect adapter |
| laser/electric/rad beam/particle visuals | Fire_At creates effect objects/spawners but has no HP callee | effect-owned; exact downstream damage route varies | DEFERRED outside ordinary G2 |

Fresh checks:

- <code>DiskLaserClass::Constructor @ 0x004A7A30</code> appends to its global array.
- <code>DiskLaserClass::AI @ 0x004A7340</code> calls <code>Apply_area_damage</code> at <code>0x004A76AF</code>.
- <code>LogicClass::PerTickUpdate</code> processes DiskLaser before the main object loop.
- <code>WaveClass::Constructor @ 0x0075E950</code> appends to its separate array after its minimum-distance gate.
- <code>FUN_0053D310 @ 0x0053D310</code> processes the Wave count after the main object loop.
- <code>Wave_splash_forces @ 0x0053CBE0</code> calls <code>Apply_area_damage</code> at <code>0x0053CDB5</code> and <code>0x0053CDD4</code>.

This proves the correction required for earlier research: “not inside Fire_At” is correct, but “uniformly next-tick” is not.

## 10. Current Rust drift and ownership boundary

### 10.1 Existing infrastructure that is directionally usable

- <code>src/sim/world/logic_vector.rs:1..73</code> stores insertion order in <code>Vec&lt;u64&gt;</code>, tail-appends, compact-removes, and serializes the order.
- <code>src/sim/world/mod.rs:840..879</code> owns membership-gated register/reveal and unregister/conceal.
- <code>src/sim/world/mod.rs:992..1022</code> explicitly distinguishes snapshot iteration from <code>for_each_live_object</code>; the latter reloads length and preserves same-pass append plus compact-removal skip semantics.

These primitives match the verified native loop shape. They are not yet the production owner for normal projectile creation, flight AI, and impact.

### 10.2 Load-bearing current drifts

| Current Rust behavior | Evidence | Native mismatch |
|---|---|---|
| Projectile movement runs before combat from a <code>special_movement_order</code> snapshot | <code>src/sim/world/mod.rs:2159..2173</code> | a bullet created during combat cannot enter that already-finished phase and receive native same-pass first AI |
| Rocket and homing detonation IDs are assigned to underscore-prefixed locals and unused | same lines | no impact damage or projectile teardown is attached to the flight result |
| Combat walks <code>live_object_order_snapshot()</code> | <code>src/sim/world/mod.rs:2334..2355</code> | cannot observe a projectile appended by its own pass |
| Combat creates area/direct <code>damage_events</code> at the fire decision | <code>src/sim/combat/mod.rs:2375..2480</code> | HP authority is fire-time, not Bullet AI impact-time |
| The combat batch applies <code>u16</code> damage by saturating subtraction | <code>src/sim/combat/mod.rs:1849..1885</code> | loses the producer's signed i32 path and exact receiver timing/mechanism |
| No production caller attaches <code>HomingState</code> or <code>RocketState</code> | focused <code>rg</code> finds only definitions and inline tests | no authoritative Fire_At-to-projectile creation route |
| <code>HomingState</code> stores tracking/kinematics only | <code>src/sim/movement/homing_movement.rs:47..87</code> | lacks source object, warhead, raw damage, scalar, source-house-at-impact rule, and exact final world-lepton CoordStruct |
| <code>RocketState</code> stores origin/target cells and flight fields only | <code>src/sim/movement/rocket_movement.rs:56..85</code> | same missing impact payload; target cell is not the native final impact CoordStruct |
| Homing proximity is a Rust 192-lepton rule and returns an ID list | <code>src/sim/movement/homing_movement.rs:530..580</code> | not the complete BulletClass AI/detonation mechanism |
| <code>world_hash.rs</code> folds homing state but has no <code>rocket_state</code> reference | focused current <code>rg</code> | persistence/hash authority is incomplete and asymmetric |

The tracked <code>HomingState::target_id</code> is not a substitute for the planned receiver <code>target_id</code>. The latter comes from Task 3A's ordered fixed records.

### 10.3 Required owner

The separate projectile design must make one authoritative Rust mechanism own:

1. Fire_At-time projectile entity creation and complete persistent payload.
2. reveal/tail insertion into the existing live Logic order.
3. per-object AI at <code>vtable+0x5C</code>-equivalent position in the unified forward live-count pass.
4. exact same-pass append and compact-removal skip behavior.
5. flight state, target detach/lifetime behavior, and final signed world-lepton impact coordinate.
6. synchronous impact dispatch before projectile uninit/removal.
7. save/load and deterministic hash ownership of every persistent projectile field.

A fixed “projectile movement before combat” phase, even if deterministic, cannot preserve this contract.

## 11. G2 adapter and gate verdict

### Required adapter sequence

At ordinary Bullet impact:

1. finish Bullet AI's exact impact-coordinate branches;
2. compute signed scaled incoming damage with the native 32-bit multiply/shift;
3. read current retained source object and derive source house at impact;
4. invoke the Task 3A area collector synchronously with literal producer flag true;
5. complete fixed-record capture;
6. for each record in native order, construct the planned call using that record's target ID/distance and the unchanged producer facts;
7. invoke the authoritative receiver before advancing to the next record;
8. return through Warhead/Bullet detonation;
9. uninit/conceal the bullet, preserving live-vector cursor effects.

### Gate verdict

**G2 = FAIL / BLOCKED.**

Reasons:

- no production Fire_At-to-projectile spawn exists;
- existing projectile movement is not owned by the live main object pass;
- detonation results are ignored;
- current projectile states do not retain the required payload;
- current combat still owns fire-time direct/AoE HP authority;
- exact final world-lepton impact coordinate is unavailable;
- required persistence/hash fields are not complete;
- Task 3A's other G2 prerequisites, including exact airborne candidate order, also remain external dependencies.

Per the approved plan, do not directly patch combat damage from this report. Run a separate <code>/brainstorm projectile impact scheduling</code>, obtain design approval, and only then produce a dedicated implementation plan.

## 12. Contradiction and correction ledger

| Prior/current claim | Rechecked result | Classification |
|---|---|---|
| <code>[FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md):548..552</code> says Sonic/Laser have instant fire-time HP paths | Fire_At has no HP/area/detonation callee. Wave and DiskLaser damage is effect-owned after Fire_At returns. Laser/electric/rad effect-specific damage remains route-specific. | WRONG / MISLEADING at the fire-call boundary |
| <code>[L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md):12..27</code> says deferred projectile is uniform and no shot changes HP in the firing pass | No HP occurs inside Fire_At, but an appended Bullet can impact later in the same live object pass, and a Wave can run in later same-frame rung P. DiskLaser first AI waits because rung G already passed. | PARTLY CORRECT call boundary; WRONG uniform timing |
| <code>L2...:70..75</code> calls projectile impact a later-tick gap | A later tick is possible, not guaranteed. Same-frame impact is mechanically reachable. | WRONG as a universal timing claim |
| <code>AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY...</code> preserves scheduler/same-tick eligibility but its old <code>speed * 90</code> scalar and numeric minima were marked provisional | Exact HomingTrack research and fresh assembly show the ROT close comparison uses <code>current_speed * 0.5</code>. This report uses only the corrected branch and does not certify the old minimum-range tick table. | scheduler finding retained; numeric examples not relied upon |
| Ghidra local label <code>BulletClass__SetOwner @ 0x0046B260</code> | body writes WeaponType pointer to <code>Bullet+0x130</code>; source object is initialized at <code>Bullet+0xB0</code> | VERIFIED label drift |
| Current Rust comments describe a production projectile-spawn follow-up | no production caller exists and both detonation ID vectors are ignored | CURRENT IMPLEMENTATION GAP |

This task's sole-output restriction prevents correcting the older documents here. The synthesis/reconciliation owner should consume this ledger.

## 13. Adversarial questions

1. **Could Fire_At hide HP application behind a direct callee name?**  
   No direct HP, area, warhead detonation, or BulletDetonation callee exists. The ordinary indirect launch slot is RTTI/vtable-bound to BulletClass::Fire, which reveals and launches but does not receive damage.

2. **Does “deferred out of Fire_At” prove next-frame damage?**  
   No. The live count reload makes a newly appended Bullet eligible in the same pass.

3. **Does same-pass eligibility prove same-frame damage for every shot?**  
   No. It proves first-AI timing. Impact branches decide whether that visit detonates.

4. **Does stock <code>Arm=2</code> forbid same-frame impact?**  
   No as a mechanism claim: the ROT close-impact branch is separate from the later proximity arming result. This report deliberately does not reuse the stale <code>speed * 90</code> stock-minimum calculation.

5. **Can the planned <code>target_id</code> be copied from the projectile's tracking target?**  
   No. <code>Apply_area_damage</code> receives no target pointer. The receiver target comes from each ordered fixed record.

6. **Is source house fixed at launch?**  
   No. Normal Bullet detonation reads <code>source_object+0x21C</code> at impact if the retained source pointer is non-null.

7. **Can Rust's existing snapshot preserve tail-appended first AI?**  
   No. A cloned order cannot observe membership changes during its own traversal.

8. **Does the existing <code>for_each_live_object</code> primitive make G2 pass?**  
   No. It provides the correct iteration primitive, but production projectile creation, AI, impact payload, and receiver dispatch are not integrated into it.

9. **Can DiskLaser and Wave be sent through the ordinary Bullet adapter?**  
   Not from current evidence. They have distinct global arrays and scheduler rungs; their field provenance must remain separately verified.

10. **Can the scaled damage use saturating or wide multiplication?**  
    No. Native uses low signed 32-bit <code>IMUL</code> followed by arithmetic <code>SAR 8</code>.

## 14. Open-question log, final state

- **RESOLVED OQ-3B-01:** scheduler owner is singleton Logic vector <code>0x0087F778</code> driven by <code>0x0055AFB0</code>.
- **RESOLVED OQ-3B-02:** reveal is a membership-gated tail append; there is no next-frame queue.
- **RESOLVED OQ-3B-03:** main object count is reloaded after every AI call.
- **RESOLVED OQ-3B-04:** same-frame first AI and same-frame damage are possible but not universal.
- **RESOLVED OQ-3B-05:** a non-impact first AI leaves the bullet registered for later visits.
- **RESOLVED OQ-3B-06:** impact damage, warhead, source object, source house, and coordinate provenance are fixed above.
- **RESOLVED OQ-3B-07:** <code>target_id</code> is a collector-record target, not the projectile tracking target.
- **RESOLVED OQ-3B-08:** receiver <code>ignore_defenses</code> is literal false.
- **RESOLVED OQ-3B-09:** Fire_At itself is not the HP mutation owner.
- **RESOLVED OQ-3B-10:** Wave and DiskLaser do not share ordinary Bullet scheduling.
- **BLOCKED OQ-3B-11:** which exact Rust type owns the complete persistent projectile payload must be decided by the required separate brainstorm/design.
- **DEFERRED OQ-3B-12:** exact per-stock-projectile latency tables require their individual trajectory/target-state evidence and are outside this slice.
- **DEFERRED OQ-3B-13:** full laser/electric/rad/particle damage routes belong to producer reconciliation, not ordinary G2.
- **BLOCKED OQ-3B-14:** executable gamemd-derived oracle certification is unavailable in this task; static proof does not replace the later retail oracle gate.

## 15. Coverage ledger

| Required area | Status | Evidence | Remaining work |
|---|---|---|---|
| Fire_At ordinary creation | VERIFIED | fresh Fire_At disassembly, allocator and Init decompile | none for ordinary route |
| BulletClass identity | VERIFIED | fresh RTTI COL/type-descriptor and slot reads | none |
| reveal/insertion | VERIFIED | fresh Fire, Reveal, register, Insert bodies | none |
| scheduler singleton and exact loop | VERIFIED | fresh Main_Tick and PerTick assembly | none |
| same-frame appended-bullet trace | VERIFIED mechanism fixture | live count reload plus first-AI impact branch | no universal stock latency claim |
| delayed-impact trace | VERIFIED mechanism fixture | no-impact AI tail and retained membership | full trajectory families deferred |
| BulletDetonation coordinate handoff | VERIFIED at adapter boundary | fresh decompile/disassembly | exact every snap branch belongs projectile implementation contract |
| signed scalar damage | VERIFIED | <code>0x00469A56..0x00469A66</code> | none |
| source object/house/warhead | VERIFIED | Init and <code>0x00469A5C..0x00469A7E</code> | native detach behavior must be represented by lifecycle design |
| area/receiver call | VERIFIED via 3A plus cold check | <code>0x00469A83</code>, <code>0x00489A97..0x00489AB6</code> | Task 3A external prerequisites remain |
| every planned DTO field | VERIFIED with boundary clarification | sections 7-8 | DTO must be per fixed record or split pre/post collection |
| DiskLaser/Wave timing classification | VERIFIED | constructors, PerTick rung positions, Apply_area xrefs | full effect math deferred |
| every special Fire_At effect | PARTIAL | direct callee inventory proves no fire-call HP | producer-specific routes deferred |
| current Rust drift | VERIFIED from current focused reads | cited source lines | separate design/implementation |
| G2 gate | FAIL / BLOCKED | missing integrated lifecycle and authority | separate brainstorm, approval, plan, implementation, gamemd oracle |

## 16. Cold spot-check and zero-add pass

### Cold spot-check A: identity

Starting only from vtable <code>0x007E46E4</code>, the COL pointer, type descriptor, and <code>.?AVBulletClass@@</code> name independently reconfirmed the class before slot semantics were used.

### Cold spot-check B: receiver arguments

Starting only from Task 3A's receiver call address, fresh assembly at <code>0x00489A97..0x00489AB6</code> independently reconfirmed the record target receiver, fresh local incoming-damage pointer, distance, unchanged warhead/source/source-house, and the two literal zeros.

### Cold spot-check C: effect timing

Starting from the two non-Bullet Fire_At constructors, fresh constructor and PerTick reads independently reconfirmed that DiskLaser's rung precedes the main object pass while Wave splash follows it.

### Zero-add pass

After the coverage and contradiction ledgers were populated, the core functions were reread in this order: Fire_At callsites, Bullet vtable, Bullet Fire, Reveal/Insert, Main_Tick/PerTick loop, Bullet AI terminal branch, BulletDetonation, Warhead detonation, Apply_area receiver. No additional in-scope producer field, scheduler owner, receiver argument, or timing class was found. Remaining facts were either already represented or explicitly classified as deferred effect/trajectory work.

## 17. Implementation handoff acceptance tests

The eventual separate projectile plan should require at least:

1. a live-order fixture in which an attacker appends a Bullet and the Bullet gets first AI later in the same pass;
2. a companion fixture where first AI misses impact and the Bullet persists to a later pass;
3. a one-lepton inclusive close-impact boundary fixture;
4. signed scalar tests including negative damage, negative scalar, overflow low-word behavior, and arithmetic shift;
5. source object present/absent and house-at-impact changes;
6. tracked target differing from one or more collected receiver targets;
7. exact world-lepton coordinate preservation through collection;
8. CellSpread-zero and multi-record area dispatch with native ordering;
9. bullet self-removal causing the native compact-and-skip cursor consequence;
10. save/load and hash round-trip of every persistent projectile field;
11. explicit proof that Fire_At no longer mutates HP in the authoritative branch;
12. separate Wave and DiskLaser timing tests rather than routing them through the Bullet adapter.

Rust-only goldens can regression-test this mechanism but cannot certify gamemd parity. Final G2 promotion requires the later retail-derived oracle evidence named by the parent plan.

## 18. Sources

### Primary binary evidence, current session

- <code>TechnoClass::Fire_At @ 0x006FDD50</code>
- <code>BulletClassAllocate @ 0x0046B050</code>
- <code>BulletClass::Init @ 0x004664C0</code>
- <code>BulletClass::Fire @ 0x00468670</code>
- <code>BulletClass::AI @ 0x004666E0</code>
- <code>BulletClass::BulletDetonation @ 0x00468D80</code>
- <code>WarheadTypeClass::Detonate @ 0x004690B0</code>
- <code>ObjectClass::Reveal @ 0x005F4EC0</code>
- <code>ObjectClass::Conceal @ 0x005F4D30</code>
- <code>ObjectClass::UnInit @ 0x005F65F0</code>
- register/remove helpers <code>0x0055BAA0</code> and <code>0x0055BAE0</code>
- <code>DynamicVector::Insert @ 0x005519B0</code>
- <code>LogicClass::PerTickUpdate @ 0x0055AFB0</code>
- <code>Main_Tick @ 0x0055D360</code>
- <code>Apply_area_damage @ 0x00489280</code>
- <code>DiskLaserClass::AI @ 0x004A7340</code>
- <code>Wave_splash_forces @ 0x0053CBE0</code>

### Research used

- <code>[docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_AREA_DISPATCH_REINVESTIGATION_2026-07-13.md)</code>
- <code>[docs/research/AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md)</code>
- <code>[docs/research/AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_FIRST_TICK_DAMAGE_LATENCY_GHIDRA_REPORT.md)</code>, scheduler finding only; reconciliation warning honored
- <code>[docs/research/AAHEATSEEKER2_GUARDWH_DETONATION_PARAMETERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_GUARDWH_DETONATION_PARAMETERS_GHIDRA_REPORT.md)</code>
- <code>[docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md)</code>
- <code>[docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md)</code>
- <code>[docs/research/BULLETCLASS_VTABLE_F8_TEARDOWN_REMOVAL_PATH_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_VTABLE_F8_TEARDOWN_REMOVAL_PATH_RESWARM_20260528.md)</code>
- <code>[docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md)</code>
- <code>[docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/L2_FIRE_DAMAGE_TIMING_VERDICT_GHIDRA_REPORT.md)</code>

### Stock data and current Rust

- <code>ini/rulesmd.ini:22569..22578</code>: MissileLauncher damage/projectile/speed/warhead/minimum range
- <code>ini/rulesmd.ini:25678..25690</code>: AAHeatSeeker2 Arm/Proximity/Ranged/ROT
- <code>ini/rulesmd.ini:26902..26912</code>: GUARDWH
- <code>src/sim/world/logic_vector.rs</code>
- <code>src/sim/world/mod.rs</code>
- <code>src/sim/game_entity.rs</code>
- <code>src/sim/movement/homing_movement.rs</code>
- <code>src/sim/movement/rocket_movement.rs</code>
- <code>src/sim/combat/mod.rs</code>
- <code>src/sim/world/world_hash.rs</code>

