# Resolver and subchannel lifecycle contract (FL-01 proposal)

**Status:** Proposed for maintainer review; no resolver or client-managed
balancer is shipped by this document. **Baseline:** `cd9d7bd8` on 2026-09-23.
**Depends on:** RT-03's absolute call deadline and RT-08's bounded drain.
Implementation is split across FL-02 (resolution), FL-03 (`pick_first`) and
FL-04 (`round_robin`) in [the task register](plan/tasks.json).

## Boundary and identity

The existing `Channel::connect`, `connect_tls`, `connect_lazy` and
`ChannelConfig::connections` keep their behavior: a pool for one `host:port`,
with DNS performed by the TCP dialer when connecting. `Target` still rejects
`dns:///`, `xds:///`, `https://` and other URI-shaped strings. DNS-at-connect
is **not** endpoint refresh, a resolver API or multi-endpoint balancing.
Resolver-managed channels will require an explicit opt-in constructor or
configuration; FL-03 owns its public signature.

Three identifiers must never be conflated:

| Identifier | Source | Can it change when DNS refreshes? |
|---|---|---|
| HTTP/2 `:authority` | Original `Target` or per-clone `Channel::origin` | No. A routing overlay does not change the dial destination or TLS verification name. |
| TLS server name / certificate identity | `ClientTls` configured for this channel | No. Every resolved IP receives the same verified name and `h2` ALPN requirement; an IP result cannot silently become the SNI name. |
| Dial address | A resolved `SocketAddr` (IP and port) | Yes, only for *new* connections selected from a current snapshot. |

The first resolver profile covers one DNS hostname/port and its resolved IPs,
not a fleet of unrelated authorities. TLS and HTTP authority stay stable
across updates. Unix sockets and already-connected `from_io` channels retain
their current path and non-redialable semantics.

## Proposed internal interface for FL-02

An injectable resolver returns a full `Resolution` snapshot: ordered,
deduplicated socket addresses, a successful-empty result distinct from an
error, and optional **observed** TTL. The internal async provider may use a
boxed future per refresh; no per-RPC allocation or new runtime dependency is
needed. A fake provider and Tokio's paused clock drive unit tests. No public
resolver trait is promised until the transport integration has a tested API.

The system `tokio::net::lookup_host` does not expose a DNS TTL. FL-02 must
report `ttl = None` for that provider and use a caller-specified refresh
interval; it must not invent an authoritative TTL from an IP address or a
socket connection. Configuration must explicitly bound `min_refresh`,
`max_refresh`, `refresh_without_ttl`, `retry_min`, `retry_max` and `max_stale`.
Reject zero/overflow/inverted bounds before starting a task. Clamp a real TTL
to the configured refresh bounds; a failed lookup uses bounded backoff rather
than being treated as an empty answer. No numerical default is approved here.

Only one refresh task belongs to each resolver-managed channel. Snapshot
publication is atomic and monotonic by generation, with a latest-value
notification rather than an unbounded queue. Simultaneous refreshes cannot
reorder generations. All timer and lookup work is cancelled when the last
channel owner shuts down; the provider and timer are joinable. Refresh
attempts, connections, pending picks and queued calls must remain subject to
the existing connection, RPC, byte and deadline budgets.

## Freshness and update rules

- A successful nonempty answer replaces the eligible **new-call** address
  list. Preserve resolver order, deduplicate exact `(IP, port)` entries, and
  treat a changed port as a different endpoint. A repeated identical answer
  does not restart healthy connections or reset their generation.
- A successful empty answer is authoritative: stop assigning new calls to
  removed addresses immediately. Without wait-for-ready, a new call fails
  `Unavailable`; with it, wait only until its existing absolute deadline.
  Do not turn empty into a stale-success fallback.
- A lookup failure preserves the last successful addresses only until
  `valid_until + max_stale`. `max_stale = 0` disables reuse. On expiry, no
  stale endpoint is eligible for new calls. The refresh loop continues with
  bounded backoff while the channel lives; no error is reported as a pass.
- Removing an endpoint prevents new picks, but existing streams retain their
  connection until completion or the configured bounded drain expires. A
  committed or in-flight RPC is not migrated or automatically replayed. A
  new replacement connection is admitted only within the global connection
  budget, not a fresh pool of `connections` for each returned IP.
- Preserve IPv4 and IPv6 in provider order. With a bound local address, only
  addresses in its family are eligible; if none remain, report the mismatch
  explicitly instead of retrying an unusable family forever. Without a local
  bind, failed address attempts may try the next current address within the
  *same* absolute dial/call deadline.

The freshness clock is monotonic. A wall-clock jump cannot extend an
expiration, and a late response from an older refresh cannot resurrect a
removed address. Resolver-error metadata is low-cardinality; raw addresses
must not become unbounded metric labels.

## Subchannel states and picking

FL-03 introduces one bounded state record per eligible address:
`Idle -> Connecting -> Ready`, with `TransientFailure`, `Draining` and
`Shutdown` as explicit transitions. A connection generation changes on
redial. A removed address transitions to `Draining`; after its deadline it
cannot be picked. Snapshot changes cannot swap the `SendStream` underneath
an already open RPC.

`pick_first` uses the first ready endpoint in resolver order; a failed or
still-connecting first address may give way to the next eligible address
without creating extra concurrent attempts beyond the dial budget. If none
is ready, a non-waiting call fails; a wait-for-ready call honors its existing
deadline and cancellation signal. FL-04's opt-in `round_robin` advances an
Arc-shared cursor across *ready* endpoints only. Neither policy retries a
committed call: transparent retry remains limited to the RT-01/02 proof of
non-execution, and future policy retries need FL-06's separate opt-in budget.

The existing direct channel and external-load-balancer deployment remain
supported. No xDS or service-config URI is implied by this contract.

## Deterministic timelines required before implementation closes

| Time / event | Required new-call behavior | Existing work and invariant |
|---|---|---|
| `t0`: A (IPv4), B (IPv6) arrive | Select by ready state and resolver order, with family filter if bound locally. | TLS verifies the configured name, not A/B's literal IP. |
| `t1`: same A/B snapshot | No needless redial or pick-order reset. | One refresh task and bounded connections remain. |
| `t2`: refresh fails before TTL | Use A/B only within the explicit stale budget; retry with capped backoff. | No false successful empty answer or deadline reset. |
| `t3`: stale deadline expires | New calls fail or wait under their own absolute deadline. | Already-open streams are not replayed. |
| `t4`: successful empty snapshot | No stale new picks, even if an old socket is still healthy. | Old streams drain within their grace period. |
| `t5`: B returns; A removed during an A stream | New calls use B only once ready. | A stream completes or is terminated by the bounded drain, never migrated. |
| `t6`: caller cancels during DNS/backoff | No further dial for that call; release permits promptly. | Resolver may continue for other channel owners. |
| `t7`: last channel owner shuts down | No refresh, reconnect or subchannel task remains. | Quiescent task/connection/byte counters are observed. |

FL-02 must prove these with a fake resolver and fake clock, including TTL
absence, errors, duplicate addresses, empty answers and IPv4/IPv6 filtering.
FL-03 adds real loopback and pinned-peer `pick_first_unary` evidence; FL-04
adds distribution and churn measurements at preregistered bounds. The FL-05
official ~540-second backoff exercise is **not** replaced by the short
fake-clock timeline.

**Review gate:** The maintainer must approve the refresh/staleness bounds,
state transitions and opt-in API before this proposal closes FL-01 or
unblocks FL-02. A design note is not production or official-gate evidence.
