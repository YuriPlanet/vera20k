#!/usr/bin/env node
// `npm test`: builds every example, type-checks them with tsc, and checks
// each module against the engine's load rules that need no engine: one
// world's imports, no `start` section, the lifecycle exports, and a
// well-formed `vera.delivery`. The engine tests run the built modules
// (README "Tests").
import { readdirSync, existsSync, readFileSync, statSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { build, generate } from "../bin/vera20k.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const examples = join(root, "examples");

/** Every project directory (one with a `src/`) below `examples/`. */
function projects(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (!statSync(path).isDirectory() || name === "node_modules" || name === "build") continue;
    if (existsSync(join(path, "src"))) out.push(path);
    else out.push(...projects(path));
  }
  return out;
}

function hasStartSection(bytes) {
  // Walk the sections: id 8 is `start`.
  let at = 8;
  while (at < bytes.length) {
    const id = bytes[at++];
    let size = 0, shift = 0, b;
    do { b = bytes[at++]; size |= (b & 0x7f) << shift; shift += 7; } while (b & 0x80);
    if (id === 8) return true;
    at += size;
  }
  return false;
}

let failed = 0;
const check = (ok, what) => { if (!ok) { failed++; console.error(`FAIL ${what}`); } };

for (const dir of projects(examples)) {
  const out = await build(dir);
  const bytes = readFileSync(out);
  const module = new WebAssembly.Module(bytes);
  const ui = out.endsWith("ui.wasm");
  const world = ui ? "vera:client@1.0" : "vera:sim@1.0";
  const name = dir.slice(examples.length + 1);
  for (const i of WebAssembly.Module.imports(module)) check(i.module === world, `${name}: imports ${i.module}.${i.name}`);
  check(!hasStartSection(bytes), `${name}: has a start section`);
  const exports = WebAssembly.Module.exports(module).map((e) => e.name);
  check(exports.includes(ui ? "client_init" : "vera_init"), `${name}: no init export`);
  check(exports.includes("memory"), `${name}: no memory export`);
  const delivery = WebAssembly.Module.customSections(module, "vera.delivery");
  check(delivery.length === 1 && delivery[0].byteLength === 9, `${name}: vera.delivery`);
}

// A compiler run can succeed before the SDK rejects its metadata. A game or
// file watcher opening the output must still see the previous complete module.
const project = mkdtempSync(join(tmpdir(), "vera20k-sdk-output-test-"));
try {
  mkdirSync(join(project, "src"));
  writeFileSync(join(project, "src/index.ts"), 'import { onScenarioStart } from "@vera20k/sdk"; onScenarioStart((): void => {});');
  const output = await build(project);
  const previous = readFileSync(output);
  writeFileSync(join(project, "vera.json"), JSON.stringify({
    vars: [{ name: "Gold", scope: "house", visibility: "unknown" }],
  }));
  await assert.rejects(build(project), /visibility must be/);
  assert.deepEqual(readFileSync(output), previous);

  // Removing a declaration cleans up SDK output. A file supplied by the
  // module's author is left alone, including on re-generation.
  const vars = join(project, "src/generated/vars.ts");
  const handWritten = "export const gold = 200;\n";
  writeFileSync(vars, handWritten);
  writeFileSync(join(project, "vera.json"), "{}");
  generate(project);
  assert.equal(readFileSync(vars, "utf8"), handWritten);
  writeFileSync(join(project, "vera.json"), JSON.stringify({ vars: [{ name: "Gold", scope: "house" }] }));
  assert.throws(() => generate(project), /hand-written source/);
  assert.equal(readFileSync(vars, "utf8"), handWritten);
} finally {
  rmSync(project, { recursive: true, force: true });
}

try {
  execFileSync(process.execPath, [join(root, "node_modules/typescript/bin/tsc"), "-p", examples], { stdio: "inherit" });
} catch {
  failed++;
  console.error("FAIL tsc -p examples");
}

if (failed) {
  console.error(`${failed} check(s) failed`);
  process.exit(1);
}
console.log("all examples build, type-check and pass the load rules");
