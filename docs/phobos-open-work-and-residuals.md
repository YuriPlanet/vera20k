# Phobos 未完成项与残渣总表（实机测试前必读）

基准：`phobos-local`（2026-11-16，wt-keys 包与 ToolTips/排序/颜色包已合并）。
扫描器：`G:\20k\scratch\phobos_chain_analysis_v3.py`（**`GAP = 0`**，suspects = 0，
compile-disabled = 4）。本表是实机测试与后续修复的唯一入口；每关一个键或修一条
残渣，都在此表划掉并更新扫描器。

## 1. 未移植键（已清零，历史分组保留）——按机制分组

出处列为 Phobos 源码，消费者列为真正读取该值的位置，便于下一步定位实现点。
“默认”均来自构造函数或读取处的 default 参数。

### 1.0 本次进展（2026-11-16，worktree `F:\20k-scratch\wt-keys`，`be130116a`）

已读取全部 25 个非 ToolTips 键（含 `Default`、
`from_ini` 四个节与回归测试），并接线了机制已存在的 21 键：

- `AllowDeployControlledMCV`：`sim/deploy.rs` 部署门 +
  `world_spawn.rs::construction_yard_redeploy_core_gate`（repack 路径）。
- `AllowChatBoxInSinglePlayer`、`AllowBeaconHotKeyInSinglePlayer`：
  `app/input/chat.rs`、`app/input/beacon.rs` 的打开门，发送/网络门保留。
- `AutoRemoveEarliestBeacon`：`BeaconManager` 新增 `placed_order`，
  满槽时先选中并删除最旧信标再放置（`0x430C64/0x43131B/0x4AC9B2`）。
- `SetTabBySelectingFactory`：`apply_selection_mutation` 按
  `ObjectClass_Select` 语义用新选中对象切页。
- `ReadyToNextMission.MovingCheck`、`AutoTarget.InsignificantWhenMindControlled`、
  `FixRepairStepCost`：读取标志并改变对应判定（`0x7442D6/0x521BA7`、
  `0x006F8364`、`0x7120DD`）。
- `RadialIndicatorVisibility`：`app/presentation/radial_indicators.rs` 按
  `0x456776` 的 owner/observer/allied/enemy 分支过滤他人建筑的指示环。
- `SortCameoByName`：`CameoKey` 增加内部 `Name` 与开关，`inserts_before` 按
  `0x6A84DB/0x6A86ED` 先 `strcmp` 内部名、相等再 `wcscmp` UIName。
- `EnablePowerSurplus` / `PowerSurplus.ScaleToDrainAmount`：`PhobosGlobals`
  增加原生 `[AI] PowerSurplus`（默认 50）；`ai_base_building.rs` 的
  `power_plant_to_splice` 由“盈余 < 0”改为“盈余 < 目标值”，并支持按
  `PowerDrain / ScaleToDrainAmount` 缩放（`Ext/House/Body.cpp:655-674`）。
- `ColorAddUse8BitRGB`：`ColorAddTable::phobos_word` 在开关打开时把
  `[ColorAdd]` 的三通道按 8-bit 输入经 `Drawing::RGB_To_Int` 收窄为
  RGB565（`Ext/Rules/Body.cpp:260`）；`app/presentation/lighting.rs` 的
  空袭激光目标色调用点传入开关。
- `UseRetintFix`：`app/presentation/lighting.rs` 在正常调色板 retint 后，
  按开关把全部仍激活的 building/radiation 光源区域重新标脏并从当前
  `PointLight` 重算（等价 `Ext/TAction/Hooks.cpp:68-107` 的
  deactivate/reactivate）。
- `VoxelLightSource`：`tools/voxel_oracle/lighting.py` 通过 Unicorn 执行
  `0x754C00` 捕获 `Matrix3D::VoxelDefaultMatrix`（
  `voxel_default_matrix_bits`，并验证它把零售源映射到原始
  `Game::VoxelLightSource`）；`VoxelLight::for_rules` 对
  `[AudioVisual] VoxelLightSource=` 归一化后乘该矩阵（
  `Ext/Rules/Body.cpp:1189-1207`），未设键保持启动光照。
- `AircraftLevelLightMultiplier` / `JumpjetLevelLightMultiplier`：
  `app/presentation/lighting.rs::phobos_air_unit_light` 在体素单位/飞机与 SHP
  飞机的绘制处重建 blit 强度：`int(level * (height/104 - bridge*4) * mult) +
  Cell.Intensity_Normal + bridge*4*level`（`Ext/Techno/Hooks.Tint.cpp:27-45,
  320-379`）；等级取场景光照 profile（风暴/支配者/核闪）。默认 1.0/0.0。
- `VeinholeWarhead`：`IsVeins=` 动画在成熟杂草格（Overlay 126、Data>=0x30、
  平地、低空占位）上每 2 帧用 `[General] VeinDamage=` + `[CombatDamage]
  VeinholeWarhead=` 伤害格内低空 techno；前置失败置 `UnableToContinue`
  （`Ext/Anim/Body.cpp:164-195`，钩子 `0x4243BC`）。`VeinDamage`、`ImmuneToVeins`
  两个原生键同批读取。
- `ColorAddUse8BitRGB`（其余三色）：`TechnoExt::GetTintColor` 的类词替换钩子已接
  ——建筑（普通铁幕）、单位体素（铁幕/狂暴）、步兵 SHP（狂暴）都改用
  `GetColorFromColorAdd` 的单臂词（开关开启时按 8 位 RGB 收窄），与空袭激光色
  共用 `ColorAddTable::phobos_word`；非 phobos 构建仍用原生 3 臂词。
- `DisplayIncome`（含 `DisplayIncome.AllowAI`）：建筑 `AccumulatedIncome`
  在研磨退款（`DoGrindingExtras`）、精炼厂/奴隶卸矿入账（`pay_refinery_owner`
  余额差）、间谍渗透（`infiltrate_building` 受害者余额差）处累积；
  `BuildingClass_AI` 后按 `frame % delay == 0` 在建筑 `GetRenderCoords` 发出
  `FlyingStrings` 金钱标签（`Ext/Building/Body.cpp:12-36`）。类型级
  `DisplayIncome`/`.Delay` 现已读取（0 强制为 1）。
- `ExtendedPlayerRepair`：`HouseState` 增加 `PlayerAutoRepair`；侧边栏修理
  按钮在开关打开且当前房为人类控制时改发 `TogglePlayerAutoRepair`
  （`Ext/Event/Body.cpp:116-128`、`Ext/House/Hooks.cpp:776-801`），
  `update_repair_and_power` 的玩家分支按该房标志为全部建筑启动/停止修理
  （`Ext/House/Hooks.cpp:803-825`）。
- `JumpjetClimbPredictHeight` / `JumpjetClimbWithoutCutOut`：
  `sim/movement/jumpjet_flight.rs` 的参考高度改为
  `JumpjetLocomotionPredictHeight` 五格预测（前沿、二分之一侧格、`Skip` 置位），
  并按 `0x54D4C0` 跳过“离目的地过低即停”的切出；`CruiseHost` 读取
  `[General]` 两键并提供建筑/85/桥面的占位高度与 Cell Level。
- `ChronoSparkleDisplayDelay` / `ChronoSparkleBuildingDisplayPositions`：
  `sim/temporal.rs` 的 Foot 与 Building 闪烁改读延迟；建筑按
  `Building`/`Occupants`/`OccupantSlots` 位选择自身、在场乘员或端口，
  建筑自身闪烁落在 `GetCenterCoords`（`Ext/Building/Hooks.cpp:39-83`）。
- ToolTips（uimd.ini）：新增 `rules/phobos_tooltips_ui.rs`，在进程启动时
  （`ProcessAssets`）从同一份 UIMD.INI 读取 `[ToolTips]` 与 `[LoadingScreen]
  DisableEmptySpawnPositions`。`ExtendedToolTips` 让 cameo/超武提示改出 Phobos
  文本：`GetBuildTime`（`max(54, Time_To_Build)` 对本地玩家完整公式）按
  `TickTimeToSeconds`（15 fps 基准 / `RealTimeTimers` 实测帧率或 `60/GameSpeed`）
  换算，`CostLabel`/`TimeLabel`/`SWShotsFormat` 经 CSF；`UIDescription` 只在
  扩展文本下追加。`AnchoredToolTips` 把 cameo 提示固定到侧栏/左超武列，
  `MaxWidth` 改变 cameo 换行宽度，`DisableEmptySpawnPositions` 关闭载入界面
  起点黑点（`Misc/PhobosToolTip.cpp`、`Misc/Hooks.UI.cpp:29-37`）。细节与残渣：
  `docs/phobos-tooltips-ui.md`。

仍未实现的键为 0；31 个键全部已实现（2026-11-16），残渣见 §3。

### 1.1 ToolTips / uimd.ini（7）——已完成（2026-11-16）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `ExtendedToolTips` | `Phobos.INI.cpp:148` | bool, no（`Phobos.h:13`） | `Misc/PhobosToolTip.cpp` |
| `AnchoredToolTips` | `Phobos.INI.cpp:151` | bool, no（`Phobos.h:41`） | `Misc/PhobosToolTip.cpp` |
| `MaxWidth` | `Phobos.INI.cpp:154` | int, 0（`MaxToolTipWidth`） | `Misc/PhobosToolTip.cpp` |
| `CostLabel` | `Phobos.INI.cpp:156` | string, `$`（`LoadStringOrDefault`） | `Misc/PhobosToolTip.cpp`、`Misc/FlyingStrings.cpp` |
| `TimeLabel` | `Phobos.INI.cpp:165` | string, `⌚` | `Misc/PhobosToolTip.cpp` |
| `SWShotsFormat` | `Phobos.INI.cpp:168` | string, `Shots: %d` | `Misc/PhobosToolTip.cpp` |
| `DisableEmptySpawnPositions` | `Phobos.INI.cpp:142`（`[LoadingScreen]`） | bool, no | `Misc/Hooks.UI.cpp` |

注意：这 7 个读的是 **uimd.ini**（`ini_uimd.LoadFromFile(UIMD_INI)`），不是
rules.ini，也不是按本仓库 rules 分层读取；VERA 需要先找到/建立 uimd 读取入口。

### 1.2 侧边栏与单人局开关（6）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `SortCameoByName` | `Ext/Rules/Body.cpp:501` | bool, no（`Body.h:411,916`） | `Misc/Hooks.UI.cpp` |
| `SetTabBySelectingFactory` | `Ext/Rules/Body.cpp:307` | bool, no（`:227,739`） | `Ext/Techno/Hooks.Misc.cpp` |
| `AllowChatBoxInSinglePlayer` | `Ext/Rules/Body.cpp:619` | bool, no（`:517,1014`） | `Misc/Hooks.MessageList.cpp` |
| `AllowBeaconHotKeyInSinglePlayer` | `Ext/Rules/Body.cpp:625` | bool, no（`:523,1022`） | `Ext/Beacon/Hooks.cpp` |
| `AutoRemoveEarliestBeacon` | `Ext/Rules/Body.cpp:624` | bool, no（`:522,1020`） | `Ext/House/Hooks.cpp` |
| `AllowDeployControlledMCV` | `Ext/Rules/Body.cpp:246` | bool, no（`:163,690`） | `Ext/Unit/Hooks.DeploysInto.cpp`、`Ext/Building/Hooks.Selling.cpp`、`Ext/TAction/Body.cpp` |

### 1.3 目标 / 任务推进（2）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `AutoTarget.InsignificantWhenMindControlled` | `Ext/Rules/Body.cpp:658` | bool, **yes**（`:551,1052`） | `Ext/Techno/Hooks.Targeting.cpp` |
| `ReadyToNextMission.MovingCheck` | `Ext/Rules/Body.cpp:646` | bool, no（`:538,1040`） | `Misc/Hooks.BugFixes.cpp`（原生 `0x7442D6` Unit / `0x521BA7` Infantry） |

### 1.4 修复 / 经济（5）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `FixRepairStepCost` | `Ext/Rules/Body.cpp:401` | bool, no（`:304,818`） | `Misc/Hooks.BugFixes.cpp` |
| `ExtendedPlayerRepair` | `Ext/Rules/Body.cpp:549` | bool, no（`:426,965`） | `Ext/Event/Body.cpp`、`Ext/House/Hooks.cpp` |
| `DisplayIncome.AllowAI` | `Ext/Rules/Body.cpp:299` | bool, **yes**（`:155,727`） | `Ext/Building/Body.cpp` |
| `EnablePowerSurplus` | `Ext/Rules/Body.cpp:243` | bool, no（`:150,687`） | `Ext/House/Body.cpp` |
| `PowerSurplus.ScaleToDrainAmount` | `Ext/Rules/Body.cpp:244` | int, 0（`:151,688`） | `Ext/House/Body.cpp` |

### 1.5 光照 / 视觉（8）——已完成（2026-11-16）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `AircraftLevelLightMultiplier` | `Ext/Rules/Body.cpp:349` | double, 1.0（`:257,778`） | `Ext/Techno/Hooks.Tint.cpp` |
| `JumpjetLevelLightMultiplier` | `Ext/Rules/Body.cpp:350` | double, 0.0（`:258,779`） | `Ext/Techno/Hooks.Tint.cpp` |
| `RadialIndicatorVisibility` | `Ext/Rules/Body.cpp:172` | AffectedHouse, Allies（`:207,736`） | `Misc/Hooks.UI.cpp` |
| `ChronoSparkleDisplayDelay` | `Ext/Rules/Body.cpp:118` | int, 24（`:37,577`） | `Ext/Techno/Hooks.Misc.cpp`、`Ext/Building/Hooks.cpp`、`Ext/Aircraft/Hooks.cpp` |
| `ChronoSparkleBuildingDisplayPositions` | `Ext/Rules/Body.cpp:119` | enum, OccupantSlots（`:38,578`） | `Ext/Building/Hooks.cpp` |
| `ColorAddUse8BitRGB` | `Ext/Rules/Body.cpp:260` | bool, no（`:178,703`） | `Utilities/GeneralUtils.cpp`（`GetColorFromColorAdd`；VERA 已有 `ColorAddTable::phobos_word`，其注释已记该键为残渣） |
| `UseRetintFix` | `Ext/Rules/Body.cpp:409` | bool, **yes**（`:311,823`） | `Ext/TAction/Hooks.cpp`（retint 后重新激活 LightSource） |
| `VoxelLightSource` | `Ext/Rules/Body.cpp:352` | `Nullable<Vector3D<float>>`，未设（`:271,780`） | `ReplaceVoxelLightSources`（`Body.cpp:1189-1207`）——**已完成**（矩阵来自 voxel_oracle） |

### 1.6 跳跃（2）与战头（1）——已完成（2026-11-16）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `JumpjetClimbPredictHeight` | `Ext/Rules/Body.cpp:336` | bool, no（`:242,766`） | `Ext/Unit/Hooks.Jumpjet.cpp` |
| `JumpjetClimbWithoutCutOut` | `Ext/Rules/Body.cpp:337` | bool, no（`:243,767`） | `Ext/Unit/Hooks.Jumpjet.cpp` |
| `VeinholeWarhead` | `Ext/Rules/Body.cpp:136` | `Nullable<WarheadTypeClass*>`, 未设（`:58,597`） | `Ext/Anim/Body.cpp`（`AnimExt::VeinAttackAI`） |

### 1.7 仍未实现的键（2026-11-16）

无。§1.1–§1.6 列出的 31 个键全部已实现；剩余残渣见 §3。

## 2. 明确排除（不移植，扫描器已列 disabled）

| 键 | 原因 |
|---|---|
| `InflictLocomotor` / `RemoveInflictedLocomotor` | `TestLocomotionClass` 开发者测试钩（`Ext/WarheadType/Detonate.cpp:251-254`），Phobos 构建内不生效 |
| `Trajectory.Sample.TargetSnapDistance` | Phobos 轨迹 Sample 模板编译关闭（`Ext/Bullet/Trajectories/PhobosTrajectory.h:5-11`） |
| `VoxelShadowLightSource` | read（`Ext/Rules/Body.cpp:353`）、Process（`:947`）、apply（`:1200-1207`）全被注释，作者自注 “doesn't really impact anything” |

## 3. 已记录残渣（按链，含触发/影响/频率/风险）

### 本会话新增链
1. **Warhead.PreventScatter**（`8bacaf9bd`）：无残渣；gamemd 对未知 warhead 名
   分配空类型，VERA 的 `warhead_prevents_scatter` 同样回落到
   `[CombatDamage] Warhead.PreventScatter`（回归测试
   `world_receiver::prevent_scatter_tests`）。
2. **AIChronoSphereSW/AIChronoWarpSW**（`6fb756008`）：
   - `ValueableIdx` 名字→索引的**跨 INI 层**差异已解决：
     `PhobosGlobals::resolve_ai_chrono_super_names` 按
     `Utilities/TemplateDef.h:1815-1827` 逐 pass 重放（空值/坏名字保留上个索引，
     `none` 存 -1，命中 `[SuperWeaponTypes]` 才换名），数据来自
     `IniSection::projected_values` 的逐层历史（回归测试
     `phobos_globals_tests::a_later_pass_with_an_unknown_ai_chrono_name_keeps_the_resolved_index`）。
     残余（2026-11-19 复核，按接受差异保留）：`FindIndex` 在 native 里对着
     **该 pass 开始时**的 `SuperWeaponTypeClass` 数组——Phobos 的读取挂在
     `RulesClass` Process 的入口块（`0x668BF0` 起，只读 `[General]` 标量），
     此时本层与之后的 `[SuperWeaponTypes]` 都还没处理；VERA 这里用所有层的
     `[SuperWeaponTypes]` 并集。触发：较早一层给这两个键写了**由较晚一层才引入**
     的类型名，native 的 `FindIndex` 返回 -1 因而保留旧索引，VERA 却能解析到该
     名字，两条键最终指向不同超武。频率：零售 `rulesmd.ini` 两条键都未设，零触发；
     只有自造分层坏数据会碰到。未修原因：精确重放需要「每个 INI pass 上该段键的
     存在时刻」，而 typed reader 的投影只保留**值**的逐 pass 历史
     （`IniSection::projected_values`，按 pass 盖写 `dword+ptr`），不保留键首次
     出现的 pass；`ProcessedRulesLayers`/`RuleSet::from_ini` 只把合并后的
     `IniFile` 交给 rules 处理器，没有 pass 序列可穿过去。补它要给 `IniSection`
     加全局 pass 戳并改 `from_ini` 的调用面与存档格式，收益仅限坏数据，故不改码。
   - 动作 56/57 未选中 super 时不发火（与 stock 一致）。
   - `Fire_SW` 用 `Type=` 当 Supers 索引：retail 列表恰好同序；自定义乱序时以
     “所选 Super 的 Type= 值”发射（oracle golden 即此行为）。
3. **Berserk 组**（`7c1d5f362`/`8f324f49e`）：无新增残渣；`QueueMission(m,false)`
   当帧不 commence，与 native 相同。
4. **JumpjetControls 构造**（`fdec98b6a`）：`[JumpjetControls]` 先于类型构造的
   读取顺序是 Phobos 特性成立的前提（无独立 capture）；段被解析两次（纯函数，
   无行为差异）。retail 段值=旧字面量，零售行为不变。
5. **体素光照**（`413e3d0c6`）：`UseFixedVoxelLighting` 只影响**图集烘焙**；改
   rules 后需重建图集（对应 native `DestroyVoxelCaches`）。`VoxelLightSource`
   同样在烘焙时应用（`Matrix3D::VoxelDefaultMatrix` 已由 voxel_oracle 捕获）。
6. **MindControl.Permanent.ReplaceSilent**（`6a261e2cb`）：无残渣；只作用于
   Ares 的 ApplyPermaMC 调用点，Dominator 原生路径不变。
7. **PossibleCellSpreadDetonate 的 `IvanBomb.Detonate` 成员**（本链）：
   `crit::detonate` 的门按 `Ext/WarheadType/Body.cpp` 的完整成员表（16 项）判定，
   原先缺 `IvanBomb.Detonate`，因此只设该键的 warhead 少掉每次引爆的两次
   buffer 抽样，整条 scenario RNG 流错位。现在解析该键
   （`rules/warhead_type.rs` 的 `phobos_ivan_bomb_detonate`）并计入同一 owner 的
   门。该键的**效果**已在本链后续实现：`sim::bomb::Simulation::phobos_ivan_bomb_detonate`
   在 `DetonateOnOneUnit` 的 Taunt 臂之后（`Detonate.cpp:239-240`）把命中目标
   （以及按 `PenetratesTransport`/`PenetratesGarrison`/`AffectsParasite` 选中的
   乘客、驻军、寄生物）身上炸弹的引信切到当前帧——Phobos 写的
   `pBomb->DetonationFrame` 就是原生 `+0x38` 的 end frame（`IsTimerExpired
   0x00438A70` 在 `current > end` 时引爆），故载体自己的
   `TechnoClass::AI_Update` 引信检查会在下一次访问点响；`AffectTypes`
   按布雷者类型过滤，`SameInvokerOnly` 只留本次爆炸主自己埋的炸弹。回归测试
   `crit_tests::ivan_bomb_detonate_is_a_possible_cell_spread_member`、
   `sim::bomb::tests::ivan_bomb_detonate_cuts_only_the_hit_targets_fuse`、
   `sim::bomb::tests::ivan_bomb_detonate_filters_affect_types_and_the_invoker`。
8. **`[General] TunnelSpeed` 的读取点（本链，结论：休眠遗留，无消费者）**：
   `find-bytes` 定位字符串 VA `0x0083CA34`（在 `HoverDampen 0x0083CA28` 与
   `CrewEscape 0x0083CA40` 之间），其唯一 `imm32` 引用是 `push 0x83CA34` @
   `0x0066EDA1`，随后 `call 0x5283D0`（`CCINIClass::ReadDouble` 形态，
   `ini` 指针来自 `[0x7F0C9C]`）把 binary64 写回 `[esi+0x20]`（`0x0066EDAE`），
   即 `[General]` 规则块一次性读取。`.text` 内没有对该字段的消费者，`Tunnel`
   locomotor 以固定 `19.0`（`0x7F5B20`）行进，零售 `TunnelSpeed=1`
   （`rulesmd.ini:365`）属 TS 遗留。VERA 的 `RuleSet::general.tunnel_speed`
   同样只读不用，与原生一致，故本项关闭（详见
   `docs/phobos-tunnel-locomotor.md:120-131`）。

### 全局已知残渣（早前链条，仍需实机抽查）
- **CreateUnit 空中出生落位（2026-11-18）**：`:668-706` 全臂已移植
  ——`sim/anim_create_unit.rs` 在 Unlimbo 落位、`SpawnAnim` 之后按
  `!pTechno->InLimbo` 三分：
  - Fly（`:674-683`）：`pCell->GetContent() || AirportBound` 走
    `EnterIdleMode(false, true)`（`crate::sim::aircraft::enter_idle_mode_for`），
    否则 `pFlyLoco->Move_To(pCell->GetCoordsWithBridge())`
    （`Simulation::move_air_coordinate`，目标为 cell 中心坐标）；测试
    `...a_fly_spawn_on_a_free_cell_is_ordered_to_the_cell_centre`/
    `...an_airport_bound_fly_spawn_enters_idle_mode`/
    `...a_fly_spawn_on_an_occupied_cell_enters_idle_mode`。
  - Jumpjet（`:684-700`）：`LocomotionFacing.SetCurrent(DirStruct(facing))`
    （`JumpjetRuntime::phobos_set_locomotion_facing` → `JumpjetFlight::set_locomotion_facing`）；
    `BalloonHover` 时直接在运行态写 `IsMoving = true` +
    `DestinationCoords = GetCoords()`（`JumpjetRuntime::phobos_straight_ascend_hold`，
    不经 `Move_To` 的搜格/NavCom/下降抬升）并置
    `FootExt::JumpjetStraightAscend`（`GameEntity::mark_jumpjet_straight_ascend`），
    否则 `IsInAir()` 时 `Move_To(location)`（`jumpjet_move_to`）；测试
    `...a_jumpjet_spawn_snaps_its_locomotion_facing`、
    `...a_balloon_hover_jumpjet_spawn_holds_its_own_coords_and_the_latch`。
  - 其余（`:702-706`）：非 `AlwaysSpawnOnGround`、未走 `SpawnParachutedInAir`、且
    locomotor 不是 Fly/Jumpjet 时置 `IsFallingDown` + `OnParachuted`（落地不扣
    落体伤害，测试 `an_air_spawned_unit_is_let_go_into_a_parachuted_fall`）。
  该处剩余残渣只剩一条：`LastMapCoords = CellClass::Coord2Cell(location)`（`:687`）
  在 VERA 无 owner（`last_map_coords` 全库零命中），只有 `Process_Dig`/回归寻路类
  玩法会读它，当前无消费者。
- **`JumpjetStraightAscend` 的两个消费者（2026-11-18）**：闸已落地
  （`Ext/Unit/Hooks.Jumpjet.cpp:394-437`）。
  - `0x54BBD0` Ascending：闩置位时跳过 `target_speed_bits` 抬升与
    `LocomotionFacing.Set` 转向，仍执行其后的 factory 通知
    （`jumpjet_flight::state1_ascend`）。
  - `0x54D600` MovementAI：闩置位且 `State <= Ascending` 时跳过
    `Set_Location` 与 body facing 快照，只改 Z；崩溃/死亡或状态越过 1 时清闩
    并按普通路径移动（`jumpjet_flight::update_coordinates_and_altitude`）。
  测试 `...a_straight_ascend_keeps_the_horizontal_position`、
  `...a_state_past_the_ascend_clears_the_straight_ascend_latch`。
  残余：闩的清除点只按 `0x54D600` 的三条分支建模；Phobos 没有别的清除点
  （`find` 全文只有这两处消费者），故不另设。
- **`TechnoExt::OnParachuted`（2026-11-18）**：闩已落地
  （`sim/object_fall.rs` 的 `on_parachuted`）：空降出生
  （`Ext/Techno/Hooks.cpp:1907`，`sim/aircraft/drop_payload.rs`）与空中重现身
  （`Ext/Bullet/Hooks.DetonateLogics.cpp:607-610`）各置位一次，落地臂按
  `:1030-1031` 读取并清除、`:1067` 以 0 伤害包越过 EMP/ratio 段（测试
  `...a_parachuted_landing_skips_the_falling_down_ratio`、
  `...a_paradropped_passenger_is_marked_on_parachuted`）。两个置位点现均已落地：
  空降/空中重现身之外，`Ext/Foot/Body.cpp:784`（换类型时空中分支，见下一条）与
  `Ext/TechnoType/Body.cpp:705`（CreateUnit 空中落体）各自置位。
- **换类型时的空中分支（2026-11-18）**：Phobos `FootExt::UpdateTypeData`
  （`Ext/Foot/Body.cpp:722-793`）已移植。`sim/game_entity/type_conversion.rs` 的
  `convert_techno_type` 在 locomotor 重绑与 `ConvertToType:452-453` 的 balloon-hover
  `Move_To` 臂之后调用 `phobos_update_type_data_in_air`：
  - 门：目标不是 Aircraft、locomotor 不是 Fly/Hover；`isinAir` =
    `is_high_flying`（`ObjectClass+0x54`）且 `!is_attacked_by_locomotor`
    （`GameEntity::is_attacked_by_locomotor` = Magnetron 吊起）。
  - Jumpjet 目标：`JumpjetRuntime::phobos_relink_for_type_change` 按 `:733-746`
    重写 `JumpjetSpeed/Climb/Accel/Crash/Deviation/NoWobbles/Wobbles/Height`
    与 `JumpjetTurnRate`（≥127 截 127），保留 state/destination/moving；
    空中且 `BalloonHover=` → `phobos_hover_in_place`（State=2、moving 置位、
    未在动时 destination=当前位置），否则未在动时 `jumpjet_move_to(location)`
    （`:761-764`）。
  - 无 Jumpjet：`raise_falling_down()`，再按 `landing_cell_verdict`
    （`parachute_descent.rs` 的 `IsClearToMove` owner，新增返回
    `LandingCellVerdict{clear,on_bridge}`）决定：拒绝 → `begin_bomb_fall()`
    （`:778` 摔死）；通过或 `TryGetCellAt` 无格 → `OnBridge`/`store_ground_layer`
    + `mark_on_parachuted()`（`:782-785`）。
  测试 `sim/game_entity/type_conversion_in_air_tests.rs`（7 条，去掉调用后 6 条红）。
  ①`:787-791` 步兵的 `PlayAnim(Sequence::Paradrop)` 已实现（2026-11-19）：该调用在
  原生就是 `InfantryClass::Do_Action @ 0x0051D6F0`（`ret 0xc`，写 `SequenceAnim`
  `+0x6C4`），VERA 已有同一 owner（`infantry_action::infantry_do_action`，含
  `0x0051D70F` 零帧拒绝、`0x0051D919..0x0051D92E` 准入、Doing 写入与 stage 重启），
  故空中臂在两个落地分支之后按 `EntityCategory::Infantry` 调
  `infantry_do_action(id, DO_PARADROP, true, rules, effects)`；SHP 通道从
  `infantry_sprite_pose()` 的 Doing 读到 Paradrop 记录。残余：本调用传
  `randomStartFrame = false`，故 `0x0051DA4A..0x0051DA84` 的随机首帧（Scenario
  抽取）不触发——与原生调用一致。②已核实为等价（2026-11-19）：YRpp
  `CellClass.h` 的 `GetLevel()` 返回 `Level + (ContainsBridge() ? BridgeLevels(4) : 0)`，
  而原生 `0x004834E1..0x00483507` 取 `groundLevel == +0x11B` 走地面臂、取
  `+0x11B + 4`（带 `has_bridge`）走桥面臂；`-1` 在 `0x00483529..0x0048353C` 同样落到
  桥面臂。桥面格上两条语义都选 deck（`+0x128`，跳过 land 半段 `0x004835D5`），
  非桥格都选 ground（`+0x124` 再过 land 半段），故 `landing_cell_verdict` 的 `-1`
  与本臂的 `GetLevel()` 行为一致；该路径由
  `type_conversion_in_air_tests::an_airborne_conversion_lands_on_a_bridge_deck_over_water`
  锁定。
- **AE 的 `CloakCheck`（2026-11-19）**：Phobos
  `AttachEffectClass::CloakCheck`（`New/Entity/AttachEffectClass.cpp:446-453`，
  `Cloaking`/`Cloaked` 都算隐身，隐身着且动画类型的 `DetachOnCloak=` 为真时
  `KillAnim()`）与 `CreateAnim` 的同款门（`:476-477`，隐身时拒绝创建）已接线。
  `sim/attach_effect.rs` 的 `update_phobos_attach_effects_with` 逐帧读业主
  `cloak.state()`（`1|2`），对 `anim_type_detach_on_cloak` 为真的目标动画把
  `desired` 置空，由动画 owner 走正常的销毁分支，从而在隐身期间既不保留也不重建；
  隐身结束后的下一趟 `UpdateAttachEffects` 重新 `CreateAnim`。测试
  `a_cloaked_owner_hides_a_detach_on_cloak_effect_animation`
  （Cloaked 销毁 + 持续隐身不重建 + 解除隐身后重建新实例）与
  `a_detach_on_cloak_opt_out_keeps_the_effect_animation_while_cloaked`
  （`DetachOnCloak=no` 保留同一动画）。残余：`CloakCheck` 是每实例状态，本实现
  每帧按业主状态重算，等价。
- **`Animation.ResetOnReapply`（2026-11-19）**：Phobos
  `AttachEffectClass::RefreshDuration`（`New/Entity/AttachEffectClass.cpp:549-555`）
  末尾的 `KillAnim()` + `CanShowAnim()` 时 `CreateAnim()`，以及两条
  “新时长更短、不刷新” 的重挂臂（`:928-934` 单体、`:991-997` 累积）已接线。
  `sim/attach_effect.rs` 的 `AttachEffectInstance` 新增 `reset_anim` 闩：
  `refresh(effect)`／`refresh_with` 的重挂入口与两处 else 臂在键为真且当前有
  动画时置位，动画 owner `maintain_phobos_attach_effect_anim` 在其下一趟先
  `destroy_anim_with_context` 再按 `CanShowAnim` 重建。测试
  `a_reapply_restarts_the_animation_when_reset_on_reapply_is_set`、
  `a_shorter_reapply_still_restarts_the_animation`、
  `a_reapply_keeps_the_animation_without_reset_on_reapply`（键默认 false）。
  残余：原生在挂接调用内立即 `CreateAnim()`，本实现的重建发生在该 techno 下一
  帧的 `UpdateAttachEffects`，最多差一帧。
- **AE 累积动画换类型（2026-11-19）**：Phobos `AnimExt::ChangeAnimType`
  （`Ext/Anim/Body.cpp:198-243`）与唯一调用者
  `AttachEffectClass::UpdateCumulativeAnim`（`New/Entity/AttachEffectClass.cpp:520-526`）
  已落地。`sim/anim_class.rs` 新增 owner `Simulation::change_anim_type`：按
  `percentThrough = Value / Type->End`（本实现以阶段偏移 `current_frame` 计），
  新类型 `End==-1` 时按 `raw_shp_frame_count`（`Shadow=` 折半）解、`LoopEnd==-1`
  取 `End`；随后换 Type、`Accum=0`/`UnableToContinue=false`/`Reverse=新类型`、
  `Rate=`/`RandomRate=`（`ScenarioClass::Random.RandomRanged`，`0x004221F5`）/
  `Normalized=` 经 `GameOptionsClass::GetAnimSpeed`、`Animation.Start(rate, ±1)` 的
  帧延迟与方向；`restart` 时 `Value = reverse ? End : Start` 并调原生
  `AnimClass::Start @ 0x00424CE0`（起始音 + `Middle`，`anim_start` 已有 owner）。
  `sim/attach_effect.rs` 的 `maintain_phobos_attach_effect_anim` 新增 `retime`，
  `CumulativeAnimations.RestartOnChange`（默认 true）决定比例续播或重启；同一
  `AnimClass` 实例保留，不再销毁重建。测试
  `a_cumulative_layer_change_retypes_the_same_animation_in_place`、
  `a_cumulative_layer_change_restarts_when_the_type_asks_for_it`
  （`AttachEffect.DurationOverrides` 让新层更短，承载实例保持被指定）。
  残余：VERA 只为一个累积类型保留一个动画（被指定实例），原生每个持有
  `Animation` 的实例各自换类型；`Start=` 非零的体会与原生差一个 `Start`（零售
  ART 无该键）。
- **BuildLimitGroup 的 `RemoveProduction`**（`docs/phobos-build-limit-group.md`）：
  已完成（2026-11-18）。灰化/`CanBuild` 的 owner 仍是纯查询，但两个 gate 都返回裁剪
  意图（`BuildLimitGroupGate`），队列入场 owner `queue_production` 调
  `phobos_trim_build_limit_group_queue` 按 native 钩子顺序应用（UpgradeCheck 先、
  ReachedBuildLimit 后），走 `FactoryRegistry::remove_one_queued`
  （`FactoryClass::RemoveFromQueue @ 0x004CA620`，不退款、保留 held 产物）。
  `NotBuildableIfQueueMatch` 因此有了读者（`Body.cpp:1008`/`:1179` 两个尾裁剪）。残余：
  裁剪发生在入队时刻而非每次 `CanBuild`/灰化读取，纯 UI 触发的裁剪不会发生——
  可见结果集相同。触发：组上限已满但没有经过入队的队列动作；影响：没有可裁副本；
  频率：罕见；风险：低。
- **残渣文档已按 `src` 对齐（2026-11-18 第二轮）**：`phobos-land-sequences.md`
  （`SecondaryFireSequenceLandOnly` 已读并接线，`infantry_action.rs:311-348`）、
  `phobos-alternate-flh.md`（`BurstPerTurret` 由
  `fire_coord::phobos_turret_pivot_offset` 消费）、`phobos-auto-death.md`
  （`IsOwnerChangeFromRevertOnExit` 闩与 AutoDeath 臂已接，仅 AttachEffect
  读取仍缺）、`phobos-transact-money.md`（`CreateGap`/`Reveal`/`SpawnsCrate`/`LaunchSW`
  都已是独立 owner，全在 `sim::combat::crit`）、`phobos-critical-hit.md`（`CellSpread.Cylinder` 已由
  `combat_aoe` 消费）。这五处此前的 “unported” 声明已作废，不再作为残渣依据。
- **残渣文档已按 `src` 对齐（2026-11-18）**：`phobos-slaved.md`（`Slaved.OwnerWhenMasterKilled`
  四值分派已接线并有 `slave_manager_cycle_tests` 覆盖）、`phobos-spawns-queue.md`
  （`InitialSpawnsNumber=`）、`phobos-hover-drown.md`（`FallingDownDamage` /
  `.Water` / `.AllowEMP`）、`phobos-projectile-au.md`（Tunnel locomotor 已安装）、
  `phobos-tooltips-ui.md`（电力读数 / `SWShotsFormat` / `MaxWidth` 的 5 px 内边距）、
  `phobos-grinding.md`（`AccumulatedIncome += refund`）。这些页面里的
  “unported” 段此前已过时，不再作为残渣依据。
- **Falling-down 的 `HoverDrownable=no` 落点（2026-11-18）**：hover 关机的非淹死臂
  现在走 `FallingDownDamage` 的 ratio 模型
  （`sim/movement/parachute_descent.rs:145-156`，回归测试
  `a_non_drowning_hover_shutdown_takes_the_falling_down_ratio`），不再固定整
  `Strength`。落点前的 `IsClearToMove`（`Ext/Techno/Hooks.cpp:1043-1055`）也已接线：
  `Simulation::landing_cell_clear` 用 cell 的 reduced `ZoneType`（
  `MOVEMENT_ZONE_PASSABILITY`）加 `LandType` 行对 `SpeedType` 的零速判定，
  被挡的落点直接整血包（测试 `a_blocked_landing_cell_takes_the_whole_health_packet`）。
  `onBridge` 也已接线（2026-11-18 第三轮）：它取 cell 的 `ContainsBridge()`，level
  `-1` 时整条判定移到桥面（`0x0048353C..0x0048354E`），跳过地面两条臂、只读桥面
  occupation 字节按两个 ignore 掩码（`0xE0`→`0x5F`，剩 `0x40`）判定——落在水上桥
  面的履带车不再整血（测试
  `a_bridge_deck_landing_reads_the_deck_plane_not_the_water_ground`）。
- **飞机等级光照与风暴/支配者同时存在的顺序**：无残渣；飞机 `DrawIt` 臂改走
  `Simulation::lighting_aircraft_profile`（Dominator→核闪→Ion），与 Phobos
  `Ext/Techno/Hooks.Tint.cpp:320-338` 一致；jumpjet 臂保持 cell-lighting 的
  Ion 优先顺序。
- **Veinhole 的空战头**：`VeinholeWarhead=` 未设时 Phobos 向 `ReceiveDamage` 传 null
  战头；VERA 的伤害包必须有活动战头，因此该键未设/未知名时不造成伤害（零售
  `VeinDamage=0` 且未设该键，故零售行为不变）。TS `VeinProof` 能力在 YR 类型上
  没有可读标志，未建模。
- **DisplayIncome 的卖出标签**：无残渣；Ares 单位卖出路径
  （`production::foot_on_sold`）在全局 `[AudioVisual] DisplayIncome=yes` 时按
  退款额在单位 `Location` 立即打印（全局 Houses 过滤），
  `a_sold_unit_prints_the_display_income_label` 覆盖（需 ares+phobos）。
- **DisplayIncome 的 TS 奴隶矿车臂**：无残渣；非建筑 master 的存款差额在
  `slave_deposit` 中按 `DeploysInto` 建筑类型的 `DisplayIncome=` 开关立即打印
  （全局 Houses 过滤，锚点 master `Location`）；建筑 master 仍累到矿厂周期标签
  （`a_unit_slave_miner_prints_the_deposit_delta`）。
- **ExtendedPlayerRepair 的多人数值同步**：无残渣；`PlayerAutoRepairRecord`
  （VERA opcode `0x33`）把开关写进同步 `CommandRecord`，解码后按房执行
  `set_player_auto_repair`，单机/联机/回放同一路径。
- **Jumpjet 预测的三角/开方**：Phobos `JumpjetLocomotionPredictHeight` 用
  `Math::cos/sin`（MSVC CRT，binary64）与 `Foot::DistanceFrom`；VERA 用 Rust
  `f64::cos/sin` 与原生 3D 距离核（同样的 `+0.5` 截断），仅在最末一位边界上
  理论上可能与 CRT 相差 1 lepton。触发：长距离斜向预测正落在格边界；频率：
  极低；风险：预测高度差 1，影响一帧爬升。
- **ToolTips 扩展文本电力行**：无残渣；建筑臂走 `Simulation::cameo_power_delta`
  （`GetEnhancedPower` 无 plant 结算 + 原生 drain），电力非零时在成本行后追加
  ` ⚡±N`；单位臂在 `[General] UnitPowerDrain=yes` 时读取单位类型 `Power=`，
  同一开关让电力评估按活跃 Infantry/Unit/Aircraft 计数结算，并在增减时重算
  （`unit_power_drain_folds_active_counts_into_the_house_totals`）。
- **`SWShotsFormat` 余量**：无残渣；`SW.Shots` 同时读入 Phobos 元数据
  （默认 -1），计数在共享 Launch 尾对 stock 类型也自增（Ares 处理臂由
  `finish_ares_launch` 计数），phobos-only 的可用性与提示行均可用
  （`an_accepted_stock_launch_counts_one_shot`、`phobos_sw_shots_defaults_to_minus_one_and_reads`）。
- **`MaxWidth` 的 5 px 绘制内边距**（钩子 `0x479029`）：无残渣；cameo 换行宽度
  在 `MaxWidth>0` 时取 `MaxWidth-5`（回归测试
  `cameo_max_width_reserves_the_five_pixel_padding`）。
- **sim-scale deviations（有意保留，AGENTS.md）**：VERA 自有渲染器与 native
  管线不同（体素光栅/着色、图集）；`sim/` 不依赖 `render/`。实机对比时以
  “玩法可辨差异”为准，不以像素级 native 一致为准。

## 4. 实机测试清单（按本次新增链）

环境：本机 mod 测试数据目录（`config.toml` 的 `ra2_dir`；`VERA.INI`：`YROriginalStyle`、
`SoloSkirmish`），启动 `vera20k.exe`；用 `--profile fast` 构建。
每项给出：改哪个 INI、看什么现象、失败时先查哪条链。

1. **PreventScatter**（`rulesmd.ini`）
   - 在测试战头的 `[Warhead]` 段加 `PreventScatter=yes`，打一个敌方车辆；预期
     被击单位**原地不散开**（对比默认散射）。
   - `[CombatDamage] Warhead.PreventScatter=yes` + 战头不写：同样不散开。
   - 战头写 `PreventScatter=no` + 全局 yes：仍然散开（可写 `no` 压全局）。
2. **AI Chrono**（`rulesmd.ini`）
   - 复制一个 ChronoSphere/ChronoWarp 超武，`[General] AIChronoSphereSW=<名字>`、
     `AIChronoWarpSW=<名字>`（**类型 ID，不是数字**）；让 AI 跑含 56/57 动作的
     脚本（AIMD 的 “Allied Chronosphere” 脚本），观察它用的是键指定的超武。
   - 只写 `AIChronoSphereSW` + 该超武 `SW.PostDependent=<warp 名>`：warp 应为指定项。
   - 写错名字（不存在）：应回落到“最后一个 Type=ChronoSphere/Warp”。
3. **Berserk**（`rulesmd.ini`）
   - 心灵控制类武器（`Psychedelic=yes` 幻觉弹）打己方：默认不触发；加
     `[CombatDamage] AllowBerzerkOnAllies=yes` 后己方目标进入 berserk 并改打身边单位。
   - `[CombatDamage] BerzerkMission=Area Guard`：被打成 berserk 的单位任务面板应为
     Area Guard（默认 Hunt）；同时其移动目的地被清空。
4. **JumpjetControls**（`rulesmd.ini`）
   - `[JumpjetControls]` 改 `Speed=20`、`Crash=9`、`NoWobbles=yes`：**没有**写
     `JumpjetSpeed=` 等键的飞行单位（如 ZEP 只写 `JumpjetHeight`）应使用新值：
     更快、俯冲更快、悬停不再摆动；写了 `JumpjetClimb=` 的单位仍用自己的值。
   - 注意 stock 拼写 `JumpJetTurnRate=`/`JumpJetAccel=` 是**无效键**
     （gamemd 大小写敏感），测试时用 `JumpjetTurnRate=`/`JumpjetAccel=`。
5. **体素光照**（`rulesmd.ini`）
   - `[AudioVisual] UseFixedVoxelLighting=yes`：体素车辆/飞机的明暗高光应明显
     变化（viewer 项被移除），关闭后恢复。
   - `VoxelLightSource=x,y,z`：体素车辆/飞机的高光方向随归一化方向变化；未写键
     保持启动光照。
6. **ReplaceSilent**（`rulesmd.ini`，需要 Ares 语义的 `MindControl.Permanent=yes` 战头）
   - 先让一个单位被控制（Yuri 心灵控制/控制塔），再用带 `MindControl.Permanent=yes`
     的战头打它（替换旧控制）：默认会听到 `MindClearedSound`；
     `[AudioVisual] MindControl.Permanent.ReplaceSilent=yes` 后应静音；
     战头写 `RemoveMindControl.Silent=no` 时即使全局 yes 也要有声音。
7. **回归抽查**（本次未改但被涉及）
   - 心灵控制释放（杀控制者）声音与光环消失；Chrono 动作 56 的两种超武发射；
     幻象弹对免疫单位无效。

## 5. 测试环境注意

- 代码测试基线失败**与本工作无关**：core phobos 总计 60 条、client phobos 7 条
  （如 `crit_tests`、`view_admission_tests`、`mirage_disguise` 等由并行会话 WIP
  或 retail ini 缺失导致）；实机测试不要以这些失败为由回滚。
- 2026-11-16 在 `wt-keys` 下，client 的 lib 编译被基线
  `src/app/input/radar.rs:307`（`state.rules()` 中 `state` 未定义，HEAD 已存在；
  应为本会话之外的 `query.rules`）阻断。临时改成 `query.rules` 后验证：
  `cargo check -p vera20k --features phobos --lib` 通过，
  `beacons::tests` 4 passed；临时改动未提交、已还原。core 侧
  `rules::phobos_globals::tests`（64 passed）与 `mission::readiness::tests`
  （15 passed）已通过。
- 共享 checkout 有并行会话 dirty 文件（`app/input/radar.rs`、
  `app/presentation/{banners,screen_shake}.rs` 等），提交/合并时只 stage 自己
  的文件。
- 每次实机发现问题，按 §1 定位键 → 查 §3 是否已记为残渣 → 修复后补一条
  regression test 并更新本表。
## 6. 实机测试记录

- 2026-11-15 首次实机（`e01c7c1b7` 之前）：科技钻油井模型不显示。定位为
  **VERA 未挂载 `ECACHE*.MIX` / `ELOCAL*.MIX` 通配家族**（gamemd
  `0x826620`/`0x826614`，`Init_Game 0x530000`），而该 mod 把
  `ZAOILD.SHP` 等建筑图放在 `ecachemd97.mix`。修复在 `e01c7c1b7`，测试
  `startup_mounts_the_ecache_and_elocal_wildcard_families`。
  该修复同时影响所有把资源放进 `ecache*/elocal*` 的 mod——测试时若发现
  “某些 mod 单位/建筑/动画缺失”，先跑 `asset find <NAME> --all-mixes`
  判断是否属于这类挂载缺口。
