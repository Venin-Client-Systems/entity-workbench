import { test, expect, type Page } from "@playwright/test";
import { spawn, execFileSync } from "node:child_process";
import { createInterface } from "node:readline";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
const root = resolve("artifacts/native-export-ui-workspace");
const captures = resolve("artifacts/native-export-ui");
test.beforeAll(() => {
  execFileSync(
    "cargo",
    [
      "build",
      "-p",
      "workbench-core",
      "--locked",
      "--example",
      "native_export_session",
    ],
    { stdio: "pipe" },
  );
});
const core = (command: Record<string, unknown>) =>
  JSON.parse(
    execFileSync(resolve("target/debug/ew-dev"), [root], {
      input: JSON.stringify(command),
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    }),
  );
function fixture(report = false) {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(captures, { recursive: true });
  const rows = Array.from(
    { length: 113 },
    (_, i) => `0001,2025-01-01,Synthetic payment ${i},-0.10000001,AUD`,
  ).join("\n");
  const w = core({
    action: "import",
    name: "synthetic-export.csv",
    bytes: [
      ...Buffer.from(`account,date,description,amount,currency\n${rows}\n`),
    ],
  }).workspace;
  if (report) {
    const saved = core({ action: "save_report" }).workspace.reports[0];
    core({
      action: "import",
      name: "later.txt",
      bytes: [...Buffer.from("Later synthetic text outside saved report")],
    });
    return { w, report: saved };
  }
  return { w };
}
type Call = { command: string; args: Record<string, any> };
async function bridge(
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
    resolve: (v: any) => void;
    reject: (e: Error) => void;
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
  process.stderr.on("data", (data) => (diagnostics += data));
  process.on("exit", () => {
    for (const item of pending.splice(0))
      item.reject(new Error(`Test core exited: ${diagnostics}`));
  });
  const send = (value: Record<string, unknown>) =>
    new Promise<any>((resolve, reject) => {
      pending.push({ resolve, reject });
      process.stdin.write(JSON.stringify(value) + "\n");
    });
  const calls: string[] = [];
  await page.route("**/native-test-ipc", async (route) => {
    const call: Call = route.request().postDataJSON();
    calls.push(call.command);
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
      const value = await send(input);
      const changed = await intercept?.(call, value);
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
        const response = await fetch("/native-test-ipc", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ command, args }),
        });
        const result = await response.json();
        if (!response.ok)
          throw new Error(
            result.error ?? "Held test transport acknowledgement lost",
          );
        return result;
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
const ledger = (page: Page) =>
  page.getByRole("region", { name: "Paged transaction ledger", exact: true });
async function open(page: Page) {
  await page.goto("/");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: /Transactions/ })
    .click();
  await expect(
    ledger(page).getByRole("button", { name: "Export JSON", exact: true }),
  ).toBeEnabled();
}
function exported() {
  return readdirSync(resolve(root, "exports")).filter((name) =>
    name.startsWith("transactions-r"),
  );
}
function staging() {
  return readdirSync(resolve(root, "native-export-staging"));
}

test("native transaction export saves exact complete canonical bytes, without a browser download, and recovers a lost commit acknowledgement", async ({
  page,
}) => {
  const { w } = fixture();
  let commits = 0;
  const b = await bridge(page, async (call) => {
    if (call.command === "commit_native_export" && ++commits === 1)
      return {
        status: 503,
        value: { error: "Synthetic lost commit acknowledgement" },
      };
  });
  const downloads: string[] = [];
  page.on("download", (file) => downloads.push(file.suggestedFilename()));
  try {
    await open(page);
    await ledger(page).getByRole("button", { name: "Export JSON" }).click();
    await expect(ledger(page)).toContainText(
      `Saved 113 matching rows at revision ${w.revision}`,
    );
    expect(commits).toBe(2);
    expect(exported()).toHaveLength(1);
    expect(staging()).toEqual([]);
    expect(downloads).toEqual([]);
    const bytes = readFileSync(resolve(root, "exports", exported()[0]));
    const expected = await b.send({
      method: "workbench",
      command: {
        action: "export_transactions",
        request: {
          query: "",
          filter: {
            date_from: null,
            date_to: null,
            account: null,
            currency: null,
            review: null,
          },
          order: "date_ascending",
        },
        expected_revision: w.revision,
      },
    });
    expect(bytes.toString()).toBe(expected.json);
    expect(JSON.parse(bytes.toString())).toHaveLength(113);
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(
      expected.sha256,
    );
    await page.screenshot({
      path: resolve(captures, "native-save-receipt.png"),
      fullPage: false,
    });
  } finally {
    await b.close();
  }
});

test("held native preparation is discarded after scope changes, with no commit or saved file", async ({
  page,
}) => {
  fixture();
  let release!: () => void, ready!: () => void;
  const held = new Promise<void>((r) => (release = r)),
    received = new Promise<void>((r) => (ready = r));
  const b = await bridge(page, async (call) => {
    if (call.command === "prepare_native_export") {
      ready();
      await held;
    }
  });
  try {
    await open(page);
    await ledger(page).getByRole("button", { name: "Export JSON" }).click();
    await received;
    expect(staging()).toHaveLength(1);
    await ledger(page).getByLabel("Filter transactions").fill("payment 80");
    await ledger(page)
      .getByRole("button", { name: "Apply ledger filters" })
      .click();
    await expect(ledger(page)).toContainText("1–1 of 1 selected rows");
    release();
    await expect(ledger(page).getByRole("alert")).toContainText(
      "Ledger changed while preparing export",
    );
    await expect.poll(staging).toEqual([]);
    expect(b.calls).not.toContain("commit_native_export");
    expect(exported()).toEqual([]);
  } finally {
    release();
    await b.close();
  }
});

test("held native preparation is discarded after leaving the ledger", async ({
  page,
}) => {
  fixture();
  let release!: () => void, ready!: () => void;
  const held = new Promise<void>((r) => (release = r)),
    received = new Promise<void>((r) => (ready = r));
  const b = await bridge(page, async (call) => {
    if (call.command === "prepare_native_export") {
      ready();
      await held;
    }
  });
  try {
    await open(page);
    await ledger(page).getByRole("button", { name: "Export JSON" }).click();
    await received;
    await page
      .getByRole("navigation")
      .getByRole("button", { name: /Assessment/ })
      .click();
    release();
    await expect
      .poll(() => b.calls.includes("discard_native_export"))
      .toBe(true);
    await expect.poll(staging).toEqual([]);
    expect(b.calls).not.toContain("commit_native_export");
    expect(exported()).toEqual([]);
  } finally {
    release();
    await b.close();
  }
});

test("mismatched native preparation metadata is refused and the real stage is discarded", async ({
  page,
}) => {
  fixture();
  const b = await bridge(page, async (call, value) =>
    call.command === "prepare_native_export"
      ? { value: { ...value, artifact: { ...value.artifact, row_count: 999 } } }
      : undefined,
  );
  try {
    await open(page);
    await ledger(page).getByRole("button", { name: "Export JSON" }).click();
    await expect(ledger(page).getByRole("alert")).toContainText(
      "identity did not match",
    );
    expect(exported()).toEqual([]);
    expect(staging()).toEqual([]);
    expect(b.calls).not.toContain("commit_native_export");
  } finally {
    await b.close();
  }
});

test("native HTML export saves exact historical HTML after later canonical changes", async ({
  page,
}) => {
  const { report } = fixture(true);
  const b = await bridge(page);
  try {
    await page.goto("/");
    await page
      .getByRole("navigation")
      .getByRole("button", { name: /Assessment/ })
      .click();
    await page
      .getByRole("button", { name: "Export self-contained HTML" })
      .click();
    await expect(
      page.getByRole("status").filter({ hasText: "Saved immutable report" }),
    ).toBeVisible();
    const saved = resolve(
      root,
      "exports",
      `assessment-${report.id}-${report.sha256}.html`,
    );
    expect(readFileSync(saved, "utf8")).toBe(report.html);
    expect(staging()).toEqual([]);
    expect(b.calls).not.toContain("inspect_report_snapshot");
  } finally {
    await b.close();
  }
});
