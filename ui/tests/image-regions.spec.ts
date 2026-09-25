import { test, expect, type Page, type Locator } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  rmSync,
  mkdirSync,
  renameSync,
} from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { ProcessingJob, ProcessingJobPage } from "../src/processing-types";
import type { ImageRegionInspection } from "../src/image-region-types";
import type { Workspace } from "../src/types";
import {
  parseRegionRaster,
  verifyRegionRaster,
} from "../src/image-region-raster";
const root = resolve("artifacts/synthetic-ui-workspace"),
  executable = resolve("target/debug/ew-dev"),
  captures = resolve("artifacts/image-regions");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let jobs: ProcessingJob[], workspace: Workspace;
const fixture = (name: string) =>
  jobs.find(
    (job) =>
      job.input.evidence_id ===
      workspace.evidence.find((item) =>
        item.name.startsWith(`region-review-${name}.`),
      )!.id,
  )!;
const inspect = (name: string, index = 0): ImageRegionInspection =>
  core({
    action: "inspect_image_region_extraction",
    extraction_id: fixture(name).result_ids[index],
  });
const resultDialog = (page: Page) =>
  page.getByRole("dialog", { name: "Image word regions", exact: true });
const jobDialog = (page: Page) =>
  page.getByRole("dialog", { name: "Document job", exact: true });
const fact = (dialog: Locator, name: string) =>
  dialog
    .locator("dt")
    .filter({ hasText: new RegExp(`^${name}$`) })
    .locator("+ dd");
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  execFileSync(executable, ["seed-image-region-review", root]);
  workspace = core({ action: "view" }).workspace;
  jobs = (core({ action: "list_processing_jobs" }) as ProcessingJobPage).jobs;
  mkdirSync(captures, { recursive: true });
});
async function list(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  const panel = page.getByRole("region", {
    name: "Document jobs",
    exact: true,
  });
  await expect(
    panel.getByText("12 shown · 12 loaded / 12 total"),
  ).toBeVisible();
  return panel;
}
async function open(page: Page, name: string, resultIndex = 0) {
  const panel = await list(page);
  await panel.locator(`#document-job-${fixture(name).id}`).click();
  const opener = jobDialog(page)
    .getByRole("button", { name: /Inspect extraction/ })
    .nth(resultIndex);
  await opener.click();
  const dialog = resultDialog(page);
  await expect(
    dialog.getByText("Loading verified image regions…"),
  ).not.toBeVisible();
  return { dialog, opener };
}
async function axe(page: Page, label: string) {
  const result = await new AxeBuilder({ page })
    .include('dialog[aria-label="Image word regions"]')
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `accessibility-${label}.json`),
    JSON.stringify(
      { scope: "Image word regions", violations: result.violations },
      null,
      2,
    ) + "\n",
  );
  expect(result.violations).toEqual([]);
}
test("canonical binary transport and strict bounded P5 validation reject altered bytes and identities", async () => {
  const value = inspect("recognized"),
    key = value.extraction.id;
  const bytes = execFileSync(executable, [
    "read-image-region-raster",
    root,
    key,
  ]);
  expect(bytes).toEqual(
    readFileSync(
      resolve(root, "derivatives/objects", value.extraction.raster!.sha256),
    ),
  );
  const buffer = bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
  expect(
    (await verifyRegionRaster(buffer, value.extraction.raster!, 1200, 230))
      .width,
  ).toBe(1200);
  const modified = buffer.slice(0);
  new Uint8Array(modified)[modified.byteLength - 1] ^= 1;
  await expect(
    verifyRegionRaster(modified, value.extraction.raster!, 1200, 230),
  ).rejects.toThrow(/checksum/);
  for (const bad of [
    "P5\n01 1\n255\nX",
    "P5\n8193 1\n255\nX",
    "P5\n6000 6000\n255\nX",
    "P5\n1 1\n256\nX",
    "P5\n1 1\n255\nXX",
  ])
    expect(() =>
      parseRegionRaster(
        new TextEncoder().encode(bad).buffer as ArrayBuffer,
        1,
        1,
      ),
    ).toThrow();
  expect(() => parseRegionRaster(buffer, 1199, 230)).toThrow(/dimensions/);
  expect(() => parseRegionRaster(new ArrayBuffer(12_000_033), 1, 1)).toThrow(
    /bounds/,
  );
  for (const args of [
    [root, "../originals"],
    [root, "0".repeat(64)],
    [root, key, "extra"],
  ]) {
    const failed = spawnSync(executable, ["read-image-region-raster", ...args]);
    expect(failed.status).toBe(1);
    expect(failed.stdout.length).toBe(0);
    expect(failed.stderr.toString()).toBe(
      "Retained raster could not be verified or read\n",
    );
  }
});
test("word review binds four identities, displays inert pixels/text, and selects boxes at fit and 100 percent", async ({
  page,
  context,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("request", (request) => {
    if (!request.url().startsWith("http://127.0.0.1:1420/"))
      external.push(request.url());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  const value = inspect("recognized"),
    before = core({ action: "view" }).workspace.revision;
  const { dialog, opener } = await open(page, "recognized");
  await expect(
    dialog.getByText("Words recognized · unreviewed", { exact: true }),
  ).toBeVisible();
  for (const [label, text] of [
    ["Original SHA-256", value.extraction.input.sha256],
    ["Raster SHA-256", value.extraction.raster!.sha256],
    ["TSV SHA-256", value.extraction.tsv!.sha256],
    ["Result SHA-256", value.extraction.result.sha256],
  ])
    await expect(fact(dialog, label)).toHaveText(text);
  const words = value.result.recognition!.regions.filter(
      (r) => r.level === "word",
    ),
    index = words.findIndex((r) => r.text === "0042"),
    word = words[index];
  const button = dialog.getByRole("button", {
    name: `Select word ${index + 1}: 0042`,
    exact: true,
  });
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(
    dialog
      .getByRole("region", { name: "Selected word" })
      .getByText("Engine score 96.123456 / 100", { exact: true }),
  ).toBeVisible();
  await expect(fact(dialog, "Raster rectangle")).toHaveText(
    `Left ${word.bounds.left} · top ${word.bounds.top} · width ${word.bounds.width} · height ${word.bounds.height}`,
  );
  const canvas = dialog.locator("canvas"),
    box = await canvas.boundingBox();
  const other = words[0].bounds;
  await canvas.click({
    position: {
      x: ((other.left + 9) * box!.width) / 1200,
      y: ((other.top + 15) * box!.height) / 230,
    },
  });
  await expect(
    dialog.getByRole("button", { name: "Select word 1: FIXED", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await dialog.getByRole("button", { name: "100%", exact: true }).click();
  expect((await canvas.boundingBox())!.width).toBe(1200);
  await canvas.click({
    position: { x: word.bounds.left + 9, y: word.bounds.top + 15 },
  });
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await dialog.getByRole("button", { name: "Hide boxes" }).click();
  const pixel = await canvas.evaluate((el: HTMLCanvasElement) => [
    ...el.getContext("2d")!.getImageData(500, 200, 1, 1).data,
  ]);
  const raw = execFileSync(executable, [
    "read-image-region-raster",
    root,
    value.extraction.id,
  ]);
  const pixels = parseRegionRaster(
    raw.buffer.slice(
      raw.byteOffset,
      raw.byteOffset + raw.byteLength,
    ) as ArrayBuffer,
    1200,
    230,
  ).pixels;
  expect(pixel).toEqual([
    pixels[200 * 1200 + 500],
    pixels[200 * 1200 + 500],
    pixels[200 * 1200 + 500],
    255,
  ]);
  await dialog.getByRole("button", { name: "Show boxes" }).click();
  await dialog.getByRole("button", { name: "Fit", exact: true }).click();
  const text = dialog.getByRole("textbox", { name: "Unreviewed OCR text" });
  await expect(text).toHaveValue(value.result.recognition!.text);
  await expect(text).toHaveAttribute("readonly", "");
  expect(
    await page.evaluate(
      () => (window as unknown as Record<string, unknown>).regionExecuted,
    ),
  ).toBeUndefined();
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await dialog.getByRole("button", { name: "Copy text" }).click();
  await expect(dialog.getByRole("status")).toHaveText(
    "Unreviewed OCR text copied to the clipboard.",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    value.result.recognition!.text,
  );
  await axe(page, "desktop");
  await dialog.screenshot({
    path: resolve(captures, "recognized-details.png"),
  });
  await dialog.evaluate((el) => {
    el.scrollTop = 0;
  });
  await dialog.screenshot({
    path: resolve(captures, "recognized-desktop.png"),
  });
  await page.setViewportSize({ width: 1440, height: 2600 });
  await dialog.evaluate((el) => {
    el.scrollTop = 0;
  });
  await dialog.screenshot({ path: resolve(captures, "recognized-full.png") });
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
  expect(core({ action: "view" }).workspace.revision).toBe(before);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});
test("word pagination preserves full denominator and selected identity in compact keyboard layout", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 900 });
  const value = inspect("many"),
    count = value.result.recognition!.regions.filter(
      (r) => r.level === "word",
    ).length;
  const { dialog } = await open(page, "many");
  await expect(
    dialog.getByText(`1–50 of ${count} words`, { exact: true }),
  ).toBeVisible();
  await expect(dialog.locator(".region-word")).toHaveCount(50);
  await dialog.getByRole("button", { name: "Next words" }).click();
  await expect(
    dialog.getByText(`51–${count} of ${count} words`, { exact: true }),
  ).toBeVisible();
  await dialog.locator(".region-word").last().click();
  const selectedText = await dialog
    .getByRole("region", { name: "Selected word" })
    .innerText();
  await dialog.getByRole("button", { name: "Previous words" }).click();
  await expect(
    dialog.getByRole("button", { name: "Select word 1: FIXED", exact: true }),
  ).toBeInViewport();
  expect(
    await dialog.getByRole("region", { name: "Selected word" }).innerText(),
  ).toBe(selectedText);
  await dialog.locator("summary").focus();
  await page.keyboard.press("Enter");
  await expect(fact(dialog, "Source image index")).toHaveText("0");
  expect(
    await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
  ).toBe(true);
  await axe(page, "compact");
  await page.setViewportSize({ width: 720, height: 3800 });
  await dialog.evaluate((el) => {
    el.scrollTop = 0;
  });
  await dialog.screenshot({ path: resolve(captures, "many-compact.png") });
  await page.setViewportSize({ width: 720, height: 500 });
  expect(
    await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
  ).toBe(true);
});
test("empty OCR keeps verified raster while decoder rejections and worker failures invent no words", async ({
  page,
}) => {
  for (const [name, heading] of [
    ["empty", "No text recognized · unreviewed"],
    ["unsupported", "Image input unsupported"],
    ["failed", "Image decoding failed"],
    ["quota", "Image resource limit exhausted"],
  ]) {
    const { dialog } = await open(page, name);
    await expect(dialog.getByText(heading, { exact: true })).toBeVisible();
    await expect(dialog.locator("textarea")).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Copy text" }),
    ).toBeDisabled();
    await expect(dialog.locator("canvas")).toHaveCount(
      name === "empty" ? 1 : 0,
    );
    if (name === "empty") {
      expect(inspect(name).result.recognition!.text).toBe("\n\f");
      await expect(
        dialog.getByText("0 of 0 words", { exact: true }),
      ).toBeVisible();
    }
    await dialog.screenshot({ path: resolve(captures, `${name}.png`) });
  }
  const panel = await list(page);
  await panel.locator(`#document-job-${fixture("worker_failed").id}`).click();
  await expect(
    jobDialog(page).getByText("No extraction has been published for this job."),
  ).toBeVisible();
});
test("missing raster and altered TSV fail real inspection without cached success or empty-result claims", async ({
  page,
}) => {
  const value = inspect("recognized"),
    { dialog } = await open(page, "recognized");
  await page.keyboard.press("Escape");
  const path = resolve(
    root,
    "derivatives/objects",
    value.extraction.raster!.sha256,
  );
  renameSync(path, path + ".missing");
  await jobDialog(page)
    .getByRole("button", { name: /Inspect extraction/ })
    .click();
  await expect(
    dialog.getByText("Derivative could not be verified", { exact: true }),
  ).toBeVisible();
  await expect(dialog.locator("canvas,textarea,.region-word")).toHaveCount(0);
  renameSync(path + ".missing", path);
  await dialog.getByRole("button", { name: "Retry inspection" }).click();
  await expect(dialog.locator("canvas")).toHaveCount(1);
  await page.keyboard.press("Escape");
  const tsv = resolve(
    root,
    "derivatives/objects",
    value.extraction.tsv!.sha256,
  );
  renameSync(tsv, tsv + ".original");
  writeFileSync(tsv, "altered", { mode: 0o400 });
  await jobDialog(page)
    .getByRole("button", { name: /Inspect extraction/ })
    .click();
  await expect(
    dialog.getByText("Derivative could not be verified", { exact: true }),
  ).toBeVisible();
  await expect(dialog.locator("canvas,textarea,.region-word")).toHaveCount(0);
  await dialog.screenshot({ path: resolve(captures, "unavailable.png") });
});
test("late binary response after close cannot replace a new derivative", async ({
  page,
}) => {
  let release!: () => void, captured!: () => void, finished!: () => void;
  const waiting = new Promise<void>((resolve) => {
      captured = resolve;
    }),
    gate = new Promise<void>((resolve) => {
      release = resolve;
    });
  const complete = new Promise<void>((resolve) => {
    finished = resolve;
  });
  await page.route("**/api/image-region-raster", async (route) => {
    const response = await route.fetch();
    captured();
    await gate;
    await route.fulfill({ response });
    finished();
  });
  const panel = await list(page);
  await panel.locator(`#document-job-${fixture("recognized").id}`).click();
  await jobDialog(page)
    .getByRole("button", { name: /Inspect extraction/ })
    .click();
  await waiting;
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  await panel.locator(`#document-job-${fixture("unsupported").id}`).click();
  await jobDialog(page)
    .getByRole("button", { name: /Inspect extraction/ })
    .click();
  await expect(
    resultDialog(page).getByText("Image input unsupported", { exact: true }),
  ).toBeVisible();
  release();
  await complete;
  await expect(resultDialog(page).locator("canvas,textarea")).toHaveCount(0);
});
test("opt-in queue acknowledgement survives navigation and terminal transition; cancel and retry remain canonical", async ({
  page,
}) => {
  let panel = await list(page);
  const source = fixture("recognized").input.evidence_id;
  await panel.getByLabel("Processing method").selectOption("image_ocr_regions");
  await panel.getByLabel("Original to process").selectOption(source);
  await expect(
    panel.getByText(/This method retains the canonical grayscale raster/),
  ).toBeVisible();
  let queued: ProcessingJob | undefined;
  await page.route("**/api/workbench", async (route) => {
    if (
      route.request().postDataJSON().action === "queue_image_ocr_regions" &&
      !queued
    ) {
      const response = await route.fetch();
      queued = await response.json();
      await route.abort("failed");
    } else await route.continue();
  });
  await panel.getByRole("button", { name: "Queue word-region OCR" }).click();
  await expect(panel.getByRole("alert")).toBeVisible();
  core({
    action: "cancel_processing_job",
    job_id: queued!.id,
    expected_attempt: queued!.attempt,
  });
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  panel = page.getByRole("region", { name: "Document jobs", exact: true });
  await panel.getByLabel("Processing method").selectOption("image_ocr_regions");
  await panel.getByLabel("Original to process").selectOption(source);
  await panel
    .getByRole("button", { name: "Recover queue acknowledgement" })
    .click();
  await expect(
    jobDialog(page).getByText(queued!.id, { exact: true }),
  ).toBeVisible();
  await expect(
    jobDialog(page).getByText("Cancelled", { exact: true }),
  ).toBeVisible();
  expect(
    (core({ action: "list_processing_jobs" }) as ProcessingJobPage).total,
  ).toBe(13);
  await jobDialog(page).getByRole("button", { name: "Review retry" }).click();
  const retry = page.getByRole("dialog", {
    name: "Retry document job",
    exact: true,
  });
  await retry
    .getByLabel("Reason for retry")
    .fill("Synthetic region UI retry verification");
  await retry.getByRole("button", { name: "Reserve retry attempt" }).click();
  await expect(
    jobDialog(page).getByText("Attempt 2 reserved and queued."),
  ).toBeVisible();
  await jobDialog(page)
    .getByRole("button", { name: "Cancel attempt 2" })
    .click();
  await expect(
    jobDialog(page).getByText("Cancelled", { exact: true }),
  ).toBeVisible();
});
test("binary bridge refuses cross-origin, invalid and extra inputs without exposing files", async ({
  request,
}) => {
  const id = inspect("recognized").extraction.id;
  for (const [data, origin] of [
    [{ extraction_id: id }, "http://example.invalid"],
    [{ extraction_id: "../workspace.db" }, "http://127.0.0.1:1420"],
    [{ extraction_id: id, path: "workspace.db" }, "http://127.0.0.1:1420"],
  ] as const) {
    const response = await request.post("/api/image-region-raster", {
      data,
      headers: { Origin: origin },
    });
    expect(response.ok()).toBe(false);
    expect(await response.text()).not.toContain(root);
  }
});

test("WebCrypto unavailable fails display explicitly without exposing unverified pixels or text", async ({
  page,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(Crypto.prototype, "subtle", { get: () => undefined }),
  );
  const { dialog } = await open(page, "recognized");
  await expect(
    dialog.getByText(/Verified raster display requires WebCrypto/),
  ).toBeVisible();
  await expect(dialog.locator("canvas,textarea,.region-word")).toHaveCount(0);
});
