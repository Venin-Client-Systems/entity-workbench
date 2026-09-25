# Synthetic acceptance inputs

`catalogue.v1.json` maps ten small, fictional inputs to the ten release scenarios. File size and SHA-256 are verified by `scripts/release_gate.py --check-policy` before policy validation succeeds.

Inputs include text and a deliberately simple bitmap statement; an OCR amount correction; namesakes with conflicting dates and leading-zero reference namespaces; copied source origins; historical address/merchant candidates; repeated purchases, overlap, refund, transfer and multiple-currency rows; a fictional discovery chain; hostile strings; and recovery/correction events.

These are **initial regression inputs and expected assertions**, not executed end-to-end scenarios or realistic large-corpus benchmarks. The scan contains the synthetic amount `18.00`; the review JSON deliberately includes a mistaken extraction of `180.00`. The transaction table retains legitimate repeats and an overlapping row separately. Import/extraction harnesses must preserve their source anchors and test those distinctions.

The discovery chain uses reserved `.example` domains and must never be submitted as live collection evidence. Hostile strings are test data, never executable setup instructions. Clean-install, sandbox, signing, recovery, performance and discovery scenarios require their actual platform-specific procedures in addition to this catalogue. A valid manifest does not pass those gates.

When changing fixture bytes, update their size and SHA-256 in the catalogue through review. A changed catalogue invalidates release-candidate acceptance bindings. Do not retrofit a passing expected result after observing a failure.
