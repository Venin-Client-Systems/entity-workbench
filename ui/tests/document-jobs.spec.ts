import { cancellationNotice } from "../src/processing-types";
import { test, expect, type Page, type Locator } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace } from "../src/types";
import type {
  Extraction,
  ProcessingJob,
  ProcessingJobPage,
} from "../src/processing-types";
const root = resolve("artifacts/synthetic-ui-workspace");
const executable = resolve("target/debug/ew-dev");
const captures = resolve("artifacts/document-jobs");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let workspace: Workspace;
let jobs: ProcessingJob[];
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  workspace = JSON.parse(
    execFileSync(executable, ["seed-processing-review", root], {
      encoding: "utf8",
    }),
  );
  jobs = (core({ action: "list_processing_jobs" }) as ProcessingJobPage).jobs;
});
function fixture(name: string) {
  return jobs.find(
    (job) =>
      job.input.evidence_id ===
      workspace.evidence.find((item) => item.name === `synthetic-${name}`)!.id,
  )!;
}
async function openList(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  const list = page.getByRole("region", { name: "Document jobs", exact: true });
  await expect(list.getByText("11 shown · 11 loaded / 11 total")).toBeVisible();
  return list;
}
async function openJob(page: Page, name: string) {
  const list = await openList(page);
  const opener = list.getByRole("button", {
    name: `Inspect document job synthetic-${name} ${fixture(name).id}`,
    exact: true,
  });
  await opener.click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(dialog.getByText("Loading document job…")).not.toBeVisible();
  return { dialog, opener };
}
async function openExtraction(page: Page, jobDialog: Locator) {
  const opener = jobDialog
    .getByRole("button", { name: /Inspect extraction/ })
    .first();
  await opener.click();
  const dialog = page.getByRole("dialog", {
    name: "Extraction review",
    exact: true,
  });
  await expect(
    dialog.getByText("Loading immutable extraction…"),
  ).not.toBeVisible();
  return { dialog, opener };
}
const axe = async (page: Page, scope: string, name: string) => {
  const result = await new AxeBuilder({ page })
    .include(scope)
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  mkdirSync(captures, { recursive: true });
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
};

test("immutable partial extraction escapes text and metadata, exposes exact provenance and restores focus", async ({
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
  const { dialog: jobDialog, opener: jobOpener } = await openJob(
    page,
    "partial.pdf",
  );
  const { dialog, opener } = await openExtraction(page, jobDialog);
  const record: Extraction = core({
    action: "inspect_extraction",
    extraction_id: fixture("partial.pdf").result_ids[0],
  });
  await expect(
    dialog.getByText("Partial parser output · unreviewed", { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("textbox", { name: "Unreviewed extracted text" }),
  ).toHaveValue(record.result.text);
  await expect(
    dialog.getByRole("textbox", { name: "Unreviewed extracted text" }),
  ).toHaveAttribute("readonly", "");
  await expect(
    dialog
      .locator("dt")
      .filter({ hasText: /^Original SHA-256$/ })
      .locator("+ dd"),
  ).toHaveText(record.input.sha256);
  await expect(
    dialog.getByText(record.result_sha256, { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByText('<svg onload="window.metadataExecuted=true">', {
      exact: true,
    }),
  ).toBeVisible();
  expect(record.schema_version).toBe(2);
  expect(record.result.protocol_version).toBe(1);
  expect(record.result.parser).toBe("pdfbox-3.0.8-local-fonts-v1");
  await expect(dialog.getByText(/OCR was not performed/)).toBeVisible();
  await expect(
    dialog.getByText(/An app-local font was substituted/),
  ).toBeVisible();
  await expect(
    dialog.getByText(/Font coverage has not been verified/),
  ).toBeVisible();
  await expect(dialog.locator("script,img,svg,iframe")).toHaveCount(0);
  expect(
    await page.evaluate(() => ({
      extracted: (window as unknown as Record<string, unknown>)
        .extractionExecuted,
      metadata: (window as unknown as Record<string, unknown>).metadataExecuted,
    })),
  ).toEqual({ extracted: undefined, metadata: undefined });
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await dialog.getByRole("button", { name: "Copy text", exact: true }).click();
  await expect(dialog.getByRole("status")).toHaveText(
    "Unreviewed text copied to the clipboard.",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    record.result.text,
  );
  await axe(
    page,
    'dialog[aria-label="Extraction review"]',
    "extraction-desktop",
  );
  await dialog.screenshot({
    path: resolve(captures, "extraction-desktop.png"),
  });
  await page.keyboard.press("Escape");
  await expect(jobDialog).toBeVisible();
  await expect(opener).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(jobOpener).toBeFocused();
  expect(core({ action: "view" }).workspace.observations).toEqual([]);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});

test("real queued cancellation and explicit retry reserve attempts while prior extraction stays immutable", async ({
  page,
}) => {
  const original: Extraction = core({
    action: "inspect_extraction",
    extraction_id: fixture("partial.pdf").result_ids[0],
  });
  const { dialog } = await openJob(page, "partial.pdf");
  await dialog.screenshot({ path: resolve(captures, "job-desktop.png") });
  await dialog.getByRole("button", { name: "Review retry" }).click();
  const retry = page.getByRole("dialog", {
    name: "Retry document job",
    exact: true,
  });
  await expect(
    retry.getByRole("button", { name: "Reserve retry attempt" }),
  ).toBeDisabled();
  await retry
    .getByRole("textbox", { name: "Reason for retry" })
    .fill("Synthetic manual review: revisit text after runtime repair.");
  await axe(page, 'dialog[aria-label="Retry document job"]', "retry-desktop");
  await retry.screenshot({ path: resolve(captures, "retry-desktop.png") });
  await retry.getByRole("button", { name: "Reserve retry attempt" }).click();
  await expect(retry).not.toBeVisible();
  await expect(
    dialog.getByText("Attempt 2 reserved and queued."),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Close document job" }),
  ).toBeFocused();
  await dialog.getByRole("button", { name: "Cancel attempt 2" }).click();
  await expect(
    dialog.getByText(
      "Cancellation command acknowledged. Current state: Cancelled.",
    ),
  ).toBeVisible();
  expect(
    core({
      action: "inspect_processing_job",
      job_id: fixture("partial.pdf").id,
    }),
  ).toMatchObject({
    attempt: 2,
    state: "cancelled",
    result_ids: [original.id],
  });
  expect(
    core({ action: "inspect_extraction", extraction_id: original.id }),
  ).toEqual(original);
  await dialog.getByRole("button", { name: "Review retry" }).click();
  await retry
    .getByRole("textbox", { name: "Reason for retry" })
    .fill("Synthetic third reservation.");
  await retry.getByRole("button", { name: "Reserve retry attempt" }).click();
  await dialog.getByRole("button", { name: "Cancel attempt 3" }).click();
  await expect(dialog.getByText(/used all 3 reserved attempts/)).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Review retry" }),
  ).toHaveCount(0);
});

test("queue uses canonical command, list polls external state, and running cancellation is only a request", async ({
  page,
}) => {
  const list = await openList(page);
  const complete = fixture("complete.txt");
  await list
    .getByLabel("Original to process")
    .selectOption(complete.input.evidence_id);
  await list
    .getByRole("button", { name: "Queue document", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(
    dialog.getByRole("button", { name: "Cancel attempt 1" }),
  ).toBeVisible();
  const queued = (
    core({ action: "list_processing_jobs" }) as ProcessingJobPage
  ).jobs.find(
    (job) =>
      job.input.evidence_id === complete.input.evidence_id &&
      job.state === "queued",
  )!;
  expect(queued.attempt).toBe(1);
  await dialog.getByRole("button", { name: "Close document job" }).click();
  await expect
    .poll(() =>
      page.evaluate((jobId) => {
        const focused = document.activeElement;
        return (
          focused?.id === `document-job-${jobId}` ||
          focused?.tagName === "SELECT"
        );
      }, queued.id),
    )
    .toBe(true);
  core({
    action: "cancel_processing_job",
    job_id: queued.id,
    expected_attempt: 1,
  });
  const row = list.getByRole("listitem").filter({
    has: page.getByRole("button", {
      name: `Inspect document job synthetic-complete.txt ${queued.id}`,
      exact: true,
    }),
  });
  await expect(row.getByText("Cancelled", { exact: true })).toBeVisible();
  await list
    .getByRole("button", {
      name: `Inspect document job synthetic-running.txt ${fixture("running.txt").id}`,
      exact: true,
    })
    .click();
  await dialog.getByRole("button", { name: "Cancel attempt 1" }).click();
  await expect(
    dialog.getByRole("button", { name: "Cancellation requested", exact: true }),
  ).toBeDisabled();
  await expect(
    dialog.getByText(
      "The request is recorded. Worker exit and final cleanup are not yet confirmed.",
    ),
  ).toBeVisible();
  expect(
    core({
      action: "inspect_processing_job",
      job_id: fixture("running.txt").id,
    }),
  ).toMatchObject({
    state: "running",
    cancellation_requested: true,
    result_ids: [],
  });
});

test("stale retry is rejected by Rust and preserves reason without applying it to a newer attempt", async ({
  page,
}) => {
  const job = fixture("blocked.txt");
  const { dialog } = await openJob(page, "blocked.txt");
  await dialog.getByRole("button", { name: "Review retry" }).click();
  const retry = page.getByRole("dialog", {
    name: "Retry document job",
    exact: true,
  });
  await retry
    .getByRole("textbox", { name: "Reason for retry" })
    .fill("Keep this draft after a concurrent reservation.");
  // Delay forwarding only the user's real mutation; neither response nor contract is mocked.
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "retry_processing_job") {
      core({
        action: "retry_processing_job",
        job_id: job.id,
        expected_attempt: 1,
        reason: "Synthetic concurrent reservation.",
      });
      core({
        action: "cancel_processing_job",
        job_id: job.id,
        expected_attempt: 2,
      });
    }
    await route.continue();
  });
  await retry.getByRole("button", { name: "Reserve retry attempt" }).click();
  await expect(
    retry
      .getByRole("alert")
      .filter({ hasText: /attempt/ })
      .first(),
  ).toBeVisible();
  await expect(
    retry.getByRole("textbox", { name: "Reason for retry" }),
  ).toHaveValue("Keep this draft after a concurrent reservation.");
  await expect(
    retry.getByRole("button", { name: "Reserve retry attempt" }),
  ).toBeDisabled();
  expect(
    core({ action: "inspect_processing_job", job_id: job.id }),
  ).toMatchObject({ state: "cancelled", attempt: 2 });
  await page.keyboard.press("Escape");
  await expect(
    dialog.getByRole("button", { name: "Review retry" }),
  ).toBeFocused();
});

test("unsupported and failed parses, blocked runtime, limits, interruption and cleanup remain distinct", async ({
  page,
}) => {
  const cases = [
    ["unsupported.bin", "Blocked", "unsupported format"],
    ["font-asset.pdf", "Failed", "document failed"],
    ["blocked.txt", "Blocked", "runtime unavailable"],
    ["limits.txt", "Quota exhausted", "worker failed"],
    ["interrupted.txt", "Failed", "interrupted"],
    ["cleanup.txt", "Failed", "cleanup failed"],
  ];
  for (const [name, status, failure] of cases) {
    const { dialog } = await openJob(page, name);
    await expect(dialog.locator(".processing-banner strong")).toHaveText(
      status,
    );
    await expect(dialog.getByText(failure, { exact: true })).toBeVisible();
    if (name === "unsupported.bin" || name === "font-asset.pdf") {
      const { dialog: extraction } = await openExtraction(page, dialog);
      await expect(
        extraction.getByRole("button", { name: "Copy text", exact: true }),
      ).toBeDisabled();
      await expect(extraction.getByText(/No text was returned/)).toBeVisible();
      if (name === "font-asset.pdf") {
        const failed: Extraction = core({
          action: "inspect_extraction",
          extraction_id: fixture(name).result_ids[0],
        });
        expect(failed.schema_version).toBe(2);
        expect(failed.result.error).toBe("font_asset_unavailable");
        expect(failed.result.metadata).toEqual({});
        await expect(
          extraction.getByText(/Required bundled font asset is unavailable/),
        ).toBeVisible();
      }
      await expect(
        extraction.getByRole("textbox", { name: "Unreviewed extracted text" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
    }
    if (name === "cleanup.txt")
      await expect(dialog.getByText(/Scratch cleanup failed/)).toBeVisible();
    await page.keyboard.press("Escape");
  }
});

test("late reads after closing or changing selection do not replace the current job", async ({
  page,
}) => {
  const list = await openList(page);
  let release: (() => void) | undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let started: (() => void) | undefined;
  const pending = new Promise<void>((resolve) => {
    started = resolve;
  });
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (
      request.action === "inspect_processing_job" &&
      request.job_id === fixture("partial.pdf").id
    ) {
      started!();
      await held;
    }
    await route.continue();
  });
  await list
    .getByRole("button", {
      name: `Inspect document job synthetic-partial.pdf ${fixture("partial.pdf").id}`,
      exact: true,
    })
    .click();
  await pending;
  await page.getByRole("button", { name: "Close document job" }).click();
  await list
    .getByRole("button", {
      name: `Inspect document job synthetic-complete.txt ${fixture("complete.txt").id}`,
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(
    dialog.getByRole("heading", { name: "synthetic-complete.txt" }),
  ).toBeVisible();
  const delivered = page.waitForResponse((response) => {
    const input = response.request().postDataJSON();
    return (
      input?.action === "inspect_processing_job" &&
      input.job_id === fixture("partial.pdf").id
    );
  });
  release!();
  await (await delivered).finished();
  await expect(
    dialog.getByRole("heading", { name: "synthetic-partial.pdf" }),
  ).toHaveCount(0);
  await expect(
    dialog.getByRole("button", { name: "Review retry" }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await expect(list).toHaveCount(0);
});

test("industrial job list and review surfaces fit desktop and compact viewports with no axe violations", async ({
  page,
}) => {
  const list = await openList(page);
  await axe(page, ".processing-panel", "jobs-desktop");
  await page.setViewportSize({ width: 1440, height: 3200 });
  await list.screenshot({ path: resolve(captures, "jobs-full-surface.png") });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await list.getByLabel("Show document jobs").selectOption("active");
  await expect(list.getByText("3 shown · 11 loaded / 11 total")).toBeVisible();
  await list.scrollIntoViewIfNeeded();
  await list.screenshot({ path: resolve(captures, "jobs-desktop.png") });
  await page.setViewportSize({ width: 720, height: 900 });
  await list.screenshot({ path: resolve(captures, "jobs-compact.png") });
  await list.getByLabel("Show document jobs").selectOption("all");
  await list
    .getByRole("button", {
      name: `Inspect document job synthetic-partial.pdf ${fixture("partial.pdf").id}`,
      exact: true,
    })
    .click();
  const job = page.getByRole("dialog", { name: "Document job", exact: true });
  await expect(job.getByRole("button", { name: "Review retry" })).toBeVisible();
  await axe(page, 'dialog[aria-label="Document job"]', "job-compact");
  await job.screenshot({ path: resolve(captures, "job-compact.png") });
  const { dialog } = await openExtraction(page, job);
  await axe(
    page,
    'dialog[aria-label="Extraction review"]',
    "extraction-compact",
  );
  expect(
    await dialog.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await dialog.screenshot({
    path: resolve(captures, "extraction-compact.png"),
  });
  await page.setViewportSize({ width: 1440, height: 2200 });
  await dialog.screenshot({
    path: resolve(captures, "extraction-full-surface.png"),
  });
});

test("lost queue acknowledgement reuses the canonical request key instead of duplicating the job", async ({
  page,
}) => {
  const list = await openList(page);
  await list
    .getByLabel("Original to process")
    .selectOption(fixture("complete.txt").input.evidence_id);
  let lost = false;
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON()?.action === "queue_document_parse" &&
      !lost
    ) {
      lost = true;
      const response = await route.fetch(); // Execute the real Rust request, then lose only its acknowledgement.
      expect(response.ok()).toBe(true);
      await response.dispose();
      await route.abort("connectionreset");
    } else await route.continue();
  });
  await list
    .getByRole("button", { name: "Queue document", exact: true })
    .click();
  await expect(list.getByRole("alert")).toContainText("reuses its request key");
  await expect(list.getByText("12 shown · 12 loaded / 12 total")).toBeVisible();
  const before: ProcessingJobPage = core({ action: "list_processing_jobs" });
  const first = before.jobs.find(
    (job) =>
      job.input.evidence_id === fixture("complete.txt").input.evidence_id &&
      job.state === "queued",
  )!;
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  core({
    action: "cancel_processing_job",
    job_id: first.id,
    expected_attempt: 1,
  });
  const terminal: ProcessingJobPage = core({ action: "list_processing_jobs" });
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  await list
    .getByLabel("Original to process")
    .selectOption(first.input.evidence_id);
  await list
    .getByRole("button", { name: "Recover queue acknowledgement", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Document job",
    exact: true,
  });
  await expect(dialog.getByText(first.id, { exact: true })).toBeVisible();
  await expect(dialog.locator(".processing-banner strong")).toHaveText(
    "Cancelled",
  );
  expect(core({ action: "list_processing_jobs" })).toEqual(terminal);
});

test("clipboard refusal offers manual selection without altering the immutable extraction", async ({
  browser,
}) => {
  const context = await browser.newContext({ permissions: [] });
  const page = await context.newPage();
  try {
    // Real browser permission denial, not a replacement clipboard or API implementation.
    const session = await context.newCDPSession(page);
    await session.send("Browser.setPermission", {
      permission: { name: "clipboard-write" },
      setting: "denied",
      origin: "http://127.0.0.1:1420",
    });
    const { dialog: job } = await openJob(page, "complete.txt");
    const { dialog } = await openExtraction(page, job);
    await dialog
      .getByRole("button", { name: "Copy text", exact: true })
      .click();
    await expect(dialog.getByRole("status")).toContainText(
      "Clipboard unavailable",
    );
    const text = dialog.getByRole("textbox", {
      name: "Unreviewed extracted text",
    });
    await expect(text).toBeFocused();
    expect(
      await text.evaluate((element: HTMLTextAreaElement) => [
        element.selectionStart,
        element.selectionEnd,
      ]),
    ).toEqual([0, (await text.inputValue()).length]);
  } finally {
    await context.close();
  }
});

test("unverified worker exit stays failed after cancellation and visibly suspends further processing", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  workspace = JSON.parse(
    execFileSync(executable, ["seed-processing-recovery-review", root], {
      encoding: "utf8",
    }),
  );
  jobs = (core({ action: "list_processing_jobs" }) as ProcessingJobPage).jobs;
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  const list = page.getByRole("region", { name: "Document jobs", exact: true });
  await expect(list.getByText("2 shown · 2 loaded / 2 total")).toBeVisible();
  const failedCancellation: ProcessingJob = core({
    action: "cancel_processing_job",
    job_id: fixture("unverified-exit.txt").id,
    expected_attempt: 1,
  });
  expect(failedCancellation.state).toBe("failed");
  expect(cancellationNotice(failedCancellation)).toBe(
    "Cancellation command acknowledged. Current state: Failed; worker exit unverified.",
  );
  for (const name of ["unverified-exit.txt", "recovery-required.txt"]) {
    await list
      .getByRole("button", {
        name: `Inspect document job synthetic-${name} ${fixture(name).id}`,
        exact: true,
      })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Document job",
      exact: true,
    });
    await expect(
      dialog.getByText(
        name === "unverified-exit.txt"
          ? "worker exit unverified"
          : "recovery required",
        { exact: true },
      ),
    ).toBeVisible();
    await expect(
      dialog.getByRole("button", { name: "Review retry" }),
    ).toHaveCount(0);
    await expect(dialog.getByText("Cancelled", { exact: true })).toHaveCount(0);
    await expect(
      dialog.getByText(/No extraction has been published/),
    ).toBeVisible();
    if (name === "unverified-exit.txt")
      await expect(
        dialog.getByText(/Worker exit is unconfirmed/),
      ).toBeVisible();
    else
      await expect(
        dialog.getByText(/Document processing is suspended/),
      ).toBeVisible();
    await dialog.screenshot({ path: resolve(captures, `job-${name}.png`) });
    await page.keyboard.press("Escape");
  }
  await list
    .getByLabel("Original to process")
    .selectOption(fixture("unverified-exit.txt").input.evidence_id);
  await list
    .getByRole("button", { name: "Queue document", exact: true })
    .click();
  await expect(list.getByRole("alert")).toContainText(/suspended/);
  expect(core({ action: "list_processing_jobs" }).total).toBe(2);
});
