import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace } from "../src/types";
const root = resolve("artifacts/synthetic-ui-workspace"),
  executable = resolve("target/debug/ew-dev"),
  captures = resolve("artifacts/transaction-comparison");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let workspace: Workspace;
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  workspace = core({
    action: "import",
    name: "synthetic-comparison.csv",
    bytes: [...readFileSync("fixtures/transactions/comparison.csv")],
  }).workspace;
  const rows = workspace.transactions;
  for (let index = 0; index < rows.length; index++) {
    if (index === 3) continue;
    workspace = core({
      action: "review_transaction",
      id: rows[index].id,
      state: index === 6 ? "rejected" : index === 7 ? "deferred" : "accepted",
      reason: "Synthetic comparison source review.",
      expected_revision: workspace.revision,
    }).workspace;
  }
  workspace = core({
    action: "match_transfer",
    first: rows[8].id,
    second: rows[9].id,
    reason: "Synthetic reviewed transfer.",
    expected_revision: workspace.revision,
  }).workspace;
});
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  return page.getByRole("region", {
    name: "Transaction period comparison",
    exact: true,
  });
}
async function calculate(page: Page) {
  const panel = await open(page);
  for (const [label, value] of [
    ["Baseline from", "2025-01-01"],
    ["Baseline through", "2025-01-31"],
    ["Comparison from", "2025-02-01"],
    ["Comparison through", "2025-02-28"],
  ])
    await panel.getByLabel(label, { exact: true }).fill(value);
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(
    panel.getByText(
      "12 workspace rows / 12 match account and currency / 0 outside those filters.",
    ),
  ).toBeVisible();
  await expect(panel.getByText(/Scope edits are not applied/)).toHaveCount(0);
  return panel;
}
async function axe(page: Page, selector: string, name: string) {
  const result = await new AxeBuilder({ page })
    .include(selector)
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `accessibility-${name}.json`),
    JSON.stringify(
      {
        scope: selector,
        tags: ["wcag2a", "wcag2aa", "wcag21aa"],
        violations: result.violations,
      },
      null,
      2,
    ) + "\n",
  );
  expect(result.violations).toEqual([]);
}
test("exact period amounts, full review partitions and inert sources use the real canonical command", async ({
  page, baseURL,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("request", (request) => {
    if (
      !request.url().startsWith(`${baseURL}/`) &&
      !/^(blob:|data:)/.test(request.url())
    )
      external.push(request.url());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  const panel = await calculate(page),
    group = panel.getByRole("region", {
      name: "Period totals 0001 / AUD",
      exact: true,
    });
  await expect(
    group.getByRole("row").filter({ hasText: "Credits" }),
  ).toContainText("20.00 / 100.00 × 100%");
  await expect(
    group.getByRole("row").filter({ hasText: "Debit magnitude" }),
  ).toContainText("10.00 / 20.00 × 100%");
  await expect(group.getByRole("row").filter({ hasText: "Net" })).toContainText(
    "10.00 / 80.00 × 100%",
  );
  await expect(
    panel.getByText(/Unequal period lengths: 31 versus 28/),
  ).toBeVisible();
  const baseline = group.getByRole("region", {
      name: "Baseline review denominator 0001 / AUD",
    }),
    comparison = group.getByRole("region", {
      name: "Comparison review denominator 0001 / AUD",
    });
  for (const [scope, label] of [
    [baseline, "Included (3)"],
    [baseline, "Pending (1)"],
    [comparison, "Rejected (1)"],
    [comparison, "Deferred (1)"],
  ] as const)
    await expect(
      scope.getByRole("button", { name: label, exact: true }),
    ).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "Outside selected periods (2)" }),
  ).toBeEnabled();
  await baseline.getByRole("button", { name: "Included (3)" }).click();
  const dialog = page.getByRole("dialog", { name: "Comparison source rows" });
  await expect(dialog.locator(".patterns-source")).toHaveCount(3);
  await expect(
    dialog.getByText("<img src=x onerror=window.comparisonExecuted=true>", {
      exact: true,
    }),
  ).toHaveCount(2);
  await expect(dialog.locator("img,script,svg,iframe")).toHaveCount(0);
  await expect(
    dialog.getByText(
      "Possible duplicate: retained in its review-state partition.",
    ),
  ).toHaveCount(2);
  await dialog
    .getByRole("button", {
      name: "Inspect source and review row 3",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await expect(
    page.getByText("Preserved source value", { exact: true }),
  ).toBeVisible();
  await expect(
    page.locator(".transaction-review img,.transaction-review iframe"),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as unknown as Record<string, unknown>).comparisonExecuted,
    ),
  ).toBeUndefined();
  expect(core({ action: "view" }).workspace.revision).toBe(workspace.revision);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});
test("draft scope stays unapplied; selected order, empty periods and zero or negative baselines stay explicit", async ({
  page,
}) => {
  const panel = await calculate(page);
  await panel.getByLabel("Baseline from", { exact: true }).fill("2025-02-01");
  await panel
    .getByLabel("Baseline through", { exact: true })
    .fill("2025-02-28");
  await panel.getByLabel("Comparison from", { exact: true }).fill("2025-01-01");
  await panel
    .getByLabel("Comparison through", { exact: true })
    .fill("2025-01-31");
  await expect(panel.getByText(/Scope edits are not applied/)).toBeVisible();
  await expect(panel.getByLabel("Applied comparison scope")).toContainText(
    "A / BASELINE · 31 DAYS",
  );
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(
    panel.getByText(/Baseline is chronologically later/),
  ).toBeVisible();
  await expect(
    panel
      .getByRole("region", { name: "Period totals 0001 / AUD", exact: true })
      .getByRole("row")
      .filter({ hasText: "Credits" }),
  ).toContainText("-20.00 / 120.00 × 100%");
  const usd = panel.getByRole("region", {
    name: "Period totals 0001 / USD",
    exact: true,
  });
  await expect(
    usd.getByText("No percentage: zero baseline (comparison nonzero)"),
  ).toHaveCount(2);
  await panel.getByLabel("Baseline from", { exact: true }).fill("2025-01-01");
  await panel
    .getByLabel("Baseline through", { exact: true })
    .fill("2025-01-31");
  await panel.getByLabel("Comparison from", { exact: true }).fill("2025-02-01");
  await panel
    .getByLabel("Comparison through", { exact: true })
    .fill("2025-02-28");
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(usd.getByText("No percentage: negative baseline")).toBeVisible();
  await panel
    .getByLabel("Comparison account", { exact: true })
    .selectOption("0003");
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(
    panel.getByText(/No imported transactions match either/),
  ).toBeVisible();
  await expect(panel.locator(".comparison-group")).toHaveCount(0);
  await expect(
    panel.getByRole("button", { name: "Outside selected periods (1)" }),
  ).toBeEnabled();
});
test("pending-only scope differs from an empty scope and reviewed transfer exclusion is explicit", async ({
  page,
}) => {
  const panel = await calculate(page);
  await panel.getByLabel("Baseline from", { exact: true }).fill("2025-01-03");
  await panel
    .getByLabel("Baseline through", { exact: true })
    .fill("2025-01-03");
  await panel
    .getByLabel("Comparison account", { exact: true })
    .selectOption("0001");
  await panel
    .getByLabel("Comparison currency", { exact: true })
    .selectOption("AUD");
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  const baseline = panel.getByRole("region", {
    name: "Baseline review denominator 0001 / AUD",
  });
  await expect(baseline.getByText(/No accepted included rows/)).toBeVisible();
  await expect(
    baseline.getByRole("button", { name: "Included (0)" }),
  ).toBeDisabled();
  await baseline.getByRole("button", { name: "Pending (1)" }).click();
  const dialog = page.getByRole("dialog", { name: "Comparison source rows" });
  await expect(
    dialog.getByText("Synthetic pending", { exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    baseline.getByRole("button", { name: "Pending (1)" }),
  ).toBeFocused();
  await panel.getByLabel("Baseline from", { exact: true }).fill("2025-01-01");
  await panel
    .getByLabel("Baseline through", { exact: true })
    .fill("2025-01-31");
  await panel
    .getByLabel("Comparison account", { exact: true })
    .selectOption("0002");
  await panel
    .getByLabel("Comparison transfers", { exact: true })
    .selectOption("exclude_reviewed_pairs");
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await panel.getByRole("button", { name: "Excluded transfers (1)" }).click();
  await expect(
    dialog.getByText(/Verified reviewed transfer peer:/),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await panel
    .getByRole("button", { name: "Verified transfer counterparts (1)" })
    .click();
  await expect(
    dialog.getByText("Synthetic transfer in", { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByText(/not an additional contribution/),
  ).toBeVisible();
});
test("stale canonical revisions and overlap failures retain honest unavailable results", async ({
  page,
}) => {
  const panel = await calculate(page);
  workspace = core({
    action: "correct_transaction",
    id: workspace.transactions[0].id,
    amount: "101.00",
    reason: "Synthetic external correction.",
    expected_revision: workspace.revision,
  }).workspace;
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(
    panel.getByText(/Comparison is stale or could not be revalidated/),
  ).toBeVisible();
  await expect(
    panel.getByRole("button", { name: "Included (3)", exact: true }),
  ).toBeDisabled();
  await panel
    .getByRole("button", { name: "Refresh workspace for comparison" })
    .click();
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(panel.getByText(/Comparison is stale/)).toHaveCount(0);
  await expect(
    panel
      .getByRole("region", { name: "Baseline review denominator 0001 / AUD" })
      .getByRole("button", { name: "Pending (2)" }),
  ).toBeEnabled();
  await panel.getByLabel("Comparison from", { exact: true }).fill("2025-01-31");
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await expect(
    panel.getByText(/Comparison periods must not overlap/),
  ).toBeVisible();
  await expect(panel.getByText(/Comparison is stale/)).toBeVisible();
});
test("late core replies after navigation cannot populate a remounted comparison", async ({
  page,
}) => {
  const panel = await calculate(page);
  let release: () => void = () => {},
    started: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve)),
    pending = new Promise<void>((resolve) => (started = resolve));
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON()?.action === "compare_transaction_periods"
    ) {
      started();
      await held;
    }
    await route.continue();
  });
  await panel.getByRole("button", { name: "Compare selected periods" }).click();
  await pending;
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  const delivered = page.waitForResponse(
    (response) =>
      response.request().postDataJSON()?.action ===
      "compare_transaction_periods",
  );
  release();
  await (await delivered).finished();
  await expect(
    panel.getByText("NOT CALCULATED", { exact: true }),
  ).toBeVisible();
  await expect(panel.locator(".comparison-group")).toHaveCount(0);
});
test("desktop and compact comparison and source dialogs retain keyboard access and zero axe violations", async ({
  page,
}) => {
  const panel = await calculate(page);
  await axe(page, ".comparison", "desktop");
  await page.setViewportSize({ width: 1440, height: 3200 });
  await panel.screenshot({
    path: resolve(captures, "comparison-full-surface.png"),
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await panel.evaluate((element) => {
    element.scrollIntoView();
    window.scrollBy(0, -110);
  });
  await page.screenshot({ path: resolve(captures, "comparison-desktop.png") });
  const opener = panel
    .getByRole("region", { name: "Baseline review denominator 0001 / AUD" })
    .getByRole("button", { name: "Included (3)" });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Comparison source rows" });
  await axe(
    page,
    'dialog[aria-label="Comparison source rows"]',
    "source-desktop",
  );
  await dialog.screenshot({ path: resolve(captures, "source-desktop.png") });
  await expect(
    dialog.getByRole("button", { name: "Close comparison source rows" }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(
    dialog.getByRole("button", {
      name: "Inspect source and review row 4",
      exact: true,
    }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close comparison source rows" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  await page.setViewportSize({ width: 720, height: 900 });
  await axe(page, ".comparison", "compact");
  expect(
    await panel.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await panel.evaluate((element) => {
    element.scrollIntoView();
    window.scrollBy(0, -110);
  });
  await page.screenshot({ path: resolve(captures, "comparison-compact.png") });
  await opener.click();
  await axe(
    page,
    'dialog[aria-label="Comparison source rows"]',
    "source-compact",
  );
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await dialog.screenshot({ path: resolve(captures, "source-compact.png") });
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
});
test("an arriving canonical refresh hides stale open source rows and restores the compare control", async ({
  page,
}) => {
  const panel = await calculate(page);
  workspace = core({
    action: "correct_transaction",
    id: workspace.transactions[0].id,
    amount: "101.00",
    reason: "Synthetic delayed refresh.",
    expected_revision: workspace.revision,
  }).workspace;
  let release: () => void = () => {},
    started: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve)),
    pending = new Promise<void>((resolve) => (started = resolve));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "view") {
      started();
      await held;
    }
    await route.continue();
  });
  await page
    .getByRole("region", { name: "Transaction patterns", exact: true })
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await pending;
  await panel
    .getByRole("region", { name: "Baseline review denominator 0001 / AUD" })
    .getByRole("button", { name: "Included (3)" })
    .click();
  const dialog = page.getByRole("dialog", { name: "Comparison source rows" });
  await expect(dialog.getByRole("alert")).toContainText(
    "Source rows could not be verified",
  );
  await expect(dialog.locator(".patterns-source")).toHaveCount(0);
  const delivered = page.waitForResponse(
    (response) => response.request().postDataJSON()?.action === "view",
  );
  release();
  await (await delivered).finished();
  await expect(dialog.getByRole("alert")).toContainText(
    "Source rows are unavailable for this stale result",
  );
  await expect(dialog.locator(".patterns-source")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(
    panel.getByRole("button", { name: "Compare selected periods" }),
  ).toBeFocused();
});
