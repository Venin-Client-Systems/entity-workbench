import type { Transaction } from "./types";
export type Delimiter = "comma" | "semicolon" | "tab";
export type ValueMapping =
  | { kind: "column"; column: string }
  | { kind: "constant"; value: string };
export type AmountMapping =
  | { kind: "signed"; column: string; positive_is_debit: boolean }
  | { kind: "debit_credit"; debit: string; credit: string };
export type StatementMapping = {
  delimiter: Delimiter;
  date_format: "iso" | "day_first" | "month_first";
  number_format: "dot_decimal" | "comma_decimal";
  row_order: "oldest_first" | "newest_first";
  date: string;
  posting_date: string | null;
  description: string;
  amount: AmountMapping;
  balance: string | null;
  account: ValueMapping;
  currency: ValueMapping;
};
export type StatementProfile = {
  id: string;
  name: string;
  mapping: StatementMapping;
  created_at: string;
};
export type StatementImportRecord = {
  evidence_id: string;
  mapping: StatementMapping;
  profile_id: string | null;
  transaction_ids: string[];
  imported_at: string;
};
export type StatementSample = {
  sha256: string;
  headers: string[];
  sample_rows: string[][];
  suggested_mapping: StatementMapping;
};
export type StatementPreview = {
  workspace_revision: number;
  sha256: string;
  preview_token: string;
  already_imported: boolean;
  total_rows: number;
  valid_rows: number;
  invalid_rows: number;
  balance_mismatches: number;
  rows: {
    source_row: number;
    original_amounts: [string, string][];
    transaction: Transaction | null;
    error: string | null;
  }[];
  issues: { source_row: number; message: string }[];
  rows_truncated: boolean;
  issues_truncated: boolean;
};
