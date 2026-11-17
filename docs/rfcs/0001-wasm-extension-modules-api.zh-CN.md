# RFC 0001 附录：API 清单

本附录是 [RFC 0001](0001-wasm-extension-modules.zh-CN.md) 的一部分，使用同一个版本号（0.0.6）。两个文件在同一次提交中修改。本清单与 RFC 正文不一致时，以 RFC 正文为准。与 RFC 正文一样，本附录目前只有这一个中文版本；RFC 定稿后再翻译成英文。

本清单是一份参考。它列出 API 将提供的每个函数、类型、事件、约束条件、效果、覆盖点和查询，每一项都给出签名和一行语义说明。它们背后的规则只在 RFC 正文中陈述一次，本清单链接到那里。版本 1 包含哪些条目是 RFC 的待定问题 12。

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

## 约定

### 签名

签名用 TypeScript 书写（兼容 AssemblyScript），限于 [RFC 5a](0001-wasm-extension-modules.zh-CN.md#5a-语言与-sdk) 的共同子集。Rust SDK 提供同样的函数，名字用蛇形命名。包名和类名只是占位。

- 把多个值组合在一起的参数用类，用 `new` 创建；带默认值的字段可以之后再设置。
- `T | null` 只用于类。缺失的句柄写作 `NO_HANDLE`（[句柄](#句柄)），缺失的 ID 写作 `NO_ID`（[ID](#id)），缺失的整数写作 `-1` 或其他写明的哨兵值。
- `ActionParams` 和 `AtomicParams` 的 `value`、`value2` 字段没有缺失值：用到它们的种类按给定值读取，不用的种类忽略它们，所以默认值 `0` 不是哨兵值。
- 为简洁起见，表格把对句柄的调用写成 `techno.kill(...)` 或 `house.addMoney(...)`。在 TypeScript SDK 中，它是以句柄为第一个参数的命名空间函数（`Techno.kill(t, ...)`、`House.addMoney(h, 500)`）；代码示例使用这种形式。

### 值类型

| 类型 | 含义 |
| --- | --- |
| `i32`、`u32`、`bool` | 普通整数和标志。模拟 API 中没有浮点值。 |
| `Fixed` | 定点数，以原始 `i32` 表示，采用模拟的 `I16F16` 格式（`SimFixed`）：`65536` 表示 `1.0`。用于倍率。 |
| `Coord` | `x`、`y`、`z`，单位为 lepton。1 格等于 256 lepton。 |
| `Cell` | `x`、`y`，单位为格。`Cell.waypoint(n)` 给出一个带有路径点编号 `n` 的格子（7.3）。 |
| `Frames` | 逻辑帧数；每游戏秒 15 个逻辑帧。 |
| `string` | 名字：节名和键名、初始化时的查找、触发器名和导出名、CSF 条目名、客户端文字。UTF-8；比较时不区分大小写，且只按 ASCII 规则（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）。对局中的调用改用 ID。 |
| `Techno`、`House`…… | 句柄（[句柄](#句柄)）。 |
| `TechnoTypeId`、`MarkId`…… | 具名事物的整数 ID（[ID](#id)）。 |
| `Uint8Array` | 字节：消息、模块数据。 |

```ts
type Fixed = i32;      // I16F16 raw value
type Frames = i32;

class Cell  { constructor(x: i32, y: i32); x: i32; y: i32; waypoint: i32 = -1; }   // waypoint: a number to resolve, -1 none
class Coord { x: i32; y: i32; z: i32; }   // leptons
class Rect  { x: i32; y: i32; width: i32; height: i32; }   // cells
```

代码注释说明：`Cell.waypoint` 是有待解析的路径点编号，`-1` 表示没有。

### 句柄

句柄是对象的创建序号：在创建时分配，从不复用，小于 2^53，与引擎如何存放对象无关（[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)、[5a](0001-wasm-extension-modules.zh-CN.md#5a-语言与-sdk)）。其他种类的句柄也以同样的方式获得编号。在 TypeScript SDK 中，每种句柄类型都是 `i64` 的别名；在 Rust SDK 中是一个 newtype。句柄从不可为空：`NO_HANDLE`（`0`）表示“没有”。对 `NO_HANDLE` 或已不存在的句柄发起的调用遵循[错误模型](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。

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
| `Node` | 客户端界面树中的一个节点（11.1） | 它被删除，或对局界面关闭 |

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

// namespace Techno (the handle comes first):
isAlive(t: Techno): bool;
asBuilding(t: Techno): Building;   // NO_HANDLE when not a building
asInfantry(t: Techno): Infantry;   // NO_HANDLE when not infantry
asVehicle(t: Techno): Vehicle;     // NO_HANDLE when not a vehicle
asAircraft(t: Techno): Aircraft;   // NO_HANDLE when not an aircraft

// a handle that may be absent is compared with NO_HANDLE (in an action):
if (e.other != NO_HANDLE) {
  ctx.log("attacked");
}
```

代码注释说明：`asBuilding`、`asInfantry`、`asVehicle` 和 `asAircraft` 在对象不属于该类别时返回 `NO_HANDLE`。

可能不指向任何对象的字段或返回值使用普通的句柄类型，并注明可能为 `NO_HANDLE`。`T | null` 只保留给 `Path`、`DisguiseInfo` 这样的类。

### ID

模块在对局中用到的具名事物都有整数 ID。模块对每个名字只解析一次，通常在 `onRules` 或 `onScenarioStart` 中完成，其他所有调用都传 ID（[RFC 15](0001-wasm-extension-modules.zh-CN.md#15-性能)）。在 TypeScript SDK 中，每种 ID 类型都是 `i32` 的别名；在 Rust SDK 中是一个 newtype。ID 一经分配就不再改变（[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)）。

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
| `MarkId` | 标记 | `Mark.id(name)`：在模拟代码中登记未知的名字；在客户端代码中只查找（[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-原子动作与持续效果)） |
| `NotifyId` | 模拟到客户端的通知，在发送它的模块的 `vera.notify` 自定义段中声明 | `Notify.id(name)`：统一大小写后的名字的哈希（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） |
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
Techno.addMark(techno, PANICKED, 0, Visibility.Allies);
```

| 情况 | 结果 |
| --- | --- |
| 没有匹配任何东西的名字 | `NO_ID`，且 `ctx.lastStatus()` 为 `NotFound` |
| 传入 `NO_ID` 的调用 | `NotFound` |
| 超出其种类取值范围的 ID | 非法参数 |

名字只保留在初始化和不频繁的调用中：上面的查找、`ctx.house(name)`、`onRules` 的写入和 `rules.set`（第 10 节）、规则读取、`Trigger.create` 以及 `Action.wasm` 的动作名、`Trigger.find` 和 `Tag.find`、AI 定义（9.2）、指定国家、音乐、EVA 语音或 CSF 条目名的不频繁场景动作，以及客户端 API 中启动时的 `find`、热键名、声音、CSF 条目名和文字（第 11 节）。

### 查询结果

查询把结果写入模块提供的缓冲区，并返回写入的数量。顺序、分页和开销：[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-查询)、[RFC 15](0001-wasm-extension-modules.zh-CN.md#15-性能)、[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)。

- 结果多于缓冲区容量时，查询会设置 `out.cursor`；用同一个缓冲区再次调用，就从上次写入的最后一个结果之后继续。最后一页写完后 `out.cursor` 为 `0`。
- 游标只在开始这次查询的那次模块调用中有效。

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

代码注释说明：`HandleBuffer` 由模块分配一次并重复使用，句柄从下标 0 开始存放；`cursor` 用于分页：为 `0` 时开始一次查询，非零时继续这次查询。`IdBuffer` 对 ID 做同样的事。

### 枚举

| 枚举 | 取值 |
| --- | --- |
| `Mission` | 0 `Sleep`、1 `Attack`、2 `Move`、3 `QMove`、4 `Retreat`、5 `Guard`、6 `Sticky`、7 `Enter`、8 `Capture`、9 `Eaten`、10 `Harvest`、11 `AreaGuard`、12 `Return`、13 `Stop`、14 `Ambush`、15 `Hunt`、16 `Unload`、17 `Sabotage`、18 `Construction`、19 `Selling`、20 `Repair`、21 `Rescue`、22 `Missile`、23 `Harmless`、24 `Open`、25 `Patrol`、26 `ParadropApproach`、27 `ParadropOverfly`、28 `Deliberate`（INI 名为 `Wait`）、29 `AttackMove`、30 `SpyplaneApproach`、31 `SpyplaneOverfly`；`None` = 255。编号与地图中的编号相同。 |
| `Relation` | `Self`、`Ally`、`Enemy`、`Neutral`（平民或被动所属方） |
| `Category` | `Infantry`、`Vehicle`、`Aircraft`、`Building`、`Projectile` |
| `Layer` | `Underground`、`Surface`、`Ground`、`Air`、`Top`。水面是地形类型，不是层。 |
| `LandType` | `Clear`、`Road`、`Water`、`Rock`、`Wall`、`Tiberium`、`Beach`、`Rough`、`Ice`、`Railroad`、`Tunnel`、`Weeds` |
| `SpeedType` / 移动区域 | `SpeedType` 和 `MovementZone` 这两个 INI 键的取值 |
| `Compare` | 0 `Less`、1 `LessEqual`、2 `Equal`、3 `GreaterEqual`、4 `Greater`、5 `NotEqual`（与 AI 触发器比较符的顺序相同） |
| `Difficulty` | `Easy`、`Normal`、`Hard`、`All`（触发器难度标志） |
| `TagRepeat` | 0 `OneTimeOr`、1 `OneTimeAnd`、2 `Repeating`（原版标签的重复类型） |
| `Visibility` | 0 `Own`、1 `Allies`、2 `InSight`、3 `Everyone`：谁能看到一个标记（第 6 节） |
| `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`；Ares 种类 |

### 其他类型

表格中用到的较小的枚举和类。除非另行给出，取值按声明顺序；由接口定义确定。

```ts
enum Source { Mod, Map }                      // the module's source in a match (RFC 2)
enum PowerLevel { Full, Low }
enum Rank { Rookie, Veteran, Elite }
enum Outcome { Win, Lose }
enum FlashSize { Small, Medium, Large }
enum Destroy { All, Buildings, Land, Naval }
enum TeamRatio { AiTriggers, Aircraft, Infantry, Units }
enum Color { Red, Green, Blue }
enum Event { }                                // 0..61 (2.1), 62..88 Ares (2.2), new events (2.3, 2.4; numbers: open question 5)
class DisguiseInfo { type: TechnoTypeId; house: House; }
class Path { kind: i32; points: Coord[]; }    // later (RFC 9 level 3): Bezier, parabola, spiral, waypoints
class LaunchCtx extends Ctx { shooter: Techno; target: Target; }   // later
class BulletState { p: Projectile; coord: Coord; speed: i32; detonate: bool; }   // later (L4)
class BulletBatch { count: i32; at(i: i32): BulletState; }        // later (L4); a view into module memory
class Every { constructor(frames: Frames); }  // onFrame registration: runs in the frames where frame % frames == 0 (RFC 5)
enum Hold { TopLeft, Top, TopRight, Left, Middle, Right, BottomLeft, Bottom, BottomRight }   // the veraui.ini Hold values (11.2)
class NodeSpec { name: string | null = null; type: string; hold: Hold; parts: NodeSpec[]; }   // the veraui.ini keys (11.2); name: for find
class Column extends NodeSpec { constructor(hold: Hold, parts: NodeSpec[]); }   // Type=Column
class Row extends NodeSpec { constructor(hold: Hold, parts: NodeSpec[]); }      // Type=Row
class Text extends NodeSpec { constructor(label: string); label: string; }      // Type=Text, Label=
class List extends NodeSpec { constructor(); }                                   // Type=List; items: Node.setItems
class OrderSpec { kind: i32; units: HandleBuffer; count: i32; target: Target; }   // kind: a Command variant (11.4)
// Scenario, Objects, MapQuery, RulesReader, Ai: the functions of sections 7.5, 7.1, 7.3, 7.4 and 9
// Override query and result classes: section 8
```

代码注释说明：`Source` 是模块在对局中的来源（RFC 2）。`Event` 的取值：0..61 为原版事件（2.1），62..88 为 Ares 事件（2.2），新事件见 2.3 和 2.4（编号属于待定问题 5）。`Path`（RFC 9 层级 3：贝塞尔曲线、抛物线、螺旋线、路径点）、`LaunchCtx`、`BulletState` 和 `BulletBatch`（L4）属于以后的层级；`BulletBatch` 是模块内存中的一个视图。`Every` 用于注册 `onFrame`：在 `frame % frames == 0` 的帧运行（RFC 5）。`Hold` 是 `veraui.ini` 的 `Hold` 取值（11.2）。`NodeSpec` 对应 `veraui.ini` 的键（11.2），`name` 供 `find` 使用；`Column`、`Row`、`Text` 和 `List` 分别对应 `Type=Column`、`Type=Row`、`Type=Text`（带 `Label=`）和 `Type=List`，`List` 的条目用 `Node.setItems` 设置。`OrderSpec.kind` 是一个 `Command` 变体（11.4）。`Scenario`、`Objects`、`MapQuery`、`RulesReader` 和 `Ai` 是第 7.5、7.1、7.3、7.4 节和第 9 节的函数；覆盖点的查询类和结果类见第 8 节。

### 错误

SDK 没有异常。错误模型，即哪些失败返回状态、哪些停用模块，见 [RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。

```ts
enum Status {
  Ok = 0,
  Gone = 1,         // the handle is NO_HANDLE or its object no longer exists
  NotFound = 2,     // a name or ID matched nothing; a missing waypoint
  Refused = 3,      // the owner declined: cell blocked, cannot attach, not allowed
  Unsupported = 4,  // off in this game (an Ares action without Ares), or not available here (the init function, a sim query in a client)
}
```

代码注释说明：`Gone` 表示句柄为 `NO_HANDLE` 或其对象已不存在；`NotFound` 表示名字或 ID 没有匹配任何东西，或路径点不存在；`Refused` 表示 owner 拒绝（格子被占、无法附加、不允许）；`Unsupported` 表示该功能在本局中关闭（例如没有 Ares 时的 Ares 动作），或在此处不可用（初始化函数中，或客户端中的模拟查询）。

会停用模块的非法参数按函数逐一列出：越界的下标（全局变量 0..49，局部变量 0..99）、负数的数量、不在列表中的枚举值，以及 `Trigger.create` 拒绝的原子动作参数（4.1）。

### 随机数

每个条目的“随机数”列写明它使用哪条随机流（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）：

| 取值 | 含义 |
| --- | --- |
| `module` | 从模块自己的随机流取数。 |
| `native` | 运行原生代码，按那段代码原本的方式从原生随机流取数。 |
| `none` | 不取随机数。 |

```ts
class Random {
  next(): u32;                    // module stream
  range(low: i32, high: i32): i32; // inclusive; module stream
}
// ctx.random: Random
```

### 条目标注

| 字段 | 取值 |
| --- | --- |
| **层级** | `v1`（RFC 计划的阶段 1 或 2）、`v1?`（待定问题 12 下的候选，对覆盖点而言是待定问题 10 下的候选）、`later` |
| **世界** | `sim`（`rules.wasm`）、`client`（`ui.wasm`）、`both` |

本清单中原版事件和动作的名字是 API 名；真正标识事件或动作的是编号。

## 1. 生命周期导出

模块只导出它用到的函数；没有导出的函数没有开销。各导出何时运行：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。

### 模拟模块（`rules.wasm`）

| 导出 | 签名 | 时机 | 层级 |
| --- | --- | --- | --- |
| 初始化函数 | 由 SDK 从顶层代码生成；模块没有 `start` 段 | 一次，在第一个其他调用之前；其中只有名字查找可用 | v1 |
| `onRules` | `onRules(rules: RulesWriter): void` | 一次，在模块所处的规则层位置 | v1 |
| `onScenarioStart` | `onScenarioStart(ctx: Ctx): void` | 一次，在场景开始时 | v1 |
| `onFrame` | `onFrame(ctx: Ctx): void`，由 SDK 生成；函数通过 `onFrame(new Every(N), fn)` 注册 | 每帧，在 `onEvents` 之后 | v1 |
| 动作 | `(ctx: Ctx, e: EventRecord): void`，通过 `action(name, fn)` 注册；列在自定义段 `vera.actions` 中 | 由 `onEvents` 调用，每条排队的记录一次 | v1 |
| `onEvents` | `onEvents(ctx: Ctx, batch: EventBatch): void`，由 SDK 生成；按 `EventRecord.action` 分派 | 每帧每个有排队记录的模块一次，在 `onMessage` 之后 | v1 |
| 覆盖点 | 见[第 8 节](#8-覆盖点) | 引擎走到该决策时 | v1? |
| `onMessage` | `onMessage(ctx: Ctx, sender: House, source: Source, data: Uint8Array): void` | 客户端消息到达模拟时 | v1（阶段 2） |
| `onLaunch` | `onLaunch(ctx: LaunchCtx, p: Projectile): Path \| null` | 绑定了模块路径的类型，每个抛射体一次 | later |
| `onTrajectoryFrame` | `onTrajectoryFrame(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | 每帧每种抛射体类型一次 | later（L4） |

没有 `onLoad`：模块内存随快照恢复。

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
  notifySpectators(id: NotifyId, data: Uint8Array): Status;   // every spectator's client
  lastStatus(): Status;
}
class Variable { get(): bool; set(value: bool): Status; }
```

用 `ctx.local(i)` 或 `ctx.global(i)` 设置变量时，也会像地图动作那样重置等待这个变量的触发器的计时器（事件 27、28、36、37）。

### 客户端模块（`ui.wasm`）

| 导出 | 签名 | 时机 | 层级 |
| --- | --- | --- | --- |
| `onStart` | `onStart(ui: Ui): void` | 一次，在对局界面建好、INI 布局读完之后，并在初始化函数之后；对重新加入的玩家，在追赶完成后运行一次 | v1（阶段 2） |
| `onRender` | `onRender(ui: Ui, dt: f32, alpha: f32): void` | 每个渲染帧一次 | v1（阶段 2） |
| `onClick` | `onClick(ui: Ui, node: Node): void` | 用 `Node.setClickable` 设为可点击的节点被点击 | v1（阶段 2） |
| `onHover` | `onHover(ui: Ui, node: Node, inside: bool): void` | 光标进入或离开节点 | v1（阶段 2） |
| `onHotkey` | `onHotkey(ui: Ui, name: string): void` | 模块注册的热键被按下 | v1（阶段 2） |
| `onSelect` | `onSelect(ui: Ui): void`；选择用 `client.selection(out)` 读取 | 本地选择发生变化 | v1（阶段 2） |
| `onNotify` | `onNotify(ui: Ui, source: Source, id: NotifyId, data: Uint8Array): void` | 发给这台机器的通知到达（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） | v1（阶段 2） |

客户端允许使用浮点：客户端世界不是确定性的。`dt` 和 `alpha` 只用于显示。客户端额度：[RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。

## 2. 事件

### 事件的用法

投递、顺序和时机：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。事件种类、绑定和轮询：[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件约束条件和动作)。被绑定的实例：[RFC 7](0001-wasm-extension-modules.zh-CN.md#7-运行时的触发器和标签)。

```ts
enum EventKind { Delivered, Polled }

class EventParam {
  value: i32 = -1;          // number parameter (house index, waypoint, seconds, ...)
  name: string | null = null;   // type name or TeamType name
  compare: Compare = Compare.GreaterEqual;   // polled events
  interval: Frames = 15;    // polled events: how often the engine checks
}

class EventDef {
  constructor(event: i32);   // an Event value
  event: i32;
  param: EventParam = new EventParam();
}

class TriggerDef {
  constructor(events: EventDef[], actions: Action[]);
  events: EventDef[];        // one or more; all must hold (AND), as in the original
  actions: Action[];         // one or more
  house: House = NO_HANDLE;  // NO_HANDLE: the subject's owner at each spring
  conditions: Cond[] = [];
  difficulty: Difficulty = Difficulty.All;
  allowWin: bool = false;    // the marker of original action 15
  transferOnCapture: bool = false;   // TriggerType token 8
}

class EventRecord {          // fixed size; read in place from the batch (RFC 15)
  action: i32;               // index of the queued module action in vera.actions; always set
  kind: i32;                 // Event value
  trigger: Trigger;          // the trigger that sprang; always set
  holder: Techno;            // tag holder: the object whose tag holds the trigger, or the bound object; NO_HANDLE otherwise
  holderCellX: i32;          // tag holder of a CellTag: its cell; -1 otherwise
  holderCellY: i32;
  subject: Techno;           // the object the trigger is attached to; may be NO_HANDLE, or read as gone
  other: Techno;             // attacker, target, transport, producer, ...; may be NO_HANDLE
  house: House;              // house involved (retained house for events 1, 24, 25, 26, 59); may be NO_HANDLE
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

代码注释说明：`TriggerDef.events` 有一个或多个，必须全部成立（AND），与原版相同；`actions` 有一个或多个；`house` 为 `NO_HANDLE` 时，触发器每次触发时的所属方是主体当时的拥有者；`allowWin` 是原版动作 15 的标记，`transferOnCapture` 是 TriggerType 的第 8 项。`EventRecord` 大小固定，在批次中原地读取（RFC 15）：`action` 是排队的模块动作在 `vera.actions` 中的下标，总会设置；`kind` 是 `Event` 值；`trigger` 是触发的那个触发器，总会设置；`holder` 是标签持有者，即标签中持有该触发器的对象或被绑定的对象，其他情况为 `NO_HANDLE`；`holderCellX`、`holderCellY` 在标签持有者是 CellTag 时为它的格子，其他情况为 `-1`；`subject` 是触发器所附着的对象，可能为 `NO_HANDLE`，也可能读出来是“不存在”；`other` 是攻击者、目标、载具、生产者等，可能为 `NO_HANDLE`；`house` 是涉及的所属方（事件 1、24、25、26、59 为保留的所属方），可能为 `NO_HANDLE`；`projectile` 和 `effect` 可能为 `NO_HANDLE`；`id` 是表中列出时的类型 ID（武器、弹头、效果类型、超级武器类型）；`value` 是伤害、武器索引、层数、帧数等，`value2` 是表中列出时的第二个整数字段；`cellX`、`cellY` 是表中的“cell”，没有时为 `-1`。`EventBatch` 每帧每个模块一个，`at(i)` 返回模块内存中的一个视图，而不是副本。

- 下面每个事件的载荷写明它填入哪些 `EventRecord` 字段；事件没有列出的字段为 `NO_HANDLE` 或 `-1`。每条记录都会填入触发器和标签持有者。
- 记录的布局属于接口定义（RFC 12）。
- 由 House 标签引发的原版事件每 8 帧轮询一次，由 Logic 标签引发的每帧轮询，与原版相同。

模块用 `Trigger.create` 创建触发器，再通过标签挂接，或绑定到类型：

```ts
Trigger.create(def: TriggerDef): Trigger;   // NO_HANDLE with NotFound (missing waypoint, 4.1) or Unsupported (Ares event without Ares, 2.2)
Tag.create(triggers: Trigger[], repeat: TagRepeat): Tag;
Techno.setTag(t: Techno, tag: Tag): Status;   // replaces the object's tag, as in the original
// class Cell: setTag(tag: Tag): Status;      // a CellTag
Trigger.bindType(t: Trigger, type: TechnoTypeId): Status;
Trigger.bindWeaponType(t: Trigger, type: WeaponTypeId): Status;     // weapon events
Trigger.bindWarheadType(t: Trigger, type: WarheadTypeId): Status;
Trigger.bindProjectileType(t: Trigger, type: ProjectileTypeId): Status;
```

代码注释说明：`Trigger.create` 在路径点不存在时返回 `NO_HANDLE` 并设置 `NotFound`（4.1），在没有 Ares 时使用 Ares 事件则返回 `NO_HANDLE` 并设置 `Unsupported`（2.2）。`Techno.setTag` 和原版一样替换对象原有的标签；`Cell` 类上的 `setTag(tag)` 设置 CellTag。`bindWeaponType` 用于武器事件。

`Trigger.disable` 和 `Trigger.destroy` 作用于被绑定触发器的每个实例。

### 2.1 原版事件 0..61

类型：`R` 在某个时刻引发（投递型），`H` House 标签轮询，`L` Logic 标签轮询。载荷：`S` 主体，`O` 另一方（行为者或来源），`Hs` 所属方。全部为 `v1`、`sim`。模块创建的触发器以 `new EventDef(n)` 使用它们；`Event` 枚举给出它们的名字。

| # | API 名称 | 参数 | 类型 | 载荷 | 备注 |
| --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | 不公开：永远不成立。 |
| 1 | `EnteredBy` | `value` 所属方下标（-1 表示任意） | R | S、O、保留的 Hs | 会写持久状态。 |
| 2 | `SpiedBy` | 忽略 | R | S、O | |
| 3 | `ThievedBy` | `value` 所属方下标 | R | S、Hs | |
| 4 | `DiscoveredByPlayer` | 忽略 | R | S、O | |
| 5 | `HouseDiscovered` | `value` 所属方 | H | Hs | |
| 6 | `AttackedByAnyHouse` | 忽略 | R | S、O | `AttackedBy` 的粗略形式。 |
| 7 | `DestroyedByAnyHouse` | 忽略 | R | S、O | 需要击杀来源。 |
| 8 | `AnyEvent` | — | H | S | 每 8 帧轮询一次。 |
| 9 | `AllUnitsDestroyed` | `value` 所属方 | H | Hs | |
| 10 | `AllBuildingsDestroyed` | `value` 所属方 | H | Hs | |
| 11 | `AllDestroyed` | `value` 所属方 | H | Hs | |
| 12 | `CreditsExceed` | `value` 金钱 | H | Hs | |
| 13 | `ElapsedTime` | `value` 秒（×15 帧） | L | — | |
| 14 | `MissionTimerExpired` | — | L | — | |
| 15 | `BuildingsDestroyedCount` | `value` 数量 | H | Hs | |
| 16 | `UnitsDestroyedCount` | `value` 数量 | H | Hs | |
| 17 | `NoFactoriesLeft` | — | H | Hs | |
| 18 | `CiviliansEvacuated` | — | H | Hs | |
| 19 | `BuildBuildingType` | `value` BuildingType 下标 | H | Hs | 会写持久状态。也接受 `name`。 |
| 20 | `BuildUnitType` | `value` VehicleType 下标 | H | Hs | 同 19。 |
| 21 | `BuildInfantryType` | `value` InfantryType 下标 | H | Hs | 同 19。 |
| 22 | `BuildAircraftType` | `value` AircraftType 下标 | H | Hs | 同 19。 |
| 23 | `TeamLeavesMap` | `name` TeamType | R | — | |
| 24 | `ZoneEntryBy` | `value` 所属方（-1 表示任意） | R | S、O、保留的 Hs | |
| 25 | `CrossesHorizontalLine` | 同 24 | R | O、保留的 Hs | |
| 26 | `CrossesVerticalLine` | 同 24 | R | O、保留的 Hs | |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — | |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — | |
| 29 | `DestroyedByAnythingNotInfiltrate` | 忽略 | R | S、O | |
| 30 | `LowPower` | `value` 所属方 | H | Hs | |
| 31 | `BridgeDestroyed` | 忽略 | R | 格子 | |
| 32 | `BuildingExists` | `value` BuildingType 下标 | H | Hs | 会写持久状态。 |
| 33 | `SelectedByPlayer` | 忽略 | R | S | 联机对局中不引发。客户端形式：`onSelect`（第 11 节）。 |
| 34 | `ComesNearWaypoint` | `value` 路径点 | R | S | 距离在 0x500 lepton 以内。 |
| 35 | `EnemyInSpotlight` | 忽略 | R | S | |
| 36 | `LocalIsSet` | `value` 0..99 | L | — | |
| 37 | `LocalIsClear` | `value` 0..99 | L | — | |
| 38 | `FirstDamagedCombat` | 忽略 | R | S、O | 有伤害来源。 |
| 39 | `HalfHealthCombat` | 忽略 | R | S、O | 黄血。 |
| 40 | `QuarterHealthCombat` | 忽略 | R | S、O | 红血。 |
| 41 | `FirstDamagedAny` | 忽略 | R | S、O（可为 `NO_HANDLE`） | |
| 42 | `HalfHealthAny` | 忽略 | R | S | |
| 43 | `QuarterHealthAny` | 忽略 | R | S | |
| 44 | `AttackedByHouse` | `value` 所属方数组下标 | R | S、O | Ares 会转换 Player@X。 |
| 45 | `AmbientLightBelow` | `value` | L | — | 环境光变化时检查。 |
| 46 | `AmbientLightAbove` | `value` | L | — | 同 45。 |
| 47 | `ElapsedScenarioTime` | `value` 秒 | L | — | |
| 48 | `DestroyedByAnything` | 忽略 | R | S、O（可为 `NO_HANDLE`） | `Killed` 的粗略形式。 |
| 49 | `PickupCrate` | 忽略 | R | S | |
| 50 | `PickupCrateAny` | 忽略 | L | — | 有箱子被捡起时检查。 |
| 51 | `RandomDelay` | `value` 秒 | L | — | 随机数：native。 |
| 52 | `CreditsBelow` | `value` 金钱 | H | Hs | |
| 53 | `SpyAsHouse` | `value` 所属方 | R | S、Hs | 会写持久状态。 |
| 54 | `SpyAsInfantry` | `value` InfantryType 下标 | R | S | 会写持久状态。 |
| 55 | `NavalUnitsDestroyed` | `value` 所属方 | H | Hs | |
| 56 | `LandUnitsDestroyed` | `value` 所属方 | H | Hs | |
| 57 | `BuildingDoesNotExist` | `value` BuildingType 下标 | H | Hs | |
| 58 | `PowerFull` | `value` 所属方 | H | Hs | |
| 59 | `EnteredOrOverflownBy` | 同 1 | R | S、O、保留的 Hs | |
| 60 | `TechTypeExists` | `name` 类型，`value` 数量 | L | — | |
| 61 | `TechTypeDoesNotExist` | `name` 类型 | L | — | |

### 2.2 Ares 事件 62..88

只有游戏按 Ares 方式读取时（`rules.ares_form()`，而不只是 `rules.ares()`：按原版方式读取的触发器数据保持原版形式）才可用；否则 `Trigger.create` 返回 `NO_HANDLE`，状态为 `Unsupported`。全部为 `v1`、`sim`。

| # | API 名称 | 参数 | 类型 |
| --- | --- | --- | --- |
| 62 | `UnderEmp` | — | R |
| 63 | `UnderEmpByHouse` | `value` 所属方 | R |
| 64 | `RemovedFromEmp` | — | R |
| 65 | `RemovedFromEmpByHouse` | `value` 所属方 | R |
| 66 | `InSpotlight` | — | R |
| 67 | `DriverKilled` | — | R |
| 68 | `DriverKilledByHouse` | `value` 所属方 | R |
| 69 | `VehicleTaken` | — | R |
| 70 | `VehicleTakenByHouse` | `value` 所属方 | R |
| 71 | `Abducted` | — | R |
| 72 | `AbductedByHouse` | `value` 所属方 | R |
| 73 | `AbductsSomething` | — | R |
| 74 | `AbductsSomethingOfHouse` | `value` 所属方 | R |
| 75 | `SuperWeaponActivated` | `value` 超级武器类型下标 | H |
| 76 | `SuperWeaponDeactivated` | `value` 超级武器类型下标 | H |
| 77 | `SuperWeaponNearWaypoint` | `name` 超级武器类型，`value` 路径点（5 格） | H |
| 78 | `HouseReverseEngineeredType` | `name` 类型 | H |
| 79 | `SomethingReverseEngineered` | — | R |
| 80 | `TypeReverseEngineered` | `name` 类型 | R |
| 81 | `HouseOwnsAtLeast` | `name`、`value` | H |
| 82 | `HouseOwnsAtMost` | `name`、`value` | H |
| 83 | `AttackedOrDestroyedByAnybody` | — | R |
| 84 | `AttackedOrDestroyedByHouse` | `value` 所属方 | R |
| 85 | `DestroyedByHouse` | `value` 所属方 | R |
| 86 | `TypeCountAtMost` | `name`、`value` | L |
| 87 | `HouseNoKeepAliveObjects` | `value` 所属方 | H |
| 88 | `HouseNoKeepAliveBuildings` | `value` 所属方 | H |

### 2.3 新的投递型事件

新事件的编号取自 Ares 和 Phobos 都没有占用的范围（待定问题 5）。只对被绑定的对象引发，在安全点引发（[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)、[6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件约束条件和动作)）。`Fire`、`HitTarget` 和 `Detonated` 也可以绑定在武器、弹头或抛射体类型上；`Cond.isWeaponType` 检查武器。全部为 `sim`。

| 事件 | 载荷（`EventRecord`） | 引发时机 | 层级 |
| --- | --- | --- | --- |
| `TakeDamage` | `other` 来源（可为 `NO_HANDLE`），`value` 护甲结算后的伤害 | 在批量伤害提交结束之后 | v1? |
| `HitByWarhead` | `id` 弹头（`WarheadTypeId`），`value` 最终伤害，`other` 来源 | 在伤害被接收之后；免疫的命中不算 | v1? |
| `AttackedBy` | `other` 攻击者，`value` 武器索引（-1 表示无），`projectile`（可为 `NO_HANDLE`） | 在目标上引发，在开火步骤之后 | v1? |
| `FatalDamage` | `other` 攻击者（可为 `NO_HANDLE`），`value` 伤害 | 在致命伤害提交之后、死亡流程之前；死亡流程会重新检查 | v1? |
| `Killed` | `other` 击杀者（可为 `NO_HANDLE`） | 在击杀记录之后 | v1? |
| `Fire` | `other` 目标（可为 `NO_HANDLE`），`value` 武器索引，`id` 武器（`WeaponTypeId`），`projectile`（可为 `NO_HANDLE`） | 在射击完成之后 | v1? |
| `SpawnFire` | `other` 子机，`value` 武器索引，`projectile`；目标在记录的 `cell` 中，或通过子机的目标获得 | 在子机射击之后 | v1? |
| `HitTarget` | `other` 目标，`value` 伤害，`id` 武器（`WeaponTypeId`），`projectile` | 在开火者上引发，在批量伤害提交结束之后 | v1? |
| `TargetChanged` | `other` 新目标（清除目标时为 `NO_HANDLE`） | 在目标设定或清除之后；来自并行目标扫描时，在扫描结束之后 | v1? |
| `ProjectileIncoming` | `projectile`，`other` 发射者（可为 `NO_HANDLE`） | 在抛射体被接纳之后 | v1? |
| `Detonated` | `projectile`，`cell`，`other` 来源（可为 `NO_HANDLE`），`id` 弹头（`WarheadTypeId`） | 在引爆之后 | v1? |
| `Birth` | `other` 生产者（可为 `NO_HANDLE`），`effect`（可为 `NO_HANDLE`），`value` 生产时间（-1 表示无） | 在对象放置之后（生产、增援、生成） | v1? |
| `TransportEntered` | 在乘客（`subject`）上引发；`other` 载具或建筑，`value` 已占空间，`value2` 1 为驻军 / 0 为载员；乘客通过查询获得 | 在进入完成之后 | v1? |
| `TransportExited` | 同 `TransportEntered` | 在离开完成之后 | v1? |
| `PassengerAdded` | 在载具或建筑（`subject`）上引发；`other` 乘客，`value` 已占空间，`value2` 驻军标志 | 同 `TransportEntered` | v1? |
| `PassengerRemoved` | 同 `PassengerAdded` | 同 `TransportExited` | v1? |
| `Crushed` | `other` 碾压者 | 在碾压之后 | v1? |
| `HouseTransfer` | `house` 新所属方，`value` 为 1 表示心控 | 在转移之前；转移会重新检查 | v1? |
| `HouseTransferred` | `house` 原所属方，`value` 为 1 表示心控 | 在转移之后 | v1? |
| `EffectStart` | `effect`，`value` 层数 | 在效果附加之后 | v1? |
| `EffectExpire` | `effect`（投递后读出为不存在），`id` 效果类型（`EffectTypeId`） | 在效果移除之后 | v1? |
| `EffectStacks` | `effect`，`value` 新层数 | 在层数增加之后 | v1? |
| `Timer` | `value` 上一次触发的帧 | 每隔 `param.interval` 帧 | v1? |
| `SuperWeaponLaunched` | `id` 超级武器类型（`SuperWeaponTypeId`），`house`，`cell` 目标；让模块实现超级武器的效果 | 在超级武器发射之后 | v1? |

### 2.4 新的轮询型事件

每隔 `param.interval` 帧检查一次，在状态翻转时触发，并按创建序号错开（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件约束条件和动作)）。比较方式是用 `param.compare` 与 `param.value` 比较。全部为 `sim`。

| 事件 | 读取内容 | 层级 |
| --- | --- | --- |
| `HealthPoll` | 生命值（`value`）；`name` 为 `"%"` 时为百分比 | v1? |
| `AmmoPoll` | 弹药数 | v1? |
| `PassengersPoll` | 乘客数 | v1? |
| `TransportSpacePoll` | 剩余空间 | v1? |
| `StoragePoll` | 载矿量 | v1? |
| `MissionPoll` | 当前任务等于 `value` | v1? |
| `MovingPoll` | `value` 1 为移动，0 为停止 | v1? |
| `LandTypePoll` | 对象所在格子的地形类型等于 `value` | v1? |
| `HouseMoneyPoll` | 触发器所属方的可用金钱 | v1? |
| `HousePowerPoll` | 发电量减耗电量 | v1? |
| `EffectStacksPoll` | 效果类型 `name` 的层数 | v1? |
| `DistancePoll` | 到记录中 `other` 的距离，单位 lepton | v1? |

每个间隔都起作用的效果用 `EffectStacksPoll` 实现，或在 `Timer` 触发器上加约束条件 `hasEffect`。

## 3. 约束条件

约束条件在模块代码运行之前，由引擎原生过滤触发器（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件约束条件和动作)）。它默认检查主体对象；用 `.onOther()` 构建时检查 `EventRecord.other`。`Cond.not(c)` 表示取反。全部为 `sim`。

```ts
class Cond {
  static not(c: Cond): Cond;
  onOther(): Cond;
}
```

| 函数 | 检查内容 | 层级 |
| --- | --- | --- |
| `Cond.relation(r: Relation)` | 被检查对象的所属方与触发器所属方的关系 | v1 |
| `Cond.category(c: Category)` | 对象类别 | v1 |
| `Cond.layer(l: Layer)` | 对象所在层 | v1 |
| `Cond.isType(type: TechnoTypeId)`；`Cond.isProjectileType(type: ProjectileTypeId)` | 类型等于 `type` | v1 |
| `Cond.isWeaponType(type: WeaponTypeId)` | 武器事件（`Fire`、`HitTarget`）的武器等于 `type` | v1 |
| `Cond.armor(armor: ArmorId)` | 护甲 | v1 |
| `Cond.civilianHouse()` | 所属方是平民阵营 | v1 |
| `Cond.health(cmp: Compare, value: i32)` | 生命值 | v1 |
| `Cond.healthPercent(cmp: Compare, percent: i32)` | 生命百分比 | v1 |
| `Cond.healthBelowPercent(p: i32)` | 上一条的简写 | v1 |
| `Cond.landType(t: LandType)` | 对象所在格子的地形类型 | v1 |
| `Cond.housePower(state: PowerLevel)` | `Full` 或 `Low` | v1 |
| `Cond.houseMoney(cmp: Compare, value: i32)` | 被检查对象所属方的金钱 | v1 |
| `Cond.hasEffect(type: EffectTypeId, minStacks: i32)` | 拥有该效果，且至少 `minStacks` 层 | v1 |
| `Cond.hasMark(m: MarkId)` | 拥有该标记 | v1 |
| `Cond.effectSource(type: EffectTypeId, r: Relation)` | 效果 `type` 的来源与之关系为 `r` | v1? |
| `Cond.inWeaponRange(weaponIndex: i32)` | `other` 在主体的武器射程内 | v1? |

## 4. 动作

### 4.1 动作的用法

动作在触发器中运行（`TriggerDef.actions`），或由模块代码直接调用。时机：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。原子动作的目标、固定规则和失败处理：[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-原子动作与持续效果)。`Action.wasm` 的名字解析：[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)。

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
  static wasm(name: string): Action;   // a name in the module's vera.actions table
  static atomic(kind: AtomicKind, target: ActionTarget, params: AtomicParams): Action;
}
enum ActionTarget { Subject, Other, TagHolder, Projectile, House, Cell }   // Subject is the default
enum AtomicKind {            // the atomic actions of 4.3 whose parameters fit, named after their functions
  FireWeapon, LaunchSuperWeapon, SpawnAnim, Remove, Kill, Damage, SetHealth, Heal,
  CreateObject, SetMission, Teleport, ClearTarget, ClearDisguise, PlaySound,
  AddExperience, SetRank, AddMoney, SetTechLevel, AllowType, SetOwner,
  ClearMindControl, SetAmmo, AddAmmo, AddPassenger, RemovePassenger,
  AddEffect, RemoveEffect, AddMark, RemoveMark,
  DetonateWeapon, DetonateWarhead, DetonateProjectile, RemoveProjectile,
  EndFor, SetScore, AddScore, Log, Convert,
}
class AtomicParams {         // the parameters of an atomic action other than its target
  id: i32 = NO_ID;           // type ID: weapon, warhead, superweapon, animation, techno, sound, effect, mark
  value: i32 = 0;            // amount, damage, points, weapon index, mission, rank, ammo, enum value
  value2: i32 = 0;           // second value where the action needs one
  cell: Cell | null = null;  // a cell, where the action takes one; a waypoint as Cell.waypoint(n)
  house: House = NO_HANDLE;  // House target, SetOwner's new owner, owner of created objects; NO_HANDLE: the trigger's house
  text: string | null = null; // Log only: the line to write
}
```

每个 `AtomicKind` 使用的目标和字段。某个种类的“目标”列没有列出的目标种类永远不适用于它（属于非法参数）。*对象*指 `Subject`、`Other` 或持有对象的 `TagHolder`；*格子*指 `Cell`，或对象目标所在的格子，包括 `Projectile`；`—` 表示没有目标或没有字段；`other` 是事件的另一方对象。

| `AtomicKind` | 目标 | 字段与第二个对象 |
| --- | --- | --- |
| `FireWeapon` | 对象（射手） | `id` 不是 `NO_ID` 时为武器，否则为 `value` 武器槽位（默认 0，即主武器）；瞄准 `cell`（若给出），否则瞄准 `other`，再否则瞄准射手当前的目标；三者都没有时为 `Gone` |
| `LaunchSuperWeapon` | `House` | `id` 超级武器类型，`cell`（必需：缺少时属于非法参数） |
| `SpawnAnim` | 格子 | `id` 动画 |
| `Remove` | 对象 | — |
| `Kill` | 对象 | `other` 为来源（可选） |
| `Damage` | 对象 | `id` 弹头，`value` 伤害；`other` 为来源（可选） |
| `SetHealth`、`Heal` | 对象 | `value` 生命点数 |
| `CreateObject` | 格子 | `id` 类型；`house` 为所属方 |
| `SetMission` | 对象 | `value` 任务，`cell` |
| `Teleport` | 对象 | `cell`，`value` 目标格被占时的规则（`Blocked`） |
| `ClearTarget`、`ClearDisguise`、`ClearMindControl` | 对象 | — |
| `PlaySound` | 格子 | `id` 声音 |
| `AddExperience` | 对象 | `value` 经验点数 |
| `SetRank` | 对象 | `value` 等级 |
| `AddMoney` | `House` | `value` 金额 |
| `SetTechLevel` | — | `id` 类型，`value` 科技等级 |
| `AllowType` | `House` | `id` 类型，`value` 1 为允许、0 为不允许 |
| `SetOwner` | 对象 | `house` 为新所属方 |
| `SetAmmo`、`AddAmmo` | 对象 | `value` 弹药 |
| `AddPassenger`、`RemovePassenger` | 对象（载具） | `other` 为乘客 |
| `AddEffect`、`RemoveEffect` | 对象 | `id` 效果类型 |
| `AddMark`、`RemoveMark` | 对象或 `House` | `id` 标记；对于 `AddMark`，`value` 为标记的值，`value2` 为它的 `Visibility` |
| `DetonateWeapon` | 格子 | `id` 武器；`other` 为来源（可选） |
| `DetonateWarhead` | 格子 | `id` 弹头，`value` 伤害；`other` 为来源（可选） |
| `DetonateProjectile`、`RemoveProjectile` | `Projectile` | — |
| `EndFor` | `House` | `value` 结果 |
| `SetScore`、`AddScore` | `House` | `value` 分数 |
| `Log` | — | `text` |
| `Convert` | 对象 | `id` 类型 |

`Action.original(n, …)` 运行原版动作 `n`；启用 Ares 时先走 Ares 的分支。4.2 中的具名函数做同样的事。原版用路径点的地方，API 用 `Cell`（`Cell.waypoint(n)` 给出路径点所在的格子）。标为“live”的行作用于标签中持有本次触发的触发器的对象；它们的直接形式改为接收对象参数。除非另有说明，每行都是 `v1`、`sim`；运行原生代码的每行随机数都是 `native`。

### 4.2 原版动作 0..145

| # | API 函数 | 参数 | 说明 |
| --- | --- | --- | --- |
| 0 | — | — | 空操作。只能用 `Action.original(0)`。 |
| 1 | `scenario.declareWinner(country: string)` | 国家 | 作用于玩家，而不是触发器所属方。 |
| 2 | `scenario.declareLoser(country: string)` | 国家 | |
| 3 | `house.beginProduction()` | — | |
| 4 | `ai.createTeam(teamType: TeamTypeId, house: House): Team` | TeamType | 受数量上限限制。小队由 S0–S68 运行。`house` 可为 `NO_HANDLE`（此时用 TeamType 的所属方）；没有创建小队时返回 `NO_HANDLE`。`Action.original(4)` 还会加上原版动作的对象放置范围；模块直接调用时没有这一步，House AI 也没有。 |
| 5 | `ai.destroyTeams(teamType: TeamTypeId)` | TeamType | |
| 6 | `house.allToHunt()` | — | |
| 7 | `ai.reinforce(teamType: TeamTypeId)` | TeamType（使用它自己的路径点） | |
| 8 | `map.dropZoneFlare(cell: Cell)` | 格子 | |
| 9 | `house.fireSale()` | — | |
| 10 | `scenario.playMovie(index: i32)` | 影片编号 | 对每个玩家生效的表现效果。 |
| 11 | `scenario.showText(csfLabel: string)` | CSF 条目名 | |
| 12 | `trigger.destroy()`；`Trigger.destroyType(id: string)` | 触发器 | 删除运行中的实例（延迟进行）。 |
| 13 | `house.beginAutocreate()` | — | |
| 14 | live；直接形式为 `techno.setOwner(house)`（4.3） | 所属方 | |
| 15 | `TriggerDef.allowWin = true` | — | 只是标志，没有可执行的分支。 |
| 16 | `map.revealAll()` | — | |
| 17 | `map.revealAround(cell: Cell)` | 格子 | |
| 18 | `map.revealZone(cell: Cell)` | 格子 | |
| 19 | `sound.play(sound: SoundId)` | 声音 | |
| 20 | `scenario.playTheme(name: string)` | 音乐 | |
| 21 | `scenario.playSpeech(eva: string)` | EVA 名称 | |
| 22 | `trigger.force()`；`Trigger.forceType(id: string)` | 触发器 | 触发时没有执行者，格子为零。 |
| 23 | `scenario.timer.start()` | — | |
| 24 | `scenario.timer.stop()` | — | |
| 25 | `scenario.timer.extend(seconds: i32)` | 秒数 | |
| 26 | `scenario.timer.shorten(seconds: i32)` | 秒数 | |
| 27 | `scenario.timer.set(seconds: i32)` | 秒数 | |
| 28 | `ctx.global(i).set(true)` | 0..49 | 超出范围：参数非法。 |
| 29 | `ctx.global(i).set(false)` | 0..49 | |
| 30 | `house.setAutoBaseBuilding(on: bool)` | 开关 | |
| 31 | `map.growShroud()` | — | |
| 32 | live；直接形式为 `techno.kill(KillMode.Normal, NO_HANDLE)`（4.3） | — | 适用 Ares 的幸存者规则。 |
| 33 | `house.grantSuperWeapon(type: SuperWeaponTypeId, oneTime: bool)` | 超级武器类型 | `oneTime = true`。 |
| 34 | 同上，`oneTime = false` | 超级武器类型 | |
| 35 | `house.setPreferredTargetKind(value: i32)` | 数值 | |
| 36 | `house.transferAllTo(to: House)` | 所属方 | |
| 37 | `house.makeAlly(other: House)` | 所属方 | 双向。 |
| 38 | `house.makeEnemy(other: House)` | 所属方 | 双向。 |
| 39 | — | — | 不开放。 |
| 40 | `scenario.setVisibleMapArea(r: Rect)` | 矩形 | |
| 41 | `map.playAnim(anim: AnimTypeId, cell: Cell)` | AnimType | 适用 Ares `MakeInfantry=` 的所属规则。 |
| 42 | `map.detonateWeapon(weapon, at, NO_HANDLE)`（4.3），`at` 为格子中心 | WeaponType | 与 4.3 的条目是同一个函数。 |
| 43 | `map.playVoxelAnim(type: VoxelAnimTypeId, cell: Cell)` | VoxelAnimType | |
| 44 | `weather.startIonStorm(duration: i32)` | 持续时间 | 风暴进行中时拒绝。 |
| 45 | `weather.stopIonStorm()` | — | 没有风暴时拒绝。 |
| 46 | `scenario.lockInput()` | — | |
| 47 | `scenario.unlockInput()` | — | |
| 48 | `scenario.centerCamera(cell: Cell, speed: i32)` | 格子、速度 | 作用于每个玩家的镜头。客户端形式见第 11 节。 |
| 49 | — | — | 放大。不开放。 |
| 50 | — | — | 缩小。不开放。 |
| 51 | `house.reshroud()` | — | |
| 52 | live；直接形式为 `building.setSpotlightBehavior(v: i32)` | 数值 | |
| 53 | `trigger.enable()` | 触发器 | 按难度准入；重置触发器的计时器。 |
| 54 | `trigger.disable()` | 触发器 | |
| 55 | `scenario.radarEvent(cell: Cell, kind: i32)` | 格子、`RadarEventType` | |
| 56 | `ctx.local(i).set(true)` | 0..99 | |
| 57 | `ctx.local(i).set(false)` | 0..99 | |
| 58 | `map.meteorShower(cell: Cell, intensity: i32)` | 格子、强度 | |
| 59 | `map.reduceOre(cell: Cell)` | 格子 | |
| 60 | live；直接形式为 `building.sell()` | — | |
| 61 | live；直接形式为 `building.setOnline(false)` | — | |
| 62 | live；直接形式为 `building.setOnline(true)` | — | |
| 63 | `map.damageBridgeAt(cell: Cell)` | 格子 | |
| 64 | `map.lightFlash(cell: Cell, size: FlashSize.Small)` | 格子 | 伤害为 50 的命中所产生的闪光。 |
| 65 | 同上，`FlashSize.Medium` | 格子 | 伤害 100。 |
| 66 | 同上，`FlashSize.Large` | 格子 | 伤害 300。 |
| 67 | `scenario.announceWin()` | — | |
| 68 | `scenario.announceLose()` | — | |
| 69 | `scenario.endScenario()` | — | |
| 70 | `tag.destroy()`；`Tag.destroyType(id: string)` | 标签 | 停止正在进行的投递。 |
| 71 | `lighting.setAmbientStep(v: i32)` | 数值 | |
| 72 | `lighting.setAmbientRate(v: i32)` | 数值 | |
| 73 | `lighting.setAmbient(v: i32)` | 数值 | |
| 74 | `house.setAiTriggersActive(true)` | — | |
| 75 | `house.setAiTriggersActive(false)` | — | |
| 76 | `house.setTeamRatio(TeamRatio.AiTriggers, v)` | 数值 | |
| 77 | `house.setTeamRatio(TeamRatio.Aircraft, v)` | 数值 | |
| 78 | `house.setTeamRatio(TeamRatio.Infantry, v)` | 数值 | |
| 79 | `house.setTeamRatio(TeamRatio.Units, v)` | 数值 | |
| 80 | `ai.reinforceAt(teamType: TeamTypeId, cell: Cell)` | TeamType、格子 | |
| 81 | live；直接形式为 `techno.wakeUp()` | — | 睡眠或无害的对象改为 Guard。 |
| 82 | `house.wakeSleepers()` | — | 唤醒不属于该所属方的睡眠对象。 |
| 83 | `house.wakeHarmless()` | — | 唤醒属于该所属方的无害对象。 |
| 84 | `scenario.wakeGroup(group: i32)` | 编组 | |
| 85 | `map.setVeinGrowth(v: i32)` | 数值 | |
| 86 | `map.setOreGrowth(v: i32)` | 数值 | |
| 87 | `map.setIceGrowth(v: i32)` | 数值 | |
| 88 | `map.createParticleSystem(type: ParticleSystemTypeId, cell: Cell)` | ParticleSystem | |
| 89 | `map.removeParticleSystems(cell: Cell)` | 格子 | |
| 90 | `weather.lightningStrike(cell: Cell)` | 格子 | |
| 91 | live；直接形式为 `infantry.goBerserk()` | — | |
| 92 | `house.setFirestorm(true)` | — | 未启用 Ares 时为 `Unsupported`。 |
| 93 | `house.setFirestorm(false)` | — | |
| 94 | `map.ionCannonStrike(cell: Cell)` | 格子 | |
| 95 | `map.nukeStrike(cell: Cell)` | 格子 | |
| 96 | `map.chemStrike(cell: Cell)` | 格子 | |
| 97 | `scenario.toggleTrainCargo()` | — | |
| 98 | `sound.playAtRandomWaypoint(sound: SoundId)` | 声音 | Ares 变体；从原生随机流取数。 |
| 99 | `sound.playAt(sound, at)`（4.3），`at` 为格子中心 | 声音、格子 | 与 4.3 的条目是同一个函数。 |
| 100 | `scenario.playRadarMovie(index: i32, pause: bool)` | 影片 | |
| 101 | `map.reshroudAt(cell: Cell)` | 格子 | |
| 102 | `weather.lightningStormStrike(cell: Cell)` | 格子 | 使用触发器所属方。 |
| 103 | `scenario.timer.setText(csfLabel: string)` | CSF 条目名 | |
| 104 | `team.flash(frames: i32)`；`ai.flashTeams(teamType, frames)` | TeamType、帧数 | |
| 105 | `scenario.talkBubble(team: Team, frame: i32)` | 小队、帧 | `team` 可为 `NO_HANDLE`。 |
| 106 | `rules.setTechLevel(type: TechnoTypeId, level: i32)` | 类型、等级 | 另见第 10 节。 |
| 107 | `ai.reinforceByChrono(teamType: TeamTypeId, cell: Cell)` | TeamType、格子 | |
| 108 | `map.createCrate(kind: i32, cell: Cell)` | 种类（20 为随机） | 随机时从原生随机流取数。 |
| 109 | `map.ironCurtainAt(cell: Cell)` | 格子 | |
| 110 | `scenario.pause(duration: i32)` | 持续时间 | |
| 111 | live；直接形式为 `building.evictOccupants()` | — | |
| 112 | `scenario.jumpCamera(cell: Cell)` | 格子 | |
| 113 | `house.cheer()` | — | |
| 114 | `scenario.setSidebarTab(tab: i32)` | 0..3 | |
| 115 | `scenario.flashCameo(type: TechnoTypeId, duration: i32)` | 类型 | |
| 116 | `sound.stopAt(cell: Cell)` | 格子 | |
| 117 | `scenario.playRadarMovie(index, true)` | 影片 | |
| 118 | `map.clearSmudges()` | — | |
| 119 | `house.destroyAll(Destroy.All)` | — | |
| 120 | `house.destroyAll(Destroy.Buildings)` | — | |
| 121 | `house.destroyAll(Destroy.Land)` | — | |
| 122 | `house.destroyAll(Destroy.Naval)` | — | |
| 123 | `house.mindControlBase(of: House)` | 所属方 | |
| 124 | `house.restoreMindControlledBase(of: House)` | 所属方 | |
| 125 | `map.createObject(type, cell, owner, new CreateParams())`（4.3），用于建筑类型 | 类型、格子 | 没有建成建筑时返回 `NO_HANDLE`。 |
| 126 | `house.restoreStarting(buildingsOnly: false)` | — | |
| 127 | `scenario.chronoScreenEffect(duration: i32)` | 持续时间 | |
| 128 | `house.teleportAll(cell: Cell)` | 格子 | |
| 129 | `house.superWeapon(type).setChargePercent(p: i32)` | 超级武器、百分比 | |
| 130 | `house.restoreStarting(buildingsOnly: true)` | — | |
| 131 | `house.flashBuildings(type: TechnoTypeId, duration: i32)` | 类型 | |
| 132 | `house.superWeapon(type).setRechargeTime(frames: i32)` | 超级武器、时间 | |
| 133 | `house.superWeapon(type).resetRechargeTime()` | 超级武器 | |
| 134 | `house.superWeapon(type).reset()` | 超级武器 | |
| 135 | `house.setPreferredTargetCell(cell: Cell)` | 格子 | |
| 136 | `house.clearPreferredTargetCell()` | — | |
| 137 | `house.setAlternateBase(cell: Cell)` | 格子 | |
| 138 | `house.clearAlternateBase()` | — | |
| 139 | `house.radarBlackout(frames: i32)` | 帧数 | |
| 140 | `house.setDefensiveTargetCell(cell: Cell)` | 格子 | |
| 141 | `house.clearDefensiveTargetCell()` | — | |
| 142 | `lighting.retint(Color.Red, v: i32)` | 数值 | |
| 143 | `lighting.retint(Color.Green, v)` | 数值 | |
| 144 | `lighting.retint(Color.Blue, v)` | 数值 | |
| 145 | `scenario.homeCamera()` | — | |

Ares 动作（仅在启用 Ares 时可用，否则为 `Unsupported`）：

| # | API 函数 |
| --- | --- |
| 146 | `house.addAuxPower(power: i32)` |
| 147 | `house.killDrivers()` |
| 148 | `scenario.setEvaVoice(v: i32)` |
| 149 | `techno.setGroup(group: i32)` |

### 4.3 原子动作

一个函数对应一个操作，这个操作只有一个 owner（[RFC 6](0001-wasm-extension-modules.zh-CN.md#6-api-分层)）。`AtomicKind` 列出的每个函数都可以在触发器中写作带同名种类的 `Action.atomic`。

| 函数 | 语义 | 随机数 | 层级 |
| --- | --- | --- | --- |
| `techno.fireWeapon(w: WeaponSel, at: Target): Status` | 用类型为 `w.weapon` 的武器（它不是 `NO_ID` 时），否则用 `w.index` 槽位中的武器（默认 0，即主武器），向格子、对象或当前目标开火。武器当前无法开火时为 `Refused` | native | v1? |
| `house.launchSuperWeapon(type: SuperWeaponTypeId, cell: Cell): Status` | 发射该所属方拥有的超级武器 | native | v1? |
| `map.spawnAnim(anim: AnimTypeId, at: Coord): Status` | 创建动画 | native | v1? |
| `techno.remove(): Status` | 移除对象：不死亡、不产生幸存者、不计击杀 | none | v1? |
| `techno.kill(mode: KillMode, source: Techno): Status` | 走伤害流程击杀，伤害等于当前生命并无视防御（有死亡动画和幸存者，击杀记给 `source`）；`source` 可为 `NO_HANDLE` | native | v1? |
| `techno.damage(amount: i32, warhead: WarheadTypeId, source: Techno): Status` | 经过弹头和护甲造成伤害，计击杀和经验；`source` 可为 `NO_HANDLE` | native | v1? |
| `techno.setHealth(points: i32): Status` / `heal(points: i32)` | 设定或回复生命，不计击杀 | none | v1? |
| `map.createObject(type: TechnoTypeId, at: Cell, owner: House, c: CreateParams): Techno` | 创建一个完整的对象；格子被占时拒绝（返回 `NO_HANDLE`） | native | v1? |
| `techno.setMission(m: Mission, target: Target \| null = null): Status` | 像玩家指令一样排入一个任务；带格子的 `Move` 移动到那里 | none | v1 |
| `techno.teleport(cell: Cell, rule: Blocked): Status` | 超时空式移动；`Blocked.Nearby` 选附近的格子，`Blocked.Fail` 直接拒绝 | native | v1? |
| `techno.clearTarget(): Status` | 放弃当前目标 | none | v1? |
| `techno.clearDisguise(): Status` | 解除间谍或幻影坦克的伪装 | none | v1? |
| `objects.inRadius(...)` | 区域搜索（一个查询，7.1） | none | v1 |
| `sound.playAt(sound: SoundId, at: Coord)` | 播放声音 | none | v1? |
| `techno.addExperience(points: i32)` / `setRank(r: Rank)` | 增加经验或设定等级 | none | v1? |
| `house.addMoney(amount: i32): Status` | 增加或扣除金钱 | none | v1? |
| `rules.setTechLevel(type, level)` / `house.allowType(type, allowed)` | 修改科技树 | none | v1? |
| `techno.setOwner(house: House): Status` | 转移所属 | none | v1? |
| `techno.clearMindControl(): Status` | 解除心灵控制，把对象还给原所属方 | none | v1? |
| `techno.setAmmo(n: i32)` / `addAmmo(n: i32)` | 修改弹药 | none | v1? |
| `transport.addPassenger(p: Techno)` / `removePassenger(p: Techno)` | 让乘客登上或离开 | native | v1? |
| `techno.addEffect(...)` / `removeEffect(...)` | 见第 5 节 | — | v1 |
| `techno.addMark(...)` / `removeMark(...)` / `setMarkValue(...)`；`House` 上同样 | 见第 6 节 | — | v1 |
| `map.createProjectile(p: ProjectileParams): Projectile` | 创建抛射体：类型、起点、目标对象或坐标、用于击杀和经验归属的来源、16 字节的初始模块数据；失败时返回 `NO_HANDLE`。不是 `AtomicKind` | native | v1? |
| `map.detonateWeapon(weapon: WeaponTypeId, at: Coord, source: Techno): Status` | 不生成抛射体，直接在某个坐标引爆武器；`source` 可为 `NO_HANDLE` | native | v1? |
| `map.detonateWarhead(warhead: WarheadTypeId, damage: i32, at: Coord, source: Techno): Status` | 按给定伤害和来源造成弹头的范围伤害；`source` 可为 `NO_HANDLE` | native | v1? |
| `projectile.detonate(): Status` | 让抛射体立即引爆 | native | later |
| `projectile.remove(): Status` | 移除抛射体，不引爆 | none | later |
| `scenario.endFor(house: House, outcome: Outcome): Status` | 让某个所属方结束游戏，胜利或失败 | none | v1? |
| `house.setScore(points: i32)` / `addScore` | 设置分数 | none | v1? |
| `ctx.log(text: string)` | 向游戏内日志写一行 | none | v1 |
| `techno.convert(type: TechnoTypeId): Status` | 改变类型，同 Ares 的类型转换 | native | v1? |

```ts
class WeaponSel { index: i32 = 0; weapon: WeaponTypeId = NO_ID; }   // weapon wins when set
class Target { techno: Techno = NO_HANDLE; cell: Cell | null = null; current: bool = false; }
enum KillMode { Normal }          // death animation, survivors, credit
enum Blocked { Nearby, Fail }
class CreateParams { inheritTag: Techno = NO_HANDLE; mission: Mission = Mission.None; }   // inheritTag: copy that object's tag
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

代码注释说明：`WeaponSel.weapon` 设置时优先于 `index`。`KillMode.Normal` 带死亡动画、幸存者和击杀奖励。`CreateParams.inheritTag` 表示复制该对象的标签。`ProjectileParams.weapon` 提供伤害和弹头；`toTechno` 和 `source` 可为 `NO_HANDLE`，`source` 用于击杀归属和经验；`data` 最多 16 字节，存档并计入哈希。

## 5. 持续效果

规则（叠加、互斥组、Phobos 开关）：[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-原子动作与持续效果)、[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)。效果类型来自 `[AttachEffectTypes]`（只在 Phobos 打开时读取），或来自 `onRules` 中的 `rules.defineEffectType`（10.1），两者由同一个读取器解析。

### 5.1 函数

```ts
class EffectParams {
  duration: Frames = -1;      // -1: the type's Duration
  delay: Frames = -1;         // -1: none
  source: Techno = NO_HANDLE;
  sourceHouse: House = NO_HANDLE;
}
// namespace Techno (the handle comes first):
addEffect(t: Techno, type: EffectTypeId, p: EffectParams): Effect;   // NO_HANDLE + Refused when stacking refuses
removeEffect(t: Techno, e: Effect): Status;
removeEffects(t: Techno, type: EffectTypeId): i32;    // returns how many
removeEffectGroup(t: Techno, group: EffectGroupId): i32;
effects(t: Techno, out: HandleBuffer): i32;           // attach order
// namespace Effect:
type(e: Effect): EffectTypeId; remaining(e: Effect): Frames; stacks(e: Effect): i32;
source(e: Effect): Techno;   // may be NO_HANDLE
```

全部为 `v1`、`sim`。

### 5.2 效果属性

| 属性 | Phobos 键 | 含义 |
| --- | --- | --- |
| 持续时间 | `Duration`；弹头一侧为 `AttachEffect.Durations`、`AttachEffect.DurationOverrides` | 单位为帧；负数表示无限 |
| 延迟 | `AttachEffect.Delays`、`AttachEffect.InitialDelays`（附加方一侧） | 效果开始前的帧数 |
| 间隔 | —（新属性） | 两次触发之间的帧数 |
| 重新附加前的冷却 | `AttachEffect.RecreationDelays`（附加方一侧） | 到期后要等多少帧才能再次附加 |
| 最大触发次数 | —（新属性；`DiscardOn.Firing.Count` 是开火若干次后丢弃，含义不同） | |
| 来源 | `AttachEffect.SameSourceOnly`、`AttachEffect.IgnoreFromSameSource`、`AttachEffect.CumulativeSourceMaxCount` | 关于效果来源的规则 |
| 变形时继承 | `AllowTransfer.Convert` | 对象转换类型时保留 |
| 创建时继承 | `AllowTransfer`；`AttachEffects.AttachOnOwnerChange` | |
| 无视铁幕 | `PenetratesIronCurtain` | |
| 叠加 | `Cumulative`、`Cumulative.MaxCount`（负数表示无上限）；`AttachEffect.ReplaceLongerDuration`；`AttachEffect.CumulativeRefreshAll`、`.OnAttach`、`AttachEffect.CumulativeRefreshSameSourceOnly` | 见 5.3 |
| 类型分组 | `Groups` | 供 `RemoveGroups`、`RequiredGroups`、`DisallowedGroups` 使用的名字 |
| 需要电力 | `Powered` | 只在对象有电时生效 |
| 丢弃 | `DiscardOn`、`DiscardOn.*` | 在移动、开火、受伤、任务、地形类型等情况下结束 |
| 到期武器 | `ExpireWeapon`、`ExpireWeapon.TriggerOn`、`.CumulativeOnlyOnce`、`.UseInvokerAsOwner` | 到期时发射一次武器 |

### 5.3 叠加与互斥组

每种效果类型的叠加方式是待定问题 13。映射到 Phobos 键的方式如下：

| 规则 | Phobos 键 |
| --- | --- |
| 叠加到上限 | `Cumulative=yes`、`Cumulative.MaxCount=N` |
| 新的替换旧的 | `Cumulative=no`（刷新已有实例） |
| 拒绝新的 | 新属性，同一个读取器 |
| 互斥组 | 新属性，`ExclusiveGroup=`（名称未定） |

### 5.4 效果种类

| 效果 | `[AttachEffectType]` 上的 Phobos 键 | 默认叠加方式 | 互斥组 | 层级 |
| --- | --- | --- | --- | --- |
| 染色 | `Tint.Color`、`Tint.Intensity`、`Tint.VisibleToHouses`、`Tint.Cumulative` | 替换 | — | v1 |
| 附着动画 | `Animation`、`CumulativeAnimations`、`Animation.OfflineAction`、`Animation.TemporalAction`、`Animation.UseInvokerAsOwner`、`Animation.HideIfAttachedWith` | 替换 | — | v1 |
| 阻止死亡 | `Unkillable` | 替换 | — | v1 |
| 变成另一类型 | — | 拒绝 | 运动组 | v1? |
| 替换武器 | — | 替换 | — | v1? |
| 运动（被抛起、被推动） | — | 拒绝 | 运动组 | later |
| 传送 | — | 拒绝 | 运动组 | v1? |
| 受伤倍率 | `ArmorMultiplier`（及 `.AllowWarheads`、`.DisallowWarheads`、`.Chance`、`.Delay`、`.AffectsHouse`、`.HitAnim`） | 叠加 | — | v1 |
| 火力倍率 | `FirepowerMultiplier` | 叠加 | — | v1 |
| 速度倍率 | `SpeedMultiplier` | 叠加 | — | v1 |
| 射速倍率 | `ROFMultiplier`、`ROFMultiplier.ApplyOnCurrentTimer` | 叠加 | — | v1 |
| 射程 | `WeaponRange.Multiplier`、`WeaponRange.ExtraRange`、`.AllowWeapons`、`.DisallowWeapons` | 叠加 | — | v1 |
| 目标优先级倍率（配合 `TargetChanged`，RFC 6） | — | 叠加 | — | v1? |
| 隐形 | `Cloakable`；`ForceDecloak` | 替换 | — | v1 |
| 对某弹头或攻击类型免疫 | —（倍率为 0 的 `ArmorMultiplier.AllowWarheads` 可覆盖一部分） | 替换 | — | v1? |
| 禁止开火 | `DisableWeapons` | 替换 | — | v1 |
| 停止索敌 | — | 替换 | — | v1? |
| 伤害修正（固定加值、强制取值） | —（`ReflectDamage.*` 是反弹伤害，不是修正） | 叠加 | — | v1? |
| 无法被选中 | — | 替换 | — | v1? |
| 无视玩家指令 | — | 替换 | — | v1? |
| 不可被选为目标 | — | 替换 | — | v1? |
| 所属方断电 | — | 替换 | — | v1? |
| 暴击 | `Crit.Multiplier`、`Crit.ExtraChance`、`Crit.AllowWarheads`、`Crit.DisallowWarheads` | 叠加 | — | v1 |
| 反弹伤害 | `ReflectDamage`、`ReflectDamage.*` | 叠加 | — | v1 |
| 复仇武器 | `RevengeWeapon`、`RevengeWeapon.*` | 叠加 | — | v1 |
| 激光拖尾 | `LaserTrail.Type` | 替换 | — | v1 |
| 穿透力场护盾 | `PenetratesForceShield` | 替换 | — | v1 |

键一列为 `—`：没有 Phobos 键的新效果种类，随模块功能一起打开（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)）。

## 6. 标记

规则：[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-原子动作与持续效果)。调用接受 `Mark.id(name)` 返回的 `MarkId`（[ID](#id)）。所属方标记与对象标记使用相同的调用和层级。

```ts
// namespace Techno, and the same in namespace House (the handle comes first):
addMark(t: Techno, m: MarkId, value: i32, visibility: Visibility): Status;
removeMark(t: Techno, m: MarkId): Status;
hasMark(t: Techno, m: MarkId): bool;
markValue(t: Techno, m: MarkId): i32;                // NotFound when absent
setMarkValue(t: Techno, m: MarkId, value: i32): Status;
marks(t: Techno, out: IdBuffer): i32;    // order: the order the marks were added
```

| 条目 | 语义 | 层级 |
| --- | --- | --- |
| `addMark` | 添加 `m` 及其值和可见度；再次添加时仍只保留一个标记，并设置两者 | v1 |
| `removeMark` | 移除 `m`；不存在时返回 `Ok` | v1 |
| `hasMark` | 按 ID 判断 | v1 |
| `markValue`、`setMarkValue` | 读取或修改对象或所属方携带的标记的值；没有该标记时为 `NotFound` | v1 |
| `marks` | 把对象或所属方的标记写入 `out` | v1 |
| 所属方标记 | 上面的全部函数用于 `House`；`House` 也是原子动作 `AddMark` 和 `RemoveMark` 的目标 | v1 |
| 效果携带的标记 | 效果类型列出若干标记，效果生效期间对象携带它们（新属性，例如 `Marks=`；名称未定） | v1? |

## 7. 查询

查询是只读的；顺序、缓冲区和分页见[查询结果](#查询结果)。`techno.effects`、`marks` 和 `scenario.houses` 的结果保持它们自己的顺序（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-查询)）。“客户端”一列说明客户端模块按 [RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块) 的规则得到什么：

- `all`：与模拟中的答案相同（公开的事实）；
- `own`：对玩家自己的所属方及其对象给出答案；对其他任何对象，调用返回它的空值（`NO_HANDLE`、`NO_ID`、`-1`、`false` 或数量 0），并设置 `Refused`；
- `seen`：自己的对象给出全部答案，其他所属方中玩家当前能看到的对象按 RFC 10 所述的限制给出答案；其他对象不存在；
- `sim`：只用于模拟；客户端调用返回它的空值，并设置 `Unsupported`。

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

| 函数 | 返回 | 层级 | 客户端 |
| --- | --- | --- | --- |
| `objects.inCell(c: Cell, f: Filter, out: HandleBuffer)` | `i32` 数量 | v1 | seen |
| `objects.inRadius(at: Coord, leptons: i32, f: Filter, out: HandleBuffer)` | `i32` 数量 | v1 | seen |
| `objects.inRect(r: Rect, f: Filter, out: HandleBuffer)` | `i32` 数量 | v1 | seen |
| `objects.all(f: Filter, out: HandleBuffer)` | `i32` 数量 | v1 | seen |
| `objects.nearest(at: Coord, leptons: i32, f: Filter)` | `Techno`（没有时为 `NO_HANDLE`） | v1? | seen |
| `objects.projectilesInRadius(at: Coord, leptons: i32, out: HandleBuffer)` | `i32` 数量 | v1? | seen |
| `techno.type()` | `TechnoTypeId` | v1 | seen（伪装的敌人返回伪装类型） |
| `techno.owner()` | `House` | v1 | seen（伪装的敌人返回伪装的所属方） |
| `techno.health()`、`strength()`、`healthPercent()` | `i32` | v1 | seen |
| `techno.coord()`、`cell()` | `Coord`、`Cell` | v1 | seen |
| `techno.mission()` | `Mission` | v1 | own |
| `techno.target()` | `Target` | v1 | own |
| `techno.rank()`、`experience()` | `Rank`、`i32` | v1 | own |
| `techno.ammo()` | `i32` | v1 | own |
| `techno.passengers(out: HandleBuffer)`、`transport()` | `i32` 数量、`Techno`（没有时为 `NO_HANDLE`） | v1 | own |
| `techno.effects(out: HandleBuffer)`、`marks(out: IdBuffer)` | `i32` 数量 | v1 | `effects`：own；`marks`：按可见性（第 6 节） |
| `techno.disguise()` | `DisguiseInfo \| null`（类型 ID、所属方）；模拟模块读到的是真实状态 | v1 | own |
| `techno.isCloaked()` | `bool` | v1? | own |
| `techno.layer()`、`landType()` | 枚举 | v1 | own |
| `projectile.type()`、`coord()`、`source()`、`target()`、`data()` | 值；`type()` 是 `ProjectileTypeId` | v1? | `type`、`coord`：seen；其他：own |
| `objects.canFireAt(t: Techno, w: i32, at: Target)`、`inRange(...)` | `bool`（探测） | v1? | own |

### 7.2 所属方

| 函数 | 返回 | 层级 | 客户端 |
| --- | --- | --- | --- |
| `house.name()`、`country()`、`side()` | `string`、`string`、`i32` | v1 | all |
| `house.money()` | `i32` | v1 | own |
| `house.powerOutput()`、`powerDrain()`、`isLowPower()` | `i32`、`bool` | v1 | own |
| `house.buildings(f, out: HandleBuffer)` / `count(type: TechnoTypeId)` | `i32` 数量、`i32` | v1 | own |
| `house.canBuild(type: TechnoTypeId)` | `bool`（前提条件已满足） | v1 | own |
| `house.techLevel()` | `i32` | v1 | own |
| `house.isAlly(other)`、`allies(out: HandleBuffer)`、`enemies(out: HandleBuffer)` | `bool`、`i32` 数量 | v1 | all |
| `house.isDefeated()`、`isHuman()` | `bool` | v1 | all |
| `house.superWeapon(type: SuperWeaponTypeId)` | `SuperWeapon`（该所属方不拥有它时为 `NO_HANDLE`；可读取是否就绪、充能） | v1? | own |
| `house.stats()` | 损失数和摧毁数 | v1? | own |
| `scenario.houses(out: HandleBuffer)` | `i32` 数量，顺序为所属方数组的顺序 | v1 | all |

### 7.3 地图

| 函数 | 返回 | 层级 | 客户端 |
| --- | --- | --- | --- |
| `map.size()` | `Rect` | v1 | all |
| `map.landType(c: Cell)` | `LandType` | v1 | all |
| `map.height(c: Cell)` | `i32` | v1 | all |
| `map.overlay(c: Cell)`、`ore(c: Cell)` | `OverlayTypeId`（没有时为 `NO_ID`）；数量 | v1 | seen（玩家当前能看到的格子） |
| `map.passable(c: Cell, zone: i32)` | `bool` | v1 | seen（玩家当前能看到的格子） |
| `map.isVisibleTo(c: Cell, h: House)` | `bool` | v1 | own（`h` 必须是玩家自己的所属方） |
| `Cell.waypoint(n: i32)` | 带有路径点编号 `n` 的 `Cell`（地图有这个路径点时设置 `x` 和 `y`）；由接收这个格子的调用解析，路径点不存在时为 `NotFound` | v1 | all |
| `map.cellTag(c: Cell)` | `Tag`（没有时为 `NO_HANDLE`） | v1 | sim |
| `map.canPlace(type: TechnoTypeId, c: Cell, h: House)` | `bool`（探测） | v1? | own（`h` 必须是玩家自己的所属方） |
| `map.closestPlaceable(type: TechnoTypeId, near: Cell, h: House)` | `Cell \| null`（探测） | v1? | own（`h` 必须是玩家自己的所属方） |

### 7.4 规则

| 函数 | 返回 | 层级 | 客户端 |
| --- | --- | --- | --- |
| `rules.getString(section: string, key: string)` | `string \| null` | v1 | all |
| `rules.getInt(section, key, fallback: i32)` | `i32` | v1 | all |
| `rules.getBool(section, key, fallback: bool)` | `bool` | v1 | all |
| `rules.getFixed(section, key, fallback: Fixed)` | `Fixed`（`I16F16`） | v1 | all |
| `rules.getList(section, key)` | `StringList` | v1 | all |
| `rules.types(list: string)` | `StringList`（例如 `"InfantryTypes"`） | v1 | all |

解析后的值就是引擎实际使用的值；Ares、Phobos 启用时也包括它们的键。引擎从不读取的键，仍然从合并后的文本中给出值。

### 7.5 场景

| 函数 | 返回 | 层级 | 客户端 |
| --- | --- | --- | --- |
| `ctx.frame()` | `i32` | v1 | all |
| `scenario.difficulty()` | `Difficulty` | v1 | all |
| `scenario.gameMode()` | `string` | v1 | all |
| `scenario.playerCount()` | `i32` | v1 | all |
| `scenario.timer.remaining()` | `Frames` | v1 | all |
| `ctx.global(i).get()`、`ctx.local(i).get()` | `bool` | v1 | sim |
| `Trigger.find(id)`、`Tag.find(id)` | 句柄 | v1 | sim |

## 8. 覆盖点

规则：链式调用和按来源的绑定见 [RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)；时机、批处理、顺序、不重入和额度见 [RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)；被停用的模块见 [RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。每一行都是版本 1 的候选；其中两三个进入版本 1（待定问题 10）。

- 覆盖点绑定到类型，从不全局生效：武器、弹头或 TechnoType 用一个 INI 键（名称属于待定问题 4）绑定它，键的值是同一来源的模块中的一个函数名。
- 下面的签名是按单个查询写的。批量调用时，SDK 生成的分派函数对每个查询运行一次该函数，就像 `onEvents` 对每条记录所做的那样。
- 查询类和结果类携带句柄和 ID，从不携带名字；批次写入模块内存，并从那里读回。

```ts
class DamageQuery {          // Override.Damage
  target: Techno;
  source: Techno;            // may be NO_HANDLE
  warhead: WarheadTypeId;
  weapon: WeaponTypeId;      // NO_ID when the damage has no weapon
  native: i32;               // the damage the native rules computed
}
class DamageResult { damage: i32 = 0; handled: bool = false; }   // not handled: the previous link's result stands
class ShotQuery {            // Override.Fire
  shooter: Techno;
  weapon: WeaponTypeId;
  slot: i32;                 // weapon slot, 0 the primary
  target: Target;
}
class ShotPlan {             // projectiles to create; at most a fixed number
  count: i32;
  at(i: i32): ProjectileParams;
  add(p: ProjectileParams): Status;
  clear(): void;
}
class OrderQuery {           // Override.CommandAllowed
  kind: i32;                 // the Command variant (11.4)
  target: Target;
}
class DeathQuery {           // Override.Death
  killer: Techno;            // may be NO_HANDLE
  warhead: WarheadTypeId;    // NO_ID when none
  native: DeathResult;       // what the native death chain would do
}
class DeathResult {
  deathWeapon: WeaponTypeId = NO_ID;
  survivors: bool = true;
  wreck: bool = false;
  handled: bool = false;
}
class TargetBatch {          // Override.TargetChoice; a view into module memory
  count: i32;
  searcher(i: i32): Techno;
  candidates(i: i32, out: HandleBuffer): i32;   // the native candidates
  native(i: i32): Techno;                       // the native choice; may be NO_HANDLE
  previous(i: i32): Techno;                     // the previous link's choice
  choose(i: i32, t: Techno): void;              // not called: the previous link's choice stands
}
```

代码注释说明：`DamageQuery.source` 可为 `NO_HANDLE`；伤害没有武器时 `weapon` 为 `NO_ID`；`native` 是原生规则算出的伤害。`DamageResult` 的 `handled` 为 `false` 时，上一环的结果保持不变。`ShotQuery.slot` 是武器槽位，0 为主武器。`ShotPlan` 是要生成的抛射体，数量有固定上限。`OrderQuery.kind` 是 `Command` 变体（11.4）。`DeathQuery.killer` 可为 `NO_HANDLE`，没有弹头时 `warhead` 为 `NO_ID`，`native` 是原生死亡流程本来会做的事。`TargetBatch` 是模块内存中的一个视图：`candidates` 写出原生候选，`native` 是原生的选择（可为 `NO_HANDLE`），`previous` 是上一环的选择；没有对某个搜索者调用 `choose` 时，上一环的选择保持不变。

| 覆盖点 | 签名 | 调用时机 | 批处理 | 层级 |
| --- | --- | --- | --- | --- |
| `Override.Damage` | `(ctx: Ctx, d: DamageQuery, previous: DamageResult): DamageResult` | 绑定的弹头或受伤方类型每产生一个伤害包调用一次，在伤害步骤 | 伤害步骤是批量提交时**批处理**；其他地方每个伤害包调用一次 | v1? |
| `Override.TargetChoice` | `(ctx: Ctx, batch: TargetBatch): void`：为每个搜索者写入一个选择（或保留上一环的选择） | 每个绑定的武器类型每帧一次，带上该类型的全部搜索者及其原生候选，在并行目标扫描之后 | **批处理**，从不按单个候选调用 | v1? |
| `Override.ProductionAllowed` | `(ctx: Ctx, house: House, type: TechnoTypeId, native: bool, previous: bool): bool` | 每帧对每个所属方和每个绑定类型各一次，在固定的步骤；侧边栏和 AI 读取缓存的结果 | 无 | v1? |
| `Override.Fire` | `(ctx: Ctx, shot: ShotQuery, previous: ShotPlan \| null, plan: ShotPlan): bool`：`previous` 是前一环的方案，没有前一环接管时为 `null`（即原生射击）；`plan` 一开始是 `previous` 的副本，或为空；返回 `true` 表示用 `plan` 接管这次射击（[RFC 9](0001-wasm-extension-modules.zh-CN.md#9-示例抛射体弹道)）；返回 `false` 保留上一环的方案，或原生射击 | 绑定武器每次射击，在开火步骤 | 开火步骤是批量代码段时**批处理**；其他地方每次射击调用一次 | v1? |
| `onLaunch` | `(ctx: LaunchCtx, p: Projectile): Path \| null` | 绑定类型的每个抛射体一次 | 无 | later |
| `onTrajectoryFrame` | `(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | 每个绑定的抛射体类型每帧一次 | 按类型**批处理**（L4） | later |
| `Override.CommandAllowed` | `(ctx: Ctx, t: Techno, order: OrderQuery, native: bool, previous: bool): bool` | 绑定类型每收到一条玩家或 AI 指令，在模拟执行这条指令时；光标的本地预览不调用它 | 无 | v1? |
| `Override.Death` | `(ctx: Ctx, t: Techno, q: DeathQuery, previous: DeathResult): DeathResult` | 绑定类型每次死亡，在死亡流程中 | 死亡流程在批量代码段中运行时**批处理**；其他地方每次死亡调用一次 | v1? |

## 9. AI

电脑所属方保留它们自己的 AI（[RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)）。所有条目都属于 `sim`。

### 9.1 函数

| 函数 | 语义 | 层级 |
| --- | --- | --- |
| `ai.defineTaskForce(id: string, d: TaskForceDef): Status` | 新增或替换一个特遣部队 | v1 |
| `ai.defineScript(id: string, lines: ScriptLine[]): Status` | 新增或替换一个小队脚本（最多 50 行） | v1 |
| `ai.defineTeamType(id: string, d: TeamTypeDef): Status` | 新增或替换一个小队类型 | v1 |
| `ai.defineAiTrigger(id: string, d: AiTriggerDef): Status` | 新增或替换一个 AI 触发器；它的当前权重变为初始权重 | v1 |
| `ai.createTeam(teamType, house)` | 启动一个小队（受 Max 限制）；与动作 4（4.2）是同一个函数 | v1 |
| `ai.createTeamUnlimited(teamType, house)` | 启动一个小队，不受 Max 限制，与脚本动作 18 相同 | v1? |
| `team.disband()` | 销毁小队，记录 AI 触发器的结果，释放成员 | v1 |
| `team.addMember(t: Techno)` / `removeMember(t: Techno)` | 加入或离开，会执行小队的准入检查 | v1? |
| `team.setScript(script: ScriptTypeId)` | 修改运行中小队的脚本 | v1 |
| `team.jumpTo(line: i32)` | 移动小队的脚本游标 | v1? |
| `ai.setAiTriggerEnabled(trigger: AiTriggerId, on: bool)` | 启用或禁用一个 AI 触发器 | v1 |
| `ai.setAiTriggerWeights(trigger: AiTriggerId, start, min, max)` | 运行时修改权重 | later |
| `house.setAiTriggersActive(on)` | 所属方级别的开关（动作 74、75） | v1 |
| `house.setTeamRatio(kind, v)` | 所属方的比例（动作 76 至 79） | v1 |
| 读取：`ai.teams(out: HandleBuffer)`、`team.type()`、`team.script()`、`team.cursor()`、`team.members(out: HandleBuffer)`、`team.succeeded()` | | v1 |

### 9.2 定义

定义属于初始化调用，接受名字；运行时的调用使用 `ai.teamTypeId`、`ai.scriptTypeId` 和 `ai.aiTriggerId` 返回的 ID（[ID](#id)）。

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

- `whiner` 会被读取，但没有效果。
- `TechLevel`、`Prebuild`、`LooseRecruit` 和 `IonImmune` 出现在原版 AIMD.INI 中，但不会被读取，所以不作为字段。AI 触发器的第 3、11、13 个字段没有被使用，也不作为字段。
- AI 触发器的条件：-1 总是成立；0 敌方拥有 N 个该类型；1 本所属方拥有；2 敌方电力盈余低于 100；3 低于 0；4 比较敌方金钱；5 己方铁幕即将就绪；6 己方超时空传送仪即将就绪；7 平民阵营的所属方拥有。

### 9.3 小队脚本动作

`ScriptLine.action` 取以下编号。作为数据，全部是 `v1`。“对应的原子动作”一列给出模块要对单个对象做同样的事时调用的函数。

| # | `ScriptAction` | 参数 | 说明 | 对应的原子动作 |
| --- | --- | --- | --- | --- |
| 0 | `AttackQuarry` | 目标类别（2..11） | | `setMission(Hunt)` |
| 1 | `AttackWaypoint` | 路径点 | | `setMission(Attack, cell)` |
| 2 | `GoBerserk`（惯用名） | — | 空操作；永不结束。 | — |
| 3 | `MoveToWaypoint` | 路径点 | | `setMission(Move, cell)` |
| 4 | `MoveToCell` | 打包的格子（x = arg%128，y = arg/128） | | `setMission(Move, cell)` |
| 5 | `Guard` | `arg*15` 帧 | | `setMission(Guard)` |
| 6 | `JumpToLine` | 从 1 开始的行号 | | `team.jumpTo` |
| 7 | `PlayerWins` | — | | `scenario.endFor` |
| 8 | `Unload` | 1、2、3 | | `removePassenger` |
| 9 | `Deploy` | — | | — |
| 10 | `FollowFriendlies` | — | | — |
| 11 | `DoMission` | 任务 ID | | `setMission` |
| 12 | `SetGlobal` | 下标 | | `ctx.global(i).set(true)` |
| 13 | `IdleAnimation` | 动画 | | — |
| 14 | `LoadOntoTransport` | — | | `addPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | 路径点 | | — |
| 16 | `PatrolToWaypoint` | 路径点 | | `setMission(Patrol)` |
| 17 | `ChangeScript` | ScriptType 序号 | | `team.setScript` |
| 18 | `ChangeTeam` | TeamType 序号 | | — |
| 19 | `Panic` | — | | — |
| 20 | `ChangeHouse` | 国家下标 | | `setOwner` |
| 21 | `Scatter` | — | | — |
| 22 | `GoToNearbyShroud` | — | 非单人模式下拒绝执行。 | — |
| 23 | `PlayerLoses` | — | | `scenario.endFor` |
| 24 | `PlaySpeech`（惯用名） | — | 只前进到下一行。 | — |
| 25 | `PlaySound` | SOUNDMD 序号 | | `sound.play` |
| 26 | `PlayMovie` | 影片 | | `scenario.playMovie` |
| 27 | `PlayMusic` | 音乐主题 | | `scenario.playTheme` |
| 28 | `ReduceOre` | — | | `map.reduceOre` |
| 29 | `BeginProduction` | — | | `house.beginProduction` |
| 30 | `FireSale` | — | | `house.fireSale` |
| 31 | `SelfDestruct` | — | | `kill` |
| 32 | `IonStormStart` | 持续时间 | | `weather.startIonStorm` |
| 33 | `IonStormEnd` | — | | `weather.stopIonStorm` |
| 34 | `CenterCamera` | 速度 | | `scenario.centerCamera` |
| 35 | `ReshroudMap` | — | | `house.reshroud` |
| 36 | `RevealMap` | — | | `map.revealAll` |
| 37 | `DeleteMembers` | — | | `remove` |
| 38 | `ClearGlobal` | 下标 | | `ctx.global(i).set(false)` |
| 39 | `SetLocal` | 下标 | | `ctx.local(i).set(true)` |
| 40 | `ClearLocal` | 下标 | | `ctx.local(i).set(false)` |
| 41 | `Unpanic` | — | | — |
| 42 | `ForceFacing` | 朝向 | | — |
| 43 | `WaitUntilFullyLoaded` | — | | — |
| 44 | `TruckUnload` | — | | `convert` |
| 45 | `TruckLoad` | — | | `convert` |
| 46 | `AttackEnemyBuilding` | 类型 `arg&0xFFFF`，模式 `arg>>16` | | — |
| 47 | `MoveToEnemyBuilding` | 同上 | | — |
| 48 | `ScoutHouses` | — | | — |
| 49 | `RegisterSuccess` | — | 前进，并把小队标记为成功。 | — |
| 50 | `Flash` | 帧数 | | — |
| 51 | `PlayAnimation` | 低 16 位为动画，高 16 位为循环次数 | | `map.spawnAnim` |
| 52 | `TalkBubble` | 帧 | | — |
| 53 | `GatherAtEnemyBase` | Ares：+ `AISafeDistance` | | — |
| 54 | `RegroupAtBase` | Ares：+ `AIFriendlyDistance` | | — |
| 55 | `IronCurtain` | Ares：`SW.Group` | | `launchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | 同 46 | | `teleport` |
| 57 | `ChronoshiftToQuarry` | 目标类别 | | `teleport` |
| 58 | `MoveToOwnBuilding` | 类型、模式 | | — |
| 59 | `AttackBuildingAtWaypoint` | 路径点 | | — |
| 60 | `EnterGrinder` | — | | — |
| 61 | `OccupyTankBunker` | — | | — |
| 62 | `EnterBioReactor` | — | | — |
| 63 | `OccupyBattleBunker` | — | | — |
| 64 | `GarrisonNearest` | — | | — |
| 65, 66 | — | — | 不使用。 | — |
| 67 | `AresSeekVehicle` | — | Ares；与 64 共用分支，被步骤检查挡住，执行不到。 | — |
| 68 | `AresConvert` | — | 仅 Ares。 | `convert` |

## 10. 规则访问

### 10.1 `onRules` 写入

层的顺序、处理和类型 ID：[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)、[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)。

```ts
class RulesWriter extends RulesReader {
  set(section: string, key: string, value: string): Status;   // value is INI text
  remove(section: string, key: string): Status;
  addType(list: string, id: string): Status;                  // e.g. ("InfantryTypes", "E7")
  defineEffectType(id: string, keys: string): EffectTypeId;   // keys: INI text of the type's section; works without Phobos
}
```

| 函数 | 语义 | 层级 |
| --- | --- | --- |
| `set`、`remove`、`addType` | 写入模块的规则层，经过原生读取器、取值限制和读取后处理 | v1 |
| `defineEffectType` | 用持续效果的读取器定义一个效果类型，不论 Phobos 是否打开 | v1 |
| 在 `onRules` 中读取 | 前面各层处理后的规则 | v1 |

### 10.2 对局中的修改

对局中的 `rules.set(section, key, value)` 只对白名单中的键起作用（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)；白名单是待定问题 11）。下面的表格是候选。

**使用时读取**（普通值，没有派生副本）：

| 节类别 | 键 | 地图上已有的对象 | 说明 |
| --- | --- | --- | --- |
| Weapon | `Damage`、`ROF`、`Burst`、`MinimumRange` | 下次使用时取新值 | |
| Weapon | `Range` | 同上 | 仅当抛射体 `ROT>0` 时；`ROT=0` 时它参与计算 `Speed`（仅限 onRules） |
| Weapon | `Report`、`Anim` | 同上 | 表现层 |
| Warhead | `Verses` | 同上 | |
| Warhead | `CellSpread`、`PercentAtMax`、`Wall`、`Wood`、`Sparky`、`Fire`、`Conventional`、`Rocker`、`InfDeath`、`AnimList` | 同上 | |
| TechnoType | `Cost`、`BuildTime` 类倍率 | 按需计算 | |
| TechnoType | `Speed` | 按需换算 | 检查各移动方式自己的状态 |
| TechnoType | `ROT`、`Points`、`Soylent`、`Crushable`、`Selfhealing`、`Explodes`、`VeteranAbilities`、`EliteAbilities` | 同上 | |
| TechnoType | `Power` | 重新计算每个拥有它的所属方的电力 | |
| Projectile | `Inaccurate`、`Arcing`、`Shadow` | 新生成的抛射体 | `Arcing` 需谨慎（见“仅限 onRules”） |
| 全局 | 数值型的 `[General]`、`[CombatDamage]`、`[AudioVisual]` 字段 | 同上 | 按帧换算的值需要重新换算 |

**在构造时复制到每个对象中的键**（只有写明已有对象的处理规则后才能进白名单）：

| 键 | 需要的规则 |
| --- | --- |
| `Strength` | 保持生命点数或保持比例 |
| `Sight` | 更新每个对象及其已揭示的区域 |
| `Ammo`、`InitialAmmo` | 修改每个对象的容量 |
| `Storage` | 更新容量 |
| `Passengers`、`SizeLimit` | 更新容量；超出时怎么办 |
| 类型上的 Phobos `AttachEffect.*` | 只影响新对象 |

**仅限 onRules**（派生缓存或读后处理）：

| 键 | 原因 |
| --- | --- |
| 武器 `Range`、`[General] Gravity`、抛射体 `ROT`、`Floater`、Phobos 抛射体 `Gravity` | 会改写 `ROT=0` 抛射体所用武器的 `Speed` |
| `Primary`、`Secondary`、`Weapon{n}`、`EliteWeapon{n}`、`OccupyWeapon`、武器的 `Warhead`/`Projectile` | 规则读取时解析为句柄 |
| `Prerequisite`、`PrerequisiteOverride`、`[General] Prerequisite*`、Ares `Prerequisite.List{n}` | 解析为建筑下标 |
| `Owner`、`RequiredHouses`、`ForbiddenHouses` | 国家位掩码 |
| `Factory` | 工厂映射 |
| `Foundation`、`[AI] AIBaseSpacing`、`BuildConst`、`IsBaseDefense`、`UndeploysInto`、`Spawned`/`MissileSpawn` | 几何信息和基地列表，复制到每个对象 |
| `Spawns`、`SpawnsNumber`、心灵控制和时间武器的设置 | 管理器只构建一次 |
| `Armor`（名称） | 解析为 `ArmorId` |
| `Image`、美术键、`Sequence` | 美术和动画绑定需要素材 |
| Ares `PoweredBy`、`PowersUnit`、`PoweredUnit` | 跨类型修正 |
| 列表节、任何指向新类型的键 | 类型只在读取规则时新增（RFC 2） |
| `[Colors]`、国家的 `Color`、`Side`、`ParentCountry` | 颜色渐变，国家和阵营的映射 |
| 声音和 EVA 键 | 加载后绑定为下标 |
| `TechLevel` | 改用 `rules.setTechLevel`（动作 106） |

Ares 和 Phobos 的键：白名单要逐个写明能否在对局中修改；目前还没有对任何一个做出分类。

## 11. 客户端 API

除非另有标注，所有条目都属于 `client` 和阶段 2（`v1`）。客户端从不直接修改游戏状态：它发出标准玩家指令，或者发送消息（[RFC 1](0001-wasm-extension-modules.zh-CN.md#1-两个世界)、[10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)）。

### 11.1 界面树

```ts
class Ui {
  find(name: string): Node;        // NO_HANDLE when no node has that name
  create(n: NodeSpec): Node;
  remove(n: Node): Status;
  registerHotkey(name: string, defaultKey: string): Status;
  viewer(): House;                 // NO_HANDLE for a spectator
  isSpectator(): bool;
  lastStatus(): Status;            // as ctx.lastStatus(); Gone after a call on a NO_HANDLE or removed node
}
// namespace Node (a handle; it comes first). A call on a NO_HANDLE or removed node does nothing (RFC 3).
name(n: Node): string;
setLabel(n: Node, csfOrText: string): void;   // Label
setArt(n: Node, name: string): void;          // Art
setPosition(n: Node, x: i32, y: i32): void;
setSize(n: Node, w: i32, h: i32): void;
setVisible(n: Node, v: bool): void;
setItems(n: Node, items: string[]): void;     // List
setFill(n: Node, v: i32): void;               // Bar: how full the bar is
setClickable(n: Node, enabled: bool): void;   // routes clicks to the module's onClick
```

代码注释说明：`Ui.find` 在没有节点叫这个名字时返回 `NO_HANDLE`；`viewer()` 对观战者返回 `NO_HANDLE`；`lastStatus()` 与 `ctx.lastStatus()` 相同，对 `NO_HANDLE` 或已删除的节点调用之后为 `Gone`。对 `NO_HANDLE` 或已删除的节点的调用什么也不做（RFC 3）。`setClickable` 把这个节点的点击转给模块的 `onClick`。

| 操作 | 层级 |
| --- | --- |
| 按名字 `find` | v1 |
| `create`、`remove` | v1 |
| `setLabel`、`setArt`、`setPosition`、`setVisible` | v1 |
| `setItems` | v1 |
| `setSize`、`setFill` | v1? |
| `setClickable`（点击到达 `onClick`）、悬停、热键 | v1 |
| 数据绑定（属性绑定到一个公开的值） | later |

控件类型（待定问题 14）。客户端代码使用 `veraui.ini` 的名字：`new Column(Hold.BottomLeft, parts)` 对应 `Type=Column`、`Hold=BottomLeft`、`Parts=`。

| 类型 | 层级 |
| --- | --- |
| `Text` | v1 |
| `Button` | v1 |
| `Row`、`Column` | v1 |
| `CameoGrid` | v1? |
| `List` | v1? |
| `Image` | v1? |
| `Bar`（进度条） | later |

### 11.2 INI 布局键

静态布局使用 `veraui.ini` 的语法（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)）。目前已有的键：

| 键 | 含义 |
| --- | --- |
| `Type` | `Button`、`Text`、`Row`、`Column`、`CameoGrid`；本 RFC 新增 `List` |
| `Hold` | `TopLeft`、`Top`、`TopRight`、`Left`、`Middle`、`Right`、`BottomLeft`、`Bottom`、`BottomRight` |
| `X`、`Y`、`Width`、`Height` | 位置和尺寸 |
| `Parts` | 行或列的子节点 |
| `Gap` | 各部分之间的像素数 |
| `Label` | 按钮文字或 CSF 条目名；本 RFC 把它加到 `Text` 部件上 |
| `Art` | 一张 SHP 图片 |
| `Command` | 键盘命令名（`StopObject`、`GuardObject`……） |
| `Value` | 部件显示的数据来源：`Credits`、`Power`、`Drain`、`Name`、`Time`、`Kills`、`Losses` |
| `Tab`、`Columns`、`Rows` | 图标网格 |

客户端代码使用相同的名字：`new Text(label)`、`Node.setLabel`、`Node.setArt`。`Value` 只保留它在 INI 中的含义；`Bar` 的填充用 `node.setFill` 设置，对象上方显示的数字用 `display(t).setNumber` 设置（11.3）。

### 11.3 单个对象的显示属性

它们只影响本机的画面（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)）。

| 属性 | 层级 |
| --- | --- |
| `display(t).setArt(name)` | v1? |
| `display(t).setPalette(name)` | v1? |
| `display(t).setTint(color, intensity)`（模拟中的染色是一种效果，见第 5 节） | v1? |
| `display(t).addAnim(name)` / `removeAnim` | v1? |
| `display(t).setVisible(v)` | v1? |
| `display(t).setLabel(text)`、`setNumber(v)`、`addButton(name)` | v1? |
| 小地图可见性、地图标注（点、线、文字） | later |

### 11.4 选择与指令

| 函数 | 语义 | 层级 |
| --- | --- | --- |
| `client.selection(out: HandleBuffer)` | 本地选择 | v1 |
| `client.select(list: HandleBuffer, count: i32, append: bool)` | 选择，作为玩家指令 | v1 |
| `client.order(o: OrderSpec)` | 一条标准玩家指令，像任何玩家输入一样校验；`OrderSpec.kind` 是 `Command` 的一个变体：`Move`、`MoveToObject`、`Stop`、`Attack`、`AttackMove`、`Guard`、`DeployMcv`、`UndeployBuilding`、`SetRally`、`QueueProduction`、`SuspendProduction`、`CancelProductionByType`、`PlaceReadyBuilding`、`SellBuilding`、`ToggleRepair`、`EjectBunker`、`UnloadPassengers`、`CaptureBuilding`、`PlantC4`、`LaunchSuperWeapon`、`ToggleInfantryDeploy`、`ForceAttackCell`、`SellWallAtCell` | v1 |
| `client.sendMessage(data: Uint8Array): Status` | 进入命令流，带有模块的来源，到达每个模拟模块的 `onMessage`；在观战者的机器上为 `Refused`；限制大小和频率（[RFC 11](0001-wasm-extension-modules.zh-CN.md#11-联机)） | v1 |
| 指令改写钩子 | 在玩家的指令发出之前改写它 | later |

### 11.5 镜头、声音、存储、消息和日志

| 函数 | 语义 | 层级 |
| --- | --- | --- |
| `camera.center(at: Coord, speed: i32)` | 移动本机镜头，同动作 48 | v1? |
| `camera.jump(at: Coord)` | 让本机镜头跳转，同动作 112 | v1? |
| `camera.home()` | 同动作 145 | v1? |
| `sound.play(name)`、`sound.playAt(name, at)` | 在本机播放声音 | v1 |
| `sound.speech(eva)` | 在本机播放 EVA 语音 | v1 |
| `storage.get(key: string): Uint8Array \| null` | 读取模块来源的存储（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） | v1 |
| `storage.set(key: string, data: Uint8Array): Status` | 写入存储；超出配额时为 `Refused` | v1 |
| `messages.show(csfOrText: string, color: i32)` | 显示一行消息 | v1 |
| `ui.log(text: string)` | 写入游戏内控制台 | v1 |

### 11.6 客户端查询

第 7 节的查询函数在客户端中以相同名字、相同的缓冲区存在，每个函数按它的“客户端”一列给出答案。其他读取遵守同样的规则：`effects` 和 5.1 中的读取为 `own`，`marks` 按其可见性（第 6 节），9.1 中的 AI 读取为 `sim`。`ui.viewer()` 给出本机所属方，观战者为 `NO_HANDLE`（`ui.isSpectator()`）。客户端模块中的 `Mark.id` 只查找名字。

