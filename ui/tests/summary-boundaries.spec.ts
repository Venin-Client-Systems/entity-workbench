import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
const root = resolve("artifacts/synthetic-ui-workspace");
function seed(count: number) {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  return JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root, "--summary"], {
      input: JSON.stringify({
        action: "import",
        name: "synthetic-paging-bound.csv",
        bytes: [
          ...Buffer.from(
            "account,date,description,amount,currency\n" +
              Array.from(
                { length: count },
                (_, i) =>
                  `0001,2025-01-01,Distinct synthetic record ${i},-1.00,AUD`,
              ).join("\n") +
              "\n",
          ),
        ],
      }),
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
    }),
  );
}
test("forward continuation survives the bounded 100-page back history", async ({
  page,
}) => {
  test.setTimeout(120_000);
  seed(2600);
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  const ledger = page.getByRole("region", {
    name: "Paged transaction ledger",
    exact: true,
  });
  await ledger.getByLabel("Ledger rows per request").selectOption("25");
  await ledger.getByRole("button", { name: "Apply ledger filters" }).click();
  for (let i = 0; i < 102; i++) {
    await expect(
      ledger.getByRole("status").filter({
        hasText: `${i * 25 + 1}–${(i + 1) * 25} of 2600 selected rows`,
      }),
    ).toBeVisible();
    await ledger.getByRole("button", { name: "Next ledger page" }).click();
  }
  await expect(ledger).toContainText("2551–2575 of 2600 selected rows");
  await expect(ledger).toContainText("Back covers the last 100 pages");
  await expect(
    ledger.getByRole("button", { name: "Next ledger page" }),
  ).toBeEnabled();
  await ledger.getByRole("button", { name: "Next ledger page" }).click();
  await expect(ledger).toContainText("2576–2600 of 2600 selected rows");
  await expect(
    ledger.getByRole("button", { name: "Next ledger page" }),
  ).toBeDisabled();
  await ledger.getByRole("button", { name: "First ledger page" }).click();
  await expect(ledger).toContainText("1–25 of 2600 selected rows");
});
test("delayed page completion remembers deliberate focus movement followed by blur", async ({
  page,
}) => {
  seed(125);
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  const ledger = page.getByRole("region", {
    name: "Paged transaction ledger",
    exact: true,
  });
  await expect(ledger).toContainText("1–100 of 125 selected rows");
  let release!: () => void, received!: () => void;
  const gate = new Promise<void>((r) => (release = r)),
    ready = new Promise<void>((r) => (received = r));
  await page.route("**/api/workbench", async (route) => {
    if (route.request().postDataJSON().action !== "search_transactions")
      return route.continue();
    const response = await route.fetch();
    received();
    await gate;
    await route.fulfill({ response });
  });
  await ledger.getByRole("button", { name: "Next ledger page" }).click();
  await ready;
  const input = ledger.getByLabel("Filter transactions");
  await input.focus();
  await input.evaluate((node) => node.blur());
  release();
  await expect(ledger).toContainText("101–125 of 125 selected rows");
  await page.evaluate(
    () =>
      new Promise<void>((done) =>
        requestAnimationFrame(() => requestAnimationFrame(() => done())),
      ),
  );
  await expect(
    ledger.getByRole("status").filter({ hasText: "101–125" }),
  ).not.toBeFocused();
});
test("summary transport guard refuses impossible denominators from copies of a real canonical result", async ({
  page,
}) => {
  const actual = seed(2);
  await page.goto("/");
  const messages = await page.evaluate(async (result) => {
    const { readDesktopSummary } = await import("/src/desktop-summary.ts");
    const mutations = [
      (value: any) => value.analysis.transaction_count++,
      (value: any) =>
        (value.analysis.balance_discrepancy_count =
          value.analysis.balance_check_count + 1),
      (value: any) =>
        (value.analysis.duplicate_candidate_row_count =
          value.analysis.transaction_count + 1),
    ];
    return mutations.map((change) => {
      const value = structuredClone(result);
      change(value);
      try {
        readDesktopSummary(value);
        return "accepted";
      } catch (error) {
        return String(error);
      }
    });
  }, actual);
  expect(messages).toHaveLength(3);
  expect(
    messages.every((message) =>
      message.includes("Inconsistent desktop summary counts"),
    ),
  ).toBe(true);
});

test("shared publication retains a newer canonical import when an older real view response arrives later", async ({
  page,
}) => {
  const older = seed(2);
  const newer = JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root, "--summary"], {
      input: JSON.stringify({
        action: "import",
        name: "synthetic-newer-import.csv",
        bytes: [
          ...Buffer.from(
            "account,date,description,amount,currency\n0042,2025-01-02,New canonical import,-3.00,AUD\n",
          ),
        ],
      }),
      encoding: "utf8",
    }),
  );
  expect(newer.workspace.revision).toBeGreaterThan(older.workspace.revision);
  await page.goto("/");
  const result = await page.evaluate(
    async ({ older, newer }) => {
      const { readDesktopSummary, retainNewestSummary } = await import(
        "/src/desktop-summary.ts"
      );
      const first = retainNewestSummary(null, readDesktopSummary(older));
      const imported = retainNewestSummary(first, readDesktopSummary(newer));
      const late = retainNewestSummary(imported, readDesktopSummary(older));
      return {
        unchangedIdentity: late === imported,
        revision: late.workspace.revision,
        count: late.analysis.transaction_count,
      };
    },
    { older, newer },
  );
  expect(result).toEqual({
    unchangedIdentity: true,
    revision: newer.workspace.revision,
    count: 3,
  });
  const current = JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root, "--summary"], {
      input: '{"action":"view"}',
      encoding: "utf8",
    }),
  );
  expect(current).toEqual(newer);
});

test("balance annotation guard refuses missing, unknown and malformed checked states from copies of a real batch", async ({
  page,
}) => {
  seed(1);
  const execute = (input: unknown) =>
    JSON.parse(
      execFileSync(resolve("target/debug/ew-dev"), [root], {
        input: JSON.stringify(input),
        encoding: "utf8",
      }),
    );
  const w = execute({
    action: "import",
    name: "synthetic-checked-balances.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency,balance\n0002,2025-01-01,Opening,-1.00,AUD,100.00\n0002,2025-01-02,Movement,-2.00,AUD,98.00\n",
      ),
    ],
  }).workspace;
  const row = w.transactions.at(-1),
    wanted = [{ id: row.id, expected_version: row.version }];
  const value = execute({
    action: "read_transaction_balances",
    request: { rows: wanted },
    expected_revision: w.revision,
  });
  expect(value.rows[0].balance.state).toBe("checked");
  await page.goto("/");
  const rejected = await page.evaluate(
    async ({ value, wanted, revision }) => {
      const { validateBalances } = await import(
        "/src/transaction-ledger-types.ts"
      );
      validateBalances(value, wanted, revision);
      const invalid = [
        undefined,
        { state: "future_state" },
        { ...value.rows[0].balance, previous_version: 0 },
        { ...value.rows[0].balance, contributing_row_count: 0 },
        { ...value.rows[0].balance, difference: null },
        { ...value.rows[0].balance, reconciled: "false" },
      ];
      return invalid.map((balance) => {
        const changed = structuredClone(value);
        changed.rows[0].balance = balance;
        try {
          validateBalances(changed, wanted, revision);
          return false;
        } catch {
          return true;
        }
      });
    },
    { value, wanted, revision: w.revision },
  );
  expect(rejected).toEqual([true, true, true, true, true, true]);
});
