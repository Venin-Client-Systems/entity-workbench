import { invoke, isTauri } from "@tauri-apps/api/core";
import type { DerivativeRef } from "./image-region-types";
export const MAX_RASTER_BYTES = 12_000_000 + 32;
export type RasterPixels = {
  width: number;
  height: number;
  pixels: Uint8Array;
};
/** Strict canonical P5 interchange only; never a general image decoder. */
export function parseRegionRaster(
  buffer: ArrayBuffer,
  width: number,
  height: number,
): RasterPixels {
  if (buffer.byteLength > MAX_RASTER_BYTES || buffer.byteLength < 12)
    throw new Error("Raster exceeds display bounds.");
  const bytes = new Uint8Array(buffer);
  const prefix = String.fromCharCode(...bytes.subarray(0, 32));
  const header = /^P5\n([1-9][0-9]{0,3}) ([1-9][0-9]{0,3})\n255\n/.exec(prefix);
  if (!header) throw new Error("Raster is not canonical grayscale P5.");
  const w = Number(header[1]),
    h = Number(header[2]);
  if (
    w > 8192 ||
    h > 8192 ||
    w * h > 12_000_000 ||
    w !== width ||
    h !== height ||
    buffer.byteLength !== header[0].length + w * h
  )
    throw new Error(
      "Raster dimensions or exact byte length differ from its result.",
    );
  return { width: w, height: h, pixels: bytes.subarray(header[0].length) };
}
export async function verifyRegionRaster(
  buffer: ArrayBuffer,
  reference: DerivativeRef,
  width: number,
  height: number,
) {
  if (
    reference.kind !== "canonical_pgm_v1" ||
    reference.bytes !== buffer.byteLength ||
    !/^[a-f0-9]{64}$/.test(reference.sha256)
  )
    throw new Error("Raster reference does not match its response.");
  const parsed = parseRegionRaster(buffer, width, height);
  if (!globalThis.crypto?.subtle)
    throw new Error(
      "Verified raster display requires WebCrypto; it is unavailable.",
    );
  const digest = [
    ...new Uint8Array(await crypto.subtle.digest("SHA-256", buffer)),
  ]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  if (digest !== reference.sha256)
    throw new Error("Raster checksum differs from the inspected derivative.");
  return parsed;
}
export async function readImageRegionRaster(
  extractionId: string,
): Promise<ArrayBuffer> {
  if (!/^[a-f0-9]{64}$/.test(extractionId))
    throw new Error("Invalid extraction identity.");
  if (isTauri()) {
    const result = await invoke<ArrayBuffer>("image_region_raster", {
      extractionId,
    });
    if (
      !(result instanceof ArrayBuffer) ||
      result.byteLength > MAX_RASTER_BYTES
    )
      throw new Error("Invalid binary raster response.");
    return result;
  }
  if (!import.meta.env.DEV)
    throw new Error("Open the native desktop to inspect retained rasters.");
  const response = await fetch("/api/image-region-raster", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ extraction_id: extractionId }),
    cache: "no-store",
  });
  if (!response.ok)
    throw new Error("Retained raster could not be verified or read.");
  if (response.headers.get("Content-Type") !== "application/octet-stream")
    throw new Error("Invalid raster response type.");
  const length = Number(response.headers.get("Content-Length"));
  if (
    !Number.isSafeInteger(length) ||
    length < 12 ||
    length > MAX_RASTER_BYTES ||
    !response.body
  )
    throw new Error("Raster response exceeds its display bound.");
  const reader = response.body.getReader();
  const result = new Uint8Array(length);
  let offset = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (offset + value.byteLength > length)
        throw new Error("Raster response exceeds its declared length.");
      result.set(value, offset);
      offset += value.byteLength;
    }
    if (offset !== length) throw new Error("Raster response was incomplete.");
  } catch (error) {
    await reader.cancel();
    throw error;
  }
  return result.buffer;
}
