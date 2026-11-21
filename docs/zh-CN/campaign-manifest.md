---
layout: default
title: 战役系列
lang: zh-CN
---

# 战役系列

[English](../campaign-manifest.md) · **简体中文** · [Deutsch](../de/campaign-manifest.md) · [العربية](../ar/campaign-manifest.md) · [Русский](../ru/campaign-manifest.md) · [Svenska](../sv/campaign-manifest.md) · [ไทย](../th/campaign-manifest.md) · [Türkçe](../tr/campaign-manifest.md)

系列清单放在 `campaigns/<系列名>/campaign.ini`。每一关指定地图，可另写规则、角色、路线，以及要带入下一关的状态。先用一张图跑通，再按关卡需要增加角色。

用 `asset campaign-check` 可直接检查关卡，无需开房。实际游玩时，作者关卡从房间启动，既可以一人开局，也可以按角色分给多位玩家。互联网房间还需要大厅支持。普通 **战役** 菜单读取游戏的 `battlemd.ini` 战役列表。

[清单键词典](campaign-manifest-keys.md) 按节列出各键的取值、默认值和读取时机。

## 从一关开始

最小目录结构如下。

```text
campaigns/broken-line/
  campaign.ini
  maps/opening.map
```

```ini
[Stage Opening]
Map=maps/opening.map
```

这份清单由地图 `[Basic] Player` 指定玩家阵营。作者自检使用一个本地来源，开房时该关也只有一个参战席。

`campaign.ini` 用 UTF-8 保存，大小上限为 64 KiB。分号之后是注释。节名、键名以及关卡、角色、路线 ID 都按 ASCII 忽略大小写。ID 使用英文字母、数字、下划线和连字符。地图路径从系列目录起算，使用正斜杠。清单和地图的实际位置都须在系列目录内，路径中避免盘符、上级目录以及 `CON`、`COM1` 等 Windows 设备名。

标题依次查找完整语言代码、基础语言和 `Title`。例如，先查 `Title.zh-CN`，再查 `Title.zh`。列表按解析得到的标题排序。把各语言标题放在同一份清单中。房间会核对清单、地图原文件和展开引用之后的地图 INI 的摘要，清单中的注释和换行也会影响摘要。

## 运行作者自检

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir "/path/to/Yuri's Revenge"
```

第二个参数是关卡 ID，帧数须大于零。工具会读取地图并核对所引用的阵营，在固定种子和普通难度下运行关卡，再从诊断回放重建并逐帧比较哈希。命令输出 JSON，包含 `stage`、`manifest_sha256`、`map_sha256`、`checked_frames` 和 `final_state_hash`。有分支时逐个检查目标关卡，帧数以该关卡能实际运行到的长度为准。

清单错误会指出文件、节、键、行号和修复建议。遇到 **grant names an undeclared role**，检查授权键中点号前面的角色 ID。若提示阵营不存在，核对地图 `[Houses]` 中的名称。房间文件不一致时，先比较清单与地图摘要，再检查地图引用的 INI。最后开一场局域网对局，验证角色选择、过场和路线投票。

## 增加角色与下一关

以下两关各有两个参战角色。`opening.map` 和 `finale.map` 的 `[Houses]` 中都要列出 `Americans` 与 `Russians`。参战玩家可到达的目标关卡，也应提供足够的角色。

```ini
[Campaign]
Title=Broken Line
Title.zh-CN=断线

[Stage Opening]
Map=maps/opening.map
Title=Breakthrough
Title.zh-CN=突破
MinPlayers=2
MaxPlayers=2

[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly

[Rules Opening]
NetworkDiplomacyBriefing=Yes

[Routes Opening]
Coast=Finale

[CarryOver Opening]
Globals=Yes
Countdown=Yes
House.Americans=Credits,Difficulty
House.Russians=Credits

[Stage Finale]
Map=maps/finale.map
MinPlayers=2
MaxPlayers=2

[Roles Finale]
Lead=Americans
Support=Americans
```

`Lead` 和 `Support` 对主阵营 `Americans` 都有完整指挥权。`Lead` 还可以给 `Russians` 下达战术指令和要求完全控制权的指令，`Support` 只能对该阵营下达战术指令。授权按关卡和角色声明。若终章需要同样的授权，在终章的 `[Grants Finale]` 中再声明一次。

结算界面显示路线的目标关卡标题。内置 **重试** 仍进入本关，使用本关开局时收到的继承数据。`Retry` 只允许指向本关，省略它仍可重试。要让路线出现在 **继续** 的选项里，选用其他路线 ID。路线指回本关时，按继续处理，带上本关结束时采集的数据。

`Globals`、`Countdown` 和 `MapStage` 为布尔值，默认 `No`。`House.<ID>` 可以写 `Credits`、`Difficulty`，也可以逗号分隔同时写入。目标地图必须包含所继承的阵营 ID。目标地图的 `[Basic] CarryOverMoney` 决定带入资金的比例，`CarryOverCap` 决定上限。设置 `CarryOverMoney=100%` 和 `CarryOverCap=-1` 可将存下的资金全额加到目标阵营的预算。要接收倒计时，目标地图还须设置 `TimerInherit=Yes`。

### 规则覆盖

设置 `NetworkDiplomacyBriefing=Yes` 后，玩家可通过 **外交** 查看任务简报和只读的角色权限表。简报可以写在地图的 `[Basic] Briefing`，也可来自场景的 `MISSIONMD.INI`。地图文本里用 `@` 换行。

## 安装后开房

把战役目录放到游戏目录或当前模组的 `campaigns/<系列名>/` 下。每位玩家使用相同的游戏构建、规则和战役文件。复制时保留目录结构，包括地图引用的 INI 文件。如果两个目录里都有同名系列，以当前模组为准。

1. 创建局域网房间，在模式列表中选择战役标题。切回 **标准多人对战** 即可使用普通多人模式。房主安装了有效的 `campaign.ini` 后，列表才会显示战役。
2. 选择关卡和难度，有关卡角色表时，在 **角色** 列给每位参战玩家分配一个角色。两个角色可以共用同一阵营。没有角色表的关卡用地图 `[Basic] Player` 指定阵营，只接受一名玩家。
3. 确认玩家已准备，再开始。缺少角色、人数超出限制或文件与房主不一致时，房间会拒绝开局。

通过互联网开房需要 WOL 大厅公告 `campaign_rooms`，并在入房、开局及发放观战信息时转发战役选项。大厅提示不支持联机战役时，改用局域网。 [联机协议](../Networking.md#multiplayer-campaign-rooms)

战役可以把其他阵营的部分指挥权授给指定角色。作者开启简报后，可在游戏里的 **外交** 按钮查看任务文本、角色和权限。观战连接没有角色，也不占用关卡名额。开始前先清理房间里普通的旁观席。有 **观战** 入口时，从该入口加入。

过场会等参战玩家到达同一叙事节点再继续。关卡结算时，**重试** 按本关开局时的记录重开。**其他路线** 切换候选目的地，**继续** 为当前显示的目的地投票，并带上本关指定的继承数据。参战玩家须选择同一项。观战玩家可随时退出，不影响投票。

## 回放已打过的关卡

使用与录制时相匹配的游戏构建和资源，并把地图放在回放所记载的 `campaigns/<系列名>/` 路径下。诊断回放记录已接纳的角色、授权、继承记录及过场期间的模拟输入。

```text
vera20k --replay replays/session.json --ra2-dir "/path/to/Yuri's Revenge"
vera20k --replay-verify replays/session.json --ra2-dir "/path/to/Yuri's Revenge"
```

第一条命令打开图形回放。第二条无需窗口或联网即可核验，完成后输出终局状态哈希。无头核验使用原版规则。工具若提示版本不兼容，重新录制一份诊断回放。 [诊断回放格式](../Networking.md#diagnostic-replays)
