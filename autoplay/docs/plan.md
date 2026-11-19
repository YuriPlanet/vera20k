# 计划书

这份文档取代 v2 时期的九份计划（`history/plan-v2.md`、`history/rebuild-plan.md`、
`history/requirements-and-plan.md`、`history/design.md`、`history/features.md`、
`history/queue.md`、`history/autonomous-plan.md`、`history/playtest-plan.md`、
`history/spec.md` 之外的那几份）。它们互相重叠、判据不一，且描述的是 v2 时期的代码。

---

## 0. 一句话现状

引擎侧三层齐全，`sim::strategy::assist` 是默认大脑（约 15k 行），`autoplay/` 是引擎外的
v2 策略。**能跑、能构建、测试绿；但强度自 2026-11-02 起没有重测，实机自改名后没有跑过。**

---

## 1. 两条边界

autoplay 这个词指两件相反的事。分开它们的判据，比合并它们更重要。

| | 托管（玩家委托） | 玩家替身（BOT） |
|---|---|---|
| 谁做决定 | 玩家 | 它自己 |
| 做什么 | 玩家一定会做、但不值得手动的事 | 建造顺序、扩张、兵种相性、进攻时机 |
| 可以激进吗 | 不可以，激进就是意外 | 可以，那是它的工作 |
| 验收 | 零意外 + 性能 | 功能 + 胜负 |
| 规模目标 | ≤ 500 行、≤ 6 条规则 | 不限 |

**本分支的现状：`sim::strategy::assist` 是右边那一列**（玩家替身，14,916 行），
不是左边。它没有伪装成薄层。`autoplay/src/policy.rs`（v2）也是右边那一列，只是
没达到可替换默认的水平。

左边那一列目前**不存在**。v5 分支写过 226 行的雏形（`autoplay/src/assist.rs`），
但它的端到端测试在装了 retail INI 的机器上是失败的，我们没有把它并回来。

## 2. 已经确定的事（不要再讨论）

| 结论 | 证据 |
|---|---|
| retail 大脑不是游戏自带 AI 的移植 | 人类房子在 native 里不跑 AI 块；模块头注与 `history/design.md` |
| 权重表更"正确"不等于行为更好 | `history/round-thirty-three.md`：per-role punch + concrete 单值，lost +110% |
| 威胁按造价加权、威胁计入建筑 | `history/excluded.md`：分别丢 GACNST、账本不变 |
| 驻扎在 1v7 lane 上是净负 | `0720ec741`：own 84 → 17 |
| 1v7 lane 没有余量，只能当回归报警器 | `history/queue.md`、`history/1v7-regression.md` |
| 军队正效益全靠 MTNK | `history/session-close.md`：MTNK 86‰，其余 5–8‰ |

## 3. 下一步（按建议顺序）

### P1 重测基线（先做，因为没有数字就无法判断任何改动）

v4 的胜负数字全部来自 v2 时期。跑一遍：

```bash
RA2_DIR=<retail> python -m tools.autoplay_check --brain v2 ^
  --seeds 0x5EED,19,10 --maps Death.mmx,Barrel.mmx --ticks 12000 ^
  --skip-tests --history target/autoplay-results/v4-baseline.jsonl
```

**验收**：一张 own / enemy / lost / result 的表，写进 `status.md`，注明 commit。

### P2 清理三处未接线的规则

`WALL_BREAKER_PRIORITY`、`MAIN_BATTLE_MAX_ENEMIES`、`SCOUTS_OUT`（`manager/military.rs`）
目前带 `#[allow(dead_code)]` 保留着。两条路，选一条：

- **接线**：按 `history/round-*.md` 里的实测结论补调用点，然后重测。
- **删除**：如果实测结论是"接了也没用"，删掉并在 `excluded.md` 记一行。

**验收**：`cargo build` 无 `dead_code` warning，或三条规则都有调用点。

### P3 让 `docs/autoplay.md` 与代码一致

它是 499 行的操作层规格，最后更新于 2026-11-02，里面仍写 "the retail policy chain"。
改名后要同步，并且要写清"assist 是玩家替身，不是薄托管"。

### P4 决定要不要划托管/BOT 的界

只有在 P1 有数字之后才有意义。两条路：

- **不划**：承认 `assist` 就是玩家替身，把名字的含义写进模块头注（已写），不再提薄层。
- **划**：按 v5-plan 的 P0 做（retail 策略 feature gate、默认大脑换薄层），但先把薄层做出来并证明它能跑。

## 4. 明确不做（本轮）

- 不拓展任何新功能：不加 reflex、不加兵种、不加地图分析。
- 不把 `sim/strategy/assist` 搬出引擎（那是 v5 P3，单独一轮）。
- 不用愿望清单逐条打勾当验收。
- 不改 `E:\Projects\vera20k` 那条线。

## 5. 验收（这一轮的"做完"定义）

| 项 | 标准 |
|---|---|
| 构建 | `--profile fast` 全绿，无新增 warning |
| 测试 | `vera20k-autoplay --lib`、`vera20k-core --lib -- sim::strategy`、`vera20k --lib -- cameo` 全绿 |
| 文档 | `autoplay/docs/` 六份现行文档与代码一致；`history/` 冻结 |
| 数字 | `status.md` 里有 v4 的实测基线，注明 commit 与环境 |
