// @vera20k/sdk/client: the TypeScript SDK for client modules (`ui.wasm`),
// RFC 0001 section 10. A project with `"kind": "ui"` in its `vera.json`
// builds with it.
//
// A client module may import only `vera:client` functions, and the engine
// refuses one importing a `vera:sim` function. Everything exported here is
// safe in a client module. The shared reference namespaces re-exported
// below (`Techno`, `House`, `Cell`) also hold simulation reads such as
// `Techno.health`, which call `vera:sim`: read properties with
// `Client.get(t, Prop.Health)` instead. Only `Cell.at`, `Cell.x`, `Cell.y`
// and `TypeRef.make` / `id` / `kind` are free of host calls.

export {
  Client, Ui, Node, NodeProps, Camera, Cursor, Local, Hotkey, ClientVar, NotifyName, Storage,
  StatusTemplates,
} from "./api";
export {
  onStart, onRender, onClientEvent,
  ClientEventRecord, ClickEvent, HoverEvent, HotkeyEvent, DismissEvent, MapClickEvent,
  SelectionChangedEvent, NotifyEvent, ForwardedEvent,
} from "./events";
export { NodeKind, ClientEvent, ClientActionKind } from "../generated/client_tables";

// Shared with the simulation SDK: statuses, references, tables, orders,
// the search builder (run it with `Client.findPage`) and forwarded records.
export {
  Status, lastStatus, NO_HANDLE, NO_ID, Ref, Techno, House, Cell, TypeRef, Waypoint, Prop, NameKind,
  Op, Category, Command, Order, ORDER_UNITS, Find, FindPage, MAX_CLAUSES, params,
} from "../core";
export { EventRecord } from "../records";
export { Event } from "../generated/events";
