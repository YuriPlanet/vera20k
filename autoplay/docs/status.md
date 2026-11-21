# 现状

更新于 2026-11-21，分支 `feature/autoplay-v4`。
**这份文档只写当前代码的事实。** v2 时期的实测记录在 `history/status-v2.md`。

---

## 1. 跑的是什么

一个由策略驱动的房子：

- **默认大脑** `sim::strategy::assist::AssistBrain`（20 文件，约 14,900 行），通过
  `VERA20K_BRAIN` 选择；不设或 `assist` 都是它，`v2` 切引擎外的
  `autoplay::policy::Policy`。
- 它做的事，按模块：展开 MCV（`expand`）、花掉钱（`build` 建基地、`composition`
  填生产线）、使用已有的（`orders` 工程师/间谍/驻扎、`advance` 推%).
- 每一步都是玩家能发出的命令：部署、建造、放置、对自己单位的下令。**它不跑
  native 电脑 AI 的决策块** —— 人类房子在 native 里本来就不跑那些。

## 2. 已验证（2026-11-21）

| 项 | 结果 |
|---|---|
| `--profile fast` 构建 | 通过，`vera20k.exe` 约 91 MB |
| `cargo test -p vera20k-core --lib -- sim::strategy::assist` | **172 passed** |
| 改名 `retail` → `assist` | 无行为改动，构建与测试通过 |
| 会话重构（`AutoplaySession`） | 行为保持：同一 host 调用、同一 take_back、同一顺序 |
| 上游合并 `0e831f2c3` | 上游 377 提交并入，冲突四处，测试通过 |
| 驻扎上限 `GARRISON_MAX = 4` | 步兵流失停止，limbo 52 → 8 |
| 袭击支队（raid） | 三种子里出现第一场胜利（seed 24301，33977） |

## 3. 基线（对局强度）

判据工具：`python -m tools.autoplay_check`，Death.mmx、america、normal、36000 ticks。
每一格跑完写一行 JSON 到 `target/autoplay-results/`，`judge()` 只认四件事：
军队（fighters）、闲置（idle）、未认领（unclaimed）、**push 门翻转次数**（`push_flips`）。

| 基线 | 提交 | seed 24301 | seed 19 | seed 10 |
|---|---|---|---|---|
| baseline-v2（驻扎上限后） | `7934c2764` | timeout own=130 army=68 | timeout own=108 army=63 | timeout own=121 army=72 |
| raid（支队不再看军队大小） | `cbeb8ec34` | **won 33977** own=155 army=84 | timeout own=128 army=66 | timeout own=106 army=55 |
| 基地锚点不再被第二个指挥中心夺走 | `e75815a59` | **won 32491** own=158 army=84 | timeout own=128 army=66 | timeout own=106 army=55 |

读法：三局都打满 36000 ticks 之前，只有第一场胜。第一场胜利来自「屯兵 + 袭扰」
两条线分开：fist 留在家里等 1.2 倍优势，支队去打法娘的采矿与精炼。

### 试过并且更差的（不要重试）

- **`ARMY_TO_ATTACK` 10 → 20**（军队门槛抬高，想「屯更多兵再出门」）：三局
  全部 timeout，连原本 32491 胜的那局也丢了。原因不是门槛本身——`army at first
  push` 三局都还是 10，说明门不是它在把；抬它把 `fist_needs` 一起抬到 20，
  支队被饿死，袭扰线没了。
- **推进目标的三种改法**（按角色权重分配目标 / 从军队中心瞄准拳头 / 一个目标
  打到最后）都输掉原本 tick 25636 赢的那局，见 `src/sim/strategy/assist/mod.rs`
  的模块头注。

### 为什么基线必须是 36000 ticks

`PUSH_REST_FRAMES = 750`、census 采样每 600 帧一次，`autoplay_check` 的
`PROGRESS_FRAMES = 150`。更粗的采样会把两次门翻转混成一次，翻转次数就读不出来。

## 4. 已知坏的 / 停用的

| 项 | 状态 | 原因 |
|---|---|---|
| `WALL_BREAKER_PRIORITY` | 未接线，`#[allow(dead_code)]` | 拆墙规则没有调用点 |
| `MAIN_BATTLE_MAX_ENEMIES` | 未接线，`#[allow(dead_code)]` | 步兵下限规则没有调用点 |
| `SCOUTS_OUT` | 未接线，`#[allow(dead_code)]` | 侦察规则没有调用点 |

## 5. 结构（改代码前必读）

```
sim/behavior  L1 执行：目标词表、单一命令漏斗、托管微操、ready 门
sim/control   L2 控制：ControlBoard、Brain port、预算、Observation 投影、
                   感知、选址、外部契约、托管表
sim/strategy  L3 决策：assist 大脑 —— 默认
autoplay/     引擎外的 v2 策略 crate
```

依赖方向 L3 → L2 → L1，`src/architecture_guards.rs` 守着。L2 不指名具体大脑。

## 6. 残留（从代码头注摘录）

- **推进**只有一条规则：军队规模 → 最近敌方建筑。没有撤退、重整、波次时机。
- **绕行**（`avoidance`）是两段几何，不是寻路：不认识墙，两面火力之间没缝时会停在近的那把枪边缘。
- **没有 native `TeamClass`**：165 个 `AITriggerTypes` 到 tick 6000 没有一个产出完整队伍。
- **军队看整张图**：雾的诚实性仍是公开问题（`docs/autoplay.md`）。
- **目标选择**是一条谓词，不是 native 那条：交战国、最近优先、基地半径内。
- **没有基地时** `advance` 读不出威胁，于是推进；但它不会重建（`CanBuild` 可能拒绝 maker）。
