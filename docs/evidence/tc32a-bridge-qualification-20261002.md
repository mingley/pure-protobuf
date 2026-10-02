# TC32a: actual mixed-runtime bridge corpus qualification

Date: 2026-10-02. Source: `754cb6c2e3a7c0cf592f64f33807e257b00ec648`
(implementation commits `7ecee2242d0a445fc74a3a8a77b9cac47918d4f5` and
`754cb6c2e3a7c0cf592f64f33807e257b00ec648`).

The benchmark corpus now invokes the shipping
`protobuf_tonic::prost_to_pbrs` and `protobuf_tonic::pbrs_to_prost` APIs across
all 84 public specimens: 48 recursive queries, three Any-bearing entity lists,
ten sparse optional-field patterns, three maps, and 20 option records.
Both directions and API-only/read-all modes produce 336 cells. Each prepared
source is completely read before work. The work call owns and drops its actual
converted target; read-all mode additionally walks the complete target.
Preparation checks full independently decoded message equality, complete read
checksums, both actual API round trips, and actual source/target wire bytes.
These checks are outside the future cost measurement window.

All 336 semantic cells pass. The independent inventory audit matches all
256 runtime-corpus rows to the retained SB26 checksum/size oracles. Regression
checks cover 320 present option fields including default-valued fields,
missing/unequal fingerprints, full-equality failures with deliberately equal
checksums, and three fresh-process source-byte reproductions for all 162
non-map direction/specimen inputs. The unchanged 25 generated outputs retain
identical SHA256 values; only the new bridge registry and option helpers were
added.

The 324 non-map cells pass their actual source/target wire guards. All 12 map
cells remain blocked from cost qualification. The actual APIs preserve the
existing default-valued map difference: prost-to-pbrs re-encodes 4678, 6831 and
24647-byte sources as 4682, 6835 and 24651 bytes; the reverse direction removes
those four bytes. The existing prost hash-map source ordering is also retained.
The fixture's common preparation image does not replace the actual source
image in the cell fingerprint. No shipping map policy or API was changed.

The bridge feature is optional and disabled by default. Default and
`--no-default-features` metadata proofs each retain 49 active packages, with
no adapter, Tokio or bridge feature active; ordinary default tests pass.
The bridge-enabled standalone lock has 111 packages. All 49 original versions
and checksums remain unchanged, and every newly added registry package matches
the reviewed root lock. The first resolution's eight version mismatches were
rejected before compilation and remain recorded with the approved offline pin
commands and final audit. No registry package or shipping dependency version
was introduced.

Final checks at the pinned source passed: 21 bridge-enabled tests, 14 default
tests, strict all-target bridge Clippy, generated-output checking, and the
independent inventory audit. The first strict Clippy failure is retained;
generated constructors were corrected with complete struct literals rather
than lint allowances. Exact commands, tool hashes, failed attempts, source
hashes and process-resume/resource limits are in the artifact directory.

The debug inventory binary SHA256 is
`e87496461ce7ecc2dbe1e56221b531f841a31448ebc9a85568b6e4addc74455f`;
the inventory JSON SHA256 is
`aa2c7238d08617ea320ba96e3432b675946c171c3ef32d9877f593b26564d9be`.
[Retained artifacts and hash index](tc32a-bridge-qualification-20261002/artifact-sha256.json)
include an executable independent coverage/oracle auditor.

No allocation, instruction or wall-time cost was measured. This completes the
TC32a corpus/API qualification leaf; TC32b's collector integration, absolute
allocation/byte/instruction results, unchanged-binary replay and the parent
TC32 acceptance remain open. Shared-host performance failures from other
campaigns are not overridden by these semantic checks.
