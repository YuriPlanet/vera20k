# Ctrl+D smart command

Ctrl+D is a single-player convenience hotkey that resolves to one of nine
actions from the *whole* current selection. It is a first-party feature in
every build (no `--features` needed) and is registered as `AutoLoad` in the
keyboard page's `vera20k` category, so it can be rebound or cleared like a
retail hotkey (`AutoLoad=0` keeps it cleared). Ctrl+D is installed as the
reload default when neither name has a saved binding.

The key only ever emits ordinary `Command`s — the same ones a mouse click
would produce — so a replay, a save or another client sees identical orders.
It never mutates simulation state directly. Admission (capacity, alliance,
bunker slots, iron curtain, …) is the engine's own read-only query, so an
order is only sent where the native click would also be accepted.

**Single-player only.** In campaign and skirmish (`session_mode().is_single_player()`)
the key works; in LAN/WOL it does nothing. The target search and "分散"
assignment are convenience heuristics whose balance has not been considered
for networked play.

## Design stance and future scope / 设计立场与未来扩展

Our position on this key is deliberate, and worth stating because a one-key
"do the obvious thing" command can otherwise look like an unfair advantage:

- **In single player it is a pure convenience.** Campaign and skirmish are
  where a player commands large groups; collapsing nine repetitive
  micro-actions into one key removes drudgery, not skill. It emits only the
  ordinary commands a mouse click would, so it grants no capability the game
  does not already allow.
- **In multiplayer it should be an opt-in switch, not a silent rule.** The
  door is intentionally left open: enabled, it lets allied players cooperate
  in PVE (shared loading, garrisoning, reactor feeding, charging each other's
  coils); disabled, it stays out of PVP where a batch micro-command could
  affect balance. The current build ships it single-player-only; the
  multiplayer switch is a future, explicit player/server choice — never
  something enabled behind a player's back.
- **The nine modes are a starting set, not a ceiling.** Because every mode is
  just a selection of ordinary `Command`s, the same dispatch can grow to cover
  Ares and Phobos capabilities (their own garrison, charge, delivery and
  infiltration behaviours) without changing the architecture. New modes are
  added as separate, mutually-exclusive roles in `role_of` / `plan_mode`.

我们的立场是明确的，值得写下来——因为一个"一键做显然之事"的命令，若不加说明，
很容易被看成不公平的优势：

- **单人游戏里它是纯粹的便利性。** 战役与遭遇战正是玩家指挥大群单位的场合，
  把九种重复微操作收进一个键，省掉的是繁琐，而不是技巧。它发出的都是鼠标
  点击本就会产生的普通指令，没有赋予游戏本身不允许的任何能力。
- **多人游戏里它应当是可选开关，而不是悄悄生效的规则。** 我们有意留了这个
  扩展点：开启时，队友可以合作打 PVE（互相装车、进驻、喂反应炉、给磁暴线圈
  充能）；关闭时，它就不出现在 PVP 里，避免批量微操作影响平衡。当前构建仅在
  单人模式启用；多人开关是未来一个明确的玩家/服务器选项——绝不会在玩家不知情
  的情况下默认开启。
- **九种模式是起点，不是上限。** 因为每种模式都只是挑选普通 `Command`，同一套
  分发机制可以继续扩展以支持 Ares 与 Phobos 的能力（它们各自的进驻、充能、
  投送、渗透行为），而无需改动架构。新模式以 `role_of` / `plan_mode` 中独立且
  互斥的 role 加入即可。

## Mode detection (mutually exclusive)

The mode is picked from what every selected unit has in common. Each unit gets
one role by the most specific capability, and the action fires only when *all*
roles match — a mixed group does nothing (except transport loading). The
order matters because the engine projects `infiltrate = infiltrate | c4 |
engineer | agent`:

| Priority | Selected units all having | Action | Emitted command |
|---|---|---|---|
| 1 | a carrier **and** a boardable unit (mixed) | balanced transport load | `MegaMission` + `Enter` |
| 2 | `Engineer=yes` | repair / capture nearby buildings | `CaptureBuilding` |
| 3 | `C4=yes` | sabotage nearby hostile buildings | `PlantC4` |
| 4 | `Agent=yes` | infiltrate nearby enemy buildings | `CaptureBuilding` |
| 5 | `Ivan=yes` | attach bombs to nearby objects | `Attack` |
| 6 | secondary weapon warhead `ElectricAssault=yes` | charge nearby friendly coils | `Attack` |
| 7 | `Infiltrate=yes` (and none of the above) | infiltrate nearby enemy buildings | `CaptureBuilding` |
| 8 | `Occupier=yes`, no reactor in range | garrison nearby structures | `CaptureBuilding` |
| 9 | tank (`Bunkerable=yes`) | enter nearest tank bunker | `MegaMission` + `Enter` |
| 10 | other infantry | feed nearby bio reactor | `MegaMission` + `Enter` |

Retail riflemen are both `Occupier=yes` and plain infantry. When a bio reactor
with a **free `Passengers=` slot** is within its (tight) 4-cell range, modes
8/10 resolve to Reactor instead of Garrison — a face-to-face reactor wins over
a garrisonable building further out, so it is no longer skipped. A full reactor
is not a target at all: it neither flips the mode nor receives orders, so the
soldiers keep their garrison order instead of endlessly queueing to feed a
full hold. Orders are capped per reactor at its remaining vacancy.

## Targets and ranges

| Action | Targets | Range (cells) | Notes |
|---|---|---|---|
| Transport load | selected carriers | 30 | original AutoLoad balance |
| Engineer / Spy / Infiltrate / Garrison / Reactor | matching buildings | 10 (reactor 4) | every selected actor gets an order in one press, spread evenly over the buildings; reactors are capped at their remaining `Passengers=` slots and full ones get no orders |
| C4 | hostile `C4=yes` buildings | 10 | one actor per target so bombs don't stack |
| Charge | friendly `Overpowerable=yes` coils | 10 | up to three chargers per coil |
| Tank bunker | empty `Bunker=yes` structures | 5 | deliberately tight so a tank never crosses the map; one tank per bunker |
| Ivan bomb | bombable living enemy Techno, no bomb already attached | 10 | one bomb per target; never bombs the planter or an ally |

Ranges are in map cells (1 cell = 256 leptons). Targets must be inside the
radius of at least one selected actor.

## Feedback

Ctrl+D posts one unified EVA line gated by the Detailed EVA Text option:
`VERA:EvaAutoLoaded = Quick actioned {count} units` (zh-CN: `快捷操作了 {count} 个单位`).
The count is the number of orders actually scheduled, never a guess.

## Implementation

- `src/app/input/smart_command.rs` — mode detection, target enumeration,
  even-spread assignment and command generation.
- `src/app/input/autoload.rs` — the transport-loading planner reused by mode 1.
- `src/app/input/dispatch.rs` — routes `HotkeyCommand::AutoLoad` to the smart
  command.
- See [VeraX](verax.md) for the extension namespace this hotkey moved out of.
