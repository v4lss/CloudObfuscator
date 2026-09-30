#!/usr/bin/env node
import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, inflateRawSync } from "node:zlib";

import { platformKey } from "../worker.mjs";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const PACKAGE_DIR = join(dirname(SCRIPT_PATH), "..");
const PACKAGE = JSON.parse(readFileSync(join(PACKAGE_DIR, "package.json"), "utf8"));

export const REPO = "v4lss/CloudObfuscator";

function expectElf(machine, label) {
  return (bytes) => {
    assert.ok(bytes.length > 20, `${label}: file is truncated`);
    assert.equal(bytes.subarray(0, 4).toString("latin1"), "\u007fELF", `${label}: not an ELF binary`);
    assert.equal(bytes[4], 2, `${label}: expected a 64-bit ELF binary`);
    assert.equal(bytes[5], 1, `${label}: expected a little-endian ELF binary`);
    assert.equal(
      bytes.readUInt16LE(18),
      machine,
      `${label}: expected e_machine ${machine}, found ${bytes.readUInt16LE(18)}`,
    );
  };
}

function expectMachO(cpuType, label) {
  return (bytes) => {
    assert.ok(bytes.length > 8, `${label}: file is truncated`);
    assert.equal(
      bytes.readUInt32LE(0),
      0xfeedfacf,
      `${label}: not a 64-bit little-endian Mach-O binary`,
    );
    assert.equal(
      bytes.readInt32LE(4),
      cpuType,
      `${label}: expected cputype ${cpuType}, found ${bytes.readInt32LE(4)}`,
    );
  };
}

function expectPe(label) {
  return (bytes) => {
    assert.ok(bytes.length > 0x40, `${label}: file is truncated`);
    assert.equal(bytes.subarray(0, 2).toString("latin1"), "MZ", `${label}: not a PE binary`);
    const signature = bytes.readUInt32LE(0x3c);
    assert.equal(
      bytes.readUInt32LE(signature),
      0x00004550,
      `${label}: missing the PE signature`,
    );
    assert.equal(
      bytes.readUInt16LE(signature + 4),
      0x8664,
      `${label}: expected a 64-bit PE image`,
    );
  };
}

export const TARGETS = [
  {
    key: "linux-x64",
    asset: "cloudobfuscator-linux-x86_64.tar.gz",
    format: "tar.gz",
    file: "cloudobfuscator",
    verify: expectElf(0x3e, "linux-x64"),
  },
  {
    key: "linux-arm64",
    asset: "cloudobfuscator-linux-aarch64.tar.gz",
    format: "tar.gz",
    file: "cloudobfuscator",
    verify: expectElf(0xb7, "linux-arm64"),
  },
  {
    key: "darwin-x64",
    asset: "cloudobfuscator-macos-x86_64.tar.gz",
    format: "tar.gz",
    file: "cloudobfuscator",
    verify: expectMachO(0x01000007, "darwin-x64"),
  },
  {
    key: "darwin-arm64",
    asset: "cloudobfuscator-macos-aarch64.tar.gz",
    format: "tar.gz",
    file: "cloudobfuscator",
    verify: expectMachO(0x0100000c, "darwin-arm64"),
  },
  {
    key: "win32-x64",
    asset: "cloudobfuscator-windows-x86_64.zip",
    format: "zip",
    file: "cloudobfuscator.exe",
    verify: expectPe("win32-x64"),
  },
];

function readCString(block, start, length) {
  const slice = block.subarray(start, start + length);
  const end = slice.indexOf(0);
  return slice.subarray(0, end === -1 ? slice.length : end).toString("utf8");
}

function readOctal(block, start, length) {
  const text = readCString(block, start, length).trim();
  return text ? Number.parseInt(text, 8) : 0;
}

function normalize(name) {
  return name.replace(/^\.\//, "").replace(/^\/+/, "");
}

export function extractTarGz(archive) {
  const data = gunzipSync(archive);
  const files = new Map();
  let offset = 0;
  let longName = null;
  while (offset + 512 <= data.length) {
    const header = data.subarray(offset, offset + 512);
    offset += 512;
    if (header.every((byte) => byte === 0)) {
      break;
    }
    const size = readOctal(header, 124, 12);
    const type = String.fromCharCode(header[156]);
    const body = data.subarray(offset, offset + size);
    offset += Math.ceil(size / 512) * 512;
    if (type === "L") {
      longName = normalize(readCString(body, 0, body.length));
      continue;
    }
    const prefix = readCString(header, 345, 155);
    const name = normalize(
      longName ?? (prefix ? `${prefix}/${readCString(header, 0, 100)}` : readCString(header, 0, 100)),
    );
    longName = null;
    if (type === "0" || type === "\u0000" || type === "7") {
      files.set(name, Buffer.from(body));
    }
  }
  assert.ok(files.size > 0, "the tarball contains no files");
  return files;
}

export function extractZip(archive) {
  let end = -1;
  for (let cursor = archive.length - 22; cursor >= 0; cursor -= 1) {
    if (archive.readUInt32LE(cursor) === 0x06054b50) {
      end = cursor;
      break;
    }
  }
  assert.ok(end !== -1, "the file is not a zip archive");
  const count = archive.readUInt16LE(end + 10);
  let cursor = archive.readUInt32LE(end + 16);
  const files = new Map();
  for (let index = 0; index < count; index += 1) {
    assert.equal(archive.readUInt32LE(cursor), 0x02014b50, "corrupt zip central directory");
    const method = archive.readUInt16LE(cursor + 10);
    const compressed = archive.readUInt32LE(cursor + 20);
    const nameLength = archive.readUInt16LE(cursor + 28);
    const extraLength = archive.readUInt16LE(cursor + 30);
    const commentLength = archive.readUInt16LE(cursor + 32);
    const local = archive.readUInt32LE(cursor + 42);
    const name = normalize(
      archive.subarray(cursor + 46, cursor + 46 + nameLength).toString("utf8"),
    );
    cursor += 46 + nameLength + extraLength + commentLength;
    if (name.endsWith("/")) {
      continue;
    }
    assert.equal(archive.readUInt32LE(local), 0x04034b50, "corrupt zip local header");
    const start =
      local + 30 + archive.readUInt16LE(local + 26) + archive.readUInt16LE(local + 28);
    const raw = archive.subarray(start, start + compressed);
    let body;
    if (method === 0) {
      body = Buffer.from(raw);
    } else if (method === 8) {
      body = inflateRawSync(raw);
    } else {
      assert.fail(`unsupported zip compression method ${method}`);
    }
    files.set(name, body);
  }
  assert.ok(files.size > 0, "the zip archive contains no files");
  return files;
}

function pick(files, file) {
  if (files.has(file)) {
    return files.get(file);
  }
  const names = [...files.keys()];
  assert.equal(
    names.length,
    1,
    `expected ${file} in the archive, found ${names.join(", ")}`,
  );
  return files.get(names[0]);
}

function readFlag(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
}

export function releaseToken() {
  const token =
    process.env.CLOUDOBFUSCATOR_RELEASE_TOKEN ??
    process.env.GITHUB_TOKEN ??
    process.env.GH_TOKEN ??
    null;
  assert.ok(
    token,
    "a GitHub token is required: set GITHUB_TOKEN or GH_TOKEN, or pass an existing token in CLOUDOBFUSCATOR_RELEASE_TOKEN",
  );
  return token;
}

async function releaseAssets(token, tag, optional) {
  const response = await fetch(
    `https://api.github.com/repos/${REPO}/releases/tags/${encodeURIComponent(tag)}`,
    { headers: { authorization: `Bearer ${token}`, accept: "application/vnd.github+json" } },
  );
  if (response.status === 404 && optional) {
    return null;
  }
  assert.equal(response.status, 200, `release ${tag} not found (HTTP ${response.status})`);
  const release = await response.json();
  return new Map(release.assets.map((asset) => [asset.name, asset]));
}

async function download(token, asset) {
  const response = await fetch(asset.url, {
    headers: { authorization: `Bearer ${token}`, accept: "application/octet-stream" },
  });
  assert.equal(response.status, 200, `download failed for ${asset.name} (HTTP ${response.status})`);
  const bytes = Buffer.from(await response.arrayBuffer());
  assert.equal(
    bytes.length,
    asset.size,
    `${asset.name}: expected ${asset.size} bytes, received ${bytes.length}`,
  );
  return bytes;
}

export async function main() {
  const tag = readFlag("--tag") ?? `v${PACKAGE.version}`;
  const only = readFlag("--only") ?? platformKey();
  const force = process.argv.includes("--force");
  const optional = process.argv.includes("--if-released");
  const targets = TARGETS.filter((target) => only === null || target.key === only);
  assert.ok(targets.length > 0, `no release target matches ${only}`);

  const token = releaseToken();
  const assets = await releaseAssets(token, tag, optional);
  if (assets === null) {
    process.stdout.write(`release ${tag} does not exist yet, nothing to package\n`);
    return;
  }
  for (const target of targets) {
    const destination = join(PACKAGE_DIR, "prebuilt", target.key, target.file);
    if (!force && existsSync(destination)) {
      process.stdout.write(`${target.key}: already present, skipping\n`);
      continue;
    }
    const asset = assets.get(target.asset);
    assert.ok(asset, `release ${tag} has no asset named ${target.asset}`);
    const archive = await download(token, asset);
    const files = target.format === "zip" ? extractZip(archive) : extractTarGz(archive);
    const binary = pick(files, target.file);
    target.verify(binary);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, binary);
    if (process.platform !== "win32") {
      chmodSync(destination, 0o755);
    }
    process.stdout.write(`${target.key}: ${binary.length} bytes from ${target.asset}\n`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT_PATH) {
  main().catch((error) => {
    process.stderr.write(`fetch-prebuilt: ${error.message}\n`);
    process.exitCode = 1;
  });
}
