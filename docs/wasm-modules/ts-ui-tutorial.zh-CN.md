# 教程：用 TypeScript 写客户端模块（`ui.wasm`）

[English](ts-ui-tutorial.md) | [简体中文](ts-ui-tutorial.zh-CN.md)

这份教程在对局画面上做一个按钮：“Reinforcements: Rhino tank, $2000”（增援：犀牛坦克，2000 块）。点一下就买一辆犀牛坦克，每个玩家只能买一次；买完按钮消失，EVA 播报增援已到达。它要两个模块配合完成：客户端模块（`ui.wasm`）负责按钮，模拟模块（`rules.wasm`）负责坦克。完整代码在 VERA20K 源码的 `sdk/typescript/examples/ui-reinforcements` 里。

- 写给谁：做过[模拟模块教程](ts-tutorial.zh-CN.md)，并且按它的第 2 节装好了 Node.js 和 SDK。
- 参考资料：[TypeScript SDK 参考](ts-sdk.zh-CN.md#客户端模块)、[模块指南](guide.zh-CN.md)；用界面布局摆放按钮见[界面布局](../interface-layout.md)。
- 更习惯 Rust？[Rust 版客户端教程](rust-ui-tutorial.zh-CN.md)做的是同一个按钮。
- 预览版提醒：名字和数值在第 1 版之前可能还会变。

## 1. 为什么要两个模块

客户端模块只在你自己的机器上运行。它能看到你的屏幕可以显示的内容，也能改界面，但不能改游戏：要是能改，联机对局里各台机器就会对不上。所以按钮只负责发一条消息。消息跟着玩家的命令一起传输，每台机器上的模拟模块都会在同一帧收到同一条消息，检查之后再执行。

```text
ui.wasm（你的机器）                  rules.wasm（每台机器）
  点击 ──── Client.sendMessage([1]) ──▶ onMessage(sender, [1])
                                         检查钱，造坦克，
                                         设置 ReinforcementsBought = 1
  播放 EVA ◀── Notify "Reinforced" ──── Reinforced.send(sender)
  隐藏按钮 ◀── 读取 ReinforcementsBought
```

两个模块之间有三种联系方式：

| 方式 | 方向 | 代码里 |
| --- | --- | --- |
| 消息 | 客户端 → 模拟 | `Client.sendMessage(bytes)` → `onMessage` |
| 通知 | 模拟 → 某个玩家的客户端 | `vera.json` 里的 `notify`，`Reinforced.send(house)` → `Notify` 事件 |
| 声明的变量 | 模拟 → 客户端，会存进存档 | `vera.json` 里的 `vars` → `ClientVar` |

## 2. 建项目

每个模块一个项目。在 VERA20K 文件夹里：

```sh
node sdk/typescript/bin/vera20k.js init my-reinforcements/rules
node sdk/typescript/bin/vera20k.js init my-reinforcements/ui --ui
cd my-reinforcements/rules
npm install
cd ../ui
npm install
cd ..
```

```text
my-reinforcements/
├── rules/
│   ├── vera.json        "kind": "rules"，变量和通知
│   └── src/index.ts     模拟模块
└── ui/
    ├── vera.json        "kind": "ui"
    └── src/index.ts     客户端模块
```

（每个项目还有自己的 `package.json`、`tsconfig.json` 和 `.gitignore`。）`--ui` 建的是客户端项目：它的 `vera.json` 写着 `"kind": "ui"`，构建出 `build/ui.wasm`，代码从 `@vera20k/sdk/client` 而不是 `@vera20k/sdk` 导入。

## 3. 模拟模块

`rules/vera.json` 声明变量和通知：

```json
{
  "kind": "rules",
  "vars": [
    { "name": "ReinforcementsBought", "scope": "house", "visibility": "private", "initial": 0 }
  ],
  "notify": ["Reinforced"]
}
```

`rules/src/index.ts`：

```ts
import { Action, House, NO_HANDLE, Rules, Status, Waypoint, lastStatus, log, onMessage } from "@vera20k/sdk";
import { ReinforcementsBought } from "./generated/vars";
import { Reinforced } from "./generated/notify";

/** 消息类型 1：购买增援。整条消息就是这一个字节。 */
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: i32 = 3;

// 这张地图的客户端模块发来的一条消息；`sender` 是引擎放在命令上的所属方，
// 客户端伪造不了。其他内容都不可信：全部检查。
onMessage((sender: House, data: Uint8Array): void => {
  if (data.length != 1 || data[0] != BUY) return;
  // 每个所属方只能买一次，而且钱要够。
  const bought = ReinforcementsBought.get(sender);
  if (lastStatus() != Status.Ok || bought != 0) return;
  const money = House.money(sender);
  if (lastStatus() != Status.Ok || money < PRICE) return;
  const htnk = Rules.technoType("HTNK");
  if (htnk == NO_HANDLE) return;
  // 地格被占时会被拒绝：只为真的造出来的坦克收钱。
  if (Action.createObject(Waypoint.number(DROP_WAYPOINT), htnk, sender) == NO_HANDLE) return;
  Action.addMoney(sender, -PRICE);
  // 客户端模块下一次读取就能看到。
  ReinforcementsBought.set(sender, 1);
  Reinforced.send(sender);
  log("ui-reinforcements: reinforcements delivered");
});
```

逐步说明：

1. **`vera.json` 里的 `vars`** 声明一个所属方变量，每个所属方一个值。`private` 表示客户端模块只能读自己玩家那一方的值。变量会存进存档，所以读档后客户端模块马上就能读到正确的值。构建会生成 `src/generated/vars.ts`，里面的 `ReinforcementsBought` 是一个 `HouseVar`。
2. **`vera.json` 里的 `notify`** 声明模块会发的每个通知名；没声明的通知名会被游戏拒绝。构建会生成 `src/generated/notify.ts`，里面有 `Reinforced`。`npm run check` 也会写出这两个生成的文件（`vera20k generate`），所以第一次构建之前就能用。
3. **`onMessage`** 登记处理消息的函数。`sender` 由游戏给出；字节来自某个玩家的机器，可能是任何内容，所以先检查长度和数值再相信它。处理函数在模块阶段运行，每台机器都会运行。
4. **检查**：每个所属方只能买一次（`ReinforcementsBought` 还是 0），钱要够。读取失败时也会返回 0，所以代码还检查了 `lastStatus()`。先造坦克，成功了再扣钱：地格被占，或者地图上没有 3 号路径点时，`createObject` 返回 `NO_HANDLE`。
5. **`Reinforced.send(sender)`** 通知那一方的玩家。那个玩家机器上的每个客户端模块都会收到一个 `Notify` 事件。通知只用于显示：不会存档，重新加入的玩家也收不到以前的通知。

## 4. 客户端模块

`ui/src/index.ts`：

```ts
import {
  ClickEvent, Client, ClientEvent, ClientVar, Local, NO_HANDLE, Node, NodeKind, NodeProps, NotifyEvent,
  NotifyName, Status, Ui, lastStatus, onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";

/** 模拟模块的变量，按它的可见性读取（private：只能读这个玩家自己那一方的）。 */
const BOUGHT = new ClientVar("ReinforcementsBought");
const REINFORCED = new NotifyName("Reinforced");
/** 消息类型 1：购买。 */
const BUY_MESSAGE = new Uint8Array(1);
BUY_MESSAGE[0] = 1;

let buy: Node = NO_HANDLE;

onStart((): void => {
  let button = Ui.find("BuyReinforcements");
  if (button == NO_HANDLE) {
    button = Ui.createWith(NodeKind.Button, NO_HANDLE, new NodeProps().at(16, 120).size(180, 24));
  }
  if (button == NO_HANDLE) {
    Client.log("ui-reinforcements: no button");
    return;
  }
  Node.setLabel(button, "Reinforcements: Rhino tank, $2000");
  Node.setClickable(button, true);
  buy = button;
});

onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
  if (e.node == buy && Client.sendMessage(BUY_MESSAGE) != Status.Ok) {
    // 观战者，或者超过了消息频率限制。
    Client.log("ui-reinforcements: the message was refused");
  }
});

onClientEvent(ClientEvent.Notify, (e: NotifyEvent): void => {
  if (REINFORCED.is(e)) Local.playSpeech("EVA_ReinforcementsHaveArrived");
});

onRender((dt: f32, alpha: f32): void => {
  if (buy == NO_HANDLE) return;
  // 观战者没有所属方，作用域 0 也不是任何所属方变量的作用域。
  const me = Client.viewer();
  let show = false;
  if (me != NO_HANDLE) {
    const bought = BOUGHT.get(me);
    show = lastStatus() == Status.Ok && bought == 0;
  }
  Node.setVisible(buy, show);
});
```

逐步说明：

1. **只用 `@vera20k/sdk/client`。** 客户端模块从 `@vera20k/sdk/client` 导入。`@vera20k/sdk` 里的模拟函数（`log`、`get`、`Action`……）调用的是模拟端的导入，游戏会拒绝带有它们的 `ui.wasm`；`vera20k build` 会先拒绝，并说出是哪个调用。所以这个文件写的是 `Client.log`。客户端包会重新导出两边共用的东西：`Status`、`NO_HANDLE`、`Prop`、`Cell.at`、`Find`、`Order` 以及其他引用。
2. **顶层代码**在模块加载时运行一次（`client_init`）。那里只能做查询，所以它只创建 `ClientVar`、`NotifyName` 和消息字节，这些此时都还不调用游戏。顶层的 `let`（`buy`）在各个回调之间保存节点。
3. **`onStart`** 在对局界面建好后运行一次，读档或重新加入后会再运行一次。`Ui.find("BuyReinforcements")` 在界面布局里找这个名字的部件（[第 6 节](#6-用界面布局摆放按钮)）；找不到时，模块在顶层（父节点写 `NO_HANDLE`）创建一个按钮，离左边 16 像素、离上边 120 像素。`NodeProps` 在这一次创建调用里最多设置 16 个属性。
4. **`Node.setClickable(button, true)`** 让按钮发出 `Click` 事件。节点只发它要求的事件（`setClickable`、`setHoverable`、`setDismissable`）。
5. **`onClientEvent`** 为每种事件登记一个处理函数，处理函数接收这种事件的视图（`ClickEvent`、`NotifyEvent`……）。点击处理函数比较被点的节点是不是这个按钮，然后发出一个字节的消息。观战者发消息，或者玩家发得太多（每帧 2 条，15 帧内 16 条）时，`sendMessage` 返回 `Refused`。
6. **通知处理函数**检查通知是不是 `Reinforced`，然后只在这台机器上播放 EVA 语音。
7. **`onRender`** 每个画面帧都运行。它读取这个玩家的 `ReinforcementsBought`，只在值为 0 时显示按钮，所以模拟模块一交付坦克，按钮就消失，读档后也保持隐藏。观战者的 `Client.viewer()` 是 `NO_HANDLE`，看不到按钮。

每帧读一次变量花不了多少。客户端模块每个画面帧有 100,000 次运算的预算，大多数界面只用到一小部分。

`onStart`、`onRender` 和 `onClientEvent` 都可以调用多次（`onClientEvent` 可以为任意多种事件调用）；登记的每个函数都会运行，按登记顺序。

**从 `vera.json` 生成变量和通知。** 不想手写 `new ClientVar(...)` 和 `new NotifyName(...)` 的话，客户端项目也可以在自己的 `vera.json` 里列出这些名字，构建会像模拟端一样生成常量：

```json
{
  "kind": "ui",
  "entry": "src/index.ts",
  "vars": [{ "name": "ReinforcementsBought", "scope": "house" }],
  "notify": ["Reinforced"]
}
```

```ts
import {
  Client, ClientEvent, Local, NO_HANDLE, Node, NotifyEvent, Ui,
  onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";
import { ReinforcementsBought } from "./generated/vars";
import { Reinforced } from "./generated/notify";

let button: Node = NO_HANDLE;

onStart((): void => {
  button = Ui.find("BuyReinforcements");
});

onClientEvent(ClientEvent.Notify, (e: NotifyEvent): void => {
  if (Reinforced.is(e)) Local.playSpeech("EVA_ReinforcementsHaveArrived");
});

onRender((dt: f32, alpha: f32): void => {
  const me = Client.viewer();
  if (me != NO_HANDLE && button != NO_HANDLE) Node.setVisible(button, ReinforcementsBought.get(me) == 0);
});
```

在客户端项目里，这些条目并不声明任何东西：变量属于模拟模块。每个条目都生成 `ClientVar`，所以只需要 `name`；`scope` 等其他键会被忽略。另一个来源的模块的名字写成 `mod.Name` 或 `map.Name`。

## 5. 编译和试玩

```sh
cd rules
npm run build        # build/rules.wasm
npm run check
cd ../ui
npm run build        # build/ui.wasm
npm run check
cd ..
```

有 `asset` 工具的话，可以检查模拟模块：

```sh
asset wasm-check rules/build/rules.wasm
```

`asset wasm-check` 只检查模拟模块。`ui.wasm` 由游戏在对局加载它时检查，拒绝时会在聊天栏里告诉你，对局则在没有它的情况下继续。

建一个放两个模块的 mod 文件夹（`Mods` 文件夹在 VERA20K 文件夹里，见[模拟模块教程第 4 节](ts-tutorial.zh-CN.md#4-放进游戏试玩)）：

```text
Mods/MyReinforcements/
├── mod.ini
├── rules.wasm       来自 rules/build/
└── ui.wasm          来自 ui/build/
```

```ini
[Mod]
Name=My Reinforcements
Version=1
About=A button that buys a Rhino tank.
```

想直接编译进 mod，在 `rules/` 里运行 `npm run build -- --out "<VERA20K 文件夹>/Mods/MyReinforcements/rules.wasm"`，在 `ui/` 里对 `ui.wasm` 做同样的事。

用这个 mod 启动游戏（**Options → Mods**，或者 `vera20k --mod MyReinforcements`），在一张至少有四个出生点的地图上开遭遇战，并且 4 号出生点不要有人：坦克会出现在 3 号路径点。钱有 2000 或更多时点按钮：坦克出现，钱少 2000，EVA 播报，按钮消失。

如果没反应：

- 按钮一直没出现：在聊天栏里找 `ui-reinforcements: no button`，或者 `ui.wasm` 被拒绝的提示。
- 按钮一直不消失：模拟模块拒绝了。没有 3 号路径点、路径点上站着单位，或者钱不够。把 `DROP_WAYPOINT` 改成地图上有的路径点。
- `the message was refused`：你是观战者，或者点得比消息限制还快。

## 6. 用界面布局摆放按钮

mod 可以用自己的界面布局摆放这个按钮，不让模块去创建。在 mod 的 `veraui.ini`（见[界面布局](../interface-layout.md)）里，给战场的部件加一个没有 `Command` 的按钮：

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

现在 `Ui.find("BuyReinforcements")` 会找到这个部件（名字是布局前缀后面的部分），模块只改它的文字和是否显示。没有 `Command` 的按钮自己什么也不做：点击只会通知模块。地图也可以用同样的方法，在 `[VeraUI]` 里给自己的客户端模块加部件（[地图自己的部件](../interface-layout.md#a-maps-own-parts)）。

## 7. 客户端模块还能做什么

下面每段代码都是一个能单独编译的完整客户端模块。要把它加到按钮模块里，把它复制进按钮模块的文件，已有的导入和声明（比如 `BUY_MESSAGE`）不用重复：它的 `onStart` 会在原来那个之后运行。

**热键。** 在 `onStart` 里注册（不要在顶层代码里注册，那里只能做查询），然后比较事件的 `id`：

```ts
import {
  Client, ClientEvent, Hotkey, HotkeyEvent, Local, NO_ID, Status, onClientEvent, onStart,
} from "@vera20k/sdk/client";

const BUY_MESSAGE = new Uint8Array(1);
BUY_MESSAGE[0] = 1;
let buyKey: i32 = NO_ID;

onStart((): void => {
  // 游戏热键已经占用这个组合键时会被拒绝：这个热键就没有按键
  Local.registerHotkey("BuyReinforcements", "Ctrl+Alt+R");
  buyKey = Hotkey.id("BuyReinforcements");
});

onClientEvent(ClientEvent.Hotkey, (e: HotkeyEvent): void => {
  if (e.id == buyKey && Client.sendMessage(BUY_MESSAGE) != Status.Ok) {
    Client.log("the message was refused");
  }
});
```

**进度条和列表。** 只创建一次（存进顶层变量），数值变化时再写入：

```ts
import { NO_HANDLE, Node, NodeKind, NodeProps, Prop, Ui, onStart } from "@vera20k/sdk/client";

let bar: Node = NO_HANDLE;
let list: Node = NO_HANDLE;

onStart((): void => {
  // 左下角（Hold 6）的一列，向右 16 像素，向上 160 像素
  const panel = Ui.createWith(NodeKind.Column, NO_HANDLE, new NodeProps().with(Prop.Hold, 6).at(16, 160));
  if (panel == NO_HANDLE) return;
  bar = Ui.createWith(NodeKind.Bar, panel, new NodeProps().size(120, 8));
  list = Ui.create(NodeKind.List, panel);
  Node.setFill(bar, 40);                            // 0..100
  Node.setItems(list, ["Rhino ready", "Wave 3"]);
});
```

`Prop.Hold` 让顶层节点以某个角或边为基准摆放：0 左上，2 右上，4 正中，6 左下，8 右下（1、3、5、7 是它们之间的边）。

**地图上的点击。** `Cursor.captureMapClicks(true)` 让地图点击只发给模块，以 `MapClick` 事件的形式发出；用完记得改回 `false`，不然玩家就没法下命令了：

```ts
import { Cell, Client, ClientEvent, Cursor, MapClickEvent, onClientEvent, onStart } from "@vera20k/sdk/client";

onStart((): void => {
  Cursor.captureMapClicks(true);    // 现在地图点击只发给这个模块
});

onClientEvent(ClientEvent.MapClick, (e: MapClickEvent): void => {
  const where = Cell.x(e.cell).toString() + "," + Cell.y(e.cell).toString();
  Client.log("clicked " + where + (e.right ? " with the right button" : ""));
  Cursor.captureMapClicks(false);   // 把地图还给玩家
});
```

**玩家自己的事件。** `ClientEvent.SimEvent` 会送来这个玩家自己的对象的 `TakeDamage`、`Killed`、`Birth`、`AttackedBy`、`Commanded` 和 `OrderRefused` 事件，用 `e.simEvent` 取出记录（没有时是 `null`）。客户端模块不用问模拟模块就能显示它们：

```ts
import { Client, ClientEvent, Event, ForwardedEvent, onClientEvent } from "@vera20k/sdk/client";

onClientEvent(ClientEvent.SimEvent, (e: ForwardedEvent): void => {
  const record = e.simEvent;
  if (record == null) return;
  if (record.kind == Event.Killed) Client.log("one of your units was killed");
});
```

**选中的单位。** `SelectionChanged` 表示本地的选择变了；用搜索 `Selected = 1` 读出选中的单位，`Client.findPage` 按这个玩家的视野执行搜索：

```ts
import {
  Client, ClientEvent, Find, Op, Prop, SelectionChangedEvent, onClientEvent,
} from "@vera20k/sdk/client";

const SELECTED = new StaticArray<i64>(64);

onClientEvent(ClientEvent.SelectionChanged, (e: SelectionChangedEvent): void => {
  const page = Client.findPage(Find.objects().where(Prop.Selected, Op.Eq, 1), SELECTED, 0);
  Client.log(page.count.toString() + " selected");
});
```

**在这台机器上记住点东西。** `Storage` 在对局之间保存少量数据，按 mod（或按地图）分开：

```ts
import { Local, Storage, onStart } from "@vera20k/sdk/client";

onStart((): void => {
  if (!Storage.has("hint-seen")) {
    Local.showMessage("Click the button to buy a Rhino tank.", 0);
    Storage.writeString("hint-seen", "1");
  }
});
```

存储是本地的：其他玩家有他们自己的存储，所以它绝不能影响模拟的行为。

## 8. 要记住的规则

- 客户端模块可以用浮点数、`Client.random` 以及编译器允许的其他任何东西：它只能通过消息改游戏，而消息由模拟模块检查。[模拟模块教程](ts-tutorial.zh-CN.md#6-这里的-typescript-和平常有什么不同)里的 TypeScript 规则这里同样适用。
- 模拟模块必须把每条消息都当成不可信的：检查长度、每个数值，以及发送者现在是否可以这样做。
- 客户端模块不存档。读档后它们会重新开始（先顶层代码，再 `onStart`）；界面必须显示的内容，要放在模拟模块声明的变量里。
- 地图的客户端模块在 mod 默认的 `[VeraWasm] MapUI=overlay` 下，只能改它自己的节点；`MapUI=no` 则不加载它。
- 目前请把客户端模块放在 mod 里发布：`asset wasm-embed` 还不能写入地图的 `ui.wasm`。

限制、调试和其他事件见[模块指南](guide.zh-CN.md#限制)和 [SDK 参考](ts-sdk.zh-CN.md#客户端模块)。`sdk/typescript/examples/ui-ping` 是最小的一对模块；`sdk/typescript/examples/muster-field/ui` 是一个完整界面，带面板、卡片和图片。
