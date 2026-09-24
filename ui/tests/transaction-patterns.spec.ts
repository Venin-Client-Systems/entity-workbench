import { test, expect, type Page } from "@playwright/test";
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
const root = resolve("artifacts/synthetic-ui-workspace"),
  executable = resolve("target/debug/ew-dev"),
  captures = resolve("artifacts/transaction-patterns");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(executable, [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );
let workspace: Workspace;
test.beforeEach(() => {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  workspace = core({
    action: "import",
    name: "synthetic-patterns.csv",
    bytes: [...readFileSync("fixtures/transactions/patterns.csv")],
  }).workspace;
  const rows = workspace.transactions;
  for (let i = 0; i < rows.length; i++) {
    if (i === 10) continue;
    workspace = core({
      action: "review_transaction",
      id: rows[i].id,
      state: i === 11 ? "rejected" : i === 12 ? "deferred" : "accepted",
      reason: "Synthetic test source review.",
      expected_revision: workspace.revision,
    }).workspace;
  }
  workspace = core({
    action: "match_transfer",
    first: rows[13].id,
    second: rows[14].id,
    reason: "Synthetic reviewed internal transfer.",
    expected_revision: workspace.revision,
  }).workspace;
});
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  return page.getByRole("region", {
    name: "Transaction patterns",
    exact: true,
  });
}
async function calculate(page: Page) {
  const panel = await open(page);
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("20 in scope / 20 workspace rows"),
  ).toBeVisible();
  await expect(panel.getByText(/Scope edits are not applied/)).toHaveCount(0);
  return panel;
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
test("exact separate totals, full denominators, cash/refund and recurrence drill through real retained sources", async ({
  page,
}) => {
  const external: string[] = [],
    errors: string[] = [];
  page.on("request", (r) => {
    if (
      !r.url().startsWith("http://127.0.0.1:1420/") &&
      !/^(blob:|data:)/.test(r.url())
    )
      external.push(r.url());
  });
  page.on("pageerror", (e) => errors.push(e.message));
  const panel = await calculate(page),
    aud = panel.getByRole("region", { name: "AUD analysis" }),
    usd = panel.getByRole("region", { name: "USD analysis" });
  await expect(aud.getByText("125.00", { exact: true })).toBeVisible();
  await expect(aud.getByText("127.00", { exact: true })).toBeVisible();
  await expect(aud.getByText("-2.00", { exact: true })).toBeVisible();
  await expect(usd.getByText("-36.00", { exact: true })).toBeVisible();
  for (const label of [
    "Included (14)",
    "Pending (1)",
    "Rejected (1)",
    "Deferred (1)",
  ])
    await expect(
      aud.getByRole("button", { name: label, exact: true }),
    ).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "Eligible debit rows (13)" }),
  ).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "Unmatched debit rows (7)" }),
  ).toBeEnabled();
  await aud.getByRole("button", { name: "Cash source rows (1)" }).click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await expect(
    dialog.getByText("ATM Synthetic Terminal", { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByText("Heuristic: debit with atm token."),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    aud.getByRole("button", { name: "Cash source rows (1)" }),
  ).toBeFocused();
  await aud.getByRole("button", { name: "Refund source rows (2)" }).click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(2);
  await expect(
    dialog.getByText("Synthetic REFUND charged back", { exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  const cafe = panel.getByRole("row").filter({ hasText: "SYNTHETIC CAFE" });
  await expect(cafe).toContainText("12.00");
  await cafe.getByRole("button", { name: "Inspect group (3)" }).click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(3);
  await expect(
    dialog.getByText(
      "Possible duplicate: retained in its review-state partition.",
    ),
  ).toHaveCount(2);
  await page.keyboard.press("Escape");
  const opener = panel.getByRole("button", {
    name: "Review cadence 0001 AUD ACME CLUB",
    exact: true,
  });
  await opener.click();
  await expect(
    dialog
      .getByRole("region", { name: "Anchored cadence schedule" })
      .getByRole("row"),
  ).toHaveCount(4);
  await expect(dialog.getByText("0 days", { exact: true })).toHaveCount(3);
  await axe(
    page,
    'dialog[aria-label="Pattern source rows"]',
    "cadence-desktop",
  );
  await dialog.screenshot({ path: resolve(captures, "cadence-desktop.png") });
  await dialog
    .getByRole("button", {
      name: "Inspect source and review row 2",
      exact: true,
    })
    .click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Close review" }),
  ).toBeFocused();
  await expect(
    page.getByText("Preserved source value", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".transaction-review")).toContainText("ACME CLUB");
  expect(core({ action: "view" }).workspace.revision).toBe(workspace.revision);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
});
test("explicit transfer exclusion and scope drafts never silently change the applied result", async ({
  page,
}) => {
  const panel = await calculate(page),
    aud = panel.getByRole("region", { name: "AUD analysis" });
  await panel
    .getByLabel("Transfer treatment")
    .selectOption("exclude_reviewed_pairs");
  await expect(panel.getByText(/Scope edits are not applied/)).toBeVisible();
  await expect(aud.getByText("125.00", { exact: true })).toBeVisible();
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(aud.getByText("105.00", { exact: true })).toBeVisible();
  await expect(aud.getByText("107.00", { exact: true })).toBeVisible();
  await aud.getByRole("button", { name: "Excluded transfers (2)" }).click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await expect(dialog.locator(".patterns-source")).toHaveCount(2);
  await expect(
    dialog.getByText(/Verified reviewed transfer peer:/),
  ).toHaveCount(2);
  await page.keyboard.press("Escape");
  await panel
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0001");
  await panel
    .getByLabel("Analysis currency", { exact: true })
    .selectOption("AUD");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("15 in scope / 20 workspace rows"),
  ).toBeVisible();
  await expect(
    aud.getByRole("button", { name: "Excluded transfers (1)" }),
  ).toBeEnabled();
  await expect(panel.getByRole("region", { name: "USD analysis" })).toHaveCount(
    0,
  );
  await panel.getByLabel("From date").fill("2030-01-01");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(panel.getByText("0 in scope / 20 workspace rows")).toBeVisible();
  await expect(
    panel.getByText("No transaction rows match this scope."),
  ).toBeVisible();
});
test("correction makes results stale and recalculation removes pending row from every pattern", async ({
  page,
}) => {
  const panel = await calculate(page);
  await panel
    .getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await dialog
    .getByRole("button", {
      name: "Inspect source and review row 2",
      exact: true,
    })
    .click();
  await page
    .getByLabel("Transaction decision reason")
    .fill("Synthetic amount correction.");
  await page.getByLabel("Corrected amount", { exact: true }).fill("-11.00");
  await page
    .getByRole("button", { name: "Save correction for review", exact: true })
    .click();
  await expect(panel.getByText(/These results are stale/)).toBeVisible();
  await expect(
    panel.getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    }),
  ).toBeDisabled();
  await expect(page.getByRole("button", { name: "Close review" })).toHaveCount(
    0,
  );
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(panel.getByText(/These results are stale/)).toHaveCount(0);
  await expect(
    panel
      .getByRole("region", { name: "AUD analysis" })
      .getByText("117.00", { exact: true }),
  ).toBeVisible();
  await expect(
    panel.getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    }),
  ).toHaveCount(0);
  await expect(
    panel.getByRole("button", {
      name: "Review cadence 0001 USD ACME CLUB",
      exact: true,
    }),
  ).toBeVisible();
});
test("stale backend revision fails honestly and late replies after unmount cannot become a new calculation", async ({
  page,
}) => {
  const panel = await calculate(page);
  workspace = core({
    action: "correct_transaction",
    id: workspace.transactions[0].id,
    amount: "-11.00",
    reason: "Synthetic external correction.",
    expected_revision: workspace.revision,
  }).workspace;
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(panel.getByRole("alert")).toContainText(
    "Calculation unavailable",
  );
  await expect(panel.getByRole("alert")).toContainText("revision");
  await panel.getByRole("button", { name: "Refresh workspace" }).click();
  await expect(panel.getByText(/These results are stale/)).toBeVisible();
  let release: () => void = () => {},
    started: () => void = () => {};
  const held = new Promise<void>((r) => (release = r)),
    pending = new Promise<void>((r) => (started = r));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "analyze_transactions") {
      started();
      await held;
    }
    await route.continue();
  });
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await pending;
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Overview/ })
    .click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  const delivered = page.waitForResponse(
    (r) => r.request().postDataJSON()?.action === "analyze_transactions",
  );
  release();
  await (await delivered).finished();
  await expect(
    panel.getByText("NOT CALCULATED", { exact: true }),
  ).toBeVisible();
});
test("industrial scope and review surfaces remain accessible at desktop and compact sizes", async ({
  page,
}) => {
  const panel = await calculate(page);
  await axe(page, ".patterns", "patterns-desktop");
  await page.setViewportSize({ width: 1440, height: 3200 });
  await panel.screenshot({
    path: resolve(captures, "patterns-full-surface.png"),
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await panel.evaluate((element) => {
    element.scrollIntoView();
    window.scrollBy(0, -110);
  });
  await page.screenshot({ path: resolve(captures, "patterns-desktop.png") });
  await page.setViewportSize({ width: 720, height: 900 });
  await axe(page, ".patterns", "patterns-compact");
  expect(await panel.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(
    true,
  );
  await panel.evaluate((element) => {
    element.scrollIntoView();
    window.scrollBy(0, -110);
  });
  await page.screenshot({ path: resolve(captures, "patterns-compact.png") });
  await panel
    .getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await axe(
    page,
    'dialog[aria-label="Pattern source rows"]',
    "cadence-compact",
  );
  expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(
    true,
  );
  await dialog.screenshot({ path: resolve(captures, "cadence-compact.png") });
  await page.keyboard.press("Escape");
  await expect(
    panel.getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    }),
  ).toBeFocused();
});

test("source pagination retains all pending rows and hostile descriptions remain escaped", async ({
  page,
}) => {
  const hostile = "<img src=x onerror=window.patternExecuted=true>";
  const text =
    "account,date,description,amount,currency\n" +
    Array.from(
      { length: 27 },
      (_, i) =>
        `0003,2025-02-01,${i === 26 ? hostile : "Synthetic page " + i},-1.00,AUD`,
    ).join("\n");
  workspace = core({
    action: "import",
    name: "synthetic-pagination.csv",
    bytes: [...Buffer.from(text)],
  }).workspace;
  const panel = await open(page);
  await panel
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0003");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("27 in scope / 47 workspace rows"),
  ).toBeVisible();
  await expect(
    panel
      .getByRole("region", { name: "AUD analysis" })
      .getByRole("button", { name: "Included (0)" }),
  ).toBeDisabled();
  await panel.getByRole("button", { name: "Pending (27)" }).click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await expect(dialog.locator(".patterns-source")).toHaveCount(25);
  await expect(dialog.getByText("1–25 of 27 source rows")).toBeVisible();
  await dialog.getByRole("button", { name: "Next source rows" }).click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(2);
  await expect(dialog.getByText("26–27 of 27 source rows")).toBeVisible();
  await expect(dialog.getByText(hostile, { exact: true })).toBeVisible();
  await expect(dialog.locator("img,script,svg,iframe")).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as unknown as Record<string, unknown>).patternExecuted,
    ),
  ).toBeUndefined();
  await dialog.getByRole("button", { name: "Previous source rows" }).click();
  await expect(dialog.locator(".patterns-source")).toHaveCount(25);
});

test("an arriving workspace refresh disables an open stale source review and restores a usable control", async ({
  page,
}) => {
  const panel = await calculate(page);
  workspace = core({
    action: "correct_transaction",
    id: workspace.transactions[0].id,
    amount: "-11.00",
    reason: "Synthetic concurrent refresh.",
    expected_revision: workspace.revision,
  }).workspace;
  let release: () => void = () => {},
    started: () => void = () => {};
  const held = new Promise<void>((r) => (release = r)),
    pending = new Promise<void>((r) => (started = r));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action === "view") {
      started();
      await held;
    }
    await route.continue();
  });
  await panel.getByRole("button", { name: "Refresh workspace" }).click();
  await pending;
  await panel
    .getByRole("button", {
      name: "Review cadence 0001 AUD ACME CLUB",
      exact: true,
    })
    .click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await expect(dialog.getByRole("alert")).toContainText(
    "Source rows could not be verified",
  );
  await expect(dialog.locator(".patterns-source")).toHaveCount(0);
  const delivered = page.waitForResponse(
    (r) => r.request().postDataJSON()?.action === "view",
  );
  release();
  await (await delivered).finished();
  await expect(dialog.getByRole("alert")).toContainText(
    "Source rows are unavailable for this stale result",
  );
  await expect(dialog.locator(".patterns-source")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(
    panel.getByRole("button", { name: "Calculate reviewed patterns" }),
  ).toBeFocused();
});

test("source reads reject altered originals and retry exact canonical rows after restoration", async ({
  page,
}) => {
  const panel = await calculate(page);
  const original = resolve(root, "originals", workspace.evidence[0].sha256);
  const bytes = readFileSync(original),
    mode = statSync(original).mode;
  chmodSync(original, 0o600);
  try {
    writeFileSync(original, "Synthetic altered retained original");
    await panel
      .getByRole("button", {
        name: "Review cadence 0001 AUD ACME CLUB",
        exact: true,
      })
      .click();
    const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
    await expect(dialog.getByRole("alert")).toContainText(
      "Source rows could not be verified",
    );
    await expect(dialog.locator(".patterns-source")).toHaveCount(0);
    writeFileSync(original, bytes);
    chmodSync(original, mode);
    await dialog.getByRole("button", { name: "Retry source rows" }).click();
    await expect(dialog.locator(".patterns-source")).toHaveCount(3);
    await expect(dialog.getByRole("alert")).toHaveCount(0);
    for (const row of workspace.transactions.slice(0, 3)) {
      await expect(dialog.getByText(row.id, { exact: true })).toBeVisible();
    }
  } finally {
    chmodSync(original, 0o600);
    writeFileSync(original, bytes);
    chmodSync(original, mode);
  }
});

test("late source-page replies cannot replace a newer bounded canonical selection", async ({
  page,
}) => {
  workspace = core({
    action: "import",
    name: "synthetic-source-pages.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency\n" +
          Array.from(
            { length: 27 },
            (_, i) => `0003,2025-02-01,Synthetic page ${i},-1.00,AUD`,
          ).join("\n"),
      ),
    ],
  }).workspace;
  const panel = await open(page);
  await panel
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0003");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("27 in scope / 47 workspace rows"),
  ).toBeVisible();
  let release: () => void = () => {},
    started: () => void = () => {},
    fulfilled: () => void = () => {};
  const held = new Promise<void>((r) => (release = r));
  const pending = new Promise<void>((r) => (started = r));
  const delivered = new Promise<void>((r) => (fulfilled = r));
  const requests: {
    expected_revision: number;
    request: { rows: { id: string; expected_version: number }[] };
  }[] = [];
  await page.route("**/api/workbench", async (route) => {
    const body = route.request().postDataJSON();
    if (body.action !== "read_transaction_sources") return route.continue();
    requests.push(body);
    if (requests.length !== 1) return route.continue();
    const response = await route.fetch();
    started();
    await held;
    await route.fulfill({ response });
    fulfilled();
  });
  try {
    await panel.getByRole("button", { name: "Pending (27)" }).click();
    await pending;
    const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
    await expect(dialog.getByRole("status")).toHaveText(
      "Verifying selected source rows…",
    );
    await dialog.getByRole("button", { name: "Next source rows" }).click();
    await expect(dialog.locator(".patterns-source")).toHaveCount(2);
    await expect(
      dialog.getByText("Synthetic page 26", { exact: true }),
    ).toBeVisible();
    release();
    await delivered;
    await expect(dialog.locator(".patterns-source")).toHaveCount(2);
    await expect(
      dialog.getByText("Synthetic page 0", { exact: true }),
    ).toHaveCount(0);
    expect(requests.map((r) => r.request.rows.length)).toEqual([25, 2]);
    const selected = workspace.transactions.filter((r) => r.account === "0003");
    expect(requests.flatMap((r) => r.request.rows)).toEqual(
      selected.map((r) => ({ id: r.id, expected_version: r.version })),
    );
    expect(
      requests.every((r) => r.expected_revision === workspace.revision),
    ).toBe(true);
  } finally {
    release();
  }
});

test("returning to an earlier source page requires a new verification before enabling review", async ({
  page,
}) => {
  workspace = core({
    action: "import",
    name: "synthetic-revisited-pages.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency\n" +
          Array.from(
            { length: 27 },
            (_, i) => `0003,2025-02-01,Synthetic return ${i},-1.00,AUD`,
          ).join("\n"),
      ),
    ],
  }).workspace;
  const panel = await open(page);
  await panel
    .getByLabel("Analysis account", { exact: true })
    .selectOption("0003");
  await panel
    .getByRole("button", { name: "Calculate reviewed patterns" })
    .click();
  await expect(
    panel.getByText("27 in scope / 47 workspace rows"),
  ).toBeVisible();
  await panel.getByRole("button", { name: "Pending (27)" }).click();
  const dialog = page.getByRole("dialog", { name: "Pattern source rows" });
  await expect(dialog.locator(".patterns-source")).toHaveCount(25);
  let count = 0,
    releaseB: () => void = () => {},
    releaseA: () => void = () => {};
  let startedB: () => void = () => {},
    startedA: () => void = () => {};
  let finishedB: () => void = () => {},
    finishedA: () => void = () => {};
  const heldB = new Promise<void>((r) => (releaseB = r)),
    heldA = new Promise<void>((r) => (releaseA = r));
  const pendingB = new Promise<void>((r) => (startedB = r)),
    pendingA = new Promise<void>((r) => (startedA = r));
  const doneB = new Promise<void>((r) => (finishedB = r)),
    doneA = new Promise<void>((r) => (finishedA = r));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON()?.action !== "read_transaction_sources")
      return route.continue();
    const ticket = ++count;
    const response = await route.fetch();
    if (ticket === 1) {
      startedB();
      await heldB;
    } else {
      startedA();
      await heldA;
    }
    await route.fulfill({ response });
    if (ticket === 1) finishedB();
    else finishedA();
  });
  try {
    await dialog.getByRole("button", { name: "Next source rows" }).click();
    await pendingB;
    const target = workspace.transactions.find((r) => r.account === "0003")!;
    workspace = core({
      action: "correct_transaction",
      id: target.id,
      amount: "-2.00",
      reason: "Synthetic concurrent source-page correction",
      expected_revision: workspace.revision,
    }).workspace;
    await dialog.getByRole("button", { name: "Previous source rows" }).click();
    await pendingA;
    await expect(dialog.getByRole("status")).toHaveText(
      "Verifying selected source rows…",
    );
    await expect(dialog.locator(".patterns-source")).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: /Inspect source and review/ }),
    ).toHaveCount(0);
    releaseB();
    await doneB;
    await expect(dialog.locator(".patterns-source")).toHaveCount(0);
    releaseA();
    await doneA;
    await expect(dialog.getByRole("alert")).toContainText(
      "Transaction source revision changed",
    );
    await expect(dialog.locator(".patterns-source")).toHaveCount(0);
  } finally {
    releaseB();
    releaseA();
  }
});
