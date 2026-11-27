# 给 mod 和地图用的 Wasm 模块

[English](guide.md) | [简体中文](guide.zh-CN.md)

mod 或地图可以带上小程序，也就是 WebAssembly 模块，用来实现 INI 键写不出来的东西：受伤就慌乱逃跑的单位、首杀奖励、每两分钟一波进攻、有自己打法的电脑对手、屏幕上的商店按钮。这份指南讲模块能做什么、放在哪、怎么编译和发布，以及有哪些限制。

| 文档 | 用途 |
| --- | --- |
| 本指南 | 模块是什么、放在哪、INI 触发器、限制、调试 |
| [教程：模拟模块](rust-tutorial.zh-CN.md)（[English](rust-tutorial.md)） | 一步步做出第一个 `rules.wasm`，从安装 Rust 到在遭遇战里玩上悬赏 mod |
| [教程：客户端模块](rust-ui-tutorial.zh-CN.md)（[English](rust-ui-tutorial.md)） | 第一个 `ui.wasm`：一个购买援军的按钮 |
| [Rust SDK 参考](rust-sdk.zh-CN.md)（[English](rust-sdk.md)） | Rust SDK 的每个函数、事件、动作和限制 |
| [RFC 0001](../rfcs/0001-wasm-extension-modules.zh-CN.md) 及其[附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md) | 设计说明，以及每个导入、事件、动作、属性和枚举及其编号 |
| [开发状态](status.md) | 当前版本实现了什么，写给贡献者 |

这是预览版。在接口 1.0 版之前，名字和编号还可能变；模块需要更新的接口时，游戏会告诉你。可以使用 [Rust SDK](rust-sdk.zh-CN.md) 或 [TypeScript SDK](ts-sdk.zh-CN.md) 编写模块。

## 两种模块

**模拟模块（`rules.wasm`）** 在对局内运行，每个玩家的机器上都跑，且保持同步：

- 它通过属性读游戏（对象的血量、所属方的钱、地格的地形），可以搜索对象、所属方和地格，还能保存自己的变量。
- 它只能通过游戏自己的规则改游戏：动作（伤害、造单位、给钱、发射超级武器）、写属性、修饰器（攻击力、护甲、速度）、给任何单位下任务（玩家的也行）、给电脑所属方下指令，以及地图触发器动作。
- 它通过触发器响应事件，触发器可以在代码里创建，也可以在 INI 里声明；它还能在游戏做决定的当下改写决定（一次命中造成多少伤害、某个单位能不能造、单位往哪开）。
- 它可以在规则加载时改一次规则（`on_rules`），也可以给 modder 提供它自己的 INI 键。

**客户端模块（`ui.wasm`）** 只在本机运行，每画一帧跑一次。它显示的是这个玩家能看到的东西：在对局界面上加按钮、文字、图片、进度条和列表，改动界面布局的各个部分，移动镜头，播放声音，注册热键。它可以给自己的模拟模块发送请求，由模拟模块检查后执行。房间允许自动化时，也可用 `Client::order` 发送标准玩家指令。

两种模块都在沙箱里运行：不能读写文件，不能联网，没有时钟，碰不到你电脑上的任何东西。模拟模块崩溃、死循环或违反接口时，所有机器会在同一时刻停用它；对局继续进行，玩家会收到提示，告诉他们是哪个模块、为什么停了。客户端模块出错时只在那台机器上停用，连同它的节点一起。

## 模块放在哪

| 模块 | 在 mod 里 | 在地图里 |
| --- | --- | --- |
| `rules.wasm` | `Mods/<Mod>/rules.wasm`，和 mod 的 `mod.ini` 放在一起 | 地图文件内的 `[VeraWasmRulesPack]`（由 `asset wasm-embed` 写入） |
| `ui.wasm` | `Mods/<Mod>/ui.wasm` | 地图文件内的 `[VeraWasmUIPack]`（目前还没有工具能写入；见[地图](#地图)） |

mod 的模块先运行，然后才是地图的。两者看到的是同一局游戏。客户端模块和同一来源的模拟模块通信：mod 的 `ui.wasm` 对应 mod 的 `rules.wasm`，地图的对应地图的。

### mod 的 `[VeraWasm]`

mod 在自己的 `rulesmd.ini` 里用 `[VeraWasm]` 控制模块（地图里的这一段不会被读取）。每个键都可以不写：

```ini
[VeraWasm]
Api=1.0            ; mod 的模块所针对的模块接口版本
Budget=500000      ; mod 的 rules.wasm 每帧的运算量（默认值和上限都是 500000）
MapBudget=100000   ; 地图的 rules.wasm 每帧的运算量（默认 100000，上限 500000）
MapWasm=yes        ; no：带 rules.wasm 的地图不能用这个 mod 开局
MapUI=overlay      ; 地图的 ui.wasm 和 [VeraUI] 部件：overlay（默认）、full 或 no
Controller=no      ; yes：由模组的模拟模块接管启动数据中标为“mod AI”的电脑座位（RFC 6i）
SeatBudget=500000  ; 每个 "mod AI" 座位给 mod 预算增加的运算量
```

- 预算低于 20,000 时拒绝开局；高于上限时降到上限。
- `MapUI=overlay`：地图的客户端模块只能改它自己的节点，这些节点画在对局界面上方的覆盖层里。`full`：它可以改任何节点，和 mod 的一样。`no`：不加载地图的 `ui.wasm` 和它的 `[VeraUI]` 部件。
- 玩家可以在 `VERA.INI` 里用 `[VERA] WasmBudget=` 调低 mod 的预算；联机时以房主的值为准。
- 大厅会比对每个玩家的 `rulesmd.ini`，所以这些键对所有人都一样。

mod 的 `rulesmd.ini` 会替换游戏的整个文件。只加模块的 mod 不需要它；需要 `[VeraWasm]` 或 INI 触发器的 mod 要从一份完整的副本开始改（见 [modding](../modding.md)）。

### 地图

- `asset wasm-embed <map> rules.wasm <out map>` 把模块写进地图的一份副本，同时写入 `[VeraWasm] Api=` 和 `RulesSize=`。
- 把 `[Basic] VeraMinVersion=` 设成你做地图时用的 VERA20K 版本。比它旧的版本会拒绝这张地图并给出提示，而不是去掉模块照常游玩。
- `[VeraWasm] StorageId=` 指定地图客户端模块的存储名（默认：地图的文件名）。
- 地图可以在 `[VeraUI]` 里为自己的客户端模块添加界面部件（[界面布局](../interface-layout.md#a-maps-own-parts)）。
- 地图不能把 INI 触发器绑到类型上：地图里的 `Wasm.Tags` 会被忽略，开局时会提示这一点。地图的模块要在代码里绑定触发器。
- 地图的 `ui.wasm` 放在 `[VeraWasmUIPack]`，并配上 `[VeraWasm] UISize=`，格式和规则包一样（分块 LZO 再 base64，每行 70 个字符，行号依次编号）。游戏能读它，但 `asset wasm-embed` 只写规则模块，所以目前请把客户端模块放在 mod 里发布。

## 编译模块

你需要装好 Rust 和 WebAssembly 编译目标：`rustup target add wasm32-unknown-unknown`。

1. 从 VERA20K 源码的 `sdk/examples/` 里复制一个示例：模拟模块用 `minimal-rules` 或 `tutorial-bounty`，一对模块用 `ui-reinforcements`。保留它的 `.cargo/config.toml`：它把编译目标设为 Wasm，并关掉游戏会拒绝的 WebAssembly 特性（引用类型、SIMD 等）。
2. 把 `Cargo.toml` 里的依赖指向 `sdk/rust/vera20k-sdk`。
3. 写代码。一个最小的模拟模块：

   ```rust
   #![no_std]
   use vera20k_sdk::prelude::*;

   fn tick() {
       let _ = Action::add_money(House::from_index(0), 100);
   }

   vera20k_sdk::on_frame!(every = 15, tick);
   ```

4. 运行 `cargo build --release`。模块在 `target/wasm32-unknown-unknown/release/<name>.wasm`（crate 名里的 `-` 会变成 `_`）；把它改名为 `rules.wasm` 或 `ui.wasm`。
5. 检查 `rules.wasm`：`asset wasm-check rules.wasm` 会跑一遍游戏的加载检查，输出 `{"bytes":…,"ok":true}` 或会被拒绝的原因。

`asset` 工具是 VERA20K 的一部分（在源码目录里运行 `cargo build --release --bin asset`）。它的 Wasm 命令不需要安装游戏：

| 命令 | 作用 |
| --- | --- |
| `asset wasm-check <rules.wasm or map>` | 对模拟模块，或地图所带的模拟模块，跑一遍加载检查 |
| `asset wasm-embed <map> <rules.wasm> <out map>` | 把模块写进地图的一份副本 |
| `asset wasm-extract <map> <out.wasm>` | 把地图里的模拟模块取出来 |

`wasm-check` 只认模拟模块：给它一个 `ui.wasm`，它会回答这个模块导入了 `vera:client`。客户端模块在对局加载它时才检查；有问题会显示在聊天栏里，对局不带它继续。

## 放进 mod

```text
Mods/MyMod/
├── mod.ini        mod 的名字和说明
├── rules.wasm     模拟模块（可选）
├── ui.wasm        客户端模块（可选）
└── rulesmd.ini    只有 mod 需要 [VeraWasm]、INI 触发器或其他规则改动时才要
```

```ini
; mod.ini
[Mod]
Name=My Mod
Version=1
About=Credits for every infantry kill.
```

`Mods` 文件夹在 VERA20K 文件夹里：Windows 上在 `vera20k.exe` 旁边，macOS 上是 `~/Library/Application Support/VERA20k/`，Linux 上是 `~/.local/share/VERA20k/`。在 **选项 → Mods** 里选择 mod，或者用 `vera20k --mod MyMod` 启动游戏。启动失败的 mod 在下次启动时会被跳过一次。

联机时每个玩家都需要同一个 mod；大厅会比对。

## 在 INI 里写触发器

mod 可以在自己的 `rulesmd.ini` 里把触发器绑到类型上，不用写代码，并在触发器里按名字调用模块的函数。

```ini
[E1]
Wasm.Tags=GIPanicTag              ; 每个 E1 都带上这个标签的触发器

[GIPanicTag]
Trigger=GIPanic                   ; 一个或多个触发器段，用逗号分隔

[GIPanic]
Event=TakeDamage
Where1=Subject.HealthPercent<30   ; 所有子句都要满足
Where2=Var(Subject,Panicked)=0
Action1=SetVar(Subject,Panicked,1)
Action2=Wasm:giPanic              ; 模块在 named_actions! 里命名的函数
Repeat=Repeating
```

`Wasm.Tags` 写在 TechnoType、武器或弹头段上。写在武器或弹头上时，触发器的每个事件都必须带有它（`Fire`、`Killed`）。抛射体段在游戏能绑定的地方可以用，否则会被拒绝并给出提示。

触发器段的键：

| 键 | 值 |
| --- | --- |
| `Event`、`Event<n>` | `name` 或 `name(op,interval,id,value)`。所有事件都要满足。`id` 是数字；`PropertyPoll` 用属性名，`VariablePoll` 用变量名。`Wasm:name` 是模块事件 |
| `Where<n>`、`Event<k>.Where<n>` | `Subject.`、`Other.` 或 `House.` 加一个属性，或 `Var(Global\|Subject\|Other\|House,name)`，然后是 `=`、`!=`、`<`、`<=`、`>` 或 `>=` 和一个数字。`Where<n>` 属于事件 1 |
| `Action`、`Action<n>` | `Wasm:name`（另一个来源用 `Wasm:mod.name`、`Wasm:map.name`）、`Wasm:name(target,text)`，或 `name(target,parameters…)` |
| `Decide` | `Wasm:name`，一个决定处理函数 |
| `House` | 国家名；留空表示主体的所属方 |
| `Difficulty` | `Easy`、`Normal`、`Hard`，用逗号分隔 |
| `Repeat` | `OneTimeOr`（默认）、`OneTimeAnd`、`Repeating` |

动作的目标是 `Subject`（或留空）、`Other`、`TagHolder`、`Projectile`、`House` 或 `Cell`；用 `Cell` 时先写路径点编号。参数按[附录第 5 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#5-动作种类)里该动作那一行的顺序写，空着的算 0：

- 类型、武器、弹头、超级武器、TeamType、变量、属性、动画、voxel 动画或粒子系统，写名字或编号都行。
- `Mission` 写任务名（`Hunt`、`Guard`、`Wait`……），`EndFor` 写 `Win` 或 `Lose`，`Teleport` 写 `Nearby` 或 `Fail`，`Modify` 写 `Add`、`Multiply` 或 `Override`；写数字也行。`Push(target,cell,distance,frames)` 和 `SetFacing(target,facing)` 写数字。
- 所属方写国家名；地格写路径点编号。
- 声音写它在 `[SoundList]` 里的编号。

写错了对局就开不了，提示里会写明是哪个段、哪个键。

## 模块注册的 INI 名称

模块可以给 modder 提供它自己的 INI 键：modder 写 INI，模块负责实现（[RFC 0001 第 6h 节](../rfcs/0001-wasm-extension-modules.zh-CN.md#6h-ini-注册)）。

```ini
[HTNK]
Kratos.Shield=HeavyShield          ; 类型上的一个键
Wasm.Tags=ShieldMedicTag

[Kratos.ShieldTypes]               ; 注册表：一个由条目段组成的列表
1=HeavyShield

[HeavyShield]                      ; 条目段，由模块读取
Strength=300

[ShieldMedicTag]
Trigger=ShieldMedic

[ShieldMedic]
Event=Wasm:Kratos.ShieldBroken     ; 模块事件
Action1=Wasm:restoreShield(Subject,50)   ; 带目标和文本的模块动作
```

| 注册（Rust） | INI | 模块收到 |
| --- | --- | --- |
| `ini::key(name, IniSection::TechnoType, KEY)` | 在该种类的段里写 `name=value`（`TechnoType`、`Weapon`、`Warhead`、`Projectile`、`SuperWeapon`、`Country`、`General`） | 每个含有这个键的段一条 `IniKeyRecord` |
| `ini::registry(name, KEY)` | 一个列表段 `[name]`，里面是条目段的名字 | 每个条目一条 `IniEntryRecord`，按列表顺序 |
| `ini::event(name, EventScope::Object)` | 触发器里的 `Event=Wasm:name` | 事件的 ID，用 `Action::raise_event` 或 INI 的 `RaiseEvent(target,name,value,value2)` 触发 |

```rust
const SHIELD_TYPES: i32 = 10;
const SHIELD: i32 = 11;
static SHIELD_BROKEN: ini::EventSlot = ini::EventSlot::new();

fn init() {
    let _ = ini::registry("Kratos.ShieldTypes", SHIELD_TYPES);
    let _ = ini::key("Kratos.Shield", IniSection::TechnoType, SHIELD);
    if let Ok(event) = ini::event("Kratos.ShieldBroken", EventScope::Object) {
        SHIELD_BROKEN.set(event);
    }
}

fn load_shield_type(r: IniEntryRecord) {
    let (mut name, mut value) = ([0u8; 64], [0u8; 32]);
    let Ok(section) = r.entry_name(&mut name) else { return };
    let _strength = Rules::get(section, "Strength", &mut value).ok().and_then(ini::int);
    // 存进模块内存
}

fn attach_shield(r: IniKeyRecord) {
    let mut value = [0u8; 64];
    let _shield = r.value(&mut value); // "HeavyShield"，对应类型 r.type_ref()
}

vera20k_sdk::init!(init);
vera20k_sdk::on_events!(SHIELD_TYPES => load_shield_type, SHIELD => attach_shield);
```

- 只能在模块的 `init` 里注册；在别处注册会停用模块。名字是 1 到 63 个 ASCII 字母、数字、`.` 和 `_`，比较时不区分大小写。mod 或地图已经注册过的名字，或以 `Vera.` 开头的名字，会得到 `Err(Refused)`。
- 顺序：mod 的 `init` 和 `on_rules`，地图的规则，地图的 `init` 和 `on_rules`，然后读取 INI 触发器（未注册的 `Event=Wasm:name` 会让对局开不了）。每个模块在 `on_scenario_start` 之前，先收到它所有的 `IniEntry` 记录，再收到所有的 `IniKey` 记录。值是最终合并后的规则文本（mod 和地图），引擎不做检查：你自己解析，值不对时用 `fail(...)` 停下，并写明是哪个段、哪个键。
- 读档时不会再投递一遍：处理函数建好的东西在模块内存里，存档会保存它。

`sdk/examples/ini-shield`、`ini-aura`、`ini-counter` 和 `ini-academy` 都是用这种方式配置的完整 mod，每个都带一份 `rulesmd.ini` 和一份中文 README。

## 调试

**日志。** `log("...")`（客户端模块里是 `client::log`）会写到聊天栏（每帧 8 行，每行 160 个字符），也会写到 VERA20K 文件夹里的 `logs/ra2.log`，target 是 `ext_module`。数字要自己写进字节缓冲区：不能用 `core::fmt`（见下文）。

**模块停了。** 聊天栏会显示一行红字，比如 `Map module stopped in on_frame: ...`，写明是哪个函数、什么原因：

| 原因 | 含义 | 怎么办 |
| --- | --- | --- |
| `exceeded its operation budget` | 一次调用用掉的运算超过了预算 | 把工作分摊到多帧（`on_frame!(every = N, …)`，每帧只处理一部分对象）；读很多值时用 `get_many` |
| `trapped: …` | 代码崩溃了：下标越界、`panic!`、`unwrap` 失败 | 用 `if let` 或 `match` 处理每个 `Err` |
| `made an invalid call: …` | 参数超出范围、变量的作用范围不对、在决定处理函数里写入、在 `init` 之外注册 | 对照 [SDK 参考](rust-sdk.zh-CN.md) 检查这次调用 |
| `cannot keep up: … event records are waiting` | 触发器记录多到模块处理不过来 | 少绑一些对象，加筛选子句（`where_prop`） |

**`wasm-check` 拒绝了模块。**

| 提示 | 原因 |
| --- | --- |
| 不支持的特性（引用类型、SIMD……） | `.cargo/config.toml` 丢了或被改了；从示例里重新复制一份 |
| 导入了 `env` 或 `wasi_*` | 用了标准库，或用了需要系统的 crate。请用 `#![no_std]` 和不做系统调用的 crate |
| 导入了 `vera:client@1.0` | 这是 `ui.wasm`；`wasm-check` 只检查模拟模块 |
| 有 `start` 段 | 把启动代码挪进 `init!` |
| `zero byte expected` | 用了 `core::fmt`（`write!`、`format_args!`、`{:?}`）；它的预编译代码带有游戏拒绝的编码 |

**对局开不了。** 通常是 INI 触发器写错、预算低于 20,000、地图需要更新的 VERA20K，或者在 `MapWasm=no` 的 mod 下用了带模块的地图。提示里会写明是哪个段、哪个键。

## 限制

模拟模块：

| 项目 | 限制 |
| --- | --- |
| 模块大小 | 8 MiB |
| 内存 | mod 的模块 32 MiB（每个 "mod AI" 座位再加 8 MiB），地图的 16 MiB |
| 每次调用的工作量 | 即预算：mod 的模块默认 500,000 运算，地图的 100,000（`[VeraWasm] Budget`、`MapBudget`；最多 500,000，最少 20,000） |
| 每帧的工作量 | 只有当本帧已用量低于预算时，才会开始下一批事件；`on_frame` 总能拿到一整份预算；只有单次调用超出预算才会停用模块。一帧的用量不超过三份预算再加上决定 |
| 决定 | 每次 20,000 运算，从半份帧预算的公共额度里扣（`ProductionAllowed` 用一整份帧预算，`Defeat` 用四分之一） |
| 启动调用（`init`、`on_rules`、`on_scenario_start`） | 每次 5,000,000 运算 |
| 函数 | 20,000 个函数，每个函数体 1 MiB |
| 搜索 | 引擎每次查询 16 个子句，Rust SDK 的 `Find` 是 8 个；每次调用 16,384 个候选 |
| 触发器 | 每个模块 256 个，其中对象轮询 16 个；连锁最深 8 层，每帧最多触发 256 次，其余的排队等待（最多 4,096） |
| 投递 | 每帧 1,024 条记录；积压超过 16,384 条记录会停用模块 |
| 所属方指令 | 每个模块每帧 256 条指令、4,096 个单位条目，每条指令 64 个单位，只限电脑所属方。任务不算在这里，对所有所属方都有效 |
| 通知 | 每条 256 字节，每个模块每帧 256 条 |
| 来自客户端模块的消息 | 每条 96 字节；每个玩家每个来源每帧 2 条，15 帧内 16 条 |
| INI 注册 | 每个模块 4,096 个 |

客户端模块：

| 项目 | 限制 |
| --- | --- |
| 模块大小、内存 | 8 MiB；mod 的 32 MiB，地图的 16 MiB |
| 每个渲染帧的工作量 | 100,000 运算；`client_init` 和 `client_on_start` 每次 5,000,000 |
| 事件 | 每个渲染帧 1,024 个 |
| 节点 | 每个模块最多是界面留给其他来源的节点数的一半 |
| 热键 | 每个模块 256 个 |
| 指令（`Client::order`） | 每个单位每 15 帧一次，所属方范围的指令每 15 帧 4 次；仅限房间允许自动化的情况 |
| 存储 | 键为 1 到 64 个 ASCII 字节，值最大 16 KiB，每个 mod 或地图 256 个键、共 256 KiB |

热点代码应当只查一次名字，读很多值时用 `get_many`，并把工作错开分到多帧。

## 存档与联机

- 存档会保存每个模拟模块的内存、变量、修饰器和触发器；读档时绝不会再运行模块代码，模块从原来的地方接着跑。客户端模块不保存：读档后它们从头开始，读取存档里的变量。
- 联机时每个玩家同步运行同样字节的模块。游戏会把模块内存和对局的其他部分一起校验，每帧校验十五分之一，所以有差异会在 15 帧内发现。
- 确定性：用 `random(lo, hi)`（模块自己的随机数流，每台机器都一样）和 `frame()`（正常速度下每秒 15 帧）；没有时钟。浮点数可以用，NaN 在各处都统一成一样的，但游戏里的数值最好用整数或 `fixed()`。客户端的动画和随机效果在本机运行。
- 浏览器版用浏览器自己的 WebAssembly 引擎运行同样的模块，可以进同样的房间一起玩。

## 示例

都在 VERA20K 源码的 `sdk/examples/` 里；每个都用 `cargo build --release` 编译。

| 示例 | 展示 |
| --- | --- |
| `minimal-rules` | 最小的模块：钱和一个变量 |
| `tutorial-bounty` | [教程](rust-tutorial.zh-CN.md)里的悬赏 mod |
| `playtest` | 每 30 秒发钱，通过决定把伤害减半 |
| `panic-triggers` | 在代码里创建的触发器，不用 `core::fmt` 输出数字 |
| `modifier-vars` | 修饰器和声明的变量 |
| `actions-demo` | 各种动作 |
| `orders-rules` | 在 `on_rules` 里改规则，给电脑所属方下指令 |
| `decide-damage` | 一个决定 |
| `veteran-fury` | 击杀和受伤事件、对象变量和全局变量、修饰器、钱、治疗和造单位、指令 |
| `siege-doctrine` | `on_rules`，以及生产、队列、命令和战败这几个可决定事件 |
| `meteor-storm` | 地格搜索和地格属性、`Timer` 触发器、在地格上放弹头和动画、`random`、任务计时器、闪电风暴 |
| `trigger-lab` | `Birth`、武器上的 `Fire`、`PassengerAdded`、各种轮询、立即动作、`once`、启用和禁用触发器、所属方变量、`get_many` |
| `ini-shield`、`ini-aura`、`ini-counter`、`ini-academy` | 只靠注册的名称、完全在 INI 里配置的 mod |
| `ai-takeover` | 接管一个电脑所属方、用指令替它打的模块（RFC 6i） |
| `control-bench` | 几乎覆盖所有模拟控制点的基准测试 mod（中文 README） |
| `ui-ping` | 最小的一对模块：`ui.wasm` 发一条消息，`rules.wasm` 用一条通知回复 |
| `ui-reinforcements` | 一个买坦克的按钮：消息、检查、变量、通知（[客户端教程](rust-ui-tutorial.zh-CN.md)） |
| `muster-field` | 一个小型自走棋战役，界面完整地做在面板上；文字为简体中文 |

`muster-field` 展示了两个值得照搬的做法。一是通过 `CommandAllowed` 把玩家的命令变成输入：处理函数里不能写入，所以它拒绝这条命令，把它排进模块内存，留给下一次 `on_frame` 处理。二是模拟模块把营地状态发布成私有的所属方变量，只在有变化时写入，这样它的 `ui.wasm` 在读档重启后也能马上显示营地；每个按钮对应营地的一个命令（雇佣、保留、刷新……），点击时发一条两字节的消息，模拟模块把它变成那个命令。它需要一名人类玩家、一个电脑对手，并开启基地（Bases）。
