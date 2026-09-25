# Document controls with mixed processing history

The public graph API adds a fifth processing-input family to the existing durable
job catalogue. The document interface now positively selects parsing, image OCR,
image-region OCR and PDF-page OCR jobs. A graph job has no original-document byte
count and cannot enter document extraction, cancellation or retry controls.

This compatibility correction reuses the existing editable
[document-job instruments](DOCUMENT-JOBS.md#editable-design). It changes response
selection and explanatory text, not the visual system. No new graph workflow
frame, Figma edit or graph interface acceptance is claimed.

The catalogue still returns the newest 200 processing jobs across all families.
When the loaded window contains other jobs, the interface labels document rows
shown separately from processing jobs loaded and the whole-catalogue total. It
does not infer the number of document jobs outside that window. A graph-only
loaded window says that no document jobs are present in the loaded history;
an initial failed read says history is unavailable. Neither condition claims
that no documents have ever been processed.

Inspection responses must identify the selected document job before exposing
provenance or controls. Mutation acknowledgements must match its job ID and
request key. Queue acknowledgements must match the retained request key,
operation, original ID/hash/byte count and, for PDF OCR, page and DPI. A mismatched
reply leaves the recovery key intact. Recovery resubmits that exact request
rather than creating a second canonical job. Rust remains the canonical writer;
these interface guards bind displayed state to the selected request.

The final browser run passed 38 document, image, image-region and PDF workflow
tests. Six cases exercise this correction: real mixed history; a wrong graph
inspection; a different document inspection; a substituted queue acknowledgement
followed by exact-key recovery; initial catalogue refusal; and 200 real queued,
then cancelled graph jobs displacing 11 retained document jobs from the loaded
window. Queue recovery verifies a single added job and unchanged canonical state
on replay. Transport substitution is limited to the explicit negative cases;
mixed and full-window fixtures use actual Rust commands. The production
TypeScript/build check passed with the existing bundle-size warning.

The interface source was independently reviewed. Two adjacent response-identity
and false-empty-history findings were repaired before the final passing run.
This is browser workflow evidence, not native graph execution, new design
approval, packaged installation or a release gate.
