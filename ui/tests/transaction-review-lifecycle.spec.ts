import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve("artifacts/synthetic-ui-workspace");
const core = (input: Record<string, unknown>) => JSON.parse(execFileSync(
  resolve("target/debug/ew-dev"), [root], { input: JSON.stringify(input), encoding: "utf8" },
));
for (const sameRow of [true, false]) {
  test(`late correction acknowledgement cannot close a reopened ${sameRow ? "same" : "different"} transaction review`, async ({ page }) => {
    rmSync(root, { recursive: true, force: true });
    mkdirSync(root, { recursive: true });
    const workspace = core({ action: "import", name: "review-lifecycle.csv", bytes: [...Buffer.from(
      "account,date,description,amount,currency,balance\n0001,2025-01-01,Synthetic first,-10.00,AUD,\n0001,2025-01-02,Synthetic second,-20.00,AUD,\n",
    )] }).workspace;
    const first = workspace.transactions.find((row: any) => row.description === "Synthetic first");
    const next = sameRow ? first : workspace.transactions.find((row: any) => row.description === "Synthetic second");
    let release!: () => void, received!: () => void;
    const held = new Promise<void>((resolve) => { release = resolve; });
    const ready = new Promise<void>((resolve) => { received = resolve; });
    const writes: unknown[] = [];
    await page.route("**/api/workbench", async (route) => {
      const input = route.request().postDataJSON();
      if (input.action !== "correct_transaction") return route.continue();
      writes.push(input);
      const response = await route.fetch();
      received();
      await held;
      await route.fulfill({ response });
    });
    await page.goto("/");
    await page.getByRole("navigation").getByRole("button", { name: /Transactions/ }).click();
    await page.locator(`[id="transaction-${first.id}"]`).click();
    const review = page.getByRole("complementary", { name: "Transaction review" });
    await page.getByLabel("Transaction decision reason").fill("Synthetic correction reason");
    await page.getByLabel("Corrected amount").fill("-9.00");
    await review.getByRole("button", { name: "Save correction for review" }).click();
    await ready;
    await page.getByRole("button", { name: "Close review" }).click();
    await expect(review).not.toBeVisible();
    await page.locator(`[id="transaction-${next.id}"]`).click();
    await page.getByLabel("Transaction decision reason").fill("Preserve this later review draft");
    release();
    await expect(page.getByText("Processing workspace action…")).not.toBeVisible();
    await expect(review).toBeVisible();
    await expect(review).toContainText("Workspace changed. This review remains at revision");
    await expect(page.getByLabel("Transaction decision reason")).toHaveValue("Preserve this later review draft");
    await expect(review.getByRole("button", { name: "Accept", exact: true })).toBeDisabled();
    await page.getByRole("button", { name: "Close review" }).click();
    await page.locator(`[id="transaction-${next.id}"]`).click();
    await expect(page.getByLabel("Corrected amount")).toHaveValue(sameRow ? "-9.00" : "-20.00");
    expect(writes).toHaveLength(1);
    const current = core({ action: "view" }).workspace;
    expect(current.revision).toBe(workspace.revision + 1);
    expect(current.transactions.find((row: any) => row.id === first.id).amount).toBe("-9.00");
    expect(current.transactions.every((row: any) => row.review === "pending")).toBe(true);
  });
}
