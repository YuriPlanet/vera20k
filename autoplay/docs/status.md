# 现状

更新于 2026-11-20，分支 `feature/autoplay-v4`。
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

## 2. 已验证（2026-11-20）

| 项 | 结果 |
|---|---|
| `--profile fast` 构建 | 通过，`vera20k.exe` 约 86 MB |
| `cargo test -p vera20k-autoplay --lib` | **52 passed** |
| `cargo test -p vera20k-core --lib -- sim::strategy::assist` | **166 passed** |
| `cargo test -p vera20k --lib -- cameo_mirror` | **3 passed** |
| 改名 `retail` → `assist` | 无行为改动，构建与测试通过 |
| 会话重构（`AutoplaySession`） | 行为保持：同一 host 调用、同一 take_back、同一顺序 |

## 3. 没有验证的

- **对局强度**。下面所有胜负数字都来自 2026-11-02 的 v2 时期，此后策略改过
  （改名、会话重构、上游合并 390 提交），**没有重测**。
- **实机**。改名与会话重构之后没有启动过客户端。

### v2 时期的数字（仅作参照，不是 v4 的成绩）

1v1 hard，Death.mmx，12000 ticks，三种子（own vs enemy）：

| Seed | Own | Enemy |
|---|---|---|
| 0x5EED | 35 | 47 |
| 19 | 35 | 40 |
| 10 | 35 | 45 |

24000 ticks 两图三种子：六局里四局活到四十分钟。1v7 hard 三种子两活。
完整表格与判据见 `history/status-v2.md`。

## 4. 已知坏的 / 停用的

| 项 | 状态 | 原因 |
|---|---|---|
| 驻扎 `garrison` | 已注释掉，代码保留 | 实测 1v7 lane 上 own 84 → 17；每个步枪兵都承重 |
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
