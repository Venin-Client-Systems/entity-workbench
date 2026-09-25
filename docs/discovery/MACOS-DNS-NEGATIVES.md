# macOS negative DNS observations

This is a private resolver correction for EW-22/#25. It adds no public command, collection activation, new destination, Windows behavior or dependency. **No native queries have run against this change.** The preceding [failed native campaign](verification/native-transport-macos.json) remains byte-identical and continues to show four passing cases and one unmet negative-callback requirement.

## Primary contract and chosen semantics

The installed macOS SDK `dns_sd.h` documents `kDNSServiceFlagsReturnIntermediates` (0x1000) as enabling delivery of intermediate negative results, which remain part of a continuing query. With that flag absent, NXDomain errors are not returned. The same header says that callback fields other than the error are undefined when the error is nonzero, and that `MoreComing` describes queued batching rather than completion of every address family or the full RRset. The exact header identity is retained in the preceding source-bound report. Apple's corresponding [public API header](https://github.com/apple-oss-distributions/mDNSResponder/blob/main/mDNSShared/dns_sd.h) is available for review.

Apple's [client implementation](https://github.com/apple-oss-distributions/mDNSResponder/blob/main/mDNSShared/dnssd_clientstub.c) also explains that `DNSServiceGetAddrInfo` exposes A/AAAA results rather than CNAME referral records. Its current construction of an error sockaddr is an implementation detail, not permission to inspect fields that the public contract declares undefined. This change does not depend on that sockaddr or implement a CNAME parser.

The resolver sets only `ReturnIntermediates` on the existing single owned dual-family subscription. It does not request a shared connection, force multicast, change interfaces, or add another lookup.

| Observed callback/state | Behavior |
| --- | --- |
| `NoSuchRecord` (-65554) | Remember that a negative was observed. Do not inspect flags/address/family, clear valid candidates, close a batch, or conclude that both families are absent. |
| Any other callback error | Fail with Network; previously observed candidates cannot hide the failure. |
| Successful add or removal | Validate the address under the existing public-IP policy, including removal notifications. Maintain the exact active set and the existing candidate/callback ceilings. |
| Successful callback ends a batch with candidates present | Return that bounded observed snapshot after owned deallocation and a final stop check. |
| Empty batch or removal leaves no candidates | Continue the existing subscription within its original bounds. A later family/addition may still produce usable candidates. |
| Internal five-second DNS stage expires with retained candidates | Return only those currently active, validated candidates; removed candidates remain absent. This does not establish a complete batch/RRset. |
| Internal stage expires without candidates, with an observed negative | Return generic Network. This is no usable candidate observed within the stage, not an authoritative total NXDOMAIN claim. |
| Internal stage expires without candidates or a negative | Return Timeout. |
| Caller cancellation, overall deadline or backwards clock | These always take priority over internal-stage fallback, including after deallocation. |

The candidate method remains `macos_dns_service_observed_batch` with `authoritative_complete_set: false`. It names the observed candidate history, not a promise that all batches or address families completed. The return boundary can be a successful batching marker or the internal DNS-stage bound. Every successful callback already observed must satisfy policy before a snapshot can escape. An error callback's other fields are also omitted from test diagnostics, represented by a null flags value.

Owned cleanup stays serialized. The resolver collects its result, deallocates the subscription, then checks the caller's stop conditions again before returning either candidates or a failure. Trusted OS calls are still not forcibly preempted; local operation completion makes no claim about shared DNS-daemon activity.

## Synthetic verification and later native scope

Test-first callback regressions on the prior code produced one pass and four failures: a negative poisoned a later valid family, prevented normal removal/add processing, bypassed meaningful negative-notification limits, and hid the classification of a later unsafe successful callback. The new cases cover negatives before and after public IPv4/IPv6 results, deliberately invalid error flags/addresses, successful add/removal order, both address and callback bounds, generic fatal errors after positives, empty/bounded snapshot decisions, and cancellation/deadline/clock precedence. All use synthetic callbacks and no native lookup.

The opt-in native harness is versioned `fixed-three-subscriptions-no-http-v2`. Its five cases, two fixed names and maximum of three subscriptions are unchanged. The overall execution window is now distinct from the five-second internal DNS-stage boundary, within the same twenty-second campaign and thirty-second outer process bound. A future negative case requires an actual `NoSuchRecord` callback, generic Network after the internal stage, and observed subscription deallocation. It does not demand immediate termination or treat one-family NODATA as total NXDOMAIN. The earlier campaign/report is not reinterpreted under this policy.

Before any further native run, the changed source, exact query scope and expected observations require integration review. HTTPS remains absent. Actual Windows execution, complete RRsets, live durable collection and release acceptance remain unproved.
