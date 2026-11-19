# 自主开发计划（2026-11-06 起）

这份文档取代 `plan.md` 里的优先级判断。它只写我被授权独立执行的事、顺序、
每一步的验收标准，以及已知的坑。写完即生效，不需要逐轮请示。

## 一、为什么重排优先级

上一阶段（第 1–26 轮）的结论集中成三条，都指向同一处：

1. **六个种子全部正效益、全部不赢。** Arena 24000 ticks，净效益 +$2500..+$4900，
   交换比 1.454x..4.692x。
2. **军队从不打建筑。** 六个种子的 `destroyed` 类型全是 `SMCV / HTNK / E2 / HTK /
   DRON / SPYP / SHK`，没有一个敌方建筑。敌方在 Arena 上一共建出 26 个建筑
   （`NAWALL`×10、`NAPOWR`×5、`NALASR`×2、`TESLA`×2，另各有 1 个
   `NABNKR/NAHAND/NAIRON/NARADR/NAREFN/NATECH/NAWEAP`），`short_game` 下只要还有
   任一建筑就不判负。
3. **IFV 泛滥，且它打不动坦克和防御塔。** 实测 10 辆 `FV`（$6000）是最贵单项，
   超过 7 辆 `MTNK`（$4900）。

所以问题不在「赚钱」，在**买错了东西、打错了目标**。之前把精力放在侦察、
行军、账本上，方向对但优先级错。

## 二、根因（已验证，不是猜测）

### 根因 1：威胁分布是空的

`autoplay/src/manager/military.rs:437` 附近：

```rust
for enemy in ctx.obs.visible_enemy_entities() {
    if ty.category == ObjectCategory::Building || ty.spawned { continue; }
    if let Some(role) = role_against_armour(ty.armor_id()) {
        threat[role.index()] += 1;
    }
}
```

三个缺陷：

| 缺陷 | 后果 |
|---|---|
| 只用 `visible_enemy_entities()` | 整场 `seen` 最多 4 个，敌方 17 个实体。威胁分布长期是残片 |
| `+= 1` 不看量级 | 一个 E1 和一个 HTNK 都记 1，HTNK 造价是 4.5 倍 |
| 跳过建筑 | 敌方 26 个建筑含 `NALASR`×2 与 `TESLA`×2（苏联特斯拉线圈），正是「打不掉防御塔」的来源，却不计入 |

### 根因 2：`Support` 是一个永远填不满的缺口

`src/rules/unit_role.rs:440`：

```rust
aim[Role::Support.index()] =
    f64::from(ty.passengers > 0) + f64::from(ty.engineer) + f64::from(dps <= 0.0);
```

`FV`（`Passengers=1`）拿到 `Support = 1.0`，是该角色的理论最高值；`MTNK` 是 `0`。
而 `role_score`（`military.rs:450`）取「最擅长补某一个缺口」的最大分：

```rust
let closest = ROLES.iter()
    .map(|role| weights.get(*role) * need[role.index()].min(GAP_CAP * 1_000) as f64)
    .fold(0.0_f64, f64::max);
```

策略几乎从不造运输车，`Support` 缺口因此长期敞开，`FV` 每期都能靠这一项拿分。
**它不是被选为反坦克单位，是被选为「补 Support 缺口」的单位。**

### 根因 3：同分时偏向贵的

`military.rs:471` 的排名是 `(score, -held, cost, id)`，同分选贵。设计意图是
「能造坦克就别塞士兵」，但配合错的威胁分布，会让贵单位在多缺口下反复胜出。

### 根因 4：建筑从不是攻击目标

敌方建筑（含防御塔）既不在威胁分布里，也不在攻击目标候选里。要赢必须拆光建筑。

## 三、执行顺序

每一步都要：先测基线 → 改一处 → 同种子重测 → 写进 `ledger final` 对比 →
不行就回退并记录。**验收一律用 Arena.mmx，24000 ticks，六种子
`0x5EED,19,10,7,42,99`，difficulty easy，brain v2。**

基线（当前 HEAD `1c803947b`）：

| 种子 | spent | lost | destroyed | net | exchange | 结果 |
|---|---|---|---|---|---|---|
| 10 | 34700 | 1300 | 6100 | +4800 | 4.692x | 存活 |
| 19 | 38800 | 3200 | 8100 | +4900 | 2.531x | 存活 |
| 0x5EED | 37600 | 2400 | 6100 | +3700 | 2.541x | 存活 |
| 42 | 38400 | 3000 | 6300 | +3300 | 2.100x | 存活 |
| 7 | 37400 | 2600 | 6600 | +4000 | 2.538x | 存活 |
| 99 | 36400 | 5500 | 8000 | +2500 | 1.454x | 存活 |

### 阶段 0：修侦察兵（前置，必须先做）

第 29 轮实测：整场对局 `Assault` 只打过 1 个目标，`seen` 最多 4 个，而敌方有
26 个建筑和 17 个单位。军队**看不见战场**。

而 `military.rs:563` 的设计约束是对的：

```
// the spec forbids aiming at anything the player could not have clicked,
// so a target is something in line of sight right now.
```

所以「打建筑」「按威胁分布造兵」都建立在一个不成立的前提上。侦察兵不抵达敌方
基地，后面每一步都是在优化一个盲眼的军队。

缺陷形态（两次实测）：同一场对局侦察兵 id 换了 7 个（1154/1155/1225/1229/1262/
1336/2749），位置在 `(95,90) (83,51) (94,106) (83,39) (90,77)` 之间摆动，目标格
`(84,152)` 从不到达。视野本身正常（`visible_13x13` 有 86–169 格）。

**验收**：侦察兵在同一场对局里保持同一个 id，位置单调接近目标，最终有人站到敌方
基地视野内，`seen` 超过 10。

### 阶段 A：修威胁分布（在阶段 0 之后）

**A1 用全量敌方实体。** `obs.enemy_entities()` 不带视野过滤（已在账本里用过，
验证有效）。威胁分布改成读它。

**A2 按造价加权。** `threat[role] += cost / 1000`，至少 1。让一辆 HTNK 值 4 个
E1，而不是持平。

**A3 建筑计入威胁。** 去掉 `Building` 的 `continue`，用 `role_against_armour`
对建筑护甲（`wood/steel/concrete` → `AntiStructure`）计权。这样「敌方有 5 个
防御塔」会真的产生反建筑需求。

**A4 验收。** 期望：`FV` 产量下降、`MTNK` 上升、`destroyed` 的 `exchange` 上升。
若 `FV` 仍占第一，记录并进入 B。

### 阶段 B：修 `Support` 缺口

**B1** `role_score` 不参与计算「永远填不满的缺口」：对 `Support` 和 `Scout`
两个 job-only 角色，当 `need` 超过 `GAP_CAP` 时截断到 `GAP_CAP`，或直接把
`Support` 从「拉生产」用途里排除（它属于能力问题，不是兵力问题）。

**B2 验收。** 期望 `Support` 不再驱动生产。若 `FV` 明显下降即为成功。

### 阶段 C：让军队打建筑

**C1** 攻击目标候选加入敌方建筑。已有 `neutral_structures()` 的先例；
敌方建筑来自 `enemy_entities()` 里的 `Structure`。
**C2** 目标排序：建筑优先于同等价值的单位（建筑是胜利条件）。
**C3** 需要 `AntiStructure` 角色的单位去拆：`role_against_armour` 已能给
`wood/steel/concrete` 定位，用 `role_score` 的同一套权重选拆塔单位。
**C4 验收。** `destroyed` 里必须出现至少一个 `NA*` / `GA*` 建筑；否则本阶段未完成。

### 阶段 D：伤害有效成分记账（度量工具，服务于 C 的验证）

已实现引擎侧（未提交，`src/sim/house_tracking.rs` + `receiver_health.rs`）：
`credits += applied_hp × target.cost / target.strength`，按攻击者类型累计。

剩余：
- **D1** `Observation` 出口：`damage_value(&self) -> &BTreeMap<InternedId, i64>`
- **D2** 策略按类型算「单位金钱产出率」：`damage_value / built_value`
- **D3** `ledger final` 增出 `value_per_type` 段
- **D4** 测试：一个 $2500 单位掉 10% 血 = $250 credit

**D 排在 C 之后**，因为 C 的验收不依赖它（有没有拆到建筑，`destroyed` 已经能答）。
D 的价值是后续精细调优。

### 阶段 E：收尾未决项

- **E1** `autoplay/src/manager/military.rs` 有 105 行未提交改动（侦察兵固定 +
  到达只按距离 + `fog:` 诊断），实测三者**都无效果**。除非某步需要 `fog:` 诊断，
  否则丢弃。**需要用户许可才能 `git checkout -- <file>`**（A2 禁止直接删除文件）。
- **E2** `is_army` 里 `FV`（`passengers=1 < TRANSPORT_SEATS=5`）算进攻单位是否
  合理：复核一次，若应排除则改 `TRANSPORT_SEATS` 语义或加 `passengers > 0` 判据。

## 四、每一步的纪律

- 每步只改一处，改完立刻测六种子，不留「改了也不知道有没有用」的改动。
- 测完把数字写进 `autoplay/docs/round-*.md`，附基线对比表。
- 三张表都记录：`own`、`lost`、`destroyed`，以及 `built` 的类型分布。
- 不通过就回退，并在文档里写明「试过什么、为什么失败」。
- 提交信息用 `git commit -F target/scratch/<file>.txt`，避免引号问题。

## 五、已知的坑（别再踩）

| 坑 | 事实 |
|---|---|
| 改 `march_cell` 会拖累军队 | 它被侦察兵和主力共用。第七轮改它丢了 4 个种子 |
| `map_extent()` 是 fog 网格 | 所有地图都是 256，**不能**判断地图大小 |
| `ground_extent()` 也不能 | 实测 Arena 70×69 / Barrel 66×63，只差 4 格 |
| `spawn_object` 不登记 | 走 `FreshScenario` 路径，不调 `Added_To_Game`，计数为 0 |
| `flush_pending_delete` 不减计数 | 只删实体；真实死亡走 `uninit_with_rules` |
| `short_game` 下「还有东西」= 基地车 | 不是任何单位。满编军队没基地车照样判负 |
| 真正跑的二进制 | `autoplay/src/bin/match.rs`，**不是** `src/bin/autoplay-match.rs` |
| `is_cell_in_playfield` 会放行 | 它承认 zone grid 拒绝的地面，必须配 `can_reach` |
| `vera20k-core` 有 28–30 个既有失败 | 全在 `net::live_commands`，与改动无关，不要当回归 |

## 六、验收的语义（重要）

「打赢」在当前 `short_game` 规则下 = **拆光敌方所有建筑**。不是打光兵。

所以胜利进度可以量化成：`敌方剩余建筑数`。Arena 上敌方建到 26 个，
六种子当前**一个都没拆**（`destroyed` 里没有任何建筑类型）。

阶段 C 完成后，这个数应该下降。

## 七、我需要的授权（仅两条）

1. 丢弃 `military.rs` 那 105 行零效果改动 —— 需要你一句许可。
2. `damage_value` 是 VERA 特有分析计数器（gamemd 没有这个账本），注释里标了
   「VERA-only、不影响玩法」。按契约这类偏差要显式记录，你确认这个做法即可。

其余阶段 A、B、C 在我现有授权内可独立执行。
