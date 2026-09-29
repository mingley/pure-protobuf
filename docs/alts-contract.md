# ALTS provider decision and proof boundary (EX-18)

Decision: no ALTS adapter now. There is no reviewed pure-Rust ALTS
record layer or handshaker client, the pinned official runner has no
Rust ALTS procedure, and ALTS stays an explicit transport boundary
(see [the gRFC map](grfc.md): Google-internal transport; JWT/mTLS
cover portable auth). ALTS must never be labeled or configured as
ordinary TLS. The `alts_credentials` gate stays `unsupported`, and
EX-19 stays blocked behind the leaf cards below plus EX-04.

**Status:** Decided by the coordinator (EX-18, 2026-09-29).
**Baseline:** `9a82b511`. **Companion:**
[the call-credential contract](credentials-contract.md) (EX-03; D11
already admits ALTS as a secure transport only once EX-18/EX-19
approve it — that approval is not given here).

## Pinned runner requirements

Verified against the vendored pin
`third_party/grpc@d1487957db6658bc532b72871775148229836627`
(`tools/run_tests/run_interop_tests.py`), which matches the
`alts_credentials` procedure source in `tests/interop/cases.json`:

- ALTS is a transport axis (`--use_tls=false --use_alts=true`), not a
  separate case list.
- ALTS clients and servers are restricted to `java`, `go`, `c++`,
  and `python`. Rust is not in the matrix: there is no official ALTS
  peer procedure for a Rust implementation at this pin.
- The runner provisions no handshaker: no handshaker address flag or
  setup exists in the runner. Runs therefore depend on the ambient
  handshaker environment of the test deployment (GCE-provided
  handshaker).

## Peer identity (not TLS identity)

ALTS peers authenticate as service accounts through mutual
handshake, not X.509 certificate chains verified against roots.
Identity, authorization, and audit text must say ALTS service
identity, never TLS server name, certificate identity, or mTLS.
`ClientTls` constructors do not qualify as ALTS and must never be
presented as an ALTS-compatible option.

## Cryptographic and provider review

No Rust ALTS record layer or handshake implementation is reviewed or
approved. A 2026-09-29 crates.io search surfaced no maintained
pure-Rust ALTS provider (top matches are unrelated name collisions);
the reference remains the C++ TSI implementation
(`src/core/tsi/alts/` in grpc/grpc) plus the ALTS whitepaper. Any
future provider — reused crate or new implementation — needs a full
review (record framing, nonce/key discipline, handshake state
machine, memory safety, MSRV, license, transitive graph) before EX-19
may name it. That review is leaf card EX-18a.

## External handshaker trust

The handshaker is a privileged local service whose assertions define
the peer's service identity. Trusting it means trusting the platform
that provides it. Requirements for any future qualification:

- a documented handshaker endpoint for the test deployment (address,
  transport, and which identities it may assert);
- isolation proof that only the approved handshaker is reachable
  (no ambient-daemon confusion with a test double);
- failure semantics: handshaker unreachable, slow, or returning
  unknown identities must fail calls loudly, never fall back to
  plaintext or to a different credential.

## Platform and access requirements

ALTS qualification needs a GCE-class environment with a real
handshaker, service accounts for both peers, and network access
between them. Per the plan contract, real credential/cloud runs need
separate operator approval and remain blocked without it. Fake local
handshakers prove adapter mechanics only and are never interop
evidence. That platform proof is leaf card EX-18b.

## Leaf cards and gate

- EX-18a: review and select a Rust ALTS record layer (reuse or
  bounded new implementation; full cryptographic and supply-chain
  review).
- EX-18b: handshaker client plus platform proof (documented
  endpoint, isolation, failure semantics, operator-approved
  environment).
- EX-19 keeps its dependencies (EX-18, EX-04) and additionally waits
  for EX-18a/EX-18b; it must not start on an unreviewed provider.
- `tests/interop/cases.json` `alts_credentials` stays
  `unsupported` with this contract cited; the full-profile gate
  stays blocked.
