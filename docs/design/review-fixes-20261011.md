# PR22 review fixes and validation

These changes build on `8d52dd42daae1d71b488fb6b57fd63d5e1f39003`.
They address request sizing, schema feedback, provider retries, fenced domain
failure settlement, EXIF handling and repeated frozen-input hashing.

**Real PDF end-to-end acceptance was not executed.** The supplied attachment
could not be transferred into the execution environment: standard download
attempts failed with proxy CONNECT HTTP 403. No paid model calls were made.
**Automatic recovery after shutdown on the final queue attempt remains
unresolved.** Preserving the domain checkpoint does not revive its dead-lettered
queue envelope. Neither limitation is represented as a passing acceptance.

## Changes

### Request sizing and schema feedback

Sizing-only pending tool receipts reserve space without authorizing the larger
correction window. Real failed receipts retain their correction authority.
Tests exercise the actual fitting and paging helpers, including retaining the
last successful receipt; they do not run a complete live-provider tool turn.

Model-facing schema diagnostics group repeated errors with a total 8192-byte
bound, at most 24 groups and three sample paths per group. Omitted counts remain
visible. Complete submitted candidates and existing audit history are retained;
full diagnostics can be reconstructed with the pinned schema. There is no
second unbounded diagnostic store or relaxation of schema validation.

A synthetic 1000-empty-object submission previously produced 6000 diagnostics
and 700425 feedback bytes; grouped feedback is 1774 bytes in six groups. This
measures the feedback producer and a synthetic envelope, not the complete
production prompt or the archived real run.

### Durable bounded provider retries

Each logical request permits at most three physical sends. The host persists
attempts and earliest retry time before sending, reuses the saved wire bytes,
and preserves unknown usage reservations. Received responses replay without
another send; committed tools and successful Discover peers are not repeated.
Waiting retries do not globally block eligible peers. Retry-After supports
seconds and HTTP dates; invalid/overflowing or excessively distant deadlines
are rejected rather than silently shortened.

Transient network/provider errors and incomplete tool output are retryable.
Actual HTTP/SSE empty output, missing tool identifiers and incomplete argument
JSON now match the driver's incomplete-response classification. Policy refusal,
complete duplicate tool IDs, authentication/configuration errors and business
schema rejection do not gain blanket retries. Timeout retains usage already
observed; cancellation can still leave provider usage unknown.

The real local HTTP/SSE-to-driver test exercises HTTP 429 with Retry-After=2s,
an empty tool response and then success. It verifies identical request bytes,
three physical reservations, usage 7+12=19 and exactly one tool execution.
The delay differs from default first backoff so the header assertion is useful.

Production has no new cumulative token/cost quota. Per-request context limits,
resource limits, cancellation, transport timeout and bounded physical retries
remain in force. Private validation budgets are separate from production.
Unknown outcomes must not be treated as free: the provider may have processed
or charged for an attempt whose response was lost.

### Fenced domain failure settlement

Only the current live acquisition can mark a run failed on a final non-cancelled
queue attempt. Earlier failure and process shutdown cancellation release the
lease, retaining the checkpoint and making the domain run pending. Failure
before acquisition cannot change another run. DbJournal checks the SQL boolean
result of release/fail and reports rejected settlement fences explicitly.

The fresh bidding baseline includes the fenced failure function. This change
was tested on a disposable database; it is not an upgrade migration or a claim
that an existing database was updated.

### EXIF and hashing cost

EXIF orientation is applied once to decoded raw-image pixels before thumbnailing
or OCR preparation. Source bytes and raw object hashes remain unchanged;
decoded/rotated buffers retain allocation limits. Tests exercise actual JPEG
decode/render and an OCR callback, not external OCR quality or all subdivision
and overlap paths. PDF page coordinates and Office locators are unchanged.

A task-local scope computes the canonical frozen-input digest once per borrowed
input. Only that same live input address can reuse it; other inputs, nested
scopes and out-of-scope calls use canonical hashing. It is not a global cache,
uses no unsafe code and does not cache request schemas, messages or budgets.
Cancellation/panic cleanup, changed inputs and exact fixed-claim wire/state
comparison are covered. DbJournal owns a cloned input, so publication safely
falls back to hashing and does not receive this optimization's benefit.

| Scripted scenario | Before | After | Model calls | Actual digest computations |
| --- | ---: | ---: | ---: | ---: |
| Mixed native document | 184.41 s | 81.37 s | 66 -> 66 | 8160 -> 13 |
| S5 image transport | 373.45 s | 138.92 s | 87 -> 87 | 16264 -> 15 |

These are single-run synthetic measurements, not production latency promises.
Baseline runs had overlapping load. Printed protocol traces match; that is not
proof that all full wire requests are byte-identical, since production request
scopes are random. Separate fixed-claim tests assert exact wire and checkpoint
equality. S5 stops before publication; the mixed scripted test reaches Check.

## Validation and limits

Tests use synthetic inputs, local HTTP and owned disposable PostgreSQL/Redis
instances, with project/service credentials scrubbed from test commands.

| Validation | Result and scope |
| --- | --- |
| Bidding library before digest optimization | 484 passed, 24 ignored |
| Bidding regression after digest optimization | Combined disjoint coverage: 488 enabled tests passed after one retry fixture correction; 24 ignored. Not one all-green full invocation |
| Final HTTP/SSE follow-up | 2 passed after the subsequent transport classification fix |
| Knowledge library | 242 passed, 1 ignored |
| Worker library | 22 passed |
| Final PostgreSQL integration via Cargo | 10 passed, 0 ignored; actual DbJournal release/fail, cancellation checkpoint retention, reacquisition and stale lease rejection |
| Final native Redis fault suite | 5 passed, 0 ignored; includes the unresolved final-shutdown dead-letter boundary |
| Final strict Clippy | bidding, worker and platform, all targets/features, `-D warnings`; knowledge also passed earlier before the follow-up changes |
| Formatting and whitespace | cargo fmt and git diff checks passed |

The retry peer test originally assumed a 60ms delay could not expire while
CPU-heavy peer preparation ran. Its final fixture uses a future deadline and
cancels after two peers have durably committed, then advances only the saved
test deadline and resumes. It asserts exact retry bytes, preserved usage and
unchanged committed peers without a timing-sensitive order assumption.

An integration Cargo invocation initially failed linking an ancillary binary
with lld SIGBUS. A subsequent attempt explicitly failed with ENOSPC. Moving
regenerable incremental caches to available temporary storage resolved the
capacity blocker; the same targeted Cargo integration command with `-j 1`
then built and ran all 10 tests successfully. No source workaround or ignored
test was used. Earlier direct-binary execution is superseded by that successful
Cargo invocation. An existing proc-macro-error2 future-compatibility notice
remains unrelated to these changes.

## Unresolved shutdown recovery

Oxana 2.1.3 maps Worker success to ACK and every ordinary error to retry or
kill. A final shutdown cancellation therefore still dead-letters the envelope,
even though the domain checkpoint is preserved. The added real Redis test uses
a resurrectable job whose first delivery is its final permitted attempt. A
separately crashed control task successfully resurrects, proving the scanner
ran; the cancelled task remains dead until explicit revive. This tests the
queue contract, not the complete production adapter at retries=3.

Public retry callbacks change delay only. Inserting the same request-scoped
unique ID is skipped; replacing it lets the old executor ACK delete the new
record. Delete/reinsert or changing IDs introduces an ownership handoff window.
Parking a handler forces the existing shutdown timeout and does not handle
failed DB lease release. No such workaround, blanket revive, private Redis
mutation, dependency fork or background coordinator was added.

A sound fix needs an owner-fenced queue shutdown disposition that preserves
identity/retries and atomically returns the current envelope without ACK or
kill, plus explicit DB release outcome handling. Process shutdown must remain
distinct from user cancellation. Combined Redis+DB acceptance must demonstrate
final-attempt shutdown, restart and automatic continuation without lost
checkpoints, repeated committed peers or stale-owner writes, while ordinary
final errors remain terminal. That requirement is still unmet.

## Archived real-run evidence

The [authorized archive](../validation/fixed-033a2ce-20261011-01/README.md) belongs
to source revision `033a2ce`; the review baseline adds only evidence documents.
Its 27 calls never reached Check. Chapter writes assigned 109 requirements,
but receipts still showed 22 unbound tables, absent response slots and missing
fulfilment targets. Call 27 requested 165 evidence references against a maximum
of 128 and changed no state. No binding, slot or fulfilment submission followed.

The private validation runner then returned PRIVATE_WINDOW_BLOCKED with 60828
input tokens remaining. The largest archived single-request estimate including
output reserve was 127969, below 131072. This was not proof of exceeding the
normal context window. Long inter-request gaps do not by themselves identify
CPU hot functions; the synthetic hashing result is not proof of the archived
run's root cause. The complete original frozen checkpoint/wire is not available
for exact replay. The new real PDF acceptance remained blocked by attachment
transfer and must not be inferred from successful mocks or local transport tests.
