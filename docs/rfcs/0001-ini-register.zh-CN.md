# RFC 0001 增补：模块注册 INI（第 1 版）

| | |
| --- | --- |
| 版本 | 1（定稿） |
| 基于 | RFC 0001 0.0.20 |
| 日期 | 2026-11-24 |
| 实现 | `feature/wasm-extension-modules`，RFC 阶段 1 之后（阶段 1b） |

模块用 INI 名字向 modder 提供功能：modder 只写 INI，模块实现机制。模块在初始化函数中用 `ini_register` 注册名字；引擎记录每个名字归哪个模块、由哪个处理函数负责，开局时把最终规则中出现的已注册名字交给处理函数。值的格式、默认值和取值范围由模块定义和检查，引擎不解析，也没有大厅校验。

本增补之外的 RFC 0001 规则不变：`vera.actions` 仍是模块动作名的唯一来源，模块动作和变量的 `mod.`、`map.` 前缀规则不变。

编号：下文先写 RFC 编号，括号内是本实现的暂定编号（第 5 问，RFC 的 1000xx 对应实现的 10xx）。

## 1. 示例

modder 写的 INI（mod 的 `rulesmd.ini`）：

```ini
[HTNK]
Kratos.Shield=HeavyShield           ; 类型上的键
Wasm.Tags=ShieldMedicTag

[Kratos.ShieldTypes]                ; 注册表
1=HeavyShield

[HeavyShield]                       ; 条目节，内容由模块读取
Strength=300

[ShieldMedicTag]
Trigger=ShieldMedic

[ShieldMedic]
Event=Wasm:Kratos.ShieldBroken      ; 模块事件
Action1=Wasm:restoreShield(Subject,50)   ; 带参数的模块动作（名字来自 vera.actions）
```

模块（Rust SDK，示意）：

```rust
#[vera::init]
fn init() {
    ini::registry("Kratos.ShieldTypes", load_shield_type);
    ini::key("Kratos.Shield", IniSection::TechnoType, attach_shield);
    SHIELD_BROKEN.set(ini::event("Kratos.ShieldBroken", EventScope::Object));
}
```

## 2. 三种注册

| 种类（`IniKind`） | INI 写法 | 引擎做什么 |
| --- | --- | --- |
| 0 `Key`：类型上的键 | 注册时指定的那类节中的 `名字=值` | 开局时，每个出现该键的节投递一条 `IniKey` 记录给处理函数 |
| 1 `Registry`：注册表 | 列表节 `[名字]`，值为条目节名 | 开局时，列表中的每个条目投递一条 `IniEntry` 记录给处理函数 |
| 2 `Event`：模块事件 | 触发器节的 `Event=Wasm:名字` | 分配事件 ID；`RaiseEvent` 引发它，按普通投递型事件触发触发器 |

## 3. 导入 `ini_register`

```
ini_register(kind: i32, section: i32, name: ptr, len: i32, key: i32, out: ptr) -> status
```

| 参数 | 含义 |
| --- | --- |
| `kind` | `IniKind` |
| `section` | `Key`：键挂在哪类节上（`IniSection`）；`Event`：0 对象事件，1 全局事件；`Registry`：0 |
| `name`、`len` | 名字 |
| `key` | `Key`、`Registry`：处理函数的分派键；`Event`：忽略 |
| `out` | 写入一个 `i32`：`Event` 为事件 ID，其他为 0 |

- **只能在初始化函数（`vera_init`）中调用**；在其他地方调用属于非法参数，模块停用。
- 名字为 1 到 63 字节，只含 ASCII 字母、数字、`.` 和 `_`，按 ASCII 规则不区分大小写；不合法为非法参数。
- 同一种类中名字在本局唯一，mod 和地图共用：已被注册（任何来源，包括自己）时返回 `Refused`。以 `Vera.` 开头的名字返回 `Refused`。不同种类可以同名。
- 每个模块至多 4,096 个注册，超出为非法参数。
- 注册表（名字、种类、来源、分派键）由引擎持有，存档并计入状态哈希。读档时不运行初始化函数，注册从存档恢复。

`IniSection`：0 `TechnoType`、1 `Weapon`、2 `Warhead`、3 `Projectile`、4 `SuperWeapon`、5 `Country`、6 `General`（只有 `[General]` 节）。

## 4. 生命周期

| 顺序 | 步骤 | 注册与使用 |
| --- | --- | --- |
| 1 | `RULESMD.INI`、`LANGRULE.INI`、模式层 | |
| 2 | mod 模块的初始化函数 | **注册** |
| 3 | mod 模块的 `on_rules` | 可以写任何键，包括已注册的；不应缓存它们的值 |
| 4 | 地图层 | 地图 INI 可以写 mod 注册的键 |
| 5 | 地图模块的初始化函数 | **注册**；重名得到 `Refused` |
| 6 | 地图模块的 `on_rules` | 同 3 |
| 7 | 引擎解析 INI 触发器 | `Event=Wasm:名字` 须是已注册的事件，否则不能开局 |
| 8 | 第一次模块阶段，各模块 `on_scenario_start` 之前 | 投递该模块的 `IniEntry`、`IniKey` |
| 9 | `on_scenario_start`，之后每帧 | 触发器调用模块动作，`RaiseEvent` 引发模块事件 |

没有 `on_rules` 的模块在场景安装时运行初始化函数（RFC 第 3 节），也早于第 7 步。两个来源的初始化顺序由此决定，重名时后注册的一方得到 `Refused`。

## 5. 开局投递

- 每个模块在它的 `on_scenario_start` 之前收到：先全部 `IniEntry`，再全部 `IniKey`。两种记录都按注册顺序；同一个注册中，`IniEntry` 按条目在合并后列表中的顺序（同名条目只投递第一次），`IniKey` 按该类节的 ID 顺序。
- 值取自规则层全部处理完之后的合并文本，引擎不检查格式。
- 投递使用 `on_events`，记录的 `key` 为注册时给出的分派键。不受每帧条数上限约束，不进积压队列；全部投递计入 `on_scenario_start` 的一次性额度，用完时模块停用。
- 处理函数发现值不合法时，模块应停止并指出节和键；这次停止按"对局中，包括 `on_scenario_start`"处理。
- 读档时不重新投递：处理函数建好的数据在模块内存中，随快照恢复。

| RFC # | 记录 | 字段 |
| --- | --- | --- |
| 100041（1041） | `IniEntry` | `args` 条目节名（字符串 ID），`value` 在列表中的位置（从 0 开始） |
| 100042（1042） | `IniKey` | `subject` 该节的类型引用（`General` 为全局规则引用），`args` 合并后的值（字符串 ID） |

两种记录的 `trigger`、`holder`、`target` 为 0，`id` 为 −1。它们不能用于触发器：`EventDef` 中出现属于非法参数。

## 6. 模块事件

| RFC # | 事件 | 字段 | 绑定 |
| --- | --- | --- | --- |
| 100040（1040） | `ModuleEvent` | `id` 事件 ID（`EventDef.id`）；`subject`；`other`、`value`、`value2`、`house` 取自 `RaiseEvent` 的参数 | 注册为对象事件时为对象，全局事件为全局 |

- 触发器的 `EventDef.id` 为事件 ID，只接收该 ID 的事件；ID 未注册为非法参数。INI 写 `Event=Wasm:名字`。
- 任何来源都可以引发任何已注册的事件，包括 INI 触发器和另一个来源的模块。

原子动作：

| RFC # | 名称 | 目标 | 参数 | 语义 | 随机数 |
| --- | --- | --- | --- | --- | --- |
| 100041（1041） | `ApplyEffect` | — | — | 保留，不执行 | — |
| 100042（1042） | `RaiseEvent` | 对象（对象事件），或不限（全局事件） | `id` 事件 ID；`other`、`value`、`value2`、`house` 写入记录 | 引发模块事件。对象事件的目标不是存在的对象时为 `Gone`；ID 未注册为非法参数 | none |

INI 写法：`RaiseEvent(目标,事件名,value,value2)`。

`RaiseEvent` 在执行处立即引发事件，引发点是安全点（RFC 第 5 节）：监听它的触发器中，原版动作和原子动作立即执行，模块动作排队；连锁按第 5 节计数。决定处理函数不能执行它（写入函数）。

## 7. 带参数的模块动作

- INI：`Action=Wasm:名字` 不带参数，目标为 `Subject`。`Action=Wasm:名字(目标,文本)`：第一个参数是目标（`ActionTarget` 名，可空），其后到最后一个右括号为止的内容是参数文本，原样保留，可含逗号。只有目标没有文本也可以。
- 代码：`Module` 动作的 `ActionDef.target` 是目标，`ActionParams.text` 是参数文本。
- 触发器触发时，引擎把参数文本合并为字符串 ID 写入记录的 `args`（没有文本为 0），目标写入 `target`。模块用 `name_of(String, args)` 读出文本。
- `Decide=Wasm:名字` 不带参数。
- 地图触发器的动作 100036 不带参数。

字符串表（参数文本、`IniEntry` 条目名、`IniKey` 值）由引擎持有，只增不减，存档并计入状态哈希。名字种类 18 `String`：`name_of` 读出文本；`lookup` 查已有文本的 ID，没有为 `NotFound`。

## 8. 布局与表的改动

`EventRecord`（120 字节），前 108 字节不变：

```rust
    args: i32,            // 108 字符串 ID：模块动作的参数文本；IniEntry 条目名；IniKey 值；没有为 0
    target: i32,          // 112 模块动作的目标（ActionTarget：0 Subject、1 Other、2 TagHolder、3 Projectile、4 House、5 Cell）；其他记录为 0
    reserved: i32,        // 116 0
```

`DecideRecord`（160 字节）：`event` 0..120、`native` 120、`previous` 128、`result` 136、`handled` 144、`area` 148、`area_cap` 152、`previous_len` 156。

- 导入表：新增 24 `ini_register`，可用于 `init`。
- 名字种类：新增 24 `ModuleEvent`（任何来源注册的事件；返回事件 ID）；18 `String` 见第 7 节。
- 枚举：新增 23 `IniKind`、24 `IniSection`。
- `has_kind`：事件 1040、1041、1042，动作 1042，名字种类 18、24 为可用。

## 9. 本版不做

- 大厅校验已注册名字的值。
- 地图 FA2 触发器的参数文本。
- 决定处理函数中引发事件：护盾在决定中被打破时，`ShieldBroken` 到下一次模块阶段才能引发。
- 注册条件（筛选子句）：模块把结论写进变量，INI 用 `Where=Var(...)` 判断。
