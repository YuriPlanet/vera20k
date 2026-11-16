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

仍未实现的 1 键及阻塞原因见 §1.7。

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

### 1.5 光照 / 视觉（8，已完成 7，1 阻塞）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `AircraftLevelLightMultiplier` | `Ext/Rules/Body.cpp:349` | double, 1.0（`:257,778`） | `Ext/Techno/Hooks.Tint.cpp` |
| `JumpjetLevelLightMultiplier` | `Ext/Rules/Body.cpp:350` | double, 0.0（`:258,779`） | `Ext/Techno/Hooks.Tint.cpp` |
| `RadialIndicatorVisibility` | `Ext/Rules/Body.cpp:172` | AffectedHouse, Allies（`:207,736`） | `Misc/Hooks.UI.cpp` |
| `ChronoSparkleDisplayDelay` | `Ext/Rules/Body.cpp:118` | int, 24（`:37,577`） | `Ext/Techno/Hooks.Misc.cpp`、`Ext/Building/Hooks.cpp`、`Ext/Aircraft/Hooks.cpp` |
| `ChronoSparkleBuildingDisplayPositions` | `Ext/Rules/Body.cpp:119` | enum, OccupantSlots（`:38,578`） | `Ext/Building/Hooks.cpp` |
| `ColorAddUse8BitRGB` | `Ext/Rules/Body.cpp:260` | bool, no（`:178,703`） | `Utilities/GeneralUtils.cpp`（`GetColorFromColorAdd`；VERA 已有 `ColorAddTable::phobos_word`，其注释已记该键为残渣） |
| `UseRetintFix` | `Ext/Rules/Body.cpp:409` | bool, **yes**（`:311,823`） | `Ext/TAction/Hooks.cpp`（retint 后重新激活 LightSource） |
| `VoxelLightSource` | `Ext/Rules/Body.cpp:352` | `Nullable<Vector3D<float>>`，未设（`:271,780`） | `ReplaceVoxelLightSources`（`Body.cpp:1189-1207`）——**阻塞**，见 §3 |

### 1.6 跳跃（2）与战头（1）——已完成（2026-11-16）

| 键 | 出处 | 类型 / 默认 | 消费者 |
|---|---|---|---|
| `JumpjetClimbPredictHeight` | `Ext/Rules/Body.cpp:336` | bool, no（`:242,766`） | `Ext/Unit/Hooks.Jumpjet.cpp` |
| `JumpjetClimbWithoutCutOut` | `Ext/Rules/Body.cpp:337` | bool, no（`:243,767`） | `Ext/Unit/Hooks.Jumpjet.cpp` |
| `VeinholeWarhead` | `Ext/Rules/Body.cpp:136` | `Nullable<WarheadTypeClass*>`, 未设（`:58,597`） | `Ext/Anim/Body.cpp`（`AnimExt::VeinAttackAI`） |

`VeinholeWarhead` 的接线：`[General] VeinDamage=` 伤害经该 warhead 发给
`IsVeins=` 动画所在格、地面 5 lepton 内的 Techno；`[ObjectType] ImmuneToVeins=`
和既有 `VEIN_PROOF` 免伤，`UnableToContinue` 条件与原生一致。

### 1.7 仍未实现的 1 项（1 键）与阻塞原因（2026-11-16）

| 键 | 阻塞点 / 下一步 |
|---|---|
| `VoxelLightSource` | 仍阻塞于 `Matrix3D::VoxelDefaultMatrix`，见 §3。 |

## 2. 明确排除（不移植，扫描器已列 disabled）

| 键 | 原因 |
|---|---|
| `InflictLocomotor` / `RemoveInflictedLocomotor` | `TestLocomotionClass` 开发者测试钩（`Ext/WarheadType/Detonate.cpp:251-254`），Phobos 构建内不生效 |
| `Trajectory.Sample.TargetSnapDistance` | Phobos 轨迹 Sample 模板编译关闭（`Ext/Bullet/Trajectories/PhobosTrajectory.h:5-11`） |
| `VoxelShadowLightSource` | read（`Ext/Rules/Body.cpp:353`）、Process（`:947`）、apply（`:1200-1207`）全被注释，作者自注 “doesn't really impact anything” |

## 3. 已记录残渣（按链，含触发/影响/频率/风险）

### 本会话新增链
1. **Warhead.PreventScatter**（`8bacaf9bd`）：rules 解析不到的 warhead 名 → VERA
   照常散射；native 会走全局。触发：坏 WARHEAD 引用；频率：仅坏数据；风险：低。
2. **AIChronoSphereSW/AIChronoWarpSW**（`6fb756008`）：
   - `ValueableIdx` 名字→索引在**跨 INI 层**的差异：native 坏名字保留旧层索引，
     VERA 单次合并读取下解析为未找到；频率：仅坏数据。
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
   见下。
6. **MindControl.Permanent.ReplaceSilent**（`6a261e2cb`）：无残渣；只作用于
   Ares 的 ApplyPermaMC 调用点，Dominator 原生路径不变。
7. **VeinholeWarhead**（本提交）：`AnimExt::VeinAttackAI` 的 `pOccupier` 高度
   用 VERA `queried_flight_height`（已含桥面/甲板高度修正），原生用
   `GetHeight()`；`Unsorted::CurrentFrame % 2` 用 `session.binary_frame() % 2`。
   触发：桥上或跨帧奇偶边界；频率：低；风险：单帧伤害时序或高度门槛相差。

### 全局已知残渣（早前链条，仍需实机抽查）
- **`VoxelLightSource`（阻塞）**：apply 需要
  `Game::VoxelLightSource = Matrix3D::VoxelDefaultMatrix * normalized(src)`。
  缺 `Matrix3D::VoxelDefaultMatrix`（`0x754C00` 用 `0x5AE860`+`0x5AF080` 在栈上
  生成）。下一步：扩 `tools/voxel_oracle/lighting.py` dump 该矩阵并断言
  `M*v == YR_WORLD_LIGHT`，再填 `VoxelLight.world`。
- **DisplayIncome 的卖出标签**：`FootClass::Sell`（`Hooks.Selling.cpp:7-27`）在
  全局 `DisplayIncome` 开启时立即打印卖出金额；VERA 目前没有玩家单位卖出路径，
  该臂未接。触发：卖出单位；频率：仅单位卖出；风险：少一条浮动金额。
- **DisplayIncome 的 TS 奴隶矿车臂**：`Hooks.Refinery.cpp:40-61` 对
  `BuildingClass` 奴隶矿车累积、对其 `DeploysInto` 类型立即打印；VERA 的奴隶
  收入统一经 `pay_refinery_owner` 累到矿厂。触发：TS 风格奴隶矿车；频率：仅
  该类单位；风险：标签锚点位置不同（零售无此单位）。
- **Level 光照倍率的取整与桥面高度**：`AircraftLevelLightMultiplier` 在默认
  1.0 时保留原生整数 `height/208` 商，仅作者改倍率时走 Phobos 的 binary64
  `level * height/208 * multiplier`；因此在高度非 208 整数倍时，默认渲染与
  Phobos 的 double 路径可差几个 intensity。`JumpjetLevelLightMultiplier` 的
  桥面臂按 Phobos 再减 4 level，而 VERA 的 `queried_flight_height` 已扣除
  桥面，桥上的 jumpjet 可能比 native 少 4 个 level 的负项。触发：改倍率或
  jumpjet 停在桥上；频率：低；风险：轻微亮度差。
- **ExtendedPlayerRepair 的多人数值同步**：Phobos 用自有 `EventClass` 房间事件
  翻转 `PlayerAutoRepair`；VERA 目前把它作为本地 `Command` 队列项，单机/战役
  正确，联机时对端看不到该房标志变化。触发：联机中玩家点修理按钮；频率：每次
  切换；风险：双方建筑自动修理状态不一致。下一步：给它一个 VERA 同步
  opcode 并让回放/校验包含该房标志（标志本身已持久化）。
- **Jumpjet 预测的三角/开方**：Phobos `JumpjetLocomotionPredictHeight` 用
  `Math::cos/sin`（MSVC CRT，binary64）与 `Foot::DistanceFrom`；VERA 用 Rust
  `f64::cos/sin` 与原生 3D 距离核（同样的 `+0.5` 截断），仅在最末一位边界上
  理论上可能与 CRT 相差 1 lepton。触发：长距离斜向预测正落在格边界；频率：
  极低；风险：预测高度差 1，影响一帧爬升。
- **ToolTips 扩展文本电力行**：`GetPower` 的建筑臂需要实时
  `PowerPlantEnhancer` 结算、单位臂需要未移植的 `UnitPowerDrain`，扩展提示暂无
  电力段。触发：开启扩展提示；频率：每次悬停电力建筑/单位；风险：少一段文本。
- **`SWShotsFormat` 余量**：`SW.Shots` 状态只在 Ares 包；仅 phobos 构建没有计数，
  射击行不显示。触发：自定义 `SW.Shots` 且开启扩展提示；风险：少一行提示。
- **`MaxWidth` 的 5 px 绘制内边距**（钩子 `0x479029`）：VERA 文字内边距为固定
  常量，不随 `MaxWidth>0` 收缩；触发：设置 MaxWidth；风险：边框附近间距差 5 px。
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
   - `VoxelLightSource=` **预期无效果**（未移植，见 §3）。
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
