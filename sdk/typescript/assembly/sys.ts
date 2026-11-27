// @ts-nocheck: `tsc` rejects decorators on declared functions; AssemblyScript reads `@external` from them.
// The raw `vera:sim@1.0` and `vera:client@1.0` imports (RFC 0001 appendix
// 1.1 and 2.1). Every import returns its status and writes its value
// through a trailing out pointer; `frame` and `client_frame` alone return
// their value. AssemblyScript emits an import only when it is called, so a
// simulation module never imports a client function and the other way
// round: the engine refuses a module importing the other world's functions.

// ------------------------------------------------------------- vera:sim

@external("vera:sim@1.0", "frame")
export declare function frame(): i32;
@external("vera:sim@1.0", "random")
export declare function random(lo: i32, hi: i32, out: usize): i32;
@external("vera:sim@1.0", "log")
export declare function log(ptr: usize, len: i32): void;
@external("vera:sim@1.0", "has_kind")
export declare function has_kind(table: i32, id: i32): i32;
@external("vera:sim@1.0", "lookup")
export declare function lookup(kind: i32, ptr: usize, len: i32, out: usize): i32;
@external("vera:sim@1.0", "name_of")
export declare function name_of(kind: i32, id: i32, out: usize, cap: i32, len_out: usize): i32;
@external("vera:sim@1.0", "get")
export declare function get(reference: i64, prop: i32, out: usize): i32;
@external("vera:sim@1.0", "get_many")
export declare function get_many(refs: usize, n: i32, props: usize, m: i32, out: usize, count_out: usize): i32;
@external("vera:sim@1.0", "set")
export declare function set(reference: i64, prop: i32, value: i64): i32;
@external("vera:sim@1.0", "modify")
export declare function modify(reference: i64, prop: i32, op: i32, value: i64, frames: i32, source: i64, group: i32, out: usize): i32;
@external("vera:sim@1.0", "unmodify")
export declare function unmodify(modifier: i64): i32;
@external("vera:sim@1.0", "rules_get")
export declare function rules_get(sec: usize, sec_len: i32, key: usize, key_len: i32, out: usize, cap: i32, len_out: usize): i32;
@external("vera:sim@1.0", "rules_set")
export declare function rules_set(sec: usize, sec_len: i32, key: usize, key_len: i32, val: usize, val_len: i32): i32;
@external("vera:sim@1.0", "var_get")
export declare function var_get(scope: i64, v: i32, out: usize): i32;
@external("vera:sim@1.0", "var_set")
export declare function var_set(scope: i64, v: i32, value: i64): i32;
@external("vera:sim@1.0", "find")
export declare function find(query: usize, len: i32, out: usize, cap: i32, res: usize): i32;
@external("vera:sim@1.0", "trigger_create")
export declare function trigger_create(def: usize, len: i32, out: usize): i32;
@external("vera:sim@1.0", "trigger_remove")
export declare function trigger_remove(trigger: i64): i32;
@external("vera:sim@1.0", "trigger_bind")
export declare function trigger_bind(trigger: i64, reference: i64): i32;
@external("vera:sim@1.0", "trigger_unbind")
export declare function trigger_unbind(trigger: i64, reference: i64): i32;
@external("vera:sim@1.0", "action")
export declare function action(kind: i32, target: i64, params: usize, len: i32, out: usize): i32;
@external("vera:sim@1.0", "send_notify")
export declare function send_notify(to: i64, id: i32, ptr: usize, len: i32): i32;
@external("vera:sim@1.0", "order")
export declare function order(house: i64, ptr: usize, len: i32): i32;
@external("vera:sim@1.0", "ini_register")
export declare function ini_register(kind: i32, section: i32, name: usize, len: i32, key: i32, out: usize): i32;

// ---------------------------------------------------------- vera:client

@external("vera:client@1.0", "client_frame")
export declare function client_frame(): i32;
@external("vera:client@1.0", "client_random")
export declare function client_random(lo: i32, hi: i32, out: usize): i32;
@external("vera:client@1.0", "client_log")
export declare function client_log(ptr: usize, len: i32): void;
@external("vera:client@1.0", "client_has_kind")
export declare function client_has_kind(table: i32, id: i32): i32;
@external("vera:client@1.0", "client_lookup")
export declare function client_lookup(kind: i32, ptr: usize, len: i32, out: usize): i32;
@external("vera:client@1.0", "client_name_of")
export declare function client_name_of(kind: i32, id: i32, out: usize, cap: i32, len_out: usize): i32;
@external("vera:client@1.0", "client_get")
export declare function client_get(reference: i64, prop: i32, out: usize): i32;
@external("vera:client@1.0", "client_get_many")
export declare function client_get_many(refs: usize, n: i32, props: usize, m: i32, out: usize, count_out: usize): i32;
@external("vera:client@1.0", "client_find")
export declare function client_find(query: usize, len: i32, out: usize, cap: i32, res: usize): i32;
@external("vera:client@1.0", "client_var_get")
export declare function client_var_get(scope: i64, v: i32, out: usize): i32;
@external("vera:client@1.0", "client_set")
export declare function client_set(reference: i64, prop: i32, value: i64): i32;
@external("vera:client@1.0", "client_set_bytes")
export declare function client_set_bytes(reference: i64, prop: i32, ptr: usize, len: i32): i32;
@external("vera:client@1.0", "client_node_create")
export declare function client_node_create(kind: i32, parent: i64, props: usize, len: i32, out: usize): i32;
@external("vera:client@1.0", "client_node_remove")
export declare function client_node_remove(node: i64): i32;
@external("vera:client@1.0", "client_node_find")
export declare function client_node_find(ptr: usize, len: i32, out: usize): i32;
@external("vera:client@1.0", "client_action")
export declare function client_action(kind: i32, target: i64, params: usize, len: i32): i32;
@external("vera:client@1.0", "client_send_order")
export declare function client_send_order(ptr: usize, len: i32): i32;
@external("vera:client@1.0", "client_send_message")
export declare function client_send_message(ptr: usize, len: i32): i32;
@external("vera:client@1.0", "client_storage_read")
export declare function client_storage_read(key: usize, key_len: i32, out: usize, cap: i32, len_out: usize): i32;
@external("vera:client@1.0", "client_storage_write")
export declare function client_storage_write(key: usize, key_len: i32, data: usize, data_len: i32): i32;
@external("vera:client@1.0", "client_status")
export declare function client_status(ptr: usize, len: i32): i32;

// -------------------------------------------------------- Fixed layouts
// Byte sizes and field offsets (appendix "Fixed layouts"). The SDK reads
// and writes them with load/store at these offsets.

export const ACTION_PARAMS_BYTES: i32 = 72;
export const CLAUSE_BYTES: i32 = 16;
export const QUERY_BYTES: i32 = 64;
export const FIND_RESULT_BYTES: i32 = 16;
export const TRIGGER_DEF_BYTES: i32 = 32;
export const EVENT_DEF_BYTES: i32 = 32;
export const ACTION_DEF_BYTES: i32 = 80;
export const EVENT_RECORD_BYTES: i32 = 120;
export const DECIDE_RECORD_BYTES: i32 = 160;
export const CLIENT_EVENT_RECORD_BYTES: i32 = 32;
export const PROP_VALUE_BYTES: i32 = 16;

// ActionParams
export const AP_ID: i32 = 0;
export const AP_ARG: i32 = 4;
export const AP_ARG2: i32 = 8;
export const AP_VALUE: i32 = 16;
export const AP_VALUE2: i32 = 24;
export const AP_CELL: i32 = 32;
export const AP_HOUSE: i32 = 40;
export const AP_OTHER: i32 = 48;
export const AP_TEXT: i32 = 56;
export const AP_TEXT_LEN: i32 = 60;
export const AP_EXTRA: i32 = 64;
export const AP_EXTRA_LEN: i32 = 68;

// EventRecord
export const ER_KEY: i32 = 0;
export const ER_KIND: i32 = 4;
export const ER_TRIGGER: i32 = 8;
export const ER_HOLDER: i32 = 16;
export const ER_SUBJECT: i32 = 24;
export const ER_OTHER: i32 = 32;
export const ER_HOUSE: i32 = 40;
export const ER_PROJECTILE: i32 = 48;
export const ER_MODIFIER: i32 = 56;
export const ER_CELL: i32 = 64;
export const ER_VALUE: i32 = 72;
export const ER_VALUE2: i32 = 80;
export const ER_ID: i32 = 88;
export const ER_FRAME: i32 = 92;
export const ER_X: i32 = 96;
export const ER_Y: i32 = 100;
export const ER_Z: i32 = 104;
export const ER_ARGS: i32 = 108;
export const ER_TARGET: i32 = 112;

// DecideRecord: an EventRecord, then these.
export const DR_NATIVE: i32 = 120;
export const DR_PREVIOUS: i32 = 128;
export const DR_RESULT: i32 = 136;
export const DR_HANDLED: i32 = 144;
export const DR_AREA: i32 = 148;
export const DR_AREA_CAP: i32 = 152;
export const DR_PREVIOUS_LEN: i32 = 156;

// ClientEventRecord
export const CR_KIND: i32 = 0;
export const CR_SOURCE: i32 = 4;
export const CR_REF: i32 = 8;
export const CR_ID: i32 = 16;
export const CR_VALUE: i32 = 20;
export const CR_DATA: i32 = 24;
export const CR_DATA_LEN: i32 = 28;
