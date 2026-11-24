# Bridge debris landing inputs

`bridge_landing_inputs.py` establishes the non-animation inputs used by
`bridge_debris_flight.py` and `terrain_debris_receiver.py` through original
instructions. VERA's interpreted/exported values do not initialize these readers.
The generator supplies lexical entries from the physical retail files, executes
original readers, and records exact scalar bits and resolved native names.

Original executable instructions are unchanged. Cached INI indexes use original
CRC results; allocator storage and CRT TLS storage are supplied. Native physical
INI loading, archive lookup, whole Rules processing and full Terrain readers are
outside this bounded comparison.

## Native input owners

| Input | Executed original source and selected retail output |
| --- | --- |
| HE/Super constructor fields | Full Warhead factory `75E3B0` and constructor `75CEC0`: eleven Verses 1, CellSpread 0, PercentAtMax 1, Wall/Wood false. |
| HE Verses | Complete `75DDCC..75DE5A`: ReadString128, strtok, strchr, signed32 atoi or binary64 atof, PC53/chop multiply and store. Retail `100,100,100,70,70,35,75,40,20,80,100` percent values are stored as exact bits in the corpus. |
| HE CellSpread/PercentAtMax | `75D3D2..75D3F1` and `75D410..75D42F`, original ReadDouble and binary32 stores: both 0.5. |
| HE Wall/Wood | `75D4F4..75D50E` and `75D542..75D55C`, original ReadBool: both true. |
| Super | Same native readers: all Verses 1, spread 0, PercentAtMax 1, Wall/Wood false. |
| MaxDamage/BridgeStrength | Original Rules constructor stores both 1000; `66CE2C..66CE57` and `66CD66..66CD8C` read signed integers, storing retail 10000/1500. Existing wider strength-domain evidence is `spatial_oracle/bridge_damage_admission`. |
| Wake | Original null constructor store, then `66D847..66D894` ReadString128 and actual AnimType factory: WAKE1. |
| SplashList | Original empty vector constructor and complete `66C184..66C287` ReadString128/strtok/factory/vector-copy block: H2O_EXP3, H2O_EXP2, H2O_EXP1 in that order. |
| C4Warhead | Original null constructor store; `66C304..66C34C` ReadString128 and actual Warhead factory: Super. |
| ConditionRed/Yellow | Original constructor stores 0.5 for both. AudioVisual `66B337..66B35E` stores Red at Rules+1708; `66B35E..66B385` stores Yellow at +1700. Retail overrides these to 0.25/0.5. |
| TreeStrength | Original constructor `666DF8` stores 25; General reader `671DD2..671DF1` stores retail 200. |
| TREE01/TIBTRE01 type fields | Full TerrainType constructor `71DA80`; bounded original Armor/Strength/Immune block `5F94B3..5F9516`, Strength sentinel fallback `71DEC8`, SpawnsTiberium block `71DF27..71DF56`, and both occupation read/stores `71E079..71E0A6`. Native original section admission runs; successful completion of unrelated base-reader fields is a supplied boundary. |
| Terrain foundation | Original static occupy initializer `71D580`, followed by `71DF61..71DF9C` against physical ART strings through original Foundation lookup `474DA0`. TREE01/TIBTRE01 both resolve enum 0, name 1x1, offsets (0,0) then (32767,32767) sentinel. No SHP/image lookup executes here. |

TREE01 resolves Strength 200, armor 6 (wood), Immune false, SpawnsTiberium false,
TemperateOccupationBits 4 and SnowOccupationBits 6. TIBTRE01 resolves 200/wood,
Immune true, SpawnsTiberium true, occupation 7/7. The contact corpus uses TREE01
as its base and explicitly overrides gates for synthetic cases. Successful
SpawnsTiberium death-animation construction remains outside that corpus.

The native constructor/reference factories run against spare-capacity registries.
Original `7C8F5E` installs the CRT floating scanner before ReadDouble. FPCW is
`0x0E7F` (53-bit precision, truncate toward zero). Rule scalar/list constructor
slices receive original EBX=0/EDI=10; the original ECX immediate at `667190`
establishes both threshold defaults. Wake/C4 slice endpoints have ESP−4 because
the following key's push occurs before the selected pointer store.

## Physical source and layer bounds

The selected input set is:

- `RULESMD.INI`: 743215 bytes, SHA-256
  `3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`,
  extracted from `expandmd01.mix`, entry `8218F9F4`.
- `LANGRULE.INI`: absent in the selected installation.
- `MPBattleMD.ini`: 295 bytes; exact hash retained in the corpus.
- `Hills.map`: the exact 144458-byte inner INI payload of loose `Hills.mmx`,
  SHA-256 `780d5d6e6d3c81ac0d510a5df326848dd426b3d840ac290114f44faf8eab9e9e`.
  This is distinct from the menu's archived XHills.MAP variant.
- Fixed `ARTMD.INI`: 336535 bytes, SHA-256
  `e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31`.
  Only the two Terrain Foundation entries are consumed here; animation input
  proof lives in [bridge_anim_inputs.md](bridge_anim_inputs.md).

The selected sections/keys are unique ordinary ASCII entries. The generator
strips comments and surrounding ASCII control/space bytes, omits empty values,
and builds cached indexes without interpreting scalar values. Every selected
source line and file hash is saved. MPBattleMD and this Hills map have no selected
sections, so their original section-admission calls return false and retain the
RULESMD values. Five separate controls exercise missing sections, missing keys,
retained lists/references/scalars, and an admitted HE section whose missing
Verses uses the original all-100% default string. These controls do not certify
all native per-pass type ownership.

## Required precision correction and connected damage

The previous Rust percentage reader used host nearest-rounded `atoi * 0.01`.
Original `75DE2D..75DE48` uses the stored binary64 0.01 literal at `7E3808` and
PC53/chop multiplication. For retail HE, the difference affects armor indices
3, 4, 5, 7, 8 and 9. Wood index 6 is exactly 0.75 in both.

The corpus executes original damage body `489180` for damage 10/20, distances
0/11/49/50/79/80/127/128/129 and all eleven armor indices: 198 rows. It also
executes each row with an explicitly supplied one-ULP-higher perturbation at
those six indices. This changes 27 integer results. At damage 20, distance 0:

| Armor | Native reader → native damage | Higher input → native damage |
| --- | ---: | ---: |
| light / medium | 13 | 14 |
| heavy | 6 | 7 |
| wood | 15 | 15 |
| steel | 7 | 8 |
| concrete | 3 | 4 |
| special_1 | 15 | 16 |

The perturbation is explicitly characterized input, not a desired Rust golden.
The production fix stays in `WarheadType`'s existing f64 reader and reuses the
integer-based `X87Chop53` arithmetic owner for this one multiplication. It uses
the native ReadString128 boundary and skips empty comma tokens like strtok.
The former duplicate bare-number scanner is removed; f32/f64 readers share the
existing decimal/exponent token scanner in `ini_value`.

Seventeen fresh-Warhead reader cases cover the retail list, selected integer
percentages, negative/fractional-percent tokens, signed32 wrap boundaries,
malformed percent tokens, bare decimals/exponents and trailing junk. Thirteen
complete natively. Four short/comma-only/truncated lists produce an original
null-token read fault: native loops eleven times without checking strtok's
result before strchr. Rust keeps its safe trailing defaults; no crash-domain
parity is claimed. Arbitrary extreme atof rounding, overflow/underflow and
non-ASCII lexical input are not exhaustively characterized by these samples.

## Comparison fixture correction

Both downstream harnesses now consume this independent native corpus. The old
Python HE `.4/.2/.8` literals were one ULP above the original stored values.
The contact fixture also mislabeled Rules+1708 as Yellow and supplied 0.5; it
now supplies the actual Red 0.25 and Yellow 0.5 fields, and TREE01's real 4/6
occupation values. C4 Super's PAM 1 is now explicit.

Regeneration after these corrections left all 32 flight payloads and all 24
contact payloads unchanged, including callbacks, damage/results, cleanup and
RNG continuation. The metadata now pins this input corpus's hash. The contact
rows supply health, placement, live flags, linked-list ordering, map cells,
neighbor counters and empty observer vectors as declared fixture state; these
are not claimed as native scenario construction. Retained Terrain coordinates
have separate [terrain_coordinate](../spatial_oracle/terrain_coordinate.py)
evidence. Child animation/SHP inputs have separate ART evidence above.

## Reproduction and Rust consumers

Extract the four physical files with the purpose-built asset tools; preserve
`Hills.mmx`'s decompressed inner map as `Hills.map`. The local run used
`/tmp/bridge-landing-inputs`. The default directory honors `CARGO_TARGET_DIR`
then `asset/bridge-landing-inputs/extract`. Do not replace native asset files.

```sh
PYTHONPATH=. VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_BRIDGE_LANDING_ASSETS=/path/to/extracted/files \
python tools/rules_oracle/bridge_landing_inputs.py --check
```

The focused Rust regressions are:

- `rules::warhead_type::tests::native_bridge_landing_verses_reader_bits`: physical
  `IniFile` and production `WarheadType` reader versus all thirteen completed
  original reader cases, with the four original fault cases explicitly excluded.
- `sim::world::bridge_orchestrator::debris_tests::flight_tests::bridge_landing_constructor_and_absent_keys_use_native_defaults`:
  native constructor TreeStrength/ConditionRed against the GeneralRules default,
  actual absent-key RuleSet loads and the Terrain convenience constructor.
- `sim::world::bridge_orchestrator::debris_tests::flight_tests::retail_bridge_landing_inputs_match_original_readers`:
  production retail RULESMD/ARTMD fields, every HE/Super bit and all 198 original
  damage outputs.
- Ignored `retail_hills_layered_bridge_landing_inputs_match_original_readers`:
  actual AssetManager, production MIX-aware MapFile loader and process-resident
  NativeRulesProcessOwner through a noncampaign scenario load, then the same
  assertions. This additionally requires the configured retail installation.
- The existing 32-row flight and 24-row contact comparisons retain their own
  declared scheduler/receiver boundaries; reader checks do not expand those.

Validation on the implementation candidate used the retail fixtures with
`VERA20K_REQUIRE_RETAIL_INI=1`. The canonical focused Cargo command
`cargo test -p vera20k --lib rules::warhead_type::tests::` passed all 15 tests.
Its compiled library-test executable was copied immediately to
`/tmp/bridge-landing-owned-libtests` (SHA-256
`f82850a9a6458fb997a17c2703a36c5bd9e63ad1ac46108a9a10b69fea251695`), and its
test list was checked for the new reader/default/ART tests. Direct runs of that
same executable then passed `rules::ini_value::` (14, one ignored),
`rules::terrain_object_type::` (7), and
`sim::world::bridge_orchestrator::debris_tests::` (5, one ignored). The ignored
`retail_hills_layered_bridge_landing_inputs_match_original_readers` passed when
explicitly enabled. These direct runs are not additional Cargo builds. The
landing-input, flight and contact native harnesses also passed `--check`.
This receipt precedes the final integrated full-library and Clippy checks.

## Required follow-ups outside the selected retail path

- The native-established TreeStrength25 and ConditionRed0.5 initializer fixes
  now live in the existing GeneralRules owner; Terrain convenience construction
  derives that default instead of maintaining another200 literal. Retail
  RULESMD still supplies200/0.25. These are required initializer corrections,
  not deferred differences. The absent C4Warhead constructor is null, while the
  current Rust name default is Super; this affects custom null-reference
  admission, not the selected authored reference. Per-pass retained reader
  ownership remains a broader rules chain; existing [terrain_strength](../spatial_oracle/terrain_strength.md)
  documents the Terrain sentinel-retention boundary.
- The legacy byte `WarheadType.verses` table still serves garrison admission,
  base-defense response and `SelectedWeapon.verses_pct` attack-mode thresholds.
  Ordinary GetFireError exact-zero admission and the selected contact/landing
  damage read `verses_f64`; ForceAttackCell uses target metadata 100. Fractional,
  negative, overflow or unusual token lists can still make those legacy targeting
  decisions differ. Migrating their surrounding native admission chains is a
  required whole-goal follow-up; this correction does not claim it.
- Full Terrain/Rules physical readers, successful spawner death animations,
  populated pointer-expiry observers, whole animation scheduling and rendered
  output require their own connected evidence. This input closure does not
  complete the whole-bridge goal.
