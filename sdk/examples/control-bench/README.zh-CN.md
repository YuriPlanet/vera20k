# Control Bench：控制点基准 mod

一个 `rules.wasm`，尽量覆盖 RFC 0001 第一阶段的全部控制点。每个控制点第一次确认生效时，
日志里写一行 `DEMO-OK <标记>`；全部到齐后再写 `DEMO-OK bench.all`。它是测试基准，
不是玩法 mod：自己在玩家出生点附近生成单位并自己指挥，所以一局无人操作、对一个电脑的
遭遇战就够了。地图是 `ControlBench.map`：原版 XMP03T4 并进本目录的 `map.ini`（两个队伍类型，
原图没有）。各区域的标记到齐后（最迟第 4800 帧，届时用 `BENCH-MISSING` 列出各区域还缺的标记），收尾（`finale`）暂停再恢复电脑的生产、让电脑战败、
宣布玩家获胜，最后才写 `bench.all`。

标记的意思是“生效”，不只是“事件来了”：能便宜地验证效果的都验证了（读回数值、下一帧
看位置/血量/任务、看随后的事件）。答错时写 `BENCH-FAIL <标记>: <原因>`。每 300 帧写一行
`BENCH frame N: 已到/总数`。

## 运行

```sh
cd sdk/examples/control-bench && cargo build --release
# 在引擎仓库根目录：
VERA20K_TEST_CONTROL_BENCH=<wasm 绝对路径> \
RA2_DIR=<原版目录> python3 -m tools.cargo_run -- test --locked -p vera20k-core --lib \
  control_bench_reaches_every_mark -- --nocapture
```

测试按真实 mod 的方式加载：在临时目录里做一个 mod 文件夹，放入 `rules.wasm`（这个模块）、
`mod.ini`、`rulesmd.ini`（原版 `rulesmd.ini` 并进本目录的 `rulesmd.ini`：mod 的 `rulesmd.ini`
会整个顶替原版的，所以要并进完整的一份）和 `ControlBench.map`，再选这个 mod 开一局。
这样模块的 `vera_init` 和 `on_rules` 在规则处理时运行，INI 触发器来自 mod 的 `rulesmd.ini`。
默认运行 6000 帧，可用 `VERA20K_TEST_CONTROL_BENCH_FRAMES` 调整。模块要一直运行到最后并写出
`DEMO-OK bench.all`。只要日志中出现 `BENCH-FAIL` 或 `refused`，测试就会失败，即使后面集齐了标记。
失败信息会列出相关日志，以及最近一次 `BENCH-MISSING` 报告中仍然缺少的标记。

## 分区（各占一个文件，互不干扰）

每个区域在出生点外自己的一角（`arena`）摆自己的单位，只对自己的对象出手；触发器键按
百位分区，`on_events`/`on_decide` 按键路由。

| 区域 | 标记数 | 内容 |
| --- | --- | --- |
| `rules`（`lib.rs`） | 4 | 规则阶段：`vera_init` 先于 `on_rules` 运行、`Rules::get` 读原版键（与类型属性一致）、规则阶段的类型名和类型属性、`Rules::set` 改 `YTNK` 的 `Cost`，对局里该类型读到新值 |
| `basics` | 86 | 宿主调用、各名字种类的 `lookup`/`name_of`、对象/房屋/类型/单元格/全局属性读取、视角读取、`set` 写入（血量、所属、等级、任务、弹药、上线）、全部已接线的修饰器（火力、护甲、速度、射速、射程、暴击、不死、隐形、强制显形、禁用武器、免疫标志、转向速度 ROT）及过期/分组/查询、声明变量、各种 `find`（含 `Types` 范围）、INI 注册（6h）；第二轮补的属性：类型的 `Name`、`Points`、`Buildable`，全局 `GameMode`，对象的 `Cloaked`、`NearbyEnemyCount`、`TargetInRange`、`CanFireAtTarget`、`DoorOpen`、`Animating` |
| `triggers` | 99 | 触发器生命周期与各种绑定、投递事件（受伤、击杀、开火、计时、出生、乘客上下、易主、心控、超武、修饰器事件、模块事件）、原版对象/房屋/逻辑/单元格事件、各种轮询、过滤子句、立即动作与连锁、几乎全部原子动作和几个原版动作、房屋命令及 `Commanded`/`OrderRefused`、控制域认领、INI 触发器、队伍类型的 `lookup`/`name_of`、援军（原版动作 7、80）、超时空援军（107）和触发器造出的建筑（125）的 `Birth` |
| `combat` | 10 | `TakeDamage` 决策与事件、`DamageIncoming`、`FatalDamage`（泰矿残留位）、`Fire` 射击计划、`FireAllowed`、`Retaliate`、`Launch`、`ProjectileFrame`、`CrushAllowed` |
| `movement` | 17 | `ProductionAllowed`、`ProductionQueued`、`CommandAllowed`、`DestinationSet`、`HarvestCell`、`DeployAllowed`（攻城直升机）、站立步兵的部署、基地车的部署和展开出的建造厂的 `Birth`、建筑被毁后幸存者的 `Birth`、`IdleMission`、`MissionStep`、`MoveFrame`、`PathFound`、`ApproachPoint`、推动、`SetFacing` |
| `finale` | 6 | 前面全部到齐后运行，最迟第 4800 帧开始（每步有时限，约 1050 帧内走完）；开始后各区域的逐帧工作、事件和决策都不再执行：认领电脑的 `Production` 控制域（电脑 AI 不再开工，生产走房屋命令），`QueueProduction` 一个电脑没在造的类型，开工后下 `SuspendProduction`（工厂进度停住）再 `QueueProduction` 同类型（进度继续），之后放回认领（电脑 AI 用工厂建筑自己的工厂生产，`SuspendProduction` 只暂停房屋的工厂，所以要先认领）；移除电脑的全部对象，`Defeat` 先否决一次（之后几帧仍未战败）再放行（读到 `IsDefeated`）；`EndFor` 宣布玩家获胜（调用成功后记标记），之后模块继续运行 30 帧 |

## 没覆盖的点

共 222 个标记，约 2900 帧全部到齐。剩下没做的都是有意不做：

- `ControlGroup`：只存在于客户端，放进模拟会导致不同步。
- 对象的 `Team`/`Tag` 引用：RFC 让它们用共享的创建计数编号，而引擎里队伍有自己的计数、
  标签是存储下标，真要做得改存档格式。现在答 `Unsupported`；读得到时记
  `triggers.prop-team`，不计入总数。
- 伪装成地形的幻影坦克的 `DisguiseType`：没有地形类型的名字种类。
- 房屋的 `Name`（玩家名不是模拟状态）、写 `DoorOpen`/`Animating`（是客户端显示）。
- 撤销部署不问 `DeployAllowed`：RFC 4.5 只规定了部署。
- 暴击与 `Unkillable` 只在 Phobos 构建里生效；非 Phobos 构建里标记确认的是“正确地答
  `Unsupported`”。
- 载具被毁后乘员的 `Birth`：引擎会发，但基准和引擎测试都还没覆盖（建筑幸存者已覆盖）。

## 基准发现并已修好的引擎问题

- 站立的步兵部署（`Mission_Unload`）、电脑 AI 的自动部署和接近目标时的部署都不问
  `DeployAllowed`；现在所有步兵部署都走同一个入口 `infantry_request_deploy`。
