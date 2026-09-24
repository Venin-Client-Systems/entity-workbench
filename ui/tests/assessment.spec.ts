import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
const root = resolve("artifacts/synthetic-ui-workspace");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
function fixture() {
  rmSync(root, { recursive: true, force: true });
  const source = (name: string, text: string) =>
    core({ action: "import", name, bytes: Array.from(Buffer.from(text)) })
      .workspace;
  source("record-a.txt", "Avery Vale born 1982\n");
  let w = source("record-b.txt", "Avery Vale born 1990\n");
  for (const [index, year] of ["1982", "1990"].entries()) {
    w = core({
      action: "add_entity",
      entity: {
        name: "Avery Vale",
        kind: "person",
        identifiers: [{ namespace: "CASE", value: `00002${index + 1}` }],
      },
      reason: "Distinct synthetic mention",
      expected_revision: w.revision,
    }).workspace;
    w = core({
      action: "add_observation",
      observation: {
        entity_id: w.entities[index].id,
        field: "birth_year",
        value: year,
        anchor: {
          kind: "text",
          evidence_id: w.evidence[index].id,
          line_start: 1,
          line_end: 1,
        },
      },
      reason: "Transcribed source line",
      expected_revision: w.revision,
    }).workspace;
    w = core({
      action: "review_observation",
      id: w.observations[index].id,
      state: "accepted",
      reason: "Inspected source line",
      expected_revision: w.revision,
    }).workspace;
  }
  return w;
}
test("assessment authoring, source inspection, review, correction and immutable exports", async ({
  page,
}) => {
  const initial = fixture();
  const errors: string[] = [],
    external: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => {
    if (!/^(http:\/\/127\.0\.0\.1:1420|blob:|data:)/.test(r.url()))
      external.push(r.url());
  });
  await page.goto("/");
  const navigate = () =>
    page
      .getByRole("navigation")
      .getByRole("button", { name: /Assessment/ })
      .click();
  await navigate();
  await page.getByRole("button", { name: "Add question", exact: true }).click();
  await page
    .getByLabel("Investigation question", { exact: true })
    .fill("Do the records describe the same person?");
  await page
    .getByLabel("Working hypothesis")
    .fill("Identity remains unresolved.");
  await page
    .getByLabel(/Alternative explanations/)
    .fill("Namesakes\nSource transcription error");
  await page
    .getByLabel(/Collection gaps/)
    .fill("Independent confirming record");
  await page
    .getByLabel("Question change reason")
    .fill("Define scope from conflicting records");
  await page
    .getByRole("button", { name: "Save question", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Add question" }),
  ).not.toBeVisible();
  await page.getByRole("button", { name: "Add finding", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "Add finding" });
  await page.getByLabel("Finding title").fill("Conflicting birth years");
  await page
    .getByLabel("Assessment", { exact: true })
    .fill(
      "The reviewed records contain conflicting birth years. Identity remains unresolved.",
    );
  await page
    .getByLabel("Limitations and outstanding enquiries")
    .fill(
      "Two records alone cannot establish identity or source independence.",
    );
  await page.getByLabel("Do the records describe the same person?").check();
  await page
    .getByLabel("Citation role for Avery Vale · birth_year: 1982", {
      exact: true,
    })
    .selectOption("supporting");
  await page
    .getByLabel("Citation role for Avery Vale · birth_year: 1990", {
      exact: true,
    })
    .selectOption("contradicting");
  await page.getByLabel("Find evidence to cite").fill("no matches");
  await expect(
    page.getByRole("region", { name: "Selected citations" }),
  ).toContainText("1982");
  const audits = [];
  for (const width of [1440, 960]) {
    await page.setViewportSize({ width, height: 1000 });
    await editor.evaluate((e) => {
      e.scrollTop = 0;
    });
    const a = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    audits.push({
      screen: `Finding editor ${width}`,
      violations: a.violations,
      incomplete: a.incomplete,
    });
    await page.screenshot({ path: `artifacts/ui-finding-editor-${width}.png` });
  }
  await page.getByRole("button", { name: "Save finding", exact: true }).click();
  await expect(editor).not.toBeVisible();
  await page
    .getByRole("button", { name: "Review finding", exact: true })
    .click();
  const review = page.getByRole("dialog", { name: "Finding review" });
  await expect(review).toContainText("2 source-origin groups");
  for (const [region, year] of [
    ["Supporting evidence", "1982"],
    ["Contradictory evidence", "1990"],
  ]) {
    const opener = page
      .getByRole("region", { name: region })
      .getByRole("button", { name: "Inspect source" });
    await opener.click();
    await expect(
      page.getByRole("region", { name: "Source anchor excerpt" }),
    ).toContainText(`Avery Vale born ${year}`);
    await page.keyboard.press("Escape");
    await expect(opener).toBeFocused();
  }
  await page
    .getByLabel("Finding review reason")
    .fill(
      "Inspected both source records; the conflict is explicitly retained.",
    );
  for (const width of [1440, 960]) {
    await page.setViewportSize({ width, height: 1000 });
    await review.evaluate((e) => {
      e.scrollTop = 0;
    });
    const a = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    audits.push({
      screen: `Finding review ${width}`,
      violations: a.violations,
      incomplete: a.incomplete,
    });
    await page.screenshot({ path: `artifacts/ui-finding-review-${width}.png` });
    expect(await review.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(
      true,
    );
  }
  await page.getByRole("button", { name: "Mark finding reviewed" }).click();
  await expect(review).not.toBeVisible();
  await expect(page.getByText("Reviewed", { exact: true })).toBeVisible();
  const savedResponse = page.waitForResponse((r) =>
    r.url().endsWith("/api/workbench") &&
    r.request().postDataJSON()?.action === "save_report",
  );
  await page.getByRole("button", { name: "Save report snapshot" }).click();
  const catalogue = (await (await savedResponse).json()).workspace.reports[0];
  expect(catalogue).not.toHaveProperty("html");
  await expect(
    page.getByRole("button", { name: "Export self-contained HTML" }),
  ).toBeVisible();
  const snapshot = core({ action: "view" }).workspace.reports[0];
  expect(snapshot.html).toContain("Contradicting:");
  expect(snapshot.html).toContain("birth_year: 1990");
  expect(snapshot.html).toContain("Source anchor:");
  const download = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Export self-contained HTML" })
    .click();
  const downloaded = await download;
  expect(downloaded.suggestedFilename()).toBe(`assessment-${snapshot.id}.html`);
  expect(readFileSync((await downloaded.path())!, "utf8")).toBe(snapshot.html);
  expect(catalogue.html_bytes).toBe(Buffer.byteLength(snapshot.html));
  const state = core({ action: "view" }).workspace;
  core({
    action: "correct_observation",
    id: initial.observations[1].id,
    value: "1982",
    anchor: initial.observations[1].anchor,
    reason: "Synthetic source interpretation correction",
    expected_revision: state.revision,
  });
  await page.reload();
  await navigate();
  await page
    .getByRole("button", { name: "Review finding", exact: true })
    .click();
  await expect(review).toContainText("Review required");
  await page
    .getByLabel("Finding review reason")
    .fill("Attempt while cited correction is pending");
  await page.getByRole("button", { name: "Mark finding reviewed" }).click();
  await expect(review.getByRole("alert")).toContainText(
    "Accept all cited observations",
  );
  await expect(page.getByLabel("Finding review reason")).toHaveValue(
    "Attempt while cited correction is pending",
  );
  await page.getByRole("button", { name: "Edit finding", exact: true }).click();
  await page
    .getByLabel("Assessment", { exact: true })
    .fill("A correction remains pending review.");
  await page
    .getByLabel("Finding change reason")
    .fill("Reopen assessment after correction");
  await page.getByRole("button", { name: "Save finding", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Edit finding" }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Review finding", exact: true }),
  ).toBeFocused();
  await page
    .getByRole("button", { name: "Review finding", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Finding review history" }),
  ).toContainText("Reopen assessment after correction");
  await page.keyboard.press("Escape");
  await page
    .getByRole("button", { name: "Edit question", exact: true })
    .click();
  await page
    .getByLabel(/Collection gaps/)
    .fill("Review corrected source interpretation");
  await page
    .getByLabel("Question change reason")
    .fill("Refine outstanding enquiry");
  await page
    .getByRole("button", { name: "Save question", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Edit question" }),
  ).not.toBeVisible();
  expect(core({ action: "view" }).workspace.reports[0]).toEqual(snapshot);
  writeFileSync(
    "artifacts/assessment-accessibility.json",
    JSON.stringify(audits, null, 2),
  );
  expect(audits.flatMap((a) => a.violations)).toEqual([]);
  expect(errors).toEqual([]);
  expect(external).toEqual([]);
});
test("stale assessment edits preserve the draft and cannot overwrite concurrent work", async ({
  page,
}) => {
  fixture();
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Assessment/ })
    .click();
  await page.getByRole("button", { name: "Add question", exact: true }).click();
  await page
    .getByLabel("Investigation question", { exact: true })
    .fill("Draft question");
  await page.getByLabel("Working hypothesis").fill("Draft hypothesis");
  await page.getByLabel("Question change reason").fill("Draft reason");
  core({
    action: "import",
    name: "concurrent.txt",
    bytes: Array.from(Buffer.from("Concurrent synthetic note")),
  });
  await page
    .getByRole("button", { name: "Save question", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Add question" });
  await expect(dialog.getByRole("alert")).toContainText(
    /revision|changed|Stale/i,
  );
  await expect(
    page.getByLabel("Investigation question", { exact: true }),
  ).toHaveValue("Draft question");
  expect(core({ action: "view" }).workspace.hypotheses).toHaveLength(0);
});


test("selected report export shows integrity failure and permits a verified retry", async ({ page }) => {
  fixture();
  const snapshot = core({ action: "save_report" }).workspace.reports[0];
  await page.goto("/");
  await page.getByRole("navigation").getByRole("button", { name: /Assessment/ }).click();
  const button = page.getByRole("button", { name: "Export self-contained HTML" });
  await expect(button).toBeVisible();
  const replaceHtml = (html: string) => execFileSync("python3", ["-c",
    "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute(\"UPDATE records SET body=json_set(body,'$.html',?) WHERE kind='report' AND id=?\",(sys.argv[2],sys.argv[3])); c.commit()",
    resolve(root, "workspace.db"), html, snapshot.id,
  ]);
  // Alter retained bytes after the catalogue loaded. The core must reject this
  // selection; the browser must expose the error without downloading a file.
  replaceHtml("Synthetic corrupted report");
  let downloads = 0;
  page.on("download", () => downloads++);
  await button.click();
  await expect(page.getByRole("alert")).toContainText("integrity check");
  await expect(button).toBeEnabled();
  expect(downloads).toBe(0);
  replaceHtml(snapshot.html);
  const downloaded = page.waitForEvent("download");
  await button.click();
  expect(readFileSync((await (await downloaded).path())!, "utf8")).toBe(snapshot.html);
  await expect(page.getByRole("alert")).not.toBeVisible();
});
