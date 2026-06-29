# omnilua

`omnilua` runs Lua in the browser or Node. It's a pure-Rust Lua runtime compiled
to WebAssembly, so you ship one `.wasm` file and a small JS wrapper — no C
interpreter to bundle and no native build step. It runs the same Lua as the
native crate.

A C-backed Lua binding can't compile to `wasm32-unknown-unknown`. omnilua is
pure Rust, so it does, with no Emscripten.

## Install

```bash
npm install omnilua
```

The package ships the `.wasm` file and an ES-module wrapper. There's no
postinstall build and no native dependency.

## Use it

Give the runtime a host environment (virtual files, env vars, a stdout sink),
then run Lua source through it. The runtime keeps one Lua state alive across
`exec` calls until you `reset()`.

```js
import { loadLuaRs, luaRsWasmUrl } from "omnilua";

const { lua } = await loadLuaRs(luaRsWasmUrl, {
  files: {
    "./greeter.lua":
      "return { message = function(name) return 'hello ' .. name end }",
  },
  onStdout: (chunk) => console.log(chunk),
});

lua.exec(`
  local greeter = require("greeter")
  print(greeter.message("wasm"))
`);
```

`exec` throws on a Lua error. To inspect the failure instead of catching an
exception, use `tryExec`:

```js
const result = lua.tryExec('error("boom")');
console.log(result.ok);    // false
console.log(result.error); // the Lua error text
```

In Node without a bundler, use the `/node` entry point. It reads the packaged
`.wasm` and otherwise behaves the same:

```js
import { loadLuaRsNode } from "omnilua/node";

const { lua } = await loadLuaRsNode({
  onStdout: (chunk) => process.stdout.write(chunk),
});
lua.exec('print("hello from node")');
```

## Running untrusted scripts

Set CPU and memory limits and strip host access before running scripts you don't
trust. The limits apply to every thread, including coroutines, and can't be
caught with `pcall`. Call `setLimits` once, then run as usual. `lastTrip` reports
which limit stopped a run, and `sandboxReset` refills the budget.

```js
lua.setLimits({
  maxInstructions: 5_000_000,
  maxMemory: 64 * 1024 * 1024,
  strict: true, // also remove os.execute, io, load, require, debug, …
});

const result = lua.tryExec("while true do end"); // runaway user script
console.log(result.ok);       // false
console.log(lua.lastTrip());  // "instructions"  ("memory" | null)

lua.sandboxReset(); // refill the budget for the next run
```

Omit a limit (or pass `0`) to leave that dimension unbounded.

## Choosing a Lua version

All five versions — 5.1, 5.2, 5.3, 5.4, and 5.5 — ship in the one `.wasm` file.
Pick the version when you load it; there's no second download and no recompile:

```js
import { loadLuaRs, luaRsWasmUrl } from "omnilua";

const { lua: lua51 } = await loadLuaRs(luaRsWasmUrl, { version: "5.1" });
const { lua: lua54 } = await loadLuaRs(luaRsWasmUrl, { version: "5.4" });

lua51.tryExec("print(3 / 3)"); // 1     (5.1 has no integer type)
lua54.tryExec("print(3 / 3)"); // 1.0   (5.4 has integers)
```

`version` accepts `"5.1"` through `"5.5"`; the default is `"5.4"`.
`lua.setVersion("5.2")` switches an existing runtime, resetting its state, and
`lua.currentVersion()` reports the current one. The version also sets the
standard-library roster: `bit32` is 5.2 only, `utf8` and `string.pack` are 5.3+,
and so on. The [playground](https://ianm199.github.io/omnilua/) uses this API to
run one snippet across all five versions.

## Size

You ship one WebAssembly module — lexer, parser, VM, GC, and standard library,
about 1.16 MB — plus a few kilobytes of JS. There's no Emscripten glue and no
separate `liblua`. Serve the `.wasm` with `Content-Type: application/wasm` and
gzip or brotli, and the browser stream-compiles it; `loadLuaRs(luaRsWasmUrl)`
fetches it for you.

## Links

- Source, issues, full docs:
  [github.com/ianm199/omnilua](https://github.com/ianm199/omnilua)
- Live playground (all five Lua versions):
  [ianm199.github.io/omnilua](https://ianm199.github.io/omnilua/)
- Embedding in Rust (the native crate):
  [`omnilua` on crates.io](https://crates.io/crates/omnilua)

## License

A port of [Lua](https://www.lua.org/) (PUC-Rio). Lua and this port are both
MIT licensed.
