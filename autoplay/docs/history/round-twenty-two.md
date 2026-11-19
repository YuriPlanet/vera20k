# 第二十二轮：建筑例外被证伪，胜利判定的疑点

写于 2026-11-05 06:35。

## 本轮改动（已回退）

给 `far_target` 加建筑例外：**远处的敌方建筑必须打，只有远处的单位才越过**。

理由：建筑不会跑，而且 `game_alive` 只要敌方还有单位就算活着，所以敌方生产建筑
是必须够到的目标。

### 结果（净负面）

| 种子 | 改动前 | 改动后 |
|---|---|---|
| Arena 0x5EED | 103 | 78 |
| Arena 19 | 127 / 击杀 11 | 109 / **击杀 27** |
| Arena 99 | 93 | **39**（损失 13→63） |
| Barrel | 0/6 | 0/6 |

击杀确实大幅上升（19 从 11 到 27），但 `own` 总体下降，种子 99 崩了。

**回退。**

## 一个未解的疑点：`enemy=0` 却不判胜

上一轮在关闭 `far_target` 的探针里，Arena seed 19 出现：

```
tick 23400   443   2555   215   59   3    0 0 0
tick 23709   443   2544   215   59   0    0 0 0
```

`enemy=0`（敌方所有实体清空），我方 59，但结果仍是 **timeout**。

### 已查清的链路

1. `enemy` 列 = 各敌方房子的 `own_entities().count()` 之和
2. 胜利 = `enemies.iter().all(|e| is_defeated(sim, e))`
3. `is_defeated` = `HouseState::is_defeated` 标志位
4. 该标志只由 `mplayer_defeated` 置位
5. 触发条件是 `defeat_gate && house_holds_nothing(owner)`
6. `defeat_gate = defeat_pass && game_mode_nonzero() && frame > 0`
7. `house_holds_nothing` 调 `game_alive`，后者数活跃单位

### 链路已查到底层，剩下的是计数器口径

`defeat_pass` 已排除：`master_frame.rs` 传的是 `self.session.tick() > 0`，正常为真。

`game_alive` 读的是 **`active_units()` / `active_infantry()` / `active_aircraft()`**，
这三者是对 `active_unit_types` / `active_infantry_types` / `active_aircraft_types`
**按类型表求和**（`wrapping_total`）。

而 census 的 `enemy` 列用的是 `own_entities().count()` —— **实时遍历实体**。

**两者口径不同：一个是按类型维护的计数表，一个是实时遍历。**

所以 `enemy=0` 只说明实体表空了，而 `active_unit_types` 表可能还有残留计数，
`game_alive` 于是返回 true，`house_holds_nothing` 为 false，永不判负。

**这是一个具体的待修项：`active_*_types` 的减计数与实体销毁不同步。**

这是通向胜利的必要条件——不修它，即使把敌方打空也不会赢。

## 当前状态

47 个测试通过，工作树干净，本轮无功能改动。
