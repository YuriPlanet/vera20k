# Tutorial: a client module in TypeScript (`ui.wasm`)

[English](ts-ui-tutorial.md) | [简体中文](ts-ui-tutorial.zh-CN.md)

This tutorial builds a button on the match screen: "Reinforcements: Rhino tank, $2000".
A click buys one Rhino tank, once per player; the button then disappears and EVA says
the reinforcements have arrived. It takes two modules that work together, a client
module (`ui.wasm`) for the button and a simulation module (`rules.wasm`) for the tank.
The complete code is `sdk/typescript/examples/ui-reinforcements` in the VERA20K source.

- Who it is for: you have done the [simulation module tutorial](ts-tutorial.md)
  and have Node.js and the SDK set up as in its section 2.
- Reference: [TypeScript SDK reference](ts-sdk.md#client-modules),
  [guide](guide.md), [interface layout](../interface-layout.md) for buttons placed
  by a layout.
- Prefer Rust? The [Rust client tutorial](rust-ui-tutorial.md) builds the same
  button.
- Preview: names and numbers may still change before version 1.

## 1. Why two modules

A client module runs only on your machine. It sees what your screen may show and can
change the interface, but it cannot change the game: if it could, the machines in an
online match would disagree. So the button only sends a message. The message travels
with the players' commands, and on every machine the simulation module receives the
same message in the same frame, checks it and acts.

```text
ui.wasm (your machine)              rules.wasm (every machine)
  click ──── Client.sendMessage([1]) ──▶ onMessage(sender, [1])
                                         checks money, makes the tank,
                                         sets ReinforcementsBought = 1
  plays EVA ◀── Notify "Reinforced" ──── Reinforced.send(sender)
  hides the button ◀── reads ReinforcementsBought
```

Three ways connect the two:

| Way | Direction | In the code |
| --- | --- | --- |
| Message | client → simulation | `Client.sendMessage(bytes)` → `onMessage` |
| Notification | simulation → one player's client | `notify` in `vera.json`, `Reinforced.send(house)` → a `Notify` event |
| Declared variable | simulation → client, kept in saves | `vars` in `vera.json` → `ClientVar` |

## 2. Set up the projects

Make one project per module. From the VERA20K folder:

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
│   ├── vera.json        "kind": "rules", the variable and the notification
│   └── src/index.ts     the simulation module
└── ui/
    ├── vera.json        "kind": "ui"
    └── src/index.ts     the client module
```

(Each project also has its `package.json`, `tsconfig.json` and `.gitignore`.) `--ui`
makes a client project: its `vera.json` says `"kind": "ui"`, it builds
`build/ui.wasm`, and its code imports `@vera20k/sdk/client` instead of `@vera20k/sdk`.

## 3. The simulation module

`rules/vera.json` declares the variable and the notification:

```json
{
  "kind": "rules",
  "vars": [
    { "name": "ReinforcementsBought", "scope": "house", "visibility": "private", "initial": 0 }
  ],
  "notify": ["Reinforced"]
}
```

`rules/src/index.ts`:

```ts
import { Action, House, NO_HANDLE, Rules, Status, Waypoint, lastStatus, log, onMessage } from "@vera20k/sdk";
import { ReinforcementsBought } from "./generated/vars";
import { Reinforced } from "./generated/notify";

/** Message type 1: buy the reinforcements. The message is the one byte. */
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: i32 = 3;

// One message from a client module of this map; `sender` is the house the
// engine put on the command, which the client cannot forge. Everything else
// is untrusted: check it all.
onMessage((sender: House, data: Uint8Array): void => {
  if (data.length != 1 || data[0] != BUY) return;
  // Once per house, and only with the money.
  const bought = ReinforcementsBought.get(sender);
  if (lastStatus() != Status.Ok || bought != 0) return;
  const money = House.money(sender);
  if (lastStatus() != Status.Ok || money < PRICE) return;
  const htnk = Rules.technoType("HTNK");
  if (htnk == NO_HANDLE) return;
  // An occupied cell refuses: charge only for a tank that exists.
  if (Action.createObject(Waypoint.number(DROP_WAYPOINT), htnk, sender) == NO_HANDLE) return;
  Action.addMoney(sender, -PRICE);
  // Visible to the client module's next read.
  ReinforcementsBought.set(sender, 1);
  Reinforced.send(sender);
  log("ui-reinforcements: reinforcements delivered");
});
```

Step by step:

1. **`vars` in `vera.json`** declares a house variable, one value per house. `private`
   means a client module may read it only for its own player's house. Variables are
   saved, so after a load the client module reads the right value at once. The build
   generates `src/generated/vars.ts` with `ReinforcementsBought`, a `HouseVar`.
2. **`notify` in `vera.json`** declares every notification name the module sends; the
   game refuses a notification whose name was not declared. The build generates
   `src/generated/notify.ts` with `Reinforced`. `npm run check` writes both generated
   files too (`vera20k generate`), so it works before the first build.
3. **`onMessage`** registers the handler for messages. `sender` comes from the game; the
   bytes come from a player's machine and may be anything, so check their length and
   values before you trust them. The handler runs in the module phase, on every machine.
4. **The checks**: once per house (`ReinforcementsBought` still 0), enough money. A read
   answers 0 on failure too, so the code checks `lastStatus()` as well. Create the tank
   first and take the money only when that worked: `createObject` answers `NO_HANDLE`
   for an occupied cell, or when the map has no waypoint 3.
5. **`Reinforced.send(sender)`** notifies that house's player. Each client module on that
   player's machine receives a `Notify` event. Notifications are presentation: they are
   not saved, and a player who rejoins does not get old ones.

## 4. The client module

`ui/src/index.ts`:

```ts
import {
  ClickEvent, Client, ClientEvent, ClientVar, Local, NO_HANDLE, Node, NodeKind, NodeProps, NotifyEvent,
  NotifyName, Status, Ui, lastStatus, onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";

/** The simulation module's variable, read under its visibility (private: this player's own house only). */
const BOUGHT = new ClientVar("ReinforcementsBought");
const REINFORCED = new NotifyName("Reinforced");
/** Message type 1: buy. */
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
    // Spectators, or over the message rate.
    Client.log("ui-reinforcements: the message was refused");
  }
});

onClientEvent(ClientEvent.Notify, (e: NotifyEvent): void => {
  if (REINFORCED.is(e)) Local.playSpeech("EVA_ReinforcementsHaveArrived");
});

onRender((dt: f32, alpha: f32): void => {
  if (buy == NO_HANDLE) return;
  // Observers have no house, and scope 0 is no house variable's.
  const me = Client.viewer();
  let show = false;
  if (me != NO_HANDLE) {
    const bought = BOUGHT.get(me);
    show = lastStatus() == Status.Ok && bought == 0;
  }
  Node.setVisible(buy, show);
});
```

Step by step:

1. **Only `@vera20k/sdk/client`.** A client module imports from `@vera20k/sdk/client`.
   The simulation functions of `@vera20k/sdk` (`log`, `get`, `Action`, ...) call the
   simulation's imports, and the game refuses a `ui.wasm` that has them; `vera20k build`
   refuses it first and names the call. That is why this file writes `Client.log`. The
   client package re-exports what both sides share: `Status`, `NO_HANDLE`, `Prop`,
   `Cell.at`, `Find`, `Order` and the other references.
2. **Top-level code** runs once when the module loads (`client_init`). Only lookups work
   there, so it only makes the `ClientVar`, the `NotifyName` and the message bytes, which
   call nothing yet. A top-level `let` (`buy`) keeps a node between callbacks.
3. **`onStart`** runs once the match interface is built, and again after a load or a
   rejoin. `Ui.find("BuyReinforcements")` looks for a part of that name in the interface
   layout ([section 6](#6-place-the-button-with-a-layout)); without one, the module
   creates a button at the top level (`NO_HANDLE` as the parent), 16 pixels from the left
   and 120 from the top. `NodeProps` sets up to 16 properties in the one create call.
4. **`Node.setClickable(button, true)`** makes the button send `Click` events. A node
   sends only the events it asks for (`setClickable`, `setHoverable`, `setDismissable`).
5. **`onClientEvent`** registers a handler per event kind; the handler takes the kind's
   view (`ClickEvent`, `NotifyEvent`, ...). The click handler compares the clicked node
   with the button and sends the one-byte message. `sendMessage` answers `Refused` for
   spectators and when the player sends too many messages (2 a frame, 16 in 15 frames).
6. **The notify handler** checks that the notification is `Reinforced` and plays EVA
   speech on this machine only.
7. **`onRender`** runs every drawn frame. It reads this player's `ReinforcementsBought`
   and shows the button only while it is 0, so the button disappears as soon as the
   simulation has delivered the tank, and stays hidden after a load. `Client.viewer()` is
   `NO_HANDLE` for a spectator, who sees no button.

Reading the variable every frame is cheap. A client module has 100,000 operations per
drawn frame; most interfaces use a small part of it.

`onStart`, `onRender` and `onClientEvent` may each be called several times
(`onClientEvent` for as many kinds as you like); every function you register runs, in
registration order.

**Variables and notifications from `vera.json`.** Instead of writing `new ClientVar(...)`
and `new NotifyName(...)` by hand, a client project can list the names in its own
`vera.json`; the build then generates the constants, as on the simulation side:

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

In a client project these entries declare nothing: the simulation module owns the
variables. Every entry becomes a `ClientVar`, so it needs only its `name`; `scope` and the
other keys are ignored. A name of the other source's module is
written `mod.Name` or `map.Name`.

## 5. Build and try it

```sh
cd rules
npm run build        # build/rules.wasm
npm run check
cd ../ui
npm run build        # build/ui.wasm
npm run check
cd ..
```

With the `asset` tool you can check the simulation module:

```sh
asset wasm-check rules/build/rules.wasm
```

`asset wasm-check` checks only simulation modules. The game checks `ui.wasm` when the
match loads it and tells you in the chat list if it refuses it; the match then plays on
without it.

Make a mod folder with both modules (the `Mods` folder is in the VERA20K folder, see
[section 4 of the simulation tutorial](ts-tutorial.md#4-try-it-in-the-game)):

```text
Mods/MyReinforcements/
├── mod.ini
├── rules.wasm       from rules/build/
└── ui.wasm          from ui/build/
```

```ini
[Mod]
Name=My Reinforcements
Version=1
About=A button that buys a Rhino tank.
```

To build straight into the mod, run `npm run build -- --out "<VERA20K folder>/Mods/MyReinforcements/rules.wasm"`
in `rules/`, and the same with `ui.wasm` in `ui/`.

Start the game with the mod (**Options → Mods**, or `vera20k --mod MyReinforcements`)
and play a skirmish on a map with at least four start positions, with nobody starting at
position 4: the tank appears at waypoint 3. With $2000 or more, click the button. The
tank appears, the money goes down by 2000, EVA speaks and the button disappears.

If nothing happens:

- The button never shows: look for `ui-reinforcements: no button` or a refused `ui.wasm`
  in the chat list.
- The button stays: the simulation refused. No waypoint 3, a unit standing on it, or not
  enough money. Change `DROP_WAYPOINT` to a waypoint the map has.
- `the message was refused`: you are a spectator, or you clicked faster than the message
  limit.

## 6. Place the button with a layout

A mod can place the button with its interface layout instead of letting the module create
it. In the mod's `veraui.ini` ([interface layout](../interface-layout.md)), add a button
without `Command` to the battlefield's parts:

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

`Ui.find("BuyReinforcements")` now finds this part (its name after the layout's prefix),
and the module only changes its label and visibility. A button without `Command` does
nothing by itself: a click only tells the modules. A map can add parts for its own client
module the same way, in `[VeraUI]` ([a map's own parts](../interface-layout.md#a-maps-own-parts)).

## 7. More things a client module can do

Each snippet is a complete client module you can build alone. To add one to the button
module, copy it into the button module's file, leaving out the imports and declarations it already has (such as `BUY_MESSAGE`):
its `onStart` runs after the existing one.

**A hotkey.** Register it in `onStart` (not in top-level code, where only lookups work)
and compare the event's `id`:

```ts
import {
  Client, ClientEvent, Hotkey, HotkeyEvent, Local, NO_ID, Status, onClientEvent, onStart,
} from "@vera20k/sdk/client";

const BUY_MESSAGE = new Uint8Array(1);
BUY_MESSAGE[0] = 1;
let buyKey: i32 = NO_ID;

onStart((): void => {
  // Refused when a game hotkey already has the chord: the hotkey then has no key
  Local.registerHotkey("BuyReinforcements", "Ctrl+Alt+R");
  buyKey = Hotkey.id("BuyReinforcements");
});

onClientEvent(ClientEvent.Hotkey, (e: HotkeyEvent): void => {
  if (e.id == buyKey && Client.sendMessage(BUY_MESSAGE) != Status.Ok) {
    Client.log("the message was refused");
  }
});
```

**A bar and a list.** Create them once (keep them in top-level variables) and write them
when the value changes:

```ts
import { NO_HANDLE, Node, NodeKind, NodeProps, Prop, Ui, onStart } from "@vera20k/sdk/client";

let bar: Node = NO_HANDLE;
let list: Node = NO_HANDLE;

onStart((): void => {
  // A column at the bottom left (Hold 6), 16 pixels in, 160 up
  const panel = Ui.createWith(NodeKind.Column, NO_HANDLE, new NodeProps().with(Prop.Hold, 6).at(16, 160));
  if (panel == NO_HANDLE) return;
  bar = Ui.createWith(NodeKind.Bar, panel, new NodeProps().size(120, 8));
  list = Ui.create(NodeKind.List, panel);
  Node.setFill(bar, 40);                            // 0..100
  Node.setItems(list, ["Rhino ready", "Wave 3"]);
});
```

`Prop.Hold` places a top-level node from a corner or edge: 0 top left, 2 top right,
4 middle, 6 bottom left, 8 bottom right (and 1, 3, 5, 7 the edges between).

**Clicks on the map.** `Cursor.captureMapClicks(true)` sends map clicks only to the
module, as `MapClick` events; set it back to `false` when you are done, or the player
cannot give orders:

```ts
import { Cell, Client, ClientEvent, Cursor, MapClickEvent, onClientEvent, onStart } from "@vera20k/sdk/client";

onStart((): void => {
  Cursor.captureMapClicks(true);    // map clicks now reach only this module
});

onClientEvent(ClientEvent.MapClick, (e: MapClickEvent): void => {
  const where = Cell.x(e.cell).toString() + "," + Cell.y(e.cell).toString();
  Client.log("clicked " + where + (e.right ? " with the right button" : ""));
  Cursor.captureMapClicks(false);   // give the map back to the player
});
```

**The player's own events.** `ClientEvent.SimEvent` delivers `TakeDamage`, `Killed`,
`Birth`, `AttackedBy`, `Commanded` and `OrderRefused` of this player's objects, with
`e.simEvent` giving the record (`null` for none). A client module can show them without
asking the simulation module:

```ts
import { Client, ClientEvent, Event, ForwardedEvent, onClientEvent } from "@vera20k/sdk/client";

onClientEvent(ClientEvent.SimEvent, (e: ForwardedEvent): void => {
  const record = e.simEvent;
  if (record == null) return;
  if (record.kind == Event.Killed) Client.log("one of your units was killed");
});
```

**The selection.** `SelectionChanged` says the local selection changed; read it with a
search for `Selected = 1`, which `Client.findPage` runs under this player's view:

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

**Remember something on this machine.** `Storage` keeps small values between matches,
per mod (or per map):

```ts
import { Local, Storage, onStart } from "@vera20k/sdk/client";

onStart((): void => {
  if (!Storage.has("hint-seen")) {
    Local.showMessage("Click the button to buy a Rhino tank.", 0);
    Storage.writeString("hint-seen", "1");
  }
});
```

Storage is local: other players have their own, so it must never change what the
simulation does.

## 8. Rules to keep in mind

- The client module may use floats, `Client.random` and anything else the compiler
  allows: it changes the game only through messages, which the simulation module checks.
  The TypeScript rules of the
  [simulation tutorial](ts-tutorial.md#6-how-this-typescript-differs) apply
  here too.
- The simulation module must treat every message as untrusted: check the length, every
  value, and whether the sender may do it now.
- Client modules are not saved. After a load they start again (top-level code, then
  `onStart`); keep what the interface must show in declared variables of the simulation
  module.
- A map's client module, under the mod's default `[VeraWasm] MapUI=overlay`, may change
  only its own nodes; `MapUI=no` leaves it out.
- For now ship a client module in a mod: `asset wasm-embed` does not yet write a map's
  `ui.wasm`.

Limits, debugging and the other events are in the [guide](guide.md#limits) and
the [SDK reference](ts-sdk.md#client-modules).
`sdk/typescript/examples/ui-ping` is the smallest pair of modules;
`sdk/typescript/examples/muster-field/ui` a full interface with panels, cards and
pictures.
