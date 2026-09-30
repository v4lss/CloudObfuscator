# CloudObfuscator

AST based JavaScript and TypeScript obfuscator written in Rust, with a
command line interface and a Vite/Rollup plugin.

Every transform works on the syntax tree produced by [swc](https://swc.rs/), not
on the source text, so nothing depends on formatting and the output keeps
running the same. The test suite executes the obfuscated bundles with Node and
compares their output with the original.

## Install

```sh
cargo install cloudobfuscator-cli
```

Every [release](https://github.com/v4lss/CloudObfuscator/releases) also attaches
prebuilt archives for Linux, macOS and Windows, and the Vite plugin bundles the
binary for every supported platform, so a build never needs to compile the
toolchain first.

Building from source needs Rust 1.85 or newer:

```sh
cargo build --release -p cloudobfuscator-cli
```

## Command line

```sh
cloudobfuscator src/app.js -o dist/app.js
cloudobfuscator dist --out-dir build --preset strong
cloudobfuscator src --preset light --seed 1234 --report report.json
```

| Flag | Description |
| --- | --- |
| `-o`, `--output <FILE>` | Write to one file. Only valid with a single input. |
| `--out-dir <DIR>` | Write into a directory, keeping the input layout. |
| `-c`, `--config <FILE>` | Config file, `.json`, `.yml` or `.yaml`. A UTF-8 BOM is tolerated. |
| `-p`, `--preset <NAME>` | `none`, `light`, `balanced` or `strong`. |
| `--seed <NUMBER>` | Makes the output reproducible. |
| `--no-minify` | Keep the output readable. |
| `--report <FILE>` | Write a JSON report with one entry per file. |
| `-q`, `--quiet` | Do not print per file summaries. |

Inputs may be files or directories, and declaration files (`.d.ts`, `.d.mts`,
`.d.cts`) are skipped. Several inputs require `--out-dir`, so the output can
never overwrite your sources by accident.

## Presets

| Preset | What it turns on |
| --- | --- |
| `none` | Disables obfuscation, useful as a passthrough. |
| `light` | String encoding only, no control flow and no number encoding. |
| `balanced` | The defaults: strings, numbers, identifiers, opaque predicates. |
| `strong` | Adds the runtime guards, property mangling and aggressive control flow. |

## Configuration

Every field of the table below can appear in a JSON or YAML config. Unknown
keys are rejected so a typo never passes silently.

```json
{
  "enabled": true,
  "seed": 1234,
  "target": "browser",
  "minify_output": true,
  "debug_protection": false,
  "self_defending": false,
  "console_trap": false,
  "integrity_check": false,
  "string_encoding": "mixed",
  "string_threshold": 8,
  "string_group_count": 3,
  "max_string_table_entries": 30000,
  "rename_identifiers": true,
  "identifier_style": "random",
  "mangle_properties": false,
  "property_mode": "safe",
  "control_flow": "safe",
  "opaque_predicates": true,
  "obfuscate_numbers": true,
  "string_array_threshold": 0,
  "string_array_group_count": 1,
  "domain_appenders": false
}
```

`target` accepts `browser`, `node` and `web-worker`. `string_encoding` accepts
`xor-chain`, `base64-custom`, `rotate-array`, `reverse-shift`, `split-halves`
and `mixed`. `identifier_style` accepts `random`, `hex`, `alpha`, `mixed`,
`unicode` and `short`. `control_flow` accepts `off`, `safe` and `aggressive`,
and `property_mode` accepts `off`, `safe` and `aggressive`.

## What it does

- **Strings** are moved into decoder tables and replaced with calls, so the
  literals never appear in the bundle. Object keys, template literals, import
  specifiers and the directive prologue are left alone on purpose.
- **Numbers** are rebuilt from arithmetic at runtime.
- **Identifiers** are renamed per run, optionally using hex, alphabetic,
  unicode or short names. Globals and reserved names are never touched.
- **Control flow** is flattened and mixed with opaque predicates.
- **Properties** can be renamed with a safe strategy that keeps the builtins
  and the DOM members reachable.
- **Runtime guards** add debugger traps, a console trap, a self defending
  check and an integrity check. They clean themselves up so they never keep
  the Node event loop alive.
- **Determinism**: the same input, config and seed always produce byte for byte
  the same output.

## Vite and Rollup

```js
// vite.config.js
import { defineConfig } from "vite";
import { cloudObfuscator } from "vite-plugin-cloudobfuscator";

export default defineConfig({
  plugins: [cloudObfuscator({ preset: "light" })],
});
```

See the [plugin README](packages/vite-plugin-cloudobfuscator/README.md) for the
options, the binary lookup order and how the packaged binaries are refreshed.
The plugin reuses a single `cloudobfuscator --server` process for the whole
build.

## Server mode

`cloudobfuscator --server` reads one JSON object per line on stdin and answers
one JSON object per line on stdout, which is how the plugin avoids paying the
process startup for every chunk.

```json
{"id":1,"file":"app.js","code":"const a = 1;","config":{"minify_output":true}}
{"id":1,"ok":true,"code":"...","report":{}}
```

`config` is merged on top of the config the server built from its own flags, so
`--preset strong` still applies and the keys you send win. Errors are reported
per request and never take the process down.

## Workspace

| Crate | Responsibility |
| --- | --- |
| `cloudobfuscator-parser` | Parses JavaScript and TypeScript into an swc AST. |
| `cloudobfuscator-analysis` | Collects bindings, strings, numbers, globals and property usage. |
| `cloudobfuscator-transform` | The individual passes and the pipeline that runs them. |
| `cloudobfuscator-runtime` | The injected JavaScript runtime, string decoders and guards. |
| `cloudobfuscator-core` | Configuration, presets, pipeline orchestration and reports. |
| `cloudobfuscator-cli` | The command line interface and the server mode. |

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd packages/vite-plugin-cloudobfuscator
npm install
npm test
```

On Windows with the GNU toolchain, copy `.cargo/config.toml.example` to
`.cargo/config.toml` and point it at your MinGW-w64 `gcc.exe`. The plugin tests
need `cargo build -p cloudobfuscator-cli` first, because they drive the real
binary.

## Publishing

`scripts/publish.mjs` runs a preflight and then publishes the plugin to npm and
the six crates to crates.io, in dependency order, waiting for the registry index
to catch up between crates:

```sh
npm adduser          # once
cargo login          # once
export GITHUB_TOKEN=...   # the repository is private, packing needs it

node scripts/publish.mjs --dry-run   # preflight only
node scripts/publish.mjs
node scripts/publish.mjs --only npm
node scripts/publish.mjs --only crates --fast
```

The preflight refuses to publish when the tree is dirty, when the manifests
disagree on the version, when a GitHub token is missing, when the tarball would
not carry all five platform binaries, or when the crates stop packaging. Because
the crates depend on each other, they have to go out in this order:

```text
parser -> runtime -> analysis -> transform -> core -> cli
```

`node scripts/publish-check.mjs` exercises the index and published-version
detection against crates and packages that already exist, so the wait logic is
tested without publishing anything. It needs network access, which is why it is
not part of `npm test`.

## License

MIT.
