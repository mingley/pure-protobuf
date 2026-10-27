# MiniTable groups and unknown fields, October 27

Source `6fba243571e269bd7a54e3d3b905567902823f99` extends the linked
MiniTable parser and serializer. The previous runtime, `a24c0e35`, skipped
known protobuf groups, encoded them as length-delimited messages, and discarded
unknown fields and known fields received with an incompatible wire type.

Singular groups now merge across occurrences before required-field validation;
repeated groups remain separate, oneof replacement is preserved, and group
end tags and recursion limits are checked. Serialization uses matching start
and end tags. Unknown fields round-trip through messages and linked groups.
A null MiniTable remains an inert sentinel; group framing is still validated
there. Map-entry behavior is unchanged.

The [results](results.json) and [raw archive](raw.tar.gz) contain the patch,
source and executable hashes, individual benchmark samples, protobuf schemas,
`protoc` reference results, negative controls, and validation logs. Run
`python3 docs/evidence/minitable-groups-20261027/check.py` from the repository
root to verify [manifest.json](manifest.json) and the archive without extracting
it. Compiled executables are omitted. Original paths inside the archive start
with `work/`; reproduction commands use the recorded Rust version and need
paths adjusted for a different checkout.

## Performance costs

An allocation-counted, parse-only fixture on a shared host compares the
previous runtime with this change. Six process pairs alternate execution
order, with five samples per variant in each pair. For 100 non-group singular
fragments, the overall median changes from 4,285 to 4,513 ns per parse
(+5.32%); all six paired changes are increases, from +3.4% to +11.0%. A single
fragment changes from 110.23 to 110.05 ns. Both variants request four
allocations and 154 bytes per parse. These are diagnostic timings, not an
endpoint throughput claim.

Three additional layout experiments were not adopted. The combined cold-helper
variant measured 4,721 versus 4,521 ns for the original candidate in its own
comparison; the helpers-only and combined-check-only variants also failed to
show a clear improvement. All samples and the variants' patches are retained.
The known-group body itself was not timed.

Preserving an unknown varint in another fixture raises allocation requests
from four / 154 bytes to six / 306 bytes; the old parser discarded the field.
The before and after perform different work. The archive includes timings for
length-delimited values and nested unknown groups as well as the non-group
controls.

## Checks

The isolated controls passed 46 MiniTable integration tests, eight runtime
unit tests and strict Clippy. The old decoder failed seven detecting group
controls; the old serializer failed the group round-trip control. Three
unknown-field controls also failed before unknown preservation was added.
The reference protobuf v35.1 tool confirms the selected group wire forms and
rejects malformed end tags.

On the clean integrated commit `6fba2435`, 198 targeted core, dynamic,
recursion and MiniTable tests passed, including all 46 MiniTable tests. Strict
core Clippy, an all-feature workspace build check, and five Rust 1.85
core/native/Tower library checks passed. The pinned protobuf conformance
runner passed its two required runs and recommended run: each had 5,631
binary/JSON and 909 text successes with no skips or failures. The full
3,902-test workspace run in the [preceding record](../grpc-parity-and-costs-20261027/README.md)
was performed before this group follow-up; it was not repeated afterward.
The documentation link and caveat suite passed 25 tests after this record
was added.
