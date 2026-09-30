import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, parse, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const IS_WINDOWS = process.platform === "win32";
const EXECUTABLE = IS_WINDOWS ? "cloudobfuscator.exe" : "cloudobfuscator";
const SHIM = IS_WINDOWS ? "cloudobfuscator.cmd" : "cloudobfuscator";

function ancestorDirectories(from) {
  const directories = [];
  let current = resolve(from);
  for (;;) {
    directories.push(current);
    const parent = dirname(current);
    if (parent === current) {
      return directories;
    }
    current = parent;
  }
}

function candidatePaths() {
  const paths = [];
  if (process.env.CLOUDOBFUSCATOR_BIN) {
    paths.push(resolve(process.env.CLOUDOBFUSCATOR_BIN));
  }
  for (const root of ancestorDirectories(process.cwd())) {
    paths.push(join(root, "target", "release", EXECUTABLE));
    paths.push(join(root, "target", "debug", EXECUTABLE));
    paths.push(join(root, "node_modules", ".bin", SHIM));
    paths.push(join(root, "node_modules", ".bin", EXECUTABLE));
  }
  const here = dirname(fileURLToPath(import.meta.url));
  for (const root of ancestorDirectories(here)) {
    paths.push(join(root, "target", "release", EXECUTABLE));
    paths.push(join(root, "target", "debug", EXECUTABLE));
  }
  return paths;
}

export function resolveBinary(options = {}) {
  if (options.binary) {
    const explicit = resolve(options.binary);
    if (!existsSync(explicit)) {
      throw new Error(`cloudobfuscator binary not found at ${explicit}`);
    }
    return explicit;
  }
  for (const candidate of candidatePaths()) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return EXECUTABLE;
}

export class ObfuscatorWorker {
  constructor(binary, args = []) {
    this.binary = binary;
    this.args = args;
    this.child = null;
    this.pending = new Map();
    this.nextId = 1;
    this.buffer = "";
    this.closing = null;
  }

  start() {
    if (this.child) {
      return this;
    }
    this.exited = false;
    const shimmed = IS_WINDOWS && /\.(cmd|bat)$/i.test(this.binary);
    const child = shimmed
      ? spawn(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", `"${this.binary}"`, ...this.args, "--server"], {
          stdio: ["pipe", "pipe", "pipe"],
          windowsHide: true,
        })
      : spawn(this.binary, [...this.args, "--server"], {
          stdio: ["pipe", "pipe", "pipe"],
          windowsHide: true,
        });
    this.child = child;
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => this.#consume(chunk));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => this.#rejectAll(String(chunk)));
    child.on("error", (error) => this.#rejectAll(error.message));
    child.on("exit", (code) => {
      this.exited = true;
      this.#rejectAll(`cloudobfuscator exited with code ${code}`);
      this.#releaseStreams(child);
    });
    return this;
  }

  #releaseStreams(child) {
    if (!child) {
      return;
    }
    for (const stream of [child.stdout, child.stderr, child.stdin]) {
      if (!stream) {
        continue;
      }
      stream.removeAllListeners("data");
      stream.on("error", () => {});
      stream.destroy();
    }
  }

  #consume(chunk) {
    this.buffer += chunk;
    let index = this.buffer.indexOf("\n");
    while (index !== -1) {
      const line = this.buffer.slice(0, index).trim();
      this.buffer = this.buffer.slice(index + 1);
      if (line) {
        this.#settle(line);
      }
      index = this.buffer.indexOf("\n");
    }
  }

  #settle(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      this.#rejectAll(`cloudobfuscator emitted invalid JSON: ${error.message}`);
      return;
    }
    const entry = this.pending.get(message.id);
    if (!entry) {
      return;
    }
    this.pending.delete(message.id);
    if (message.ok) {
      entry.resolve(message);
    } else {
      entry.reject(new Error(message.error ?? "unknown obfuscation failure"));
    }
  }

  #rejectAll(reason) {
    const entries = [...this.pending.values()];
    this.pending.clear();
    for (const entry of entries) {
      entry.reject(new Error(reason));
    }
  }

  request(file, code, config) {
    if (!this.child) {
      throw new Error("worker is not started");
    }
    const id = this.nextId++;
    const payload = JSON.stringify({ id, file, code, config: config ?? null });
    const reply = new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
    this.child.stdin.write(`${payload}\n`);
    return reply;
  }

  async close() {
    if (!this.child) {
      return;
    }
    if (this.closing) {
      await this.closing;
      return;
    }
    const child = this.child;
    this.child = null;
    this.closing = this.exited
      ? Promise.resolve()
      : new Promise((resolve) => {
          child.once("exit", resolve);
          child.stdin.end();
          setTimeout(() => child.kill(), 2000).unref();
        });
    await this.closing;
    this.closing = null;
  }
}
