# Adoption program: faster, and easy to integrate

Teams running prost and tonic services should be able to adopt pbrs one layer
at a time. They should not have to rewrite handlers, middleware, discovery,
TLS or build pipelines, and they should see clearly lower CPU and memory
cost on their real workloads. That is a program target, not a current claim.

**Coordinator:** Michael Ingley.
**Cards:** the `AD` lane plus adoption cards in other lanes, in
[`../world-class/tasks.json`](../world-class/tasks.json).
**Start work:** [current priorities](../../../TODO.md).
**Worker rules:** [worker protocol](../world-class/README.md#worker-protocol),
plus the program rules below.

## Where we stand

On 2026-09-29 we evaluated pbrs `18e36617` against prost 0.14.3 and tonic
0.14.6 on a private production-shaped service. It has deeply nested request
trees, responses that list entity records with `google.protobuf.Any`
payloads, and option-heavy schemas. Numbers are Linux callgrind instructions
and exact allocation counts. They are dev-loop evidence, not claim-grade.

| Area | Measured | Next |
|---|---|---|
| Transport only: prost messages on pbrs-grpc | 5–8% fewer instructions, 1–8% fewer allocations and 3–29% fewer allocated bytes per unary RPC than tonic | Real but small. Remove the code changes it needs (TC-29, TC-30) and grow the margin (AD-04). |
| Codec: decode without reading fields | 49–75% fewer instructions than prost, about 3 allocations per message | Lazy decode helps only when fields go unread. |
| Codec: decode, then read every field | 95% to 418% more instructions than prost | Deep and `Any`-heavy messages defeat the lazy path (PK-26, PK-27). |
| pbrs messages on either transport | 10% to 60% more instructions per RPC | Today the codec, not the transport, decides the result. |
| Code generation | 284k generated lines against prost's 1.8k for the same 20 files. Fields named `default`, `clone` or `serialize` do not compile. | GN-10, GN-11 |
| Integration | The TLS crypto provider is fixed. There is no `NamedService` or tonic connect-info. The Tower client is unary-only. The config-only `codec_path` route works but is undocumented. | TC-25 to TC-28 |

## Targets

SB-26 measures every target on public, production-shaped corpora in one
harness. Handler work is identical on both sides, and every handler reads the
whole message. Both peers run at their defaults unless a card says otherwise.
These thresholds steer the work. They become binding only when the maintainer
records them in the [benchmark contract](../../benchmark-contract.md).

Performance targets compare against prost 0.14 and tonic 0.14:

| ID | Target |
|---|---|
| P1 | Floor: no adoption cell costs more instructions or allocations than prost/tonic. |
| P2 | Codec: decoding and reading every field takes at most 0.8× prost's instructions and 0.5× its allocations on every corpus. Encoding a freshly built message takes at most 0.8× prost's instructions. |
| P3 | RPC, unary and streaming, with handlers that read whole messages: at most 0.7× the instructions and 0.6× the allocations per RPC. This holds for both the transport-only and full-native profiles. |
| P4 | Time to first RPC, cold and with TLS, is no slower than tonic's. |
| P5 | Generated source is at most 3× prost's size, and incremental `cargo check` takes at most 1.5× prost's time on the option-heavy corpus. |

Integration targets:

| ID | Target |
|---|---|
| I1 | Every legal `.proto` file compiles, including reserved-looking field names and option-heavy imports. |
| I2 | Swapping the transport needs no code changes: unmodified tonic-generated clients and servers, and existing Tower middleware, run over pbrs-grpc. |
| I3 | Swapping message types is a build-configuration change, and prost and pbrs messages coexist in one build. |
| I4 | Adopters bring their own TLS configs, crypto provider, certificate rotation, service discovery, load balancing, observability and request-context propagation. |
| I5 | Behavior differences from tonic that callers branch on are documented and, where useful, switchable. |
| I6 | Moving handlers from prost types to pbrs types needs no hand rewrite. |
| I7 | Mixed fleets interoperate, and each adoption step rolls back independently. |
| I8 | Integration APIs are semver-checked and supported, and adopters get a security and provenance packet. |

## Phases

Work top to bottom. Within a phase, cards run in parallel unless their JSON
lists a dependency. Use `python3 scripts/plan-status.py --lane AD` and
`python3 scripts/plan-status.py --card ID` for full contracts.

| Phase | Cards | Done when |
|---|---|---|
| 0. Measure what adopters run | SB-26 | Corpora and the four-profile matrix land, with a baseline recorded for every target. |
| 1. Unblock | GN-10, TC-25, PK-26 then PK-27, GN-11 | I1 holds, callers can inject TLS configs, and P1 holds for the codec. |
| 2. Swap the transport without code changes | TC-29, TC-30, TC-26, TC-27, CH-12, GF-12, TC-31 | I2, I4 and I5 hold, with tests. |
| 3. Swap the codec without rewrites | TC-28, TC-32, AD-02, PK-28 | I3 and I6 hold. |
| 4. Much better performance | AD-04 and the lane cards it files, CL-07, and existing RX, SV, CL and PK cards | P2 to P5 hold on SB-26. |
| 5. Adopter trust | AD-03, QG-08, QG-09, AD-01; existing QG-06, QL-01, QL-02, QL-05 | I7 and I8 hold. An independent adoption exercise passes using the AD-01 playbook. |

## Program rules

- **Keep inputs public.** Use public schemas and synthetic reproducers.
  Never commit private schemas, service names or captured payloads.
- **Read everything.** An optimization counts only when the SB-26 cells that
  read whole messages improve. A decode-only win does not close a card.
- **Do identical work.** Handler work, response bytes and read checksums
  must match across the compared profiles.
- **Keep adapters optional.** Integration adapters live behind opt-in
  features. The default dependency graph and pure-Rust profiles stay
  unchanged unless a decision record approves otherwise.
- **Escalate decisions.** Public API shape, thresholds, new dependencies and
  external actions need maintainer approval, as the worker protocol says.
