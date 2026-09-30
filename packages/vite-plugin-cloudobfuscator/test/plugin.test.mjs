import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { deflateRawSync, gzipSync } from "node:zlib";

import { cloudObfuscator } from "../index.mjs";
import { EXECUTABLE, ObfuscatorWorker, platformKey, resolveBinary } from "../worker.mjs";
import { TARGETS, extractTarGz, extractZip } from "../scripts/fetch-prebuilt.mjs";

const workspaceRoot = resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const binary = resolveBinary({ binary: join(workspaceRoot, "target", "debug", EXECUTABLE) });

function scratch() {
  return mkdtempSync(join(tmpdir(), "cloudobfuscator-vite-"));
}

function octal(value, width) {
  return `${value.toString(8).padStart(width - 1, "0")}\0`;
}

function tarHeader(name, size, type = "0") {
  const header = Buffer.alloc(512);
  header.write(name, 0, 100, "utf8");
  header.write("000644 \0", 100, 8, "utf8");
  header.write(octal(0, 8), 108, 8, "utf8");
  header.write(octal(0, 8), 116, 8, "utf8");
  header.write(octal(size, 12), 124, 12, "utf8");
  header.write(octal(0, 12), 136, 12, "utf8");
  header.write("        ", 148, 8, "utf8");
  header.write(type, 156, 1, "utf8");
  header.write("ustar\0", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");
  let sum = 0;
  for (const byte of header) {
    sum += byte;
  }
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "utf8");
  return header;
}

function makeTarGz(entries) {
  const blocks = [];
  for (const [name, body] of entries) {
    if (body === null) {
      blocks.push(tarHeader(name, 0, "5"));
      continue;
    }
    const payload = Buffer.from(body, "utf8");
    blocks.push(
      tarHeader(name, payload.length),
      payload,
      Buffer.alloc((512 - (payload.length % 512)) % 512),
    );
  }
  blocks.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(blocks));
}

function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function makeZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, body] of entries) {
    const nameBytes = Buffer.from(name, "utf8");
    const payload = Buffer.from(body, "utf8");
    const deflated = deflateRawSync(payload);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc32(payload), 14);
    local.writeUInt32LE(deflated.length, 18);
    local.writeUInt32LE(payload.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, deflated);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(8, 10);
    central.writeUInt32LE(crc32(payload), 16);
    central.writeUInt32LE(deflated.length, 20);
    central.writeUInt32LE(payload.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBytes);
    offset += local.length + nameBytes.length + deflated.length;
  }
  const localBlock = Buffer.concat(locals);
  const centralBlock = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBlock.length, 12);
  end.writeUInt32LE(localBlock.length, 16);
  return Buffer.concat([localBlock, centralBlock, end]);
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
    assert.equal(binary, join(workspaceRoot, "target", "debug", EXECUTABLE));
  });

  it("fails loudly when the binary is missing", () => {
    assert.throws(
      () => resolveBinary({ binary: join(workspaceRoot, "target", "debug", "not-here-anywhere") }),
      /binary not found/,
    );
  });

  it("discovers the debug build from the working directory", () => {
    assert.equal(resolveBinary({ packageDir: scratch() }), binary);
  });

  it("maps the supported platforms to release keys", () => {
    assert.equal(platformKey("linux", "x64"), "linux-x64");
    assert.equal(platformKey("linux", "arm64"), "linux-arm64");
    assert.equal(platformKey("darwin", "x64"), "darwin-x64");
    assert.equal(platformKey("darwin", "arm64"), "darwin-arm64");
    assert.equal(platformKey("win32", "x64"), "win32-x64");
  });

  it("refuses to guess on unsupported platforms", () => {
    assert.equal(platformKey("linux", "ia32"), null);
    assert.equal(platformKey("freebsd", "x64"), null);
    assert.equal(platformKey("win32", "arm64"), null);
  });

  it("covers every release target with a resolvable key", () => {
    const expected = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-x64"];
    assert.deepEqual(
      TARGETS.map((target) => target.key).sort(),
      expected,
    );
  });

  it("prefers the packaged binary over the local build", () => {
    const dir = scratch();
    const packaged = join(dir, "prebuilt", platformKey(), EXECUTABLE);
    mkdirSync(join(dir, "prebuilt", platformKey()), { recursive: true });
    writeFileSync(packaged, "not a real binary");
    try {
      assert.equal(resolveBinary({ packageDir: dir }), packaged);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
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

describe("release archives", () => {
  it("reads a gzipped tar and drops the ./ prefix", () => {
    const files = extractTarGz(makeTarGz([["./", null], ["./cloudobfuscator", "ELF-ish"]]));
    assert.deepEqual([...files.keys()], ["cloudobfuscator"]);
    assert.equal(files.get("cloudobfuscator").toString(), "ELF-ish");
  });

  it("reads a deflated zip", () => {
    const body = Buffer.from("MZ binary payload", "utf8");
    const files = extractZip(makeZip([["cloudobfuscator.exe", body]]));
    assert.equal(files.get("cloudobfuscator.exe").toString(), body.toString());
  });

  it("rejects something that is not an archive", () => {
    assert.throws(() => extractZip(Buffer.alloc(64)), /not a zip archive/);
    assert.throws(() => extractTarGz(Buffer.alloc(64)), /incorrect header check/);
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
