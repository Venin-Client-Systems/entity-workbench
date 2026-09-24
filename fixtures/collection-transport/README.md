# Synthetic local TLS fixtures

`ca.der`, `server.der` and `server-test-key.der` exist only for Rust unit-test
servers bound to an ephemeral loopback port. The leaf identity is
`collection.invalid`. The leaf private key is deliberately public test material,
not an account credential or a production signing key. The CA signing key was
discarded after fixture creation. These certificates must never be installed in
an OS trust store or used by a released application.

The test-only endpoint configuration adds this CA to its one temporary reqwest
client. Production code has neither the endpoint override nor this trust input.
Negative tests retain certificate validation: an untrusted CA or wrong hostname
must fail before the server receives an HTTP request.

Fixtures were generated locally with OpenSSL (`req`, `x509`, `pkcs8`); generation
made no network request and is not part of application execution. Certificates
are valid for ten years from generation. Their deliberate expiry eventually
requires replacing this test material rather than disabling certificate checks.
