# Ares NoManualFire: retained reader and ordinary input admission

Replay from the checkout root with Unicorn 2.1.4 and locally supplied binaries:

```powershell
$env:VERA20K_ARES_DLL = 'C:/your-install/Ares.dll'
$env:VERA20K_GAMEMD_EXE = 'C:/your-install/gamemd.exe'
python -m tools.spatial_oracle.bunker_attack_action --no-manual-fire --check
```

`--write` deliberately regenerates only `no_manual_fire.json` and its sidecar.
Historical default, allied-object and voice corpora are unchanged. The dedicated
payload records the Ares DLL's byte count and SHA256; every gamemd.exe build is the
same game, so it records none for the executable. Its Ares input is the
same enrolled DLL as `rules_oracle.ares_prerequisites`; no DLL, game executable,
MO INI or other mod asset is distributed.

The action owner remains `bunker_attack_action.run`. The reader controls reuse
`ares_prerequisites.Probe`; physical INI controls reuse
`rules_oracle.weapon_speed_order.retail_history`. There is no second action,
boolean parser, native mapper or physical INI implementation.

## Native reader established

The original whole TypeExt constructor `1003C710` writes the dword at `+4C4`
to 1 at `1003CDDA`, making `NoManualFire +4C6` false. The constructor control
starts that dword as `CCCCCCCC` and observes `01000000`.

Installed Ares hook `10040DF0`, registered at the original TechnoType reader
success/failure suffixes `716123` / `716132`, passes the current INI from the
original stack frame `+380` to `1003DEC0`. The common reader `1003DFB0` checks
the section named by current `Type.ID +24` through `100091C0`; an absent section
returns without field reads. The field block `1003F002..1003F018` reads literal
`NoManualFire` through `1000EBF0 -> 1000FC50 -> 10007860 -> 528A10`, with capacity
2048 and empty string default. It uses this passed Rules INI, with no ART switch.
The adjacent readers are NoManualUnload and NoManualEnter.

`10076CC0` accepts the first ASCII character: `1/T/Y` true, `0/F/N` false,
case insensitive. Thus `yeSplease`, `y`, `t` and `11` succeed. `on`, `off`
and `junk` are invalid. Missing/empty/invalid strings retain the prior field;
invalid nonempty strings request a parse diagnostic. There is no special
NoManualFire clamp or post-read rewrite on the inspected reader path.

The 24 `readers.controls` rows supply the already-read string at the declared
ReadString boundary. Their raw-space controls deliberately omit trimming at
that boundary. Use `physical_readers.rows[*].physical_ini` when testing a
production INI reader: those nine rows execute actual physical INI loading,
exact-case lookup and `528A10` trim. They establish missing/empty/invalid
retention, missing section, rejection of `NOMANUALFIRE`, and whitespace around
`yeSplease` / `no`. Each reached read records section, key, empty default and
capacity 2048 from the native stack.

The physical filenames declare a sequence of supplied files. This corpus does
not execute complete session startup, language digest admission or Rules reset.
Actual layer selection/order remains the existing Rules caller evidence:
RULESMD, admitted LANGRULE, selected mode, then map; preserve the current field
at each type read. See [the Rules file/layer contract](../../src/rules/mod.rs).
Unrelated fields in the whole extension reader are outside these controls.

## Native action and ordinary left click demonstrated

The installed `.syhks00` records register six displaced bytes at `700536`
(Object) and `7008D4` (Cell). Original exports `1004A460` / `1004A430` obtain
the actor from the Syringe frame `+0C`, use the original type getter, then read
`Type +2FC -> Ext +4C6`. False returns 0, executing the original displaced
instructions. True returns `70056C` / `700AB7`, entering the original
nonattack tail. The harness composes those exact exports with unchanged native
code and restores the complete CPU context after each hook call.

32 hover `rows` cover both routes, both reader-produced values and all eight
Ctrl/Shift/Alt combinations. They explicitly supply `mouse_over=true`, matching
the original hover caller `69300B`. DetermineAction forwards that 1 to the
Cell query, while `69263B` hard-codes 0 for Object queries. Object click
re-query also receives 0. Each row records these actual Unit-entry arguments
in `query_inputs`; they are not treated as one interchangeable boolean.
These are supplied normal Unit/House/weapon/map
priors and the historical environment/range services, rather than whole game
construction or range arithmetic. Selected observed outcomes:

| Route / modifiers | False | True | Hook reached |
| --- | --- | --- | --- |
| enemy Object, ordinary or Ctrl | Attack 5 / cursor 20 | NoMove 2 / cursor 19 | yes |
| enemy Object, Ctrl+Shift | Attack 5 / cursor 71 | NoMove 2 / cursor 19 | yes |
| enemy Object, Alt or Ctrl+Alt | Move 1 / cursor 18 | same | no |
| enemy Object, Shift or Shift+Alt | Select 8 / cursor 17 | same | no |
| enemy Object, Ctrl+Shift+Alt | Move 1 / cursor 71 | same | no |
| Cell, Ctrl | Attack 5 / cursor 20 | Move 1 / cursor 18 | yes |
| Cell, Ctrl+Alt | AreaGuard 26 / cursor 22 | same | no |
| Cell, ordinary / Alt / Shift / Shift+Alt | Move 1 / cursor 18 | same | yes |
| Cell, Ctrl+Shift, with or without Alt | Move 1 / cursor 71 | same | yes |

For Cell Ctrl with true, the original nonattack tail tests the forwarded hover
argument at `700B51` and returns Move 1 at `700C08`. This already occurs inside
Techno, rather than a None 0 result invented into a later Unit fallback.
`cell_return_values` observes EAX at `4DDDFE` (Techno returned to Foot) and
`7404FB` (Foot returned to Unit): both 5 with false, both 1 with true.
Their supplied movement state does not certify every movement fallback. All
32 full action/cursor results and hook visits are in the payload.

For ordinary enemy Object clicks, original DetermineAction `692610` traverses
Unit `73FD50`, Foot `4DDED0`, Techno `6FFEC0`, then Display `4AAE90`. LeftUp
`4AB9B0` dispatches the Object route; UnitClickedObject `738890` re-queries the
same action and hook. With false it reaches `6FFBE0` with mission 1 and the
hovered target. With true it preserves the nonattack fallback and reaches
`4D78BA`, before navigation-coordinate selection. Swallowing the entire click
would change native behavior.

These boundaries precede event encoding/delivery, navigation, firing and voice.
The existing click observer asserts no Main/RandomRanged draw, action timer
start or detach call in this bounded continuation. Later command effects are
outside the control. Original gamemd text and the enrolled Ares executable
section `10001000..10093000` remain unchanged.

## Manual/automatic boundary

These hooks govern UI action queries. They do not justify a simulation-wide
CanFire, AttackAI or target-acquisition restriction. The pinned upstream Ares
[reader](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/TechnoType/Body.cpp#L436)
and [two action hooks](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/Techno/Hooks.cpp#L2031)
corroborate this responsibility. The installed DLL instructions remain the
authority; that source revision is not claimed to match every DLL detail.

Script-issued or already-queued Attack and automatic targeting which bypass
these UI queries should retain their existing behavior. Whole combat/script/AI
execution is not compared here. Native action admission is demonstrated for
the supplied ordinary Unit path; full gameplay and Rust parity require their
own implementation/integration checks.
