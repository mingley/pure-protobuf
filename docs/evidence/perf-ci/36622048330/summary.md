## Dev-loop evidence

Base `d9c34c067d9b8115651904fd5778298e6cd5fdfe`; head `3a7aa12857e8d9f7b3569ec3847ccd9b8963ef1e`.

Valid base/head measurements; performance comparisons remain advisory.

Common cells: 88; new: 0; removed: 0.
Missing counters stay not_run and are excluded from eligible_metrics.

| Cell | Instructions base → head | Allocations base → head |
|---|---|---|
| codec.pbrs.blob_encode_1mib | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.blob_encode_4mib | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.blob_encode_64kib | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.blob_encode_8mib | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.blob_encode_mixed | not_run → not_run | 19.000 → 19.000 |
| codec.pbrs.blob_encode_shared_1mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_encode_shared_4mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_encode_shared_64kib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_encode_shared_8mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_encode_shared_mixed | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_parse_1mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_parse_4mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_parse_64kib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_parse_8mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_parse_mixed | not_run → not_run | 3.000 → 3.000 |
| codec.pbrs.blob_parse_shared_1mib | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.blob_parse_shared_4mib | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.blob_parse_shared_64kib | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.blob_parse_shared_8mib | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.blob_parse_shared_mixed | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.blob_touch_1mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_touch_4mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_touch_64kib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_touch_8mib | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.blob_touch_mixed | not_run → not_run | 3.000 → 3.000 |
| codec.pbrs.cached_encode | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.fresh_encode | not_run → not_run | 6.700 → 6.700 |
| codec.pbrs.owned_decode | not_run → not_run | 7.000 → 7.000 |
| codec.pbrs.packed_256_owned_decode | not_run → not_run | 3.000 → 3.000 |
| codec.pbrs.packed_256_parse_touch | not_run → not_run | 8.000 → 8.000 |
| codec.pbrs.parse_touch | not_run → not_run | 12.000 → 12.000 |
| codec.pbrs.small_empty_decode | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.small_empty_encode | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.small_id_decode | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.small_id_encode | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.small_name80_decode | not_run → not_run | 1.000 → 1.000 |
| codec.pbrs.small_name80_encode | not_run → not_run | 0.000 → 0.000 |
| codec.pbrs.tags_32_owned_decode | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.tags_32_parse_touch | not_run → not_run | 2.000 → 2.000 |
| codec.pbrs.unpacked_256_owned_decode | not_run → not_run | 3.000 → 3.000 |
| codec.pbrs.unpacked_256_parse_touch | not_run → not_run | 8.000 → 8.000 |
| codec.prost.blob_encode_1mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_encode_4mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_encode_64kib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_encode_8mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_encode_mixed | not_run → not_run | 10.000 → 10.000 |
| codec.prost.blob_parse_1mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_parse_4mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_parse_64kib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_parse_8mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_parse_mixed | not_run → not_run | 18.000 → 18.000 |
| codec.prost.blob_touch_1mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_touch_4mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_touch_64kib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_touch_8mib | not_run → not_run | 3.000 → 3.000 |
| codec.prost.blob_touch_mixed | not_run → not_run | 18.000 → 18.000 |
| codec.prost.cached_encode | not_run → not_run | 5.000 → 5.000 |
| codec.prost.fresh_encode | not_run → not_run | 5.000 → 5.000 |
| codec.prost.owned_decode | not_run → not_run | 10.000 → 10.000 |
| codec.prost.parse_touch | not_run → not_run | 10.000 → 10.000 |
| codec.prost.small_empty_decode | not_run → not_run | 0.000 → 0.000 |
| codec.prost.small_empty_encode | not_run → not_run | 0.000 → 0.000 |
| codec.prost.small_id_decode | not_run → not_run | 0.000 → 0.000 |
| codec.prost.small_id_encode | not_run → not_run | 0.000 → 0.000 |
| codec.prost.small_name80_decode | not_run → not_run | 1.000 → 1.000 |
| codec.prost.small_name80_encode | not_run → not_run | 0.000 → 0.000 |
| codec.v4.cached_encode | not_run → not_run | 1.000 → 1.000 |
| codec.v4.fresh_encode | not_run → not_run | 1.000 → 1.000 |
| codec.v4.owned_decode | not_run → not_run | 0.000 → 0.000 |
| codec.v4.parse_touch | not_run → not_run | 0.000 → 0.000 |
| lb.least_request.pick | not_run → not_run | 0.000 → 0.000 |
| lb.outlier_detection.pick | not_run → not_run | 0.000 → 0.000 |
| lb.pick_first.pick | not_run → not_run | 0.000 → 0.000 |
| lb.priority.pick | not_run → not_run | 0.000 → 0.000 |
| lb.random_subsetting_experimental.pick | not_run → not_run | 0.000 → 0.000 |
| lb.ring_hash.pick | not_run → not_run | 0.000 → 0.000 |
| lb.round_robin.pick | not_run → not_run | 0.000 → 0.000 |
| lb.weighted_round_robin.pick | not_run → not_run | 0.000 → 0.000 |
| rpc.pbrs.server_stream | not_run → not_run | 53.025 → 53.025 |
| rpc.pbrs.server_stream_compressed | not_run → not_run | 916.025 → 916.025 |
| rpc.pbrs.unary | not_run → not_run | 38.010 → 38.010 |
| rpc.pbrs.unary_compressed | not_run → not_run | 68.015 → 68.015 |
| rpc.prost.server_stream | not_run → not_run | 57.025 → 57.025 |
| rpc.prost.unary | not_run → not_run | 38.000 → 38.000 |
| rpc.tonic.server_stream | not_run → not_run | 81.405 → 81.405 |
| rpc.tonic.unary | not_run → not_run | 71.375 → 71.385 |
| rpc.tonic_prost.server_stream | not_run → not_run | 81.415 → 81.395 |
| rpc.tonic_prost.unary | not_run → not_run | 71.405 → 71.395 |

Comparison exit: 1 (advisory; SB-20 sets gates).
