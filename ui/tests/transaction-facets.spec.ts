import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";

const root = resolve("artifacts/synthetic-ui-workspace");
const captures = resolve("artifacts/transaction-facets-ui");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
const accounts = [
  "0001",
  "1",
  "A",
  "Café",
  "Café",
  "a",
  ...Array.from({ length: 199 }, (_, i) => `SYN-${String(i).padStart(3, "0")}`),
];
const currencies = Array.from(
  { length: 125 },
  (_, i) =>
    `A${String.fromCharCode(65 + Math.floor(i / 26))}${String.fromCharCode(65 + (i % 26))}`,
);
let revision: number;
function importRows(name: string, rows: string[]) {
  return core({
    action: "import",
    name,
    bytes: [
      ...Buffer.from(
        `account,date,description,amount,currency\n${rows.join("\n")}\n`,
      ),
    ],
  }).workspace;
}
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  let workspace = importRows(
    "synthetic-facets.csv",
    accounts.map(
      (account, i) =>
        `${account},2025-01-01,Synthetic facet ${i},-1.00,${currencies[i % currencies.length]}`,
    ),
  );
  for (const [i, state] of ["accepted", "rejected", "deferred"].entries()) {
    workspace = core({
      action: "review_transaction",
      id: workspace.transactions[i].id,
      state,
      reason: "Synthetic facet denominator.",
      expected_revision: workspace.revision,
    }).workspace;
  }
  revision = workspace.revision;
});
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
}
const facet = (page: Page, label: string) =>
  page.locator(`.transaction-facet[data-facet-label="${label}"]`);
async function firstReady(page: Page) {
  await open(page);
  for (const label of ["Analysis account", "Comparison account"])
    await expect(facet(page, label).getByRole("status")).toHaveText(
      "1–100 of 205 accounts",
    );
  for (const label of ["Analysis currency", "Comparison currency"])
    await expect(facet(page, label).getByRole("status")).toHaveText(
      "1–100 of 125 currencies",
    );
}
async function accessibility(page: Page, name: string) {
  const result = await new AxeBuilder({ page })
    .include(".transaction-facet")
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `${name}-axe.json`),
    JSON.stringify({ violations: result.violations }, null, 2),
  );
  expect(result.violations).toEqual([]);
}

test("both consumers page exact whole-ledger choices, retain an off-page selection and preserve draft scope", async ({
  page,
}) => {
  const requests: Record<string, any>[] = [];
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (request.url().endsWith("/api/workbench"))
      requests.push(request.postDataJSON());
  });
  await firstReady(page);
  const expected = core({
    action: "page_transaction_facets",
    request: { facet: "account", page_size: 100, cursor: null },
    expected_revision: revision,
  });
  expect(
    await facet(page, "Analysis account")
      .locator("option")
      .evaluateAll((options) =>
        options.slice(1).map((option) => (option as HTMLOptionElement).value),
      ),
  ).toEqual(expected.values.map((row: { value: string }) => row.value));
  await expect(facet(page, "Analysis account")).toContainText(
    "205 whole-ledger rows · all review states",
  );
  const initialReads = requests.filter(
    (request) => request.action === "page_transaction_facets",
  );
  expect(initialReads).toHaveLength(4);
  expect(
    initialReads.every(
      (request) =>
        request.request.page_size === 100 && request.request.cursor === null,
    ),
  ).toBe(true);

  for (const label of ["Analysis account", "Comparison account"]) {
    const box = facet(page, label);
    await box.getByLabel(label, { exact: true }).selectOption("0001");
    await box
      .getByRole("button", { name: `Next ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText("101–200 of 205 accounts");
    await expect(box.getByLabel(label, { exact: true })).toHaveValue("0001");
    await expect(box.locator('option[value="0001"]')).toHaveText(
      "0001 · retained selection",
    );
    await box
      .getByRole("button", { name: `Next ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText("201–205 of 205 accounts");
    await expect(box.getByRole("status")).toBeFocused();
    const last = await box.locator("option").last().getAttribute("value");
    await box.getByLabel(label, { exact: true }).selectOption(last!);
    await expect(
      box.getByRole("button", { name: `Next ${label.toLowerCase()} page` }),
    ).toBeDisabled();
    await box
      .getByRole("button", { name: `Previous ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText("101–200 of 205 accounts");
    await box
      .getByRole("button", { name: `First ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText("1–100 of 205 accounts");
    await expect(box.getByLabel(label, { exact: true })).toHaveValue(last!);
    await expect(box.locator("option")).toHaveCount(102);
  }
  for (const label of ["Analysis currency", "Comparison currency"]) {
    const box = facet(page, label);
    await box
      .getByRole("button", { name: `Next ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText(
      "101–125 of 125 currencies",
    );
    await box.getByLabel(label, { exact: true }).selectOption(currencies[124]);
    await box
      .getByRole("button", { name: `First ${label.toLowerCase()} page` })
      .click();
    await expect(box.getByRole("status")).toHaveText("1–100 of 125 currencies");
    await expect(box.getByLabel(label, { exact: true })).toHaveValue(
      currencies[124],
    );
    await expect(box.locator("option")).toHaveCount(102);
  }
  const patterns = page.getByRole("region", {
    name: "Transaction patterns",
    exact: true,
  });
  await patterns.getByLabel("From date", { exact: true }).fill("2025-01-01");
  await patterns
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    patterns.getByText("0 in scope / 205 workspace rows"),
  ).toBeVisible();
  expect(
    requests.find((request) => request.action === "analyze_transactions")
      ?.request,
  ).toMatchObject({
    account: "a",
    currency: currencies[124],
    date_from: "2025-01-01",
  });
  const comparison = page.getByRole("region", {
    name: "Transaction period comparison",
    exact: true,
  });
  for (const [label, value] of [
    ["Baseline from", "2025-01-01"],
    ["Baseline through", "2025-01-31"],
    ["Comparison from", "2025-02-01"],
    ["Comparison through", "2025-02-28"],
  ])
    await comparison.getByLabel(label, { exact: true }).fill(value);
  await comparison
    .getByRole("button", { name: "Compare selected periods" })
    .click();
  await expect(
    comparison.getByText(
      "205 workspace rows / 0 match account and currency / 205 outside those filters.",
    ),
  ).toBeVisible();
  expect(
    requests.find((request) => request.action === "compare_transaction_periods")
      ?.request,
  ).toMatchObject({
    account: "a",
    currency: currencies[124],
    baseline: { from: "2025-01-01", through: "2025-01-31" },
  });
  await accessibility(page, "desktop");
  await page.setViewportSize({ width: 1440, height: 2200 });
  await patterns.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.evaluate(() => window.scrollBy(0, -140));
  await patterns.screenshot({
    path: resolve(captures, "patterns-desktop.png"),
  });
  await comparison.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.evaluate(() => window.scrollBy(0, -140));
  await comparison.screenshot({
    path: resolve(captures, "comparison-desktop.png"),
  });
  await page.setViewportSize({ width: 960, height: 2200 });
  await accessibility(page, "compact");
  await patterns.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.evaluate(() => window.scrollBy(0, -140));
  await patterns.screenshot({
    path: resolve(captures, "patterns-compact.png"),
  });
  await comparison.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.evaluate(() => window.scrollBy(0, -140));
  await comparison.screenshot({
    path: resolve(captures, "comparison-compact.png"),
  });
  expect(errors).toEqual([]);
});

test("transport failure is unavailable, retained selection can be cleared, and Retry reads the real page", async ({
  page,
}) => {
  await firstReady(page);
  const box = facet(page, "Analysis account");
  await box
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0001");
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (
      fail &&
      request.action === "page_transaction_facets" &&
      request.request.facet === "account" &&
      request.request.cursor
    ) {
      fail = false;
      await route.abort("failed");
    } else await route.continue();
  });
  await box.getByRole("button", { name: "Next analysis account page" }).click();
  await expect(box.getByRole("alert")).toContainText("choices unavailable");
  await expect(
    box.getByText("No accounts recorded.", { exact: true }),
  ).toHaveCount(0);
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "0001",
  );
  await accessibility(page, "unavailable");
  await box.screenshot({ path: resolve(captures, "unavailable.png") });
  await box.getByLabel("Analysis account", { exact: true }).selectOption("");
  await box.getByRole("button", { name: "Retry analysis account" }).click();
  await expect(box.getByRole("status")).toHaveText("101–200 of 205 accounts");
  await expect(box.getByRole("status")).toBeFocused();
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "",
  );
});

test("empty canonical scope is explicit and has no continuation", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  core({ action: "view" });
  await open(page);
  for (const label of [
    "Analysis account",
    "Comparison account",
    "Analysis currency",
    "Comparison currency",
  ]) {
    const box = facet(page, label);
    await expect(box.getByRole("status")).toHaveText(
      `No ${label.endsWith("account") ? "accounts" : "currencies"} recorded.`,
    );
    await expect(box.locator("option")).toHaveCount(1);
    await expect(
      box.getByRole("button", { name: `Next ${label.toLowerCase()} page` }),
    ).toBeDisabled();
    await expect(box.getByRole("alert")).toHaveCount(0);
  }
  await accessibility(page, "empty");
});

test("canonical stale failure preserves drafts and existing result, refresh restarts choices at new revision", async ({
  page,
}) => {
  await firstReady(page);
  const box = facet(page, "Analysis account");
  const panel = page.getByRole("region", {
    name: "Transaction patterns",
    exact: true,
  });
  await box
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0001");
  await panel.getByLabel("From date", { exact: true }).fill("2025-01-01");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("1 in scope / 205 workspace rows"),
  ).toBeVisible();
  revision = importRows("synthetic-new-facet.csv", [
    "NEW,2025-01-01,Synthetic new facet,-2.00,AUD",
  ]).revision;
  await box.getByRole("button", { name: "Next analysis account page" }).click();
  await expect(box.getByRole("alert")).toContainText(
    "Transaction facet revision changed",
  );
  await expect(
    panel.getByText("1 in scope / 205 workspace rows"),
  ).toBeVisible();
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "0001",
  );
  await panel
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(box.getByRole("status")).toHaveText("1–100 of 206 accounts");
  await expect(box).toContainText(`revision ${revision}`);
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "0001",
  );
  await expect(panel.getByLabel("From date", { exact: true })).toHaveValue(
    "2025-01-01",
  );
  await expect(
    panel.getByText("1 in scope / 205 workspace rows"),
  ).toBeVisible();
  await expect(panel.locator(".patterns-revision")).toContainText("STALE");
});

test("one active and only the latest pending revision per selector; old real responses cannot publish", async ({
  page,
}) => {
  const requests: Record<string, any>[] = [];
  let release!: () => void;
  let held = false;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (
      request.action !== "page_transaction_facets" ||
      request.request.facet !== "account"
    ) {
      await route.continue();
      return;
    }
    requests.push(request);
    if (!held) {
      held = true;
      const response = await route.fetch();
      await gate;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await open(page);
  await expect.poll(() => requests.length).toBe(2);
  const heldBox = page.locator(
    '.transaction-facet[data-facet-kind="account"][aria-busy="true"]',
  );
  await expect(heldBox).toHaveCount(1);
  const label = await heldBox.getAttribute("data-facet-label");
  const box = facet(page, label!);
  const panel = page.getByRole("region", {
    name: "Transaction patterns",
    exact: true,
  });
  const middle = importRows("synthetic-middle.csv", [
    "MIDDLE,2025-01-01,Synthetic intermediate,-1.00,AUD",
  ]).revision;
  await panel
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(box).toContainText(`revision ${middle}`);
  await expect
    .poll(
      () =>
        requests.filter((request) => request.expected_revision === middle)
          .length,
    )
    .toBe(1);
  const latest = importRows("synthetic-latest.csv", [
    "LATEST,2025-01-01,Synthetic latest,-1.00,AUD",
  ]).revision;
  await panel
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(box).toContainText(`revision ${latest}`);
  await expect
    .poll(
      () =>
        requests.filter((request) => request.expected_revision === latest)
          .length,
    )
    .toBe(1);
  await expect(box.getByRole("status")).toHaveText("Loading accounts…");
  release();
  await expect(box.getByRole("status")).toHaveText("1–100 of 207 accounts");
  await expect(box).toContainText(`revision ${latest}`);
  expect(
    requests.filter((request) => request.expected_revision === middle),
  ).toHaveLength(1);
  expect(
    requests.filter((request) => request.expected_revision === latest),
  ).toHaveLength(2);
});

test("delayed continuation never restores a changed draft selection or steals deliberately moved then blurred focus", async ({
  page,
}) => {
  await firstReady(page);
  const box = facet(page, "Analysis account");
  await box
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0001");
  let release!: () => void;
  let captured = false;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (
      request.action === "page_transaction_facets" &&
      request.request.facet === "account" &&
      request.request.cursor
    ) {
      const response = await route.fetch();
      captured = true;
      await gate;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await box.getByRole("button", { name: "Next analysis account page" }).click();
  await expect.poll(() => captured).toBe(true);
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "0001",
  );
  await box.getByLabel("Analysis account", { exact: true }).selectOption("");
  await box.getByLabel("Analysis account", { exact: true }).focus();
  await box
    .getByLabel("Analysis account", { exact: true })
    .evaluate((element) => element.blur());
  release();
  await expect(box.getByRole("status")).toHaveText("101–200 of 205 accounts");
  await expect(box.getByLabel("Analysis account", { exact: true })).toHaveValue(
    "",
  );
  await expect(box.getByRole("status")).not.toBeFocused();
  expect(
    await page.evaluate(
      () =>
        document.activeElement === document.body ||
        document.activeElement === document.documentElement,
    ),
  ).toBe(true);
});

test("pager completion restores its status when focus falls to documentElement without another control taking focus", async ({
  page,
}) => {
  await firstReady(page);
  const box = facet(page, "Analysis account");
  let release!: () => void;
  let captured = false;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (
      request.action === "page_transaction_facets" &&
      request.request.facet === "account" &&
      request.request.cursor
    ) {
      const response = await route.fetch();
      captured = true;
      await gate;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await box.getByRole("button", { name: "Next analysis account page" }).click();
  await expect.poll(() => captured).toBe(true);
  await page.evaluate(() => {
    document.documentElement.tabIndex = -1;
    document.documentElement.focus();
  });
  expect(
    await page.evaluate(
      () => document.activeElement === document.documentElement,
    ),
  ).toBe(true);
  release();
  await expect(box.getByRole("status")).toHaveText("101–200 of 205 accounts");
  await expect(box.getByRole("status")).toBeFocused();
});
