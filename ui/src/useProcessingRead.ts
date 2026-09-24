import { useEffect, useState } from "react";
import { command } from "./api";

/** Serial local reads. Replacement requests and unmount invalidate all earlier replies. */
export function useProcessingRead<T>(
  action: string,
  key: string | null,
  enabled = true,
  poll = true,
) {
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = async () => {
      try {
        const args: Record<string, unknown> = { action };
        if (key !== null)
          args[
            action === "inspect_extraction" ||
            action === "inspect_image_extraction" ||
            action === "inspect_pdf_extraction"
              ? "extraction_id"
              : "job_id"
          ] = key;
        const next = await command<T>(args);
        if (current) {
          setValue(next);
          setError("");
        }
      } catch (cause) {
        if (current) setError(String(cause));
      } finally {
        if (current) {
          setLoading(false);
          if (poll) timer = setTimeout(() => void read(), 1500);
        }
      }
    };
    void read();
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [action, key, enabled, poll, generation]);
  return {
    value,
    setValue,
    error,
    loading,
    refresh: () => setGeneration((n) => n + 1),
  };
}
