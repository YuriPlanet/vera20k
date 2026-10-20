# Vera20k 开发参考手册（架构 / 结构 / 证据纪律）

> 定位：给**后期开发**用的"地图 + 红线"手册。读它能回答三件事——
> ① 代码在哪、各层干啥；② 一条命令/一帧是怎么流动的；③ 动手前必须守哪些纪律。
> 行号是勘察快照值（2026-10），改动后以实际文件为准，别拿它当绝对锚点。
> 权威契约永远是 `AGENTS.md` 与各模块头注释；本手册是它们的**导航层**。

---

## 0. 30 秒全景

Vera20k = **用 Rust 重写 Red Alert 2（Yuri's Revenge）的 `gamemd.exe` 引擎**，
目标 2 万单位 / 30 玩家确定性联机。它不是"重做"，而是**逐条对齐 native**：
每个数值、每个时序都要拿 native 二进制 / 零售并排游玩背书，否则记 residual，不许瞎编。

工程分两个 crate（见 `Cargo.toml` workspace）：

```
Cargo.toml  (workspace root)
├── src/Cargo.toml   ->  crate "vera20k-core"   确定性引擎内核（lib）
│     └─ 规则/地图/模拟/网络逻辑，全部在 sim 层确定性
└── (root) main      ->  crate "vera20k"         客户端瘦壳（app/bin）
      └─ winit + wgpu + 渲染/UI/音频，呈现"消费"内核的事实
```

**铁律（`src/architecture_guards.rs` 编译期强制 + `src/sim/mod.rs` 头注释）**：

```
依赖方向只许向下：  app/render/ui/audio/net  ──►  sim  ──►  rules/map/util
                    （上层）                     （内核）   （底座）
```

- **`sim/` 永远不许依赖 `net/`、`render/`、`ui/`、`audio/`、`app/`。**
  （锁步收敛测试故意放在 `src/net/lockstep_sim_convergence_tests.rs`，就是这条边界的活例子：
  它用 `net` 侧引 `sim`，绝不反过来。）
- `net/` 可以依赖 `sim/`（网络驱动模拟）。
- 呈现层（render/ui/audio）**只能消费** sim 输出的事实束，**不能**反写成模拟权威。

---

## 1. 顶层模块布局

```
src/
├── util/        底座：SimFixed 定点、native 坐标/数学、确定性哈希、parallel
├── assets/      资产解析：INI/调色板/动画/模型/声音，retail 语料认证
├── rules/       规则集：UnitType/BuildType/Weapon 等 native 结构读取
├── map/         地图：theater/桥/RMG(随机地图)/地形观察
├── sim/         ★ 确定性引擎内核（最大，~683k 行，详见 §2）
├── net/         联机：lockstep 命令队列、会话、checksum 收敛测试（依赖 sim）
├── app/         应用：window/match_runtime/input/scene（客户端编排）
├── render/      渲染：wgpu 场景/粒子/voxel/地形绘制
├── ui/          UI：sidebar/shell 菜单/光标
├── audio/       音频：SFX/主题
├── test_fixture.rs   Rust 侧读 golden JSON 的唯一入口（见 §6，禁 include_str!）
└── architecture_guards.rs  编译期源码扫描，强制上面那套依赖方向
```

> 记忆锚点：`sim` 683k 行（世界/战斗/移动三大件），`app` 194k，`render` 68k，
> `rules` 62k，`map` 61k，`net` 仅 2.2k（联机是最大缺口）。

---

## 2. sim 内核解剖（权威状态 + 相位化主帧）

一句话：**单一权威 `Simulation` + 按 native `Main_Tick` 顺序排相位的帧推进 + 事实束输出**。

### 2.1 子模块树（规模=近似 .rs 行数量级）

```
src/sim/
├── world/            52.9k  Simulation 编排：命令排程/帧推进/lifecycle/哈希/spawn/trigger 桥接
│     └── mod.rs      ~7.2k  ★ 主帧 advance_app_frame 在这
├── combat/            74.7k  目标选择/武器/AOE/开火门/伤害结算（含 damage/）
├── movement/          76.6k  地面寻路/速度斜坡/格子迁移/跳跃/特殊 locomotor（residual 最密）
├── production/        26.4k  建造队列/放置/经济/科技树/出售
├── superweapon/       20.0k  超级武器计时与效果
├── pathfinding/       17.7k  A*/区域连通/地形速度/平滑
├── team_script_vm/    12.6k  TeamScript VM
├── aircraft/          7.5k   飞行器任务状态机（攻击/守卫/返航/空投/侦察）
├── docking/           8.2k   维修船坞 + 机库降落
├── particles/        7.4k   烟/毒气/火
├── bridge_state/      5.6k   桥破损/修复/通行
├── miner/            4.6k   采矿状态机/入坞/矿石交付
├── transport_unload/  1.9k   运输卸载
└── (顶层散文件，无 ai//trigger//power/ 目录——那几块是顶层 .rs)
      game_entity.rs   实体模型  entity_store.rs  实体存储  command.rs  命令类型
      runtime.rs       SimRuntime 门面（生产帧入口唯一路径）
      trigger_runtime.rs   触发器（最大伴生测试群 ~24k）  team_script_vm.rs
      multiplayer_checksum.rs  零售逐帧同步校验  state_hasher.rs  world_hash.rs
      house_state.rs   每家钱包/建造/超武/统计  economy.rs  credit_income.rs
      ai_*.rs 一组      AI（基地建造/防御/组队），不是子目录
```

### 2.2 关键类型（带注释）

| 类型 | 位置 | 干啥 |
|---|---|---|
| `GameEntity` | `game_entity.rs:120` | 一个 unit/infantry/building/aircraft 就是一个它；核心字段恒在（stable_id/position/facing），子系统状态用 `Option<T>` 附属 record，**机制模块独占写、他人只读** |
| `EntityStore` | `entity_store.rs:47` | 按 `stable_id` 升序存 + 迭代（**lockstep 多机确定性关键**） |
| `Command` | `command.rs:612` | 游戏命令 payload 枚举；变体注释带 native 地址证据（如 `EventClass::Execute @ 0x4C6CB0`） |
| `CommandEnvelope` | `command.rs:838` | `{owner, execute_tick, payload}` = 命令排程单位 |
| `CommandRecord` | `command.rs:78` | native 十六进制记录编解码（replay/快照同字节通路） |
| `Simulation` | `world/mod.rs:1043` | 权威游戏状态所有者 |
| `SimRuntime` / `SimView` | `runtime.rs:59/83` | 生产帧事务门面 / 不可变呈现门面 |
| `HouseState` | `house_state.rs:393` | 每家的钱包/建造/超武/统计；`Simulation.houses` 用 `BTreeMap`（确定性遍历序） |
| `TriggerRuntime` | `trigger_runtime.rs:615` | 帧内触发器求值（Eq+hashable，参与世界哈希） |
| `SimFrameOutput` | `world/mod.rs:323` | 帧输出事实束（trigger/生死/fire/sound/multiplayer_checksum），呈现层只读它 |

### 2.3 一帧怎么流动（数据流，严格复刻 native `Main_Tick` 序）

入口链：
```
SimRuntime::advance_frame (runtime.rs:188)          ← 生产唯一路径
  └► Simulation::advance_app_frame (world/mod.rs:6722)
        └► advance_master_frame (world/mod.rs:6824)  ← 真正的 Main_Tick 形态
```

相位顺序（`advance_master_frame`，**别乱改顺序**）：
1. `execute_tick = session.tick + 1` 先算；native 帧计数**迟提交**（帧尾 +1），全相位读同一帧 N
2. `apply_due_frame_ingress_commands`（`command_schedule.rs:575`）→ `admit_frame_commands`（:120）
   —— 先注入前帧完成的建造 PLACE 事件，再按 **House 序规范 dispatcher** 接纳本帧 batch
3. `sort_display_ground` → `poll_triggers_for_master_frame`（**触发器先于逻辑向量**）→ shroud
4. 相位 1–6：光照迁移 / ore / bomb / kamikaze / laser / 超武 / 辐射衰减 / TeamScript
5. `advance_live_object_pass`：live 主扫，**每个对象 AI 先于自身运动（interleaved）**，新建子弹/anim 入本帧扫描
6. 相位 7：`production::step_factories`（按 `insertion_seq` 时序扣费）→ 船坞 → `refresh_fog`
7. `run_late_region`：House AI/败局尾部、帧 commit、**唯一** pending-delete drain（复刻 native 帧尾一次 `ProcessPendingDelete`）

输出：`collect_frame_output`（:6775）把帧内暂存一次性 `mem::take` 进 `SimFrameOutput`；
`TickResult.state_hash` 在选中帧/终结帧计算。

---

## 3. 确定性六不变量（改 sim 前必读）

1. **双钟帧计数**：`ScenarioSession` 持 monotonic `tick`（命令排程用 ordinal）+ wrapping `binary_frame`（native 帧语义），**迟提交**统一 +1。
2. **三路 RNG 分离**（`world/mod.rs:1074–1097`）：`scenario_rng`（**入快照/哈希**）、`main_rng`（进程全局，**不入**）、`mapgen_rng`（地图专用）。`rng_routing_tests.rs` 钉死路由。
3. **`SimFixed = I16F16` 定点**（`util/fixed_math.rs:24`）：sim 默认数型；少数机制（bounce/voxel_anim）保留 native 浮点宽——靠"保留运算序+位宽+RNG 序"保确定性，**别拿 x87 模拟除非有 native 证明**。
4. **升序迭代**：实体按 `stable_id`、House 按 `BTreeMap`、工厂按 `insertion_seq`。
5. **固定算法状态哈希**：`state_hasher.rs`（黄金比奇数乘、四 lane 分道混大网格）→ `Simulation::state_hash`。
6. **并行只走无副作用折叠**：`parallel.rs` per-item 独立摘要再有序合并，结果与线程数无关。

---

## 4. 证据三层模型（`AGENTS.md` L48–58，动手的"刻度尺"）

| 级别 | 含义 | 证据来源 |
|---|---|---|
| **Native established** | native 行为被坐实 | 指令/调用者/数据字节，或零售并排游玩观察 |
| **Rust regression tested** | Rust 实现被回归测住 | 具名实现级测试 |
| **Parity demonstrated** | native↔Rust 对齐 | gamemd 可执行对照或穷尽证明（**按声明的覆盖边界，样例≠整机制**） |

配套铁律：
- **Never invent**（L40）："Never invent offsets, identities or behavior."
- golden **只能**来自 native 执行/零售字节，**绝不**来自手算或既有 Rust（L56–64）
- 误读易扩散处才钉 golden（数值核/RNG 序/x87 精度）；纯控制流可停在指令级阅读 + 并排游玩（L60–68）
- 每 PR 要声明每条结论达到哪一级（L70–76）

---

## 5. native 取证链路（怎么"看懂" gamemd.exe）

三条腿，文档在 `docs/ghidra-workflow.md` + `tools/native_oracle.md` + `tools/native_inspect.md`：

```
gamemd.exe（零售/Steam，SHA-256 白名单；仓库不分发可执行）
   │  ① 静态取证   tools.native_inspect（不执行/不写 golden）
   │  ② 有界执行   tools.native_oracle runner（Unicorn 2.1.4，默认 --check 只读）
   │  ③ 交互分析   Ghidra（MCP 5.14.2 + Ghidra 12.1.2）
   ▼
tools/**/*_oracle.py  →  *.json golden + *.meta.json sidecar（记录可执行 SHA-256/假设/替换）
   ▼
Rust #[test] 经 src/test_fixture.rs::json() 在【运行时】读 golden（禁 include_str!/include_bytes!）
```

**关键约定：**
- **白名单**（`tools/native_oracle.py:35–41`）：零售 `1cdd1180...` / Steam `3e81a617...`，
  `_verified_image()` 对整文件算 SHA-256，不在名单即抛错。Steam 证据**另存路径**（`--output logs/steam/`），不覆盖旧 golden。
- **环境变量**：`VERA20K_GAMEMD_EXE`（显式指向、优先、不静默回退）或 `RA2_DIR`（零售目录）。
- 依赖装：`python -m pip install -r tools/requirements-test.txt` + `pip install unicorn==2.1.4`。
- ⚠️ capstone/unicorn 在本机装在 `E:\WorkBuddyData\.workbuddy\binaries\python\envs\native-inspect\`，
  跑 native_inspect 要用它自己的 `Scripts\python.exe`（系统 python 没装）。

`native_inspect` 六个子命令（**精确调用**，`--start/--bytes` 可显式限范围）：
```sh
python -m tools.native_inspect sections                       # 每段 VA/偏移/长度/标志/整段 SHA-256
python -m tools.native_inspect read 0x8610B4 --bytes 16       # 读文件支撑的原始字节
python -m tools.native_inspect disasm 0x773070 --bytes 96     # 反汇编（要求落在单一文件支撑段）
python -m tools.native_inspect calls 0x773070 [--include-jumps]  # 近立即数 call 目标（间接记 unresolved）
python -m tools.native_inspect field 0x68A --width 1          # 字段访问宽度候选（LEA 单列）
python -m tools.native_inspect find-bytes "32 c0 c2 04 00"    # 全文件支撑段扫字节
```
> 每个 JSON 包记 schema/native SHA-256/Capstone 版本/完整请求。`read`/`disasm` 字节数必须完整落在
> **单一文件支撑段**内（头部/间隙/BSS/跨段都失败）——native_oracle 明说 BSS 不是文件内容。

---

## 6. 工具链 / 证据资产清单（`tools/`，754MB）

- **Oracle 子目录（~25 套件）**：`spatial_oracle`（最大，数百 json）、`projectile_oracle`、
  `rules_oracle`、`storage_oracle`、`input_oracle`、`sidebar_oracle`、`procedural_drawing_oracle`、
  `garrison_oracle`、`voxel_oracle`、`terrain_draw_oracle`、`palette_oracle`、`rmg_oracle`、
  `mouse_oracle`、`free_radar_oracle`、`bridge_click(_state)_oracle`、`exact_shell_ui_matrix`、
  `ghidra_compare` / `ghidra_pass`（Ghidra 批量回放）、`shell/tactical_certification`、`asset_browser`。
- **顶层扁平 golden**：`ai_*_oracle`、`can_build_oracle`、`house_money_oracle`、`superweapon_oracle` 等
  （`*.json` 产物 + `*.meta.json` 溯源 sidecar）。
- **基础设施**：`cargo_run.py`（Cargo 串行化）、`prepare_checkout.py`、`run_tests.py`（工具测试入口）、
  `native_oracle.py` / `native_inspect.py`（取证）、`scale_map.py`（500×500 规模基准）。

三类资产区分：**黄金 JSON**（Rust 运行时读）/ **Python 捕获脚本**（Unicorn 有界执行，默认 `--check`）/ **原生取证**（无执行）。

---

## 7. residual 纪律（VERA 与 native 的已知缺口登记）

- **residual ≠ bug 单**，是"原生有、我没移植/简化/休眠"的**显式一行登记**。
- 存放：**没有单一 global 文件**，分布式在 ①模块级 `RESIDUAL:` doc 注释（movement/world 最密，全仓 645 处）②PR 描述。
- 写法（AGENTS.md L43 / L177–179 / L193）：命名 **触发条件 / 效果 / 频率 / 下游风险**；
  "原生正文存在的地方发明行为，是一条带理由的 recorded residual，**绝不许是静默默认**"。
- 玩家看不见的 1–2px/小色差只记一行，不另开调查。

---

## 8. 构建 / 运行 / 测试约束

- **项目原本不是 git 仓库**；`tools/cargo_run.py` 靠 git（`rev-parse`/`ls-files`）协调多 worktree 共享锁。
  **本 checkout 现已 init git（本地 only，无 remote）**。约定：Cargo 一律 `python -m tools.cargo_run -- <args>`；
  新 worktree 先 `python -m tools.prepare_checkout`。
- **`test-support` feature**（`src/Cargo.toml:27–32`）：让客户端 crate 的测试也能到内核侧被测钩子；
  纯测试模块仍 `#[cfg(test)]`；锁步代码 `#[cfg(any(test, feature="test-support"))]`。
- **`#[ignore]` 测试默认不跑**，分五类：`slow` / 需零售 `RA2_DIR` / 需 GPU(wgpu) / 需录制语料 / 需零售音频。
  只有你改的东西恰好是它们覆盖范围时才 `-- --ignored`（AGENTS.md L240–241）。

跑子集标准姿势：
```sh
# 聚焦内核某模块
python -m tools.cargo_run -- test -p vera20k-core --lib sim::world::turret_rotate_sound::tests::
# 聚焦客户端
python -m tools.cargo_run -- test -p vera20k --lib app::input::cursor::tests::bunker_
# 只跑被忽略的
python -m tools.cargo_run -- test -p vera20k-core --lib -- --ignored
# 零售 INI 场景
VERA20K_REQUIRE_RETAIL_INI=1  python -m tools.cargo_run -- test -p <crate> --lib <module>::
# 工具侧
python -m tools.run_tests          # 全套（装好 requirements-test.txt）
python -m tools.run_tests --list
# 发行形态（无 dev chord）
python -m tools.cargo_run -- build --locked --release --no-default-features --bin vera20k
```
PR 就绪 = 改动模块 + 直接消费者聚焦测试 + 零售 `ini/`（`VERA20K_REQUIRE_RETAIL_INI=1`）；
**全 workspace 全量 `cargo test` 默认不需要**，被要求才跑。

---

## 9. 后期开发速查（动手前对号入座）

| 你要做的事 | 走这条 | 别踩 |
|---|---|---|
| 加一个新命令 | 在 `command.rs` 加变体（注释带 native 地址证据）→ `command_schedule.rs` 排程 → 落进 `SimFrameOutput` | 别凭空编 opcode，先 `native_inspect` 取证；native 本地不同步的就记 residual |
| 补 native 编码（如未接 opcode） | 先出**取证报告**（`docs/native_opcode_dispatch.md` 是范式）→ 单独授权后才写 encode/decode | "research 不授权实现"；alias 要成组（0x04≡0x05、0x2a≡0x2b≡0x2c） |
| 改帧相位/加机制 | 严格按 §2.3 顺序插，触发器必须先于逻辑向量 | 乱改相位序 = 直接 desync |
| 数值计算 | 默认 `SimFixed`；要 native 浮点宽度时保留运算序 | 别引入 x87 模拟除非 native 有证 |
| 确定性验证 | `multiplayer_checksum.rs` + 双实例逐帧 checksum（`lockstep_sim_convergence_tests.rs`） | `sim/` 别 import `net/`（用 net 侧测试引 sim） |
| 留缺口 | 写一行 `RESIDUAL:` 注释（触发/效果/频率/风险） | 绝不静默默认、绝不发明 native 行为 |
| 跑测试 | `python -m tools.cargo_run -- test -p <crate> --lib <module>::` | 别直接裸 `cargo`（多 worktree 锁要经 cargo_run） |

---

## 附：一句话总结

Vera20k 是"**单一权威 Simulation + 相位化主帧 + 事实束输出**"的确定性内核，
确定性靠双钟帧计数、三路 RNG 分离、`SimFixed` 定点、升序迭代、固定哈希共同钉死；
每一处 native 对齐都必须过**三层证据刻度尺**，对不齐的地方以 `RESIDUAL:` 显式登记、
**绝不发明**——这就是整个工程的地基。
