# Wasm 模块 Mod 开发教程（TypeScript 版）

这份教程带你从零开始，用 TypeScript 给 VERA20K 写一个 Wasm 模块，并放进游戏里试玩。读完后你会做出一个“击杀悬赏”mod：谁打死美国大兵或动员兵，谁就拿 200 块赏金。

> **注意：TypeScript SDK 还没有实现。** 本教程假设它已经存在，按 [RFC 0001](rfcs/0001-wasm-extension-modules.md) 第 5a 节和[附录第 3 节](rfcs/0001-wasm-extension-modules-api.md#3-sdk-mappings)的约定写成，用来提前评审 API 用起来顺不顺手。包名 `@vera20k/sdk`、`npm create` 模板和构建命令都是占位，代码没有编译过。现在能用的是 Rust SDK，见 [Rust 版教程](wasm-modules-tutorial.zh-CN.md)。

- 写给谁：会改 `rulesmd.ini`、想做 INI 做不到的玩法的 modder。写过一点 JavaScript 或 TypeScript 就够了。
- 参考资料：功能清单见[模块指南](wasm-modules.md)；完整设计见 [RFC 0001](rfcs/0001-wasm-extension-modules.md)；每个函数、事件、动作、属性的编号与含义见[附录](rfcs/0001-wasm-extension-modules-api.md)。
- 预览版提醒：接口还在 `0.x` 预览阶段，名字和编号在正式版前可能会变。游戏会在模块需要更新的接口时直接告诉你。

## 目录

1. [模块是什么](#1-模块是什么)
2. [准备环境](#2-准备环境)
3. [第一个模块：在聊天栏说一句话](#3-第一个模块在聊天栏说一句话)
4. [放进游戏试玩](#4-放进游戏试玩)
5. [做一个真正的玩法：击杀悬赏](#5-做一个真正的玩法击杀悬赏)
6. [这里的 TypeScript 和平常有什么不同](#6-这里的-typescript-和平常有什么不同)
7. [常用功能速查](#7-常用功能速查)
8. [INI 触发器和地图嵌入](#8-ini-触发器和地图嵌入)
9. [调试与排错](#9-调试与排错)
10. [规则与限制](#10-规则与限制)

## 1. 模块是什么

模块是一个编译好的 WebAssembly 文件，游戏里叫 `rules.wasm`。它和对局一起运行，每个玩家的机器上都跑同一份，所以联机也不会不同步。

TypeScript 源码不会进游戏。SDK 先用 AssemblyScript 把它编译成 Wasm，游戏只加载编译结果，和 Rust 写的模块没有区别。

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

到 <https://nodejs.org> 安装 Node.js（LTS 版即可），装好后命令行里能运行 `node` 和 `npm`。编辑器推荐 VS Code，它能直接识别 SDK 带的类型，写代码时有补全和报错提示。

### 2.2 获取 `asset` 工具

`asset` 工具用来检查模块，以及把模块嵌进地图。它的 Wasm 相关命令不需要安装游戏。它随 VERA20K 源码提供，需要 Rust 编译一次：

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cargo build --release --bin asset
# 编译好的程序在 target/release/asset（Windows 上是 asset.exe）
```

SDK 正式发布后，`asset` 的这几个命令也会打进 npm 包（`npx vera20k check`），那时就不用装 Rust 了。

## 3. 第一个模块：在聊天栏说一句话

### 3.1 建项目

用模板新建项目：

```sh
npm create @vera20k/module@latest my-bounty
cd my-bounty
npm install
```

项目结构：

```text
my-bounty/
├── package.json        项目名、依赖和 build / watch 命令
├── tsconfig.json       类型检查设置，不要删
├── asconfig.json       编译设置：只生成游戏接受的 Wasm 特性，不要改
├── vera.json           模块自己的变量声明（第 7 节）
└── src/index.ts        模块代码
```

`asconfig.json` 一定要保留原样。它关掉了游戏会拒绝的 Wasm 特性（引用类型、SIMD 等，RFC 第 3 节），也让编译结果不带 `start` 段和 `env.abort` 导入。改了它，`wasm-check` 多半不会通过。

### 3.2 写代码

把 `src/index.ts` 换成最简单的模块：

```ts
import { log, onScenarioStart } from "@vera20k/sdk";

// 告诉游戏：对局开始时运行这个函数
onScenarioStart((): void => {
  log("Hello from my first module!");
});
```

`log` 写的内容会出现在游戏聊天栏，同时写进游戏日志文件。

注意箭头函数写了返回类型 `: void`。AssemblyScript 要求函数的参数和返回值都写明类型，这一点和平常写 TypeScript 不同，第 6 节会集中说明。

### 3.3 编译和检查

```sh
npm run build
```

编译结果是 `build/rules.wasm`，名字已经是游戏要的。再用游戏的加载检查跑一遍：

```sh
asset wasm-check build/rules.wasm
# 通过时输出：{"bytes":…,"ok":true}
```

没通过时，它会告诉你游戏为什么不接受这个模块。

开发时可以用 `npm run watch`：保存文件就自动重新编译。

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

只加模块的话，不要往 mod 里放 `rulesmd.ini`：mod 里同名的文件会整个替换原版的文件。需要改规则时，请看 [Rust 版教程第 7 节](wasm-modules-tutorial.zh-CN.md#7-不写代码用-ini-声明触发器)。

想省掉每次复制，可以在 `package.json` 里给 build 加一个输出目录，直接写进 mod 文件夹：

```json
"scripts": {
  "build": "vera20k build --out \"<VERA20K 文件夹>/Mods/MyBounty/rules.wasm\""
}
```

### 4.3 启动

有两种方式：

- 在游戏里打开 **Options → Mods**，选中你的 mod，游戏会重启进入它；
- 或者用命令行启动：`vera20k --mod MyBounty`。

然后开一局遭遇战。开局时聊天栏应该出现 `Hello from my first module!`。

## 5. 做一个真正的玩法：击杀悬赏

目标：谁打死美国大兵（`E1`）或动员兵（`E2`），谁就得到 200 块。

```ts
import { log, onScenarioStart, namedAction,
         Trigger, Event, KilledEvent, Rules, Techno, Action,
         Status, NO_HANDLE } from "@vera20k/sdk";

// ⑤ 处理一次击杀：给凶手的所属方加钱
function onKill(e: KilledEvent): void {
  const house = Techno.owner(e.killer);
  if (house == NO_HANDLE) return;          // 凶手可能在同一帧也死了
  if (Action.addMoney(house, 200) == Status.Ok) {
    log("Bounty: +200 credits");
  }
}
// ① 给处理函数起名字，INI 里可以用 Wasm:OnKill 调用它
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

1. **`namedAction("OnKill", onKill)`** 把函数登记成模块的“动作”。名字必须直接写字符串，因为构建时 SDK 会把它抄进模块的 `vera.actions` 段，INI 触发器才能用 `Wasm:OnKill` 找到它。只在代码里用的函数也可以不登记，直接交给 `enqueue`。
2. **`Trigger.on(Event.Killed)`** 开始描述一个触发器。触发器就是“发生某个事件时做某件事”，和地图编辑器里的触发器一样。
3. **`bind(...)`** 决定触发器管哪些对象。绑到类型（`Rules.technoType("E1")`）时，所有该类型的单位都受影响，包括后来造出的；也可以绑到某一个具体单位。名字查不到时 `Rules.technoType` 返回 `NO_HANDLE`，所以先判断再绑定。
4. **`enqueue(onKill)`** 表示触发时把事件记录交给 `onKill`。记录会在每帧的模块阶段统一投递。**`done()`** 才真正创建触发器，成功时返回触发器的引用，失败时返回 `NO_HANDLE`。
5. **事件视图** `KilledEvent` 把事件记录包装成有名字的字段：`e.victim`（死者）、`e.killer`（凶手）、`e.position`（位置）。视图只在回调运行期间有效，不要把它存起来；要留着用，就把里面的字段值存下来。
6. `Techno.owner(t)` 读单位的所属方。引用是普通的数值，不是对象，所以要写成 `Techno.owner(t)`，而不是 `t.owner`（原因见第 6 节）。

编译、检查，然后替换 mod 里的 `rules.wasm`，再开一局：每打死一个大兵或动员兵，聊天栏出现 `Bounty: +200 credits`，钱也跟着涨。

> 同样的玩法用 Rust 写的版本在 `sdk/examples/tutorial-bounty`，已经编译并通过了 `asset wasm-check`。两份代码调用的游戏接口完全相同。

### 想再加点什么？

- **每 30 秒发一次工资**：游戏每秒 15 帧，450 帧就是 30 秒。

  ```ts
  function pay(): void { /* 给每个人类玩家加钱，见第 7 节“搜索” */ }
  onFrame(new Every(450), pay);
  ```

- **只奖励人类玩家**：在 `onKill` 里先判断 `House.isHuman(house)`。
- **被打残的单位受伤减半**：用“可决定事件”，见第 7 节。

## 6. 这里的 TypeScript 和平常有什么不同

模块代码由 AssemblyScript 编译成 Wasm。SDK 的接口只用 TypeScript 和 AssemblyScript 都支持的写法，所以同一份代码能用 `tsc` 做类型检查，也能用 AssemblyScript 编译。为此有几条限制：

| 不能用 | 改成 |
| --- | --- |
| `any`、`unknown`，以及除 `T \| null` 以外的联合类型 | 写明具体类型 |
| 对象字面量 `{ x: 1 }`、结构化类型 | 用 SDK 的类或构建器，例如 `new Every(15)`、`Order.moveTo(...)` |
| `try` / `catch` / `throw` | 检查返回值：`Status.Ok`、`NO_HANDLE` |
| 回调里用外层函数的局部变量（闭包） | 用模块顶层的变量、常量，或者具名函数 |
| `for…of`、`for…in` | 普通的 `for (let i = 0; …)` 循环 |
| 省略类型的函数，如 `() => {}` | 写明参数和返回类型：`(): void => {}` |
| npm 上的普通 JS 库 | 只能用为 AssemblyScript 写的库 |

几个概念：

- **数值类型**：SDK 用 AssemblyScript 的 `i32`、`i64`、`f32`。游戏里的值和引用都是 `i64`；在 `tsc` 里它们都是 `number`，不影响类型检查。
- **引用是数值，不是对象**：`Techno`、`House`、`Cell`、`TypeRef` 等都是 `i64` 的别名。读写属性用命名空间函数，引用放第一个参数：`Techno.health(t)`、`Techno.setMission(t, Mission.Hunt)`。这样每个引用不用单独分配对象，模块又小又快。
- **顶层代码就是初始化**：写在文件顶层的语句会在模块加载时运行一次，相当于 Rust 的 `init!`。`onScenarioStart`、`onFrame` 这些登记函数也写在顶层。
- **错误处理**：返回引用的函数失败时返回 `NO_HANDLE`（就是 0）；动作、写属性等返回 `Status`，成功是 `Status.Ok`，其他值说明原因，比如 `Gone`（对象已经不存在）、`NotFound`（名字不存在）、`Refused`（游戏拒绝）、`Unsupported`（这个版本还没有）。返回 `NO_HANDLE` 时，`lastStatus()` 给出具体原因。

## 7. 常用功能速查

下面的写法都假设已经从 `@vera20k/sdk` 导入了用到的名字。

### 生命周期

```ts
// 顶层代码：模块加载时运行一次
onRules((): void => { ... });                 // 读规则时运行一次，可以改规则
onScenarioStart((): void => { ... });         // 对局开始时运行一次
onFrame(new Every(1), tick);                  // 每帧运行
onFrame(new Every(15), tick);                 // 每 15 帧（1 秒）运行一次
Trigger.on(...).enqueue(fn).done();           // 处理触发器投递的事件
Trigger.on(...).decideSync(fn);               // 处理可决定事件
```

### 名字和引用

模块用“引用”指代游戏里的东西：`Techno`（单位或建筑）、`House`（所属方）、`Cell`（地格）、`TypeRef`（类型）、`Waypoint`（路径点）。名字只在开局时查一次，存进顶层变量：

```ts
let GI: TypeRef = NO_HANDLE;

onScenarioStart((): void => {
  GI = Rules.technoType("E1");                // 单位类型
  const warhead = Rules.warhead("Super");     // 弹头
  const cell = Cell.at(50, 60);               // 地格坐标，不调用游戏
});
```

### 读属性

```ts
const hp = Techno.health(unit);
const owner = Techno.owner(unit);
const money = House.money(house);
const land = Cell.landType(cell);
const name = House.name(house);               // 字符串属性返回 string
```

全部属性见[附录第 7 节](rfcs/0001-wasm-extension-modules-api.md#7-properties)。一次要读很多对象的属性时用批量读，比逐个读快：

```ts
const values = Techno.read(units, [Prop.Health, Prop.Owner]);
```

### 搜索

```ts
// 第一个属于 house 的美国大兵，没有时是 NO_HANDLE
const first = Find.objects().ownedBy(house).ofType(GI).first();

// 所有人类玩家（分页读，每页最多 page 的长度个）
const page = new StaticArray<House>(16);
let after: i64 = 0;
do {
  const result = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(page, after);
  for (let i = 0; i < result.count; i++) Action.addMoney(page[i], 1000);
  after = result.next;
} while (after != 0);
```

搜索还可以用 `near(cell, 半径)` 和 `within(cell, 宽, 高)` 限定范围。结果按创建顺序排列，`next` 不为 0 时说明还有下一页。

### 动作

```ts
Action.addMoney(house, 500);                        // 给钱（负数是扣钱）
Action.damage(unit, 50, warhead);                   // 用某个弹头造成伤害
Action.heal(unit, 50);                              // 治疗
Action.kill(unit, NO_HANDLE);                       // 击杀，第二个参数是记功的凶手
const tank = Action.createObject(cell, RHINO, house);  // 在地格上造一个单位，失败是 NO_HANDLE
Techno.setMission(unit, Mission.Hunt);              // 改任务
Action.nukeStrike(cell, house);                     // 核弹
```

原子动作和原版触发器动作的完整列表见[附录第 5 节](rfcs/0001-wasm-extension-modules-api.md#5-action-kinds)。

### 修饰器（临时改属性倍率）

```ts
// 攻击力 ×2，持续 150 帧（10 秒）；frames 写负数表示永久
const boost = Modifier.multiply(unit, Prop.FirepowerMultiplier, fixed(2.0), 150);
Modifier.remove(boost);   // 提前移除
```

可用的倍率有攻击力、护甲、速度、射速、射程、暴击，还有不死、隐形、禁用武器等。它们和 Phobos 的 AttachEffect 叠加，没有修饰器时行为和原版完全一致。

### 变量

原版的局部变量和全局变量：

```ts
Var.local(3).set(1);
const on = Var.global(5).get();
```

模块自己的变量写在项目的 `vera.json` 里。构建时 SDK 把它们写进模块的 `vera.vars` 段，并生成 `src/generated/vars.ts`：

```json
{
  "vars": [
    { "name": "Wave",     "scope": "global", "visibility": "public",  "initial": 1 },
    { "name": "Panicked", "scope": "object", "visibility": "private", "initial": 0 }
  ]
}
```

```ts
import { Wave, Panicked } from "./generated/vars";

Wave.set(2);
Panicked.set(unit, 1);
const panicked = Panicked.get(unit);
```

作用域有全局（`global`）、所属方（`house`）、单位（`object`）三种。变量会存进存档。

### 触发器的筛选条件和即时动作

```ts
Trigger.on(Event.TakeDamage)
  .bind(GI)
  .where(Prop.HealthPercent, Op.Lt, 30)                    // 只在血量低于 30% 时触发
  .whereVar(VarTarget.Subject, Panicked, Op.Eq, 0)         // 并且还没恐慌过
  .immediate(ActionDef.setVar(Target.Subject, Panicked, 1))  // 触发的同时由游戏执行
  .enqueue(giPanic)                                        // 再交给模块处理
  .done();
```

筛选条件由游戏检查，不满足时不会打扰模块，比在回调里自己判断省得多。`.once()` 让触发器只触发一次。

### 可决定事件（同步改写游戏的判断）

在游戏做出判断的那一刻同步调用模块，模块可以改写结果。例如让美国大兵受到的伤害减半：

```ts
function halve(d: TakeDamageDecision): void {
  d.setResult(d.previous / 2);
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi != NO_HANDLE) Trigger.on(Event.TakeDamage).bind(gi).decideSync(halve);
});
```

目前可以决定的有：受到的伤害、能否建造某类型、能否排进生产队列、所属方是否判负、是否接受玩家命令。决定函数里**只能读、不能写**，在里面执行动作或写属性会让模块被停用。

### 给单位下任务、给电脑方下指令

```ts
House.order(computerHouse, Order.moveTo([tank], Cell.at(60, 40)));
House.order(computerHouse, Order.queueProduction(Rules.technoType("HTNK")));
```

`House.order` 只能下给电脑方，在下一帧执行，和玩家点鼠标下的命令走同一套校验。

要指挥**任何一方**的单位，包括玩家自己的，用任务：它以单位所属一方的名义执行，就像那一方的玩家点了鼠标。

```ts
Techno.setMission(tank, Mission.Hunt);
```

### 加载时改规则

```ts
onRules((): void => {
  Rules.set("E1", "Strength", "250");   // 美国大兵血量改成 250
});
```

`onRules` 只在规则读入时运行一次，改动对整局生效，所有玩家的机器上结果一致。对局开始后，`Rules.set` 会被拒绝。

### 其他

```ts
const now = frame();          // 当前帧号，每秒 15 帧
const roll = random(1, 100);  // 确定性随机数，每台机器序列相同
```

## 8. INI 触发器和地图嵌入

这两部分和用什么语言写模块无关，请直接看 Rust 版教程：

- [不写代码：用 INI 声明触发器](wasm-modules-tutorial.zh-CN.md#7-不写代码用-ini-声明触发器)。INI 里的 `Action1=Wasm:OnKill` 调用的就是你用 `namedAction("OnKill", …)` 登记的函数。
- [把模块嵌进地图](wasm-modules-tutorial.zh-CN.md#8-把模块嵌进地图)：`asset wasm-embed 原地图.map build/rules.wasm 新地图.map`。

TypeScript 模块要嵌进地图时，体积更要紧：地图模块最好控制在几十 KB 以内。`npm run build` 默认就是优化体积的发布构建。

## 9. 调试与排错

**看日志**

- `log("...")` 的内容会出现在聊天栏，每帧最多显示 8 行，每行最多 160 个字符。拼接数字用 `"hp=" + hp.toString()`。
- 所有日志都会完整写进游戏日志文件 `logs/ra2.log`，它在第 4.1 节说的 VERA20K 文件夹里。

**模块被停用时**

聊天栏会出现一条红色提示，比如 `Mod module stopped in on_frame: ...`，说明停在哪个函数、为什么停。常见原因：

| 提示里的原因 | 意思 | 怎么办 |
| --- | --- | --- |
| `exceeded its operation budget` | 一帧内运算太多，超出预算 | 把工作分摊到多帧（`new Every(N)`），批量读属性 |
| `trapped: …` | 代码崩溃了：数组越界、对 `null` 取字段、类型转换失败、内存用光 | 访问数组前检查长度，用 `if (x != null)` 判断 |
| `made an invalid call: …` | 接口用错了，比如参数超范围、变量作用域不对，后面会写具体原因 | 对照附录检查参数 |
| `cannot keep up: … event records are waiting` | 事件积压太多，处理不过来 | 减少绑定范围，或加筛选条件 `.where(...)` |

**编译不通过**

AssemblyScript 的报错一般能直接指出问题。最常见的是用了第 6 节表里的写法：缺类型、对象字面量、闭包、`for…of`。先运行 `npx tsc --noEmit` 看类型错误，往往更好懂。

**`wasm-check` 不通过**

| 报错 | 原因 |
| --- | --- |
| 不支持的特性（reference types、SIMD 等） | `asconfig.json` 被改了，从模板里重新复制一份 |
| 导入了 `env.abort` / `env.trace` 等 | 没用 `npm run build`，而是直接运行了 `asc`；或者用了依赖宿主环境的库 |
| 有 `start` 段 | 同上：SDK 的构建会把顶层代码导出成初始化函数，直接用 `asc` 不会 |

**对局开不起来**

一般是 INI 触发器写错了，或者地图要求的版本比游戏新。看开局时的提示，它会写明是哪个段、哪个键。

## 10. 规则与限制

**确定性**

联机时每台机器都跑同一份模块，所以模块的结果必须在每台机器上都一样：

- 需要随机数时用 SDK 的 `random(lo, hi)`，不要用 `Math.random()`。它用的是模块自己的随机数流，每台机器的序列都相同。
- 浮点运算可以用，NaN 会被统一成同一个值。不过模拟相关的数值建议用整数或 `fixed()`。
- 读不到系统时间，`Date.now()` 不可用。需要计时就用 `frame()`，游戏每秒 15 帧。

**资源上限**

| 项目 | 上限 |
| --- | --- |
| 模块大小 | 8 MiB |
| 内存 | mod 模块 32 MiB，地图模块 16 MiB |
| 每帧运算量 | 默认也是最多 200,000；mod 可以在 `[VeraWasm]` 里调低 |
| 启动调用（初始化、`onRules`、`onScenarioStart`） | 每次 5,000,000 |
| 每个模块的触发器 | 256 个 |
| 每次搜索的筛选条件 | 16 个 |
| 每帧给电脑方的指令 | 64 条 |

**存档**

存档会保存模块的内存、变量、修饰器和触发器。读档时不会重新运行模块代码（包括顶层代码），模块从存档那一刻接着运行，顶层变量的值也原样恢复。

**性能建议**

- 名字在开局时查一次，存进顶层变量。
- 一次要读很多值时用批量读（`Techno.read`）。
- 需要扫描全图的工作分摊到多帧，比如每帧只处理一部分对象。
- 少在每帧都运行的代码里新建对象、数组和拼接字符串：每次分配都消耗运算量，也会让垃圾回收变频繁。可以复用顶层的 `StaticArray`。

## 参考

- [Rust 版教程](wasm-modules-tutorial.zh-CN.md)：现在就能编译运行的版本
- [模块指南（英文）](wasm-modules.md)：功能总览和 INI 语法
- [RFC 0001](rfcs/0001-wasm-extension-modules.md)：设计说明，TypeScript SDK 的约定在第 5a 节；[附录](rfcs/0001-wasm-extension-modules-api.md)：全部接口、事件、动作、属性，TypeScript 写法在第 3 节
- [开发进度](wasm-modules-status.md)：当前版本实现了哪些、还缺什么
- RFC 待定问题 2：TypeScript 用 AssemblyScript 编译，还是在模块里嵌一个 JavaScript 引擎。本教程按 AssemblyScript 写；如果最后选了后者，第 6 节的限制大多会取消，但模块会大很多。
