# Retained impact animation inputs and Type reset

Native evidence uses original `gamemd.exe`.
The adjacent Python harnesses execute the original instructions; the JSON files
are native outputs, and the metadata records fixture substitutions.

| Owner/field | Actual key and section | Constructor | Reader |
| --- | --- | --- | --- |
| Warhead +14D | `[WH] Conventional` | false, 75CF89 | retained ReadBool5295F0 at75D4E9 |
| Warhead +154 | `[WH] EMEffect` | false, 75CFB3 | retained ReadBool5295F0 at75D7C1 |
| Warhead vector +104 | `[WH] AnimList` | empty | full Warhead reader75D3A0 |
| Rules +17B4 | `[General] LightningWarhead` | null, 6676C2 | 671053..671072 /67B500 |
| Rules +2F4 | `[General] WeatherConBoltExplosion` | null, 665A8B | 66DF19..66DF60 |
| Rules +350 | `[General] WeaponNullifyAnim` | null, 665B27 | 66E2AF..66E2E5 |
| Rules vector +BC0 | `[CombatDamage] SplashList` | empty | 66C184..66C287 |

These names come from the original literal bytes and executed readers. In
particular, +154 is **EMEffect**, and the Lightning selector uses
**LightningWarhead/WeatherConBoltExplosion**. Names such as RandomAnims and
IonCannonWarhead do not describe these native reads. WeaponNullifyAnim is the
alternate animation consumed by Bullet46A2A1 after AreaDamage returns2. The
ordinary SelectAnim call still occurs before that alternate branch; effect
ordering is owned by the separate impact/selector harnesses.

`select_anim_inputs.py` executes full Rules665650 and Warhead75CEC0 constructors,
then12 sequential passes on the same objects. It executes full Warhead75D3A0,
plus the four isolated Rules read blocks in their relative Process order.
It supplies the live pushed buffer argument at66E2AF; it does not patch reader
instructions or return values. No physical file loader or full Process runs in
this first harness. All names/defaults come from executed readers and factories.

The controls cover missing sections, later missing keys, lexical empty omission,
invalid/numeric bools, exact `none` clearing, case-insensitive Type lookup,
case-sensitive INI keys, repeated/untrimmed list tokens and128-byte read limits.
Scalar references retain on an empty ReadString result. Lists retain on a missing
or empty value but can be explicitly emptied by comma-only/none token input.
The same native Type registry supplies canonical stored names across passes.
`EMEffect` and `Conventional` retain the current byte as their reader default.
No RNG, runtime Anim construction or selection result is claimed by this harness.

## Reset boundary and reread

`select_anim_reset.py` separately executes original Reset6686C0 and every
populated Animation/Weapon/Warhead scalar/base destructor. Read-only observations
record the final operator-delete calls and the original loop phases. Original
ObjectType/AbstractType vector initialization and an empty SEH chain are supplied
before the reset. The inherited unrelated synthetic Color entry is absent.

The first case stops at6689E7, after Type deletion and before Process. Both Type
registries are empty; LightningWarhead is null. WeatherConBoltExplosion,
WeaponNullifyAnim and SplashList still contain the exact addresses of deleted
AnimTypes. The output explicitly records them as raw addresses, not valid native
object identities. No deleted object is dereferenced to produce a name after
reset. The heap boundary observes frees without reusing storage, so behavior
through reused/dangling pointers is outside this comparison.

Rust's `NativeRulesRegistryState::destructive_reset` clears those references and
lists. This is an explicit deterministic safety policy for native stale-pointer
state, **not** a claim that gamemd clears every Anim reference. It never rebinds
retired Types merely because a later Type has the same name. Authored subsequent
reads recreate valid references through the existing registry owner.

The second case executes full Process668BF0 before reset and again from reset's
actual668A27 call, stopping at668A2C before its later file/INI reload tail. Inputs
are physical RULESMD lexical General keys for these three references,
CombatDamage SplashList, and the complete referenced IonWH section. Both passes
produce LightningWarhead=IonWH, WeatherConBoltExplosion=EXPLOLB,
WeaponNullifyAnim=IRONFX, SplashList=H2O_EXP3/H2O_EXP2/H2O_EXP1,
IonWH Conventional=true, EMEffect=false and AnimList=EXPLOSML. The fixture records
the physical RULESMD SHA. This establishes that reread through full Process
rebuilds the selected physical bindings after deletion; it does not execute the
complete retail Type universe, all mode/map layers or the remaining Reset tail.

## Production bindings and validation

`src/rules/native_processing.rs` owns retained inputs in the existing registry
receipt and uses the existing discovery order/factories. `RuleSet` projects those
resolved fields and includes them in the simulation configuration hash. The
existing stable names identify live registry bindings; they do not substitute
for runtime native numeric constructor IDs.

`rules::native_processing::select_anim_rules_tests` contains:

- `select_anim_inputs_match_native_retained_passes`: all12 native passes through
  production lexical/rules readers, retained receipts and RuleSet projection;
  also checks that equal empty current INIs with different retained inputs have
  different simulation configuration hashes.
- `type_reset_clears_retired_references_then_rebuilds_native_physical_bindings`:
  asserts the explicit reset policy separately, then compares the production
  selected physical reread with the original full-Process output.

Independent `--check` passes for both native corpora. Both production Rust
tests pass on the joined candidate, including all 12 retained passes with
WeaponNullifyAnim and the reset/physical-reread comparison. The retained test
binary SHA256 is
`40f4b5d6d42b5f56da44f8b495d5f083c86dc4ab04594760abebb420810a7970`;
the local receipt is `/tmp/bridge-ifv-joined-rules.log`. These two passing tests
do not extend the native coverage beyond the reader/reset boundaries above.

From the repository, with `VERA20K_GAMEMD_EXE` set to the matching executable and
`VERA20K_PROJECTILE_RENDER_ASSETS` pointing to the extracted physical inputs:

```sh
PYTHONPATH=. python -m tools.rules_oracle.select_anim_inputs --check
PYTHONPATH=. python -m tools.rules_oracle.select_anim_reset --check
```

Use the repository native-oracle environment (Unicorn2.1.4); allocator, TLS,
lexical cache and archive boundaries are declared in each metadata file.
