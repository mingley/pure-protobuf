# GN-11 shared google.protobuf ownership fixtures

`opt_a.proto` extends `google.protobuf.MessageOptions`; `opt_b.proto` and
`opt_c.proto` set that option on one-field messages. The trio exercises the
previous three copies of the descriptor schema. `wkt_a.proto` and
`wkt_b.proto` exercise shared Timestamp identity, Value/NullValue proxies,
single-file nested inclusion, and the documented `::pbrs::wkt` mapping.
`local/descriptor.proto` checks that an automatically added descriptor owner
does not remove the explicitly requested input's flat `descriptor.rs` alias.

The checked descriptor sets were generated with `libprotoc 35.1` from
protobuf `v35.1`, source SHA `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`:

```bash
protoc -I tests/fixtures/codegen-wkt-sharing -I third_party/protobuf/src \
  --include_imports \
  --descriptor_set_out=tests/fixtures/codegen-wkt-sharing/options.fds \
  tests/fixtures/codegen-wkt-sharing/opt_a.proto \
  tests/fixtures/codegen-wkt-sharing/opt_b.proto \
  tests/fixtures/codegen-wkt-sharing/opt_c.proto
protoc -I tests/fixtures/codegen-wkt-sharing -I third_party/protobuf/src \
  --include_imports \
  --descriptor_set_out=tests/fixtures/codegen-wkt-sharing/wkts.fds \
  tests/fixtures/codegen-wkt-sharing/wkt_a.proto \
  tests/fixtures/codegen-wkt-sharing/wkt_b.proto
protoc -I third_party/protobuf/src --include_imports \
  --descriptor_set_out=tests/fixtures/codegen-wkt-sharing/timestamp.fds \
  google/protobuf/timestamp.proto
protoc -I tests/fixtures/codegen-wkt-sharing -I third_party/protobuf/src \
  --include_imports \
  --descriptor_set_out=tests/fixtures/codegen-wkt-sharing/alias.fds \
  tests/fixtures/codegen-wkt-sharing/local/descriptor.proto \
  tests/fixtures/codegen-wkt-sharing/wkt_b.proto
```

| Descriptor set | SHA-256 |
|---|---|
| `options.fds` | `0bb0720ccd707def77869c2ae2c9372c7ea9d1fe23dc02a36455a92f3cfee3b9` |
| `wkts.fds` | `2e0709b0c25ec8f01e79791b700e5a1ab8247552e86e4373d6451c317ee20cb2` |
| `timestamp.fds` | `2af537ffe8f72cc57d40aa07ae6aab13ba9f1ce671e92edfd827c5dacd35d27b` |
| `alias.fds` | `e01ac878770e73da2c85092cc82eba48efe913ed2372f00edd8dec8222918a21` |

The regression suite uses these sets so it needs neither a downloaded
protobuf source tree nor a new protoc in ordinary CI. Live pinned-source
generation, byte counts and bundled drift validation are recorded separately
in the GN-11 evidence.
