# 教程：客户端模块（`ui.wasm`）

[English](rust-ui-tutorial.md) | [简体中文](rust-ui-tutorial.zh-CN.md)

这份教程在对局画面上做一个按钮：“Reinforcements: Rhino tank, $2000”（增援：犀牛坦克，2000 块）。
点一下就买一辆犀牛坦克，每个玩家只能买一次；买完按钮消失，EVA 播报增援已到达。它要两个模块配合完成：
客户端模块（`ui.wasm`）负责按钮，模拟模块（`rules.wasm`）负责坦克。
完整代码在 VERA20K 源码的 `sdk/examples/ui-reinforcements` 里。

- 写给谁：做过[模拟模块教程](rust-tutorial.zh-CN.md)，或者 Rust 熟到能看懂它。
- 参考资料：[Rust SDK 参考](rust-sdk.zh-CN.md)、[模块指南](guide.zh-CN.md)；
  用界面布局摆放按钮见[界面布局](../interface-layout.md)。
- 预览版提醒：名字和数值在第 1 版之前可能还会变。

## 1. 为什么要两个模块

客户端模块只在你自己的机器上运行。它能看到你的屏幕可以显示的内容，也能改界面，但不能改游戏：
要是能改，联机对局里各台机器就会对不上。所以按钮只负责发一条消息。消息跟着玩家的命令一起传输，
每台机器上的模拟模块都会在同一帧收到同一条消息，检查之后再执行。

```text
ui.wasm（你的机器）               rules.wasm（每台机器）
  点击 ──── send_message([1]) ───▶ on_message(sender, [1])
                                      检查钱，造坦克，
                                      设置 ReinforcementsBought = 1
  播放 EVA ◀── 通知 "Reinforced" ── REINFORCED.send(sender)
  隐藏按钮 ◀── 读取 ReinforcementsBought
```

两个模块之间有三种连接方式：

| 方式 | 方向 | 代码里怎么写 |
| --- | --- | --- |
| 消息 | 客户端 → 模拟 | `Client::send_message(&bytes)` → `on_message!` |
| 通知 | 模拟 → 某一个玩家的客户端 | `notify_names!`、`Notify::send` → `Notify` 事件 |
| 声明的变量 | 模拟 → 客户端，会存进存档 | `vars!` → `ClientVar` |

## 2. 建项目

你需要装好带 `wasm32-unknown-unknown` 目标的 Rust，并拿到 VERA20K 源码，做法和
[模拟模块教程](rust-tutorial.zh-CN.md#2-准备环境)的 2.1 节、2.2 节一样。复制示例：

```sh
cp -R sdk/examples/ui-reinforcements sdk/examples/my-reinforcements
```

它是一个 Cargo 工作区，每个模块一个 crate：

```text
my-reinforcements/
├── .cargo/config.toml   Wasm 编译目标和允许的特性，必须保留
├── Cargo.toml           工作区：成员 rules 和 ui，release 配置
├── rules/
│   ├── Cargo.toml       包名 ui-reinforcements-rules
│   └── src/lib.rs       模拟模块
└── ui/
    ├── Cargo.toml       包名 ui-reinforcements-ui
    └── src/lib.rs       客户端模块
```

想改名的话，在 `rules/Cargo.toml` 和 `ui/Cargo.toml` 里改这两个包名（`.wasm` 文件按包名命名）。
两个包都用 `vera20k-sdk = { path = "../../../rust/vera20k-sdk" }` 依赖 SDK；项目放在别处时改一下这个路径。

## 3. 模拟模块

`rules/src/lib.rs`：

```rust
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::vars! {
    // 私有：只有这个所属方自己的玩家能读。
    pub static BOUGHT: HouseVar = ("ReinforcementsBought", Private, 0);
}

vera20k_sdk::notify_names! {
    pub static REINFORCED: Notify = "Reinforced";
}

/// 消息类型 1：购买增援。整条消息就是这一个字节。
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: u32 = 3;

/// 来自同一来源客户端模块的一条消息；`sender` 是引擎写在命令上的所属方，
/// 客户端没法伪造。其他内容都不可信：全部要检查。
fn buy_message(sender: House, data: &[u8]) {
    if data.len() != 1 || data[0] != BUY {
        return;
    }
    // 每个所属方一次，而且钱要够。
    if BOUGHT.get(sender) != Ok(0) || !sender.money().is_ok_and(|m| m >= i64::from(PRICE)) {
        return;
    }
    let Ok(htnk) = TypeRef::techno_type("HTNK") else {
        return;
    };
    // 地格被占用时会被拒绝：只为真正造出来的坦克扣钱。
    if Action::create_object(Waypoint::number(DROP_WAYPOINT), htnk, sender).is_err() {
        return;
    }
    let _ = Action::add_money(sender, -PRICE);
    // 客户端模块下一次读取时就能看到。
    let _ = BOUGHT.set(sender, 1);
    let _ = REINFORCED.send(sender, &[]);
    log("ui-reinforcements: reinforcements delivered");
}

vera20k_sdk::on_message!(buy_message);
```

逐段说明：

1. **`vars!`** 声明一个所属方变量，每个所属方一个值。`Private` 表示客户端模块只能读自己玩家所属方的值。
   变量会存进存档，所以读档后客户端模块马上就能读到正确的值。
2. **`notify_names!`** 声明模块会发出的所有通知名。名字没声明过的通知，游戏会拒绝。
3. **`on_message!`** 导出处理消息的函数。`sender` 由游戏提供；字节内容来自玩家的机器，可能是任何东西，
   所以在信任它之前先检查长度和数值。处理函数在模块阶段运行，每台机器上都会跑。
4. **检查**：每个所属方一次（`BOUGHT` 还是 0），钱要够。先造坦克，造成功了再扣钱：
   地格被占用时 `create_object` 返回 `Err(Refused)`，地图上没有 3 号路径点时返回 `Err(NotFound)`。
5. **`REINFORCED.send(sender, &[])`** 通知这个所属方的玩家。这个玩家机器上的每个客户端模块都会收到一个
   `Notify` 事件。通知只用于表现：不会存进存档，重新加入的玩家也收不到以前的通知。

## 4. 客户端模块

`ui/src/lib.rs`：

```rust
#![no_std]

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static BUY: NodeSlot = NodeSlot::new();
/// 模拟模块的变量，按它的可见性读取（私有：
/// 只能读这个玩家自己的所属方）。
static BOUGHT: ClientVar = ClientVar::new("ReinforcementsBought");
static REINFORCED: NotifyName = NotifyName::new("Reinforced");

/// 消息类型 1：购买。
const BUY_MESSAGE: [u8; 1] = [1];

fn start() {
    let button = Ui::find("BuyReinforcements").or_else(|_| {
        Ui::create_with(
            NodeKind::Button,
            Node(NO_HANDLE),
            &[
                (Prop::X, 16),
                (Prop::Y, 120),
                (Prop::Width, 180),
                (Prop::Height, 24),
            ],
        )
    });
    let Ok(button) = button else {
        client::log("ui-reinforcements: no button");
        return;
    };
    let _ = button.set_label("Reinforcements: Rhino tank, $2000");
    let _ = button.set_clickable(true);
    BUY.set(button);
}

fn on_click(event: &ClientEventRecord) {
    if event.node() == BUY.get() && Client::send_message(&BUY_MESSAGE).is_err() {
        // 观战者，或者超过了消息频率。
        client::log("ui-reinforcements: the message was refused");
    }
}

fn on_notify(event: &ClientEventRecord) {
    if REINFORCED.is(event) {
        let _ = Local::play_speech("EVA_ReinforcementsHaveArrived");
    }
}

fn render(_dt: f32, _alpha: f32) {
    let button = BUY.get();
    if button.is_none() {
        return;
    }
    // 观战者没有所属方，作用域 0 不属于任何所属方变量。
    let me = Client::viewer();
    let show = !me.is_none() && BOUGHT.get_house(me) == Ok(0);
    let _ = button.set_visible(show);
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_render!(render);
vera20k_sdk::on_client_event!(
    ClientEvent::Click => on_click,
    ClientEvent::Notify => on_notify,
);
```

逐段说明：

1. **只用 `client::` 的函数。** 客户端模块用的是 `vera20k_sdk::client` 和它的 prelude。
   crate 根上的函数（`log`、`get`、`Action` 等）是给模拟用的，导入了它们的 `ui.wasm` 会被游戏拒绝。
   所以这个文件里写的是 `client::log`。
2. **`on_start!`** 在对局界面建好后运行一次，读档或重新加入后会再运行一次。`Ui::find("BuyReinforcements")`
   在界面布局里找这个名字的部件（见[第 6 节](#6-用界面布局摆放按钮)）；找不到时，模块就在顶层自己创建一个按钮，
   距左边 16 像素、距顶部 120 像素。`NodeSlot` 把这个节点保存下来，给其他回调用。
3. **`set_clickable(true)`** 让按钮发出 `Click` 事件。节点只会发出它要求的事件（`Clickable`、`Hoverable`、`Dismissable`）。
4. **`on_client_event!`** 为每种事件列一个处理函数。`on_click` 比较被点击的节点是不是这个按钮，
   是就发出那条一字节的消息。观战者发消息，或者玩家发得太多（每帧 2 条，15 帧内 16 条）时，
   `send_message` 返回 `Err(Refused)`。
5. **`on_notify`** 确认通知是 `Reinforced`，然后只在这台机器上播放 EVA 语音。
6. **`on_render!`** 每个渲染帧都运行。它读取这个玩家的 `ReinforcementsBought`，只有值为 0 时才显示按钮。
   所以模拟一把坦克送到，按钮就消失，读档后也保持隐藏。对观战者来说，`Client::viewer()` 是 `House(NO_HANDLE)`，
   他们看不到按钮。

每帧读一次变量开销很小。客户端模块每个渲染帧有 100,000 的运算量，大多数界面只用掉一小部分。

## 5. 编译和试玩

```sh
cd sdk/examples/my-reinforcements
cargo build --release
```

工作区会把两个模块都编译到 `target/wasm32-unknown-unknown/release/` 里：
`ui_reinforcements_rules.wasm` 和 `ui_reinforcements_ui.wasm`（改过包名的话就是你的包名，`-` 变成 `_`）。
检查模拟模块：

```sh
cp target/wasm32-unknown-unknown/release/ui_reinforcements_rules.wasm rules.wasm
cp target/wasm32-unknown-unknown/release/ui_reinforcements_ui.wasm ui.wasm
asset wasm-check rules.wasm
```

`asset wasm-check` 只检查模拟模块。`ui.wasm` 由游戏在对局加载它时检查，拒绝时会在聊天栏告诉你；
对局随后会在没有它的情况下继续进行。

建一个包含两个模块的 mod 文件夹（`Mods` 文件夹在 VERA20K 文件夹里，见[模块指南](guide.zh-CN.md)）：

```text
Mods/MyReinforcements/
├── mod.ini
├── rules.wasm
└── ui.wasm
```

```ini
[Mod]
Name=My Reinforcements
Version=1
About=A button that buys a Rhino tank.
```

用这个 mod 启动游戏（**Options → Mods**，或者 `vera20k --mod MyReinforcements`），
选一张至少有四个出生点的地图开遭遇战，而且不要有人从 4 号位出生：坦克会出现在 3 号路径点。
钱有 2000 块或以上时，点按钮。坦克出现，钱少了 2000，EVA 播报，按钮消失。

如果什么都没发生：

- 按钮一直不出现：在聊天栏里找 `ui-reinforcements: no button`，或者 `ui.wasm` 被拒绝的提示。
- 按钮一直不消失：模拟拒绝了。可能是没有 3 号路径点、上面站着单位，或者钱不够。
  把 `DROP_WAYPOINT` 改成地图上有的路径点。
- 出现 `the message was refused`：你是观战者，或者点得比消息限制还快。

## 6. 用界面布局摆放按钮

mod 可以用自己的界面布局来摆放按钮，而不是让模块去创建。在 mod 的 `veraui.ini`（见[界面布局](../interface-layout.md)）里，
给战场的部件加一个不带 `Command` 的按钮：

```ini
[Mine.Field]
Parts=Messages,Timers,Banner,Powers,Tooltip,BuyReinforcements

[Mine.BuyReinforcements]
Type=Button
Label=Reinforcements
Hold=BottomLeft
X=10
Y=60
```

这样 `Ui::find("BuyReinforcements")` 就能找到这个部件（名字是去掉界面布局前缀后的部分），
模块只需要改它的文字和可见性。不带 `Command` 的按钮本身什么都不做：点击只会通知模块。
地图也可以用同样的方法，在 `[VeraUI]` 里给自己的客户端模块加部件（见[地图自己的部件](../interface-layout.md#a-maps-own-parts)）。

## 7. 客户端模块还能做什么

下面每段代码都加到上面的客户端模块里。

**热键。** 在 `on_start` 里注册（不要在 `client_init` 里注册，那里只能做查询），然后比较事件的 `id`：

```rust
use core::sync::atomic::{AtomicI32, Ordering::Relaxed};

static BUY_KEY: AtomicI32 = AtomicI32::new(-1);

fn register_key() {
    // 游戏热键已经占用这个组合键时会被拒绝，这个热键就没有按键。
    let _ = Local::register_hotkey("BuyReinforcements", "Ctrl+Alt+R");
    if let Ok(id) = Hotkey::id("BuyReinforcements") {
        BUY_KEY.store(id, Relaxed);
    }
}

fn on_hotkey(event: &ClientEventRecord) {
    if event.id == BUY_KEY.load(Relaxed) {
        let _ = Client::send_message(&BUY_MESSAGE);
    }
}
```

在 `start()` 里调用 `register_key()`，再把 `ClientEvent::Hotkey => on_hotkey` 加进 `on_client_event!`。

**进度条和列表。** 只创建一次（存进 `NodeSlot`），数值变化时再写入：

```rust
fn make_panel() -> vera20k_sdk::Result<()> {
    let panel = Ui::create_with(NodeKind::Column, Node(NO_HANDLE), &[(Prop::X, 16), (Prop::Y, 160)])?;
    let bar = Ui::create_with(NodeKind::Bar, panel, &[(Prop::Width, 120), (Prop::Height, 8)])?;
    let list = Ui::create(NodeKind::List, panel)?;
    bar.set_fill(40)?;                         // 0..100
    list.set_items(&["Rhino ready", "Wave 3"])
}
```

`Prop::Hold` 让顶层节点以某个角或边为基准摆放：0 左上，2 右上，4 正中，6 左下，8 右下（1、3、5、7 是它们之间的边）。

**地图上的点击。** `Cursor::capture_map_clicks(true)` 让地图点击只发给模块，以 `MapClick` 事件的形式发出，
用 `event.cell()` 取地格；用完记得改回 `false`，不然玩家就没法下命令了。

**玩家自己的事件。** `ClientEvent::SimEvent` 会送来这个玩家自己的对象的 `TakeDamage`、`Killed`、`Birth`、
`AttackedBy`、`Commanded` 和 `OrderRefused` 事件，用 `event.sim_event()` 取出记录。
客户端模块不用问模拟模块就能显示它们。

**在这台机器上记住点东西。** `Storage` 在对局之间保存少量数据，按 mod（或按地图）分开：

```rust
let mut buf = [0u8; 1];
let hint_seen = Storage::read("hint-seen", &mut buf).is_ok();
if !hint_seen {
    let _ = Local::show_message("Click the button to buy a Rhino tank.", 0);
    let _ = Storage::write("hint-seen", &[1]);
}
```

存储是本地的：其他玩家有他们自己的存储，所以它绝不能影响模拟的行为。

## 8. 要记住的规则

- 客户端模块可以用浮点数和 `client::random` 制作本地效果。商店把购买请求发给模拟模块，由模拟模块核验。
- 模拟模块必须把每条消息都当成不可信的：检查长度、每个数值，以及发送者现在是否可以这样做。
- 客户端模块不存档。读档后它们会重新开始（`on_start`）；界面必须显示的内容，要放在模拟模块声明的变量里。
- 地图的客户端模块在 mod 默认的 `[VeraWasm] MapUI=overlay` 下，只能改它自己的节点；`MapUI=no` 则不加载它。
- 目前请把客户端模块放在 mod 里发布：`asset wasm-embed` 还不能写入地图的 `ui.wasm`。

限制、调试和其他事件见[模块指南](guide.zh-CN.md)和 [SDK 参考](rust-sdk.zh-CN.md)。
`sdk/examples/ui-ping` 是最小的一对模块；`sdk/examples/muster-field/ui` 是一个完整界面，带面板、卡片和图片。
