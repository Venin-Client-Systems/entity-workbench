import { isTauri } from "@tauri-apps/api/core";

/** Browser development only. Native saves must use a typed core-generated export. */
export async function downloadExport(
  content: string,
  name: string,
  type: string,
): Promise<null> {
  if (isTauri())
    throw new Error("Native exports require the verified prepare/commit path.");
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return null;
}
