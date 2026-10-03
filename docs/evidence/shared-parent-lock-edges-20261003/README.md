At qualified main `ad79b48ee34d4fd67346560da195f69ee6a846c3`, the standalone
parent's frozen lock lacks seven direct `pbrs-grpc` support edges introduced by
the private embedded h2 backend. An actual `cargo metadata --offline --locked
--features bridge` rejects that old parent lock, exit 101. Its complete argv,
environment overrides, stdout/stderr, timestamps and old input bytes are
retained. No automatic resolver update or compilation was used to repair it.

The only shipping-file change adds atomic-waker, fnv, futures-sink, indexmap,
slab, tokio-util and tracing to the existing `pbrs-grpc` dependency list in
`bench/devloop/Cargo.lock`. Every one of the frozen 154 parent package
name/version/source/checksum tuples remains unchanged, including indexmap
2.14.2 and all 153 pre-bridge parent pins. Root's 172 package pins/root lock and
adoption's 111 pins/lock remain byte-identical. Adoption contains no grpc node.
The existing ordinary indexmap 2.14.0 manifest requirement accepts root 2.14.0
and standalone 2.14.2 without changing either manifest or any registry pin.

The parent's old SHA256 is
`a5fa1e79eb2b3741513e479c393fda804f2f6a19416b9560929f033fb944398c`;
its corrected SHA256 is
`61399401c8e57ceb6aa697fb404bee9ac22f6ec48441faa8dfae74daabbf83c4`.
The before/after files, parsed exact edge/package audit and Git blob identities
are retained. The controlled overlay then passes locked offline metadata for
parent bridge/default and adoption bridge/default. Every reported metadata
package is bound to the unchanged corresponding lock tuple. These records
qualify graph preparation only; no compilation, release build, LTO, benchmark or
performance qualification ran.

The correction used an isolated sparse worktree with its own checkout/index.
The initial no-checkout worktree required `read-tree` to populate its empty
index before the first Cargo invocation; that setup issue is recorded rather
than counted as a Cargo rejection. Main checkout/index were not mutated.
The executed correction script is included for review and should not be
re-executed on an already corrected lock. `verify.py` is an independent
read-only checker for the sealed bytes, exact edges and resolver records.

The release-preparation refresh has 477 tracked source inputs, all 41 required
TC32 paths and the unchanged 88 schema pins, including all 14 required upstream
schemas, five tracked OTLP schemas and the fallback FDS. Historical 659333e
inventory pins are preserved as historical inputs. The current provisional
inventory is explicitly ad79b48e; the corrected standalone lock is a separately
recorded overlay until root merges this commit. The work-only final-input
helper was corrected to read the existing inline-table indexmap version field,
with before/after helper hashes retained; no shipping manifest changed.

The frozen 336 bridge cells, 512 SB32 transport IDs and ordered full 20 original
controls, all thresholds, N/repetitions/warmup/preparation, release profile and
tool versions remain unchanged. The old 20 controls stay N200/400 times three,
warmup100/prep400, codec 1%/RPC 2%, separate from the new bridge and transport
N16/32 times three. Historical failed controls stay failed. Native low-level
profiles cannot replace unchanged tonic-generated transport rows.

The previously completed historical cache reclamation remains qualified by
root-pushed 0c62 plus additive 8865 preservation evidence; this task performs no
new cache reclamation or build. `source_commit` in the release plan remains
null pending qualified opt-out integration and the coordinator's final source
freeze. No current provisional inventory is relabeled as a completed release
sidecar. Run `python3 verify.py` from this directory to check this proof.
