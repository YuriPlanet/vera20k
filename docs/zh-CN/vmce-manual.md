---
layout: default
title: VMCE 使用说明书
lang: zh-CN
---

# VMCE 使用说明书

[返回文档首页](index.md) · **简体中文** · [English](../vmce.md)

VERA Multiplayer Campaign Expansion（VMCE）让玩家在 VERA20k 的 LAN 房间游玩作者编写的多人战役。一个系列由 `campaign.ini` 和若干张地图组成。每关可以安排不同的参战角色、共享单位、房规和续关条件。

- **玩家和房主**可先开一局，再查旁观、TAB 外交和结算规则。 [安装与启动](#安装与启动) · [玩家指南](#玩家指南)
- **地图作者**可复制 House 和分支触发器示例，并核对清单。 [地图制作者指南](#地图制作者指南) · [键值参考](#campaignini-键值)

## 与原始版本的关系

| 模式 | 入口与参战者 | 开局和指挥权 | 结算与继续 |
|---|---|---|---|
| 原版单人战役 | 主菜单选择战役，一名真人。`BATTLEMD.INI` 指定入口任务。 | 地图的 `[Basic] Player` 指定当前 House，地图还可通过 `PlayerControl=` 让玩家操作其他 House。 | 胜利走战役结算和续关，可继承全局变量、资金等战役状态。失败时可重玩本关。[战役入口](../../src/rules/campaigns.rs) · [当前 House](../../src/sim/scenario_bootstrap.rs) · [胜利](../../src/app/campaign_victory.rs) · [失败](../../src/app/campaign_defeat.rs) |
| 原版遭遇战（Skirmish） | 主菜单进入遭遇战，一名真人与电脑席位同场。零售模式列表有 Battle、Free For All 和 Team Alliance。 | 对局按地图起点和开局选项分配 House 并生成起始单位。电脑使用遭遇战的席位与难度设置。 | 本局按遭遇战规则结算。再玩一局时重新选图和配置。[模式列表](../../src/skirmish_modes.rs) · [单位生成](../../src/sim/scenario_bootstrap.rs) · [结算入口](../../src/app/match_runtime/scenario_exit.rs) |
| 原版 Cooperative（合作战役） | 多人房选择 Cooperative。模式提供两个真人席位，搭档共同对付电脑。 | `CoopCampMD.ini` 指定关卡变体及每关玩家和敌方的国家名单。搭档和电脑方按合作战役的任务配置入场。 | 有战役进度时使用 `BC6` 结果页，胜利按搭档记录进度并推进关卡。[模式与席位](../../src/skirmish_modes.rs) · [关卡配置](../../src/app/frontend/skirmish_session/cooperative.rs) · [合作结算](../../src/app/cooperative_round.rs) |
| VMCE | 在 VERA20k 的普通 LAN 房间选 **多人战役** 和关卡。关卡指定参战人数、角色及是否开放观察席。 | 房间把每位参战者绑定到一个主 House，并冻结地图作者授予的附属 House 权限。地图可预置单位和基地，也可用触发器生成。 | 地图触发器决定胜负。`campaign.ini` 定义系列顺序、分支和末关。胜利用 `0x103` 奖章页，房主继续后回房预选后继关。失败演出后使用 `BC6` 统计页，可重试本关。[关卡清单和后继](../../src/map/vmce.rs) · [结算与回房](../../src/app/match_runtime/vmce_result.rs) |

原版单人战役和 Cooperative 都通过侧栏按钮重看任务简报。Cooperative 还可用默认 `Tab` 打开外交表。原版遭遇战可以用侧栏外交按钮或默认 `Tab` 直接打开外交表。VMCE 先打开任务简报，再从右栏「外交」进入外交表。关卡的 `AllowAlliesChange` 和本局席位决定谁能改盟。[外交入口](../../src/app/input/diplomacy.rs) · [改盟权限](../../src/sim/house_diplomacy.rs)

## 安装与启动

### 前置条件

房主、参战者和观察者使用同一 VERA20k 构建和游戏功能配置，并配置各自的《尤里的复仇》原版资源目录。使用 Mod 时，各端须匹配有效规则。把相同的系列清单和本关地图放进各自的设置目录。 [联网校验](../Networking.md#authored-scenario-setup)

### 构建与启用

从源码构建时，使用 VERA20k 的常规构建命令。准备好有效的系列后，在 LAN 房间的选图窗口选择「多人战役」。

```sh
python -m tools.cargo_run -- build -p vera20k --profile fast --bin vera20k
```

### 战役目录

每个系列占用 `Maps/VMCE/` 下的一个子目录。游戏以 VERA 的实际设置目录为根。若在 `config.toml` 的 `[paths]` 中指定了 `settings_dir`，就从该目录下的 `Maps` 查找。

```text
<设置目录>/Maps/VMCE/
└── Example/
    ├── campaign.ini
    ├── entry.map
    ├── next.map
    ├── branch.map
    └── credits.txt
```

- 清单的地图路径以系列目录为根。子目录可写成 `Stage01/entry.map`。把对应的 `[Scenario:Stage01/entry.map]` 段写进清单。
- 关卡使用系列目录内的松散地图文件，例如 `.map` 或 `.yrm`。准备房右栏分两行显示系列名和关卡名。系列 `[Campaign] Name`、地图 `[Basic] Name` 和主 House 的 `UIName` 可引用 CSF，依次查询系列目录下的 `ra2md.csf` 和当前游戏的 CSF。系列缺标签时显示目录名。地图 `[Basic] Name` 也可填写标题，缺少可读标题时显示关卡编号。角色缺专名时显示角色编号。
- `campaign.ini` 最大 1 MiB，可选的系列 `ra2md.csf` 最大 8 MiB。每个翻译名称最多 128 个字符。翻译表缺失、损坏或超限时仍可验图和开局。
- 房间按当前所选关卡核对 `campaign.ini` 和地图的原始字节，并执行 VERA 的有效地图和规则校验。可先安装清单与本关地图。选中后续关卡时，再校验各端安装的对应地图。可选翻译表用于本机显示。
- 系列末关可以读取可选的 `credits.txt`。文件用 UTF-8 编码，可带 BOM，最大 32 KiB。每行是一段居中字幕，空行留白。缺失或无效时播放原生滚动字幕。

### 最小 campaign.ini

用一张地图建立一个带两个可选角色的系列。将这份清单与 `entry.map` 放在同一目录。地图的 `[Houses]` 中须有 `HouseAlpha` 和 `HouseCharlie`。

```ini
[Campaign]
Entry=entry.map
ScenarioCount=1
Scenario1=entry.map

[Scenario:entry.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes
```

在地图没有人数额外设置时，这个例子允许一名或两名参战者开局。`Entry` 排在该系列列表首位。房主也可以直接选择其他已安装的合法关卡。

### 启动一局

1. 房主从「网络」建立 LAN 房间，打开选图窗口，选择「多人战役」和已安装并校验通过的关卡。进入多人战役房后，可用右栏「选择关卡」换关。房主使用参战席位。若普通房间里已添加大厅 AI，先移除这些席位。
2. 其他玩家从 LAN 列表加入，可以勾选「只显示多人战役」来筛选 VMCE 房间。旁观者在加入前勾选观察选项，所选关卡须开放旁观，房间也须有空闲观察席。
3. 参战者在自己昵称下方选择空闲角色或「随机」。从「详情」查看开局资金、科技等级、地图预置单位和建筑数量及共控部队。房主编辑关卡开放的房规。中栏是聊天，右栏可查看缩略图并打开任务简报。长文可用「上一页」「下一页」阅读。
4. 客机验图通过后点「准备」。观察者也要验图并准备。参战人数和准备状态符合本关要求时，房主点「开始」。
5. 同关改角色、房规或名单时，等复验通过后恢复已确认的准备意愿。换关或回房开新一局时重新点准备。载入画面显示任务原画、简报和本机进度。有其他玩家仍在加载时，进度条内显示「等待其他玩家...」。

### 如何阅读本页的 INI 片段

方括号包住段名，等号左边是键，右边是值。尖括号中的文字代表应替换的名称或路径。`campaign.ini` 示例按系列目录写相对路径。标为地图片段的内容应并入地编保存的完整地图。

## 玩家指南

### 角色、主 House 与共享 House

角色对应地图里的一条主 House。该 House 的国家、颜色、单位和开局资金来自地图。房间先保留显式选角，再用房主的种子打乱剩余角色，按房间席序分给选「随机」的玩家。单人选择「随机」时使用首个角色。同一条主 House 只分给一名参战者。未分配的 House 保留地图中的单位、AI 和触发器。

地图作者可以把一条附属 House 的控制权授予一个或多个角色。目标须存在于地图 `[Houses]`，且位于本关主角色列表之外。参战者可点击、框选和按类型选择已发现的共享机动单位，用 `1–9` 编队，再按单位能力移动、攻击、强制攻击、部署、进入载具或卸载。获授权且支持集结点的建筑可以设定集结格。

生产、资金、超武、维修、变卖和收起建造厂仍由主 House 管理。视野和迷雾沿用地图与同盟规则。

### 任务中的规则

- **参战人数**遵循 `1 <= MinPlayers <= MaxPlayers <= PlayerCount <= 30`。观察席从参战容量之后分配，`MaxPlayers=30` 时已占满连接容量。角色分配、下令权限和统计按参战席位计算。
- **地图同盟**来自各 House 的 `Allies=` 和触发器。战局按 `Tab` 或侧栏按钮打开本关简报，再点右栏「外交」。表格只列真人，显示昵称、角色、国旗和阵营、同盟及会谈。`AllowAlliesChange=yes` 时，仍在参战的真人可彼此改盟。地图锁定时可从文字区分双向结盟、两种单向结盟和未结盟。外交页先返回简报，再返回战场。
- **结算**由地图触发。主 House 全灭后，任务继续，玩家仍能指挥在场的获授权单位。地图预置和触发器生成的箱子有效。旧版留下的短局、随机箱子和胜负模式设置会被忽略。
- **剧情**会等待所有已连接的参战者和观察者各自看完暂停型视频。可按 Esc 跳过，素材缺失或播放失败也算本端跳过。非暂停雷达视频播放时任务继续。按 Esc 打开普通菜单时，其他玩家仍可继续任务。

### 难度

`Difficulty` 写 `0`、`1`、`2` 分别表示简单、普通、困难，默认普通。每个值对应地图触发器的 Easy、Medium、Hard 列，并给每位参战者的主 House 应用相应的火力、生产等难度属性和开局资金调整。`LockDifficulty=yes` 时，关卡配置固定难度。改动可编辑的难度会让客户端重新校验。

### 结果页与继续

- **胜利**进入 `0x103` 奖章页。奖章上方显示任务报告，优先使用地图提供的内容，缺省显示「战后报告」和「任务完成。」。奖章下方始终显示标准用时和本局用时。「统计数据」打开右栏子页，查看玩家战绩和击毁数。每人自行点「继续」。客机可先回关间房聊天，预览区显示「暂无预览」，地图缩略图和准备操作等房主回来后恢复。房主回房后预选下一关并可改选。下一关有相同且可用的主 House 时，恢复对应角色。等待两分钟仍无法重入原房时，客机回到 LAN 列表并收到提示。末关继续时播放片尾，再回原房并选中末关。新一局重新验图和准备。
- **失败**先播放可用的单人战役失败演出，再显示 `0xBC6`。右栏显示本关缩略图和关卡名，并提供「重试本关」「返回房间」和「离开房间」。参战者确认重试、原连接齐全且各端复验通过后，按原关、名单、角色、房规、种子和开局继承状态加载。观察者随房复验文件。「返回房间」可取消正在等待的重试。重试进入交接时，退出操作暂时禁用。交接中断时，回到 LAN 加入房主为原关重开的房间。
- **混合胜负**按每位玩家自己的结果演出。原房的预选关卡以房主的结果为准。房主胜利预选后继关，房主失败保留本关。
- **退局和统计**。中途退局或断线时，统计表省略离场者。其主 House 和部队留在地图上，仍参与触发器判定。若房主离场，其余玩家完成任务后回到 LAN 列表。
- **统计项目**包含留场参战者的击毁、损失、建造、分数和收入。收入缺数据时显示短横线。

## 地图制作者指南

### 角色 House 的写法

先在地图中登记各角色所属方。下面用 `HouseAlpha`、`HouseCharlie` 担任主 House，`HouseBravo` 提供共享单位。三条 House 使用已有的独立国家，并互相列入地图同盟。把这些字段并入地图的原有 House 设置，再给每个角色放置需要的初始单位或基地。

```ini
[Houses]
0=HouseAlpha
1=HouseBravo
2=HouseCharlie

[HouseAlpha]
Country=Americans
Allies=HouseAlpha,HouseBravo,HouseCharlie

[HouseBravo]
Country=Russians
Allies=HouseAlpha,HouseBravo,HouseCharlie

[HouseCharlie]
Country=British
Allies=HouseAlpha,HouseBravo,HouseCharlie
```

### 国家、所属方和触发器的关系

清单的 `Player1=HouseAlpha` 引用 House 名称。给各角色写按国家触发的条件时，用 `Americans` 和 `British` 区分。需要发给某个角色的增援，应在地编中明确指定对应所属方。多个 House 的任务可分别编写条件，再复用相同的脚本和编队。

在 `campaign.ini` 的 `[Scenario:entry.map:HouseAlpha]` 中可写 `CameraWaypoint=2`，让该角色开局镜头对准地图 `[Waypoints]` 的第 `2` 号航点。支持 `0` 到 `701`。缺少有效航点时使用主 House 的常规开局镜头。

### 房规与分支清单

有效房规按地图 `[Basic]`、`[Campaign]`、`[Scenario:<地图路径>]`、房主在大厅的合法编辑逐层覆盖。某一层省略键就继承下层。显式 `no` 和难度 `0` 都是有效的覆盖值。`AltNextGlobal` 单独从系列值继承，单关可以重新指定。

下面的系列有入口关、普通后继和分支后继。三关均要求两人参战并允许旁观。入口关把 `HouseBravo` 的控制权交给角色 1，另外两关都作为系列末关。

```ini
[Campaign]
Entry=entry.map
ScenarioCount=3
Scenario1=entry.map
Scenario2=next.map
Scenario3=branch.map
MinPlayers=2
AllowObservers=yes
Difficulty=1
LockDifficulty=no
AllowAlliesChange=no
AltNextGlobal=22

[Scenario:entry.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
NextScenario=next.map
AltNextScenario=branch.map

[Scenario:entry.map:HouseBravo]
Player1Control=yes

[Scenario:next.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes

[Scenario:branch.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes
```

房主胜利后，回房时按以下顺序预选关卡。

1. `EndOfGame=yes` 时完成系列，回到原房保留本关。
2. 声明了 `AltNextScenario` 且终局的 `AltNextGlobal` 已置位时，预选该分支。
3. 其余情况取 `NextScenario`，再回退到清单中的下一项。清单已经到末尾时保留本关。

房主回到大厅后仍可改选。普通后继和分支后继均须出现在系列清单中。写续关时，以各关自己的 House、单位和触发器为准。

### 继承全局变量、倒计时和资金

房主胜利回房后，按系统预选的后继关开局会继承上一关终局的战役全局变量。下一关地图的 `[Basic]` 设置 `TimerInherit=yes` 后，仍有剩余时间的任务倒计时也会继承，显示下一关配置的标题。房主改选其他关卡后，上一关的继承状态清空。关闭房间后，进度随之结束。

如需继承资金，在 `campaign.ini` 为每一条转场分别声明对应关系。等号左侧是上一关的主 House 名，右侧是下一关的主 House 名，各自只能出现一次。关卡路径和 House 名可忽略大小写；同名 House 也须显式填写。

```ini
[CarryOver:entry.map:next.map]
HouseAlpha=HouseCharlie
HouseCharlie=HouseAlpha
```

写入下一关地图 `[Basic]` 的 `CarryOverMoney` 和 `CarryOverCap` 决定每个角色继承的比例和上限。继承额加到下一关已经按难度调整的开局资金，转给实际接手目标角色的玩家。分支需自己的映射段。直接重试恢复首次开局接收的记录。

### 触发器的写法

地图 `[Basic]` 的 `Intro` 和 `Action` 视频在第一帧模拟前播放。影片表无法解析 `Intro` 时改用 `Brief`。战中动作 `10` 播放全屏视频，`117` 播放暂停型雷达视频，`100` 播放非暂停雷达视频。动作 `48/112` 将各客户端的镜头移到指定位置。`110` 暂停场景，正时长到期后同步恢复，值为 `0` 时持续等候该停顿的解除事件。`46/47` 只锁定或解锁操作，模拟照常进行。

地图 `[AITriggerTypesEnable]` 的 `yes/no` 分别启用和禁用 AI 触发器。按单人战役规则检查组队资格，已有可招募单位可以满足缺工厂的检查。没有显式设置 `TechLevel` 的 House 使用 VMCE 的开局值 `10`。附属 House 的共享控制沿用 `PlayerNControl`。

任一参战者发现目标即可满足对应的发现事件。按国家检查全灭的事件使用按地图顺序注册的第一个匹配 House。VMCE 不为 `Player @ A/B/…` 添加角色映射。

#### 触发器头

保留地编生成的触发器格式。`Owner` 使用国家名。使用按国家查找第一条 House 的事件时，为各角色设置独立的 `Country`，再为每个角色写相应条件。

```ini
[Triggers]
<ID>=<Owner>,<Linked>,<Name>,<Disabled>,<Easy>,<Medium>,<Hard>,<Transfer>
```

#### 结局动作

| 动作 | 名称 | 本版 VMCE 行为 |
|---|---|---|
| `1` | 匹配国家胜利 | 指定国家的所有开局参战者及其直接双向盟友判胜，其余判负。 |
| `2` | 匹配国家失败 | 指定国家的所有开局参战者及其直接双向盟友判负，其余判胜。 |
| `67` | 宣告胜利 | 为参战者设置共同胜利结果。 |
| `68` | 宣告失败 | 为参战者设置共同失败结果。 |
| `69` | 结束任务 | 已有结果时恢复结算计时。单独执行时请求胜利。 |

没有任何参战 House 匹配动作 `1` 时，全员失败。动作 `2` 的无匹配情况全员胜利。首个整局结果锁定后，后续的相反动作无法改写。任务胜利、失败和双灭都应有对应的地图条件。旧版依靠自动全灭的地图需要添加结局触发器。

VMCE 沿用联网结算节奏，胜利不等待 `AllowWin`。小队脚本动作 `7/23` 当前仍令全队共同胜负，与单人战役只作用于当前玩家有差异。

#### 进入目标格后选择分支

下面的入口关地图片段使用 M4 验收的目标格和动作顺序。`HouseBravo` 使用 `Russians`，其坦克可移动到 `(30,36)`。进入格事件检测该国家的单位。并入地图时，使用本图未占用的触发器和标签编号，并保持各段引用一致。

```ini
[Triggers]
01000001=Russians,<none>,Finish with the shared tank,0,1,1,1,0

[Events]
01000001=1,1,0,8

[Actions]
01000001=3,28,0,22,0,0,0,0,A,67,0,0,0,0,0,0,A,69,0,0,0,0,0,0,A

[Tags]
01000003=0,Terminal cell,01000001

[CellTags]
36030=01000003
```

动作 `28` 在胜利前置位全局变量 `22`，房主继续时预选 `branch.map`。需要普通后继时，让另一条胜利触发器只执行 `67` 和 `69`，并保持变量 `22` 未置位。失败条件可安排 `68` 和 `69`，回房保留入口关。`8` 是该用例原版规则中 `Russians` 的国家索引，换用其他规则或国家时按地编结果填写。

### 从单机地图改造的检查清单

1. 在地编中复制地图，确认每条角色和附属 House 都在 `[Houses]` 注册，给角色预置单位、基地和地图同盟。
2. 写 `campaign.ini` 并检查路径，把 `PlayerN` 指到各关自己的主 House。为需要共享的附属 House 写对应的 `PlayerNControl=yes`。
3. 逐一检查原图中依赖单人所属方的触发器、增援和判胜条件。需要每个角色独立处理时，给各自的国家写对应条件。
4. 先从 LAN 大厅用主 House 或共享单位实际到达目标格，再验胜利、失败和重试。分支关分别核对变量置位、未置位和末关标记。
5. 准备地图副本时复制原文件。在改图或改清单后重新选关、验图并准备，再发起下一局。

可参考本仓库已使用的双关地图及角色配置。 [双关清单](../../tests/fixtures/vmce/lan_outcomes/campaign.ini) · [入口地图](../../tests/fixtures/vmce/lan_outcomes/outcome_stage01.map)

## 参考

### campaign.ini 键值

`[Campaign]` 段定义整个系列。

| 键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `Name` | CSF 标签 | 省略时用目录名 | 系列显示名。依次查询系列翻译表和当前游戏文本。 |
| `Entry` | 地图相对路径 | 必填 | 系列入口，须出现在关卡清单中，排在本系列首位。 |
| `ScenarioCount` | 正整数 | 必填 | 清单中的关卡数量。 |
| `Scenario1..ScenarioN` | 地图相对路径 | 必填 | 从 1 开始连续编号，每个路径唯一。 |

`[Scenario:<地图路径>]` 段定义这一关的角色和后继。

| 键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `PlayerCount` | 整数 `1..30` | 必填 | 可分配的主角色数，实际参战人数还受房规和连接席位约束。 |
| `Player1..PlayerN` | House 名称 | 必填 | 顺序绑定地图的主 House，各角色使用不同的 House。 |
| `NextScenario` | 地图相对路径 | 省略时取清单下一项 | 房主胜利后通常预选的关卡。 |
| `AltNextScenario` | 地图相对路径 | 省略 | 分支变量置位后的预选关卡。 |
| `EndOfGame` | 布尔 | `no` | 设为 `yes` 时结束系列，胜利继续后回房保留本关。 |

两段都可以填写房规。以下默认值在对应的下层配置也省略时生效。

| 键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `MinPlayers` | 正整数 | `1` | 开始所需的最少参战者，须小于或等于 `MaxPlayers`。 |
| `MaxPlayers` | 正整数 | `PlayerCount` | 参战者上限，最大为该关角色数。 |
| `AllowObservers` | 布尔 | `no` | 参战容量之后有剩余连接席时开放观察席。 |
| `AllowAlliesChange` | 布尔 | `no` | 允许仍在参战的真人玩家彼此改盟。 |
| `AutoEnemySelection` | 布尔 | `no` | 允许电脑在缺少当前敌人时使用普通多人模式的自动选敌步骤。 |
| `Difficulty` | 整数 `0..2` | `1` | 简单、普通、困难，对应触发器及 AI 难度。 |
| `LockDifficulty` | 布尔 | `no` | 将难度锁定为本关配置值。 |
| `AltNextGlobal` | 整数 `0..49` | `1` | 决定预选分支的全局变量编号。单关可覆盖系列编号。 |

`[Scenario:<地图路径>:<House 名称>]` 段定义对附属 House 的授权。

| 键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `Player1Control..PlayerNControl` | 布尔 | `no` | 向相应角色授予附属 House 的共享控制权。目标须在本关 `[Houses]` 中并位于主角色列表之外，角色编号须在 `PlayerCount` 内。 |

### 地图新增键值

| 地图位置 | 键或字段 | 说明 |
|---|---|---|
| `[Basic]` | `Name` | 关卡显示名。 |
| `[Basic]` | 人数、观察者、外交、难度、难度锁定和 `AutoEnemySelection` | 提供房规基值，可由清单覆盖。分支编号和续关目标在清单中设置。 |
| `[Basic]` | `TimerInherit`、`CarryOverMoney`、`CarryOverCap` | 控制本关接收倒计时和资金。资金沿清单中本次转场的主 House 对应表传递。 |
| `[Houses]` | `N=<House 名称>` | 登记地图中的主 House 与附属 House。 |
| `[<House 名称>]` | `Country`、`Allies`、`Credits` | 配置该 House 的国家、开局同盟和资金。 |

### 触发器格式

| 地图位置 | 键或字段 | 说明 |
|---|---|---|
| `[Triggers]` | `Owner`、`Easy`、`Medium`、`Hard` | 设置触发器所属国家和启用的难度列。 |
| `[Events]`、`[Actions]` | 数量与记录 | 行首数量对应行中实际的事件或动作记录数。 |
| `[Tags]`、`[CellTags]` | 标签和格子 | 把目标格关联到触发器。示例的 `36030` 对应 `(30,36)`。 |

其他地图片段及原版字段见地图格式文档。 [VERA 地图格式](../map-format.md)

### 故障排查

| 情况 | 可以检查 |
|---|---|
| 找不到 VMCE 或系列关卡 | 检查 VERA 的设置目录、`Maps/VMCE/<系列>/campaign.ini`、`Entry` 和 `ScenarioN`。到游戏日志查该系列的读取错误。 |
| 角色缺失或选图失败 | 核对 `[Scenario:<地图路径>]` 对应当前地图，`PlayerN` 和授权段所写 House 都已在 `[Houses]` 注册。检查 `MinPlayers <= MaxPlayers <= PlayerCount`。 |
| 客机不能准备 | 比对双方构建、清单和当前图的原始文件。房主换关后等待校验并手动准备。同关修改使客机重新校验，校验通过后恢复此前的准备状态。 |
| 观察者无法加入 | 确认关卡的有效房规为 `AllowObservers=yes`，且房间有足够的空闲连接席位。 |
| TAB 的同盟框无法点击 | 检查本关有效 `AllowAlliesChange` 及双方是否仍在参战。开启外交页可以查看地图预设关系。 |
| 开始后停在等待 | 若 GO 后卡载并出现「其他电脑并无回应」，点击确认。房主重新开放原关，正常客机经 LAN 列表重入并在同关验图后恢复 Ready，卡载客机的席位开放。若有人在首帧同步认证前离场，包括开场影片期间，也走这条恢复路径。 |
| House 全灭后任务继续 | 检查地图是否为该情形设置了结局触发器，再核对触发器使用的国家和难度列。 |
| 胜利后预选与设计不同 | 检查 `EndOfGame`、`AltNextGlobal`、变量是否先于胜利动作置位，以及后继目标是否列在清单中。 |
| 重试被取消 | 检查参战者是否已返回大厅，或当前关卡和清单是否在任务期间发生变化。恢复一致后回房重新验图和开局。 |

### 版本与存档兼容性

本指南的操作入口是 LAN。联机使用相同构建和相同的关卡文件。失败重试从本次开局的冻结配置重新加载，按关继续则使用回房后重新确认的配置。任务内存档属于后续阶段。 [LAN 房间同步约定](../Networking.md#authored-scenario-setup)

### 原版资源与许可

原版游戏资源由玩家自行准备。VERA20k 的安装、资源与许可证说明见项目 README。 [项目说明](../../README.md)
