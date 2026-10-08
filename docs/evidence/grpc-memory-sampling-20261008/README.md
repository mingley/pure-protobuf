# Resource memory sampling

The second preview returned success from its child and all seven independent fault-recovery probes, but report validation failed at cycle 2: RSS was 27,025,408 bytes and high-water was 26,628,096 bytes. The fixture had read those values from separate /proc snapshots. The failed preview is retained in [attempt 002](../grpc-resource-soak-20261008-attempt-002/README.md).

Schema v4 reads the two Linux counters once, preserves their raw approximate values, and computes sampled_rss_peak_bytes from all retained RSS observations. It does not clamp or relabel the Linux high-water value. Positive counters and the single-read source are required. The exact sampled peak is checked independently; missing or changed values fail. The address-space limit and 32 MiB post-drain RSS tolerance are unchanged.

Five ordinary resource tests, strict Clippy, and all 15 reporting guards passed. Logs and working-tree file hashes are retained here. These checks precede commit and do not establish clean-source production qualification. The next frozen campaign supplies its own source and executable pins.

The raw test stdout is gzip-compressed to preserve its original trailing blank line without a source whitespace-check warning. source.json records its decoded checksum.
