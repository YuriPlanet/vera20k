// Run a wasm32-wasip1 build of a headless tool, such as sim-bench, under
// Node's WASI:
//
//   node tools/wasm/wasi_run.mjs <program.wasm> [--dir <guest>=<host>]... -- <args...>
//
// Each --dir opens a host folder to the program at a guest path, for example
// `--dir /game=<retail install>`; the program sees no other files. The process
// exits with the program's exit code.
import { readFile } from 'node:fs/promises';
import process from 'node:process';
import { WASI } from 'node:wasi';

const usage = 'usage: node wasi_run.mjs <program.wasm> [--dir <guest>=<host>]... -- <args...>';
const [program, ...rest] = process.argv.slice(2);
const separator = rest.indexOf('--');
if (!program || separator < 0) {
    console.error(usage);
    process.exit(2);
}
const preopens = {};
const options = rest.slice(0, separator);
for (let index = 0; index < options.length; index += 2) {
    const value = options[index + 1] ?? '';
    const split = value.indexOf('=');
    if (options[index] !== '--dir' || split <= 0) {
        console.error(usage);
        process.exit(2);
    }
    preopens[value.slice(0, split)] = value.slice(split + 1);
}

const wasi = new WASI({
    version: 'preview1',
    args: [program, ...rest.slice(separator + 1)],
    env: {},
    preopens,
    returnOnExit: true,
});
const module = await WebAssembly.compile(await readFile(program));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
process.exitCode = wasi.start(instance);
