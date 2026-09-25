"""Closed cancellation profile; shares the actual canonical receipt checks, never rewrites an outcome."""
from native_https_receipt import (
    SEED, ROBOTS, FIXTURE_SHA, FIXTURE_BYTES, PREFIX, EventDecodeFailure, decode, _validate,
)

POLICY = "fixed-owned-response-cancellation-v1"
TEST = "coordinator::native_https_proof::cancellation::native_durable_https_cancellation_campaign"


def validate(events, source, nonce, exit_code):
    _validate(events, source, nonce, exit_code, cancellation=True)
