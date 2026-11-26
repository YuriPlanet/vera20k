// The compiler's `abort` (`--use abort=…`): an AssemblyScript runtime error
// such as an index out of range, a null dereference or a `throw` logs its
// message and source position, then traps. The trap stops the module and
// the engine names it (RFC 3); the line before it says why. One handler per
// world, each calling only its own world's `log`.

import * as sys from "./sys";

function line(message: string | null, file: string | null, lineNumber: u32, column: u32): ArrayBuffer {
  return String.UTF8.encode(
    "abort: " + (message ? message : "error") + " at " + (file ? file : "?") + ":" +
    lineNumber.toString() + ":" + column.toString(),
  );
}

export function simAbort(message: string | null, file: string | null, lineNumber: u32, column: u32): void {
  const text = line(message, file, lineNumber, column);
  sys.log(changetype<usize>(text), text.byteLength);
  unreachable();
}

export function clientAbort(message: string | null, file: string | null, lineNumber: u32, column: u32): void {
  const text = line(message, file, lineNumber, column);
  sys.client_log(changetype<usize>(text), text.byteLength);
  unreachable();
}
