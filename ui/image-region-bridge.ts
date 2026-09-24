import { spawn } from "node:child_process";
import { resolve } from "node:path";
import type { ViteDevServer } from "vite";
const MAX_BYTES = 12_000_000 + 32;
/** Development-only canonical raster reader. Never included in desktop assets. */
export function registerImageRegionBridge(server: ViteDevServer) {
  server.middlewares.use("/api/image-region-raster", (req, res) => {
    res.setHeader("Cache-Control", "no-store");
    const reject = (status: number) => {
      if (!res.writableEnded && !res.destroyed) {
        res.statusCode = status;
        res.end("Retained raster unavailable");
      }
    };
    if (
      req.method !== "POST" ||
      req.headers.host !== "127.0.0.1:1420" ||
      req.headers.origin !== "http://127.0.0.1:1420" ||
      req.url !== "/"
    ) {
      reject(403);
      return;
    }
    let body = "",
      size = 0,
      invalid = false;
    req.on("data", (chunk) => {
      size += chunk.length;
      if (size > 256) {
        invalid = true;
        reject(413);
      } else body += chunk;
    });
    req.on("end", () => {
      if (invalid || req.aborted || res.destroyed) return;
      let id: string;
      try {
        const value = JSON.parse(body);
        if (
          !value ||
          Object.keys(value).length !== 1 ||
          typeof value.extraction_id !== "string" ||
          !/^[a-f0-9]{64}$/.test(value.extraction_id)
        )
          throw new Error();
        id = value.extraction_id;
      } catch {
        reject(400);
        return;
      }
      const child = spawn(
        resolve("target/debug/ew-dev"),
        [
          "read-image-region-raster",
          resolve("artifacts/synthetic-ui-workspace"),
          id,
        ],
        { stdio: ["ignore", "pipe", "pipe"] },
      );
      let bytes = 0,
        stderr = 0,
        failed = false;
      const chunks: Buffer[] = [];
      const stop = () => {
        failed = true;
        child.kill("SIGKILL");
      };
      const timer = setTimeout(stop, 10_000);
      const disconnected = () => {
        if (!res.writableEnded) stop();
      };
      res.on("close", disconnected);
      child.stdout.on("data", (chunk: Buffer) => {
        bytes += chunk.length;
        if (bytes > MAX_BYTES) stop();
        else if (!failed) chunks.push(chunk);
      });
      child.stderr.on("data", (chunk: Buffer) => {
        stderr += chunk.length;
        if (stderr > 1024) stop();
      });
      child.on("error", () => {
        failed = true;
      });
      // Wait for close, including pipe closure, before publishing bytes or reporting failure.
      child.on("close", (code) => {
        clearTimeout(timer);
        res.off("close", disconnected);
        if (failed || code !== 0 || bytes < 12 || stderr !== 0) {
          reject(400);
          return;
        }
        if (res.destroyed) return;
        res.setHeader("Content-Type", "application/octet-stream");
        res.setHeader("Content-Length", bytes);
        res.end(Buffer.concat(chunks, bytes));
      });
    });
  });
}
