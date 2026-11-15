# Auto-target scan fixes

Two `[General]` switches that reshape `TechnoClass::SelectAutoTarget`'s scan:
`AIAirTargetingFix` lets the flat walk reach airborne targets, and
`DisableOveroptimizationInTargeting` removes the expanding-ring walk's band
returns.

```ini
[General]
AIAirTargetingFix=false                  ; Valueable<bool>
DisableOveroptimizationInTargeting=false ; Valueable<bool>
```

Readers: `Ext/Rules/Body.cpp:452` and `:531`; constructor defaults `false`
(`Ext/Rules/Body.h:362,867` and `:440,951`). Both are `[General]` keys, not
`[AI]` ones.

## AIAirTargetingFix

Native's flat (Hunt mask `0`, a quarry mask) walk gates airborne objects twice:

```asm
0x006F9B7E  MOV AL,[ESP+0x14]     ; the derived flags word
0x006F9B82  TEST AL,0x4           ; Aircraft bit
0x006F9B84  JZ 0x006F9C56         ; no bit: skip the aircraft pass
...
0x006F9D13  CMP dword ptr [EDI+0x94],0x2   ; LastLayer == Ground
0x006F9D1A  JNE 0x006F9D93                 ; else reject the object
```

The Aircraft bit reaches that word only at `0x006F9C5D` (`MOV BL,[ESP+0x78] ;
TEST BL,0x10 ; OR AL,0x4`), from the scanner's own weapon class bits
(`FUN_00772A90`: AA → `4`, AG → `0xB8`). An anti-ground weapon (`0xB8`) sets
`0x10` without `4`, so the aircraft pass is skipped at `0x006F9B7E` and the
object gate then refuses every non-Ground `LastLayer`: an AI unit with an
anti-ground weapon holds no aircraft at all.

Yes remaps `0x006F9B7E` to `0x006F9C56` (`Misc/Hooks.BugFixes.cpp:3074-3077`),
dropping the separate pass, and replaces the object test with

```cpp
canTargetRtti & 4 ? pTarget->LastLayer != Layer::Underground
                  : pTarget->LastLayer == Layer::Ground
```

(`Misc/Hooks.BugFixes.cpp:3079-3097`, hook `0x006F9D13`, OK `0x006F9D1C`,
reject `0x006F9D93`). One Techno walk then carries the airborne target whenever
the flags word has the Aircraft bit; a weapon with neither AA nor AG leaves the
word clear and keeps stock's Ground-only admission.

## DisableOveroptimizationInTargeting

The expanding-ring cell walk returns once a ring that held an object is the
`radius/4` or the `radius/2` band (`0x006F9AE2..0x006F9B0A`: `MOV EAX,[ESP+0x38]
; CDQ ; AND EDX,0x3 ; ADD ; SAR 2`, then the halved form, in turn compared with
the ring index). Yes jumps past the whole test to `0x006F9B1B`
(`Ext/Techno/Hooks.Targeting.cpp:88-92`, hook `0x006F9AF4`), so the walk reaches
its radius even when a band held a worse candidate. The wall-cell return
(`0x006F9B1B`) and the AA-only early return (`XOR EAX,5 ; JZ 0x006F9DA1`) are
untouched.

## Native anchors

- `TechnoClass::SelectAutoTarget @ 0x006F8FE0` — the radius block
  (`0x006F9AE2..0x006F9B0A`), the aircraft pass (`0x006F9B6E..0x006F9C52`) and
  the flat Techno walk with its object gate (`0x006F9C56..0x006F9D93`).
- Phobos hooks: `0x006F9AF4` (`DisableOveroptimizationInTargeting`), `0x006F9B7E`
  and `0x006F9D13` (`AIAirTargetingFix`).

## VERA

`rules::phobos_globals::PhobosGlobals::ai_air_targeting_fix` and
`disable_overoptimization_in_targeting` read the keys from `[General]` with
`read_bool`. Both consumers sit in the scan's one owner,
`sim::combat::greatest_threat`:

- `targeting_fixes` supplies the pair (both `false` without the `phobos`
  feature).
- `global_list_scan` enters the aircraft pass only while the fix is off, and
  the pass runs before the walk's `mask & 0x10` widening, as native's order.
- `flat_score` applies the layer gate: with the fix and the Aircraft bit,
  any registered non-Underground layer; otherwise Ground only.
- the ring walk's band return is skipped with the fix on.

`DisplayLayer::UNDERGROUND` names native's layer `0`.

## Residuals

- Both hooks rewrite bytes inside `TechnoClass::SelectAutoTarget`, which VERA
  owns for every caller, so `Greatest_Threat` calls from mission handlers, the
  retaliate scan and quarry scans all see the switches; native does too.
- `flat_score`'s layer gate reads VERA's Display registration
  (`display_layers::layer_of`), which answers `None` for an object the Display
  never submitted, while native's `LastLayer` always holds `0..4` after
  Unlimbo. Trigger: an object constructed without a Display submission (test
  fixtures, debug spawns). Effect: stock refuses it, the fixed arm refuses it
  too (it needs `Some`). Frequency: fixtures only. Risk: a scan for such an
  object differs from native; no retail path builds one.
- The aircraft pass walks `EntityCategory::Aircraft` members; native walks the
  Display's aircraft list (`0x00A8E394`, count `0x00A8E3A0`). Trigger: an
  aircraft in limbo, which VERA still counts as a member. Frequency: rare.
  Risk: a fix-off scan may score an airborne aircraft native would not see.
  With the fix on the pass does not run, so the difference is fix-off only.
- The clear-bit arm of the replacement gate (`canTargetRtti & 4` false, which
  keeps stock's Ground-only test) is modelled but not exercised: VERA's
  `combat_weapon::weapon_class_bits` defaults to anti-ground `(false, true)`
  when a weapon's `Projectile` does not resolve, so every armed fixture
  scanner reaches the widened word. Trigger: a weapon whose projectile is
  neither AA nor AG. Frequency: retail authors no such weapon. Risk: none
  observable; the arm can only refuse more.
- Retail rules author neither key, so the defaults reproduce the stock scan.
