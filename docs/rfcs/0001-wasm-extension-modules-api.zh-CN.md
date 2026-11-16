# RFC 0001 附录：API 清单

[English](0001-wasm-extension-modules-api.md) · **简体中文**

本附录是 [RFC 0001](0001-wasm-extension-modules.zh-CN.md) 的一部分，使用同一个版本号（0.0.6）。两个文件在同一次提交中修改。本清单与 RFC 正文不一致时，以 RFC 正文为准。本附录的中英文两个版本一起修改；两者不一致时，以英文版为准。

本清单列出 API 将提供的每个函数、事件、约束条件、效果和覆盖点，每一项由哪段引擎代码负责，以及每个想法的来源。它不决定版本 1 包含哪些内容：那是 RFC 的待定问题 13。来自调研的条目注明了所依赖的提议编号，以及 RFC 0.0.5 是否采纳，见[提议的 RFC 修改](#提议的-rfc-修改)。

所有文件路径都相对于仓库根目录。行号有意省略，因为它们会变。代码事实来自 `main` 的 `16f18f4b7`。

## 目录

- [约定](#约定)
- [1. 生命周期导出](#1-生命周期导出)
- [2. 事件](#2-事件)
- [3. 约束条件](#3-约束条件)
- [4. 动作](#4-动作)
- [5. 持续效果](#5-持续效果)
- [6. 标记](#6-标记)
- [7. 查询](#7-查询)
- [8. 覆盖点](#8-覆盖点)
- [9. AI](#9-ai)
- [10. 规则访问](#10-规则访问)
- [11. 客户端 API](#11-客户端-api)
- [引擎中的缺口](#引擎中的缺口)
- [提议的 RFC 修改](#提议的-rfc-修改)

## 约定

### 签名

签名用 TypeScript 书写，同时也能用 AssemblyScript 编译（RFC 5a）。Rust SDK 提供同样的函数，名字用蛇形命名。包名和类名和 RFC 中一样只是占位。

每个签名都遵守共同子集的规则：

- 不用 `any`；
- 除 `T | null` 外不用联合类型；
- 回调不捕获外部变量：每个回调都是顶层函数；
- 不用 `try`/`catch`：错误作为返回值（见[错误](#错误)）；
- 把多个值组合在一起的参数用类，不用对象形状。

在 AssemblyScript 中，`T | null` 只能用于类；缺失的句柄写作 `NO_HANDLE`（见[句柄](#句柄)）。缺失的整数写作 `-1`（或其他写明的哨兵值），社区表格也是这样做的（“负数表示无”）。

### 值类型

| 类型 | 含义 | 说明 |
| --- | --- | --- |
| `i32`、`u32`、`bool` | 普通整数和标志 | 模拟 API 中没有浮点值（RFC 6a）。 |
| `Fixed` | 定点数，以原始 `i32` 表示 | 引擎的 `SimFixed` 是 `I16F16`（`src/util/fixed_math.rs`）：`65536` 表示 `1.0`。用于倍率。 |
| `Coord` | `x`、`y`、`z`，单位为 lepton | 1 格等于 256 lepton。 |
| `Cell` | `x`、`y`，单位为格 | |
| `Frames` | 逻辑帧数 | 每游戏秒 15 个逻辑帧（`RA2_LOGIC_FRAMES_PER_SECOND`，`src/util/fixed_math.rs`）。 |
| `string` | 名字 | 节名和键名、初始化时的查找、触发器名和导出名、CSF 标签、客户端文字。比较时不区分大小写（RFC 4）。对局中的调用改用 ID（见 [ID](#id)）。 |
| `Techno`、`House`…… | 带代数的句柄 | `i64` 值；见[句柄](#句柄)。 |
| `TechnoTypeId`、`MarkId`…… | 具名事物的整数 ID | `i32` 值；见 [ID](#id)。 |
| `Uint8Array` | 字节 | 消息、模块数据。 |

```ts
type Fixed = i32;      // I16F16 raw value
type Frames = i32;

class Cell  { x: i32; y: i32; }
class Coord { x: i32; y: i32; z: i32; }   // leptons
class Rect  { x: i32; y: i32; width: i32; height: i32; }   // cells
```

### 句柄

对象引用是带代数的句柄（RFC 5）。对象已不存在的句柄读出来是“不存在”，绝不会读到被复用的槽位。位布局不属于 API，它取决于待定问题 2（ABI）。

句柄是值类型。在 TypeScript SDK 中，每种句柄类型都是 `i64` 的别名，`i64` 是 SDK 的可移植整数类型之一，`tsc` 和 AssemblyScript 都接受；在 Rust SDK 中，每种句柄都是一个 newtype。传递、保存或接收句柄都不分配内存（RFC 15）。本清单把调用写成句柄上的方法（`techno.kill(...)`），和 RFC 的示例一致；在 TypeScript SDK 中，这样的调用是一个以句柄为第一个参数的函数，最终写法取决于待定问题 3。句柄从不可为空：`NO_HANDLE`（`0`）表示“没有”，从不指向存活的对象。

| 句柄 | 指向 | 何时失效 |
| --- | --- | --- |
| `Techno` | 任意单位或建筑 | 对象从游戏中移除 |
| `Building`、`Infantry`、`Vehicle`、`Aircraft` | 该类别的 `Techno` | 同 `Techno` |
| `Projectile` | 飞行中的抛射体 | 它引爆或被移除 |
| `House` | 玩家或电脑所属方 | 场景进行期间从不失效 |
| `Team` | 存活的 AI 小队 | 小队解散 |
| `Trigger` | 存活的触发器 | 它被删除 |
| `Tag` | 存活的标签 | 它被删除 |
| `Effect` | 一个持续效果实例 | 它到期或被移除 |
| `SuperWeapon` | 某个所属方的某种超级武器 | 该所属方不再拥有它 |
| `Node` | 客户端界面树中的一个节点（第 11 节） | 它被删除，或对局界面关闭 |

```ts
type Techno     = i64;   // any unit or building
type Building   = i64;   // a Techno of that category
type Infantry   = i64;
type Vehicle    = i64;
type Aircraft   = i64;
type Projectile = i64;
type House      = i64;
type Team       = i64;
type Trigger    = i64;
type Tag        = i64;
type Effect     = i64;
type SuperWeapon = i64;
type Node       = i64;   // client
const NO_HANDLE: i64 = 0;

// on Techno:
isAlive(): bool;
asBuilding(): Building;   // NO_HANDLE when not a building
asInfantry(): Infantry;   // NO_HANDLE when not infantry
asVehicle(): Vehicle;     // NO_HANDLE when not a vehicle
asAircraft(): Aircraft;   // NO_HANDLE when not an aircraft

// a handle that may be absent is compared with NO_HANDLE (in an action):
if (e.other != NO_HANDLE) {
  ctx.log("attacked");
}
```

可能不指向任何对象的签名、字段或返回值使用普通的句柄类型，并注明可能为 `NO_HANDLE`。事件参数中可能缺失的字段（攻击者、抛射体）缺失时为 `NO_HANDLE`。`T | null` 只保留给 `Path`、`DisguiseInfo` 这样的类（见[签名](#签名)）。

类型以及 [ID](#id) 中列出的其他命名种类，在 INI 和初始化调用中用 ID 字符串命名；模块代码持有查找返回的整数 ID。

### ID

模块在对局中用到的具名事物都有整数 ID。模块对每个名字只解析一次，通常在 `onRules` 或 `onScenarioStart` 中完成，把 ID 保存在自己的内存里，其他所有调用都传 ID（RFC 15）。SDK 会缓存解析过的 ID。在 TypeScript SDK 中，每种 ID 类型都是 `i32` 的别名；在 Rust SDK 中，每种都是一个 newtype。和句柄一样，ID 是值类型，不分配内存。

| ID 类型 | 名字 | 查找 |
| --- | --- | --- |
| `TechnoTypeId` | 步兵、载具、飞行器和建筑类型 | `ctx.typeId(name)` |
| `WeaponTypeId` | 武器 | `ctx.weaponTypeId(name)` |
| `WarheadTypeId` | 弹头 | `ctx.warheadTypeId(name)` |
| `ProjectileTypeId` | 抛射体（BulletType） | `ctx.projectileTypeId(name)` |
| `AnimTypeId` | 动画 | `ctx.animTypeId(name)` |
| `VoxelAnimTypeId` | 体素动画 | `ctx.voxelAnimTypeId(name)` |
| `ParticleSystemTypeId` | 粒子系统 | `ctx.particleSystemTypeId(name)` |
| `SuperWeaponTypeId` | 超级武器 | `ctx.superWeaponTypeId(name)` |
| `EffectTypeId` | 持续效果类型（`[AttachEffectTypes]`） | `ctx.effectTypeId(name)` |
| `EffectGroupId` | 效果类型组（`Groups`） | `ctx.effectGroupId(name)` |
| `ArmorId` | 护甲名称 | `ctx.armorId(name)` |
| `OverlayTypeId` | 覆盖物 | `ctx.overlayTypeId(name)` |
| `SoundId` | 模拟 API 中的声音 | `ctx.soundId(name)` |
| `MarkId` | 标记 | `Mark.id(name)` |
| `NotifyId` | 模拟到客户端的通知 | `Notify.id(name)`：名字统一大小写后的稳定哈希，在每台机器上、在两侧都相同；加载时对本局所有模块一起检查冲突 |
| `TeamTypeId`、`ScriptTypeId`、`AiTriggerId` | AI 小队类型、小队脚本、AI 触发器 | `ai.teamTypeId(name)`、`ai.scriptTypeId(name)`、`ai.aiTriggerId(name)` |

```ts
type TechnoTypeId = i32;   // every ID type above is an alias of i32
type WarheadTypeId = i32;
type MarkId = i32;
const NO_ID: i32 = -1;

// once, in onScenarioStart:
const E1 = ctx.typeId("E1");          // NO_ID and NotFound when nothing matches
const PANICKED = Mark.id("Panicked");
// during play:
techno.addMark(PANICKED);
```

查找时名字比较不区分大小写（RFC 4）。没有匹配的名字返回 `NO_ID`，并把 `ctx.lastStatus()` 设为 `NotFound`：按[错误模型](#错误)这是预期结果，因为名字来自 mod 可能修改的数据。传入 `NO_ID` 的调用同样返回 `NotFound`；任何查找都不会返回的 ID 属于非法参数。`Mark.id` 遇到没见过的名字会登记它，所以总能返回 ID。`Notify.id` 也总能返回 ID：引擎在加载模块时，把本局加载的所有模块（mod 的和地图的合在一起）的通知名放在一起检查哈希冲突，名字发生冲突的模块会被拒绝，并给出提示（RFC 3 第 6 步；RFC 10）。对局中不能新增类型（RFC 8），所以一个 ID 在整个场景内一直有效，存档和读档之后也不变。

名字只保留在初始化和不频繁的调用中：上面的查找、`ctx.house(name)`、`onRules` 的写入和 `rules.set`（第 10 节）、规则读取、`Trigger.create` 以及 `Action.wasm` 的导出名、`Trigger.find` 和 `Tag.find`、AI 定义（9.2）、指定国家、音乐、EVA 语音或 CSF 标签的不频繁场景动作，以及客户端 API 中启动时的 `find`、热键名、声音、CSF 标签和文字（第 11 节）。

### 事件投递

引擎不会每个事件或每个动作调用一次模块。触发器触发时，其中的原版动作和原子动作立即执行，与原版中的原版动作一致；其中的每个模块动作（`Action.wasm`）向模块的队列写入一条固定大小的记录，记录带着触发该触发器的那个事件的载荷。模块只能通过这样的触发器收到事件：没有单独的事件记录，每条记录都是一个排队的模块动作。到模块阶段，引擎按触发顺序把整个数组一次调用交给模块（RFC 5、15）：`onEvents(ctx, batch)` 由 SDK 生成，它对每条记录运行对应的动作函数。如果在模块阶段之前游戏已经结束，或者触发器已被禁用或删除，排队的动作会被丢弃。一条记录包含模块动作、事件类型、主体句柄、另一方句柄、所属方、一个抛射体句柄和一个效果句柄、一个类型 ID，以及几个整数载荷字段（`EventRecord`，[第 2 节](#2-事件)）。本清单中每个事件的载荷都写明它填入哪些记录字段；事件没有列出的字段为 `NO_HANDLE` 或 `-1`。每种记录的布局属于接口定义（RFC 12）。SDK 原地读取记录，不为每个事件分配内存。

VERA 新增的每个事件都在安全点引发，即不处在任何计算之中的时刻：一次计算完成之后（例如 `TakeDamage` 在伤害已经提交之后），或一次计算开始之前（例如 `HouseTransfer` 在转移开始之前）。触发器的原版动作和原子动作从这一点起立即执行（RFC 5）。正因为立即执行，它们可以在一帧之内连锁：伤害引发 `TakeDamage`，它的触发器用原子动作造成伤害，又引发 `TakeDamage`。连锁中含有由模块创建或绑定的触发器、VERA 新增的事件或原子动作时，引擎限制它每帧的深度；会超过上限的触发器改到下一帧触发，所以这样的连锁不会无限循环（缺口 G-T11）。具体上限是 RFC 的待定问题 18。只由原版事件和原版动作组成的连锁，与原版中完全一样地运行（RFC 目标 5）。

### 查询结果

查询把结果写入模块提供的缓冲区，并返回写入的数量；引擎每次查询都不分配内存（RFC 6c、15）。结果按对象创建顺序返回，这个顺序来自引擎在每个对象和抛射体创建时分配的创建序号，所以结果从不按存放位置排序。结果多于缓冲区容量时，查询会设置 `out.cursor`，用同一个缓冲区再次调用，就从上次写入的最后一个结果之后继续；最后一页写完后 `out.cursor` 为 `0`。

```ts
class HandleBuffer {         // allocated once by the module and reused
  constructor(capacity: i32);
  items: i64[];              // handles, from index 0
  cursor: i64;               // paging: 0 starts a query, non-zero continues it
}
class IdBuffer {             // the same for IDs
  constructor(capacity: i32);
  items: i32[];
  cursor: i64;
}
// objects.inRadius(at: Coord, leptons: i32, f: Filter, out: HandleBuffer): i32
```

### 旁路表

只有部分对象才用到的状态放在按句柄索引的旁路表里，而不是放进每个对象：标记（第 6 节）、类型标签绑定和其他订阅（`Wasm.Tags`，RFC 7），以及每个抛射体 16 字节的模块数据（RFC 9）。不用这些的对象没有任何开销。每个对象类型、武器类型、弹头类型和抛射体类型带一个标志位，表示是否有模块绑定它；事件触发点先检查这个标志位，标志位未置位时不再做任何事（RFC 15）。旁路表随模拟状态一起存档并计入哈希。

### 枚举

| 枚举 | 取值 | 来源 |
| --- | --- | --- |
| `Mission` | 0 `Sleep`、1 `Attack`、2 `Move`、3 `QMove`、4 `Retreat`、5 `Guard`、6 `Sticky`、7 `Enter`、8 `Capture`、9 `Eaten`、10 `Harvest`、11 `AreaGuard`、12 `Return`、13 `Stop`、14 `Ambush`、15 `Hunt`、16 `Unload`、17 `Sabotage`、18 `Construction`、19 `Selling`、20 `Repair`、21 `Rescue`、22 `Missile`、23 `Harmless`、24 `Open`、25 `Patrol`、26 `ParadropApproach`、27 `ParadropOverfly`、28 `Deliberate`（INI 名为 `Wait`）、29 `AttackMove`、30 `SpyplaneApproach`、31 `SpyplaneOverfly`；`None` = 255 | `MissionType`，`src/rules/mission_data.rs`。编号与地图中的编号相同。`QMove` 和 `Ambush` 在 YR 中处于休眠状态；`Rescue` 只供 AI 使用；`AttackMove` 在分派器中没有对应分支。`setMission` 是否应接受这四个值未核实。 |
| `Relation` | `Self`、`Ally`、`Enemy`、`Neutral` | RFC 6a。联盟关系的 owner：`src/sim/house_diplomacy.rs`。“Neutral”指平民或被动所属方；具体规则未核实。 |
| `Category` | `Infantry`、`Vehicle`、`Aircraft`、`Building`、`Projectile` | RFC 6a；`src/rules/object_type.rs` 中的 `ObjectCategory` 涵盖前四个。 |
| `Layer` | `Underground`、`Surface`、`Ground`、`Air`、`Top` | 引擎中的层名未核实；RFC 6a 用的是空中、地面、水面。水面是地形类型，不是层。 |
| `LandType` | `Clear`、`Road`、`Water`、`Rock`、`Wall`、`Tiberium`、`Beach`、`Rough`、`Ice`、`Railroad`、`Tunnel`、`Weeds` | `src/rules/terrain_rules.rs` 读取的地形节。 |
| `SpeedType` / 移动区域 | 与 `Locomotor`/`SpeedType`/`MovementZone` 的读取器一致 | `src/rules/locomotor_type.rs`；具体取值列表在此未核实。 |
| `Compare` | 0 `Less`、1 `LessEqual`、2 `Equal`、3 `GreaterEqual`、4 `Greater`、5 `NotEqual` | 与 AI 触发器比较符的顺序相同（`compare`，`src/sim/ai_team_creation.rs`）。 |
| `Difficulty` | `Easy`、`Normal`、`Hard`、`All` | 触发器难度标志（`TriggerType.difficulty`）。 |
| `TagRepeat` | 0 `OneTimeOr`、1 `OneTimeAnd`、2 `Repeating` | `TagType.repeat`；名字沿用惯例，行为来自 `dispatch_live_tag_event`。 |
| `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`；Ares 种类 | `SuperWeaponKind`，`src/rules/superweapon_type.rs`。 |

### 错误

SDK 没有异常。失败分两类：

1. **预期结果**返回 `Status`（返回句柄的函数则返回 `NO_HANDLE`），由模块决定如何处理。
2. **编程错误**（trap、超出运算额度、参数非法）会让模块在所有机器的同一帧被停用，如 RFC 3 所述。

```ts
enum Status {
  Ok = 0,
  Gone = 1,         // a handle's object no longer exists
  NotFound = 2,     // a name matched no type, section, trigger or node
  Refused = 3,      // the owner declined: cell blocked, cannot attach, not allowed
  Unsupported = 4,  // the feature is off in this game (for example an Ares action without Ares)
}
```

哪些参数算非法（因而会停用模块）按函数逐一列出：越界的下标（全局变量 0..49，局部变量 0..99）、负数的数量、不在列表中的枚举值。没有匹配的名字返回 `NotFound`，因为名字来自 mod 可能修改的数据。触发器中原子动作的目标种类不适用于该动作，同样属于数据错误，返回状态（[4.1](#41-动作的用法)）。两类错误如何划分是**已在 RFC 0.0.5 采纳**（提议 P18）。

对返回句柄、ID 或数值的函数，`ctx.lastStatus(): Status` 给出上一次调用的状态。

### 随机数

| 列值 | 含义 |
| --- | --- |
| `module` | 从模块自己的随机流取数，这条流由场景种子派生（RFC 4）。 |
| `native` | 运行原生代码，按那段代码原本的方式从原生随机流取数。取数清单以 Rust 移植为准。 |
| `none` | 不取随机数。 |

```ts
class Random {
  next(): u32;                    // module stream
  range(low: i32, high: i32): i32; // inclusive; module stream
}
// ctx.random: Random
```

每个原生动作的取数与它的 Rust 移植相同。还没有逐个动作核对取数次数；这类条目写作 `native (unverified)`。

### 条目标注

每个条目都带有以下列或字段：

| 字段 | 取值 |
| --- | --- |
| **层级** | `v1`（RFC 计划中的 P1 或 P2）、`v1?`（待定问题 13 下的候选）、`later` |
| **世界** | `sim`（`rules.wasm`）、`client`（`ui.wasm`）、`both` |
| **Owner** | 实际完成工作的 Rust `file::function`，或**尚无 owner**：这时[缺口列表](#引擎中的缺口)写明需要补充什么 |
| **来源** | 条目的出处（缩写见下表） |

来源缩写：

| 缩写 | 含义 |
| --- | --- |
| `E12` | 原版地图触发器事件 12 |
| `A42` | 原版地图触发器动作 42 |
| `S46` | AI 小队脚本动作 46 |
| `Ar:` | Ares 的事件、动作或键 |
| `Ph:` | Phobos 的键 |
| `T5`、`F12`、`C8` | 社区表格：“触发器”表第 5 行、“效果器”表第 12 行、“约束条件”表第 8 行 |
| `Rc:` | Recoil（Spring）中的类似机制，来自调研笔记 |
| `RFC 6b` | 提到该条目的 RFC 章节 |

`TR` 指 `src/sim/trigger_runtime.rs`，`TRA` 指 `src/sim/trigger_runtime_ares.rs`，`TSV` 指 `src/sim/team_script_vm/`。

本清单中原版事件和动作的名字就是 API 名。代码里没有名字的，以 FinalAlert2 的惯用名为起点；真正标识事件或动作的是编号。

## 1. 生命周期导出

模块只导出它用到的函数。没有导出的函数没有开销（RFC 5）。

### 模拟模块（`rules.wasm`）

| 导出 | 签名 | 时机 | 层级 | Owner | 来源 |
| --- | --- | --- | --- | --- | --- |
| `onRules` | `onRules(rules: RulesWriter): void` | 一次，在 INI 层读完后（mod：在 mod 层之后；地图：在地图层之后） | v1 | `src/rules/native_processing.rs::RulesLayerStack::push`/`process`；模块层：**尚无 owner** | RFC 5、8；Rc：定义后处理阶段 |
| `onScenarioStart` | `onScenarioStart(ctx: Ctx): void` | 一次，在场景开始时 | v1 | **尚无 owner**（模块宿主） | RFC 5；Rc：`GameStart` |
| `onFrame` | `onFrame(ctx: Ctx): void`，通过 `onFrame({ every: N }, fn)` 注册 | 每 N 帧一次，在模块阶段，`onEvents` 之后 | v1 | **尚无 owner**（`src/sim/world/master_frame.rs` 中的模块阶段） | RFC 5；T27 `HoldDuration=yes`；Rc：`GameFrame` |
| 动作 | `(ctx: Ctx, e: EventRecord): void`，由 `action(fn)` 包装 | 带这个动作的触发器触发时排队；在模块阶段随本帧的批次（`onEvents`）执行 | v1 | TR `spring_live_trigger` → 事件记录：**尚无 owner** | RFC 5、6a |
| `onEvents` | `onEvents(ctx: Ctx, batch: EventBatch): void`，由 SDK 生成 | 每帧每个有排队记录的模块一次，在模块阶段、`onFrame` 之前 | v1 | **尚无 owner**（批量事件写入，G-T9） | RFC 5、15 |
| 覆盖 | 见[第 8 节](#8-覆盖点) | 引擎走到该决策时同步调用 | later（v1 选两三个） | 每个覆盖点各自的 owner | RFC 5、6 L3 |
| `onMessage` | `onMessage(ctx: Ctx, sender: House, data: Uint8Array): void` | 客户端消息到达模拟时，在模块阶段 | v1（P2） | `src/sim/command.rs::Command`：**尚无模块变体** | RFC 5、10；Rc：`RecvLuaMsg(msg, playerID)` |
| `onLaunch` | `onLaunch(ctx: LaunchCtx, p: Projectile): Path \| null` | 绑定了模块路径的类型，每个抛射体一次 | later | `ProjectileTrajectory` 的模块分支：**尚无 owner** | RFC 9 层级 3 |
| `onTrajectoryFrame` | `onTrajectoryFrame(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | 每帧每种抛射体类型一次 | later（L4） | **尚无 owner** | RFC 6 L4、9 层级 4 |
| `onLoad` | — | 不定义：模块内存随快照恢复，`onScenarioStart` 不会再次运行 | **已在 RFC 0.0.5 采纳**（P2） | — | Rc：`Load`，笔记 6.10 |

`ctx` 是模拟 API 唯一的入口：

```ts
class Ctx {
  frame(): i32;
  random: Random;
  scenario: Scenario;   // section 7
  objects: Objects;     // section 7
  map: MapQuery;        // section 7
  rules: RulesReader;   // section 10
  ai: Ai;               // section 9
  house(name: string): House;   // lookup, NO_HANDLE when not found; ID lookups: see IDs
  global(index: i32): Variable;  // 0..49
  local(index: i32): Variable;   // 0..99
  log(text: string): void;
  notify(to: House, id: NotifyId, data: Uint8Array): Status;  // section 11
  lastStatus(): Status;
}
class Variable { get(): bool; set(value: bool): Status; }
```

`ctx.local(i)` 和 `ctx.global(i)` 经过唯一的写入者
`TR::Simulation::set_trigger_variable`。它还会重置等待这个变量的触发器的计时器（事件 27/28/36/37）。
来源：A28、A29、A56、A57、S12、S38、S39、S40。

### 客户端模块（`ui.wasm`）

| 导出 | 签名 | 时机 | 层级 | Owner | 来源 |
| --- | --- | --- | --- | --- | --- |
| `onStart` | `onStart(ui: Ui): void` | 一次，在对局界面建好、INI 布局读完之后 | v1（P2） | **尚无 owner**（`src/ui/` 中的客户端宿主） | RFC 5、10 |
| `onRender` | `onRender(ui: Ui, dt: f32, alpha: f32): void` | 每个渲染帧一次 | v1（P2） | **尚无 owner** | RFC 5；Rc：`Update(dt)` |
| `onClick` | `onClick(ui: Ui, node: Node): void` | 带点击处理的节点被点击 | v1（P2） | **尚无 owner** | RFC 10；T39 `OnButtonClick` |
| `onHover` | `onHover(ui: Ui, node: Node, inside: bool): void` | 光标进入或离开节点 | v1（P2） | **尚无 owner** | RFC 10 |
| `onHotkey` | `onHotkey(ui: Ui, name: string): void` | 模块注册的热键被按下 | v1（P2） | **尚无 owner** | RFC 10 |
| `onSelect` | `onSelect(ui: Ui): void`；选择用 `client.selection(out)` 读取 | 本地选择发生变化 | v1（P2） | `src/app/input/` 中的选择 owner（未核实） | RFC 10；T38 `OnSelect` |
| `onNotify` | `onNotify(ui: Ui, id: NotifyId, data: Uint8Array): void` | 发给本地玩家的模拟通知到达 | v1（P2） | **尚无 owner** | RFC 1、11（“本地通知”）；导出名为**已在 RFC 0.0.5 采纳**（P19） |
| `onPeerMessage` | `onPeerMessage(ui: Ui, from: House, data: Uint8Array): void` | 盟友或观战者的客户端发来消息 | later | **尚无 owner** | **RFC 0.0.5 未采纳**（P5）；Rc：`SendLuaUIMsg` |

客户端允许使用浮点：客户端世界本来就不是确定性的（RFC 1）。

## 2. 事件

### 事件的用法

```ts
enum EventKind { Delivered, Polled }

class EventParam {
  value: i32 = -1;          // number parameter (house index, waypoint, seconds, ...)
  name: string | null = null;   // type name or TeamType name
  compare: Compare = Compare.GreaterEqual;   // polled events
  interval: Frames = 15;    // polled events: how often the engine checks
}

class TriggerDef {
  house: House = NO_HANDLE;
  event: i32 = 0;            // an Event value
  param: EventParam = new EventParam();
  conditions: Cond[] = [];
  actions: Action[] = [];
  difficulty: Difficulty = Difficulty.All;
  allowWin: bool = false;    // the A15 marker
  transferOnCapture: bool = false;   // TriggerType token 8
}

class EventRecord {          // fixed size; read in place from the batch (RFC 15)
  action: i32;               // the queued module action (an Action.wasm export); always set
  kind: i32;                 // Event value
  subject: Techno;           // the object the trigger is attached to; may be NO_HANDLE
  other: Techno;             // attacker, target, transport, producer, ...; may be NO_HANDLE
  house: House;              // house involved (retained house for E1/E24/E25/E26/E59); may be NO_HANDLE
  projectile: Projectile;    // may be NO_HANDLE
  effect: Effect;            // may be NO_HANDLE
  id: i32;                   // type ID where listed: weapon, warhead, effect type, superweapon type
  value: i32;                // damage, weapon index, stack count, frame, ...
  value2: i32;               // second integer field where listed
  cellX: i32;                // "cell" in the tables; -1 when none
  cellY: i32;
}
class EventBatch {           // one per module per frame
  count: i32;
  at(i: i32): EventRecord;   // a view into module memory, not a copy
}
```

某个事件没有列出的 `EventRecord` 字段为 `NO_HANDLE` 或 `-1`。记录按触发顺序排列；每条记录是一个排队的模块动作，加上触发其触发器的那个事件的载荷，没有单独的事件记录。布局属于接口定义（见[事件投递](#事件投递)）。RFC 6a：每个事件都标明它所附着的对象，以及涉及的另一个对象（如果有的话）。

原版事件在现有的触发器运行时里求值（`TR::has_occurred`）。触发器的原版动作和原子动作在触发时执行，带模块动作的触发器也是如此；只有模块动作要等到模块阶段（第 4.1 节，RFC 5）。由 House 标签引发的原版事件每 8 帧轮询一次（`TR::raise_house_tags`），Logic 标签的事件每帧轮询（`TR::advance_triggers`）。和原版一样，一个触发器的所有事件都要成立（AND）。

### 2.1 原版事件 0..61

类型：`R` 在某个时刻引发（投递型），`H` House 标签轮询，`L` Logic 标签轮询。载荷：`S` 主体，`O` 另一方（行为者或来源），`Hs` 所属方。所有原版事件都是 `v1`、`sim`；除非该行另有说明，求值的 owner 都是 `TR::has_occurred`。

| # | API 名称 | 参数 | 类型 | 载荷 | 引发位置（owner） | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | 无 | 不公开：永远不成立。 |
| 1 | `EnteredBy` | `value` 所属方下标（-1 表示任意） | R | S、O、保留的 Hs | `src/sim/building_entry.rs`、`c4_plant.rs`、`world/world_orders.rs`、`movement/per_cell.rs`、`vehicle_theft.rs`、`TR::dispatch_foot_cell_tag_events` | 会写持久状态。 |
| 2 | `SpiedBy` | 忽略 | R | S、O | **未找到引发点** | 目前从不触发。 |
| 3 | `ThievedBy` | `value` 所属方下标 | R | S、Hs | **未找到引发点** | 目前从不触发。 |
| 4 | `DiscoveredByPlayer` | 忽略 | R | S、O | `world/lifecycle.rs` 的发现逻辑；`combat/record_kill.rs::record_kill_tag_prefix` | |
| 5 | `HouseDiscovered` | `value` 所属方 | H | Hs | `TR::raise_house_tags` | |
| 6 | `AttackedByAnyHouse` | 忽略 | R | S、O | `TR::raise_house_attacked_tags`；`combat/object_health.rs`；`capture_manager.rs`；`record_kill.rs` | T4 `OnAttackedBy`（粗略形式）。 |
| 7 | `DestroyedByAnyHouse` | 忽略 | R | S、O | `combat/record_kill.rs` | 需要击杀来源。 |
| 8 | `AnyEvent` | — | H | S | `TR::raise_house_tags`（每 8 帧） | |
| 9 | `AllUnitsDestroyed` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 10 | `AllBuildingsDestroyed` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 11 | `AllDestroyed` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 12 | `CreditsExceed` | `value` 金钱 | H | Hs | House 标签轮询 | T31 `OnPlayerMoney`。 |
| 13 | `ElapsedTime` | `value` 秒（×15 帧） | L | — | `TR::advance_triggers` | 计时器由 `reset_trigger_timers` 重置。T5 `OnTimer`。 |
| 14 | `MissionTimerExpired` | — | L | — | `TR::advance_triggers` | |
| 15 | `BuildingsDestroyedCount` | `value` 数量 | H | Hs | House 标签轮询 | |
| 16 | `UnitsDestroyedCount` | `value` 数量 | H | Hs | House 标签轮询 | |
| 17 | `NoFactoriesLeft` | — | H | Hs | House 标签轮询 | |
| 18 | `CiviliansEvacuated` | — | H | Hs | House 标签轮询 | |
| 19 | `BuildBuildingType` | `value` BuildingType 下标 | H | Hs | House 标签轮询 | 会写持久状态。API 也接受 `name`。 |
| 20 | `BuildUnitType` | `value` VehicleType 下标 | H | Hs | House 标签轮询 | 同 19。 |
| 21 | `BuildInfantryType` | `value` InfantryType 下标 | H | Hs | House 标签轮询 | 同 19。 |
| 22 | `BuildAircraftType` | `value` AircraftType 下标 | H | Hs | House 标签轮询 | 同 19。 |
| 23 | `TeamLeavesMap` | `name` TeamType | R | — | `TR::raise_team_left_map_tags` ← `TSV/team_ai.rs` | |
| 24 | `ZoneEntryBy` | `value` 所属方（-1 表示任意） | R | S、O、保留的 Hs | `TR::dispatch_foot_cell_tag_events`，遍历地图标签 | |
| 25 | `CrossesHorizontalLine` | 同 24 | R | O、保留的 Hs | 同上，线标志 0x100000 | |
| 26 | `CrossesVerticalLine` | 同 24 | R | O、保留的 Hs | 同上，线标志 0x200000 | |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — | `TR::evaluate_event` | |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — | `TR::evaluate_event` | |
| 29 | `DestroyedByAnythingNotInfiltrate` | 忽略 | R | S、O | `combat/record_kill.rs` | |
| 30 | `LowPower` | `value` 所属方 | H | Hs | House 标签轮询 | T32 `OnPlayerPower`。 |
| 31 | `BridgeDestroyed` | 忽略 | R | 格子 | `TR::dispatch_bridge_cell_tag_event` | |
| 32 | `BuildingExists` | `value` BuildingType 下标 | H | Hs | House 标签轮询 | 会写持久状态。 |
| 33 | `SelectedByPlayer` | 忽略 | R | S | `world/world_commands.rs` | 联机对局中不引发。T38（客户端形式见第 11 节）。 |
| 34 | `ComesNearWaypoint` | `value` 路径点 | R | S | `movement/per_cell.rs` | 距离在 0x500 lepton 以内。 |
| 35 | `EnemyInSpotlight` | 忽略 | R | S | `world/spotlight.rs` | |
| 36 | `LocalIsSet` | `value` 0..99 | L | — | `TR::evaluate_event` | |
| 37 | `LocalIsClear` | `value` 0..99 | L | — | `TR::evaluate_event` | |
| 38 | `FirstDamagedCombat` | 忽略 | R | S、O | `combat/object_health.rs` | 有伤害来源。 |
| 39 | `HalfHealthCombat` | 忽略 | R | S、O | `combat/object_health.rs` | 黄血。 |
| 40 | `QuarterHealthCombat` | 忽略 | R | S、O | `combat/object_health.rs` | 红血。 |
| 41 | `FirstDamagedAny` | 忽略 | R | S、O（可为 `NO_HANDLE`） | `combat/object_health.rs`；`world/crash.rs` | |
| 42 | `HalfHealthAny` | 忽略 | R | S | `combat/object_health.rs` | |
| 43 | `QuarterHealthAny` | 忽略 | R | S | `combat/object_health.rs` | |
| 44 | `AttackedByHouse` | `value` 所属方数组下标 | R | S、O | `combat/object_health.rs`；`capture_manager.rs` | Ares 会转换 Player@X。 |
| 45 | `AmbientLightBelow` | `value` | L | — | 环境光变化时的 `TR::advance_triggers` | |
| 46 | `AmbientLightAbove` | `value` | L | — | 同上 | |
| 47 | `ElapsedScenarioTime` | `value` 秒 | L | — | `TR::evaluate_event` | |
| 48 | `DestroyedByAnything` | 忽略 | R | S、O（可为 `NO_HANDLE`） | `record_kill.rs`；`building_entry.rs`；`world/world_orders.rs`；`movement/per_cell.rs`；`vehicle_theft.rs`；`world/crash.rs` | T9 `OnKilled`（粗略形式）。 |
| 49 | `PickupCrate` | 忽略 | R | S | `crates/pickup.rs` | |
| 50 | `PickupCrateAny` | 忽略 | L | — | 有箱子被捡起时的 `TR::advance_triggers` | |
| 51 | `RandomDelay` | `value` 秒 | L | — | `TR::advance_triggers` | 随机数：原生（`scenario_rng`，在 `reset_trigger_timers` 中）。 |
| 52 | `CreditsBelow` | `value` 金钱 | H | Hs | House 标签轮询 | |
| 53 | `SpyAsHouse` | `value` 所属方 | R | S、Hs | `TR::dispatch_foot_cell_tag_events`（Agent 步兵） | 会写持久状态。 |
| 54 | `SpyAsInfantry` | `value` InfantryType 下标 | R | S | 同上 | 会写持久状态。 |
| 55 | `NavalUnitsDestroyed` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 56 | `LandUnitsDestroyed` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 57 | `BuildingDoesNotExist` | `value` BuildingType 下标 | H | Hs | House 标签轮询 | |
| 58 | `PowerFull` | `value` 所属方 | H | Hs | House 标签轮询 | |
| 59 | `EnteredOrOverflownBy` | 同 1 | R | S、O、保留的 Hs | `TR::dispatch_foot_cell_tag_events`；`TR::dispatch_airborne_cell_tag_event` | |
| 60 | `TechTypeExists` | `name` 类型，`value` 数量 | L | — | `TR::evaluate_event`；Ares：`TRA::ares_has_occurred` | |
| 61 | `TechTypeDoesNotExist` | `name` 类型 | L | — | 同上 | |

在模块创建的触发器里，原版事件仍可按编号使用：`TriggerDef.event = n`。`Event` 枚举给出它们的名字。

### 2.2 Ares 事件 62..88

只有游戏按 Ares 方式读取时（`rules.ares_form()`）才可用；否则 `Trigger.create` 返回 `NO_HANDLE`，状态为 `Unsupported`。在 `TRA::ares_has_occurred` 中求值。全部为 `v1`、`sim`，来源是同编号的 `Ar:` 事件。

| # | API 名称 | 参数 | 类型 | 引发位置 |
| --- | --- | --- | --- | --- |
| 62 | `UnderEmp` | — | R | `src/sim/game_entity/emp.rs` |
| 63 | `UnderEmpByHouse` | `value` 所属方 | R | `emp.rs` |
| 64 | `RemovedFromEmp` | — | R | `emp.rs` |
| 65 | `RemovedFromEmpByHouse` | `value` 所属方 | R | `emp.rs` |
| 66 | `InSpotlight` | — | R | `world/spotlight.rs` |
| 67 | `DriverKilled` | — | R | `vehicle_theft/driver_kill.rs` |
| 68 | `DriverKilledByHouse` | `value` 所属方 | R | `driver_kill.rs` |
| 69 | `VehicleTaken` | — | R | `vehicle_theft.rs` |
| 70 | `VehicleTakenByHouse` | `value` 所属方 | R | `vehicle_theft.rs` |
| 71 | `Abducted` | — | R | `abduction.rs` |
| 72 | `AbductedByHouse` | `value` 所属方 | R | `abduction.rs` |
| 73 | `AbductsSomething` | — | R | `abduction.rs` |
| 74 | `AbductsSomethingOfHouse` | `value` 所属方 | R | `abduction.rs`（`TRA::dispatch_object_ares_event`） |
| 75 | `SuperWeaponActivated` | `value` 超级武器类型下标 | H | `superweapon/mod.rs`（`TRA::raise_ares_super_event`） |
| 76 | `SuperWeaponDeactivated` | `value` 超级武器类型下标 | H | `superweapon/mod.rs` |
| 77 | `SuperWeaponNearWaypoint` | `name` 超级武器类型，`value` 路径点（5 格） | H | `superweapon/mod.rs` |
| 78 | `HouseReverseEngineeredType` | `name` 类型 | H | House 标签轮询 |
| 79 | `SomethingReverseEngineered` | — | R | `reverse_engineering.rs` |
| 80 | `TypeReverseEngineered` | `name` 类型 | R | `reverse_engineering.rs` |
| 81 | `HouseOwnsAtLeast` | `name`、`value` | H | House 标签轮询 |
| 82 | `HouseOwnsAtMost` | `name`、`value` | H | House 标签轮询 |
| 83 | `AttackedOrDestroyedByAnybody` | — | R | `combat/object_health.rs` |
| 84 | `AttackedOrDestroyedByHouse` | `value` 所属方 | R | `combat/object_health.rs` |
| 85 | `DestroyedByHouse` | `value` 所属方 | R | `combat/record_kill.rs` |
| 86 | `TypeCountAtMost` | `name`、`value` | L | Logic 标签轮询 |
| 87 | `HouseNoKeepAliveObjects` | `value` 所属方 | H | House 标签轮询 |
| 88 | `HouseNoKeepAliveBuildings` | `value` 所属方 | H | House 标签轮询 |

### 2.3 新的投递型事件（RFC 6a）

新事件编号取自不与 Ares、Phobos 冲突的范围（待定问题 6），这里不分配编号。新的投递型事件只对两类对象引发：其类型用 `Wasm.Tags` 绑定了含该事件的标签，或其自身的标签含有这样的触发器。从不对所有对象引发（RFC 5，目标 6）。武器事件（携带抛射体句柄的 `Fire`、`HitTarget` 和 `Detonated`，RFC 6a）还可以绑定在武器、弹头或抛射体类型上（在其节中写 `Wasm.Tags=`）：这种绑定对使用该类型的每个射手生效，不论射手自身是什么类型，射手就是主体。每个引发点先检查对象所属类型、或武器、弹头、抛射体类型的一个标志位（见[旁路表](#旁路表)）；`Cond.isWeaponType` 用来检查武器。每个事件都在安全点引发，即不处在任何计算之中的时刻：产生它的那次计算完成之后，对 `HouseTransfer` 来说是转移开始之前（见[事件投递](#事件投递)）；“引发位置”一列写出该时刻的 owner，不知道确切安全点的地方会注明。

| 事件 | 载荷（`EventRecord`） | 引发位置（该时刻的 owner） | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `TakeDamage` | `other` 来源（可为 `NO_HANDLE`），`value` 护甲结算后的伤害 | `src/sim/combat/world_receiver.rs::commit_entities`，在伤害提交之后；引发钩子**尚无 owner** | v1? | RFC 6a；T2 `OnTakeDamage`（历史字段 ③④ 放在模块内存里，RFC 6a）；Rc：`UnitDamaged` |
| `HitByWarhead` | `id` 弹头（`WarheadTypeId`），`value` 最终伤害，`other` 来源 | `combat/damage/receive.rs::receive_damage`，在它完成之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a；T3 `OnHitBy`（免疫的命中不算） |
| `AttackedBy` | `other` 攻击者，`value` 武器索引（-1 表示无），`projectile`（可为 `NO_HANDLE`） | `src/sim/combat/world_fire.rs` 中的开火步骤（在目标上引发），在该步骤完成之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a；T4 |
| `FatalDamage` | `other` 攻击者（可为 `NO_HANDLE`），`value` 伤害 | `combat/world_receiver.rs`，在致命伤害提交之后、死亡流程之前（这是否算安全点未核实）；钩子**尚无 owner** | v1? | RFC 6a；T22 `OnFatalDamage` |
| `Killed` | `other` 击杀者（可为 `NO_HANDLE`） | `src/sim/combat/record_kill.rs`，在击杀记录之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a；T9；Rc：`UnitDestroyed` |
| `Fire` | `other` 目标（可为 `NO_HANDLE`），`value` 武器索引，`id` 武器（`WeaponTypeId`），`projectile`（可为 `NO_HANDLE`） | `combat/world_fire.rs` 的 FireAt 步骤；`src/sim/aircraft/fire_at.rs::fire_at`；在射击完成之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a、9 层级 2；T10 `OnFire` |
| `SpawnFire` | `other` 子机，`value` 武器索引，`projectile`；目标在记录的 `cell` 中，或通过子机的目标获得 | `src/sim/spawn_manager.rs`（在子机射击之后；确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a；T11 `OnSpawnFire` |
| `HitTarget` | `other` 目标，`value` 伤害，`id` 武器（`WeaponTypeId`），`projectile` | `combat/world_receiver.rs`（在开火者上引发），在伤害提交之后；钩子**尚无 owner** | v1? | RFC 6a；T21 `OnHitTarget` |
| `TargetChanged` | `other` 新目标（清除目标时为 `NO_HANDLE`） | 目标指派和清除（`src/sim/world/techno_ai/scan_batch.rs` 和指令路径；未核实），在目标设定或清除之后；钩子**尚无 owner** | v1? | RFC 6a、6（P13）；T6 `OnAimed` |
| `ProjectileIncoming` | `projectile`，`other` 发射者（可为 `NO_HANDLE`） | 抛射体接纳 `src/sim/combat/projectile_delivery.rs::admit_fired_bullet`，在抛射体被接纳之后；钩子**尚无 owner** | v1? | RFC 6a；T7 `OnProjectileComing` |
| `Detonated` | `projectile`，`cell`，`other` 来源（可为 `NO_HANDLE`），`id` 弹头（`WarheadTypeId`） | `src/sim/projectile` 中的引爆判定，在引爆之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6 L2（“抛射体引爆”）、9 层级 2；T36；Rc：`ProjectileDestroyed`、`Explosion` |
| `Birth` | `other` 生产者（可为 `NO_HANDLE`），`effect`（可为 `NO_HANDLE`），`value` 生产时间（-1 表示无） | `src/sim/world/world_spawn.rs`、生产出厂、增援，在对象放置之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a；T8 `OnBirth`；Rc：`UnitCreated` |
| `Enter` | `other` 载具或建筑，`value` 已占空间，`value2` 1 为驻军 / 0 为载员；乘客通过查询获得 | `src/sim/building_entry.rs`、登上载具（`src/sim/passenger.rs` 的调用方），在进入完成之后；钩子**尚无 owner** | v1? | RFC 6a；T13 `OnEntry`；Rc：`UnitLoaded` |
| `Exit` | 同 `Enter`，在离开之后 | 卸载路径，在离开完成之后；钩子**尚无 owner** | v1? | RFC 6a；T14 `OnExit`；Rc：`UnitUnloaded` |
| `EnteredBy`（新） | `other` 乘客，`value` 已占空间，`value2` 驻军标志 | 同 `Enter`，在容纳方上引发 | v1? | RFC 6a；T15 `OnEntryBy` |
| `ExitedBy` | 同 `EnteredBy` | 同 `Exit` | v1? | RFC 6a；T16 `OnExitBy` |
| `Crushed` | `other` 碾压者 | 移动中的碾压步骤（文件未核实），在碾压之后；钩子**尚无 owner** | v1? | RFC 6a；T25 `OnCrush` |
| `HouseTransfer` | `house` 新所属方，`value` 为 1 表示心控；在转移之前引发 | `src/sim/world/mod.rs::change_owner_with_rules`；`src/sim/capture_manager.rs`；在转移开始之前；钩子**尚无 owner** | v1? | RFC 6a；T28 `OnHouseTransfer`；Rc：`UnitTaken` |
| `HouseTransferred` | `house` 原所属方，`value` 为 1 表示心控；在转移之后引发 | `change_owner_with_rules`，在转移完成之后；钩子**尚无 owner** | v1? | RFC 6a（P10）；Rc：`UnitGiven`（两阶段转移） |
| `EffectStart` | `effect`，`value` 层数 | `src/sim/attach_effect.rs::AttachEffectList::attach`，在效果附加之后；钩子**尚无 owner** | v1? | RFC 6a；T17 `OnBehaviorStart` |
| `EffectExpire` | `effect`（投递后读出为不存在），`id` 效果类型（`EffectTypeId`） | `attach_effect.rs` 的更新和移除，在效果移除之后；钩子**尚无 owner** | v1? | RFC 6a；T19 `OnBehaviorExpire` |
| `EffectStacks` | `effect`，`value` 新层数 | `attach_effect.rs` 的累加附加，在层数增加之后；钩子**尚无 owner** | v1? | RFC 6a；T20 `OnBehaviorStack` |
| `Timer` | `value` 上一次触发的帧 | 模块触发器计时器：**尚无 owner**（E13 的计时器复用 `CdTimer`） | v1? | RFC 6a；T5 `OnTimer` |
| `SuperWeaponLaunched` | `id` 超级武器类型（`SuperWeaponTypeId`），`house`，`cell` 目标 | `src/sim/superweapon/fire.rs::fire_super_weapon`，在超级武器发射之后（确切位置未核实）；钩子**尚无 owner** | v1? | RFC 6a |

T18 `OnBehavior`（效果持续中，每个间隔触发一次）对应下面的轮询事件 `EffectStacksPoll`，或在 `Timer` 触发器上加约束条件 `hasEffect`。

### 2.4 新的轮询型事件（RFC 6a）

引擎每隔 `param.interval` 帧原生检查一次状态，状态成立时触发。比较方式：用 `param.compare` 和 `param.value` 比较。轮询的 owner：**尚无 owner**（在 `TR::advance_triggers` 中 Logic 标签旁边加一个模块轮询列表）。每个事件都读取一个现有的 owner。

| 事件 | 读取内容 | 读取的 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `HealthPoll` | 生命值（`value`）；`name` 为 `"%"` 时为百分比 | `src/sim/combat/object_health.rs::Health` | v1? | RFC 6a；T26 |
| `AmmoPoll` | 弹药数 | `src/sim/ammo.rs` | v1? | RFC 6a；T29 |
| `PassengersPoll` | 乘客数 | `src/sim/passenger.rs::PassengerCargo::count` | v1? | RFC 6a；T33 |
| `TransportSpacePoll` | 剩余空间 | `PassengerCargo::total_size`、`capacity` | v1? | RFC 6a；T23 |
| `StoragePoll` | 载矿量 | 矿车存储（`Miner`，由 `src/sim/world/world_spawn/construction.rs` 创建） | v1? | RFC 6a；T35 |
| `MissionPoll` | 当前任务等于 `value` | `src/sim/mission/state.rs` | v1? | RFC 6a；T12 `OnMission` |
| `MovingPoll` | `value` 1 为移动，0 为停止 | 运动状态（读取函数未核实） | v1? | RFC 6a；T34 `OnStartMoving` |
| `LandTypePoll` | 对象所在格子的地形类型等于 `value` | 地图格子地形（语义见 `src/rules/terrain_rules.rs`） | v1? | RFC 6a；T24 |
| `HouseMoneyPoll` | 触发器所属方的可用金钱 | `src/sim/economy.rs::Economy`（`available_money`） | v1? | RFC 6a；T31 |
| `HousePowerPoll` | 发电量减耗电量 | `src/sim/power_system.rs::PowerState::total_output`、`total_drain` | v1? | RFC 6a；T32 |
| `EffectStacksPoll` | 效果类型 `name` 的层数 | `attach_effect.rs`（`active_count`） | v1? | RFC 6a；T18、T20 |
| `DistancePoll` | 到记录中 `other` 的距离，单位 lepton | 两个对象的坐标 | v1? | RFC 6a；T37 `Compare`（只有距离；其他比较写在模块代码里） |

### 2.5 其他候选事件（来自调研）

RFC 0.0.5 把这些加入了待定问题 13 的其他候选事件（提议 P11，RFC 6a）。`Stunned`/`Unstunned` 和 `Decloaked` 是本清单补充的，RFC 的清单没有列出。`HouseTransferred` 在 RFC 的主表中，列在 2.3。

| 事件 | 载荷 | 会在哪里引发 | 来源 |
| --- | --- | --- | --- |
| `Built` | `other` 建造者 | 建造完成（建造动画结束） | Rc：`UnitFinished` |
| `Idle` | — | 任务结束且没有排队的任务（任务 owner `src/sim/mission/`） | Rc：`UnitIdle` |
| `MoveFailed` / `Arrived` | `cell` 目的地 | 移动 owner | Rc：`UnitMoveFailed`、`UnitArrivedAtGoal` |
| `Stunned` / `Unstunned` | `other` 来源 | EMP（`game_entity/emp.rs`）；Ares 的情况由 Ares 62/64 覆盖 | Rc：`UnitStunned` |
| `Promoted` | `value` 新等级 | `src/sim/combat/veterancy.rs` | Rc：`UnitExperience` |
| `Cloaked` / `Decloaked` | — | `src/sim/cloak_disguise.rs` 的隐形更新 | Rc：`UnitCloaked` |
| `Spotted` / `Lost` | `house` 现在看到或失去它的所属方 | `src/sim/vision/mod.rs` | Rc：`UnitEnteredLos`、`UnitLeftLos` |
| `AmmoChanged` | `value` 旧值，`value2` 新值 | `src/sim/ammo.rs` | Rc：`StockpileChanged` |
| `MissionChanged` | `value` 任务 | `src/sim/mission/` | Rc：`UnitCommand` |
| `HouseDefeated` | `house` | `src/sim/world/house_defeat.rs` | Rc：`TeamDied` |
| `PlayerLeft` | `house` | `HouseState.left_game` 的写入者 | Rc：`PlayerRemoved` |
| `StorageFull` | `house` | `src/sim/economy.rs`（Ares 存储） | Rc：`ResourceExcess` |
| `MotionEnded` | `value` 原因 | 运动效果（第 5 节） | Rc：`MoveCtrlNotify` |

### 2.6 表格中未作为事件采用的触发项

| 行 | 触发项 | 去向 |
| --- | --- | --- |
| T27 | `HoldDuration=yes`（无条件，每帧） | `onFrame` |
| T30 | `OnCounterCondition` | 不进入引擎（RFC 6a）：放在模块内存里 |
| T37 | 距离以外的 `Compare` | 模块代码 |
| T38 | `OnSelect` | 客户端 `onSelect`（第 11 节）；模拟中为 E33 |
| T39 | `OnButtonClick` | 客户端 `onClick` |
| T40 | `RecorderCondition` | 不进入引擎（RFC 6a） |

表格把伤害写成浮点数（T2、T3、T21、T22）。按 RFC 的要求，API 使用整数。

## 3. 约束条件

约束条件在模块代码运行之前，由引擎原生过滤触发器（RFC 6a）。它默认检查主体对象；用 `.onOther()` 构建时检查 `EventRecord.other`。`Cond.not(c)` 表示取反。所有约束条件都属于 `sim`。求值 owner：**尚无 owner**（在 TriggerType 上放一个约束条件列表，由 `TR` 在触发之前检查）。

```ts
class Cond {
  static not(c: Cond): Cond;
  onOther(): Cond;
}
```

| 函数 | 检查内容 | 读取 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `Cond.relation(r: Relation)` | 被检查对象的所属方与触发器所属方的关系 | `src/sim/house_diplomacy.rs` | v1 | RFC 6a；C5 |
| `Cond.category(c: Category)` | 对象类别 | `ObjectType` 类别 / 抛射体 | v1 | RFC 6a；C3、C14 |
| `Cond.layer(l: Layer)` | 对象所在层 | 实体所在层（读取函数未核实） | v1 | RFC 6a；C4 |
| `Cond.isType(type: TechnoTypeId)`；`Cond.isProjectileType(type: ProjectileTypeId)` | 类型等于 `type` | `ObjectType` id；抛射体类型 | v1 | RFC 6a；C12、C13（取反）、C15（抛射体类型） |
| `Cond.isWeaponType(type: WeaponTypeId)` | 武器事件（`Fire`、`HitTarget`）的武器等于 `type` | 记录的 `id` | v1 | RFC 6a（武器、弹头和抛射体绑定） |
| `Cond.armor(armor: ArmorId)` | 护甲 | `src/rules/armor.rs::armor_index`；Ares `ares_armor.rs` | v1 | RFC 6a；C18 |
| `Cond.civilianHouse()` | 所属方是平民阵营 | `HouseState` 阵营/国家（规则未核实） | v1 | RFC 6a；C16 |
| `Cond.health(cmp: Compare, value: i32)` | 生命值 | `combat/object_health.rs::Health` | v1 | RFC 6a；C19 |
| `Cond.healthPercent(cmp: Compare, percent: i32)` | 生命百分比 | `Health` | v1 | RFC 6a；C19 |
| `Cond.healthBelowPercent(p: i32)` | 上一条的简写 | `Health` | v1 | RFC 示例 |
| `Cond.landType(t: LandType)` | 对象所在格子的地形类型 | 地图格子地形 | v1 | RFC 6a；C2 |
| `Cond.housePower(state: PowerLevel)` | `Full` 或 `Low` | `src/sim/power_system.rs::PowerState::is_low_power` | v1 | RFC 6a；C6（红/黄/绿：引擎中是否有第三档未核实） |
| `Cond.houseMoney(cmp: Compare, value: i32)` | 被检查对象所属方的金钱 | `src/sim/economy.rs` | v1 | RFC 6a；C7 |
| `Cond.hasEffect(type: EffectTypeId, minStacks: i32)` | 拥有该效果，且至少 `minStacks` 层 | `attach_effect.rs` `active_count` | v1 | RFC 6a；C8、C9（取反） |
| `Cond.hasMark(m: MarkId)` | 拥有该标记 | 标记：**尚无 owner** | v1 | RFC 6a；C10、C11（取反） |
| `Cond.effectSource(type: EffectTypeId, r: Relation)` | 效果 `type` 的来源与之关系为 `r` | `AttachEffectInstance` 来源（`is_self_owned`） | v1? | RFC 6a；C21 |
| `Cond.inWeaponRange(weaponIndex: i32)` | `other` 在主体的武器射程内 | 射程 owner `attach_effect.rs::range_with_attach_effects` 及武器 `Range` | v1? | RFC 6a；C20 |
| `Cond.isStandIn()` | 对象是“替身” | RFC 未涉及；引擎中没有这个概念 | —（未采用） | C17 |

## 4. 动作

### 4.1 动作的用法

动作有两种运行方式：

- 在触发器内：`TriggerDef.actions` 中放 `Action.original(n, params)`、`Action.wasm(name)` 或一个原子动作值；
- 由模块代码通过下面的函数直接调用。

在触发器内，原版动作（`Action.original`）和原子动作值在触发器触发时执行，与原版中的原版动作一致，触发器含有模块动作时也是如此。只有 `Action.wasm` 动作排队，每个都带着触发该触发器的那个事件的载荷，在模块阶段按触发顺序、每个模块每帧一批执行（RFC 5；[事件投递](#事件投递)）：动作为 [原版 A、模块 B、原子 C] 的触发器，在触发时执行 A 和 C，B 在同一帧稍后执行。如果在模块阶段之前游戏已经结束，或者触发器已被禁用或删除，排队的模块动作会被丢弃。需要严格顺序的作者，把触发器的全部动作都写成模块代码，再通过下面的函数调用原版动作。由模块代码直接调用时，动作立即执行。

在触发器中，每个原子动作值都带一个明确的目标参数（`ActionTarget`）：`Subject`（默认）为记录中的主体；`Other` 为记录中的另一方对象；`TagHolder` 为标签中持有本次触发的触发器的对象，即 4.2 中标为“live”的行所作用的对象，触发器由 CellTag 持有时为那个格子；`House` 为触发器的所属方，除非明确给出另一个所属方；`Cell` 为一个格子，可以直接给出，也可以用路径点给出。目标种类不适用于该动作（例如给 `techno.kill` 一个 `Cell`）属于数据错误，不是编程错误：动作返回错误状态，模块保持启用（见[错误](#错误)；RFC 3、6b）。立即执行的原版动作和原子动作可能引发新的事件，并在一帧之内连锁；含有由模块创建或绑定的触发器、VERA 新增的事件或原子动作的连锁，深度受每帧上限约束（见[事件投递](#事件投递)）。

```ts
class ActionParams {
  value: i32 = 0;
  value2: i32 = 0;           // auxiliary value (paramType 11)
  name: string | null = null; // type, CSF label, sound, theme, EVA name
  team: string | null = null;
  trigger: string | null = null;
  tag: string | null = null;
  cell: Cell | null = null;   // replaces the waypoint token
}
class Action {
  static original(n: i32, p: ActionParams): Action;
  static wasm(exportName: string): Action;
}
enum ActionTarget { Subject, Other, TagHolder, House, Cell }   // target of an atomic action in a trigger; Subject is the default; House defaults to the trigger's house
```

`Action.original(n, …)` 运行 `TR::execute_live_trigger_action` 中现有的分支；启用 Ares 时先走 `TRA::ares_execute_action`。4.2 中的具名函数调用同样的 owner。原版用路径点的地方，API 用 `Cell`；`ctx.map.waypoint(n)` 返回路径点所在的格子。目前读取路径点编号的 owner 需要增加按格子的入口（缺口 G-A1）。标为“live”的行作用于标签中持有本次触发的触发器的对象；它们的直接形式改为接收对象参数。

除非另有说明，每一行都是 `v1`、`sim`。随机数：凡执行原生代码的行都是 `native (unverified)`，注明的除外。

### 4.2 原版动作 0..145

| # | API 函数 | 参数 | Owner | 说明 |
| --- | --- | --- | --- | --- |
| 0 | — | — | 空操作 | 只能用 `Action.original(0)`。 |
| 1 | `scenario.declareWinner(country: string)` | 国家 | `src/sim/world/house_defeat.rs::request_player_outcome` | 作用于玩家，而不是触发器所属方。 |
| 2 | `scenario.declareLoser(country: string)` | 国家 | 同上 | |
| 3 | `house.beginProduction()` | — | `HouseState::enable_scripted_production` | |
| 4 | `ai.createTeam(teamType: TeamTypeId, house: House): Team` | TeamType | `TSV/reinforcements.rs::trigger_create_team` | 受数量上限限制。小队由 S0–S68 运行。`house` 可为 `NO_HANDLE`；没有创建小队时返回 `NO_HANDLE`。 |
| 5 | `ai.destroyTeams(teamType: TeamTypeId)` | TeamType | `TSV/membership.rs::destroy_teams_of_type` | |
| 6 | `house.allToHunt()` | — | `house_strategy::all_to_hunt` | |
| 7 | `ai.reinforce(teamType: TeamTypeId)` | TeamType（使用它自己的路径点） | `TSV/reinforcements.rs::spawn_team_reinforcements` | |
| 8 | `map.dropZoneFlare(cell: Cell)` | 格子 | `src/sim/anim_class.rs::trigger_drop_zone_flare` | G-A1。 |
| 9 | `house.fireSale()` | — | `strategy_emergency_mut().set_state_four()` | |
| 10 | `scenario.playMovie(index: i32)` | 影片编号 | `TriggerEffect::FullscreenMovie` | 对每个玩家生效的表现效果。 |
| 11 | `scenario.showText(csfLabel: string)` | CSF 标签 | `TriggerEffect::LocalizedMessage` | |
| 12 | `trigger.destroy()`；`Trigger.destroyType(id: string)` | 触发器 | TR（延迟删除运行中的实例） | |
| 13 | `house.beginAutocreate()` | — | `HouseState::enable_scripted_autocreate` | |
| 14 | live；直接形式为 `techno.setOwner(house)`（4.3） | 所属方 | `src/sim/world/mod.rs::change_owner_with_rules` | |
| 15 | `TriggerDef.allowWin = true` | — | `TR::classify_map_triggers`（`register_allow_win_tag_type`） | 只是标志，没有可执行的分支。 |
| 16 | `map.revealAll()` | — | `src/sim/world/shroud_refresh.rs::trigger_reveal_entire_map` | |
| 17 | `map.revealAround(cell: Cell)` | 格子 | `trigger_reveal_at_waypoint` | G-A1。 |
| 18 | `map.revealZone(cell: Cell)` | 格子 | `trigger_reveal_zone_at_waypoint` | G-A1。 |
| 19 | `sound.play(sound: SoundId)` | 声音 | `SimSoundEvent::VocCentered` | |
| 20 | `scenario.playTheme(name: string)` | 音乐 | `TriggerEffect::MusicTheme` | |
| 21 | `scenario.playSpeech(eva: string)` | EVA 名称 | `SimSoundEvent::TriggerEva` | |
| 22 | `trigger.force()`；`Trigger.forceType(id: string)` | 触发器 | `TR::apply_live_trigger_action` | 触发时没有执行者，格子为零。 |
| 23 | `scenario.timer.start()` | — | `scenario_countdown.rs`（`change_scenario_timer(Start)`） | |
| 24 | `scenario.timer.stop()` | — | 同上（`Stop`） | |
| 25 | `scenario.timer.extend(seconds: i32)` | 秒数 | 同上（`Extend`） | |
| 26 | `scenario.timer.shorten(seconds: i32)` | 秒数 | 同上（`Shorten`） | |
| 27 | `scenario.timer.set(seconds: i32)` | 秒数 | 同上（`Set`） | |
| 28 | `ctx.global(i).set(true)` | 0..49 | `TR::set_trigger_variable` | 超出范围：参数非法。 |
| 29 | `ctx.global(i).set(false)` | 0..49 | 同上 | |
| 30 | `house.setAutoBaseBuilding(on: bool)` | 开关 | `src/sim/world/house_base.rs::trigger_auto_base_building` | |
| 31 | `map.growShroud()` | — | `grow_shroud` | |
| 32 | live；直接形式为 `techno.kill(KillMode.Normal)`（4.3） | — | TR 中的 C4 直接伤害 + `apply_damage_to_cell`（桥梁） | 适用 Ares 的幸存者规则。 |
| 33 | `house.grantSuperWeapon(type: SuperWeaponTypeId, oneTime: bool)` | 超级武器类型 | `src/sim/superweapon/mod.rs::trigger_grant_super` | `oneTime = true`。 |
| 34 | 同上，`oneTime = false` | 超级武器类型 | 同上 | |
| 35 | `house.setPreferredTargetKind(value: i32)` | 数值 | `set_preferred_super_weapon_target_kind` | |
| 36 | `house.transferAllTo(to: House)` | 所属方 | `trigger_change_all_house_objects` | |
| 37 | `house.makeAlly(other: House)` | 所属方 | `src/sim/house_diplomacy.rs::make_ally` | 双向。 |
| 38 | `house.makeEnemy(other: House)` | 所属方 | `house_diplomacy.rs::make_enemy` | 双向。 |
| 39 | — | — | 没有分支（返回 false） | 不开放。 |
| 40 | `scenario.setVisibleMapArea(r: Rect)` | 矩形 | `change_visible_map_area` | |
| 41 | `map.playAnim(anim: AnimTypeId, cell: Cell)` | AnimType | `anim_class.rs::trigger_play_anim_at_waypoint` | G-A1。适用 Ares `MakeInfantry=` 的所属规则。 |
| 42 | `map.explode(weapon: WeaponTypeId, cell: Cell)` | WeaponType | `src/sim/world/trigger_explosion.rs::trigger_explosion_at_waypoint` | G-A1。坐标形式见 4.3。 |
| 43 | `map.playVoxelAnim(type: VoxelAnimTypeId, cell: Cell)` | VoxelAnimType | `trigger_play_voxel_anim_at_waypoint` | G-A1。 |
| 44 | `weather.startIonStorm(duration: i32)` | 持续时间 | `superweapon/lightning_storm::start` | 风暴进行中时拒绝。 |
| 45 | `weather.stopIonStorm()` | — | `lightning_storm::request_end` | 没有风暴时拒绝。 |
| 46 | `scenario.lockInput()` | — | `TriggerEffect::InputLockChanged` | |
| 47 | `scenario.unlockInput()` | — | `unlock_input` | |
| 48 | `scenario.centerCamera(cell: Cell, speed: i32)` | 格子、速度 | `TriggerEffect::CameraAtCoord` | 作用于每个玩家的镜头。客户端形式见第 11 节。 |
| 49 | — | — | 没有分支（休眠的遗留项） | 放大。不开放。 |
| 50 | — | — | 没有分支 | 缩小。不开放。 |
| 51 | `house.reshroud()` | — | `trigger_reshroud_map` | |
| 52 | live；直接形式为 `building.setSpotlightBehavior(v: i32)` | 数值 | `set_spotlight_behavior` | |
| 53 | `trigger.enable()` | 触发器 | TR（启用、按难度准入、重置计时器） | |
| 54 | `trigger.disable()` | 触发器 | TR | |
| 55 | `scenario.radarEvent(cell: Cell, kind: i32)` | 格子、`RadarEventType` | `SimSoundEvent::RadarEvent` | |
| 56 | `ctx.local(i).set(true)` | 0..99 | `TR::set_trigger_variable` | |
| 57 | `ctx.local(i).set(false)` | 0..99 | 同上 | |
| 58 | `map.meteorShower(cell: Cell, intensity: i32)` | 格子、强度 | `anim_class.rs::trigger_meteor_shower` | G-A1。 |
| 59 | `map.reduceOre(cell: Cell)` | 格子 | `src/sim/world/mod.rs::trigger_reduce_tiberium_at_waypoint` | G-A1。 |
| 60 | live；直接形式为 `building.sell()` | — | `production::sell_back` | |
| 61 | live；直接形式为 `building.setOnline(false)` | — | `src/sim/building_power.rs::go_building_offline` | |
| 62 | live；直接形式为 `building.setOnline(true)` | — | `building_power.rs::go_building_online` | |
| 63 | `map.damageBridgeAt(cell: Cell)` | 格子 | `bridge_orchestrator::damage_at_waypoint` | G-A1。 |
| 64 | `map.lightFlash(cell: Cell, size: FlashSize.Small)` | 格子 | C4 `CombatLightRequest::Impact`，伤害 50 | |
| 65 | 同上，`FlashSize.Medium` | 格子 | 伤害 100 | |
| 66 | 同上，`FlashSize.Large` | 格子 | 伤害 300 | |
| 67 | `scenario.announceWin()` | — | `PlayerOutcomeRequest::Flag{Victory}` | |
| 68 | `scenario.announceLose()` | — | `Flag{Defeat}` | |
| 69 | `scenario.endScenario()` | — | `EndScenario` | |
| 70 | `tag.destroy()`；`Tag.destroyType(id: string)` | 标签 | `TR::delete_live_tag` | 停止正在进行的投递。 |
| 71 | `lighting.setAmbientStep(v: i32)` | 数值 | `set_ambient_change_input(false, v)` | |
| 72 | `lighting.setAmbientRate(v: i32)` | 数值 | `set_ambient_change_input(true, v)` | |
| 73 | `lighting.setAmbient(v: i32)` | 数值 | `set_ambient_light` | |
| 74 | `house.setAiTriggersActive(true)` | — | `src/sim/house_state.rs::HouseState::set_ai_triggers_active` | |
| 75 | `house.setAiTriggersActive(false)` | — | 同上 | |
| 76 | `house.setTeamRatio(TeamRatio.AiTriggers, v)` | 数值 | `src/sim/ai_team_creation.rs::HouseTeamCreation::set_trigger_ratio` | |
| 77 | `house.setTeamRatio(TeamRatio.Aircraft, v)` | 数值 | `set_aircraft_team_ratio` | |
| 78 | `house.setTeamRatio(TeamRatio.Infantry, v)` | 数值 | `set_infantry_team_ratio` | |
| 79 | `house.setTeamRatio(TeamRatio.Units, v)` | 数值 | `set_unit_team_ratio` | |
| 80 | `ai.reinforceAt(teamType: TeamTypeId, cell: Cell)` | TeamType、格子 | `spawn_team_reinforcements(waypoint)` | G-A1。 |
| 81 | live；直接形式为 `techno.wakeUp()` | — | 对睡眠或无害的单位改为 Guard（TR） | |
| 82 | `house.wakeSleepers()` | — | TR（不属于该所属方的 Sleep 步兵和载具） | |
| 83 | `house.wakeHarmless()` | — | TR（属于该所属方的 Harmless 步兵和载具） | |
| 84 | `scenario.wakeGroup(group: i32)` | 编组 | TR | |
| 85 | `map.setVeinGrowth(v: i32)` | 数值 | `session.set_vein_growth` | |
| 86 | `map.setOreGrowth(v: i32)` | 数值 | `ore_growth_config.set_enabled_from_trigger` | |
| 87 | `map.setIceGrowth(v: i32)` | 数值 | `session.set_ice_growth` | |
| 88 | `map.createParticleSystem(type: ParticleSystemTypeId, cell: Cell)` | ParticleSystem | `particles/spawn.rs::trigger_create_particle_system_at_waypoint` | G-A1。 |
| 89 | `map.removeParticleSystems(cell: Cell)` | 格子 | `trigger_mark_particle_systems_done_at_waypoint` | G-A1。 |
| 90 | `weather.lightningStrike(cell: Cell)` | 格子 | `lightning_storm::create_cloud_bolt` | G-A1。 |
| 91 | live；直接形式为 `infantry.goBerserk()` | — | `make_psycho` | |
| 92 | `house.setFirestorm(true)` | — | Ares：`src/sim/ares_firestorm.rs::ares_set_firestorm_state` | 未启用 Ares 时为 `Unsupported`。 |
| 93 | `house.setFirestorm(false)` | — | 同上 | |
| 94 | `map.ionCannonStrike(cell: Cell)` | 格子 | `src/sim/world/ion_blast.rs::trigger_ion_blast` | G-A1。 |
| 95 | `map.nukeStrike(cell: Cell)` | 格子 | `src/sim/world/trigger_strikes.rs::trigger_nuke_strike` | G-A1。 |
| 96 | `map.chemStrike(cell: Cell)` | 格子 | `trigger_chemical_strike` | G-A1。 |
| 97 | `scenario.toggleTrainCargo()` | — | `session.toggle_train_crate` | |
| 98 | `sound.playAtRandomWaypoint(sound: SoundId)` | 声音 | `src/sim/world/object_sound.rs::trigger_play_sound_at_random_waypoint` | Ares 变体；从原生随机流取数。 |
| 99 | `sound.playAt(sound: SoundId, cell: Cell)` | 声音、格子 | `trigger_play_sound_at_waypoint` | G-A1。 |
| 100 | `scenario.playRadarMovie(index: i32, pause: bool)` | 影片 | `TriggerEffect::RadarMovie{pause:false}` | |
| 101 | `map.reshroudAt(cell: Cell)` | 格子 | `trigger_conceal_at_waypoint` | G-A1。 |
| 102 | `weather.lightningStormStrike(cell: Cell)` | 格子 | `lightning_storm::start`，使用规则中的持续时间 | 使用触发器所属方。 |
| 103 | `scenario.timer.setText(csfLabel: string)` | 标签 | `set_scenario_timer_caption` | |
| 104 | `team.flash(frames: i32)`；`ai.flashTeams(teamType, frames)` | TeamType、帧数 | `TSV/actions.rs::flash_teams_of_type` | |
| 105 | `scenario.talkBubble(team: Team, frame: i32)` | 小队、帧 | `talk_bubble_set_object_and_frame` | `team` 可为 `NO_HANDLE`。 |
| 106 | `rules.setTechLevel(type: TechnoTypeId, level: i32)` | 类型、等级 | `src/sim/world/type_tech_levels.rs::set_object_type_tech_level` | 另见第 10 节。 |
| 107 | `ai.reinforceByChrono(teamType: TeamTypeId, cell: Cell)` | TeamType、格子 | `spawn_chrono_team_reinforcements` | G-A1。 |
| 108 | `map.createCrate(kind: i32, cell: Cell)` | 种类（20 为随机） | `crates::place_crate` | 随机时从原生随机流取数。G-A1。 |
| 109 | `map.ironCurtainAt(cell: Cell)` | 格子 | `superweapon::iron_curtain::at_waypoint` | G-A1。 |
| 110 | `scenario.pause(duration: i32)` | 持续时间 | `session.pause_scenario` | |
| 111 | live；直接形式为 `building.evictOccupants()` | — | `garrison_eject::sell_building_occupants` | |
| 112 | `scenario.jumpCamera(cell: Cell)` | 格子 | `TriggerEffect::CameraAtCoord{velocity:None}` | |
| 113 | `house.cheer()` | — | `house_cheer` | |
| 114 | `scenario.setSidebarTab(tab: i32)` | 0..3 | `TriggerEffect::SelectSidebarTab` | |
| 115 | `scenario.flashCameo(type: TechnoTypeId, duration: i32)` | 类型 | `TriggerEffect::FlashSidebarCameo` | |
| 116 | `sound.stopAt(cell: Cell)` | 格子 | `trigger_stop_sound_at_waypoint` | G-A1。 |
| 117 | `scenario.playRadarMovie(index, true)` | 影片 | `RadarMovie{pause:true}` | |
| 118 | `map.clearSmudges()` | — | `src/sim/world/time_warp.rs::trigger_clear_all_smudges` | |
| 119 | `house.destroyAll(Destroy.All)` | — | `house_blowup(All)` | |
| 120 | `house.destroyAll(Destroy.Buildings)` | — | `house_blowup(Buildings)` | |
| 121 | `house.destroyAll(Destroy.Land)` | — | `house_blowup(Land)` | |
| 122 | `house.destroyAll(Destroy.Naval)` | — | `house_blowup(Naval)` | |
| 123 | `house.mindControlBase(of: House)` | 所属方 | `src/sim/capture_manager.rs::control_house_buildings(.., false)` | |
| 124 | `house.restoreMindControlledBase(of: House)` | 所属方 | `control_house_buildings(.., true)` | |
| 125 | `map.createBuilding(type: TechnoTypeId, cell: Cell, owner: House): Building` | 类型、格子 | `TR::trigger_create_building_at_waypoint` | G-A1。没有建成时返回 `NO_HANDLE`。 |
| 126 | `house.restoreStarting(buildingsOnly: false)` | — | `restore_initial_house_technos(.., true)` | |
| 127 | `scenario.chronoScreenEffect(duration: i32)` | 持续时间 | `start_chrono_screen_effect` | |
| 128 | `house.teleportAll(cell: Cell)` | 格子 | `src/sim/world/trigger_chrono.rs::chronoshift_house_to` | |
| 129 | `house.superWeapon(type).setChargePercent(p: i32)` | 超级武器、百分比 | `SuperWeaponInstance::set_charge_percentage` | |
| 130 | `house.restoreStarting(buildingsOnly: true)` | — | `restore_initial_house_technos(.., false)` | |
| 131 | `house.flashBuildings(type: TechnoTypeId, duration: i32)` | 类型 | `entity.flashing.start` | |
| 132 | `house.superWeapon(type).setRechargeTime(frames: i32)` | 超级武器、时间 | `SuperWeaponInstance::set_recharge_time` | |
| 133 | `house.superWeapon(type).resetRechargeTime()` | 超级武器 | `reset_recharge_time` | |
| 134 | `house.superWeapon(type).reset()` | 超级武器 | `superweapon::reset_super` | |
| 135 | `house.setPreferredTargetCell(cell: Cell)` | 格子 | `set_preferred_super_weapon_target` | |
| 136 | `house.clearPreferredTargetCell()` | — | `clear_preferred_super_weapon_target` | |
| 137 | `house.setAlternateBase(cell: Cell)` | 格子 | `HouseState::set_alternate_base` | |
| 138 | `house.clearAlternateBase()` | — | `clear_alternate_base` | |
| 139 | `house.radarBlackout(frames: i32)` | 帧数 | `src/sim/power_system.rs::PowerState::start_radar_outage` | |
| 140 | `house.setDefensiveTargetCell(cell: Cell)` | 格子 | `alert_super_weapon_defense` | |
| 141 | `house.clearDefensiveTargetCell()` | — | `clear_super_weapon_defense` | |
| 142 | `lighting.retint(Color.Red, v: i32)` | 数值 | `retint_normal_component(0)` | |
| 143 | `lighting.retint(Color.Green, v)` | 数值 | 分量 1 | |
| 144 | `lighting.retint(Color.Blue, v)` | 数值 | 分量 2 | |
| 145 | `scenario.homeCamera()` | — | `TriggerEffect::HomeCamera` | |

Ares 动作（仅在启用 Ares 时可用，否则为 `Unsupported`；owner 为 `TRA::ares_execute_action`）：

| # | API 函数 | Owner | 来源 |
| --- | --- | --- | --- |
| 146 | `house.addAuxPower(power: i32)` | `PowerState::add_aux_power` | Ar：146 |
| 147 | `house.killDrivers()` | `TRA::ares_trigger_kill_drivers` | Ar：147 |
| 148 | `scenario.setEvaVoice(v: i32)` | `TriggerEffect::EvaVoice` | Ar：148 |
| 149 | `techno.setGroup(group: i32)` | `assign_object_group` | Ar：149 |

### 4.3 原子动作（RFC 6b）与表格中的瞬时效果

一个函数对应一个操作，这个操作只有一个 owner（RFC 第 6 节的粒度原则）。在触发器中，每个函数都接受 [4.1](#41-动作的用法) 的目标参数。

| 函数 | 语义 | 随机数 | Owner | 层级 | 来源 |
| --- | --- | --- | --- | --- | --- |
| `techno.fireWeapon(w: WeaponSel, at: Target): Status` | 通过 FireAt，用 `w.index` 槽位或类型为 `w.weapon` 的武器，向格子、对象或当前目标开火。武器当前无法开火时拒绝 | native | **尚无 owner**（目前只有 `src/sim/combat/world_fire.rs::visit_fire` 中的开火更新会走到 FireAt） | v1? | RFC 6b；F4 `LaunchWeapon`；Rc：`UnitWeaponFire` |
| `house.launchSuperWeapon(type: SuperWeaponTypeId, cell: Cell): Status` | 发射该所属方拥有的超级武器 | native | `src/sim/superweapon/fire.rs::fire_super_weapon` | v1? | RFC 6b；F5 |
| `map.spawnAnim(anim: AnimTypeId, at: Coord): Status` | 创建动画 | native | `src/sim/anim_class.rs`（按坐标的入口；G-A1） | v1? | RFC 6b；F6 |
| `techno.remove(): Status` | 移除对象：不死亡、不产生幸存者、不计击杀 | none | `src/sim/world/lifecycle.rs::Simulation::uninit_with_context` | v1? | RFC 6b；F7 `Remove`；Rc：`DestroyUnit(reclaimed)` |
| `techno.kill(mode: KillMode, source: Techno): Status` | 走伤害流程击杀，伤害等于当前生命并带 `ignore_defenses`（有死亡动画和幸存者，击杀记给 `source`）；`source` 可为 `NO_HANDLE` | native | `src/sim/world/mod.rs::commit_direct_damage_receiver`（`EntityDamageEvent::direct_receiver`） | v1? | RFC 6b；F8 `Suicide` |
| `techno.damage(amount: i32, warhead: WarheadTypeId, source: Techno): Status` | 经过弹头和护甲造成伤害，计击杀和经验；`source` 可为 `NO_HANDLE` | native | `commit_direct_damage_receiver`；计算在 `combat/damage/receive.rs::receive_damage` | v1? | RFC 6b“修改生命（增减）”；F9；Rc：`AddUnitDamage`；**已在 RFC 0.0.5 采纳**（P9：增减走弹头流程） |
| `techno.setHealth(points: i32): Status` / `heal(points: i32)` | 设定或回复生命，不计击杀 | none | `combat/object_health.rs::Health::set`、`heal`（没有 Simulation 层的 owner） | v1? | RFC 6b；F9 `ModifyHealth` |
| `map.createObject(type: TechnoTypeId, at: Cell, owner: House, c: CreateParams): Techno` | 创建一个完整的对象；格子被占时拒绝（返回 `NO_HANDLE`） | native | `src/sim/world/world_spawn.rs::spawn_object_at_height`、`place_building_with_cleared_foundation`、`place_foot_with_cleared_cell`（文档注明“用于测试、工具、调试”） | v1? | RFC 6b；F10 `CreateUnit`（继承触发器和效果：`CreateParams.inheritTag`） |
| `techno.setMission(m: Mission, target: Target \| null): Status` | 像玩家指令一样排入一个任务 | none | `src/sim/mission/retask.rs::queue_order_mission`、`queue_megamission` | v1 | RFC 6b、示例；F16 `Mission`；F13 `CommandMoveTo`（= 带格子的 `Move`） |
| `techno.teleport(cell: Cell, rule: Blocked): Status` | 超时空式移动；`Blocked.Nearby` 选附近的格子，`Blocked.Fail` 直接拒绝 | native | **尚无 owner**（`superweapon/chronosphere.rs::warp_object` 是私有的；`trigger_chrono.rs::chronoshift_house_to` 作用于整个所属方） | v1? | RFC 6b；F12 `ForceMoveTo` |
| `techno.clearTarget(): Status` | 放弃当前目标 | none | **尚无 owner**（只有各移动器的 `clear_target` trait） | v1? | RFC 6b；F15 |
| `techno.clearDisguise(): Status` | 解除间谍或幻影坦克的伪装 | none | `src/sim/cloak_disguise.rs`（`DisguiseRuntime::clear_unit`；没有事务 owner） | v1? | RFC 6b；F14 |
| `objects.inRadius(...)` | 区域搜索（一个查询） | none | 第 7 节 | v1 | RFC 6b；F17 `SearchArea` |
| `sound.playAt(sound: SoundId, at: Coord)` | 播放声音 | none | `SimSoundEvent` | v1? | RFC 6b；F18 `PlayAudio` |
| `techno.addExperience(points: i32)` / `setRank(r: Rank)` | 增加经验或设定等级 | none | `src/sim/combat/veterancy.rs::Veterancy`（`set_elite`、`set_veteran`、`set_rookie`、`set_from_percent`） | v1? | RFC 6b；F19 `GetEXP` |
| `house.addMoney(amount: i32): Status` | 增加或扣除金钱 | none | `src/sim/economy.rs::Economy::add_credits`、`spend_money` | v1? | RFC 6b；F20 `GetMoney` |
| `rules.setTechLevel(type, level)` / `house.allowType(type, allowed)` | 修改科技树 | none | 已有 `set_object_type_tech_level`；按所属方修改前提条件：**尚无 owner** | v1? | RFC 6b；F21 `TechModifier` |
| `techno.setOwner(house: House): Status` | 转移所属 | none | `change_owner_with_rules` | v1? | RFC 6b；F22 |
| `techno.clearMindControl(): Status` | 解除心灵控制，把对象还给原所属方 | none | `src/sim/capture_manager.rs`（有原所属方记录；未确认有释放函数） | v1? | RFC 6b；F23 |
| `techno.setAmmo(n: i32)` / `addAmmo(n: i32)` | 修改弹药 | none | `src/sim/ammo.rs`（`add`、`decrease_by`） | v1? | RFC 6b；F24 |
| `transport.addPassenger(p: Techno)` / `removePassenger(p: Techno)` | 让乘客登上或离开 | native | **尚无 owner**（`PassengerCargo::board_forced_chain`、`disembark` 只是列表操作，不做 limbo/unlimbo） | v1? | RFC 6b；F25 |
| `techno.addEffect(...)` / `removeEffect(...)` | 见第 5 节 | — | 第 5 节 | v1 | RFC 6b；F2、F3 |
| `techno.addMark(...)` / `removeMark(...)` | 见第 6 节 | — | 第 6 节 | v1 | RFC 6b |
| `map.createProjectile(p: ProjectileParams): Projectile` | 创建抛射体：类型、起点、目标对象或坐标、用于击杀和经验归属的来源、16 字节的初始模块数据；失败时返回 `NO_HANDLE` | native | `src/sim/world/mod.rs::Simulation::admit_projectile` | v1? | RFC 6b、9；Rc：`SpawnProjectile` |
| `map.detonateWeapon(weapon: WeaponTypeId, at: Coord, source: Techno): Status` | 不生成抛射体，直接在某个坐标引爆武器；`source` 可为 `NO_HANDLE` | native | `src/sim/world/trigger_explosion.rs::trigger_explosion_at_waypoint`（只支持路径点；G-A1） | v1? | 带坐标的 A42（RFC 6：坐标作为参数） |
| `map.detonateWarhead(warhead: WarheadTypeId, damage: i32, at: Coord, source: Techno): Status` | 按给定伤害和来源造成弹头的范围伤害；`source` 可为 `NO_HANDLE` | native | `src/sim/combat/world_receiver.rs::apply_area_damage` | v1? | **已在 RFC 0.0.5 采纳**（P7）；Rc：`SpawnExplosion` |
| `projectile.detonate(): Status` | 让抛射体立即引爆 | native | `src/sim/projectile` 的引爆准入（没有公开入口） | later | **已在 RFC 0.0.5 采纳**（P8）；Rc：`SetProjectileCollision` |
| `projectile.remove(): Status` | 移除抛射体，不引爆 | none | **尚无 owner** | later | **已在 RFC 0.0.5 采纳**（P8）；Rc：`DeleteProjectile` |
| `scenario.endFor(house: House, outcome: Outcome): Status` | 让某个所属方结束游戏，胜利或失败 | none | `house_defeat.rs::request_player_outcome` | v1? | RFC 6b |
| `house.setScore(points: i32)` / `addScore` | 设置分数 | none | `Economy::add_score`（没有设定函数） | v1? | RFC 6b |
| `ctx.log(text: string)` | 向游戏内日志写一行 | none | **尚无 owner** | v1 | RFC 6b、10 |
| `techno.convert(type: TechnoTypeId): Status` | 改变类型（同 Ares 的类型转换） | native | `src/sim/game_entity/type_conversion.rs::convert_techno_type` | v1? | RFC 8（按对象换类型）；S68 |

```ts
class WeaponSel { index: i32 = -1; weapon: WeaponTypeId = NO_ID; }
class Target { techno: Techno = NO_HANDLE; cell: Cell | null = null; current: bool = false; }
enum KillMode { Normal }          // death animation, survivors, credit
enum Blocked { Nearby, Fail }
class CreateParams { inheritTag: Techno = NO_HANDLE; mission: Mission = Mission.None; }
class ProjectileParams {
  type: ProjectileTypeId;
  weapon: WeaponTypeId = NO_ID;   // supplies damage and warhead
  from: Coord;
  toTechno: Techno;          // may be NO_HANDLE
  toCoord: Coord | null;
  source: Techno;            // kill credit and experience; may be NO_HANDLE
  data: Uint8Array | null;   // up to 16 bytes, saved and hashed
}
```

## 5. 持续效果

owner：`src/sim/attach_effect.rs`（`AttachEffectList`、`AttachEffectInstance`），类型读取器是 `src/rules/attach_effect.rs`（`[AttachEffectTypes]`）。两者目前大部分还在 `cfg(feature = "phobos")` 后面；RFC 8 要求先把它们迁到运行时开关（缺口 G-E1）。唯一完整的附加流程是弹头路径 `Simulation::apply_warhead_attach_effects`；没有“把这种效果类型附加到这个对象上”的 Simulation 函数（缺口 G-E2）。

### 5.1 函数

```ts
class EffectParams {
  duration: Frames = -1;      // -1: the type's Duration
  delay: Frames = -1;         // -1: none
  source: Techno = NO_HANDLE;
  sourceHouse: House = NO_HANDLE;
}
// on Techno:
addEffect(type: EffectTypeId, p: EffectParams): Effect;   // NO_HANDLE + Refused when stacking refuses
removeEffect(e: Effect): Status;
removeEffects(type: EffectTypeId): i32;    // returns how many
removeEffectGroup(group: EffectGroupId): i32;
effects(out: HandleBuffer): i32;           // deterministic order: attach order
// on Effect:
type(): EffectTypeId; remaining(): Frames; stacks(): i32; source(): Techno;   // source() may be NO_HANDLE
```

| 函数 | Owner | 层级 | 来源 |
| --- | --- | --- | --- |
| `addEffect` | `AttachEffectList::attach(type_index, AttachEffectParams, &AttachEffectType, AttachPolicy, source)` 加上对外发布：**尚无 owner** | v1 | RFC 6b；F2 `AddBehavior`；Ph: `AttachEffect.AttachTypes` |
| `removeEffect`、`removeEffects` | `remove_attach_effect_type`（私有） | v1 | RFC 6b；F3；Ph: `AttachEffect.RemoveTypes` |
| `removeEffectGroup` | 同上 | v1 | Ph: `AttachEffect.RemoveGroups` |
| `effects`、`remaining`、`stacks` | `AttachEffectList::instances`、`AttachEffectInstance::remaining`、`active_count` | v1 | RFC 6c |

效果类型在 INI 中定义（`[AttachEffectTypes]`，Phobos 读取器），或由 `onRules` 写入这些节。新增属性使用同一个读取器（RFC 6b）。

### 5.2 效果属性

| 属性 | Phobos 键（读取器 `src/rules/attach_effect.rs`） | 含义 | 来源 |
| --- | --- | --- | --- |
| 持续时间 | `Duration`；弹头一侧为 `AttachEffect.Durations`、`AttachEffect.DurationOverrides` | 单位为帧；负数表示无限（未核实） | RFC 6b；C3 |
| 延迟 | `AttachEffect.Delays`、`AttachEffect.InitialDelays`（附加方一侧） | 效果开始前的帧数 | RFC 6b；C12 |
| 间隔 | 未找到 Phobos 键 | 两次触发之间的帧数 | RFC 6b；C13；**尚无读取器** |
| 重新附加前的冷却 | `AttachEffect.RecreationDelays`（附加方一侧） | 到期后要等多少帧才能再次附加 | RFC 6b；C11 |
| 最大触发次数 | 未找到 Phobos 键（`DiscardOn.Firing.Count` 是开火若干次后丢弃，含义不同） | | RFC 6b；C8；**尚无读取器** |
| 来源 | `AttachEffect.SameSourceOnly`、`AttachEffect.IgnoreFromSameSource`、`AttachEffect.CumulativeSourceMaxCount` | 关于效果来源的规则 | RFC 6b；C5 |
| 变形时继承 | `AllowTransfer.Convert` | 对象转换类型时保留 | RFC 6b；C6 |
| 创建时继承 | `AllowTransfer`；`AttachEffects.AttachOnOwnerChange`（语义未核实） | | RFC 6b；C6 |
| 无视铁幕 | `PenetratesIronCurtain` | | RFC 6b；C7 |
| 叠加 | `Cumulative`、`Cumulative.MaxCount`（负数表示无上限）；`AttachEffect.ReplaceLongerDuration`；`AttachEffect.CumulativeRefreshAll`、`.OnAttach`、`AttachEffect.CumulativeRefreshSameSourceOnly` | 见 5.3 | RFC 6b；C2、C4 |
| 类型分组 | `Groups` | 供 `RemoveGroups`、`RequiredGroups`、`DisallowedGroups` 使用的名字 | C9 |
| 标签文字 | — | 标记（第 6 节） | C10 |
| 计数器 | — | 不进入引擎（RFC 6a） | C17 |
| 需要电力 | `Powered` | 只在对象有电时生效 | Ph |
| 丢弃 | `DiscardOn`、`DiscardOn.*` | 在移动、开火、受伤、任务、地形类型等情况下结束 | Ph |
| 到期武器 | `ExpireWeapon`、`ExpireWeapon.TriggerOn`、`.CumulativeOnlyOnce`、`.UseInvokerAsOwner` | 到期时发射一次武器 | Ph |

### 5.3 叠加与互斥组

RFC 要求每种效果类型声明自己的叠加方式（待定问题 14）。提议按下表映射到 Phobos 读取器：

| 规则 | Phobos 键 |
| --- | --- |
| 叠加到上限 | `Cumulative=yes`、`Cumulative.MaxCount=N` |
| 新的替换旧的 | `Cumulative=no`（刷新已有实例；具体刷新规则见 `should_refresh`，未核实） |
| 拒绝新的 | **尚无键**（新属性，同一个读取器） |
| 互斥组 | **尚无键**：提议用 `ExclusiveGroup=`（新属性；名称未核实） |

RFC 把变形、传送和运动放在同一个互斥组里，由 owner 保证同一时间只有一个生效。

### 5.4 效果种类

| 效果（RFC 6b） | `[AttachEffectType]` 上的 Phobos 键 | 默认叠加方式 | 互斥组 | Owner | 层级 | 来源 |
| --- | --- | --- | --- | --- | --- | --- |
| 染色 | `Tint.Color`、`Tint.Intensity`、`Tint.VisibleToHouses`、`Tint.Cumulative` | 替换 | — | `attach_effect.rs` 加渲染 | v1 | RFC 6b；F30 `Paintball` |
| 附着动画 | `Animation`、`CumulativeAnimations`、`Animation.OfflineAction`、`Animation.TemporalAction`、`Animation.UseInvokerAsOwner`、`Animation.HideIfAttachedWith` | 替换 | — | `attach_effect.rs` | v1 | RFC 6b；F31 |
| 阻止死亡 | `Unkillable` | 替换 | — | `attach_effect.rs::apply_unkillable_clamp` | v1 | RFC 6b；F32 |
| 变成另一类型 | 无键 | 拒绝 | 运动组 | `type_conversion.rs::convert_techno_type`；效果：**尚无 owner** | v1? | RFC 6b；F33 |
| 替换武器 | 无键 | 替换 | — | **尚无 owner** | v1? | RFC 6b；F34 `OverrideWeapon` |
| 运动（被抛起、被推动） | 无键 | 拒绝 | 运动组 | **尚无 owner**（叠加到移动上，从不以坐标形式写入） | later | RFC 6b；F35 `Force`；Rc: MoveCtrl |
| 传送 | 无键 | 拒绝 | 运动组 | **尚无 owner** | v1? | RFC 6b；F36 |
| 受伤倍率 | `ArmorMultiplier`（及 `.AllowWarheads`、`.DisallowWarheads`、`.Chance`、`.Delay`、`.AffectsHouse`、`.HitAnim`） | 叠加 | — | `attach_effect.rs::attach_effect_armor_multiplier` | v1 | RFC 6b；F37 `Status` |
| 火力倍率 | `FirepowerMultiplier` | 叠加 | — | `stat_multipliers` | v1 | RFC 6b；F37 |
| 速度倍率 | `SpeedMultiplier` | 叠加 | — | `stat_multipliers` | v1 | RFC 6b；F37 |
| 射速倍率 | `ROFMultiplier`、`ROFMultiplier.ApplyOnCurrentTimer` | 叠加 | — | `stat_multipliers` | v1 | RFC 6b；F37 |
| 射程 | `WeaponRange.Multiplier`、`WeaponRange.ExtraRange`、`.AllowWeapons`、`.DisallowWeapons` | 叠加 | — | `range_with_attach_effects` | v1 | RFC 6b；F37 |
| 目标优先级倍率 | 无键 | 叠加 | — | **尚无 owner**（目标扫描 `src/sim/world/techno_ai/scan_batch.rs`） | v1? | RFC 6、6b（P13）；与 `TargetChanged`（2.3）配合 |
| 隐形 | `Cloakable`；`ForceDecloak` | 替换 | — | `attach_effect.rs` 加 `cloak_disguise.rs` | v1 | RFC 6b；F37 |
| 对某弹头或攻击类型免疫 | 部分可用倍率为 0 的 `ArmorMultiplier.AllowWarheads`（未核实）；没有免疫键 | 替换 | — | **尚无 owner** | v1? | RFC 6b；F40 `ImmuneTo` |
| 禁止开火 | `DisableWeapons` | 替换 | — | `weapon_attach_effects_allow` | v1 | RFC 6b；F41 `Silence` |
| 停止索敌 | 无键 | 替换 | — | **尚无 owner** | v1? | RFC 6b；F42 |
| 伤害修正（固定加值、强制取值） | 无键（`ReflectDamage.*` 是反弹伤害，不是修正） | 叠加 | — | **尚无 owner** | v1? | RFC 6b；F43 |
| 无法被选中 | 无键 | 替换 | — | **尚无 owner**（客户端和指令校验） | v1? | RFC 6b；F45 |
| 无视玩家指令 | 无键 | 替换 | — | **尚无 owner**（指令准入 `src/sim/command.rs`） | v1? | RFC 6b；F47 |
| 不可被选为目标 | 无键 | 替换 | — | **尚无 owner**（目标扫描） | v1? | RFC 6b；F48 |
| 所属方断电 | 无键 | 替换 | — | **尚无 owner**（`PowerState::start_blackout` 作用于整个所属方且有时限） | v1? | RFC 6b；F49 |
| 暴击 | `Crit.Multiplier`、`Crit.ExtraChance`、`Crit.AllowWarheads`、`Crit.DisallowWarheads` | 叠加 | — | `attach_effect.rs` | v1 | Ph |
| 反弹伤害 | `ReflectDamage`、`ReflectDamage.*` | 叠加 | — | `apply_attach_effect_reflect_damage` | v1 | Ph |
| 复仇武器 | `RevengeWeapon`、`RevengeWeapon.*` | 叠加 | — | `apply_attach_effect_revenge_weapons` | v1 | Ph |
| 激光拖尾 | `LaserTrail.Type` | 替换 | — | `attach_effect.rs` | v1 | Ph |
| 穿透力场护盾 | `PenetratesForceShield` | 替换 | — | `attach_effect.rs` | v1 | Ph |

表格中未作为效果采用的持续效果：

| 行 | 效果 | 去向 |
| --- | --- | --- |
| F38 `Info`、F44 `ShowCSF`、F50 `ShowButton` | 对象上方的文字、数值、按钮 | 客户端显示属性（RFC 6b、10；第 11 节） |
| F39 `Stand` | 替身对象 | RFC 未涉及；用模块代码加创建/移除实现 |
| F46 `SearchOnAllies` | 在盟友中搜索目标 | RFC 未涉及；`later` 的候选 |
| F51 `Recorder`、F52 `Operator`、F53 记录器用法 | 记录器和算子 | 不进入引擎（RFC 6a） |
| F54 `Mark.Names` | 标记 | 第 6 节 |

## 6. 标记

标记是对象携带的文字记号，一个对象可以携带任意多个（RFC 6b、术语）。动作和效果负责添加和移除，约束条件负责判断。owner：**尚无 owner**（缺口 G-M1）：需要一个按句柄索引的旁路表（RFC 15），存档并计入哈希，对象移除时清空。调用传入由 `Mark.id(name)` 得到的 `MarkId`（见 [ID](#id)）。

```ts
// on Techno:
addMark(m: MarkId): Status;
removeMark(m: MarkId): Status;
hasMark(m: MarkId): bool;
marks(out: IdBuffer): i32;    // order: the order the marks were added
```

| 条目 | 语义 | 层级 | 来源 |
| --- | --- | --- | --- |
| `addMark` | 添加 `m`；重复添加只保留一个 | v1 | RFC 6b；F54 `Mark.Names` |
| `removeMark` | 移除 `m`；不存在时返回 `Ok` | v1 | RFC 6b |
| `hasMark` | 按 ID 判断；解析名字时不区分大小写 | v1 | RFC 6b；C10、C11 |
| `marks` | 把对象的标记写入 `out` | v1 | RFC 6c（对象读取中的“标记”） |
| 效果携带的标记 | 效果类型列出若干标记，效果生效期间对象携带它们（新属性，例如 `Marks=`；名称未核实） | v1? | F54“持有带它的行为就获得这个标签” |
| 标记的值和可见性 | **已在 RFC 0.0.5 采纳**（P3） | later | Rc: 带访问级别的 rules params |
| 所属方标记 | **已在 RFC 0.0.5 采纳**（P3） | later | Rc: `SetTeamRulesParam` |

客户端模块只能读取自己看得到的对象的标记，而且只能读取可见度等级允许其玩家看到的标记（RFC 6b，P3）。

## 7. 查询

查询是只读的（RFC 6c）。结果按对象创建顺序返回（**已在 RFC 0.0.5 采纳**，P6），这个顺序来自引擎维护的创建序号（RFC 15）；结果写入模块提供的缓冲区：查询返回写入的数量，结果多时分页返回（见[查询结果](#查询结果)）。在客户端模块中，每个查询都按本地玩家的视野过滤（RFC 10）。

```ts
class Filter {
  house: House = NO_HANDLE;         // NO_HANDLE: any
  relationTo: House = NO_HANDLE;  relation: i32 = -1;   // Relation, -1 any
  category: i32 = -1;               // Category, -1 any
  type: TechnoTypeId = NO_ID;       // NO_ID: any
  mark: MarkId = NO_ID;             // NO_ID: any
}
// results: HandleBuffer or IdBuffer (Conventions)
```

### 7.1 对象

| 函数 | 返回 | 读取的 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `objects.inCell(c: Cell, f: Filter, out: HandleBuffer)` | `i32` 数量 | 格子占用（owner 未核实） | v1 | RFC 6c |
| `objects.inRadius(at: Coord, leptons: i32, f: Filter, out: HandleBuffer)` | `i32` 数量 | 空间索引（owner 未核实） | v1 | RFC 6c；F17；Rc: `GetUnitsInCylinder` |
| `objects.inRect(r: Rect, f: Filter, out: HandleBuffer)` | `i32` 数量 | 同上 | v1 | RFC 6c；Rc: `GetUnitsInRectangle` |
| `objects.all(f: Filter, out: HandleBuffer)` | `i32` 数量 | 实体存储；按所属方的类型用 `HouseTracking` | v1 | RFC 6c |
| `objects.nearest(at: Coord, leptons: i32, f: Filter)` | `Techno`（没有时为 `NO_HANDLE`） | 同上 | v1? | Rc: `GetUnitNearestEnemy` |
| `objects.projectilesInRadius(at: Coord, leptons: i32, out: HandleBuffer)` | `i32` 数量 | 抛射体存储 | v1? | RFC 6c（抛射体类别） |
| `techno.type()` | `TechnoTypeId` | `ObjectType` ID | v1 | RFC 6c |
| `techno.owner()` | `House` | 实体的所属方 | v1 | RFC 6c |
| `techno.health()`、`strength()`、`healthPercent` | `i32` | `combat/object_health.rs::Health`；`Strength` | v1 | RFC 6c、示例 |
| `techno.coord()`、`cell()` | `Coord`、`Cell` | 实体位置 | v1 | RFC 6c |
| `techno.mission()` | `Mission` | `src/sim/mission/state.rs` | v1 | RFC 6c |
| `techno.target()` | `Target` | 目标状态（owner 未核实） | v1 | RFC 6c |
| `techno.rank()`、`experience()` | `Rank`、`i32` | `combat/veterancy.rs` | v1 | RFC 6c |
| `techno.ammo()` | `i32` | `src/sim/ammo.rs` | v1 | RFC 6c |
| `techno.passengers(out: HandleBuffer)`、`transport()` | `i32` 数量、`Techno`（没有时为 `NO_HANDLE`） | `src/sim/passenger.rs::PassengerCargo::passengers` | v1 | RFC 6c |
| `techno.effects(out: HandleBuffer)`、`marks(out: IdBuffer)` | `i32` 数量 | 第 5、6 节 | v1 | RFC 6c |
| `techno.disguise()` | `DisguiseInfo \| null`（类型 ID、所属方） | `src/sim/cloak_disguise.rs::forced_disguise_type`、`disguise_house` | v1 | 模拟模块读到的是真实状态 |
| `techno.isCloaked()` | `bool` | `cloak_disguise.rs::is_fully_cloaked` | v1? | |
| `techno.layer()`、`landType()` | 枚举 | 实体所在层；地图 | v1 | RFC 6a 约束条件 |
| `projectile.type()`、`coord()`、`source()`、`target()`、`data()` | 值；`type()` 是 `ProjectileTypeId` | `src/sim/projectile` | v1? | RFC 9 |
| `objects.canFireAt(t: Techno, w: i32, at: Target)`、`inRange(...)` | `bool` | **尚无 owner**（结构化的合法性探测） | later | **已在 RFC 0.0.5 采纳**（P15）；Rc: `GetUnitWeaponTestTarget` |

### 7.2 所属方

| 函数 | 返回 | 读取的 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `house.name()`、`country()`、`side()` | `string`、`string`、`i32` | `src/sim/house_state.rs::HouseState`（`name`、`country`、`side_index`） | v1 | RFC 6c |
| `house.money()` | `i32` | `Economy::available_money` | v1 | RFC 6c |
| `house.powerOutput()`、`powerDrain()`、`isLowPower()` | `i32`、`bool` | `src/sim/power_system.rs::PowerState::total_output`、`total_drain`、`is_low_power` | v1 | RFC 6c |
| `house.buildings(f, out: HandleBuffer)` / `count(type: TechnoTypeId)` | `i32` 数量、`i32` | `src/sim/house_tracking.rs::HouseTracking` | v1 | RFC 6c |
| `house.canBuild(type: TechnoTypeId)` | `bool`（前提条件已满足） | `src/sim/production/can_build.rs::can_build`、`requirements_met`、`owns_prerequisites` | v1 | RFC 6c |
| `house.techLevel()` | `i32` | `HouseState::tech_level` | v1 | RFC 6c |
| `house.isAlly(other)`、`allies(out: HandleBuffer)`、`enemies(out: HandleBuffer)` | `bool`、`i32` 数量 | `src/sim/house_diplomacy.rs` | v1 | RFC 6c |
| `house.isDefeated()`、`isHuman()` | `bool` | `HouseState.is_defeated`、`is_human` | v1 | RFC 6c |
| `house.superWeapon(type: SuperWeaponTypeId)` | `SuperWeapon`（该所属方不拥有它时为 `NO_HANDLE`；可读取是否就绪、充能） | `src/sim/superweapon/mod.rs::HouseSuperWeapons` | v1? | |
| `house.stats()` | 损失数和摧毁数 | `MatchStatistics` | v1? | E15、E16 |
| `scenario.houses(out: HandleBuffer)` | `i32` 数量，顺序为所属方数组的顺序 | `src/sim/houses.rs::Houses` | v1 | RFC 6c |

### 7.3 地图

| 函数 | 返回 | 读取的 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `map.size()` | `Rect` | 地图 | v1 | RFC 6c |
| `map.landType(c: Cell)` | `LandType` | 地形语义（`src/rules/terrain_rules.rs`） | v1 | RFC 6c |
| `map.height(c: Cell)` | `i32` | 格子高度 | v1 | RFC 6c |
| `map.overlay(c: Cell)`、`ore(c: Cell)` | `OverlayTypeId`（没有时为 `NO_ID`）；数量 | 覆盖物和矿石状态 | v1 | RFC 6c |
| `map.passable(c: Cell, zone: i32)` | `bool` | 移动区域检查（owner 未核实） | v1 | RFC 6c |
| `map.isVisibleTo(c: Cell, h: House)` | `bool` | `src/sim/vision/mod.rs::is_cell_revealed(owner, rx, ry)` | v1 | RFC 6c |
| `map.waypoint(n: i32)` | `Cell \| null` | 地图路径点 | v1 | RFC 6c |
| `map.cellTag(c: Cell)` | `Tag`（没有时为 `NO_HANDLE`） | `TriggerRuntime.cell_tags` | v1 | RFC 6c |
| `map.canPlace(type: TechnoTypeId, c: Cell, h: House)` | `bool` | `src/sim/build_site.rs::can_place_building_at` | v1? | **已在 RFC 0.0.5 采纳**（P15）；Rc: `TestBuildOrder` |
| `map.closestPlaceable(type: TechnoTypeId, near: Cell, h: House)` | `Cell \| null` | **尚无 owner**（`src/sim/ai_base_site.rs` 有一个 AI 用的版本） | later | P15；Rc: `ClosestBuildPos` |

### 7.4 规则

| 函数 | 返回 | Owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `rules.getString(section: string, key: string)` | `string \| null` | 处理后的规则（`ProcessedRulesLayers::ini()` 投影） | v1 | RFC 6c |
| `rules.getInt(section, key, fallback: i32)` | `i32` | `src/rules/ini_value.rs` 中的读取器 | v1 | RFC 6c |
| `rules.getBool(section, key, fallback: bool)` | `bool` | 同上 | v1 | RFC 6c |
| `rules.getFixed(section, key, fallback: Fixed)` | `Fixed` | 同上，转换为 I16F16 | v1 | RFC 6c |
| `rules.getList(section, key)` | `StringList` | 同上 | v1 | RFC 6c |
| `rules.types(list: string)` | `StringList`（例如 `"InfantryTypes"`） | `RuleSet` 中的注册表 | v1 | RFC 6c |

解析后的值就是引擎实际使用的值；Ares、Phobos 启用时也包括它们的键（RFC 8）。引擎从不读取的键，仍然从合并后的文本中给出值。

### 7.5 场景

| 函数 | 返回 | Owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `ctx.frame()` | `i32` | 模拟帧 | v1 | RFC 6c |
| `scenario.difficulty()` | `Difficulty` | `TR::trigger_difficulty` / 场景 | v1 | RFC 6c |
| `scenario.gameMode()` | `string` | 会话（owner 未核实） | v1 | RFC 6c |
| `scenario.playerCount()` | `i32` | `Houses` | v1 | RFC 6c |
| `scenario.timer.remaining()` | `Frames` | `scenario_countdown.rs` | v1 | E14 |
| `ctx.global(i).get()`、`ctx.local(i).get()` | `bool` | `TR::global_variable_is_set`、`local_variable_is_set` | v1 | RFC 示例 |
| `Trigger.find(id)`、`Tag.find(id)` | 句柄 | `TR` 中的定义 | v1 | RFC 7 |

### 7.6 以所属方身份查询

```ts
// sim:
ctx.asHouse(h: House): View;   // View has objects, map, houses, filtered like a client of h
```

用所属方 `h` 的视野过滤来运行同样的查询。这样模块 AI 可以公平对战，任务逻辑也可以问“所属方 H 是否知道 X”。模拟模块的默认查询仍然掌握全部信息。层级为 `later`，**RFC 0.0.5 未采纳**（P4）。owner：客户端视野过滤（G-C3）。Rc: `CallAsTeam`。

### 7.7 客户端结果

在客户端模块中（RFC 10）：

- 黑幕下的对象不存在；迷雾下、玩家当前看不到的敌方对象也不存在：引擎不保存“最后一次看到”的副本（RFC 10，P17）；
- 伪装的敌方对象读出来是它伪装成的样子：`type()` 返回伪装类型，`owner()` 返回伪装的所属方，使用 `src/sim/cloak_disguise.rs::is_disguised_to`、`disguise_house`、`disguise_hides_house_from`。过滤器的 owner：**尚无 owner**（G-C3）。Rc: 向非盟友返回诱饵定义；
- 隐形的敌人除非被侦测到，否则不存在；
- 观战者看到观战者能看到的范围，房间允许时为整张地图。

## 8. 覆盖点

覆盖点读取状态并返回结果，不能执行动作（RFC 5）。它们以游戏概念命名。多个模块之间按**链式**处理：mod 的模块先算出结果，地图的模块拿到这个结果后可以再改，引擎使用最终值（RFC 2）。已停用模块的覆盖点会被跳过。版本 1 提供哪两三个，是待定问题 11。

组合规则（RFC 第 2 节，采纳自 P1）：链上每一环都拿到原生值和上一环的结果，后一环可以推翻前一环的否决，数值类覆盖点返回明确的“已处理”标志。

覆盖点绑定到类型，从不全局生效：由武器、弹头或 TechnoType 在 INI 中指定覆盖点，引擎只对这些类型调用模块（**已在 RFC 0.0.5 采纳**，P12；绑定键的名字是待定问题 5）。查询类和结果类（`DamageQuery`、`ShotQuery`、`OrderQuery`、`DeathQuery`、`DeathResult`、`TargetBatch`）携带句柄和 ID（弹头、武器、类型），从不携带名字；批量数据直接写入模块内存，再从中读回（RFC 15）。

| 覆盖点 | 签名 | 频率 | 批处理 | 决策 owner | 层级 | 来源 |
| --- | --- | --- | --- | --- | --- | --- |
| `Override.Damage` | `(ctx: Ctx, d: DamageQuery, current: i32): DamageResult`（伤害值、`handled`） | 绑定的弹头或受伤方类型每产生一个伤害包调用一次 | 无（每个伤害包调用一次；预计绑定类型的调用次数不多） | `src/sim/combat/damage/receive.rs::receive_damage` | later（v1 候选） | RFC 6 L3；Rc: `UnitPreDamaged`；LUS `HitByWeapon` |
| `Override.TargetChoice` | `(ctx: Ctx, batch: TargetBatch): void`，为每个搜索者写入一个选择（或“保持原生结果”） | 每个绑定的武器类型每帧一次，带上该类型的全部搜索者及其原生候选 | **批处理**，从不按单个候选调用 | 目标扫描 `src/sim/world/techno_ai/scan_batch.rs` | later（v1 候选） | RFC 6 L3；Rc: `AllowWeaponTarget`（按候选调用是热点，#3087） |
| `Override.ProductionAllowed` | `(ctx: Ctx, house: House, type: TechnoTypeId, current: bool): bool` | 引擎检查绑定类型能否建造时（刷新侧栏、AI 选择） | 无；结果可以按帧缓存（未核实） | `src/sim/production/can_build.rs::can_build` | later（v1 候选） | RFC 6 L3；Rc: `AllowUnitCreation` |
| `Override.Fire` | `(ctx: Ctx, shot: ShotQuery): bool`，返回 `true` 表示接管这次射击，原生抛射体不再生成 | 绑定武器每次射击 | 无 | FireAt 步骤 `src/sim/combat/world_fire.rs`；飞行器为 `fire_at.rs` | later（v1 候选） | RFC 6 L3、9 |
| `onLaunch` | `(ctx: LaunchCtx, p: Projectile): Path \| null` | 绑定类型的每个抛射体一次 | 无 | `ProjectileTrajectory` 的模块分支（RFC 9）：**尚无 owner** | later | RFC 9 第 3 层 |
| `onTrajectoryFrame` | `(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | 每个绑定的抛射体类型每帧一次 | 按类型**批处理**（L4） | 同上 | later | RFC 6 L4、9 第 4 层 |
| `Override.CommandAllowed` | `(ctx: Ctx, t: Techno, order: OrderQuery, current: bool): bool` | 绑定类型每收到一条玩家或 AI 指令 | 无 | 指令准入 `src/sim/command.rs`（具体函数未核实） | later | **已在 RFC 0.0.5 采纳**（P13）；Rc: `AllowCommand` |
| `Override.Death` | `(ctx: Ctx, t: Techno, q: DeathQuery): DeathResult`（死亡武器、幸存者、残骸） | 绑定类型每次死亡 | 无 | 死亡流程 `combat/record_kill.rs` 和幸存者规则（未核实） | later | **已在 RFC 0.0.5 采纳**（P13）；LUS `Killed -> wreckLevel` |

RFC 示例中的覆盖点（选择目标、伤害、是否允许生产、接管武器的一次射击）都已列出。目标优先级方面“优先用数据加事件”的做法（把优先级倍率做成持续效果，再加一个 `TargetChanged` 事件）**已在 RFC 0.0.5 采纳**（P13，RFC 第 6 节）。

## 9. AI

电脑所属方保留它们的 AI（RFC 6d）。模块通过 `src/sim/ai_team_creation.rs` 和 `TSV` 中现有的 owner，创建特遣部队和小队类型、指派小队脚本、启动和解散小队，以及开关 AI 触发器。所有条目都属于 `sim`。

### 9.1 函数

| 函数 | 语义 | Owner | 层级 | 来源 |
| --- | --- | --- | --- | --- |
| `ai.defineTaskForce(id: string, d: TaskForceDef): Status` | 新增或替换一个特遣部队 | `TeamScriptVm::register_task_force` | v1 | RFC 6d |
| `ai.defineScript(id: string, lines: ScriptLine[]): Status` | 新增或替换一个小队脚本（最多 50 行） | `TeamScriptVm::register_script` | v1 | RFC 6d |
| `ai.defineTeamType(id: string, d: TeamTypeDef): Status` | 新增或替换一个小队类型 | `TeamScriptVm::register_team_type` | v1 | RFC 6d |
| `ai.defineAiTrigger(id: string, d: AiTriggerDef): Status` | 新增或替换一个 AI 触发器（把当前权重重置为初始权重） | `TeamScriptVm::register_ai_trigger` | v1 | RFC 6d |
| `ai.createTeam(teamType, house)` | 启动一个小队（受 Max 限制） | `src/sim/team_script_vm/reinforcements.rs::Simulation::create_team` | v1 | RFC 6d；A4 |
| `ai.createTeamUnlimited(teamType, house)` | 启动一个小队，不受 Max 限制 | `Simulation::construct_team_unlimited` | v1? | S18 的路径 |
| `team.disband()` | 销毁小队，记录 AI 触发器的结果，释放成员 | `TSV/membership.rs::destroy_team` | v1 | RFC 6d；A5 |
| `team.addMember(t: Techno)` / `removeMember(t: Techno)` | 加入或离开（会执行 `team_can_add`） | `team_add_member`、`team_remove_member` | v1? | |
| `team.setScript(script: ScriptTypeId)` | 修改运行中小队的脚本 | **尚无 owner**（只有 S17 内联写入） | v1 | RFC 6d“指派 AI 小队脚本” |
| `team.jumpTo(line: i32)` | 移动小队的脚本游标 | **尚无 owner**（只有 S6 内联写入，以及仅供测试的辅助函数） | v1? | |
| `ai.setAiTriggerEnabled(trigger: AiTriggerId, on: bool)` | 启用或禁用一个 AI 触发器 | **尚无 owner**（`enabled` 只在加载时设置） | v1 | RFC 6d |
| `ai.setAiTriggerWeights(trigger: AiTriggerId, start, min, max)` | 运行时修改权重 | **尚无 owner**（`ai_trigger_records` 是私有的） | later | |
| `house.setAiTriggersActive(on)` | 所属方级别的开关 | `HouseState::set_ai_triggers_active` | v1 | A74、A75 |
| `house.setTeamRatio(kind, v)` | 所属方的比例 | `HouseTeamCreation::set_trigger_ratio` 及同类函数 | v1 | A76–A79 |
| 读取：`ai.teams(out: HandleBuffer)`、`team.type()`、`team.script()`、`team.cursor()`、`team.members(out: HandleBuffer)`、`team.succeeded()` | | `TeamScriptVm::teams_in_order`、`TeamScriptState` 的读取函数 | v1 | |

### 9.2 定义

定义属于初始化调用，接受名字；运行时的调用使用 `ai.teamTypeId`、`ai.scriptTypeId` 和 `ai.aiTriggerId` 返回的 ID（见 [ID](#id)）。

```ts
class TaskForceEntry { count: i32; type: string; }
class TaskForceDef { entries: TaskForceEntry[]; group: i32 = -1; }   // at most 6 entries
class ScriptLine { action: i32; argument: i32; }                     // at most 50 lines
class TeamTypeDef {          // keys of [TeamTypes] sections; defaults from the reader
  name: string | null = null;     // default: the ID
  script: string; taskForce: string;
  priority: i32 = 7; max: i32 = -1; group: i32 = -1;
  house: string | null = null; veteranLevel: i32 = 1;
  waypoint: i32 = -1; transportWaypoint: i32 = -1;
  isBaseDefense: bool = false; suicide: bool = false; aggressive: bool = false;
  useTransportOrigin: bool = false; full: bool = false; whiner: bool = false;
  autocreate: bool = false; areTeamMembersRecruitable: bool = true;
  reinforce: bool = false; avoidThreats: bool = false; onTransOnly: bool = false;
  recruiter: bool = false; annoyance: bool = false; guardSlower: bool = false;
  transportsReturnOnUnload: bool = false; mindControlDecision: i32 = 0;
  droppod: bool = false; onlyTargetHouseEnemy: bool = false; loadable: bool = false;
  tag: string | null = null;
}
class AiTriggerDef {         // the 18 tokens of an [AITriggerTypes] row
  name: string; primaryTeam: string | null; owner: string | null;   // null: <all>
  condition: i32 = -1; objectType: string | null = null;
  amount: i32 = 0; compare: Compare = Compare.Less;   // comparator bytes 0..8
  startWeight: i32; minWeight: i32; maxWeight: i32;
  multiplayer: bool; side: i32 = 0; secondaryTeam: string | null = null;
  easy: bool = true; normal: bool = true; hard: bool = true;
}
```

字段来自 `src/rules/team_ai_ini.rs` 和 `TSV/registry_install.rs::resolve_ini_definitions`。`Whiner` 会被读取，但没有使用者。`TechLevel`、`Prebuild`、`LooseRecruit` 和 `IonImmune` 出现在原版 AIMD.INI 中，但不会被读取，所以不作为字段。AI 触发器的第 3、11、13 个字段没有被使用，也不作为字段。AI 触发器的条件：-1 总是成立；0 敌方拥有 N 个该类型；1 本所属方拥有；2 敌方电力盈余低于 100；3 低于 0；4 比较敌方金钱；5 己方铁幕即将就绪；6 己方超时空传送仪即将就绪；7 平民阵营的所属方拥有（`condition_holds`）。

### 9.3 小队脚本动作

`ScriptLine.action` 取以下编号。分派函数：`TSV/actions.rs::team_run_action`。作为数据，全部是 `v1`。“A” = `TSV/actions.rs`，“SA” = `super_actions.rs`，“FF” = `follow_friendlies.rs`，“SH” = `scout_houses.rs`。“对应的原子动作”一列给出模块要对单个对象做同样的事时调用的函数。

| # | `ScriptAction` | 参数 | Rust | 对应的原子动作 |
| --- | --- | --- | --- | --- |
| 0 | `AttackQuarry` | 目标类别（2..11） | A `team_action_attack_quarry` | `setMission(Hunt)` |
| 1 | `AttackWaypoint` | 路径点 | A `team_action_attack_waypoint` | `setMission(Attack, cell)` |
| 2 | `GoBerserk`（惯用名） | — | 内联的空操作，永不结束 | — |
| 3 | `MoveToWaypoint` | 路径点 | A `team_action_move_to_waypoint` | `setMission(Move, cell)` |
| 4 | `MoveToCell` | 打包的格子（x = arg%128，y = arg/128） | 内联 | `setMission(Move, cell)` |
| 5 | `Guard` | `arg*15` 帧 | 内联 | `setMission(Guard)` |
| 6 | `JumpToLine` | 从 1 开始的行号 | 内联 | `team.jumpTo` |
| 7 | `PlayerWins` | — | 内联 `request_player_outcome(Victory)` | `scenario.endFor` |
| 8 | `Unload` | 1、2、3 | A `team_action_unload` | `removePassenger` |
| 9 | `Deploy` | — | A `team_action_deploy` | — |
| 10 | `FollowFriendlies` | — | FF `team_action_follow_friendlies` | — |
| 11 | `DoMission` | 任务 ID | A `team_action_do_mission` | `setMission` |
| 12 | `SetGlobal` | 下标 | 内联 `set_trigger_variable` | `ctx.global(i).set(true)` |
| 13 | `IdleAnimation` | 动画 | 内联 `foot_play_idle_animation` | — |
| 14 | `LoadOntoTransport` | — | A `team_action_load_onto_transport` | `addPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | 路径点 | A `team_action_spy_waypoint` | — |
| 16 | `PatrolToWaypoint` | 路径点 | A `team_action_patrol_waypoint` | `setMission(Patrol)` |
| 17 | `ChangeScript` | ScriptType 序号 | 内联 | `team.setScript` |
| 18 | `ChangeTeam` | TeamType 序号 | A `team_action_change_team` | — |
| 19 | `Panic` | — | 内联 `infantry::apply_panic_force` | — |
| 20 | `ChangeHouse` | 国家下标 | 内联 `change_owner_with_rules` | `setOwner` |
| 21 | `Scatter` | — | 内联 `scatter` | — |
| 22 | `GoToNearbyShroud` | — | 内联 `nearby_shrouded_cell`；非单人模式下拒绝执行 | — |
| 23 | `PlayerLoses` | — | 内联 `request_player_outcome(Defeat)` | `scenario.endFor` |
| 24 | `PlaySpeech`（惯用名） | — | 内联：只前进到下一行 | — |
| 25 | `PlaySound` | SOUNDMD 序号 | 内联 `SimSoundEvent::VocCenteredIndex` | `sound.play` |
| 26 | `PlayMovie` | 影片 | 内联 `TriggerEffect::FullscreenMovie` | `scenario.playMovie` |
| 27 | `PlayMusic` | 音乐主题 | 内联 `TriggerEffect::MusicTheme` | `scenario.playTheme` |
| 28 | `ReduceOre` | — | 内联 `reduce_tiberium_in_radius` | `map.reduceOre` |
| 29 | `BeginProduction` | — | 内联 `enable_scripted_production` | `house.beginProduction` |
| 30 | `FireSale` | — | 内联 `set_state_four` | `house.fireSale` |
| 31 | `SelfDestruct` | — | 内联 `commit_direct_damage_receiver` | `kill` |
| 32 | `IonStormStart` | 持续时间 | 内联 `lightning_storm::start` | `weather.startIonStorm` |
| 33 | `IonStormEnd` | — | 内联 `lightning_storm::request_end` | `weather.stopIonStorm` |
| 34 | `CenterCamera` | 速度 | 内联 `TriggerEffect::CameraAtCoord` | `scenario.centerCamera` |
| 35 | `ReshroudMap` | — | 内联 `reset_shroud_keeping_counters` | `house.reshroud` |
| 36 | `RevealMap` | — | 内联 `reveal_entire_map` | `map.revealAll` |
| 37 | `DeleteMembers` | — | 内联 `techno_limbo_with_context` + `uninit_with_context` | `remove` |
| 38 | `ClearGlobal` | 下标 | 内联 `set_trigger_variable` | `ctx.global(i).set(false)` |
| 39 | `SetLocal` | 下标 | 内联 | `ctx.local(i).set(true)` |
| 40 | `ClearLocal` | 下标 | 内联 | `ctx.local(i).set(false)` |
| 41 | `Unpanic` | — | 内联 `apply_unpanic_force` | — |
| 42 | `ForceFacing` | 朝向 | A `team_action_force_facing` | — |
| 43 | `WaitUntilFullyLoaded` | — | A `team_action_wait_until_loaded` | — |
| 44 | `TruckUnload` | — | 内联 `store_techno_type` | `convert` |
| 45 | `TruckLoad` | — | 内联 `store_techno_type` | `convert` |
| 46 | `AttackEnemyBuilding` | 类型 `arg&0xFFFF`，模式 `arg>>16` | A `team_action_attack_enemy_building` | — |
| 47 | `MoveToEnemyBuilding` | 同上 | A `team_action_move_to_enemy_building` | — |
| 48 | `ScoutHouses` | — | SH `team_action_scout_houses` | — |
| 49 | `RegisterSuccess` | — | 内联（前进，标记成功） | — |
| 50 | `Flash` | 帧数 | 内联 `flash_members` | — |
| 51 | `PlayAnimation` | 低 16 位为动画，高 16 位为循环次数 | A `team_action_play_animation` | `map.spawnAnim` |
| 52 | `TalkBubble` | 帧 | A `team_action_talk_bubble` | — |
| 53 | `GatherAtEnemyBase` | Ares：+ `AISafeDistance` | A `team_action_gather_at_enemy_base` | — |
| 54 | `RegroupAtBase` | Ares：+ `AIFriendlyDistance` | A `team_action_regroup_at_base` | — |
| 55 | `IronCurtain` | Ares：`SW.Group` | SA `team_action_iron_curtain` | `launchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | 同 46 | SA `team_action_chronoshift(Building)` | `teleport` |
| 57 | `ChronoshiftToQuarry` | 目标类别 | SA `team_action_chronoshift(Quarry)` | `teleport` |
| 58 | `MoveToOwnBuilding` | 类型、模式 | A `team_action_move_to_own_building` | — |
| 59 | `AttackBuildingAtWaypoint` | 路径点 | A `team_action_attack_waypoint_building` | — |
| 60 | `EnterGrinder` | — | 内联 `foot_enter_grinder` | — |
| 61 | `OccupyTankBunker` | — | 内联 `foot_enter_tank_bunker` | — |
| 62 | `EnterBioReactor` | — | 内联 `foot_enter_bio_reactor` | — |
| 63 | `OccupyBattleBunker` | — | 内联 `foot_enter_battle_bunker` | — |
| 64 | `GarrisonNearest` | — | 内联 `foot_enter_nearest_occupiable` | — |
| 65, 66 | — | — | 代码中没有 | — |
| 67 | `AresSeekVehicle` | — | 与 64 共用的 Ares 分支；被步骤检查挡住，执行不到 | — |
| 68 | `AresConvert` | — | 内联 `convert_techno_type`（仅 Ares） | `convert` |

## 10. 规则访问

### 10.1 `onRules` 写入

`onRules` 看到的是 INI 各层读完之后的规则，可以修改任意键、新增类型。写入经过原生 INI 读取器、钳制和读后处理，并计入大厅哈希（RFC 8）。这些写入在层叠中构成一个新的层，位于 mod（或地图）的层之后，所以每一遍的钳制和“查找或分配”都和 INI 文本的行为一致。

```ts
class RulesWriter extends RulesReader {
  set(section: string, key: string, value: string): Status;   // value is INI text
  remove(section: string, key: string): Status;
  addType(list: string, id: string): Status;                  // e.g. ("InfantryTypes", "E7")
}
```

| 函数 | Owner | 层级 | 来源 |
| --- | --- | --- | --- |
| `set`、`remove`、`addType` | 先 `src/rules/native_processing.rs::RulesLayerStack::push` 再 `process`，然后 `RuleSet::from_processed_rules`。模块层：**尚无 owner**（G-R1） | v1 | RFC 8 |
| 在 `onRules` 中读取 | 前面各层处理后的层叠 | v1 | RFC 2（地图能看到 mod 的结果） |

### 10.2 对局中的修改

对局中调用 `rules.set("E1", "Speed", "6")` 时，值像 INI 文本一样解析，从下一帧起生效，让相关缓存失效，并作为规则覆盖层存档（RFC 8）。目前 `RuleSet` 在对局中不可变（`src/sim/runtime.rs::SimResources`），也没有覆盖层的 owner（G-R2）。每个白名单键都要写明它对地图上已有对象的影响（RFC 8）。白名单是待定问题 12。

**候选**（使用时通过 `&RuleSet` 读取的普通值，没有发现派生副本）：

| 节类别 | 键 | 地图上已有的对象 | 说明 |
| --- | --- | --- | --- |
| Weapon | `Damage`、`ROF`、`Burst`、`MinimumRange` | 下次使用时取新值 | |
| Weapon | `Range` | 同上 | 仅当抛射体 `ROT>0` 时；`ROT=0` 时它参与计算 `Speed`（仅限 onRules） |
| Weapon | `Report`、`Anim` | 同上 | 表现层 |
| Warhead | `Verses` | 同上 | 启用 Ares 时检查 `install_ares_verses` |
| Warhead | `CellSpread`、`PercentAtMax`、`Wall`、`Wood`、`Sparky`、`Fire`、`Conventional`、`Rocker`、`InfDeath`、`AnimList` | 同上 | |
| TechnoType | `Cost`、`BuildTime` 类倍率 | 按需计算（`cost_of`、`type_cost`、`building_actual_cost`） | |
| TechnoType | `Speed` | 按需换算 | 检查各移动方式自己的状态（`movement/foot_speed.rs`、`drive_locomotion.rs` 的 `raw_type_speed`） |
| TechnoType | `ROT`、`Points`、`Soylent`、`Crushable`、`Selfhealing`、`Explodes`、`VeteranAbilities`、`EliteAbilities` | 同上 | |
| TechnoType | `Power` | 每个拥有它的所属方都要失效（`PowerState::invalidate`） | |
| Projectile | `Inaccurate`、`Arcing`、`Shadow` | 新生成的抛射体 | `Arcing` 需谨慎（见“仅限 onRules”） |
| 全局 | `GeneralRules` 中来自 `[General]`、`[CombatDamage]`、`[AudioVisual]` 的数值字段 | 同上 | 按帧换算的值需要重新换算 |

**在构造时复制到每个对象中的键**（只有写明已有对象的处理规则后才能进白名单）：

| 键 | 复制位置 | 需要的规则 |
| --- | --- | --- |
| `Strength` | `src/sim/world/world_spawn/construction.rs`（`Health::new`） | 保持生命点数或保持比例（`Health::set_fraction`） |
| `Sight` | `construction.rs`（`GameEntity.sight`） | 更新每个对象及其已揭示的区域 |
| `Ammo`、`InitialAmmo` | `src/sim/ammo.rs::TechnoAmmo::from_type` | 已有 `TechnoAmmo::change_type_capacity` |
| `Storage` | `construction.rs`（`Miner::new`） | 更新容量 |
| `Passengers`、`SizeLimit` | `construction.rs`（`PassengerCargo::new`） | 更新容量；超出时怎么办 |
| 类型上的 Phobos `AttachEffect.*` | `attach_effect.rs::attach_initial_effects` | 只影响新对象 |

**仅限 onRules**（派生缓存或读后处理）：

| 键 | 原因 | 参考 |
| --- | --- | --- |
| 武器 `Range`、`[General] Gravity`、抛射体 `ROT`、`Floater`、Phobos 抛射体 `Gravity` | 会改写 `ROT=0` 抛射体所用武器的 `Speed` | `native_processing.rs::postprocess_weapon_speeds` |
| `Primary`、`Secondary`、`Weapon{n}`、`EliteWeapon{n}`、`OccupyWeapon`、武器的 `Warhead`/`Projectile` | 解析为带 `rules_serial` 标记的句柄 | `ruleset.rs::resolve_handles`；`src/sim/type_handle_table.rs` |
| `Prerequisite`、`PrerequisiteOverride`、`[General] Prerequisite*`、Ares `Prerequisite.List{n}` | 解析为建筑数组下标 | `read_prerequisite_lists`；`prerequisite.rs::parse_prerequisites` |
| `Owner`、`RequiredHouses`、`ForbiddenHouses` | 国家位掩码 | `object_type.rs::HouseMasks::derive` |
| `Factory` | `factory_map` | `ruleset.rs` |
| `Foundation`、`[AI] AIBaseSpacing`、`BuildConst`、`IsBaseDefense`、`UndeploysInto`、`Spawned`/`MissileSpawn` | 几何信息和基地列表，由 `TypeFacts::stamp` 复制到每个对象 | `ruleset.rs::complete_building_type_ext`；`construction.rs` |
| `Spawns`、`SpawnsNumber`、心灵控制和时间武器的设置 | 管理器只构建一次 | `init_capture_manager`、`init_temporal`、`init_spawn_manager` |
| `Armor`（名称） | 解析为 `ArmorId` | `armor.rs::armor_index`；`ares_armor.rs` |
| `Image`、美术键、`Sequence` | 美术和动画绑定需要素材 | `install_art_data`、`rebuild_animation_sequences` |
| Ares `PoweredBy`、`PowersUnit`、`PoweredUnit` | 跨类型修正 | `ruleset.rs` 中的 `ValidateThings` |
| 列表节、任何指向新类型的键 | 新类型会改变所有下标 | `find_or_allocate`、`EXPLICIT_RULE_TYPE_FAMILIES` |
| `[Colors]`、国家的 `Color`、`Side`、`ParentCountry` | 颜色渐变，国家和阵营的映射 | `refresh_country_side_projection`、`project_country_side_registry` |
| 声音和 EVA 键 | 加载后绑定为下标 | `bind_type_sound_references`、`bind_type_eva_references` |
| `TechLevel` | 改用按类型的运行时 owner | `set_object_type_tech_level`（A106） |

Ares 和 Phobos 的键：白名单要逐个写明能否在对局中修改（RFC 8）；本清单还没有对任何一个做出分类。

## 11. 客户端 API

除非另有标注，所有条目都属于 `client` 和 P2（`v1`）。客户端从不直接修改游戏状态：它发出标准玩家指令，或者发送消息（RFC 1、10）。

### 11.1 界面树

引擎持有界面树，负责布局和绘制。修改按渲染帧批量提交（RFC 10）。面向模块的界面树 owner：**尚无 owner**（G-C1）；现有的布局读取器是 `src/ui/match_layout.rs` 和 `src/ui/sidebar/layout_spec.rs`（`veraui.ini`）。

```ts
class Ui {
  find(name: string): Node;        // NO_HANDLE when no node has that name
  create(n: NodeSpec): Node;
  remove(n: Node): Status;
  registerHotkey(name: string, defaultKey: string): Status;
  viewer(): House;                 // NO_HANDLE for a spectator
}
class Node {
  name(): string;
  setLabel(csfOrText: string): void;   // Label
  setArt(name: string): void;          // Art
  setPosition(x: i32, y: i32): void;
  setSize(w: i32, h: i32): void;
  setVisible(v: bool): void;
  setItems(items: string[]): void;   // List
  setFill(v: i32): void;             // Bar: how full the bar is
  onClick(enabled: bool): void;      // routes clicks to the module's onClick
}
```

| 操作 | 层级 | 来源 |
| --- | --- | --- |
| 按名字 `find` | v1 | RFC 10，示例 |
| `create`、`remove` | v1 | RFC 10，示例 |
| `setLabel`、`setArt`、`setPosition`、`setVisible` | v1 | RFC 10 |
| `setItems` | v1 | RFC 10 示例 |
| `setSize`、`setFill` | v1? | |
| `onClick`、悬停、热键 | v1 | RFC 10 |
| 数据绑定（属性绑定到一个公开的值） | later | Rc：RmlUi 数据模型 |

控件类型（待定问题 15）。客户端代码使用 `veraui.ini` 的名字（`Column.hold(Hold.BottomLeft)` 对应 `Type=Column`、`Hold=BottomLeft`）。RFC 示例用到 `Column`、`Text` 和 `List`；`veraui.ini` 目前有 `Button`、`Text`、`Row`、`Column`、`CameoGrid`（`docs/interface-layout.md`）。

| 类型 | 来源 | 层级 |
| --- | --- | --- |
| `Text` | RFC 10；`veraui.ini` | v1 |
| `Button` | `veraui.ini` | v1 |
| `Row`、`Column` | `veraui.ini`；RFC 10 示例（`Column.hold`） | v1 |
| `CameoGrid` | `veraui.ini` | v1? |
| `List` | RFC 10 示例（`setItems`） | v1? |
| `Image` | RFC 10（`image`） | v1? |
| `Bar`（进度条） | — | later |

### 11.2 INI 布局键

静态布局使用 `veraui.ini` 的语法（RFC 10）。目前已有的键（`docs/interface-layout.md`）：

| 键 | 含义 |
| --- | --- |
| `Type` | `Button`、`Text`、`Row`、`Column`、`CameoGrid` |
| `Hold` | `TopLeft`、`Top`、`TopRight`、`Left`、`Middle`、`Right`、`BottomLeft`、`Bottom`、`BottomRight` |
| `X`、`Y`、`Width`、`Height` | 位置和尺寸 |
| `Parts` | 行或列的子节点 |
| `Gap` | 各部分之间的像素数 |
| `Label` | 按钮文字或 CSF 标签 |
| `Art` | 一张 SHP 图片 |
| `Command` | 键盘命令名（`StopObject`、`GuardObject`……） |
| `Value` | `Credits`、`Power`、`Drain`、`Name`、`Time`、`Kills`、`Losses` |
| `Tab`、`Columns`、`Rows` | 图标网格 |

RFC 0.0.5 的示例改用这些键，并新增 `List` 部件类型和 `Text` 部件的 `Label` 键（P16）。mod 和地图把布局节放在哪里是待定问题 15。客户端代码使用同样的名字：`Text.label(...)`、`node.setLabel` 和 `node.setArt`（RFC 10）。`Value` 只保留它在 INI 中的含义，即部件显示的数据来源（`Credits`、`Power`……）；`Bar` 的填充量用 `node.setFill` 设置。

### 11.3 单个对象的显示属性

| 属性 | Owner | 层级 | 来源 |
| --- | --- | --- | --- |
| `display(t).setArt(name)` | 渲染绑定（**尚无 owner**） | v1? | RFC 10 |
| `display(t).setPalette(name)` | **尚无 owner** | v1? | RFC 10 |
| `display(t).setTint(color, intensity)` | **尚无 owner**（模拟中的染色是一种效果，见第 5 节） | v1? | RFC 10 |
| `display(t).addAnim(name)` / `removeAnim` | **尚无 owner** | v1? | RFC 10 |
| `display(t).setVisible(v)` | **尚无 owner** | v1? | RFC 10 |
| `display(t).setLabel(text)`、`setValue(v)`、`addButton(name)` | **尚无 owner** | v1? | RFC 6b、10；F38 `Info`、F44 `ShowCSF`、F50 `ShowButton` |
| 小地图可见性、地图标注（点、线、文字） | **尚无 owner** | later | Rc：`SetUnitNoMinimap`、`MarkerAddPoint` |

显示属性只影响本机画面。隐身等影响玩法的属性属于模拟模块（RFC 10）。

### 11.4 选择与指令

| 函数 | Owner | 层级 | 来源 |
| --- | --- | --- | --- |
| `client.selection(out: HandleBuffer)` | `src/app/input/` 中的选择状态（未核实） | v1 | RFC 10 |
| `client.select(list: HandleBuffer, count: i32, append: bool)` | `Command::Select`（`src/sim/command.rs`） | v1 | RFC 10 |
| `client.order(o: OrderSpec)` | `Command` 的各个变体：`Move`、`MoveToObject`、`Stop`、`Attack`、`AttackMove`、`Guard`、`DeployMcv`、`UndeployBuilding`、`SetRally`、`QueueProduction`、`SuspendProduction`、`CancelProductionByType`、`PlaceReadyBuilding`、`SellBuilding`、`ToggleRepair`、`EjectBunker`、`UnloadPassengers`、`CaptureBuilding`、`PlantC4`、`LaunchSuperWeapon`、`ToggleInfantryDeploy`、`ForceAttackCell`、`SellWallAtCell` | v1 | RFC 10（像任何玩家输入一样校验） |
| `client.sendMessage(data: Uint8Array): Status` | 命令流：**尚无模块专用的变体**（G-C2） | v1 | RFC 1、10、11；Rc：`SendLuaRulesMsg` |
| `client.broadcast(to: Audience, data: Uint8Array)` | **尚无 owner** | later | **RFC 0.0.5 未采纳**（P5）；Rc：`SendLuaUIMsg` |
| 指令改写钩子（类似 `CommandNotify`） | **尚无 owner** | later | Rc：`CommandNotify`、`DefaultCommand` |

每条消息的发送者由引擎填写，引擎还限制消息大小和频率（RFC 11）。

### 11.5 镜头、声音、存储、消息和日志

| 函数 | Owner | 层级 | 来源 |
| --- | --- | --- | --- |
| `camera.center(at: Coord, speed: i32)` | 应用层镜头（`TriggerEffect::CameraAtCoord` 的作用目标） | v1? | A48；Rc：`SetCameraTarget` |
| `camera.jump(at: Coord)` | 同上（`velocity: None`） | v1? | A112 |
| `camera.home()` | `TriggerEffect::HomeCamera` 的作用目标 | v1? | A145 |
| `sound.play(name)`、`sound.playAt(name, at)` | `src/audio/`（本机） | v1 | RFC 10 |
| `sound.speech(eva)` | EVA 播放（本机） | v1 | RFC 10 |
| `storage.get(key: string): Uint8Array \| null` | **尚无 owner**（G-C4） | v1 | RFC 10 |
| `storage.set(key: string, data: Uint8Array): Status` | **尚无 owner** | v1 | RFC 10 |
| `messages.show(csfOrText: string, color: i32)` | `src/ui/messages.rs`（入口未核实） | v1 | RFC 10 |
| `ui.log(text: string)` | 游戏内控制台：**尚无 owner** | v1 | RFC 10 |
| 接收模拟发来的通知（`onNotify`） | **尚无 owner** | v1 | RFC 1、11 |

### 11.6 客户端查询

第 7 节的查询函数在客户端中以相同名字、相同的缓冲区存在，按 7.7 的方式过滤。`ui.viewer()` 给出本机所属方。

## 引擎中的缺口

每个缺口都是一个 API 条目，它的 owner 目前还不存在。修复总是放在现有 owner 的模块里（项目契约），没有一项是第二份实现。

### 模块宿主

- **G-H1** 模块加载器、校验、计量和停用流程（RFC 3、4）：新增，位于 core 中，在 `sim/` 热循环之外。
- **G-H2** 模拟帧中的模块阶段（待定问题 9）：`src/sim/world/master_frame.rs` 新增一个阶段，先运行 `onEvents`，再运行 `onFrame`；`onFrame` 调度。
- **G-H3** 快照和定期的完整状态哈希中的模块内存，不计入联机每帧的同步校验（RFC 4、15）：存档、读档和 `hash_state` 路径。
- **G-H4** `ctx.log` 和游戏内的出错提示（RFC 3、14）：模拟侧的一行日志，以类似 `TriggerEffect` 的输出发布，由 app 显示。
- **G-H5** 模拟到客户端的通知（`ctx.notify`、`onNotify`），只投递给目标玩家；内容是模块自己的字节，引擎不过滤（RFC 11）。
- **G-H6** 跟踪模块内存的脏页，让定期的完整状态哈希只覆盖上次哈希之后被写过的内存页（RFC 15）。

### 触发器、事件和约束条件

- **G-T1** 模块触发器动作（`Action.wasm`）：在 `TR::execute_live_trigger_action` 中新增一个动作编号，把模块调用排进队列（RFC 6a）。
- **G-T2** 模块在运行时创建触发器和标签（`Trigger.create`、`Tag.create`、`techno.setTag`、`cell.setTag`）：目前定义只来自 `RuntimeDefinitions::read_map`/`read_teams`；需要不会和地图编号冲突的 ID（RFC 7）。
- **G-T3** 类型标签绑定（`Wasm.Tags`）放在 `AttachedTag` 槽位之外，作为按句柄索引的旁路表中的订阅记录（RFC 7、15）；以及武器、弹头和抛射体类型上用于武器事件的 `Wasm.Tags`（RFC 6a）。
- **G-T4** TriggerType 上的约束条件列表，在触发前由引擎原生检查（RFC 6a）。
- **G-T5** 轮询型事件列表，每个触发器带检查间隔（RFC 6a），放在 Logic 标签旁边；有最小间隔，不同对象的检查分散到不同的帧（RFC 15）。
- **G-T6** 为 2.3 中每个新的投递型事件加上触发钩子，位置在那里写明的 owner 中；只对绑定的类型触发（G-T10）。
- **G-T7** 事件参数从触发点一路带进模块的 `EventRecord`（第 2 节）；目前 `dispatch_object_tag_event` 只带执行者和来源。
- **G-T8** 原版事件 2（`SpiedBy`）和 3（`ThievedBy`）没有触发点（原本就存在，不是模块造成的）。
- **G-T9** 批量队列：每个模块一帧一个数组，存放固定大小的 `EventRecord`，每个排队的模块动作一条，带着触发其触发器的那个事件的载荷，在含模块动作的触发器触发时按触发顺序写入，在 `onFrame` 之前每帧每个模块一次调用交出；触发器已被禁用或删除的排队动作会被丢弃，游戏结束后所有排队动作都会被丢弃（RFC 5、15）。
- **G-T10** 对象类型、武器类型、弹头类型和抛射体类型各带一个模块标志位，有模块绑定该类型时置位；每个事件触发点先检查它，未绑定的类型不再运行其他代码（RFC 15）。
- **G-T11** `TR` 中触发器触发处的每帧连锁深度计数器：连锁中含有由模块创建或绑定的触发器、VERA 新增的事件或原子动作时，统计一帧之内立即触发的触发器连锁的深度，并把会超过上限的触发器改到下一帧；只由原版事件和原版动作组成的连锁不受限制，与原版中一样运行（RFC 5、目标 5；上限是待定问题 18）。

### 动作

- **G-A1** 读取路径点下标的 owner（动作 8、17、18、41、42、43、58、59、63、80、88、89、90、94–96、99、101、107–109、116、125）需要一个按格子的入口；路径点形式调用它。
- **G-A2** 让对象立即发射武器（`techno.fireWeapon`）：目前只有开火更新（`combat/world_fire.rs::visit_fire`）能走到 FireAt。
- **G-A3** 把一个对象传送到某格，并带目标格被占时的规则：`chronosphere.rs::warp_object` 是私有的；没有通用 owner。
- **G-A4** 清除目标：目前只有各个 locomotor trait 的方法。
- **G-A5** 把清除伪装做成一次完整事务（发布、视野）：`DisguiseRuntime::clear_unit` 只是字段操作。
- **G-A6** 在 Simulation 层设置生命并发布结果（生命类事件、为 0 时死亡）：目前只有 `Health::set`/`heal`。
- **G-A7** 解除单个对象的心灵控制：`capture_manager.rs` 中有原主人记录；尚未确认有解除函数。
- **G-A8** 增删乘客，并处理 limbo 和 unlimbo：`PassengerCargo` 只有列表操作。
- **G-A9** 在运行时修改单个所属方的前提条件或可建造性。
- **G-A10** 设置所属方的分数（目前只有 `add_score`），以及由模块让单个所属方结束游戏，超出 `request_player_outcome` 的部分（需检查它是否支持单个所属方）。
- **G-A11** 为玩法创建对象（不是为测试）：`spawn_object` 的文档写明用于测试和工具；需要一个生产用入口，处理归属、发布，并在目标格被占时拒绝。
- **G-A12** 移除或强制引爆单个抛射体（P8，已在 RFC 0.0.5 采纳）。
- **G-A13** 在某坐标引爆弹头，带伤害和来源（P7，已在 RFC 0.0.5 采纳）：`apply_area_damage` 已有，带击杀归属的 API 包装还没有。

### 持续效果和标记

- **G-E1** 把 `src/sim/attach_effect.rs` 和 `src/rules/attach_effect.rs` 从 `cfg(feature = "phobos")` 迁到运行时开关（RFC 8）。
- **G-E2** Simulation 层的“给对象添加效果类型”和“移除效果”事务；`remove_attach_effect_type` 是私有的。
- **G-E3** 没有 Phobos 键的新效果种类：变形、替换武器、运动、传送、免疫、停止索敌、目标优先级倍率、伤害修正、无法被选中、无视指令、不可被选为目标、所属方断电。
- **G-E4** 新的效果属性：间隔、最大触发次数、“拒绝新的”叠加方式、互斥组、效果携带的标记。
- **G-M1** 标记：按句柄索引的旁路表（RFC 15），存档并计入哈希，对象移除时清空。

### 查询

- **G-Q1** 创建序号：每个对象和抛射体创建时分配，随存档保存，用作查询顺序和分页位置（RFC 6c、15）。
- **G-Q2** 把句柄或 ID 写入模块缓冲区的查询入口，返回数量，结果多时分页；每次查询都不分配内存（RFC 15）。

### 覆盖点

- **G-O1** 第 8 节每个覆盖点的调用点和类型绑定（伤害在 `receive_damage`，批量选择目标在 `scan_batch.rs`，生产在 `can_build`，开火在 FireAt 的各步骤中）。
- **G-O2** `ProjectileTrajectory` 中的模块弹道分支、放在按句柄索引的旁路表中的每个抛射体 16 字节模块数据、钳制（RFC 9、15）。

### AI

- **G-AI1** 设置一支现存小队的脚本（`team.setScript`）。
- **G-AI2** 在测试之外移动小队的脚本游标（`team.jumpTo`）。
- **G-AI3** 在运行时启用或禁用单个 AI 触发器。
- **G-AI4** 在运行时修改 AI 触发器的权重。

### 规则

- **G-R1** 一个模块规则层，在 mod 或地图层之后压入 `RulesLayerStack`，并计入大厅校验的哈希。
- **G-R2** 对局中的规则覆盖层：存档、计入哈希，让以 `rules.serial()` 为键的缓存失效（`movement_tick.rs`、`house_defeat.rs`、`scatter.rs`、`movement_commands.rs`），并为每个键写明对已有对象的规则。
- **G-R3** 每种 ID 的名字到 ID 查找（见 [ID](#id)），建在现有注册表（`RuleSet` 的类型下标、声音绑定、AI 定义）之上，未知名字返回 `NotFound`；另加供 `Mark.id` 使用的标记名注册表，以及供 `Notify.id` 使用的通知名稳定哈希和加载时对本局所有模块（mod 的和地图的）一起进行的冲突检查（RFC 3、10、15）。

### 客户端

- **G-C1** 由模块驱动的界面树：新建、删除、查找、设置属性，点击和悬停的分发，按渲染帧批量提交，建在 `veraui.ini` 布局读取器之上。
- **G-C2** 在 `src/sim/command.rs::Command` 中新增模块消息变体，带发送者、大小和频率上限，并记入回放。
- **G-C3** 客户端查询的视野过滤：黑幕、迷雾规则、隐形、伪装（`cloak_disguise.rs` 中已有辅助函数）。
- **G-C4** 玩家本机上每个 mod 的键值存储。
- **G-C5** 单个对象的显示属性（图像、调色板、着色、附加动画、可见性、文字、数值、按钮）。
- **G-C6** 供 `log()` 使用的游戏内控制台。

合计：52 个缺口（H 6、T 11、A 13、E/M 5、Q 2、O 2、AI 4、R 3、C 6）。G-T8 是原版触发器系统原本就有的缺口。

## 提议的 RFC 修改

这些是调研发现、而 RFC 0.0.4 还没写到的内容。RFC 0.0.5 逐条作了决定：除 P4 和 P5 外全部采纳。依赖它们的条目注明了提议编号。

- **P1 每个覆盖点的组合规则。** *已在 0.0.5 采纳。* RFC 第 2 节说覆盖按链式处理。应逐个覆盖点写明：链中下一环拿到的是原生值、上一环的值，还是两者都有；布尔否决能否被后面的环节撤销（Recoil 对布尔值取“与”，遇到第一个否决就停止）；并给每个返回数值的覆盖点一个明确的“已完全处理”结果，而不是用魔数表示。
- **P2 `onScenarioStart` 与存档。** *已在 0.0.5 采纳。* 写明读档后不运行 `onScenarioStart`（模块内存会被恢复），也没有 `onLoad`。增加测试：保存在模块内存里的句柄，读档后仍指向相同的对象。
- **P3 标记的值和可见度。** *已在 0.0.5 采纳。* 让标记可以带一个可选的整数和一个可见度等级（私有、盟友、视野内、公开），这样客户端模块可以显示状态，又不泄露隐藏的状态。增加所属方级别的标记。现在就确定值的类型，并写明“不允许嵌套”。
- **P4 以所属方身份查询。** *未采纳。* 在模拟 API 中增加 `ctx.asHouse(h)`：同样的查询，按所属方 `h` 能看到的范围过滤。模拟查询默认仍然掌握全部信息。
- **P5 盟友之间的客户端通道。** *未采纳。* 决定客户端模块能否不经过模拟、向盟友或观战者发送限频消息，还是所有这类通信都走 `sendMessage`。
- **P6 查询顺序。** *已在 0.0.5 采纳。* 写明具体的确定性顺序（例如对象创建顺序），使它在引擎重构和为模拟规模调整顺序后依然成立。
- **P7 在某格引爆弹头。** *已在 0.0.5 采纳。* 在 6b 清单中加入“在某坐标引爆弹头，带伤害和来源”（离子风暴、自定义超级武器和连锁反应都靠它实现）。
- **P8 移除抛射体。** *已在 0.0.5 采纳。* 增加“移除抛射体而不引爆”和“立即引爆抛射体”两个独立动作。
- **P9 伤害与生命。** *已在 0.0.5 采纳。* 把“修改生命（增减或设定）”拆成两项：“经由弹头和护甲流程造成伤害，带用于击杀归属的来源”和“设定生命，不计归属”。
- **P10 两阶段的所属转移。** *已在 0.0.5 采纳。* RFC 6a 在转移之前触发 HouseTransfer。增加一个转移之后的事件，或者写明处理函数看到的是哪个状态。
- **P11 更多候选事件。** *已在 0.0.5 采纳。* 把 2.5 中的候选（Built、Idle、MoveFailed、Arrived、Promoted、Cloaked、Spotted、Lost、AmmoChanged、MissionChanged、HouseDefeated、PlayerLeft、StorageFull、MotionEnded）加入待定问题 13 的候选清单。
- **P12 只允许按类型绑定的订阅。** *已在 0.0.5 采纳。* 写明高频事件（伤害、开火、抛射体创建或引爆、碾压、移动失败）和所有覆盖点只对绑定的类型触发，从不通过全局的“所有对象”订阅触发。
- **P13 热点决策先用数据，再用回调。** *已在 0.0.5 采纳。* 目标优先级优先用优先级倍率（一种持续效果）加一个 `TargetChanged` 事件，逐次调用的覆盖点留给不常发生的决策。把 `Override.CommandAllowed` 和 `Override.Death` 列为候选。把自定义排队任务（由模块定义、模块每帧执行的任务）记为以后的 RFC。
- **P14 无法取消的事件。** *已在 0.0.5 采纳。* 写明投递型事件改变不了哪些原生结果（例如取消一次死亡），因为事件是在模块阶段才到达的；这类需求需要覆盖点或效果（阻止死亡）。
- **P15 探测查询。** *已在 0.0.5 采纳。* 增加“这把武器现在能否向那个目标开火”、“是否在射程内”、“所属方 H 能否把类型 T 放在格子 C”和“最近的可放置格子”，让模块复用原生的合法性判断。
- **P16 布局语法。** *已在 0.0.5 采纳。* RFC 的 INI 示例用的是 `Anchor=`、`Size=`、`Children=` 和 `Type=Panel`；`veraui.ini` 用的是 `Hold=`、`Width=`/`Height=` 和 `Parts=`，也没有 `Panel`。要么使用现有的键，要么写明新增哪些键。
- **P17 客户端查询的迷雾规则。** *已在 0.0.5 采纳。* 写明客户端查询对迷雾下的敌方对象返回什么（不存在，还是最后一次看到的状态），以及客户端模块能否看到盟友所属方的标记、数值和生产。
- **P18 错误模型。** *已在 0.0.5 采纳。* 写明哪些失败返回状态（句柄已失效、名字未知、被引擎规则拒绝），哪些会停用模块（参数非法、trap、超出额度）。
- **P19 通知 API。** *已在 0.0.5 采纳。* 给模拟到客户端的通知函数命名，并给通知一个声明的名字，而不是靠“开头的魔数字节”区分。
- **P20 边界上的定点数。** *已在 0.0.5 采纳。* 写明倍率和分数的定点格式（`I16F16` 原始值，与 `SimFixed` 相同），因为 Phobos 把倍率当作小数读取。
- **P21 永不改变单位或含义。** *已在 0.0.5 采纳。* 在第 12 节加入：已发布的函数永不改变单位或含义；要改变就新增函数或版本。

合计：21 项提议。
