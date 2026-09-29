# Security checks

The workspace keeps supply-chain checks local and CI-friendly:

```bash
cargo audit
cargo deny --workspace --all-features check
```

`deny.toml` rejects RustSec vulnerability and unsoundness advisories, yanked
crates, unknown registries, and all git sources. License checks allow the
licenses already present in the workspace dependency graph: MIT, Apache-2.0,
BSD-3-Clause, ISC, Unicode-3.0, and Zlib. The existing `webpki-roots` dependency is
the only per-crate exception because its Mozilla root store is distributed as
`CDLA-Permissive-2.0`.

For private vulnerability reporting, use the
[security policy](../SECURITY.md). For release and maintenance expectations,
see the [support policy](support-policy.md).
