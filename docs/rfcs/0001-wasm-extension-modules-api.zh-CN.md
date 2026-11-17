# RFC 0001 附录：函数与表

本附录是 [RFC 0001](0001-wasm-extension-modules.zh-CN.md) 的一部分，使用同一个版本号（0.0.7）。两个文件在同一次提交中修改。本附录与 RFC 正文不一致时，以 RFC 正文为准。与 RFC 正文一样，本附录目前只有这一个中文版本；RFC 定稿后再翻译成英文。

本附录是一份参考，分成两部分：冻结的函数（第 1、2 节），以及只追加的表（第 4 到 8 节）。每一项都给出签名或布局和一行语义说明。它们背后的规则只在 RFC 正文中陈述一次，本附录链接到那里。版本 1 包含哪些表项是 RFC 的待定问题 12。

## 目录

- [约定](#约定)
- [1. 模拟 ABI](#1-模拟-abi)
- [2. 客户端 ABI](#2-客户端-abi)
- [3. SDK 对应](#3-sdk-对应)
- [4. 事件种类](#4-事件种类)
- [5. 动作种类](#5-动作种类)
- [6. AI 小队](#6-ai-小队)
- [7. 属性](#7-属性)
- [8. 名字种类、搜索范围与其他枚举](#8-名字种类搜索范围与其他枚举)

## 约定

### 引用

引用（Ref）是一个 `i64`，所有值都小于 2^53，0 表示“没有”。第 48 到 52 位是引用种类，低 48 位是内容。SDK 负责编码和解码，模块代码通常不需要知道这些位。

| 种类 | 名称 | 内容 | 何时失效 |
| --- | --- | --- | --- |
| 0 | 模拟对象句柄 | 创建序号（1 到 2^48 − 1）。单位、建筑、抛射体、所属方、小队、触发器、标签、修饰器、超级武器共用同一个计数器 | 对象被移除；所属方在场景进行期间从不失效 |
| 1 | 类型引用 | `名字种类 << 32 | ID`（8.1），例如 TechnoType、武器、弹头、抛射体、超级武器、国家 | 从不（ID 一经分配就不再改变） |
| 2 | 格子引用 | `x << 16 | y`，单位为格 | 从不 |
| 3 | 路径点引用 | 路径点编号；接收格子的函数把它解析为路径点所在的格子，地图没有这个路径点时为 `NotFound` | 从不 |
| 4 | 全局规则引用 | 0：场景和全局规则（7.8） | 从不 |
| 5 | 客户端节点句柄 | 本机的节点序号，与模拟的创建序号分开 | 节点被删除，或对局界面关闭 |
| 6 | 客户端全局引用 | 1：镜头，2：本地设置（7.10） | 从不 |

- 模拟对象句柄是对象的创建序号：在创建时分配，从不复用，与引擎如何存放对象无关（[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)）。
- 对 0 或已不存在的引用发起的调用遵循[错误模型](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。
- 在 TypeScript SDK 中，每种引用（`Techno`、`House`、`Projectile`、`Trigger`、`Node`……）都是 `i64` 的别名，`NO_HANDLE` 为 0；在 Rust SDK 中是 newtype。

### ID

ID 是一个 `i32`，由 `lookup(kind, name)` 给出，−1（`NO_ID`）表示“没有”。名字种类见 [8.1](#81-名字种类lookup)。模块对每个名字只解析一次，通常在 `vera_init`、`on_rules` 或 `on_scenario_start` 中完成，其他所有调用都传 ID 或引用（[RFC 15](0001-wasm-extension-modules.zh-CN.md#15-性能)）。ID 一经分配就不再改变（[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)）。

| 情况 | 结果 |
| --- | --- |
| 没有匹配任何东西的名字 | −1，且 `last_status()` 为 `NotFound` |
| 传入 −1 的调用 | `NotFound` |
| 超出其种类取值范围的 ID | 非法参数 |

**变量 ID**（[RFC 6e](0001-wasm-extension-modules.zh-CN.md#6e-变量)）：

| ID | 变量 |
| --- | --- |
| 0..99 | 红警 2 局部变量 0..99 |
| 100..149 | 红警 2 全局变量 0..49 |
| 1000 起 | `vera.vars` 中声明的变量，按加载顺序（先 mod 的，再地图的）、按声明顺序编号 |

属性 ID、事件种类和动作种类的编号由表决定：原版事件和原版动作用原版编号，其他表项按表中的顺序由接口定义分配，只在末尾追加（[RFC 12](0001-wasm-extension-modules.zh-CN.md#12-版本与-abi)）。

### 值

属性的值、变量和记录中的整数都装在 `i64` 里，按属性表中的值类型解释：

| 值类型 | 含义 |
| --- | --- |
| `int` | 整数。模拟 API 中没有浮点值。 |
| `bool` | 0 或 1；写入非零值即为 1。 |
| `fixed` | 定点数，`I16F16` 原始值（`SimFixed`）：`65536` 表示 `1.0`。用于倍率。 |
| `lepton` | 坐标或距离，1 格等于 256 lepton。 |
| `frames` | 逻辑帧数；每游戏秒 15 个逻辑帧。 |
| `enum` | 第 8.3 节中某个枚举的值。 |
| `mask` | 位掩码，各位的含义在该属性中写明。 |
| `ref` | 一个引用。 |
| `id` | 某个名字种类的 ID。 |
| `string` | 一个字符串 ID：用 `name_of(String, id, ...)` 读出文字；SDK 直接返回字符串。写入字符串属性用 `client_set_bytes`。 |

### 字符串与缓冲区

- 字符串和字节以“指针加长度”跨越边界，指向模块内存。字符串是 UTF-8；名字比较时不区分大小写，且只按 ASCII 规则（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）。
- 返回数量的函数把结果写进模块提供的缓冲区（`out`、`cap`），返回写入的数量。
- 返回文字的函数（`name_of`、`rules_get`、`client_storage_read`）返回完整的长度；长度大于 `cap` 时什么也不写，模块用更大的缓冲区再调用一次。
- 搜索结果按创建序号排序；分页时把上一页最后一个结果放进查询的 `after` 字段（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-对象属性与修饰器)）。

### 状态

```rust
enum Status {
    Ok = 0,
    Gone = 1,         // the ref is 0 or its object no longer exists
    NotFound = 2,     // a name or ID matched nothing; a missing waypoint
    Refused = 3,      // the owner declined: cell blocked, property not writable now, quota full
    Unsupported = 4,  // off in this game, unknown table entry, or not available here (init function, decide handler)
}
```

代码注释说明：`Gone` 表示引用为 0 或其对象已不存在；`NotFound` 表示名字或 ID 没有匹配任何东西，或路径点不存在；`Refused` 表示 owner 拒绝（格子被占、属性此时不可写、配额已满）；`Unsupported` 表示该功能在本局中关闭、本引擎不认识该表项，或在此处不可用（初始化函数中）。

每个函数都设置 `last_status()`。会停用模块的非法参数按函数逐一列出：越界的 ID 或下标、负数的数量或容量、不在表中的枚举值、作用域与声明不符的变量、`trigger_create` 和 `action` 拒绝的动作参数（5.1）、非法的筛选子句，以及在 `on_decide` 中调用写入函数。

### 随机数

每个表项的“随机数”列写明它使用哪条随机流（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）：

| 取值 | 含义 |
| --- | --- |
| `module` | 从模块自己的随机流取数（`random`）。 |
| `native` | 运行原生代码，按那段代码原本的方式从原生随机流取数。 |
| `none` | 不取随机数。 |

### 条目标注

| 字段 | 取值 |
| --- | --- |
| **层级** | `v1`（RFC 计划的阶段 1 或 2）、`v1?`（待定问题 12 下的候选；对可决定事件而言也是待定问题 10 下的候选）、`later` |

表中原版事件和动作的名字是 API 名；真正标识事件或动作的是编号。

### 固定布局

所有布局都是小端序，按字段自然对齐。注释中的数字是字节偏移。

**事件记录**，每条 96 字节，在 `on_events` 的批次中原地读取：

```rust
struct EventRecord {      // 96 bytes
    key: i32,             // 0   dispatch key of the module action or decide handler
    kind: i32,            // 4   event kind (section 4)
    trigger: i64,         // 8   the trigger that sprang; always set
    holder: i64,          // 16  tag holder: the object whose tag holds the trigger, the bound object, or a CellTag's cell ref; 0 otherwise
    subject: i64,         // 24  the object the trigger is attached to; may be 0, or read as gone
    other: i64,           // 32  attacker, target, transport, producer, ...; may be 0
    house: i64,           // 40  house involved (retained house for events 1, 24, 25, 26, 59); may be 0
    projectile: i64,      // 48  may be 0
    modifier: i64,        // 56  may be 0
    cell: i64,            // 64  "cell" in the tables: a cell ref; 0 when none
    value: i64,           // 72  damage, weapon index, stack count, frame, ...
    value2: i64,          // 80  second integer where listed
    id: i32,              // 88  type ID where listed: weapon, warhead, superweapon type; -1 otherwise
    reserved: i32,        // 92  0
}
```

代码注释说明：`key` 是模块动作或决定处理函数的分派键；`kind` 是事件种类（第 4 节）；`trigger` 是触发的那个触发器，总会设置；`holder` 是标签持有者，即标签中持有该触发器的对象、被绑定的对象，或 CellTag 的格子引用，其他情况为 0；`subject` 是触发器所附着的对象，可能为 0，也可能读出来是“不存在”；`other` 是攻击者、目标、载具、生产者等，可能为 0；`house` 是涉及的所属方（事件 1、24、25、26、59 为保留的所属方），可能为 0；`projectile` 和 `modifier` 可能为 0；`cell` 是表中的“cell”，即格子引用，没有时为 0；`value` 是伤害、武器索引、层数、帧数等，`value2` 是表中列出时的第二个整数；`id` 是表中列出时的类型 ID（武器、弹头、超级武器类型），其他情况为 −1；`reserved` 为 0。

- 第 4 节中每个事件写明它填入哪些字段；没有列出的引用字段为 0，整数字段为 −1。每条记录都会填入 `key`、`kind`、`trigger` 和 `holder`。

**决定记录**，每条 136 字节，在 `on_decide` 的批次中原地读取和写入（[RFC 6f](0001-wasm-extension-modules.zh-CN.md#6f-可决定事件)）：

```rust
struct DecideRecord {     // 136 bytes
    event: EventRecord,   // 0    as above
    native: i64,          // 96   the engine's own result
    previous: i64,        // 104  the previous link's result; native for the first link
    result: i64,          // 112  written by the handler
    handled: i32,         // 120  the handler sets 1; 0 keeps the previous link's result
    area: i32,            // 124  result area in module memory; 0 when the kind has none
    area_cap: i32,        // 128  bytes available in the area
    previous_len: i32,    // 132  bytes of the previous link's result, copied into the area
}
```

代码注释说明：`native` 是引擎自己的结果；`previous` 是上一环的结果，第一环拿到的是原生值；`result` 由处理函数写入；`handled` 由处理函数设为 1，为 0 时上一环的结果保持不变；`area` 是该请求在模块内存中的结果区，事件种类没有结果区时为 0；`area_cap` 是结果区的可用字节数；`previous_len` 是上一环写入结果区的字节数，引擎已把它复制进来。

**筛选子句**，每条 16 字节，用于事件定义和查询（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）：

```rust
struct Clause {           // 16 bytes
    subject: u8,          // 0  0 Subject, 1 Other, 2 House, 3 Var
    op: u8,               // 1  Op (8.3)
    scope: u8,            // 2  Var only: 0 global, 1 Subject, 2 Other, 3 House
    reserved: u8,         // 3  0
    prop: i32,            // 4  property ID; for Var, the variable ID
    value: i64,           // 8  the constant to compare with
}
```

代码注释说明：`subject` 为 0 表示主体、1 表示另一方、2 表示所属方（触发器的所属方）、3 表示变量；`op` 是比较运算符（8.3）；`scope` 只用于变量：0 全局、1 主体、2 另一方、3 所属方；`prop` 是属性 ID，对变量是变量 ID；`value` 是要比较的常量。在查询中，`Subject` 是被检查的候选，`Other` 和 `House` 不可用（非法参数）。

**查询**，`find` 的输入，64 字节加上子句：

```rust
struct Query {            // 64 bytes, then clause_count Clause
    scope: i32,           // 0   search scope (8.2)
    area: i32,            // 4   0 whole map, 1 cell + radius, 2 rect
    cell: i64,            // 8   area 1: the centre; area 2: the top-left corner (cell refs)
    radius: i32,          // 16  area 1: leptons; area 2: width in cells
    height: i32,          // 20  area 2: height in cells
    type: i64,            // 24  type ref; 0 any
    owner: i64,           // 32  house ref; 0 any
    viewpoint: i64,       // 40  house for viewpoint properties (7); 0 none
    after: i64,           // 48  paging: results after this ref; 0 from the start
    clause_count: i32,    // 56  at most N (open question 7)
    reserved: i32,        // 60  0
}
```

代码注释说明：`scope` 是搜索范围（8.2）；`area` 为 0 表示整张地图，1 表示格子加半径，2 表示矩形；`cell` 在范围 1 中是中心，在范围 2 中是左上角；`radius` 在范围 1 中是 lepton 半径，在范围 2 中是宽度（格）；`height` 是范围 2 的高度；`type` 为类型引用，0 表示任意；`owner` 为所属方引用，0 表示任意；`viewpoint` 是视点属性用的所属方（第 7 节）；`after` 用于分页，0 表示从头开始；`clause_count` 至多为 N（待定问题 7）。

**触发器定义**，`trigger_create` 的输入：

```rust
struct TriggerDef {       // 32 bytes, then event_count EventDef (each followed by its clauses), then action_count ActionDef
    house: i64,           // 0   0: the subject's owner at each spring
    repeat: i32,          // 8   TagRepeat (8.3)
    difficulty: i32,      // 12  Difficulty (8.3)
    flags: i32,           // 16  bit 0 starts disabled; bit 1 allowWin (original action 15); bit 2 transferOnCapture (TriggerType token 8)
    decide_key: i32,      // 20  dispatch key of the decide handler; -1 none
    event_count: i32,     // 24  one or more; all must hold (AND), as in the original
    action_count: i32,    // 28  zero or more; at least one action or a decide handler
}
struct EventDef {         // 32 bytes, then clause_count Clause
    kind: i32,            // 0   event kind
    op: i32,              // 4   polled events: Op comparing the polled value with value
    interval: i32,        // 8   polled events: frames between checks
    id: i32,              // 12  type, variable or TeamType ID parameter; -1 none
    value: i64,           // 16  number parameter (house index, waypoint, seconds, threshold, ...)
    clause_count: i32,    // 24  at most N
    reserved: i32,        // 28  0
}
struct ActionDef {        // 8 bytes, then ActionParams
    kind: i32,            // 0   action kind (section 5)
    target: i32,          // 4   ActionTarget (5.1)
}
```

代码注释说明：`TriggerDef.house` 为 0 时，触发器每次触发时的所属方是主体当时的所属方；`repeat` 是原版标签的重复类型；`flags` 第 0 位表示创建时处于禁用状态，第 1 位是原版动作 15 的标记，第 2 位是 TriggerType 的第 8 项；`decide_key` 是决定处理函数的分派键，−1 表示没有；事件有一个或多个，必须全部成立（AND），与原版相同；动作可以没有，但动作和决定处理函数至少要有一个。`EventDef.op` 和 `interval` 用于轮询型事件；`id` 是类型、变量或 TeamType 的 ID 参数；`value` 是数值参数（所属方下标、路径点、秒数、阈值等）。`ActionDef.kind` 是动作种类（第 5 节），`target` 是目标选择（5.1）。

**动作参数**，72 字节，用于 `action` 和触发器定义中的动作：

```rust
struct ActionParams {     // 72 bytes
    id: i32,              // 0   type ID (weapon, warhead, superweapon, animation, techno, sound, TeamType...);
                          //     property ID for SetProperty and Modify; variable ID for SetVar; dispatch key for Module
    arg: i32,             // 4   small enum where listed: Blocked, ModifierOp, Mission, Outcome, ...
    arg2: i32,            // 8   modifier group where listed
    reserved: i32,        // 12  0
    value: i64,           // 16  amount, damage, points, weapon index, rank, ammo, value to set, ...
    value2: i64,          // 24  second value: frames, ...
    cell: i64,            // 32  cell or waypoint ref; 0 none
    house: i64,           // 40  House target, SetOwner's new owner, owner of created objects; 0: the trigger's house
    other: i64,           // 48  second object: source, passenger, team, trigger, tag; in triggers taken from the event (5.1)
    text: i32,            // 56  string parameter (CSF label, theme, EVA name, country, log line); copied by trigger_create
    text_len: i32,        // 60
    extra: i32,           // 64  variable-size data where listed (module code only)
    extra_len: i32,       // 68
}
```

代码注释说明：`id` 是类型 ID（武器、弹头、超级武器、动画、单位类型、声音、TeamType……），对 `SetProperty` 和 `Modify` 是属性 ID，对 `SetVar` 是变量 ID，对 `Module` 是分派键；`arg` 是表中列出的小枚举（占格规则、修饰器运算、任务、结果……）；`arg2` 是表中列出的修饰器组；`value` 是数量、伤害、分数、武器槽位、等级、弹药、要写入的值等；`value2` 是第二个值（帧数等）；`cell` 是格子或路径点引用；`house` 是 `House` 目标、`SetOwner` 的新所属方或所创建对象的所属方，为 0 时使用触发器的所属方；`other` 是第二个对象（来源、乘客、小队、触发器、标签），在触发器中取自事件（5.1）；`text` 是字符串参数（CSF 条目名、音乐、EVA 名称、国家、日志文字），`trigger_create` 会复制它；`extra` 是表中列出时的变长数据，只用于模块代码。

`value` 和 `value2` 没有缺失值：用到它们的种类按给定值读取，不用的种类忽略它们，所以默认值 0 不是哨兵值。

**客户端事件记录**，每条 32 字节，在 `client_on_events` 的批次中原地读取：

```rust
struct ClientEventRecord { // 32 bytes
    kind: i32,             // 0   client event kind (4.6)
    source: i32,           // 4   Notify: 0 mod, 1 map; 0 otherwise
    node: i64,             // 8   Click, Hover: the node; 0 otherwise
    id: i32,               // 16  Hotkey: hotkey ID; Notify: notify ID; -1 otherwise
    value: i32,            // 20  Hover: 1 entered, 0 left
    data: i32,             // 24  Notify: the bytes, in the delivery area
    data_len: i32,         // 28
}
```

代码注释说明：`kind` 是客户端事件种类（4.6）；`source` 对 `Notify` 为发送模块的来源，0 为 mod、1 为地图；`node` 对 `Click` 和 `Hover` 是节点；`id` 对 `Hotkey` 是热键 ID，对 `Notify` 是通知 ID；`value` 对 `Hover` 为 1 表示进入、0 表示离开；`data` 和 `data_len` 是通知的数据，放在投递区中。

**其他布局：**

```rust
struct PropValue {        // 16 bytes: client_node_create's initial properties
    prop: i32,            // 0  property ID
    reserved: i32,        // 4  0
    value: i64,           // 8
}
struct Order {            // 24 bytes, then count unit handles (i64): client_send_order
    kind: i32,            // 0   Command (8.3)
    count: i32,           // 4   number of units that follow
    target: i64,          // 8   object or cell ref; 0 none
    value: i64,           // 16  type ref for production commands; mode for others; 0 otherwise
}
struct ProjectileSpec {   // 32 bytes: CreateProjectile's extra data and each entry of a Fire shot plan
    type: i32,            // 0   projectile type ID
    weapon: i32,          // 4   weapon ID that supplies damage and warhead; -1 none
    target: i64,          // 8   object or cell ref; 0: the shot's target
    data: [u8; 16],       // 16  initial module data, saved and hashed
}
```

代码注释说明：`PropValue` 是 `client_node_create` 的初始属性。`Order` 是 `client_send_order` 的输入：`kind` 是 `Command`（8.3），`count` 是后面跟着的单位句柄数，`target` 是对象或格子引用，`value` 对生产指令是类型引用。`ProjectileSpec` 是 `CreateProjectile` 的变长数据，也是 `Fire` 射击方案的每一条：`type` 是抛射体类型 ID，`weapon` 提供伤害和弹头，`target` 为 0 时是这次射击的目标，`data` 是 16 字节的初始模块数据，存档并计入哈希。

`get_many` 和 `client_get_many` 的 `out` 是 `n × m` 个 `i64`，按行排列：第 `i` 个引用的第 `j` 个属性在 `out[i * m + j]`。已不存在的引用那一行全部写 0，`last_status()` 为 `Gone`，其他行照常写入。

### 自定义段

每个段以一个格式版本字节开头。名字是 UTF-8，必须是 ASCII，只按 ASCII 规则统一大小写（[RFC 12](0001-wasm-extension-modules.zh-CN.md#12-版本与-abi)）。SDK 构建时按项目配置写入它们。

| 段 | 内容 | 用途 |
| --- | --- | --- |
| `vera.actions` | 名字和分派键（`i32`）的列表 | INI 和地图用 `Action=Wasm:name` 引用模块动作或决定处理函数（[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)）；`lookup(ModuleAction, name)` 返回分派键 |
| `vera.notify` | 模拟模块发送的通知名 | 加载时检查哈希冲突（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） |
| `vera.vars` | 每个变量的名字、作用域（0 全局、1 所属方、2 对象）、可见度（`Visibility`）和默认值（`i64`） | 变量声明（[RFC 6e](0001-wasm-extension-modules.zh-CN.md#6e-变量)）；只出现在模拟模块中 |
| `vera.delivery` | 投递区在模块内存中的偏移和字节数 | 引擎把事件记录、决定记录及其结果区、客户端事件和通知数据写在这里，再把指针交给 `on_events`、`on_decide` 或 `client_on_events` |

## 1. 模拟 ABI

接口 `vera:sim@1`。签名写成 core Wasm 的类型；`ptr` 和 `len` 是 `i32`。每个函数的费用写在接口定义中（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）。

### 1.1 导入

一共 23 个。“可用于”一列：`init` 为初始化函数 `vera_init`；`decide` 为 `on_decide`，只允许读取函数，在其中调用其他函数会停用模块；`all` 为其他所有调用。初始化函数中不可用的函数返回 `Unsupported`。

| # | 函数 | 签名 | 语义 | 可用于 |
| --- | --- | --- | --- | --- |
| 1 | `frame` | `() -> i32` | 当前帧号 | all、decide |
| 2 | `random` | `(lo: i32, hi: i32) -> i32` | 从模块随机流取一个 `[lo, hi]` 中的整数；`lo > hi` 为非法参数 | all、decide |
| 3 | `log` | `(ptr, len)` | 向游戏内日志写一行 | init、all、decide |
| 4 | `last_status` | `() -> i32` | 上一次调用的 `Status` | init、all、decide |
| 5 | `has_kind` | `(table: i32, id: i32) -> i32` | 本引擎是否认识表 `table`（8.3 的 `Table`）中的 `id`：1 或 0 | init、all、decide |
| 6 | `lookup` | `(kind: i32, ptr, len) -> i32` | 把名字解析为 ID（8.1）；没有时为 −1 和 `NotFound` | init、all、decide |
| 7 | `name_of` | `(kind: i32, id: i32, out: ptr, cap: i32) -> i32` | 把 ID 的名字写进 `out`，返回长度 | init、all、decide |
| 8 | `get` | `(ref: i64, prop: i32) -> i64` | 读一个属性的有效值（含修饰器） | all、decide |
| 9 | `get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr) -> i32` | 读 `n` 个引用的 `m` 个属性，返回写入的值数 | all、decide |
| 10 | `set` | `(ref: i64, prop: i32, value: i64) -> i32` | 写一个属性，走它的 owner；返回状态 | all |
| 11 | `modify` | `(ref: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32) -> i64` | 加一个修饰器（`ModifierOp`），返回修饰器句柄；`frames` 为负数表示无限，`group` 为 0 表示不分组 | all |
| 12 | `unmodify` | `(modifier: i64) -> i32` | 移除修饰器；已到期时为 `Gone` | all |
| 13 | `rules_get` | `(sec: ptr, sec_len, key: ptr, key_len, out: ptr, cap: i32) -> i32` | 读出键的原始字符串，返回长度；键不存在时为 −1 和 `NotFound` | all、decide；`on_rules` |
| 14 | `rules_set` | `(sec: ptr, sec_len, key: ptr, key_len, val: ptr, val_len) -> i32` | 写一个键：`on_rules` 中任意键；对局中只限白名单（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)），否则为 `Refused` | all；`on_rules` |
| 15 | `var_get` | `(scope: i64, var: i32) -> i64` | 读变量；`scope` 为 0 表示全局，否则为所属方或对象引用 | all、decide |
| 16 | `var_set` | `(scope: i64, var: i32, value: i64) -> i32` | 写变量，重置等待它的触发器的计时器 | all |
| 17 | `find` | `(query: ptr, len, out: ptr, cap: i32) -> i32` | 搜索（`Query`），把引用写进 `out`，返回数量 | all、decide |
| 18 | `trigger_create` | `(def: ptr, len) -> i64` | 创建触发器（`TriggerDef`），返回句柄；路径点不存在时为 0 和 `NotFound`，没有 Ares 时用 Ares 事件为 0 和 `Unsupported` | all |
| 19 | `trigger_remove` | `(t: i64) -> i32` | 删除触发器及其所有实例（延迟进行） | all |
| 20 | `trigger_bind` | `(t: i64, ref: i64) -> i32` | 绑定到对象、类型、武器、弹头或抛射体类型，或格子 | all |
| 21 | `trigger_unbind` | `(t: i64, ref: i64) -> i32` | 解除一个绑定 | all |
| 22 | `action` | `(kind: i32, target: i64, params: ptr, len) -> i64` | 执行一个动作（第 5 节，`ActionParams`）；返回创建的对象，否则为 0 | all |
| 23 | `send_notify` | `(to: i64, id: i32, ptr, len) -> i32` | 向所属方 `to` 的玩家发通知；`to` 为 0 时发给观战者 | all |

- `on_rules` 中可用：名字解析函数（`lookup`、`name_of`、`has_kind`、`last_status`）、`log`、`rules_get`、`rules_set`，以及对类型引用和全局规则引用的 `get`、`get_many`、`set`。触发器、动作、变量、修饰器、`find` 和对其他引用的读写在 `on_rules` 中返回 `Unsupported`。
- 写入函数是 `set`、`modify`、`unmodify`、`rules_set`、`var_set`、`trigger_create`、`trigger_remove`、`trigger_bind`、`trigger_unbind`、`action` 和 `send_notify`。

### 1.2 导出

一共 7 个。模块只导出它用到的函数；没有导出的函数没有开销。何时运行：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。

| 导出 | 签名 | 时机 | 层级 |
| --- | --- | --- | --- |
| `vera_init` | `()` | 由 SDK 从顶层代码生成（模块没有 `start` 段）；一次，在第一个其他调用之前 | v1 |
| `on_rules` | `()` | 一次，在模块所处的规则层位置 | v1 |
| `on_scenario_start` | `()` | 一次，在场景开始时 | v1 |
| `on_frame` | `()` | 每帧，在模块阶段的最后；SDK 运行用 `onFrame(new Every(N), fn)` 注册的函数 | v1 |
| `on_events` | `(ptr, count: i32)` | 每帧每个有排队记录的模块一次，在 `on_message` 之后；`ptr` 指向 `count` 条 `EventRecord` | v1 |
| `on_decide` | `(ptr, count: i32)` | 在决策点同步调用；`ptr` 指向 `count` 条 `DecideRecord` | v1? |
| `on_message` | `(sender: i64, ptr, len)` | 同一来源的客户端模块的消息到达模拟时；`sender` 是发送者的所属方 | v1（阶段 2） |

没有 `on_load`：模块内存随快照恢复。抛射体弹道以后用可决定事件 `Launch` 和 `ProjectileFrame` 实现（4.5），不增加导出。

## 2. 客户端 ABI

接口 `vera:client@1`。每个函数都以 `client_` 开头；加载器拒绝导入 `client_*` 的模拟模块，也拒绝导入模拟函数的客户端模块（[RFC 1](0001-wasm-extension-modules.zh-CN.md#1-两个世界)）。客户端允许使用浮点：客户端世界不是确定性的。客户端额度：[RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。

### 2.1 导入

一共 21 个。读取只给出本玩家自己的界面能显示的内容（第 7 节各表的“客户端”一列）。

| # | 函数 | 签名 | 语义 |
| --- | --- | --- | --- |
| 1 | `client_frame` | `() -> i32` | 最近一次算完的模拟帧号 |
| 2 | `client_random` | `(lo: i32, hi: i32) -> i32` | 本机随机数；不确定，只用于显示 |
| 3 | `client_log` | `(ptr, len)` | 写入游戏内控制台 |
| 4 | `client_last_status` | `() -> i32` | 上一次调用的 `Status` |
| 5 | `client_has_kind` | `(table: i32, id: i32) -> i32` | 同 `has_kind` |
| 6 | `client_lookup` | `(kind: i32, ptr, len) -> i32` | 同 `lookup`；变量和通知名查的是本局模拟模块声明过的名字 |
| 7 | `client_name_of` | `(kind: i32, id: i32, out: ptr, cap: i32) -> i32` | 同 `name_of` |
| 8 | `client_get` | `(ref: i64, prop: i32) -> i64` | 读一个属性；看不到的对象为 `Gone`，不可见的属性为空值和 `Refused` |
| 9 | `client_get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr) -> i32` | 同 `get_many`，规则同上 |
| 10 | `client_find` | `(query: ptr, len, out: ptr, cap: i32) -> i32` | 同 `find`，只返回本玩家能看到的结果 |
| 11 | `client_var_get` | `(scope: i64, var: i32) -> i64` | 读变量，受它的可见度限制（[RFC 6e](0001-wasm-extension-modules.zh-CN.md#6e-变量)） |
| 12 | `client_set` | `(ref: i64, prop: i32, value: i64) -> i32` | 写客户端拥有的引用的属性：节点、镜头、本地设置 |
| 13 | `client_set_bytes` | `(ref: i64, prop: i32, ptr, len) -> i32` | 写字符串属性（`Label`、`Art`、`Items`……） |
| 14 | `client_node_create` | `(kind: i32, parent: i64, props: ptr, len) -> i64` | 新建节点（`NodeKind`），带初始属性（`PropValue` 数组）；`parent` 为 0 表示顶层 |
| 15 | `client_node_remove` | `(node: i64) -> i32` | 删除节点及其子节点 |
| 16 | `client_node_find` | `(ptr, len) -> i64` | 按名字找节点（`veraui.ini` 中的节名，或 `Name` 属性）；没有时为 0 |
| 17 | `client_action` | `(kind: i32, target: i64, params: ptr, len) -> i32` | 执行一个本地动作（5.4，`ActionParams`） |
| 18 | `client_send_order` | `(ptr, len) -> i32` | 发出一条标准玩家指令（`Order`），像任何玩家输入一样校验 |
| 19 | `client_send_message` | `(ptr, len) -> i32` | 把消息放进命令流，到达同一来源的模拟模块的 `on_message`；观战者为 `Refused`；限制大小和频率（[RFC 11](0001-wasm-extension-modules.zh-CN.md#11-联机)） |
| 20 | `client_storage_read` | `(key: ptr, key_len, out: ptr, cap: i32) -> i32` | 读取模块来源的存储，返回长度；没有时为 −1 和 `NotFound`（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） |
| 21 | `client_storage_write` | `(key: ptr, key_len, data: ptr, data_len) -> i32` | 写入存储；超出配额时为 `Refused` |

在 `client_init` 中只有 `client_lookup`、`client_name_of`、`client_has_kind`、`client_last_status` 和 `client_log` 可用。

### 2.2 导出

一共 4 个。

| 导出 | 签名 | 时机 | 层级 |
| --- | --- | --- | --- |
| `client_init` | `()` | 由 SDK 从顶层代码生成；一次，在第一个其他调用之前 | v1（阶段 2） |
| `client_on_start` | `()` | 一次，在对局界面建好、INI 布局读完之后；对重新加入的玩家，在追赶完成后运行一次 | v1（阶段 2） |
| `client_on_render` | `(dt: f32, alpha: f32)` | 每个渲染帧一次；`dt` 和 `alpha` 只用于显示 | v1（阶段 2） |
| `client_on_events` | `(ptr, count: i32)` | 每个渲染帧一次，在 `client_on_render` 之前，带本帧的客户端事件（`ClientEventRecord`，4.6） | v1（阶段 2） |

## 3. SDK 对应

SDK 从接口定义和本附录的表生成包装函数，每个包装函数恰好对应一次宿主调用（[RFC 5a](0001-wasm-extension-modules.zh-CN.md#5a-语言与-sdk)）。下表以 TypeScript 为例；Rust 用蛇形命名，引用是带方法的 newtype（`t.health()`、`t.set_owner(h)`）。包名和类名只是占位。

| 用途 | TypeScript | 宿主调用 |
| --- | --- | --- |
| 读属性 | `Techno.health(t)`、`House.money(h)`、`Cell.landType(c)` | `get` |
| 写属性 | `Techno.setOwner(t, h)`、`Techno.setMission(t, Mission.Hunt)`、`Trigger.setEnabled(t, false)` | `set` |
| 批量读 | `Techno.read(list, [Prop.Health, Prop.Owner])` | `get_many` |
| 字符串属性 | `House.name(h)` 返回 `string` | `get` 加 `name_of` |
| 修饰器 | `Modifier.add(t, Prop.SpeedMultiplier, ModifierOp.Multiply, fixed(0.5), 150, src, 0)`、`Modifier.remove(m)` | `modify`、`unmodify` |
| 动作 | 每个动作种类一个函数：`Action.nukeStrike(cell)`、`Action.launchSuperWeapon(house, type, cell)`、`Action.createObject(cell, type, house)`、`Action.addMoney(house, amount)` | `action` |
| 触发器中的动作 | `ActionDef.damage(Target.Other, warhead, 50)`、`ActionDef.setVar(Target.Subject, v, 1)`：只构造 `ActionDef`，不执行 | — |
| 触发器 | `Trigger.on(Event.Fire).bind(ref).where(Prop.HealthPercent, Op.Lt, 30).whereVar(Scope.Subject, v, Op.Eq, 0).once().house(h).act(def).action(fn)`；可决定事件用 `.decide(fn)`；只有原版和原子动作时用 `.done()` 结束 | `.action`、`.decide`、`.done` 调用 `trigger_create`，再对每个 `.bind` 调用 `trigger_bind` |
| 绑定 | `Trigger.bind(t, ref)`、`Trigger.unbind(t, ref)`、`Trigger.remove(t)`、`Trigger.bindAllTypes(t, TypeKind.Techno)` | `trigger_bind`、`trigger_unbind`、`trigger_remove`；`bindAllTypes` 对每个类型调用一次 `trigger_bind` |
| 变量 | `Var.id(name)`、`Var.get(scope, v)`、`Var.set(scope, v, value)`、`Var.local(n)`、`Var.global(n)`、`Var.GLOBAL` | `lookup`、`var_get`、`var_set` |
| 名字 | `Rules.technoType("E1")`、`Rules.weapon("RedEye2")`、`Rules.superWeaponType("NukeSpecial")` 返回类型引用 | `lookup` |
| 规则 | `Rules.get(section, key)`、`Rules.set(section, key, value)` | `rules_get`、`rules_set` |
| 搜索 | `Find.objects().inRadius(cell, leptons).owner(h).where(...).first()`；`.all(buffer)`；`House.byName(name)` | `find` |
| 其他 | `Cell.at(x, y)`、`Waypoint.cell(n)`（只编码引用，不调用宿主）；`Notify.id(name)`、`Notify.send(to, id, data)` | —；`lookup`、`send_notify` |
| 注册 | 顶层的 `onScenarioStart(fn)`、`onFrame(new Every(N), fn)`、`onMessage(fn)`、`namedAction(name, fn)`；客户端的 `onStart(fn)`、`onRender(fn)`、`onClientEvent(ClientEvent.Click, fn)` | SDK 生成的导出 |
| 客户端 | `Ui.find(name)`、`Ui.create(NodeKind.Text, parent)`、`Node.setLabel(n, text)`、`Node.setVisible(n, v)`、`Client.viewer()`、`Client.sendMessage(data)`、`Client.order(...)`、`Storage.read(key)` | `client_*` |

- 在模块动作和决定处理函数中，事件记录以 `EventRecord` 和 `DecideRecord` 视图交给函数，SDK 原地读取，不复制。`DecideRecord` 的 `setResult(v)` 写 `result` 并把 `handled` 设为 1。
- `Target` 是 `ActionTarget`（5.1）在 SDK 中的名字。
- 触发器构建器中，`.once()` 把重复类型设为一次性，`.house(h)` 设定所属方；`.where` 的第一个参数可以是 `Prop`，也可以用 `.whereOther`、`.whereHouse` 判断另一方和所属方。
- 没有官方 SDK 的语言用机器可读的表生成同样的包装函数。

## 4. 事件种类

投递、顺序和时机：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。绑定、筛选子句和轮询：[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)。可决定事件：[RFC 6f](0001-wasm-extension-modules.zh-CN.md#6f-可决定事件)。被绑定的实例：[RFC 7](0001-wasm-extension-modules.zh-CN.md#7-运行时的触发器和标签)。

- 每个事件写明它填入哪些 `EventRecord` 字段。
- “绑定”一列：`对象` 表示只对绑定了触发器的对象（对象绑定、类型绑定或标签）引发；`全局` 表示不需要绑定。
- “可决定”一列为“是”的事件，在带决定处理函数的触发器中以决定请求的形式在决策点引发；“结果槽”写明 `result` 的含义，`native` 是同样含义的原生值。
- 由 House 标签引发的原版事件每 8 帧轮询一次，由 Logic 标签引发的每帧轮询，与原版相同。

### 4.1 原版事件 0..61

类型：`R` 在某个时刻引发（投递型），`H` House 标签轮询，`L` Logic 标签轮询。记录字段：`S` 为 `subject`，`O` 为 `other`，`Hs` 为 `house`。全部为 `v1`，都不可决定。

| # | API 名称 | 参数 | 类型 | 记录字段 | 绑定 | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | — | 不公开：永远不成立。 |
| 1 | `EnteredBy` | `value` 所属方下标（-1 表示任意） | R | S、O、保留的 Hs | 对象 | 会写持久状态。 |
| 2 | `SpiedBy` | 忽略 | R | S、O | 对象 | |
| 3 | `ThievedBy` | `value` 所属方下标 | R | S、Hs | 对象 | |
| 4 | `DiscoveredByPlayer` | 忽略 | R | S、O | 对象 | |
| 5 | `HouseDiscovered` | `value` 所属方 | H | Hs | 全局 | |
| 6 | `AttackedByAnyHouse` | 忽略 | R | S、O | 对象 | `AttackedBy` 的粗略形式。 |
| 7 | `DestroyedByAnyHouse` | 忽略 | R | S、O | 对象 | 需要击杀来源。 |
| 8 | `AnyEvent` | — | H | S | 全局 | 每 8 帧轮询一次。 |
| 9 | `AllUnitsDestroyed` | `value` 所属方 | H | Hs | 全局 | |
| 10 | `AllBuildingsDestroyed` | `value` 所属方 | H | Hs | 全局 | |
| 11 | `AllDestroyed` | `value` 所属方 | H | Hs | 全局 | |
| 12 | `CreditsExceed` | `value` 金钱 | H | Hs | 全局 | |
| 13 | `ElapsedTime` | `value` 秒（×15 帧） | L | — | 全局 | |
| 14 | `MissionTimerExpired` | — | L | — | 全局 | |
| 15 | `BuildingsDestroyedCount` | `value` 数量 | H | Hs | 全局 | |
| 16 | `UnitsDestroyedCount` | `value` 数量 | H | Hs | 全局 | |
| 17 | `NoFactoriesLeft` | — | H | Hs | 全局 | |
| 18 | `CiviliansEvacuated` | — | H | Hs | 全局 | |
| 19 | `BuildBuildingType` | `value` BuildingType 下标 | H | Hs | 全局 | 会写持久状态。也接受 `id`。 |
| 20 | `BuildUnitType` | `value` VehicleType 下标 | H | Hs | 全局 | 同 19。 |
| 21 | `BuildInfantryType` | `value` InfantryType 下标 | H | Hs | 全局 | 同 19。 |
| 22 | `BuildAircraftType` | `value` AircraftType 下标 | H | Hs | 全局 | 同 19。 |
| 23 | `TeamLeavesMap` | `id` TeamType | R | — | 全局 | |
| 24 | `ZoneEntryBy` | `value` 所属方（-1 表示任意） | R | S、O、保留的 Hs | 对象 | |
| 25 | `CrossesHorizontalLine` | 同 24 | R | O、保留的 Hs | 格子 | |
| 26 | `CrossesVerticalLine` | 同 24 | R | O、保留的 Hs | 格子 | |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — | 全局 | |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — | 全局 | |
| 29 | `DestroyedByAnythingNotInfiltrate` | 忽略 | R | S、O | 对象 | |
| 30 | `LowPower` | `value` 所属方 | H | Hs | 全局 | |
| 31 | `BridgeDestroyed` | 忽略 | R | `cell` | 格子 | |
| 32 | `BuildingExists` | `value` BuildingType 下标 | H | Hs | 全局 | 会写持久状态。 |
| 33 | `SelectedByPlayer` | 忽略 | R | S | 对象 | 联机对局中不引发。客户端形式：`SelectionChanged`（4.6）。 |
| 34 | `ComesNearWaypoint` | `value` 路径点 | R | S | 对象 | 距离在 0x500 lepton 以内。 |
| 35 | `EnemyInSpotlight` | 忽略 | R | S | 对象 | |
| 36 | `LocalIsSet` | `value` 0..99 | L | — | 全局 | |
| 37 | `LocalIsClear` | `value` 0..99 | L | — | 全局 | |
| 38 | `FirstDamagedCombat` | 忽略 | R | S、O | 对象 | 有伤害来源。 |
| 39 | `HalfHealthCombat` | 忽略 | R | S、O | 对象 | 黄血。 |
| 40 | `QuarterHealthCombat` | 忽略 | R | S、O | 对象 | 红血。 |
| 41 | `FirstDamagedAny` | 忽略 | R | S、O（可为 0） | 对象 | |
| 42 | `HalfHealthAny` | 忽略 | R | S | 对象 | |
| 43 | `QuarterHealthAny` | 忽略 | R | S | 对象 | |
| 44 | `AttackedByHouse` | `value` 所属方数组下标 | R | S、O | 对象 | Ares 会转换 Player@X。 |
| 45 | `AmbientLightBelow` | `value` | L | — | 全局 | 环境光变化时检查。 |
| 46 | `AmbientLightAbove` | `value` | L | — | 全局 | 同 45。 |
| 47 | `ElapsedScenarioTime` | `value` 秒 | L | — | 全局 | |
| 48 | `DestroyedByAnything` | 忽略 | R | S、O（可为 0） | 对象 | `Killed` 的粗略形式。 |
| 49 | `PickupCrate` | 忽略 | R | S | 对象 | |
| 50 | `PickupCrateAny` | 忽略 | L | — | 全局 | 有箱子被捡起时检查。 |
| 51 | `RandomDelay` | `value` 秒 | L | — | 全局 | 随机数：native。 |
| 52 | `CreditsBelow` | `value` 金钱 | H | Hs | 全局 | |
| 53 | `SpyAsHouse` | `value` 所属方 | R | S、Hs | 对象 | 会写持久状态。 |
| 54 | `SpyAsInfantry` | `value` InfantryType 下标 | R | S | 对象 | 会写持久状态。 |
| 55 | `NavalUnitsDestroyed` | `value` 所属方 | H | Hs | 全局 | |
| 56 | `LandUnitsDestroyed` | `value` 所属方 | H | Hs | 全局 | |
| 57 | `BuildingDoesNotExist` | `value` BuildingType 下标 | H | Hs | 全局 | |
| 58 | `PowerFull` | `value` 所属方 | H | Hs | 全局 | |
| 59 | `EnteredOrOverflownBy` | 同 1 | R | S、O、保留的 Hs | 对象 | |
| 60 | `TechTypeExists` | `id` 类型，`value` 数量 | L | — | 全局 | |
| 61 | `TechTypeDoesNotExist` | `id` 类型 | L | — | 全局 | |

### 4.2 Ares 事件 62..88

只有游戏按 Ares 方式读取时（`rules.ares_form()`，而不只是 `rules.ares()`：按原版方式读取的触发器数据保持原版形式）才可用；否则 `trigger_create` 返回 0，状态为 `Unsupported`，`has_kind` 返回 0。全部为 `v1`，都不可决定；`R` 事件填入 `subject`，绑定为对象，`H` 和 `L` 事件为全局。

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
| 77 | `SuperWeaponNearWaypoint` | `id` 超级武器类型，`value` 路径点（5 格） | H |
| 78 | `HouseReverseEngineeredType` | `id` 类型 | H |
| 79 | `SomethingReverseEngineered` | — | R |
| 80 | `TypeReverseEngineered` | `id` 类型 | R |
| 81 | `HouseOwnsAtLeast` | `id`、`value` | H |
| 82 | `HouseOwnsAtMost` | `id`、`value` | H |
| 83 | `AttackedOrDestroyedByAnybody` | — | R |
| 84 | `AttackedOrDestroyedByHouse` | `value` 所属方 | R |
| 85 | `DestroyedByHouse` | `value` 所属方 | R |
| 86 | `TypeCountAtMost` | `id`、`value` | L |
| 87 | `HouseNoKeepAliveObjects` | `value` 所属方 | H |
| 88 | `HouseNoKeepAliveBuildings` | `value` 所属方 | H |

### 4.3 新的投递型事件

新事件的编号取自 Ares 和 Phobos 都没有占用的范围（待定问题 5）。在安全点引发（[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)）。“绑定”为 `对象+武器` 的事件也可以绑定在武器、弹头或抛射体类型上（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）。高频事件只对绑定的对象引发。

| 事件 | 记录字段 | 引发时机 | 绑定 | 可决定 | 结果槽 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| `TakeDamage` | `other` 来源（可为 0），`value` 护甲结算后的伤害，`id` 弹头 | 在批量伤害提交结束之后 | 对象 | 是：在伤害提交之前 | 最终伤害；伤害步骤是批量提交时成批决定 | v1? |
| `HitByWarhead` | `id` 弹头，`value` 最终伤害，`other` 来源 | 在伤害被接收之后；免疫的命中不算 | 对象 | 否 | — | v1? |
| `AttackedBy` | `other` 攻击者，`value` 武器槽位（-1 表示无），`projectile`（可为 0） | 在目标上引发，在开火步骤之后 | 对象 | 否 | — | v1? |
| `FatalDamage` | `other` 攻击者（可为 0），`value` 伤害，`id` 弹头 | 在致命伤害提交之后、死亡流程之前；死亡流程会重新检查 | 对象 | 是：在死亡流程中 | 死亡方式：第 0 到 31 位为死亡武器 ID 加 1（0 为没有），第 32 位为产生幸存者，第 33 位为留下残骸 | v1? |
| `Killed` | `other` 击杀者（可为 0） | 在击杀记录之后 | 对象 | 否 | — | v1? |
| `Fire` | `other` 目标（可为 0），`cell` 目标格子，`value` 武器槽位，`id` 武器，`projectile`（可为 0） | 在射击完成之后 | 对象+武器 | 是：在射出之前 | 射击方案的条数，方案是结果区中的 `ProjectileSpec` 数组（[RFC 9](0001-wasm-extension-modules.zh-CN.md#9-示例抛射体弹道)） | v1? |
| `SpawnFire` | `other` 子机，`value` 武器槽位，`projectile`；目标在 `cell` 中，或通过子机的 `Target` 属性获得 | 在子机射击之后 | 对象 | 否 | — | v1? |
| `HitTarget` | `other` 目标，`value` 伤害，`id` 武器，`projectile` | 在开火者上引发，在批量伤害提交结束之后 | 对象+武器 | 否 | — | v1? |
| `TargetChanged` | `other` 新目标（清除目标时为 0） | 在目标设定或清除之后；来自并行目标扫描时，在扫描结束之后 | 对象 | 是：在设定目标之前 | 选中的目标引用（0 为不选）；候选用 `find` 读取；在并行扫描之后成批决定 | v1? |
| `ProjectileIncoming` | `projectile`，`other` 发射者（可为 0） | 在抛射体被接纳之后 | 对象 | 否 | — | v1? |
| `Detonated` | `projectile`，`cell`，`other` 来源（可为 0），`id` 弹头 | 在引爆之后 | 对象+武器 | 否 | — | v1? |
| `Birth` | `other` 生产者（可为 0），`value` 生产时间（-1 表示无） | 在对象放置之后（生产、增援、生成） | 对象 | 否 | — | v1? |
| `TransportEntered` | 在乘客（`subject`）上引发；`other` 载具或建筑，`value` 已占空间，`value2` 1 为驻军 / 0 为载员 | 在进入完成之后 | 对象 | 否 | — | v1? |
| `TransportExited` | 同 `TransportEntered` | 在离开完成之后 | 对象 | 否 | — | v1? |
| `PassengerAdded` | 在载具或建筑（`subject`）上引发；`other` 乘客，`value` 已占空间，`value2` 驻军标志 | 同 `TransportEntered` | 对象 | 否 | — | v1? |
| `PassengerRemoved` | 同 `PassengerAdded` | 同 `TransportExited` | 对象 | 否 | — | v1? |
| `Crushed` | `other` 碾压者 | 在碾压之后 | 对象 | 否 | — | v1? |
| `HouseTransfer` | `house` 新所属方，`value` 为 1 表示心控 | 在转移之前；转移会重新检查 | 对象 | 否 | — | v1? |
| `HouseTransferred` | `house` 原所属方，`value` 为 1 表示心控 | 在转移之后 | 对象 | 否 | — | v1? |
| `ModifierStart` | `modifier`，`value` 属性 ID，`other` 来源（可为 0） | 在修饰器加入之后 | 对象 | 否 | — | v1? |
| `ModifierExpire` | `modifier`（投递后读出为不存在），`value` 属性 ID | 在修饰器到期或被移除之后 | 对象 | 否 | — | v1? |
| `ModifierStacks` | `modifier`，`value` 同组的新修饰器数 | 在同组修饰器数增加之后 | 对象 | 否 | — | v1? |
| `Timer` | `value` 上一次触发的帧 | 每隔事件定义的 `interval` 帧 | 全局 | 否 | — | v1? |
| `SuperWeaponLaunched` | `id` 超级武器类型，`house`，`cell` 目标；让模块实现超级武器的效果 | 在超级武器发射之后 | 全局 | 否 | — | v1? |

### 4.4 新的轮询型事件

每隔事件定义的 `interval` 帧检查一次，在状态翻转时触发，并按创建序号错开（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）。比较方式是用事件定义的 `op` 与 `value` 比较。所属方事件为全局，其他为对象；都不可决定。

| 事件 | 读取内容 | 层级 |
| --- | --- | --- |
| `HealthPoll` | 生命值；`id` 为 1 时为百分比 | v1? |
| `AmmoPoll` | 弹药数 | v1? |
| `PassengersPoll` | 乘客数 | v1? |
| `TransportSpacePoll` | 剩余空间 | v1? |
| `StoragePoll` | 载矿量 | v1? |
| `MissionPoll` | 当前任务等于 `value` | v1? |
| `MovingPoll` | `value` 1 为移动，0 为停止 | v1? |
| `LandTypePoll` | 对象所在格子的地形类型等于 `value` | v1? |
| `HouseMoneyPoll` | 触发器所属方的可用金钱 | v1? |
| `HousePowerPoll` | 发电量减耗电量 | v1? |
| `ModifierCountPoll` | 主体上组 `id` 中、来自本模块来源的修饰器数 | v1? |
| `DistancePoll` | 到记录中 `other` 的距离，单位 lepton | v1? |
| `VariablePoll` | 变量 `id`：按它声明的作用域读主体、触发器所属方或全局的值；变量被写入时重置计时器 | v1? |

每隔一段时间起作用的修饰器用 `Timer` 触发器加筛选子句（例如比较被修饰的属性或一个变量）实现。

### 4.5 只可决定的事件

只能用于带决定处理函数的触发器；没有投递形式。

| 事件 | 记录字段 | 决策点 | 绑定 | 结果槽 | 层级 |
| --- | --- | --- | --- | --- | --- |
| `ProductionAllowed` | `house`，`id` 类型 | 每帧对每个所属方和每个绑定类型各一次，在固定的步骤；侧边栏和 AI 读取缓存的结果 | 类型 | 1 允许，0 不允许 | v1? |
| `CommandAllowed` | `subject`，`value` 指令（`Command`），`other` 目标对象，`cell` 目标格子 | 绑定对象每收到一条玩家或 AI 指令，在模拟执行这条指令时；光标的本地预览不引发 | 对象 | 1 允许，0 拒绝 | v1? |
| `Launch` | `projectile`，`subject` 发射者，`other` 目标，`cell` 目标格子 | 绑定类型的每个抛射体射出时一次 | 对象+武器 | 路径点数；结果区中是路径（种类加点列：贝塞尔、抛物线、螺旋、路径点） | later |
| `ProjectileFrame` | `projectile`，`value` 已飞行的帧数 | 每帧每个绑定的抛射体类型一批，带该类型的全部抛射体 | 抛射体类型 | 第 0 位请求引爆；结果区中是新的速度（三个 `i64`，lepton 每帧） | later |

### 4.6 客户端事件种类

客户端事件在 `client_on_events` 中按渲染帧成批到达，记录为 `ClientEventRecord`。全部为 `v1`（阶段 2）。

| 种类 | 记录字段 | 何时 | 条件 |
| --- | --- | --- | --- |
| `Click` | `node` | 节点被点击 | 节点的 `Clickable` 为 1 |
| `Hover` | `node`，`value` 1 进入 / 0 离开 | 光标进入或离开节点 | 节点的 `Hoverable` 为 1 |
| `Hotkey` | `id` 热键 ID | 模块注册的热键被按下 | 用本地动作 `RegisterHotkey` 注册（5.4） |
| `SelectionChanged` | — | 本地选择发生变化；选择用带 `Selected=1` 子句的 `client_find` 读取 | 总是 |
| `Notify` | `source`，`id` 通知 ID，`data`、`data_len` | 发给这台机器的通知到达（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） | 总是 |

## 5. 动作种类

### 5.1 动作的用法

动作在触发器中运行（`TriggerDef` 后面的 `ActionDef`），或由模块代码用 `action` 直接执行。时机：[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。目标、固定规则和失败处理：[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-动作)。模块动作的名字解析：[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)。

```rust
enum ActionTarget { Subject = 0, Other = 1, TagHolder = 2, Projectile = 3, House = 4, Cell = 5 }   // in triggers; Subject is the default
```

- 在触发器中，目标是 `ActionTarget`；`other` 字段取自事件的另一方对象，`ActionParams.other` 被忽略。
- 直接调用时，目标是一个引用；`ActionParams.other` 给出第二个对象（来源、乘客、小队、触发器或标签）。
- 下面的“目标”列中：*对象*指 `Subject`、`Other` 或持有对象的 `TagHolder`（直接调用时为对象句柄）；*格子*指 `Cell`，或对象目标所在的格子，包括 `Projectile`（直接调用时为格子、路径点或对象引用）；`House` 为所属方；`—` 表示没有目标。某个种类没有列出的目标种类永远不适用于它（属于非法参数）。
- 每个原版动作在原版中作用于标签持有者的那些（标为 *live*），在触发器中默认目标为 `TagHolder`。
- 原版动作 `n` 启用 Ares 时先走 Ares 的分支。除非另有说明，每行都是 `v1`；运行原生代码的每行随机数都是 `native`。

### 5.2 原版动作 0..145

参数一列写的是 `ActionParams` 的字段。原版用路径点的地方，API 用 `cell` 字段中的格子或路径点引用。

| # | API 名称 | 目标 | 参数 | 说明 |
| --- | --- | --- | --- | --- |
| 0 | — | — | — | 空操作。 |
| 1 | `DeclareWinner` | — | `text` 国家 | 作用于玩家，而不是触发器所属方。 |
| 2 | `DeclareLoser` | — | `text` 国家 | |
| 3 | `BeginProduction` | `House` | — | |
| 4 | `CreateTeam` | `House` | `id` TeamType | 受数量上限限制。小队由 S0–S68 运行。所属方为 0 时用 TeamType 的所属方；返回小队句柄，没有创建时为 0。在触发器中还会加上原版动作的对象放置范围；直接调用时没有这一步，House AI 也没有。 |
| 5 | `DestroyTeams` | — | `id` TeamType | |
| 6 | `AllToHunt` | `House` | — | |
| 7 | `Reinforce` | — | `id` TeamType | 使用 TeamType 自己的路径点。 |
| 8 | `DropZoneFlare` | 格子 | — | |
| 9 | `FireSale` | `House` | — | |
| 10 | `PlayMovie` | — | `value` 影片编号 | 对每个玩家生效的表现效果。 |
| 11 | `ShowText` | — | `text` CSF 条目名 | |
| 12 | `DestroyTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 删除运行中的实例（延迟进行）。与 `trigger_remove` 相同。 |
| 13 | `BeginAutocreate` | `House` | — | |
| 14 | `ChangeHouse` | 对象（*live*） | `house` 新所属方 | 与原子动作 `SetOwner` 相同。 |
| 15 | `AllowWin` | — | — | 只是标志（`TriggerDef.flags` 第 1 位），没有可执行的分支。 |
| 16 | `RevealAll` | — | — | |
| 17 | `RevealAround` | 格子 | — | |
| 18 | `RevealZone` | 格子 | — | |
| 19 | `PlaySound` | — | `id` 声音 | |
| 20 | `PlayTheme` | — | `text` 音乐 | |
| 21 | `PlaySpeech` | — | `text` EVA 名称 | |
| 22 | `ForceTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 触发时没有执行者，格子为零。 |
| 23 | `TimerStart` | — | — | |
| 24 | `TimerStop` | — | — | |
| 25 | `TimerExtend` | — | `value` 秒数 | |
| 26 | `TimerShorten` | — | `value` 秒数 | |
| 27 | `TimerSet` | — | `value` 秒数 | |
| 28 | `SetGlobal` | — | `value` 0..49 | 超出范围：参数非法。与 `var_set` 相同。 |
| 29 | `ClearGlobal` | — | `value` 0..49 | |
| 30 | `AutoBaseBuilding` | `House` | `value` 开关 | |
| 31 | `GrowShroud` | — | — | |
| 32 | `DestroyObject` | 对象（*live*） | — | 适用 Ares 的幸存者规则。与原子动作 `Kill` 相同。 |
| 33 | `OneTimeSuperWeapon` | `House` | `id` 超级武器类型 | |
| 34 | `RepeatingSuperWeapon` | `House` | `id` 超级武器类型 | |
| 35 | `PreferredTarget` | `House` | `value` | |
| 36 | `AllChangeHouse` | `House` | `house` 新所属方 | |
| 37 | `MakeAlly` | `House` | `house` 另一所属方 | 双向。 |
| 38 | `MakeEnemy` | `House` | `house` 另一所属方 | 双向。 |
| 39 | — | — | — | 不开放。 |
| 40 | `SetVisibleMapArea` | 格子（左上角） | `value` 宽，`value2` 高 | |
| 41 | `PlayAnimAt` | 格子 | `id` 动画 | 适用 Ares `MakeInfantry=` 的所属规则。 |
| 42 | `DetonateWeaponAt` | 格子 | `id` 武器 | 与原子动作 `DetonateWeapon` 相同，没有来源。 |
| 43 | `PlayVoxelAnimAt` | 格子 | `id` 体素动画 | |
| 44 | `IonStormStart` | — | `value` 持续时间 | 风暴进行中时为 `Refused`。 |
| 45 | `IonStormStop` | — | — | 没有风暴时为 `Refused`。 |
| 46 | `LockInput` | — | — | |
| 47 | `UnlockInput` | — | — | |
| 48 | `CenterCamera` | 格子 | `value` 速度 | 作用于每个玩家的镜头。客户端形式见 5.4。 |
| 49 | — | — | — | 放大。不开放。 |
| 50 | — | — | — | 缩小。不开放。 |
| 51 | `Reshroud` | `House` | — | |
| 52 | `SpotlightBehavior` | 对象（*live*，建筑） | `value` | |
| 53 | `EnableTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 按难度准入；重置触发器的计时器。与写 `Enabled` 属性相同。 |
| 54 | `DisableTrigger` | — | `other` 触发器，或 `text` 触发器 ID | |
| 55 | `RadarEvent` | 格子 | `value` `RadarEventType` | |
| 56 | `SetLocal` | — | `value` 0..99 | |
| 57 | `ClearLocal` | — | `value` 0..99 | |
| 58 | `MeteorShower` | 格子 | `value` 强度 | |
| 59 | `ReduceOre` | 格子 | — | |
| 60 | `SellBuilding` | 对象（*live*，建筑） | — | |
| 61 | `TurnOffBuilding` | 对象（*live*，建筑） | — | 与写 `Online` 属性为 0 相同。 |
| 62 | `TurnOnBuilding` | 对象（*live*，建筑） | — | |
| 63 | `DamageBridgeAt` | 格子 | — | |
| 64 | `LightFlashSmall` | 格子 | — | 伤害为 50 的命中所产生的闪光。 |
| 65 | `LightFlashMedium` | 格子 | — | 伤害 100。 |
| 66 | `LightFlashLarge` | 格子 | — | 伤害 300。 |
| 67 | `AnnounceWin` | — | — | |
| 68 | `AnnounceLose` | — | — | |
| 69 | `EndScenario` | — | — | |
| 70 | `DestroyTag` | — | `other` 标签，或 `text` 标签 ID | 停止正在进行的投递。 |
| 71 | `SetAmbientStep` | — | `value` | |
| 72 | `SetAmbientRate` | — | `value` | |
| 73 | `SetAmbientLight` | — | `value` | |
| 74 | `AiTriggersOn` | `House` | — | 与写所属方的 `AiTriggersActive` 属性相同。 |
| 75 | `AiTriggersOff` | `House` | — | |
| 76 | `RatioAiTriggers` | `House` | `value` | |
| 77 | `RatioAircraft` | `House` | `value` | |
| 78 | `RatioInfantry` | `House` | `value` | |
| 79 | `RatioUnits` | `House` | `value` | |
| 80 | `ReinforceAt` | 格子 | `id` TeamType | |
| 81 | `WakeUp` | 对象（*live*） | — | 睡眠或无害的对象改为 Guard。 |
| 82 | `WakeSleepers` | `House` | — | 唤醒不属于该所属方的睡眠对象。 |
| 83 | `WakeHarmless` | `House` | — | 唤醒属于该所属方的无害对象。 |
| 84 | `WakeGroup` | — | `value` 编组 | |
| 85 | `VeinGrowth` | — | `value` | |
| 86 | `OreGrowth` | — | `value` | |
| 87 | `IceGrowth` | — | `value` | |
| 88 | `CreateParticleSystem` | 格子 | `id` 粒子系统 | |
| 89 | `RemoveParticleSystems` | 格子 | — | |
| 90 | `LightningStrike` | 格子 | — | |
| 91 | `GoBerserk` | 对象（*live*，步兵） | — | |
| 92 | `FirestormOn` | `House` | — | 未启用 Ares 时为 `Unsupported`。 |
| 93 | `FirestormOff` | `House` | — | 同上。 |
| 94 | `IonCannonStrike` | 格子 | — | |
| 95 | `NukeStrike` | 格子 | — | |
| 96 | `ChemStrike` | 格子 | — | |
| 97 | `ToggleTrainCargo` | — | — | |
| 98 | `PlaySoundAtRandomWaypoint` | — | `id` 声音 | Ares 变体；从原生随机流取数。 |
| 99 | `PlaySoundAt` | 格子 | `id` 声音 | 与原子动作 `PlaySound` 相同。 |
| 100 | `PlayRadarMovie` | — | `value` 影片 | |
| 101 | `ReshroudAt` | 格子 | — | |
| 102 | `LightningStormStrike` | 格子 | — | 使用触发器所属方。 |
| 103 | `TimerText` | — | `text` CSF 条目名 | |
| 104 | `FlashTeam` | — | `id` TeamType，或 `other` 小队；`value` 帧数 | |
| 105 | `TalkBubble` | — | `other` 小队（可为 0），`value` 帧 | |
| 106 | `SetTechLevel` | — | `id` 类型，`value` 等级 | 另见 7.7。 |
| 107 | `ReinforceByChrono` | 格子 | `id` TeamType | |
| 108 | `CreateCrate` | 格子 | `value` 种类（20 为随机） | 随机时从原生随机流取数。 |
| 109 | `IronCurtainAt` | 格子 | — | |
| 110 | `PauseGame` | — | `value` 持续时间 | |
| 111 | `EvictOccupants` | 对象（*live*，建筑） | — | |
| 112 | `JumpCamera` | 格子 | — | |
| 113 | `Cheer` | `House` | — | |
| 114 | `SetSidebarTab` | — | `value` 0..3 | |
| 115 | `FlashCameo` | — | `id` 类型，`value` 持续时间 | |
| 116 | `StopSoundsAt` | 格子 | — | |
| 117 | `PlayRadarMovieAndPause` | — | `value` 影片 | |
| 118 | `ClearSmudges` | — | — | |
| 119 | `DestroyAll` | `House` | — | |
| 120 | `DestroyAllBuildings` | `House` | — | |
| 121 | `DestroyAllLandUnits` | `House` | — | |
| 122 | `DestroyAllNavalUnits` | `House` | — | |
| 123 | `MindControlBase` | `House` | `house` 被控制的所属方 | |
| 124 | `RestoreMindControlledBase` | `House` | `house` 被控制的所属方 | |
| 125 | `CreateBuildingAt` | 格子 | `id` 建筑类型 | 返回建筑，没有建成时为 0。 |
| 126 | `RestoreStartingTechnos` | `House` | — | |
| 127 | `ChronoScreenEffect` | — | `value` 持续时间 | |
| 128 | `TeleportAll` | `House` | `cell` 目的地 | |
| 129 | `SuperWeaponSetCharge` | `House` | `id` 超级武器类型，`value` 百分比 | 与写超级武器的 `ChargePercent` 属性相同。 |
| 130 | `RestoreStartingBuildings` | `House` | — | |
| 131 | `FlashBuildings` | `House` | `id` 类型，`value` 持续时间 | |
| 132 | `SuperWeaponSetRecharge` | `House` | `id` 超级武器类型，`value` 帧数 | |
| 133 | `SuperWeaponResetRecharge` | `House` | `id` 超级武器类型 | |
| 134 | `SuperWeaponReset` | `House` | `id` 超级武器类型 | |
| 135 | `SetPreferredTargetCell` | `House` | `cell` | |
| 136 | `ClearPreferredTargetCell` | `House` | — | |
| 137 | `SetAlternateBase` | `House` | `cell` | |
| 138 | `ClearAlternateBase` | `House` | — | |
| 139 | `RadarBlackout` | `House` | `value` 帧数 | |
| 140 | `SetDefensiveTargetCell` | `House` | `cell` | |
| 141 | `ClearDefensiveTargetCell` | `House` | — | |
| 142 | `RetintRed` | — | `value` | |
| 143 | `RetintGreen` | — | `value` | |
| 144 | `RetintBlue` | — | `value` | |
| 145 | `HomeCamera` | — | — | |

Ares 动作（仅在启用 Ares 时可用，否则为 `Unsupported`）：

| # | API 名称 | 目标 | 参数 |
| --- | --- | --- | --- |
| 146 | `AddAuxPower` | `House` | `value` 电力 |
| 147 | `KillDrivers` | `House` | — |
| 148 | `SetEvaVoice` | — | `value` |
| 149 | `SetGroup` | 对象 | `value` 编组 |

### 5.3 原子动作

一个种类对应一个操作，这个操作只有一个 owner（[RFC 6](0001-wasm-extension-modules.zh-CN.md#6-五个概念)）。编号属于待定问题 5。“仅代码”的种类需要变长数据，只能用 `action` 直接执行。

| 名称 | 目标 | 参数与第二个对象 | 返回 | 语义 | 随机数 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| `FireWeapon` | 对象（射手） | `id` 不是 −1 时为武器，否则为 `value` 武器槽位（默认 0，即主武器）；瞄准 `cell`（若给出），否则瞄准 `other`，再否则瞄准射手当前的目标；三者都没有时为 `Gone` | — | 开火；武器当前无法开火时为 `Refused` | native | v1? |
| `LaunchSuperWeapon` | `House` | `id` 超级武器类型，`cell`（必需：缺少时属于非法参数） | — | 发射该所属方拥有的超级武器 | native | v1? |
| `SpawnAnim` | 格子 | `id` 动画 | — | 创建动画 | native | v1? |
| `AttachAnim` | 对象 | `id` 动画，`value2` 帧数（负数为直到移除） | — | 把动画附着在对象上（原“附着动画”效果） | native | v1 |
| `Remove` | 对象 | — | — | 移除对象：不死亡、不产生幸存者、不计击杀 | none | v1? |
| `Kill` | 对象 | `other` 为来源（可选） | — | 走伤害流程击杀，伤害等于当前生命并无视防御（有死亡动画和幸存者，击杀记给来源） | native | v1? |
| `Damage` | 对象 | `id` 弹头，`value` 伤害；`other` 为来源（可选） | — | 经过弹头和护甲造成伤害，计击杀和经验 | native | v1? |
| `SetHealth` | 对象 | `value` 生命点数 | — | 设定生命，不计击杀（同写 `Health`） | none | v1? |
| `Heal` | 对象 | `value` 生命点数 | — | 回复生命，不计击杀 | none | v1? |
| `CreateObject` | 格子 | `id` 类型；`house` 为所属方；`arg` 初始任务（`Mission`，255 为默认）；`other` 不为 0 时复制它的标签 | 新对象 | 创建一个完整的对象；格子被占时为 `Refused` 并返回 0 | native | v1? |
| `SetMission` | 对象 | `arg` 任务，`cell` 或 `other` 为任务目标 | — | 像玩家指令一样排入一个任务；带格子的 `Move` 移动到那里 | none | v1 |
| `Teleport` | 对象 | `cell`，`arg` 目标格被占时的规则（`Blocked`） | — | 超时空式移动；`Nearby` 选附近的格子，`Fail` 直接拒绝 | native | v1? |
| `ClearTarget` | 对象 | — | — | 放弃当前目标 | none | v1? |
| `ClearDisguise` | 对象 | — | — | 解除间谍或幻影坦克的伪装 | none | v1? |
| `ClearMindControl` | 对象 | — | — | 解除心灵控制，把对象还给原所属方 | none | v1? |
| `PlaySound` | 格子 | `id` 声音 | — | 在格子中心播放声音 | none | v1? |
| `AddExperience` | 对象 | `value` 经验点数 | — | 增加经验 | none | v1? |
| `SetRank` | 对象 | `value` 等级（`Rank`） | — | 设定等级（同写 `Rank`） | none | v1? |
| `AddMoney` | `House` | `value` 金额（可为负） | — | 增加或扣除金钱 | none | v1? |
| `SetTechLevel` | — | `id` 类型，`value` 科技等级 | — | 修改科技树（同原版动作 106） | none | v1? |
| `AllowType` | `House` | `id` 类型，`value` 1 为允许、0 为不允许 | — | 修改该所属方的可建造列表 | none | v1? |
| `SetOwner` | 对象 | `house` 为新所属方 | — | 转移所属（同写 `Owner`） | none | v1? |
| `SetAmmo` | 对象 | `value` 弹药 | — | 设定弹药（同写 `Ammo`） | none | v1? |
| `AddAmmo` | 对象 | `value` 弹药 | — | 增加弹药 | none | v1? |
| `AddPassenger` | 对象（载具） | `other` 为乘客 | — | 让乘客登上 | native | v1? |
| `RemovePassenger` | 对象（载具） | `other` 为乘客 | — | 让乘客离开 | native | v1? |
| `SetProperty` | 对象、`House` 或格子 | `id` 属性，`value` 值 | — | 在触发器中写一个可写属性，同 `set` | none | v1 |
| `Modify` | 对象或 `House` | `id` 属性，`arg` 运算（`ModifierOp`），`arg2` 组，`value` 值，`value2` 帧数；`other` 为来源（可选） | 修饰器 | 在触发器中加一个修饰器，同 `modify` | none | v1 |
| `RemoveModifiers` | 对象或 `House` | `arg2` 组；`id` 不是 −1 时只移除该属性上的 | — | 移除本模块来源在该组中的修饰器 | none | v1 |
| `SetVar` | 对象、`House` 或 — | `id` 变量，`value` 值 | — | 写变量；目标按变量声明的作用域取主体、所属方或忽略（全局），同 `var_set` | none | v1 |
| `DetonateWeapon` | 格子 | `id` 武器；`other` 为来源（可选） | — | 不生成抛射体，直接在格子中心引爆武器 | native | v1? |
| `DetonateWarhead` | 格子 | `id` 弹头，`value` 伤害；`other` 为来源（可选） | — | 按给定伤害和来源造成弹头的范围伤害 | native | v1? |
| `CreateProjectile` | 对象或格子（起点） | `extra` 为一个 `ProjectileSpec`；`other` 为来源（击杀和经验归属，可为 0） | 抛射体 | 创建抛射体；失败时为 0。仅代码 | native | v1? |
| `DetonateProjectile` | `Projectile` | — | — | 让抛射体立即引爆 | native | later |
| `RemoveProjectile` | `Projectile` | — | — | 移除抛射体，不引爆 | none | later |
| `EndFor` | `House` | `arg` 结果（`Outcome`） | — | 让某个所属方结束游戏，胜利或失败 | none | v1? |
| `SetScore` | `House` | `value` 分数 | — | 设置分数 | none | v1? |
| `AddScore` | `House` | `value` 分数 | — | 增加分数 | none | v1? |
| `Log` | — | `text` | — | 向游戏内日志写一行 | none | v1 |
| `Convert` | 对象 | `id` 类型，`value2` 帧数（0 为永久；大于 0 时到期后变回原类型） | — | 改变类型，同 Ares 的类型转换；限时形式取代“变成另一类型”效果，与传送和运动互斥 | native | v1? |
| `Push` | 对象 | `cell` 方向目标，`value` 力度 | — | 运动：被推动或抛起，由引擎叠加到对象的移动上；与变形和传送互斥 | native | later |
| `Module` | 不限 | `id` 分派键 | — | 运行模块动作：在模块阶段排队（[RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)）；INI 写作 `Action=Wasm:name` | none | v1 |
| `CreateTeamUnlimited` | `House` | `id` TeamType | 小队 | 启动一个小队，不受 Max 限制，与脚本动作 18 相同 | native | v1? |
| `DisbandTeam` | — | `other` 小队 | — | 销毁小队，记录 AI 触发器的结果，释放成员 | none | v1 |
| `AddTeamMember` | 对象 | `other` 小队 | — | 加入小队，会执行小队的准入检查 | none | v1? |
| `RemoveTeamMember` | 对象 | — | — | 离开所在的小队 | none | v1? |

### 5.4 客户端动作种类

用 `client_action(kind, target, params, len)` 执行，只影响本机。参数同样用 `ActionParams`。全部为 `v1`（阶段 2），镜头动作为 `v1?`。

| 名称 | 目标 | 参数 | 语义 |
| --- | --- | --- | --- |
| `PlaySound` | 格子或 0 | `id` 声音 | 在本机播放声音；目标为 0 时不定位 |
| `PlaySpeech` | — | `text` EVA 名称 | 在本机播放 EVA 语音 |
| `CenterCamera` | 格子 | `value` 速度 | 移动本机镜头，同动作 48 |
| `JumpCamera` | 格子 | — | 让本机镜头跳转，同动作 112 |
| `HomeCamera` | — | — | 同动作 145 |
| `Select` | — | `extra` 为对象句柄数组，`value` 1 为追加 | 本地选择；只接受本玩家能选的对象 |
| `FlashObject` | 对象 | `value` 帧数 | 让对象在本机闪烁 |
| `ShowMessage` | — | `text` CSF 条目名或文字，`value` 颜色 | 显示一行消息 |
| `RegisterHotkey` | — | `text` 热键名，`extra` 默认按键（文字） | 注册热键，之后用 `client_lookup(Hotkey, name)` 取它的 ID |

## 6. AI 小队

电脑所属方保留它们自己的 AI（[RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)）。启动和解散小队、加入和移出成员是动作（5.2 的 4、5、7、80、107，5.3 的小队动作）；小队和 AI 触发器的状态是属性（7.9）。

### 6.1 定义

定义是 INI，在 `on_rules` 中用 `rules_set` 写入，经过原生读取器；运行时的调用使用 `lookup(TeamType | ScriptType | AiTrigger, name)` 返回的 ID 和对应的类型引用。

| 节 | 内容 | 说明 |
| --- | --- | --- |
| `[TaskForces]` 和每个特遣部队的节 | `Name`、`Group`、`0=数量,类型` 等行 | 最多 6 行 |
| `[ScriptTypes]` 和每个脚本的节 | `Name`、`0=动作,参数` 等行 | 最多 50 行；动作编号见 6.2 |
| `[TeamTypes]` 和每个小队类型的节 | `Name`、`Script`、`TaskForce`、`Priority`、`Max`、`Group`、`House`、`VeteranLevel`、`Waypoint`、`TransportWaypoint`、`IsBaseDefense`、`Suicide`、`Aggressive`、`UseTransportOrigin`、`Full`、`Whiner`、`Autocreate`、`AreTeamMembersRecruitable`、`Reinforce`、`AvoidThreats`、`OnTransOnly`、`Recruiter`、`Annoyance`、`GuardSlower`、`TransportsReturnOnUnload`、`MindControlDecision`、`Droppod`、`OnlyTargetHouseEnemy`、`Loadable`、`Tag` | 默认值来自读取器 |
| `[AITriggerTypes]` | 每行 18 项：名字、主小队、所属方、条件、对象类型、数量、比较符、起始权重、最小权重、最大权重、多人、阵营、次小队、简单、普通、困难…… | 写入后它的当前权重变为初始权重 |

- `Whiner` 会被读取，但没有效果。
- `TechLevel`、`Prebuild`、`LooseRecruit` 和 `IonImmune` 出现在原版 AIMD.INI 中，但不会被读取。AI 触发器的第 3、11、13 项没有被使用。
- AI 触发器的条件：-1 总是成立；0 敌方拥有 N 个该类型；1 本所属方拥有；2 敌方电力盈余低于 100；3 低于 0；4 比较敌方金钱；5 己方铁幕即将就绪；6 己方超时空传送仪即将就绪；7 平民阵营的所属方拥有。比较符 0..8 与 `Compare`（8.3）相同。
- 运行时修改 AI 触发器的权重是 `later`。

### 6.2 小队脚本动作

`[ScriptTypes]` 每行的动作取以下编号。作为数据，全部是 `v1`。“对应的动作或属性”一列给出模块要对单个对象做同样的事时使用的表项。

| # | `ScriptAction` | 参数 | 说明 | 对应的动作或属性 |
| --- | --- | --- | --- | --- |
| 0 | `AttackQuarry` | 目标类别（2..11） | | `SetMission`（`Hunt`） |
| 1 | `AttackWaypoint` | 路径点 | | `SetMission`（`Attack`，格子） |
| 2 | `GoBerserk`（惯用名） | — | 空操作；永不结束。 | — |
| 3 | `MoveToWaypoint` | 路径点 | | `SetMission`（`Move`，格子） |
| 4 | `MoveToCell` | 打包的格子（x = arg%128，y = arg/128） | | `SetMission`（`Move`，格子） |
| 5 | `Guard` | `arg*15` 帧 | | `SetMission`（`Guard`） |
| 6 | `JumpToLine` | 从 1 开始的行号 | | 小队的 `Cursor` 属性 |
| 7 | `PlayerWins` | — | | `EndFor` |
| 8 | `Unload` | 1、2、3 | | `RemovePassenger` |
| 9 | `Deploy` | — | | — |
| 10 | `FollowFriendlies` | — | | — |
| 11 | `DoMission` | 任务 ID | | `SetMission` |
| 12 | `SetGlobal` | 下标 | | `SetVar`（全局变量） |
| 13 | `IdleAnimation` | 动画 | | — |
| 14 | `LoadOntoTransport` | — | | `AddPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | 路径点 | | — |
| 16 | `PatrolToWaypoint` | 路径点 | | `SetMission`（`Patrol`） |
| 17 | `ChangeScript` | ScriptType 序号 | | 小队的 `Script` 属性 |
| 18 | `ChangeTeam` | TeamType 序号 | | — |
| 19 | `Panic` | — | | — |
| 20 | `ChangeHouse` | 国家下标 | | `SetOwner` |
| 21 | `Scatter` | — | | — |
| 22 | `GoToNearbyShroud` | — | 非单人模式下拒绝执行。 | — |
| 23 | `PlayerLoses` | — | | `EndFor` |
| 24 | `PlaySpeech`（惯用名） | — | 只前进到下一行。 | — |
| 25 | `PlaySound` | SOUNDMD 序号 | | 原版动作 19 |
| 26 | `PlayMovie` | 影片 | | 原版动作 10 |
| 27 | `PlayMusic` | 音乐主题 | | 原版动作 20 |
| 28 | `ReduceOre` | — | | 原版动作 59 |
| 29 | `BeginProduction` | — | | 原版动作 3 |
| 30 | `FireSale` | — | | 原版动作 9 |
| 31 | `SelfDestruct` | — | | `Kill` |
| 32 | `IonStormStart` | 持续时间 | | 原版动作 44 |
| 33 | `IonStormEnd` | — | | 原版动作 45 |
| 34 | `CenterCamera` | 速度 | | 原版动作 48 |
| 35 | `ReshroudMap` | — | | 原版动作 51 |
| 36 | `RevealMap` | — | | 原版动作 16 |
| 37 | `DeleteMembers` | — | | `Remove` |
| 38 | `ClearGlobal` | 下标 | | `SetVar`（全局变量） |
| 39 | `SetLocal` | 下标 | | `SetVar`（局部变量） |
| 40 | `ClearLocal` | 下标 | | `SetVar`（局部变量） |
| 41 | `Unpanic` | — | | — |
| 42 | `ForceFacing` | 朝向 | | — |
| 43 | `WaitUntilFullyLoaded` | — | | — |
| 44 | `TruckUnload` | — | | `Convert` |
| 45 | `TruckLoad` | — | | `Convert` |
| 46 | `AttackEnemyBuilding` | 类型 `arg&0xFFFF`，模式 `arg>>16` | | — |
| 47 | `MoveToEnemyBuilding` | 同上 | | — |
| 48 | `ScoutHouses` | — | | — |
| 49 | `RegisterSuccess` | — | 前进，并把小队标记为成功。 | 小队的 `Succeeded` 属性（只读） |
| 50 | `Flash` | 帧数 | | 原版动作 104 |
| 51 | `PlayAnimation` | 低 16 位为动画，高 16 位为循环次数 | | `SpawnAnim` |
| 52 | `TalkBubble` | 帧 | | 原版动作 105 |
| 53 | `GatherAtEnemyBase` | Ares：+ `AISafeDistance` | | — |
| 54 | `RegroupAtBase` | Ares：+ `AIFriendlyDistance` | | — |
| 55 | `IronCurtain` | Ares：`SW.Group` | | `LaunchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | 同 46 | | `Teleport` |
| 57 | `ChronoshiftToQuarry` | 目标类别 | | `Teleport` |
| 58 | `MoveToOwnBuilding` | 类型、模式 | | — |
| 59 | `AttackBuildingAtWaypoint` | 路径点 | | — |
| 60 | `EnterGrinder` | — | | — |
| 61 | `OccupyTankBunker` | — | | — |
| 62 | `EnterBioReactor` | — | | — |
| 63 | `OccupyBattleBunker` | — | | — |
| 64 | `GarrisonNearest` | — | | — |
| 65, 66 | — | — | 不使用。 | — |
| 67 | `AresSeekVehicle` | — | Ares；与 64 共用分支，被步骤检查挡住，执行不到。 | — |
| 68 | `AresConvert` | — | 仅 Ares。 | `Convert` |

## 7. 属性

属性按引用种类分组。列的含义：

- **值**：值类型（见[值](#值)）。
- **读写**：`R` 只读；`W` 任何时候可写，走该属性的 owner；`W(rules)` 只在 `on_rules` 中可写（经 `rules_set`）；`W(白名单)` 是对局中可写的白名单候选（待定问题 11），在其他时候和 `W(rules)` 一样。不可写时 `set` 返回 `Refused`。
- **客户端**：客户端模块按 [RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块) 的规则得到什么。`all` 与模拟中的答案相同（公开的事实）；`own` 只对玩家自己的所属方及其对象给出答案，对其他对象返回空值并设置 `Refused`；`seen` 对自己的对象给出全部答案，对其他所属方中玩家当前能看到的对象按 RFC 10 所述的限制给出答案，其他对象不存在；`sim` 只用于模拟，客户端调用返回空值并设置 `Unsupported`。
- **修**：能否用 `modify` 修饰（7.2）。
- **视点属性**取决于一个视点所属方：在筛选子句中是触发器的所属方，在搜索中是查询的 `viewpoint`，在客户端中是本玩家；直接 `get` 时为 `Unsupported`（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-对象属性与修饰器)）。
- **派生属性**由引擎算出，只读。

### 7.1 对象

单位和建筑（`Techno`）的属性。

| 属性 | 值 | 读写 | 客户端 | 修 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| `Type` | ref（类型） | R | seen（伪装的敌人返回伪装类型） | 否 | v1 | 改变类型用 `Convert` |
| `Owner` | ref（所属方） | W | seen（伪装的敌人返回伪装的所属方） | 否 | v1 | 写入即转移所属，同 `SetOwner` |
| `Category` | enum `Category` | R | seen | 否 | v1 | |
| `Health` | int | W | seen | 否 | v1 | 写入同 `SetHealth`，不计击杀 |
| `Strength` | int | R | seen | 否 | v1 | 最大生命 |
| `HealthPercent` | int | R（派生） | seen | 否 | v1 | |
| `X`、`Y`、`Z` | lepton | R | seen | 否 | v1 | 位置只能通过移动、传送和运动改变（[RFC 6](0001-wasm-extension-modules.zh-CN.md#6-五个概念)） |
| `Cell` | ref（格子） | R | seen | 否 | v1 | |
| `Layer` | enum `Layer` | R | own | 否 | v1 | |
| `LandType` | enum `LandType` | R（派生） | own | 否 | v1 | 所在格子的地形类型 |
| `Mission` | enum `Mission` | W | own | 否 | v1 | 写入像玩家指令一样排入任务，没有目标 |
| `Target` | ref | R | own | 否 | v1 | 对象或格子；清除用 `ClearTarget` |
| `Rank` | enum `Rank` | W | own | 否 | v1 | 写入同 `SetRank` |
| `Experience` | int | R | own | 否 | v1 | 增加用 `AddExperience` |
| `Ammo` | int | W | own | 否 | v1 | |
| `Storage` | int | R | own | 否 | v1 | 载矿量 |
| `Transport` | ref | R | own | 否 | v1 | 所在的载具或建筑，没有时为 0；乘客用带 `Transport = t` 子句的 `find` 读取 |
| `PassengerCount` | int | R | own | 否 | v1 | |
| `TransportSpaceLeft` | int | R（派生） | own | 否 | v1 | |
| `Team` | ref（小队） | R | sim | 否 | v1 | 加入和离开用小队动作 |
| `Tag` | ref（标签） | W | sim | 否 | v1 | 写入一个已有的标签，和原版一样替换原有的标签（[RFC 7](0001-wasm-extension-modules.zh-CN.md#7-运行时的触发器和标签)） |
| `DisguiseType`、`DisguiseOwner` | ref | R | own | 否 | v1 | 模拟读到的是真实状态；没有伪装时为 0 |
| `Cloaked` | bool | R | own | 否 | v1? | |
| `Moving` | bool | R | own | 否 | v1 | |
| `Armor` | id（`Armor`） | R | own | 否 | v1 | |
| `Online` | bool | W | own | 否 | v1 | 建筑：写入同原版动作 61、62 |
| `Selected` | bool | R | 客户端专用 | 否 | v1（阶段 2） | 本地选择；模拟中为 `Unsupported` |
| `Relation` | enum `Relation` | R（视点） | own | 否 | v1 | 与视点所属方的关系 |
| `NearbyEnemyCount` | int | R（派生） | own | 否 | v1? | 视野（`Sight`）内的敌方对象数 |
| `TargetInRange` | bool | R（派生） | own | 否 | v1? | 当前目标在主武器射程内（原探测 `inRange`） |
| `CanFireAtTarget` | bool | R（派生） | own | 否 | v1? | 现在能向当前目标开火（原探测 `canFireAt`） |
| 可修饰属性 | 见 7.2 | | | 是 | | |

### 7.2 可修饰属性

旧版本中的持续效果种类变成了下面这些属性。它们的基础值来自类型或默认值，有效值由修饰器算出（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-对象属性与修饰器)）；不用 `modify` 时也可以直接读。全部只对 `modify` 可写（`set` 为 `Refused`），客户端为 `own`，除非另行注明。

| 属性 | 值 | 基础值 | 常用运算 | 原效果种类 | 对应的 Phobos 键 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| `ArmorMultiplier` | fixed | 1.0 | 乘 | 受伤倍率 | `ArmorMultiplier`（`.AllowWarheads`、`.DisallowWarheads` 等弹头过滤只在 Phobos 的效果类型中可用） | v1 |
| `FirepowerMultiplier` | fixed | 1.0 | 乘 | 火力倍率 | `FirepowerMultiplier` | v1 |
| `SpeedMultiplier` | fixed | 1.0 | 乘 | 速度倍率 | `SpeedMultiplier` | v1 |
| `ROFMultiplier` | fixed | 1.0 | 乘 | 射速倍率 | `ROFMultiplier`、`ROFMultiplier.ApplyOnCurrentTimer` | v1 |
| `WeaponRangeMultiplier` | fixed | 1.0 | 乘 | 射程 | `WeaponRange.Multiplier` | v1 |
| `WeaponRangeExtra` | lepton | 0 | 加 | 射程 | `WeaponRange.ExtraRange` | v1 |
| `TargetPriorityMultiplier` | fixed | 1.0 | 乘 | 目标优先级倍率（配合 `TargetChanged`） | — | v1? |
| `CritMultiplier` | fixed | 1.0 | 乘 | 暴击 | `Crit.Multiplier` | v1 |
| `CritExtraChance` | fixed | 0 | 加 | 暴击 | `Crit.ExtraChance` | v1 |
| `DamageReceivedExtra` | int | 0 | 加 | 伤害修正（固定加值）；强制取值用可决定的 `TakeDamage` | — | v1? |
| `ReflectDamagePercent` | fixed | 0 | 加 | 反弹伤害 | `ReflectDamage`、`ReflectDamage.*` | v1 |
| `RevengeWeapon` | id（武器） | −1 | 覆盖 | 复仇武器 | `RevengeWeapon`、`RevengeWeapon.*` | v1 |
| `PrimaryWeapon`、`SecondaryWeapon` | id（武器） | 类型的武器 | 覆盖 | 替换武器（特殊属性） | — | v1? |
| `Unkillable` | bool | 0 | 覆盖 | 阻止死亡 | `Unkillable` | v1 |
| `Cloakable` | bool | 类型的 `Cloakable` | 覆盖 | 隐形 | `Cloakable` | v1 |
| `ForceDecloak` | bool | 0 | 覆盖 | 隐形 | `ForceDecloak` | v1 |
| `DisableWeapons` | bool | 0 | 覆盖 | 禁止开火 | `DisableWeapons` | v1 |
| `DisableTargeting` | bool | 0 | 覆盖 | 停止索敌 | — | v1? |
| `Selectable` | bool | 1 | 覆盖 | 无法被选中 | — | v1? |
| `IgnoreOrders` | bool | 0 | 覆盖 | 无视玩家指令 | — | v1? |
| `Targetable` | bool | 1 | 覆盖 | 不可被选为目标 | — | v1? |
| `PenetratesForceShield` | bool | 0 | 覆盖 | 穿透力场护盾 | `PenetratesForceShield` | v1 |
| `ImmunityFlags` | mask | 类型的免疫 | 覆盖 | 对攻击类型免疫（特殊属性）：第 0 位心灵控制、1 辐射、2 毒素、3 心灵、4 超时空、5 EMP、6 狂暴；对单个弹头免疫用 `ArmorMultiplier` 为 0 的 Phobos 效果类型，或可决定的 `TakeDamage` | —（`ArmorMultiplier.AllowWarheads` 倍率为 0 可覆盖一部分） | v1? |
| `TintColor` | int（RGB） | 0 | 覆盖 | 染色；客户端为 `seen` | `Tint.Color` | v1 |
| `TintIntensity` | fixed | 0 | 加 | 染色；客户端为 `seen` | `Tint.Intensity`、`Tint.Cumulative` | v1 |
| `LaserTrailType` | id | −1 | 覆盖 | 激光拖尾（特殊属性）；客户端为 `seen` | `LaserTrail.Type` | v1 |
| `PowerOutage` | bool | 0 | 覆盖 | 所属方断电（所属方属性，7.5） | — | v1? |

成为动作而不是属性的效果种类：

| 原效果种类 | 现在 | 原因 |
| --- | --- | --- |
| 附着动画 | 动作 `AttachAnim`（带帧数） | 纯视觉，附着关系由动画 owner 持有 |
| 变成另一类型 | 动作 `Convert`（带帧数，到期变回） | 类型不是一个可以叠加的值 |
| 运动（被抛起、被推动） | 动作 `Push`（later） | 运动由引擎叠加到移动上，从不以坐标形式写入 |
| 传送 | 动作 `Teleport` | 瞬时操作 |

变形、超时空移动和运动互斥，由 owner 保证同一时间只有一个生效。

**Phobos AttachEffect 的键**只在 Phobos 开关打开时读取；读取后，每种效果类型生效时，引擎按上表在同一个 owner 中加入修饰器。效果类型其余的键这样对应：

| Phobos 键 | 对应 |
| --- | --- |
| `Duration`；`AttachEffect.Durations`、`AttachEffect.DurationOverrides` | 修饰器的帧数 |
| `AttachEffect.Delays`、`AttachEffect.InitialDelays` | Phobos 效果自己的延迟；模块用 `Timer` 触发器实现 |
| `AttachEffect.RecreationDelays` | Phobos 效果自己的冷却；模块用变量记录 |
| `AttachEffect.SameSourceOnly`、`AttachEffect.IgnoreFromSameSource`、`AttachEffect.CumulativeSourceMaxCount` | 修饰器的来源规则（待定问题 13） |
| `Cumulative`、`Cumulative.MaxCount`、`AttachEffect.ReplaceLongerDuration`、`AttachEffect.CumulativeRefreshAll`、`.OnAttach`、`AttachEffect.CumulativeRefreshSameSourceOnly` | 叠加上限和刷新（待定问题 13） |
| `Groups`；`RemoveGroups`、`RequiredGroups`、`DisallowedGroups` | 修饰器组 |
| `AllowTransfer.Convert`、`AllowTransfer`、`AttachEffects.AttachOnOwnerChange` | 变形和创建时是否保留修饰器 |
| `PenetratesIronCurtain` | 铁幕下是否加入 |
| `Powered` | 只在对象有电时生效 |
| `DiscardOn`、`DiscardOn.*` | 在移动、开火、受伤等情况下移除 |
| `ExpireWeapon`、`ExpireWeapon.TriggerOn`、`.CumulativeOnlyOnce`、`.UseInvokerAsOwner` | 到期时发射武器；模块用 `ModifierExpire` 事件加 `DetonateWeapon` 实现 |
| `Animation`、`CumulativeAnimations`、`Animation.*` | 附着动画 |

键一列为 `—` 的属性没有 Phobos 键，随模块功能一起打开（[RFC 14](0001-wasm-extension-modules.zh-CN.md#14-项目契约)）。

### 7.3 修饰器

修饰器是句柄（`modify` 的返回值），到期或被移除时失效。全部 `R`，客户端为 `sim`。用 `find`（范围 `Modifiers`，子句 `Target = 引用`）列出某个对象上的修饰器，按加入的顺序。

| 属性 | 值 | 说明 | 层级 |
| --- | --- | --- | --- |
| `Target` | ref | 被修饰的对象或所属方 | v1 |
| `Property` | int | 属性 ID | v1 |
| `Op` | enum `ModifierOp` | | v1 |
| `Value` | int | 原样保存的值 | v1 |
| `Remaining` | frames | 剩余帧数；无限为 −1 | v1 |
| `Source` | ref | 来源对象，可为 0 | v1 |
| `Group` | int | 0 表示不分组 | v1 |
| `Origin` | enum `Source` | 加入它的模块来源（mod、地图），或 Phobos 效果类型 | v1 |

### 7.4 抛射体

| 属性 | 值 | 读写 | 客户端 | 层级 |
| --- | --- | --- | --- | --- |
| `Type` | ref（抛射体类型） | R | seen | v1? |
| `Category` | enum `Category`：总是 `Projectile` | R | seen | v1? |
| `X`、`Y`、`Z` | lepton | R | seen | v1? |
| `Source` | ref | R | own | v1? |
| `Target` | ref | R | own | v1? |
| `Data0`、`Data1` | int | W | sim | v1?（16 字节模块数据，存档并计入哈希） |

### 7.5 所属方

| 属性 | 值 | 读写 | 客户端 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `Name` | string | R | all | v1 | |
| `Country` | ref（国家类型） | R | all | v1 | `House.byName` 用带 `Country` 子句的 `find` 实现 |
| `Side` | int | R | all | v1 | |
| `Index` | int | R | all | v1 | 所属方数组中的下标 |
| `Money` | int | R | own | v1 | 修改用 `AddMoney` |
| `PowerOutput`、`PowerDrain` | int | R | own | v1 | |
| `LowPower` | bool | R | own | v1 | |
| `TechLevel` | int | R | own | v1 | |
| `IsDefeated`、`IsHuman`、`IsCivilian` | bool | R | all | v1 | |
| `Kills`、`Losses` | int | R | own | v1? | 统计 |
| `AiTriggersActive` | bool | W | sim | v1 | 同原版动作 74、75 |
| `Relation` | enum `Relation` | R（视点） | all | v1 | 盟友和敌人用带 `Relation` 子句的 `find` 列出 |
| `PowerOutage` | bool | R | own | v1? | 可修饰（7.2） |

### 7.6 格子

格子引用的属性；格子不会失效。

| 属性 | 值 | 读写 | 客户端 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `LandType` | enum `LandType` | R | all | v1 | |
| `Height` | int | R | all | v1 | |
| `Overlay` | ref（覆盖物类型） | R | seen（玩家当前能看到的格子） | v1 | 没有时为 0 |
| `Ore` | int | R | seen | v1 | |
| `Passable.<SpeedType>` | bool | R | seen | v1 | 每种 `SpeedType` 一个属性（`Passable.Foot`、`Passable.Track`……） |
| `Revealed` | bool | R（视点） | own | v1 | 视点所属方是否已揭开该格 |
| `Placeable` | bool | R（视点） | own | v1? | 查询的 `type` 能否由视点所属方放在该格（原探测）；“最近的可放置格子”用 `find` 加模块代码 |
| `CellTag` | ref（标签） | R | sim | v1 | 没有时为 0 |
| `Waypoint` | int | R | all | v1 | 位于该格的路径点编号，没有时为 −1 |

### 7.7 类型与规则

类型引用的属性是常用 INI 键的带类型形式，读到的就是引擎实际使用的解析后的值；其他任何键用 `rules_get` 读原始字符串（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)）。写入在 `on_rules` 中可以用属性，也可以用 `rules_set`；对局中只限白名单。全部客户端为 `all`。

| 属性 | 值 | 类型 | 读写 | 层级 |
| --- | --- | --- | --- | --- |
| `Name` | string | 任意类型 | R | v1 |
| `Buildable` | bool | TechnoType | R（视点）：视点所属方的前提条件已满足 | v1 |
| `Enabled` | bool | AI 触发器 | W：启用或禁用 | v1 |

**使用时读取**（普通值，没有派生副本；对局中可写的白名单候选，`W(白名单)`）：

| 类型 | 属性（键） | 地图上已有的对象 | 说明 |
| --- | --- | --- | --- |
| Weapon | `Damage`、`ROF`、`Burst`、`MinimumRange` | 下次使用时取新值 | |
| Weapon | `Range` | 同上 | 仅当抛射体 `ROT>0` 时；`ROT=0` 时它参与计算 `Speed`（仅限 `on_rules`） |
| Weapon | `Report`、`Anim` | 同上 | 表现层；字符串，只用 `rules_set` |
| Warhead | `Verses` | 同上 | 列表，只用 `rules_set` |
| Warhead | `CellSpread`、`PercentAtMax`、`Wall`、`Wood`、`Sparky`、`Fire`、`Conventional`、`Rocker`、`InfDeath`、`AnimList` | 同上 | `AnimList` 只用 `rules_set` |
| TechnoType | `Cost`、`BuildTime` 类倍率 | 按需计算 | |
| TechnoType | `Speed` | 按需换算 | 检查各移动方式自己的状态 |
| TechnoType | `ROT`、`Points`、`Soylent`、`Crushable`、`Selfhealing`、`Explodes`、`VeteranAbilities`、`EliteAbilities` | 同上 | 能力列表只用 `rules_set` |
| TechnoType | `Power` | 重新计算每个拥有它的所属方的电力 | |
| Projectile | `Inaccurate`、`Arcing`、`Shadow` | 新生成的抛射体 | `Arcing` 需谨慎（见“仅限 on_rules”） |
| 全局 | 数值型的 `[General]`、`[CombatDamage]`、`[AudioVisual]` 字段 | 同上 | 用全局规则引用的属性或 `rules_set`；按帧换算的值需要重新换算 |

**在构造时复制到每个对象中的键**（属性只读；只有写明已有对象的处理规则后才能进白名单）：

| 键 | 需要的规则 |
| --- | --- |
| `Strength` | 保持生命点数或保持比例 |
| `Sight` | 更新每个对象及其已揭示的区域 |
| `Ammo`、`InitialAmmo` | 修改每个对象的容量 |
| `Storage` | 更新容量 |
| `Passengers`、`SizeLimit` | 更新容量；超出时怎么办 |
| 类型上的 Phobos `AttachEffect.*` | 只影响新对象 |

**仅限 `on_rules`**（派生缓存或读后处理；`W(rules)`）：

| 键 | 原因 |
| --- | --- |
| 武器 `Range`、`[General] Gravity`、抛射体 `ROT`、`Floater`、Phobos 抛射体 `Gravity` | 会改写 `ROT=0` 抛射体所用武器的 `Speed` |
| `Primary`、`Secondary`、`Weapon{n}`、`EliteWeapon{n}`、`OccupyWeapon`、武器的 `Warhead`/`Projectile` | 规则读取时解析为句柄；单个对象的替换用 `PrimaryWeapon`、`SecondaryWeapon` 修饰器 |
| `Prerequisite`、`PrerequisiteOverride`、`[General] Prerequisite*`、Ares `Prerequisite.List{n}` | 解析为建筑下标 |
| `Owner`、`RequiredHouses`、`ForbiddenHouses` | 国家位掩码 |
| `Factory` | 工厂映射 |
| `Foundation`、`[AI] AIBaseSpacing`、`BuildConst`、`IsBaseDefense`、`UndeploysInto`、`Spawned`/`MissileSpawn` | 几何信息和基地列表，复制到每个对象 |
| `Spawns`、`SpawnsNumber`、心灵控制和时间武器的设置 | 管理器只构建一次 |
| `Armor`（名称） | 解析为 `Armor` ID |
| `Image`、美术键、`Sequence` | 美术和动画绑定需要素材 |
| Ares `PoweredBy`、`PowersUnit`、`PoweredUnit` | 跨类型修正 |
| 列表节、任何指向新类型的键 | 类型只在读取规则时新增（RFC 2） |
| `[Colors]`、国家的 `Color`、`Side`、`ParentCountry` | 颜色渐变，国家和阵营的映射 |
| 声音和 EVA 键 | 加载后绑定为下标 |
| `TechLevel` | 对局中改用 `SetTechLevel`（动作 106） |
| `[TaskForces]`、`[ScriptTypes]`、`[TeamTypes]`、`[AITriggerTypes]` | AI 定义（6.1） |

Ares 和 Phobos 的键：白名单要逐个写明能否在对局中修改；目前还没有对任何一个做出分类。

### 7.8 全局引用

全局规则引用（引用种类 4）的属性：场景和全局状态。

| 属性 | 值 | 读写 | 客户端 | 层级 |
| --- | --- | --- | --- | --- |
| `Frame` | int | R | all | v1 |
| `Difficulty` | enum `Difficulty` | R | all | v1 |
| `GameMode` | string | R | all | v1 |
| `PlayerCount` | int | R | all | v1 |
| `MissionTimer` | frames | R | all | v1（修改用原版动作 23 至 27） |
| `MissionTimerRunning` | bool | R | all | v1 |
| `AmbientLight` | int | R | all | v1 |
| `IonStormActive` | bool | R | all | v1? |
| 全局数值键 | 见 7.7 | `W(白名单)` | all | v1? |

### 7.9 触发器、标签、小队和超级武器

全部客户端为 `sim`。

| 引用 | 属性 | 值 | 读写 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| 触发器 | `Enabled` | bool | W | v1 | 写 1 同原版动作 53（按难度准入，重置计时器），写 0 同 54；作用于所有实例 |
| 触发器 | `House` | ref | R | v1 | 0 表示主体的所属方 |
| 触发器 | `Repeat` | enum `TagRepeat` | R | v1 | |
| 触发器 | `MapId` | string | R | v1 | 地图中的触发器 ID；按 ID 找地图触发器用带 `MapId` 子句的 `find` |
| 标签 | `MapId` | string | R | v1 | |
| 标签 | `Repeat` | enum `TagRepeat` | R | v1 | |
| 小队 | `Type` | ref（TeamType） | R | v1 | |
| 小队 | `House` | ref | R | v1 | |
| 小队 | `Script` | ref（ScriptType） | W | v1 | 修改运行中小队的脚本 |
| 小队 | `Cursor` | int | W | v1? | 脚本游标；写入即跳到该行 |
| 小队 | `Succeeded` | bool | R | v1 | |
| 小队 | `MemberCount` | int | R | v1 | 成员用带 `Team = t` 子句的 `find` 读取 |
| 超级武器 | `Type` | ref | R | v1? | |
| 超级武器 | `Owner` | ref | R | v1? | |
| 超级武器 | `Ready` | bool | R | v1? | |
| 超级武器 | `ChargePercent` | int | W | v1? | 写入同原版动作 129 |
| 超级武器 | `RechargeTime` | frames | W | v1? | 写入同原版动作 132 |

### 7.10 客户端节点

节点和客户端全局引用只属于客户端：`client_set`、`client_set_bytes` 写它们；模拟中不存在。

**节点种类**（`NodeKind`；版本 1 的集合是待定问题 14）：

| 种类 | `veraui.ini` 的 `Type` | 层级 |
| --- | --- | --- |
| `Text` | `Text` | v1 |
| `Button` | `Button` | v1 |
| `Row`、`Column` | `Row`、`Column` | v1 |
| `CameoGrid` | `CameoGrid` | v1? |
| `List` | `List`（本 RFC 新增） | v1? |
| `Image` | — | v1? |
| `Bar`（进度条） | — | later |
| `Decoration` | —：挂在一个对象上的显示属性 | v1? |
| `MapMarker` | —：小地图或地图上的点、线、文字 | later |

**节点属性**（全部可写，除非注明；`string` 属性用 `client_set_bytes` 写）：

| 属性 | 值 | 适用于 | 说明 |
| --- | --- | --- | --- |
| `Name` | string | 全部 | `client_node_find` 用的名字；`veraui.ini` 的节点为节名，只读 |
| `Label` | string | `Text`、`Button`、`Decoration` | 文字或 CSF 条目名 |
| `Art` | string | `Button`、`Image`、`Decoration` | 一张 SHP 图片的名字 |
| `X`、`Y`、`Width`、`Height` | int | 全部 | 像素 |
| `Hold` | enum `Hold` | 全部 | |
| `Gap` | int | `Row`、`Column` | 各部分之间的像素数 |
| `Visible` | bool | 全部 | |
| `Clickable` | bool | 全部 | 为 1 时点击产生 `Click` 事件 |
| `Hoverable` | bool | 全部 | 为 1 时产生 `Hover` 事件 |
| `Items` | string | `List` | 每行一个条目（CSF 条目名或文字），以 `\n` 分隔 |
| `Fill` | int | `Bar` | 0..100 |
| `Anchor` | ref（对象） | `Decoration` | 显示在哪个对象上；对象看不见时节点也不显示 |
| `Palette` | string | `Decoration` | |
| `Tint`、`TintIntensity` | int | `Decoration` | 本机的染色；模拟中的染色是可修饰属性（7.2） |
| `Anim` | string | `Decoration` | 附加的动画 |
| `HideAnchor` | bool | `Decoration` | 在本机隐藏对象本身 |
| `Number` | int | `Decoration` | 显示在对象上方的数字 |

**INI 布局键**（静态布局使用 `veraui.ini` 的语法，[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)）：

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

节点属性用相同的名字。`Value` 只保留它在 INI 中的含义。

**客户端全局引用**（引用种类 6）：

| 引用 | 属性 | 值 | 读写 | 层级 |
| --- | --- | --- | --- | --- |
| 镜头 | `Cell` | ref（格子） | W：写入即跳转，同 `JumpCamera` | v1? |
| 本地设置 | `Viewer` | ref（所属方） | R：本机所属方，观战者为 0 | v1 |
| 本地设置 | `IsSpectator` | bool | R | v1 |
| 本地设置 | 其他本地设置 | — | 待定问题 14 | later |

## 8. 名字种类、搜索范围与其他枚举

### 8.1 名字种类（`lookup`）

| # | 种类 | 名字 | 说明 |
| --- | --- | --- | --- |
| 0 | `TechnoType` | 步兵、载具、飞行器和建筑类型 | |
| 1 | `Weapon` | 武器 | |
| 2 | `Warhead` | 弹头 | |
| 3 | `Projectile` | 抛射体（BulletType） | |
| 4 | `Anim` | 动画 | |
| 5 | `VoxelAnim` | 体素动画 | |
| 6 | `ParticleSystem` | 粒子系统 | |
| 7 | `SuperWeapon` | 超级武器类型 | |
| 8 | `Armor` | 护甲名称 | |
| 9 | `Overlay` | 覆盖物 | |
| 10 | `Sound` | 声音 | |
| 11 | `Country` | 国家（HouseType） | |
| 12 | `TeamType` | AI 小队类型 | |
| 13 | `ScriptType` | 小队脚本 | |
| 14 | `AiTrigger` | AI 触发器 | |
| 15 | `Variable` | 变量（`vera.vars`；前缀 `mod.`、`map.` 指向另一方） | 返回变量 ID |
| 16 | `Notify` | 通知（`vera.notify`） | 返回名字的哈希 |
| 17 | `ModuleAction` | 模块动作和决定处理函数（`vera.actions`） | 返回分派键 |
| 18 | `String` | 引擎已持有的字符串：CSF 条目名、音乐、EVA 名称、地图中的触发器和标签 ID | 不存在时为 `NotFound`；不新建字符串 |
| 19 | `Property` | 属性名 | 返回属性 ID |
| 20 | `EventKind` | 事件种类名 | |
| 21 | `ActionKind` | 动作种类名 | |
| 22 | `Hotkey` | 本模块注册的热键（客户端） | |

### 8.2 搜索范围（`find`）

| # | 范围 | 结果 | 顺序 | 客户端 |
| --- | --- | --- | --- | --- |
| 0 | `Objects` | 单位、建筑和抛射体的句柄 | 创建序号 | seen |
| 1 | `Houses` | 所属方句柄 | 所属方数组的顺序 | all |
| 2 | `Cells` | 格子引用 | 格子下标（先 y 后 x） | 本玩家能看到的格子 |
| 3 | `Types` | `type` 字段的种类中的类型引用（例如带 `Buildable=1` 子句列出可建造的类型） | 类型 ID | all |
| 4 | `Teams` | 小队句柄 | 创建序号 | sim |
| 5 | `Triggers` | 触发器句柄 | 创建序号 | sim |
| 6 | `Tags` | 标签句柄 | 创建序号 | sim |
| 7 | `Modifiers` | 修饰器句柄 | 加入的顺序 | sim |
| 8 | `SuperWeapons` | 超级武器句柄 | 创建序号 | own |

### 8.3 枚举

| 枚举 | 取值 |
| --- | --- |
| `Op`（筛选子句） | 0 `Eq`（`=`）、1 `Ne`（`!=`）、2 `Lt`（`<`）、3 `Le`（`<=`）、4 `Gt`（`>`）、5 `Ge`（`>=`） |
| `Compare`（AI 触发器） | 0 `Less`、1 `LessEqual`、2 `Equal`、3 `GreaterEqual`、4 `Greater`、5 `NotEqual`（与 AI 触发器比较符的顺序相同） |
| `ModifierOp` | 0 `Add`、1 `Multiply`、2 `Override` |
| `Scope`（变量） | 0 `Global`、1 `House`、2 `Object` |
| `Visibility`（变量） | 0 `Private`、1 `Allies`、2 `InSight`（只用于对象作用域）、3 `Public` |
| `Source` | 0 `Mod`、1 `Map`、2 `PhobosEffect`（修饰器的 `Origin`） |
| `Table`（`has_kind`） | 0 事件种类、1 动作种类、2 属性、3 名字种类、4 搜索范围、5 节点种类、6 客户端事件种类、7 客户端动作种类、8 其他枚举（`id` 为枚举编号乘 256 加取值） |
| `Mission` | 0 `Sleep`、1 `Attack`、2 `Move`、3 `QMove`、4 `Retreat`、5 `Guard`、6 `Sticky`、7 `Enter`、8 `Capture`、9 `Eaten`、10 `Harvest`、11 `AreaGuard`、12 `Return`、13 `Stop`、14 `Ambush`、15 `Hunt`、16 `Unload`、17 `Sabotage`、18 `Construction`、19 `Selling`、20 `Repair`、21 `Rescue`、22 `Missile`、23 `Harmless`、24 `Open`、25 `Patrol`、26 `ParadropApproach`、27 `ParadropOverfly`、28 `Deliberate`（INI 名为 `Wait`）、29 `AttackMove`、30 `SpyplaneApproach`、31 `SpyplaneOverfly`；`None` = 255。编号与地图中的编号相同。 |
| `Relation` | 0 `Self`、1 `Ally`、2 `Enemy`、3 `Neutral`（平民或被动所属方） |
| `Category` | 0 `Infantry`、1 `Vehicle`、2 `Aircraft`、3 `Building`、4 `Projectile` |
| `Layer` | `Underground`、`Surface`、`Ground`、`Air`、`Top`。水面是地形类型，不是层。 |
| `LandType` | `Clear`、`Road`、`Water`、`Rock`、`Wall`、`Tiberium`、`Beach`、`Rough`、`Ice`、`Railroad`、`Tunnel`、`Weeds` |
| `SpeedType` | `SpeedType` 这个 INI 键的取值；每个取值对应一个 `Passable.<SpeedType>` 格子属性 |
| `Difficulty` | `Easy`、`Normal`、`Hard`、`All`（触发器难度标志） |
| `TagRepeat` | 0 `OneTimeOr`、1 `OneTimeAnd`、2 `Repeating`（原版标签的重复类型） |
| `Rank` | 0 `Rookie`、1 `Veteran`、2 `Elite` |
| `Outcome` | 0 `Win`、1 `Lose` |
| `Blocked` | 0 `Nearby`、1 `Fail` |
| `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`；Ares 种类 |
| `Command` | `Move`、`MoveToObject`、`Stop`、`Attack`、`AttackMove`、`Guard`、`DeployMcv`、`UndeployBuilding`、`SetRally`、`QueueProduction`、`SuspendProduction`、`CancelProductionByType`、`PlaceReadyBuilding`、`SellBuilding`、`ToggleRepair`、`EjectBunker`、`UnloadPassengers`、`CaptureBuilding`、`PlantC4`、`LaunchSuperWeapon`、`ToggleInfantryDeploy`、`ForceAttackCell`、`SellWallAtCell` |
| `Hold` | `TopLeft`、`Top`、`TopRight`、`Left`、`Middle`、`Right`、`BottomLeft`、`Bottom`、`BottomRight`（`veraui.ini` 的 `Hold` 取值） |

除非另行给出，取值按声明顺序，由接口定义确定。
