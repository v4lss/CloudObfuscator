# vite-plugin-cloudobfuscator

Vite and Rollup plugin that obfuscates the built chunks with
[CloudObfuscator](https://github.com/v4lss/CloudObfuscator).

The plugin runs in the `renderChunk` hook with `enforce: "post"`, so it receives
the final bundle that Vite produced, and it reuses a single long lived
`cloudobfuscator --server` process for the whole build. Source maps are dropped
for the transformed chunks because the obfuscated code no longer maps back to
the original sources.

## Install

```sh
npm i -D vite-plugin-cloudobfuscator
```

The plugin needs the `cloudobfuscator` binary. Build it from the repository:

```sh
cargo build --release -p cloudobfuscator-cli
```

## Usage

```js
// vite.config.js
import { defineConfig } from "vite";
import { cloudObfuscator } from "vite-plugin-cloudobfuscator";

export default defineConfig({
  plugins: [cloudObfuscator()],
});
```

```js
// rollup.config.js
import { cloudObfuscator } from "vite-plugin-cloudobfuscator";

export default {
  input: "src/main.js",
  plugins: [cloudObfuscator({ preset: "light" })],
};
```

## Options

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `binary` | `string` | auto detected | Path to the `cloudobfuscator` executable. |
| `config` | `object` | `{}` | Keys of `ObfuscationConfig` merged on top of the preset. |
| `seed` | `number` | random | Seed for reproducible builds. |
| `preset` | `none \| light \| balanced \| strong` | Rust defaults | Preset applied by the server process. |
| `minify` | `boolean` | `true` | Minify the obfuscated chunk. |
| `include` | `RegExp \| RegExp[]` | `/\.[cm]?js$/` | Chunk names to obfuscate. |
| `exclude` | `RegExp \| RegExp[]` | `node_modules`, `*.min.js` | Chunk names to skip. |
| `onReport` | `(report) => void` | – | Called with the report of every chunk. |

`config` only overrides the keys you set, so `preset: "strong"` plus
`config: { minify_output: false }` keeps the strong preset and turns
minification off.

## Binary resolution

The plugin looks for the executable in this order:

1. the `binary` option;
2. the `CLOUDOBFUSCATOR_BIN` environment variable;
3. `target/release` and `target/debug` in the current directory and every parent
   directory, which covers this repository and monorepo checkouts;
4. `node_modules/.bin/cloudobfuscator` in the same directories;
5. `cloudobfuscator` on `PATH`.

If you bundle the binary yourself, point `binary` at it:

```js
cloudObfuscator({ binary: "./vendor/cloudobfuscator.exe" });
```

## Reports

The plugin instance exposes every report it collected during the build:

```js
const plugin = cloudObfuscator();
export default { plugins: [plugin] };

// after `vite build`
console.log(plugin.reports());
```

## Development

```sh
cargo build -p cloudobfuscator-cli
cd packages/vite-plugin-cloudobfuscator
npm install
npm test
```

The tests drive the real binary, including a Vite build whose output is executed
with Node to prove that the bundle still behaves the same.
