# Ares building SuperAnim selection

This mechanism adds the installed Ares building-animation path behind `ares`.
The original retail path still compares the Super Type enum with the building's
primary definition index. Ares replaces that loop; changing the retail comparison
would be an incorrect stock fix.

`BuildingTypeExt::GetSuperWeapon` (`1000D610`) tests the primary, secondary and
`SuperWeapons` entries in order. Its `100329E0` availability decision checks shot
count, mode-aware human/AI control, native AuxBuilding, actual Country masks and
Ares auxiliary/negative building lists. It returns the first eligible retained
House Super, without a granted/readiness test. The installed opening hook
`4463F0 ->10034A60` then reaches the original timer/variant block at `44643E`.
Without an eligible Super, a present primary definition still creates slot14.
The update hook `450F9E ->10034AF0` reaches `451030` only with an eligible Super;
otherwise it leaves the existing slot alone. Existing slot/Anim owners retain
allocation, clear, expiry15-to16/17-to14, pause and rendering responsibilities.

The ordered rules processor retains the constructor defaults, exact-case keys
and original readers over its normal rules/language/mode/scenario passes:

| Fields | Default | Reader / installed source |
| --- | --- | --- |
| SW.AllowPlayer, SW.AllowAI | true, true | Ares bool2048, 1003349E..100334C5 |
| SW.RequiredHouses, SW.ForbiddenHouses | FFFFFFFF, 0 | ReadOwners128 /4750D0, 10033A0F..10033A38 |
| SW.AuxBuildings, SW.NegBuildings | empty | existing lookup-only building vectors2048 |
| SW.Shots | -1 | Ares int2048 /1000A570, 10033A7B..10033A8C |
| building SuperWeapons | empty | IndexLookup1000D4D0, vector1000EC70, 2048 |

Missing/empty values retain prior values. Unknown Country tokens contribute no
bit; `<random>` contributes bit30. The additional Super list uses a raw stored-ID
index lookup, so an actually registered sentinel-named entry can be found.

`superweapon::super_weapon_available` is the shared decision owner for providers,
hold/grant, detection, AI and commands. `first_available_building_super` selects
from those same providers. There is no second Super collection. The House's one
retained `SuperWeaponInstance` per definition privately holds the Ares successful
launch count and last frame; deactivate/regrant do not reset them. Native
`100226D0` increments a signed32 count and writes the current binary frame only
after Activate succeeds (`10032ED0..10032F15`). Reaching the limit dirties the
existing House tech-recheck owner (`10032F54..10032F8F`).

The current successful-write integration covers supported GenericWarhead and
UnitDelivery paths. All twelve stock types have installed Ares redirected
handlers (their HandlesType virtuals are selected by1006CFB0), but reproducing
each redirected Activate success contract is a later chain: a nondefault
`SW.Shots` on those paths will not accumulate successful launches here and can
leave the power available after its native limit. This is a real mod gameplay
residual, not evidence that those tags are unused. Existing default `SW.Shots=-1`
selection and display do not depend on that remaining counter integration.
The provider owner also retains its existing upgrade-provided Super enumeration
residual; this chain covers the unupgraded building path, not every Ares grant route.
The immutable availability catalog includes every registered definition even
when its Type has no implemented effect body. Both typed consumers and building
name queries use this same projection of the retained registry. The closed
1000D610/100329E0/opening-timer call chain does not read SuperType+B4, so the
executed index/timer observations also cover an unsupported dispatch value by
input-independence proof. A regression uses an unimplemented Type with the same
observed14/16 timer outcomes. Displaying that body does not admit its effect.

## Evidence and limits

`ares_super_anims.json` contains four newly executed synthetic histories. The
first three cover index-versus-Type selection,14/16 opening,14-to15/16-to17
updates, secondary/extra order and unavailable fallback. The fourth executes
native bool/int/Country-mask readers, the real successful-launch counter writer
and subsequent availability/fallback. Three native RNG streams stay unchanged;
no code bytes are replaced. INI ReadString values and retained world registries
are explicit fixture inputs; PlayAnim is the existing oracle recording boundary,
not full Anim construction or GPU evidence. Whole effect-handler activation is
excluded; its successful return to count-write order is instruction evidence.
The library regressions connect these observations to actual Rust Anim members
and to a real GenericWarhead command, damage, limit admission and save/restore.
Their results must be recorded separately after running them.

Reproduce only this corpus:

```text
python -m tools.rules_oracle.ares_super_anims --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check
```

Ares-only persisted fields change that build's saved layout; the existing save
layout guard rejects incompatible saves. The OFF struct layout/hash path is
unchanged. This is not a rendered parity claim or exhaustive Ares SW support.

The SuperWeapons restore owner also rejects a nonzero retained launch count
without a last-launch frame. Native 100226D0 writes both together; admitting
that damaged pair would let `SW.Shots` affect admission while the history hash
omits it. This is a Rust snapshot invariant, not an additional native load port.
Signed wrapping counts remain valid when their frame is present. The focused
`ares_super_launch_history_rejects_incomplete_restored_pair` regression crosses
the save codec and production restore admission; it adds no native controls.
