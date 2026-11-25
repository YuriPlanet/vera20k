# Control Bench：控制点基准 mod

一个 `rules.wasm`，尽量覆盖 RFC 0001 第一阶段的全部控制点。每个控制点第一次确认生效时，
日志里写一行 `DEMO-OK <标记>`；全部到齐后再写 `DEMO-OK bench.all`。它是测试基准，
不是玩法 mod：自己在玩家出生点附近生成单位并自己指挥，所以一局无人操作、对一个电脑的
遭遇战就够了（XMP03T4，约 4500 帧全部到齐）。

标记的意思是“生效”，不只是“事件来了”：能便宜地验证效果的都验证了（读回数值、下一帧
看位置/血量/任务、看随后的事件）。答错时写 `BENCH-FAIL <标记>: <原因>`。每 300 帧写一行
`BENCH frame N: 已到/总数`。

## 运行

```sh
cd sdk/examples/control-bench && cargo build --release
# 在引擎仓库根目录：
VERA20K_TEST_INI_DEMOS="<wasm>|sdk/examples/control-bench/rulesmd.ini|bench.all|6000" \
RA2_DIR=<原版目录> python3 -m tools.cargo_run -- test --locked -p vera20k-core --lib \
  ini_demo_mods_reach_their_marks -- --nocapture
```

只填 `bench.all` 时，缺哪个标记要看日志；把各区域 `MARKS` 里的名字全部列出（逗号分隔），
失败信息会直接点名缺的那个。

## 分区（各占一个文件，互不干扰）

每个区域在出生点外自己的一角（`arena`）摆自己的单位，只对自己的对象出手；触发器键按
百位分区，`on_events`/`on_decide` 按键路由。

| 区域 | 标记数 | 内容 |
| --- | --- | --- |
| `basics` | 75 | 宿主调用、各名字种类的 `lookup`/`name_of`、对象/房屋/类型/单元格/全局属性读取、视角读取、`set` 写入（血量、所属、等级、任务、弹药、上线）、全部已接线的修饰器（火力、护甲、速度、射速、射程、暴击、不死、隐形、强制显形、禁用武器、免疫标志、转向速度 ROT）及过期/分组/查询、声明变量、各种 `find`、INI 注册（6h） |
| `triggers` | 94 | 触发器生命周期与各种绑定、投递事件（受伤、击杀、开火、计时、出生、乘客上下、易主、心控、超武、修饰器事件、模块事件）、原版对象/房屋/逻辑/单元格事件、各种轮询、过滤子句、立即动作与连锁、几乎全部原子动作和几个原版动作、房屋命令及 `Commanded`/`OrderRefused`、控制域认领、INI 触发器 |
| `combat` | 10 | `TakeDamage` 决策与事件、`DamageIncoming`、`FatalDamage`（泰矿残留位）、`Fire` 射击计划、`FireAllowed`、`Retaliate`、`Launch`、`ProjectileFrame`、`CrushAllowed` |
| `movement` | 13 | `ProductionAllowed`、`ProductionQueued`、`CommandAllowed`、`DestinationSet`、`HarvestCell`、`DeployAllowed`、`IdleMission`、`MissionStep`、`MoveFrame`、`PathFound`、`ApproachPoint`、推动、`SetFacing` |

## 没覆盖的点

- `Defeat`：只有房屋一无所有时才会问；场景里两个房屋整局都要用。
- `EndFor`：会直接结束这局。
- `lookup`/`name_of(TeamType)`：XMP03T4 没有队伍类型，`name_of` 一个不存在的 ID 会停掉模块。
- 援军与触发器造出的建筑的 `Birth`：地图没有队伍类型和空闲路点。
- `SuspendProduction` 命令：会停掉电脑整类生产，`movement` 依赖电脑的生产。
- 引擎还没读取的属性（`ControlGroup`、`GameMode`、成员的 `Team`/`Tag`、`Cloaked`、
  `NearbyEnemyCount` 等）。
- 暴击与 `Unkillable` 只在 Phobos 构建里生效；非 Phobos 构建里标记确认的是“正确地答
  `Unsupported`”。
- `on_rules`：规则阶段的钩子不在这个测试的路径上（`siege-doctrine` 覆盖它）。

## 基准发现的引擎问题

- 站立的步兵部署（`ToggleInfantryDeploy` → `Mission_Unload`，`src/sim/deploy.rs`
  `infantry_mission_unload`）不问 `DeployAllowed`，电脑 AI 的自动部署
  （`techno_ai/mission_handlers.rs`）也不问；只有行进中步兵的待定部署、简单部署者和 MCV 会问。
  所以基准用攻城直升机测部署。
