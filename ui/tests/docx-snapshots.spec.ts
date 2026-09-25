import { test, expect, type Page } from "@playwright/test";
import { execFileSync, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { randomUUID, createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";

const root = resolve("artifacts/synthetic-ui-workspace");
const captures = resolve("artifacts/docx-ui");
const core = (command: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(command),
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    }),
  );
const view = () => core({ action: "view" }).workspace;
const addSource = (name = "later.txt") =>
  core({
    action: "import",
    name,
    bytes: [...Buffer.from(`Synthetic source ${name}`)],
  }).workspace;
const capture = () =>
  core({
    action: "save_docx_snapshot",
    request_id: randomUUID(),
    expected_revision: view().revision,
  });
function fixture(count = 0) {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  addSource("docx-synthetic.txt");
  return Array.from({ length: count }, capture);
}
const catalogue = () =>
  core({
    action: "page_docx_snapshots",
    expected_revision: view().revision,
    request: { page_size: 50, cursor: null },
  });
const panel = (page: Page) =>
  page.getByRole("region", { name: "Editable DOCX snapshots", exact: true });
async function recordDesignState(page: Page, name: string) {
  const output = resolve("output/playwright/docx-design");
  mkdirSync(output, { recursive: true });
  const result = await new AxeBuilder({ page })
    .include('[aria-label="Editable DOCX snapshots"]')
    .analyze();
  expect(result.violations).toEqual([]);
  writeFileSync(
    resolve(output, `${name}.accessibility.json`),
    JSON.stringify(
      {
        violations: result.violations.map((item) => item.id),
        manual_review_rules: result.incomplete.map((item) => item.id),
        scope: "Editable DOCX snapshots",
      },
      null,
      2,
    ),
  );
  await panel(page).screenshot({
    path: resolve(output, `${name}.png`),
    // The native path is real, verified and machine-specific. Public design
    // comparison retains the receipt status but masks this private local path.
    mask: [panel(page).locator("[data-native-export-location]")],
    maskColor: "#d8dcd8",
  });
}
async function navigate(page: Page, name: "Assessment" | "Overview") {
  await page
    .getByRole("navigation")
    .getByRole("button", { name: new RegExp(name) })
    .click();
}
async function open(page: Page) {
  await page.goto("/");
  await navigate(page, "Assessment");
  await expect(
    panel(page).getByRole("button", {
      name: "Refresh DOCX catalogue",
      exact: true,
    }),
  ).toBeEnabled();
}
function gate() {
  let release!: () => void;
  const wait = new Promise<void>((r) => {
    release = r;
  });
  return { wait, release };
}

test("DOCX catalogue distinguishes empty and failed reads, creates a canonical snapshot and records accessible compact metadata", async ({
  page,
}) => {
  fixture();
  await open(page);
  await expect(panel(page)).toContainText("No DOCX snapshots recorded.");
  await recordDesignState(page, "empty-desktop");
  const calls: string[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/api/workbench"))
      calls.push(request.postDataJSON().action);
  });
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Snapshot captured:");
  await expect(panel(page)).toContainText("1–1 of 1 DOCX snapshots");
  const row = catalogue().rows[0];
  await expect(panel(page).locator("[data-docx-id]")).toHaveAttribute(
    "data-docx-id",
    row.id,
  );
  await expect(panel(page)).toContainText(row.document.sha256);
  await expect(panel(page)).toContainText(row.docx.sha256);
  await expect(panel(page)).toContainText(
    "Open the native desktop application to save this DOCX file.",
  );
  expect(
    calls.filter((action) => action === "save_docx_snapshot"),
  ).toHaveLength(1);
  expect(calls).not.toContain("inspect_docx_snapshot");
  await recordDesignState(page, "catalogue-desktop");
  await page.screenshot({
    path: resolve(captures, "catalogue-desktop.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 960, height: 1000 });
  await panel(page).scrollIntoViewIfNeeded();
  await recordDesignState(page, "catalogue-compact");
  await page.screenshot({
    path: resolve(captures, "catalogue-compact.png"),
    fullPage: true,
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  const axe = await new AxeBuilder({ page })
    .include('[aria-label="Editable DOCX snapshots"]')
    .analyze();
  writeFileSync(
    resolve(captures, "accessibility.json"),
    JSON.stringify(axe, null, 2),
  );
  expect(axe.violations).toEqual([]);
  let fail = true;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action === "page_docx_snapshots" && fail)
      await route.fulfill({
        status: 503,
        json: { error: "Synthetic unavailable catalogue" },
      });
    else await route.continue();
  });
  await panel(page)
    .getByRole("button", { name: "Refresh DOCX catalogue", exact: true })
    .click();
  await expect(panel(page).getByRole("alert")).toContainText(
    "Synthetic unavailable catalogue",
  );
  await expect(panel(page)).not.toContainText("No DOCX snapshots recorded.");
  await recordDesignState(page, "unavailable-compact");
  fail = false;
  await panel(page)
    .getByRole("button", { name: "Retry DOCX catalogue read", exact: true })
    .click();
  await expect(panel(page)).toContainText("1–1 of 1 DOCX snapshots");
});

test("bounded catalogue pages retain exact canonical order without omissions and avoid focus theft after a deliberate move and blur", async ({
  page,
}) => {
  const records = fixture(43).reverse();
  const requests: any[] = [];
  const held = gate(),
    received = gate();
  let hold = false;
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (input.action !== "page_docx_snapshots") return route.continue();
    requests.push(input);
    const response = await route.fetch();
    if (hold) {
      received.release();
      await held.wait;
    }
    await route.fulfill({ response });
  });
  await open(page);
  await expect(panel(page)).toContainText("1–20 of 43 DOCX snapshots");
  const firstCard = await panel(page)
    .locator("[data-docx-id]")
    .first()
    .boundingBox();
  const controls = await panel(page)
    .getByRole("group", { name: "DOCX catalogue pages" })
    .boundingBox();
  const metadata = await panel(page)
    .getByText("Retained metadata catalogue", { exact: true })
    .boundingBox();
  expect(firstCard).not.toBeNull();
  expect(controls).not.toBeNull();
  expect(metadata).not.toBeNull();
  expect(controls!.y + controls!.height).toBeLessThan(firstCard!.y);
  expect(metadata!.y + metadata!.height).toBeLessThan(firstCard!.y);
  const ids = () =>
    panel(page)
      .locator("[data-docx-id]")
      .evaluateAll((elements) =>
        elements.map((el) => el.getAttribute("data-docx-id")),
      );
  const seen = await ids();
  await panel(page).getByRole("button", { name: "Next DOCX page" }).focus();
  await page.keyboard.press("Enter");
  await expect(panel(page)).toContainText("21–40 of 43 DOCX snapshots");
  await expect(
    panel(page).getByRole("status").filter({ hasText: "21–40 of 43" }),
  ).toBeFocused();
  seen.push(...(await ids()));
  hold = true;
  await panel(page).getByRole("button", { name: "Next DOCX page" }).click();
  await received.wait;
  const target = panel(page).getByRole("button", {
    name: "Capture DOCX snapshot",
    exact: true,
  });
  await target.focus();
  await target.evaluate((element) => (element as HTMLElement).blur());
  held.release();
  await expect(panel(page)).toContainText("41–43 of 43 DOCX snapshots");
  expect(
    await page.evaluate(() => document.activeElement === document.body),
  ).toBe(true);
  seen.push(...(await ids()));
  expect(seen).toEqual(records.map((row) => row.id));
  expect(new Set(seen).size).toBe(43);
  await panel(page)
    .getByRole("button", { name: "Back to previous DOCX page" })
    .click();
  await expect(panel(page)).toContainText("21–40 of 43 DOCX snapshots");
  await panel(page).getByRole("button", { name: "First DOCX page" }).click();
  await expect(panel(page)).toContainText("1–20 of 43 DOCX snapshots");
  expect(requests.every((request) => request.request.page_size === 20)).toBe(
    true,
  );
});

test("lost creation acknowledgement survives navigation and retries only the same UUID and captured revision after a later change", async ({
  page,
}) => {
  fixture();
  const requests: any[] = [];
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (input.action !== "save_docx_snapshot") return route.continue();
    requests.push(input);
    const response = await route.fetch();
    if (requests.length === 1)
      await route.fulfill({
        status: 503,
        json: { error: "Synthetic lost capture acknowledgement" },
      });
    else await route.fulfill({ response });
  });
  await open(page);
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  await expect(panel(page)).toContainText(
    "Workspace refresh does not create a new capture.",
  );
  await expect(panel(page)).toContainText(
    "only while this application remains open",
  );
  await recordDesignState(page, "uncertain-desktop");
  expect(catalogue().total_count).toBe(1);
  await navigate(page, "Overview");
  const changed = addSource();
  await navigate(page, "Assessment");
  await panel(page)
    .getByRole("button", { name: "Refresh DOCX catalogue", exact: true })
    .click();
  await expect(panel(page)).toContainText(
    `catalogue revision ${changed.revision}`,
  );
  await panel(page)
    .getByRole("button", { name: "Retry same DOCX capture" })
    .click();
  await expect(panel(page)).toContainText("Snapshot captured:");
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
  expect(catalogue().total_count).toBe(1);
  expect(view().revision).toBe(changed.revision);
});

test("creation completes after unmount and failed refresh remains a saved snapshot, not a repeated write", async ({
  page,
}) => {
  fixture();
  const held = gate(),
    received = gate();
  let rejectRefresh = false,
    writes = 0;
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (input.action === "view" && rejectRefresh)
      return route.fulfill({
        status: 503,
        json: { error: "Synthetic refresh failure" },
      });
    if (input.action !== "save_docx_snapshot") return route.continue();
    writes++;
    const response = await route.fetch();
    received.release();
    await held.wait;
    rejectRefresh = true;
    await route.fulfill({ response });
  });
  await open(page);
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await received.wait;
  await navigate(page, "Overview");
  held.release();
  await expect(
    page.getByRole("alert").filter({ hasText: "Synthetic refresh failure" }),
  ).toBeVisible();
  await navigate(page, "Assessment");
  await expect(panel(page)).toContainText(
    "Workspace refresh failed; the snapshot is saved.",
  );
  await recordDesignState(page, "saved-refresh-failed-desktop");
  expect(writes).toBe(1);
  rejectRefresh = false;
  await panel(page)
    .getByRole("button", { name: "Refresh DOCX catalogue", exact: true })
    .click();
  await expect(panel(page)).toContainText("1–1 of 1 DOCX snapshots");
});

test("obsolete catalogue responses cannot reappear across revision refresh and A to B to A navigation", async ({
  page,
}) => {
  fixture(1);
  const held = gate(),
    received = gate();
  let reads = 0;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "page_docx_snapshots")
      return route.continue();
    const index = ++reads,
      response = await route.fetch();
    if (index === 1) {
      received.release();
      await held.wait;
    }
    await route.fulfill({ response });
  });
  await open(page);
  await received.wait;
  capture();
  await panel(page)
    .getByRole("button", { name: "Refresh DOCX catalogue", exact: true })
    .click();
  await expect(panel(page)).toContainText(
    `catalogue revision ${view().revision}`,
  );
  await navigate(page, "Overview");
  await navigate(page, "Assessment");
  expect(reads).toBe(1);
  held.release();
  await expect(panel(page)).toContainText("1–2 of 2 DOCX snapshots");
  expect(reads).toBe(2);
  expect(await panel(page).locator("[data-docx-id]").count()).toBe(2);
});

test("stale creation and corrupt catalogue metadata are explicit failures", async ({
  page,
}) => {
  fixture(1);
  await open(page);
  await expect(panel(page)).toContainText("1–1 of 1 DOCX snapshots");
  addSource();
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  await expect(panel(page)).toContainText(
    "retry cannot substitute the current revision",
  );
  expect(catalogue().total_count).toBe(1);
  execFileSync("python3", [
    "-c",
    "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute(\"update records set body='{}' where kind='docx_snapshot'\"); c.commit()",
    resolve(root, "workspace.db"),
  ]);
  await panel(page)
    .getByRole("button", { name: "Refresh DOCX catalogue", exact: true })
    .click();
  await expect(panel(page)).toContainText("Catalogue unavailable:");
  await expect(panel(page)).not.toContainText("No DOCX snapshots recorded.");
});

test("typed later-revision absence permits only an explicit new capture and retains the acknowledged request", async ({
  page,
}) => {
  fixture();
  const requests: any[] = [];
  page.on("request", (request) => {
    if (
      request.url().endsWith("/api/workbench") &&
      request.postDataJSON().action === "save_docx_snapshot"
    )
      requests.push(request.postDataJSON());
  });
  await open(page);
  const newer = addSource();
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  await panel(page)
    .getByRole("button", { name: "Check DOCX capture outcome" })
    .click();
  await expect(panel(page)).toContainText(
    `No snapshot is recorded for this request at workspace revision ${newer.revision}`,
  );
  await expect(
    panel(page).getByRole("button", { name: "Start a new DOCX snapshot" }),
  ).toBeEnabled();
  expect(requests).toHaveLength(1);
  expect(catalogue().total_count).toBe(0);
  await recordDesignState(page, "later-revision-absence-desktop");
  await panel(page)
    .getByRole("button", { name: "Start a new DOCX snapshot" })
    .click();
  await expect(panel(page)).toContainText("Snapshot captured:");
  expect(requests).toHaveLength(2);
  expect(requests[1].request_id).not.toBe(requests[0].request_id);
  expect(requests[1].expected_revision).toBe(newer.revision);
  expect(catalogue().total_count).toBe(1);
  await panel(page)
    .getByText(/Recent acknowledged capture outcomes/)
    .click();
  await expect(panel(page)).toContainText(requests[0].request_id);
  await expect(panel(page)).toContainText(
    "A new capture was explicitly requested.",
  );
});

test("typed absence at the same revision never enables a replacement and retries the original request", async ({
  page,
}) => {
  fixture();
  const requests: any[] = [];
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (input.action !== "save_docx_snapshot") return route.continue();
    requests.push(input);
    if (requests.length === 1)
      return route.fulfill({
        status: 503,
        json: { error: "Synthetic request delivery failure" },
      });
    await route.continue();
  });
  await open(page);
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  await panel(page)
    .getByRole("button", { name: "Check DOCX capture outcome" })
    .click();
  await expect(panel(page)).toContainText(
    "A previously sent capture may still publish at this revision.",
  );
  await expect(
    panel(page).getByRole("button", { name: "Start a new DOCX snapshot" }),
  ).toHaveCount(0);
  await recordDesignState(page, "same-revision-absence-desktop");
  await panel(page)
    .getByRole("button", { name: "Retry same DOCX capture" })
    .click();
  await expect(panel(page)).toContainText("Snapshot captured:");
  expect(requests).toHaveLength(2);
  expect(requests[0]).toEqual(requests[1]);
  expect(catalogue().total_count).toBe(1);
});

test("verified outcome lookup recovers a saved acknowledgement through navigation without another capture", async ({
  page,
}) => {
  fixture();
  const held = gate(),
    received = gate();
  let writes = 0;
  await page.route("**/api/workbench", async (route) => {
    const input = route.request().postDataJSON();
    if (input.action === "save_docx_snapshot") {
      writes++;
      await route.fetch();
      return route.fulfill({
        status: 503,
        json: { error: "Synthetic lost capture acknowledgement" },
      });
    }
    if (input.action !== "resolve_docx_capture") return route.continue();
    const response = await route.fetch();
    received.release();
    await held.wait;
    await route.fulfill({ response });
  });
  await open(page);
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  const saved = catalogue().rows[0],
    revision = view().revision;
  await panel(page)
    .getByRole("button", { name: "Check DOCX capture outcome" })
    .click();
  await received.wait;
  await navigate(page, "Overview");
  held.release();
  await navigate(page, "Assessment");
  await expect(panel(page)).toContainText(`Snapshot captured: ${saved.id}`);
  expect(writes).toBe(1);
  expect(view().revision).toBe(revision);
});

test("corrupt saved artifact blocks typed recovery and never becomes permission for a replacement", async ({
  page,
}) => {
  fixture();
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "save_docx_snapshot")
      return route.continue();
    await route.fetch();
    await route.fulfill({
      status: 503,
      json: { error: "Synthetic lost capture acknowledgement" },
    });
  });
  await open(page);
  await panel(page)
    .getByRole("button", { name: "Capture DOCX snapshot", exact: true })
    .click();
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  const saved = catalogue().rows[0];
  const path = resolve(root, "derivatives/objects", saved.document.sha256);
  chmodSync(path, 0o600);
  writeFileSync(path, "Synthetic corrupt frozen document");
  await panel(page)
    .getByRole("button", { name: "Check DOCX capture outcome" })
    .click();
  await expect(panel(page).getByRole("alert")).not.toContainText(
    "Synthetic lost capture acknowledgement",
  );
  await expect(panel(page)).toContainText("Capture completion is unconfirmed:");
  await expect(
    panel(page).getByRole("button", { name: "Start a new DOCX snapshot" }),
  ).toHaveCount(0);
  expect(catalogue().total_count).toBe(1);
});

type Call = { command: string; args: Record<string, any> };
async function nativeBridge(
  page: Page,
  intercept?: (
    call: Call,
    value: any,
  ) => Promise<{ status?: number; value?: any } | void>,
) {
  const process = spawn(
    resolve("target/debug/examples/native_export_session"),
    [root],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  const pending: Array<{
    resolve: (value: any) => void;
    reject: (error: Error) => void;
  }> = [];
  const lines = createInterface({ input: process.stdout });
  lines.on("line", (line) => {
    const item = pending.shift();
    if (!item) return;
    const value = JSON.parse(line);
    if (value.error) item.reject(new Error(value.error));
    else item.resolve(value.ok);
  });
  let diagnostics = "";
  process.stderr.on("data", (value) => (diagnostics += value));
  process.on("exit", () => {
    for (const item of pending.splice(0))
      item.reject(new Error(`Synthetic native session exited: ${diagnostics}`));
  });
  const send = (value: Record<string, unknown>) =>
    new Promise<any>((resolve, reject) => {
      pending.push({ resolve, reject });
      process.stdin.write(JSON.stringify(value) + "\n");
    });
  const calls: Call[] = [];
  await page.route("**/docx-native-test", async (route) => {
    const call: Call = route.request().postDataJSON();
    calls.push(call);
    const args = call.args;
    const input =
      call.command === "commit_native_export"
        ? {
            method: call.command,
            ticket: args.ticket,
            expected_sha256: args.expectedSha256,
            expected_bytes: args.expectedBytes,
          }
        : { method: call.command, ...args };
    try {
      const value = await send(input),
        changed = await intercept?.(call, value);
      await route.fulfill({
        status: changed?.status ?? 200,
        json: changed?.value ?? value,
      });
    } catch (error) {
      await route.fulfill({ status: 400, json: { error: String(error) } });
    }
  });
  await page.addInitScript(() => {
    (window as any).isTauri = true;
    (window as any).__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: unknown) => {
        const response = await fetch("/docx-native-test", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ command, args }),
        });
        const value = await response.json();
        if (!response.ok) throw new Error(value.error);
        return value;
      },
    };
  });
  return {
    calls,
    send,
    close: async () => {
      await page.close();
      await send({ method: "shutdown" });
      process.stdin.end();
      await new Promise<void>((resolve) =>
        process.once("exit", () => resolve()),
      );
      lines.close();
    },
  };
}
const staging = () => readdirSync(resolve(root, "native-export-staging"));

test("typed native DOCX save recovers lost commit acknowledgement and preserves old frozen bytes after current changes", async ({
  page,
}) => {
  fixture();
  // The canonical API accepts canonical UUIDs of any version, not only the
  // v4 identifiers minted by this UI. Existing valid records remain readable.
  const row = core({
    action: "save_docx_snapshot",
    request_id: "123e4567-e89b-12d3-a456-426614174000",
    expected_revision: view().revision,
  });
  const original = readFileSync(
    resolve(root, "derivatives/objects", row.docx.sha256),
  );
  addSource();
  let commits = 0;
  const bridge = await nativeBridge(page, async (call) =>
    call.command === "commit_native_export" && ++commits === 1
      ? {
          status: 503,
          value: { error: "Synthetic lost commit acknowledgement" },
        }
      : undefined,
  );
  const downloads: string[] = [];
  page.on("download", (file) => downloads.push(file.suggestedFilename()));
  try {
    await open(page);
    await panel(page).getByRole("button", { name: "Save DOCX file" }).click();
    await expect(panel(page)).toContainText("Saved immutable DOCX:");
    const filename = `assessment-${row.id}-${row.docx.sha256}.docx`;
    const bytes = readFileSync(resolve(root, "exports", filename));
    expect(bytes.equals(original)).toBe(true);
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(
      row.docx.sha256,
    );
    expect(commits).toBe(2);
    expect(staging()).toEqual([]);
    expect(downloads).toEqual([]);
    expect(
      bridge.calls.find((call) => call.command === "prepare_native_export")!
        .args.request,
    ).toEqual({
      kind: "docx_report",
      report_id: row.id,
      expected_document_sha256: row.document.sha256,
      expected_docx_sha256: row.docx.sha256,
    });
    await panel(page).scrollIntoViewIfNeeded();
    await recordDesignState(page, "native-receipt-desktop");
    await page.screenshot({
      path: resolve(captures, "native-save-receipt.png"),
      fullPage: true,
    });
  } finally {
    await bridge.close();
  }
});

test("late DOCX native preparation is discarded after navigation with no commit", async ({
  page,
}) => {
  fixture(1);
  const held = gate(),
    received = gate();
  const bridge = await nativeBridge(page, async (call) => {
    if (call.command === "prepare_native_export") {
      received.release();
      await held.wait;
    }
  });
  try {
    await open(page);
    await panel(page).getByRole("button", { name: "Save DOCX file" }).click();
    await received.wait;
    expect(staging()).toHaveLength(1);
    await navigate(page, "Overview");
    held.release();
    await expect.poll(staging).toEqual([]);
    expect(
      bridge.calls.some((call) => call.command === "discard_native_export"),
    ).toBe(true);
    expect(
      bridge.calls.some((call) => call.command === "commit_native_export"),
    ).toBe(false);
  } finally {
    held.release();
    await bridge.close();
  }
});

test("native DOCX rejects substituted preparation identity and actual corrupted frozen artifacts", async ({
  page,
}) => {
  const [row] = fixture(1);
  let substitute = true;
  const bridge = await nativeBridge(page, async (call, value) =>
    call.command === "prepare_native_export" && substitute
      ? {
          value: {
            ...value,
            artifact: { ...value.artifact, document_sha256: "0".repeat(64) },
          },
        }
      : undefined,
  );
  try {
    await open(page);
    await panel(page).getByRole("button", { name: "Save DOCX file" }).click();
    await expect(panel(page).getByRole("alert")).toContainText(
      "identity did not match",
    );
    await expect.poll(staging).toEqual([]);
    expect(
      bridge.calls.some((call) => call.command === "commit_native_export"),
    ).toBe(false);
    substitute = false;
    const path = resolve(root, "derivatives/objects", row.docx.sha256);
    chmodSync(path, 0o600);
    writeFileSync(path, "Corrupted synthetic artifact");
    await panel(page).getByRole("button", { name: "Save DOCX file" }).click();
    await expect(panel(page).getByRole("alert")).not.toContainText(
      "identity did not match",
    );
    await expect(panel(page).getByRole("alert")).toBeVisible();
    expect(
      bridge.calls.some((call) => call.command === "commit_native_export"),
    ).toBe(false);
  } finally {
    await bridge.close();
  }
});
