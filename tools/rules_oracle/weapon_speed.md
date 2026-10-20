# Retained weapon speed and Rules pass order

`weapon_speed.py` and `weapon_speed_order.py` execute original gamemd.exe.
Their JSON results are native outputs, not calculations copied from Rust.
The `.meta.json` files pin the payload and list supplied boundaries.

## Native behavior established

Weapon constructor771C70 initializes Speed+A8 and Range+B4 to zero.
ReadINI772080 calls ReadSpeed474810 at7722FD with the current speed default:
literal -1 retains it; other signed values clamp to0..100, scale by256/100,
then clamp to255. Physical HoverMissile Speed40 therefore stores102.
Thirty original reader controls cover signed overflow, hexadecimal, numeric
prefixes, exact key case, empty values and retained defaults.

ReadTypeData679A10 runs Weapon, Bullet and Warhead readers, then invokes
Weapon7729F0 at679B8C for every live weapon, including absent sections.
A nonnull projectile with ROT0 overwrites speed using retained Range and
Rules Gravity through48AB90; Floater halves Gravity. Null projectiles and
nonzero ROT, including negative ROT, retain speed. ReadString's empty
Projectile value retains the old pointer; `none` clears it.

**Gravity is read later.** Full Process668BF0 calls TypeData at668EF0,
then AudioVisual6691E0 at668F56. Its Gravity read66B3C4 uses the retained
signed dword default. Full constructor665650 sets Gravity3. A Range5/ROT0
weapon stores67 before the same pass changes Gravity to12; GetSpeed500
after that pass returns84. A preceding cold AudioVisual6 changes the stored
postpass result to95. The order corpus executes two histories of 14 complete
Process calls, with absent sections, ROT changes, null/empty references,
Floater and negative/zero Gravity.

Cold caller52D132 invokes AudioVisual before later Process calls.
Reset6686C0 destroys Type registries while retaining its RulesClass receiver;
the body does not rerun its constructor or reset Gravity before Process668A27.
These caller/lifetime statements rest on original instructions. Full reset
execution is **not** claimed: an exploratory fixture stopped at an unrelated
supplied Color entry's destructor. The complete Process calls above do execute.

The physical-input component in `weapon_speed.py` intentionally reads Gravity
before its isolated postpass. It establishes scalar inputs and the ROT60 arm,
not full Process chronology. Its 38 acceleration visits begin at supplied
cardinal velocity1 and do not establish a complete missile trajectory.

## Production owner and comparison

`RulesPassProcessor` retains speed, range and the allocated projectile handle
on its existing live Weapon state. It runs the postpass before AudioVisual,
and its move-only process state retains Gravity across handoff and Type reset.
The compatibility dependency graph follows the retained projectile pointer.
`RuleSet` publishes the stored result and hashes effective speed, binding and
Gravity, since identical current source stacks can inherit different values.

`util/native_ballistics.rs` shares the existing deterministic FireAt kernel
between the Rules postpass and shot launch. There is no rules-to-simulation
dependency or second arithmetic implementation. Original qword stores,
Sqrt_Approx and conversion boundaries remain intact; native flight/contact
comparisons require those rounding points. Existing 359 GetSpeed rows cover
negative/zero gravity and signed distance boundaries.

On 2026-09-26 the new four production-owner tests passed, including the physical
retail cold-start/scenario path. All58 native-processing tests, 10 weapon-reader
tests, two ordinary launch tests and all five executable-table launch tests
also passed. This validates the speed prerequisite, not the unfinished IFV
flight, impact, rendering or whole-bridge chain.

## Reproduce

Set `VERA20K_GAMEMD_EXE` or `RA2_DIR` and
`VERA20K_PROJECTILE_RENDER_ASSETS` as described in
[the physical-input guide](../projectile_oracle/bridge_render_inputs.md).
Use Unicorn 2.1.4 and run from the repository root:

```sh
PYTHONPATH=. python tools/rules_oracle/weapon_speed.py --check
PYTHONPATH=. python tools/rules_oracle/weapon_speed_order.py --check
VERA20K_REQUIRE_RETAIL_INI=1 cargo test -p vera20k-core --lib rules::native_processing::weapon_speed_tests::
VERA20K_REQUIRE_RETAIL_INI=1 cargo test -p vera20k-core --lib sim::projectile::launch::tests::
```

Cached INI indexes, allocator/CRT/TLS/archive boundaries, one preconstructed
Weapon and unrelated Color fallback are supplied. These witnesses do not
execute a full physical file loader or whole game. Original RNG, bridge
destruction, object lifetime and rendering are outside this speed prerequisite.
Ghidra postpass7729F0 was renamed to `WeaponTypeClass__RecomputeBallisticSpeed`;
Process/reset/AudioVisual comments record chronology and evidence limits.
The program was saved and all four comments read back.
