import { test, expect, type Page, type Locator } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { ProcessingJob, ProcessingJobPage } from "../src/processing-types";
import type { PdfExtraction } from "../src/pdf-processing-types";
import type { Workspace } from "../src/types";

const root = resolve("artifacts/synthetic-ui-workspace");
const executable = resolve("target/debug/ew-dev");
const captures = resolve("artifacts/pdf-jobs");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let jobs: ProcessingJob[];
let workspace: Workspace;
const extraction = (id: string): PdfExtraction =>
  core({ action: "inspect_pdf_extraction", extraction_id: id });
const fixture = (name: string) =>
  jobs.find(
    (job) =>
      job.input.evidence_id ===
      workspace.evidence.find((item) => item.name === `pdf-review-${name}.pdf`)!
        .id,
  )!;
const jobDialog = (page: Page) =>
  page.getByRole("dialog", { name: "Document job", exact: true });
const resultDialog = (page: Page) =>
  page.getByRole("dialog", { name: "PDF page OCR review", exact: true });
const fact = (dialog: Locator, name: string) =>
  dialog
    .locator("dt")
    .filter({ hasText: new RegExp(`^${name}$`) })
    .locator("+ dd");
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  execFileSync(executable, ["seed-pdf-processing-review", root]);
  jobs = (core({ action: "list_processing_jobs" }) as ProcessingJobPage).jobs;
  workspace = core({ action: "view" }).workspace;
  mkdirSync(captures, { recursive: true });
});
async function list(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  const region = page.getByRole("region", {
    name: "Document jobs",
    exact: true,
  });
  await expect(
    region.getByText("11 shown · 11 loaded / 11 total"),
  ).toBeVisible();
  return region;
}
async function openJob(page: Page, name: string) {
  const region = await list(page);
  const job = fixture(name);
  await region.locator(`#document-job-${job.id}`).click();
  await expect(
    jobDialog(page).getByText("Loading document job…"),
  ).not.toBeVisible();
  return jobDialog(page);
}
async function openResult(page: Page, name: string, resultIndex = 0) {
  const job = await openJob(page, name);
  const opener = job
    .getByRole("button", { name: /Inspect extraction/ })
    .nth(resultIndex);
  await opener.click();
  const dialog = resultDialog(page);
  await expect(
    dialog.getByText("Loading immutable PDF extraction…"),
  ).not.toBeVisible();
  return { dialog, opener, job };
}
async function axe(page: Page, scope: string, name: string) {
  const result = await new AxeBuilder({ page })
    .include(scope)
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `accessibility-${name}.json`),
    JSON.stringify(
      {
        scope,
        tags: ["wcag2a", "wcag2aa", "wcag21aa"],
        violations: result.violations,
      },
      null,
      2,
    ) + "\n",
  );
  expect(result.violations).toEqual([]);
}

test("PDF page review preserves three identities, exact geometry and inert unreviewed text", async ({
  page,
  context,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("request", (request) => {
    if (
      !request.url().startsWith("http://127.0.0.1:1420/") &&
      !/^(blob:|data:)/.test(request.url())
    )
      external.push(request.url());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  const record = extraction(fixture("recognized").result_ids[0]);
  const { dialog, opener } = await openResult(page, "recognized");
  await expect(
    dialog.getByText("Text recognized · unreviewed", { exact: true }),
  ).toBeVisible();
  for (const [label, value] of [
    ["Original SHA-256", record.input.sha256],
    ["Raster SHA-256", record.result.render.raster!.sha256],
    ["Result SHA-256", record.result_sha256],
  ])
    await expect(fact(dialog, label)).toHaveText(value);
  await expect(fact(dialog, "Selected page")).toHaveText("2 / 2 pages");
  await expect(fact(dialog, "Render resolution")).toHaveText("144 DPI");
  await expect(fact(dialog, "Effective CropBox")).toHaveText(
    "[0, 0, 600, 115] points",
  );
  await expect(fact(dialog, "Rotation")).toHaveText("0°");
  await expect(dialog.getByLabel("PDF to raster affine")).toHaveText(
    `[${record.result.render.geometry!.pdf_to_raster.join(", ")}]`,
  );
  const text = dialog.getByRole("textbox", { name: "Unreviewed OCR text" });
  await expect(text).toHaveValue(record.result.recognition!.text);
  await expect(text).toHaveAttribute("readonly", "");
  expect(record.result.recognition!.text).toContain("<script>");
  await expect(
    dialog.locator("script,img,svg,iframe,object,embed"),
  ).toHaveCount(0);
  await expect(
    dialog.getByText(/validated raster was discarded/),
  ).toBeVisible();
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await dialog.getByRole("button", { name: "Copy OCR text" }).click();
  await expect(dialog.getByRole("status")).toHaveText(
    "Unreviewed OCR text copied to the clipboard.",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    record.result.recognition!.text,
  );
  await axe(page, 'dialog[aria-label="PDF page OCR review"]', "desktop");
  await dialog.screenshot({
    path: resolve(captures, "recognized-desktop.png"),
  });
  await page.setViewportSize({ width: 1440, height: 1800 });
  await dialog.screenshot({ path: resolve(captures, "recognized-full.png") });
  await dialog
    .getByText("Full PDF / OCR provenance and limitations", { exact: true })
    .click();
  await expect(fact(dialog, "Renderer")).toHaveText(
    `${record.result.render.renderer} / Java ${record.result.render.java_runtime}`,
  );
  await expect(fact(dialog, "OCR engine")).toHaveText(
    record.result.recognition!.engine,
  );
  await expect(fact(dialog, "OCR raster SHA-256")).toHaveText(
    record.result.render.raster!.sha256,
  );
  await expect(fact(dialog, "English model SHA-256")).toHaveText(
    record.result.recognition!.model_sha256,
  );
  await page.setViewportSize({ width: 1440, height: 2800 });
  await dialog.screenshot({
    path: resolve(captures, "recognized-provenance.png"),
  });
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  expect(core({ action: "view" }).workspace.observations).toEqual([]);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("empty, encrypted, unsupported, failed and quota results remain distinct and cannot copy text", async ({
  page,
}) => {
  for (const [name, title] of [
    ["empty", "No text recognized · unreviewed"],
    ["encrypted", "Encrypted PDF · OCR did not run"],
    ["unsupported", "PDF input unsupported · OCR did not run"],
    ["failed", "PDF rendering failed · OCR did not run"],
    ["quota", "PDF limit exhausted · OCR did not run"],
  ]) {
    const record = extraction(fixture(name).result_ids[0]);
    const { dialog } = await openResult(page, name);
    await expect(dialog.getByText(title, { exact: true })).toBeVisible();
    await expect(
      dialog.getByRole("button", { name: "Copy OCR text" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("textbox", { name: "Unreviewed OCR text" }),
    ).toHaveCount(0);
    await expect(
      dialog.getByText(/not evidence that the page or other parts/),
    ).toBeVisible();
    if (name === "empty") {
      expect(record.result.recognition!.text.trim()).toBe("");
      expect(record.result.recognition!.text.length).toBeGreaterThan(0);
      await expect(
        dialog.getByRole("region", { name: "Page geometry" }),
      ).toBeVisible();
    } else {
      expect(record.result.recognition).toBeNull();
      await expect(fact(dialog, "Raster SHA-256")).toHaveText("Not produced");
      await expect(
        dialog.getByRole("region", { name: "Page geometry" }),
      ).toHaveCount(0);
      await expect(
        dialog.getByText(/validated raster was discarded/),
      ).toHaveCount(0);
    }
    await dialog.screenshot({ path: resolve(captures, `${name}.png`) });
  }
});

test("page and DPI controls require bounded integer settings and reserve exactly the selected operation", async ({
  page,
}) => {
  const region = await list(page);
  const evidenceId = fixture("recognized").input.evidence_id;
  const requests: Record<string, unknown>[] = [];
  page.on("request", (request) => {
    if (
      request.url().endsWith("/api/workbench") &&
      request.postDataJSON()?.action === "queue_pdf_page_ocr"
    )
      requests.push(request.postDataJSON());
  });
  await region.getByLabel("Processing method").selectOption("pdf_page_ocr");
  await region.getByLabel("Original to process").selectOption(evidenceId);
  const queue = region.getByRole("button", {
    name: "Queue PDF page OCR",
    exact: true,
  });
  const number = region.getByLabel("Page number (1-based)"),
    dpi = region.getByLabel("Resolution (DPI)");
  await expect(queue).toBeDisabled();
  await expect(dpi).toHaveValue("144");
  for (const value of ["0", "-1", "1.5", "1001"]) {
    await number.fill(value);
    await expect(queue).toBeDisabled();
  }
  await number.fill("1");
  for (const value of ["", "71", "144.5", "301"]) {
    await dpi.fill(value);
    await expect(queue).toBeDisabled();
  }
  expect(requests).toEqual([]);
  await dpi.fill("72");
  await axe(page, 'section[aria-label="Document jobs"]', "queue-desktop");
  await region.screenshot({ path: resolve(captures, "queue-desktop.png") });
  await queue.click();
  await expect(
    jobDialog(page).getByText("PDF page OCR · page 1 · 72 DPI · English", {
      exact: true,
    }),
  ).toBeVisible();
  const queued = core({ action: "list_processing_jobs" }).jobs.find(
    (job: ProcessingJob) => job.request_key === requests[0].request_key,
  );
  expect(queued.input).toMatchObject({
    operation: "pdf_page_ocr",
    evidence_id: evidenceId,
    page_number: 1,
    dpi: 72,
  });
  await jobDialog(page)
    .getByRole("button", { name: "Cancel attempt 1" })
    .click();
  await expect(jobDialog(page).locator(".processing-banner strong")).toHaveText(
    "Cancelled",
  );
  await jobDialog(page).getByRole("button", { name: "Review retry" }).click();
  const retry = page.getByRole("dialog", {
    name: "Retry document job",
    exact: true,
  });
  await retry
    .getByRole("textbox", { name: "Reason for retry" })
    .fill("Synthetic analyst retries the selected page.");
  await retry.getByRole("button", { name: "Reserve retry attempt" }).click();
  await expect(retry).toHaveCount(0);
  await expect(jobDialog(page).locator(".processing-banner strong")).toHaveText(
    "Queued",
  );
  await expect(fact(jobDialog(page), "Reserved attempt")).toHaveText("2 / 3");
  await page.keyboard.press("Escape");
  await expect(queue).toBeDisabled();
  await number.fill("1000");
  await dpi.fill("300");
  await expect(queue).toBeEnabled();
  await queue.click();
  await expect(
    jobDialog(page).getByText("PDF page OCR · page 1000 · 300 DPI · English", {
      exact: true,
    }),
  ).toBeVisible();
  expect(requests).toHaveLength(2);
  expect(requests[1].request_key).not.toBe(requests[0].request_key);
  expect(core({ action: "list_processing_jobs" }).total).toBe(13);
});

test("lost acknowledgement identity includes method, original, page and DPI across section remount", async ({
  page,
}) => {
  const region = await list(page),
    id = fixture("recognized").input.evidence_id;
  await region.getByLabel("Original to process").selectOption(id);
  await region.getByLabel("Processing method").selectOption("pdf_page_ocr");
  await region.getByLabel("Page number (1-based)").fill("3");
  let lost = false;
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON()?.action === "queue_pdf_page_ocr" &&
      !lost
    ) {
      lost = true;
      const response = await route.fetch();
      expect(response.ok()).toBe(true);
      await response.dispose();
      await route.abort("connectionreset");
    } else await route.continue();
  });
  await region
    .getByRole("button", { name: "Queue PDF page OCR", exact: true })
    .click();
  await expect(region.getByRole("alert")).toContainText(
    "reuses its request key",
  );
  const queued: ProcessingJob = core({
    action: "list_processing_jobs",
  }).jobs.find(
    (job: ProcessingJob) =>
      job.input.evidence_id === id &&
      job.input.operation === "pdf_page_ocr" &&
      job.input.page_number === 3,
  );
  await region.getByLabel("Page number (1-based)").fill("4");
  await region
    .getByRole("button", { name: "Queue PDF page OCR", exact: true })
    .click();
  await expect(
    jobDialog(page).getByText("PDF page OCR · page 4 · 144 DPI · English", {
      exact: true,
    }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await region.getByLabel("Page number (1-based)").fill("3");
  await region.getByLabel("Resolution (DPI)").fill("145");
  await region
    .getByRole("button", { name: "Queue PDF page OCR", exact: true })
    .click();
  await expect(
    jobDialog(page).getByText("PDF page OCR · page 3 · 145 DPI · English", {
      exact: true,
    }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await region.getByLabel("Resolution (DPI)").fill("144");
  await region
    .getByLabel("Original to process")
    .selectOption(fixture("unsupported").input.evidence_id);
  await region
    .getByRole("button", { name: "Queue PDF page OCR", exact: true })
    .click();
  await expect(
    jobDialog(page).getByRole("heading", {
      name: "pdf-review-unsupported.pdf",
    }),
  ).toBeVisible();
  await expect(
    jobDialog(page).getByText("PDF page OCR · page 3 · 144 DPI · English", {
      exact: true,
    }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await region.getByLabel("Original to process").selectOption(id);
  await region.getByLabel("Processing method").selectOption("parse_document");
  await region
    .getByRole("button", { name: "Queue document", exact: true })
    .click();
  await expect(
    jobDialog(page).getByText("Document parsing", { exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  core({
    action: "cancel_processing_job",
    job_id: queued.id,
    expected_attempt: 1,
  });
  await region.getByLabel("Original to process").selectOption(id);
  await region.getByLabel("Processing method").selectOption("pdf_page_ocr");
  await region.getByLabel("Page number (1-based)").fill("3");
  await region
    .getByRole("button", { name: "Recover queue acknowledgement" })
    .click();
  await expect(
    jobDialog(page).getByText(queued.id, { exact: true }),
  ).toBeVisible();
  await expect(jobDialog(page).locator(".processing-banner strong")).toHaveText(
    "Cancelled",
  );
  expect(core({ action: "list_processing_jobs" }).total).toBe(16);
});

test("clipboard refusal selects inert PDF recognition without changing the canonical result", async ({
  page,
}) => {
  const before = extraction(fixture("recognized").result_ids[0]);
  const { dialog } = await openResult(page, "recognized");
  // A local platform-permission failure, not a fabricated core response.
  await page.evaluate(() => {
    Object.defineProperty(navigator.clipboard, "writeText", {
      value: async () => {
        throw new DOMException(
          "Synthetic clipboard refusal",
          "NotAllowedError",
        );
      },
    });
  });
  await dialog.getByRole("button", { name: "Copy OCR text" }).click();
  await expect(dialog.getByRole("status")).toHaveText(
    "Clipboard unavailable. The text is selected; use your system copy shortcut.",
  );
  const text = dialog.getByRole("textbox", { name: "Unreviewed OCR text" });
  await expect(text).toBeFocused();
  expect(
    await text.evaluate((element: HTMLTextAreaElement) => [
      element.selectionStart,
      element.selectionEnd,
    ]),
  ).toEqual([0, before.result.recognition!.text.length]);
  expect(extraction(before.id)).toEqual(before);
});

test("retry keeps the earlier PDF outcome immutable and binds each derivative to its own attempt", async ({
  page,
}) => {
  const job = fixture("retry"),
    before = job.result_ids.map(extraction);
  const { dialog, job: parent } = await openResult(page, "retry");
  await expect(fact(dialog, "Recorded attempt")).toHaveText("1");
  await expect(
    dialog.getByRole("button", { name: "Copy OCR text" }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await parent
    .getByRole("button", { name: /Inspect extraction/ })
    .nth(1)
    .click();
  await expect(
    dialog.getByText("Text recognized · unreviewed", { exact: true }),
  ).toBeVisible();
  await expect(fact(dialog, "Recorded attempt")).toHaveText("2");
  expect(job.result_ids.map(extraction)).toEqual(before);
});

test("late extraction replies and failed reads never replace the current PDF review", async ({
  page,
}) => {
  const job = await openJob(page, "recognized");
  let release!: () => void, started!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const pending = new Promise<void>((resolve) => {
    started = resolve;
  });
  const originalId = fixture("recognized").result_ids[0];
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (
      input.action === "inspect_pdf_extraction" &&
      input.extraction_id === originalId
    ) {
      started();
      await held;
    }
    await route.continue();
  });
  await job.getByRole("button", { name: /Inspect extraction/ }).click();
  await pending;
  await resultDialog(page)
    .getByRole("button", { name: "Close PDF page OCR review" })
    .click();
  await page.keyboard.press("Escape");
  await page.locator(`#document-job-${fixture("empty").id}`).click();
  await job.getByRole("button", { name: /Inspect extraction/ }).click();
  await expect(
    resultDialog(page).getByText("No text recognized · unreviewed", {
      exact: true,
    }),
  ).toBeVisible();
  const delivered = page.waitForResponse(
    (response) =>
      response.request().postDataJSON()?.extraction_id === originalId,
  );
  release();
  await delivered;
  await expect(
    resultDialog(page).getByRole("textbox", { name: "Unreviewed OCR text" }),
  ).toHaveCount(0);
  await page.unrouteAll();
  await page.keyboard.press("Escape");
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "inspect_pdf_extraction")
      await route.abort("connectionreset");
    else await route.continue();
  });
  await job.getByRole("button", { name: /Inspect extraction/ }).click();
  await expect(resultDialog(page).getByRole("alert")).toContainText(
    "PDF extraction unavailable",
  );
  await expect(
    resultDialog(page).getByRole("button", { name: "Copy OCR text" }),
  ).toHaveCount(0);
  await page.unrouteAll();
  await resultDialog(page)
    .getByRole("button", { name: "Reload PDF extraction" })
    .click();
  await expect(
    resultDialog(page).getByText("No text recognized · unreviewed", {
      exact: true,
    }),
  ).toBeVisible();
});

test("compact queue and PDF review retain keyboard provenance, focus and accessible containment", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 900 });
  const region = await list(page);
  await region.getByLabel("Processing method").selectOption("pdf_page_ocr");
  await region
    .getByLabel("Original to process")
    .selectOption(fixture("recognized").input.evidence_id);
  await region.getByLabel("Page number (1-based)").fill("2");
  await axe(page, 'section[aria-label="Document jobs"]', "queue-compact");
  expect(
    await region.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await region.screenshot({ path: resolve(captures, "queue-compact.png") });
  await region.locator(`#document-job-${fixture("recognized").id}`).click();
  const opener = jobDialog(page).getByRole("button", {
    name: /Inspect extraction/,
  });
  await opener.click();
  const dialog = resultDialog(page);
  await expect(
    dialog.getByRole("textbox", { name: "Unreviewed OCR text" }),
  ).toBeVisible();
  await dialog.getByRole("textbox", { name: "Unreviewed OCR text" }).focus();
  await page.keyboard.press("Tab");
  const summary = dialog.locator("summary");
  await expect(summary).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(fact(dialog, "OCR engine")).toBeVisible();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close PDF page OCR review" }),
  ).toBeFocused();
  await axe(page, 'dialog[aria-label="PDF page OCR review"]', "compact");
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await dialog.screenshot({
    path: resolve(captures, "recognized-compact.png"),
  });
  await page.setViewportSize({ width: 720, height: 500 });
  await summary.click();
  await axe(
    page,
    'dialog[aria-label="PDF page OCR review"]',
    "compact-provenance",
  );
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
});
