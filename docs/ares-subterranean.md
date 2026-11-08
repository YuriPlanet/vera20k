# Subterranean units and Ares dig effects

No retail Yuri's Revenge type digs, but `gamemd.exe` still carries Tiberian
Sun's subterranean locomotor, `TunnelLocomotionClass`. VERA20K ports it in
every build, so a mod can select it:

```ini
[DIGGER]                                              ; rulesmd.ini VehicleType
Locomotor={4A582743-9839-11D1-B709-00A024DDAFD1}      ; Tunnel (Phobos alias: Tunnel)
ROT=8                                                 ; dig-in/dig-out time is 64/ROT frames

[General]
TunnelSpeed=1        ; scales the sink and rise speed, divides the dig-in time (default 1.0)

[AudioVisual]
DigSound=...         ; played as the unit digs in and out (retail: the nuke siren)
Dig=...              ; anim spawned as it digs in and out (retail: unset)
```

Theater INIs gate where a unit may surface with `[TileSetNNNN]
AllowBurrowing=` (default yes).

## What the unit does

A move order goes through the Unit setter. A trip of fewer than 12 cells
whose ground route is cheap, a destination on a bridge, or a cell that refuses
burrowing (a building, a terrain object, a slope, a bridge or
`AllowBurrowing=no`) drives on the surface: a Drive takes over until it
stops. Any other trip digs:

1. the unit turns toward the destination, then digs in for `64 / ROT /
   TunnelSpeed` frames, playing `DigSound` and `Dig`;
2. it sinks to 256 leptons below zero and leaves its cell;
3. it travels in a straight line, 19 leptons a frame, below every cell;
4. near the destination it surfaces, or, when its own cell is blocked,
   re-targets a nearby passable cell (no cell at all destroys it with
   `C4Warhead`);
5. it rises to the ground, reveals around it and digs out for `64 / ROT`
   frames, then stops.

Underground, it is in no cell list, so nothing scans, crushes, area-damages,
Chronoshifts or EMPs it. Its layer is the underground one, so weapons cannot
fire at it, and it draws as a ripple that a sensor query sees as hidden. It
cannot fire while digging. A surfacing unit given a new order queues the
order and finishes surfacing first. A war factory drives a Tunnel product out
of its bay on a Drive, as it does a Chrono Miner.

## Ares dig effects

Build with `--features ares` to let a TechnoType choose its dig effects:

```ini
[DIGGER]
DigInSound=...    ; replaces DigSound while digging in; `none` silences it
DigOutSound=...   ; replaces DigSound while surfacing
DigIn=...         ; replaces Dig while digging in; `none` spawns nothing
DigOut=...        ; replaces Dig while digging out
```

An unset key keeps the `[AudioVisual]` value. Ares also turns a dig-in with
no destination into an immediate stop instead of a turn toward the map
origin.

## Where VERA20K owns it

- `rules/locomotor_type.rs`: the Tunnel CLSID (`0x007E9A50`) and Phobos
  alias. `rules/ruleset.rs`, `rules/rules_type_references.rs`: `TunnelSpeed`
  (`0x0066EDA9`, constructor 1.0) and `Dig` (`0x0066A59B`).
  `map/theater.rs`: `AllowBurrowing` (`0x00546177`).
- `sim/movement/tunnel_movement.rs`: the class state, its timers and
  arithmetic. `sim/movement/tunnel_process.rs`: Process (`0x00728E30`),
  Move_To (`0x00728AF0`), Stop_Moving (`0x00728C00`), CellClass's burrow
  test (`0x00486FF0`), the Unit setter's Tunnel arm (`0x007427C0`) and
  `AStarAttempt` (`0x00746000`).
- Consumers: the underground layer (`sim/world/display_registry.rs`,
  `In_Which_Layer 0x0072A1A0`), the visual character (`sim/game_entity.rs`,
  `0x007291D0`), FIRE_MOVING (`sim/combat/fire_error_world.rs`,
  `0x0072A1C0`), the surfacing NavQueue arm (`sim/movement/track_path.rs`,
  `0x007422F7`) and the war-factory exit (`0x0044DEDD`).
- Ares: `rules/ares_techno_type.rs` reads the four keys;
  `sim/movement/tunnel_process.rs` plays them (`PlayDigEffects`, Ares.dll
  `0x100443E0`, from hooks `0x728EF0`, `0x7292CF`, `0x7297C4`, `0x7299A9`)
  and ports the null-destination fix (`0x1005D500`, hook `0x72920C`). The
  fifth hook, `0x7293DA`, sits in `ProcessDiggingIn` (`0x00729370`), which
  nothing calls.

## Not covered

- `MovementZone=Subterannean`'s own Unit-setter arm and planner
  (`0x00742097`, `0x007447B0`), its click-order flags (`0x004D785F`,
  `0x004D7A3B`, `0x004DE3DB`) and its `Find_Path` flag (`0x004D3AD0`). A
  Tunnel unit with that zone takes the ordinary arms.
- Reinforcements that arrive underground (`0x0065E010`, an all-Tunnel team)
  and a Tunnel infantryman's move orders.
- The dig tilt and depth drawing (`Draw_Matrix 0x00729B40`, `Z_Adjust
  0x00729E50`); the new Drive's `Force_New_Slope` (`0x00742BE3`).
- A save during a timed dig phase resumes that phase where native's Load
  restarts it (`0x0072A150`).
