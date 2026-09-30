import { ObfuscatorWorker, resolveBinary } from "./worker.mjs";

const DEFAULT_INCLUDE = /\.[cm]?js$/;
const DEFAULT_EXCLUDE = [/(^|[\\/])node_modules([\\/]|$)/, /\.min\.js$/];

function toArray(value) {
  if (value === undefined || value === null) {
    return [];
  }
  return Array.isArray(value) ? value : [value];
}

function matches(id, include, exclude) {
  const normalized = id.split("\\").join("/");
  if (!include.some((pattern) => pattern.test(normalized))) {
    return false;
  }
  return !exclude.some((pattern) => pattern.test(normalized));
}

export function cloudObfuscator(options = {}) {
  const {
    binary,
    config,
    seed,
    preset,
    minify = true,
    include = DEFAULT_INCLUDE,
    exclude = DEFAULT_EXCLUDE,
    onReport,
  } = options;

  const includePatterns = toArray(include);
  const excludePatterns = toArray(exclude);
  const workerArgs = [];
  if (preset) {
    workerArgs.push("--preset", preset);
  }

  let reportList = [];
  let worker = null;

  return {
    name: "cloudobfuscator",
    apply: "build",
    enforce: "post",

    async buildStart() {
      if (!worker) {
        worker = new ObfuscatorWorker(resolveBinary({ binary }), workerArgs).start();
      }
      reportList = [];
    },

    async renderChunk(code, chunk) {
      if (!matches(chunk.fileName, includePatterns, excludePatterns)) {
        return null;
      }
      if (!worker) {
        worker = new ObfuscatorWorker(resolveBinary({ binary }), workerArgs).start();
      }
      const reply = await worker.request(chunk.fileName, code, {
        ...(config ?? {}),
        seed: seed ?? config?.seed ?? null,
        minify_output: minify,
      });
      if (reply.report) {
        reportList.push(reply.report);
        if (typeof onReport === "function") {
          onReport(reply.report);
        }
      }
      return { code: reply.code, map: null };
    },

    async closeBundle() {
      if (worker) {
        await worker.close();
        worker = null;
      }
    },

    reports() {
      return reportList;
    },
  };
}

export default cloudObfuscator;
