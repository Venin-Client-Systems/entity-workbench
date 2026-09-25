# Fictional discovery replay

These files exercise the offline benchmark validator/scorer. They contain no downloaded pages, real investigation data or live measurements.

- `synthetic-benchmark.v1.json`: 30 fictional tasks on ten `.example` publishers, with the same limits and scoring shape as the public reference benchmark.
- `make_synthetic_replay.py`: generates clearly marked fabricated originals, application-view receipts, access decisions, empty-corpus receipts and relevance labels into a new directory.

The generated expectation is 18 relevant tasks and nine useful expansions out of 30. It includes irrelevant successful results, no results, blocked access, technical failure and quota exhaustion. This reaches exactly the proposed threshold to test its boundary. It is not a forecast or product result. Every generated body declares itself synthetic; the scorer refuses to mix synthetic benchmark material with live run mode.

The executable tests under `scripts/tests/test_discovery_benchmark.py` mutate these fictional records to verify denominator preservation, incomplete measurement states, independent review, fixed seeds, hop/request/time constraints, artifact integrity and unsafe inputs. They make no network requests and do not invoke the application or substitute for later application-based measurements.
