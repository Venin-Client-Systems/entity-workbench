import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace, Finding } from "../src/types";
import type { ReviewDecisionPage } from "../src/review-history-types";

const workspaceRoot = resolve("artifacts/synthetic-ui-workspace");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [workspaceRoot], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
const inputFor = (finding: Finding) => ({
  title: finding.title,
  assessment: finding.assessment,
  supporting_ids: finding.supporting_ids,
  contradicting_ids: finding.contradicting_ids,
  limitations: finding.limitations,
  hypothesis_ids: finding.hypothesis_ids,
});
function edit(w: Workspace, finding: Finding, reason: string): Workspace {
  return core({
    action: "update_finding",
    id: finding.id,
    finding: inputFor(finding),
    reason,
    expected_revision: w.revision,
  }).workspace;
}
function fixture(count = 53) {
  rmSync(workspaceRoot, { recursive: true, force: true });
  let w: Workspace = core({
    action: "import",
    name: "synthetic-history-source.txt",
    bytes: [...Buffer.from("SYNTHETIC review history source 0042.\n")],
  }).workspace;
  for (const title of ["Synthetic finding A", "Synthetic finding B"]) {
    w = core({
      action: "add_finding",
      title,
      assessment: "History navigation specimen.",
      limitations: "Synthetic UI fixture; no investigative conclusion.",
      supporting_ids: [w.evidence[0].id],
      contradicting_ids: [],
      hypothesis_ids: [],
      expected_revision: w.revision,
    }).workspace;
  }
  const [a, b] = w.findings;
  for (let i = 0; i < count; i++) {
    w =
      i === 1
        ? core({
            action: "review_finding",
            id: a.id,
            reason: "Decision 0002 — inspected synthetic whole source",
            expected_revision: w.revision,
          }).workspace
        : edit(
            w,
            a,
            `Decision ${String(i + 1).padStart(4, "0")} — synthetic edit\n<script>globalThis.historyExecuted = true</script>`,
          );
  }
  return { w, a, b };
}
const history = (page: Page) =>
  page.getByRole("region", { name: "Finding review history" });
const open = async (page: Page, id: string) =>
  page.locator(`[id="finding-${id}"]`).click();
async function start(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Assessment/ })
    .click();
}
function canonicalPage(
  w: Workspace,
  id: string,
  cursor: string | null = null,
): ReviewDecisionPage {
  return core({
    action: "page_review_decisions",
    request: { target_id: id, page_size: 50, cursor },
    expected_revision: w.revision,
  });
}

test("bounded finding history preserves canonical order, exact records, count and keyboard paging", async ({
  page,
}) => {
  const { w, a } = fixture();
  const expected = canonicalPage(w, a.id),
    tail = canonicalPage(w, a.id, expected.next_cursor);
  const calls: string[] = [],
    external: string[] = [],
    errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (!request.url().startsWith("http://127.0.0.1:1420/"))
      external.push(request.url());
    if (request.url().endsWith("/api/workbench")) {
      const body = request.postDataJSON();
      if (body.action === "page_review_decisions")
        calls.push(body.request.target_id);
    }
  });
  await start(page);
  await open(page, a.id);
  const region = history(page);
  await expect(region).toContainText("1–50 of 53 recorded decisions");
  await expect(region.locator("li code")).toHaveText(
    expected.rows.map((row) => row.id),
  );
  await expect(region.locator("li time")).toHaveText(
    expected.rows.map((row) => row.at),
  );
  await expect(region.locator("li .preserve-lines")).toHaveText(
    expected.rows.map((row) => row.reason),
  );
  await expect(region.getByText("Reviewed", { exact: true })).toHaveCount(1);
  expect(await page.evaluate(() => "historyExecuted" in globalThis)).toBe(
    false,
  );
  const next = region.getByRole("button", { name: "Next history page" });
  await next.focus();
  await page.keyboard.press("Enter");
  await expect(region).toContainText("51–53 of 53 recorded decisions");
  await expect(region.locator("li code")).toHaveText(
    tail.rows.map((row) => row.id),
  );
  await expect(next).toBeDisabled();
  await expect(region.getByRole("status")).toBeFocused();
  await region.getByRole("button", { name: "Previous history page" }).click();
  await expect(region).toContainText("1–50 of 53 recorded decisions");
  await next.click();
  await expect(region).toContainText("51–53 of 53 recorded decisions");
  await region.getByRole("button", { name: "First history page" }).click();
  await expect(region).toContainText("1–50 of 53 recorded decisions");
  // Whole-evidence citations keep their source inspector; they are not history targets.
  const callsBeforeSource = calls.length;
  await page
    .getByRole("region", { name: "Supporting evidence" })
    .getByRole("button", { name: "Inspect source" })
    .click();
  await expect(page.locator(".source-lines")).toContainText("0042");
  await page.keyboard.press("Escape");
  expect(calls).toHaveLength(callsBeforeSource);
  expect(new Set(calls)).toEqual(new Set([a.id]));
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("actual empty finding differs from failed read and retry restores result focus", async ({
  page,
}) => {
  const { b } = fixture(0);
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON().action === "page_review_decisions" &&
      fail
    ) {
      fail = false;
      await route.abort("failed");
    } else await route.continue();
  });
  await start(page);
  await open(page, b.id);
  const region = history(page);
  await expect(region.getByRole("alert")).toContainText("History unavailable");
  await expect(region).not.toContainText("No decision recorded.");
  await expect(
    region.getByRole("button", { name: "Next history page" }),
  ).toBeDisabled();
  mkdirSync("artifacts/review-history", { recursive: true });
  await region.scrollIntoViewIfNeeded();
  await page.screenshot({ path: "artifacts/review-history/history-error.png" });
  await region.getByRole("button", { name: "Retry history read" }).click();
  await expect(region).toContainText("No decision recorded.");
  await expect(region.getByRole("status")).toBeFocused();
  await expect(region.getByRole("alert")).toHaveCount(0);
  await page.screenshot({ path: "artifacts/review-history/history-empty.png" });
});

test("stale continuation clears rows and refresh invalidates cursors without losing a review draft", async ({
  page,
}) => {
  let { w, a } = fixture();
  await start(page);
  await open(page, a.id);
  const region = history(page),
    draft = page.getByLabel("Finding review reason");
  await expect(region).toContainText("1–50 of 53 recorded decisions");
  await draft.fill("Keep this draft while checking newer history.");
  w = edit(w, a, "Decision 0054 — concurrent canonical writer");
  await region.getByRole("button", { name: "Next history page" }).click();
  await expect(region.getByRole("alert")).toContainText("History unavailable");
  await expect(region.locator("li")).toHaveCount(0);
  await expect(region).not.toContainText("No decision recorded.");
  await region
    .getByRole("button", { name: "Refresh workspace and history" })
    .click();
  await expect(region).toContainText("1–50 of 54 recorded decisions");
  await expect(region).toContainText(`revision ${w.revision}`);
  await expect(draft).toHaveValue(
    "Keep this draft while checking newer history.",
  );
  await expect(draft).toBeDisabled();
  await expect(
    page.getByRole("dialog", { name: "Finding review" }),
  ).toContainText("Close and reopen the finding");
  await region.getByRole("button", { name: "Next history page" }).click();
  await expect(region).toContainText("51–54 of 54 recorded decisions");
  await expect(region).toContainText("concurrent canonical writer");
  await page.keyboard.press("Escape");
  await expect(page.locator(`[id="finding-${a.id}"]`)).toBeFocused();
  await open(page, a.id);
  await expect(page.getByLabel("Finding review reason")).toBeEnabled();
});

test("late real continuation cannot overwrite a closed and reopened A to B to A review", async ({
  page,
}) => {
  const { a, b } = fixture();
  let release!: () => void, fetched!: () => void, fulfilled!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    fetched = resolve;
  });
  const done = new Promise<void>((resolve) => {
    fulfilled = resolve;
  });
  let delayed = false;
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (
      body.action === "page_review_decisions" &&
      body.request.cursor &&
      !delayed
    ) {
      delayed = true;
      const response = await route.fetch();
      fetched();
      await gate;
      await route.fulfill({ response });
      fulfilled();
    } else await route.continue();
  });
  await start(page);
  await open(page, a.id);
  await expect(history(page)).toContainText("1–50 of 53 recorded decisions");
  await history(page)
    .getByRole("button", { name: "Next history page" })
    .click();
  await ready;
  await page.keyboard.press("Escape");
  await open(page, b.id);
  await expect(history(page)).toContainText("No decision recorded.");
  await page.keyboard.press("Escape");
  await open(page, a.id);
  await expect(history(page)).toContainText("1–50 of 53 recorded decisions");
  release();
  await done;
  await expect(history(page)).toContainText("1–50 of 53 recorded decisions");
  await expect(
    history(page).getByRole("button", { name: "Previous history page" }),
  ).toBeDisabled();
});

test("industrial history controls fit compact review and retain accessible modal navigation", async ({
  page,
}) => {
  const { a } = fixture();
  await start(page);
  await open(page, a.id);
  const region = history(page),
    dialog = page.getByRole("dialog", { name: "Finding review" });
  await expect(region).toContainText("1–50 of 53 recorded decisions");
  await region.getByRole("button", { name: "Next history page" }).click();
  await expect(region).toContainText("51–53 of 53 recorded decisions");
  mkdirSync("artifacts/review-history", { recursive: true });
  const audits = [];
  for (const width of [1440, 720]) {
    await page.setViewportSize({ width, height: 1000 });
    await dialog.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    expect(
      await dialog.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);
    const audit = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    audits.push({
      width,
      violations: audit.violations,
      incomplete: audit.incomplete,
    });
    await page.screenshot({
      path: `artifacts/review-history/history-${width}.png`,
    });
    expect(audit.violations).toEqual([]);
  }
  writeFileSync(
    "artifacts/review-history/accessibility.json",
    JSON.stringify(audits, null, 2),
  );
  const close = page.getByRole("button", { name: "Close finding review" });
  await close.focus();
  await page.keyboard.press("Shift+Tab");
  await expect(
    region.getByRole("button", { name: "Previous history page" }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator(`[id="finding-${a.id}"]`)).toBeFocused();
});

test("a completed history read does not steal focus from an analyst's review draft", async ({
  page,
}) => {
  const { a } = fixture();
  let release!: () => void, fetched!: () => void, fulfilled!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    fetched = resolve;
  });
  const done = new Promise<void>((resolve) => {
    fulfilled = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action === "page_review_decisions" && body.request.cursor) {
      const response = await route.fetch();
      fetched();
      await gate;
      await route.fulfill({ response });
      fulfilled();
    } else await route.continue();
  });
  await start(page);
  await open(page, a.id);
  await expect(history(page)).toContainText("1–50 of 53 recorded decisions");
  await history(page)
    .getByRole("button", { name: "Next history page" })
    .click();
  await ready;
  const draft = page.getByLabel("Finding review reason");
  await draft.fill("Continue drafting while canonical history is read.");
  release();
  await done;
  await expect(history(page)).toContainText("51–53 of 53 recorded decisions");
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(draft).toBeFocused();
  await expect(draft).toHaveValue(
    "Continue drafting while canonical history is read.",
  );
});
