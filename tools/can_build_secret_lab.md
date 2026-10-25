# SecretLab grant controls

Run from the checkout with `VERA20K_GAMEMD_EXE` selecting the original retail
`gamemd.exe`:

```sh
python -m tools.can_build_oracle --secret-lab --check
```

`--write` deliberately replaces the corpus and sidecar. The default
`python -m tools.can_build_oracle --check` still checks its separate existing
CanBuild/CheckBuildLimit corpus. Rust consumes this profile in the SecretLab,
rules-processing and production-admission tests.

## Executed components

The `assignment` rows execute the whole original `ScenarioClass` assignment
entry `0x0068C050` and `BuildingClass::GetSecretProduction` at `0x00459840`.
They supply the three registered typed lists, their total and the global lab
vector (`0x008B41E0`), including declared limbo membership. They do not substitute
assignment, selection, getter or RNG answers. Original seed `0x0065C6D0`, ranged
RNG `0x0065C7E0`, raw RNG instructions, temporary vector operations and cleanup
execute through the existing `Emu` owner. Each row records the complete before
and after `0x3F4` bytes of Scenario, main and map-generator RNG, the ranged
arguments/results/callers and the raw advance instruction observations.

The additive `authored_startup` rows use the same assignment owner after
executing the authored Full_Init admission gate at `0x00687BC8`. ESI is zero
(from `0x0068773D`), GameMode is 0 or 3, and the saved incoming input byte at
`[ESP+0x13]` is zero. Execution stops at the actual reached address: blocked
`0x00687BF1`, or admitted `0x00687BD8` before the Official reader. All three
complete RNG buffers must remain unchanged across the gate. An admitted row
then invokes original `0x0068C050` through the same fixture owner; a blocked
row leaves its supplied null fallback alone. The outer before/after buffers
cover both the gate and the eligible assignment. No successful Post_Map_Init
return, preceding startup, or intervening Post_Map work is supplied or claimed.

These four controls use the stock SOV05/SOV07 shape: one lab, lists of four
infantry, three units and one building, with an explicit DESO or GTGCAN override.
Mode 0 skips assignment and consumes no RNG; mode 3 admits one `[0,7]` request.
The explicit reward wins in both modes, so the campaign mismatch concerns the
extra RNG request and fallback write, not the visible reward. Generated `.SED`
startup has a separate Post_Map_Init caller (`0x00684990`) and is outside this
authored gate control. These shapes do not claim a whole campaign load or its
arriving RNG state; production readers and campaign capture validate that path.

The native algorithm first requires a positive choice count, at least one lab,
and no more labs than choices. Failure leaves the prior instance fallback
unchanged. Otherwise it draws a position in the shrinking temporary list,
removes that position, but indexes the original concatenated type lists with
the drawn position itself. Consequently different labs can receive the same
type. The original instruction sequence, rather than a corrected sampling
algorithm, establishes the outputs. Equal-bound `RandomRanged(0,0)` records one
request and advances no raw word; rejection can make other requests advance
more than one word. Override fields do not suppress initial assignment draws.

The `getter` rows execute the complete straight-line `0x00459840..0x0045986A`:
the type's `+0xEA4` SecretInfantry pointer wins, followed by `+0xEA8` SecretUnit,
then `+0xEAC` SecretBuilding, then the instance's `+0x6F4` fallback. These controls
establish pointer priority, including null and masked fallback outcomes. They
do not establish type readers or lab membership.

The `admission` rows reuse `BuildEmu` and the existing CanBuild fixture schema.
`secret_labs` declares the prior House `+0x110` list and each lab's three override
identities and optional fallback. Whole `HouseClass::CanBuild` at `0x004F7870`
calls the actual getter. Its early `0x004F78E4..0x004F7925` grant arm bypasses
ordinary technology, country, stolen-tech and prerequisites for both humans and
AI, while still executing `CheckBuildLimit` at `0x0050B370`. In particular a
computer house's ordinary arm can admit a type at its build limit while the
SecretLab arm refuses it. The existing type RTTI/index/object-type virtual
fixture stubs remain explicit substitutions; no admission/getter answer is
supplied. This profile does not execute capture, registration/removal, factory
selection, payment, delivery or sidebar refresh.

## Retained native key readers

The `readers` section reuses `bridge_anim_inputs.Reader` for the image, INI cache,
heap and CRT TLS. It runs these original slices and their complete callees:

| Component | Native source | Field and behavior |
| --- | --- | --- |
| Three General vector constructors | `0x00666773..0x006667F8` | Empty vectors at Rules `+0xD00`, `+0xD1C`, `+0xD38`; total `+0xD54=0` |
| General Secret lists | `0x0066FA13..0x0066FAD6` | Exact `SecretInfantry`, `SecretUnits`, `SecretBuildings`, in that order, then total count |
| Infantry list | `0x0067BB10` | Original `ReadString(128)`, comma `strtok`, `FindOrAllocate(0x00524CB0)`, vector copy |
| Unit list | `0x0067B720` | Same reader capacity, original `FindOrAllocate(0x007480D0)` |
| Building list | `0x0067B550` | Same reader capacity, original `FindOrAllocate(0x00466000)` |
| Override constructor stores | `0x0045DE94..0x0045DEA6` | Null `+0xEA4/+0xEA8/+0xEAC` |
| Override reads | `0x004605BB..0x00460683` | Exact `SecretInfantry`, singular `SecretUnit`, singular `SecretBuilding`; `ReadString(128)`; retained pointer on empty read, otherwise original typed lookup |
| Flag constructor store | `0x0045E0EA..0x0045E0F0` | False `+0x16B0` |
| Flag read | `0x004609B6..0x004609D6` | Exact `SecretLab`, `ReadBool(0x005295F0)`, current flag default |

The supplied type registries contain every non-null reference used by these
controls. Original case-insensitive lookup and native list operations execute;
unknown-type construction is outside their coverage. Missing, empty and
whitespace-only strings retain existing values. Native list parsing keeps
duplicate identities, drops `none`/`<none>` and replaces a list with empty for
comma-only input. Scalar `none`/`<none>` clears an override; empty scalar input
retains it. Lowercase key names do not replace the exact native keys. Invalid
boolean text retains the current flag. No separate Python type parser supplies
expected values.

Physical layers reuse `weapon_laser.physical_layers` and the existing bounded
`bridge_render_inputs.lexical` owner. The corpus pins the full file hashes,
lengths and selected physical key strings. The sequence is `RULESMD.INI` from
`expandmd01.mix`, optional loose `LANGRULE.INI`, Battle mode from
`ra2md.mix/localmd.mix`, then `XMP03T4.MAP` from `multimd.mix`. Stock General lists
yield four infantry, three units and one building. Stock CASLAB sets
`SecretLab=yes` and leaves all three overrides null; the other present selected
layers retain those values. The cache fixture does not execute physical INI
loading, full Rules Process, the entire BuildingType reader, map admission or
mounted archive precedence. Rules from other modes/maps require their own
production-reader integration coverage.

The metadata identifies the original binary, replay command, entry points,
substitutions and canonical payload. It pins no repository source hashes.
