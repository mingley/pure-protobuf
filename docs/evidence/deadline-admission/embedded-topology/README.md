# Private embedded H2 topology qualification

Source `604f4c2e5b1e33050bf859a97dc69fb7ae791f3a` passes the isolated topology
gates below. Native callers still use the original ready/send operations.
This evidence does not qualify native owned-admission integration, production
shipping, or runtime/codegen performance.

The private crate-root module embeds pinned h2 0.4.19, preserving its MIT license
and per-file original/candidate/transformation hashes. Only the five declared
admission files differ from the pinned original algorithm. Namespace, edition,
disabled upstream feature/fixture gates, documentation fences and individually
named lint adaptations are recorded in provenance. The native transport's
unsafe forbid remains intact; the imported HPACK UTF-8 view has one narrow
validated exception, with actual constructor/lifetime Miri tests.

Registry h2 remains optional under Tower and present for independent peers.
Default normal dependencies decrease from 60 to 59; Tower stays 68 and Tonic
stays 90. All 172 workspace package name/version/source/checksum tuples remain
unchanged. Only pbrs-grpc dependency edges change. The default profile drops
inert indexmap/tokio-util default markers while retaining std/codec/io; the
Tower/Tonic feature sets stay unchanged.
The 172 workspace tuples comprise 166 registry packages and six local packages.

| Gate at source 604f4c2e | Result |
| --- | --- |
| Strict all-feature library/test Clippy and formatter | Pass |
| Strict Rust 1.88 Tonic rustdoc | Pass |
| Rust 1.88 Tonic doctests | 177 passed, 1 existing ignore |
| Rust 1.85 default library units | 526 passed, 1 upstream existing ignore |
| Rust 1.88 Tonic library units | 544 passed, 1 upstream existing ignore |
| Pinned nightly strict-provenance Miri | 3 actual UTF-8 invariant tests passed |
| Offline locked cargo package in independent staging | Pass on first attempt |
| Default/Tower/Tonic independent path consumers | Pass |
| Default/Tower/Tonic normalized unpacked-package consumers | Pass |
| Same Rust 1.85 unpolled layout probe | All reported byte deltas zero |

Each external consumer executes all four native RPC shapes. Tower additionally
executes a typed unary Service call. Tonic checks the exact Channel Service
contract using tonic Body and tonic Status, and the raw registry-h2/Tonic error
reason/message identity. Packaged source contains all 54 embedded Rust members
and the exact upstream license/provenance. There is no backend patch. The only
unpacked-consumer patch points the local pbrs core version at its unpacked
package before registry publication. Consumer registry pins remain members of
the source lock's unchanged tuple set.

The initial path/default and path/Tower runs recorded repository source, tools,
commands and passing output, but did not capture per-run before/after hashes of
the external consumer inputs. Their `consumer-inputs/path-initial` files were
retained after those runs and before correcting the Tonic-only assertion, which
was disabled for both earlier profiles. This is a fixture-capture limitation;
the later corrected Tonic and unpacked-package runs have separate direct-input
snapshots. No before/after external-input guard is claimed for the first two
runtime runs.

Status unit oracles preserve the existing private transport-wrapper behavior:
Tonic sees UNKNOWN and the original display because the underlying H2 Error has
no std Error source. Tonic's top-level raw registry-h2 mapping remains a separate
reason-based oracle. Native pre-header/post-dispatch/send mappings and retry
evidence are checked separately; no wrapper reason remapping is introduced.

The preserved original-backend ELF belongs to source
`76d43b7d064ee63d2231fb0be2e31128b0f165b0`. Both it and the final ELF use the
same verified Rust 1.85 compiler and Cargo binaries. Their reported unpolled
sizes are acquire 8184 B, grab 8784 B, handshake 7816 B and public Call wrapper
32 B. This excludes the boxed RPC body, heap state and execution/allocation
costs. The first zero-test listing is retained as an incorrect selector that
omitted the pool module; the corrected exact selector executes one test in
each ELF.

The archive retains all raw red stages, fixes and phase-specific source maps.
These include imported-source Clippy diagnostics, the two unfulfilled main gzip
test expectations later removed by the coordinator, an offline sysroot bootstrap
dependency miss, a conservative apparent-cache guard stopping a Tonic consumer
build, and an incorrect external fixture Status bound. Only the fixture was
corrected for the latter. Completed own caches were inventoried before bounded
reclamation; qualified ELFs, source/tool identities, raw logs and the pinned
sysroot were retained. Earlier passing counts qualify their earlier source maps,
not the final bytes.

Remaining integration requirements are request-timeout refresh before each
actual owned-admission poll, native cancellation/deadline/hostile-peer tests,
and measured client/server/codegen performance. The external HPACK fixtures and
fuzz suite absent from the registry archive remain explicitly unrun; one
upstream ignored unit and one existing ignored native doctest remain ignored.
The backend wake-all strategy has no FIFO guarantee. Tower/Tonic compile both
backend copies, and embedding couples backend rebuilds to kernel rebuilds.

[proof.json](proof.json) maps the exact passing gate records and limits.
[raw.tar.gz](raw.tar.gz) contains source inputs, normalized packages, external
consumer fixtures, tool/source/log records, qualified ELFs and the separately
sealed independent review. [members-sha256.json](members-sha256.json) was verified
against every archived regular member and its corresponding input bytes.
