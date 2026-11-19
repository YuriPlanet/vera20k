---
layout: default
title: 模式底座路线图审查与 Phase 0
lang: zh-CN
---

# 模式底座路线图审查与 Phase 0

以 `307c7bd6f` 为审查基线。路线图按入口、玩法、执行和本地视角拆分的方向可以保留。
原清单把零售 `GameMode`、锁步执行和在线连接说成同一条轴，也把存档格式变化混进了基础重构。
这些假设会影响原版遭遇战、历史存档和联机回放，需在后续迁移前修正。

## 本次执行范围

Phase 0 涵盖 R01–R03，复用现有启动和会话所有者，提供可在生产路径读取的规则、执行和本地视角投影。
零售规则与拓扑由已有启动事实推导，合作专有规则继续取自其模式策略行。
保留现有存档与回放字段布局及哈希输入顺序。新字段、剧本覆写、权限表和测试构造器随首个实际消费者进入对应切片。
[启动交接](../../src/app/match_bootstrap.rs) · [会话状态](../../src/sim/scenario_session.rs) · [模式策略](../../src/skirmish_modes.rs)

| 步骤 | 修订后的完成标准 |
| --- | --- |
| R01 | 生产启动区分单人战役、离线遭遇战、原版合作与记录的会话模式。`LoadingStartup` 保留启动权威，`ScenarioSession` 提供规则与执行查询。 |
| R02 | 保持已有存档布局指纹和回放字段，并验证保存、读取和恢复后的哈希。本地视角切换保持共享哈希，须保留原版对当前阵营的玩法语义。 |
| R03 | 记录固定地图、种子、帧数和参数的 `--release` 20,000 单位 / 30 阵营数据，连同终局哈希供后续对照。零售三形态启动与大规模性能分别验收。 |

## 需要修正的架构边界

- **规则、锁步语义与活跃传输应区分。** 离线遭遇战与单人战役均在本机执行，前者的零售模式为 `5`，后者为 `0`。零售飞机迷雾禁入和 `Techno` 发现历史只读模式 `0`。LAN/WOL 回放保留记录的命令队列语义，播放由回放边界提供交换结果。
  [会话模式](../../src/sim/scenario_session.rs) · [飞机禁入](../../src/sim/world/object_entry.rs) · [发现历史](../../src/sim/world/lifecycle.rs) · [回放边界](../../src/sim/replay.rs)
- **零售玩法谓词与命令授权各有所有者。** `HouseState::is_controlled_by_human` 还驱动建造蓝图与 AI，`event_dispatch_eligible` 在所有模式接收 `IsHuman || PlayerControl`。可共享命令鉴权的执行入口，跨阵营委托的范围要按命令和资金归属核实。
  [House 谓词](../../src/sim/house_state.rs) · [命令执行](../../src/sim/world/world_commands.rs)
- **原版合作复用多人构造路径并拥有专属策略。** 模式行决定人数、观战、结盟与共享胜利，续关由现有应用会话持有。迁移时从现有模式行投影仿真所需字段，避免重复定义合作预设。
  [多人模式行](../../src/skirmish_modes.rs) · [共享胜利](../../src/sim/world/house_defeat.rs) · [联机相位](../../src/app/match_runtime/session.rs)
- **确定性由状态所有者和恢复顺序保证。** `fold_identity()` 已纳入零售模式与多人模式类，重复折叠派生策略会改变现有哈希。位置编码依赖布局指纹拒绝其他布局，JSON 回放另有启动版本与初始哈希校验；新持久化规则源应分别决定存档、回放和哈希的兼容方式。
  [会话哈希](../../src/sim/scenario_session.rs) · [存档布局](../../src/sim/snapshot.rs) · [回放格式](../../src/sim/replay.rs)
- **本地视角与对局席位各有生命周期。** `observer_houses` 来自各端共有的房间席位，也供共享 AI 决策使用。当前本机阵营、自己的发令来源与跟随视角影响本机路径。已有视角存档还持有书签、选区与光标等展示状态，后续复用时要逐字段确认归属与恢复次序。
  [会话视角与观战席位](../../src/sim/scenario_session.rs) · [已存视角](../../src/sim/snapshot/saved_local_view.rs)

## R04 及以后拟议验收门槛

| 阶段 | 进入实现前应明确的条件 |
| --- | --- |
| R04–R07 | 按一条可验收的行为链迁移。逐处区分零售玩法、构造、锁步执行及本地投影，保留离线遭遇战与单人战役的反例。原清单的 `206 + 45` 与分桶合计 `234` 存在 17 项差额，先以固定提交、检索命令和边界清单对齐，再做静态棘轮。 |
| R08–R10 | 先定共享同一 `House` 的多个来源是否分别获得跨阵营权限。当前设想的 `House × House` 表会为这些来源授予相同权限。若授权按角色，索引需保留来源维度。按现有接收次序执行同帧命令，并核对执行内闸、炸弹和墙的权威归属。 |
| R11–R12 | 先让原版遭遇战与合作通过现有回放入口重建，再接入剧本蓝图。携带 `ControlGrantTable` 的剧本蓝图依赖 R08，整项与 Phase 2 并行的假设需拆开。历史 JSON 应分别验证可读、可重建和哈希可验，三项均成立才算支持旧回放。 |
| R13–R15 | 从现有 `MatchSession` / `NetworkPhase` 和观战入口扩展。LAN 当前同时传输已应用房速 `HostRoom.current_speed` 与设置值 `options.game_speed`，迁移后要保有两者。分别验证 LAN 与在线大厅的同构建、旧服务和回滚路径。 |
| R16–R18 | `feature/vmce-v2` 作为历史归档。主线联机战役可独立定义清单、存档和房间协议。主线既有原版模式的存档、回放和联机兼容仍按现有契约验收。 |

[命令派发](../../src/sim/world/world_commands.rs) · [回放执行](../../src/sim/replay.rs) · [LAN 房间选项](../../src/app/lan_service/options.rs) · [联网兼容性](../Networking.md)

`sim-bench march` 主要衡量移动与寻路。R03 若以 `match` 作为门槛，应选有至少 30 个可用出生点的地图，并记录实际席位和单位预算。
固定输入和同一哈希版本用于等价改动的前后比较。零售对照应另覆盖实际被触及的加载或玩法路径。
[基准场景与参数](../../src/bin/sim-bench.rs) · [项目验证契约](../../AGENTS.md)

## R03 性能基线

2026-11-19，Windows x86-64，Intel Core Ultra 7 255H。本次使用原版规则、30 个参战阵营（29 个电脑）和 20,000 单位预算，开局共有 21,060 个存活实体。

| 固定输入 | 模拟帧 | 预热后平均帧耗时 | 全程 P95 | 终局共享哈希 |
| --- | ---: | ---: | ---: | --- |
| `match`、30 出生位、种子 `0x5EED0001`、`--client-timing` | 300 | 63.65 ms（第 151–300 帧） | 77.11 ms | `a442638ed2f52f75` |

这次压力样本的帧耗时高于 22 ms 仿真节拍。后续等价迁移用同一场景、种子和硬件对照终局哈希与帧耗时。

```powershell
python -m tools.scale_map target/scale/mode-core-match30.map --starts 30
python -m tools.cargo_run -- build --release -p vera20k-core --bin sim-bench
$bench = python -m tools.cargo_run --resolve sim-bench --profile release
$map = (Resolve-Path target/scale/mode-core-match30.map).Path
& $bench --ra2-dir "$env:RA2_DIR" --map "$map" --scenario match --units 20000 --houses 30 --computers 29 --seed 1592590337 --ticks 300 --client-timing
```

## R04–R05 首个切片

加载期使用 `BootstrapRuleProfile` 表达 AI 触发启用、任务组科技阈值和预置木箱规则。客户端依据已接纳的地图构造路径，无头加载依据现有遭遇战构造配置，随机地图沿用早期 `Full_Init` 的入口。原版预置木箱只在零模式保留，离线遭遇战和 LAN/WOL 均在构造覆盖物前跳过。
[AI INI 读取](../../src/rules/team_ai_ini.rs) · [预置覆盖物](../../src/map/authored_overlay.rs) · [启动加载](../../src/app/loading/init.rs)

活跃小队继续由 `TeamScriptVm` 持有。普通创建按战役全局或非零模式逐阵营执行 `Max=`，AI 触发候选筛选继续逐阵营检查，触发器建队与增援沿用已有豁免。非零模式下，从未满编的空队在严格超过 `DissolveUnfilledTeamDelay=` 后解散。满足离场条件时，事件 23 仍先于销毁。
[小队存续与创建](../../src/sim/team_script_vm.rs) · [AI 触发筛选](../../src/sim/ai_team_creation.rs)

`Paranoid`、基地建造、`DoubleOwned=`、威胁与经济决策拥有各自的原版调用条件，按各自行为链继续迁移。

在 `48e858260` 和首切片上分别按 R03 的同一组参数运行 300 帧，终局哈希均为 `a442638ed2f52f75`。两次运行的预热后平均耗时分别为 77.34 和 61.02 ms，全程 P95 分别为 93.05 和 74.39 ms。耗时各取一次样本。

### AllowWin 胜利门控

实时 House 更新和挂起帧的存档恢复校验都通过 `HouseState::pending_outcome` 使用同一条规则。结果计时器到期后，单人战役的胜利仍受正数 `AllowWin` 计数阻塞。非零模式的胜利和各模式的失败只检查原有结果计时器。地图 TagType 注册和活动 Tag 析构继续分别持有计数与 60 帧计时器写入。
[House 判定](../../src/sim/house_state.rs) · [挂起帧恢复](../../src/sim/world/master_frame.rs) · [Tag 生命周期](../../src/sim/trigger_runtime.rs)
