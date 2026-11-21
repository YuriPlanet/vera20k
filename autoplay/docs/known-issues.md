# 实机问题记录

来源：`G:\Projects\vera20k-dist-autoplay-v4\vera20k.exe`（SHA-256 `bcd70ac…`，2026-11-21 22:06 拷贝）的一局实机。日志取自 `logs/ra2.log.1` 末段 `2026-11-21T14:07Z` 起共 6140 行。

每条给出：现象（用户原话要点）→ 证据（日志/源码）→ 初步判断 → 未决问题。
标注 `[源码证实]` 的已读代码确认，`[实测]` 的来自本局日志，`[推论]` 的尚未验证。

---

## 1. 基地车反复部署/收回，最后一起发呆 `[实测]`

**现象**（用户）：找不到合适的地方，瞎部署、收回，还会把开始的基地收回，反复不行之后一起发呆。

**证据**

- 同一段日志里 `maker=` 出现过 6 个不同 entity id：`3868 / 12167 / 20782 / 25586 / 17576 / 7577`，其中多数的 cell 是 `(0, 0)`（不在图上）。
  ```
  maker=Some((12167, (0, 0))) far
  maker=Some((25586, (0, 0))) far
  maker=Some((20782, (110, 93))) far
  ```
- `admits=false` 45 次 / `admits=true` 7 次 —— 86% 的采样里部署位不满足。
- `yard=true maker=None` 9 次：有基地但没有 maker，说明 maker 收起之后没有补回来。

**源码**

- `src/sim/strategy/assist/expand.rs:102` `ty.deploys_into`、`:171`、`:218`、`:626`、`:923` —— maker 身份完全来自类型的 `DeploysInto=`。
- `expand.rs:203` 与 `expand.rs:287` 两处 `maker {} first seen at {:?}, recalling it`。
- `src/sim/strategy/assist/orders.rs` `pack_captured_yard` 发 `UndeployBuilding`。

**初步判断** `[推论]`：代码**确实**按「第一次被看到的格子」区分自家 maker 与缴获 maker（`expand.rs:270-290` 的注释写了这个判据），所以不是没有判据。真正可疑的是日志里这些候选的**当前 cell 是 `(0,0)`**：

```
maker 3868 first seen at (126, 21), recalling it
maker=Some((12167, (0, 0))) far
maker=Some((25586, (0, 0))) far
```

`(0,0)` 不是地图上的格子。`cell_distance((0,0), home)` 必然大于任何阈值，于是这些 maker 一律被判成 `far`，`retrieve_makers` 把它们一路召回；而它们的 cell 永远解析不出真值，「recall → 走 → 还是 (0,0) → 再 recall」就成了死循环。同一段里 6 个不同 id 轮流出现，说明候选集本身在抖 —— `yard_commit` 按 entity id 记承诺，键一直在换，承诺等于没记。

**未决**（按顺序查）：

1. `Observation::cell` 对「在运输舱里 / 尚未 unlimbo / 正在部署」的单位返回什么，`(0,0)` 是哪一种。
2. maker 候选要不要显式排除 `cell` 未解析的单位，而不是让它进 `retrieve_makers`。
3. `census.base_cell` 在两座 yard 时是「看得最晚的那座」（`orders.rs:553-554` 的注释自己承认了），`home_cells` 会不会因此漏掉主基地周围。

---

## 2. 不屯兵，造出来就送前线 `[实测]`

**现象**（用户）：非必要、没有大把握胜利就不出击；在家养精蓄锐，屯兵再攻击。

**证据**

- 日志 `fighters=79..80`，其中 `idle=35..61` —— 七成部队在家闲着；同时 `push=push`。
- `unclaimed=51..62`（80 个单位里只有二三十个有活）。
- `threat=[50,21,10,0,0,0,0]`，即在被攻击的威胁下仍然出击。
- `roster=[E1:43,GGI:10,JUMPJET:9]`：43 个步枪兵。

**源码** `[源码证实]`

- `src/sim/strategy/assist/squads.rs:72` `pub(super) const ARMY_TO_ATTACK: usize = 10;`
- `src/sim/strategy/assist/advance.rs:312` `let gate = if fighters.len() < super::squads::ARMY_TO_ATTACK && !mop_up(...) { "too-small" }`
- `advance.rs:329` `"cannot-replace"`、`:336` `"base-defended"` 两个门都还要再叠加 `raided`。

**初步判断** `[推论]`：数量门只有一个绝对值 10，没有「我方 vs 敌方可见兵力」的对比门；造兵是逐个单位完成的，第 11 个单位一出来就满足了门，于是每次都是「刚够 10 个就推」。

**未决**：屯兵的目标数量按什么算（敌方可见兵力、经济、还是固定值）；要不要区分「防御姿态」与「进攻姿态」两套门限。

---

## 3. 攻击时散开，快的等慢的 `[实测]`

**现象**（用户）：应当集群化聚拢，现在跑得快的不等跑得慢的。

**证据**

- `squads=[2, 8, 27]`、`squads=[2, 8, 29]`、`squads=[2, 8, 65]` —— 三队规模差异很大。
- 日志里 `idle` 高企同时 `push=push`。

**源码** `[源码证实]`

- `src/sim/strategy/assist/army.rs:35` `let anchor = members.first().map_or((0, 0), |unit| unit.cell);` —— anchor 是**第一个成员**的实时格子，不是集合中心，也不是最慢者。
- `advance.rs:466` `let anchor = army.anchor;`、`:468` `nearest_target(&targets, anchor)`、`:534` 目标在 anchor 之后取。
- `squads.rs:300` `squared_distance(unit.cell, enemy_anchor(census))`、`squads.rs:348 fn enemy_anchor`。

**初步判断** `[推论]`：每个单位各自收到到同一 cell 的 `Goal::Move`，没有「集结 → 等齐 → 一起推进」三段；anchor 取第一个成员，导致谁在前面就以谁为准，后面的单位永远在追一个动着的点。

**未决**：集合点放哪（anchor 还是集合中心），等齐的判据（最慢单位到位 / 全员进入半径）。

---

## 4. 载具回维修厂只修一辆 `[源码证实]`

**现象**（用户）：部分操作始终不正常，比如把载具拉回维修厂修复，修一辆另一个不修了。

**源码**

- `src/sim/strategy/assist/census.rs:509` `pub(super) repair_cell: Option<(u16, u16)>,`
- `census.rs:514` `pub(super) repair_building: Option<u64>,`
- `census.rs:731-733` `if traits.repairs_units { self.repair_cell.get_or_insert(entity.cell); self.repair_building.get_or_insert(entity.entity_id); }`
- `src/sim/strategy/assist/orders.rs:1879` `let depot = brain.census.repair_building;`、`:1884` `.repair_cell`

**初步判断** `[源码证实]`：这两个都是**单值** `Option`，用的是 `get_or_insert`，所以一个 house 里**第一座**维修厂永远是唯一选择，第二座从不被使用；也没有任何「每座厂有几个车位」的排队模型。多辆车涌向同一个 cell，`HOLD_FRAMES = 15`（`orders.rs:1983`）把单位按住，看起来就是「修了一辆，另一辆不动」。

**未决**：容量从哪来（census 有没有 pad 数），还是先把候选改成列表轮转。

---

## 5. 对着防御塔冲，不造对策也不绕路 `[实测]`

**现象**（用户）：发动攻击仍然有概率直接对着防御塔和敌人冲过去被扫荡死；要么造对策单位解决防御，要么绕路，而不是反复来回乱跑。

**源码** `[源码证实]`

- `advance.rs:484` `blocking_structure(&structures, anchor, nearest)` + `covering_defense(&structures, blocker)` —— 先打掩护目标的结构，这一层已经有了。
- `advance.rs:468` `nearest_target(&targets, anchor)` —— 取**最近**，不看血量、不看防御类型。
- `src/sim/strategy/assist/composition.rs` 里没有任何读敌方防御构成的函数（`fill_lines` / `product_for` / `fighter` 都不看敌人）。
- `Goal::Move` 只带目标 cell，没有路径字段，所以「绕路」在指挥层无法表达。

**初步判断** `[推论]`：目标选择只按距离，不按「打不打得过」；`composition.rs` 不读敌方防御，所以永远不会因为对面是光棱塔而停产坦克。绕路没有实现路径，只能直冲。

**未决**：对策单位要不要进 `composition.rs`（需要新增「敌方防御构成 → 需要的兵种」这一步）；绕路是走 `avoidance.rs` 还是给 Move 加路径。

---

## 附：本局日志关键数字

| 项 | 值 |
|---|---|
| 日志段 | `2026-11-21T14:07Z` 起 6140 行 |
| `maker=` 出现过的 entity id | 6 个（3868 / 12167 / 20782 / 25586 / 17576 / 7577） |
| `admits` | false 45 / true 7 |
| push 门控采样序列 | none 1 → push 24 → base-defended 4 → cannot-replace 1 → base-defended 2 → push 20 |
| `claimed-and-stuck` | 99 行，intent 全是 `Some((Avoid|Guard|Kite|Enter|Capture, None))` |
| 军队 | fighters 79–80，idle 35–61，unclaimed 51–62 |

门控在 `base-defended` / `cannot-replace` / `push` 之间仍来回摆 —— `PUSH_REST_FRAMES = 750` 的粘滞没有压住这条链，这条要单独看。
