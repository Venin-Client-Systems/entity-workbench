import { test, expect, type Page } from "@playwright/test";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";

const source = readFileSync("fixtures/statement-mapped.csv");
test.beforeEach(() =>
  rmSync(resolve("artifacts/synthetic-ui-workspace"), {
    recursive: true,
    force: true,
  }),
);
async function configure(page: Page) {
  await page
    .getByLabel("Column separator", { exact: true })
    .selectOption("semicolon");
  await expect(
    page.getByRole("region", { name: "Statement source sample" }),
  ).toContainText("000017");
  for (const [label, value] of [
    ["Statement date format", "day_first"],
    ["Statement number format", "comma_decimal"],
    ["Source row order", "newest_first"],
    ["Amount interpretation", "separate"],
    ["Transaction date column", "Booked"],
    ["Description column", "Narrative"],
    ["Balance column", "Running balance"],
    ["Debit column", "Paid out"],
    ["Credit column", "Paid in"],
    ["Account source", "column"],
    ["Account column", "Account ref"],
    ["Currency source", "column"],
    ["Currency column", "Unit"],
  ])
    await page.getByLabel(label, { exact: true }).selectOption(value);
}
async function audit(page: Page, name: string) {
  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  writeFileSync(
    `artifacts/statement-${name}-accessibility.json`,
    JSON.stringify(
      { violations: result.violations, incomplete: result.incomplete },
      null,
      2,
    ),
  );
  expect(result.violations).toEqual([]);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
}

test("mapped statement preview retains sources, saves profiles and blocks stale or duplicate imports", async ({
  page,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => {
    if (!/^(http:\/\/127\.0\.0\.1:1420|blob:|data:)/.test(r.url()))
      external.push(r.url());
  });
  await page.goto("/");
  await page
    .locator("input[type=file]")
    .setInputFiles({
      name: "statement-mapped.csv",
      mimeType: "text/csv",
      buffer: source,
    });
  const dialog = page.getByRole("dialog", {
    name: "Import statement",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  await configure(page);
  await audit(page, "mapping");
  await page.screenshot({ path: "artifacts/ui-statement-mapping-1440.png" });
  // Delay a real response, edit its interpretation, then release it. It must never enable import.
  let release!: () => void,
    held = false;
  const gate = new Promise<void>((r) => {
    release = r;
  });
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "preview_statement") {
      const response = await route.fetch();
      held = true;
      await gate;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page.getByRole("button", { name: "Preview all rows" }).click();
  await expect.poll(() => held).toBe(true);
  await page.getByLabel("Statement date format").selectOption("month_first");
  release();
  await page.unrouteAll({ behavior: "wait" });
  await expect(
    page.getByRole("heading", { name: "Review interpreted transactions" }),
  ).not.toBeVisible();
  await page.getByLabel("Statement date format").selectOption("day_first");
  await page.getByRole("button", { name: "Preview all rows" }).click();
  await expect(
    page.getByRole("heading", { name: "Review interpreted transactions" }),
  ).toBeFocused();
  const preview = page.getByRole("region", {
    name: "Statement import preview",
  });
  await expect(preview).toContainText("2025-03-04");
  await expect(preview).toContainText("12,30");
  await expect(preview).toContainText("-12.30");
  await expect(preview).toContainText("000017");
  await expect(
    dialog.locator(".stat").filter({ hasText: "Balance mismatches" }),
  ).toHaveText("Balance mismatches0");
  await audit(page, "preview");
  await page.getByLabel("Save as a reusable mapping").check();
  await page.getByLabel("Mapping name").fill("Synthetic semicolon statement");
  await page.screenshot({ path: "artifacts/ui-statement-preview-1440.png" });
  await page.setViewportSize({ width: 960, height: 640 });
  await audit(page, "compact-preview");
  await page
    .getByRole("button", { name: "Import 3 pending transactions" })
    .focus();
  await expect(
    page.getByRole("button", { name: "Import 3 pending transactions" }),
  ).toBeInViewport();
  await page.screenshot({ path: "artifacts/ui-statement-preview-960.png" });
  await page
    .getByRole("button", { name: "Import 3 pending transactions" })
    .click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByText("Imported 3 transactions as pending review.", { exact: false }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Synthetic books receipt 04" })
    .click();
  await expect(
    page.getByRole("region", { name: "Original transaction excerpt" }),
  ).toContainText("12,30");
  await page.getByRole("button", { name: "Inspect preserved source" }).click();
  await expect(
    page.getByRole("region", { name: "Source anchor excerpt" }),
  ).toContainText("12,30");
  await expect(
    page.getByRole("dialog", { name: "Evidence source" }),
  ).toContainText("Paid out");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Inspect preserved source" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await page.reload();
  await page
    .locator("input[type=file]")
    .setInputFiles({
      name: "same-source.csv",
      mimeType: "text/csv",
      buffer: source,
    });
  await page
    .getByLabel("Saved mapping")
    .selectOption({ label: "Synthetic semicolon statement" });
  await expect(page.getByLabel("Statement date format")).toHaveValue(
    "day_first",
  );
  await page.getByRole("button", { name: "Preview all rows" }).click();
  await expect(dialog.getByRole("alert")).toContainText("already retained");
  await expect(
    page.getByRole("button", { name: "Import 3 pending transactions" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Close statement import" }).click();
  await page
    .locator("input[type=file]")
    .setInputFiles({
      name: "overlap.csv",
      mimeType: "text/csv",
      buffer: Buffer.from(
        source.toString() +
          "06/03/2025;Synthetic extra debit;1,00;;999,00;000017;AUD\n",
      ),
    });
  await page
    .getByLabel("Saved mapping")
    .selectOption({ label: "Synthetic semicolon statement" });
  await page.getByRole("button", { name: "Preview all rows" }).click();
  await page
    .getByRole("button", { name: "Import 4 pending transactions" })
    .click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByText("Imported 4 transactions as pending review.", { exact: false }),
  ).toBeVisible();
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("invalid amount rows block all imports and compact mapping remains usable", async ({
  page,
}) => {
  await page.goto("/");
  await page.setViewportSize({ width: 960, height: 640 });
  await page
    .locator("input[type=file]")
    .setInputFiles({
      name: "invalid-statement.csv",
      mimeType: "text/csv",
      buffer: Buffer.from(
        source.toString().replace(";;12,30;", ";2,00;12,30;"),
      ),
    });
  await configure(page);
  await audit(page, "compact-mapping");
  await page.getByRole("button", { name: "Preview all rows" }).focus();
  await page.screenshot({ path: "artifacts/ui-statement-mapping-960.png" });
  await page.getByRole("button", { name: "Preview all rows" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "No rows will be imported",
  );
  await expect(
    page.getByRole("button", { name: "Import 2 pending transactions" }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Import statement" }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Load synthetic investigation" }),
  ).toBeVisible();
});
