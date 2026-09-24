import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace } from "../src/types";
import type { CitationCataloguePage } from "../src/citation-types";
const root = resolve("artifacts/synthetic-ui-workspace"),
  captures = resolve("artifacts/citation-ui");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
function fixture() {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  core({
    action: "import",
    name: "synthetic-citations.txt",
    bytes: [
      ...Buffer.from(
        "Synthetic source 0042\nΟΣ <script>globalThis.citationExecuted=true</script>\n",
      ),
    ],
  });
  let w: Workspace = core({
    action: "import",
    name: "synthetic-citations.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency\n" +
          Array.from(
            { length: 53 },
            (_, i) =>
              `0042,2025-01-${String((i % 28) + 1).padStart(2, "0")},Synthetic payment ${String(i + 1).padStart(3, "0")},-${i + 1}.10,AUD`,
          ).join("\n") +
          "\n",
      ),
    ],
  }).workspace;
  w = core({
    action: "add_entity",
    entity: { name: "Avery Vale", kind: "person", identifiers: [] },
    reason: "Synthetic catalogue fixture",
    expected_revision: w.revision,
  }).workspace;
  for (const value of [
    "ΟΣ <script>globalThis.citationExecuted=true</script>",
    "Second  literal value",
  ]) {
    w = core({
      action: "add_observation",
      observation: {
        entity_id: w.entities[0].id,
        field: "mention",
        value,
        anchor: {
          kind: "text",
          evidence_id: w.evidence[0].id,
          line_start: 1,
          line_end: 2,
        },
      },
      reason: "Synthetic transcription",
      expected_revision: w.revision,
    }).workspace;
    w = core({
      action: "review_observation",
      id: w.observations.at(-1)!.id,
      state: "accepted",
      reason: "Synthetic source review",
      expected_revision: w.revision,
    }).workspace;
  }
  for (const index of [0, 2])
    w = core({
      action: "review_transaction",
      id: w.transactions[index].id,
      state: "accepted",
      reason: "Synthetic source review",
      expected_revision: w.revision,
    }).workspace;
  return w;
}
function addFinding(w: Workspace, title = "Synthetic ordered finding") {
  return core({
    action: "add_finding",
    title,
    assessment: "Synthetic assessment.",
    limitations: "Synthetic fixture only.",
    supporting_ids: [w.transactions[2].id, w.observations[1].id],
    contradicting_ids: [w.observations[0].id, w.transactions[0].id],
    hypothesis_ids: [],
    expected_revision: w.revision,
  }).workspace as Workspace;
}
async function start(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Assessment/ })
    .click();
}
const available = (page: Page) =>
  page.getByRole("region", { name: "Available citations", exact: true });
const selected = (page: Page) =>
  page.getByRole("region", { name: "Selected citations", exact: true });
const editor = (page: Page) =>
  page.getByRole("dialog", { name: /Add finding|Edit finding/ });
async function add(page: Page) {
  await start(page);
  await page.getByRole("button", { name: "Add finding", exact: true }).click();
}
const pageRequest = (w: Workspace, query = "", excluded_ids: string[] = []) =>
  core({
    action: "page_citation_catalogue",
    request: { query, excluded_ids, page_size: 50, cursor: null },
    expected_revision: w.revision,
  }) as CitationCataloguePage;

test("canonical citation pages preserve full counts, selected rows, literal search and keyboard focus", async ({
  page,
}) => {
  const w = fixture(),
    first = pageRequest(w);
  const calls: string[] = [],
    errors: string[] = [],
    external: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (r) => {
    if (!r.url().startsWith("http://127.0.0.1:1420/")) external.push(r.url());
    if (r.url().endsWith("/api/workbench")) calls.push(r.postDataJSON().action);
  });
  await add(page);
  await expect(editor(page)).toContainText(
    "1–50 of 57 matching uncited records",
  );
  expect(
    await available(page)
      .locator("[data-citation-id]")
      .evaluateAll((nodes) =>
        nodes.map((n) => n.getAttribute("data-citation-id")),
      ),
  ).toEqual(first.rows.map((r) => r.id));
  const next = page.getByRole("button", { name: "Next citation page" });
  await next.focus();
  await page.keyboard.press("Enter");
  await expect(editor(page)).toContainText(
    "51–57 of 57 matching uncited records",
  );
  await expect(next).toBeDisabled();
  await expect(
    page.locator(".citation-catalogue > [role=status]"),
  ).toBeFocused();
  const target = w.transactions[52].id;
  await available(page)
    .locator(`[data-citation-id="${target}"]`)
    .getByRole("combobox")
    .selectOption("supporting");
  await expect(
    selected(page).locator(`[data-citation-id="${target}"]`),
  ).toBeVisible();
  await expect(selected(page).getByRole("combobox")).toBeFocused();
  await expect(editor(page)).toContainText(
    "1–50 of 56 matching uncited records",
  );
  await page.getByLabel("Find evidence to cite").fill("Second  literal");
  await expect(editor(page)).toContainText("1–1 of 1 matching uncited records");
  await expect(available(page)).toContainText("Second  literal value");
  await page.getByLabel("Find evidence to cite").fill("ΟΣ");
  await expect(available(page).locator("[data-citation-id]")).toHaveCount(
    pageRequest(w, "ΟΣ", [target]).rows.length,
  );
  await expect(available(page)).toContainText("<script>");
  expect(await page.evaluate(() => "citationExecuted" in globalThis)).toBe(
    false,
  );
  await page.getByLabel("Find evidence to cite").fill("not present anywhere");
  await expect(editor(page)).toContainText("No matching uncited records.");
  await expect(selected(page)).toContainText("Synthetic payment 053");
  await page.getByLabel("Find evidence to cite").fill("🙂".repeat(65));
  await expect(editor(page).getByRole("alert")).toContainText(
    "256 UTF-8 bytes",
  );
  expect(calls).toContain("page_citation_catalogue");
  expect(calls).toContain("read_citation_selections");
  expect(errors).toEqual([]);
  expect(external).toEqual([]);
});

test("saved role order, exact source inspection and immutable report export remain intact", async ({
  page,
}) => {
  let w = addFinding(fixture());
  const finding = w.findings[0];
  const report = core({ action: "save_report" }).workspace.reports[0];
  await start(page);
  await page.locator(`[id="finding-${finding.id}"]`).click();
  const support = page.getByRole("region", {
    name: "Supporting evidence",
    exact: true,
  });
  const contra = page.getByRole("region", {
    name: "Contradictory evidence",
    exact: true,
  });
  await expect(support.locator("[data-citation-id]")).toHaveCount(2);
  expect(
    await support
      .locator("[data-citation-id]")
      .evaluateAll((nodes) =>
        nodes.map((n) => n.getAttribute("data-citation-id")),
      ),
  ).toEqual(finding.supporting_ids);
  expect(
    await contra
      .locator("[data-citation-id]")
      .evaluateAll((nodes) =>
        nodes.map((n) => n.getAttribute("data-citation-id")),
      ),
  ).toEqual(finding.contradicting_ids);
  await expect(
    page.getByRole("dialog", { name: "Finding review" }),
  ).toContainText("2 source-origin groups");
  const opener = support
    .getByRole("button", { name: "Inspect source" })
    .first();
  await opener.click();
  await expect(
    page.getByRole("region", { name: "Source anchor excerpt" }),
  ).toContainText("-3.10");
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  const expected = core({
    action: "read_citation_selections",
    request: { ids: [...finding.supporting_ids, ...finding.contradicting_ids] },
    expected_revision: core({ action: "view" }).workspace.revision,
  });
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(4);
  expect(
    await selected(page)
      .locator("[data-citation-id]")
      .evaluateAll((nodes) =>
        nodes.map((n) => n.getAttribute("data-citation-id")),
      ),
  ).toEqual(expected.rows.map((r: { id: string }) => r.id));
  await page
    .getByLabel("Finding change reason")
    .fill("Preserve explicit role order");
  await page.getByRole("button", { name: "Save finding", exact: true }).click();
  await expect(editor(page)).not.toBeVisible();
  w = core({ action: "view" }).workspace;
  expect(w.findings[0].supporting_ids).toEqual(finding.supporting_ids);
  expect(w.findings[0].contradicting_ids).toEqual(finding.contradicting_ids);
  expect(core({ action: "view" }).workspace.reports[0]).toEqual(report);
  const download = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export self-contained HTML" })
    .click();
  expect(readFileSync((await (await download).path())!, "utf8")).toBe(
    report.html,
  );
});

test("selected and catalogue failures preserve roles and drafts and never claim empty or partial results", async ({
  page,
}) => {
  const w = addFinding(fixture());
  let selectedFail = true,
    catalogueFail = true;
  await page.route("**/api/workbench", async (route) => {
    const action = route.request().postDataJSON().action;
    if (
      (action === "read_citation_selections" && selectedFail) ||
      (action === "page_citation_catalogue" && catalogueFail)
    )
      await route.abort("failed");
    else await route.continue();
  });
  await start(page);
  await page.locator(`[id="finding-${w.findings[0].id}"]`).click();
  const review = page.getByRole("dialog", { name: "Finding review" });
  await expect(review.getByRole("alert")).toContainText(
    "Selected citation details unavailable",
  );
  await expect(review).not.toContainText("source-origin groups among");
  await expect(
    review.getByRole("button", { name: "Mark finding reviewed" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  await page
    .getByLabel("Assessment", { exact: true })
    .fill("Keep unsaved assessment");
  await expect(selected(page)).toContainText(w.findings[0].supporting_ids[0]);
  await expect(editor(page)).toContainText("Citation catalogue unavailable");
  await expect(editor(page)).not.toContainText("No matching uncited records.");
  await expect(
    page.getByRole("button", { name: "Save finding", exact: true }),
  ).toBeDisabled();
  await page.screenshot({
    path: resolve(captures, "citations-unavailable.png"),
  });
  selectedFail = false;
  await page.getByRole("button", { name: "Retry selected citations" }).click();
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(4);
  await expect(selected(page).getByRole("status")).toBeFocused();
  await expect(
    page.getByRole("button", { name: "Save finding", exact: true }),
  ).toBeEnabled();
  catalogueFail = false;
  await page.getByRole("button", { name: "Retry citation search" }).click();
  await expect(editor(page)).toContainText(
    "1–50 of 53 matching uncited records",
  );
  await expect(page.getByLabel("Assessment", { exact: true })).toHaveValue(
    "Keep unsaved assessment",
  );
});

test("concurrent revision and refresh preserve editor fields and roles without rebasing its mutation", async ({
  page,
}) => {
  const w = addFinding(fixture());
  await start(page);
  await page.locator(`[id="finding-${w.findings[0].id}"]`).click();
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  await page
    .getByLabel("Assessment", { exact: true })
    .fill("Draft held at original revision");
  await page.getByLabel("Finding change reason").fill("Retained reason");
  await expect(editor(page)).toContainText(
    "1–50 of 53 matching uncited records",
  );
  core({
    action: "import",
    name: "concurrent-note.txt",
    bytes: [...Buffer.from("Synthetic concurrent evidence")],
  });
  await page.getByRole("button", { name: "Next citation page" }).click();
  await expect(editor(page)).toContainText("Citation revision changed");
  await page
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(editor(page)).toContainText(
    "this draft has not adopted a newer revision",
  );
  await expect(page.getByLabel("Assessment", { exact: true })).toHaveValue(
    "Draft held at original revision",
  );
  await expect(page.getByLabel("Finding change reason")).toHaveValue(
    "Retained reason",
  );
  await expect(selected(page).getByRole("combobox")).toHaveCount(4);
  await expect(
    page.getByRole("button", { name: "Save finding", exact: true }),
  ).toBeDisabled();
  await page.screenshot({ path: resolve(captures, "citations-stale.png") });
  expect(core({ action: "view" }).workspace.findings[0]).toEqual(w.findings[0]);
});

for (const focusCase of ["restore", "moved", "moved_then_blurred"]) {
  test(`delayed real workspace refresh preserves the draft and focus: ${focusCase}`, async ({
    page,
  }) => {
    const w = addFinding(fixture());
    await start(page);
    await page.locator(`[id="finding-${w.findings[0].id}"]`).click();
    await page
      .getByRole("button", { name: "Edit finding", exact: true })
      .click();
    await expect(selected(page).getByRole("combobox")).toHaveCount(4);
    await page
      .getByLabel("Assessment", { exact: true })
      .fill("Unsaved refresh assessment");
    await page
      .getByLabel("Finding change reason")
      .fill("Unsaved refresh reason");
    await selected(page)
      .getByRole("combobox")
      .first()
      .selectOption("contradicting");
    const roles = await selected(page)
      .getByRole("combobox")
      .evaluateAll((nodes) =>
        nodes.map((node) => ({
          label: node.getAttribute("aria-label"),
          role: (node as HTMLSelectElement).value,
        })),
      );
    let release!: () => void, received!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const ready = new Promise<void>((resolve) => {
      received = resolve;
    });
    await page.route("**/api/workbench", async (route) => {
      if (route.request().postDataJSON().action !== "view")
        return route.continue();
      const response = await route.fetch();
      received();
      await gate;
      await route.fulfill({ response });
    });
    const refresh = page.getByRole("button", {
      name: "Refresh workspace",
      exact: true,
    });
    await refresh.focus();
    await page.keyboard.press("Enter");
    await ready;
    await expect(
      page.getByRole("button", { name: "Refreshing workspace…", exact: true }),
    ).toBeDisabled();
    const status = selected(page).getByRole("status");
    if (focusCase !== "restore") {
      await status.focus();
      if (focusCase === "moved_then_blurred")
        await status.evaluate((node) => node.blur());
    }
    // Chromium also drops focus when disabling the button; the native WebKit
    // failure followed the same body/HTML focus path during a real read.
    else
      await expect
        .poll(() =>
          page.evaluate(
            () =>
              document.activeElement === document.body ||
              document.activeElement === document.documentElement,
          ),
        )
        .toBe(true);
    release();
    await expect(refresh).toBeEnabled();
    if (focusCase === "moved_then_blurred") {
      // Let the same completion animation frame run before checking that
      // restoration remembers the earlier deliberate focus movement.
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      await expect(refresh).not.toBeFocused();
    } else await expect(focusCase === "moved" ? status : refresh).toBeFocused();
    await expect(page.getByLabel("Assessment", { exact: true })).toHaveValue(
      "Unsaved refresh assessment",
    );
    await expect(page.getByLabel("Finding change reason")).toHaveValue(
      "Unsaved refresh reason",
    );
    expect(
      await selected(page)
        .getByRole("combobox")
        .evaluateAll((nodes) =>
          nodes.map((node) => ({
            label: node.getAttribute("aria-label"),
            role: (node as HTMLSelectElement).value,
          })),
        ),
    ).toEqual(roles);
    expect(core({ action: "view" }).workspace.findings[0]).toEqual(
      w.findings[0],
    );
  });
}

test("late canonical selection cannot restore an obsolete A to B to A review", async ({
  page,
}) => {
  let w = addFinding(fixture());
  const a = w.findings[0];
  w = core({
    action: "add_finding",
    title: "Synthetic finding B",
    assessment: "Whole source",
    limitations: "Synthetic",
    supporting_ids: [w.evidence[0].id],
    contradicting_ids: [],
    hypothesis_ids: [],
    expected_revision: w.revision,
  }).workspace;
  const b = w.findings[1];
  let release!: () => void, received!: () => void, delivered!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    received = resolve;
  });
  const done = new Promise<void>((resolve) => {
    delivered = resolve;
  });
  let held = false;
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (
      body.action === "read_citation_selections" &&
      body.request.ids.length === 4 &&
      !held
    ) {
      held = true;
      const response = await route.fetch();
      received();
      await gate;
      await route.fulfill({ response });
      delivered();
    } else await route.continue();
  });
  await start(page);
  await page.locator(`[id="finding-${a.id}"]`).click();
  await ready;
  await page.keyboard.press("Escape");
  await page.locator(`[id="finding-${b.id}"]`).click();
  await expect(
    page
      .getByRole("region", { name: "Supporting evidence", exact: true })
      .locator("[data-citation-id]"),
  ).toHaveCount(1);
  core({
    action: "import",
    name: "late-revision.txt",
    bytes: [
      ...Buffer.from("New canonical revision while old response is held"),
    ],
  });
  await page.keyboard.press("Escape");
  await page.locator(`[id="finding-${a.id}"]`).click();
  await expect(
    page
      .getByRole("dialog", { name: "Finding review" })
      .getByText(/Selected citation details unavailable/),
  ).toBeVisible();
  await page
    .getByLabel("Finding review reason")
    .fill("Keep focus and stale reason");
  release();
  await done;
  await expect(page.getByLabel("Finding review reason")).toBeFocused();
  await expect(
    page
      .getByRole("region", { name: "Supporting evidence", exact: true })
      .locator("[data-citation-id]"),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Mark finding reviewed" }),
  ).toBeDisabled();
});

test("catalogue lane coalesces intermediate queries and keeps one active read", async ({
  page,
}) => {
  fixture();
  let release!: () => void, received!: () => void, delivered!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    received = resolve;
  });
  const done = new Promise<void>((resolve) => {
    delivered = resolve;
  });
  let held = false;
  const queries: string[] = [];
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action === "page_citation_catalogue")
      queries.push(body.request.query);
    if (
      body.action === "page_citation_catalogue" &&
      body.request.cursor &&
      !held
    ) {
      held = true;
      const response = await route.fetch();
      received();
      await gate;
      await route.fulfill({ response });
      delivered();
    } else await route.continue();
  });
  await add(page);
  await expect(editor(page)).toContainText(
    "1–50 of 57 matching uncited records",
  );
  await page.getByRole("button", { name: "Next citation page" }).click();
  await ready;
  for (const query of ["intermediate one", "intermediate two", "no match"]) {
    await page.getByLabel("Find evidence to cite").fill(query);
    await expect(
      page.locator(`[data-citation-query="${query}"]`),
    ).toBeVisible();
    expect(queries).toEqual(["", ""]);
  }
  await page
    .getByLabel("Assessment", { exact: true })
    .fill("Do not steal focus");
  release();
  await done;
  await expect(editor(page)).toContainText("No matching uncited records.");
  expect(queries).toEqual(["", "", "no match"]);
  await expect(page.getByLabel("Assessment", { exact: true })).toBeFocused();
  await expect(available(page).locator("[data-citation-id]")).toHaveCount(0);
  await page.getByLabel("Find evidence to cite").fill("");
  await expect(editor(page)).toContainText(
    "1–50 of 57 matching uncited records",
  );
});

test("selected lane coalesces changing ID sets and clearing the last ID retains no old row", async ({
  page,
}) => {
  const w = addFinding(fixture());
  await start(page);
  await page.locator(`[id="finding-${w.findings[0].id}"]`).click();
  await expect(
    page
      .getByRole("region", { name: "Supporting evidence", exact: true })
      .locator("[data-citation-id]"),
  ).toHaveCount(2);
  let release!: () => void, received!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    received = resolve;
  });
  const selections: string[][] = [];
  let held = false;
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action === "read_citation_selections") {
      selections.push(body.request.ids);
      if (!held) {
        held = true;
        const response = await route.fetch();
        received();
        await gate;
        await route.fulfill({ response });
        return;
      }
    }
    await route.continue();
  });
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  await ready;
  for (const id of w.findings[0].supporting_ids) {
    await selected(page)
      .getByLabel(`Citation role for ${id}`, { exact: true })
      .selectOption("none");
    await expect(
      selected(page).getByLabel(`Citation role for ${id}`, { exact: true }),
    ).toHaveCount(0);
    expect(selections).toHaveLength(1);
  }
  release();
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(2);
  expect(selections.map((ids) => ids.length)).toEqual([4, 2]);
  expect(selections[1]).toEqual([...w.findings[0].contradicting_ids].sort());
  await selected(page).getByRole("combobox").first().selectOption("none");
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(1);
  await selected(page).getByRole("combobox").selectOption("none");
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(0);
  await expect(selected(page)).toContainText(
    "Choose at least one supporting or contradictory record.",
  );
});

test("closing the editor clears its pending catalogue request", async ({
  page,
}) => {
  fixture();
  let release!: () => void, received!: () => void, delivered!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const ready = new Promise<void>((resolve) => {
    received = resolve;
  });
  const done = new Promise<void>((resolve) => {
    delivered = resolve;
  });
  const queries: string[] = [];
  let held = false;
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action === "page_citation_catalogue") {
      queries.push(body.request.query);
      if (!held) {
        held = true;
        const response = await route.fetch();
        received();
        await gate;
        await route.fulfill({ response });
        delivered();
        return;
      }
    }
    await route.continue();
  });
  await add(page);
  await ready;
  await page.getByLabel("Find evidence to cite").fill("pending query");
  await expect(
    page.locator('[data-citation-query="pending query"]'),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close finding editor" }).click();
  release();
  await done;
  // A round-trip to the same real core gives the settled client a completion boundary.
  await page.getByRole("button", { name: "Add finding", exact: true }).click();
  await expect(editor(page)).toContainText(
    "1–50 of 57 matching uncited records",
  );
  expect(queries).toEqual(["", ""]);
});

test("industrial citation controls retain compact modal layout and accessible states", async ({
  page,
}) => {
  const w = addFinding(fixture());
  await start(page);
  await page.locator(`[id="finding-${w.findings[0].id}"]`).click();
  await expect(
    page
      .getByRole("region", { name: "Supporting evidence", exact: true })
      .locator("[data-citation-id]"),
  ).toHaveCount(2);
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  await expect(selected(page).locator("[data-citation-id]")).toHaveCount(4);
  const audits = [];
  for (const width of [1440, 720]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.getByLabel("Find evidence to cite").fill("no match");
    await expect(editor(page)).toContainText("No matching uncited records.");
    await page
      .getByRole("button", { name: "First citation page" })
      .scrollIntoViewIfNeeded();
    const audit = await new AxeBuilder({ page })
      .include("dialog[open]")
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    audits.push({
      width,
      violations: audit.violations,
      incomplete: audit.incomplete,
    });
    expect(
      await editor(page).evaluate(
        (node) => node.scrollWidth <= node.clientWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: resolve(captures, `citations-${width}.png`),
    });
  }
  for (const width of [1440, 720]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.getByLabel("Find evidence to cite").fill("");
    await expect(editor(page)).toContainText(
      "1–50 of 53 matching uncited records",
    );
    await page.getByRole("button", { name: "Next citation page" }).click();
    await expect(editor(page)).toContainText(
      "51–53 of 53 matching uncited records",
    );
    await page
      .getByRole("button", { name: "Previous citation page" })
      .scrollIntoViewIfNeeded();
    expect(
      await editor(page).evaluate(
        (node) => node.scrollWidth <= node.clientWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: resolve(captures, `citations-page-${width}.png`),
    });
    await page.getByLabel("Find evidence to cite").fill("no match");
    await expect(editor(page)).toContainText("No matching uncited records.");
  }
  writeFileSync(
    resolve(captures, "accessibility.json"),
    JSON.stringify(audits, null, 2),
  );
  expect(audits.flatMap((value) => value.violations)).toEqual([]);
  // Chromium's search field consumes its first Escape to clear a nonempty query.
  await page.keyboard.press("Escape");
  await expect(page.getByLabel("Find evidence to cite")).toHaveValue("");
  await expect(editor(page)).toBeVisible();
  await page.getByRole("button", { name: "Close finding editor" }).focus();
  await page.keyboard.press("Escape");
  await expect(editor(page)).not.toBeVisible();
  await expect(
    page.locator(`[id="finding-${w.findings[0].id}"]`),
  ).toBeFocused();
});
