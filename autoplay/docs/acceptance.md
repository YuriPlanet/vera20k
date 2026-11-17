# Autoplay acceptance: functional correctness and unattended completion

Two layers, both required. The functional layer asks *"did the feature fire when
it should?"*; the task layer asks *"could it finish a whole match by itself?"*.
A feature checklist can go green while the game is still unplayable, so the task
layer is what keeps the functional layer honest.

The reference matches in `autoplay/reference/` are the source of the functional
list: a function the reference never uses is not a gap.

## Functional layer

`autoplay-replay` groups every event it decoded into a checklist function and
prints a coverage table:

```
autoplay-replay --ra2-dir <retail> --replay <file.json> --out <dir> \
  [--house <name>] [--allow-rules-mismatch]
```

| function | meaning | why it is on the list |
|---|---|---|
| `mcv:deploy` | 基地车展开 | the match starts with it (tick 175) |
| `mcv:redeploy` | 基地车再部署 | the reference re-places captured yards |
| `build:power` | 电厂 | tick 374 |
| `build:refinery` | 矿厂 | tick 1,795 |
| `build:barracks` | 兵营 | tick 1,178 |
| `build:factory` | 战车工厂 | tick 5,538 |
| `build:airfield` | 机场 | tick 4,657 |
| `build:battlelab` | 作战实验室 | tick 7,759 |
| `build:depot` | 维修厂 | tick 7,063 |
| `build:radar` | 雷达/间谍卫星 | tick 9,296 |
| `build:defense` | 防御建筑 | tick 5,104 (NABNKR) |
| `produce:infantry` | 步兵 | tick 2,216 |
| `produce:vehicle` | 载具 | tick 8,261 |
| `produce:aircraft` | 空军 | tick 8,070 |
| `unit:engineer` | 工程师 | tick 965 (1v95) |
| `unit:spy` | 间谍 | tick 18,186 |
| `move` | 移动 | tick 2,583 |
| `attack` | 攻击 | tick 20,100 |
| `attack-move` | 进攻 | tick 78,075 |
| `guard` | 警戒 | — |
| `stop` | 停止 | tick 49,016 |
| `rally` | 集结点 | tick 43,182 |
| `capture` | 占领/渗透 | — |
| `repair` | 维修 | — |
| `sell` | 卖建筑 | tick 60,703 |
| `cancel` | 取消生产 | — |
| `superweapon` | 超武 | 1v95 (log) |
| `defend` | 防守（本方损失） | 322 losses |

A row with count **0** is a gap. A row with a count but a first tick far later
than the reference's is a timing question, not a pass.

## Task layer

The run is unattended: launch, walk away, come back to a finished match. The
criteria are evaluated from the report (`check_acceptance.py`):

| id | criterion | threshold | why |
|---|---|---|---|
| T1 | MCV deployed | ≤ 2,000 tick (90 s) | a base that never lands is the classic silent failure |
| T2 | first refinery | ≤ 16,000 tick (6 min) | the reference has one at 1:12 |
| T3 | first factory | ≤ 16,000 tick (6 min) | the reference has one at 2:03 |
| T4 | first attack | ≤ 108,000 tick (40 min) | the reference attacks at 28:55 |
| T5 | match ends | ≤ 243,000 tick (90 min) | the reference ends at 79 min |
| T6 | no stall | no 60 s window with credits, structures, units and army all unchanged | "broke and small" looks exactly like this |
| T7 | legal end state | no building queued with no factory, no MCV left undeployed | a match that ends with a stuck queue is not finished |
| T8 | no crash | the process ran to the end, no panic in the log | — |

T6 is the important one: it catches the autoplay's actual failure mode. A run
that ends at T5 with 2 buildings and 11 fighters has *completed* the match and
still failed T6 long before.

## Protocol

1. Build `autoplay-match` and `autoplay-replay` (release for real runs).
2. Run the autoplay on the reference setup and have it write a replay.
3. Read the replay with `autoplay-replay` (coverage table + events).
4. Grade it with `check_acceptance.py`.
5. Diff the coverage table against the reference's (`replay_tick214095.report.md`).

A change is accepted only if the coverage table gains or keeps rows and the
acceptance table stays green. A change that trades one for the other is a
regression, not a win.