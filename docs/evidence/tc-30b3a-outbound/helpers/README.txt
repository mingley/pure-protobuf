Prepared only; no Cargo/version/test command has run. Root must grant the ordinary lease after GN1000.

Use pinned env.sh, then TC30B_OUTBOUND_ORDINARY_LEASE=granted python3 run-stage.py STAGE. The plan.json arrays are the exact staged argv; no shell interpolation.

Before the first build, capture actual pinned rustup-selected Rust/Cargo/Clippy/rustdoc versions, executable hashes, genuine protoc version/hash, env.sh hash and source/manifest/lock hashes with the established tool/source capture recipe. Retain failures as attempts; never relabel a setup/build failure as a semantic red.

Run baseline-native-build/native-red, then baseline-generated-build/generated-red. Preserve their test ELFs, generated Rust, dependency files and Cargo fingerprint metadata with source/tool hashes before retiring the single owned qualification cache. Run candidate body/native gates, then generated transport, strict/docs and MSRV/graphs. Archive each completed stage before cache retirement; if the sampled aggregate cache guard stops a command, preserve the failure, archive completed artifacts, reclaim only the owned idle cache and use a new retry record name.

The guard includes both isolated target roots (aggregate <=2GiB), checks global free >=2GiB before launch and every second, and retains final samples. Driver limits CPU affinity to four available CPUs and Cargo jobs to one. The parent owns the 16GiB memory lease. No performance or full private-buffer budget claim.
