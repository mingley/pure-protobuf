# Frozen parent bindings

`old_parent.rs` is literal output from the clean 05a3 generator. The manifest
pins its schema, generator and generated-source hashes. Do not format or
rewrite it when changing the current generator.

The parent resolves `Child` to the selected consumer. Its older lazy ancestors
must keep their established raw-wire behavior, while regenerated ancestors
with the map selection must reach the child's ordered storage.
