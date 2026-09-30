#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(import.meta.url), "..", "..");
const PLUGIN_DIR = join(ROOT, "packages", "vite-plugin-cloudobfuscator");
const NPM_NAME = "vite-plugin-cloudobfuscator";

export const CRATES = [
  "cloudobfuscator-parser",
  "cloudobfuscator-runtime",
  "cloudobfuscator-analysis",
  "cloudobfuscator-transform",
  "cloudobfuscator-core",
  "cloudobfuscator-cli",
];

const PLATFORMS = [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-x64",
];

const isWindows = process.platform === "win32";
const npm = isWindows ? "npm.cmd" : "npm";

function commandFor(name) {
  if (isWindows && /\.(cmd|bat)$/i.test(name)) {
    return { file: process.env.ComSpec ?? "cmd.exe", prefix: ["/d", "/s", "/c", name] };
  }
  return { file: name, prefix: [] };
}

function run(command, args, options = {}) {
  const { file, prefix } = commandFor(command);
  const result = spawnSync(file, [...prefix, ...args], {
    cwd: options.cwd ?? ROOT,
    encoding: "utf8",
    stdio: options.quiet ? "pipe" : "inherit",
  });
  if (result.error) {
    throw new Error(`${command} could not start: ${result.error.message}`);
  }
  return { status: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

function check(label, fn) {
  process.stdout.write(`  ${label} ... `);
  try {
    const detail = fn();
    process.stdout.write(`ok${detail ? ` (${detail})` : ""}\n`);
    return true;
  } catch (error) {
    process.stdout.write(`FAILED\n    ${error.message.split("\n").join("\n    ")}\n`);
    return false;
  }
}

function note(message) {
  process.stdout.write(`  ! ${message}\n`);
}

function workspaceVersion() {
  const manifest = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const match = manifest.match(/^\[workspace\.package\]\s*\nversion = "([^"]+)"/m);
  assert.ok(match, "could not read [workspace.package].version from Cargo.toml");
  return match[1];
}

function pluginVersion() {
  return JSON.parse(readFileSync(join(PLUGIN_DIR, "package.json"), "utf8")).version;
}

export async function isPublished(kind, name, version) {
  if (kind === "npm") {
    const result = run(npm, ["view", `${name}@${version}`, "version"], {
      quiet: true,
      cwd: PLUGIN_DIR,
    });
    return result.status === 0;
  }
  const response = await fetch(`https://crates.io/api/v1/crates/${name}/${version}`, {
    headers: { "User-Agent": `${name} publish preflight` },
  });
  return response.status === 200;
}

export function indexPath(name) {
  const lower = name.toLowerCase();
  return `https://index.crates.io/${lower.slice(0, 2)}/${lower.slice(2, 4)}/${lower}`;
}

export async function indexedVersions(name) {
  const response = await fetch(indexPath(name), {
    headers: { accept: "text/plain", "User-Agent": `${name} publish preflight` },
  });
  if (response.status !== 200) {
    return [];
  }
  return (await response.text())
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line).vers;
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

export async function waitForIndex(name, version, timeoutMs = 15 * 60 * 1000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const versions = await indexedVersions(name);
    if (versions.includes(version)) {
      return true;
    }
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      throw new Error(`${name} ${version} was not indexed within ${timeoutMs / 60000} minutes`);
    }
    process.stdout.write(`    waiting for the index to pick up ${name} ${version}\n`);
    await new Promise((done) => setTimeout(done, Math.min(15_000, remaining)));
  }
}

async function preflight({ fast }) {
  process.stdout.write("preflight\n");
  const version = workspaceVersion();
  const failures = [];

  const clean = check("the working tree is clean", () => {
    const result = run("git", ["status", "--porcelain"], { quiet: true });
    assert.equal(result.status, 0, "git status failed");
    if (result.stdout.trim()) {
      throw new Error(`uncommitted changes:\n${result.stdout.trim()}`);
    }
  });
  if (!clean) {
    failures.push("dirty tree");
  }

  if (!check(`every manifest says ${version}`, () => {
    assert.equal(pluginVersion(), version, `package.json says ${pluginVersion()}`);
  })) {
    failures.push("version mismatch");
  }

  if (!check("a GitHub token is available for the release download", () => {
    const token =
      process.env.CLOUDOBFUSCATOR_RELEASE_TOKEN ?? process.env.GITHUB_TOKEN ?? process.env.GH_TOKEN;
    assert.ok(
      token,
      "the repository is private, so packing needs a token: set GITHUB_TOKEN or GH_TOKEN",
    );
  })) {
    failures.push("no GitHub token");
  }

  if (!check("the release binaries are packaged", () => {
    const fetch = run("node", ["scripts/fetch-prebuilt.mjs", "--if-released", "--force"], {
      cwd: PLUGIN_DIR,
      quiet: true,
    });
    if (fetch.status !== 0) {
      throw new Error(`fetch-prebuilt failed: ${fetch.stderr.trim() || fetch.stdout.trim()}`);
    }
    const pack = run(npm, ["pack", "--dry-run", "--json"], { cwd: PLUGIN_DIR, quiet: true });
    if (pack.status !== 0) {
      throw new Error(`npm pack failed: ${pack.stderr.trim() || pack.stdout.trim()}`);
    }
    const [tarball] = JSON.parse(pack.stdout);
    const files = new Set(tarball.files.map((entry) => entry.path));
    for (const platform of PLATFORMS) {
      const file = platform === "win32-x64" ? "cloudobfuscator.exe" : "cloudobfuscator";
      assert.ok(
        files.has(`prebuilt/${platform}/${file}`),
        `the tarball is missing prebuilt/${platform}/${file}`,
      );
    }
    return `${tarball.entryCount} files, ${(tarball.size / 1e6).toFixed(1)} MB`;
  })) {
    failures.push("incomplete tarball");
  }

  if (!fast) {
    const verifiable = ["cloudobfuscator-parser", "cloudobfuscator-runtime"];
    if (!check("the crates without internal deps package and verify", () => {
      for (const crate of verifiable) {
        const result = run("cargo", ["package", "-p", crate], { quiet: true });
        if (result.status !== 0) {
          throw new Error(`${crate}: ${result.stderr.trim()}`);
        }
      }
      return verifiable.join(", ");
    })) {
      failures.push("cargo package");
    }
  }

  const tag = run("git", ["rev-parse", `v${version}`], { quiet: true });
  if (tag.status !== 0) {
    note(`there is no v${version} tag yet`);
  } else {
    const head = run("git", ["rev-parse", "HEAD"], { quiet: true });
    if (head.stdout.trim() !== tag.stdout.trim()) {
      const ahead = run("git", ["rev-list", "--count", `v${version}..HEAD`], { quiet: true });
      note(`HEAD is ${ahead.stdout.trim()} commits ahead of v${version}; make sure that is intended`);
    }
  }

  return { version, failures };
}

async function publishNpm(version) {
  process.stdout.write(`\nnpm: ${NPM_NAME}@${version}\n`);
  const login = run(npm, ["whoami"], { quiet: true, cwd: PLUGIN_DIR });
  if (login.status !== 0) {
    throw new Error(`not logged in to npm: run "npm adduser" first\n${login.stdout}${login.stderr}`);
  }
  const result = run(npm, ["publish"], { cwd: PLUGIN_DIR });
  assert.equal(result.status, 0, "npm publish failed");
}

async function publishCrates(version) {
  for (const [position, crate] of CRATES.entries()) {
    process.stdout.write(`\ncrates.io: ${crate} ${version}\n`);
    if (await isPublished("crates", crate, version)) {
      note("already published, making sure the index has it");
    } else {
      const result = run("cargo", ["publish", "-p", crate]);
      if (result.status !== 0) {
        throw new Error(`cargo publish failed for ${crate}; fix it and re-run, the order is safe`);
      }
    }
    if (position < CRATES.length - 1) {
      await waitForIndex(crate, version);
      process.stdout.write(`    ${crate} ${version} is indexed\n`);
    }
  }
}

async function main() {
  const args = process.argv.slice(2);
  const fast = args.includes("--fast");
  const only = args.includes("--only") ? args[args.indexOf("--only") + 1] : null;
  assert.ok(
    only === null || only === "npm" || only === "crates",
    "--only accepts npm or crates",
  );
  const dryRun = args.includes("--dry-run");

  const { version, failures } = await preflight({ fast });
  if (failures.length > 0) {
    process.stderr.write(`\npreflight failed: ${failures.join(", ")}\n`);
    process.exitCode = 1;
    return;
  }
  if (dryRun) {
    process.stdout.write(`\npreflight passed for ${version}, nothing was published\n`);
    return;
  }

  if (only !== "crates") {
    if (await isPublished("npm", NPM_NAME, version)) {
      process.stdout.write(`\nnpm: ${NPM_NAME}@${version} is already published\n`);
    } else {
      await publishNpm(version);
    }
  }
  if (only !== "npm") {
    await publishCrates(version);
  }
  process.stdout.write(`\ndone: ${version} is on npm and crates.io\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`\npublish: ${error.message}\n`);
    process.exitCode = 1;
  });
}
