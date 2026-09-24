import { command } from "./api";
import {
  digest,
  type LedgerScope,
  type Matching,
  type TransactionExport,
} from "./transaction-ledger-types";
/** Complete scope export is intentionally distinct from a bounded visible page. */
export async function readTransactionExport(
  scope: LedgerScope,
  revision: number,
  count: number,
  matching: Matching,
) {
  if (!globalThis.crypto?.subtle)
    throw new Error(
      "WebCrypto is unavailable; export integrity could not be verified.",
    );
  const request = {
    query: scope.query,
    filter: scope.filter,
    order: scope.order,
  };
  const value = await command<TransactionExport>({
    action: "export_transactions",
    request,
    expected_revision: revision,
  });
  const sameFilter =
    value.request &&
    Object.keys(request.filter).every(
      (key) =>
        value.request.filter[key as keyof typeof request.filter] ===
        request.filter[key as keyof typeof request.filter],
    );
  if (
    value.schema_version !== 1 ||
    value.workspace_revision !== revision ||
    !value.request ||
    value.request.query !== request.query ||
    value.request.order !== request.order ||
    !sameFilter ||
    value.matching.algorithm !== matching.algorithm ||
    value.matching.unicode_version.join(".") !==
      matching.unicode_version.join(".") ||
    !digest(value.query_sha256) ||
    !digest(value.sha256) ||
    value.row_count !== count ||
    !Number.isSafeInteger(value.bytes) ||
    value.bytes < 0 ||
    value.bytes > 256 * 1024 * 1024 ||
    typeof value.json !== "string"
  )
    throw new Error(
      "Complete transaction export identity, scope or count did not match.",
    );
  const bytes = new TextEncoder().encode(value.json);
  if (bytes.length !== value.bytes)
    throw new Error("Transaction export byte length did not match.");
  const hash = Array.from(
    new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (n) => n.toString(16).padStart(2, "0"),
  ).join("");
  if (hash !== value.sha256)
    throw new Error("Transaction export digest did not match.");
  const rows: unknown = JSON.parse(value.json);
  if (!Array.isArray(rows) || rows.length !== value.row_count)
    throw new Error(
      "Complete transaction export row count did not match its content.",
    );
  return value;
}
