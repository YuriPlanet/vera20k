# Tutorial: a client module (`ui.wasm`)

[English](rust-ui-tutorial.md) | [简体中文](rust-ui-tutorial.zh-CN.md)

This tutorial builds a button on the match screen: "Reinforcements: Rhino tank, $2000".
A click buys one Rhino tank, once per player; the button then disappears and EVA says
the reinforcements have arrived. It takes two modules that work together, a client
module (`ui.wasm`) for the button and a simulation module (`rules.wasm`) for the tank.
The complete code is `sdk/examples/ui-reinforcements` in the VERA20K source.

- Who it is for: you have done the [simulation module tutorial](rust-tutorial.md),
  or you know enough Rust to read it.
- Reference: [Rust SDK reference](rust-sdk.md#client-modules),
  [guide](guide.md), [interface layout](../interface-layout.md) for buttons placed
  by a layout.
- Preview: names and numbers may still change before version 1.

## 1. Why two modules

A client module runs only on your machine. It sees what your screen may show and can
change the interface, but it cannot change the game: if it could, the machines in an
online match would disagree. So the button only sends a message. The message travels
with the players' commands, and on every machine the simulation module receives the
same message in the same frame, checks it and acts.

```text
ui.wasm (your machine)           rules.wasm (every machine)
  click ──── send_message([1]) ───▶ on_message(sender, [1])
                                      checks money, makes the tank,
                                      sets ReinforcementsBought = 1
  plays EVA ◀── Notify "Reinforced" ── REINFORCED.send(sender)
  hides the button ◀── reads ReinforcementsBought
```

Three ways connect the two:

| Way | Direction | In the code |
| --- | --- | --- |
| Message | client → simulation | `Client::send_message(&bytes)` → `on_message!` |
| Notification | simulation → one player's client | `notify_names!`, `Notify::send` → a `Notify` event |
| Declared variable | simulation → client, kept in saves | `vars!` → `ClientVar` |

## 2. Set up the project

You need Rust with `wasm32-unknown-unknown` and the VERA20K source, as in sections 2.1
and 2.2 of the [simulation tutorial](rust-tutorial.md#2-set-up). Copy the
example:

```sh
cp -R sdk/examples/ui-reinforcements sdk/examples/my-reinforcements
```

It is a Cargo workspace with one crate per module:

```text
my-reinforcements/
├── .cargo/config.toml   the Wasm target and allowed features; keep it
├── Cargo.toml           the workspace: members rules and ui, release profile
├── rules/
│   ├── Cargo.toml       package ui-reinforcements-rules
│   └── src/lib.rs       the simulation module
└── ui/
    ├── Cargo.toml       package ui-reinforcements-ui
    └── src/lib.rs       the client module
```

Rename the two packages in `rules/Cargo.toml` and `ui/Cargo.toml` if you like (the
`.wasm` files are named after them). Both depend on the SDK with
`vera20k-sdk = { path = "../../../rust/vera20k-sdk" }`; change the path if the project
lives elsewhere.

## 3. The simulation module

`rules/src/lib.rs`:

```rust
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::vars! {
    // Private: only the house's own player reads it.
    pub static BOUGHT: HouseVar = ("ReinforcementsBought", Private, 0);
}

vera20k_sdk::notify_names! {
    pub static REINFORCED: Notify = "Reinforced";
}

/// Message type 1: buy the reinforcements. The message is the one byte.
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: u32 = 3;

/// One message from a client module of this source; `sender` is the house the
/// engine put on the command, which the client cannot forge. Everything
/// else is untrusted: check it all.
fn buy_message(sender: House, data: &[u8]) {
    if data.len() != 1 || data[0] != BUY {
        return;
    }
    // Once per house, and only with the money.
    if BOUGHT.get(sender) != Ok(0) || !sender.money().is_ok_and(|m| m >= i64::from(PRICE)) {
        return;
    }
    let Ok(htnk) = TypeRef::techno_type("HTNK") else {
        return;
    };
    // An occupied cell refuses: charge only for a tank that exists.
    if Action::create_object(Waypoint::number(DROP_WAYPOINT), htnk, sender).is_err() {
        return;
    }
    let _ = Action::add_money(sender, -PRICE);
    // Visible to the client module's next read.
    let _ = BOUGHT.set(sender, 1);
    let _ = REINFORCED.send(sender, &[]);
    log("ui-reinforcements: reinforcements delivered");
}

vera20k_sdk::on_message!(buy_message);
```

Step by step:

1. **`vars!`** declares a house variable, one value per house. `Private` means a client
   module may read it only for its own player's house. Variables are saved, so after a
   load the client module reads the right value at once.
2. **`notify_names!`** declares every notification name the module sends. The game
   refuses a notification whose name was not declared.
3. **`on_message!`** exports the handler for messages. `sender` comes from the game; the
   bytes come from a player's machine and may be anything, so check their length and
   values before you trust them. The handler runs in the module phase, on every machine.
4. **The checks**: once per house (`BOUGHT` still 0), enough money. Create the tank first
   and take the money only when that worked: `create_object` answers `Err(Refused)` for
   an occupied cell, and `Err(NotFound)` when the map has no waypoint 3.
5. **`REINFORCED.send(sender, &[])`** notifies that house's player. Each client module on
   that player's machine receives a `Notify` event. Notifications are presentation: they
   are not saved, and a player who rejoins does not get old ones.

## 4. The client module

`ui/src/lib.rs`:

```rust
#![no_std]

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static BUY: NodeSlot = NodeSlot::new();
/// The simulation module's variable, read under its visibility (private:
/// this player's own house only).
static BOUGHT: ClientVar = ClientVar::new("ReinforcementsBought");
static REINFORCED: NotifyName = NotifyName::new("Reinforced");

/// Message type 1: buy.
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
        // Spectators, or over the message rate.
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
    // Observers have no house, and scope 0 is no house variable's.
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

Step by step:

1. **Only `client::` functions.** A client module uses `vera20k_sdk::client` and its
   prelude. The crate-root functions (`log`, `get`, `Action`, ...) are the simulation's,
   and the game refuses a `ui.wasm` that imports them. That is why this file writes
   `client::log`.
2. **`on_start!`** runs once the match interface is built, and again after a load or a
   rejoin. `Ui::find("BuyReinforcements")` looks for a part of that name in the
   interface layout ([section 6](#6-place-the-button-with-a-layout)); without one, the
   module creates a button at the top level, 16 pixels from the left and 120 from the
   top. `NodeSlot` keeps the node for the other callbacks.
3. **`set_clickable(true)`** makes the button send `Click` events. A node sends only the
   events it asks for (`Clickable`, `Hoverable`, `Dismissable`).
4. **`on_client_event!`** lists a handler per event kind. `on_click` compares the clicked
   node with the button and sends the one-byte message. `send_message` answers
   `Err(Refused)` for spectators and when the player sends too many messages (2 a frame,
   16 in 15 frames).
5. **`on_notify`** checks that the notification is `Reinforced` and plays EVA speech on
   this machine only.
6. **`on_render!`** runs every drawn frame. It reads this player's `ReinforcementsBought`
   and shows the button only while it is 0, so the button disappears as soon as the
   simulation has delivered the tank, and stays hidden after a load. `Client::viewer()` is
   `House(NO_HANDLE)` for a spectator, who sees no button.

Reading the variable every frame is cheap. A client module has 100,000 operations per
drawn frame; most interfaces use a small part of it.

## 5. Build and try it

```sh
cd sdk/examples/my-reinforcements
cargo build --release
```

The workspace builds both modules into `target/wasm32-unknown-unknown/release/`:
`ui_reinforcements_rules.wasm` and `ui_reinforcements_ui.wasm` (or the names of your
renamed packages, with `_` for `-`). Check the simulation module:

```sh
cp target/wasm32-unknown-unknown/release/ui_reinforcements_rules.wasm rules.wasm
cp target/wasm32-unknown-unknown/release/ui_reinforcements_ui.wasm ui.wasm
asset wasm-check rules.wasm
```

`asset wasm-check` checks only simulation modules. The game checks `ui.wasm` when the
match loads it and tells you in the chat list if it refuses it; the match then plays on
without it.

Make a mod folder with both modules (the `Mods` folder is in the VERA20K folder, see the
[guide](guide.md#put-it-in-a-mod)):

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

`Ui::find("BuyReinforcements")` now finds this part (its name after the layout's prefix),
and the module only changes its label and visibility. A button without `Command` does
nothing by itself: a click only tells the modules. A map can add parts for its own client
module the same way, in `[VeraUI]` ([a map's own parts](../interface-layout.md#a-maps-own-parts)).

## 7. More things a client module can do

Each snippet goes into the client module above.

**A hotkey.** Register it in `on_start` (not in `client_init`, where only lookups work)
and compare the event's `id`:

```rust
use core::sync::atomic::{AtomicI32, Ordering::Relaxed};

static BUY_KEY: AtomicI32 = AtomicI32::new(-1);

fn register_key() {
    // Refused when a game hotkey already has the chord; the hotkey then has no key.
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

Call `register_key()` from `start()` and add `ClientEvent::Hotkey => on_hotkey` to
`on_client_event!`.

**A bar and a list.** Create them once (keep them in `NodeSlot`s) and write them when the
value changes:

```rust
fn make_panel() -> vera20k_sdk::Result<()> {
    let panel = Ui::create_with(NodeKind::Column, Node(NO_HANDLE), &[(Prop::X, 16), (Prop::Y, 160)])?;
    let bar = Ui::create_with(NodeKind::Bar, panel, &[(Prop::Width, 120), (Prop::Height, 8)])?;
    let list = Ui::create(NodeKind::List, panel)?;
    bar.set_fill(40)?;                         // 0..100
    list.set_items(&["Rhino ready", "Wave 3"])
}
```

`Prop::Hold` places a top-level node from a corner or edge: 0 top left, 2 top right,
4 middle, 6 bottom left, 8 bottom right (and 1, 3, 5, 7 the edges between).

**Clicks on the map.** `Cursor::capture_map_clicks(true)` sends map clicks only to the
module, as `MapClick` events with `event.cell()`; set it back to `false` when you are
done, or the player cannot give orders.

**The player's own events.** `ClientEvent::SimEvent` delivers `TakeDamage`, `Killed`,
`Birth`, `AttackedBy`, `Commanded` and `OrderRefused` of this player's objects, with
`event.sim_event()` giving the record. A client module can show them without asking the
simulation module.

**Remember something on this machine.** `Storage` keeps small values between matches,
per mod (or per map):

```rust
let mut buf = [0u8; 1];
let hint_seen = Storage::read("hint-seen", &mut buf).is_ok();
if !hint_seen {
    let _ = Local::show_message("Click the button to buy a Rhino tank.", 0);
    let _ = Storage::write("hint-seen", &[1]);
}
```

Storage is local: other players have their own, so it must never change what the
simulation does.

## 8. Rules to keep in mind

- The client module may use floats and `client::random` for local effects. The shop
  sends purchase requests to its simulation module, which checks them.
- The simulation module must treat every message as untrusted: check the length, every
  value, and whether the sender may do it now.
- Client modules are not saved. After a load they start again (`on_start`); keep what
  the interface must show in declared variables of the simulation module.
- A map's client module, under the mod's default `[VeraWasm] MapUI=overlay`, may change
  only its own nodes; `MapUI=no` leaves it out.
- For now ship a client module in a mod: `asset wasm-embed` does not yet write a map's
  `ui.wasm`.

Limits, debugging and the other events are in the [guide](guide.md#limits) and
the [SDK reference](rust-sdk.md#client-modules). `sdk/examples/ui-ping` is the
smallest pair of modules; `sdk/examples/muster-field/ui` a full interface with panels,
cards and pictures.
