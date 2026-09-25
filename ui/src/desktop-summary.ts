import type { DesktopSummaryResponse } from "./types";

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const count = (value: unknown) =>
  Number.isSafeInteger(value) && Number(value) >= 0;

/** Check the distinct transport boundary. Never synthesize absent full arrays. */
export function readDesktopSummary(value: unknown): DesktopSummaryResponse {
  if (
    !record(value) ||
    value.schema_version !== 1 ||
    !record(value.workspace) ||
    !record(value.analysis)
  )
    throw new Error("Unexpected desktop summary response.");
  const w = value.workspace,
    a = value.analysis;
  if (
    !Number.isSafeInteger(w.schema_version) ||
    Number(w.schema_version) < 1 ||
    !count(w.revision) ||
    !count(w.review_decision_count) ||
    "transactions" in w ||
    "decisions" in w ||
    ![
      "entities",
      "evidence",
      "observations",
      "assertions",
      "addresses",
      "locations",
      "leads",
      "jobs",
      "findings",
      "hypotheses",
      "merges",
      "identity_decisions",
      "reports",
      "statement_profiles",
      "statement_imports",
    ].every((key) => Array.isArray(w[key])) ||
    ![
      "transaction_count",
      "duplicate_candidate_row_count",
      "balance_check_count",
      "balance_discrepancy_count",
    ].every((key) => count(a[key])) ||
    !record(a.review_counts) ||
    !["accepted", "pending", "rejected", "deferred"].every((key) =>
      count((a.review_counts as Record<string, unknown>)[key]),
    ) ||
    !Array.isArray(a.totals) ||
    !a.totals.every(
      (row) =>
        record(row) &&
        ["currency", "credits", "debits", "net"].every(
          (key) => typeof row[key] === "string",
        ) &&
        count(row.included_count) &&
        count(row.excluded_transfer_count),
    )
  )
    throw new Error(
      "Invalid desktop summary fields; workspace was not replaced.",
    );
  const reviewTotal = Object.values(
    a.review_counts as Record<string, number>,
  ).reduce((sum, value) => sum + value, 0);
  if (
    reviewTotal !== a.transaction_count ||
    Number(a.balance_discrepancy_count) > Number(a.balance_check_count) ||
    Number(a.balance_check_count) > Number(a.transaction_count) ||
    Number(a.duplicate_candidate_row_count) > Number(a.transaction_count)
  )
    throw new Error(
      "Inconsistent desktop summary counts; workspace was not replaced.",
    );
  return value as unknown as DesktopSummaryResponse;
}
export function isBackupResult(value: unknown): value is { backup: string } {
  return (
    record(value) &&
    Object.keys(value).length === 1 &&
    typeof value.backup === "string" &&
    value.backup.length > 0
  );
}

/** Revisions are comparable only within the current workspace. A future
 * workspace switch must reset this state with explicit workspace identity. */
export function retainNewestSummary(
  current: DesktopSummaryResponse | null,
  next: DesktopSummaryResponse,
): DesktopSummaryResponse {
  return current && current.workspace.revision > next.workspace.revision
    ? current
    : next;
}
