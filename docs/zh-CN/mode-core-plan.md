---
layout: default
title: 模式底座重构约束
lang: zh-CN
---

# 模式底座重构约束

## R04–R07 规则与会话状态

`MatchSpec` 在启动接纳时组合地图构造、规则策略和执行拓扑。`init.rs` 直接消费这份启动配置，把具名策略交给 `ScenarioDescriptor`。若 `ScenarioSession::from_descriptor` 仍将 `desc.rule_policy` 转为 `ScenarioGameMode` 并丢失独立策略，则该实现仍处于过渡态。Phase 1 收口与首个自定义规则消费者接入均以 `ScenarioSession` 直接持有接纳的权威 `RulePolicy` 为前提，仿真读取该值。原版模式用于选择默认预设和接纳旧数据。

木箱、`AllowWin` 与胜负、Paranoid、AI 建家、`DoubleOwned=`、小队及外交各有独立策略。构造期和会话期通过类型传递已选规则，移除枚举经布尔值转回枚举的转换。注册表与覆盖物加载器接收具体策略类型，调用方测试显式选择枚举值。`mod_additions.rs` 等行为入口消费具名策略，模式类分支归入原版预设。

`RulePolicy` 的纯二元开关统一存入紧凑位标志，例如 `u32` 位掩码。只有需要表达至少三种互斥取值的规则才保留 `#[repr(u8)]` 子枚举。补齐 R05–R06 全部规则后，仍须满足 `size_of::<RulePolicy>() <= 16`。全部字段保持私有，读取统一使用 `const fn` getter，构造和覆写统一使用 `const fn with_*`。

`ScenarioSession` 中的四个本地视角字段归并到实际持有数据的 `LocalViewIdentity` 子结构体。所有调用经由状态所有者读取。迁移验收检查共享哈希排除本地视角，守卫拒绝旁路读写，原版序列化字段由现有编解码边界保留。守卫白名单不得扩大，`architecture_guards.rs` 对 `sim/scenario_session.rs` 的 `current_house` 计数上限回到迁移前的 5。

离线遭遇战 mode 5 使用多人规则和本地执行，是必须保留的反例。单人战役 mode 0 也在本地执行，但使用战役规则。R04–R07 的相关行为链必须用这两个输入对照。回放保留记录的执行语义，地图构造仍使用已接纳的场景类别。

会话首次直接持有权威策略的同一切片必须明确存档布局、回放与共享哈希的兼容契约。验收覆盖历史原版样本、加载后的共享哈希和继续执行。单人战役、遭遇战（含 Teams 共享）与原版合作保留各自的存档及回放布局。独立规则值若进入新的持久化形态，必须在存档、回放和哈希协议中一并定义其保存与恢复方式。

现行存档和回放保留原版预设对局的字段、顺序和共享哈希输入，解码时由旧模式字段恢复原版策略。本地视角仍排除在共享哈希之外。现行编码无法表示自定义策略，存档和录制须明确报错。
[快照布局](../../src/sim/snapshot.rs) · [回放启动](../../src/sim/replay.rs)

结算、AI 和工厂通过 `HouseClass::IsControlledByHuman` 对应的规则读取 House 自身的 `Human` 和 `PlayerControl`。锁步接纳固定只采用同步的 `Human` 字段。发令授权由 Phase 2 的来源授权表裁决。单人战役的本地迷雾用于飞机准入和发现回调，要求本地执行且采用战役视角。离线遭遇战保留原有视野路径。休眠还须满足触发器、自动修理和本地发现的唤醒条件。

R07 守卫禁止 `rules/`、`map/` 和已迁移的玩法链读取原始模式，并锁住其余 `sim/` 原始读取总数。迁移调用时同步收紧总额度。原版模式仍用于旧数据解码、启动预设和兼容路径。
[架构守卫](../../src/architecture_guards.rs)

## Phase 2 发令来源与阵营权限

权限按 `Source × House` 查找。`ControlGrantTable` 持有规范排序和去重后的权威授权，相同授权合并，同一键的冲突授权拒绝接纳。`ControlScopeTable` 是 O(1) 紧凑平面派生表。每次使用缓存都校验实时 `house_count`，阵营数变化时按新维度重建。空授权保留 `has_any_grants` 快路径，沿用原版判定且无需分配或读取派生表。

权限结果为 `OwnFull`、`FullControl`、`UnitsOnly`、`None`。主体解析与能力映射均使用穷尽 `match`，禁止用 `_ =>` 兜底吞并新增命令。

- `OwnFull` 专属包括 `ExitMatch`、`Destruct`、`Ally`、`RemovePlayer`、`LiveGameSpeed`、`SetGameSpeed`、`OpenOptions`、`SetLatencyFudge`、`Cheer`。
- `FullControl`（含 `OwnFull`）包括 `SellBuilding`、`ToggleRepair`、`SetBuildingOnLine`、`TogglePrimaryFactory`、`SellWallAtCell`、`UndeployBuilding` 及目标阵营的生产、放置与超武。卖墙按 `cell.wall_owner` 查表。实体绑定的出售和维修按 `entity.owner()` 结算。原版侧栏生产队列默认仍归来源的主 `House`。
- `UnitsOnly` 允许 `Move`、`MoveToObject`、`Stop`、`Attack`、`AttackMove`、`ForceAttackCell`、`Guard`、`Scatter`、`DeployMcv`、`ToggleInfantryDeploy`、`UnloadPassengers`、`EjectBunker`、`CaptureBuilding`、`PlantC4`、`SetRally`、`ArchiveObject`、`MegaMission`、`PlanningRecord`、`DetonateRecord` 和 `AresTrenchRedirectRecord`。引爆按 `entity.bomb.house()` 查表。

从发令、锁步验证到执行与回放复原保留同一来源身份。锁步直接把原生 `CommandRecord.house_id` 传入 `apply_command_with_overlays` 和鉴权入口，离线或无共享场景回退到主 `House` 索引。`CommandEnvelope` 保留现有序列化布局。

单人战役（包括 `PlayerControl=yes`）、遭遇战（含 Teams 共享）与原版合作保留原有规则。`HouseState` 继续持有供 AI、蓝图和原版玩法使用的人类属性。空授权保持快照布局、共享哈希和旧回放编码。非空授权纳入共享哈希，在版本化持久化落地前明确拒绝写入旧存档和回放。

按以下三笔独立 PR 依次落地。

1. **2A 数据与缓存。** 将权威授权和派生表挂入 `ScenarioSession`，测试排序去重、对局中新增阵营后的重建、共享哈希与旧存档和回放边界。
2. **2B 执行鉴权。** 显式传递来源。统一 `resolve_command_subject`、`CommandCapability` 与执行门控，迁移 `world_commands.rs` 外闸和内部全部七处 `entity_owned_by_id` 分支。
3. **2C 输入与投影。** 将 `context_order.rs`、`dispatch.rs`、`move_cell_input.rs`、`selection_navigation.rs`、`house_controls_selection` 和 `NativePlayerIdentity` 的消费者（含 `render/radar`）收敛到同一只读投影。用 `architecture_guards.rs` 棘轮封住裸权限判断，原生可见性继续按所代表视角裁决。

端到端验收让两个来源共享主 `House`，对第一个第三方目标分别授 `FullControl` 与 `UnitsOnly`，对第二个目标只授权一个来源，再于对局中新增阵营。通过真实命令入口与输入投影分别验证允许、拒绝和来源保留，同时核对三种原版形态、快照布局、旧回放编码和 20k `sim-bench` 共享哈希。

## R11–R12 回放依赖

原版启动与合作回放可各自沿现有入口推进。携带权限表的剧本蓝图须在 Phase 2 确立授权主体、索引与复原契约后接入，并用分别授权的共享 `House` 对局验证初始哈希和回放执行结果。

## Phase 4 房间信封

LAN 的已生效速度 `HostRoom.current_speed` 与设定速度 `options.game_speed` 各自传输和应用。房间信封迁移必须以两值不同的对照验证接收、显示和开局复制，并核对混版本与回滚路径。
[现行联机协议](../Networking.md)

## PR 提交

引入本规划约束的 PR 及其后续提交均须执行这些约束。所有提交的 PR 都使用英文撰写标题和正文。跑分和验证结果只记录在对应的 PR 正文中。

`docs/Networking.md` 只记录已经生效的协议。Phase 2 的跨阵营权限表和 Phase 4 的房间信封要求保留在本规划中，各自随协议与实现实际合入的同一笔 PR 同步更新联机协议文档。
