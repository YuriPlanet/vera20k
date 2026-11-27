// Delivery records read in place (appendix "Fixed layouts"): an
// `EventRecord` in `on_events`, a `DecideRecord` in `on_decide`. Views are
// valid only during their callback; keep the values, not the view.

import * as sys from "./sys";
import { Techno, House, Cell, text } from "./core";

/** What a trigger action acts on (`ActionTarget`, appendix 5.1). */
export enum Target {
  Subject = 0,
  Other = 1,
  TagHolder = 2,
  Projectile = 3,
  House = 4,
  Cell = 5,
}

/** One delivered record (`EventRecord`, 120 bytes). */
@unmanaged
export class EventRecord {
  /** The dispatch key of the module action that queued it. */
  get key(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_KEY);
  }
  /** The event kind (`Event`). */
  get kind(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_KIND);
  }
  get trigger(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_TRIGGER);
  }
  /** The tag holder: the bound object, or the object or cell whose tag holds the trigger (0 for none). */
  get holder(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_HOLDER);
  }
  get subject(): Techno {
    return load<i64>(changetype<usize>(this), sys.ER_SUBJECT);
  }
  get other(): Techno {
    return load<i64>(changetype<usize>(this), sys.ER_OTHER);
  }
  get house(): House {
    return load<i64>(changetype<usize>(this), sys.ER_HOUSE);
  }
  get projectile(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_PROJECTILE);
  }
  get modifier(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_MODIFIER);
  }
  get cell(): Cell {
    return load<i64>(changetype<usize>(this), sys.ER_CELL);
  }
  get value(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_VALUE);
  }
  get value2(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_VALUE2);
  }
  get id(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_ID);
  }
  get frame(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_FRAME);
  }
  get x(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_X);
  }
  get y(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_Y);
  }
  get z(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_Z);
  }
  /** String ID: a module action's parameter text, an `IniEntry` entry name, an `IniKey` value; 0 for none. */
  get args(): i32 {
    return load<i32>(changetype<usize>(this), sys.ER_ARGS);
  }
  /** The `args` text (`""` without any). */
  get argsText(): string {
    return text(this.args);
  }
  /** A module action's target, as its trigger names it (`Subject` on other records). */
  get target(): Target {
    const raw = load<i32>(changetype<usize>(this), sys.ER_TARGET);
    return raw >= 0 && raw <= 5 ? <Target>raw : Target.Subject;
  }
}

/** One decision request (`DecideRecord`, 160 bytes), read and written in place. */
@unmanaged
export class DecideRecord {
  /** The embedded event. */
  get event(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return load<i64>(changetype<usize>(this), sys.DR_NATIVE);
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return load<i64>(changetype<usize>(this), sys.DR_PREVIOUS);
  }
  get result(): i64 {
    return load<i64>(changetype<usize>(this), sys.DR_RESULT);
  }
  /** Writes `result` and sets `handled`. */
  setResult(value: i64): void {
    store<i64>(changetype<usize>(this), value, sys.DR_RESULT);
    store<i32>(changetype<usize>(this), 1, sys.DR_HANDLED);
  }
  get handled(): bool {
    return load<i32>(changetype<usize>(this), sys.DR_HANDLED) != 0;
  }
  /** The result area's address in module memory (0 without one). */
  get area(): usize {
    return <usize>load<i32>(changetype<usize>(this), sys.DR_AREA);
  }
  /** The result area's capacity in bytes. */
  get areaCap(): i32 {
    return load<i32>(changetype<usize>(this), sys.DR_AREA_CAP);
  }
  /** Bytes of the previous link's answer in the area. */
  get previousLen(): i32 {
    return load<i32>(changetype<usize>(this), sys.DR_PREVIOUS_LEN);
  }
}
