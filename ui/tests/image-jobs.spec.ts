import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { ProcessingJob, ProcessingJobPage } from "../src/processing-types";
import type { ImageExtraction } from "../src/image-processing-types";

const root = resolve("artifacts/synthetic-ui-workspace");
const executable = resolve("target/debug/ew-dev");
const captures = resolve("artifacts/image-jobs");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let jobs: ProcessingJob[];
const extraction = (id: string): ImageExtraction =>
  core({ action: "inspect_image_extraction", extraction_id: id });
const resultJob = (status: string) =>
  jobs.find(
    (job) =>
      job.attempt === 1 &&
      job.result_ids.length === 1 &&
      (extraction(job.result_ids[0]).result.recognition?.status ??
        extraction(job.result_ids[0]).result.decoder.status) === status,
  )!;
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  execFileSync(executable, ["seed-image-processing-review", root]);
  jobs = (core({ action: "list_processing_jobs" }) as ProcessingJobPage).jobs;
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
    region.getByText("10 shown · 10 loaded / 10 total"),
  ).toBeVisible();
  return region;
}
async function openJob(page: Page, job: ProcessingJob) {
  const region = await list(page);
  await region.locator(`#document-job-${job.id}`).click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(
    dialog.getByText("Image OCR · English", { exact: true }),
  ).toBeVisible();
  return dialog;
}
async function openResult(page: Page, job: ProcessingJob, resultIndex = 0) {
  const jobDialog = await openJob(page, job);
  const opener = jobDialog
    .getByRole("button", { name: /Inspect extraction/ })
    .nth(resultIndex);
  await opener.click();
  const dialog = page.getByRole("dialog", {
    name: "Image OCR review",
    exact: true,
  });
  await expect(
    dialog.getByText("Loading immutable image extraction…"),
  ).not.toBeVisible();
  return { dialog, opener, jobDialog };
}
async function axe(page: Page, name: string) {
  const result = await new AxeBuilder({ page })
    .include('dialog[aria-label="Image OCR review"]')
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `accessibility-${name}.json`),
    JSON.stringify(
      {
        tags: ["wcag2a", "wcag2aa", "wcag21aa"],
        violations: result.violations,
      },
      null,
      2,
    ) + "\n",
  );
  expect(result.violations).toEqual([]);
}

test("OCR review displays separate original/raster/result identities and inert unreviewed text", async ({
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
  const job = resultJob("recognized"),
    record = extraction(job.result_ids[0]);
  const { dialog, opener } = await openResult(page, job);
  await expect(
    dialog.getByText("Text recognized · unreviewed", { exact: true }),
  ).toBeVisible();
  for (const [label, value] of [
    ["Original SHA-256", record.input.sha256],
    ["Raster SHA-256", record.result.decoder.raster!.sha256],
    ["Result SHA-256", record.result_sha256],
  ]) {
    await expect(
      dialog
        .locator("dt")
        .filter({ hasText: new RegExp(`^${label}$`) })
        .locator("+ dd"),
    ).toHaveText(value);
  }
  const text = dialog.getByRole("textbox", { name: "Unreviewed OCR text" });
  await expect(text).toHaveValue(record.result.recognition!.text);
  await expect(text).toHaveAttribute("readonly", "");
  await expect(dialog.locator("script,img,svg,iframe")).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as Record<string, unknown>).imageRecognitionExecuted,
    ),
  ).toBeUndefined();
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await dialog.getByRole("button", { name: "Copy OCR text" }).click();
  await expect(dialog.getByRole("status")).toHaveText(
    "Unreviewed OCR text copied to the clipboard.",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    record.result.recognition!.text,
  );
  await axe(page, "desktop");
  await dialog.screenshot({
    path: resolve(captures, "recognized-desktop.png"),
  });
  await page.setViewportSize({ width: 1440, height: 1800 });
  await dialog.screenshot({ path: resolve(captures, "recognized-full.png") });
  await dialog
    .getByText("Full processing provenance and limitations", { exact: true })
    .click();
  await expect(
    dialog.getByText(record.result.recognition!.model_sha256, { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByText(/Source image index zero is not a document page/),
  ).toBeVisible();
  await page.setViewportSize({ width: 1440, height: 2600 });
  await dialog.screenshot({
    path: resolve(captures, "recognized-provenance.png"),
  });
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  expect(core({ action: "view" }).workspace.observations).toEqual([]);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("blank recognition, unsupported input, decoder failure and pixel quota retain distinct explanations", async ({
  page,
}) => {
  for (const [status, label] of [
    ["no_text_recognized", "No text recognized · unreviewed"],
    ["unsupported", "Image input unsupported · OCR did not run"],
    ["failed", "Image decoding failed · OCR did not run"],
    ["quota_exhausted", "Image limit exhausted · OCR did not run"],
  ]) {
    const { dialog } = await openResult(page, resultJob(status));
    await expect(dialog.getByText(label, { exact: true })).toBeVisible();
    await expect(
      dialog.getByRole("button", { name: "Copy OCR text" }),
    ).toBeDisabled();
    await expect(
      dialog.getByRole("textbox", { name: "Unreviewed OCR text" }),
    ).toHaveCount(0);
    await expect(
      dialog.getByText(
        /not evidence that the original contains no relevant information/,
      ),
    ).toBeVisible();
    if (status !== "no_text_recognized") {
      await expect(
        dialog.getByText("Not produced", { exact: true }),
      ).toBeVisible();
      await expect(
        dialog.getByText(/validated raster was discarded/),
      ).toHaveCount(0);
    }
    await dialog.screenshot({ path: resolve(captures, `${status}.png`) });
  }
});

test("successful image retry keeps the failed earlier derivative and its original attempt", async ({
  page,
}) => {
  const job = jobs.find((item) => item.attempt === 2)!;
  const before = job.result_ids.map(extraction);
  const { dialog, jobDialog } = await openResult(page, job);
  await expect(
    dialog.getByText("Image decoding failed · OCR did not run", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    dialog
      .locator("dt")
      .filter({ hasText: /^Recorded attempt$/ })
      .locator("+ dd"),
  ).toHaveText("1");
  await page.keyboard.press("Escape");
  await jobDialog
    .getByRole("button", { name: /Inspect extraction/ })
    .nth(1)
    .click();
  await expect(
    dialog.getByText("Text recognized · unreviewed", { exact: true }),
  ).toBeVisible();
  await expect(
    dialog
      .locator("dt")
      .filter({ hasText: /^Recorded attempt$/ })
      .locator("+ dd"),
  ).toHaveText("2");
  expect(job.result_ids.map(extraction)).toEqual(before);
});

test("method-specific pending keys recover a lost OCR acknowledgement without confusing document parsing", async ({
  page,
}) => {
  const region = await list(page),
    id = resultJob("unsupported").input.evidence_id;
  await region.getByLabel("Original to process").selectOption(id);
  await region.getByLabel("Processing method").selectOption("image_ocr");
  let lost = false;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "queue_image_ocr" && !lost) {
      lost = true;
      const response = await route.fetch();
      expect(response.ok()).toBe(true);
      await response.dispose();
      await route.abort("connectionreset");
    } else await route.continue();
  });
  await region
    .getByRole("button", { name: "Queue image OCR", exact: true })
    .click();
  await expect(region.getByRole("alert")).toContainText(
    "reuses its request key",
  );
  const queued: ProcessingJob = core({
    action: "list_processing_jobs",
  }).jobs.find(
    (job: ProcessingJob) =>
      job.input.evidence_id === id && job.state === "queued",
  );
  expect(queued.input.operation).toBe("image_ocr");
  await region.getByLabel("Processing method").selectOption("parse_document");
  await region
    .getByRole("button", { name: "Queue document", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(
    dialog.getByText("Document parsing", { exact: true }),
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
  await region.getByLabel("Processing method").selectOption("image_ocr");
  await region
    .getByRole("button", { name: "Recover queue acknowledgement" })
    .click();
  await expect(dialog.getByText(queued.id, { exact: true })).toBeVisible();
  await expect(dialog.locator(".processing-banner strong")).toHaveText(
    "Cancelled",
  );
  expect(core({ action: "list_processing_jobs" }).total).toBe(12);
});

test("image review remains usable in compact and short viewports without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 900 });
  const { dialog } = await openResult(page, resultJob("recognized"));
  await axe(page, "compact");
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await dialog.screenshot({
    path: resolve(captures, "recognized-compact.png"),
  });
  await page.setViewportSize({ width: 720, height: 500 });
  await dialog
    .getByText("Full processing provenance and limitations", { exact: true })
    .click();
  await axe(page, "compact-provenance");
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await expect(
    dialog.getByRole("button", { name: "Close image OCR review" }),
  ).toBeAttached();
});
