import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import type { Workspace } from "../src/types";

const root = resolve("artifacts/synthetic-ui-workspace");
const core = (input: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(input),
      encoding: "utf8",
    }),
  );

test("direct transaction and source excerpts refuse an altered original despite a cached derivative", async ({
  page,
}) => {
  rmSync(root, { recursive: true, force: true });
  const w: Workspace = core({
    action: "import",
    name: "synthetic.csv",
    bytes: [
      ...Buffer.from(
        "account,date,description,amount,currency\n0042,2025-01-01,Synthetic exact purchase,-0.10000001,AUD\n",
      ),
    ],
  }).workspace;
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  // The bounded ledger now verifies its page originals before displaying rows.
  // Corrupt only after that real page is visible to exercise fresh excerpt checks.
  await expect(
    page.getByRole("button", { name: "Synthetic exact purchase", exact: true }),
  ).toBeVisible();
  const original = resolve(root, "originals", w.evidence[0].sha256);
  const bytes = readFileSync(original),
    mode = statSync(original).mode;
  chmodSync(original, 0o600);
  try {
    // Same length proves this is digest verification, not just a size check.
    const changed = Buffer.from(bytes);
    changed[0] = "X".charCodeAt(0);
    writeFileSync(original, changed);
    await page
      .getByRole("button", { name: "Synthetic exact purchase", exact: true })
      .click();
    const excerpt = page.getByRole("region", {
      name: "Original transaction excerpt",
    });
    await expect(excerpt.getByRole("alert")).toContainText(
      "Original evidence checksum mismatch",
    );
    await expect(excerpt.locator(".source-quote")).toHaveCount(0);
    await page
      .getByRole("button", { name: "Inspect preserved source" })
      .click();
    const source = page.getByRole("region", { name: "Source anchor excerpt" });
    await expect(source.getByRole("alert")).toContainText(
      "Original evidence checksum mismatch",
    );
    await expect(source.locator(".source-quote")).toHaveCount(0);
    await expect(source).not.toContainText(
      "Validated against workspace revision",
    );
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Close review" }).click();
    writeFileSync(original, bytes);
    chmodSync(original, mode);
    await page
      .getByRole("button", { name: "Synthetic exact purchase", exact: true })
      .click();
    await expect(excerpt.locator(".source-quote")).toHaveText("-0.10000001");
    await expect(excerpt.getByRole("alert")).toHaveCount(0);
    await page
      .getByRole("button", { name: "Inspect preserved source" })
      .click();
    await expect(source.locator(".source-quote")).toHaveText("-0.10000001");
    await expect(source).toContainText(
      `Validated against workspace revision ${w.revision}`,
    );
    expect(core({ action: "view" }).workspace).toEqual(w);
  } finally {
    chmodSync(original, 0o600);
    writeFileSync(original, bytes);
    chmodSync(original, mode);
  }
});
