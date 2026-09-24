import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { CitationReadLane as ReadLane } from "./citation-read-lane";
import type { Transaction } from "./types";
import {
  validateBalances,
  type Balances,
  type BalanceState,
} from "./transaction-ledger-types";

export function useTransactionBalances(
  rows: Transaction[] | undefined,
  revision: number,
  retry = 0,
) {
  const key = JSON.stringify({
    revision,
    retry,
    rows:
      rows?.map((row) => ({ id: row.id, expected_version: row.version })) ??
      null,
  });
  const latest = useRef(key);
  latest.current = key;
  const [lane] = useState(() => new ReadLane<Balances>());
  const [result, setResult] = useState<{ key: string; value: Balances } | null>(
    null,
  );
  const [error, setError] = useState<{ key: string; message: string } | null>(
    null,
  );
  useEffect(() => {
    lane.open();
    return () => lane.close();
  }, [lane]);
  useEffect(() => {
    let active = true;
    lane.clearPending();
    const wanted = JSON.parse(key).rows as
      | { id: string; expected_version: number }[]
      | null;
    if (!wanted?.length) return;
    void lane
      .read(() =>
        command<Balances>({
          action: "read_transaction_balances",
          request: { rows: wanted },
          expected_revision: revision,
        }),
      )
      .then((value) => {
        if (!active || latest.current !== key) return;
        validateBalances(value, wanted, revision);
        setResult({ key, value });
        setError(null);
      })
      .catch((cause) => {
        if (active && latest.current === key)
          setError({ key, message: String(cause) });
      });
    return () => {
      active = false;
      lane.clearPending();
    };
  }, [key, revision, lane]);
  return {
    rows: result?.key === key ? result.value.rows : null,
    error: error?.key === key ? error.message : "",
  };
}
export function BalanceLabel({
  balance,
  unavailable,
}: {
  balance?: BalanceState;
  unavailable: boolean;
}) {
  if (unavailable) return <small>Balance check unavailable</small>;
  if (!balance) return <small>Checking source-order balance…</small>;
  if (balance.state === "no_balance") return <small>No retained balance</small>;
  if (balance.state === "no_prior_balance")
    return <small>No prior source balance</small>;
  return (
    <span
      className={`pill ${balance.reconciled ? "accepted" : "warning"}`}
      title={`Difference ${balance.difference}; ${balance.contributing_row_count} source-order contributing rows; previous ${balance.previous_id} version ${balance.previous_version}`}
    >
      {balance.reconciled
        ? "Balance reconciled"
        : `Balance mismatch · ${balance.difference}`}
    </span>
  );
}
