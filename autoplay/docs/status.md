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

- **把 `Item::Refinery` 加进 `yard_reserve`**（想让第二个精炼厂也能存钱买到）：
  唯一一局胜利从 `won at tick 32491` 掉到 `won at tick 33964`，own 144 → 136，
  income 8613 → 5402，全局 `auto play: build` 行 785 → 242。加 `fighters>=` 条件
  也没用（那条件几乎恒真，两局数字完全一样）。步兵被钱荒饿死，
  `src/sim/strategy/assist/build.rs` 里那段注释早就写过这个价。
  另一条否证：seed 10 的收入在加与不加时都是 1943/min，可见它的贫穷不是
  「精炼厂不够」造成的，第二个 bay 不是那局的瓶颈。
- **`ARMY_TO_ATTACK` 10 → 20**（军队门槛抬高，想「屯更多兵再出门」）：三局
  全部 timeout，连原本 32491 胜的那局也丢了。原因不是门槛本身——`army at first
  push` 三局都还是 10，说明门不是它在把；抬它把 `fist_needs` 一起抬到 20，
  支队被饿死，袭扰线没了。
- **把 fist 的门单独抬到 20**（新增 `PUSH_ARMY`，`mop_up` 不再能把它降回 10）：
  机制确实生效了——`army at first push` 从三局都停在 10 抬到 31 / 39 / 35，
  但三局全部 timeout：seed 24301 从 `won at tick 32491` 掉到 own=134 enemy=34
  army=71 income=6956（基线 own=144 enemy=1 army=84 income=8613），seed 19
  own=127 enemy=59，seed 10 own=78 enemy=41。也就是说早出门的那十个兵不是
  「散着送」，而是维持压力和 income 的一部分：等凑够 20 个再走，攒兵的速度
  反而跟不上消耗，peak army 从 84 掉到 71。
- **推进目标的三种改法**（按角色权重分配目标 / 从军队中心瞄准拳头 / 一个目标
  打到最后）都输掉原本 tick 25636 赢的那局，见 `src/sim/strategy/assist/mod.rs`
  的模块头注。

### 已经排除的嫌疑（别再查）

- **步兵占比失控（编制层）**：一场看起来要输的局里 `infantry=59 / fighters=74`，
  55% 的上限像是没生效。加了 `AUTOPLAY_TRACE_COMPOSITION=1`（见 `31b7d9348`）
  实测：钱袋在大部分采样点是负的（-7492..3273），`vehicle_floor` 从 2000 落到
  600，`rich` 全程 false，20 个采样点里 15 个 barracks 是 held 的。步兵仍然
  从 7 涨到 56，是因为钱袋长期停在「车价 600」和「一兵 200」之间，而那一档
  正是设计好的 `a_soldier_would_reset_the_saving` 要放行步兵的档位。
  编制层不是漏洞，步兵过多的军队要从别处解释。
- **推进目标来回跳**：`target=` 在 seed 10 的尾段里 27 个采样点盯着 3105、
  14 个盯着 7017，整局只出现过 4 个不同目标；seed 24301 也只有 9 个，
  且单个目标能连盯 19 个采样点。目标承诺是有效的，拳头没有被拖着跑。

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
