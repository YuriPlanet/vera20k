# Rust SDK 参考

[English](rust-sdk.md) | [简体中文](rust-sdk.zh-CN.md)

这里列出 Wasm 模块 Rust SDK `vera20k-sdk`（在 VERA20K 源码的 `sdk/rust/vera20k-sdk`）的全部类型、函数和宏。第一部分讲模拟模块（`rules.wasm`），第二部分讲客户端模块（`ui.wasm`）。模块放在哪、有哪些限制，见[模块指南](guide.zh-CN.md)；两份教程分别手把手带你写一个[模拟模块](rust-tutorial.zh-CN.md)和一个[客户端模块](rust-ui-tutorial.zh-CN.md)。

本参考以 SDK 源码为准。文中的编号（属性、事件、动作的 ID）就是 SDK 和本引擎使用的编号；[RFC 附录](../rfcs/0001-wasm-extension-modules-api.zh-CN.md)列出了同样的条目和完整语义。

**级别。** 接口目前是预览版（crate 版本 0.1.0）。RFC 把每个条目分到一个级别：`v1`（版本 1 的候选）、`preview`（现在能用，可能不经通知就改）和 `later`（不在版本 1 里；引擎对 `has_kind` 回答 0，使用时返回 `Unsupported`）。下面凡不是 `v1` 的条目都会注明，其余都是 `v1`，不过在版本 1 定稿前它们也仍可能改动。

## 目录

- [准备](#准备)
- 模拟模块：[导出](#导出)、[基本调用](#基本调用)、
  [引用](#引用)、[属性](#属性)、[修饰器](#修饰器)、
  [变量](#变量)、[搜索](#搜索)、[动作](#动作)、
  [任务与所属方指令](#任务与所属方指令)、[规则](#规则)、
  [触发器](#触发器)、[事件](#事件)、[可决定事件](#可决定事件)、
  [INI 注册](#ini-注册)、[通知与消息](#通知与消息)
- 客户端模块：[概述](#客户端模块)、[客户端导出](#客户端导出)、
  [客户端调用](#客户端调用)、[读取游戏状态](#读取游戏状态)、
  [节点](#节点)、[玩家、镜头与光标](#玩家镜头与光标)、
  [本地动作](#本地动作)、[客户端事件](#客户端事件)、
  [存储](#存储)

## 准备

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
vera20k-sdk = { path = "../../rust/vera20k-sdk" }   # adjust to where the SDK is

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
strip = true
```

请保留示例里的 `.cargo/config.toml`：它让项目编译到 `wasm32-unknown-unknown`，并且只启用游戏接受的 WebAssembly 特性（MVP，加上可变全局变量、符号扩展、不陷入的浮点转整数、批量内存操作和多返回值）。

- 这个 crate 是 `#![no_std]` 的。默认特性 `panic-handler` 提供一个 panic 处理函数，panic 时陷入，只会停掉你的模块。如果你的模块自带 panic 处理函数，就关掉这个特性。
- `use vera20k_sdk::prelude::*;` 引入模拟模块通常要用的东西；客户端模块对应的是 `use vera20k_sdk::client::prelude::*;`。
- 大多数调用返回 `Result<T>`，也就是 `core::result::Result<T, Status>`：

  | `Status` | 含义 |
  | --- | --- |
  | `Gone` | 引用是 0，或者对象已经不存在 |
  | `NotFound` | 没有这个名字或 ID，或者路径点不存在 |
  | `Refused` | 游戏拒绝了：地格被占、这个值现在不能写、配额已满、缓冲区太小 |
  | `Unsupported` | 这局对局或这个版本里没有，或者是级别为 `later` 的条目 |

  调用写错不算 `Err`：无效参数（作用域和变量不符、随机范围是负的、在决定处理函数里写东西……）会直接停掉模块。
- 模块里不要用 `core::fmt`（`write!`、`format_args!`、`{:?}`）：预编译的 `core` 带有加载检查会拒绝的指令编码（`zero byte expected`）。数字请自己写进字节缓冲区，可以参考 `sdk/examples/panic-triggers` 里的 `write_decimal`。

---

# 模拟模块

模拟模块（`rules.wasm`）导入 `vera:sim@1.0`，在每台机器上和对局同步运行。下面的函数如果没写模块路径，都在 crate 根下（`vera20k_sdk::...`）。

## 导出

每个宏导出一个由引擎调用的函数。每个宏在一个模块里最多用一次。

| 宏 | 导出 | 引擎何时调用 |
| --- | --- | --- |
| `init!(f)` | `vera_init` | 最先调用，只调用一次：如果模块也导出了 `on_rules`，在规则阶段调用，否则在对局装载时调用。`ini::*` 注册只能写在这里 |
| `on_rules!(f)` | `on_rules` | 规则读入时调用一次（在 `rulesmd.ini` 之后、规则处理之前）。这里可以用 `Rules::set` 写任何键 |
| `on_scenario_start!(f)` | `on_scenario_start` | 对局开始时调用一次，在模块的 INI 记录之后 |
| `on_frame!(f)` / `on_frame!(every = N, f)` | `on_frame` | 每帧调用，或者只在帧号能被 `N` 整除的帧调用（正常速度下 15 帧是一秒） |
| `on_events!(KEY => f, ...)` | `on_events` | 在模块阶段调用，传入模块的触发器排进队列的记录。每个处理函数接收 `&EventRecord`、一个事件视图（`KilledEvent` 等）或一个 INI 记录视图 |
| `on_decide!(KEY => f, ...)` | `on_decide` | 在引擎的判断点同步调用，用于可决定事件触发器。每个处理函数接收 `&mut DecideRecord` 或一个决定视图 |
| `on_message!(f)` | `on_message` | 在模块阶段、`on_events` 之前调用；同一来源的客户端模块发来的每条消息调用一次；`f(sender: House, data: &[u8])` |
| `named_actions! { NAME = "Name" => key, ... }` | 段 `vera.actions` | 声明模块的具名动作：INI 触发器用 `Wasm:Name` 调用它们，记录以分发键 `key` 送到 `on_events`。同时定义 `pub const NAME: i32 = key` |
| `vars! { static X: GlobalVar = ("Name", Visibility, default); ... }` | 段 `vera.vars` | 声明模块的变量（[变量](#变量)） |
| `notify_names! { pub static X: Notify = "Name"; ... }` | 段 `vera.notify` | 声明模块要发送的通知名（[通知](#通知与消息)） |

- `on_events!`、`on_decide!` 和 `on_message!` 都会顺带声明模块的投递区（段 `vera.delivery`）。一个模块用了其中不止一个时，后面的要加上 `shared_delivery:`，这样投递区只声明一次：

  ```rust
  vera20k_sdk::on_events!(ON_KILL => on_kill);
  vera20k_sdk::on_decide!(shared_delivery: HALVE => halve);
  vera20k_sdk::on_message!(shared_delivery: buy);
  ```

- `named_actions!`、`vars!` 和 `notify_names!` 各写一个自定义段；同一个宏调用第二次会追加第二块数据，引擎会拒绝这个模块。所有内容请写在一次调用里。
- 分发键是你自己定的数字。`named_actions!`、`Trigger::enqueue`、`Trigger::decide_sync` 和 `ini::*` 注册用的键不要重复。
- 不要把处理函数命名为 `on_message`（或其他导出的名字）：宏已经导出了这个符号。

对局开始时的顺序：mod 的 `vera_init` 和 `on_rules`，地图的规则，地图的 `vera_init` 和 `on_rules`；读入 INI 触发器；然后逐个模块投递它的 INI 记录（`on_events`）并调用它的 `on_scenario_start`。每一帧都是 mod 的模块先于地图的模块运行。

## 基本调用

| 函数 | 作用 |
| --- | --- |
| `frame() -> i32` | 当前帧 |
| `random(lo, hi) -> i32` | 从模块自己的随机数流取一个 `lo..=hi` 内的数，每台机器上都相同。`lo > hi` 会停掉模块。在决定处理函数里，它从一个子流里取数，不动主流 |
| `log(line: &str)` | 在聊天栏写一行（每帧最多 8 行，显示 160 个字符），同时写进 `logs/ra2.log`（目标 `ext_module`） |
| `fail(why: &str) -> !` | 记下 `why`，并在所有机器上永久停掉模块。用于无法使用的值，比如格式不对的已注册 INI 键；请写明段名和键名 |
| `lookup(kind: NameKind, name) -> Result<i32>` | `name` 的 ID。名字在开局时查一次，把 ID 保存下来 |
| `name_of(kind, id, buf) -> Result<&str>` | `id` 的名字，写进 `buf`；`buf` 太小时返回 `Err(Refused)` |
| `text(id, buf) -> Result<&str>` | 按字符串 ID（`NameKind::String`）取引擎保存的文本：模块动作的参数文本、INI 值。ID 0 是 `""` |
| `sys::has_kind(table, id) -> i32` | 本引擎实现了表 `table` 的条目 `id` 时返回 1（0 事件种类，1 动作种类，2 属性，3 名字种类，4 搜索范围）。这是原始绑定，SDK 在模拟模块这边没有封装 |

`NameKind` 的取值：`TechnoType`、`Weapon`、`Warhead`、`Projectile`、`Anim`、`VoxelAnim`、`ParticleSystem`、`SuperWeapon`、`Country`、`TeamType`、`Variable`、`ModuleAction`、`String`、`Property`、`ActionKind`、`ModuleEvent`（v1）；`Notify`、`Hotkey`（配合客户端模块）；`Armor`、`Overlay`、`Sound`、`ScriptType`、`AiTrigger`、`EventKind` 已声明，但本引擎对它们回答 `Unsupported`。

## 引用

引用是一个 `i64` 句柄。SDK 给每种引用包了一个类型，都实现了 `Ref` trait，`get`、`set` 和 `find` 都接收它。`NO_HANDLE`（0）表示没有对象；每种引用类型都有 `is_none()`。

| 类型 | 指什么 | 怎么得到 |
| --- | --- | --- |
| `Techno` | 单位或建筑 | 来自搜索、事件或 `Action::create_object` |
| `Projectile` | 飞行中的子弹（preview，7.4） | 来自 `Launch` 或 `ProjectileFrame` 可决定事件 |
| `House` | 所属方（玩家或电脑一方） | `House::from_index(i)`（所属方数组下标）、`techno.owner()`、搜索 |
| `Cell` | 地图上的地格 | `Cell::at(x, y)`；`cell.x()`、`cell.y()` |
| `Waypoint` | 地图上的路径点 | `Waypoint::number(n)` |
| `TypeRef` | 规则里的类型 | `TypeRef::techno_type("E1")`、`::weapon`、`::warhead`、`::projectile`、`::super_weapon`；`TypeRef::new(kind, id)`；`TypeRef::any(kind)`（给 `Find::types` 用的通配）；`ty.id()` |
| `Global` | 全局规则引用（7.8） | `Global` |
| `Modifier` | 修饰器 | `Modifier::new` 等 |
| `TriggerRef` | 触发器 | `Trigger::...done()` |

`anim_id(name) -> Result<i32>` 给出动画的 ID（它在规则动画列表里的位置），`Action::spawn_anim` 和 `Action::attach_anim` 用的就是它。

`House::from_index` 是暂定的：引擎把所属方编码成所属方下标加上第 47 位（RFC 待定问题 5）。

## 属性

| 函数 | 作用 |
| --- | --- |
| `get(reference, prop) -> Result<i64>` | `prop` 的实际值（已计入修饰器） |
| `set(reference, prop, value) -> Result<()>` | 通过游戏自己的代码写 `prop` |
| `get_many(refs: &[i64], props: &[i32], out: &mut [i64]) -> Result<usize>` | 一次调用读出每个引用的 `props`，按行排列（`out[i * props.len() + j]`）。返回第一个失败的状态，读不到的位置写空值 |
| `Prop::named(name) -> Result<Prop>` | 按名字取属性 |

每个属性都是一个常量：`Prop::Health`、`Prop::Money`、`Prop::FirepowerMultiplier`……名字里的 `.` 写成 `_`（`Prop::Passable_Foot` 就是 `Passable.Foot`，`Prop::Factory_Vehicle_Ready` 就是 `Factory.Vehicle.Ready`）。完整列表以及值类型、能否写入和级别见[附录第 7 节](../rfcs/0001-wasm-extension-modules-api.zh-CN.md#7-属性)；编号在 `sdk/rust/vera20k-sdk/src/props.rs` 里。

能用 `set` 写的：`Health`（不记击杀；0 或更小是 `Refused`）、`Owner`、`Rank`、`Mission`、`Ammo`、`Online`，触发器的 `Enabled`、`House` 和 `Repeat`，所属方的 `ControlDomains`，超级武器的 `ChargePercent`。可修饰属性（[修饰器](#修饰器)）拒绝 `set`。

每个快捷函数各做一次 `get`：

| 类型 | 快捷函数 |
| --- | --- |
| `Techno` | `health`、`type_ref`、`owner`、`layer`（`Layer`）、`target`（对象或地格引用）、`experience`（百分比：100 老兵，200 精英）、`storage`（矿包数）、`transport`、`passenger_count`、`transport_space_left`、`disguise_type`、`disguise_owner`、`moving`、`facing`（0..255）、`turret_facing`、`armor`、`mission`、`ammo` |
| `Techno`（写入） | `set_health`、`set_owner(house)`、`set_mission(Mission)`、`set_ammo`（限制在 `Ammo=` 以内）、`set_online(bool)`（建筑） |
| `House` | `money` |
| `Cell` | `land_type`、`height`、`overlay`（`TypeRef`）、`ore`（钱数）、`waypoint`（没有时为 -1）、`passable(SpeedType)` |
| `Global` | `Global::frame`、`difficulty`（0 简单，1 普通，2 困难）、`mission_timer`、`mission_timer_running`、`ambient_light`、`ion_storm_active` |

属性值用到的枚举：`Category`（`Infantry`、`Vehicle`、`Aircraft`、`Building`、`Projectile`）、`Layer`、`Relation`（`SameHouse`、`Ally`、`Enemy`、`Neutral`）、`SpeedType`（`Foot` ... `FloatBeach`，`passable()` 给出对应的地格属性）、`Mission`。

**视角属性。** `Relation`、地格的 `Revealed` 和 `Placeable`、`Visible`、`Engage` 以及 TechnoType 的 `Buildable`，取决于从谁的视角读。直接 `get` 会回答 `Unsupported`。请在触发器的筛选条件里读（以触发器的所属方为视角），或者在带 `Find::seen_by(house)` 的搜索里读。`Placeable` 只能在指定了建筑类型（`of_type`）的 `Find::cells()` 搜索里用。

## 修饰器

修饰器在一段时间内改变对象的某个属性，到时自己消失。不同模块的修饰器和 Phobos 的 AttachEffect 会叠加；没有修饰器时，游戏的计算和原来完全一样。

| 函数 | 作用 |
| --- | --- |
| `Modifier::multiply(target, prop, factor, frames)` | 乘以一个定点数 `factor`（`fixed(1.5)`） |
| `Modifier::add(target, prop, value, frames)` | 加上 `value`（比如 `WeaponRangeExtra`，单位是 lepton） |
| `Modifier::override_(target, prop, value, frames)` | 生效期间替换这个值 |
| `Modifier::new(target, prop, op, value, frames, source, group)` | 通用形式：`ModifierOp`（`Add`、`Multiply`、`Override`）、来源对象（或 0）和互斥组（0 表示无；本模块在同一对象上同一组的第二个修饰器会替换第一个） |
| `modifier.remove()` | 提前移除；已过期时返回 `Err(Gone)` |
| `fixed(x) -> i64`、`FIXED_ONE` | 定点数（16 位小数）：`fixed(1.0) == FIXED_ONE == 65536` |

`frames` 小于 0 时一直生效，直到被移除。用 `get(modifier, Prop::Remaining)` 读修饰器（还有 `Property`、`Op`、`Value`、`Source`、`Group`、`Origin`）；过期的修饰器读出来是 `Gone`。`Action::modify` 和 `Action::remove_modifiers` 是对应的动作形式。

本版本里能用的可修饰属性：

| 属性 | 运算 | 级别 |
| --- | --- | --- |
| `FirepowerMultiplier`、`ArmorMultiplier` | Add、Multiply、Override | v1 |
| `SpeedMultiplier`（地面单位、飞机、喷气兵、火箭） | Add、Multiply、Override | v1 |
| `ROFMultiplier`、`WeaponRangeMultiplier`、`WeaponRangeExtra` | Add、Multiply、Override | v1 |
| `Cloakable`、`ForceDecloak`、`DisableWeapons` | Override | v1 |
| `ROTMultiplier`（载具和建筑车身的转向速度，不含炮塔和飞机） | Multiply | preview |
| `ImmunityFlags`（第 0..6 位：心灵控制、辐射、毒、心灵伤害、超时空、EMP、狂暴） | Override | preview |
| `CritMultiplier`、`CritExtraChance`、`Unkillable` | 见附录 | later；只在 Phobos 版本里起作用 |

附录 7.2 里其余的可修饰属性都是 `later`。

## 变量

**红警 2 变量**：`Var::local(n)`（局部变量 0..99）和 `Var::global(n)`（全局变量 0..49）。`var.get() -> i64`，`var.set(value) -> Result<()>`。

**声明的变量**属于模块自己。它们会存进存档、计入校验哈希，客户端模块能按可见性读取。所有变量都在一个 `vars!` 里声明：

```rust
vera20k_sdk::vars! {
    pub static WAVE: GlobalVar = ("Wave", Public, 0);
    pub static SCORE: HouseVar = ("Score", Allies, 0);
    pub static PANICKED: ObjectVar = ("Panicked", Private, 0);
}

fn example(house: House, unit: Techno) -> vera20k_sdk::Result<()> {
    WAVE.set(WAVE.get()? + 1)?;
    SCORE.set(house, 10)?;
    PANICKED.set(unit, 1)
}
```

| 类型 | 调用 | 作用域 |
| --- | --- | --- |
| `GlobalVar` | `get()`、`set(value)` | 每局一个值 |
| `HouseVar` | `get(house)`、`set(house, value)` | 每个所属方一个值 |
| `ObjectVar` | `get(object)`、`set(object, value)` | 每个对象一个值，跟着对象走 |

可见性（`vars::Visibility`）决定哪些客户端模块能读：`Private`（这个所属方自己的玩家）、`Allies`、`InSight`（只用于对象作用域）、`Public`。名字用 ASCII；其他来源的变量写成 `mod.Name` 或 `map.Name` 来查。每个访问器在第一次使用时查一次自己的 ID。

## 搜索

`Find` 用来拼一个查询；在调用 `page` 或 `first` 之前，什么都不会送到引擎。

| 调用 | 作用 |
| --- | --- |
| `Find::objects()` | 单位和建筑，按创建顺序 |
| `Find::houses()` | 所属方，按下标（不能按区域、类型或所有者筛选） |
| `Find::cells()` | 地格，按地格下标（先 y 后 x）；用 `near` 或 `within` 缩小范围 |
| `Find::types(ty)` | 类型（TechnoType、Weapon、Warhead）：`TypeRef::any(kind)` 列出该种类的所有类型 |
| `.near(center: Cell, radius)` | 离这个地格 `radius` lepton 以内（256 lepton 是一格） |
| `.within(top_left, width, height)` | 在一个地格矩形里 |
| `.of_type(ty)`、`.owned_by(house)` | 按类型和所有者筛选 |
| `.where_prop(prop, op, value)` | 只留下 `prop` 满足比较（`Op::Eq`、`Ne`、`Lt`、`Le`、`Gt`、`Ge`）的候选 |
| `.seen_by(house)` | 以 `house` 的视角读视角属性 |
| `.page(&mut out, after) -> Result<(count, next)>` | 从位置 `after`（第一页是 0）开始填充 `out`；`next` 是下一页的起点，最后一页为 0 |
| `.first() -> i64` | 第一个结果或 `NO_HANDLE`，会自动往后翻页 |

```rust
// 所有人类玩家的所属方，一页 16 个。
let mut humans = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1);
let mut page = [0i64; 16];
let (count, _) = humans.page(&mut page, 0)?;
for house in &page[..count] {
    let _ = Action::add_money(House(*house), 1000);
}
```

- 一个 `Find` 最多放 8 个条件（`MAX_CLAUSES`），多出来的会被丢掉。引擎本身接受 16 个。
- 一次调用最多检查 16,384 个候选；候选的开销超过这次调用一半预算时，检查得更少。所以只有 `next` 为 0 时，才说明这是最后一页，哪怕页面很短甚至是空的；只要 `next != 0` 就继续调用。
- 开销：每个候选 2 运算量，每个条件对每个候选 6 运算量（`Engage`、`NearbyEnemyCount`、`TargetInRange`、`CanFireAtTarget` 再加 60），每个结果 10 运算量。

## 动作

引擎能执行的每个动作在 `Action` 里都有一个函数。每个函数就是一次宿主调用，返回 `Result<()>`（`create_object` 返回新建的 `Techno`）。参数无效会停掉模块；游戏拒绝时返回 `Err(Refused)`。

**原子动作**（附录 5.3）：

| 函数 | 作用 | 级别 |
| --- | --- | --- |
| `damage(target, amount, warhead)`、`damage_from(target, amount, warhead, source)` | 按护甲规则造成伤害；`source` 记击杀、得经验 | v1 |
| `kill(target, source)` | 走伤害流程杀死目标，记功给 `source`（没有时写 `Techno(NO_HANDLE)`） | v1 |
| `remove(target)` | 直接移除，不算死亡，不出幸存者，不记功 | v1 |
| `heal(target, amount)` | 恢复生命值，最多到 Strength，不记功 | v1 |
| `create_object(at, kind, house) -> Techno` | 在地格或路径点上造一个单位或建筑；地格被占时为 `Refused` | v1 |
| `set_mission(target, mission, at)`、`set_mission_on(target, mission, on)` | 下一个指向地格/路径点（没有时写 `Cell(NO_HANDLE)`）或对象的任务 | v1 |
| `teleport(target, to, Blocked::Nearby \| Blocked::Fail)` | 超时空传送一辆载具或一个步兵 | v1 |
| `clear_target`、`clear_disguise`、`clear_mind_control` | 如名字所示 | v1 |
| `add_experience(target, points)` | 加经验：等于击杀一个价值 `points` 的目标所得的经验 | v1 |
| `add_money(house, amount)` | 给钱；负数是扣钱 | v1 |
| `allow_type(house, kind, allow)` | `false` 禁止该所属方建造某个 TechnoType，`true` 解除本模块的禁止 | v1 |
| `set_owner(target, house)` | 把对象交给这个所属方 | v1 |
| `add_ammo(target, amount)` | 不超过类型的 `Ammo=` | v1 |
| `add_passenger(transport, passenger)` | 只要运输工具肯装，立刻登上 | v1 |
| `set_property(target, prop, value)` | `set` 的动作形式 | v1 |
| `modify(target, prop, op, value, frames, group) -> Modifier`、`remove_modifiers(target, group, prop)` | 修饰器的动作形式 | v1 |
| `detonate_weapon(at, weapon, source)` | 在地格或对象上直接引爆一个武器，不产生抛射体 | v1 |
| `detonate_warhead(at, warhead, damage)` | 弹头的范围伤害 | v1 |
| `fire_weapon(shooter, slot)`、`fire_weapon_at(shooter, slot, at)`、`fire_weapon_at_cell(shooter, slot, at)` | 让 0 号或 1 号武器立刻开火；不能开火时（装填中、超出射程）为 `Refused` | v1 |
| `launch_super_weapon(house, kind, at)` | 发射该所属方的超级武器 | v1 |
| `spawn_anim(at, anim)` | 播放一个动画（`anim_id`） | v1 |
| `attach_anim(target, anim)`、`attach_anim_for(target, anim, frames)`、`detach_anim(target, anim)` | 在对象身上播放一次动画，或让它循环 `frames` 帧（负数：直到 `detach_anim`）；`detach_anim` 传 -1 移除本模块在它身上保持的全部动画 | preview |
| `end_for(house, Outcome::Win \| Outcome::Lose)` | 让某个所属方结束对局 | v1 |
| `set_score(house, n)`、`add_score(house, n)` | 分数 | v1 |
| `log(line)` | `log` 的动作形式 | v1 |
| `convert(target, kind)`、`convert_for(target, kind, frames)` | 改变对象的类型，永久或一段时间 | v1 |
| `raise_event(event, target, other, value, value2)`、`raise_event_with_house(...)` | 触发一个模块事件（[INI 注册](#ini-注册)）；不能在决定处理函数里用 | v1 |
| `push(target, toward: Cell, distance, frames)` | 在地面单位原有移动之外，把它朝一个地格推 `distance` lepton（负数是推离），用 `frames` 帧（0：每 64 lepton 一帧）；0 结束推动 | preview |
| `set_facing(target, facing)` | 把车身转到 0..255 | preview |

**原版动作**（地图触发器动作，附录 5.2）的函数按作用命名：`reveal_all`、`play_speech("EVA...")`、`show_text(csf)`、`play_anim_at(waypoint, anim)`、`create_team(team)`、`reinforce_at(team, waypoint)`、`nuke_strike(waypoint)`、`lightning_storm_strike(waypoint, house)`、`create_building_at(waypoint, kind, house)`、`set_tech_level(kind, level)`、`super_weapon_set_charge(house, kind, percent)`、`change_house(target, house)`、`sell_building(target)`、`timer_set(seconds)`、`set_global(n)`、`force_trigger(id)`，还有大约 70 个；每个函数的文档注释写着它对应的原版动作编号。TeamType 用 `TypeRef::new(NameKind::TeamType, lookup(NameKind::TeamType, name)?)` 得到。

`Action::run(kind, target, &params)` 和 `Action::run_text(kind, target, text)` 可以用原始的 `sys::ActionParams` 执行任意编号的动作；`Action::FIRE_WEAPON` ... `Action::SET_FACING` 是原子动作的编号。

## 任务与所属方指令

指挥单位有两种方式：

- **任务**对任何所属方都有效，不管是人类还是电脑，效果和那一方自己点鼠标一样：`techno.set_mission(Mission::Hunt)`（不带目标）或 `Action::set_mission(unit, Mission::Move, Cell::at(60, 40))`。任务会经过该所属方的 `CommandAllowed` 可决定事件，但模块自己的那个除外。
- **所属方指令**用玩家命令一次指挥电脑方的许多单位，也能管生产：`house.order(&Order::...)`。指令在下一帧执行，按那一方玩家的输入来校验。对人类玩家的所属方会返回 `Refused`。

```rust
let units = [tank];
computer.order(&Order::attack_move(&units, Cell::at(60, 40)))?;
computer.order(&Order::queue_production(TypeRef::techno_type("HTNK")?))?;
```

`Order` 的构造函数：`move_to`、`move_to_object`、`stop`、`attack`、`attack_move`、`guard`（一个对象，或者 `Techno(NO_HANDLE)` 表示原地警戒）、`deploy_mcv`、`set_rally`、`queue_production(ty)`、`cancel_production(ty)`、`place_ready_building(ty, cell)`、`sell`、`launch_super_weapon(ty, cell)`、`take_product(kind)`（2 步兵，3 载具，4 海军，5 飞机），以及适用于任意 `Command` 的 `Order::new(command, target, value, units)`。`EjectBunker` 是 `later`。

- 一条 `Order` 最多带 64 个单位（`ORDER_UNITS`）；一个模块每帧最多发 256 条指令、共 4,096 个单位条目。任务不计入这个限制。
- 被拒绝和已执行的指令可以用 `Commanded` 和 `OrderRefused` 事件观察。

## 规则

| 函数 | 作用 |
| --- | --- |
| `Rules::get(section, key, buf) -> Result<&str>` | `[section] key` 合并后（mod 和地图）的规则文本；键不存在时为 `NotFound` |
| `Rules::set(section, key, value)` | 写 INI 文本。在 `on_rules` 里可以写任何键，和其余规则一样处理，每台机器上都一样。对局进行中回答 `Refused` |

```rust
fn rules() {
    let _ = Rules::set("E1", "Strength", "250");
}
vera20k_sdk::on_rules!(rules);
```

## 触发器

触发器就是“当这个事件发生在绑定的对象上时，检查这些条件，然后执行这些动作”。`Trigger` 在模块内存里拼出一个触发器，`done()` 才真正创建它。

```rust
vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1 }

fn start() {
    let mut trigger = Trigger::on(Event::KILLED);
    for name in ["E1", "E2"] {
        if let Ok(infantry) = TypeRef::techno_type(name) {
            trigger = trigger.bind(infantry);
        }
    }
    if trigger.enqueue(ON_KILL).done().is_err() {
        log("the trigger was refused");
    }
}
```

| 构造调用 | 作用 |
| --- | --- |
| `Trigger::on(kind)` | 针对事件 `kind` 的重复触发器，所有难度都有效 |
| `Trigger::every(frames)` | `Timer` 触发器，从创建起每 `frames` 帧触发一次 |
| `Trigger::on_property(prop, op, value, interval)` | `PropertyPoll`：`prop op value` 变为真时触发，每 `interval` 帧检查一次 |
| `Trigger::on_variable(var, op, value, interval)` | `VariablePoll` |
| `Trigger::on_house_power(op, value, interval)` | `HousePowerPoll`（触发器所属方的发电量减去耗电量） |
| `Trigger::on_modifier_count(group, op, count, interval)` | `ModifierCountPoll` |
| `Trigger::on_distance(cell, op, leptons, interval)` | `DistancePoll` |
| `Trigger::on_module_event(event)` | 一个模块事件（[INI 注册](#ini-注册)） |
| `.and(kind)` | 再加一个事件；所有事件都要发生 |
| `.value(v)`、`.id(id)` | 最后加的那个事件的参数（原版事件的所属方下标或数量、类型 ID） |
| `.where_prop(prop, op, value)`、`.where_other(..)`、`.where_house(..)`、`.where_var(var, op, value)` | 最后加的那个事件的筛选条件：针对主体、另一个对象、触发器的所属方、全局变量 |
| `.bind(reference)` | 一个对象、一个 TechnoType（该类型的所有对象，包括之后造出的）、一个武器、弹头或抛射体类型（用于 `Fire`），或者一个地格（原版事件 25 和 26） |
| `.house(house)` | 触发器的所属方，代替主体的所有者 |
| `.once()` | 每个实例只触发一次 |
| `.disabled()` | 创建时处于禁用状态 |
| `.immediate(ActionDef)` | 触发时立即执行一个动作 |
| `.enqueue(key)`、`.enqueue_with(key, target, text)` | 把一条记录排进队列，以分发键 `key` 交给 `on_events` |
| `.done() -> Result<TriggerRef>` | 创建并绑定触发器 |
| `.decide_sync(key) -> Result<TriggerRef>` | 把它创建为可决定事件触发器（[可决定事件](#可决定事件)）；它不带动作 |

- 一个构造器能放 4 个事件、每个事件 4 个条件、8 个动作和 8 个绑定。任何一项多出一个，`done()` 都会回答 `Refused`，什么也不创建；更多的绑定请之后用 `TriggerRef::bind` 加。
- 一个模块最多 256 个触发器，其中对象轮询最多 16 个。
- 记录在模块阶段投递，每次 `on_events` 调用最多 16 条，每帧最多 1,024 条。积压超过 16,384 条会停掉模块。
- 连锁：由触发器自己的动作引起的触发最多嵌套 8 层，每帧最多 256 次；其余的等到下一帧（最多 4,096 个在等）。

`TriggerRef`：`bind(reference)`、`unbind(reference)`、`remove()`、`set_enabled(bool)`。

**`ActionDef`**（即时动作）：`add_money(amount)`（触发器的所属方）、`add_money_to(house, amount)`、`set_var(var, value)`、`heal(target, amount)`、`set_property(target, prop, value)`、`log(line)`、`raise_event(target, event, value, value2)`、`module(key)`、`module_with(key, target, text)`，以及适用于任意动作的 `ActionDef::new(kind, target, params)`。`Target` 说明动作作用于谁：`Subject`、`Other`、`TagHolder`、`Projectile`、`House`、`Cell`。

## 事件

事件种类是 `Event` 的常量。投递来的事件以 `EventRecord` 的形式到达 `on_events`；事件视图用有名字的字段来读它。

`EventRecord` 的方法：`trigger()`、`subject()`、`other()`、`holder()`、`house()`、`args()`、`args_text(buf)`、`target()`；原始字段 `key`、`kind`、`projectile`、`modifier`、`cell`、`value`、`value2`、`id`、`frame`、`x`、`y`、`z`。

| 事件 | 视图：字段 | 级别 |
| --- | --- | --- |
| `TAKE_DAMAGE` | `TakeDamageEvent`：`victim`、`attacker`、`damage`（计算护甲后）、`warhead`、`position` | v1 |
| `KILLED` | `KilledEvent`：`victim`、`killer`、`position`；`KilledByWarheadEvent` 另加 `warhead` | v1 |
| `FATAL_DAMAGE` | `FatalDamageEvent`：`victim`、`attacker`、`damage`、`warhead`、`position` | v1 |
| `ATTACKED_BY` | `AttackedByEvent`：`target`、`attacker`、`weapon_slot`、`projectile` | v1 |
| `FIRE` | `FireEvent`：`shooter`、`target`、`target_cell`、`weapon_slot`、`weapon`、`projectile` | v1 |
| `TARGET_CHANGED` | `TargetChangedEvent`：`object`、`target`、`target_cell` | v1 |
| `DETONATED` | `DetonatedEvent`：`source`、`projectile`、`warhead`、`cell`、`position` | v1 |
| `CRUSHED` | `CrushedEvent`：`victim`、`crusher`、`position` | v1 |
| `BIRTH` | `BirthEvent`：`object`、`producer` | v1 |
| `TRANSPORT_ENTERED`、`TRANSPORT_EXITED` | `TransportEnteredEvent`、`TransportExitedEvent`：`passenger`、`transport`、`space`、`garrison` | v1 |
| `PASSENGER_ADDED`、`PASSENGER_REMOVED` | `PassengerAddedEvent`、`PassengerRemovedEvent`：`transport`、`passenger`、`space`、`garrison` | v1 |
| `HOUSE_TRANSFERRED` | `HouseTransferredEvent`：`object`、`old_house`、`mind_control` | v1 |
| `MODIFIER_START`、`MODIFIER_EXPIRE` | `ModifierEvent`：`target`、`modifier`、`property`、`source` | v1 |
| `MODIFIER_STACKS` | `ModifierStacksEvent`：`target`、`modifier`、`count` | v1 |
| `TIMER` | `TimerEvent`：`last_frame` | v1 |
| `SUPER_WEAPON_LAUNCHED` | `SuperWeaponLaunchedEvent`：`super_weapon`、`house`、`target_cell` | v1 |
| `DEPLOYED` | `DeployedEvent`：`object`、`became`、`deployed`（1 部署，0 收起） | v1 |
| `PROMOTED` | `PromotedEvent`：`object`、`rank`（1 老兵，2 精英） | v1 |
| `EXPERIENCE_GAINED` | `ExperienceGainedEvent`：`object`、`victim`、`gain` | v1 |
| `CLOAKED`、`DECLOAKED` | `CloakEvent`：`object` | v1 |
| `COMMANDED` | `&EventRecord`：`value` 是 `Command`，`other`/`cell` 是目标，`value2` 是每条指令的序号 | v1 |
| `ORDER_REFUSED` | `&EventRecord`：`subject`（所属方整体指令为 0）、`house`、`value` 是 `Command`，`value2` 是原因（附录 8.3 `RefuseReason`） | v1 |
| `MODULE_EVENT` | `ModuleEventRecord` | v1 |
| `PROPERTY_POLL`、`HOUSE_POWER_POLL`、`MODIFIER_COUNT_POLL`、`DISTANCE_POLL`、`VARIABLE_POLL` | `PollEvent`：`subject`、`reading` | v1 |
| 原版事件 1..61（`ENTERED_BY`、`DESTROYED_BY_ANY_HOUSE`、`CREDITS_EXCEED`、`ELAPSED_TIME`、`COMES_NEAR_WAYPOINT`……） | `StockEvent`：`subject`、`other`、`house`、`bound_cell` | v1 |
| Ares 事件 62..88 | `StockEvent` | 只在启用 Ares 时 |
| `MISSION_CHANGED` | `MissionChangedEvent` | later：不会触发（`has_kind` 为 0） |
| `HIT_BY_WARHEAD`、`SPAWN_FIRE`、`HIT_TARGET`、`PROJECTILE_INCOMING`、`HOUSE_TRANSFER`、`PRODUCTION_CANCELLED` | — | later：不会触发 |

只来自 INI 的记录 `INI_ENTRY` 和 `INI_KEY` 不经过触发器投递（[INI 注册](#ini-注册)）。

## 可决定事件

可决定事件触发器在游戏做判断的那一刻同步询问模块，模块可以改掉结果。每次询问都会交给该触发器的键所对应的 `on_decide!` 处理函数。

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

- 处理函数只能读（`get`、`find`、`var_get`、`random`、`log`……）。任何写入都会停掉模块：`set`、修饰器、写变量、任何 `Action::*`（包括 `raise_event` 和动作 `Action::log`）、创建、绑定或移除触发器、`Rules::set`、通知或所属方指令。想根据决定做点什么，就把它记在模块内存里，到下一次 `on_frame` 再做。
- 每个决定都有 `native()`（游戏自己的结果）、`previous()`（前一个模块的结果；第一个模块拿到的是游戏自己的结果）和 `set_result(value)`。不调用 `set_result` 时，沿用前一个结果。`request()` 给出完整的 `DecideRecord`（`record()`、`area()`）。
- 预算：每次询问 20,000 运算量，从一个等于半个帧预算的池子里扣（`ProductionAllowed` 用帧预算，`Defeat` 用它自己预算的四分之一）。
- 一个决定可以有多个模块回答：mod 的模块先，地图的模块后。

| 事件 | 视图：字段 | 结果 | 级别 |
| --- | --- | --- | --- |
| `TAKE_DAMAGE` | `TakeDamageDecision`：`victim`、`attacker`、`damage`、`warhead`、`position` | 最终伤害（0 挡下这次攻击） | v1 |
| `PRODUCTION_ALLOWED`（类型绑定） | `ProductionAllowedDecision`：`house`、`techno_type` | 1 允许，0 禁止到下一帧；1 不能解除游戏自己的禁止 | preview |
| `PRODUCTION_QUEUED` | `ProductionQueuedDecision`：`house`、`techno_type` | 1 允许，0 拒绝 | v1 |
| `DEFEAT` | `DefeatDecision`：`house` | 1 判负，0 否决这次检查 | v1 |
| `COMMAND_ALLOWED`（对象或类型绑定） | `CommandAllowedDecision`：`unit`、`command`、`target`、`target_cell` | 1 允许，0 拒绝（模块的指令也算） | preview |
| `DAMAGE_INCOMING` | `DamageIncomingDecision`：`victim`、`attacker`、`warhead`、`incoming`、`preview`、`position` | 即将受到的伤害；0 或同号的值 | preview |
| `FIRE_ALLOWED` | `FireAllowedDecision`：`shooter`、`target`、`target_cell`、`weapon_slot`、`weapon` | 0 否决这次开火 | preview |
| `RETALIATE` | `RetaliateDecision`：`victim`、`source`、`warhead` | 1 还击，0 不还击 | preview |
| `DESTINATION_SET` | `DestinationSetDecision`：`object`、`destination`、`destination_cell` | 目的地引用，0 表示没有 | preview |
| `CRUSH_ALLOWED` | `CrushAllowedDecision`：`victim`、`crusher` | 0 放过被碾者 | preview |
| `HARVEST_CELL` | `HarvestCellDecision`：`harvester`、`scanned` | 地格引用，0 表示没有 | preview |
| `DEPLOY_ALLOWED` | `DeployAllowedDecision`：`object` | 0 拒绝部署 | preview |
| `IDLE_MISSION` | `IdleMissionDecision`：`unit` | 一个 `Mission` 编号，255 表示没有 | preview |
| `MISSION_STEP` | `MissionStepDecision`：`unit`、`mission`、`target`；`step()`、`set_target`、`set_destination`、`set_mission(Option<Mission>)`、`set_delay`、`set_step` | Guard、Area Guard、Hunt 或 Attack 任务主体的决定，可以替换其中一部分。主体已经执行过了 | preview |
| `MOVE_FRAME` | `MoveFrameDecision`：`unit`、`moving`、`destination`、`destination_cell`、`position`；`step_by(dx, dy)`、`set_step(&MoveStep)`（`MOVE`、`FACING`、`STOP` 标志）、`previous_step()` | 代替运动模式，让地面单位（Drive、Ship、Walk、Mech、Hover）每帧移动最多 2,048 lepton | preview |
| `PATH_FOUND` | `PathFoundDecision`：`unit`、`goal`、`position`；`previous_cell(i)`、`set_cells(&[PathCell])`（最多 24 个，每个都与前一个相邻）、`no_route()` | 替换路线 | preview |
| `APPROACH_POINT` | `ApproachPointDecision`：`unit`、`chosen`、`target`、`target_cell`、`position`；`set_cell(cell)` | 追击射程外目标的单位开往的地格 | preview |
| `FIRE`（决定形式） | `FireDecision`：`shooter`、`target`、`target_cell`、`slot`、`weapon`；`previous_entry(i)`、`set_plan(&[ProjectileSpec])`（最多 16 个） | 射击计划：代替原本那发抛射体发射出去的抛射体 | SDK 中为 preview |
| `LAUNCH`（武器、弹头或抛射体类型绑定） | `LaunchDecision`：`projectile`、`launcher`、`target`、`target_cell`；`set_path(PathKind, speed, param, points)` | 子弹的轨迹（`Bezier`、`Parabola`、`Spiral`、`Waypoints`） | preview |
| `PROJECTILE_FRAME`（抛射体类型绑定） | `ProjectileFrameDecision`：`projectile`、`frames`；`set_step(&ProjectileStep)`、`previous_step()` | 子弹每帧的一步 | preview |
| `FATAL_DAMAGE`（决定形式） | `FatalDamageDecision`：死亡计划位 `EXPLODE`、`DEATH_WEAPON`、`DEBRIS`、`SURVIVORS`、`CREW`、`ANNOUNCE`、`TIBERIUM_REMAINS`；`with(bit, on)`、`with_inf_death`、`with_death_weapon` | 死后留下什么 | SDK 中为 preview |

结果的结构（`ProjectileSpec`、`ProjectileStep`、`MoveStep`、`PathCell`、`MissionStepResult`、`PathSpecHeader`）在 `vera20k_sdk::sys` 里。

## INI 注册

模块可以给 modder 提供自己的 INI 名字：类型段上的一个键、一个注册表（列表段）和模块事件。modder 写 INI，模块负责实现。只能在 `init!` 里注册，在别处注册会停掉模块。

| 函数 | INI | 模块收到什么 |
| --- | --- | --- |
| `ini::key(name, IniSection::TechnoType, KEY)` | 该种类的段（`TechnoType`、`Weapon`、`Warhead`、`Projectile`、`SuperWeapon`、`Country`、`General`）里的 `name=value` | 每个带这个键的段一条 `IniKeyRecord`：`type_ref()`、`is_general()`、`value(buf)`、`section_name(buf)` |
| `ini::registry(name, KEY)` | 列表段 `[name]`，列出各个条目段 | 每个条目一条 `IniEntryRecord`，按列表顺序：`entry_name(buf)`、`position()` |
| `ini::event(name, EventScope::Object \| Global) -> ModuleEvent` | 触发器段里的 `Event=Wasm:name` | 事件的 ID，给 `Trigger::on_module_event` 和 `Action::raise_event` 用 |

- 名字由 1 到 63 个 ASCII 字母、数字、`.` 和 `_` 组成，比较时不区分大小写。已经被注册过的名字（不论哪个来源）或以 `Vera.` 开头的名字返回 `Err(Refused)`。每个模块最多注册 4,096 个。
- 记录通过 `on_events!` 以你给的键送到，时间在 `on_scenario_start` 之前：先是全部 `IniEntry` 记录，再是全部 `IniKey` 记录。它们算在 `on_scenario_start` 的预算里。读档时不会再投递一次。
- 值是最终合并后的规则文本，引擎不做检查。条目段用 `Rules::get` 读，用 `ini::split`、`ini::int`、`ini::ints::<N>`、`ini::yes_no` 解析。值写错时，用 `fail(...)` 停下并写明段名和键名。
- `ini::EventSlot` 把事件 ID 存在一个 `static` 里（`set`、`get`）；`ModuleEvent::named(name)` 可以查到任何来源注册的事件。
- 模块动作的记录带着它的目标和文本：`record.target()`、`record.args_text(buf)`（INI 里的 `Wasm:name(target,text)`，或 `ActionDef::module_with`）。

INI 那边的写法见[模块指南](guide.zh-CN.md)；完整的 mod 见 `sdk/examples/ini-shield`、`ini-aura`、`ini-counter` 和 `ini-academy`。

## 通知与消息

模拟模块和同一来源（mod 或地图）的客户端模块通过游戏互相通信，这样每台机器都保持同步：

- **客户端到模拟**：客户端模块调用 `Client::send_message(&bytes)`，经过命令延迟后，消息在每台机器上送到模拟模块的 `on_message!` 处理函数，并附带发送者的所属方，客户端无法伪造。这些字节不可信：每一个都要检查。
- **模拟到客户端**：`NOTIFY.send(house, &bytes)` 通知该所属方的玩家；`send_observers(&bytes)` 通知观战者。那边的每个客户端模块都会收到一个 `Notify` 事件。
- **共享状态**：客户端模块用 `ClientVar` 读取声明的变量。变量会存进存档，所以读档后重新启动的客户端模块马上就能看到它。

```rust
vera20k_sdk::notify_names! {
    pub static PONG: Notify = "Pong";
}

fn ping(sender: House, data: &[u8]) {
    if data != [7] {
        return;
    }
    let _ = Action::add_money(sender, 1);
    let _ = PONG.send(sender, &[]);
}

vera20k_sdk::on_message!(ping);
```

| 限制 | 值 |
| --- | --- |
| 消息 | 96 字节（`notify::MAX_MESSAGE_BYTES`）；每个玩家每个来源每帧 2 条、每 15 帧 16 条；观战者不能发送 |
| 通知 | 256 字节（`notify::MAX_NOTIFY_BYTES`）；每个模块每帧 256 条；超出为 `Refused` |

`Notify::id()` 是名字的 FNV-1a 哈希，每台机器上都相同；`notify::send_raw(to, id, data)` 是原始调用。发送通知的模块必须用 `notify_names!` 声明每一个名字。

---

# 客户端模块

客户端模块（`ui.wasm`）导入 `vera:client@1.0`，只在本机运行，每个渲染帧一次。它读取这个玩家的界面能显示的内容，修改界面、镜头和光标，播放本地效果；它只能通过发给自己模拟模块的消息和标准玩家指令影响游戏。它可以用浮点数。

客户端模块里只用 `vera20k_sdk::client::*`。crate 根下的函数（`get`、`log`、`Var`、`Action`……）导入的是 `vera:sim` 的函数，导入了这些函数的客户端模块会被游戏拒绝。引用类型（`House`、`Cell`、`Techno`、`Prop`）以及 `Find` 和 `Order` 是两边共用的。

```rust
#![no_std]

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static BUY: NodeSlot = NodeSlot::new();

fn start() {
    if let Ok(button) = Ui::find("BuyReinforcements") {
        let _ = button.set_clickable(true);
        BUY.set(button);
    }
}

fn on_click(e: &ClientEventRecord) {
    if e.node() == BUY.get() && Client::send_message(&[1]).is_err() {
        client::log("the message was refused");
    }
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_client_event!(ClientEvent::Click => on_click);
```

## 客户端导出

| 宏 | 导出 | 何时调用 |
| --- | --- | --- |
| `client_init!(f)` | `client_init` | 加载后调用一次。这里只有 `client::lookup`、`name_of`、`has_kind` 和 `log` 能用 |
| `on_start!(f)` | `client_on_start` | 对局界面建好后调用一次；重连玩家追赶完成后、读档后会再调用一次 |
| `on_client_event!(ClientEvent::Kind => f, ...)` | `client_on_events` | 每个渲染帧、在 `on_render` 之前调用，传入这一帧的事件；`f(&ClientEventRecord)` |
| `on_render!(f)` | `client_on_render` | 每个渲染帧调用；`f(dt: f32, alpha: f32)` |

存档不包含客户端模块：读档后它们从头启动（先 `client_init`，再 `client_on_start`），存储保持离开时的样子。追赶期间（重连、观战者追赶）它们不运行。

## 客户端调用

| 函数 | 作用 |
| --- | --- |
| `client::frame() -> i32` | 最近一个已完成的模拟帧（在 `client_init` 里为 0） |
| `client::random(lo, hi)` | 一个只用于显示的本地随机数；`lo > hi` 会停掉模块 |
| `client::log(line)` | 在聊天栏写一行（每个渲染帧最多 8 行），同时写进日志文件 |
| `client::has_kind(table, id) -> bool` | 表和模拟模块那边相同，另加 5 节点种类、6 客户端事件种类、7 客户端动作种类 |
| `client::lookup(kind, name)`、`client::name_of(kind, id, buf)` | 和模拟模块那边一样。变量名和通知名是这局对局的模拟模块声明的那些；热键名是本模块的 |

## 读取游戏状态

| 函数 | 作用 |
| --- | --- |
| `client::get(reference, prop)` | 按这个玩家界面能显示的样子读属性：视野外的对象为 `Err(Gone)`，隐藏的属性为 `Err(Refused)` |
| `client::get_many(refs, props, out)` | 同 `get_many`，每一项都按 `get` 的可见性规则 |
| `client::find_page(&mut find, &mut out, after)` | 一页 `Find` 结果，只包含这个玩家看得到的 |
| `ClientVar::new("Name")` | 模拟模块声明的变量，按它的可见性读取：`get()`（全局）、`get_house(house)`（私有变量只能读这个玩家自己的所属方）、`get_object(object)` |
| `client::var_get(scope, var)` | 原始读取；全局变量的作用域写 0 |

盟友的对象只显示 `seen` 属性；间谍按屏幕上的样子显示它的伪装；观战者能读到什么取决于房间设置。

## 节点

对局界面是一棵节点树：`veraui.ini` 布局的各个部件、地图的 `[VeraUI]` 部件，以及模块创建的节点。

| 函数 | 作用 |
| --- | --- |
| `Ui::find(name) -> Result<Node>` | 按名字找布局部件（不带布局前缀：`[Mine.Buy]` 写 `Buy`），或者按 `Name` 找创建的节点。先找本模块的来源，再找引擎、mod 和地图 |
| `Ui::create(kind, parent)` | 新建一个节点；父节点写 `Node(NO_HANDLE)` 表示顶层 |
| `Ui::create_with(kind, parent, &[(Prop, i64)])` | 一次调用里带上最多 16 个初始属性 |
| `Ui::create_named(kind, parent, name)` | 创建并命名；名字被拒绝时会再移除 |
| `Ui::remove(node)`、`node.remove()` | 移除它和它的子节点 |
| `NodeSlot` | 在回调之间把节点存在一个 `static` 里：`set(node)`、`get()` |

`NodeKind`：`Text`、`Button`、`Row`、`Column`、`List`、`Image`、`Bar`（模块可以创建这些）；`CameoGrid`（只有布局能创建；模块可以找到它，写它的通用属性和 `ItemTooltips`）。

节点方法：`get(prop)`、`set(prop, value)`、`set_name`、`set_label`（CSF 条目名或直接写文本）、`set_art`（按名字指定图片文件，比如 `mybutton.shp` 或 `panel.pcx`）、`visible`、`set_visible`、`set_enabled`、`set_clickable`、`set_hoverable`、`set_dismissable`、`set_position(x, y)`、`set_size(w, h)`、`set_fill(0..100)`（`Bar`）、`set_backdrop(bool)`（战场上的 `Text` 默认垫黑底，关掉才没有）、`set_items(&[&str])`（`List` 的各行，总共 1 KiB）。其他属性用 `set`/`client::set_bytes` 写：`Hold`（0 `TopLeft`，1 `Top`，2 `TopRight`，3 `Left`，4 `Middle`，5 `Right`，6 `BottomLeft`，7 `Bottom`，8 `BottomRight`）、`Gap`（`Row`、`Column`）、`ItemTooltips`（`CameoGrid`）。`ItemProgress`、`Decoration` 和 `MapMarker` 是 `later`。

- **谁能写什么。** mod 的模块可以读、写、移除任何节点。地图的模块在 mod 设了 `[VeraWasm] MapUI=overlay`（默认）时，能找到和读取任何节点，但只能写地图自己的；`MapUI=full` 时拥有和 mod 一样的权限；`MapUI=no` 时不加载。写了不允许写的节点返回 `Err(Refused)`。
- **画在哪。** 模块的顶层节点画在面板层；`MapUI=overlay` 下地图模块的顶层节点画在对局界面之上的覆盖层。坐标在侧边栏里是侧边栏像素，在其他地方是屏幕像素。
- **能建多少。** 每个模块最多占用界面允许其他来源创建的节点数的一半；超出时创建为 `Refused`。
- 后创建的节点画在先创建的节点上面。

[界面布局](../interface-layout.md#parts-for-interface-modules)指南介绍了给模块准备的布局部件（没有 `Command` 的按钮、`Image`、`Bar`、`List`）。

## 玩家、镜头与光标

| 函数 | 作用 |
| --- | --- |
| `Client::viewer() -> House` | 这个玩家的所属方，观战者为 `House(NO_HANDLE)` |
| `Client::is_spectator()` | |
| `Client::send_message(&bytes)` | 发给本来源模拟模块的 `on_message`（[限制](#通知与消息)）；观战者发送或超出限制时为 `Refused` |
| `Client::order(&Order)` | 以这个玩家的名义下一条标准玩家指令，按玩家输入校验。每个单位每 15 帧一次，所属方整体指令每 15 帧 4 条。房间不允许自动化（全局 `AutomationAllowed`）时为 `Refused`；大厅还没有提供这个开关，所以目前都会被拒绝 |
| `Camera::cell()` | 镜头中心所在的地格 |
| `Camera::jump_to(cell)`、`Camera::center_on(cell, speed)` | 移动本地镜头 |
| `Cursor::position() -> (x, y)` | 屏幕像素，和节点的 `X`、`Y` 同一套坐标 |
| `Cursor::cell()` | 光标下的地格，在地图外为 `Cell(NO_HANDLE)` |
| `Cursor::capture_map_clicks(bool)` | 设为 true 时，地图点击只送给本模块（`MapClick`），不交给游戏 |

## 本地动作

只在本机生效的效果（`Local`，附录 5.4）：

| 函数 | 作用 |
| --- | --- |
| `Local::play_sound(id, cell)` | 在一个地格上播放声音 `id`（它在 `[SoundList]` 里的编号），写 `Cell(NO_HANDLE)` 则不定位。本版本里 `lookup(Sound, ..)` 回答 `Unsupported` |
| `Local::play_speech(name)` | EVA 语音，比如 `"EVA_ReinforcementsHaveArrived"` |
| `Local::select(&units, append)` | 选中这个玩家可以选中的单位（最多 64 个） |
| `Local::flash(object, frames)` | 让一个对象闪烁 |
| `Local::show_message(text, color)` | 显示一行：CSF 条目名或直接写文本 |
| `Local::register_hotkey(name, "Ctrl+Alt+K")` | 注册一个热键和它的默认组合键；`Hotkey::id(name)` 给出它的事件带的 ID。组合键已被游戏热键占用时为 `Refused`（热键仍然存在，只是没有绑键）。每个模块 256 个 |
| `Local::run(kind, target, &params)` | 任意客户端动作；`HOME_CAMERA` 是 `later` |

## 客户端事件

`ClientEvent` 的种类，送到 `on_client_event!`（每个渲染帧最多 1,024 个）：

| 种类 | 何时 | 读什么 |
| --- | --- | --- |
| `Click` | 点击了可点击的节点（这次点击不会传到地图） | `node()`；`value` 是 `List` 的行或 `CameoGrid` 的项，其他情况为 -1 |
| `Hover` | 指针进入（`value` 为 1）或离开（0）可悬停的节点 | `node()` |
| `Hotkey` | 按下了注册的热键 | `id` |
| `Dismiss` | 在可见的可关闭节点外按下，或按了 Esc | `node()` |
| `MapClick` | 既不是 `Click` 也不是 `Dismiss` 的地图点击 | `cell()`；`value` 0 左键，1 右键 |
| `SelectionChanged` | 本地选择变了；用 `find_page` 加 `Selected = 1` 条件读出来 | — |
| `Notify` | 模拟模块发给本机的通知 | `id`、`data()`、`from_map()`；用 `NotifyName::is(event)` 比较 |
| `SimEvent` | 这个玩家的对象或所属方发生的 `Commanded`、`OrderRefused`、`TakeDamage`、`Killed`、`Birth` 或 `AttackedBy` | `sim_event() -> Option<&EventRecord>`（这个玩家看不到时 `other` 为 0） |

`ClientEventRecord` 解引用为 `sys::ClientEventRecord`（`kind`、`source`、`reference`、`id`、`value`、`data`、`data_len`）。`NotifyName::new("Name")` 解析模拟模块声明的通知名；`is(event)` 检查一个事件是不是它。

## 存储

每个来源（mod 按它的名字；地图按 `[VeraWasm] StorageId=` 或它的文件名）在本机上有一个小小的键值存储，在对局之间保留。

| 函数 | 作用 |
| --- | --- |
| `Storage::read(key, &mut buf) -> Result<&[u8]>` | 读值；没有值时为 `NotFound`，`buf` 太小时为 `Refused` |
| `Storage::len(key)` | 值的长度 |
| `Storage::write(key, data)` | 写入；空数据为 `Refused` |
| `Storage::remove(key)` | 删除 |

键是 1 到 64 个 ASCII 字节；值最多 16 KiB；每个存储最多 256 个键、256 KiB。超出限制时写入为 `Refused`，什么也不改。存储最多每秒写一次磁盘。它只在本机：绝不能让它影响模拟的结果。
