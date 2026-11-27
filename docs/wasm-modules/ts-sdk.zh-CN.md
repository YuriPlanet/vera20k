# TypeScript SDK 参考

[English](ts-sdk.md) | [简体中文](ts-sdk.zh-CN.md)

Wasm 模块的 TypeScript SDK `@vera20k/sdk`（VERA20K 源码里的 `sdk/typescript`）的每个命名空间、类和函数，以及它的 `vera20k` 命令。第一部分讲模拟模块（`rules.wasm`），第二部分讲客户端模块（`ui.wasm`）。模块放在哪、有哪些限制见[模块指南](guide.zh-CN.md)；一步步的做法见[模拟模块教程](ts-tutorial.zh-CN.md)和[客户端模块教程](ts-ui-tutorial.zh-CN.md)。

这个 SDK 的功能和 [Rust SDK](rust-sdk.zh-CN.md) 相同，只是换成了 TypeScript 的名字；本页对某个条目的含义写得简略时，Rust 参考和 [RFC 附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)里有更多说明。属性、事件、动作表以及事件和决定的视图都是从 Rust SDK 生成的，所以编号完全相同。

**层级。** 接口是预览版（包版本 0.1.0）。RFC 把每个条目分到一个层级：`v1`（第 1 版的候选）、`preview`（现在能用，可能不经通知就改）和 `later`（不在第 1 版里；引擎对它的 `hasKind` 返回 0，使用时返回 `Unsupported`）。下面不是 `v1` 的条目会注明。

## 目录

- [准备](#准备)：[`vera20k` 命令](#vera20k-命令)、[`vera.json`](#verajson)、[`vera20k build` 做了什么](#vera20k-build-做了什么)、[约定](#约定)
- 模拟模块：[回调](#回调)、[基本调用](#基本调用)、[引用](#引用)、[属性](#属性)、[修饰器](#修饰器)、[变量](#变量)、[搜索](#搜索)、[动作](#动作)、[任务与所属方指令](#任务与所属方指令)、[规则](#规则)、[触发器](#触发器)、[事件](#事件)、[可决定事件](#可决定事件)、[INI 注册](#ini-注册)、[通知与消息](#通知与消息)
- 客户端模块：[概述](#客户端模块)、[客户端回调](#客户端回调)、[客户端调用](#客户端调用)、[读取游戏状态](#读取游戏状态)、[节点](#节点)、[玩家、镜头与光标](#玩家镜头与光标)、[本地动作](#本地动作)、[客户端事件](#客户端事件)、[存储](#存储)

## 准备

需要 Node.js 20 或更新的版本。包发布之前，从 VERA20K 源码使用 SDK：先在 `sdk/typescript` 里运行一次 `npm install`，然后用 `node sdk/typescript/bin/vera20k.js init <dir>` 建项目。这样建的项目依赖这份源码（它的 `package.json` 里是 `"@vera20k/sdk": "file:…/sdk/typescript"`）。

| 导入 | 用于 |
| --- | --- |
| `@vera20k/sdk` | 模拟模块（`rules.wasm`） |
| `@vera20k/sdk/client` | 客户端模块（`ui.wasm`），以及模拟 SDK 里在那里也能安全使用的部分 |

编译器是 AssemblyScript，固定为 0.28.20。TypeScript 本身（5.9.3）只做类型检查。

### `vera20k` 命令

| 命令 | 作用 |
| --- | --- |
| `vera20k init <dir>` | 在 `<dir>` 里新建模拟模块项目，`<dir>` 必须为空或不存在：`package.json`（脚本 `build`、`watch`、`check`）、`tsconfig.json`、`vera.json`、`.gitignore`、`src/index.ts`。包名取文件夹名 |
| `vera20k init <dir> --ui` | 同上，建客户端项目：`"kind": "ui"`，以及客户端的 `src/index.ts` |
| `vera20k build [dir] [--out file] [--debug]` | 构建 `dir`（默认 `.`）里的项目。`--out` 覆盖 `vera.json` 的 `out`（相对于项目）。`--debug` 用 `-O1` 并带调试信息构建，而不是优化体积的发布构建 |
| `vera20k watch [dir] [--out file] [--debug]` | 先构建，之后入口文件所在文件夹（`src/`，不含 `src/generated/`）或 `vera.json` 每次变动后 100 毫秒再构建 |
| `vera20k generate [dir]` | 检查 `vera.json` 并据此写出 `src/generated/`，不编译（`tsc` 需要这些文件） |

在项目里由 `npm run build`、`npm run watch` 和 `npm run check`（`vera20k generate && tsc --noEmit -p .`）运行它们；选项写在 `--` 后面，如 `npm run build -- --out ../mod/rules.wasm`。`check` 会先生成 `src/generated/`，所以第一次构建之前也能用。

### `vera.json`

| 键 | 默认值 | 含义 |
| --- | --- | --- |
| `kind` | `"rules"` | `"rules"`（模拟模块）或 `"ui"`（客户端模块） |
| `entry` | `"src/index.ts"` | 要编译的文件，相对于项目 |
| `out` | `"build/rules.wasm"` 或 `"build/ui.wasm"` | 输出文件，相对于项目 |
| `vars` | `[]` | 声明的变量（见下） |
| `notify` | `[]` | 模块会发的通知名，如 `["WaveStarted"]` |

`vars` 的一项：

| 键 | 取值 |
| --- | --- |
| `name` | 不含空格的可打印 ASCII，最长 65,535 字节；忽略大小写后不能重复 |
| `scope` | `"global"`、`"house"` 或 `"object"`；模拟项目里必填，缺了或写错都是构建错误 |
| `visibility` | `"private"`（默认）、`"allies"`、`"insight"`（也可写 `"in-sight"`）、`"public"`；不区分大小写 |
| `initial` | 初始值，整数（默认 0）；也可以写成 `default` |

```json
{
  "kind": "rules",
  "entry": "src/index.ts",
  "vars": [
    { "name": "Wave", "scope": "global", "visibility": "public", "initial": 1 },
    { "name": "Panicked", "scope": "object", "visibility": "private", "initial": 0 }
  ],
  "notify": ["WaveStarted"]
}
```

在客户端项目（`"kind": "ui"`）里，`vars` 和 `notify` 不声明任何东西：它们只为模拟模块声明的名字生成 `ClientVar` 和 `NotifyName` 常量。那里每个 `vars` 条目都生成 `ClientVar`，所以不用写 `scope`、`visibility` 和 `initial`，写了也会被忽略。

### `vera20k build` 做了什么

1. 读取 `vera.json`，检查变量名和通知名（可打印 ASCII，忽略大小写后不重复），模拟项目里还检查每个变量的 `scope`。
2. 在入口文件旁边生成 `generated/vars.ts`（每个变量一个常量：模拟项目里是 `GlobalVar`、`HouseVar` 或 `ObjectVar`，客户端项目里是 `ClientVar`）和 `generated/notify.ts`（每个名字一个 `Notify` 或 `NotifyName`）。列表为空时不写对应的文件，列表清空后会删掉原来的文件，内容没变时也不重写。`vera20k generate` 只做这一步。名字里字母、数字、`_`、`$` 以外的字符变成 `_`，作为标识符（`map.Wave` 变成 `map_Wave`）。
3. 把入口文件和 SDK 对应种类的导出文件一起编译，使用 AssemblyScript 的 incremental 运行时、`--noAssert`（去掉 `assert` 调用），不带 `--debug` 时还有 `-O3 --shrinkLevel 2 --converge`。
   - 顶层代码变成 `vera_init` 导出（客户端模块是 `client_init`），绝不会生成 `start` 段。
   - 没有 `env.abort` 导入：运行时错误（下标越界、解引用 `null`、`throw`）先通过模块自己的日志记下 `abort: <message> at <file>:<line>:<column>`，再陷入（trap）。
   - 1,024 以下的内存地址留给投递区。
   - 只保留代码里登记过的生命周期导出：只有调用了 `onFrame` 才有 `on_frame`，只有用了 `enqueue`、`enqueueWith`、`ActionDef.module`、`namedAction`、`Ini.key` 或 `Ini.registry` 才有 `on_events`，只有用了 `decideSync`，或者 `namedAction` 的处理函数接收决定视图，才有 `on_decide`，依此类推，所以引擎不会做模块不需要的调用。构建是在源码文字里找这些调用的（忽略注释）。
4. 收集 `namedAction("Name", fn)` 的名字。名字必须是字符串字面量。忽略大小写后不能重复；两个名字的分派键撞了也会被拒绝（“rename one”）。
5. 拒绝导入了本世界以外函数的模块：模拟模块只能导入 `vera:sim@1.0`，客户端模块只能导入 `vera:client@1.0`。提示会列出每个外来导入，比如 `env.seed`（`Math.random`）、`env.console.log`（`console`）、`env.Date.now`（`Date`），或者 `vera:sim@1.0.log`（客户端模块里用了模拟端的调用）。
6. 追加自定义段：`vera.delivery`（第 8 到 1,024 字节的投递区，总是有）、`vera.actions`（命名动作及其分派键，有的话），模拟模块还有来自 `vera.json` 的 `vera.vars` 和 `vera.notify`。
7. 打印输出路径、大小和命名动作。

引擎的其他加载检查它不做；那些由 `asset wasm-check` 做。

### 约定

- **错误。** 写操作返回 `Status`；读取或创建的调用直接返回值，失败时返回空值（`0`、`NO_HANDLE` 或 `NO_ID`），这时 `lastStatus()` 给出原因。

  | `Status` | 含义 |
  | --- | --- |
  | `Ok` | 成功 |
  | `Gone` | 引用是 0，或对象已经不存在 |
  | `NotFound` | 没有这个名字或 ID，或者路径点不存在 |
  | `Refused` | 游戏拒绝了：地格被占、现在不能写这个值、配额满了、构建器超出上限 |
  | `Unsupported` | 这一局或这个版本没有，或者是 `later` 层级的条目 |

  用错了接口不会返回状态：无效的参数（作用域和变量不符、随机数范围是负的、在决定函数里写东西……）会让模块被停用。
- **引用是 `i64`。** `Techno`、`House`、`Cell` 等引用类型都是 `i64` 的别名，通过命名空间函数读写，引用放第一个参数（`Techno.health(t)`）。`NO_HANDLE`（0）表示没有对象。
- **视图**（`KilledEvent`、`TakeDamageDecision`、`ClickEvent`……）直接读取引擎的记录，只在回调期间有效。每个视图都有 `record`（完整的 `EventRecord`）；决定视图还有 `request`（`DecideRecord`）。
- **每次登记都会运行。** `onRules`、`onScenarioStart`、`onMessage`、`onStart`、`onRender`、`onFrame` 和 `onClientEvent` 每调用一次就多一个函数，全部按登记顺序运行。
- **语言。** 代码是 AssemblyScript 兼容的 TypeScript；[教程](ts-tutorial.zh-CN.md#6-这里的-typescript-和平常有什么不同)列出了因此不能用的写法（捕获局部变量的闭包、`for…of`、`try`、联合类型、没有类型的对象字面量）。
- 源码里标了 `@internal` 的名字（`check`、`utf8`、`OUT`、`keyOf`、`run*`……）虽然导出了，但只供 SDK 自己用，不要依赖它们。

---

# 模拟模块

模拟模块（`rules.wasm`）导入 `vera:sim@1.0`，在每台机器上和对局同步运行。下面的一切都由 `@vera20k/sdk` 导出。

## 回调

回调在顶层代码里登记。构建会保留每个回调需要的引擎导出。

| 函数 | 导出 | 引擎何时调用 |
| --- | --- | --- |
| 顶层代码 | `vera_init` | 最先调用一次：同时用了 `onRules` 的模块在规则阶段调用，否则在对局安装时调用。`Ini.*` 注册只能在这里做 |
| `onRules(fn: () => void)` | `on_rules` | 规则读入时调用一次（读完 `rulesmd.ini` 之后、处理规则之前）。在这里 `Rules.set` 可以写任何键 |
| `onScenarioStart(fn: () => void)` | `on_scenario_start` | 对局开始时调用一次，在模块的 INI 记录之后 |
| `onFrame(every: Every, fn: () => void)` | `on_frame` | 帧号能被 `every.frames` 整除的每一帧，在模块阶段结束时调用。每个函数调用一次 `onFrame`，各有各的间隔 |
| `new Every(frames: i32)` | | `onFrame` 的间隔（正常速度下 15 帧是一秒；小于 1 按 1 算） |
| `onMessage(fn: (sender: House, data: Uint8Array) => void)` | `on_message` | 在模块阶段、事件之前调用，同一来源的客户端模块每发一条消息调用一次。`data` 是消息字节的副本 |
| `namedAction<T>(name: string, fn: (e: T) => void): i32` | `on_events`（`fn` 接收决定视图时是 `on_decide`）、`vera.actions` 段 | 给 `fn` 起名字供 INI 使用（`Action=Wasm:name`；接收决定视图的处理函数用 `Decide=Wasm:name`），返回它的分派键。`name` 必须是字符串字面量 |
| `actionKey(name: string): i32` | | 命名动作的分派键（对小写后的名字做 FNV-1a，置第 30 位），和构建写进去的一样 |

交给 `Trigger.enqueue`、`Trigger.decideSync`、`ActionDef.module`、`Ini.key` 和 `Ini.registry` 的处理函数会自动得到分派键，不需要手写。`T` 是处理函数接收的记录视图：事件视图、决定视图、`EventRecord`、`DecideRecord` 或 INI 记录。

对局开始时的顺序：mod 的 `vera_init` 和 `on_rules`，地图的规则，地图的 `vera_init` 和 `on_rules`；读取 INI 触发器；然后每个模块依次收到它的 INI 记录并运行 `on_scenario_start`。每一帧都是 mod 的模块先于地图的模块运行。

## 基本调用

| 函数 | 作用 |
| --- | --- |
| `frame(): i32` | 当前帧号 |
| `random(lo: i32, hi: i32): i32` | 从模块自己的随机数流取一个 `lo..hi`（含两端）的数，每台机器都一样。`lo > hi` 会让模块被停用。在决定函数里它从一个子流取数，不影响主流 |
| `log(line: string)` | 在聊天栏（每帧最多 8 行，每行显示 160 个字符）和 `logs/ra2.log` 里写一行 |
| `fail(why: string)` | 记下 `why`，并在每台机器上永久停用模块。用于无法使用的值，比如格式不对的注册 INI 键；请写明段和键 |
| `lookup(kind: NameKind, name: string): i32` | `name` 的 ID，或 `NO_ID`。名字只查一次，把 ID 存下来 |
| `nameOf(kind: NameKind, id: i32): string` | `id` 的名字，或 `""` |
| `text(id: i32): string` | 引擎按字符串 ID 保存的文字：模块动作的参数文字、INI 值、字符串属性。ID 0 是 `""` |
| `hasKind(table: i32, id: i32): bool` | 这个引擎是否实现了 `table` 的第 `id` 项（0 事件种类，1 动作种类，2 属性，3 名字种类，4 搜索范围） |
| `lastStatus(): Status` | 最近一次返回值的 SDK 调用的状态 |

`NameKind`：`TechnoType`、`Weapon`、`Warhead`、`Projectile`、`Anim`、`VoxelAnim`、`ParticleSystem`、`SuperWeapon`、`Armor`、`Overlay`、`Sound`、`Country`、`TeamType`、`ScriptType`、`AiTrigger`、`Variable`、`Notify`、`ModuleAction`、`String`、`Property`、`EventKind`、`ActionKind`、`Hotkey`、`ModuleEvent`。这个引擎对 `Armor`、`Overlay`、`Sound`、`ScriptType`、`AiTrigger` 和 `EventKind` 返回 `Unsupported`。

## 引用

| 类型（`i64`） | 是什么 | 怎么得到 |
| --- | --- | --- |
| `Ref` | 任何引用 | `get`、`set` 和 `Find` 接受它 |
| `Techno` | 单位或建筑（它的创建序号） | 搜索、事件、`Action.createObject` |
| `Projectile` | 飞行中的抛射体（preview） | `Launch` 或 `ProjectileFrame` 决定 |
| `House` | 所属方（玩家或电脑） | `Techno.owner(t)`、搜索、`House.fromIndex(i)` |
| `Cell` | 地图上的地格 | `Cell.at(x, y)` |
| `Waypoint` | 地图上的路径点 | `Waypoint.number(n)` |
| `TypeRef` | 规则里的类型 | `Rules.technoType("E1")` 以及其他 `Rules.*` 查找、`TypeRef.make(kind, id)` |
| `Modifier` | 修饰器 | `Modifier.*`、`Action.modify` |
| `TriggerRef` | 触发器 | `Trigger...done()` |

| 函数 | 作用 |
| --- | --- |
| `NO_HANDLE` | 0：没有对象 |
| `NO_ID` | −1：没有 ID |
| `TypeRef.make(kind: NameKind, id: i32)` | `kind` 里第 `id` 个类型的引用；不调用游戏 |
| `TypeRef.id(t)`、`TypeRef.kind(t)` | 它的 ID 和名字种类；不调用游戏 |
| `TypeRef.any(kind)` | 某个名字种类的通配符，只用于 `Find.types` |
| `Cell.at(x, y)`、`Cell.x(c)`、`Cell.y(c)` | 坐标和地格互相转换；不调用游戏 |
| `Waypoint.number(n)`、`Waypoint.cell(n)` | 第 `n` 个路径点（同一个函数的两个名字）；接受地格的函数会解析它 |
| `House.fromIndex(index)` | 数组下标为 `index` 的所属方。暂定：引擎把所属方编码为下标加上第 47 位（RFC 待定问题 5） |
| `Global.REF` | 全局规则的引用（附录 7.8） |
| `Rules.anim(name): i32` | 动画的 ID，`Action.spawnAnim`、`attachAnim` 和 `playAnimAt` 接受它 |

## 属性

| 函数 | 作用 |
| --- | --- |
| `get(reference: Ref, prop: Prop): i64` | `prop` 的实际值，算上修饰器 |
| `set(reference: Ref, prop: Prop, value: i64): Status` | 通过游戏自己的代码写 `prop` |
| `getText(reference, prop): string` | 字符串属性（先 `get`，再 `text`） |
| `getMany(refs: StaticArray<i64>, props: StaticArray<i32>, out: StaticArray<i64>): i32` | 一次调用读出每个引用的 `props`，按行存进 `out`（`out[i * props.length + j]`），返回写入的值的个数。`out` 不够大时返回 `Refused` |
| `propNamed(name: string): Prop` | 按引擎里的名字取属性，或 `NO_ID` |

`Prop` 是一组常量的命名空间（`Prop.Health`、`Prop.Money`、`Prop.FirepowerMultiplier`……），类型 `Prop` 就是 `i32`。引擎名字里的 `.` 写成 `_`：`Prop.Passable_Foot` 就是 `Passable.Foot`。完整列表以及值的类型、能否写入和层级见[附录第 7 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#7-属性)。

能用 `set` 写的：`Health`（不记击杀；0 或以下返回 `Refused`）、`Owner`、`Rank`、`Mission`、`Ammo`、`Online`，触发器的 `Enabled`、`House` 和 `Repeat`，所属方的 `ControlDomains`，超级武器的 `ChargePercent`。可修饰的属性拒绝 `set`。

读取函数（每个一次 `get`）和写入函数：

| 命名空间 | 函数 |
| --- | --- |
| `Techno` | `health`、`owner`、`typeRef`、`mission`（`Mission`）、`ammo`、`layer`（`Layer`）、`target`（对象或地格引用）、`experience`、`storage`、`transport`、`passengerCount`、`transportSpaceLeft`、`disguiseType`、`disguiseOwner`、`moving`（`bool`）、`facing`、`turretFacing`、`armor`、`cell` |
| `Techno`（写入） | `setHealth`、`setOwner(t, house)`、`setMission(t, Mission)`（像所属方玩家下令一样）、`setAmmo`、`setOnline(t, bool)` |
| `Techno.read(list, props): StaticArray<i64>` | 用 `getMany` 读进一个新数组；会分配内存，热路径上请用 `getMany` 加自己的缓冲区 |
| `House` | `money`、`name`（`string`）、`isHuman`、`isDefeated`（`bool`）；`order`（见[所属方指令](#任务与所属方指令)） |
| `Cell` | `landType`、`height`、`overlay`（`TypeRef`）、`ore`、`waypoint`（没有时是 −1）、`passable(c, SpeedType)` |
| `Global` | `frame`、`difficulty`（0 简单，1 普通，2 困难）、`missionTimer`、`missionTimerRunning`、`ambientLight`、`ionStormActive` |

属性值用的枚举：`Category`（`Infantry`、`Vehicle`、`Aircraft`、`Building`、`Projectile`）、`Layer`（`Underground`、`Surface`、`Ground`、`Air`、`Top`）、`Relation`（`SameHouse`、`Ally`、`Enemy`、`Neutral`）、`SpeedType`（`Foot` … `FloatBeach`；`SpeedType.passable(speed)` 给出对应的地格属性）、`Mission`（`Sleep`、`Attack`、`Move`……`Hunt`……`SpyplaneOverfly`，编号和游戏一致）。

**视角属性。** `Relation`、地格的 `Revealed` 和 `Placeable`、`Visible`、`Engage` 以及 TechnoType 的 `Buildable` 取决于从谁的视角读。直接 `get` 会返回 `Unsupported`。请在触发器的筛选条件里读（按触发器的所属方），或者在带 `.seenBy(house)` 的搜索里读。

## 修饰器

修饰器在一段时间内改变某个对象的属性，到时自己消失。不同模块的修饰器和 Phobos 的 AttachEffect 会叠加；没有修饰器时，游戏的计算和以前完全一样。

| 函数 | 作用 |
| --- | --- |
| `Modifier.multiply(target, prop, factor: i64, frames: i32): Modifier` | 乘以一个 `fixed` 系数（`fixed(1.5)`） |
| `Modifier.add(target, prop, value: i64, frames: i32): Modifier` | 加上 `value`（比如 `WeaponRangeExtra`，单位 lepton） |
| `Modifier.overrideWith(target, prop, value: i64, frames: i32): Modifier` | 持续期间替换成这个值 |
| `Modifier.create(target, prop, op: ModifierOp, value, frames, source = 0, group = 0): Modifier` | 完整形式：`ModifierOp`（`Add`、`Multiply`、`Override`），来源对象（或 0），以及互斥组（0 表示不分组；本模块在同一对象上同一组的第二个修饰器会替换第一个） |
| `Modifier.remove(m): Status` | 提前移除；已经过期时返回 `Gone` |
| `fixed(value: f64): i64`、`FIXED_ONE` | 定点数（16 位小数）：`fixed(1.0) == FIXED_ONE == 65536` |

每个函数返回修饰器，失败时返回 `NO_HANDLE`。`frames` 小于 0 表示直到移除。用 `get(m, Prop.Remaining)` 读修饰器（还有 `Property`、`Op`、`Value`、`Source`、`Group`、`Origin`）；过期的修饰器读出来是 `Gone`。`Action.modify` 和 `Action.removeModifiers` 是对应的动作形式。

| 属性 | 运算 | 层级 |
| --- | --- | --- |
| `FirepowerMultiplier`、`ArmorMultiplier` | Add、Multiply、Override | v1 |
| `SpeedMultiplier`（地面单位、飞机、喷气兵、火箭） | Add、Multiply、Override | v1 |
| `ROFMultiplier`、`WeaponRangeMultiplier`、`WeaponRangeExtra` | Add、Multiply、Override | v1 |
| `Cloakable`、`ForceDecloak`、`DisableWeapons` | Override | v1 |
| `ROTMultiplier`（载具和建筑车身的转向速度） | Multiply | preview |
| `ImmunityFlags`（第 0..6 位：心控、辐射、毒、心灵伤害、超时空、EMP、狂暴） | Override | preview |
| `CritMultiplier`、`CritExtraChance`、`Unkillable` | 见附录 | later；只在带 Phobos 的版本里生效 |

## 变量

| 类或函数 | 作用 |
| --- | --- |
| `Var.local(n): GlobalVar` | 红警 2 的第 `n` 个局部变量（0..99） |
| `Var.global(n): GlobalVar` | 红警 2 的第 `n` 个全局变量（0..49） |
| `new GlobalVar(name)` | 声明的全局变量：`get(): i64`、`set(value): Status` |
| `new HouseVar(name)` | 声明的所属方变量：`get(house)`、`set(house, value)` |
| `new ObjectVar(name)` | 声明的对象变量：`get(object)`、`set(object, value)`；它跟着对象走 |
| `Var` | 基类：`id`（第一次使用时按名字查，名字不存在时是 `NO_ID`）、`name`、`read(scope)`、`write(scope, value)`，全局变量的 scope 是 0，否则是所属方或对象 |
| `Scope`、`Visibility` | 声明用的枚举（`Global`、`House`、`Object`；`Private`、`Allies`、`InSight`、`Public`） |

模块的变量在 `vera.json` 里声明（[见上](#verajson)）；构建会生成 `src/generated/vars.ts`，所以很少需要自己写 `new HouseVar(...)`：

```ts
import { log } from "@vera20k/sdk";
import { Wave } from "./generated/vars";

function nextWave(): void {
  Wave.set(Wave.get() + 1);
  log("wave " + Wave.get().toString());
}
```

声明的变量会存档、计入校验，客户端模块按它的可见性读取：`Private`（这个所属方自己的玩家）、`Allies`、`InSight`（只用于对象作用域）、`Public`。另一个来源的变量写成 `mod.Name` 或 `map.Name`。筛选条件（`whereVar`）、`Trigger.onVariable` 和 `ActionDef.setVar` 接受任何 `Var`。

## 搜索

`Find` 在模块内存里构建查询；调用 `page` 或 `first` 之前不会碰到引擎。

| 调用 | 作用 |
| --- | --- |
| `Find.objects()` | 单位和建筑，按创建顺序 |
| `Find.houses()` | 所属方，按下标 |
| `Find.cells()` | 地格，按地格下标；用 `near` 或 `within` 缩小范围 |
| `Find.types(t: TypeRef)` | 和 `t` 匹配的类型；`TypeRef.any(kind)` 列出这个种类的所有类型 |
| `.near(center: Cell, radius: i32)` | 离这个地格 `radius` lepton 以内（256 lepton 是一格） |
| `.within(topLeft: Cell, width, height)` | 在一个地格矩形里 |
| `.ofType(t)`、`.ownedBy(house)` | 按类型、按所属方筛选 |
| `.country(country: TypeRef)` | 某个国家的条目（`Rules.country(name)`）；即一个 `Country` 条件 |
| `.where(prop, op: Op, value: i64)` | 只留下 `prop` 比较成立的候选（`Op.Eq`、`Ne`、`Lt`、`Le`、`Gt`、`Ge`） |
| `.seenBy(house)` | 只要 `house` 看得见的，并按它的视角读视角属性 |
| `.page(out: StaticArray<i64>, after: i64): FindPage` | 从排序键 `after`（0 表示从头）开始填 `out`。返回的页有 `count`（写入的个数）和 `next`（下一页从哪里开始，最后一页是 0）。返回的页对象是共用的：在下一次搜索之前读完 |
| `.first(): i64` | 第一个匹配项或 `NO_HANDLE`，会自动翻页 |

- 一个 `Find` 最多 16 个条件（`MAX_CLAUSES`）；更多时搜索返回 `Refused`。
- 一次调用最多检查 16,384 个候选，花费超过这次调用预算的一半时还会更少。所以一页不满甚至是空的，只有 `next` 为 0 时才是最后一页；`next != 0` 时就继续调用。
- 一个 `Find` 可以只建一次（在顶层代码里），反复运行。

## 动作

`Action` 里每个引擎能执行的动作对应一个函数。每个都是一次宿主调用，返回 `Status`（`createObject` 和 `modify` 返回创建出的东西，失败时是 `NO_HANDLE`）。参数无效会让模块被停用；游戏拒绝时返回 `Refused`。

**原子动作**（附录 5.3）：

| 函数 | 作用 | 层级 |
| --- | --- | --- |
| `damage(target, amount, warhead: TypeRef, source = NO_HANDLE)` | 按护甲规则造成伤害；`source` 记击杀和经验 | v1 |
| `kill(target, source)` | 走伤害流程击杀，记给 `source`（没有时写 `NO_HANDLE`） | v1 |
| `remove(target)` | 直接移除：不死亡、不出幸存者、不记功 | v1 |
| `heal(target, amount)` | 回血，最多回到 Strength，不记功 | v1 |
| `createObject(at: Ref, kind: TypeRef, house): Techno` | 在地格或路径点上创建单位或建筑；地格被占时返回 `Refused` | v1 |
| `setMission(target, mission, at: Ref = 0)`、`setMissionOn(target, mission, on: Techno)` | 指向地格或路径点（没有时写 0）或某个对象的任务 | v1 |
| `teleport(target, to: Ref, blocked: Blocked)` | 超时空传送载具或步兵；目标格被占时 `Blocked.Nearby` 或 `Blocked.Fail` | v1 |
| `clearTarget`、`clearDisguise`、`clearMindControl` | 如名 | v1 |
| `addExperience(target, points)` | 加上击杀价值 `points` 的单位所得的经验 | v1 |
| `addMoney(house, amount)` | 给钱；负数是扣钱 | v1 |
| `allowType(house, kind, allow: bool)` | `false` 禁止这一方造某个 TechnoType，`true` 解除本模块的禁止 | v1 |
| `setOwner(target, house)` | 把对象交给这一方 | v1 |
| `addAmmo(target, amount)` | 在类型的 `Ammo=` 范围内加弹药 | v1 |
| `addPassenger(transport, passenger)` | 运输工具会装载时立即上车 | v1 |
| `setProperty(target, prop, value)` | `set` 的动作形式 | v1 |
| `modify(target, prop, op, value, frames, group): Modifier`、`removeModifiers(target, group, prop = -1)` | 修饰器的动作形式；`prop` 为 −1 时移除这一组所有属性的修饰器 | v1 |
| `detonateWeapon(at, weapon: TypeRef, source)` | 不发射抛射体，直接在地格或对象处引爆武器 | v1 |
| `detonateWarhead(at, warhead: TypeRef, damage)` | 弹头的范围伤害 | v1 |
| `fireWeapon(shooter, slot, at = NO_HANDLE, cell = 0)` | 立即用 0 或 1 号武器向它的目标、对象 `at` 或地格 `cell` 开火；不能开火时返回 `Refused` | v1 |
| `launchSuperWeapon(house, kind: TypeRef, at: Ref)` | 发射这一方的超级武器 | v1 |
| `spawnAnim(at, anim: i32)` | 播放动画（`Rules.anim`） | v1 |
| `attachAnim(target, anim: i32, frames: i64 = 0)`、`detachAnim(target, anim: i32 = -1)` | 在对象上附着播放一次动画，或让它循环 `frames` 帧（负数：直到 `detachAnim`） | preview |
| `endFor(house, outcome: Outcome)` | 为某一方结束对局（`Outcome.Win`、`Outcome.Lose`） | v1 |
| `setScore(house, n)`、`addScore(house, n)` | 分数 | v1 |
| `log(line)` | `log` 的动作形式 | v1 |
| `convert(target, kind: TypeRef, frames = 0)` | 改变对象的类型，持续 `frames` 帧，0 表示永久 | v1 |
| `raiseEvent(event, target, other, value, value2, house = NO_HANDLE)` | 在 `target` 上触发模块事件（[INI 注册](#ini-注册)；全局事件写 `Global.REF`）；不能在决定函数里用 | v1 |
| `push(target, toward: Cell, distance, frames)` | 在 `frames` 帧内把地面单位朝某个地格推 `distance` lepton（负数是推开） | preview |
| `setFacing(target, facing)` | 把车身转向 0..255 | preview |

**原版动作**（地图触发器动作，附录 5.2），按参数分类：

| 参数 | 函数 |
| --- | --- |
| 无 | `revealAll`、`timerStart`、`timerStop`、`growShroud`、`ionStormStop`、`lockInput`、`unlockInput`、`announceWin`、`announceLose`、`endScenario`、`toggleTrainCargo`、`clearSmudges`、`homeCamera` |
| 字符串 | `declareWinner(country)`、`declareLoser(country)`、`showText(label)`、`destroyTrigger(id)`、`playTheme(theme)`、`playSpeech(eva)`、`forceTrigger(id)`、`enableTrigger(id)`、`disableTrigger(id)`、`destroyTag(id)`、`timerText(label)` |
| 数字 | `playMovie`、`timerExtend(seconds)`、`timerShorten`、`timerSet`、`setGlobal(n)`、`clearGlobal`、`ionStormStart(duration)`、`setLocal(n)`、`clearLocal`、`setAmbientLight`、`wakeGroup`、`playRadarMovie`、`pauseGame`、`setSidebarTab`、`chronoScreenEffect` |
| TeamType（`Rules.teamType`） | `createTeam(team)`、`destroyTeams(team)`、`reinforce(team)`、`reinforceAt(team, waypoint)`、`reinforceByChrono(team, waypoint)` |
| 路径点 | `dropZoneFlare`、`revealAround`、`revealZone`、`lightningStrike`、`ionCannonStrike`、`nukeStrike`、`chemStrike`、`reshroudAt`、`ironCurtainAt`、`jumpCamera`；`playAnimAt(at, anim)`、`detonateWeaponAt(at, weaponId: i32)`、`centerCamera(at, speed)`、`meteorShower(at, intensity)`、`createCrate(at, kind)`、`lightningStormStrike(at, house)`、`createBuildingAt(at, kind: TypeRef, house)` |
| 类型 | `setTechLevel(kind, level)`、`flashCameo(kind, frames)` |
| 所属方 | `autoBaseBuilding(house, on)`、`reshroud`、`wakeSleepers`、`wakeHarmless`、`radarBlackout(house, frames)`、`oneTimeSuperWeapon(house, kind)`、`repeatingSuperWeapon(house, kind)`、`superWeaponSetCharge(house, kind, percent)`、`superWeaponReset(house, kind)`、`teleportAll(house, to)`、`setAlternateBase(house, at)`、`clearAlternateBase(house)`、`flashBuildings(house, kind, frames)` |
| 对象 | `changeHouse(target, house)`、`destroyObject`、`sellBuilding`、`turnOffBuilding`、`turnOnBuilding` |

**任意动作。** `Action.runRaw(kind, target, p: usize): i64` 用地址 `p` 处的 `ActionParams`（`params()` 清空 SDK 共用的那份并返回它的地址）执行第 `kind` 号动作，返回它创建的东西；`Action.runText(kind, target, text)` 执行带文字参数的动作。`ActionKind` 里是原子动作的编号（`ActionKind.FireWeapon` = 1000……）；原版动作保持 0..145 的编号。

## 任务与所属方指令

- **任务**对每一方都有效，不管是人类还是电脑，效果和那一方自己点鼠标一样：`Techno.setMission(t, Mission.Hunt)`（没有目标），或 `Action.setMission(t, Mission.Move, Cell.at(60, 40))`。
- **所属方指令**让电脑方一次给很多单位下玩家命令，也可以管生产：`House.order(house, order): Status`。指令在下一帧执行，按那一方的玩家输入校验。人类玩家的所属方返回 `Refused`。

| `Order` 构造函数 | 命令 |
| --- | --- |
| `Order.moveTo(units, cell)`、`Order.moveToObject(units, target)` | 移动 |
| `Order.stop(units)` | 停止 |
| `Order.attack(units, target)`、`Order.attackMove(units, cell)` | 攻击 |
| `Order.guard(units, target)` | 守卫某个对象，写 `NO_HANDLE` 表示原地守卫 |
| `Order.deployMcv(units)` | 展开 |
| `Order.setRally(factories, cell)` | 集结点 |
| `Order.queueProduction(t)`、`Order.cancelProduction(t)` | 生产 |
| `Order.placeReadyBuilding(t, cell)` | 放下造好的建筑 |
| `Order.sell(buildings)` | 卖掉 |
| `Order.launchSuperWeapon(t, cell)` | 超级武器 |
| `Order.takeProduct(kind)` | 取出某类工厂造好的产品（2 步兵，3 载具，4 海军，5 飞机） |
| `new Order(command, target, value, units)` | 任何 `Command`（`Move` … `TakeProduct`；`EjectBunker` 是 `later`） |

`units` 是 `StaticArray<Techno>`。一条指令最多带 64 个单位（`ORDER_UNITS`）；一个模块每帧最多发 256 条指令、共 4,096 个单位。任务不计入这个限制。`Commanded` 和 `OrderRefused` 事件会报告执行了和被拒绝的指令。

## 规则

| 函数 | 作用 |
| --- | --- |
| `Rules.technoType(name): TypeRef` | 一个 TechnoType，名字不存在时是 `NO_HANDLE` |
| `Rules.weapon`、`Rules.warhead`、`Rules.projectile`、`Rules.superWeaponType`、`Rules.country`、`Rules.teamType` | 其他种类，用法相同 |
| `Rules.anim(name): i32` | 动画的 ID（没有时是 `NO_ID`） |
| `Rules.get(section, key): string` | 合并后（mod 和地图）的规则里 `[section] key` 的原文；键不存在时是 `""`，状态为 `NotFound` |
| `Rules.set(section, key, value): Status` | 写 INI 文字。在 `onRules` 里可以写任何键，和其他规则一样处理，每台机器上结果相同。对局进行中返回 `Refused` |

## 触发器

触发器就是“被绑定的对象发生这个事件时，检查这些条件，然后执行这些动作”。`Trigger` 在模块内存里构建一个触发器；`done()` 才真正创建它。

| 构建调用 | 作用 |
| --- | --- |
| `Trigger.on(kind: i32)` | 监听事件 `kind`（一个 `Event`）的可重复触发器，所有难度都生效 |
| `Trigger.every(interval)` | `Timer` 触发器，从创建起每 `interval` 帧触发一次 |
| `Trigger.onProperty(prop, op, value, interval)` | `PropertyPoll`：`prop op value` 变为成立时触发，每 `interval` 帧检查一次 |
| `Trigger.onVariable(v: Var, op, value, interval)` | `VariablePoll` |
| `Trigger.onHousePower(op, value, interval)` | `HousePowerPoll`（触发器所属方的发电量减耗电量） |
| `Trigger.onModifierCount(group, op, count, interval)` | `ModifierCountPoll` |
| `Trigger.onDistance(cell, op, leptons, interval)` | `DistancePoll` |
| `Trigger.onModuleEvent(event)` | 模块事件（[INI 注册](#ini-注册)） |
| `.and(kind)` | 再加一个事件；所有事件都要发生 |
| `.value(v)`、`.id(id)` | 最近一个事件的参数（原版事件的所属方下标或数量、类型 ID） |
| `.where(prop, op, value)`、`.whereOther(...)`、`.whereHouse(...)` | 最近一个事件的筛选条件：针对主体、另一个对象、触发器的所属方 |
| `.whereVar(target: VarTarget, v: Var, op, value)` | 针对 `VarTarget.Global`、`Subject`、`Other` 或 `House` 的变量的条件 |
| `.bind(reference)` | 一个对象、一个 TechnoType（这个类型的所有对象，包括以后造的）、一个武器、弹头或抛射体类型（用于 `Fire`），或一个地格（原版事件 25 和 26） |
| `.house(house)` | 触发器的所属方，代替主体的所属方 |
| `.difficulty(mask)` | 只在这些难度生效：第 0 位简单，1 普通，2 困难 |
| `.once()` | 每个实例只触发一次 |
| `.disabled()` | 创建时是禁用的 |
| `.immediate(action: ActionDef)` | 触发时立即执行一个动作 |
| `.enqueue(fn)` | 在模块阶段把事件交给 `fn` |
| `.enqueueWith(fn, target: Target, text)` | 同上，带一个目标和一段参数文字（`e.record.target`、`e.record.argsText`） |
| `.done(): TriggerRef` | 创建并绑定触发器；没有创建时返回 `NO_HANDLE`（`lastStatus()` 说明原因）。绑定失败时仍然返回触发器，`lastStatus()` 是绑定的状态 |
| `.decideSync(fn): TriggerRef` | 创建成决定触发器，请求同步交给 `fn`（[可决定事件](#可决定事件)）；它不带动作 |

- 一个构建器最多 4 个事件（`MAX_EVENTS`）、每个事件 4 个条件（`MAX_EVENT_CLAUSES`）、8 个动作（`MAX_ACTIONS`）和 8 个绑定（`MAX_BINDS`）。任何一项多一个，`done()` 都会返回 `NO_HANDLE`，状态为 `Refused`；更多的绑定之后用 `TriggerRef.bind` 加。
- 一个模块最多 256 个触发器，其中对象轮询最多 16 个。
- 记录在模块阶段投递，每帧最多 1,024 条。积压超过 16,384 条会让模块被停用。
- 连锁：由触发器自己的动作引起的触发最多嵌套 8 层，每帧最多 256 次；其余的等到下一帧（最多 4,096 个在等）。

**`TriggerRef`**（也是 `Trigger` 的静态函数）：`bind(t, reference)`、`unbind(t, reference)`、`remove(t)`（触发器和它的所有实例）、`setEnabled(t, bool)`、`bindAllTypes(t, kind: NameKind): i32`（绑定这个种类的所有类型，返回绑定的个数）。

**`ActionDef`**（即时动作，给 `.immediate` 用）：

| 函数 | 作用 |
| --- | --- |
| `ActionDef.addMoney(amount, house = NO_HANDLE)` | 给 `house` 钱，不写时给触发器的所属方 |
| `ActionDef.setVar(target: Target, v: Var, value)` | 写目标的变量（红警 2 的变量忽略目标） |
| `ActionDef.heal(target, amount)`、`ActionDef.damage(target, warhead, amount)` | 回血、伤害 |
| `ActionDef.setProperty(target, prop, value)` | 和 `set` 一样 |
| `ActionDef.log(line)` | 一行日志 |
| `ActionDef.raiseEvent(target, event, value, value2)` | 触发模块事件 |
| `ActionDef.module(fn, target = Target.Subject, text = "")` | 给 `fn` 排一条记录，和 `enqueue` 一样 |
| `ActionDef.raw(kind, target)` 以及 `.id`、`.arg`、`.arg2`、`.value`、`.value2`、`.cell`、`.house`、`.other` | 任何动作，带原始的 `ActionParams` |

`Target` 说明动作作用在什么上：`Subject`、`Other`、`TagHolder`、`Projectile`、`House`、`Cell`。

## 事件

`Event` 里是事件种类（`Event.Killed`、`Event.TakeDamage`……），编号和引擎触发它们时一致：原版事件 1..88，模块事件从 1000 开始。处理函数接收 `EventRecord` 或一个视图：

**`EventRecord`**（120 字节）：`key`、`kind`、`trigger`、`holder`（被绑定的对象，或者标签上挂着这个触发器的对象或地格）、`subject`、`other`、`house`、`projectile`、`modifier`、`cell`、`value`、`value2`、`id`、`frame`、`x`、`y`、`z`、`args`（字符串 ID）、`argsText`（它的文字）、`target`（模块动作的 `Target`）。

| 事件 | 视图：字段 | 层级 |
| --- | --- | --- |
| `TakeDamage` | `TakeDamageEvent`：`victim`、`attacker`、`damage`（护甲之后）、`warhead`、`x`、`y`、`z` | v1 |
| `Killed` | `KilledEvent`：`victim`、`killer`、`x`、`y`、`z`；`KilledByWarheadEvent`：`victim`、`killer`、`warhead` | v1 |
| `FatalDamage` | `FatalDamageEvent`：`victim`、`attacker`、`damage`、`warhead`、`x`、`y`、`z` | v1 |
| `AttackedBy` | `AttackedByEvent`：`target`、`attacker`、`weaponSlot`、`projectile` | v1 |
| `Fire` | `FireEvent`：`shooter`、`target`、`targetCell`、`weaponSlot`、`weapon`、`projectile` | v1 |
| `TargetChanged` | `TargetChangedEvent`：`object`、`target`、`targetCell` | v1 |
| `Detonated` | `DetonatedEvent`：`source`、`projectile`、`warhead`、`cell`、`x`、`y`、`z` | v1 |
| `Crushed` | `CrushedEvent`：`victim`、`crusher`、`x`、`y`、`z` | v1 |
| `Birth` | `BirthEvent`：`object`、`producer` | v1 |
| `TransportEntered`、`TransportExited` | `TransportEnteredEvent`、`TransportExitedEvent`：`passenger`、`transport`、`space`、`garrison` | v1 |
| `PassengerAdded`、`PassengerRemoved` | `PassengerAddedEvent`、`PassengerRemovedEvent`：`transport`、`passenger`、`space`、`garrison` | v1 |
| `HouseTransferred` | `HouseTransferredEvent`：`object`、`oldHouse`、`mindControl` | v1 |
| `ModifierStart`、`ModifierExpire` | `ModifierEvent`：`target`、`modifier`、`property`、`source` | v1 |
| `ModifierStacks` | `ModifierStacksEvent`：`target`、`modifier`、`count` | v1 |
| `Timer` | `TimerEvent`：`lastFrame` | v1 |
| `SuperWeaponLaunched` | `SuperWeaponLaunchedEvent`：`superWeapon`、`house`、`targetCell` | v1 |
| `Deployed` | `DeployedEvent`：`object`、`became`、`deployed` | v1 |
| `Promoted` | `PromotedEvent`：`object`、`rank`（1 老兵，2 精英） | v1 |
| `ExperienceGained` | `ExperienceGainedEvent`：`object`、`victim`、`gain` | v1 |
| `Cloaked`、`Decloaked` | `CloakEvent`：`object` | v1 |
| `Commanded` | `EventRecord`：`value` 是 `Command`，`other` 和 `cell` 是目标，`value2` 是每条指令的序号 | v1 |
| `OrderRefused` | `EventRecord`：`subject`（整方指令是 0）、`house`、`value` 是 `Command`、`value2` 是原因（附录 8.3 `RefuseReason`） | v1 |
| `ModuleEvent` | `ModuleEventRecord`（[INI 注册](#ini-注册)） | v1 |
| `PropertyPoll`、`HousePowerPoll`、`ModifierCountPoll`、`DistancePoll`、`VariablePoll` | `PollEvent`：`subject`、`reading` | v1 |
| 原版事件 1..61（`EnteredBy`、`DestroyedByAnyHouse`、`CreditsExceed`、`ElapsedTime`、`ComesNearWaypoint`……） | `StockEvent`：`subject`、`other`、`house`、`boundCell` | v1 |
| Ares 事件 62..88 | `StockEvent` | 只在开启 Ares 时 |
| `MissionChanged` | `MissionChangedEvent`：`object`、`mission` | later：不会触发 |
| `HitByWarhead`、`SpawnFire`、`HitTarget`、`ProjectileIncoming`、`HouseTransfer`、`ProductionCancelled` | — | later：不会触发 |

`IniEntry` 和 `IniKey` 记录不经过触发器投递（[INI 注册](#ini-注册)）。

## 可决定事件

决定触发器（`.decideSync(fn)`，或 INI 里的 `Decide=Wasm:Name`）在游戏做出判断的那一刻同步询问模块，模块可以改写结果。

```ts
import { Event, NO_HANDLE, Rules, TakeDamageDecision, Trigger, onScenarioStart } from "@vera20k/sdk";

function halve(d: TakeDamageDecision): void {
  d.setResult(d.previous / 2);
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi != NO_HANDLE) Trigger.on(Event.TakeDamage).bind(gi).decideSync(halve);
});
```

- 决定函数只能读（`get`、`Find`、读变量、`random`、`log`……）。任何写操作都会让模块被停用：`set`、修饰器、写变量、任何 `Action.*`（包括 `raiseEvent` 和 `Action.log`）、创建、绑定或移除触发器、`Rules.set`、通知或所属方指令。要根据决定去做事，先记在模块内存里，到下一次 `onFrame` 再做。决定函数也不应该分配内存。
- 每个决定视图都有 `native`（游戏自己的答案）、`previous`（前一个模块的答案，第一个模块拿到的是游戏的答案）和 `setResult(value)`。不调用 `setResult` 就保持之前的答案。`request` 给出完整的 `DecideRecord`：`event`、`native`、`previous`、`result`、`setResult`、`handled`、`area` 和 `areaCap`（结果区）、`previousLen`。
- 预算：每个请求 20,000 次运算，从一帧预算一半大小的池子里扣。
- 一个决定可以由多个模块回答：先 mod 的模块，再地图的模块。

| 事件 | 视图：字段 | 结果 | 层级 |
| --- | --- | --- | --- |
| `TakeDamage` | `TakeDamageDecision`：`victim`、`attacker`、`damage`、`warhead`、`x`、`y`、`z` | 最终伤害（0 挡下这次攻击） | v1 |
| `ProductionAllowed`（绑定类型） | `ProductionAllowedDecision`：`house`、`technoType` | 1 允许，0 在下一帧之前不允许；1 不能解除游戏自己的拒绝 | preview |
| `ProductionQueued` | `ProductionQueuedDecision`：`house`、`technoType` | 1 允许，0 拒绝 | v1 |
| `Defeat` | `DefeatDecision`：`house` | 1 判负，0 否决这次检查 | v1 |
| `CommandAllowed`（绑定对象或类型） | `CommandAllowedDecision`：`unit`、`command`、`target`、`targetCell` | 1 允许，0 拒绝（模块的指令也算） | preview |
| `DamageIncoming` | `DamageIncomingDecision`：`victim`、`attacker`、`warhead`、`incoming`、`preview`、`x`、`y`、`z` | 进入的伤害；0 或与 `previous` 同号 | preview |
| `FireAllowed` | `FireAllowedDecision`：`shooter`、`target`、`targetCell`、`weaponSlot`、`weapon` | 0 否决这次开火 | preview |
| `Retaliate` | `RetaliateDecision`：`victim`、`source`、`warhead` | 1 还击，0 不还击 | preview |
| `DestinationSet` | `DestinationSetDecision`：`object`、`destination`、`destinationCell` | 目的地引用，0 表示没有 | preview |
| `CrushAllowed` | `CrushAllowedDecision`：`victim`、`crusher` | 0 放过受害者 | preview |
| `HarvestCell` | `HarvestCellDecision`：`harvester`、`scanned` | 地格引用，0 表示没有 | preview |
| `DeployAllowed` | `DeployAllowedDecision`：`object` | 0 拒绝部署 | preview |
| `IdleMission` | `IdleMissionDecision`：`unit` | `Mission` 编号，255 表示没有 | preview |
| `MissionStep` | `MissionStepDecision`：`unit`、`mission`、`target`；`step`（目前的结果，一个 `MissionStepResult`）、`setStep`、`setTarget`、`setDestination`、`setMission`、`setNoMission`、`setDelay` | Guard、Area Guard、Hunt 或 Attack 任务体的结果，替换其中一部分 | preview |
| `MoveFrame` | `MoveFrameDecision`：`unit`、`moving`、`destination`、`destinationCell`、`x`、`y`、`z`；`previousStep`、`newStep()`、`setStep(step)`、`stepBy(dx, dy)`；标志 `MOVE`、`FACING`、`STOP` | 代替运动方式，每帧把地面单位移动最多 2,048 lepton | preview |
| `PathFound` | `PathFoundDecision`：`unit`、`goal`、`x`、`y`、`z`；`previousCount`、`previousCell(i)`、`cell(i)`、`setCellCount(n)`、`setCells(xy)`、`noRoute()`；`MAX_CELLS` 24 | 替换路线，每格都要和前一格相邻 | preview |
| `ApproachPoint` | `ApproachPointDecision`：`unit`、`chosen`、`target`、`targetCell`、`x`、`y`、`z`；`setCell(cell)` | 追击目标的单位要开去的地格 | preview |
| `Fire`（决定形式） | `FireDecision`：`shooter`、`target`、`targetCell`、`slot`、`weapon`；`previousCount`、`previousEntry(i)`、`planCapacity`、`planEntry(i)`、`setPlanLength(n)`、`setPlan(plan)` | 射击计划：最多 16 个 `ProjectileSpec`，代替原本的抛射体发射 | preview |
| `Launch`（绑定武器、弹头或抛射体类型） | `LaunchDecision`：`projectile`、`launcher`、`target`、`targetCell`；`setPath(kind, speed, param0, param1, points)`、`setPathTo(kind, speed, param0, param1, x, y, z)` | 抛射体的路径（`PathKind.Bezier`、`Parabola`、`Spiral`、`Waypoints`） | preview |
| `ProjectileFrame`（绑定抛射体类型） | `ProjectileFrameDecision`：`projectile`、`frames`；`previousStep`、`newStep()`、`setStep(step)` | 抛射体每帧的一步 | preview |
| `FatalDamage`（决定形式） | `FatalDamageDecision`：`victim`、`killer`、`damage`、`warhead`、`x`、`y`、`z`；`with(bit, on)`；静态的 `withInfDeath(plan, n)`、`withDeathWeapon(plan, weapon)`；标志位 `EXPLODE`、`DEATH_WEAPON`、`DEBRIS`、`SURVIVORS`、`CREW`、`ANNOUNCE`、`TIBERIUM_REMAINS` | 死亡后留下什么 | preview |

**结果结构。** 带结果区的视图就地读写这些结构，不分配内存。每个结构都有 `static at(ptr)`（模块内存上的视图）、`BYTES` 和各字段的读写器；大多数还有 `clear()` 和 `copyFrom(other)`：

| 结构 | 字段 |
| --- | --- |
| `ProjectileSpec`（48 字节） | `projectileType`、`weapon`（−1 是这次射击自己的）、`target`（0 是这次射击的）、`data0`、`data1`、`flags`（`ORIGIN_ABSOLUTE`）、`originX`、`originY`、`originZ` |
| `ProjectileStep`（56 字节） | `flags`（`DETONATE`、`SET_VELOCITY`、`SET_POSITION`、`SET_DATA`）、`velocityX..Z`（每帧 lepton，16 位小数）、`positionX..Z`、`data0`、`data1` |
| `MissionStepResult`（24 字节） | `target`、`destination`、`mission`（−1 表示没有）、`delay` |
| `MoveStep`（16 字节） | `flags`、`facing`、`dx`、`dy` |
| `PathCell`（8 字节） | `x`、`y`、`cell` |

要一个自己的结构（用来保存或复制），先预留一次内存：`ProjectileStep.at(memory.data(ProjectileStep.BYTES, 8))`。`PATH_HEADER_BYTES` 和 `PATH_POINT_BYTES` 是 `Launch` 路径布局的大小。

## INI 注册

模块可以给 modder 提供自己的 INI 名字：类型段里的键、注册表（列表段）和模块事件。modder 写 INI，模块负责实现。只能在顶层代码里注册；写在别处会让模块被停用。

| 函数 | INI | 模块收到 |
| --- | --- | --- |
| `Ini.key(name, section: IniSection, fn): Status` | 这一类段里的 `name=value`（`IniSection.TechnoType`、`Weapon`、`Warhead`、`Projectile`、`SuperWeapon`、`Country`、`General`） | 每个写了这个键的段一条 `IniKeyRecord`：`typeRef`、`isGeneral`、`value`、`sectionName`、`record` |
| `Ini.registry(name, fn): Status` | 列出条目段的列表段 `[name]` | 每个条目一条 `IniEntryRecord`，按列表顺序：`entryName`、`position`、`record` |
| `Ini.event(name, scope: EventScope): ModuleEvent` | 触发器段里的 `Event=Wasm:name` | 事件的 ID（被拒绝时是 `NO_ID`），给 `Trigger.onModuleEvent` 和 `Action.raiseEvent` 用。`EventScope.Object` 触发绑定到这个对象（或它的类型）的触发器，`EventScope.Global` 触发不绑定的触发器 |

- 名字是 1 到 63 个 ASCII 字母、数字、`.` 和 `_`，比较时不区分大小写。已经被（任何来源）注册的名字，或以 `Vera.` 开头的名字，返回 `Refused`。每个模块最多 4,096 个注册。
- 记录在 `onScenarioStart` 之前送达：先是所有 `IniEntry` 记录，再是所有 `IniKey` 记录。它们算在 `onScenarioStart` 的预算里。读档时不会再送一遍。
- 值是合并后的规则原文，引擎不检查。用 `Rules.get` 读条目段，用下面的函数解析。值不对时用 `fail(...)` 停下，并写明段和键。
- `ModuleEvent.named(name)` 找到任何来源（包括别的模块）注册的事件，没有时是 `NO_ID`。触发的事件以 `ModuleEventRecord` 送达：`event`、`subject`、`other`、`value`、`value2`、`house`、`record`。

解析（格式不对时返回 `otherwise`，`lastStatus()` 为 `NotFound`）：

| 函数 | 作用 |
| --- | --- |
| `Ini.split(text): string[]` | 按逗号分开的各部分，各自去掉两边的空格 |
| `Ini.stripComment(text): string` | 去掉 `;` 注释和两边空格的 `text` |
| `Ini.int(text, otherwise = 0): i64` | 可带正负号的十进制整数 |
| `Ini.yesNo(text, otherwise = false): bool` | 首字母 `y`、`t` 或 `1` 为真；`n`、`f` 或 `0` 为假 |
| `Ini.ints(text, out: StaticArray<i64>, otherwise = 0): i32` | 把前 `out.length` 个整数读进 `out`；返回一个掩码，第 `i` 部分是合法整数时第 `i` 位为 1 |

INI 那一侧见[模块指南](guide.zh-CN.md#模块注册的-ini-名称)，完整的 mod 见 `sdk/typescript/examples/ini-counter`。

## 通知与消息

同一来源（mod 或地图）的模拟模块和客户端模块通过游戏通信，所以每台机器都保持同步：

- **客户端到模拟**：客户端模块的 `Client.sendMessage(bytes)` 经过命令延迟后，在每台机器上到达模拟模块的 `onMessage` 处理函数，并带上发送者的所属方，客户端伪造不了它。字节不可信：每一个都要检查。
- **模拟到客户端**：`notify.send(house, data)` 通知那一方的玩家；`notify.sendObservers(data)` 通知观战者。那里的每个客户端模块都会收到一个 `Notify` 事件。
- **共享状态**：客户端模块用 `ClientVar` 读的声明变量。它会存档，所以读档后重新开始的客户端模块马上就能看到它。

| 名字 | 作用 |
| --- | --- |
| `new Notify(name)` | 模块在 `vera.json` 里声明的通知名（构建会在 `src/generated/notify.ts` 里生成它们） |
| `notify.id: i32` | 它的 ID，即名字的 FNV-1a 哈希，每台机器上和客户端世界里都一样；没声明的名字是 `NO_ID` |
| `notify.send(to: House, data = empty): Status` | 通知 `to` 的玩家；超过大小或每帧限制时返回 `Refused` |
| `notify.sendObservers(data = empty): Status` | 通知所有观战者 |
| `sendNotify(to, id, data): Status` | 原始调用；`to` 为 0 表示观战者 |
| `MAX_MESSAGE_BYTES` | 96：`onMessage` 收到的消息最大长度；每个玩家每个来源每帧 2 条、15 帧内 16 条 |
| `MAX_NOTIFY_BYTES` | 256：通知的最大长度；每个模块每帧 256 条 |

---

# 客户端模块

客户端模块（`ui.wasm`）导入 `vera:client@1.0`，只在本机运行，每个画面帧一次。它读取这个玩家的界面可以显示的内容，改界面、镜头和光标，播放本地效果；它只能通过发给模拟模块的消息和标准玩家指令影响游戏。它可以用浮点数。

只从 `@vera20k/sdk/client` 导入。`@vera20k/sdk` 的函数（`get`、`log`、`Var`、`Action`……）调用 `vera:sim` 的导入，引擎会拒绝带有它们的客户端模块；`vera20k build` 会先拒绝，并说出是哪些。`@vera20k/sdk/client` 重新导出两边共用的东西：`Status`、`lastStatus`、`NO_HANDLE`、`NO_ID`、引用类型（`Ref`、`Techno`、`House`、`Cell`、`TypeRef`、`Waypoint`）、`Prop`、`NameKind`、`Op`、`Category`、`Command`、`Order`、`ORDER_UNITS`、`Find`、`FindPage`、`MAX_CLAUSES`、`params`、`EventRecord` 和 `Event`。共用的命名空间里，只有不调用宿主的函数在那里能用：`Cell.at`、`Cell.x`、`Cell.y`、`TypeRef.make`、`TypeRef.id`、`TypeRef.kind`、`Waypoint.number`，以及 `Find` 和 `Order` 的构建器。读属性请用 `Client.get(t, Prop.Health)`，不要用 `Techno.health(t)`。

```ts
import { ClickEvent, Client, ClientEvent, NO_HANDLE, Node, Ui, onClientEvent, onStart } from "@vera20k/sdk/client";

const BUY = new Uint8Array(1);
BUY[0] = 1;
let button: Node = NO_HANDLE;

onStart((): void => {
  button = Ui.find("BuyReinforcements");
  if (button != NO_HANDLE) Node.setClickable(button, true);
});

onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
  if (e.node == button) Client.sendMessage(BUY);
});
```

## 客户端回调

| 函数 | 导出 | 何时 |
| --- | --- | --- |
| 顶层代码 | `client_init` | 加载后运行一次。那里只有 `Client.lookup`、`nameOf`、`hasKind` 和 `log` 能用 |
| `onStart(fn: () => void)` | `client_on_start` | 对局界面建好后运行一次；重新加入的玩家追上进度后、读档后会再运行 |
| `onClientEvent<T>(kind: ClientEvent, fn: (e: T) => void)` | `client_on_events` | 每个画面帧、`onRender` 之前，`kind` 的每个事件调用一次，按投递顺序。`T` 是这种事件的视图或 `ClientEventRecord`。同一种事件的多个处理函数按登记顺序运行 |
| `onRender(fn: (dt: f32, alpha: f32) => void)` | `client_on_render` | 每个画面帧，在这一帧的事件之后。两个参数都只用于显示 |

存档不包含客户端模块：读档后它们从头开始（先顶层代码，再 `onStart`），存储保持原样。追赶进度时（重新加入、观战者追赶）它们不运行。

## 客户端调用

| 函数 | 作用 |
| --- | --- |
| `Client.frame(): i32` | 最近一个已完成的模拟帧（在 `client_init` 里是 0） |
| `Client.random(lo, hi): i32` | 只用于显示的本地随机数；`lo > hi` 会让模块被停用 |
| `Client.log(line)` | 在聊天栏（每个画面帧 8 行）和日志文件里写一行 |
| `Client.fail(why)` | 记下 `why` 并停用这个客户端模块，只在这台机器上 |
| `Client.hasKind(table, id): bool` | 表和模拟端一样，另有 5 节点种类，6 客户端事件种类，7 客户端动作种类 |
| `Client.lookup(kind, name): i32`、`Client.nameOf(kind, id): string`、`Client.text(id): string` | 和模拟端一样。变量名和通知名是这一局的模拟模块声明的；热键名是本模块的 |

## 读取游戏状态

| 函数 | 作用 |
| --- | --- |
| `Client.get(reference, prop): i64` | 这个玩家的界面可以显示的属性值：看不见的对象是 `Gone`，隐藏的属性是 `Refused`，两者都返回 0 |
| `Client.getText(reference, prop): string` | 字符串属性 |
| `Client.getMany(refs, props, out): i32` | 和 `getMany` 一样，每一项都按 `get` 的可见性 |
| `Client.findPage(query: Find, out, after): FindPage` | 只限这个玩家看得见的内容的一页 `Find` 结果；页对象是共用的 |
| `Client.findFirst(query: Find): i64` | 这个玩家看得见的第一个匹配项，或 `NO_HANDLE` |
| `new ClientVar(name)` | 模拟模块声明的变量，第一次使用时查找：`id`、`name`、`get(scope = NO_HANDLE)`，全局变量写 `NO_HANDLE`，否则是所属方（private 的只能读这个玩家自己的）或对象；失败时返回 0 |
| `Client.varGet(scope, id): i64` | 原始读取；全局变量的 scope 是 `NO_HANDLE` |

盟友的对象只显示 `seen` 属性；间谍按屏幕上的伪装显示；观战者按房间设置读取。

## 节点

对局界面是一棵节点树：`veraui.ini` 布局的部件、地图 `[VeraUI]` 的部件，以及模块创建的节点。`Node` 是一个引用（`i64`）；要在回调之间保存，就放在顶层的 `let` 里。

| 函数 | 作用 |
| --- | --- |
| `Ui.find(name): Node` | 按名字找布局部件（不带布局前缀：`[Mine.Buy]` 就写 `Buy`），或按 `Name` 找创建的节点。先找本模块的来源，再找引擎、mod、地图；没有时是 `NO_HANDLE`（`NotFound`） |
| `Ui.create(kind: NodeKind, parent: Node): Node` | 新节点；父节点写 `NO_HANDLE` 表示顶层（地图模块是覆盖层） |
| `Ui.createWith(kind, parent, props: NodeProps): Node` | 在一次调用里带上最多 16 个初始属性 |
| `Ui.createNamed(kind, parent, name): Node` | 创建并命名；名字被拒绝时会再删掉 |
| `Ui.remove(node): Status`、`Node.remove(node)` | 删除它和它的子节点 |
| `new NodeProps()` | 初始属性：`.with(prop, value)`、`.at(x, y)`、`.size(width, height)`；最多 `NodeProps.MAX`（16）个，更多时创建返回 `Refused` |

`NodeKind`：`Text`、`Button`、`Row`、`Column`、`List`、`Image`、`Bar`（模块能创建这些）；`CameoGrid`（只有布局能做；模块可以找到它并写它的通用属性）。

`Node` 的函数（每个是一次 `Client.set`、`Client.setText` 或 `Client.get`）：

| 函数 | 作用 |
| --- | --- |
| `Node.isNode(n): bool` | `n` 是否是节点引用；不调用宿主 |
| `Node.get(n, prop)`、`Node.set(n, prop, value)` | 任何属性 |
| `Node.setName(n, name)` | 给本模块创建的节点起名字，供 `Ui.find` 使用：每个节点只能起一次，在本来源的节点里不能重名 |
| `Node.setLabel(n, text)` | CSF 条目名或直接的文字（`Text`、`Button`） |
| `Node.setArt(n, name)` | 按文件名指定图片，如 `mybutton.shp` 或 `panel.pcx`（`Button`、`Image`） |
| `Node.visible(n)`、`Node.setVisible(n, bool)` | 是否显示 |
| `Node.setEnabled(n, bool)` | `Button`、`List`：为 false 时画成禁用状态，也不发 `Click` |
| `Node.setClickable`、`Node.setHoverable`、`Node.setDismissable` | 节点发哪些事件（`Click`、`Hover`、`Dismiss`） |
| `Node.setPosition(n, x, y)`、`Node.setSize(n, width, height)` | 像素：在侧边栏里是侧边栏像素，在别处是屏幕像素 |
| `Node.setFill(n, percent)` | `Bar` 的填充，0..100 |
| `Node.setBackdrop(n, bool)` | 战场上的 `Text` 是否垫黑底（默认垫） |
| `Node.setItems(n, items: string[])` | `List` 的各行，总共 1 KiB |

其他属性通过 `Node.set`、`Client.setText` 或 `Client.setBytes` 写：`Prop.Hold`（0 左上，1 上，2 右上，3 左，4 正中，5 右，6 左下，7 下，8 右下）、`Prop.Gap`（`Row`、`Column`）、`Prop.ItemTooltips`（`CameoGrid`）。`ItemProgress`、`Decoration` 和 `MapMarker` 是 `later`。

- **谁能写什么。** mod 的模块可以读、写、删除任何节点。地图的模块在 mod 的 `[VeraWasm] MapUI=overlay`（默认）下，能找到并读取任何节点，但只能写地图自己的；`MapUI=full` 下和 mod 的权限一样；`MapUI=no` 下不加载。模块无权做的写操作返回 `Refused`。
- **数量。** 每个模块最多占界面允许其他来源节点数的一半；超出时创建返回 `Refused`。后创建的节点画在先创建的上面。

| 函数 | 作用 |
| --- | --- |
| `Client.set(reference, prop, value): Status` | 写客户端拥有的引用的属性：节点、镜头、光标 |
| `Client.setText(reference, prop, value: string): Status` | 写字符串属性（`Label`、`Art`、`Items`……） |
| `Client.setBytes(reference, prop, data: Uint8Array): Status` | 写字节属性 |

## 玩家、镜头与光标

| 函数 | 作用 |
| --- | --- |
| `Client.viewer(): House` | 这个玩家的所属方，观战者是 `NO_HANDLE` |
| `Client.isSpectator(): bool` | |
| `Client.sendMessage(data: Uint8Array): Status` | 发给本来源模拟模块的 `onMessage`（最多 96 字节）；观战者和超出限制时返回 `Refused` |
| `Client.order(order: Order): Status` | 以这个玩家的名义下一条标准玩家指令，按玩家输入校验。每个单位每 15 帧一次，整方指令每 15 帧 4 次。房间不允许自动化（`AutomationAllowed`）时返回 `Refused`；大厅还没有这个开关，所以目前总是被拒绝 |
| `Camera.REF` | 镜头的引用 |
| `Camera.cell(): Cell` | 镜头中心所在的地格 |
| `Camera.jumpTo(cell)`、`Camera.centerOn(cell, speed)` | 移动本地镜头 |
| `Cursor.REF` | 光标的引用 |
| `Cursor.x()`、`Cursor.y()` | 屏幕像素，和节点的 `X`、`Y` 一样计算 |
| `Cursor.cell(): Cell` | 光标下的地格，在地图外是 `NO_HANDLE` |
| `Cursor.captureMapClicks(capture: bool)` | 为 true 时地图点击只发给本模块（`MapClick`），不交给游戏 |

## 本地动作

只在这台机器上的效果（`Local`，附录 5.4）：

| 函数 | 作用 |
| --- | --- |
| `Local.playSound(id, cell)` | 在某个地格播放声音 `id`（它在 `[SoundList]` 里的编号），写 `NO_HANDLE` 表示不定位。这个版本的 `Client.lookup(NameKind.Sound, ..)` 返回 `Unsupported` |
| `Local.playSpeech(name)` | EVA 语音，如 `"EVA_ReinforcementsHaveArrived"` |
| `Local.select(units: StaticArray<Techno>, append: bool)` | 选中这个玩家可以选的单位（最多 64 个） |
| `Local.flash(object, frames)` | 让对象闪烁 |
| `Local.showMessage(text, color)` | 显示一行：CSF 条目名或直接的文字 |
| `Local.registerHotkey(name, chord)` | 注册热键和它的默认组合键，如 `"Ctrl+Alt+K"`。游戏热键已占用这个组合键时返回 `Refused`（热键仍然存在，只是没有按键）。每个模块最多 256 个；不能在顶层代码里调用 |
| `Hotkey.id(name): i32` | `Hotkey` 事件为 `name` 带的 ID，或 `NO_ID` |
| `Local.homeCamera()` | `later`：目前返回 `Unsupported` |
| `Local.run(kind, target, p: usize)` | 用地址 `p` 处的 `ActionParams`（`params()`）执行任何客户端动作（`ClientActionKind`） |

## 客户端事件

`ClientEvent` 的种类，投递给 `onClientEvent` 的处理函数（每个画面帧最多 1,024 个）：

| 种类 | 视图 | 何时 | 读取 |
| --- | --- | --- | --- |
| `Click` | `ClickEvent` | 可点击的节点被点了（这次点击不会传到地图） | `node`、`itemIndex`（`List` 的行或 `CameoGrid` 的项，其他节点是 −1） |
| `Hover` | `HoverEvent` | 指针进入或离开可悬停的节点 | `node`、`entered` |
| `Hotkey` | `HotkeyEvent` | 按下了注册的热键 | `id`（和 `Hotkey.id(name)` 比较） |
| `Dismiss` | `DismissEvent` | 在显示中的可关闭节点外按下，或按了 Esc | `node` |
| `MapClick` | `MapClickEvent` | 不是 `Click` 也不是 `Dismiss` 的地图点击 | `cell`、`right` |
| `SelectionChanged` | `SelectionChangedEvent` | 本地选择变了；用 `Client.findPage` 加 `Selected = 1` 条件读取 | — |
| `Notify` | `NotifyEvent` | 模拟模块发给这台机器的通知 | `id`、`data`（副本）、`dataAt(i)`（就地读）、`fromMap`、`source`；用 `NotifyName.is(e)` 判断 |
| `SimEvent` | `ForwardedEvent` | 这个玩家的对象或所属方的 `Commanded`、`OrderRefused`、`TakeDamage`、`Killed`、`Birth` 或 `AttackedBy` | `simEvent`：`EventRecord`，或 `null`（这个玩家看不见时 `other` 是 0） |

每个视图都继承自 `ClientEventRecord`（32 字节）：`kind`、`source`（0 mod，1 地图）、`ref`、`id`、`value`、`dataPtr`、`dataLen`，以及上面那些有名字的读取器。

`new NotifyName(name)` 是这一局某个模拟模块声明的通知名，第一次使用时查找：`id`（没有模块声明时是 `NO_ID`）和 `is(event): bool`。`ClientEvent`、`NodeKind` 和 `ClientActionKind` 是生成的枚举。

## 存储

每个来源（mod 按名字；地图按 `[VeraWasm] StorageId=` 或文件名）在这台机器上有一个小的键值存储，在对局之间保留。

| 函数 | 作用 |
| --- | --- |
| `Storage.read(key): Uint8Array` | 值；没有时为空，状态为 `NotFound` |
| `Storage.readString(key): string` | 把值当作 UTF-8 文字，没有时是 `""` |
| `Storage.len(key): i32` | 值的长度，没有时是 −1 |
| `Storage.has(key): bool` | 这个键是否有值 |
| `Storage.write(key, data: Uint8Array): Status` | 写入；空数据返回 `Refused` |
| `Storage.writeString(key, value): Status` | 写入 UTF-8 文字；`""` 返回 `Refused` |
| `Storage.remove(key): Status` | 删除 |

键是 1 到 64 个 ASCII 字节；值最多 16 KiB；每个存储最多 256 个键、共 256 KiB。超出限制时写入返回 `Refused`，什么也不改。存储最多每秒写一次磁盘。它只在这台机器上：绝不能让它影响模拟的行为。
