import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export async function downloadExport(content: string, name: string, type: string): Promise<string | null> {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  if (!isTauri()) {
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return null; // A browser download event is owned by the browser, not native IPC.
  }
  let dispose = () => {};
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    let resolveResult!: (value: string) => void;
    let rejectResult!: (reason: Error) => void;
    const completed = new Promise<string>((resolve, reject) => {
      resolveResult = resolve;
      rejectResult = reject;
    });
    dispose = await listen<{ url: string; success: boolean; name: string | null }>("workbench-download", ({ payload }) => {
      if (payload.url !== url) return;
      if (payload.success && payload.name) resolveResult(payload.name);
      else rejectResult(new Error("The native export did not complete. No saved file is confirmed."));
    });
    timeout = setTimeout(() => rejectResult(new Error("Export completion was not confirmed. Check Downloads before retrying.")), 120_000);
    anchor.click();
    return await completed;
  } finally {
    if (timeout) clearTimeout(timeout);
    dispose();
    URL.revokeObjectURL(url);
  }
}
