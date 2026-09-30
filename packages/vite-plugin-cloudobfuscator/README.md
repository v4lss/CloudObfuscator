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

The package ships the native binary for every supported platform, so nothing
else is needed. The binaries live in `prebuilt/<platform>` and the plugin picks
the one that matches the current host. Because all five are bundled, the
tarball is around 8 MB.

Supported hosts are Linux and macOS on x64 and arm64, plus Windows on x64.
Anything else has to provide its own `cloudobfuscator` on `PATH` or through the
`binary` option, which you can also build from source:

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
3. the binary bundled with this package for the current platform;
4. `target/release` and `target/debug` in the current directory and every parent
   directory, which covers this repository and monorepo checkouts;
5. `node_modules/.bin/cloudobfuscator` in the same directories;
6. `cloudobfuscator` on `PATH`.

To pin a different build, for example one you compiled against your own
toolchain, point `binary` at it:

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

`prebuilt/` is not tracked in git. To refresh it from a published release you
need a GitHub token and a matching tag:

```sh
GITHUB_TOKEN=... npm run fetch:prebuilt            # every platform
GITHUB_TOKEN=... npm run fetch:prebuilt -- --only linux-x64
GITHUB_TOKEN=... npm run fetch:prebuilt -- --force # re-download
```

`npm publish` runs the same script with `--force` so a release can never ship
without the binaries.
