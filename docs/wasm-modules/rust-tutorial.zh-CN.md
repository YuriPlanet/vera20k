# Wasm 模块 Mod 开发教程

[English](rust-tutorial.md) | [简体中文](rust-tutorial.zh-CN.md)

这份教程带你从零开始，用 Rust 给 VERA20K 写一个 Wasm 模块，并放进游戏里试玩。读完后你会做出一个“击杀悬赏”mod：谁打死美国大兵或动员兵，谁就拿 200 块赏金。

- 写给谁：会改 `rulesmd.ini`、想做 INI 做不到的玩法的 modder。用过一点编程语言就够了，Rust 只用到最基础的部分。
- 参考资料：模块放在哪、有哪些限制见[模块指南](guide.zh-CN.md)；SDK 的每个函数见 [Rust SDK 参考](rust-sdk.zh-CN.md)；完整设计见 [RFC 0001](../rfcs/0001-wasm-extension-modules.zh-CN.md)；每个函数、事件、动作、属性的编号与含义见[附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)。
- 想做界面（按钮、面板）？先读完这份，再看[客户端模块教程](rust-ui-tutorial.zh-CN.md)。
- 预览版提醒：接口还在 `0.x` 预览阶段，名字和编号在正式版前可能会变。游戏会在模块需要更新的接口时直接告诉你。
- 想用 TypeScript？见 [TypeScript 版教程](ts-tutorial.zh-CN.md)。

## 目录

1. [模块是什么](#1-模块是什么)
2. [准备环境](#2-准备环境)
3. [第一个模块：在聊天栏说一句话](#3-第一个模块在聊天栏说一句话)
4. [放进游戏试玩](#4-放进游戏试玩)
5. [做一个真正的玩法：击杀悬赏](#5-做一个真正的玩法击杀悬赏)
6. [常用功能速查](#6-常用功能速查)
7. [不写代码：用 INI 声明触发器](#7-不写代码用-ini-声明触发器)
8. [把模块嵌进地图](#8-把模块嵌进地图)
9. [调试与排错](#9-调试与排错)
10. [规则与限制](#10-规则与限制)

## 1. 模块是什么

模块是一个编译好的 WebAssembly 文件。这份教程做的是**模拟模块**，游戏里叫 `rules.wasm`：它和对局一起运行，每个玩家的机器上都跑同一份，所以联机也不会不同步。（另一种是**客户端模块** `ui.wasm`，只在本机运行、负责界面，见[客户端模块教程](rust-ui-tutorial.zh-CN.md)。）

模块能做的事：

- **读游戏状态**：单位的血量、位置、所属方，所属方的钱和电力，地格的地形，规则里的数值……
- **改游戏**：只能通过游戏自己的规则来改，比如执行动作（伤害、造单位、给钱、发射超级武器）、写属性、加修饰器（攻击力、护甲、速度倍率）、给任何一方的单位下任务（包括玩家的）、给电脑方下指令，以及原版触发器动作。
- **响应事件**：用触发器监听受伤、被击杀、开火、出生等事件，可以在代码里创建，也可以在 INI 里声明。
- **改写判断结果**：比如受到的伤害、能不能造某个单位、要不要接受某条命令（“可决定事件”）。
- **加载时改规则**：在规则读入时修改一次（`on_rules`）。

模块不能做的事：

- 读写文件、访问网络、读系统时间。它运行在沙箱里，碰不到你的电脑。
- 让游戏崩溃。模块出错、死循环或违反接口时，所有机器会在同一时刻停用它，对局继续进行，聊天栏会提示是哪个模块、为什么停了。

模块有两个来源，可以同时存在。mod 的模块先运行，地图的模块后运行。

| 来源 | 放在哪 | 适合 |
| --- | --- | --- |
| mod | mod 文件夹里的 `rules.wasm` | 整个 mod 通用的玩法 |
| 地图 | 嵌在地图文件里（用 `asset wasm-embed`） | 某张地图专属的任务、关卡逻辑 |

## 2. 准备环境

### 2.1 安装 Rust

到 <https://rustup.rs> 按提示安装 Rust，然后加上 WebAssembly 编译目标：

```sh
rustup target add wasm32-unknown-unknown
```

### 2.2 获取 VERA20K 源码

SDK 和示例都在 VERA20K 源码里：

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
```

这里面你会用到：

| 路径 | 是什么 |
| --- | --- |
| `sdk/rust/vera20k-sdk` | Rust SDK |
| `sdk/examples/` | 示例模块，本教程的完整代码在 `tutorial-bounty` |

### 2.3 编译 `asset` 工具

`asset` 工具用来检查模块，以及把模块嵌进地图。它的 Wasm 相关命令不需要安装游戏：

```sh
cargo build --release --bin asset
# 编译好的程序在 target/release/asset（Windows 上是 asset.exe）
```

## 3. 第一个模块：在聊天栏说一句话

### 3.1 建项目

最省事的办法是复制一个示例。下面在 `sdk/examples` 旁边新建一个 `my-bounty`：

```sh
cp -R sdk/examples/tutorial-bounty sdk/examples/my-bounty
```

项目里有三个文件，作用如下：

```text
my-bounty/
├── .cargo/config.toml   编译目标和允许的 Wasm 特性，必须保留
├── Cargo.toml           项目名和对 SDK 的依赖
└── src/lib.rs           模块代码
```

在 `Cargo.toml` 里把 `name` 改成你自己的名字。如果项目不在 `sdk/examples` 下，还要改依赖路径，让它指向 SDK：

```toml
[dependencies]
vera20k-sdk = { path = "../../rust/vera20k-sdk" }
```

`.cargo/config.toml` 一定要保留。它让编译器只生成游戏接受的 WebAssembly 特性，去掉了游戏会拒绝的引用类型和 SIMD 等（RFC 第 3 节）。少了它，`wasm-check` 会报“不支持的特性”。

### 3.2 写代码

把 `src/lib.rs` 换成最简单的模块：

```rust
#![no_std]                       // 模块不带标准库，体积小、启动快

use vera20k_sdk::prelude::*;     // SDK 的常用功能一次引入

fn start() {
    log("Hello from my first module!");
}

// 告诉游戏：对局开始时调用 start
vera20k_sdk::on_scenario_start!(start);
```

`log` 写的内容会出现在游戏聊天栏，同时写进游戏日志文件。

### 3.3 编译和检查

```sh
cd sdk/examples/my-bounty
cargo build --release
```

编译结果在 `target/wasm32-unknown-unknown/release/my_bounty.wasm`（项目名里的 `-` 会变成 `_`）。把它改名为 `rules.wasm`，再用游戏的加载检查跑一遍：

```sh
asset wasm-check rules.wasm
# 通过时输出：{"bytes":…,"ok":true}
```

没通过时，它会告诉你游戏为什么不接受这个模块。

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
└── rules.wasm    你的模块
```

`mod.ini` 示例：

```ini
[Mod]
Name=My Bounty
Version=1
About=Credits for every infantry kill.
```

只加模块的话，不要往 mod 里放 `rulesmd.ini`：mod 里同名的文件会整个替换原版的文件。需要改规则时，请看[第 7 节](#7-不写代码用-ini-声明触发器)。

### 4.3 启动

有两种方式：

- 在游戏里打开 **Options → Mods**，选中你的 mod，游戏会重启进入它；
- 或者用命令行启动：`vera20k --mod MyBounty`。

然后开一局遭遇战。开局时聊天栏应该出现 `Hello from my first module!`。

## 5. 做一个真正的玩法：击杀悬赏

目标：谁打死美国大兵（`E1`）或动员兵（`E2`），谁就得到 200 块。完整代码在 `sdk/examples/tutorial-bounty/src/lib.rs`：

```rust
#![no_std]

use vera20k_sdk::prelude::*;

// ① 给模块里的处理函数起名字，并分配一个分发键
vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1 }

fn start() {
    log("Bounty: 200 credits for every GI or conscript killed.");
    // ② 创建触发器：监听“被击杀”事件
    let mut trigger = Trigger::on(Event::KILLED);
    // ③ 绑定到单位类型：这类单位全部生效，包括之后才造出来的
    for name in ["E1", "E2"] {
        if let Ok(infantry) = TypeRef::techno_type(name) {
            trigger = trigger.bind(infantry);
        }
    }
    // ④ 触发时把事件交给 ON_KILL；done() 才真正创建
    if trigger.enqueue(ON_KILL).done().is_err() {
        log("Bounty: the trigger was refused");
    }
}

// ⑤ 处理一次击杀：给凶手的所属方加钱
fn on_kill(event: KilledEvent) {
    let Ok(house) = event.killer().owner() else {
        return; // 凶手可能在同一帧也死了
    };
    if Action::add_money(house, 200).is_ok() {
        log("Bounty: +200 credits");
    }
}

vera20k_sdk::on_scenario_start!(start);
// ⑥ 把 ON_KILL 的事件派给 on_kill
vera20k_sdk::on_events!(ON_KILL => on_kill);
```

逐段说明：

1. **`named_actions!`** 声明模块自己的“动作”。每个动作有一个名字（`"OnKill"`）和一个分发键（`1`）。名字用于在 INI 里用 `Wasm:OnKill` 调用，键用于在代码里分发事件。一个模块只能写一个 `named_actions!`，所有动作都写在里面。
2. **`Trigger::on(Event::KILLED)`** 开始描述一个触发器。触发器就是“发生某个事件时做某件事”，和地图编辑器里的触发器一样。
3. **`bind(...)`** 决定触发器管哪些对象。绑到类型（`TypeRef::techno_type("E1")`）时，所有该类型的单位都受影响，包括后来造出的；也可以绑到某一个具体单位。
4. **`enqueue(ON_KILL)`** 表示触发时把事件记录交给模块。记录会在每帧的模块阶段统一投递。**`done()`** 才真正创建触发器，失败时返回错误。
5. **事件视图** `KilledEvent` 把事件记录包装成有名字的字段：`victim()`（死者）、`killer()`（凶手）、`position()`（位置）。`owner()` 读出单位的所属方。
6. **`on_events!`** 把不同键的事件分派给对应函数。

编译、改名、检查，然后替换 mod 里的 `rules.wasm`，再开一局：每打死一个大兵或动员兵，聊天栏出现 `Bounty: +200 credits`，钱也跟着涨。

> 本节代码已用 SDK 编译，并通过了 `asset wasm-check`。如果对局里看到的效果和描述不一致，请反馈。

### 想再加点什么？

- **每 30 秒发一次工资**：写个函数 `pay()`，用 `vera20k_sdk::on_frame!(every = 450, pay);` 注册。游戏每秒 15 帧，450 帧就是 30 秒。可以参考 `sdk/examples/playtest`。
- **只奖励人类玩家**：在 `on_kill` 里先读所属方的 `IsHuman` 属性：`vera20k_sdk::get(house, Prop::IsHuman) == Ok(1)`。
- **被打残的单位受伤减半**：用“可决定事件”，见下一节。

## 6. 常用功能速查

下面的写法都假设已经 `use vera20k_sdk::prelude::*;`。SDK 函数一般返回 `Result`：失败时的 `Err(Status)` 说明原因，比如 `Gone`（对象已经不存在）、`NotFound`（名字不存在）、`Refused`（游戏拒绝）、`Unsupported`（这个版本还没有）。

### 生命周期

```rust
vera20k_sdk::init!(f);                  // 模块加载时运行一次
vera20k_sdk::on_rules!(f);              // 读规则时运行一次，可以改规则
vera20k_sdk::on_scenario_start!(f);     // 对局开始时运行一次
vera20k_sdk::on_frame!(f);              // 每帧运行
vera20k_sdk::on_frame!(every = 15, f);  // 每 15 帧（1 秒）运行一次
vera20k_sdk::on_events!(KEY => f);      // 处理触发器投递的事件
vera20k_sdk::on_decide!(KEY => f);      // 处理可决定事件
vera20k_sdk::on_message!(f);            // 处理客户端模块发来的消息
```

每个宏在一个模块里最多用一次。

### 名字和引用

模块用“引用”指代游戏里的东西：`Techno`（单位或建筑）、`House`（所属方）、`Cell`（地格）、`TypeRef`（类型）、`Waypoint`（路径点）。名字只在开局时查一次，查到后保存下来：

```rust
let gi = TypeRef::techno_type("E1")?;        // 单位类型
let warhead = TypeRef::warhead("Super")?;    // 弹头
let cell = Cell::at(50, 60);                 // 地格坐标
```

### 读属性

```rust
let hp = vera20k_sdk::get(unit, Prop::Health)?;   // 任意属性
let money = vera20k_sdk::get(house, Prop::Money)?;
let hp = unit.health()?;                          // 常用属性有快捷函数
let owner = unit.owner()?;
```

全部属性见[附录第 7 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#7-属性)。一次要读很多对象的属性时用 `get_many`，比逐个读快。

### 搜索

```rust
// 第一个属于 house 的美国大兵
let first = Find::objects().owned_by(house).of_type(gi).first();

// 所有人类玩家（分页读，每页最多 page 的长度个）
let mut page = [0i64; 16];
let (count, next) = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).page(&mut page, 0)?;
```

搜索还可以用 `near(cell, 半径)`（半径单位是 lepton，256 lepton 为一格）和 `within(cell, 宽, 高)` 限定范围。结果按创建顺序排列，`next` 不为 0 时说明还有下一页（哪怕这一页没装满，甚至是空的）。一个 `Find` 最多带 8 个筛选条件。

### 动作

```rust
Action::add_money(house, 500)?;                     // 给钱（负数是扣钱）
Action::damage(unit, 50, warhead)?;                 // 用某个弹头造成伤害
Action::heal(unit, 50)?;                            // 治疗
Action::kill(unit, Techno(NO_HANDLE))?;             // 击杀，第二个参数是记功的凶手
let tank = Action::create_object(cell, rhino, house)?;  // 在地格上造一个单位
unit.set_mission(Mission::Hunt)?;                   // 改任务
```

原子动作和原版触发器动作的完整列表见[附录第 5 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#5-动作种类)。

### 修饰器（临时改属性倍率）

```rust
// 攻击力 ×2，持续 150 帧（10 秒）；frames 写负数表示永久
let boost = Modifier::multiply(unit, Prop::FirepowerMultiplier, fixed(2.0), 150)?;
boost.remove()?;   // 提前移除
```

可用的有攻击力、护甲、速度、射速、射程倍率和射程加成，还有可隐形、强制显形、禁用武器；转向速度（`ROTMultiplier`）和免疫（`ImmunityFlags`）是 `preview`；暴击和不死只在带 Phobos 的版本里生效。它们和 Phobos 的 AttachEffect 叠加，没有修饰器时行为和原版完全一致。

### 变量

```rust
// 原版的局部变量和全局变量
Var::local(3).set(1)?;
let on = Var::global(5).get();

// 模块自己声明的变量：全局、所属方、单位三种作用域，会存进存档
vera20k_sdk::vars! {
    static WAVE: GlobalVar = ("Wave", Public, 1);
    static PANICKED: ObjectVar = ("Panicked", Private, 0);
}
WAVE.set(2)?;
PANICKED.set(unit, 1)?;
```

一个模块只能写一个 `vars!`，所有变量都写在里面。

### 可决定事件（同步改写游戏的判断）

在游戏做出判断的那一刻同步调用模块，模块可以改写结果。例如让某类单位受到的伤害减半：

```rust
const HALVE: i32 = 1;

fn start() {
    if let Ok(gi) = TypeRef::techno_type("E1") {
        let _ = Trigger::on(Event::TAKE_DAMAGE).bind(gi).decide_sync(HALVE);
    }
}

fn halve(mut d: TakeDamageDecision) {
    d.set_result(d.previous() / 2);
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_decide!(HALVE => halve);
```

可以决定的有：受到的伤害、能否建造某类型、能否排进生产队列、所属方是否判负、是否接受玩家命令，以及开火、反击、目的地、碾压、采哪一格矿、能否展开、闲置时的任务、移动和寻路等（后面这些多数是 `preview`），完整列表见 [SDK 参考](rust-sdk.zh-CN.md)。决定函数里**只能读、不能写**（`log` 可以用），在里面执行动作、写属性或变量会让模块被停用。

一个模块同时用 `on_events!` 和 `on_decide!` 时，后写的那个要加 `shared_delivery:`，例如 `vera20k_sdk::on_decide!(shared_delivery: HALVE => halve);`，因为两个宏都会声明投递区，而一个模块只能有一个。

### 给单位下任务、给电脑方下指令

```rust
let units = [tank];
computer_house.order(&Order::move_to(&units, Cell::at(60, 40)))?;
```

`House::order` 只能下给电脑方，在下一帧执行，和玩家点鼠标下的命令走同一套校验。一条指令最多带 64 个单位。

要指挥**任何一方**的单位，包括玩家自己的，用任务：它以单位所属一方的名义执行，就像那一方的玩家点了鼠标。

```rust
tank.set_mission(Mission::Hunt)?; // 写属性，不带目标
Action::set_mission(tank, Mission::Move, Cell::at(60, 40))?; // 动作，带目标格
```

### 加载时改规则

```rust
fn rules() {
    let _ = Rules::set("E1", "Strength", "250");   // 美国大兵血量改成 250
}
vera20k_sdk::on_rules!(rules);
```

`on_rules` 只在规则读入时运行一次，改动对整局生效，所有玩家的机器上结果一致。对局开始后，`Rules::set` 会被拒绝。

## 7. 不写代码：用 INI 声明触发器

只用原版动作和模块里已有的函数时，可以直接在 mod 的 `rulesmd.ini` 里声明触发器，不用改代码。

**先准备好 mod 的 `rulesmd.ini`**：mod 里的 `rulesmd.ini` 会整个替换原版的，所以要从完整副本开始改。在游戏里点 **Options → Mods → New Mod** 可以新建一个带原版文件的 mod，详见 [docs/modding.md](../modding.md)。

例子：美国大兵血量掉到 30% 以下时，自动回 50 血，每个大兵只触发一次。

```ini
[E1]
Wasm.Tags=GIHealTag        ; 把标签挂到美国大兵类型上

[GIHealTag]
Trigger=GIHeal             ; 这个标签带哪些触发器，逗号分隔

[GIHeal]
Event=TakeDamage
Where1=Subject.HealthPercent<30
Action1=Heal(Subject,50)
Repeat=OneTimeOr
```

要调用模块里的函数，写 `Action1=Wasm:OnKill`，这里的名字就是 `named_actions!` 里起的名字。

触发器段可以用的键：

| 键 | 写法 |
| --- | --- |
| `Event`、`Event<n>` | 事件名，或 `名字(op,interval,id,value)`；多个事件要同时满足 |
| `Where<n>` | `Subject.`、`Other.` 或 `House.` 加属性名，或 `Var(作用域,变量名)`，再加比较符（`=`、`!=`、`<`、`<=`、`>`、`>=`）和数字 |
| `Action`、`Action<n>` | `Wasm:名字`，或 `动作名(目标,参数…)` |
| `Decide` | `Wasm:名字`，决定处理函数 |
| `House` | 国家名；留空表示触发对象自己的所属方 |
| `Difficulty` | `Easy`、`Normal`、`Hard`，逗号分隔 |
| `Repeat` | `OneTimeOr`（默认）、`OneTimeAnd`、`Repeating` |

写错时，对局不会开始，提示会指出是哪个段的哪个键出了问题。完整语法见[模块指南](guide.zh-CN.md)。

地图不能用 `Wasm.Tags` 绑定类型，地图里写了会被忽略，开局时会有提示。地图的模块要在代码里绑定。

## 8. 把模块嵌进地图

地图可以自带模块，玩家选这张地图时就会加载它：

```sh
asset wasm-embed 原地图.map rules.wasm 新地图.map   # 写入模块
asset wasm-check 新地图.map                         # 检查地图里的模块
asset wasm-extract 新地图.map 取出来的.wasm         # 把模块取出来
```

`wasm-embed` 会自动写好 `[VeraWasm] Api=` 和 `RulesSize=`。还建议在地图的 `[Basic]` 里加上 `VeraMinVersion=`，写你制作时用的 VERA20K 版本。这样旧版本的游戏会明确拒绝这张地图，而不是悄悄地在没有模块的情况下开局。

`wasm-embed` 只写模拟模块（`rules.wasm`）。地图的客户端模块（`ui.wasm`）目前还没有工具能嵌进去，暂时请放在 mod 里。

把新地图放进 VERA20K 文件夹下的 `Maps` 里，就能在地图列表里选到它。

mod 可以控制地图模块，在 mod 自己的 `rulesmd.ini` 里设置：

```ini
[VeraWasm]
MapWasm=no        ; 不允许带模块的地图在这个 mod 下开局
MapBudget=100000  ; 地图模块每帧可用的运算量（默认 100000，最多 500000）
```

## 9. 调试与排错

**看日志**

- `log("...")` 的内容会出现在聊天栏，每帧最多显示 8 行，每行最多 160 个字符。
- 所有日志都会完整写进游戏日志文件 `logs/ra2.log`，它在第 4.1 节说的 VERA20K 文件夹里。

**模块被停用时**

聊天栏会出现一条红色提示，比如 `Map module stopped in on_frame: ...`，说明停在哪个函数、为什么停。常见原因：

| 提示里的原因 | 意思 | 怎么办 |
| --- | --- | --- |
| `exceeded its operation budget` | 一帧内运算太多，超出预算 | 把工作分摊到多帧（`every = N`），用 `get_many` 批量读 |
| `trapped: …` | 代码崩溃了（数组越界、`panic!`、`unwrap` 失败） | 用 `if let` / `match` 处理 `Err` |
| `made an invalid call: …` | 接口用错了，比如参数超范围、变量作用域不对、在决定函数里写东西，后面会写具体原因 | 对照 [SDK 参考](rust-sdk.zh-CN.md)检查参数 |
| `cannot keep up: … event records are waiting` | 事件积压太多，处理不过来 | 减少绑定范围，或加筛选条件 `where_prop` |

**`wasm-check` 不通过**

| 报错 | 原因 |
| --- | --- |
| 不支持的特性（reference types、SIMD 等） | `.cargo/config.toml` 丢了或被改了，从示例里重新复制一份 |
| 导入了 `env` / `wasi_*` | 用了标准库或某个需要系统功能的库。加上 `#![no_std]`，换用不依赖系统的库 |
| 有 `start` 段 | 不要用会生成 start 函数的写法，启动代码放进 `init!` |
| `zero byte expected` | 用了 `core::fmt`（`write!`、`format_args!`、`{:?}`）。预编译的 `core` 里这部分代码带着游戏不接受的指令编码。数字转文字请自己写，可以参考示例里的 `Line` |

**对局开不起来**

一般是 INI 触发器写错了、`[VeraWasm]` 里的预算低于 20,000、地图要求的版本比游戏新，或者 mod 设了 `MapWasm=no` 却选了带模块的地图。看开局时的提示，它会写明是哪个段、哪个键。

## 10. 规则与限制

**确定性**

联机时每台机器都跑同一份模块，所以模块的结果必须在每台机器上都一样：

- 需要随机数时用 SDK 的 `random(lo, hi)`。它用的是模块自己的随机数流，每台机器的序列都相同。
- 浮点运算可以用，NaN 会被统一成同一个值。不过模拟相关的数值建议用整数或 `fixed()`。
- 读不到系统时间。需要计时就用 `frame()`，游戏每秒 15 帧。

**资源上限**

| 项目 | 上限 |
| --- | --- |
| 模块大小 | 8 MiB |
| 内存 | mod 模块 32 MiB，地图模块 16 MiB |
| 每次调用的运算量（预算） | mod 模块默认也是最多 500,000，地图模块默认 100,000；mod 可以在 `[VeraWasm]` 里用 `Budget`、`MapBudget` 调整（最少 20,000，最多 500,000） |
| 启动调用（`init`、`on_rules`、`on_scenario_start`） | 每次 5,000,000 |
| 每个模块的触发器 | 256 个 |
| 每次搜索的筛选条件 | 引擎 16 个，Rust SDK 的 `Find` 8 个 |
| 每帧给电脑方的指令 | 256 条，共 4,096 个单位；每条最多 64 个单位 |

**存档**

存档会保存模块的内存、变量、修饰器和触发器。读档时不会重新运行模块代码，模块从存档那一刻接着运行。

**性能建议**

- 名字在开局时查一次，存进 `static` 变量。
- 一次要读很多值时用 `get_many`。
- 需要扫描全图的工作分摊到多帧，比如每帧只处理一部分对象。

## 参考

- [模块指南](guide.zh-CN.md)：模块放在哪、INI 语法、限制和排错
- [Rust SDK 参考](rust-sdk.zh-CN.md)：SDK 的每个函数、事件、动作
- [客户端模块教程](rust-ui-tutorial.zh-CN.md)：做一个界面按钮（`ui.wasm`）
- [RFC 0001](../rfcs/0001-wasm-extension-modules.zh-CN.md)：设计说明，[附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)：全部接口、事件、动作、属性
- [开发进度](status.md)：当前版本实现了哪些、还缺什么
- 示例模块（`sdk/examples/`）：

  | 示例 | 演示 |
  | --- | --- |
  | `tutorial-bounty` | 本教程：击杀悬赏 |
  | `playtest` | 定时发钱、伤害减半（可决定事件） |
  | `minimal-rules` | 最小模块：给所属方加钱、写变量 |
  | `panic-triggers` | 代码里创建触发器 |
  | `modifier-vars` | 修饰器和自定义变量 |
  | `actions-demo` | 各种动作 |
  | `orders-rules` | 改规则、给电脑方下指令 |
  | `decide-damage` | 可决定事件 |
  | `veteran-fury` | 试玩 mod：击杀悬赏、老兵晋升、残血爆发、敌军定时进攻、存款利息 |
  | `siege-doctrine` | 试玩 mod：加载时改规则，四种可决定事件（生产、排队、命令、判负） |
  | `meteor-storm` | 试玩 mod：地格搜索与属性、计时触发器、弹头与动画、随机数、任务倒计时 |
  | `trigger-lab` | 试玩 mod：出生、开火、载员事件，四种轮询，即时动作、一次性、启用/禁用触发器、批量读 |
  | `ini-shield`、`ini-aura`、`ini-counter`、`ini-academy` | 只用 INI 配置的 mod：模块注册 INI 名字 |
  | `ai-takeover` | 接管电脑所属方，用指令来玩 |
  | `control-bench` | 基准 mod：几乎覆盖所有模拟控制点 |
  | `ui-ping`、`ui-reinforcements` | 客户端模块和模拟模块配合：消息、通知、变量 |
  | `muster-field` | 一个小型自走棋战役，带完整界面 |
