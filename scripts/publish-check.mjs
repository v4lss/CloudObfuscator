import { indexPath, indexedVersions, isPublished, waitForIndex } from "./publish.mjs";

const results = [];
function report(label, ok, detail = "") {
  results.push(ok);
  process.stdout.write(`${ok ? "PASS" : "FAIL"}  ${label}${detail ? ` -> ${detail}` : ""}\n`);
}

// The sparse index path convention: 2 chars, then 2 chars, then the full name.
report("indexPath(serde)", indexPath("serde") === "https://index.crates.io/se/rd/serde", indexPath("serde"));
report(
  "indexPath(cloudobfuscator-parser)",
  indexPath("cloudobfuscator-parser") === "https://index.crates.io/cl/ou/cloudobfuscator-parser",
  indexPath("cloudobfuscator-parser"),
);

const serdeVersions = await indexedVersions("serde");
report(
  "indexedVersions(serde) returns real versions",
  serdeVersions.length > 100 && serdeVersions.includes("1.0.0"),
  `${serdeVersions.length} versions`,
);

const missing = await indexedVersions("cloudobfuscator-nope-not-real-xyz");
report("indexedVersions of an unknown crate is empty", missing.length === 0, `${missing.length}`);

const started = Date.now();
await waitForIndex("serde", "1.0.0", 20000);
report("waitForIndex returns at once when present", Date.now() - started < 5000, `${Date.now() - started}ms`);

let timedOut = false;
const t0 = Date.now();
try {
  await waitForIndex("cloudobfuscator-nope-not-real-xyz", "9.9.9", 3000);
} catch (error) {
  timedOut = /not indexed within/.test(error.message);
}
report("waitForIndex times out on an unknown version", timedOut && Date.now() - t0 < 15000);

report("isPublished crates: serde 1.0.0 exists", (await isPublished("crates", "serde", "1.0.0")) === true);
report("isPublished crates: serde 0.0.0-nope does not", (await isPublished("crates", "serde", "0.0.0-nope")) === false);

report("isPublished npm: vite 7.0.0 exists", (await isPublished("npm", "vite", "7.0.0")) === true);
report(
  "isPublished npm: our package is not out yet",
  (await isPublished("npm", "vite-plugin-cloudobfuscator", "0.1.0")) === false,
);

process.stdout.write(`\n${results.filter(Boolean).length}/${results.length} passed\n`);
process.exitCode = results.every(Boolean) ? 0 : 1;
