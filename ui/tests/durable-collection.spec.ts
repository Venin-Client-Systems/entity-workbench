import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  rmSync,
  readFileSync,
  writeFileSync,
  chmodSync,
} from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type {
  CollectionRunPage,
  CollectionInspection,
} from "../src/durable-collection-types";
const root = resolve("artifacts/synthetic-ui-workspace"),
  exe = resolve("target/debug/ew-dev");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(exe, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
const captures = resolve("artifacts/durable-collection");
let revision: number;
let rows: CollectionRunPage["rows"];
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  execFileSync(exe, ["seed-durable-collection-review", root]);
  revision = core({ action: "view" }).workspace.revision;
  const a: CollectionRunPage = core({
    action: "page_collection_runs",
    request: { page_size: 25, cursor: null },
    expected_revision: revision,
  });
  const b: CollectionRunPage = core({
    action: "page_collection_runs",
    request: { page_size: 25, cursor: a.next_cursor },
    expected_revision: revision,
  });
  rows = [...a.rows, ...b.rows];
  mkdirSync(captures, { recursive: true });
});
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Discovery/ })
    .click();
  await expect(
    page.getByText("1–25 of 30 collections", { exact: true }),
  ).toBeVisible();
}
async function review(page: Page, state: string) {
  const run = rows.find((r) => r.state === state)!;
  if (rows.indexOf(run) >= 25)
    await page
      .getByRole("navigation", { name: "Collection catalogue pages" })
      .getByRole("button", { name: "Next", exact: true })
      .click();
  await page
    .getByRole("button", {
      name: `Review collection run ${run.id}`,
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Durable collection review",
    exact: true,
  });
  await expect(
    dialog.getByText("Recorded outcome", { exact: true }),
  ).toBeVisible();
  return { dialog, run };
}
async function axe(page: Page, name: string) {
  const result = await new AxeBuilder({ page }).analyze();
  writeFileSync(
    resolve(captures, name + "-axe.json"),
    JSON.stringify(
      {
        rules: result.passes.map((p) => p.id),
        violations: result.violations,
        incomplete: result.incomplete.map((p) => ({
          id: p.id,
          impact: p.impact,
        })),
      },
      null,
      2,
    ),
  );
  expect(result.violations).toEqual([]);
  await page.screenshot({
    path: resolve(captures, name + ".png"),
    fullPage: true,
  });
}
test("Rust normalizes preview and rejects invalid scope; controls stay honestly unavailable", async ({
  page,
}) => {
  const outbound: string[] = [];
  page.on("request", (r) => {
    if (r.url().startsWith("https://")) outbound.push(r.url());
  });
  await open(page);
  const form = page.getByRole("region", {
    name: "Direct public-web collection",
  });
  const preview = core({ action: "preview_collection", input: rows[0].input });
  expect(preview.collector_policy).toBe("direct-https-durable-html-bounded-v4");
  await form
    .getByLabel("Seed URLs", { exact: true })
    .fill("https://EXAMPLE.org:443/research/?x=1\nhttps://example.org/about/");
  await form.getByRole("button", { name: "Preview disclosure" }).click();
  await expect(
    form.getByText("https://example.org/research/?x=1", { exact: true }),
  ).toBeVisible();
  await expect(
    form.getByText("https://example.org/robots.txt", { exact: true }),
  ).toBeVisible();
  await expect(
    form.getByRole("button", { name: "Queue reviewed collection" }),
  ).toBeDisabled();
  await expect(form.getByText(/This development view/)).toBeVisible();
  await axe(page, "preview-wide");
  await page.setViewportSize({ width: 1440, height: 1800 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await form.screenshot({ path: resolve(captures, "preview-surface.png") });
  await form.getByLabel("Maximum requests").fill("51");
  await expect(
    form.getByRole("heading", { name: "Reviewed scope" }),
  ).toHaveCount(0);
  await form.getByRole("button", { name: "Preview disclosure" }).click();
  await expect(form.getByRole("alert")).toContainText(
    "Collection limits exceed policy",
  );
  expect(outbound).toEqual([]);
  expect(core({ action: "view" }).workspace.revision).toBe(revision);
});
test("late real preview cannot restore changed scope", async ({ page }) => {
  await open(page);
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action === "preview_collection") {
      const response = await route.fetch();
      entered = true;
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page.getByRole("button", { name: "Preview disclosure" }).click();
  await expect.poll(() => entered).toBe(true);
  await page
    .getByLabel("Seed URLs", { exact: true })
    .fill("https://different.example/");
  release();
  await expect(
    page.getByRole("heading", { name: "Reviewed scope" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Queue reviewed collection" }),
  ).toHaveCount(0);
});
test("complete bounded catalogue pages retain identity and reset on revision change", async ({
  page,
}) => {
  await open(page);
  const catalogue = page.getByRole("region", {
      name: "Durable collection catalogue",
    }),
    nav = catalogue.getByRole("navigation");
  const ids = () =>
    catalogue
      .getByRole("button", { name: /Review collection run / })
      .evaluateAll((bs) =>
        bs.map((b) =>
          b.getAttribute("aria-label")!.replace("Review collection run ", ""),
        ),
      );
  await expect.poll(ids).toEqual(rows.slice(0, 25).map((r) => r.id));
  await nav.getByRole("button", { name: "Next", exact: true }).click();
  await expect(catalogue.getByText("26–30 of 30 collections")).toBeVisible();
  expect(await ids()).toEqual(rows.slice(25).map((r) => r.id));
  await nav
    .getByRole("button", { name: "Back to previous collection page" })
    .click();
  await expect.poll(ids).toEqual(rows.slice(0, 25).map((r) => r.id));
  core({
    action: "import",
    name: "later.txt",
    bytes: Array.from(Buffer.from("Synthetic later canonical revision")),
  });
  await page
    .getByRole("button", { name: "Refresh collections", exact: true })
    .click();
  await expect(
    catalogue.getByText(
      `Oldest publication first · 25 collections per page · catalogue revision ${revision + 1}`,
    ),
  ).toBeVisible();
  await expect(
    nav.getByRole("button", { name: "Back to previous collection page" }),
  ).toBeDisabled();
});
test("recorded progress, exact source verification and keyboard modal use real retained receipts", async ({
  page,
}) => {
  await open(page);
  const { dialog, run } = await review(page, "successful");
  await expect(dialog.getByText("Successful", { exact: true })).toBeVisible();
  await expect(dialog.getByText("Unavailable", { exact: true })).toBeVisible();
  for (const name of [
    "Cancel collection",
    "Resume collection",
    "Retry saving response",
  ])
    await expect(
      dialog.getByRole("button", { name, exact: true }),
    ).toBeDisabled();
  const canonical: CollectionInspection = core({
    action: "inspect_collection_run",
    job_id: run.id,
  });
  expect(canonical.requests).toHaveLength(2);
  await expect(
    dialog.getByText(canonical.requests[1].original!.sha256, { exact: true }),
  ).toBeVisible();
  await dialog
    .getByRole("button", { name: "Inspect retained source", exact: true })
    .last()
    .click();
  const source = page.getByRole("dialog", {
    name: "Retained collection source",
    exact: true,
  });
  await expect(source.getByText(/Synthetic retained source/)).toBeVisible();
  await expect(source.locator("script")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(source).toHaveCount(0);
  await expect(
    dialog.getByRole("button", { name: "Inspect retained source" }).last(),
  ).toBeFocused();
  await axe(page, "review-wide");
  await page.setViewportSize({ width: 1440, height: 2400 });
  await dialog.evaluate((element) => {
    element.scrollTop = 0;
  });
  await dialog.screenshot({ path: resolve(captures, "review-surface.png") });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: `Review collection run ${run.id}` }),
  ).toBeFocused();
});
test("unknown completion remains distinct and compact controls remain accessible", async ({
  page,
}) => {
  await page.setViewportSize({ width: 760, height: 900 });
  await open(page);
  const { dialog } = await review(page, "interrupted");
  await expect(dialog.getByText("Interrupted", { exact: true })).toBeVisible();
  await expect(
    dialog.getByText(
      /The request may have reached the website. Its charge remains spent/,
    ),
  ).toBeVisible();
  await expect(
    dialog.getByText(
      "No complete response original was retained for this attempt.",
    ),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Resume collection", exact: true }),
  ).toBeDisabled();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await axe(page, "interrupted-compact");
  await dialog.evaluate((element) => {
    element.scrollTop = 0;
  });
  await dialog.screenshot({
    path: resolve(captures, "interrupted-compact-top.png"),
  });
  await dialog.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await dialog.screenshot({
    path: resolve(captures, "interrupted-compact-bottom.png"),
  });
});
test("a corrupted retained original makes inspection unavailable, never successful", async ({
  page,
}) => {
  await open(page);
  const run = rows.find((r) => r.state === "successful")!;
  const record: CollectionInspection = core({
    action: "inspect_collection_run",
    job_id: run.id,
  });
  const hash = record.requests[1].original!.sha256;
  const path = resolve(root, "originals", hash);
  const original = readFileSync(path);
  chmodSync(path, 0o600);
  writeFileSync(path, Buffer.alloc(original.length, 0x78));
  const { dialog } = await (async () => {
    await page
      .getByRole("button", { name: `Review collection run ${run.id}` })
      .click();
    return {
      dialog: page.getByRole("dialog", { name: "Durable collection review" }),
    };
  })();
  await expect(dialog.getByRole("alert")).toBeVisible();
  await expect(dialog.getByText("Successful", { exact: true })).toHaveCount(0);
  writeFileSync(path, original);
  chmodSync(path, 0o400);
  await dialog
    .getByRole("button", { name: "Refresh collection", exact: true })
    .click();
  await expect(dialog.getByText("Successful", { exact: true })).toBeVisible();
});

for (const failure of ["real standalone refusal", "injected preparation conflict"]) {
  test(`stale Cancel controls require refresh after ${failure}`, async ({ page }) => {
    await open(page);
    const run = rows.find((r) => r.state === "interrupted")!;
    const before = core({ action: "inspect_collection_run", job_id: run.id });
    let staleInspection = true;
    const requests: Record<string, unknown>[] = [];
    await page.route("**/api/workbench", async (route) => {
      const request = route.request().postDataJSON();
      if (request.action === "inspect_collection_run" && request.job_id === run.id) {
        const response = await route.fetch();
        if (staleInspection) {
          staleInspection = false;
          const value: CollectionInspection = await response.json();
          // Test-only stale availability: the standalone backend has no executor.
          // This exercises refusal recovery, not enabled native collection.
          value.availability = "synthetic_fixture";
          value.controls.can_cancel = true;
          await route.fulfill({ response, json: value });
        } else await route.fulfill({ response });
      } else if (request.action === "cancel_collection") {
        requests.push(request);
        if (failure === "injected preparation conflict") {
          // The Rust coordinator tests establish the real conflicting write.
          // Here only its transport/UI consequence is injected deliberately.
          await route.fulfill({
            status: 400,
            json: { error: "Collection changed during cancellation preparation" },
          });
        } else {
          const response = await route.fetch();
          expect(response.status()).toBe(400);
          await route.fulfill({ response });
        }
      } else await route.continue();
    });
    const { dialog } = await review(page, "interrupted");
    const cancel = dialog.getByRole("button", { name: "Cancel collection", exact: true });
    await expect(cancel).toBeEnabled();
    await cancel.click();
    await expect(dialog.getByRole("alert")).toContainText(
      "Control outcome is unconfirmed. Refresh before taking another action.",
    );
    if (failure === "injected preparation conflict")
      await expect(dialog.getByRole("alert")).toContainText(
        "Collection changed during cancellation preparation",
      );
    await expect(cancel).toHaveCount(0);
    await expect(dialog.getByText("Recorded outcome", { exact: true })).toHaveCount(0);
    await expect(dialog.getByRole("status")).toHaveCount(0);
    expect(requests).toEqual([
      { action: "cancel_collection", job_id: run.id, expected_generation: run.generation },
    ]);
    expect(core({ action: "inspect_collection_run", job_id: run.id })).toEqual(before);
    expect(core({ action: "view" }).workspace.revision).toBe(revision);
    await dialog.getByRole("button", { name: "Refresh collection", exact: true }).click();
    await expect(dialog.getByRole("alert")).toHaveCount(0);
    await expect(dialog.getByText("Interrupted", { exact: true })).toBeVisible();
    await expect(cancel).toBeDisabled();
    expect(requests).toHaveLength(1); // Refresh must not retry the mutation.
  });
}

test("held old inspection cannot populate a reopened different record", async ({
  page,
}) => {
  await open(page);
  const a = rows[0],
    b = rows[1];
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false;
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (
      q.action === "inspect_collection_run" &&
      q.job_id === a.id &&
      !entered
    ) {
      const response = await route.fetch();
      entered = true;
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page
    .getByRole("button", { name: `Review collection run ${a.id}` })
    .click();
  await expect.poll(() => entered).toBe(true);
  await page
    .getByRole("button", { name: "Close collection review", exact: true })
    .click();
  await page
    .getByRole("button", { name: `Review collection run ${b.id}` })
    .click();
  release();
  const dialog = page.getByRole("dialog", {
    name: "Durable collection review",
  });
  await expect(
    dialog.getByText("Recorded outcome", { exact: true }),
  ).toBeVisible();
  await expect(dialog.getByText(a.id, { exact: true })).toHaveCount(0);
  await expect(dialog.getByText(b.id, { exact: true })).toBeVisible();
});
test("catalogue failure stays unavailable and retry differs from an empty catalogue", async ({
  page,
}) => {
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    if (
      fail &&
      route.request().postDataJSON().action === "page_collection_runs"
    )
      await route.abort();
    else await route.continue();
  });
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Discovery/ })
    .click();
  await expect(
    page.getByText(/Collection catalogue unavailable:/),
  ).toBeVisible();
  await expect(page.getByText("No durable collections recorded.")).toHaveCount(
    0,
  );
  fail = false;
  await page
    .getByRole("button", { name: "Retry collection catalogue" })
    .click();
  await expect(
    page.getByText("1–25 of 30 collections", { exact: true }),
  ).toBeVisible();
});
test("uncertain genuinely refused queue retains exact identity through component remount", async ({
  page,
}) => {
  await page.goto("/");
  const bodies: unknown[] = [];
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false;
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (q.action === "queue_collection") {
      bodies.push(q);
      const response = await route.fetch();
      expect(response.status()).toBe(400);
      if (!entered) {
        entered = true;
        await held;
        await route.abort();
      } else await route.fulfill({ response });
    } else await route.continue();
  });
  await page.evaluate(
    async ({ revision }) => {
      const h = await import("/tests/durable-collection-harness.tsx");
      h.mount(revision, []);
      void h.initiateRefusedQueue(revision);
    },
    { revision },
  );
  await expect.poll(() => entered).toBe(true);
  await page.evaluate(async () => {
    (await import("/tests/durable-collection-harness.tsx")).unmount();
  });
  release();
  await expect
    .poll(async () =>
      page.evaluate(
        async () =>
          (await import("/tests/durable-collection-harness.tsx")).state().phase,
      ),
    )
    .toBe("uncertain");
  await page.evaluate(
    async ({ revision }) => {
      (await import("/tests/durable-collection-harness.tsx")).mount(
        revision,
        [],
      );
    },
    { revision },
  );
  await page
    .getByRole("button", { name: "Retry same collection", exact: true })
    .click();
  await expect.poll(() => bodies.length).toBe(2);
  expect(bodies[1]).toEqual(bodies[0]);
  await expect(page.getByText(/Queue completion is unconfirmed/)).toBeVisible();
  expect(core({ action: "view" }).workspace.revision).toBe(revision);
});

test("A to B to A reopening revalidates originals instead of accepting a held old success", async ({
  page,
}) => {
  await open(page);
  const a = rows.find((r) => r.state === "successful")!,
    b = rows[0];
  const inspect: CollectionInspection = core({
    action: "inspect_collection_run",
    job_id: a.id,
  });
  const path = resolve(root, "originals", inspect.requests[1].original!.sha256);
  const original = readFileSync(path);
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  let entered = false;
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (
      q.action === "inspect_collection_run" &&
      q.job_id === a.id &&
      !entered
    ) {
      const response = await route.fetch();
      entered = true;
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await page
    .getByRole("button", { name: `Review collection run ${a.id}` })
    .click();
  await expect.poll(() => entered).toBe(true);
  await page
    .getByRole("button", { name: "Close collection review", exact: true })
    .click();
  await page
    .getByRole("button", { name: `Review collection run ${b.id}` })
    .click();
  await page
    .getByRole("button", { name: "Close collection review", exact: true })
    .click();
  chmodSync(path, 0o600);
  writeFileSync(path, Buffer.alloc(original.length, 0x78));
  await page
    .getByRole("button", { name: `Review collection run ${a.id}` })
    .click();
  release();
  const dialog = page.getByRole("dialog", {
    name: "Durable collection review",
    exact: true,
  });
  await expect(dialog.getByRole("alert")).toBeVisible();
  await expect(dialog.getByText("Successful", { exact: true })).toHaveCount(0);
  writeFileSync(path, original);
  chmodSync(path, 0o400);
});

test("fresh workspace has an explicit empty durable catalogue and never enables queueing", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  core({ action: "view" });
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Discovery/ })
    .click();
  await expect(
    page.getByText("No durable collections recorded.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry collection catalogue" }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Preview disclosure" }).click();
  await expect(
    page.getByRole("button", { name: "Queue reviewed collection" }),
  ).toBeDisabled();
  await axe(page, "empty-preview");
});

test("queue acknowledgement binds the exact canonical request key even for identical input", async ({
  page,
}) => {
  const a = rows[0],
    b = rows[1];
  expect(a.input).toEqual(b.input);
  expect(a.record_version).toBe(4);
  expect(b.record_version).toBe(4);
  expect(rows.slice(2).every((row) => row.record_version === 3)).toBe(true);
  expect(a.request_key).not.toBe(b.request_key);
  const exact: CollectionInspection = core({
    action: "inspect_collection_run",
    job_id: a.id,
  });
  const substituted: CollectionInspection = core({
    action: "inspect_collection_run",
    job_id: b.id,
  });
  let response = substituted;
  const requests: Record<string, unknown>[] = [];
  await page.goto("/");
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (q.action === "queue_collection") {
      requests.push(q);
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(response),
      });
    } else await route.continue();
  });
  // Both response bodies are real canonical inspections of fixed synthetic rows.
  // This deliberately substitutes the envelope; it is not successful live queue evidence.
  await page.evaluate(
    async ({ key, urls, revision }) => {
      await (
        await import("/tests/durable-collection-harness.tsx")
      ).useCanonicalRequest(key, urls, revision);
    },
    { key: a.request_key, urls: a.input.urls, revision },
  );
  await expect
    .poll(async () =>
      page.evaluate(
        async () =>
          (await import("/tests/durable-collection-harness.tsx")).state().phase,
      ),
    )
    .toBe("uncertain");
  response = exact;
  await page.evaluate(
    async ({ revision }) => {
      (await import("/tests/durable-collection-harness.tsx")).mount(
        revision,
        [],
      );
    },
    { revision },
  );
  await page
    .getByRole("button", { name: "Retry same collection", exact: true })
    .click();
  await expect
    .poll(async () =>
      page.evaluate(
        async () =>
          (await import("/tests/durable-collection-harness.tsx")).state().phase,
      ),
    )
    .toBe("saved");
  expect(requests).toHaveLength(2);
  expect(requests[0]).toEqual(requests[1]);
  expect(requests[0].request_key).toBe(a.request_key);
  expect(core({ action: "view" }).workspace.revision).toBe(revision);
});

test("keyboard pagination restores range focus but respects a deliberate move followed by blur", async ({
  page,
}) => {
  await open(page);
  const nav = page.getByRole("navigation", {
    name: "Collection catalogue pages",
  });
  await nav.getByRole("button", { name: "Next", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(
    page.getByText("26–30 of 30 collections", { exact: true }),
  ).toBeFocused();
  await nav
    .getByRole("button", { name: "Back to previous collection page" })
    .focus();
  await page.keyboard.press("Enter");
  await expect(
    page.getByText("1–25 of 30 collections", { exact: true }),
  ).toBeFocused();
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let entered = false;
  await page.route("**/api/workbench", async (route) => {
    const q = route.request().postDataJSON();
    if (q.action === "page_collection_runs" && q.request.cursor && !entered) {
      const response = await route.fetch();
      entered = true;
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  await nav.getByRole("button", { name: "Next", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => entered).toBe(true);
  const seeds = page.getByLabel("Seed URLs", { exact: true });
  await seeds.focus();
  await seeds.evaluate((element) => element.blur());
  release();
  await expect(
    page.getByText("26–30 of 30 collections", { exact: true }),
  ).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.tagName))
    .toBe("BODY");
});
