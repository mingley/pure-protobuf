# Retained elapsed-time and allocation results

Four paired samples per side and cell; medians of elapsed ns/parse. Allocation events include alloc, alloc_zeroed and realloc, counted in a separate 100-operation phase. These are shared-host diagnostics.

| Case | Profile | Input | Operation | Baseline ns | Candidate ns | Change | Allocation events before → after |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| scalar1 | owned | borrowed | owned_decode | 90.39 | 53.61 | -40.69% | 3 → 2 |
| scalar1 | owned | borrowed | parse_touch | 91.61 | 56.44 | -38.39% | 3 → 2 |
| scalar1 | owned | shared | owned_decode | 105.84 | 106.02 | +0.17% | 2 → 2 |
| scalar1 | owned | shared | parse_touch | 107.70 | 108.71 | +0.94% | 2 → 2 |
| scalar32 | owned | borrowed | owned_decode | 1529.23 | 1104.52 | -27.77% | 6 → 5 |
| scalar32 | owned | borrowed | parse_touch | 1603.43 | 1206.52 | -24.75% | 7 → 6 |
| scalar32 | owned | shared | owned_decode | 1555.10 | 1546.32 | -0.56% | 5 → 5 |
| scalar32 | owned | shared | parse_touch | 1642.54 | 1651.40 | +0.54% | 6 → 6 |
| scalar512 | owned | borrowed | owned_decode | 23850.50 | 17276.17 | -27.56% | 10 → 9 |
| scalar512 | owned | borrowed | parse_touch | 25003.76 | 18603.83 | -25.60% | 12 → 11 |
| scalar512 | owned | shared | owned_decode | 24028.51 | 23947.42 | -0.34% | 9 → 9 |
| scalar512 | owned | shared | parse_touch | 25086.35 | 24877.15 | -0.83% | 11 → 11 |
| scalar32u | owned | borrowed | owned_decode | 4446.96 | 3599.59 | -19.06% | 102 → 101 |
| scalar32u | owned | borrowed | parse_touch | 4592.79 | 3724.02 | -18.92% | 103 → 102 |
| scalar32u | owned | shared | owned_decode | 4076.41 | 4056.74 | -0.48% | 101 → 101 |
| scalar32u | owned | shared | parse_touch | 4222.33 | 4268.02 | +1.08% | 102 → 102 |
| scalar32 | default | borrowed | owned_decode | 1097.55 | 1101.04 | +0.32% | 5 → 5 |
| scalar32 | default | borrowed | parse_touch | 1186.37 | 1227.06 | +3.43% | 6 → 6 |
| scalar32 | default | shared | owned_decode | 1163.69 | 1144.39 | -1.66% | 5 → 5 |
| scalar32 | default | shared | parse_touch | 1225.30 | 1258.57 | +2.71% | 6 → 6 |
| scalar32 | prost | borrowed | owned_decode | 1387.01 | 1389.34 | +0.17% | 5 → 5 |
| scalar32 | prost | borrowed | parse_touch | 1436.38 | 1434.26 | -0.15% | 5 → 5 |
| blob32 | owned | borrowed | owned_decode | 2323.08 | 2307.92 | -0.65% | 6 → 6 |
| blob32 | owned | borrowed | parse_touch | 2423.95 | 2535.09 | +4.59% | 7 → 7 |
| blob32 | owned | shared | owned_decode | 2176.90 | 2201.46 | +1.13% | 37 → 37 |
| blob32 | owned | shared | parse_touch | 2329.02 | 2381.78 | +2.27% | 38 → 38 |
| blob8192 | owned | borrowed | owned_decode | 7469.39 | 7508.43 | +0.52% | 6 → 6 |
| blob8192 | owned | borrowed | parse_touch | 7642.45 | 7670.30 | +0.36% | 7 → 7 |
| blob8192 | owned | shared | owned_decode | 2388.45 | 2375.91 | -0.53% | 5 → 5 |
| blob8192 | owned | shared | parse_touch | 2497.80 | 2542.38 | +1.78% | 6 → 6 |
| node64 | owned | borrowed | owned_decode | 1214.19 | 1212.82 | -0.11% | 63 → 63 |
| node64 | owned | borrowed | parse_touch | 1292.47 | 1299.82 | +0.57% | 63 → 63 |
| node64 | owned | shared | owned_decode | 2165.59 | 2163.78 | -0.08% | 63 → 63 |
| node64 | owned | shared | parse_touch | 2247.37 | 2246.65 | -0.03% | 63 → 63 |
