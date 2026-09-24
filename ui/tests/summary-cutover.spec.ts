import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace } from "../src/types";

const root = resolve("artifacts/synthetic-ui-workspace"),
  captures = resolve("artifacts/summary-cutover");
const core = (input: Record<string, unknown>, summary = false) =>
  JSON.parse(
    execFileSync(
      resolve("target/debug/ew-dev"),
      [root, ...(summary ? ["--summary"] : [])],
      {
        input: JSON.stringify(input),
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
      },
    ),
  );
function fixture(count = 251, repeats = false): Workspace {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  const rows = Array.from({ length: count }, (_, i) =>
    repeats
      ? `0001,2025-01-01,"Repeated ${'""'.repeat(3990)}",-1.00,AUD,`
      : `000${(i % 2) + 1},2025-01-${String((i % 28) + 1).padStart(2, "0")},Synthetic payment ${String(i).padStart(3, "0")}${i === 0 ? " <script>globalThis.ledgerExecuted=true</script>" : ""},-${i + 1}.00,${i % 3 ? "AUD" : "USD"},${i < 6 ? 1000 - i : ""}`,
  );
  let w = core({
    action: "import",
    name: "synthetic-summary.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency,balance\n" +
          rows.join("\n") +
          "\n",
      ),
    ],
  }).workspace as Workspace;
  if (!repeats)
    for (const [i, state] of [
      "accepted",
      "accepted",
      "rejected",
      "deferred",
    ].entries())
      w = core({
        action: "review_transaction",
        id: w.transactions[i].id,
        state,
        reason: "Synthetic review denominator",
        expected_revision: w.revision,
      }).workspace;
  return w;
}
const ledger = (page: Page) =>
  page.getByRole("region", { name: "Paged transaction ledger", exact: true });
const review = (page: Page) =>
  page.getByRole("complementary", { name: "Transaction review" });
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
const request = (
  w: Workspace,
  pageSize = 100,
  cursor: string | null = null,
) => ({
  action: "search_transactions",
  request: {
    query: "",
    page: {
      filter: {
        date_from: null,
        date_to: null,
        account: null,
        currency: null,
        review: null,
      },
      order: "date_ascending",
      page_size: pageSize,
      cursor,
    },
  },
  expected_revision: w.revision,
});
const visibleIds = (page: Page) =>
  ledger(page)
    .locator("tbody button[id^=transaction-]")
    .evaluateAll((nodes) =>
      nodes.map((node) => node.id.slice("transaction-".length)),
    );

test("summary transport omits full arrays; bounded ledger pages keep exact counts, date order and complete export", async ({
  page,
}) => {
  const w = fixture();
  const summaries: any[] = [],
    requests: any[] = [],
    external: string[] = [];
  page.on("response", async (response) => {
    if (
      response.url().endsWith("/api/workbench") &&
      response.request().postDataJSON().action === "view"
    )
      summaries.push(await response.json());
  });
  page.on("request", (request) => {
    if (request.url().endsWith("/api/workbench"))
      requests.push(request.postDataJSON());
    if (!/^(http:\/\/127\.0\.0\.1:1420\/|blob:|data:)/.test(request.url()))
      external.push(request.url());
  });
  await open(page);
  const first = core(request(w));
  expect(await visibleIds(page)).toEqual(
    first.page.rows.map((row: any) => row.id),
  );
  await expect(ledger(page)).toContainText("1–100 of 251 selected rows");
  await expect(ledger(page)).toContainText(
    "2 accepted · 247 pending · 1 rejected · 1 deferred",
  );
  expect(summaries[0].workspace).not.toHaveProperty("transactions");
  expect(summaries[0].workspace).not.toHaveProperty("decisions");
  expect(summaries[0].analysis).not.toHaveProperty("balance_checks");
  expect(summaries[0].analysis.totals[0]).not.toHaveProperty("transaction_ids");
  expect(core({ action: "view" }).workspace.transactions).toHaveLength(251);
  await ledger(page).getByRole("button", { name: "Next ledger page" }).click();
  await expect(ledger(page)).toContainText("101–200 of 251 selected rows");
  expect(await visibleIds(page)).toEqual(
    core(request(w, 100, first.page.next_cursor)).page.rows.map(
      (row: any) => row.id,
    ),
  );
  await expect(
    ledger(page).getByRole("status").filter({ hasText: "101–200" }),
  ).toBeFocused();
  await ledger(page).getByRole("button", { name: "Back ledger page" }).click();
  await expect(ledger(page)).toContainText("1–100 of 251 selected rows");
  await ledger(page).getByLabel("Filter transactions").fill("payment 250");
  await expect(ledger(page)).toContainText("Draft filters are not applied");
  await expect(
    ledger(page).getByRole("button", { name: "Export JSON" }),
  ).toBeDisabled();
  expect(await visibleIds(page)).toEqual(
    first.page.rows.map((row: any) => row.id),
  );
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(ledger(page)).toContainText("1–1 of 1 selected rows");
  await ledger(page)
    .getByRole("button", { name: "Clear draft filters" })
    .click();
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(ledger(page)).toContainText("1–100 of 251 selected rows");
  const download = page.waitForEvent("download");
  await ledger(page).getByRole("button", { name: "Export JSON" }).click();
  const file = await download,
    bytes = readFileSync((await file.path())!);
  const exported = core({
    action: "export_transactions",
    request: {
      query: "",
      filter: request(w).request.page.filter,
      order: "date_ascending",
    },
    expected_revision: w.revision,
  });
  expect(bytes.toString()).toBe(exported.json);
  expect(JSON.parse(bytes.toString())).toHaveLength(251);
  expect(createHash("sha256").update(bytes).digest("hex")).toBe(
    exported.sha256,
  );
  await expect(ledger(page)).toContainText("Prepared 251 matching rows");
  expect(
    requests
      .filter((r) => r.action === "search_transactions")
      .every((r) => r.request.page.page_size <= 200),
  ).toBe(true);
  expect(await page.evaluate(() => "ledgerExecuted" in globalThis)).toBe(false);
  expect(external).toEqual([]);
});

test("byte-short pages advance by returned rows, retain duplicates and distinguish zero selected from empty scope", async ({
  page,
}) => {
  const w = fixture(300, true);
  await open(page);
  await ledger(page).getByLabel("Ledger rows per request").selectOption("200");
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  const first = core(request(w, 200));
  expect(first.page.rows.length).toBeLessThan(200);
  await expect(ledger(page)).toContainText(
    `1–${first.page.rows.length} of 300 selected rows`,
  );
  expect(await visibleIds(page)).toEqual(
    first.page.rows.map((row: any) => row.id),
  );
  await ledger(page).getByRole("button", { name: "Next ledger page" }).click();
  const second = core(request(w, 200, first.page.next_cursor));
  await expect(ledger(page)).toContainText(
    `${first.page.rows.length + 1}–${first.page.rows.length + second.page.rows.length} of 300 selected rows`,
  );
  await ledger(page)
    .getByLabel("Review filter", { exact: true })
    .selectOption("accepted");
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(ledger(page)).toContainText(
    "0 of 0 selected rows · 300 matching rows before review selection",
  );
  await expect(ledger(page)).toContainText("300 pending");
  expect(await visibleIds(page)).toEqual([]);
});

test("an off-page transaction keeps draft text; external correction and refresh disable stale mutation without rebasing", async ({
  page,
}) => {
  let w = fixture();
  await open(page);
  const first = core(request(w)).page.rows[0];
  await page.locator(`[id="transaction-${first.id}"]`).click();
  await page
    .getByLabel("Transaction decision reason")
    .fill("Keep this unsaved decision");
  await page.getByLabel("Corrected amount").fill("-3.25");
  await ledger(page).getByRole("button", { name: "Next ledger page" }).click();
  await expect(ledger(page)).toContainText("101–200");
  await expect(review(page)).toContainText("not on this page");
  await expect(page.getByLabel("Transaction decision reason")).toHaveValue(
    "Keep this unsaved decision",
  );
  w = core({
    action: "correct_transaction",
    id: first.id,
    amount: "-9.75",
    reason: "Synthetic concurrent correction",
    expected_revision: w.revision,
  }).workspace;
  await ledger(page)
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(review(page)).toContainText(
    "Workspace changed. This review remains at revision",
  );
  await expect(page.getByLabel("Corrected amount")).toHaveValue("-3.25");
  await expect(
    review(page).getByRole("button", { name: "Save correction for review" }),
  ).toBeDisabled();
  await expect(
    review(page).getByRole("button", { name: "Accept", exact: true }),
  ).toBeDisabled();
  expect(
    core({ action: "view" }).workspace.transactions.find(
      (row: any) => row.id === first.id,
    ).amount,
  ).toBe("-9.75");
  await page.getByRole("button", { name: "Close review" }).click();
  await expect(page.locator(`[id="transaction-${first.id}"]`)).toBeFocused();
  await page.locator(`[id="transaction-${first.id}"]`).click();
  await expect(page.getByLabel("Corrected amount")).toHaveValue("-9.75");
});

test("held real ledger reads coalesce queries and cannot restore an obsolete page after a workspace revision", async ({
  page,
}) => {
  let w = fixture();
  await open(page);
  let release!: () => void, received!: () => void;
  const gate = new Promise<void>((r) => (release = r)),
    ready = new Promise<void>((r) => (received = r));
  const queries: string[] = [];
  let held = false;
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action === "search_transactions") {
      queries.push(body.request.query);
      if (body.request.page.cursor && !held) {
        held = true;
        const response = await route.fetch();
        received();
        await gate;
        return route.fulfill({ response });
      }
    }
    await route.continue();
  });
  await ledger(page).getByRole("button", { name: "Next ledger page" }).click();
  await ready;
  for (const query of ["payment 050", "payment 070", "payment 080"]) {
    await ledger(page).getByLabel("Filter transactions").fill(query);
    await ledger(page)
      .getByRole("button", { name: "Apply ledger filters" })
      .click();
  }
  w = core({
    action: "import",
    name: "concurrent.txt",
    bytes: [...Buffer.from("Synthetic revision while real read is held")],
  }).workspace;
  await ledger(page)
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(page.locator(".revision")).toContainText(`REV ${w.revision}`);
  release();
  await expect(ledger(page)).toContainText("1–1 of 1 selected rows");
  expect(queries).toEqual(["", "payment 080"]);
  await expect(ledger(page)).toContainText("Synthetic payment 080");
  expect(await visibleIds(page)).toHaveLength(1);
});

test("broad transfer candidates page accepted other-account rows and keep unequal and other-currency options", async ({
  page,
}) => {
  let w = fixture(125);
  const target = w.transactions[0];
  for (const row of w.transactions.filter(
    (row) => row.account !== target.account,
  )) {
    if (row.review !== "accepted")
      w = core({
        action: "review_transaction",
        id: row.id,
        state: "accepted",
        reason: "Synthetic candidate review",
        expected_revision: w.revision,
      }).workspace;
  }
  await open(page);
  await page.locator(`[id="transaction-${target.id}"]`).click();
  const candidates = review(page).getByRole("region", {
    name: "Transfer candidates",
  });
  await expect(candidates).toContainText("1–50 of 62 selected rows");
  const options = await candidates
    .getByLabel("Transfer counterpart", { exact: true })
    .locator("option")
    .allTextContents();
  expect(options.some((value) => value.includes("USD"))).toBe(true);
  expect(options.some((value) => value.includes("AUD"))).toBe(true);
  const firstId = await candidates
    .getByLabel("Transfer counterpart", { exact: true })
    .locator("option")
    .nth(1)
    .getAttribute("value");
  await candidates
    .getByLabel("Transfer counterpart", { exact: true })
    .selectOption(firstId!);
  await candidates.getByRole("button", { name: "Next candidate page" }).click();
  await expect(candidates).toContainText("51–62 of 62 selected rows");
  await expect(
    candidates.getByLabel("Transfer counterpart", { exact: true }),
  ).toHaveValue(firstId!);
  await page
    .getByLabel("Transaction decision reason")
    .fill("Synthetic unequal pair must remain rejected");
  await review(page)
    .getByRole("button", { name: "Match internal transfer" })
    .click();
  await expect(page.getByRole("alert").first()).toBeVisible();
  expect(
    core({ action: "view" }).workspace.transactions.find(
      (row: any) => row.id === target.id,
    ).transfer_peer,
  ).toBeNull();
});

test("original corruption refuses a complete export without turning its successful page into an empty result", async ({
  page,
}) => {
  const w = fixture();
  await open(page);
  const path = resolve(root, "originals", w.evidence[0].sha256),
    original = readFileSync(path),
    mode = statSync(path).mode;
  chmodSync(path, 0o600);
  writeFileSync(path, Buffer.from("Synthetic altered original"));
  const downloads: string[] = [];
  page.on("download", (file) => downloads.push(file.suggestedFilename()));
  try {
    await ledger(page).getByRole("button", { name: "Export JSON" }).click();
    await expect(ledger(page).getByRole("alert")).toBeVisible();
    await expect(ledger(page)).toContainText("1–100 of 251 selected rows");
    expect(downloads).toEqual([]);
  } finally {
    writeFileSync(path, original);
    chmodSync(path, mode);
  }
});

test("balance read failure is independent of a successful page and retry restores canonical annotations", async ({
  page,
}) => {
  fixture();
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON().action === "read_transaction_balances" &&
      fail
    )
      return route.abort("failed");
    await route.continue();
  });
  await open(page);
  await expect(ledger(page)).toContainText(
    "Source-order balance checks unavailable",
  );
  await expect(ledger(page)).toContainText("1–100 of 251 selected rows");
  await expect(
    ledger(page).getByRole("button", { name: "Export JSON" }),
  ).toBeEnabled();
  await expect(
    ledger(page).getByText("Balance mismatch", { exact: false }),
  ).toHaveCount(0);
  fail = false;
  await ledger(page)
    .getByRole("button", { name: "Retry balance checks" })
    .click();
  await expect(ledger(page)).not.toContainText(
    "Source-order balance checks unavailable",
  );
  await expect(
    ledger(page).getByText("Balance mismatch", { exact: false }).first(),
  ).toBeVisible();
});

test("a held verified complete export is refused after a newly applied scope", async ({
  page,
}) => {
  fixture();
  await open(page);
  let release!: () => void, received!: () => void;
  const gate = new Promise<void>((r) => (release = r)),
    ready = new Promise<void>((r) => (received = r));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "export_transactions")
      return route.continue();
    const response = await route.fetch();
    received();
    await gate;
    await route.fulfill({ response });
  });
  const downloads: string[] = [];
  page.on("download", (file) => downloads.push(file.suggestedFilename()));
  await ledger(page).getByRole("button", { name: "Export JSON" }).click();
  await ready;
  await ledger(page).getByLabel("Filter transactions").fill("payment 080");
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(ledger(page)).toContainText("1–1 of 1 selected rows");
  release();
  await expect(ledger(page).getByRole("alert")).toContainText(
    "Ledger changed while preparing export",
  );
  expect(downloads).toEqual([]);
});

test("an empty canonical ledger and an unavailable read retain different states; backup is explicitly confirmed", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  await open(page);
  await expect(ledger(page)).toContainText(
    "0 of 0 selected rows · 0 matching rows before review selection",
  );
  await page.getByRole("button", { name: "Back up", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "Recoverable backup saved" }),
  ).toBeVisible();
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action === "search_transactions" && fail)
      return route.abort("failed");
    await route.continue();
  });
  await ledger(page)
    .getByRole("button", { name: "Apply ledger filters" })
    .click();
  await expect(ledger(page)).toContainText(
    "Ledger unavailable. No partial page is shown.",
  );
  await expect(ledger(page)).not.toContainText(
    "No transactions match this applied selection",
  );
  fail = false;
  await ledger(page)
    .getByRole("button", { name: "Retry ledger", exact: true })
    .click();
  await expect(ledger(page)).toContainText(
    "No transactions match this applied selection",
  );
});

test("compact ledger and review preserve readable controls, inert source text and accessible navigation", async ({
  page,
}) => {
  fixture(25);
  await page.setViewportSize({ width: 960, height: 800 });
  await open(page);
  await expect(ledger(page)).toContainText("1–25 of 25 selected rows");
  const audits = [];
  for (const width of [1440, 960]) {
    await page.setViewportSize({ width, height: 1000 });
    const audit = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    audits.push({
      width,
      violations: audit.violations,
      incomplete: audit.incomplete,
    });
    expect(audit.violations).toEqual([]);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await ledger(page).evaluate((element) =>
      window.scrollTo(
        0,
        element.getBoundingClientRect().top + window.scrollY - 72,
      ),
    );
    await page.screenshot({
      path: resolve(captures, `ledger-${width}.png`),
    });
  }
  await ledger(page)
    .getByRole("button", { name: /Synthetic payment 000/ })
    .click();
  const dialog = page.getByRole("dialog", { name: "Transaction review" });
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await expect(
    dialog.getByRole("region", { name: "Original transaction excerpt" }),
  ).toContainText("-1.00");
  await expect(dialog.getByRole("heading", { level: 2 })).toContainText(
    "<script>",
  );
  expect(await page.evaluate(() => "ledgerExecuted" in globalThis)).toBe(false);
  const audit = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  audits.push({
    width: 960,
    violations: audit.violations,
    incomplete: audit.incomplete,
  });
  expect(audit.violations).toEqual([]);
  await page.screenshot({ path: resolve(captures, "review-960.png") });
  await dialog
    .getByLabel("Transfer counterpart", { exact: true })
    .scrollIntoViewIfNeeded();
  await page.screenshot({
    path: resolve(captures, "review-candidates-960.png"),
  });
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(
    ledger(page).getByRole("button", { name: /Synthetic payment 000/ }),
  ).toBeFocused();
  writeFileSync(
    resolve(captures, "accessibility.json"),
    JSON.stringify(audits, null, 2),
  );
});
