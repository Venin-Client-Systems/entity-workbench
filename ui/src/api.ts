import { invoke, isTauri } from "@tauri-apps/api/core";
export async function command<T>(command: Record<string, unknown>): Promise<T> {
  if (isTauri()) return invoke<T>("workbench", { command });
  if (import.meta.env.DEV) {
    const result = await fetch("/api/workbench", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(command),
    });
    const value: unknown = await result.json();
    if (!result.ok)
      throw new Error(
        typeof value === "object" && value && "error" in value
          ? String(value.error)
          : "Workspace command failed",
      );
    return value as T;
  }
  throw new Error("Open the native desktop application to access a workspace.");
}
