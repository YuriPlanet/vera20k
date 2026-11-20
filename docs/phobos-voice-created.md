# Sound on unit creation (`VoiceCreated`)

A `NullableIdx<VoxClass>` line that a factory's completed object speaks, and a
switch that trades the EVA "unit ready" line for it.

## Keys

| Key | Type | Default |
|---|---|---|
| `VoiceCreated=` | `NullableIdx<VoxClass>` | unset |
| `[AudioVisual] IsVoiceCreatedGlobal=` | `Valueable<bool>` | `no` |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:853`),
declaration `Ext/TechnoType/Body.h:119`, constructor `Body.h:602`. VERA reads
`VoiceCreated=` in `src/rules/phobos_techno_extras.rs` (owner
`ObjectType::phobos_techno_extras.voice_created`) and
`IsVoiceCreatedGlobal=` in `RuleSet::phobos_globals` (`Ext/Rules/Body.cpp:306`,
constructor `Ext/Rules/Body.h:738`).

## Behaviour

Two hooks in `HouseClass::UnitFromFactory` (`Ext/House/Hooks.UnitFromFactory.cpp`):

```cpp
// 0x4FB63A, replacing VoxClass::Play(EVA_UnitReady)
if (!RulesExt::Global()->IsVoiceCreatedGlobal.Get())
    VoxClass::Play(GameStrings::EVA_UnitReady);

// 0x4FB64B
if (pThisTechno->Owner->IsControlledByCurrentPlayer() && pThisTechnoType->VoiceCreated.isset())
{
    if (RulesExt::Global()->IsVoiceCreatedGlobal.Get())
        pThisTechno->QueueVoice(pThisTechnoType->VoiceCreated);
    else
        VocClass::PlayAt(pThisTechnoType->VoiceCreated, pThisTechno->Location);
}
```

The EVA suppression is unconditional: it does not care whether the produced
type names a `VoiceCreated=`. The played line is the opposite — silent unless
the key is set *and* the owner is the current player.

VERA splits the two:
- `crate::app::match_runtime::sound_dispatch`'s `SimSoundEvent::UnitComplete`
  arm drops the `EVA_UnitReady` request when the global is on, after the radar
  accept has already run (native's `0x4FB631` precedes the hook).
- `sim::production::production_placement` emits the line itself, under the same
  `owner_is_human` gate as the EVA request: `SimSoundEvent::UnitVoiceRequested`
  for the `QueueVoice` arm and `SimSoundEvent::VocAt` (positional
  `VocClass::PlayAt`) otherwise.

## Residuals

- **Parse-time Vox resolution.** Native converts the name to a Vox index while
  reading (`NullableIdx<Lookuper>::Read`, `Utilities/TemplateDef.h:1878-1895`);
  VERA has no Vox registry at rules-parse time, so the **name** is kept and
  resolved at use — the same deferred-lookup DRIFT `[CrateRules]
  HealCrateSound=` records.
- **`IsControlledByCurrentPlayer` vs `IsHumanPlayer`.** Native's second hook
  uses the controller test; VERA uses the same `owner_is_human` gate the EVA
  line uses. The two differ only for a mind-controlled factory owner.
- **Blank `VoiceCreated=`.** Native sets the nullable with index `-1` (an
  explicit "no voice"); VERA treats a blank read as unset. Both are inaudible;
  only the `isset()` gate differs, and with no sound to play it has no effect.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group.
- `src/rules/phobos_globals_tests.rs` — `the_disguise_voice_and_hover_globals`.
