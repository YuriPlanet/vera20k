# The work queue

Audited against `spec.md` and `G:/Documents/autoplay-v2.txt` on 2026-11-03. One
line per item, with the reading that accepts it and its real state. The chronicle
is status.md; the design is design.md; the acceptance criteria are spec.md.

## The current target: 1v7 easy, survive

Set 2026-11-03, and reached. Death.mmx, seven computer houses, 24000 ticks, three
seeds, **all three survive**:

| Seed | own | enemy | lost | result |
|---|---|---|---|---|
| 0x5EED | 57 | 287 | 18:5 | timeout |
| 19 | 41 | 298 | 13:10 | timeout |
| 10 | 50 | 288 | 6:11 | timeout |

The same build on the neighbouring lanes, which is what makes the target
meaningful:

| Lane | Result | own / enemy | lost |
|---|---|---|---|
| 1v1 easy | timeout | **65 / 42** | 5:4 |
| 1v7 easy | timeout ×3 | 57 / 287, 41 / 298, 50 / 288 | 18:5, 13:10, 6:11 |
| 1v7 normal | **lost at 17188** | 9 / 375 | 32:0 |

**Read the result column before reading anything else.** All three 1v7 easy cells
are `timeout`: the house stood until the clock ran out. No enemy house was
defeated, and `enemy` climbs from 119 at tick 0 to 287 at 23400 while ours peaks
at 72 and falls to 57. Surviving is the milestone; it is not a win and nothing
here claims one.

What is settled by it: the difficulty parameter is live (normal dies at 17188
with the house reduced to nine objects), the army does engage (seed 0x5EED turns
`engage=true` at tick 9000 and `seen` climbs 0 → 19), and the remaining gap is
**structural** — seven houses open with 119 objects against our 13, and the
tactical layer is deciding inside that ratio.

Reproduce:

    python -m tools.autoplay_check --brain v2 --seeds 0x5EED,19,10 \
        --maps Death.mmx --ticks 24000 --opponents 7 --difficulty easy \
        --history target/autoplay-results/easy1v7.jsonl

Three minutes per match. The capability table prints after the matrix.

**Read the state column as a fact, not an intention.** `DONE` means a measured
reading exists. `PARTLY` means code exists and the spec's judgement does not
hold. `STUB` means the manager is an empty `propose` — the file exists so the
registry has an owner, and it decides nothing. `NOT STARTED` means neither.

The single number that governs this page: **of the four managers, two decide
nothing.** `manager/build.rs` (10 lines) and `manager/abilities.rs` (9 lines) are
stubs. Everything the spec asks about walls, placement, repairs, transports,
capture, infiltration, garrisons and superweapons belongs to them.

## Where the work stands

Measured on the current HEAD, retail YR, hard computer, six-cell matrix
(Death.mmx and Barrel.mmx, seeds 0x5EED / 19 / 10, 12000 ticks):

| Reading | Value |
|---|---|
| own | 46-50 per cell; 24000 ticks reaches 67 |
| army (hit points) | ~7100 per cell and rising to 24000 |
| unowned | 0 in every cell |
| stuck | 0-3, harvesters only |
| lost | 0:0 on most cells, 0:2-0:8 where the column fights |
| 24000 ticks | six of six survive |
| 1v7, 12000 ticks | three of three survive |
| 1v7, 24000 ticks | lost |

The house builds an economy, keeps two refineries and four defences, runs
twenty-plus harvesters, composes a four-to-five-type army from the role model, and
marches a column that fights in the field. It does not reach the enemy base, does
not build walls, does not capture, does not transport, does not use a superweapon.

**Observed in one 24000-tick match (audit1.jsonl) — the capability count is zero
for every one of these:**

| Command class | Uses in 24000 ticks |
|---|---|
| `Capture` | 0 |
| `Transport` | 0 |
| `LaunchSuperWeapon` | 0 |
| `Garrison` | 0 |
| `Repair` | 0 |
| wall placement | 0 |

That table is the honest measure of how much of the wish list is live.

## The four managers, and what each owns

| Manager | File | Lines | Decides |
|---|---|---|---|
| Economy | manager/economy.rs | 305 | build order, harvesters, refineries, power, defences |
| Military | manager/military.rs | 713 | unit composition, column, march, engage, defend |
| Build | manager/build.rs | 10 | **nothing (stub)** |
| Abilities | manager/abilities.rs | 9 | **nothing (stub)** |

Of the fourteen `JobKind`s, the policy can only ever issue five: `Deploy`,
`Harvest`, `Assault`, `Defend` from the two live managers, and `ReturnHome`, which
`policy.rs` proposes itself for a unit whose fight is over far from home. The
remaining nine — `Repair`, `Capture`, `Garrison`, `Breach`, `Scout`, `Transport`,
`Retreat`, `ReturnOre` — are declared, registered, and never proposed by anyone.

## Reconciliation against the wish list

Grouped by the headings in `G:/Documents/autoplay-v2.txt`, so it can be read side
by side with it.

### 建造基地 (base building)

| Want | State | Evidence |
|---|---|---|
| 跨国家生产能力 | NOT STARTED | needs `Capture`; zero captures in 24000 ticks |
| 分基地 (MCV) | NOT STARTED | one yard, never a second |
| 围墙 / 防御塔 | PARTLY | four defences per cell (economy), **zero walls built** |
| 建筑不堵死 | NOT STARTED | no placement graph; buildings placed by the native site search only |
| 重要建筑不靠前 | NOT STARTED | no distance rule |
| 经济好时补电厂/工厂 | PARTLY | power is raised when short (`POWER_MARGIN`); no capacity check |
| 科技优先（雷达/精炼器/工业工厂） | NOT STARTED | build order stops at refinery → barracks → factory → 2nd refinery → defences |
| 出售保不住的建筑 | NOT STARTED | no sell decision |
| 占据矿脉建分基地 | NOT STARTED | — |
| 经济管理滚雪球 | PARTLY | two refineries and `HARVESTERS_PER_REFINERY = 2`; wallet still freezes at 300-900 |
| 回收厂/维修厂/克隆缸 | NOT STARTED | `Build` is a stub |
| 切换主生产工厂 | NOT STARTED | no `TogglePrimaryFactory` use |

### 建造单位 (unit production)

| Want | State | Evidence |
|---|---|---|
| 按战斗情况和单位特点生产 | PARTLY | picker reads the role model against visible armour (`unit_role`), 4-5 types per cell; no reaction to the enemy's changing mix (spec P1) |
| 队列不闲置 | PARTLY | queue idle much of the match; `economy: nothing to build` on most readings |

### 小队作战 (squads)

| Want | State | Evidence |
|---|---|---|
| 组合有效的战斗小队 | PARTLY | column marches and fights; composition by role is done. No squad as a unit of state — military recomputes per term. Spec S1 (≥2 roles, ≥4 units) holds by accident, not by rule |
| 不在家门口集结 | PARTLY | frontier march starts near home and walks out; never asserted |

### 单独作战 (per-unit behaviour) — the largest gap

| Spec ID | Want | State |
|---|---|---|
| U1 | 风筝 | NOT STARTED |
| U2 | 躲避弹道 | NOT STARTED |
| U3 | 绕开威胁 | NOT STARTED |
| U4 | 打不过就跑 | **tried and reverted** — keyed on `seen`, fired zero times (status.md) |
| U5 | 对策性 | PARTLY — the picker buys the right counter; individual units do not decline bad targets |
| U6 | 子弹分配 | NOT STARTED |
| U7 | 卡位攻击 | NOT STARTED |
| U8 | 保护英雄/高价值 | NOT STARTED |
| U9 | 英雄照常参战 | NOT STARTED |
| U10 | 引诱/扛伤 | NOT STARTED |
| U11 | 自爆单位 | NOT STARTED |
| U12 | 感染寄生 | NOT STARTED |
| U13 | 掩护关键单位 | NOT STARTED |

Thirteen of thirteen unstarted or reverted. This is the "最复杂" part of the
wish list and almost none of it exists.

### 单位功能 (unit capabilities)

| Spec ID | Want | State |
|---|---|---|
| A1 | 捡箱子 | NOT STARTED |
| A2 | 探索迷雾 | NOT STARTED — six attempts reverted; `seen=0` on every reading |
| A3 | 海上运输 | NOT STARTED — `Transport` job never proposed |
| A4 | 空中运输 | NOT STARTED |
| A5 | 渗透 | NOT STARTED — `Abilities` is a stub |
| A6 | 占领建造场（重中之重） | NOT STARTED — the wish list's stated priority is not begun |
| A7 | 特殊能力 | NOT STARTED |
| A8 | 矿车微操 | NOT STARTED |
| A9 | 建筑特殊能力 | NOT STARTED |

### 中立建筑 (neutral structures)

| Spec ID | Want | State |
|---|---|---|
| N1 | 驻扎判断 | NOT STARTED |
| N2 | 开局全知 | PARTLY — L0 map data is available; nothing reads the neutral list |
| N3 | 重点设施保护 | NOT STARTED |
| N4 | 过远放弃 | NOT STARTED |

### 能力使用 (support powers)

| Spec ID | Want | State |
|---|---|---|
| C1 | 支援能力使用 | NOT STARTED — zero launches |
| C2 | 超级武器目标 | NOT STARTED |

### 组合技 / 压制

| Spec ID | Want | State |
|---|---|---|
| K1 | 伊文装炸药 | NOT STARTED |
| K2 | 工程兵进 IFV | NOT STARTED |

### 技术层面 (the interface)

| Want | State | Evidence |
|---|---|---|
| 类 bwapi 的查询/命令接口 | **DONE** | `Observation` + `Outbox` + `Funnel`; a brain reads a projection and answers through the same door a click uses |
| 可接入神经网络等外部模块 | **DONE by design** | a brain is `Box<dyn Brain>`; `match.rs --brain` selects one by name |
| 独立的 BOT，纯代码驱动 | **DONE** | `autoplay/` is a workspace member outside the engine crate |
| 会计算识别单位能力 | **PARTLY** | `rules::unit_role` scores a type against armour and reads `Capabilities`; it is the strongest part of the work |

### 限制 (constraints)

| # | Constraint | State |
|---|---|---|
| 1 | 全局/生产/战斗/能力 四个开关 | **DONE** — `AutoPlayDomains` on the board |
| 2 | 不控制不可控单位 | PARTLY — `orderable` is checked; not asserted for scripted transports |
| 3 | 每局状态独立 | assumed, not tested |
| 4 | battle control offline 时停止 | **DONE** — `sim.session().input_locked()` returns empty |
| 5 | 编队单位不托管 | **DONE** — `take_back` / `excluded` |

### 历史问题 (H1-H18 permanent counters)

Spec §5 asks for eighteen counters, each printed in the matrix and asserted so it
cannot regress. The harness judges three fields —
`tools/autoplay_check.py: JUDGED = ("own", "unowned", "stuck")` — and the census
prints nine columns (tick, credits, enemy credits, power, own, enemy, ready,
rejected, not ready) plus `army` and `lost`.

Mapped against the eighteen:

| Spec counter | Reading that covers it | State |
|---|---|---|
| H2 墙口卡死 | `stuck` | exists, but count is harvesters on ore |
| H15 执行成功后发呆 | `stuck` + `finished` | partly |
| H16 兵种单一 | role count per match | exists, asserted |
| H5 高频指令 | `issued` per term | partly (counted, not asserted ≤1) |
| H1 / H7 挂机 | `unowned` | exists |
| H3, H4, H6, H8-H14, H17, H18 | — | **not counted at all** |

So twelve of eighteen have no counter. This is the cheapest large win available:
most need one line in the census, and they turn "we fixed it once" into a number
that cannot regress. It is P0 below.

## What this means for the queue

The old queue below was written when the army was the whole story. It is now
badly out of date: it lists B1 as done on a reading that has since been superseded,
B3 as done when the flag is on, and D4 as open when six attempts have been reverted
and the question has moved.

Priority, by what the spec judges and what the code cannot do:

**P0 — the 1v7 easy economy.** Seven houses hold 287 objects at tick 23400 while
the house holds 57, and the wallet reads 300-900 credits with `economy: nothing
to build` on most readings. Three saving rules have been tried and all lost to
the army, so the direction to try is the other one: a cheaper truck, a closer ore
field, or ore worth more per trip. This is the only line that changes the ratio
rather than the fight inside it.

**P1 — something that can remove a building.** `lost` on the easy lane is 18:5,
13:10 and 6:11 over 24000 ticks: the house cannot shrink a 287-object colony
five objects at a time. Artillery exists as a type (`HOWI` is buildable from the
first war factory) and the picker never buys it, because the only source of
artillery demand is the roster mix rather than the sight of an enemy building.

**P2 — the judges the spec asks for and nothing supplies.** Twelve of the spec's
eighteen H counters have no reading at all.

**P3 — the two empty managers.** `Build` and `Abilities` between them own walls,
placement, repairs, capture, transports, infiltration, garrisons and superweapons.

**P4 — per-unit behaviour (U1-U13).** Thirteen items, none started. The largest
single body of work in the wish list and it should not be attempted before P2
gives it a reading.

## The rule that decides

One change per measurement. Six cells, or three at 1v7. Keep the change when own
does not fall, no surviving seed is lost, unowned stays zero and stuck does not
rise — and read `army` and `lost` beside `own`, because a change that raises the
object count while killing nothing is a change that made the house hide.

Every measurement names the file it was compared against. **Run the whole test
suite, not a filtered subset**: one test in the tree had failed from the day it
was written and went unnoticed for four commits (handoff.md).
