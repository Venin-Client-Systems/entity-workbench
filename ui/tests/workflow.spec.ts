import { test, expect } from "@playwright/test";
import { rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
test.beforeAll(() =>
  rmSync(resolve("artifacts/synthetic-ui-workspace"), {
    recursive: true,
    force: true,
  }),
);
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
  await expect(
    page.getByRole("dialog", { name: "Transaction review" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByLabel("Transfer counterpart")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await page.getByRole("button", { name: "Inspect preserved source" }).click();
  await expect(
    page.getByRole("button", { name: "Close source" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Evidence source" }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Inspect preserved source" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Transaction review" }),
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
  await expect(page.getByText("DEMO:000042")).toBeVisible();
  await page
    .getByLabel("Identity decision reason")
    .fill("Synthetic mistaken identity decision");
  await page
    .getByRole("button", { name: "Record merge of the two people" })
    .click();
  await expect(
    page.getByText("Merged into person-b; original observations retained."),
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
    page.getByText("Merged into person-b; original observations retained."),
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
  await expect(
    page.getByRole("button", { name: "Export JSON" }),
  ).toBeInViewport();
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
