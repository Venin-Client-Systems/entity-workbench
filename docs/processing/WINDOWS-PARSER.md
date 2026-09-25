# Windows canonical document parsing — development integration

The Windows Rust parser adapter calls only the fixed AppContainer Java parser
recipe. It receives the selected runtime parent, canonical coordinator scratch,
verified original bytes and the coordinator cancellation token. The adapter
chooses the parser role and its `parser` runtime subdirectory; documents cannot
supply executable names, JVM flags, classpaths or filesystem grants.

Acceptance requires the outer request UUID, inner protocol UUID, raw output
digest, source digest and byte length to agree. The raw bounded reply is decoded
directly with the shared strict parser types before canonical validation. It
must not pass through a generic JSON map that erases duplicate fields.
The canonical coordinator is the sole publisher of immutable unreviewed
extraction records. The recipe cannot publish observations or modify evidence.

Typed worker outcomes retain unavailable runtime, invalid result, resource
limit, cancellation, cleanup failure and unverified termination distinctions.
Termination uncertainty takes precedence over cancellation and retains the
assignment. Unexpected API, execution I/O and exit errors remain worker failures;
they are not inferred resource limits. Verified cancellation discards even a
valid late success. All current AppContainer capabilities, ACLs and resource
budgets remain unchanged.

Host tests cover these adapter bindings and both canonical dispatch modes,
including restart and queue replay with no derivative after invalid output.
They do not prove native Windows execution. The native canonical campaign is
being integrated separately and must pass on the final combined source before
this adapter is described as verified on Windows. Search, OCR and rendering
application adapters are separate work; a parser integration cannot establish
them. Windows 11 clean installation and signed distribution remain unpassed.
