# Independent Zstandard context fixtures

Produced with system **libzstd 1.5.7**, using `ZSTD_compress` at level 3 through Python ctypes. These frames test decoding and resetting entropy contexts across independent frames; they are not performance measurements.

For frame index `i`, the plaintext starts with one byte per `n` in `0..8192+i*37`:

```text
(n*n/17 + n*(19+i*11) + (n/64)%31) % 251
```

Division uses integer arithmetic. Append the first 4,096 bytes, then append `context reset ` 97 times. The test reconstructs this plaintext instead of storing a second copy.

| File | Compressed bytes | Decoded bytes | Compressed SHA-256 |
| --- | ---: | ---: | --- |
| frame-0.zst | 8,224 | 13,646 | `b6858f3b5f37b1d779abf5860fd4e8cc185e87e1eb73889572f788bcf3d0edde` |
| frame-1.zst | 8,248 | 13,683 | `93acd6bc65b7e4240093c2eee94e211dad0d654a03c27052a955f68c28039a85` |
| frame-2.zst | 8,282 | 13,720 | `b5d8eb6d590601992f860680575307ce73d55b520b2a19a0e3de772d73d8f5d3` |

Decoded SHA-256 values:

- Frame 0: `d6d0a4d631922c2bd7ce3529d215480e00dcecb9820999777df661f416a339be`
- Frame 1: `99759598116e2276eb4b4871b7b587f5e0d19c9a06773a933696b41128aab36a`
- Frame 2: `cd864b9e2412abc1de831facc7a5cfdc707c215779792396e82d57cf438e4da8`
