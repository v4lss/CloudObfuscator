import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import { cloudObfuscator } from "../index.mjs";
import { ObfuscatorWorker, resolveBinary } from "../worker.mjs";

const workspaceRoot = resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const binary = resolveBinary({ binary: join(workspaceRoot, "target", "debug", "cloudobfuscator.exe") });

function scratch() {
  return mkdtempSync(join(tmpdir(), "cloudobfuscator-vite-"));
}

async function runChunk(code, fileName, options = {}) {
  const plugin = cloudObfuscator({ binary, ...options });
  await plugin.buildStart();
  try {
    const result = await plugin.renderChunk(code, { fileName });
    return { result, reports: plugin.reports() };
  } finally {
    await plugin.closeBundle();
  }
}

describe("binary discovery", () => {
  it("finds the debug build inside the workspace", () => {
    assert.ok(binary.endsWith("cloudobfuscator.exe"));
  });

  it("fails loudly when the binary is missing", () => {
    assert.throws(
      () => resolveBinary({ binary: join(workspaceRoot, "target", "debug", "not-here.exe") }),
      /binary not found/,
    );
  });

  it("discovers the debug build from the working directory", () => {
    const discovered = resolveBinary();
    assert.ok(discovered.endsWith("cloudobfuscator.exe"), `unexpected binary ${discovered}`);
  });
});

describe("worker protocol", () => {
  it("answers one reply per request and keeps ids aligned", async () => {
    const worker = new ObfuscatorWorker(binary, []).start();
    try {
      const [first, second] = await Promise.all([
        worker.request("one.js", 'const a = "first request string";', null),
        worker.request("two.js", 'const b = "second request string";', null),
      ]);
      assert.equal(first.ok, true);
      assert.equal(second.ok, true);
      assert.notEqual(first.id, second.id);
      assert.ok(!first.code.includes("first request string"));
      assert.ok(!second.code.includes("second request string"));
    } finally {
      await worker.close();
    }
  });

  it("surfaces parse errors instead of hanging", async () => {
    const worker = new ObfuscatorWorker(binary, []).start();
    try {
      await assert.rejects(worker.request("bad.js", "function ( { oops", null), /failed to parse/);
    } finally {
      await worker.close();
    }
  });

  it("rejects every pending request when the process dies", async () => {
    const worker = new ObfuscatorWorker(binary, []).start();
    const pending = worker.request("one.js", "const a = 1;", null);
    worker.child.kill();
    await assert.rejects(pending, /exited with code|EPIPE/);
    await worker.close();
  });

  it("refuses requests after the worker was closed", async () => {
    const worker = new ObfuscatorWorker(binary, []).start();
    await worker.close();
    assert.throws(() => worker.request("one.js", "const a = 1;", null), /not started/);
  });
});

describe("renderChunk", () => {
  it("obfuscates matching chunks and reports them", async () => {
    const { result, reports } = await runChunk(
      'const greeting = "hello from the vite plugin";\nconsole.log(greeting);\n',
      "assets/index-abc123.js",
    );
    assert.ok(result.code.length > 0);
    assert.ok(!result.code.includes("hello from the vite plugin"));
    assert.equal(reports.length, 1);
    assert.equal(reports[0].file, "assets/index-abc123.js");
    assert.ok(reports[0].output_bytes > 0);
  });

  it("skips chunks that do not match the filters", async () => {
    const { result, reports } = await runChunk('const a = "untouched chunk value";', "assets/style.css");
    assert.equal(result, null);
    assert.equal(reports.length, 0);
  });

  it("respects a custom include pattern", async () => {
    const { result } = await runChunk('const a = "custom include string";', "assets/thing.mjs", {
      include: /\.mjs$/,
    });
    assert.ok(result.code.length > 0);
    assert.ok(!result.code.includes("custom include string"));
  });

  it("honours the node_modules exclusion", async () => {
    const { result } = await runChunk('const a = "vendored string value";', "node_modules/dep/index.js");
    assert.equal(result, null);
  });

  it("passes the config through so enabled false leaves the code readable", async () => {
    const { result } = await runChunk(
      'const keep = "readable passthrough value";',
      "assets/plain.js",
      { config: { enabled: false } },
    );
    assert.ok(result.code.includes("readable passthrough value"));
  });

  it("honours the preset from the server arguments", async () => {
    const { result } = await runChunk('const keep = "preset none passthrough";', "assets/preset.js", {
      preset: "none",
    });
    assert.ok(result.code.includes("preset none passthrough"));
  });

  it("lets request options win over the preset", async () => {
    const { result } = await runChunk('const keep = "preset override check";', "assets/override.js", {
      preset: "none",
      config: { enabled: true },
    });
    assert.ok(!result.code.includes("preset override check"));
  });
});

describe("end to end with vite", () => {
  let vite;

  it("builds a real project and the bundle stays executable", async (t) => {
    try {
      vite = await import("vite");
    } catch {
      t.skip("vite is not installed");
      return;
    }

    const dir = scratch();
    const cleanup = () => rmSync(dir, { recursive: true, force: true });
    after(cleanup);

    mkdirSync(join(dir, "src"), { recursive: true });
    writeFileSync(
      join(dir, "src", "main.js"),
      'import { add } from "./math.js";\nconst label = "bundle output check";\nconsole.log(label, add(20, 22));\n',
    );
    writeFileSync(join(dir, "src", "math.js"), "export const add = (a, b) => a + b;\n");

    const outDir = join(dir, "dist");
    await vite.build({
      root: dir,
      logLevel: "error",
      plugins: [cloudObfuscator({ binary, preset: "light" })],
      build: {
        outDir,
        minify: false,
        lib: {
          entry: join(dir, "src", "main.js"),
          formats: ["es"],
          fileName: () => "bundle.js",
        },
      },
    });

    const bundle = readFileSync(join(outDir, "bundle.js"), "utf8");
    assert.ok(!bundle.includes("bundle output check"), "the label string survived obfuscation");
    assert.ok(bundle.length > 0, "the bundle is empty");

    const { execFileSync } = await import("node:child_process");
    const printed = execFileSync(process.execPath, [join(outDir, "bundle.js")], { encoding: "utf8" });
    assert.equal(printed.trim(), "bundle output check 42", "the obfuscated bundle changed behaviour");
  });
});
