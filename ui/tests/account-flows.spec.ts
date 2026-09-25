import { test, expect, type Page, type Locator } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import type { Workspace } from "../src/types";
import type { AccountFlowResult } from "../src/account-flow-types";
const root = resolve("artifacts/synthetic-ui-workspace"),
  captures = resolve("artifacts/account-flows");
const core = (command: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(command),
      encoding: "utf8",
    }),
  );
let workspace: Workspace;
const csv = `account,date,description,amount,currency
0001,2024-01-01,Synthetic income,100.00,AUD
0001,2024-01-31,Synthetic repeated transfer,-20.00,AUD
0001,2024-01-31,Synthetic repeated transfer,-20.00,AUD
0001,2024-01-15,<img src=x onerror=window.flowExecuted=true>,-5.00,AUD
0001,2024-01-15,Pending activity,-7.00,AUD
0001,2024-01-15,Deferred activity,-3.00,AUD
0002,2024-02-01,Synthetic credit,20.00,AUD
0002,2024-02-01,Synthetic repeated credit,20.00,AUD
0001,2024-01-15,Separate currency exact decimal,-0.10000001,USD
0003,2024-01-15,Rejected activity,-1.00,AUD
`;
function seed(
  text = csv,
  pairs: [number, number][] = [
    [1, 6],
    [2, 7],
  ],
) {
  workspace = core({
    action: "import",
    name: "synthetic-account-flows.csv",
    bytes: [...Buffer.from(text)],
  }).workspace;
  const rows = workspace.transactions;
  for (let i = 0; i < rows.length; i++) {
    if (text === csv && i === 4) continue;
    workspace = core({
      action: "review_transaction",
      id: rows[i].id,
      state:
        text === csv && i === 5
          ? "deferred"
          : text === csv && i === 9
            ? "rejected"
            : "accepted",
      reason: "Synthetic account flow review.",
      expected_revision: workspace.revision,
    }).workspace;
  }
  for (const [a, b] of pairs)
    workspace = core({
      action: "match_transfer",
      first: rows[a].id,
      second: rows[b].id,
      reason: "Synthetic reciprocal source pair.",
      expected_revision: workspace.revision,
    }).workspace;
}
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  seed();
});
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  return page.getByRole("region", {
    name: "Reviewed account flows",
    exact: true,
  });
}
async function calculate(panel: Locator) {
  await panel
    .getByRole("button", { name: "Calculate reviewed flows", exact: true })
    .click();
  await expect(panel.getByLabel("Applied flow scope")).toBeVisible();
  await expect(
    panel.getByRole("button", {
      name: "Calculate reviewed flows",
      exact: true,
    }),
  ).toBeEnabled();
}
async function selectScope(panel: Locator) {
  await panel.getByLabel("Flow from", { exact: true }).fill("2024-01-01");
  await panel.getByLabel("Flow through", { exact: true }).fill("2024-01-31");
  await panel.getByLabel("Flow account", { exact: true }).selectOption("0001");
  await panel.getByLabel("Flow currency", { exact: true }).selectOption("AUD");
}
async function axe(page: Page, scope: string, name: string) {
  const result = await new AxeBuilder({ page })
    .include(scope)
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  writeFileSync(
    resolve(captures, `axe-${name}.json`),
    JSON.stringify({ scope, violations: result.violations }, null, 2),
  );
  expect(result.violations).toEqual([]);
}
async function capture(page: Page, panel: Locator, name: string) {
  const previous = page.viewportSize()!;
  // Keep the full instrument clear of the sticky app bar in the screenshot only.
  await page.setViewportSize({ width: previous.width, height: 2700 });
  await panel.scrollIntoViewIfNeeded();
  await panel.screenshot({ path: resolve(captures, name) });
  await page.setViewportSize(previous);
}
test("once-per-pair exact flows retain currencies, repeated rows and every review denominator without writes", async ({
  page,
  baseURL,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("request", (r) => {
    if (!r.url().startsWith(baseURL!) && !/^(blob:|data:)/.test(r.url()))
      external.push(r.url());
  });
  page.on("pageerror", (e) => errors.push(e.message));
  const panel = await open(page);
  await calculate(panel);
  const expected: AccountFlowResult = core({
    action: "analyze_account_flows",
    request: { date_from: null, date_to: null, account: null, currency: null },
    expected_revision: workspace.revision,
  });
  expect(expected.edges).toHaveLength(1);
  expect(expected.edges[0].pairs).toHaveLength(2);
  const edge = panel.getByRole("article", { name: "Flow 0001 to 0002 AUD" });
  await expect(edge).toContainText(`${expected.edges[0].amount} AUD`);
  await expect(edge).toContainText("Debit to credit · 2 pairs");
  const aud = panel.getByRole("region", {
    name: "Account activity 0001 / AUD",
  });
  await expect(aud).toContainText("6 selected rows", { ignoreCase: true });
  await expect(aud).toContainText(
    "4 accepted / 1 pending / 0 rejected / 1 deferred",
  );
  await expect(aud).toContainText("2 possible duplicate rows");
  await expect(
    panel.getByRole("region", { name: "Account activity 0001 / USD" }),
  ).toContainText("-0.10000001");
  await expect(
    panel.getByRole("region", { name: "Account activity 0003 / AUD" }),
  ).toContainText("No accepted selected rows");
  await aud
    .getByRole("button", { name: "Accepted ledger (4)", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Account flow sources" });
  await expect(dialog.locator(".patterns-source")).toHaveCount(4);
  await expect(
    dialog.getByText("<img src=x onerror=window.flowExecuted=true>", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.locator("img,script,iframe")).toHaveCount(0);
  await dialog
    .getByRole("button", {
      name: "Inspect source and review row 5",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("button", { name: "Close review", exact: true }),
  ).toBeFocused();
  await expect(
    page.getByText("Preserved source value", { exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as unknown as Record<string, unknown>).flowExecuted,
    ),
  ).toBeUndefined();
  expect(core({ action: "view" }).workspace.revision).toBe(workspace.revision);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});
test("asymmetric scope shows support-only endpoints and exact source provenance with accessible modal handoff", async ({
  page,
}) => {
  const panel = await open(page);
  await selectScope(panel);
  await calculate(panel);
  const counts = panel.getByLabel("Flow scope denominators");
  await expect(counts).toContainText("Selected rows6");
  await expect(counts).toContainText("Support-only rows2");
  const support = panel.getByRole("region", {
    name: "Account activity 0002 / AUD",
  });
  await expect(support).toContainText("SUPPORT ONLY / OUTSIDE SCOPE");
  await expect(support.locator(".flow-money")).toHaveCount(0);
  await axe(page, ".account-flows", "wide");
  await capture(page, panel, "app-wide.png");
  const opener = panel.getByRole("button", {
    name: "Review pairs",
    exact: true,
  });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Account flow sources" });
  await expect(dialog).toContainText("1–2 of 2 pairs shown");
  await expect(dialog.locator(".flow-pair").first()).toContainText(
    "Credit 2024-02-01 — support only, outside scope",
  );
  await axe(page, 'dialog[aria-label="Account flow sources"]', "pairs");
  await dialog
    .getByRole("button", { name: "Inspect both source rows" })
    .first()
    .click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(2);
  await expect(dialog.locator(".flow-source-context").first()).toContainText(
    "Selected activity",
  );
  await expect(dialog.locator(".flow-source-context").last()).toContainText(
    "Support only",
  );
  await expect(dialog.locator(".patterns-source").first()).toContainText(
    "-20.00 AUD",
  );
  await expect(dialog.locator(".patterns-source").last()).toContainText(
    "20.00 AUD",
  );
  await dialog.screenshot({ path: resolve(captures, "app-sources.png") });
  await page.keyboard.press("Escape");
  await expect(opener).toBeFocused();
});
test("draft, empty and pending-only scopes remain distinct from unavailable and stale results", async ({
  page,
}) => {
  const panel = await open(page);
  await selectScope(panel);
  await calculate(panel);
  await panel.getByLabel("Flow from", { exact: true }).fill("2024-01-15");
  await panel.getByLabel("Flow through", { exact: true }).fill("2024-01-15");
  await expect(panel.getByText(/Scope edits are not applied/)).toBeVisible();
  await expect(panel.getByLabel("Applied flow scope")).toContainText(
    "2024-01-01",
  );
  await calculate(panel);
  await expect(
    panel.getByText(/No verified internal transfer pairs/),
  ).toBeVisible();
  // Canonical correction makes every selected row pending; no fabricated result.
  for (const row of workspace.transactions.filter(
    (t) =>
      t.account === "0001" && t.date === "2024-01-15" && t.currency === "AUD",
  ))
    workspace = core({
      action: "review_transaction",
      id: row.id,
      state: "pending",
      reason: "Synthetic reconsideration.",
      expected_revision: workspace.revision,
    }).workspace;
  await panel.getByRole("button", { name: "Refresh flow workspace" }).click();
  await expect(panel.getByText(/This retained result is stale/)).toBeVisible();
  await calculate(panel);
  await expect(panel.getByText(/No accepted selected rows/)).toBeVisible();
  await expect(panel.getByLabel("Flow scope denominators")).toContainText(
    "Selected rows3",
  );
  await panel.getByLabel("Flow from", { exact: true }).fill("2020-01-01");
  await panel.getByLabel("Flow through", { exact: true }).fill("2020-01-31");
  await calculate(panel);
  await expect(
    panel.getByText("No imported rows match the applied scope.", {
      exact: true,
    }),
  ).toBeVisible();
  await panel.getByLabel("Flow from", { exact: true }).fill("2025-02-01");
  await calculate(panel);
  await expect(
    panel.getByRole("alert").filter({ hasText: "Account flows unavailable" }),
  ).toBeVisible();
  await expect(panel.getByText(/This retained result is stale/)).toBeVisible();
});
test("source corruption fails explicit verification, restoration retries and later corrections block old source reads", async ({
  page,
}) => {
  const panel = await open(page);
  await calculate(panel);
  const evidence = workspace.evidence[0],
    path = resolve(root, "originals", evidence.sha256);
  const original = readFileSync(path),
    mode = statSync(path).mode;
  chmodSync(path, 0o600);
  writeFileSync(path, Buffer.alloc(original.length, 120));
  await panel
    .getByRole("button", { name: "Review pairs", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Inspect both source rows" })
    .first()
    .click();
  await expect(
    page.getByText(
      "Source rows could not be verified. Refresh the workspace and recalculate if it changed.",
      { exact: true },
    ),
  ).toBeVisible();
  writeFileSync(path, original);
  chmodSync(path, mode);
  await page
    .getByRole("button", { name: "Retry source rows", exact: true })
    .click();
  await expect(page.locator("dialog .patterns-source")).toHaveCount(2);
  await page.keyboard.press("Escape");
  const debit = workspace.transactions[1];
  workspace = core({
    action: "correct_transaction",
    id: debit.id,
    amount: "-21.00",
    reason: "Synthetic correction after captured flow.",
    expected_revision: workspace.revision,
  }).workspace;
  await panel
    .getByRole("button", { name: "Review pairs", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Inspect both source rows" })
    .first()
    .click();
  await expect(
    page.getByText(/Source rows could not be verified/),
  ).toBeVisible();
  await expect(page.locator("dialog .patterns-source")).toHaveCount(0);
});
test("late response after navigation cannot publish into a new flow instance", async ({
  page,
}) => {
  let release!: () => void, ready!: () => void;
  const held = new Promise<void>((r) => {
      release = r;
    }),
    received = new Promise<void>((r) => {
      ready = r;
    });
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "analyze_account_flows")
      return route.continue();
    const response = await route.fetch();
    ready();
    await held;
    await route.fulfill({ response });
  });
  const panel = await open(page);
  await panel.getByRole("button", { name: "Calculate reviewed flows" }).click();
  await received;
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  release();
  await expect(
    panel.getByText(
      "Choose a scope and calculate. No flow result has been requested.",
    ),
  ).toBeVisible();
  await expect(panel.getByLabel("Applied flow scope")).toHaveCount(0);
});
test("response identity mismatch is unavailable, never displayed as a valid flow", async ({
  page,
}) => {
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "analyze_account_flows")
      return route.continue();
    const response = await route.fetch(),
      value = await response.json();
    value.request.account = "different";
    await route.fulfill({ response, json: value });
  });
  const panel = await open(page);
  await panel.getByRole("button", { name: "Calculate reviewed flows" }).click();
  await expect(panel.getByText(/response does not match/)).toBeVisible();
  await expect(panel.locator(".flow-lane")).toHaveCount(0);
});
test("compact controls, modal focus containment and exact values remain accessible", async ({
  page,
}) => {
  await page.setViewportSize({ width: 760, height: 1000 });
  const panel = await open(page);
  await selectScope(panel);
  const button = panel.getByRole("button", {
    name: "Calculate reviewed flows",
  });
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(panel.getByLabel("Applied flow scope")).toBeVisible();
  await axe(page, ".account-flows", "compact");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await capture(page, panel, "app-compact.png");
  await panel
    .getByRole("button", { name: "Review pairs", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Account flow sources" });
  const last = dialog
    .getByRole("button", { name: "Inspect both source rows" })
    .last();
  await last.focus();
  await page.keyboard.press("Tab");
  await expect(
    dialog.getByRole("button", { name: "Close account flow sources" }),
  ).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(last).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    panel.getByRole("button", { name: "Review pairs", exact: true }),
  ).toBeFocused();
});
test("pair and source pages retain every real repeated transfer without silent cutoff", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  const pairs: [number, number][] = [],
    rows: string[] = [];
  for (let i = 0; i < 27; i++) {
    pairs.push([i * 2, i * 2 + 1]);
    rows.push(
      `0001,2024-01-31,Synthetic pair ${i},-1.00,AUD`,
      `0002,2024-02-01,Synthetic pair ${i},1.00,AUD`,
    );
  }
  seed(`account,date,description,amount,currency\n${rows.join("\n")}\n`, pairs);
  const panel = await open(page);
  await calculate(panel);
  await expect(
    panel.getByRole("article", { name: "Flow 0001 to 0002 AUD" }),
  ).toContainText("27.00 AUD");
  await panel
    .getByRole("button", { name: "Review pairs", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Account flow sources" });
  await expect(dialog.locator(".flow-pair")).toHaveCount(25);
  await dialog.getByRole("button", { name: "Next transfer pairs" }).click();
  await expect(dialog.locator(".flow-pair")).toHaveCount(2);
  await expect(dialog).toContainText("26–27 of 27 pairs shown");
  await page.keyboard.press("Escape");
  await panel
    .getByRole("region", { name: "Account activity 0001 / AUD" })
    .getByRole("button", { name: "Accepted ledger (27)", exact: true })
    .click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(25);
  const first = await dialog
    .locator(".patterns-source > code")
    .allTextContents();
  await dialog.getByRole("button", { name: "Next flow source rows" }).click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(2);
  const second = await dialog
    .locator(".patterns-source > code")
    .allTextContents();
  expect(new Set([...first, ...second]).size).toBe(27);
});
test("a real revision change while calculation is held refuses the late captured result", async ({
  page,
}) => {
  let release!: () => void, ready!: () => void;
  const held = new Promise<void>((r) => {
      release = r;
    }),
    received = new Promise<void>((r) => {
      ready = r;
    });
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "analyze_account_flows")
      return route.continue();
    const response = await route.fetch();
    ready();
    await held;
    await route.fulfill({ response });
  });
  const panel = await open(page);
  await panel.getByRole("button", { name: "Calculate reviewed flows" }).click();
  await received;
  workspace = core({
    action: "review_transaction",
    id: workspace.transactions[0].id,
    state: "pending",
    reason: "Synthetic concurrent review.",
    expected_revision: workspace.revision,
  }).workspace;
  await page
    .getByRole("region", { name: "Paged transaction ledger", exact: true })
    .getByRole("button", { name: "Refresh workspace", exact: true })
    .click();
  await expect(
    panel.getByText(`WORKSPACE R${workspace.revision}`, { exact: true }),
  ).toBeVisible();
  release();
  await expect(
    panel.getByText(/Workspace changed during calculation/),
  ).toBeVisible();
  await expect(panel.locator(".flow-lane")).toHaveCount(0);
});
test("relationship and account pages explicitly bound the rendered subset and retain all groups", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  const pairs: [number, number][] = [],
    rows: string[] = [];
  for (let i = 0; i < 27; i++) {
    pairs.push([i * 2, i * 2 + 1]);
    rows.push(
      `D${String(i).padStart(3, "0")},2024-01-31,Synthetic debit ${i},-1.00,AUD`,
      `C${String(i).padStart(3, "0")},2024-02-01,Synthetic credit ${i},1.00,AUD`,
    );
  }
  seed(`account,date,description,amount,currency\n${rows.join("\n")}\n`, pairs);
  const panel = await open(page);
  await calculate(panel);
  await expect(panel.locator(".flow-lane")).toHaveCount(25);
  await expect(panel.getByText(/1–25 of 27 relationships shown/)).toBeVisible();
  const firstEdges = await panel
    .locator(".flow-lane")
    .evaluateAll((items) => items.map((n) => n.getAttribute("aria-label")));
  await panel.getByRole("button", { name: "Next flow relationships" }).click();
  await expect(panel.locator(".flow-lane")).toHaveCount(2);
  const lastEdges = await panel
    .locator(".flow-lane")
    .evaluateAll((items) => items.map((n) => n.getAttribute("aria-label")));
  expect(new Set([...firstEdges, ...lastEdges]).size).toBe(27);
  const groups: string[] = [];
  for (const count of [25, 25, 4]) {
    await expect(panel.locator(".flow-node")).toHaveCount(count);
    groups.push(...(await panel.locator(".flow-node h4").allTextContents()));
    const next = panel.getByRole("button", { name: "Next account groups" });
    if (count === 4) await expect(next).toBeDisabled();
    else await next.click();
  }
  expect(new Set(groups).size).toBe(54);
});
test("a refused workspace refresh preserves the old result but disables its source actions", async ({
  page,
}) => {
  const panel = await open(page);
  await calculate(panel);
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "view")
      return route.continue();
    await route.fulfill({
      status: 503,
      json: { error: "Synthetic workspace refresh unavailable" },
    });
  });
  await panel
    .getByRole("button", { name: "Refresh flow workspace", exact: true })
    .click();
  await expect(
    panel.getByText(/Workspace refresh was not confirmed/),
  ).toBeVisible();
  await expect(panel.getByLabel("Applied flow scope")).toBeVisible();
  await expect(
    panel.getByRole("button", { name: "Review pairs", exact: true }),
  ).toBeDisabled();
  await expect(panel.getByText(/This retained result is stale/)).toBeVisible();
});
