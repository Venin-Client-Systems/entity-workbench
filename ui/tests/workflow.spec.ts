import { test, expect } from "@playwright/test";
import { rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
test.beforeEach(() =>
  rmSync(resolve("artifacts/synthetic-ui-workspace"), {
    recursive: true,
    force: true,
  }),
);
test.afterEach(async ({ page }, info) => {
  if (info.status !== info.expectedStatus)
    console.log(
      "Workspace alerts:",
      await page.getByRole("alert").allTextContents(),
    );
});
test("synthetic investigation flows through the real Rust workspace", async ({
  page,
}) => {
  const external: string[] = [];
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => {
    if (
      !r.url().startsWith("http://127.0.0.1:1420") &&
      !r.url().startsWith("blob:") &&
      !r.url().startsWith("data:")
    )
      external.push(r.url());
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Load synthetic investigation" })
    .click();
  await expect(
    page.getByText("What relationship is supported by the records?"),
  ).toBeVisible();
  const audit = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  writeFileSync(
    "artifacts/design-accessibility-audit.json",
    JSON.stringify(
      {
        screen: "Overview",
        violations: audit.violations,
        incomplete: audit.incomplete,
      },
      null,
      2,
    ),
  );
  expect(audit.violations).toEqual([]);
  await page.screenshot({ path: "artifacts/ui-overview.png", fullPage: true });
  await page
    .getByRole("button", { name: "Transactions", exact: false })
    .first()
    .click();
  await page.getByRole("button", { name: "Harbour Cafe OCR review" }).click();
  const review = page.getByRole("complementary", {
    name: "Transaction review",
  });
  await expect(review).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await expect(
    review.getByRole("region", { name: "Original transaction excerpt" }),
  ).toContainText("-180.00");
  expect(
    (
      await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
        .analyze()
    ).violations,
  ).toEqual([]);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: "artifacts/ui-transaction-review-1440.png" });
  await page
    .getByLabel("Transaction decision reason")
    .fill("Unsaved review draft");
  await page.setViewportSize({ width: 960, height: 640 });
  await expect(
    page.getByRole("dialog", { name: "Transaction review" }),
  ).toBeVisible();
  await expect(page.getByLabel("Transaction decision reason")).toHaveValue(
    "Unsaved review draft",
  );
  await page.setViewportSize({ width: 1440, height: 1000 });
  await expect(review).toBeVisible();
  await expect(page.getByLabel("Transaction decision reason")).toHaveValue(
    "Unsaved review draft",
  );
  await page.getByLabel("Transaction decision reason").fill("");
  await page.getByLabel("Currency filter", { exact: true }).selectOption("USD");
  await page.getByRole("button", { name: "Apply ledger filters" }).click();
  await expect(
    review
      .getByRole("status")
      .filter({ hasText: "outside the current filters" }),
  ).toContainText("outside the current filters");
  await expect(review).toContainText("Harbour Cafe OCR review");
  await page.getByLabel("Currency filter", { exact: true }).selectOption("");
  await page.getByRole("button", { name: "Apply ledger filters" }).click();
  await expect(
    review.getByText("Selected transaction is outside the current filters."),
  ).not.toBeVisible();
  await page.getByRole("button", { name: "Inspect preserved source" }).click();
  await expect(
    page.getByRole("button", { name: "Close source" }),
  ).toBeFocused();
  await expect(
    page.getByRole("region", { name: "Source anchor excerpt" }),
  ).toContainText("-180.00");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Evidence source" }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Inspect preserved source" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("complementary", { name: "Transaction review" }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Harbour Cafe OCR review" }),
  ).toBeFocused();
  await page.getByRole("button", { name: "Harbour Cafe OCR review" }).click();
  await page
    .getByLabel("Transaction decision reason")
    .fill("Reviewed synthetic source balance");
  await page.getByLabel("Corrected amount").fill("-18.00");
  await page
    .getByRole("button", { name: "Save correction for review" })
    .click();
  await page.getByRole("button", { name: "Harbour Cafe OCR review" }).click();
  await expect(page.getByLabel("Corrected amount")).toHaveValue("-18.00");
  await page
    .getByLabel("Transaction decision reason")
    .fill("Corrected amount reconciles");
  await page.getByRole("button", { name: "Accept", exact: true }).click();
  await expect(page.getByText("AUD reviewed net")).toBeVisible();
  await page
    .getByRole("button", { name: "Entities", exact: false })
    .first()
    .click();
  await expect(page.getByText("000042", { exact: true })).toBeVisible();
  await page
    .getByLabel("Identity decision reason")
    .fill("Synthetic mistaken identity decision");
  await page.getByRole("button", { name: "Merge selected records" }).click();
  await expect(
    page.getByText(/Merged into Rowan Ellis.*original observations retained/),
  ).toBeVisible();
  await expect(
    page.getByText("Processing workspace action…"),
  ).not.toBeVisible();
  await page
    .getByLabel("Identity decision reason")
    .fill("Birth years conflict");
  await page
    .getByRole("button", { name: "Reverse merge", exact: true })
    .click();
  await expect(
    page.getByText(/Merged into Rowan Ellis.*original observations retained/),
  ).not.toBeVisible();
  await page
    .getByRole("button", { name: "Locations", exact: false })
    .first()
    .click();
  await expect(
    page.getByText("Regional basemap coverage is unavailable.", {
      exact: false,
    }),
  ).toBeVisible();
  await page.screenshot({ path: "artifacts/ui-locations.png", fullPage: true });
  await page
    .getByRole("button", { name: "Assessment", exact: false })
    .first()
    .click();
  await page.getByRole("button", { name: "Save report snapshot" }).click();
  await expect(
    page.getByRole("button", { name: "Export self-contained HTML" }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByText("What relationship is supported by the records?"),
  ).toBeVisible();
  const screens = [];
  for (const section of [
    "Evidence",
    "Entities",
    "Transactions",
    "Relationships",
    "Locations",
    "Discovery",
    "Assessment",
  ]) {
    await page
      .getByRole("navigation")
      .getByRole("button", { name: new RegExp(section) })
      .click();
    const result = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    screens.push({
      screen: section,
      violations: result.violations,
      incomplete: result.incomplete,
    });
  }
  writeFileSync(
    "artifacts/design-accessibility-screens.json",
    JSON.stringify(screens, null, 2),
  );
  expect(
    screens.flatMap((s) =>
      s.violations.map((v) => ({
        screen: s.screen,
        rule: v.id,
        targets: v.nodes.map((n) => n.target),
      })),
    ),
  ).toEqual([]);
  await page.setViewportSize({ width: 960, height: 640 });
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  await page
    .getByRole("button", { name: "Export JSON" })
    .scrollIntoViewIfNeeded();
  await expect(
    page.getByRole("button", { name: "Export JSON" }),
  ).toBeInViewport();
  await page.getByRole("button", { name: "Harbour Cafe OCR review" }).click();
  const compactReview = page.getByRole("dialog", {
    name: "Transaction review",
  });
  await expect(compactReview).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await expect(page.getByLabel("Transfer counterpart")).toBeEnabled();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByLabel("Transfer counterpart")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  expect(
    (
      await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
        .analyze()
    ).violations,
  ).toEqual([]);
  await page.screenshot({ path: "artifacts/ui-transaction-review-960.png" });
  await page.keyboard.press("Escape");
  await expect(compactReview).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Harbour Cafe OCR review" }),
  ).toBeFocused();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "artifacts/ui-transactions-minimum.png",
    fullPage: true,
  });
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("analyst-authored identities retain sources, decisions and corrections", async ({
  page,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => {
    if (
      !r.url().startsWith("http://127.0.0.1:1420") &&
      !r.url().startsWith("blob:") &&
      !r.url().startsWith("data:")
    )
      external.push(r.url());
  });
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Load synthetic investigation" }),
  ).toBeVisible();
  await page.locator("input[type=file]").setInputFiles({
    name: "identities.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("Avery Vale born 1982\nAvery Vale born 1990\n"),
  });
  await expect(
    page.getByRole("button", { name: /TXT identities.txt/ }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Entities/ })
    .click();
  for (const number of ["000021", "000022"]) {
    await page.getByRole("button", { name: "Add entity", exact: true }).click();
    await page.getByLabel("Entity name", { exact: true }).fill("Avery Vale");
    await page
      .getByLabel("Reference namespace 1", { exact: true })
      .fill("CASE");
    await page.getByLabel("Reference Number 1", { exact: true }).fill(number);
    await page
      .getByLabel("Entity change reason")
      .fill("Created from a distinct synthetic source mention");
    await page
      .getByRole("button", { name: "Save entity", exact: true })
      .click();
    await expect(
      page.getByRole("region", {
        name: `Avery Vale · CASE:${number}`,
        exact: true,
      }),
    ).toBeVisible();
  }
  await expect(
    page.getByText("DEMO RECORDS PRESENT", { exact: true }),
  ).not.toBeVisible();
  await page
    .getByLabel("First entity", { exact: true })
    .selectOption({ label: "Avery Vale · CASE:000021" });
  await page
    .getByLabel("Second entity", { exact: true })
    .selectOption({ label: "Avery Vale · CASE:000022" });
  for (const [number, year, line] of [
    ["000021", "1982", "1"],
    ["000022", "1990", "2"],
  ]) {
    const card = page.getByRole("region", {
      name: `Avery Vale · CASE:${number}`,
      exact: true,
    });
    await card
      .getByRole("button", { name: "Add observation", exact: true })
      .click();
    await page
      .getByLabel("Observation field", { exact: true })
      .fill("birth_year");
    await page.getByLabel("Observation value", { exact: true }).fill(year);
    await page.getByLabel("First source line", { exact: true }).fill(line);
    await page.getByLabel("Last source line", { exact: true }).fill(line);
    await page
      .getByLabel("Observation change reason")
      .fill("Transcribed a synthetic source mention");
    await page
      .getByRole("button", { name: "Save observation", exact: true })
      .click();
    await card
      .getByRole("button", { name: "Review observation", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Inspect observation source" })
      .click();
    await expect(
      page.getByRole("region", { name: "Source anchor excerpt" }),
    ).toContainText(`Avery Vale born ${year}`);
    await page
      .getByRole("button", { name: "Close source", exact: true })
      .click();
    await page
      .getByLabel("Observation review reason")
      .fill("Inspected the cited source line");
    await page
      .getByRole("button", { name: "Accept observation", exact: true })
      .click();
    await expect(
      page.getByRole("dialog", { name: "Observation review" }),
    ).not.toBeVisible();
  }
  await expect(
    page.getByRole("heading", {
      name: "birth year · Different reviewed values",
    }),
  ).toBeVisible();
  await expect(
    page.getByText("1 source groups among accepted observations"),
  ).toBeVisible();
  // Keep the next real comparison response in flight while a decision changes
  // the workspace revision. A previous comparison must not enable a new action.
  let releaseComparison!: () => void;
  let heldComparison = false;
  const comparisonGate = new Promise<void>((resolve) => {
    releaseComparison = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "compare_entities") {
      const response = await route.fetch();
      heldComparison = true;
      await comparisonGate;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page
    .getByLabel("Identity decision reason")
    .fill("Conflicting birth years; retain namesakes");
  await page
    .getByRole("button", { name: "Keep separate", exact: true })
    .click();
  const decisionHistory = page.getByRole("region", {
    name: "Identity decision history",
  });
  await expect(
    decisionHistory.getByText("Conflicting birth years; retain namesakes"),
  ).toBeVisible();
  await page
    .getByLabel("Identity decision reason")
    .fill("Await an independent confirming record");
  await expect.poll(() => heldComparison).toBe(true);
  await expect(
    page.getByRole("button", { name: "Defer identity decision" }),
  ).toBeDisabled();
  releaseComparison();
  await expect(
    page.getByRole("button", { name: "Defer identity decision" }),
  ).toBeEnabled();
  await expect(page.getByLabel("Identity decision reason")).toHaveValue(
    "Await an independent confirming record",
  );
  await page.unrouteAll({ behavior: "wait" });
  await page.getByRole("button", { name: "Defer identity decision" }).click();
  await expect(
    decisionHistory.getByText("Await an independent confirming record"),
  ).toBeVisible();
  const second = page.getByRole("region", {
    name: "Avery Vale · CASE:000022",
    exact: true,
  });
  await second.getByRole("button", { name: "Review observation" }).click();
  await page
    .getByRole("button", { name: "Correct observation", exact: true })
    .click();
  await page.getByLabel("Observation value", { exact: true }).fill("1982");
  await page
    .getByLabel("Observation change reason")
    .fill("Synthetic correction; original source retained");
  await page
    .getByRole("button", { name: "Save observation correction" })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "birth year · Insufficient reviewed evidence",
    }),
  ).toBeVisible();
  await second
    .getByRole("button", { name: "Inspect source", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Source anchor excerpt" }),
  ).toContainText("Avery Vale born 1990");
  await page.getByRole("button", { name: "Close source", exact: true }).click();
  const audit = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  writeFileSync(
    "artifacts/identity-accessibility-audit.json",
    JSON.stringify(
      { violations: audit.violations, incomplete: audit.incomplete },
      null,
      2,
    ),
  );
  expect(audit.violations).toEqual([]);
  await page.screenshot({
    path: "artifacts/ui-identity-authoring.png",
    fullPage: true,
  });
  await page.reload();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Entities/ })
    .click();
  await expect(
    decisionHistory.getByText("Conflicting birth years; retain namesakes"),
  ).toBeVisible();
  await expect(
    decisionHistory.getByText("Await an independent confirming record"),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Avery Vale · CASE:000021", exact: true }),
  ).toBeVisible();
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});
