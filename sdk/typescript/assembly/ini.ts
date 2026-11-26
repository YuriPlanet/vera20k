// INI names a module registers (RFC 0001 section 6h, appendix section 3
// "INI registration"): keys on type sections, registries (list sections)
// and module events, so a modder writes INI and the module implements it.
//
// Register in top-level code (the initialization function); a
// registration anywhere else stops the module. Before `onScenarioStart`,
// every `IniEntry` record reaches its registry's handler, then every
// `IniKey` record its key's handler. Values are the final merged rules
// text; the engine does not check them.
//
//   function loadShieldType(r: IniEntryRecord): void {
//     const strength = Ini.int(Rules.get(r.entryName, "Strength"), 100);
//   }
//   function attachShield(r: IniKeyRecord): void {
//     const shield = r.value; // "HeavyShield" on the type r.typeRef
//   }
//   Ini.registry("Kratos.ShieldTypes", loadShieldType);
//   Ini.key("Kratos.Shield", IniSection.TechnoType, attachShield);
//   const SHIELD_BROKEN = Ini.event("Kratos.ShieldBroken", EventScope.Object);

import * as sys from "./sys";
import {
  Status, check, lastStatus, OUT, utf8, utf8Len, NO_ID, NameKind, TypeRef, Techno, House, Global,
  lookup, nameOf, text,
} from "./core";
import { keyOf } from "./triggers";
import { EventRecord } from "./records";

/** The section kind a registered key sits in (`IniSection`). */
export enum IniSection {
  TechnoType = 0,
  Weapon = 1,
  Warhead = 2,
  Projectile = 3,
  SuperWeapon = 4,
  Country = 5,
  /** Only `[General]`. */
  General = 6,
}

/** Where a module event happens. */
export enum EventScope {
  /** On an object: triggers bound to it (or its type) fire. */
  Object = 0,
  /** Once for the game: unbound triggers fire. */
  Global = 1,
}

/** A module event's ID, from `Ini.event` or `ModuleEvent.named`; `NO_ID` for none. */
export type ModuleEvent = i32;

export namespace ModuleEvent {
  /** The event any source registered as `name` (another module's too), or `NO_ID`. */
  export function named(name: string): ModuleEvent {
    return lookup(NameKind.ModuleEvent, name);
  }
}

// `IniKind`.
const KIND_KEY: i32 = 0;
const KIND_REGISTRY: i32 = 1;
const KIND_EVENT: i32 = 2;

function register(kind: i32, section: i32, name: string, key: i32): Status {
  const at = utf8(name);
  return check(sys.ini_register(kind, section, at, utf8Len, key, OUT));
}

export namespace Ini {
  /**
   * Registers key `name` on `section` sections: before `onScenarioStart`,
   * every such section holding `name=` sends one `IniKeyRecord` to `fn`.
   * Names are 1 to 63 ASCII letters, digits, `.` and `_`, compared
   * ignoring case. `Refused` when another registration (any source) has
   * the name, or it starts with `Vera.`.
   */
  export function key(name: string, section: IniSection, fn: (r: IniKeyRecord) => void): Status {
    return register(KIND_KEY, <i32>section, name, keyOf(changetype<usize>(fn)));
  }

  /**
   * Registers the list section `[name]`: before `onScenarioStart`, each
   * entry in it (an entry section's name; the first of duplicates) sends
   * one `IniEntryRecord` to `fn`, in list order. Refused as `key` is.
   */
  export function registry(name: string, fn: (r: IniEntryRecord) => void): Status {
    return register(KIND_REGISTRY, 0, name, keyOf(changetype<usize>(fn)));
  }

  /**
   * Registers module event `name`, which INI triggers use as
   * `Event=Wasm:name` and code as `Trigger.onModuleEvent`, and returns its
   * ID; `NO_ID` when refused as `key` is (`lastStatus()` says why).
   */
  export function event(name: string, scope: EventScope): ModuleEvent {
    if (register(KIND_EVENT, <i32>scope, name, -1) != Status.Ok) return NO_ID;
    return load<i32>(OUT);
  }

  // ------------------------------------------------------------ Parsing
  // Each returns its value, `otherwise` for a malformed one; `lastStatus()`
  // is then `NotFound` (`Ok` for a good value).

  /**
   * The comma-separated parts of `text`, each without surrounding ASCII
   * spaces: `split("Subject, 50")` gives `"Subject"`, `"50"`.
   */
  export function split(text: string): string[] {
    const parts = text.split(",");
    for (let i = 0; i < parts.length; i++) unchecked((parts[i] = trim(unchecked(parts[i]))));
    return parts;
  }

  /** `text` without a `;` comment or surrounding ASCII spaces. */
  export function stripComment(text: string): string {
    const at = text.indexOf(";");
    return trim(at < 0 ? text : text.substring(0, at));
  }

  /** A decimal integer with an optional sign, surrounding spaces ignored. */
  export function int(text: string, otherwise: i64 = 0): i64 {
    const s = trim(text);
    let i = 0;
    let negative = false;
    if (s.length > 0) {
      const c = s.charCodeAt(0);
      if (c == 45 /* - */ || c == 43 /* + */) {
        negative = c == 45;
        i = 1;
      }
    }
    if (i >= s.length) return bad(otherwise);
    // Accumulated negatively, so the most negative i64 parses too.
    let value: i64 = 0;
    const limit: i64 = i64.MIN_VALUE;
    for (; i < s.length; i++) {
      const digit = s.charCodeAt(i) - 48;
      if (digit < 0 || digit > 9) return bad(otherwise);
      if (value < (limit + <i64>digit) / 10) return bad(otherwise);
      value = value * 10 - <i64>digit;
    }
    if (!negative) {
      if (value == limit) return bad(otherwise);
      value = -value;
    }
    check(Status.Ok);
    return value;
  }

  /**
   * An INI yes/no as the game reads it: first letter `y`, `t` or `1` is
   * true, `n`, `f` or `0` false; `otherwise` for anything else.
   */
  export function yesNo(text: string, otherwise: bool = false): bool {
    const s = trim(text);
    if (s.length > 0) {
      const c = s.charCodeAt(0);
      if (c == 121 || c == 89 || c == 116 || c == 84 || c == 49) {
        check(Status.Ok);
        return true;
      }
      if (c == 110 || c == 78 || c == 102 || c == 70 || c == 48) {
        check(Status.Ok);
        return false;
      }
    }
    check(Status.NotFound);
    return otherwise;
  }

  /**
   * The first `out.length` comma-separated integers of `text` (`split`,
   * `int`) into `out`; a missing or malformed part is `otherwise`. Returns
   * a mask with bit `i` set when part `i` was a good integer (parts past
   * 31 are not in it).
   */
  export function ints(text: string, out: StaticArray<i64>, otherwise: i64 = 0): i32 {
    const parts = split(text);
    let mask = 0;
    for (let i = 0; i < out.length; i++) {
      let value = otherwise;
      if (i < parts.length) {
        value = int(unchecked(parts[i]), otherwise);
        if (lastStatus() == Status.Ok && i < 32) mask |= 1 << i;
      }
      unchecked((out[i] = value));
    }
    check(Status.Ok);
    return mask;
  }
}

function bad(otherwise: i64): i64 {
  check(Status.NotFound);
  return otherwise;
}

function isSpace(c: i32): bool {
  // As Rust's `trim_ascii`: space, tab, LF, FF, CR.
  return c == 32 || c == 9 || c == 10 || c == 12 || c == 13;
}

function trim(s: string): string {
  let start = 0;
  let end = s.length;
  while (start < end && isSpace(s.charCodeAt(start))) start++;
  while (end > start && isSpace(s.charCodeAt(end - 1))) end--;
  return start == 0 && end == s.length ? s : s.substring(start, end);
}

// --------------------------------------------------------------- Records

/** `IniEntry`: one entry of a registered list section. */
@unmanaged
export class IniEntryRecord {
  /** The entry's position in the merged list, from 0. */
  get position(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_VALUE);
  }
  /** The entry section's name; read its keys with `Rules.get`. */
  get entryName(): string {
    return text(this.record.args);
  }
  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
}

/** `IniKey`: one section holding a registered key. */
@unmanaged
export class IniKeyRecord {
  /** The section's type (`TypeRef.id` within its kind); `Global.REF` for `[General]` (`isGeneral`). */
  get typeRef(): TypeRef {
    return load<i64>(changetype<usize>(this), sys.ER_SUBJECT);
  }
  /** Whether the key is in `[General]`. */
  get isGeneral(): bool {
    return this.typeRef == Global.REF;
  }
  /** The key's merged value text. */
  get value(): string {
    return text(this.record.args);
  }
  /** The section's name, for a message that names the section and key of a bad value. */
  get sectionName(): string {
    if (this.isGeneral) return "General";
    const t = this.typeRef;
    return nameOf(TypeRef.kind(t), TypeRef.id(t));
  }
  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
}

/** `ModuleEvent`: a raised module event, with what `RaiseEvent` gave. */
@unmanaged
export class ModuleEventRecord {
  get event(): ModuleEvent {
    return load<i32>(changetype<usize>(this), sys.ER_ID);
  }
  /** The object an object event was raised on. */
  get subject(): Techno {
    return load<i64>(changetype<usize>(this), sys.ER_SUBJECT);
  }
  get other(): Techno {
    return load<i64>(changetype<usize>(this), sys.ER_OTHER);
  }
  get value(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_VALUE);
  }
  get value2(): i64 {
    return load<i64>(changetype<usize>(this), sys.ER_VALUE2);
  }
  get house(): House {
    return load<i64>(changetype<usize>(this), sys.ER_HOUSE);
  }
  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
}
