<!-- SB-14 perf-PR evidence template. Required sections for every PK, GN,
CL, SV, H2 and RX optimization PR per worker protocol rule 3. Non-perf
PRs: delete the perf sections and keep Correctness + Rollback. -->

## Hypothesis

One sentence: what gets faster, on which cells/categories, and why.

- Target cells:
- Target categories:

## Dev-loop evidence (before / after)

Commands run (base SHA first, then this change):

```sh
devloop run --cells <a,b> --out target/devloop/base.json   # base SHA
devloop run --cells <a,b> --out target/devloop/head.json   # this change
devloop compare --baseline target/devloop/base.json --current target/devloop/head.json
```

- Before JSON:
- After JSON:
- `compare` verdict (win rule):

## Profile evidence

```sh
./scripts/profile.sh --cell <hottest-cell>   # base SHA and this change
```

- Base `top.txt` / flamegraph link:
- Head `top.txt` / flamegraph link:
- What moved (symbols + shares):

## Correctness checks run

- Card `checks`:
- Affected gates (conformance F1 / interop F2/F3 / Miri / sanitizers):
- New or updated regression tests:

## Rollback criterion

The exact signal that reverts this change (cell + threshold + where it
is gated, e.g. "dev-loop compare on codec.pbrs.owned_decode regresses >
5% in the CI perf lane"):

## Claim label

- [ ] Every number above is labeled `dev-loop` or `claim-grade`.
- [ ] No "fastest" claim without a named matrix (contract §10.2).
- [ ] No benchmark-only code path (contract §10.3).
