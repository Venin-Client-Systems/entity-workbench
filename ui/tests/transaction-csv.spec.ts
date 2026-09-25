import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import AxeBuilder from "@axe-core/playwright";
import { nativeExportBridge } from "./native-export-test-bridge";
import type { CsvExport } from "../src/transaction-csv";
const root = resolve("artifacts/synthetic-ui-workspace"),
  exe = resolve("target/debug/ew-dev"),
  captures = resolve("artifacts/typed-csv-ui");
const hash = (b: Buffer) => createHash("sha256").update(b).digest("hex");
const core = (q: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(exe, [root], {
      input: JSON.stringify(q),
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    }),
  );
const selection = {
  query: "",
  filter: {
    date_from: null,
    date_to: null,
    account: null,
    currency: null,
    review: null,
  },
  order: "date_ascending",
};
let revision: number;
let rows: any[];
const quote = (s: string) => '"' + s.replaceAll('"', '""') + '"';
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  const special = [
    "=SUM(A1)",
    "  +1",
    "@cmd",
    "＝１",
    'Line one\r\n"Line two", quoted',
    "null",
    "Synthetic optional fields remain missing",
  ];
  const data = Array.from({ length: 113 }, (_, i) =>
    [
      "000042",
      "2024-02-29",
      special[i] ?? `Synthetic payment ${i}`,
      i === 0 ? "-123456789012345.12345678" : "-0.10000001",
      "AUD",
    ]
      .map(quote)
      .join(","),
  ).join("\r\n");
  revision = core({
    action: "import",
    name: "synthetic-typed-export.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency\r\n" + data + "\r\n",
      ),
    ],
  }).workspace.revision;
  rows = JSON.parse(
    core({
      action: "export_transactions",
      request: selection,
      expected_revision: revision,
    }).json,
  );
  for (const [i, state] of ["accepted", "rejected", "deferred"].entries())
    revision = core({
      action: "review_transaction",
      id: rows[i].id,
      state,
      reason: "Synthetic CSV UI fixture",
      expected_revision: revision,
    }).workspace.revision;
});
const ledger = (page: Page) =>
  page.getByRole("region", { name: "Paged transaction ledger", exact: true });
const panel = (page: Page) =>
  page.getByRole("region", {
    name: "Complete transaction export",
    exact: true,
  });
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  await expect(
    ledger(page).getByRole("button", { name: "Export JSON", exact: true }),
  ).toBeEnabled();
}
async function csv(page: Page, allow = true) {
  await panel(page).getByLabel("Transaction output format").selectOption("csv");
  if (allow)
    await panel(page)
      .getByLabel("Non-accepted selected rows — CSV review policy")
      .selectOption("allow_selected");
}
function canonical(request = selection, policy = "allow_selected"): CsvExport {
  return core({
    action: "export_transaction_csv",
    request: { selection: request, non_accepted: policy },
    expected_revision: revision,
  });
}
async function capture(page: Page, name: string) {
  const ax = await new AxeBuilder({ page }).analyze();
  expect(ax.violations).toEqual([]);
  writeFileSync(
    resolve(captures, name + "-axe.json"),
    JSON.stringify(
      { violations: ax.violations, passes: ax.passes.map((v) => v.id) },
      null,
      2,
    ),
  );
  const viewport = page.viewportSize()!;
  await page.setViewportSize({ width: viewport.width, height: 2200 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await panel(page).screenshot({ path: resolve(captures, name + ".png") });
  await page.setViewportSize(viewport);
}
function independentlyDecode(
  bytes: Buffer,
  dictionary: CsvExport["dictionary"],
) {
  return JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        `import csv,io,json,sys
x=json.load(sys.stdin); rows=list(csv.reader(io.StringIO(x['csv'].lstrip('\\ufeff'),newline=''))); cols=x['dictionary']['columns']; assert rows[0]==[c['name'] for c in cols]; result=[]
for row in rows[1:]:
 assert len(row)==len(cols); decoded={}
 for value,c in zip(row,cols):
  if value=='null': assert c['nullable']; v=None
  else:
   assert value.startswith(c['prefix']); v=value[len(c['prefix']):]
   if c['logical_type']=='unsigned_integer': v=int(v)
   elif c['logical_type'] in ('source_anchor_json','string_array_json'): v=json.loads(v)
  decoded[c['name']]=v
 result.append(decoded)
print(json.dumps(result))`,
      ],
      {
        input: JSON.stringify({ csv: bytes.toString("utf8"), dictionary }),
        encoding: "utf8",
      },
    ),
  );
}
test("CSV requires an explicit mixed-review choice and downloads every exact canonical row while JSON remains available", async ({
  page,
}) => {
  await open(page);
  await csv(page, false);
  let downloads = 0;
  page.on("download", () => downloads++);
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .click();
  await expect(
    ledger(page)
      .getByRole("alert")
      .filter({ hasText: "CSV selection contains non-accepted records" }),
  ).toBeVisible();
  expect(downloads).toBe(0);
  await panel(page)
    .getByLabel("Non-accepted selected rows — CSV review policy")
    .selectOption("allow_selected");
  await capture(page, "csv-wide");
  const expected = canonical();
  const downloaded = page.waitForEvent("download");
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .click();
  const artifact = await downloaded;
  const bytes = readFileSync((await artifact.path())!);
  expect(artifact.suggestedFilename()).toBe(
    `transactions-typed-v1-r${revision}-${expected.sha256}.csv`,
  );
  expect(hash(bytes)).toBe(expected.sha256);
  expect(bytes).toEqual(Buffer.from(expected.csv));
  writeFileSync(resolve(captures, "synthetic-transactions.csv"), bytes);
  const { csv: _body, ...metadata } = expected;
  writeFileSync(
    resolve(captures, "browser-export-metadata.json"),
    JSON.stringify(metadata, null, 2),
  );
  const decoded = independentlyDecode(bytes, expected.dictionary);
  expect(decoded).toHaveLength(113);
  const canonicalRows = JSON.parse(
    core({
      action: "export_transactions",
      request: selection,
      expected_revision: revision,
    }).json,
  );
  expect(
    decoded.map(({ workspace_revision, ...row }: any) => {
      expect(workspace_revision).toBe(revision);
      return row;
    }),
  ).toEqual(canonicalRows);
  expect(bytes.toString()).toContain("decimal:-123456789012345.12345678");
  expect(bytes.toString()).toContain("text:=SUM(A1)");
  expect(bytes.toString()).toContain("text:000042");
  await panel(page)
    .getByLabel("Transaction output format")
    .selectOption("json");
  const jsonDownload = page.waitForEvent("download");
  await panel(page)
    .getByRole("button", { name: "Export JSON", exact: true })
    .click();
  expect(
    JSON.parse(readFileSync((await (await jsonDownload).path())!, "utf8")),
  ).toEqual(canonicalRows);
  expect(core({ action: "view" }).workspace.revision).toBe(revision);
});
test("accepted-only and empty scopes obey the explicit policy; compact controls remain accessible", async ({
  page,
}) => {
  await page.setViewportSize({ width: 760, height: 1000 });
  await open(page);
  await ledger(page)
    .getByLabel("Review filter", { exact: true })
    .selectOption("accepted");
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(
    panel(page).getByText(
      `1 selected row across all pages · captured workspace revision ${revision}`,
      { exact: true },
    ),
  ).toBeVisible();
  await csv(page, false);
  await capture(page, "csv-compact");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  const download = page.waitForEvent("download");
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .focus();
  await page.keyboard.press("Enter");
  expect((await download).suggestedFilename()).toMatch(/\.csv$/);
  await ledger(page)
    .getByLabel("Filter transactions", { exact: true })
    .fill("absent synthetic phrase");
  await expect(
    panel(page).getByRole("button", { name: "Export CSV", exact: true }),
  ).toBeDisabled();
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(
    panel(page).getByText(
      `0 selected rows across all pages · captured workspace revision ${revision}`,
      { exact: true },
    ),
  ).toBeVisible();
  const empty = page.waitForEvent("download");
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .click();
  const bytes = readFileSync((await (await empty).path())!);
  expect(independentlyDecode(bytes, canonical().dictionary)).toEqual([]);
});
test("browser export rejects altered dictionary, policy, hash and malformed fixed CSV instead of downloading", async ({
  page,
}) => {
  await open(page);
  await csv(page);
  let mode = "dictionary",
    downloads = 0;
  page.on("download", () => downloads++);
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (q.action !== "export_transaction_csv") return route.continue();
    const response = await route.fetch();
    const v = await response.json();
    if (mode === "dictionary")
      v.dictionary.columns[0].meaning = "Substituted meaning";
    if (mode === "policy") v.request.non_accepted = "reject";
    if (mode === "hash") v.sha256 = "0".repeat(64);
    if (mode === "csv") {
      v.csv = v.csv.slice(0, -2);
      v.bytes = Buffer.byteLength(v.csv);
      v.sha256 = hash(Buffer.from(v.csv));
    }
    await route.fulfill({ response, json: v });
  });
  for (mode of ["dictionary", "policy", "hash", "csv"]) {
    await panel(page)
      .getByRole("button", { name: "Export CSV", exact: true })
      .click();
    await expect(
      ledger(page).getByRole("alert").filter({ hasText: /CSV/ }),
    ).toBeVisible();
    await expect(
      panel(page).getByRole("button", { name: "Export CSV", exact: true }),
    ).toBeEnabled();
  }
  expect(downloads).toBe(0);
});
test("late browser CSV is not downloaded after the applied scope changes", async ({
  page,
}) => {
  await open(page);
  await csv(page);
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false,
    downloads = 0;
  page.on("download", () => downloads++);
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "export_transaction_csv")
      return route.continue();
    const response = await route.fetch();
    entered = true;
    await held;
    await route.fulfill({ response });
  });
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .click();
  await expect.poll(() => entered).toBe(true);
  await ledger(page)
    .getByLabel("Filter transactions", { exact: true })
    .fill("Synthetic payment 100");
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  release();
  await expect(
    ledger(page)
      .getByRole("alert")
      .filter({ hasText: "Ledger changed while preparing export" }),
  ).toBeVisible();
  expect(downloads).toBe(0);
});
test("native CSV uses metadata-only version2 saves and same-ticket lost-ack recovery with exact bytes", async ({
  page,
}) => {
  const expected = canonical();
  let commits = 0;
  const tickets: string[] = [];
  let verifiedReceipt: Record<string, unknown> | undefined;
  const app = await nativeExportBridge(page, root, async (call, value) => {
    if (call.command === "prepare_native_export") {
      expect(value.schema_version).toBe(2);
      expect(value.artifact.format_sha256).toBe(expected.format_sha256);
      expect(value.artifact).not.toHaveProperty("csv");
      expect(call.args.request).not.toHaveProperty("bytes");
    }
    if (call.command === "commit_native_export") {
      tickets.push(String(call.args.ticket));
      const { location: _location, ...receipt } = value;
      verifiedReceipt = { ...receipt, location_verified_by_test: true };
      if (++commits === 1) return { status: 503 };
    }
  });
  try {
    await open(page);
    await csv(page);
    let downloads = 0;
    page.on("download", () => downloads++);
    await panel(page)
      .getByRole("button", { name: "Export CSV", exact: true })
      .click();
    await expect(
      ledger(page).getByText(/Saved 113 matching rows/),
    ).toBeVisible();
    expect(commits).toBe(2);
    expect(new Set(tickets).size).toBe(1);
    expect(downloads).toBe(0);
    const names = readdirSync(resolve(root, "exports"));
    expect(names).toEqual([
      `transactions-typed-v1-r${revision}-${expected.sha256}.csv`,
    ]);
    expect(readFileSync(resolve(root, "exports", names[0]))).toEqual(
      Buffer.from(expected.csv),
    );
    expect(readdirSync(resolve(root, "native-export-staging"))).toEqual([]);
    writeFileSync(
      resolve(captures, "native-saved-metadata.json"),
      JSON.stringify(verifiedReceipt, null, 2),
    );
    await capture(page, "csv-native-controls");
  } finally {
    await app.close();
  }
});
test("native CSV rejects wrong envelope/dictionary receipts and cleans the actual stage", async ({
  page,
}) => {
  let mode = "envelope";
  const app = await nativeExportBridge(page, root, async (call, value) => {
    if (call.command === "prepare_native_export") {
      if (mode === "envelope")
        return { value: { ...value, schema_version: 1 } };
      return {
        value: {
          ...value,
          artifact: {
            ...value.artifact,
            dictionary: {
              ...value.artifact.dictionary,
              null_literal: "missing",
            },
          },
        },
      };
    }
  });
  try {
    await open(page);
    await csv(page);
    for (mode of ["envelope", "dictionary"]) {
      await panel(page)
        .getByRole("button", { name: "Export CSV", exact: true })
        .click();
      await expect(
        ledger(page)
          .getByRole("alert")
          .filter({ hasText: "Prepared native export identity did not match" }),
      ).toBeVisible();
      await expect
        .poll(() => readdirSync(resolve(root, "native-export-staging")).length)
        .toBe(0);
    }
    expect(app.calls.filter((v) => v === "commit_native_export")).toHaveLength(
      0,
    );
    expect(readdirSync(resolve(root, "exports"))).toEqual([]);
  } finally {
    await app.close();
  }
});
test("late native CSV preparation is discarded on unmount and never commits", async ({
  page,
}) => {
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false;
  const app = await nativeExportBridge(page, root, async (call) => {
    if (call.command === "prepare_native_export") {
      entered = true;
      await held;
    }
  });
  try {
    await open(page);
    await csv(page);
    await panel(page)
      .getByRole("button", { name: "Export CSV", exact: true })
      .click();
    await expect.poll(() => entered).toBe(true);
    await page
      .getByRole("navigation")
      .getByRole("button", { name: /Overview/ })
      .click();
    release();
    await expect
      .poll(() => app.calls.includes("discard_native_export"))
      .toBe(true);
    await expect
      .poll(() => readdirSync(resolve(root, "native-export-staging")).length)
      .toBe(0);
    expect(app.calls).not.toContain("commit_native_export");
    expect(readdirSync(resolve(root, "exports"))).toEqual([]);
  } finally {
    release();
    await app.close();
  }
});
test("two lost native commit acknowledgements remain unconfirmed without a replacement preparation", async ({
  page,
}) => {
  const app = await nativeExportBridge(page, root, async (call) => {
    if (call.command === "commit_native_export") return { status: 503 };
  });
  try {
    await open(page);
    await csv(page);
    await panel(page)
      .getByRole("button", { name: "Export CSV", exact: true })
      .click();
    await expect(
      ledger(page)
        .getByRole("alert")
        .filter({ hasText: "Native save completion is unconfirmed" }),
    ).toBeVisible();
    expect(app.calls.filter((v) => v === "prepare_native_export")).toHaveLength(
      1,
    );
    expect(app.calls.filter((v) => v === "commit_native_export")).toHaveLength(
      2,
    );
    await expect(ledger(page).getByText(/Saved 113 matching rows/)).toHaveCount(
      0,
    );
    expect(readdirSync(resolve(root, "exports"))).toHaveLength(1);
  } finally {
    await app.close();
  }
});

test("stale revisions and corrupted originals refuse CSV without a download", async ({
  page,
}) => {
  await open(page);
  await csv(page);
  let downloads = 0;
  page.on("download", () => downloads++);
  const evidence = core({ action: "view" }).workspace.evidence[0];
  const path = resolve(root, "originals", evidence.sha256);
  const original = readFileSync(path);
  try {
    chmodSync(path, 0o600);
    writeFileSync(path, Buffer.alloc(original.length, 65));
    await panel(page)
      .getByRole("button", { name: "Export CSV", exact: true })
      .click();
    await expect(
      ledger(page)
        .getByRole("alert")
        .filter({ hasText: /integrity|digest|original/i }),
    ).toBeVisible();
    expect(downloads).toBe(0);
  } finally {
    writeFileSync(path, original);
    chmodSync(path, 0o400);
  }
  core({
    action: "review_transaction",
    id: rows[0].id,
    state: "accepted",
    reason: "Concurrent synthetic reviewer",
    expected_revision: revision,
  });
  await panel(page)
    .getByRole("button", { name: "Export CSV", exact: true })
    .click();
  await expect(
    ledger(page)
      .getByRole("alert")
      .filter({ hasText: /revision/i }),
  ).toBeVisible();
  expect(downloads).toBe(0);
});
