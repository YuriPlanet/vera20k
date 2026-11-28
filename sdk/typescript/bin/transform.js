// The compiler transform `vera20k build` runs. After parsing it
// - collects every `namedAction("Name", fn)` name for `vera.actions`, and
//   refuses a name that is not a string literal;
// - keeps only the lifecycle exports the module's own sources register
//   something for: `on_frame` only with `onFrame`, `on_events` only with a
//   handler that can receive records, and so on.
import { Transform } from "assemblyscript/transform";

// Which export each registration needs (sim, then client).
const NEEDS = {
  on_scenario_start: [/\bonScenarioStart\s*\(/],
  on_rules: [/\bonRules\s*\(/],
  on_frame: [/\bonFrame\s*\(/],
  on_events: [/\.\s*enqueue(With)?\s*[<(]/, /\bnamedAction\s*[<(]/, /\bActionDef\s*\.\s*module\s*[<(]/, /\bIni\s*\.\s*(key|registry)\s*[<(]/],
  // A named action reaches `on_decide` only as an INI `Decide=` handler,
  // which takes a decision view (see `namedDecider` below).
  on_decide: [/\.\s*decideSync\s*[<(]/],
  on_message: [/\bonMessage\s*\(/],
  client_on_start: [/\bonStart\s*\(/],
  client_on_render: [/\bonRender\s*\(/],
  client_on_events: [/\bonClientEvent\s*[<(]/],
};

const NAMED = /\bnamedAction\s*(?:<[^>()]*>)?\s*\(\s*([^,)]*)/g;

export default class VeraTransform extends Transform {
  afterParse(parser) {
    const state = globalThis.__vera20kBuild;
    if (!state) return;
    const isExports = (s) => s.normalizedPath === state.exportsPath;
    const own = parser.sources.filter((s) => !s.normalizedPath.startsWith("~lib/") && !isExports(s));
    const text = own.map((s) => stripComments(s.text)).join("\n");
    for (const source of own) {
      const body = stripComments(source.text);
      for (const m of body.matchAll(NAMED)) {
        const arg = m[1].trim();
        const literal = arg.match(/^"([^"\\]*)"$/) ?? arg.match(/^'([^'\\]*)'$/);
        if (!literal) {
          const line = body.slice(0, m.index).split("\n").length;
          // Reported by `vera20k build` as its own error, without a stack.
          state.found.error ??= `${source.normalizedPath}:${line}: namedAction's name must be a string literal, not ${arg}`;
          continue;
        }
        if (!state.found.actions.includes(literal[1])) state.found.actions.push(literal[1]);
      }
    }
    for (const [name, patterns] of Object.entries(NEEDS)) {
      if (patterns.some((p) => p.test(text))) state.found.uses.add(name);
    }
    if (namedDecider(text)) state.found.uses.add("on_decide");
    for (const source of parser.sources) {
      if (!isExports(source)) continue;
      for (const statement of source.statements) {
        const name = statement.name?.text;
        if (name && NEEDS[name] && !state.found.uses.has(name)) statement.flags &= ~2; // CommonFlags.Export
      }
    }
  }
}

/**
 * Whether a `namedAction("Name", fn)` names a function whose parameter is a
 * decision view (`…Decision` or `DecideRecord`), which INI can bind as
 * `Decide=Wasm:Name`.
 */
function namedDecider(text) {
  for (const m of text.matchAll(/\bnamedAction\s*(?:<\s*(\w+)\s*>)?\s*\(\s*["'][^"']*["']\s*,\s*(\w+)/g)) {
    if (m[1] && /(Decision|DecideRecord)$/.test(m[1])) return true;
    const fn = m[2];
    const decl = new RegExp(`(?:function\\s+${fn}\\s*\\(|\\b${fn}\\s*=\\s*\\()\\s*\\w+\\s*:\\s*(\\w+)`);
    const d = text.match(decl);
    if (d && /(Decision|DecideRecord)$/.test(d[1])) return true;
  }
  return false;
}

class VeraError extends Error {
  constructor(message) {
    super(message);
    this.vera = true;
  }
}

/** Drops comments so a commented-out registration needs no export. Strings stay. */
function stripComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " ")).replace(/(^|[^:"'`\\])\/\/[^\n]*/g, "$1");
}
