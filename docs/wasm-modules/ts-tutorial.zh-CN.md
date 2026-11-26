# Wasm 模块 Mod 开发教程（TypeScript 版）

[English](ts-tutorial.md) | [简体中文](ts-tutorial.zh-CN.md)

这份教程带你从零开始，用 TypeScript 给 VERA20K 写一个 Wasm 模块，并放进游戏里试玩。读完后你会做出一个“击杀悬赏”mod：谁打死美国大兵或动员兵，谁就拿 200 块赏金。

- 写给谁：会改 `rulesmd.ini`、想做 INI 做不到的玩法的 modder。写过一点 JavaScript 或 TypeScript 就够了。
- 参考资料：模块放在哪、有哪些限制见[模块指南](guide.zh-CN.md)；SDK 的每个函数见 [TypeScript SDK 参考](ts-sdk.zh-CN.md)；完整设计见 [RFC 0001](../rfcs/0001-wasm-extension-modules.zh-CN.md)；每个函数、事件、动作、属性的编号与含义见[附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)。
- 想做界面（按钮、面板）？先读完这份，再看[客户端模块教程](ts-ui-tutorial.zh-CN.md)。
- 更习惯 Rust？[Rust 版教程](rust-tutorial.zh-CN.md)做的是同一个 mod。
- 预览版提醒：接口还在 `0.x` 预览阶段，名字和编号在正式版前可能会变。游戏会在模块需要更新的接口时直接告诉你。

## 目录

1. [模块是什么](#1-模块是什么)
2. [准备环境](#2-准备环境)
3. [第一个模块：在聊天栏说一句话](#3-第一个模块在聊天栏说一句话)
4. [放进游戏试玩](#4-放进游戏试玩)
5. [做一个真正的玩法：击杀悬赏](#5-做一个真正的玩法击杀悬赏)
6. [这里的 TypeScript 和平常有什么不同](#6-这里的-typescript-和平常有什么不同)
7. [常用功能速查](#7-常用功能速查)
8. [不写代码：用 INI 声明触发器](#8-不写代码用-ini-声明触发器)
9. [调试与排错](#9-调试与排错)
10. [规则与限制](#10-规则与限制)

## 1. 模块是什么

模块是一个编译好的 WebAssembly 文件。这份教程做的是**模拟模块**，游戏里叫 `rules.wasm`：它和对局一起运行，每个玩家的机器上都跑同一份，所以联机也不会不同步。（另一种是**客户端模块** `ui.wasm`，只在你自己的机器上运行，负责界面，见[客户端模块教程](ts-ui-tutorial.zh-CN.md)。）

TypeScript 源码不会进游戏。SDK 先用 [AssemblyScript](https://www.assemblyscript.org) 把它编译成 WebAssembly，游戏只加载编译结果，和 Rust 写的模块没有区别。

模块能做的事：

- **读游戏状态**：单位的血量、位置、所属方，所属方的钱和电力，地格的地形，规则里的数值……
- **改游戏**：只能通过游戏自己的规则来改，比如执行动作（伤害、造单位、给钱、发射超级武器）、写属性、加修饰器（攻击力、护甲、速度倍率）、给任何一方的单位下任务（包括玩家的）、给电脑方下指令，以及原版触发器动作。
- **响应事件**：用触发器监听受伤、被击杀、开火、出生等事件，可以在代码里创建，也可以在 INI 里声明。
- **改写判断结果**：比如受到的伤害、能不能造某个单位、要不要接受某条命令（“可决定事件”）。
- **加载时改规则**：在规则读入时修改一次（`onRules`）。

模块不能做的事：

- 读写文件、访问网络、读系统时间。它运行在沙箱里，碰不到你的电脑。也没有浏览器或 Node.js 的 API：没有 `console`、`fetch`、`setTimeout`、`Date`。
- 让游戏崩溃。模块出错、死循环或违反接口时，所有机器会在同一时刻停用它，对局继续进行，聊天栏会提示是哪个模块、为什么停了。

模块有两个来源，可以同时存在。mod 的模块先运行，地图的模块后运行。

| 来源 | 放在哪 | 适合 |
| --- | --- | --- |
| mod | mod 文件夹里的 `rules.wasm` | 整个 mod 通用的玩法 |
| 地图 | 嵌在地图文件里（用 `asset wasm-embed`） | 某张地图专属的任务、关卡逻辑 |

## 2. 准备环境

### 2.1 安装 Node.js

到 <https://nodejs.org> 安装 Node.js 20 或更新的版本（LTS 版即可），装好后命令行里能运行 `node` 和 `npm`。编辑器推荐 VS Code，它能直接识别 SDK 带的类型，写代码时有补全和报错提示。

### 2.2 获取 SDK

SDK 在 VERA20K 源码的 `sdk/typescript` 里。`@vera20k/sdk` 包发布到 npm 之前，先从源码使用它：

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k/sdk/typescript
npm install          # SDK 自己用的编译器 AssemblyScript 0.28.20
cd ../..
```

会用到：

| 路径 | 内容 |
| --- | --- |
| `sdk/typescript` | TypeScript SDK 和它的 `vera20k` 命令 |
| `sdk/typescript/examples/` | 示例模块；本教程的完整代码是 `tutorial-bounty` |

### 2.3 `asset` 工具（可选）

`asset` 工具用来对模块跑一遍游戏的加载检查，以及把模块嵌进地图。只有做这两件事时才需要它；最常见的错误 `vera20k build` 自己就会拦下。它随 VERA20K 源码提供，需要 Rust 编译一次（<https://rustup.rs>）：

```sh
cargo build --release --bin asset
# 编译好的程序在 target/release/asset（Windows 上是 asset.exe）
```

## 3. 第一个模块：在聊天栏说一句话

### 3.1 建项目

在 VERA20K 文件夹里用 `vera20k init` 新建项目并安装：

```sh
node sdk/typescript/bin/vera20k.js init my-bounty
cd my-bounty
npm install
```

项目可以放在任何地方；`init` 会让它依赖你这份源码里的 SDK。`@vera20k/sdk` 发布以后，用 `npx -p @vera20k/sdk vera20k init my-bounty` 就能做同样的事，不需要源码。

项目结构：

```text
my-bounty/
├── package.json     项目名、依赖，以及 build / watch / check 命令
├── tsconfig.json    SDK 提供的类型检查设置，不要删
├── vera.json        构建什么：种类、入口文件、变量、通知
├── .gitignore
└── src/index.ts     模块代码
```

`vera.json` 告诉 `vera20k build` 要做什么：

```json
{
  "kind": "rules",
  "entry": "src/index.ts",
  "vars": [],
  "notify": []
}
```

`kind` 是 `rules` 表示模拟模块（客户端模块是 `ui`）；`entry` 是要编译的文件。`vars` 和 `notify` 声明模块的变量和通知名，[第 7 节](#变量)会用到。

### 3.2 写代码

`src/index.ts` 里已经是最简单的模块：

```ts
import { log, onScenarioStart } from "@vera20k/sdk";

// 对局开始时运行一次。
onScenarioStart((): void => {
  log("Hello from my first module!");
});
```

`log` 写的内容会出现在游戏聊天栏，同时写进游戏日志文件。

注意箭头函数写了返回类型 `: void`。这里的 TypeScript 由 AssemblyScript 编译，它要求把类型写明；和平常写 TypeScript 的不同之处集中在[第 6 节](#6-这里的-typescript-和平常有什么不同)。

### 3.3 编译和检查

```sh
npm run build        # 生成 build/rules.wasm
npm run check        # 用 tsc 检查 src/ 的类型
```

编译结果是 `build/rules.wasm`，名字已经是游戏要的。构建会打印文件大小；代码编译不过，或者调用了游戏会拒绝的东西时，构建会失败并说明原因。`npm run check` 先根据 `vera.json` 写出 `src/generated/`（`vera20k generate`），再运行 TypeScript 的类型检查，它的报错往往比编译器的更好懂。不用先构建。

开发时可以用 `npm run watch`：每次保存 `src/` 里的文件或 `vera.json`，就自动重新编译。

有 `asset` 工具的话，还可以跑一遍游戏自己的加载检查：

```sh
asset wasm-check build/rules.wasm
# 通过时输出：{"bytes":…,"ok":true}
```

## 4. 放进游戏试玩

### 4.1 找到 Mods 文件夹

| 系统 | VERA20K 文件夹 |
| --- | --- |
| Windows | 安装 VERA20K 的文件夹，`vera20k.exe` 所在处 |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

mod 放在这个文件夹下的 `Mods` 里，每个 mod 一个子文件夹。

### 4.2 建 mod 文件夹

```text
Mods/MyBounty/
├── mod.ini       mod 的名字和说明
└── rules.wasm    你的模块（从 build/ 复制过来）
```

`mod.ini` 示例：

```ini
[Mod]
Name=My Bounty
Version=1
About=Credits for every infantry kill.
```

只加模块的话，不要往 mod 里放 `rulesmd.ini`：mod 里同名的文件会整个替换原版的文件。需要改规则时，请看[第 8 节](#8-不写代码用-ini-声明触发器)。

想省掉每次复制，可以直接编译到 mod 文件夹里。`--` 后面的参数会交给 `vera20k build`：

```sh
npm run build -- --out "<VERA20K 文件夹>/Mods/MyBounty/rules.wasm"
```

也可以把 `vera.json` 里的 `"out"` 设成这个路径。

### 4.3 启动

有两种方式：

- 在游戏里打开 **Options → Mods**，选中你的 mod，游戏会重启进入它；
- 或者用命令行启动：`vera20k --mod MyBounty`。

然后开一局遭遇战。开局时聊天栏应该出现 `Hello from my first module!`。

## 5. 做一个真正的玩法：击杀悬赏

目标：谁打死美国大兵（`E1`）或动员兵（`E2`），谁就得到 200 块。把 `src/index.ts` 换成下面的代码；同样的代码在 `sdk/typescript/examples/tutorial-bounty/src/index.ts`：

```ts
import {
  Action, Event, KilledEvent, NO_HANDLE, Rules, Status, Techno, Trigger,
  log, namedAction, onScenarioStart,
} from "@vera20k/sdk";

// ⑤ 处理一次击杀：给凶手的所属方加钱
function onKill(e: KilledEvent): void {
  // 凶手可能在同一帧也死了
  const house = Techno.owner(e.killer);
  if (house == NO_HANDLE) return;
  if (Action.addMoney(house, 200) == Status.Ok) {
    log("Bounty: +200 credits");
  }
}

// ① 给处理函数起名字：INI 触发器用 Wasm:OnKill 调用它
namedAction("OnKill", onKill);

onScenarioStart((): void => {
  log("Bounty: 200 credits for every GI or conscript killed.");
  // ② 创建触发器：监听“被击杀”事件
  const trigger = Trigger.on(Event.Killed);
  // ③ 绑定到单位类型：这类单位全部生效，包括之后才造出来的
  const names: string[] = ["E1", "E2"];
  for (let i = 0; i < names.length; i++) {
    const infantry = Rules.technoType(names[i]);
    if (infantry != NO_HANDLE) trigger.bind(infantry);
  }
  // ④ 触发时把事件交给 onKill；done() 才真正创建
  if (trigger.enqueue(onKill).done() == NO_HANDLE) {
    log("Bounty: the trigger was refused");
  }
});
```

逐段说明：

1. **`namedAction("OnKill", onKill)`** 给处理函数起名字，INI 触发器才能用 `Wasm:OnKill` 调用它。名字必须直接写字符串：构建时 SDK 不运行模块，直接把它抄进模块的 `vera.actions` 段。只在代码里用的处理函数不需要名字，`enqueue` 接受任何函数。
2. **`Trigger.on(Event.Killed)`** 开始描述一个触发器。触发器就是“发生某个事件时做某件事”，和地图编辑器里的触发器一样。
3. **`bind(...)`** 决定触发器管哪些对象。绑到类型（`Rules.technoType("E1")`）时，所有该类型的单位都受影响，包括后来造出的；也可以绑到某一个具体单位。规则里没有这个名字时 `Rules.technoType` 返回 `NO_HANDLE`，所以先判断再绑定。
4. **`enqueue(onKill)`** 表示触发时把事件记录交给 `onKill`。记录会在每帧的模块阶段统一投递。**`done()`** 才真正创建触发器，成功时返回触发器，被游戏拒绝时返回 `NO_HANDLE`。
5. **事件视图** `KilledEvent` 给事件记录的字段起了名字：`e.victim`（死者）、`e.killer`（凶手），位置是 `e.x`、`e.y`、`e.z`。视图只在处理函数运行期间有效，不要把它存起来；要留着用，就把里面的字段值存下来。`Techno.owner(t)` 读单位的所属方：引用是普通的数值，不是对象，所以要写成 `Techno.owner(t)`，而不是 `t.owner`（原因见[第 6 节](#6-这里的-typescript-和平常有什么不同)）。

编译，复制（或者用 `--out` 直接编译过去），再开一局：每打死一个大兵或动员兵，聊天栏出现 `Bounty: +200 credits`，钱也跟着涨。

> 同样玩法的 Rust 版本在 `sdk/examples/tutorial-bounty`，两份代码调用的游戏接口完全相同。如果你在对局里看到的效果不一样，请反馈给我们。

### 想再加点什么？

- **每 30 秒发一次工资**：游戏每秒 15 帧，450 帧就是 30 秒。`onFrame` 在帧号能被这个数整除的每一帧运行函数：

  ```ts
  import { Action, Every, Find, Op, Prop, onFrame } from "@vera20k/sdk";

  // 一页所属方，只创建一次，反复使用
  const PAGE = new StaticArray<i64>(16);

  function pay(): void {
    const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1);
    let after: i64 = 0;
    do {
      const page = humans.page(PAGE, after);
      for (let i = 0; i < page.count; i++) Action.addMoney(PAGE[i], 500);
      after = page.next;
    } while (after != 0);
  }

  // 每秒 15 帧：450 帧就是 30 秒
  onFrame(new Every(450), pay);
  ```

- **只奖励人类玩家**：在 `onKill` 里先判断 `House.isHuman(house)`。
- **被打残的单位受伤减半**：用“可决定事件”，见[第 7 节](#可决定事件同步改写游戏的判断)。

## 6. 这里的 TypeScript 和平常有什么不同

AssemblyScript 编译的是 TypeScript 的一个严格子集。SDK 只用 TypeScript 和 AssemblyScript 都接受的写法，所以同一份代码既能通过 `tsc`（`npm run check`），也能编译（`npm run build`）。你的代码也要这样写：

| 不能用 | 改成 |
| --- | --- |
| 不写类型的函数，如 `(e) => {}` | 每个参数和返回值都写类型：`(e: KilledEvent): void => {}` |
| `any`、`unknown`，以及除 `T \| null` 以外的联合类型 | 写明具体类型 |
| 没有类型的对象字面量 `{ x: 1 }` | 用类：`class Point { x: i32 = 0; }`，再 `new Point()` |
| 闭包：回调里用外层函数的局部变量 | 用模块顶层的变量，或者具名函数 |
| `for…of`、`for…in` | `for (let i = 0; i < list.length; i++)` |
| `try` / `catch` | 检查返回值：`Status.Ok`、`NO_HANDLE`（`throw` 会让模块停用） |
| 字符串直接加数字 `"hp=" + hp` | `"hp=" + hp.toString()`，或者模板字符串 `` `hp=${hp}` `` |
| `console.log`、`Math.random()`、`Date.now()` | `log(...)`、`random(lo, hi)`、`frame()` |
| npm 上为 JavaScript 写的库 | 只能用为 AssemblyScript 写、不依赖宿主环境的库 |

`tsc` 发现不了其中所有问题（比如它接受 `"hp=" + hp`），构建会发现。每一种都是一条意思明确的编译错误（`Not implemented: Closures`、`Not implemented: Iterators`……）；依赖宿主的调用则不同，构建会说出模块需要哪个导入并拒绝它，比如 `Math.random()` 会报 `imports env.seed`。

几个概念：

- **数值类型**：写 `i32`（32 位整数）、`i64`（64 位整数）、`f32`、`f64`、`u8`、`bool`。`0` 这样的字面量是 `i32`，要存 64 位值的变量得写明：`let after: i64 = 0`。整数除法结果还是整数：`7 / 2` 是 `3`。游戏里的值和引用都是 `i64`。
- **引用是数值，不是对象**：`Techno`、`House`、`Cell`、`TypeRef` 等都是 `i64` 的别名。读写属性用命名空间函数，引用放第一个参数：`Techno.health(t)`、`Techno.setMission(t, Mission.Hunt)`。这样每个引用不用单独分配对象，模块又小又快。`NO_HANDLE`（0）表示“没有”。
- **顶层代码就是初始化**：写在文件顶层的语句会在模块加载时运行一次。回调都在这里登记：`onScenarioStart`、`onFrame`、`namedAction`、`Ini.key`……这些函数都可以调用多次：登记的每个函数都会运行，按登记顺序。
- **错误是返回值**：写操作（动作、写属性）返回 `Status`：成功是 `Status.Ok`，其他值说明原因，比如 `Gone`（对象已经不存在）、`NotFound`（名字不存在）、`Refused`（游戏拒绝）、`Unsupported`（这个版本还没有）。读取或创建的调用直接返回值，失败时返回空值（`0`、`NO_HANDLE`、`NO_ID`），这时 `lastStatus()` 给出原因。
- **视图只在回调期间有效**：事件和决定视图（`KilledEvent`、`TakeDamageDecision`……）直接读取引擎的记录。需要的字段请复制到自己的变量里。

## 7. 常用功能速查

下面每段代码都是一个能编译的完整文件，按需取用。每个函数都在 [SDK 参考](ts-sdk.zh-CN.md)里。

### 生命周期

```ts
import {
  Event, Every, House, KilledEvent, NO_HANDLE, Rules, TakeDamageDecision, Trigger,
  log, namedAction, onFrame, onMessage, onRules, onScenarioStart,
} from "@vera20k/sdk";

// 顶层代码：模块加载时运行一次
log("loaded");

function tick(): void {}
function onKill(e: KilledEvent): void {}
function halve(d: TakeDamageDecision): void {}

onRules((): void => {});                  // 读规则时运行一次，可以改规则
onScenarioStart((): void => {             // 对局开始时运行一次
  const gi = Rules.technoType("E1");
  if (gi == NO_HANDLE) return;
  Trigger.on(Event.Killed).bind(gi).enqueue(onKill).done();    // 事件：在模块阶段投递
  Trigger.on(Event.TakeDamage).bind(gi).decideSync(halve);     // 可决定事件：立即调用
});
onFrame(new Every(1), tick);              // 每帧运行
onFrame(new Every(15), tick);             // 每 15 帧（1 秒）运行一次
onMessage((sender: House, data: Uint8Array): void => {});     // 客户端模块发来的消息
namedAction("OnKill", onKill);            // INI 里用 Wasm:OnKill 调用
```

### 名字和引用

模块用“引用”指代游戏里的东西：`Techno`（单位或建筑）、`House`（所属方）、`Cell`（地格）、`TypeRef`（类型）、`Waypoint`（路径点）。名字只在开局时查一次，存进顶层变量：

```ts
import { Cell, NO_HANDLE, Rules, TypeRef, Waypoint, log, onScenarioStart } from "@vera20k/sdk";

// 名字只查一次，存进顶层变量
let GI: TypeRef = NO_HANDLE;
let SUPER: TypeRef = NO_HANDLE;

onScenarioStart((): void => {
  GI = Rules.technoType("E1");       // 单位类型；名字不存在时是 NO_HANDLE
  SUPER = Rules.warhead("Super");    // 弹头
  const cell = Cell.at(50, 60);      // 按坐标取地格，不调用游戏
  const drop = Waypoint.number(3);   // 3 号路径点
  if (GI == NO_HANDLE) log("these rules have no E1");
});
```

### 读属性

```ts
import { Cell, House, Prop, Techno, get, log } from "@vera20k/sdk";

function report(unit: Techno, house: House, cell: Cell): void {
  const hp = Techno.health(unit);
  const owner = Techno.owner(unit);
  const money = House.money(house);
  const land = Cell.landType(cell);
  const name = House.name(house);                  // 字符串属性返回 string
  const percent = get(unit, Prop.HealthPercent);   // 任何属性都能用 Prop 读
  log(name + ": " + money.toString() + " credits, unit at " + percent.toString() + "%");
}

// 一次读很多对象：一次调用，而不是每个值一次
const PROPS: StaticArray<i32> = [Prop.Health, Prop.Owner];

function readAll(units: StaticArray<Techno>): void {
  const values = Techno.read(units, PROPS);
  for (let i = 0; i < units.length; i++) {
    const health = values[i * 2];
    const owner = values[i * 2 + 1];
  }
}
```

全部属性见[附录第 7 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#7-属性)；在 SDK 里它们是 `Prop` 的常量，名字里的 `.` 写成 `_`（`Prop.Passable_Foot` 就是 `Passable.Foot`）。

### 搜索

```ts
import { Action, Find, House, Op, Prop, Techno, TypeRef } from "@vera20k/sdk";

// 某一方的第一个美国大兵，没有时是 NO_HANDLE
function firstGi(house: House, gi: TypeRef): Techno {
  return Find.objects().ownedBy(house).ofType(gi).first();
}

// 所有人类玩家，一页一页地读
const PAGE = new StaticArray<i64>(16);

function payHumans(): void {
  const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1);
  let after: i64 = 0;
  do {
    const page = humans.page(PAGE, after);
    for (let i = 0; i < page.count; i++) Action.addMoney(PAGE[i], 1000);
    after = page.next;
  } while (after != 0);
}
```

搜索还可以用 `near(cell, 半径)`（半径单位是 lepton，一格 256）和 `within(cell, 宽, 高)` 限定范围。结果按创建顺序排列。只要 `next` 不为 0 就继续读：一页可能不满，甚至是空的，但不一定是最后一页。一个 `Find` 最多 16 个筛选条件。

### 动作

```ts
import { Action, Cell, House, Mission, NO_HANDLE, Rules, Techno, Waypoint } from "@vera20k/sdk";

function examples(unit: Techno, house: House): void {
  const warhead = Rules.warhead("Super");
  const rhino = Rules.technoType("HTNK");
  Action.addMoney(house, 500);                    // 给钱（负数是扣钱）
  Action.damage(unit, 50, warhead);               // 用某个弹头造成伤害
  Action.heal(unit, 50);                          // 治疗
  Action.kill(unit, NO_HANDLE);                   // 击杀，第二个参数是记功的凶手
  const tank = Action.createObject(Cell.at(60, 40), rhino, house);  // 失败时是 NO_HANDLE
  Techno.setMission(tank, Mission.Hunt);          // 改任务
  Action.nukeStrike(Waypoint.number(3));          // 原版触发器动作“核弹打击”
}
```

原子动作和原版触发器动作的完整列表见[附录第 5 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#5-动作种类)。

### 修饰器（临时改属性倍率）

```ts
import { Modifier, Prop, Techno, fixed } from "@vera20k/sdk";

function boost(unit: Techno): void {
  // 攻击力 ×2，持续 150 帧（10 秒）；frames 写负数表示直到移除
  const m = Modifier.multiply(unit, Prop.FirepowerMultiplier, fixed(2.0), 150);
  Modifier.remove(m);   // 提前移除
}
```

可用的有攻击力、护甲、速度、射速、射程倍率和额外射程，以及可隐形、强制显形和禁用武器；其余的见 [SDK 参考](ts-sdk.zh-CN.md#修饰器)。修饰器和 Phobos 的 AttachEffect 叠加，没有修饰器时行为和原版完全一致。

### 变量

红警 2 的局部变量和全局变量是 `Var.local(n)` 和 `Var.global(n)`。模块自己的变量写在 `vera.json` 里：

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

构建时 SDK 把它们写进模块的 `vera.vars` 段，并生成 `src/generated/vars.ts`，每个变量一个常量，导入它就能用（`npm run check` 和 `vera20k generate` 不编译也会写出这个文件）：

```ts
import { Techno, Var } from "@vera20k/sdk";
import { Panicked, Wave } from "./generated/vars";

function example(unit: Techno): void {
  Var.local(3).set(1);                // 红警 2 的 3 号局部变量
  const on = Var.global(5).get();     // 红警 2 的 5 号全局变量
  Wave.set(Wave.get() + 1);           // 在 vera.json 里声明的变量
  Panicked.set(unit, 1);
  const panicked = Panicked.get(unit);
}
```

`scope` 是 `global`（整局一个值）、`house`（每个所属方一个）或 `object`（每个单位或建筑一个），必填。`visibility` 决定哪些客户端模块能读：`private`（这个所属方自己的玩家）、`allies`（盟友）、`insight`（看得见的，只用于 object）、`public`（所有人）。变量会存进存档。`notify` 声明给客户端模块用的通知名（生成到 `src/generated/notify.ts`），[客户端模块教程](ts-ui-tutorial.zh-CN.md)里会用到。

### 触发器的筛选条件和即时动作

```ts
import {
  ActionDef, Event, NO_HANDLE, Op, Prop, Rules, TakeDamageEvent, Target, Trigger, VarTarget,
  log, onScenarioStart,
} from "@vera20k/sdk";
import { Panicked } from "./generated/vars";

function giPanic(e: TakeDamageEvent): void {
  log("A GI panics");
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi == NO_HANDLE) return;
  Trigger.on(Event.TakeDamage)
    .bind(gi)
    .where(Prop.HealthPercent, Op.Lt, 30)                      // 只在血量低于 30% 时
    .whereVar(VarTarget.Subject, Panicked, Op.Eq, 0)           // 并且还没恐慌过
    .immediate(ActionDef.setVar(Target.Subject, Panicked, 1))  // 触发的同时由游戏执行
    .enqueue(giPanic)                                          // 再交给模块处理
    .done();
});
```

筛选条件由游戏检查，不满足时不会打扰模块，比在处理函数里自己判断省得多。`.once()` 让触发器只触发一次。

### 可决定事件（同步改写游戏的判断）

在游戏做出判断的那一刻同步调用模块，模块可以改写结果。例如让美国大兵受到的伤害减半：

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

`d.native` 是游戏自己的答案，`d.previous` 是到目前为止的答案（可能已经有别的模块先回答过），`d.setResult(value)` 给出你的答案；不调用它就保持之前的答案。可以决定的有：受到的伤害、能否建造或排进生产队列、所属方是否判负、是否接受玩家命令，还有开火、还击、目的地、碾压、采矿、部署、空闲任务、移动和寻路，完整列表见 [SDK 参考](ts-sdk.zh-CN.md#可决定事件)。决定函数里**只能读**（`log` 可以用）：在里面执行动作、写属性或写变量会让模块被停用。也不要在里面分配内存（不要 `new`，不要拼字符串）。

### 给单位下任务、给电脑方下指令

```ts
import { Action, Cell, House, Mission, Order, Rules, Techno } from "@vera20k/sdk";

function command(computer: House, tank: Techno): void {
  const units: StaticArray<Techno> = [tank];
  House.order(computer, Order.moveTo(units, Cell.at(60, 40)));
  House.order(computer, Order.queueProduction(Rules.technoType("HTNK")));
}

function anyHouse(tank: Techno): void {
  Techno.setMission(tank, Mission.Hunt);                    // 写属性，没有目标
  Action.setMission(tank, Mission.Move, Cell.at(60, 40));   // 执行动作，带目标地格
}
```

`House.order` 只能下给电脑方，在下一帧执行，和玩家点鼠标下的命令走同一套校验；一条指令最多带 64 个单位。要指挥**任何一方**的单位，包括玩家自己的，用任务：它以单位所属一方的名义执行，就像那一方的玩家点了鼠标。

### 加载时改规则

```ts
import { Rules, log, onRules } from "@vera20k/sdk";

onRules((): void => {
  Rules.set("E1", "Strength", "250");            // 美国大兵血量改成 250
  log("E1 Strength is now " + Rules.get("E1", "Strength"));
});
```

`onRules` 只在规则读入时运行一次，改动对整局生效，所有玩家的机器上结果一致。对局开始后，`Rules.set` 会被拒绝。

### 其他

```ts
import { NO_HANDLE, Rules, Status, frame, lastStatus, log, random } from "@vera20k/sdk";

function misc(): void {
  const now = frame();             // 当前帧号，每秒 15 帧
  const roll = random(1, 100);     // 确定性随机数：每台机器都一样
  const rhino = Rules.technoType("HTNK");
  if (rhino == NO_HANDLE && lastStatus() == Status.NotFound) log("no HTNK");
  log("frame " + now.toString() + ", roll " + roll.toString());
}
```

## 8. 不写代码：用 INI 声明触发器

只需要原版动作和模块里已有的处理函数时，可以直接在 mod 的 `rulesmd.ini` 里声明触发器，不用改代码。

**先准备好 mod 的 `rulesmd.ini`**：mod 里的 `rulesmd.ini` 会整个替换原版的，所以要从一份完整的副本开始。在游戏里用 **Options → Mods → New Mod** 新建的 mod 自带原版文件，见 [modding](../modding.md)。

例子：美国大兵血量低于 30% 时回 50 血，每个大兵只触发一次。

```ini
[E1]
Wasm.Tags=GIHealTag        ; 把标签挂到美国大兵类型上

[GIHealTag]
Trigger=GIHeal             ; 标签对应的触发器，多个用逗号分隔

[GIHeal]
Event=TakeDamage
Where1=Subject.HealthPercent<30
Action1=Heal(Subject,50)
Repeat=OneTimeOr
```

要调用模块的处理函数，写 `Action1=Wasm:OnKill`，名字就是 `namedAction("OnKill", onKill)` 里起的。`Decide=Wasm:Name` 把触发器变成可决定事件的触发器，请求交给这个名字的处理函数；这个处理函数接收决定视图，比如 `TakeDamageDecision`。

触发器段的键：

| 键 | 值 |
| --- | --- |
| `Event`、`Event<n>` | 事件名，或 `name(op,interval,id,value)`；写多个时要全部满足 |
| `Where<n>` | `Subject.`、`Other.` 或 `House.` 加一个属性，或 `Var(scope,name)`，然后是比较符（`=`、`!=`、`<`、`<=`、`>`、`>=`）和一个数字 |
| `Action`、`Action<n>` | `Wasm:name`、`Wasm:name(target,text)`，或 `action(target,parameters…)` |
| `Decide` | `Wasm:name`，一个决定处理函数 |
| `House` | 国家名；留空表示主体自己的所属方 |
| `Difficulty` | `Easy`、`Normal`、`Hard`，用逗号分隔 |
| `Repeat` | `OneTimeOr`（默认）、`OneTimeAnd`、`Repeating` |

写错了对局就开不了，提示里会写明是哪个段、哪个键。完整语法见[模块指南](guide.zh-CN.md#在-ini-里写触发器)。地图不能用 `Wasm.Tags` 绑定类型（在地图里会被忽略，开局时会提示）；地图的模块在代码里绑定。

`Wasm:name(target,text)` 会把目标和一段文字交给处理函数：在处理函数里用 `e.record.target` 和 `e.record.argsText` 读取。

### 模块自己的 INI 键

模块还可以给 modder 提供自己的 INI 名字：类型段里的键、列表段（注册表）和模块事件。modder 写 INI，模块负责实现。注册只能写在顶层代码里，写在别处会让模块被停用。

```ts
import {
  EventScope, Ini, IniEntryRecord, IniKeyRecord, IniSection, NO_ID, Rules, fail, log,
} from "@vera20k/sdk";

// [Demo.ShieldTypes] 的一个条目：读取并检查它的段
function loadShieldType(r: IniEntryRecord): void {
  const strength = Ini.int(Ini.stripComment(Rules.get(r.entryName, "Strength")), -1);
  if (strength < 1) fail("[" + r.entryName + "] Strength: needs a number of 1 or more");
}

// 写了 Demo.Shield=... 的一个段
function attachShield(r: IniKeyRecord): void {
  log("[" + r.sectionName + "] Demo.Shield=" + r.value);
}

// 顶层代码：只有这里能注册 INI 名字
Ini.registry("Demo.ShieldTypes", loadShieldType);
Ini.key("Demo.Shield", IniSection.TechnoType, attachShield);
const SHIELD_BROKEN = Ini.event("Demo.ShieldBroken", EventScope.Object);
if (SHIELD_BROKEN == NO_ID) log("another module registered Demo.ShieldBroken");
```

mod 里有下面这段 INI 时，模块会在 `onScenarioStart` 之前收到 `HeavyShield` 的一条 `IniEntryRecord` 和 `[HTNK]` 的一条 `IniKeyRecord`：

```ini
[Demo.ShieldTypes]
1=HeavyShield

[HeavyShield]
Strength=300

[HTNK]
Demo.Shield=HeavyShield
```

INI 触发器用 `Event=Wasm:Demo.ShieldBroken` 使用这个模块事件；代码用 `Action.raiseEvent` 触发它，用 `Trigger.onModuleEvent` 监听它。值是合并后的规则原文，游戏不做检查：用 `Ini.int`、`Ini.ints`、`Ini.yesNo`、`Ini.split` 解析，值不对时用 `fail(...)` 停下，并写明是哪个段、哪个键。见[模块指南](guide.zh-CN.md#模块注册的-ini-名称)，完整的 mod 见 `sdk/typescript/examples/ini-counter`。

### 把模块嵌进地图

地图可以带自己的模拟模块。用 `asset` 工具：

```sh
asset wasm-embed 原地图.map build/rules.wasm 新地图.map   # 把模块写进地图
asset wasm-check 新地图.map                               # 检查地图里的模块
```

`wasm-embed` 会自动写好 `[VeraWasm] Api=` 和 `RulesSize=`。另外请在地图的 `[Basic]` 里写上 `VeraMinVersion=`，值是你制作地图时用的 VERA20K 版本。地图里的模块更在乎体积，最好控制在几十 KB 以内。`npm run build` 默认就是优化体积的发布构建。

## 9. 调试与排错

**看日志**

- `log("...")` 的内容会出现在聊天栏，每帧最多显示 8 行，每行最多 160 个字符。数字用 `.toString()` 转成文字。
- 所有日志都会完整写进游戏日志文件 `logs/ra2.log`，它在第 4.1 节说的 VERA20K 文件夹里。

**模块被停用时**

聊天栏会出现一条红色提示，比如 `Mod module stopped in on_frame: ...`，说明停在哪个函数、为什么停。常见原因：

| 提示里的原因 | 意思 | 怎么办 |
| --- | --- | --- |
| `exceeded its operation budget` | 一次调用里运算太多，超出预算 | 把工作分摊到多帧（`new Every(N)`），用 `Techno.read` 或 `getMany` 批量读 |
| `trapped: …` | 代码崩溃了：数组越界、把 `null` 当对象用、`throw`、`fail(...)`、内存用光 | 访问数组前检查长度，用之前先 `if (x != null)`。下标越界、解引用 `null` 或 `throw` 时，模块日志在这一行之前会记下 `abort: <message> at <file>:<line>:<column>`，即源码里出错的位置 |
| `made an invalid call: …` | 接口用错了，比如参数超范围、变量作用域不对、在决定函数里写东西、在顶层以外注册；后面会写具体原因 | 对照 [SDK 参考](ts-sdk.zh-CN.md)检查这次调用 |
| `cannot keep up: … event records are waiting` | 事件积压太多，处理不过来 | 减少绑定范围，或加筛选条件 `.where(...)` |

**构建失败**

| 提示 | 原因 |
| --- | --- |
| 带源码行的 `ERROR TS…` 或 `ERROR AS…` | 编译错误；常见的见[第 6 节](#6-这里的-typescript-和平常有什么不同)（`Not implemented: Closures`、`Iterators`、`Exceptions`、`union types`） |
| `a simulation module may import only vera:sim@1.0, but this one imports env.…` | 用了依赖宿主的东西：`console`（`env.console.log`）、`Math.random`（`env.seed`）、`Date`（`env.Date.now`），或者某个 JavaScript 库 |
| `namedAction's name must be a string literal` | `namedAction(NAME, fn)` 用了变量；把名字直接写在引号里 |
| `named actions "A" and "B" share dispatch key …` | 两个名字的哈希值撞了，改掉其中一个 |
| `variable "…" repeats "…" (names ignore case)` | `vera.json` 里两个声明只有大小写不同 |
| `vera.json: variable … scope must be global, house or object` | `vars` 里有一项没写 `scope` 或写错了 |
| `Cannot find module './generated/vars'`（`tsc` 报的） | 没有 `src/generated/`：运行 `npm run check` 或 `vera20k generate`，它们会根据 `vera.json` 写出这些文件。`vera.json` 里没有 `vars` 时就没有 `vars.ts` 可导入 |

构建有时还会打印 `Last converge was suboptimal.`，这是优化器的提示，模块没有问题。

`npm run build -- --debug` 会关掉体积优化并带上调试名字，用反汇编工具看模块时更好读。

**对局开不起来**

一般是 INI 触发器写错了、`[VeraWasm]` 里的预算低于 20,000、地图要求的版本比游戏新，或者 mod 写了 `MapWasm=no` 却用了带模块的地图。看开局时的提示，它会写明是哪个段、哪个键。

## 10. 规则与限制

**确定性**

联机时每台机器都跑同一份模块，所以模块的结果必须在每台机器上都一样：

- 需要随机数时用 SDK 的 `random(lo, hi)`。它用的是模块自己的随机数流，每台机器的序列都相同。`Math.random()` 编译不过。
- 浮点运算可以用，NaN 会被统一成同一个值。不过模拟相关的数值建议用整数或 `fixed()`。
- 读不到系统时间。需要计时就用 `frame()`，游戏每秒 15 帧。

**资源上限**

| 项目 | 上限 |
| --- | --- |
| 模块大小 | 8 MiB |
| 内存 | mod 模块 32 MiB，地图模块 16 MiB |
| 每次调用的运算量（预算） | mod 模块默认也是最多 500,000，地图模块默认 100,000；mod 可以在 `[VeraWasm]` 里用 `Budget` 和 `MapBudget` 设置 |
| 启动调用（顶层代码、`onRules`、`onScenarioStart`） | 每次 5,000,000 |
| 每个模块的触发器 | 256 个 |
| 每次搜索的筛选条件 | 16 个 |
| 每帧给电脑方的指令 | 256 条，总共 4,096 个单位；每条最多 64 个单位 |

**存档**

存档会保存模块的内存、变量、修饰器和触发器。读档时不会重新运行模块代码（包括顶层代码），模块从存档那一刻接着运行，顶层变量的值也原样恢复。

**性能建议**

- 名字在开局时查一次，存进顶层变量。
- 一次要读很多值时用 `Techno.read` 或 `getMany` 批量读。
- 需要扫描全图的工作分摊到多帧，比如每帧只处理一部分对象。
- 少在每帧都运行的代码里分配内存：每次 `new`、建数组、拼字符串都消耗运算量，也给垃圾回收添活。像上面的搜索例子那样，复用顶层的 `StaticArray`。

## 参考

- [TypeScript SDK 参考](ts-sdk.zh-CN.md)：每个函数、`vera.json` 和构建命令
- [客户端模块教程](ts-ui-tutorial.zh-CN.md)：在屏幕上做一个按钮（`ui.wasm`）
- [模块指南](guide.zh-CN.md)：模块放在哪、INI 语法、限制和调试
- [Rust 版教程](rust-tutorial.zh-CN.md)：用 Rust 做同一个 mod
- [RFC 0001](../rfcs/0001-wasm-extension-modules.zh-CN.md)：设计说明，TypeScript SDK 的约定在第 5a 节；[附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)：全部接口、事件、动作、属性
- `sdk/typescript/examples/` 里的示例：Rust 示例的移植版；`tutorial-bounty` 就是本教程的代码。
