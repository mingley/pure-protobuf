# Generated namespace controls

This consumer checks legal schema names that also name Rust traits, runtime
types or enum constructors. It covers 34 message names and the enum names
`Some`, `None`, `Ok` and `Err`, with ProtoJSON, text and Prost 0.14.4 wire controls.
RPC services are outside this fixture.

The generator qualifies its own types, traits and value constructors. It
preserves existing `PbOption`, `PbResult`, `PbVec`, `PbBox` and `PbString` names,
and existing suffixes needed for genuine generated proxy-name collisions.
The lowercase names `pbrs`, `std` and `core` also shadow external crate names.
An imported message uses the explicit Rust path
`crate::foreign::pbrs::Child`, which must remain intact.

Four storage/default profiles exercise lazy and owned message storage, each
with ordinary or safe defaults. Each runs with the default runtime path, a
bare alias, an absolute alias and a `crate::` module path: 16 combinations.
User attribute and documentation literals containing `pbrs::` remain unchanged.
Safe profiles use consumer `#![forbid(unsafe_code)]`; runtime
dependencies keep their own lint policy. The direct legacy-macro control also
shadows standard type names without the generator's `Pb` naming convention.
Its default view must share a valid immutable default instance and remain the
size of a reference. The new six-argument `default_view` form supplies that
implementation. The original five-argument form keeps its existing contract:
the caller must supply the view's `Default` implementation. A separate control
keeps such an existing caller valid, and an expected-failure check confirms
that the original form alone still fails its `MessageView` Default bound.

Each profile checks:

- Scalar, optional, repeated, packed, map, oneof and imported message wire data
  in both directions against Prost.
- Typed views, copy/clear operations, shared input, JSON/text and error paths.
- Proto2 required fields, declared defaults and closed-enum conversions.
- Exported dynamic-message macro expansion under standard names and value
  constructor shadows.

The proto2 unknown-enum observation records presence, numeric value and a
Prost round trip. Both frameworks retain `Some(99)` in this legacy proto2
case; rejection by the enum's `TryFrom<i32>` API does not imply wire rejection.
This fixture makes no claim about all edition-specific enum rules.

Fetch locked dependencies once before running the offline driver:

```sh
cargo fetch --locked --manifest-path tests/proof/namespace_symbols/Cargo.toml
python3 tests/proof/namespace_symbols/run.py --out target/proof/namespace-symbols
```

Use a fresh output directory. The driver saves command outputs and a selected
source inventory, then exits nonzero if any profile fails or sources change.
These controls do not prove all legal schemas compile or establish a
performance result.
