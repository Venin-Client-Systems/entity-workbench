// Compare the committed Rust fixture against the actual JavaScript operation.
// Node is a development check, not an installed-application dependency.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

const path = new URL("../fixtures/transactions/literal-search.v1.json", import.meta.url);
const bytes = readFileSync(path);
const fixture = JSON.parse(bytes);
assert.equal(fixture.schema_version, 1);
assert.equal(fixture.cases.length, 32);
for (const item of fixture.cases) {
  const text = `${item.description} ${item.account} ${item.date}`;
  assert.equal(text.toLowerCase().includes(item.query.toLowerCase()), item.matches, item.name);
}
process.stdout.write(`${JSON.stringify({
  outcome: "passed",
  cases: fixture.cases.length,
  fixture_sha256: createHash("sha256").update(bytes).digest("hex"),
  node: process.version,
  unicode: process.versions.unicode,
  icu: process.versions.icu,
  limitation: "These scalar-text cases are not universal cross-version or WebView equivalence.",
}, null, 2)}\n`);
