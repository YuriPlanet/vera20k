#!/usr/bin/env node
// Prints a module's imports, exports and custom sections.
import { readFileSync } from "node:fs";
const m = new WebAssembly.Module(readFileSync(process.argv[2]));
console.log("imports:", WebAssembly.Module.imports(m).map((i) => `${i.module}.${i.name}`).join(" "));
console.log("exports:", WebAssembly.Module.exports(m).map((e) => e.name).join(" "));
for (const name of ["vera.delivery", "vera.actions", "vera.vars", "vera.notify"]) {
  for (const s of WebAssembly.Module.customSections(m, name)) console.log(`${name}:`, Buffer.from(s).toString("hex"));
}
