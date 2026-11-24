# Freed-slave owner and sound

Phobos lets each enslaved infantry type choose what happens to it when its
master dies, and override the release sound.

```ini
[SLAVE]                             ; InfantryType
Slaved.OwnerWhenMasterKilled=killer ; killer|master|suicide|neutral, default killer
SlavesFreeSound=                    ; NullableIdx<VocClass>, unset inherits the rule
```

Readers: `Ext/InfantryType/Body.cpp:18-19`, declarations `Body.h:13-14`,
constructors `Body.h:38-39`. `SlavesFreeSound` is nullable and falls back to
`[AudioVisual] SlavesFreeSound=`; `none`/`<none>` is an explicit silence. The
enum spellings are the native `Killer`/`Master`/`Suicide`/`Neutral` values
(`Utilities/Enum.h:178-184`), and an unknown spelling keeps the constructor
value.

## Master death

`SlaveManagerClass_Killed` hooks `0x6B0B9C`/`0x6B0BA4`
(`Ext/Techno/Hooks.Misc.cpp:31-74`) run per enslaved infantry, before the
vanilla owner change. For a slave type:

- `killer` (default): the killer's House; without a killer, the `Neutral`
  country's House (`HouseClass::FindNeutral`) else death — the hook's
  `0x6B0BA4` origin ignores the explicit House argument.
- `master`: the slave keeps its current owner (still re-owned, reset to Guard
  and cheered through the same tail).
- `suicide`: the slave is damaged to death with `C4Warhead`; the vanilla
  0x6B0BDF arm.
- `neutral`: the `Neutral` country's House when one exists, else the `killer`
  decision.

The vanilla free arm also gets a Phobos patch at `0x6B0BF7`
(`Hooks.Misc.cpp:76-78`) changing the suicide hit's ignore-defense argument to
true, so armor multipliers no longer keep a slave alive.

VERA keeps the decision in `sim/slave_manager.rs::free_slaves_retaining_storage`:
each live slave reads its own type's `phobos_slaved` settings, and
`slave_dies_unfreed` passes `ignore_defenses` under the feature. In-limbo
slaves keep the existing "dies with the master" arm before the owner choice.

## Release sound

`FreeSlaves`' hook `0x6B0C2C` (`Hooks.Misc.cpp:8-21`) plays the first freed
slave's type `SlavesFreeSound=`, else the fixed `[AudioVisual]` value. VERA
reads it after the loop from the first slave that changed owner; a slave that
dies instead never becomes the sound's position source.

## Residuals

- The type `SlavesFreeSound` cue name is retained without a Vox-catalog
  resolution, so an unknown authored name still emits a sound event the
  playback layer drops; native `NullableIdx` keeps the unset state and plays
  the global. Trigger: an unknown type sound name. Frequency: authored mods.
  Risk: a silent cue.
- The suicide test uses a `Verses=100%` warhead, so the `0x6B0BF7`
  ignore-defense patch's effect on armor multipliers has no focused
  regression; the flag is set under the feature.
- `neutral` needs a House whose type is the `Neutral` country; the regression
  registers one, and without it the arm falls back to `killer` as native does.
- Retail rules author none of these keys; the defaults reproduce the stock
  killer/global-sound behavior.
