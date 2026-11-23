# RFC 0001 附录：函数与表

[English](0001-wasm-extension-modules-api.md)

[RFC 0001](0001-wasm-extension-modules.zh-CN.md) 的参考附录，版本 0.0.20，与正文在同一次提交中修改，不一致时以正文为准；规则只在正文中陈述，版本 1 的清单见[第 9 节](#9-版本-1-清单)和各表的层级列。

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
- [9. 版本 1 清单](#9-版本-1-清单)

## 约定

### 引用

`i64`，所有值小于 2^53，0 表示“没有”；第 48 到 52 位是种类，低 48 位是内容，由 SDK 编解码。

| 种类 | 名称 | 内容 | 何时失效 |
| --- | --- | --- | --- |
| 0 | 模拟对象句柄 | 创建序号（1 到 2^48 − 1），单位、建筑、抛射体、所属方、小队、触发器、标签、修饰器、超级武器共用一个计数器 | 对象被移除；所属方从不 |
| 1 | 类型引用 | `名字种类 << 32 \| ID`（8.1）；ID 部分为 `0xFFFFFFFF` 时为该名字种类的通配引用，只用于 `find` 的 `Types` 范围 | 从不 |
| 2 | 格子引用 | `x << 16 \| y`，单位为格 | 从不 |
| 3 | 路径点引用 | 路径点编号；接收格子的函数解析为其格子，不存在时为 `NotFound` | 从不 |
| 4 | 全局规则引用 | 0：场景和全局规则（7.8） | 从不 |
| 5 | 客户端节点句柄 | 本机节点序号，与创建序号分开 | 节点被删除或对局界面关闭 |
| 6 | 客户端全局引用 | 1 镜头，2 本地设置，3 光标（7.10） | 从不 |

TypeScript 引用为 `i64` 别名，Rust 为 newtype；`NO_HANDLE = 0`。

### ID

`lookup(kind, name)` 返回稳定的 `i32` ID。无匹配时返回 `(−1, NotFound)`；传入 −1（`NO_ID`）返回 `NotFound`，其他越界 ID 为非法参数。名字种类见 [8.1](#81-名字种类lookup)。

**变量 ID**（[RFC 6e](0001-wasm-extension-modules.zh-CN.md#6e-变量)）：

| ID | 变量 |
| --- | --- |
| 0..99 | 红警 2 局部变量 0..99 |
| 100..149 | 红警 2 全局变量 0..49 |
| 1000 起 | `vera.vars` 中的变量，按加载顺序（先 mod 后地图）和声明顺序 |

事件、动作、客户端动作和属性各表内名称唯一。同名属性共用 ID，值类型和单位按引用种类解释。属性、事件和动作保留原版编号；新增事件和动作用表中写明的编号（100000..199999），只追加，表行顺序不是标识。

### 值

属性值、变量和记录中的整数都装在 `i64` 中，按值类型解释：

| 值类型 | 含义 |
| --- | --- |
| `int` | 整数；模拟 API 中没有浮点 |
| `bool` | 0 或 1；写入非零即 1 |
| `fixed` | `I16F16` 原始值（`SimFixed`），`65536` 为 `1.0`；用于倍率 |
| `lepton` | 坐标或距离，1 格 = 256 lepton |
| `frames` | 逻辑帧，每游戏秒 15 帧 |
| `enum` | 8.3 中某个枚举的值 |
| `mask` | 位掩码，各位见该属性 |
| `ref` | 引用 |
| `id` | 某个名字种类的 ID |
| `string` | 字符串 ID：用 `name_of(String, id, ...)` 读出（SDK 直接返回字符串）；用 `client_set_bytes` 写 |
| `bytes` | 字节数据，布局见该属性；用 `client_set_bytes` 写 |

### 字符串与缓冲区

- 字符串和字节缓冲区均传模块内存指针和长度。字符串按 UTF-8 编码，字节缓冲区按接口布局解释；名字比较按 ASCII 不区分大小写（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）。
- 返回数量的函数写入模块的缓冲区（`out`、`cap`），写入数量经末尾的输出指针写回。
- `len` 一律是字节数；`cap` 对字符串和字节是字节数，对引用缓冲区（`find`、`client_find` 的 `out`）是 `i64` 条目数；写回的数量单位同 `cap`。`get_many`、`client_get_many` 的 `out` 至少容纳 `n × m` 个 `i64`。
- `name_of`、`rules_get`、`client_storage_read` 把完整长度写入 `len_out`；大于 `cap` 时 `out` 中什么也不写。
- 分页调用返回状态，`count` 和 `next` 写入 `FindResult`（固定布局）。首页 `after=0`；续页沿用查询，只取排序键 ≥ `after` 的项。按 [8.2](#82-搜索范围find) 使用创建序号、所属方或格子下标、类型 ID 或稳定的修饰器加入序号。因单次扫描上限（正文 15.4）结束时，`next` 是第一个未检查的键，`count` 可能小于容量或为 0；页满时 `next=页末键+1`，下一页可能为空；其余情况 `next=0`。只有 `next=0` 表示最后一页。有效查询容量为零时写入 `count=0`、`next=0` 并返回 `Ok`，不扫描。

### 状态

```rust
enum Status {
    Ok = 0,
    Gone = 1,         // 引用为 0 或其对象已不存在
    NotFound = 2,     // 名字或 ID 没有匹配；路径点不存在
    Refused = 3,      // owner 拒绝：格子被占、属性此时不可写、配额已满
    Unsupported = 4,  // 本局关闭、本引擎不认识该表项，或此处不可用（初始化函数、决定处理函数）
}
```

- ABI：宿主导入最多返回一个值，因为稳定版 Rust 和 AssemblyScript 都无法导入多返回值的函数（Rust 把返回的值对降为隐藏的结果指针参数）。取值或创建句柄的导入返回 `status: i32`，值经末尾的输出指针以小端序写入模块内存：按签名为 `i32` 或 `i64`，分页调用为 `FindResult`。纯写操作返回状态。`frame` 和 `client_frame` 不会失败，直接返回帧号。成功为 `Ok`，失败值依各接口的空值约定；状态不为 `Ok` 时宿主仍把该空值写入输出指针。输出指针超出模块内存为非法参数。本附录中的 `(value, Status)` 指经输出指针写入的值和返回的状态。
- 模块内部仍可使用多返回值（[RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)），只有宿主导入不用。
- SDK：可只取值，也可一次取值和状态。无句柄动作直接给出状态；热路径使用标量或调用方的缓冲区。
- 停用模块：ID 或下标越界、负数量或容量、表外枚举（探测函数对未知表项返回 0）、变量作用域错误、非法动作参数或筛选子句、在 `on_decide` 中写入。

### 随机数

表中“随机数”列（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）：

| 取值 | 含义 |
| --- | --- |
| `module` | 模块自己的随机流（`random`；决定处理函数中为派生子流） |
| `native` | 运行原生代码，按其原本方式从原生随机流取数 |
| `none` | 不取随机数 |

### 条目标注

**层级**：`v1`（版本 1 的冻结候选，须通过 [RFC 流程](0001-wasm-extension-modules.zh-CN.md#流程)第 5 步）、`preview`（预览中提供，不受兼容性保证）、`later`（不在版本 1）。不在版本 1 的表项由 `has_kind` 报告为 0，使用时返回 `Unsupported`。表中名字是 API 名，编号才是标识。

### 固定布局

小端序，按字段自然对齐；注释中的数字是字节偏移。

**事件记录**，在 `on_events` 的批次中原地读取：

```rust
struct EventRecord {      // 112 字节
    key: i32,             // 0   模块动作或决定处理函数的分派键
    kind: i32,            // 4   事件种类（第 4 节）
    trigger: i64,         // 8   触发的触发器；总会设置
    holder: i64,          // 16  标签持有者：标签持有该触发器的对象、被绑定的对象或 CellTag 的格子引用；否则为 0
    subject: i64,         // 24  触发器所附着的对象；可为 0，或读出为不存在
    other: i64,           // 32  攻击者、目标、载具、生产者……；可为 0
    house: i64,           // 40  涉及的所属方（事件 1、24、25、26、59 为保留的所属方）；表中未列出时为这次触发的触发器所属方
    projectile: i64,      // 48  可为 0
    modifier: i64,        // 56  可为 0
    cell: i64,            // 64  表中的“cell”：格子引用；没有时为 0
    value: i64,           // 72  伤害、武器索引、层数、帧……
    value2: i64,          // 80  表中列出时的第二个整数
    id: i32,              // 88  表中列出时的类型 ID：武器、弹头、超级武器类型；否则为 -1
    frame: i32,           // 92  事件发生的帧
    x: i32,               // 96  表中列出时：事件发生的坐标（lepton）
    y: i32,               // 100
    z: i32,               // 104
    reserved: i32,        // 108 0
}
```

每条都填 `key`、`kind`、`trigger`、`holder`、`frame`；第 4 节未列出的引用字段为 0（`house` 除外，见上），整型载荷字段为 −1；`reserved` 始终为 0。

**决定记录**，在 `on_decide` 中原地读写（[RFC 6f](0001-wasm-extension-modules.zh-CN.md#6f-可决定事件)）：

```rust
struct DecideRecord {     // 152 字节
    event: EventRecord,   // 0    同上
    native: i64,          // 112  引擎自己的结果
    previous: i64,        // 120  上一环的结果；第一环为原生值
    result: i64,          // 128  由处理函数写入
    handled: i32,         // 136  处理函数设为 1；为 0 时保留上一环的结果
    area: i32,            // 140  模块内存中的结果区；该种类没有时为 0
    area_cap: i32,        // 144  结果区的可用字节数
    previous_len: i32,    // 148  上一环结果的字节数，已复制进结果区
}
```

**筛选子句**，用于事件定义和查询（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）：

```rust
struct Clause {           // 16 字节
    subject: u8,          // 0  0 Subject，1 Other，2 House（触发器的所属方），3 Var；查询中 Subject 是候选，Other、House 为非法参数
    op: u8,               // 1  Op（8.3）
    scope: u8,            // 2  仅 Var：VarTarget（8.3）；查询中只允许 0 Global 和 1 Subject
    reserved: u8,         // 3  0
    prop: i32,            // 4  属性 ID；Var 时为变量 ID
    value: i64,           // 8  比较的常量
}
```

**查询**，`find` 的输入：

```rust
struct Query {            // 64 字节，后接 clause_count 条 Clause
    scope: i32,           // 0   搜索范围（8.2）
    area: i32,            // 4   0 整张地图，1 格子加半径，2 矩形
    cell: i64,            // 8   area 1：中心；area 2：左上角（格子引用）
    radius: i32,          // 16  area 1：lepton；area 2：宽（格）
    height: i32,          // 20  area 2：高（格）
    type: i64,            // 24  类型引用；Types 范围须为种类通配引用或具体类型（8.2）；其他范围 0 表示不限类型
    owner: i64,           // 32  所属方引用；0 为任意
    viewpoint: i64,       // 40  视点属性用的所属方（第 7 节）；0 为没有
    after: i64,           // 48  分页：上一页返回的 next；0 为首页
    clause_count: i32,    // 56  至多 16
    reserved: i32,        // 60  0
}
```

**触发器定义**，`trigger_create` 的输入：

```rust
struct TriggerDef {       // 32 字节，后接 event_count 个 EventDef（各自后接其子句），再接 action_count 个 ActionDef
    house: i64,           // 0   0：每次触发时取主体当时的所属方；没有主体时为地图的中立所属方（Neutral）
    repeat: i32,          // 8   TagRepeat（8.3）
    difficulty: i32,      // 12  位掩码：位 0 Easy，位 1 Normal，位 2 Hard；0b111 为全部，0 为非法参数
    flags: i32,           // 16  第 0 位创建时禁用；第 1 位 allowWin（原版动作 15）；第 2 位 transferOnCapture（TriggerType 第 8 项）
    decide_key: i32,      // 20  决定处理函数的分派键；-1 为没有；不为 -1 时 event_count 必须为 1 且该事件可决定，否则为非法参数
    event_count: i32,     // 24  一个或多个，全部成立（AND），同原版
    action_count: i32,    // 28  零个或多个；动作和决定处理函数至少有一个
}
struct EventDef {         // 32 字节，后接 clause_count 条 Clause
    kind: i32,            // 0   事件种类
    op: i32,              // 4   轮询型事件：比较轮询值与 value 的 Op
    interval: i32,        // 8   轮询型事件：两次检查之间的帧数
    id: i32,              // 12  属性、类型、变量或 TeamType ID 参数；-1 为没有
    value: i64,           // 16  数值参数（所属方下标、路径点、秒数、阈值……）
    clause_count: i32,    // 24  至多 16
    reserved: i32,        // 28  0
}
struct ActionDef {        // 8 字节，后接 ActionParams
    kind: i32,            // 0   动作种类（第 5 节）
    target: i32,          // 4   ActionTarget（5.1）
}
```

**动作参数**，用于 `action` 和触发器中的动作：

```rust
struct ActionParams {     // 72 字节
    id: i32,              // 0   类型 ID（武器、弹头、超级武器、动画、单位类型、声音、TeamType……）；
                          //     SetProperty、Modify 为属性 ID；SetVar 为变量 ID；Module 为分派键
    arg: i32,             // 4   表中列出的小枚举：Blocked、ModifierOp、Mission、Outcome……
    arg2: i32,            // 8   表中列出的修饰器组
    reserved: i32,        // 12  0
    value: i64,           // 16  数量、伤害、分数、武器槽位、等级、弹药、要写入的值……
    value2: i64,          // 24  第二个值：帧数……
    cell: i64,            // 32  格子或路径点引用；0 为没有
    house: i64,           // 40  House 目标、SetOwner 的新所属方、所创建对象的所属方，以及原版动作隐含的所属方；
                          //     0：触发器中为触发器的所属方；直接调用中为非法参数
    other: i64,           // 48  第二个对象：来源、乘客、小队、触发器、标签；触发器中取自事件（5.1）
    text: i32,            // 56  字符串参数（CSF 条目名、音乐、EVA 名称、国家、日志文字）；trigger_create 会复制
    text_len: i32,        // 60
    extra: i32,           // 64  表中列出时的变长数据（仅模块代码）
    extra_len: i32,       // 68
}
```

`value`、`value2` 没有缺失值：不用它们的种类忽略它们，0 不是哨兵值。

**客户端事件记录**，在 `client_on_events` 的批次中原地读取：

```rust
struct ClientEventRecord { // 32 字节
    kind: i32,             // 0   客户端事件种类（4.6）
    source: i32,           // 4   Notify：发送模块的来源，0 mod，1 地图；否则为 0
    ref: i64,              // 8   Click、Hover、Dismiss：节点；MapClick：格子引用；否则为 0
    id: i32,               // 16  Hotkey：热键 ID；Notify：通知 ID；否则为 -1
    value: i32,            // 20  Hover：1 进入，0 离开；Click：List 的行号或 CameoGrid 的条目下标，其他为 −1；
                           //     MapClick：0 左键，1 右键
    data: i32,             // 24  Notify：数据，位于投递区
    data_len: i32,         // 28
}
```

**其他布局：**

```rust
struct PropValue {        // 16 字节：client_node_create 的初始属性
    prop: i32,            // 0  属性 ID
    reserved: i32,        // 4  0
    value: i64,           // 8
}
struct FindResult {       // 16 字节：find 和 client_find 写入
    count: i32,           // 0  写入 out 的条目数
    reserved: i32,        // 4  0
    next: i64,            // 8  续页位置，末页为 0
}
struct Order {            // 24 字节，后接 count 个单位句柄（i64）：order 和 client_send_order 的输入
    kind: i32,            // 0   Command（8.3）
    count: i32,           // 4   后面的单位数
    target: i64,          // 8   对象或格子引用；0 为没有
    value: i64,           // 16  指令表（8.3）中列出时为类型引用；否则为 0
}
struct ProjectileSpec {   // 48 字节：CreateProjectile 的变长数据，也是 Fire 射击方案的一条
    type: i32,            // 0   抛射体类型 ID
    weapon: i32,          // 4   提供伤害和弹头的武器 ID；-1 为没有
    target: i64,          // 8   对象或格子引用；0：Fire 方案中为这次射击的目标，
                          //     CreateProjectile 中为起点对象当前的 Target（没有时为 Refused）
    data: [u8; 16],       // 16  初始模块数据（Data0 为第 0..7 字节，Data1 为第 8..15 字节），存档并计入哈希
    flags: i32,           // 32  位 0：origin 为绝对坐标；为 0 时 origin 是相对起点的偏移
    origin: [i32; 3],     // 36  x、y、z（lepton）；起点在 Fire 方案中为原生的射出位置，
                          //     在 CreateProjectile 中为目标对象的中心或目标格子的中心
}
struct PathSpec {         // 32 字节，后接 point_count 个点：Launch 的结果区
    kind: i32,            // 0   PathKind（8.3）
    point_count: i32,     // 4   后面的点数
    speed: i64,           // 8   速度：定点数（16 位小数），lepton 每帧
    param: [i64; 2],      // 16  种类参数，如螺旋的半径和周期、抛物线的顶点高度
}                         //     每个点为 x、y、z 三个 i64（lepton），共 24 字节
struct ProjectileStep {   // 56 字节：ProjectileFrame 的结果区
    flags: i32,           // 0   位 0 引爆，位 1 设速度，位 2 设位置，位 3 设模块数据
    reserved: i32,        // 4   0
    velocity: [i32; 3],   // 8   新速度 x、y、z：定点数（16 位小数），lepton 每帧
    reserved2: i32,       // 20  0
    position: [i32; 3],   // 24  新位置 x、y、z（lepton）
    reserved3: i32,       // 36  0
    data: [u8; 16],       // 40  新的模块数据（Data0 为第 0..7 字节，Data1 为第 8..15 字节）
}
```

`get_many` 和 `client_get_many` 的 `out` 是按行排列的 `n × m` 个 `i64`：第 `i` 个引用的第 `j` 个属性在 `out[i * m + j]`；空引用或已消失的引用整行写 0，状态为 `Gone`。各项沿用单项读取的空值和状态；写入的槽数（含空值）写入 `count_out`，返回行优先顺序的首个非 `Ok` 状态，全部成功时为 `Ok`。有效空批次返回 `(0, Ok)`。

### 自定义段

SDK 构建时写入；小端序，紧密排列（不对齐）；每段以格式版本字节开头；名字为 `u16` 字节长度加 ASCII 字节，按 ASCII 规则统一大小写（[RFC 12](0001-wasm-extension-modules.zh-CN.md#12-版本与-abi)）。格式版本不认识、内容截断、末尾有多余字节、名字不是 ASCII 或在段内重复时，加载拒绝该模块。

```rust
// vera.actions
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len], key: i32 }; count]

// vera.notify
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len] }; count]

// vera.vars（仅模拟模块）
format: u8,               // 1
count: u32,
[{ name_len: u16, name: [u8; name_len], scope: u8, visibility: u8, default: i64 }; count]   // Scope、Visibility（8.3）

// vera.delivery
format: u8,               // 1
offset: u32,              // 投递区在模块内存中的偏移
bytes: u32,               // 投递区的字节数
```

`vera.actions` 供 INI 和 `lookup(ModuleAction, name)` 分派；加载时检查 `vera.notify` 的名字哈希冲突。`vera.vars.scope` 为 0 全局、1 所属方、2 对象，可见度见 8.3。`vera.delivery` 供事件、决定及结果区、消息和通知共用；单条消息不得超过投递区容量。

### INI 键

规则见 [RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)，取值见 [RFC 15.4](0001-wasm-extension-modules.zh-CN.md#154-预览期取值)。

| 文件和节 | 键 | 值 | 默认 |
| --- | --- | --- | --- |
| 地图 `[Basic]` | `VeraMinVersion` | 最低游戏版本；模块地图须填写 | 无默认值 |
| 地图 `[VeraWasm]` | `Api` | `主.次`，同时说明 `vera:sim` 和 `vera:client` | 有模块时必需 |
| 地图 `[VeraWasm]` | `RulesSize`、`UISize` | 对应模块解压后的字节数 | 没有该模块时不写 |
| 地图 `[VeraWasm]` | `StorageId` | ASCII 名字 | 地图的文件名 |
| 地图 `[VeraWasmRulesPack]`、`[VeraWasmUIPack]` | `1`、`2`…… | 标准 base64，每个值 70 个字符（读取最多接受 127 字节）；键为从 1 连续编号、无前导零的十进制数；按编号而不是按文件中的顺序拼接，重复、缺号、非数字键或超长的值拒绝开局；解码后按 `IsoMapPack5` 的分块 LZO 格式解压 | — |
| mod `rulesmd.ini` `[VeraWasm]` | `Api` | `主.次`，与该 mod 模块的导入版本一致 | 有模块时必需 |
| mod `rulesmd.ini` `[VeraWasm]` | `Budget`、`MapBudget` | mod 模块、地图模块每帧的额度 | 引擎默认值；引擎设上限 |
| mod `rulesmd.ini` `[VeraWasm]` | `MapWasm` | `yes`、`no` | `yes` |
| mod `rulesmd.ini` `[VeraWasm]` | `MapUI` | `overlay`、`full`、`no` | `overlay` |
| mod `rulesmd.ini` 的 TechnoType、Weapon、Warhead、Projectile 节 | `Vera.Tags` | 逗号分隔的标签节名 | 没有 |
| mod `rulesmd.ini` `[VeraVars]` | 变量名 | `Scope,Visibility,Default`（8.3 中的名字） | 没有 |
| 地图 `[VeraUI]`、`[VeraUI.<部件名>]` | `Parts`；部件键 | 同 `veraui.ini` 的新增部件（7.10） | 没有 |
| 地图 `[Actions]` | 动作种类 100036 | `100036,4,名字,0,0,0,0,A`；名字可带 `mod.`、`map.` | — |
| 标签节 | `Trigger` | 逗号分隔的触发器节名 | — |
| 触发器节 | `Event`、`Event<n>` | `名字` 或 `名字(op,interval,id,value)`（`EventDef` 字段，可省略末尾的）；多个事件全部成立 | 至少一个 |
| 触发器节 | `Where<n>`、`Event<k>.Where<n>` | 筛选子句（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）；`Where<n>` 属于第一个事件 | 没有 |
| 触发器节 | `Action`、`Action<n>` | `Wasm:名字` 为模块动作；其他写作 `名字(目标,参数……)`，目标为 `ActionTarget` 名，参数按第 5 节“参数”列的顺序 | 没有 |
| 触发器节 | `Decide` | `Wasm:名字`，决定处理函数 | 没有 |
| 触发器节 | `House` | 国家名 | 0（主体当时的所属方；无主体时为 Neutral） |
| 触发器节 | `Difficulty` | `Easy`、`Normal`、`Hard` 的逗号列表 | 全部 |
| 触发器节 | `Repeat` | `TagRepeat` 名：`OneTimeOr`、`OneTimeAnd`、`Repeating` | `OneTimeOr` |
| `veraui.ini` | `[Layouts]`、布局节的 `Version`、`<布局>.<部件>` 节 | 见 [界面布局](../interface-layout.md)；部件键见 7.10 | — |

## 1. 模拟 ABI

接口 `vera:sim@1`。导入的模块名为 `vera:sim@<主>.<次>`（模块需要的最低版本；一个模块的所有导入相同，并与 `Api=` 一致），导入名和导出名就是下表中的函数名。签名为 core Wasm 类型，`ptr`、`len` 为 `i32`，费用见接口定义（[RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)）。

### 1.1 导入

“可用于”：`init` 为 `vera_init`；`rules` 为 `on_rules`；`decide` 为 `on_decide`（调用其他函数会停用模块）；`all` 为其他调用；`init` 和 `rules` 中不可用的函数返回 `Unsupported`。

| # | 函数 | 签名 | 语义 | 可用于 |
| --- | --- | --- | --- | --- |
| 1 | `frame` | `() -> i32` | 当前帧号；不会失败，直接返回值，没有状态 | all、decide |
| 2 | `random` | `(lo: i32, hi: i32, out: ptr) -> i32` | 写入 `[lo, hi]` 的 `i32` 整数，取自模块随机流；在 `on_decide` 中取自按当前这条请求派生的子流（键见 [RFC 4](0001-wasm-extension-modules.zh-CN.md#4-确定性)），不改变主随机流；`lo > hi` 为非法参数 | all、decide |
| 3 | `log` | `(ptr, len)` | 写一行游戏内日志 | init、rules、all、decide |
| 4 | `has_kind` | `(table: i32, id: i32) -> i32` | 表 `table`（`Table`）中的 `id` 可用为 1，未知或本局关闭为 0 | init、rules、all、decide |
| 5 | `lookup` | `(kind: i32, ptr, len, out: ptr) -> i32` | 名字解析为 `i32` ID（8.1）写入 `out`；没有时为 −1 和 `NotFound` | init、rules、all、decide |
| 6 | `name_of` | `(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> i32` | 把 ID 的名字写进 `out`，长度（`i32`）写进 `len_out` | init、rules、all、decide |
| 7 | `get` | `(ref: i64, prop: i32, out: ptr) -> i32` | 写入属性的有效值（`i64`，含修饰器） | all、decide、rules（仅类型和全局规则引用） |
| 8 | `get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> i32` | `n` 个引用的 `m` 个属性，写入的值数（`i32`）写进 `count_out` | all、decide、rules（仅类型和全局规则引用） |
| 9 | `set` | `(ref: i64, prop: i32, value: i64) -> i32` | 经 owner 写一个属性；返回状态 | all、rules（仅类型和全局规则引用） |
| 10 | `modify` | `(ref: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32, out: ptr) -> i32` | 加修饰器（`ModifierOp`），写入其句柄（`i64`）；`frames` 为负表示无限，`group` 为 0 表示不分组 | all |
| 11 | `unmodify` | `(modifier: i64) -> i32` | 移除修饰器；已到期为 `Gone` | all |
| 12 | `rules_get` | `(sec: ptr, sec_len, key: ptr, key_len, out: ptr, cap: i32, len_out: ptr) -> i32` | 键的原始字符串，长度（`i32`）写进 `len_out`；不存在为 −1 和 `NotFound` | all、decide、rules |
| 13 | `rules_set` | `(sec: ptr, sec_len, key: ptr, key_len, val: ptr, val_len) -> i32` | 在 `on_rules` 中写一个键（AI 文档除外，[RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)）；版本 1 中对局中总是 `Refused`（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)） | all、rules |
| 14 | `var_get` | `(scope: i64, var: i32, out: ptr) -> i32` | 读变量（`i64`）；`scope` 0 为全局，否则为所属方或对象引用 | all、decide |
| 15 | `var_set` | `(scope: i64, var: i32, value: i64) -> i32` | 写变量，重置等待它的触发器的计时器 | all |
| 16 | `find` | `(query: ptr, len, out: ptr, cap: i32, res: ptr) -> i32` | 搜索（`Query`），引用写入 `out`，`FindResult`（count、next）写入 `res`，返回状态，见“字符串与缓冲区” | all、decide |
| 17 | `trigger_create` | `(def: ptr, len, out: ptr) -> i32` | 创建触发器（`TriggerDef`），写入句柄（`i64`）；路径点不存在为 0 和 `NotFound`；使用未启用的 Ares 事件（4.2）时为 (0, Unsupported) | all |
| 18 | `trigger_remove` | `(t: i64) -> i32` | 删除触发器及其所有实例（延迟） | all |
| 19 | `trigger_bind` | `(t: i64, ref: i64) -> i32` | 绑定到对象、类型、武器、弹头或抛射体类型，或格子（仅限 4.1 注明“绑定格子”的事件） | all |
| 20 | `trigger_unbind` | `(t: i64, ref: i64) -> i32` | 解除一个绑定 | all |
| 21 | `action` | `(kind: i32, target: i64, params: ptr, len, out: ptr) -> i32` | 执行动作（第 5 节，`ActionParams`）；写入创建的对象（`i64`），否则为 0 | all |
| 22 | `send_notify` | `(to: i64, id: i32, ptr, len) -> i32` | 通知所属方 `to` 的玩家；`to` 为 0 时发给观战者；超出大小或每帧条数为 `Refused` | all |
| 23 | `order` | `(house: i64, ptr, len) -> i32` | 为电脑所属方 `house` 排入一条标准玩家指令（`Order`），下一帧执行，像该所属方的玩家输入一样校验（[RFC 6g](0001-wasm-extension-modules.zh-CN.md#6g-所属方指令)） | all |

- `on_rules` 中对类型引用和全局规则引用的 `set` 等于以值的 INI 文本调用 `rules_set`；`get` 读到本层之前处理过的值。触发器、动作、变量、修饰器、`find` 和其他引用的读写返回 `Unsupported`。
- 写入函数：`set`、`modify`、`unmodify`、`rules_set`、`var_set`、`trigger_create`、`trigger_remove`、`trigger_bind`、`trigger_unbind`、`action`、`send_notify`、`order`。

### 1.2 导出

函数按需导出；时机见 [RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)。定义内存的模块必须把它导出为 `memory`（[RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)）。

| 导出 | 签名 | 时机 | 层级 |
| --- | --- | --- | --- |
| `vera_init` | `()` | SDK 从顶层代码生成（无 `start` 段）；第一次其他调用之前一次 | v1 |
| `on_rules` | `()` | 在模块所处的规则层位置一次 | v1 |
| `on_scenario_start` | `()` | 场景开始时一次 | v1 |
| `on_frame` | `()` | 每帧，模块阶段最后；SDK 运行 `onFrame(new Every(N), fn)` 注册的函数 | v1 |
| `on_events` | `(ptr, count: i32)` | 每帧在模块阶段、`on_message` 之后，按顺序分一次或几次调用投递排队的记录；`count` 条 `EventRecord` | v1 |
| `on_decide` | `(ptr, count: i32)` | 引擎在决策点提交结果时同步调用；`count` 条 `DecideRecord`，版本 1 中总是 1 | v1 |
| `on_message` | `(sender: i64, ptr, len)` | 同一来源的客户端消息到达时，每条一次；`sender` 为发送者的所属方；`ptr` 位于投递区 | v1（阶段 2） |

没有 `on_load`；抛射体弹道用可决定事件 `Launch`、`ProjectileFrame`（4.5），不增加导出。

## 2. 客户端 ABI

接口 `vera:client@1`，导入的模块名为 `vera:client@<主>.<次>`，规则同第 1 节；函数都以 `client_` 开头，可用浮点；导入限制和额度见 [RFC 1](0001-wasm-extension-modules.zh-CN.md#1-两个世界)、[RFC 3](0001-wasm-extension-modules.zh-CN.md#3-加载与校验)。

### 2.1 导入

读取只给出本玩家界面能显示的内容（第 7 节“客户端”列）。

| # | 函数 | 签名 | 语义 |
| --- | --- | --- | --- |
| 1 | `client_frame` | `() -> i32` | 最近算完的模拟帧号；不会失败，直接返回值，没有状态 |
| 2 | `client_random` | `(lo: i32, hi: i32, out: ptr) -> i32` | 写入本机随机 `i32`，只用于显示 |
| 3 | `client_log` | `(ptr, len)` | 写游戏内控制台 |
| 4 | `client_has_kind` | `(table: i32, id: i32) -> i32` | 同 `has_kind` |
| 5 | `client_lookup` | `(kind: i32, ptr, len, out: ptr) -> i32` | 同 `lookup`；变量名和通知名查本局模拟模块声明过的 |
| 6 | `client_name_of` | `(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> i32` | 同 `name_of` |
| 7 | `client_get` | `(ref: i64, prop: i32, out: ptr) -> i32` | 读属性（`i64`）；看不到的对象为 `Gone`，不可见的属性为空值和 `Refused` |
| 8 | `client_get_many` | `(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> i32` | 布局和批次状态同 `get_many`；逐项沿用 `client_get` 的可见度检查 |
| 9 | `client_find` | `(query: ptr, len, out: ptr, cap: i32, res: ptr) -> i32` | 同 `find`，只返回本玩家能看到的 |
| 10 | `client_var_get` | `(scope: i64, var: i32, out: ptr) -> i32` | 读变量（`i64`），受可见度限制（[RFC 6e](0001-wasm-extension-modules.zh-CN.md#6e-变量)） |
| 11 | `client_set` | `(ref: i64, prop: i32, value: i64) -> i32` | 写客户端拥有的引用的属性：节点、镜头、光标、本地设置；无权写的节点为 `Refused`（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） |
| 12 | `client_set_bytes` | `(ref: i64, prop: i32, ptr, len) -> i32` | 写字符串或字节属性（`Label`、`Art`、`Items`、`ItemProgress`……）；无权写的节点为 `Refused` |
| 13 | `client_node_create` | `(kind: i32, parent: i64, props: ptr, len, out: ptr) -> i32` | 新建节点（`NodeKind`）并写入其句柄（`i64`），初始属性为 `PropValue` 数组；`parent` 0 为顶层 |
| 14 | `client_node_remove` | `(node: i64) -> i32` | 删除节点及子节点；无权删除的节点为 `Refused` |
| 15 | `client_node_find` | `(ptr, len, out: ptr) -> i32` | 按 `Name` 找节点并写入其句柄（`i64`）（7.10：INI 节点为当前布局中的部件名）；先找模块自己来源的，同组同名时为先创建的；没有为 (0, NotFound) |
| 16 | `client_action` | `(kind: i32, target: i64, params: ptr, len) -> i32` | 执行本地动作（5.4，`ActionParams`） |
| 17 | `client_send_order` | `(ptr, len) -> i32` | 发出标准玩家指令（`Order`），像玩家输入一样校验 |
| 18 | `client_send_message` | `(ptr, len) -> i32` | 消息进入命令流，到达同一来源模拟模块的 `on_message`；观战者为 `Refused`；限大小和频率（[RFC 11](0001-wasm-extension-modules.zh-CN.md#11-联机)） |
| 19 | `client_storage_read` | `(key: ptr, key_len, out: ptr, cap: i32, len_out: ptr) -> i32` | 读模块来源的存储，长度（`i32`）写进 `len_out`；没有为 −1 和 `NotFound`（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） |
| 20 | `client_storage_write` | `(key: ptr, key_len, data: ptr, data_len) -> i32` | 写存储；`data_len` 为 0 时删除该键；键 1..64 字节 ASCII，值 ≤ 16 KiB，每个来源 ≤ 256 个键、键和值合计 ≤ 256 KiB；超出为 `Refused`，存储不变 |

`client_init` 中只可用 `client_lookup`、`client_name_of`、`client_has_kind`、`client_log`。

### 2.2 导出

全部为 v1（阶段 2）。

| 导出 | 签名 | 时机 |
| --- | --- | --- |
| `client_init` | `()` | SDK 从顶层代码生成；第一次其他调用之前一次 |
| `client_on_start` | `()` | 对局界面建好、INI 布局读完后一次；重新加入的玩家在追赶完成后一次 |
| `client_on_render` | `(dt: f32, alpha: f32)` | 每个渲染帧一次；`dt`、`alpha` 只用于显示 |
| `client_on_events` | `(ptr, count: i32)` | 每个渲染帧一次，在 `client_on_render` 之前；`ClientEventRecord`（4.6） |

## 3. SDK 对应

TypeScript 映射如下；Rust 使用 newtype 和蛇形命名（`where_prop`）。名称暂定。

| 用途 | TypeScript | 宿主调用 |
| --- | --- | --- |
| 读属性 | `Techno.health(t)`、`House.money(h)`、`Cell.landType(c)` | `get` |
| 写属性 | `Techno.setOwner(t, h)`、`Techno.setMission(t, Mission.Hunt)`、`Trigger.setEnabled(t, false)` | `set` |
| 批量读 | `Techno.read(list, [Prop.Health, Prop.Owner])` | `get_many` |
| 字符串属性 | `House.name(h)` 返回 `string` | `get` 加 `name_of` |
| 修饰器 | `Modifier.multiply(t, Prop.SpeedMultiplier, fixed(0.5), 150)`；加值用 `Modifier.add`，覆盖用 `Modifier.override`，提前移除用 `Modifier.remove(m)` | `modify`、`unmodify` |
| 动作 | 每个动作种类一个函数：`Action.nukeStrike(cell, house)`、`Action.launchSuperWeapon(house, type, cell)`、`Action.createObject(cell, type, house)`、`Action.addMoney(house, amount)` | `action` |
| 所属方指令 | `House.order(h, Order.queueProduction(Rules.technoType("HTNK")))`、`House.order(h, Order.attackMove(units, cell))`；`Client.order` 使用同样的构造器 | `order` |
| 触发器中的动作 | `ActionDef.damage(Target.Other, warhead, 50)`、`ActionDef.setVar(Target.Subject, v, 1)`：只构造 `ActionDef` | — |
| 触发器 | `Trigger.on(event).bind(ref).immediate(def).enqueue(fn).done()`、`.decideSync(fn)` | `.decideSync`、`.done` 调用 `trigger_create`，再对每个 `.bind` 调用 `trigger_bind` |
| 属性轮询 | `Trigger.onProperty(prop, op, value, interval)` | 构造一个 `PropertyPoll` 的 `EventDef`（4.4） |
| 绑定 | `Trigger.bind(t, ref)`、`Trigger.unbind(t, ref)`、`Trigger.remove(t)`、`Trigger.bindAllTypes(t, NameKind.TechnoType)` | `trigger_bind`、`trigger_unbind`、`trigger_remove`；`bindAllTypes` 先 `find`（范围 `Types`，种类通配引用），再对每个类型调用一次 `trigger_bind` |
| 变量 | 声明生成 `Wave.get()`、`Panicked.get(unit)`、`ReinforcementsBought.set(house, 1)`；原版变量为 `Var.local(n).get()`、`Var.global(n).set(value)` | 初始化时缓存 `lookup` 的结果（原版用固定 ID）；访问时一次 `var_get` 或 `var_set`，客户端只有 `client_var_get` |
| 名字 | `Rules.technoType("E1")`、`Rules.weapon("RedEye2")`、`Rules.superWeaponType("NukeSpecial")` 返回类型引用（含按国家名查类型的 `Rules.country(name)`） | `lookup` 成功时编码类型引用；未找到时 SDK 返回 `NO_HANDLE`，保留 `NotFound` 状态 |
| 规则 | `Rules.get(section, key)`、`Rules.set(section, key, value)` | `rules_get`、`rules_set` |
| 搜索 | `.first()` 或 `.page(buffer, after)`；`.country(americans)` 按国家筛选 | 每次 `.page` 一次 `find`；first 使用容量 1，沿 `next` 继续，直到找到或 `next=0`，没有匹配时返回 0。country 接受已解析的国家类型引用，生成 Country 相等子句 |
| 其他 | `Cell.at(x, y)`、`Waypoint.cell(n)`（只编码引用）；`Notify.id(name)`、`Notify.send(to, id, data)` | —；`lookup`、`send_notify` |
| 注册 | 顶层的 `onScenarioStart(fn)`、`onFrame(new Every(N), fn)`、`onMessage(fn)`、`namedAction(name, fn)`；客户端的 `onStart(fn)`、`onRender(fn)`、`onClientEvent(ClientEvent.Click, fn)` | SDK 生成的导出 |
| 客户端 | `Ui.find(name)`、`Ui.create(NodeKind.Text, parent)`、`Node.setLabel(n, text)`、`Node.setVisible(n, v)`、`Client.viewer()`、`Client.sendMessage(data)`、`Client.order(...)`、`Storage.read(key)`、`Node.setItems(n, items)` | `client_*`，各一次；`Storage.read` 的值大于缓冲区时调用两次；`Node.setItems` 用 `\n` 连接条目后调用 `client_set_bytes` |

- 事件视图仅在本次回调内有效。具名字段原地读取下表所示的 ABI 字段，沿用其单位与空值；公共字段沿用原名。决定视图读取内嵌事件，`setResult(v)` 写入 `result` 并置 `handled=1`。
- `Target`、`VarTarget`、`NameKind` 分别见 5.1、8.3、8.1。`.once()` 设为一次性，`.house(h)` 指定所属方。`.where`、`.whereOther`、`.whereHouse` 分别检查主体、另一方和所属方的属性。
- `.immediate(def)` 和 `.enqueue(fn)` 可重复。前者在触发点执行，后者在模块阶段投递。`.done()` 完成构建；可决定事件用 `.decideSync(fn)` 指定同步决定并完成构建。
- `namedAction(name, fn)` 的名字必须是字符串字面量，构建时写入 `vera.actions`。

| SDK 视图 | 具名字段 → 记录字段 |
| --- | --- |
| `TakeDamageEvent` | `victim → subject`，`attacker → other`，`damage → value`，`warhead → id` |
| `FireEvent` | `shooter → subject`，`target → other`，`targetCell → cell`，`weaponSlot → value`，`weapon → id` |
| `TargetChangedEvent` | `newTarget → other` |
| `ClickEvent`（客户端） | `node → ref`，`itemIndex → value` |

## 4. 事件种类

规则见 [RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)、[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)、[RFC 6f](0001-wasm-extension-modules.zh-CN.md#6f-可决定事件)、[RFC 7](0001-wasm-extension-modules.zh-CN.md#7-运行时的触发器和标签)。

图例：**字段**为填入的 `EventRecord` 字段；**绑定** `对象` 为只对绑定了触发器的对象（对象绑定、类型绑定或标签）引发，`类型` 为只对绑定了触发器的类型引发，`格子` 为只对绑定了触发器的格子（CellTag 或格子绑定）引发，`全局` 为无需绑定；**结果槽**为 `result` 的含义（`native` 同义）；结果区的字节数按事件种类固定（[RFC 15.4](0001-wasm-extension-modules.zh-CN.md#154-预览期取值)）。House 标签事件每 8 帧轮询，Logic 标签事件每帧轮询。

### 4.1 原版事件 0..61

类型 `R` 投递，`H` House 标签轮询，`L` Logic 标签轮询；字段 `S` subject，`O` other，`Hs` house；“持久”表示写入持久状态。H、L 均为全局绑定；R 默认绑定对象，例外见表。除 0、2、3、24、31、33、51、53、54、59 为 `later` 外都是 `v1`，不可决定。

| # | API 名称 | 参数 | 类型 | 字段 | 绑定例外及备注 |
| --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | 不公开；永不成立 |
| 1 | `EnteredBy` | `value` 所属方下标（−1 任意） | R | S、O、保留的 Hs | 持久 |
| 2 | `SpiedBy` | 忽略 | R | S、O |  |
| 3 | `ThievedBy` | `value` 所属方下标 | R | S、Hs |  |
| 4 | `DiscoveredByPlayer` | 忽略 | R | S、O |  |
| 5 | `HouseDiscovered` | `value` 所属方 | H | Hs |  |
| 6 | `AttackedByAnyHouse` | 忽略 | R | S、O |  |
| 7 | `DestroyedByAnyHouse` | 忽略 | R | S、O | 需要击杀来源 |
| 8 | `AnyEvent` | — | H | S |  |
| 9 | `AllUnitsDestroyed` | `value` 所属方 | H | Hs |  |
| 10 | `AllBuildingsDestroyed` | `value` 所属方 | H | Hs |  |
| 11 | `AllDestroyed` | `value` 所属方 | H | Hs |  |
| 12 | `CreditsExceed` | `value` 金钱 | H | Hs |  |
| 13 | `ElapsedTime` | `value` 秒（×15 帧） | L | — |  |
| 14 | `MissionTimerExpired` | — | L | — |  |
| 15 | `BuildingsDestroyedCount` | `value` 数量 | H | Hs |  |
| 16 | `UnitsDestroyedCount` | `value` 数量 | H | Hs |  |
| 17 | `NoFactoriesLeft` | — | H | Hs |  |
| 18 | `CiviliansEvacuated` | — | H | Hs |  |
| 19 | `BuildBuildingType` | `value` BuildingType 下标 | H | Hs | 持久；也接受 `id` |
| 20 | `BuildUnitType` | `value` VehicleType 下标 | H | Hs | 同 19 |
| 21 | `BuildInfantryType` | `value` InfantryType 下标 | H | Hs | 同 19 |
| 22 | `BuildAircraftType` | `value` AircraftType 下标 | H | Hs | 同 19 |
| 23 | `TeamLeavesMap` | `id` TeamType | R | — | 绑定全局 |
| 24 | `ZoneEntryBy` | `value` 所属方（−1 任意） | R | S、O、保留的 Hs |  |
| 25 | `CrossesHorizontalLine` | 同 24 | R | O、保留的 Hs | 绑定格子 |
| 26 | `CrossesVerticalLine` | 同 24 | R | O、保留的 Hs | 绑定格子 |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — |  |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — |  |
| 29 | `DestroyedByAnythingNotInfiltrate` | 忽略 | R | S、O |  |
| 30 | `LowPower` | `value` 所属方 | H | Hs |  |
| 31 | `BridgeDestroyed` | 忽略 | R | `cell` | 绑定格子 |
| 32 | `BuildingExists` | `value` BuildingType 下标 | H | Hs | 持久 |
| 33 | `SelectedByPlayer` | 忽略 | R | S | 联机不引发；客户端形式 `SelectionChanged`（4.6） |
| 34 | `ComesNearWaypoint` | `value` 路径点 | R | S | 0x500 lepton 以内 |
| 35 | `EnemyInSpotlight` | 忽略 | R | S |  |
| 36 | `LocalIsSet` | `value` 0..99 | L | — |  |
| 37 | `LocalIsClear` | `value` 0..99 | L | — |  |
| 38 | `FirstDamagedCombat` | 忽略 | R | S、O | 需要伤害来源 |
| 39 | `HalfHealthCombat` | 忽略 | R | S、O |  |
| 40 | `QuarterHealthCombat` | 忽略 | R | S、O |  |
| 41 | `FirstDamagedAny` | 忽略 | R | S、O（可为 0） |  |
| 42 | `HalfHealthAny` | 忽略 | R | S |  |
| 43 | `QuarterHealthAny` | 忽略 | R | S |  |
| 44 | `AttackedByHouse` | `value` 所属方数组下标 | R | S、O | Ares 转换 Player@X |
| 45 | `AmbientLightBelow` | `value` | L | — | 环境光变化时检查 |
| 46 | `AmbientLightAbove` | `value` | L | — | 同 45 |
| 47 | `ElapsedScenarioTime` | `value` 秒 | L | — |  |
| 48 | `DestroyedByAnything` | 忽略 | R | S、O（可为 0） |  |
| 49 | `PickupCrate` | 忽略 | R | S |  |
| 50 | `PickupCrateAny` | 忽略 | L | — | 有箱子被捡起时检查 |
| 51 | `RandomDelay` | `value` 秒 | L | — | 随机数 native |
| 52 | `CreditsBelow` | `value` 金钱 | H | Hs |  |
| 53 | `SpyAsHouse` | `value` 所属方 | R | S、Hs | 持久 |
| 54 | `SpyAsInfantry` | `value` InfantryType 下标 | R | S | 持久 |
| 55 | `NavalUnitsDestroyed` | `value` 所属方 | H | Hs |  |
| 56 | `LandUnitsDestroyed` | `value` 所属方 | H | Hs |  |
| 57 | `BuildingDoesNotExist` | `value` BuildingType 下标 | H | Hs |  |
| 58 | `PowerFull` | `value` 所属方 | H | Hs |  |
| 59 | `EnteredOrOverflownBy` | 同 1 | R | S、O、保留的 Hs |  |
| 60 | `TechTypeExists` | `id` 类型，`value` 数量 | L | — |  |
| 61 | `TechTypeDoesNotExist` | `id` 类型 | L | — |  |

### 4.2 Ares 事件 62..88

仅当 `rules.ares_form()` 时可用，否则 `trigger_create` 返回 0 和 `Unsupported`、`has_kind` 返回 0；62..86 为 `v1`，87、88 为 `later`，不可决定；`R` 填 `subject`、绑定对象，`H`、`L` 为全局。

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

修饰器事件按目标绑定：对象绑定该对象；所属方为全局事件，填 `house`。

在安全点引发；`对象+武器` 也可绑定到武器、弹头或抛射体类型（[RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)）。层级属于投递形式；决定形式只有 `TakeDamage` 为 `v1`，`Fire`、`FatalDamage`、`TargetChanged` 的为 `later`。100037、100038 保留（`OnBehavior`、`OnBehaviorExpire`）。

| # | 事件 | 字段 | 引发时机 | 绑定 | 决定时机 → 结果（— 为不可决定） | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| 100000 | `TakeDamage` | `other` 来源（可为 0），`value` 结算后的正伤害：已经过护甲倍率、免疫、距离衰减、主体护甲的 Verses、上限、建筑最小值和 Ares 处决；投递时已按受击前生命截断，决定时尚未截断；`id` 弹头，`x`、`y`、`z` 主体位置（lepton） | 伤害提交后 | 对象 | 伤害提交前，且只在这个值大于 0 时 → 截断前的最终伤害（钳制到 0..2^31−1）；零伤害和治疗不询问 | v1 |
| 100001 | `HitByWarhead` | `id` 弹头，`value` 最终伤害，`other` 来源，`x`、`y`、`z` 主体位置（lepton） | 伤害被接收后；免疫的命中不算 | 对象 | — | later |
| 100002 | `AttackedBy` | `other` 攻击者，`value` 武器槽位（−1 无），`projectile`（可为 0） | 开火步骤后，在目标上 | 对象 | — | v1 |
| 100003 | `FatalDamage` | `other` 攻击者（可为 0），`value` 伤害，`id` 弹头，`x`、`y`、`z` 主体位置（lepton） | 致命伤害提交后、死亡流程前；死亡流程重新检查 | 对象 | 死亡流程中 → 死亡方式：位 0–31 为死亡武器 ID 加 1（0 无），位 32 产生幸存者，位 33 留下残骸 | v1 |
| 100004 | `Killed` | `other` 击杀者（可为 0），`id` 致死弹头（无为 −1），`x`、`y`、`z` 死亡位置（lepton） | 击杀记录后 | 对象 | — | v1 |
| 100005 | `Fire` | `other` 目标（可为 0），`cell` 目标格子（总会填入），`value` 武器槽位，`id` 武器，`projectile`（可为 0） | 射击完成后 | 对象+武器 | 射出前 → 射击方案条数；方案是结果区中的 `ProjectileSpec` 数组（[RFC 9](0001-wasm-extension-modules.zh-CN.md#9-示例抛射体弹道)） | v1 |
| 100006 | `SpawnFire` | `other` 子机，`value` 武器槽位，`projectile`；目标在 `cell` 或子机的 `Target` 属性 | 子机射击后 | 对象 | — | later |
| 100007 | `HitTarget` | `other` 目标，`value` 伤害，`id` 武器，`projectile` | 伤害提交后，在开火者上 | 对象+武器 | — | later |
| 100008 | `TargetChanged` | `other` 新目标（清除时为 0） | 目标设定或清除后 | 对象 | 在对象的回合里设定目标前；并行的索敌预答不引发 → 选中的目标引用（0 不选）；不能攻击的目标用原生结果；候选用 `find` 读 | later |
| 100009 | `ProjectileIncoming` | `projectile`，`other` 发射者（可为 0） | 抛射体被接纳后 | 对象 | — | later |
| 100010 | `Detonated` | `projectile`（投递时读出为不存在），`cell`，`other` 来源（可为 0），`id` 弹头，`x`、`y`、`z` 引爆点（lepton） | 引爆后 | 对象+武器 | — | later |
| 100011 | `Birth` | `other` 生产者（可为 0），`value` 生产时间（−1 无） | 对象放置后（生产、增援、生成） | 对象 | — | v1 |
| 100012 | `TransportEntered` | 在乘客（`subject`）上；`other` 载具或建筑，`value` 已占空间，`value2` 1 驻军 / 0 载员 | 进入完成后 | 对象 | — | v1 |
| 100013 | `TransportExited` | 同 `TransportEntered` | 离开完成后 | 对象 | — | v1 |
| 100014 | `PassengerAdded` | 在载具或建筑（`subject`）上；`other` 乘客，`value` 已占空间，`value2` 驻军标志 | 同 `TransportEntered` | 对象 | — | v1 |
| 100015 | `PassengerRemoved` | 同 `PassengerAdded` | 同 `TransportExited` | 对象 | — | v1 |
| 100016 | `Commanded` | 在单位（`subject`）上；`value` 指令（`Command`），`other` 目标对象（可为 0），`cell` 目标格子（可为 0），`value2` 指令序号（一次点击下给多个单位的同一条指令共用） | 模拟执行绑定对象的玩家或 AI 指令后 | 对象 | — | later |
| 100017 | `Crushed` | `other` 碾压者，`x`、`y`、`z` 位置（lepton） | 碾压后 | 对象 | — | later |
| 100018 | `HouseTransfer` | `house` 新所属方，`value` 1 为心控 | 转移前；转移重新检查 | 对象 | — | later |
| 100019 | `HouseTransferred` | `house` 原所属方，`value` 1 为心控 | 转移后 | 对象 | — | v1 |
| 100020 | `ModifierStart` | `modifier`，`value` 属性 ID，`other` 来源（可为 0） | 修饰器加入后 | 修饰器目标 | — | v1 |
| 100021 | `ModifierExpire` | `modifier`（投递后读出为不存在），`value` 属性 ID | 修饰器到期或移除后 | 修饰器目标 | — | v1 |
| 100022 | `ModifierStacks` | `modifier`，`value` 主体上同来源、同属性、组 0 的修饰器数 | 组 0 的修饰器加入后，该数目大于 1 时 | 修饰器目标 | — | v1 |
| 100023 | `Timer` | `value` 上次触发的帧 | 每 `interval` 帧 | 全局 | — | v1 |
| 100024 | `SuperWeaponLaunched` | `id` 超级武器类型，`house`，`cell` 目标 | 超级武器发射后 | 全局 | — | v1 |
| 100025 | `ProductionCancelled` | `house`，`id` 类型，`value` 退还的金钱 | 玩家或 AI 取消生产队列中的条目后 | 类型 | — | later |

### 4.4 新的轮询型事件

全部为 `v1`，不可决定。采样间隔、错帧和触发边沿见 [RFC 6a](0001-wasm-extension-modules.zh-CN.md#6a-触发器事件筛选子句和动作)。默认以 `op` 比较读数与阈值 `EventDef.value`；记录中的 `value` 为实际读数。所属方事件填 `house`，为全局事件；对象事件填 `subject`。

| # | 事件 | 读数 |
| --- | --- | --- |
| 100026 | `PropertyPoll` | `EventDef.id` 为属性 ID，`op`、`value`、`interval` 为比较运算、阈值和间隔。对象属性限 `Health`、`HealthPercent`、`Ammo`、`PassengerCount`、`TransportSpaceLeft`、`Storage`、`Mission`、`Moving`、`LandType`，取绑定对象；`Money` 取触发器所属方（全局事件）。其余属性在创建时拒绝。SDK 字段 `reading` 对应 `EventRecord.value`。 |
| 100027 | `HousePowerPoll` | 发电量减耗电量 |
| 100028 | `ModifierCountPoll` | 主体上组 `id` 中、来自本模块来源的修饰器数 |
| 100029 | `DistancePoll` | 到格子的距离（lepton）：`EventDef.value` 为格子或路径点引用，`op` 比较距离与 `id`（lepton） |
| 100030 | `VariablePoll` | 变量 `id`，按声明的作用域读主体、触发器所属方或全局；变量被写入时重置计时器 |

### 4.5 只可决定的事件

只用于带决定处理函数的触发器，没有投递形式。

| # | 事件 | 字段 | 决策点 | 绑定 | 结果槽 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| 100031 | `ProductionAllowed` | `house`，`id` 类型 | 每帧每个所属方和每个绑定类型一次，在固定步骤；侧边栏和 AI 读缓存结果 | 类型 | 1 允许，0 不允许 | preview |
| 100032 | `ProductionQueued` | `house`，`id` 类型，`subject` 0 | 玩家或 AI 把类型加入生产队列时；续造和强制重排不询问；拒绝电脑所属方时引擎让它重选 | 类型 | 1 允许，0 拒绝：拒绝的条目不入队，不花钱 | v1 |
| 100033 | `Defeat` | `house` | 引擎的判负检查即将判定该所属方失败时；`EndFor` 和原版的胜负动作不经过它 | 全局 | 1 判负，0 否决：本次不判负，下次检查再问 | v1 |
| 100034 | `CommandAllowed` | `subject`，`value` 指令（`Command`），`other` 目标对象，`cell` 目标格子 | 模拟执行绑定对象的每条玩家或 AI 指令时；光标预览不引发 | 对象 | 1 允许，0 拒绝 | preview |
| 100035 | `Launch` | `projectile`，`subject` 发射者，`other` 目标，`cell` 目标格子 | 绑定类型的每个抛射体射出时一次 | 对象+武器 | `PathSpec` 的字节数（32 + 24 × 点数）；结果区为一个 `PathSpec` | later |
| 100036 | `ProjectileFrame` | `projectile`，`value` 已飞行帧数 | 每帧引擎推进抛射体时，按引擎的顺序，在目标解析和帧数引信之后、计算候选位置之前，每个本帧要移动的绑定抛射体一次 | 抛射体类型 | 不用；结果区为一个 `ProjectileStep`；`handled` 为 0 时按原生推进 | later |
| 100039 | `DamageIncoming` | `subject`，`other` 来源（可为 0），`id` 弹头，`value` 入射伤害（有符号），`value2` 把 `previous` 按触发器 `EventDef.id` 的护甲（−1 为主体的护甲）经距离衰减、Verses 和上限算出的伤害，符号同 `previous`；`EventDef.value` 位 0 为 1 时，正值先按主体护甲倍率的乘积相除并向零取整；`x`、`y`、`z` 主体位置（lepton） | Techno 受伤入口：步兵卧倒、寄生前缀和引擎自己的护盾之后，阻止死亡、主体护甲和最小伤害 1 之前。入射为 0、无视防御、狂暴或超时空弹头，以及会被免疫、铁幕或敌我许可拒绝的命中不询问；主体护甲的 Verses 为 0 不影响询问 | 对象 | 继续进入原生流程的入射伤害：须为 0 或与 `previous` 同号，否则保留上一环；每一环按自己的 `previous` 重算 `value2`；变成 0 时不再施加最小伤害 1 | preview |

### 4.6 客户端事件种类

在 `client_on_events` 中按渲染帧成批到达（`ClientEventRecord`）；全部 `v1`（阶段 2）。

| 种类 | 字段 | 何时 | 条件 |
| --- | --- | --- | --- |
| `Click` | `ref` 节点，`value` `List` 的行号或 `CameoGrid` 的条目下标（从 0 开始），其他节点为 −1 | 节点被点击；点击不再传给地图 | 节点 `Clickable` 为 1 |
| `Hover` | `ref` 节点，`value` 1 进入 / 0 离开 | 光标进入或离开节点 | 节点 `Hoverable` 为 1 |
| `Hotkey` | `id` 热键 ID | 注册的热键被按下 | 用本地动作 `RegisterHotkey` 注册（5.4）；按键也绑定了游戏热键时只执行游戏热键 |
| `Dismiss` | `ref` 节点 | 在节点外按下鼠标或按 Esc | 节点 `Dismissable` 为 1 且可见 |
| `MapClick` | `ref` 格子，`value` 0 左键 / 1 右键 | 在地图上点击；已产生 `Click` 或 `Dismiss` 的点击除外 | 总是；光标 `CaptureMapClick` 为 1 时点击不再传给游戏 |
| `SelectionChanged` | — | 本地选择变化；用带 `Selected=1` 子句的 `client_find` 读取 | 总是 |
| `Notify` | `source`，`id` 通知 ID，`data`、`data_len` | 发给本机的通知到达（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） | 总是 |

## 5. 动作种类

### 5.1 动作的用法

规则见 [RFC 5](0001-wasm-extension-modules.zh-CN.md#5-执行模型)、[RFC 6b](0001-wasm-extension-modules.zh-CN.md#6b-动作)、[RFC 2](0001-wasm-extension-modules.zh-CN.md#2-来源mod-和地图)。

```rust
enum ActionTarget { Subject = 0, Other = 1, TagHolder = 2, Projectile = 3, House = 4, Cell = 5 }   // 用于触发器；默认为 Subject
```

- 触发器中：目标为 `ActionTarget`，`other` 取自事件，忽略 `ActionParams.other`。直接调用：目标为引用，`ActionParams.other` 为第二个对象（来源、乘客、小队、触发器或标签）。
- “目标”列：*对象* 为 `Subject`、`Other` 或持有对象的 `TagHolder`（直接调用为对象句柄）；*格子* 为 `Cell` 或对象目标（含 `Projectile`）所在格子（直接调用为格子、路径点或对象引用）；`House` 为所属方；`—` 为无目标；未列出的目标种类为非法参数。
- *live*：原版中作用于标签持有者，触发器中默认目标为 `TagHolder`。
- 启用 Ares 时原版动作先走 Ares 分支；除另注外为 `v1`，运行原生代码的随机数为 `native`。

### 5.2 原版动作 0..145

参数为 `ActionParams` 字段；原版用路径点处，API 用 `cell` 中的格子或路径点引用。

版本 1 的层级见[第 9 节](#9-版本-1-清单)。

| # | API 名称 | 目标 | 参数 | 说明 |
| --- | --- | --- | --- | --- |
| 0 | — | — | — | 空操作 |
| 1 | `DeclareWinner` | — | `text` 国家 | 作用于玩家，而非触发器所属方 |
| 2 | `DeclareLoser` | — | `text` 国家 | |
| 3 | `BeginProduction` | `House` | — | |
| 4 | `CreateTeam` | `House` | `id` TeamType | 受数量上限；所属方为 0 时用 TeamType 的；返回小队句柄或 0；仅触发器中加原版的对象放置范围 |
| 5 | `DestroyTeams` | — | `id` TeamType | |
| 6 | `AllToHunt` | `House` | — | |
| 7 | `Reinforce` | — | `id` TeamType | 用 TeamType 自己的路径点 |
| 8 | `DropZoneFlare` | 格子 | — | |
| 9 | `FireSale` | `House` | — | |
| 10 | `PlayMovie` | — | `value` 影片编号 | 对每个玩家 |
| 11 | `ShowText` | — | `text` CSF 条目名 | |
| 12 | `DestroyTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 删除运行中的实例（延迟）；同 `trigger_remove` |
| 13 | `BeginAutocreate` | `House` | — | |
| 14 | `ChangeHouse` | 对象（*live*） | `house` 新所属方 | 同 `SetOwner` |
| 15 | `AllowWin` | — | — | 只是标志（`TriggerDef.flags` 第 1 位），无执行分支 |
| 16 | `RevealAll` | — | — | |
| 17 | `RevealAround` | 格子 | — | |
| 18 | `RevealZone` | 格子 | — | |
| 19 | `PlaySound` | — | `id` 声音 | |
| 20 | `PlayTheme` | — | `text` 音乐 | |
| 21 | `PlaySpeech` | — | `text` EVA 名称 | |
| 22 | `ForceTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 触发时无执行者，格子为零 |
| 23 | `TimerStart` | — | — | |
| 24 | `TimerStop` | — | — | |
| 25 | `TimerExtend` | — | `value` 秒数 | |
| 26 | `TimerShorten` | — | `value` 秒数 | |
| 27 | `TimerSet` | — | `value` 秒数 | |
| 28 | `SetGlobal` | — | `value` 0..49 | 超出范围为非法参数；同 `var_set` |
| 29 | `ClearGlobal` | — | `value` 0..49 | |
| 30 | `AutoBaseBuilding` | `House` | `value` 开关 | |
| 31 | `GrowShroud` | — | — | |
| 32 | `DestroyObject` | 对象（*live*） | — | Ares 幸存者规则；同 `Kill` |
| 33 | `OneTimeSuperWeapon` | `House` | `id` 超级武器类型 | |
| 34 | `RepeatingSuperWeapon` | `House` | `id` 超级武器类型 | |
| 35 | `PreferredTarget` | `House` | `value` | |
| 36 | `AllChangeHouse` | `House` | `house` 新所属方 | |
| 37 | `MakeAlly` | `House` | `house` 另一所属方 | 双向 |
| 38 | `MakeEnemy` | `House` | `house` 另一所属方 | 双向 |
| 39 | — | — | — | 不开放 |
| 40 | `SetVisibleMapArea` | 格子（左上角） | `value` 宽，`value2` 高 | |
| 41 | `PlayAnimAt` | 格子 | `id` 动画 | Ares `MakeInfantry=` 所属规则 |
| 42 | `DetonateWeaponAt` | 格子 | `id` 武器 | 同 `DetonateWeapon`，无来源 |
| 43 | `PlayVoxelAnimAt` | 格子 | `id` 体素动画 | |
| 44 | `IonStormStart` | — | `value` 持续时间 | 风暴进行中为 `Refused` |
| 45 | `IonStormStop` | — | — | 无风暴为 `Refused` |
| 46 | `LockInput` | — | — | |
| 47 | `UnlockInput` | — | — | |
| 48 | `CenterCamera` | 格子 | `value` 速度 | 每个玩家的镜头；客户端形式见 5.4 |
| 49 | — | — | — | 放大；不开放 |
| 50 | — | — | — | 缩小；不开放 |
| 51 | `Reshroud` | `House` | — | |
| 52 | `SpotlightBehavior` | 对象（*live*，建筑） | `value` | |
| 53 | `EnableTrigger` | — | `other` 触发器，或 `text` 触发器 ID | 按难度准入，重置计时器；同写 `Enabled` |
| 54 | `DisableTrigger` | — | `other` 触发器，或 `text` 触发器 ID | |
| 55 | `RadarEvent` | 格子 | `value` `RadarEventType` | |
| 56 | `SetLocal` | — | `value` 0..99 | |
| 57 | `ClearLocal` | — | `value` 0..99 | |
| 58 | `MeteorShower` | 格子 | `value` 强度 | |
| 59 | `ReduceOre` | 格子 | — | |
| 60 | `SellBuilding` | 对象（*live*，建筑） | — | |
| 61 | `TurnOffBuilding` | 对象（*live*，建筑） | — | 同写 `Online` 为 0 |
| 62 | `TurnOnBuilding` | 对象（*live*，建筑） | — | |
| 63 | `DamageBridgeAt` | 格子 | — | |
| 64 | `LightFlashSmall` | 格子 | — | 伤害 50 命中的闪光 |
| 65 | `LightFlashMedium` | 格子 | — | 伤害 100 |
| 66 | `LightFlashLarge` | 格子 | — | 伤害 300 |
| 67 | `AnnounceWin` | — | — | |
| 68 | `AnnounceLose` | — | — | |
| 69 | `EndScenario` | — | — | |
| 70 | `DestroyTag` | — | `other` 标签，或 `text` 标签 ID | 停止进行中的投递 |
| 71 | `SetAmbientStep` | — | `value` | |
| 72 | `SetAmbientRate` | — | `value` | |
| 73 | `SetAmbientLight` | — | `value` | |
| 74 | `AiTriggersOn` | `House` | — | 同写 `AiTriggersActive` |
| 75 | `AiTriggersOff` | `House` | — | |
| 76 | `RatioAiTriggers` | `House` | `value` | |
| 77 | `RatioAircraft` | `House` | `value` | |
| 78 | `RatioInfantry` | `House` | `value` | |
| 79 | `RatioUnits` | `House` | `value` | |
| 80 | `ReinforceAt` | 格子 | `id` TeamType | |
| 81 | `WakeUp` | 对象（*live*） | — | 睡眠或无害的对象改为 Guard |
| 82 | `WakeSleepers` | `House` | — | 唤醒不属于该所属方的睡眠对象 |
| 83 | `WakeHarmless` | `House` | — | 唤醒属于该所属方的无害对象 |
| 84 | `WakeGroup` | — | `value` 编组 | |
| 85 | `VeinGrowth` | — | `value` | |
| 86 | `OreGrowth` | — | `value` | |
| 87 | `IceGrowth` | — | `value` | |
| 88 | `CreateParticleSystem` | 格子 | `id` 粒子系统 | |
| 89 | `RemoveParticleSystems` | 格子 | — | |
| 90 | `LightningStrike` | 格子 | — | |
| 91 | `GoBerserk` | 对象（*live*，步兵） | — | |
| 92 | `FirestormOn` | `House` | — | 未启用 Ares 为 `Unsupported` |
| 93 | `FirestormOff` | `House` | — | 同上 |
| 94 | `IonCannonStrike` | 格子 | — | |
| 95 | `NukeStrike` | 格子 | — | |
| 96 | `ChemStrike` | 格子 | — | |
| 97 | `ToggleTrainCargo` | — | — | |
| 98 | `PlaySoundAtRandomWaypoint` | — | `id` 声音 | Ares 变体；随机数 native |
| 99 | `PlaySoundAt` | 格子 | `id` 声音 | 在格子中心播放 |
| 100 | `PlayRadarMovie` | — | `value` 影片 | |
| 101 | `ReshroudAt` | 格子 | — | |
| 102 | `LightningStormStrike` | 格子 | — | 用触发器所属方 |
| 103 | `TimerText` | — | `text` CSF 条目名 | |
| 104 | `FlashTeam` | — | `id` TeamType，或 `other` 小队；`value` 帧数 | |
| 105 | `TalkBubble` | — | `other` 小队（可为 0），`value` 帧 | |
| 106 | `SetTechLevel` | — | `id` 类型，`value` 等级 | 另见 7.7 |
| 107 | `ReinforceByChrono` | 格子 | `id` TeamType | |
| 108 | `CreateCrate` | 格子 | `value` 种类（20 为随机） | 随机时随机数 native |
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
| 125 | `CreateBuildingAt` | 格子 | `id` 建筑类型 | 返回建筑或 0 |
| 126 | `RestoreStartingTechnos` | `House` | — | |
| 127 | `ChronoScreenEffect` | — | `value` 持续时间 | |
| 128 | `TeleportAll` | `House` | `cell` 目的地 | |
| 129 | `SuperWeaponSetCharge` | `House` | `id` 超级武器类型，`value` 百分比 | 同写 `ChargePercent` |
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

Ares 动作（未启用 Ares 时为 `Unsupported`）：

| # | API 名称 | 目标 | 参数 |
| --- | --- | --- | --- |
| 146 | `AddAuxPower` | `House` | `value` 电力 |
| 147 | `KillDrivers` | `House` | — |
| 148 | `SetEvaVoice` | — | `value` |
| 149 | `SetGroup` | 对象 | `value` 编组 |

### 5.3 原子动作

编号是标识，只在末尾追加；100041 保留（`ApplyEffect`）。“仅代码”动作通过 `action` 传变长数据。未注明返回句柄的动作值为 0，状态随调用返回。`Modify`、`RemoveModifiers` 在版本 1 中只对对象。

| # | 名称 | 目标 | 参数 | 语义及非零返回值 | 随机数 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| 100000 | `FireWeapon` | 对象（射手） | `id` 武器，为 −1 时用 `value` 武器槽位（默认 0 主武器）；瞄准 `cell`，否则 `other`，否则当前目标，都没有为 `Gone` | 开火；当前无法开火为 `Refused` | native | v1 |
| 100001 | `LaunchSuperWeapon` | `House` | `id` 超级武器类型，`cell`（必需，缺少为非法参数） | 发射该所属方的超级武器 | native | v1 |
| 100002 | `SpawnAnim` | 格子 | `id` 动画 | 创建动画 | native | v1 |
| 100003 | `AttachAnim` | 对象 | `id` 动画，`value2` 帧数（负数直到移除） | 把动画附着在对象上 | native | later |
| 100004 | `Remove` | 对象 | — | 移除：不死亡、无幸存者、不计击杀 | none | v1 |
| 100005 | `Kill` | 对象 | `other` 来源（可选） | 经伤害流程击杀，伤害为当前生命、无视防御（有死亡动画和幸存者，击杀记给来源） | native | v1 |
| 100006 | `Damage` | 对象 | `id` 弹头，`value` 伤害；`other` 来源（可选） | 经弹头和护甲造成伤害，计击杀和经验 | native | v1 |
| 100007 | `Heal` | 对象 | `value` 生命点数 | 回复生命，不计击杀 | none | v1 |
| 100008 | `CreateObject` | 格子，或工厂建筑 | `id` 类型；`house` 所属方；`arg` 初始任务（`Mission`，255 默认）；`other` 非 0 时复制其标签 | 成功返回新对象。工厂开门、驶出、前往集结点；出口堵塞则等待。工厂不能生产该类别或目标格已占用时返回 (0, Refused) | native | v1 |
| 100009 | `SetMission` | 对象 | `arg` 任务，`cell` 或 `other` 为任务目标 | 像玩家指令一样排入任务；带格子的 `Move` 移到该格 | none | v1 |
| 100010 | `Teleport` | 对象 | `cell`，`arg` 目标格被占时的规则（`Blocked`） | 超时空式移动；`Nearby` 选附近格子，`Fail` 拒绝 | native | v1 |
| 100011 | `ClearTarget` | 对象 | — | 放弃当前目标 | none | v1 |
| 100012 | `ClearDisguise` | 对象 | — | 解除间谍或幻影坦克的伪装 | none | v1 |
| 100013 | `ClearMindControl` | 对象 | — | 解除心灵控制，还给原所属方 | none | v1 |
| 100014 | `AddExperience` | 对象 | `value` 经验点数 | 增加经验 | none | v1 |
| 100015 | `AddMoney` | `House` | `value` 金额（可为负） | 增减金钱 | none | v1 |
| 100016 | `AllowType` | `House` | `id` 类型，`value` 1 允许 / 0 不允许 | 修改可建造列表 | none | v1 |
| 100017 | `SetOwner` | 对象 | `house` 新所属方 | 转移所属（同写 `Owner`） | none | v1 |
| 100018 | `AddAmmo` | 对象 | `value` 弹药 | 增加弹药 | none | v1 |
| 100019 | `AddPassenger` | 对象（载具） | `other` 乘客 | 乘客登上 | native | v1 |
| 100020 | `RemovePassenger` | 对象（载具） | `other` 乘客 | 乘客离开 | native | later |
| 100021 | `SetProperty` | 对象、`House` 或格子 | `id` 属性，`value` 值 | 写可写属性，同 `set` | none | v1 |
| 100022 | `Modify` | 对象或 `House` | `id` 属性，`arg` 运算（`ModifierOp`），`arg2` 组，`value` 值，`value2` 帧数；`other` 来源（可选） | 加修饰器，同 `modify`；返回修饰器 | none | v1 |
| 100023 | `RemoveModifiers` | 对象或 `House` | `arg2` 组；`id` 非 −1 时只限该属性 | 移除本模块来源在该组的修饰器 | none | v1 |
| 100024 | `SetVar` | 对象、`House` 或 — | `id` 变量，`value` 值 | 写变量：对象作用域取目标对象，所属方作用域取 `House` 目标（为 0 时同 `ActionParams.house`），全局作用域忽略目标；同 `var_set` | none | v1 |
| 100025 | `DetonateWeapon` | 格子 | `id` 武器；`other` 来源（可选） | 在格子中心直接引爆武器，不生成抛射体 | native | v1 |
| 100026 | `DetonateWarhead` | 格子 | `id` 弹头，`value` 伤害；`other` 来源（可选） | 弹头范围伤害 | native | v1 |
| 100027 | `CreateProjectile` | 对象或格子（起点） | `extra` 一个 `ProjectileSpec`；`other` 来源（击杀和经验归属，可为 0） | 从 `origin` 给出的位置创建抛射体，失败为 0；仅代码；返回抛射体 | native | later |
| 100028 | `DetonateProjectile` | `Projectile` | — | 立即引爆 | native | later |
| 100029 | `RemoveProjectile` | `Projectile` | — | 移除，不引爆 | none | later |
| 100030 | `EndFor` | `House` | `arg` 结果（`Outcome`） | 让该所属方胜利或失败 | none | v1 |
| 100031 | `SetScore` | `House` | `value` 分数 | 设定分数 | none | v1 |
| 100032 | `AddScore` | `House` | `value` 分数 | 增加分数 | none | v1 |
| 100033 | `Log` | — | `text` | 写一行游戏内日志 | none | v1 |
| 100034 | `Convert` | 对象 | `id` 类型，`value2` 帧数（0 永久；大于 0 时到期变回） | 改变类型，同 Ares 类型转换；与传送和运动互斥 | native | v1 |
| 100035 | `Push` | 对象 | `cell` 方向目标，`value` 力度 | 被推动或抛起，叠加到移动上；与变形和传送互斥 | native | later |
| 100036 | `Module` | 不限 | `id` 分派键 | 运行模块动作，在模块阶段排队；具名节写作 `Action=Wasm:name`，地图写作动作 100036 | none | v1 |
| 100037 | `CreateTeamUnlimited` | `House` | `id` TeamType | 启动小队，不受 Max 限制（同脚本动作 18）；返回小队 | native | later |
| 100038 | `DisbandTeam` | — | `other` 小队 | 销毁小队，记录 AI 触发器结果，释放成员 | none | later |
| 100039 | `AddTeamMember` | 对象 | `other` 小队 | 加入小队，执行准入检查 | none | later |
| 100040 | `RemoveTeamMember` | 对象 | — | 离开所在小队 | none | later |

### 5.4 客户端动作种类

用 `client_action(kind, target, params, len)` 执行，只影响本机，参数为 `ActionParams`；除 `HomeCamera` 为 `later` 外都是 `v1`（阶段 2）。

| 名称 | 目标 | 参数 | 语义 |
| --- | --- | --- | --- |
| `PlaySound` | 格子或 0 | `id` 声音 | 本机播放；目标 0 时不定位 |
| `PlaySpeech` | — | `text` EVA 名称 | 本机播放 EVA 语音 |
| `CenterCamera` | 格子 | `value` 速度 | 移动本机镜头，同动作 48 |
| `JumpCamera` | 格子 | — | `Camera.jumpTo(cell)` 跳转本机镜头；地图动作 112 保留原有投递范围 |
| `HomeCamera` | — | — | 同动作 145 |
| `Select` | — | `extra` 对象句柄数组，`value` 1 为追加 | 本地选择；只接受本玩家能选的对象 |
| `FlashObject` | 对象 | `value` 帧数 | 对象在本机闪烁 |
| `ShowMessage` | — | `text` CSF 条目名或文字，`value` 颜色 | 显示一行消息 |
| `RegisterHotkey` | — | `text` 热键名，`extra` 默认按键（文字，如 `Ctrl+Alt+K`：修饰键加 `+` 加键名） | 注册热键；ID 用 `client_lookup(Hotkey, name)` 取；默认按键已绑定游戏热键时注册成功但不绑定按键，状态为 `Refused`；游戏热键始终优先 |

## 6. AI 小队

见 [RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)；小队动作为 5.2 的 4、5、7、80、107 和 5.3 的小队动作，状态为属性（7.9）。

### 6.1 定义

AI 文档的读写层次见 [RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)。运行时用 `lookup(TeamType, name)`、`lookup(ScriptType, name)`、`lookup(AiTrigger, name)` 返回的 ID 和对应的类型引用。

| 节 | 内容 | 说明 |
| --- | --- | --- |
| `[TaskForces]` 和每个特遣部队的节 | `Name`、`Group`、`0=数量,类型` 等行 | 最多 6 行 |
| `[ScriptTypes]` 和每个脚本的节 | `Name`、`0=动作,参数` 等行 | 最多 50 行；动作编号见 6.2 |
| `[TeamTypes]` 和每个小队类型的节 | `Name`、`Script`、`TaskForce`、`Priority`、`Max`、`Group`、`House`、`VeteranLevel`、`Waypoint`、`TransportWaypoint`、`IsBaseDefense`、`Suicide`、`Aggressive`、`UseTransportOrigin`、`Full`、`Whiner`、`Autocreate`、`AreTeamMembersRecruitable`、`Reinforce`、`AvoidThreats`、`OnTransOnly`、`Recruiter`、`Annoyance`、`GuardSlower`、`TransportsReturnOnUnload`、`MindControlDecision`、`Droppod`、`OnlyTargetHouseEnemy`、`Loadable`、`Tag` | 默认值来自读取器 |
| `[AITriggerTypes]` | 每行 18 项：名字、主小队、所属方、条件、对象类型、数量、比较符、起始权重、最小权重、最大权重、多人、阵营、次小队、简单、普通、困难…… | 写入后当前权重变为初始权重 |

- `Whiner` 被读取但无效果；`TechLevel`、`Prebuild`、`LooseRecruit`、`IonImmune` 不被读取；AI 触发器第 3、11、13 项未使用。
- AI 触发器条件：-1 总是成立；0 敌方拥有 N 个该类型；1 本所属方拥有；2 敌方电力盈余低于 100；3 低于 0；4 比较敌方金钱；5 己方铁幕即将就绪；6 己方超时空传送仪即将就绪；7 平民阵营的所属方拥有。比较符 0..5 同 `Compare`（8.3），其他值永不成立。
- 运行时修改 AI 触发器权重为 `later`。

### 6.2 小队脚本动作

`[ScriptTypes]` 每行的动作编号，作为数据全部 `v1`；“对应”为对单个对象做同样事情的表项。

| # | `ScriptAction` | 参数 | 对应 |
| --- | --- | --- | --- |
| 0 | `AttackQuarry` | 目标类别（2..11） | `SetMission`（`Hunt`） |
| 1 | `AttackWaypoint` | 路径点 | `SetMission`（`Attack`，格子） |
| 2 | `GoBerserk`（惯用名） | —；空操作，永不结束 | — |
| 3 | `MoveToWaypoint` | 路径点 | `SetMission`（`Move`，格子） |
| 4 | `MoveToCell` | 打包的格子（x = arg%128，y = arg/128） | `SetMission`（`Move`，格子） |
| 5 | `Guard` | `arg*15` 帧 | `SetMission`（`Guard`） |
| 6 | `JumpToLine` | 从 1 开始的行号 | 小队的 `Cursor` 属性 |
| 7 | `PlayerWins` | — | `EndFor` |
| 8 | `Unload` | 1、2、3 | `RemovePassenger` |
| 9 | `Deploy` | — | — |
| 10 | `FollowFriendlies` | — | — |
| 11 | `DoMission` | 任务 ID | `SetMission` |
| 12 | `SetGlobal` | 下标 | `SetVar`（全局变量） |
| 13 | `IdleAnimation` | 动画 | — |
| 14 | `LoadOntoTransport` | — | `AddPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | 路径点 | — |
| 16 | `PatrolToWaypoint` | 路径点 | `SetMission`（`Patrol`） |
| 17 | `ChangeScript` | ScriptType 序号 | 小队的 `Script` 属性 |
| 18 | `ChangeTeam` | TeamType 序号 | — |
| 19 | `Panic` | — | — |
| 20 | `ChangeHouse` | 国家下标 | `SetOwner` |
| 21 | `Scatter` | — | — |
| 22 | `GoToNearbyShroud` | —；非单人模式下拒绝执行 | — |
| 23 | `PlayerLoses` | — | `EndFor` |
| 24 | `PlaySpeech`（惯用名） | —；只前进到下一行 | — |
| 25 | `PlaySound` | SOUNDMD 序号 | 原版动作 19 |
| 26 | `PlayMovie` | 影片 | 原版动作 10 |
| 27 | `PlayMusic` | 音乐主题 | 原版动作 20 |
| 28 | `ReduceOre` | — | 原版动作 59 |
| 29 | `BeginProduction` | — | 原版动作 3 |
| 30 | `FireSale` | — | 原版动作 9 |
| 31 | `SelfDestruct` | — | `Kill` |
| 32 | `IonStormStart` | 持续时间 | 原版动作 44 |
| 33 | `IonStormEnd` | — | 原版动作 45 |
| 34 | `CenterCamera` | 速度 | 原版动作 48 |
| 35 | `ReshroudMap` | — | 原版动作 51 |
| 36 | `RevealMap` | — | 原版动作 16 |
| 37 | `DeleteMembers` | — | `Remove` |
| 38 | `ClearGlobal` | 下标 | `SetVar`（全局变量） |
| 39 | `SetLocal` | 下标 | `SetVar`（局部变量） |
| 40 | `ClearLocal` | 下标 | `SetVar`（局部变量） |
| 41 | `Unpanic` | — | — |
| 42 | `ForceFacing` | 朝向 | — |
| 43 | `WaitUntilFullyLoaded` | — | — |
| 44 | `TruckUnload` | — | `Convert` |
| 45 | `TruckLoad` | — | `Convert` |
| 46 | `AttackEnemyBuilding` | 类型 `arg&0xFFFF`，模式 `arg>>16` | — |
| 47 | `MoveToEnemyBuilding` | 同上 | — |
| 48 | `ScoutHouses` | — | — |
| 49 | `RegisterSuccess` | —；前进，并把小队标记为成功 | 小队的 `Succeeded` 属性（只读） |
| 50 | `Flash` | 帧数 | 原版动作 104 |
| 51 | `PlayAnimation` | 低 16 位为动画，高 16 位为循环次数 | `SpawnAnim` |
| 52 | `TalkBubble` | 帧 | 原版动作 105 |
| 53 | `GatherAtEnemyBase` | Ares：+ `AISafeDistance` | — |
| 54 | `RegroupAtBase` | Ares：+ `AIFriendlyDistance` | — |
| 55 | `IronCurtain` | Ares：`SW.Group` | `LaunchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | 同 46 | `Teleport` |
| 57 | `ChronoshiftToQuarry` | 目标类别 | `Teleport` |
| 58 | `MoveToOwnBuilding` | 类型、模式 | — |
| 59 | `AttackBuildingAtWaypoint` | 路径点 | — |
| 60 | `EnterGrinder` | — | — |
| 61 | `OccupyTankBunker` | — | — |
| 62 | `EnterBioReactor` | — | — |
| 63 | `OccupyBattleBunker` | — | — |
| 64 | `GarrisonNearest` | — | — |
| 65, 66 | — | —；不使用 | — |
| 67 | `AresSeekVehicle` | —；Ares，与 64 共用分支，被步骤检查挡住，执行不到 | — |
| 68 | `AresConvert` | —；仅 Ares | `Convert` |

## 7. 属性

按引用种类分组。列：

- **值**：[值类型](#值)。
- **读写**：`R` 只读；`W` 经 owner 写入；`W(rules)` 仅在 `on_rules` 写入（类型或全局规则上的 `set` 等同 `rules_set`）。版本 1 中类型和全局规则在对局中不可写（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)）。不可写时返回 `Refused`。
- **客户端**（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)）：`all` 同模拟；`own` 只对本玩家所属方及其对象，其他为空值和 `Refused`；`seen` 自己的全部，其他所属方中当前能看到的对象按 RFC 10 的限制，其他对象不存在；`sim` 仅模拟，客户端为空值和 `Unsupported`。
- **视点**属性取决于视点所属方（筛选子句：触发器所属方；搜索：`viewpoint`；客户端：本玩家），直接 `get` 为 `Unsupported`（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-对象属性与修饰器)）；**派生**属性由引擎算出，只读；标为“派生，高”的需要搜索或射程判定，在筛选子句中求值时按费用计入所属模块的额度（[RFC 15](0001-wasm-extension-modules.zh-CN.md#15-性能)），其他派生属性的费用与读一个字段相同。

### 7.1 对象

`Techno` 的属性；除最后一行外都不可修饰。

| 属性 | 值 | 读写 | 客户端 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `Type` | ref（类型） | R | seen（伪装的敌人为伪装类型） | v1 | 改类型用 `Convert` |
| `Owner` | ref（所属方） | W | seen（伪装的敌人为伪装所属方） | v1 | 写入同 `SetOwner` |
| `Category` | enum `Category` | R | seen | v1 |  |
| `Health` | int | W | seen | v1 | 写入设定生命值，不计击杀 |
| `Strength` | int | R | seen | v1 | 最大生命 |
| `HealthPercent` | int | R（派生） | seen | v1 |  |
| `X`、`Y`、`Z` | lepton | R | seen | v1 |  |
| `Cell` | ref（格子） | R | seen | v1 |  |
| `Layer` | enum `Layer` | R | own | v1 |  |
| `LandType` | enum `LandType` | R（派生） | own | v1 | 所在格子的地形类型 |
| `Mission` | enum `Mission` | W | own | v1 | 写入像玩家指令一样排入任务，无目标 |
| `Target` | ref | R | own | v1 | 对象或格子；清除用 `ClearTarget` |
| `Rank` | enum `Rank` | W | own | v1 | 写入设定等级 |
| `Experience` | int | R | own | v1 | 增加用 `AddExperience` |
| `Ammo` | int | W | own | v1 |  |
| `Storage` | int | R | own | v1 | 载矿量 |
| `Transport` | ref | R | own | v1 | 所在载具或建筑，无为 0；乘客用带 `Transport = t` 子句的 `find` |
| `PassengerCount` | int | R | own | v1 |  |
| `TransportSpaceLeft` | int | R（派生） | own | v1 |  |
| `Team` | ref（小队） | R | sim | later | 加入和离开用小队动作 |
| `Tag` | ref（标签） | W | sim | later | 写入已有标签，替换原标签（[RFC 7](0001-wasm-extension-modules.zh-CN.md#7-运行时的触发器和标签)） |
| `DisguiseType`、`DisguiseOwner` | ref | R | own | v1 | 模拟读真实状态；无伪装为 0 |
| `Cloaked` | bool | R | own | later |  |
| `Moving` | bool | R | own | v1 |  |
| `Facing` | int 0..255 | R | own | v1 | 车体朝向，刻度同地图单位的朝向和脚本动作 `ForceFacing` |
| `TurretFacing` | int 0..255 | R | own | v1 | 炮塔朝向，刻度同 `Facing`；没有炮塔为 `Unsupported` |
| `Armor` | id（`Armor`） | R | own | later |  |
| `Online` | bool | W | own | v1 | 建筑；同原版动作 61、62 |
| `Selected` | bool | R | 客户端专用 | v1（阶段 2） | 本地选择；模拟中为 `Unsupported` |
| `Relation` | enum `Relation` | R（视点） | own | v1 | 与视点所属方的关系 |
| `NearbyEnemyCount` | int | R（派生，高） | own | later | 视野（`Sight`）内的敌方对象数 |
| `TargetInRange` | bool | R（派生，高） | own | later | 当前目标在主武器射程内 |
| `CanFireAtTarget` | bool | R（派生，高） | own | later | 现在能向当前目标开火 |
| `DoorOpen` | bool | W | seen | later | 建筑：工厂出口的门开着；没有出口门的建筑和其他对象为 `Unsupported`；写入只改引擎绘制的显示状态，不开始生产或出厂 |
| `Animating` | bool | W | seen | later | 建筑：工作动画开着；其他对象为 `Unsupported`；写入只改引擎绘制的显示状态，不开始生产或出厂 |
| 可修饰属性 | 见 7.2 | | | | 可修饰 |

### 7.2 可修饰属性

基础值来自类型或默认值，有效值由修饰器算出（[RFC 6c](0001-wasm-extension-modules.zh-CN.md#6c-对象属性与修饰器)），Phobos 键不为 `—` 的属性按 Phobos 的算法合成；只能用 `modify` 写（`set` 为 `Refused`），客户端为 `own`，另注除外。

| 属性 | 值 | 基础值 | 允许的运算 | 含义 | Phobos 键 | 层级 |
| --- | --- | --- | --- | --- | --- | --- |
| `ArmorMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 受伤倍率 | `ArmorMultiplier`（`.AllowWarheads`、`.DisallowWarheads` 等弹头过滤只在 Phobos 效果类型中可用） | v1 |
| `TargetArmor` | id（`Armor`） | 类型的护甲 | 覆盖 | 选目标、威胁评分、选武器和开火许可使用的护甲；不改变伤害结算 | — | preview |
| `FirepowerMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 火力倍率 | `FirepowerMultiplier` | v1 |
| `SpeedMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 速度倍率 | `SpeedMultiplier` | v1 |
| `ROFMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 射速倍率 | `ROFMultiplier`、`ROFMultiplier.ApplyOnCurrentTimer` | v1 |
| `WeaponRangeMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 射程 | `WeaponRange.Multiplier` | v1 |
| `WeaponRangeExtra` | lepton | 0 | 加、乘、覆盖 | 射程 | `WeaponRange.ExtraRange` | v1 |
| `TargetPriorityMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 目标优先级倍率（配合 `TargetChanged`） | — | later |
| `CritMultiplier` | fixed | 1.0 | 加、乘、覆盖 | 暴击 | `Crit.Multiplier` | later |
| `CritExtraChance` | fixed | 0 | 加、乘、覆盖 | 暴击 | `Crit.ExtraChance` | later |
| `DamageReceivedExtra` | int | 0 | 加、乘、覆盖 | 受伤固定加值 | — | later |
| `ReflectDamagePercent` | fixed | 0 | 加 | 反弹伤害 | `ReflectDamage`、`ReflectDamage.*` | later |
| `RevengeWeapon` | id（武器） | −1 | 覆盖 | 复仇武器 | `RevengeWeapon`、`RevengeWeapon.*` | later |
| `PrimaryWeapon`、`SecondaryWeapon` | id（武器） | 类型的武器 | 覆盖 | 替换武器 | — | later |
| `Unkillable` | bool | 0 | 覆盖 | 阻止死亡 | `Unkillable` | later |
| `Cloakable` | bool | 类型的 `Cloakable` | 覆盖 | 隐形 | `Cloakable` | v1 |
| `ForceDecloak` | bool | 0 | 覆盖 | 隐形 | `ForceDecloak` | v1 |
| `DisableWeapons` | bool | 0 | 覆盖 | 禁止开火 | `DisableWeapons` | v1 |
| `DisableTargeting` | bool | 0 | 覆盖 | 停止索敌 | — | later |
| `Selectable` | bool | 1 | 覆盖 | 为 0 时无法被选中 | — | later |
| `IgnoreOrders` | bool | 0 | 覆盖 | 无视玩家指令 | — | later |
| `Targetable` | bool | 1 | 覆盖 | 为 0 时不可被选为目标 | — | later |
| `PenetratesForceShield` | bool | 0 | 覆盖 | 穿透力场护盾 | `PenetratesForceShield` | later |
| `ImmunityFlags` | mask | 类型的免疫 | 覆盖 | 免疫位：0 心灵控制、1 辐射、2 毒素、3 心灵、4 超时空、5 EMP、6 狂暴 | —（`ArmorMultiplier.AllowWarheads` 倍率为 0 可覆盖一部分） | later |
| `TintColor` | int（RGB） | 0 | 覆盖 | 染色；客户端为 `seen` | `Tint.Color` | later |
| `TintIntensity` | fixed | 0 | 加 | 染色；客户端为 `seen` | `Tint.Intensity`、`Tint.Cumulative` | later |
| `LaserTrailType` | id | −1 | 覆盖 | 激光拖尾；客户端为 `seen` | `LaserTrail.Type` | later |
| `PowerOutage` | bool | 0 | 覆盖 | 所属方断电（所属方属性，7.5） | — | later |

附着动画、限时转换、推动和传送分别见 [5.3](#53-原子动作) 的 `AttachAnim`、`Convert`、`Push`、`Teleport`。

**Phobos AttachEffect 键**仅在 Phobos 开关打开时读取，语义以 Phobos 提交 `31ae9f85` 的实现为准，由 AttachEffect 的 owner 按实例处理，不映射成修饰器：

| Phobos 键 | 语义 |
| --- | --- |
| `Duration`、`AttachEffect.DurationOverrides` | 实例时长，负数无限；刷新时未给覆盖值则沿用实例原来的；在对象回合中递减，离场、被冻结或在载具内时暂停 |
| `Cumulative=no`、`AttachEffect.ReplaceLongerDuration` | 同类型一个实例；新时长不短于剩余或允许替换时刷新 |
| `Cumulative`、`Cumulative.MaxCount` | 按类型计层，默认不限；满时刷新符合条件中剩余最短的实例 |
| `AttachEffect.CumulativeSourceMaxCount` | 按（施加者、来源弹头或对象）计层 |
| `AttachEffect.CumulativeRefreshAll`、`.OnAttach`、`AttachEffect.CumulativeRefreshSameSourceOnly` | 刷新全部符合条件的实例；`.OnAttach` 时未满也刷新并新增；默认只刷新同来源 |
| `AttachEffect.Delays`、`AttachEffect.InitialDelays`、`AttachEffect.RecreationDelays` | 到期后的重建延迟、首次生效延迟、被移除后的重建延迟；都不是冷却 |
| `Groups`、`RemoveTypes`、`RemoveGroups` | 标签和移除，不是修饰器的互斥组；弹头先附加后移除 |
| `RequiredGroups`、`DisallowedGroups`、`IgnoreFromSameSource`、`SameSourceOnly` | 武器的目标筛选，不影响施加 |
| `AllowTransfer`、`AllowTransfer.Convert`、`AttachEffects.AttachOnOwnerChange` | 只作用于 Phobos 效果 |
| `PenetratesIronCurtain`、`Powered`、`DiscardOn`、`DiscardOn.*` | 实例的准入、生效和丢弃 |
| `ExpireWeapon*`、`Animation*`、`CumulativeAnimations*` | 实例的到期武器和动画 |

Phobos 键为 `—` 的属性随模块功能开启（[RFC 14](0001-wasm-extension-modules.zh-CN.md#14-项目契约)）。

### 7.3 修饰器

`modify` 返回的句柄，到期或移除时失效；全部 `R`、客户端 `sim`、`v1`；用 `find`（范围 `Modifiers`，子句 `Target = 引用`）按加入顺序列出。

| 属性 | 值 | 说明 |
| --- | --- | --- |
| `Target` | ref | 被修饰的对象或所属方 |
| `Property` | int | 属性 ID |
| `Op` | enum `ModifierOp` | |
| `Value` | int | 原样保存的值 |
| `Remaining` | frames | 剩余帧数；无限为 −1 |
| `Source` | ref | 来源对象，可为 0 |
| `Group` | int | 0 表示不分组 |
| `Origin` | enum `Source` | 加入它的来源：mod、地图或 INI 行为。Phobos 效果不是修饰器 |

### 7.4 抛射体

全部 `later`。

| 属性 | 值 | 读写 | 客户端 |
| --- | --- | --- | --- |
| `Type` | ref（抛射体类型） | R | seen |
| `Category` | enum `Category`：总是 `Projectile` | R | seen |
| `X`、`Y`、`Z` | lepton | R | seen |
| `VX`、`VY`、`VZ` | fixed：速度，定点数（16 位小数），lepton 每帧 | R | own |
| `Source` | ref | R | own |
| `Target` | ref | R | own |
| `Data0`、`Data1` | int：16 字节模块数据，`Data0` 为第 0..7 字节，`Data1` 为第 8..15 字节，小端序；存档并计入哈希 | W | sim |

### 7.5 所属方

| 属性 | 值 | 读写 | 客户端 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `Name` | string | R | all | later |  |
| `Country` | ref（国家类型） | R | all | v1 | 同一国家可对应多个所属方，按所属方下标分页 |
| `Side` | int | R | all | later |  |
| `Index` | int | R | all | v1 | 所属方数组下标 |
| `Money` | int | R | own | v1 | 修改用 `AddMoney` |
| `PowerOutput`、`PowerDrain` | int | R | own | v1 |  |
| `LowPower` | bool | R | own | v1 |  |
| `TechLevel` | int | R | own | v1 |  |
| `IsDefeated`、`IsHuman`、`IsCivilian` | bool | R | all | v1 |  |
| `Kills`、`Losses` | int | R | own | later | 统计 |
| `AiTriggersActive` | bool | W | sim | later | 同原版动作 74、75 |
| `Relation` | enum `Relation` | R（视点） | all | v1 | 盟友和敌人用带 `Relation` 子句的 `find` 列出 |
| `PowerOutage` | bool | R | own | later | 可修饰（7.2） |

### 7.6 格子

| 属性 | 值 | 读写 | 客户端 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `LandType` | enum `LandType` | R | all | v1 |  |
| `Height` | int | R | all | v1 |  |
| `Overlay` | ref（覆盖物类型） | R | seen（当前能看到的格子） | v1 | 没有为 0 |
| `Ore` | int | R | seen | v1 |  |
| `Passable.<SpeedType>` | bool | R | seen | v1 | 只看地形，不看对象和覆盖物；每种 `SpeedType` 一个（`Passable.Foot`、`Passable.Track`……） |
| `Revealed` | bool | R（视点） | own | v1 | 视点所属方是否已揭开 |
| `Placeable` | bool | R（视点） | own | v1 | 查询的 `type` 能否由视点所属方放在该格；只在 `find`、`client_find` 中有效，其他地方为 `Unsupported` |
| `CellTag` | ref（标签） | R | sim | v1 | 没有为 0 |
| `Waypoint` | int | R | all | v1 | 该格的路径点编号，没有为 −1 |

### 7.7 类型与规则

常用 INI 键的带类型形式（解析后的值），其他键用 `rules_get`（[RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则)）；客户端全部 `all`。版本 1 中对局中全部只读；层级见[第 9 节](#9-版本-1-清单)。

| 属性 | 值 | 类型 | 读写 |
| --- | --- | --- | --- |
| `Name` | string | 任意类型 | R |
| `Category` | enum `Category` | TechnoType | R |
| `Buildable` | bool | TechnoType | R（视点）：视点所属方的前提条件已满足 |
| `Enabled` | bool | AI 触发器 | W：启用或禁用 |

下面两表中标了值类型的键也是同名属性，对局中都为 `R`；列表和字符串键没有属性，只用 `rules_get`、`rules_set`。

**以后的白名单候选**（版本 1 中只读；进入白名单前须逐键满足 [RFC 8](0001-wasm-extension-modules.zh-CN.md#8-修改规则) 的条件）：

| 类型 | 属性（键） | 已有对象 | 说明 |
| --- | --- | --- | --- |
| Weapon | `Damage` int、`ROF` frames、`Burst` int、`MinimumRange` lepton | 下次使用时取新值 | |
| Weapon | `Range` lepton | 同上 | 仅当抛射体 `ROT>0`；`ROT=0` 时仅限 `on_rules` |
| Warhead | `Verses` | 同上 | 列表，只用 `rules_set` |
| Warhead | `CellSpread` lepton、`PercentAtMax` fixed、`Wall`、`Wood`、`Sparky`、`Fire`、`Conventional`、`Rocker` bool、`InfDeath` int | 同上 | |
| TechnoType | `BuildTime` 类倍率 fixed | 按需计算 | |
| TechnoType | `Speed` int（INI 值） | 按需换算 | 检查各移动方式自己的状态 |
| TechnoType | `Soylent` int、`Selfhealing`、`Explodes` bool、`VeteranAbilities`、`EliteAbilities` | 同上 | 能力列表只用 `rules_set` |
| TechnoType | `Power` int | 重新计算每个拥有它的所属方的电力 | |
| 全局 | 数值型的 `[General]`、`[CombatDamage]`、`[AudioVisual]` 字段 | 同上 | 全局规则引用的属性或 `rules_set`；值类型同键：整数为 int，小数为 fixed，以格计的距离为 lepton，时间为 frames；按帧换算的值需重新换算 |

**构造时复制到每个对象**（只读；写明已有对象的处理规则后才能进白名单）：

| 键 | 需要的规则 |
| --- | --- |
| `Cost` int | 已付款未完成的生产和退款：否则改价后取消会凭空得钱 |
| `ROT` int | 更新每个对象的朝向控制器 |
| `Crushable` bool | 更新每个对象的副本 |
| `Strength` int | 保持生命点数或保持比例 |
| `Sight` int（格） | 更新每个对象及其已揭示的区域 |
| `Ammo`、`InitialAmmo` int | 修改每个对象的容量 |
| `Storage` int | 更新容量 |
| `Passengers`、`SizeLimit` int | 更新容量；超出时怎么办 |
| 类型上的 Phobos `AttachEffect.*` | 只影响新对象 |

**仅限 `on_rules`**（`W(rules)`）：

| 键 | 对局中的替代 |
| --- | --- |
| 武器 `Range`、`[General] Gravity`、抛射体 `ROT`、`Floater`、Phobos 抛射体 `Gravity` | |
| `Primary`、`Secondary`、`Weapon{n}`、`EliteWeapon{n}`、`OccupyWeapon`、武器的 `Warhead`/`Projectile` | 单个对象用 `PrimaryWeapon`、`SecondaryWeapon` 修饰器 |
| `Prerequisite`、`PrerequisiteOverride`、`[General] Prerequisite*`、Ares `Prerequisite.List{n}` | |
| `Owner`、`RequiredHouses`、`ForbiddenHouses` | |
| 武器 `Report`、`Anim`，弹头 `AnimList`，抛射体 `Inaccurate`、`Arcing`、`Shadow` | |
| `Factory` | |
| `Foundation`、`[AI] AIBaseSpacing`、`BuildConst`、`IsBaseDefense`、`UndeploysInto`、`Spawned`/`MissileSpawn` | |
| `Spawns`、`SpawnsNumber`、心灵控制和时间武器的设置 | |
| `Armor`（名称） | |
| `Image`、美术键、`Sequence` | |
| Ares `PoweredBy`、`PowersUnit`、`PoweredUnit` | |
| 列表节、任何指向新类型的键 | |
| `[Colors]`、国家的 `Color`、`Side`、`ParentCountry` | |
| 声音和 EVA 键 | |
| `TechLevel` | `SetTechLevel`（动作 106） |
| `[TaskForces]`、`[ScriptTypes]`、`[TeamTypes]`、`[AITriggerTypes]` | 版本 1 中在 `on_rules` 也为 `Refused`（[RFC 6d](0001-wasm-extension-modules.zh-CN.md#6d-ai)） |

Ares 和 Phobos 的键尚未分类。

### 7.8 全局引用

全局规则引用（种类 4）：场景和全局状态。

| 属性 | 值 | 读写 | 客户端 | 层级 |
| --- | --- | --- | --- | --- |
| `Frame` | int | R | all | v1 |
| `Difficulty` | enum `Difficulty` | R | all | v1 |
| `GameMode` | string | R | all | later |
| `PlayerCount` | int | R | all | v1 |
| `MissionTimer` | frames | R | all | v1（修改用原版动作 23 至 27） |
| `MissionTimerRunning` | bool | R | all | v1 |
| `AmbientLight` | int | R | all | v1 |
| `IonStormActive` | bool | R | all | v1 |
| `GameSpeed` | int | R | all | v1 |
| 全局数值键 | 见 7.7 | R | all | later |

`GameSpeed` 取值同游戏选项中的游戏速度档位；版本 1 中只读。

### 7.9 触发器、标签、小队和超级武器

触发器、标签和小队的属性客户端为 `sim`；超级武器的属性客户端为 `own`。

| 引用 | 属性 | 值 | 读写 | 层级 | 说明 |
| --- | --- | --- | --- | --- | --- |
| 触发器 | `Enabled` | bool | W | v1 | 写 1 同原版动作 53，写 0 同 54；作用于所有实例 |
| 触发器 | `House` | ref | R | v1 | 0 表示主体当时的所属方（没有主体时为中立所属方） |
| 触发器 | `Repeat` | enum `TagRepeat` | R | v1 |  |
| 触发器 | `MapId` | string | R | later | 地图中的触发器 ID；按 ID 找用带 `MapId` 子句的 `find` |
| 标签 | `MapId` | string | R | later |  |
| 标签 | `Repeat` | enum `TagRepeat` | R | later |  |
| 小队 | `Type` | ref（TeamType） | R | later |  |
| 小队 | `House` | ref | R | later |  |
| 小队 | `Script` | ref（ScriptType） | W | later | 修改运行中小队的脚本 |
| 小队 | `Cursor` | int | W | later | 脚本游标；写入即跳到该行 |
| 小队 | `Succeeded` | bool | R | later |  |
| 小队 | `MemberCount` | int | R | later | 成员用带 `Team = t` 子句的 `find` |
| 超级武器 | `Type` | ref | R | later |  |
| 超级武器 | `Owner` | ref | R | later |  |
| 超级武器 | `Ready` | bool | R | later |  |
| 超级武器 | `ChargePercent` | int | W | later | 同原版动作 129 |
| 超级武器 | `RechargeTime` | frames | W | later | 同原版动作 132 |

### 7.10 客户端节点

只属于客户端，用 `client_set`、`client_set_bytes` 写；模拟中不存在。

**节点种类**（`NodeKind`）。版本 1 可创建 `Text`、`Button`、`Row`、`Column`、`Image`、`Bar`、`List`。`CameoGrid` 只能由 INI 创建：模块可以查找、读取、写它的通用属性和 `ItemTooltips`，`client_node_create(CameoGrid)` 返回 `Unsupported`。侧边栏内的坐标为侧边栏像素，战场和覆盖层内为屏幕像素：

| 种类 | `veraui.ini` 的 `Type` | 层级 |
| --- | --- | --- |
| `Text` | `Text` | v1 |
| `Button` | `Button` | v1 |
| `Row`、`Column` | `Row`、`Column` | v1 |
| `CameoGrid` | `CameoGrid` | v1 |
| `List` | `List`（本 RFC 新增） | v1 |
| `Image` | `Image`（本 RFC 新增） | v1 |
| `Bar`（进度条） | `Bar`（本 RFC 新增） | v1 |
| `Decoration` | —：挂在一个对象上的显示属性 | later |
| `MapMarker` | —：小地图或地图上的点、线、文字 | later |

**节点属性**（除注明外可写；`string`、`bytes` 用 `client_set_bytes` 写）：

| 属性 | 值 | 适用于 | 说明 |
| --- | --- | --- | --- |
| `Name` | string | 全部 | `client_node_find` 用的名字；`veraui.ini` 节点为部件名（节名中布局前缀之后的部分），只读 |
| `Label` | string | `Text`、`Button`、`Decoration` | CSF 条目名或文字：有该名字的 CSF 条目时显示条目，否则显示文字本身 |
| `Art` | string | `Button`、`Image`、`Decoration` | 图片名，按名字解析，格式由引擎决定 |
| `X`、`Y`、`Width`、`Height` | int | 全部 | 像素 |
| `Hold` | enum `Hold` | 全部 | |
| `Gap` | int | `Row`、`Column` | 各部分间距（像素） |
| `Visible` | bool | 全部 | |
| `Enabled` | bool | `Button`、`List` | 为 0 时灰显且不产生 `Click` |
| `Dismissable` | bool | 全部 | 为 1 且可见时，在节点外按下鼠标或按 Esc 产生 `Dismiss`；这次按下不传给地图 |
| `Clickable` | bool | 全部 | 为 1 时产生 `Click`；INI 的 `Button` 默认为 1，其他为 0 |
| `Hoverable` | bool | 全部 | 为 1 时产生 `Hover` |
| `Items` | string | `List`、`CameoGrid` | 每行一个条目，`\n` 分隔：`List` 为 CSF 条目名或文字；`CameoGrid` 为类型名，只用于模块新建的网格，原生网格只读 |
| `ItemProgress` | bytes | `CameoGrid` | `later`。按 `Items` 的顺序每个条目一个 `i32`：0..100，−1 为没有 |
| `ItemTooltips` | string | `CameoGrid` | 每行 `类型名=附加文字`，附加在该类型的提示文字之后；原生网格也可写 |
| `Fill` | int | `Bar` | 0..100 |
| `Anchor` | ref（对象） | `Decoration` | 所挂的对象；对象看不见时不显示 |
| `Palette` | string | `Decoration` | |
| `Tint`、`TintIntensity` | int | `Decoration` | 本机染色（模拟中的染色见 7.2） |
| `Anim` | string | `Decoration` | 附加动画 |
| `HideAnchor` | bool | `Decoration` | 在本机隐藏对象本身 |
| `Number` | int | `Decoration` | 显示在对象上方的数字 |

**INI 布局键。** `Type` 取节点种类表中的 INI 名。`X`、`Y`、`Width`、`Height`、`Gap`、`Art` 与节点属性同义。`Hold` 取 [8.3](#83-枚举) 的九个位置；INI 的 `Label` 用于按钮和文本。

| 布局专用键 | 取值 |
| --- | --- |
| `Parts` | 行或列的子节点 |
| `Command` | 键盘命令名，如 `StopObject`、`GuardObject` |
| `Value` | `Credits`、`Power`、`Drain`、`Name`、`Time`、`Kills`、`Losses` |
| `Tab`、`Columns`、`Rows` | 图标网格 |

**客户端全局引用**（种类 6）：

| 引用 | 属性 | 值 | 读写 | 层级 |
| --- | --- | --- | --- | --- |
| 镜头 | `Cell` | ref（格子） | R：`Camera.cell()` 读取当前格子 | v1 |
| 镜头 | `X`、`Y` | lepton | W：镜头中心，写入即跳转，钳制在地图内 | later |
| 本地设置 | `Viewer` | ref（所属方） | R：本机所属方，观战者为 0 | v1 |
| 本地设置 | `IsSpectator` | bool | R | v1 |
| 本地设置 | 其他本地设置 | — | 不开放（[RFC 10](0001-wasm-extension-modules.zh-CN.md#10-客户端模块)） | later |
| 光标 | `X`、`Y` | int | R：屏幕像素，与节点 `X`、`Y` 同一坐标系 | v1（阶段 2） |
| 光标 | `Cell` | ref（格子） | R：光标下的格子；不在地图上时为 0 | v1（阶段 2） |
| 光标 | `CaptureMapClick` | bool | W：为 1 时地图点击只作为 `MapClick` 交给模块，不传给游戏；为 0（默认）时照常传给游戏，`MapClick` 仍会到达 | v1（阶段 2） |

## 8. 名字种类、搜索范围与其他枚举

### 8.1 名字种类（`lookup`）

| # | 种类 | 解析目标或特殊返回值 |
| --- | --- | --- |
| 0 | `TechnoType` | 步兵、载具、飞行器和建筑类型 |
| 1 | `Weapon` | 武器 |
| 2 | `Warhead` | 弹头 |
| 3 | `Projectile` | 抛射体（BulletType） |
| 4 | `Anim` | 动画 |
| 5 | `VoxelAnim` | 体素动画 |
| 6 | `ParticleSystem` | 粒子系统 |
| 7 | `SuperWeapon` | 超级武器类型 |
| 8 | `Armor` | 护甲名称 |
| 9 | `Overlay` | 覆盖物 |
| 10 | `Sound` | 声音 |
| 11 | `Country` | 国家（HouseType） |
| 12 | `TeamType` | AI 小队类型 |
| 13 | `ScriptType` | 小队脚本 |
| 14 | `AiTrigger` | AI 触发器 |
| 15 | `Variable` | 变量（`vera.vars`；前缀 `mod.`、`map.` 指向另一方）；返回变量 ID |
| 16 | `Notify` | 通知（`vera.notify`）；返回小写折叠后名字的 FNV-1a 哈希 |
| 17 | `ModuleAction` | 模块动作和决定处理函数（本模块的 `vera.actions`；不接受前缀）；返回分派键 |
| 18 | `String` | 引擎已持有、且在所有机器上相同的字符串：`string` 属性返回的字符串、音乐、EVA 名称、地图中的触发器和标签 ID；不存在为 `NotFound`；不新建字符串。模拟中不查询 CSF 或任何翻译表：CSF 条目名作为文字参数原样传递，`lookup` 不判断它是否存在 |
| 19 | `Property` | 属性名；返回属性 ID |
| 20 | `EventKind` | 事件种类名 |
| 21 | `ActionKind` | 动作种类名 |
| 22 | `Hotkey` | 本模块注册的热键（客户端） |
| 23 | `VeraEffect` | 保留 |

### 8.2 搜索范围（`find`）

| # | 范围 | 结果 | 顺序 | 客户端 |
| --- | --- | --- | --- | --- |
| 0 | `Objects` | 单位、建筑和抛射体的句柄 | 创建序号 | seen |
| 1 | `Houses` | 所属方句柄 | 所属方数组顺序 | all |
| 2 | `Cells` | 格子引用 | 格子下标（先 y 后 x） | 本玩家能看到的格子 |
| 3 | `Types` | `type` 为种类通配引用时为该名字种类的全部类型引用，为具体类型时只有它，为 0 时为非法参数（如带 `Buildable=1` 子句列出可建造类型，带 `Category` 子句筛选步兵、载具、飞行器或建筑） | 类型 ID | all |
| 4 | `Teams` | 小队句柄 | 创建序号 | sim |
| 5 | `Triggers` | 触发器句柄 | 创建序号 | sim |
| 6 | `Tags` | 标签句柄 | 创建序号 | sim |
| 7 | `Modifiers` | 修饰器句柄 | 加入顺序 | sim |
| 8 | `SuperWeapons` | 超级武器句柄 | 创建序号 | own |

### 8.3 枚举

未给出编号的取值按声明顺序，由接口定义确定。

| # | 枚举 | 取值 |
| --- | --- | --- |
| 0 | `Op`（筛选子句） | 0 `Eq`（`=`）、1 `Ne`（`!=`）、2 `Lt`（`<`）、3 `Le`（`<=`）、4 `Gt`（`>`）、5 `Ge`（`>=`） |
| 1 | `Compare`（AI 触发器） | 0 `Less`、1 `LessEqual`、2 `Equal`、3 `GreaterEqual`、4 `Greater`、5 `NotEqual`（同 AI 触发器比较符的顺序） |
| 2 | `ModifierOp` | 0 `Add`、1 `Multiply`、2 `Override` |
| 3 | `Scope`（变量） | 0 `Global`、1 `House`、2 `Object` |
| 4 | `VarTarget`（筛选子句的变量） | 0 `Global`、1 `Subject`、2 `Other`、3 `House`；查询中只允许 0 和 1 |
| 5 | `Visibility`（变量） | 0 `Private`、1 `Allies`、2 `InSight`（只用于对象作用域）、3 `Public` |
| 6 | `Source` | 0 `Mod`、1 `Map`、2 `PhobosEffect`（保留，版本 1 不产生）、3 `Behavior`（INI 行为）：修饰器的 `Origin` |
| 7 | `Table`（`has_kind`） | 0 事件种类、1 动作种类、2 属性、3 名字种类、4 搜索范围、5 节点种类、6 客户端事件种类、7 客户端动作种类、8 其他枚举（`id` 为 8.3 中的枚举编号乘 256 加取值） |
| 8 | `Mission` | 0 `Sleep`、1 `Attack`、2 `Move`、3 `QMove`、4 `Retreat`、5 `Guard`、6 `Sticky`、7 `Enter`、8 `Capture`、9 `Eaten`、10 `Harvest`、11 `AreaGuard`、12 `Return`、13 `Stop`、14 `Ambush`、15 `Hunt`、16 `Unload`、17 `Sabotage`、18 `Construction`、19 `Selling`、20 `Repair`、21 `Rescue`、22 `Missile`、23 `Harmless`、24 `Open`、25 `Patrol`、26 `ParadropApproach`、27 `ParadropOverfly`、28 `Deliberate`（INI 名为 `Wait`）、29 `AttackMove`、30 `SpyplaneApproach`、31 `SpyplaneOverfly`；`None` = 255；编号同地图 |
| 9 | `Relation` | 0 `Self`、1 `Ally`、2 `Enemy`、3 `Neutral`（平民或被动所属方） |
| 10 | `Category` | 0 `Infantry`、1 `Vehicle`、2 `Aircraft`、3 `Building`、4 `Projectile` |
| 11 | `Layer` | `Underground`、`Surface`、`Ground`、`Air`、`Top`（水面是地形类型，不是层） |
| 12 | `LandType` | `Clear`、`Road`、`Water`、`Rock`、`Wall`、`Tiberium`、`Beach`、`Rough`、`Ice`、`Railroad`、`Tunnel`、`Weeds` |
| 13 | `SpeedType` | INI 键 `SpeedType` 的取值；每个对应一个 `Passable.<SpeedType>` 格子属性 |
| 14 | `Difficulty` | `Easy`、`Normal`、`Hard`（全局引用的 `Difficulty` 属性；触发器难度用 `TriggerDef.difficulty` 掩码） |
| 15 | `TagRepeat` | 0 `OneTimeOr`、1 `OneTimeAnd`、2 `Repeating`（原版标签的重复类型） |
| 16 | `Rank` | 0 `Rookie`、1 `Veteran`、2 `Elite` |
| 17 | `Outcome` | 0 `Win`、1 `Lose` |
| 18 | `Blocked` | 0 `Nearby`、1 `Fail` |
| 19 | `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`；Ares 种类 |
| 20 | `Command` | 见下面的指令表 |
| 21 | `Hold` | `TopLeft`、`Top`、`TopRight`、`Left`、`Middle`、`Right`、`BottomLeft`、`Bottom`、`BottomRight`（`veraui.ini` 的 `Hold` 取值） |
| 22 | `PathKind` | 0 `Bezier`、1 `Parabola`、2 `Spiral`、3 `Waypoints` |

**指令**（`Command`，`Order` 的字段，用于 `order` 和 `client_send_order`）；`—` 的字段为 0，不符合本表的指令为 `Refused`；层级见第 9 节：

| # | 指令 | `target` | `value` | 单位列表 |
| --- | --- | --- | --- | --- |
| 0 | `Move` | 格子 | — | 单位 |
| 1 | `MoveToObject` | 对象 | — | 单位 |
| 2 | `Stop` | — | — | 单位 |
| 3 | `Attack` | 对象 | — | 单位 |
| 4 | `AttackMove` | 格子 | — | 单位 |
| 5 | `Guard` | 对象或 0（原地） | — | 单位 |
| 6 | `DeployMcv` | — | — | 基地车 |
| 7 | `UndeployBuilding` | — | — | 建筑 |
| 8 | `SetRally` | 格子 | — | 工厂建筑 |
| 9 | `QueueProduction` | — | 类型引用 | 无 |
| 10 | `SuspendProduction` | — | 类型引用 | 无 |
| 11 | `CancelProductionByType` | — | 类型引用 | 无 |
| 12 | `PlaceReadyBuilding` | 格子 | 建筑类型引用 | 无 |
| 13 | `SellBuilding` | — | — | 建筑 |
| 14 | `ToggleRepair` | — | — | 建筑 |
| 15 | `EjectBunker` | — | — | 建筑 |
| 16 | `UnloadPassengers` | — | — | 载具或建筑 |
| 17 | `CaptureBuilding` | 对象（建筑） | — | 工程师 |
| 18 | `PlantC4` | 对象（建筑） | — | 单位 |
| 19 | `LaunchSuperWeapon` | 格子 | 超级武器类型引用 | 无 |
| 20 | `ToggleInfantryDeploy` | — | — | 步兵 |
| 21 | `ForceAttackCell` | 格子 | — | 单位 |
| 22 | `SellWallAtCell` | 格子 | — | 无 |
| 23 | `Scatter` | — | — | 单位 |
| 24 | `TogglePrimaryFactory` | — | — | 工厂建筑 |
| 25 | `Enter` | 对象 | — | 单位 |

`Scatter` 让单位向旁边让开一小段，和散开键一样。`TogglePrimaryFactory` 切换工厂的主工厂标记，和点击一座非主工厂一样。`Enter` 登上运输载具，或进入碉堡、维修厂、矿石精炼厂、回收厂（Grinder），和点击目标一样。

## 9. 版本 1 清单

有层级列的表以该列为准；本节列出没有层级列的部分。每一项冻结前须通过 [RFC 流程](0001-wasm-extension-modules.zh-CN.md#流程)第 5 步。

| 表 | `v1` | `later` 或随阶段 2 |
| --- | --- | --- |
| 模拟导入 | 除 `send_notify` 外的全部 22 个 | `send_notify` 随阶段 2 |
| 模拟导出 | 除 `on_message` 外的全部 | `on_message` 随阶段 2 |
| 原版和 Ares 动作 | 1、2、4、5、7、8、10..12、14、16..18、20..35、41..48、51、53..73、76..80、82..90、92..97、100..110、112、114..118、125、127..129、131..145、148、149 | 其余 |
| TechnoType 属性 | `Category`、`Strength`、`Cost`、`Soylent`、`Speed`、`ROT`、`Crushable`、`Selfhealing`、`Explodes`、`Power`、`Sight`、`Ammo`、`InitialAmmo`、`Storage`、`Passengers`、`SizeLimit`、`TechLevel` | 其余 |
| Weapon 属性 | `Damage`、`ROF`、`Burst`、`MinimumRange`、`Range` | 其余 |
| Warhead 属性 | `CellSpread`、`PercentAtMax`、`Wall`、`Wood`、`Sparky`、`Fire`、`Conventional`、`Rocker`、`InfDeath` | 其余 |
| 名字种类 | `TechnoType`、`Weapon`、`Warhead`、`Projectile`、`Anim`、`VoxelAnim`、`ParticleSystem`、`SuperWeapon`、`Country`、`TeamType`、`Variable`、`ModuleAction`、`Property`、`ActionKind`；阶段 2：`Notify`、`Hotkey` | 其余；`Armor` 随 `TargetArmor` 预览 |
| 搜索范围 | `Objects`（不含抛射体）、`Houses`（不按区域、类型、所属方筛选）、`Cells`、`Modifiers` | 其余 |
| 指令 | 8.3 中除右列外的全部 | `SuspendProduction`、`EjectBunker`、`Enter` |
| 客户端节点属性 | 7.10 中除右列外的全部 | 只属于 `Decoration` 的属性、`ItemProgress`、写 `CameoGrid` 的 `Items` |
