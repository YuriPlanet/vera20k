---
layout: default
title: 战役清单键词典
lang: zh-CN
---

# 战役清单键词典

[English](../campaign-manifest-keys.md) · **简体中文** · [Deutsch](../de/campaign-manifest-keys.md) · [العربية](../ar/campaign-manifest-keys.md) · [Русский](../ru/campaign-manifest-keys.md) · [Svenska](../sv/campaign-manifest-keys.md) · [ไทย](../th/campaign-manifest-keys.md) · [Türkçe](../tr/campaign-manifest-keys.md)

以下各节写在 `campaign.ini` 中，`<id>` 换成关卡 ID。布尔值接受 `Yes`、`No`、`True`、`False`、`1`、`0`。默认栏为从该清单加载的关卡使用的值。 表中的「—」表示省略该项。用不到的可选项直接留掉，键后不写空值。 [开房、清单示例与检查命令](campaign-manifest.md)

## `[Campaign]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `Title` | 文本 | 系列目录名 | 客户端建立已安装的系列列表时，在房间模式列显示这个标题。例如 `Title=Broken Line`。 |
| `Title.<locale>` | 文本 | `Title` | 客户端为系列列表选择与界面语言相符的标题，例如 `Title.zh-CN=断线`。依次回退到基础语言和 `Title`。 |

## `[Stage <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `Map` | 相对路径 | 必填 | 房间校验和开局加载都从系列目录打开这个文件。写现有地图的相对路径，例如 `Map=maps/opening.map`。文件实际位置必须在系列目录内。 |
| `MapSha256` | 64 个十六进制字符 | — | 校验地图时，把文件原始字节的 SHA-256 与这一项比较。省略它，房间仍会核对各客户端的地图摘要。 |
| `MinPlayers` | 1–128 | `1` | 开局前要求至少有这么多参战连接。必须两人都参加的关卡可写 `MinPlayers=2`，观战者不计数。 |
| `MaxPlayers` | 1–128 | 角色数和房间容量 | 入房时限制参战人数。有效上限同时受此值、角色数和房间容量约束。无角色表时最多一人，两人关可写 `MaxPlayers=2`。 |
| `Title` | 文本 | `ID` | 选关列表和结算界面显示这个标题。例如 `Title=Breakthrough`。 |
| `Title.<locale>` | 文本 | `Title` | 选关和结算时按界面语言选择关卡标题，例如 `Title.zh-CN=突破`。回退规则与 `[Campaign] Title.<locale>` 相同。 |

## `[Roles <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `RoleId=HouseId` | 地图阵营 ID | 单人用 `[Basic] Player` | 编排角色和开局时，把一个角色绑定到地图 `[Houses]` 中的阵营。`Lead=Americans` 与 `Support=Americans` 可给同一主阵营两个角色。每个角色只分给一人。 |

## `[Grants <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `RoleId.HouseId` | `FullControl` / `UnitsOnly` | — | 开局时给已声明的角色增加对另一个阵营的权限。`Lead.Russians=FullControl` 允许战术指令及要求完全控制权的指令。`Support.Russians=UnitsOnly` 允许战术指令。主阵营专属命令仍通过该角色的主阵营发出。 |

## `[Routes <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `BranchId=StageId` | 已声明的关卡 ID | — | 读清单时把目标解析为已声明的关卡。结算界面在继续选项显示目标，例如 `Coast=Finale`。`Retry` 只允许指向本关，省略该项也可使用内置重试，按入关记录重开。 |

## `[CarryOver <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `Globals` | `Yes` / `No` | `No` | 离开关卡时采集全局触发变量。`Globals=Yes` 会把当时的值带入目标地图，在首帧前恢复。 |
| `Countdown` | `Yes` / `No` | `No` | 结算时采集剩余任务倒计时。这里写 `Countdown=Yes`，目标地图也要在 `[Basic]` 中写 `TimerInherit=Yes`。 |
| `MapStage` | `Yes` / `No` | `No` | 结算时采集地图内部的战役进度数值。`MapStage=Yes` 会把该值写入下一张地图。关卡 ID 和路线另行解析。 |
| `House.<id>` | `Credits` / `Difficulty` / `Credits,Difficulty` | — | 结算时按阵营采集可用资金、难度或两者。`House.Americans=Credits,Difficulty` 要求两张地图都有 `Americans`。目标地图的 `CarryOverMoney` 和 `CarryOverCap` 决定带入的资金增量。 |

## `[Rules <id>]`

| 键或键格式 | 可填值 | 默认 | 读取时机与作用 |
| --- | --- | --- | --- |
| `Bootstrap` | `Campaign` | `Campaign` | 开局时使用地图预设阵营的构造路径。需要显式声明时写 `Bootstrap=Campaign`。 |
| `AllowWinTags` | `RequireClear` / `Ignore` | `RequireClear` | 核准胜利时，`RequireClear` 要等阵营的 AllowWin 标签计数清零，`Ignore` 跳过计数检查。结果计时仍然有效。 |
| `ComputerParanoid` | `Disabled` / `React` | `Disabled` | AI 收到重新判断敌我或结盟的请求时，`React` 允许进入 Computer_Paranoid 逻辑。其他阵营和规则检查仍会进行。 |
| `Crates` | `Keep` / `Skip` | `Keep` | 载入覆盖物时，`Keep` 保留地图上摆好的箱子，`Skip` 跳过。对局中定时刷箱子的处理由 `UsesMatchCrates` 控制。 |
| `AiBase` | `RetainAuthoredNodes` / `ReplaceUnavailableNodes` | `RetainAuthoredNodes` | AI 处理基地节点时，`RetainAuthoredNodes` 保留地图布置，`ReplaceUnavailableNodes` 允许按 AI 规则替换失效或反复放置失败的节点。 |
| `DoubleOwned` | `OwnerMask` / `AllCountries` | `OwnerMask` | 选择工厂时，`OwnerMask` 用通常的国家所有者掩码。`AllCountries` 会放宽标有 `DoubleOwned=Yes` 的类型的掩码。 |
| `TeamCreationLimitScope` | `AllHouses` / `PerHouse` | `AllHouses` | 创建小队时，计算 TeamType 的 `Max=` 是否已满。可以统计所有阵营的实例，也可只统计创建者的阵营。 |
| `CountsPlayerControlAsHuman` | `No` | `No` | 开局以已接纳的角色识别人类阵营。需要显式声明时写 `CountsPlayerControlAsHuman=No`。 |
| `AllowsConstructionYardRedeploy` | `Yes` / `No` | `No` | 玩家尝试把建造场收成基地车时，允许符合条件的建造场走部署回收流程。仍要通过建筑和对局选项检查。 |
| `AllowsIdleObjectSleep` | `Yes` / `No` | `No` | 每帧更新时，让符合触发和维修条件的闲置对象休眠。地图上有大量闲置对象时可开启。 |
| `AllowsOwnerlessTerrainLogic` | `Yes` / `No` | `Yes` | 更新地形对象时，纳入原本会被原版身份检查跳过的无主对象。 |
| `UsesCountryDifficultyBiases` | `Yes` / `No` | `No` | 给阵营应用难度时，再结合所选国家的难度修正值。 |
| `SkipsLocalPlayerAuthoredBuildings` | `Yes` / `No` | `No` | 只有本地执行时建筑加载才会检查这一项。锁步联机战役会载入地图中摆好的建筑。要在该模式去掉建筑，直接编辑地图。 |
| `UsesManagedAiBase` | `Yes` / `No` | `No` | 游戏中允许符合条件的 AI 自动建造、选址和部署。失效基地节点如何处理由 `AiBase` 决定。 |
| `UsesAiEconomy` | `Yes` / `No` | `No` | AI 经济更新时启用多人模式的矿石计价和符合条件的卖楼行为。价格和适用条件仍由规则与地图决定。 |
| `RunsAiStrategy` | `Yes` / `No` | `No` | 策略更新时，让 AI 定期寻找敌人，作出建造和其他策略决定。 |
| `UsesHouseTeamRules` | `Yes` / `No` | `No` | 载入和激活 AI 小队时，使用按阵营判定的触发路径。难度、科技等级检查随阵营和小队路径处理。 |
| `UsesCampaignDiscovery` | `Yes` / `No` | `Yes` | 视野发现及目标选择采用战役检查。只供一个客户端使用的发现流程仍要求本地执行。此项不会在角色之间共享视野。 |
| `UsesCampaignBaseDefense` | `Yes` / `No` | `Yes` | 基地受攻击时，采用战役规则决定被攻击对象的应对方式，以及哪些防守单位能响应。 |
| `UsesDirectedHunterSeekerTargeting` | `Yes` / `No` | `No` | 发射 Ares 猎杀者时，允许 AI 采用已指定的敌对目标阵营。`RandomOnly` 允许时人类阵营可优先选择非被动目标。 |
| `UsesMatchCrates` | `Yes` / `No` | `No` | 对局中使用多人箱子流程，包括定时位置和拾取判定。要自动刷箱子，还需启用对局的箱子选项。 |
| `UsesEliminationResults` | `Yes` / `No` | `No` | 结算检查时按阵营淘汰规则得出结果并清理。默认值保留关卡用触发器设置胜负的路径。 |
| `UsesRoomOptions` | `Yes` / `No` | `No` | 构造场景时使用房间起始标志，采用对局的采矿车免伤及矿石生长、蔓延路径。这些字段改由该路径给出。 |
| `UsesCampaignDiplomacy` | `Yes` / `No` | `Yes` | 点外交按钮时使用战役简报行为。此选项也会改变被动或平民阵营的外交处理。带角色权限的简报另见 `NetworkDiplomacyBriefing`。 |
| `AllowsManualAlliances` | `Yes` / `No` | `No` | 玩家发出结盟命令时允许进入该流程。独立的结盟规则和指挥权限仍会检查。 |
| `ModMapAdditions` | `Yes` / `No` | `No` | 图形客户端载入地图前，允许当前模组给地图追加内容。所有玩家须用相同模组文件。独立地图及无头自检所需的设置应写入作者文件。 |
| `SharedAlliedVictory` | `Yes` / `No` | `No` | 与锁步淘汰结果一起使用时，为符合条件的存活盟友启用共享胜利检查。脚本结算走各自的触发路径。 |
| `SkipsNetworkScoreDialog` | `Yes` / `No` | `No` | 锁步模式走到普通多人计分界面时跳过。联机战役续关使用自己的结算界面。 |
| `ParanoidIgnoresAlliesAllowed` | `Yes` / `No` | `No` | `ComputerParanoid=React` 进入联盟判断时允许越过 `AlliesAllowed` 检查。玩家改联盟的权限另行处理。 |
| `NetworkDiplomacyBriefing` | `Yes` / `No` | `No` | 点击游戏内的外交按钮时打开任务简报和只读角色、阵营权限表。例 `NetworkDiplomacyBriefing=Yes` 配合地图 `[Basic] Briefing`。 |
| `StartingHouseRoleAlliances` | `Yes` / `No` | `No` | 用席位构造阵营时，把同为人类或同为 AI 的起始阵营连成盟友。使用地图预设阵营的战役应在地图里定义初始同盟。 |
| `AresPsychicRevealExclusions` | `Yes` / `No` | `No` | Ares 心灵揭示在锁步对局执行时，保留竞技路径要求的三个相对坐标排除格。 |
| `DissolveUnfilledTeams` | `Yes` / `No` | `No` | 小队 AI 更新时，允许一直未满员且已空的小队在规则 `DissolveUnfilledTeamDelay` 指定的时间后解散。 |
