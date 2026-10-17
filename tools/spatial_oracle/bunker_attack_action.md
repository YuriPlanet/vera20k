# Object action and allied-House controls

The default `bunker_attack_action` mode retains its historical action, bunker,
deployer, Display and repair payload. Its assumptions remain in that sidecar;
the ordinary/default fixture is not a retail construction or full weapon-range
oracle.

```
python -m tools.spatial_oracle.bunker_attack_action --check
python -m tools.spatial_oracle.bunker_attack_action --allied-objects --check
```

Select the supported original executable with `VERA20K_GAMEMD_EXE` (see
[native_oracle.md](../native_oracle.md)). Add `--write` only to deliberately
regenerate a reference from original execution. The additive mode writes
`allied_object_action.json` and its metadata sidecar. No native instructions are
patched. Both modes share `run()` and the same actor/map/Display bootstrap;
Move acknowledgment executes on that same VM. RNG initialization and continuation
reuse `gen_rng_vectors.seeded_struct`/`draws`, not a new generator.

## Consumption boundaries

`action_controls` execute original DetermineAction692610 ->Unit73FD50
->Foot4DDED0 ->Techno6FFEC0 and the original Display4AAE90 cursor path.
An original House4F9A90 observer records its real result; it is not substituted.
House indices and directional alliance masks are supplied inputs. The real
Unit owner getters6F9DC0/+3C and6F9DB0/+38 distinguish same-owner/index from
another allied House. The supplied ordinary MTNK's attack grants are false.

Each row keeps `input`, `action`, `cursor`, `trace`, plus:

- `ally_queries`: original real-owner alliance decisions;
- `left_up`: original4AB9B0 entry to its selection or dispatch boundary;
- `clicked_action`: original738890 re-query and Foot receiver, when dispatched;
- zero observed RNG draws, action-timer starts and detach calls within that bound.

Ordinary other-House ally: action2/cursor19, LeftUp dispatch4AE750, then
NoMove receiver4D77BD ->Move admission4D77FB ->navigation boundary4D78BA.
Same-owner unselected: action7/cursor17 and replacement-selection boundary.
Enemy and forced Ctrl: action5/cursor20. Shift explicitly selects the toggle
path (action8); it must not be conflated with unmodified allied input.

The NoMove result is **not** a command refusal. The fixture stops before
navigation coordinates and map passability/nearby-cell choice; no expected
destination or movement RNG is invented. Original4D7C76..4D7CBA subsequently
emits mission2 with the chosen Object/Cell through existing ClickedMission.
The app/Core movement and input tests consume that existing production owner.

`grant_controls` execute complete Techno6FFEC0 only. IvanBomb+target
Passengers>0 blocks AttackCursorOnFriendlies; AttackFriendlies overrides that
block. After rejection, the full Unit wrapper would enter transport admission,
which this ordinary action fixture does not initialize. No transport callback
is replaced to manufacture an ordinary-path verdict. Sonic+Foot infestation
and other no-capacity grant controls retain the full ordinary caller path.
These are declared consumer priors, not parasite/bomb/transport lifecycle proof.

`voice_controls` distinguish `input.caller_entry`:

- `0x006FFBE0`: native Foot Move argument producer4D7C76 runs first with
  ESI=actor, EDI=supplied chosen Object and EAX=null planning Cell. It produces
  mission2/NULL attack target, then enters complete ClickedMission prefix,
  original non-planning query637AA0 and enable gate6FFCBD. Stop6FFDA5 precedes
  target/event encoding. The spatial choice is explicitly supplied.
- `0x00708FC0`: direct VoiceMove, documenting receiver ordering separately
  from admission through the order caller.

`output` contains full `main_before_hex`/`main_after_hex`, observed raw
`main_draws`, original `main_next_four`, Scenario/MapGen unchanged flags,
pending voice before/after, actual queue requests, VoiceMove call count,
native-produced Move arguments and boundary trace. Fixture sound IDs101/202/303
are supplied, not stock catalog indices. A nonempty list spends one Main draw
even for singleton or-1, before QueueVoice708D90's enabled/ID/House checks.
Disabled order caller skips the draw; direct disabled VoiceMove still draws.
House50B6F0's campaign human/player-control alternatives execute unchanged.
No sample/device playback is claimed.

`infantry_bomb_controls` append the original Infantry consumer51EB24..51EB7E
on the same VM. EDI=actor, ESI=ordinary target and EBP=canonical base action
are explicit priors; the actual Infantry/InfantryType vtables and native saved
register/local stack shape are supplied. No whole Infantry WhatAction prefix or
transport lifecycle is claimed. Original House50B6F0 and target+88 type getter
execute; no virtual result is substituted. The branch tests local control,
baseAction5, Ivan+EAE, Bombable+22E and attachedBomb+38. It consumes the canonical
Attack result without re-reading AttackFriendlies/AttackCursorOnFriendlies.

The AttackFriendlies=true/AttackCursorOnFriendlies=false positive returns53
(IvanBomb); nonbombable or already-bombed controls return54 (NoIvanBomb). Failed
House/Ivan/base-action admissions stop at51EB81 before the remaining Infantry
tail. Their `output.action` is null and `base_action_at_tail` is recorded; it is
not an invented final fallback action. No RNG, action-timer or detach entry is
observed. Production tests separately exercise the actual layered IVAN patch
and shared canonical attack admission before these cursor/click consumers.

## Retail and residual scope

`retail_inputs` records actual selected physical strings and archive/file hashes
using existing physical-layer and lexical owners. It is not a native scalar
reader execution. The sidecar cites original constructor/current-default reader
instructions for AttackFriendlies, AttackCursorOnFriendlies and LegalTarget.
The selected stack is RULESMD.INI, optional LANGRULE.INI, MPBattleMD.ini, then
XMP03T4.MAP. ART is fixed and is irrelevant to these logical gates. MTNK uses
105mm/AP and VoiceMove=GenAllVehicleMove; no selected later layer changes these
grant keys. Production-reader regressions validate their actual retained reads.

This comparison does not certify whole DetermineAction, Techno/Foot/Unit action
functions, Display input, arbitrary disguise/terrain/LegalTarget/RTTI routes,
transport, nearby-cell movement, events or audio lifecycle. Original full
Display dispatch clears voice enable after every actor/restores1 at4AE852/85F;
LeftUp restarts action-line timer25 at4ABFAE ->70D150. Those later effects are
instruction evidence and existing-owner production tests, not this bounded
window execution. No detach appears in the measured immediate route.

Historical sidecars retain their actual executable identities. If a historical
reference names3e81 while the selected file is1cdd, a strict provenance check
correctly refuses that identity change even when all payload behavior matches.
Use the shared [section-compatibility evidence](../native_oracle.md#steam-compatibility-and-evidence-identity)
to qualify a separate semantic replay; do not relabel or rebaseline old evidence.

## Attack acknowledgement

The additive `--attack-order-voice` mode writes `attack_order_voice.json` and
its sidecar. It reuses `run()` and the existing Move/Attack `voice_control`
recorder, RNG owner and native type/INI/Sound owners. Historical default and
allied outputs remain independent of this option.

```
VERA20K_SHRAPNEL_INPUTS=/path/to/existing/extract \
  python -m tools.spatial_oracle.bunker_attack_action --attack-order-voice --check
```

That existing extraction contains SOUNDMD.INI for the shared Sound owner.
The selected rules/mode/map bytes come from the original game's archives via
`weapon_laser.physical_layers`, rather than the extraction's older Hills map.
Use `VERA20K_GAMEMD_EXE` as above; `--write` deliberately reexecutes the corpus.

`readers.constructor` records whole native UnitType7470D0, WeaponType771C70
and Rules665650 construction results. `readers.layers` records actual section
admission526810 and selected unchanged reader blocks for RULESMD.INI,
optional LANGRULE.INI, MPBattleMD.ini and XMP03T4.MAP. This does not execute
whole file/type readers. Actual selected stock MTNK has Primary105mm,
SecondaryNULL, TurretCount0, IsGattlingfalse, VoiceAttack singleton and four
weapon-voice overrides−1. **Weapon Damage is+A4; AmbientDamage is+98.**

VoiceAttack vector+484 defaults empty and reads through712C5B..712CC5.
The four scalar slots580/584/588/58C default−1 (710F55..710F68), in order
`VoicePrimaryWeaponAttack`, `VoicePrimaryEliteWeaponAttack`,
`VoiceSecondaryWeaponAttack`, `VoiceSecondaryEliteWeaponAttack`.
Original713679..713775 uses ReadString128/Voc7514D0 and retains the current
slot on missing, empty or unresolved names. `readers.retained_controls` keeps
these native results, including absent/admitted sections, list duplicates,
unresolved list names and exact-case keys. No Python sound parser supplies the
expected values.

`readers.sound` retains the physical SOUNDMD source keys, selected Defaults,
sound sections, sample names and actual native binding. Numeric registry and
sample indices are local to the selected registry, not full-game numeric IDs.
Original Sound initialization and7514D0 lookup share the existing BulletReader
VM through `Sound.make_ini`/`mtnk_attack.sound_inputs`; physical archive loading,
AudioIndex/device/PCM and complete sound catalog are outside this comparison.

`readers.voice_ifv_repair` records Rules+230 constructor−1 at665976,
exact AudioVisual literal83A87C `VoiceIFVRepair`, native reader669D63..669DA7
(store669DA1) and selected physical `IFVMove`. The current signed ID is retained
for missing/empty/unresolved names. Actual RepairBullet Damage−50 is read from
the physical weapon section. The bounded FV consumer controls supply the type
identity/weapon selection prior; they do not claim the IFV gunner/repair lifecycle.

Each `voice_controls` row retains `input` and `output`. The two ordinary rows
are `stock_mtnk_target` and `stock_gapowr_target`. They execute native Foot
attack admission/argument producer4D769F with ESI=actor and its local stack
target slot supplied, then accepted mission1→6FFBE0→whole7090A0→QueueVoice.
Stop6FFDA5 precedes event encoding. Target class/type/vtable and live Unit/House
state are explicit consumer priors; full target/actor constructors and whole
ClickedAction dispatch are not claimed. Reader facts transfer by value between
the two existing reader/action runtimes, never as transplanted pointers.

During the measured continuation, the historical selector seam is released:
original Unit746CD0→Techno6F3330→GetWeapon70E140 executes unchanged. Stock
TurretCount0, original41BFB0=false and SecondaryNULL select primary0 before any
target read. The recorder explicitly reports `selector_target_memory_reads`.
Secondary override controls use the original DeployFire index branch with
declared supplied state; they are not stock MTNK deploy/secondary reachability.
Raw veterancy bits, list/scalar state, pending777 and RNG seeds are priors.

`output` includes actual produced mission/arguments, selector/weapon events,
queue requests and pending state, full before/after Main/Scenario/MapGen states,
raw Main draws and native next-four continuation. Singleton lists spend one Main
draw. Disabled caller skips7090A0; direct disabled/nonhuman calls spend the draw
before QueueVoice rejection. Empty lists spend none; scalar primary/secondary
and raw-elite overrides spend none. Negative-Damage literalFV uses Rules+230
without a draw when bound; unbound or non-FV falls back to the generic list.
The `fv_negative_unbound_bypasses_weapon_overrides` contrast retains all four
valid native-read overrides while Rules+230 is−1: original7090EB→70914A
bypasses those overrides and still spends Main on the generic list.
Missing-warhead controls establish that this decision only needs the raw weapon
and Damage, not Warhead resolution. No measured timer start or detach occurs.

NULL target with stock primary-only selection and valid slot.weaponNULL both
still reach generic voice selection. Selected index−1 is different: original
GetWeapon returns a NULL slot and7090C0 dereferences it. Its explicit direct
boundary control records that native fault before any RNG/queue request; no
fallback result or ordinary command reachability is invented. All whole action,
mission/event, arbitrary weapon/target and playback functions remain partial.
