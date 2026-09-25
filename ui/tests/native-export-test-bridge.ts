import type { Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { resolve } from "node:path";
export type Call = { command: string; args: Record<string, any> };
export async function nativeExportBridge(
  page: Page,
  root: string,
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
