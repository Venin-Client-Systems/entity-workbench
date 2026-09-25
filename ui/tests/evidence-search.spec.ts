import { test, expect, type Page, type Route } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Evidence } from "../src/types";

// Canonical evidence/revision comes from Rust. ONLY search replies below are
// injected boundary fixtures; this suite does not execute or prove Lucene.
const root = resolve("artifacts/synthetic-ui-workspace");
const captures = resolve("artifacts/evidence-search-state");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let evidence: Evidence[];
let revision: number;
let external: string[];
let requests: { query: string; route: Route }[];
function reply(ids = evidence.map((e) => e.id).reverse(), observed = revision) {
  return {
    workspace_revision: String(observed),
    total: ids.length,
    hits: ids.map((id, i) => ({
      id,
      name: evidence.find((e) => e.id === id)!.name,
      score: 10 - i,
    })),
  };
}
const rows = (page: Page) => page.locator(".evidence-row strong");
const search = (page: Page) =>
  page.getByRole("button", { name: "Search local index", exact: true });
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  await expect(rows(page)).toHaveText(evidence.map((e) => e.name));
}
async function attempt(page: Page, query = "text:rank") {
  await page.getByRole("textbox", { name: "Search evidence" }).fill(query);
  await search(page).click();
}
async function answer(index: number, value: unknown = reply(), status = 200) {
  await expect.poll(() => requests.length).toBeGreaterThan(index);
  await requests[index].route.fulfill({
    status,
    contentType: "application/json",
    body: JSON.stringify(value),
  });
}
test.beforeEach(async ({ page }) => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  for (const [i, name] of [
    "First source.txt",
    "Second source.txt",
    "Third source.txt",
  ].entries()) {
    const result = core({
      action: "import",
      name,
      bytes: [...Buffer.from(`Synthetic source ${i}. Plain local text.`)],
    });
    evidence = result.workspace.evidence;
    revision = result.workspace.revision;
  }
  external = [];
  requests = [];
  page.on("request", (r) => {
    if (
      !r.url().startsWith("http://127.0.0.1:1420") &&
      !r.url().startsWith("blob:") &&
      !r.url().startsWith("data:")
    )
      external.push(r.url());
  });
  await page.route("**/api/workbench", async (route) => {
    const request = route.request().postDataJSON();
    if (request.action === "search")
      requests.push({ query: request.query, route });
    else await route.continue();
  });
});
test.afterEach(async () => {
  expect(external).toEqual([]);
});

test("injected index rank is preserved against real canonical order; wide/compact existing surface", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await expect(rows(page)).toHaveCount(0);
  await answer(0);
  await expect(rows(page)).toHaveText(evidence.map((e) => e.name).reverse());
  expect(requests[0].query).toBe("text:rank");
  await expect(page.getByText("3 source items", { exact: true })).toBeVisible();
  for (const [name, width, height] of [
    ["rank-wide", 1440, 1000],
    ["rank-compact", 960, 720],
  ] as const) {
    await page.setViewportSize({ width, height });
    const audit = await new AxeBuilder({ page }).analyze();
    expect(audit.violations).toEqual([]);
    writeFileSync(
      resolve(captures, name + "-axe.json"),
      JSON.stringify(
        {
          violations: audit.violations,
          incomplete: audit.incomplete.map((r) => r.id),
        },
        null,
        2,
      ),
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: resolve(captures, name + ".png"),
      fullPage: true,
    });
  }
  await page.locator(".evidence-row").first().click();
  await expect(
    page.getByRole("dialog", { name: "Evidence source" }),
  ).toContainText(evidence.at(-1)!.sha256);
  expect(core({ action: "view" }).workspace.revision).toBe(revision);
});

test("query A to B to A clears ready hits and rejects a late same-query generation", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await page
    .getByRole("textbox", { name: "Search evidence" })
    .fill("other-query");
  await page
    .getByRole("textbox", { name: "Search evidence" })
    .fill("text:rank");
  await answer(0);
  await expect(search(page)).toBeEnabled();
  await expect(rows(page)).toHaveCount(0);
  await search(page).click();
  await answer(1);
  await expect(rows(page)).toHaveCount(3);
  await page
    .getByRole("textbox", { name: "Search evidence" })
    .fill("absent-new-query");
  await expect(rows(page)).toHaveCount(0);
});

test("navigation away and back drops old replies and old errors without changing shared busy", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await expect.poll(() => requests.length).toBe(1);
  await attempt(page, "queued-on-old-section");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Evidence/ })
    .click();
  await page
    .getByRole("textbox", { name: "Search evidence" })
    .fill("text:rank");
  await answer(0, { error: "obsolete index failure" }, 400);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(search(page)).toBeEnabled();
  await expect(rows(page)).toHaveCount(0);
  await attempt(page, "text:current");
  await answer(1);
  await expect(rows(page)).toHaveCount(3);
  expect(requests.map((request) => request.query)).toEqual([
    "text:rank",
    "text:current",
  ]);
});

test("one active request coalesces pending queries; obsolete completion cannot clear newer pending or failure", async ({
  page,
}) => {
  await open(page);
  await attempt(page, "first-held");
  await expect.poll(() => requests.length).toBe(1);
  await attempt(page, "second-never-sent");
  await attempt(page, "third-latest");
  await expect(search(page)).toBeDisabled();
  expect(requests.map((r) => r.query)).toEqual(["first-held"]);
  await answer(0);
  await expect
    .poll(() => requests.map((r) => r.query))
    .toEqual(["first-held", "third-latest"]);
  await expect(search(page)).toBeDisabled();
  await expect(rows(page)).toHaveCount(0);
  await answer(1, { error: "current injected index failure" }, 400);
  await expect(page.getByRole("alert")).toContainText(
    "current injected index failure",
  );
  await expect(search(page)).toBeEnabled();
  await expect(rows(page)).toHaveCount(0);
});

test("new attempt and failure clear ready hits; empty success stays empty", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await answer(0);
  await expect(rows(page)).toHaveCount(3);
  await search(page).click();
  await expect(rows(page)).toHaveCount(0);
  await answer(1, { error: "explicit injected refusal" }, 400);
  await expect(page.getByRole("alert")).toContainText(
    "explicit injected refusal",
  );
  await expect(rows(page)).toHaveCount(0);
  await search(page).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await answer(2, reply([]));
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByText("0 source items", { exact: true })).toBeVisible();
});

test("real import revision invalidates a held reply without blocking mutation controls", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await expect.poll(() => requests.length).toBe(1);
  await expect(
    page.getByRole("button", { name: /Import evidence/ }),
  ).toBeEnabled();
  await page
    .locator('input[type="file"]')
    .first()
    .setInputFiles({
      name: "Fourth source.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("Synthetic later revision."),
    });
  await expect(page.locator(".revision")).toHaveText(`REV ${revision + 1}`);
  await answer(0);
  await expect(rows(page)).toHaveCount(0);
  await expect(search(page)).toBeEnabled();
  await search(page).click();
  await answer(1); // The same stale revision is also refused on a current request.
  await expect(page.getByRole("alert")).toContainText(
    "does not match the current evidence revision",
  );
  await expect(rows(page)).toHaveCount(0);
});

test("malformed injected replies never show a partial valid prefix", async ({
  page,
}) => {
  await open(page);
  const good = reply();
  const malformed: unknown[] = [
    { hits: good.hits },
    { ...good, workspace_revision: revision },
    { ...good, total: -1 },
    { ...good, total: 0 },
    { ...good, total: Number.MAX_SAFE_INTEGER + 1 },
    { ...good, extra: true },
    { ...good, hits: [good.hits[0], good.hits[0]] },
    { ...good, hits: [good.hits[0], { ...good.hits[1], id: "unknown-id" }] },
    { ...good, hits: [good.hits[0], { ...good.hits[1], score: null }] },
    {
      ...good,
      hits: [good.hits[0], { ...good.hits[1], name: "substituted name" }],
    },
    { ...good, hits: [good.hits[0], { ...good.hits[1], score: "1" }] },
    {
      ...good,
      total: 101,
      hits: Array.from({ length: 101 }, () => good.hits[0]),
    },
  ];
  for (const [i, value] of malformed.entries()) {
    await attempt(page, `invalid-${i}`);
    await answer(i, value);
    await expect(page.getByRole("alert")).toContainText(
      "Local index response is invalid",
    );
    await expect(rows(page)).toHaveCount(0);
    await expect(search(page)).toBeEnabled();
  }
});

test("obsolete search completion cannot clear a real import's pending mutation state", async ({
  page,
}) => {
  await open(page);
  await attempt(page);
  await expect.poll(() => requests.length).toBe(1);
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let imported = false;
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "import") {
      await route.fallback();
      return;
    }
    const response = await route.fetch();
    imported = true;
    await held;
    await route.fulfill({ response });
  });
  await page
    .locator('input[type="file"]')
    .first()
    .setInputFiles({
      name: "Held import.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("Synthetic mutation pending reply."),
    });
  await expect.poll(() => imported).toBe(true);
  await page
    .getByRole("textbox", { name: "Search evidence" })
    .fill("new-query");
  await answer(0);
  await expect(
    page.getByRole("button", { name: /Import evidence/ }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Back up", exact: true }),
  ).toBeDisabled();
  await expect(rows(page)).toHaveCount(0);
  release();
  await expect(page.locator(".revision")).toHaveText(`REV ${revision + 1}`);
  await expect(
    page.getByRole("button", { name: /Import evidence/ }),
  ).toBeEnabled();
});
