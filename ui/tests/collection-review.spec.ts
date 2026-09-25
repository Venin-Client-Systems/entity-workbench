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
import { resolve, dirname } from "node:path";
import { createHash } from "node:crypto";
import AxeBuilder from "@axe-core/playwright";
import type { CollectionReceipt } from "../src/collection-types";
import type { Workspace } from "../src/types";

const root = resolve("artifacts/synthetic-ui-workspace");
const executable = resolve("target/debug/ew-dev");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
let workspace: Workspace;
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  workspace = JSON.parse(
    execFileSync(executable, ["seed-collection-review", root], {
      encoding: "utf8",
    }),
  );
});
async function open(page: Page, path: string) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Discovery/ })
    .click();
  const opener = page.getByRole("button", {
    name: `Review collection https://archive.example${path}`,
    exact: true,
  });
  await opener.click();
  const dialog = page.getByRole("dialog", {
    name: "Collection review",
    exact: true,
  });
  await expect(
    dialog.getByText("Loading validated acquisition receipt…"),
  ).not.toBeVisible();
  return { dialog, opener };
}
function receipt(path: string): CollectionReceipt {
  const job = workspace.jobs.find(
    (item) => item.queries[0] === `https://archive.example${path}`,
  )!;
  return core({ action: "inspect_collection", job_id: job.id });
}
const captures = resolve("artifacts/collection-review");

test("request ancestry, escaped source, immutable export and keyboard review use the real core", async ({
  page,
  baseURL,
}) => {
  const errors: string[] = [],
    external: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (
      !request.url().startsWith(`${new URL(baseURL!).origin}/`) &&
      !/^(blob:|data:)/.test(request.url())
    )
      external.push(request.url());
  });
  const { dialog, opener } = await open(page, "/research");
  await expect(
    dialog.getByText("Synthetic replay", { exact: false }),
  ).toBeVisible();
  await expect(
    dialog.getByText("Quota exhausted", { exact: true }),
  ).toBeVisible();
  await expect(dialog.getByText("4 / 4", { exact: true })).toBeVisible();
  await expect(dialog.getByText("3 unique", { exact: true })).toBeVisible();
  const details = dialog.getByRole("region", { name: "Request details" });
  await expect(details.getByText("Retained · 0 bytes")).toBeVisible();
  await dialog
    .getByRole("button", {
      name: "Inspect request 04 https://archive.example/project/alpha",
      exact: true,
    })
    .click();
  await details
    .getByRole("button", { name: "Inspect parent request 03" })
    .click();
  await expect(
    details.getByRole("heading", { name: "Request 03", exact: true }),
  ).toBeVisible();
  await dialog.getByLabel("Show requests").selectOption("failed");
  await expect(
    dialog.getByText("No requests match this filter."),
  ).toBeVisible();
  await details
    .getByRole("button", { name: "Inspect parent request 02" })
    .click();
  await expect(dialog.getByLabel("Show requests")).toHaveValue("all");
  await expect(
    details.getByText("Redirect destination", { exact: true }),
  ).toBeVisible();
  await expect(
    details
      .locator("dd")
      .filter({ hasText: "https://archive.example/programme" }),
  ).toContainText("Recorded destination");
  await dialog
    .getByRole("button", {
      name: "Inspect request 03 https://archive.example/programme",
      exact: true,
    })
    .click();
  const sourceButton = details.getByRole("button", {
    name: "Inspect retained text",
  });
  await sourceButton.click();
  const source = page.getByRole("dialog", { name: "Retained collection text" });
  await expect(
    source.getByText("<script>window.syntheticExecution = true</script>", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(source.locator("script,img,iframe,svg")).toHaveCount(0);
  await expect(
    page.evaluate(() => "syntheticExecution" in window),
  ).resolves.toBe(false);
  await page.keyboard.press("Escape");
  await expect(source).not.toBeVisible();
  await expect(sourceButton).toBeFocused();
  mkdirSync(captures, { recursive: true });
  await dialog.evaluate((element) => {
    element.scrollTop = 0;
  });
  await dialog.screenshot({
    path: resolve(captures, "collection-review-desktop.png"),
  });
  // Additional tall viewport exposes the complete instrument for design comparison.
  await page.setViewportSize({ width: 1440, height: 2200 });
  await dialog.evaluate((element) => {
    element.scrollTop = 0;
  });
  await dialog.screenshot({
    path: resolve(captures, "collection-review-full-surface.png"),
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  const accessibility = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, "accessibility-desktop.json"),
    JSON.stringify(
      {
        violations: accessibility.violations,
        passes: accessibility.passes.length,
      },
      null,
      2,
    ),
  );
  expect(accessibility.violations).toEqual([]);

  await dialog
    .getByRole("button", { name: "Export acquisition bundle", exact: true })
    .click();
  const saved = dialog
    .getByRole("status")
    .filter({ hasText: "Acquisition bundle saved" });
  await expect(saved).toBeVisible();
  await saved.screenshot({
    path: resolve(captures, "collection-export-saved.png"),
  });
  const exportedPath = await saved.locator("dd").nth(0).innerText();
  const digest = await saved.locator("dd").nth(1).innerText();
  const bytes = readFileSync(resolve(root, exportedPath));
  expect(sha(bytes)).toBe(digest);
  const manifest = JSON.parse(bytes.toString());
  expect(manifest.receipt.mode).toBe("synthetic");
  expect(manifest.receipt.requests).toHaveLength(4);
  expect(manifest.originals).toHaveLength(3);
  for (const original of manifest.originals) {
    const originalBytes = readFileSync(
      resolve(root, dirname(exportedPath), original.path),
    );
    expect(sha(originalBytes)).toBe(original.sha256);
    expect(originalBytes.length).toBe(original.bytes);
  }
  // Saving the export and refreshing the workspace are separate real commands.
  // Hold the second refresh to exercise the interval where the saved receipt is
  // visible but modal actions remain disabled; do not focus disabled controls.
  let releaseRefresh!: () => void;
  let observedRefresh!: () => void;
  const refreshGate = new Promise<void>((resolve) => {
    releaseRefresh = resolve;
  });
  const refreshObserved = new Promise<void>((resolve) => {
    observedRefresh = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "view")
      return route.continue();
    const response = await route.fetch();
    observedRefresh();
    await refreshGate;
    await route.fulfill({ response });
  });
  await dialog
    .getByRole("button", { name: "Export acquisition bundle", exact: true })
    .click();
  await expect(saved.locator("dd").nth(0)).not.toHaveText(exportedPath);
  expect(readFileSync(resolve(root, exportedPath))).toEqual(bytes);
  expect(readdirSync(resolve(root, "exports"))).toHaveLength(2);
  await refreshObserved;
  const closeReview = dialog.getByRole("button", {
    name: "Close collection review",
  });
  await expect(closeReview).toBeDisabled();
  releaseRefresh();
  await expect(closeReview).toBeEnabled();
  await page.unroute("**/api/workbench");
  await closeReview.focus();
  await expect(closeReview).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(
    dialog.getByRole("button", {
      name: "Export acquisition bundle",
      exact: true,
    }),
  ).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close collection review" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(opener).toBeFocused();
  expect(errors).toEqual([]);
  expect(external).toEqual([]);
});

test("a zero-based parent sequence remains inspectable", async ({ page }) => {
  const { dialog } = await open(page, "/first-parent");
  await dialog
    .getByRole("button", {
      name: "Inspect request 02 https://archive.example/first-child",
      exact: true,
    })
    .click();
  await dialog
    .getByRole("button", { name: "Inspect parent request 01", exact: true })
    .click();
  await expect(
    dialog
      .getByRole("region", { name: "Request details" })
      .getByRole("heading", { name: "Request 01", exact: true }),
  ).toBeVisible();
});

test("incomplete, blocked and failed attempts preserve their different missing-response states", async ({
  page,
}) => {
  for (const [path, label, status] of [
    [
      "/incomplete",
      "Unavailable — response body was incomplete",
      "200 · Incomplete body",
    ],
    [
      "/blocked",
      "Unavailable — no complete response",
      "No HTTP status · blocked",
    ],
    [
      "/failed",
      "Unavailable — no complete response",
      "No HTTP status · failed",
    ],
  ]) {
    const { dialog } = await open(page, path);
    const details = dialog.getByRole("region", { name: "Request details" });
    await expect(details.getByText(label, { exact: true })).toBeVisible();
    await expect(details.getByText(status, { exact: true })).toBeVisible();
    await expect(
      details.getByText("Response SHA-256", { exact: true }),
    ).not.toBeVisible();
    await expect(dialog.getByText("1 / 50", { exact: true })).toBeVisible();
    await expect(dialog.getByText("0 unique", { exact: true })).toBeVisible();
    await expect(
      dialog.getByRole("button", {
        name: "Export acquisition bundle",
        exact: true,
      }),
    ).toBeEnabled();
    await page.keyboard.press("Escape");
  }
});

test("partial retention disables export and unavailable legacy receipt does not invent an empty result", async ({
  page,
}) => {
  const { dialog } = await open(page, "/partial-retention");
  await expect(dialog.getByText("Partial", { exact: true })).toBeVisible();
  await expect(
    dialog.getByText("Not retained — publication failed", { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", {
      name: "Export acquisition bundle",
      exact: true,
    }),
  ).toBeDisabled();
  await expect(dialog.getByRole("alert")).toContainText(
    "Retention is incomplete",
  );
  mkdirSync(captures, { recursive: true });
  await dialog.screenshot({
    path: resolve(captures, "collection-partial.png"),
  });
  await page.keyboard.press("Escape");
  const legacy = (await open(page, "/interrupted")).dialog;
  await expect(
    legacy.getByRole("heading", { name: "Acquisition receipt unavailable" }),
  ).toBeVisible();
  await expect(
    legacy.getByRole("button", {
      name: "Export acquisition bundle",
      exact: true,
    }),
  ).not.toBeVisible();
  await expect(
    legacy.getByRole("region", { name: "Charged request ledger" }),
  ).not.toBeVisible();
  await legacy
    .getByRole("button", { name: "Retry receipt inspection" })
    .click();
  await expect(legacy.getByRole("alert")).toContainText(
    "legacy or interrupted",
  );
  await legacy.screenshot({
    path: resolve(captures, "collection-unavailable.png"),
  });
});

test("empty originals and unsupported formats are retained without claiming searchable or relevant results", async ({
  page,
}) => {
  const { dialog } = await open(
    page,
    "/empty, https://archive.example/document.pdf",
  );
  await expect(
    dialog.getByText("Successful with no searchable pages", { exact: true }),
  ).toBeVisible();
  const details = dialog.getByRole("region", { name: "Request details" });
  await expect(
    details.getByText("Retained · 0 bytes", { exact: true }),
  ).toBeVisible();
  await expect(
    details.getByRole("button", { name: "Inspect retained text" }),
  ).not.toBeVisible();
  await dialog
    .getByRole("button", {
      name: "Inspect request 02 https://archive.example/document.pdf",
      exact: true,
    })
    .click();
  await expect(
    details.getByText("application/pdf", { exact: true }),
  ).toBeVisible();
  await expect(
    details.getByRole("button", { name: "Inspect retained text" }),
  ).not.toBeVisible();
  await expect(dialog.getByText("2 unique", { exact: true })).toBeVisible();
});

test("Rust rechecks originals at export and surfaces failure without reporting a saved bundle", async ({
  page,
}) => {
  const recorded = receipt("/research");
  const { dialog } = await open(page, "/research");
  const evidenceId = recorded.requests[2].original_evidence_id!;
  const originalPath = resolve(root, "originals", evidenceId);
  chmodSync(originalPath, 0o600);
  writeFileSync(originalPath, "deliberate synthetic corruption");
  await dialog
    .getByRole("button", { name: "Export acquisition bundle", exact: true })
    .click();
  await expect(dialog.getByRole("alert")).toBeVisible();
  await expect(
    dialog.getByText("Acquisition bundle saved", { exact: true }),
  ).not.toBeVisible();
  await expect(
    dialog.getByRole("button", {
      name: "Export acquisition bundle",
      exact: true,
    }),
  ).toBeEnabled();
});

test("compact review reflows, stays inside the modal and has no automated accessibility violations", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 900 });
  const { dialog } = await open(page, "/research");
  await dialog
    .getByRole("button", {
      name: "Inspect request 03 https://archive.example/programme",
      exact: true,
    })
    .click();
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    ),
  ).toBe(true);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  const headerFits = await dialog
    .locator(".collection-review-header")
    .evaluate((element) => {
      const header = element.getBoundingClientRect();
      return Array.from(element.children).every(
        (child) => child.getBoundingClientRect().bottom <= header.bottom + 1,
      );
    });
  expect(headerFits).toBe(true);
  const ledger = await dialog
    .locator(".collection-ledger")
    .evaluate((element) => getComputedStyle(element).gridTemplateColumns);
  expect(ledger.split(" ")).toHaveLength(1);
  const accessibility = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  expect(accessibility.violations).toEqual([]);
  mkdirSync(captures, { recursive: true });
  writeFileSync(
    resolve(captures, "accessibility-compact.json"),
    JSON.stringify(
      {
        violations: accessibility.violations,
        passes: accessibility.passes.length,
      },
      null,
      2,
    ),
  );
  await dialog.evaluate((element) => {
    element.scrollTop = 0;
  });
  await dialog.screenshot({
    path: resolve(captures, "collection-review-compact.png"),
  });
});
