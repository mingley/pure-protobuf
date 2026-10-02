#!/usr/bin/env bash
set -uo pipefail
cd /workspace/scratch/work/sb21
source /workspace/pure-protobuf/work/toolchain/env.sh
export PATH=/workspace/pure-protobuf/work/toolchain/go1253/go/bin:$PATH
export GOTOOLCHAIN=local
export GOMODCACHE=/workspace/pure-protobuf/work/toolchain/go-mod-cache
export GOCACHE=/workspace/pure-protobuf/work/toolchain/go-build-cache
export GOFLAGS=-p=1
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$PWD/target/native"
export GRPC_QPS_NATIVE_WORKER="$PWD/target/native/release/rpc-bench"
start_ns=$(date +%s%N)
mkdir -p work/sb21/optimized
python3 - <<'PY' > work/sb21/optimized/pins.json
import hashlib, json, pathlib, subprocess
p = pathlib.Path('target/native/release/rpc-bench')
print(json.dumps({'binary_source_commit': '1e6b1119182e1e4e90f2d93656765b3bf4929b81',
 'harness_commit': subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
 'native_release_sha256': hashlib.sha256(p.read_bytes()).hexdigest(),
 'sampler_denominator': 'old observed-thread-count; headroom unverified',
 'claim_eligible': False}, indent=2))
PY
bash scripts/grpc-qps-interop.sh --skip-build --scenarios=rpc-bench/scenarios/claims.json --scenario=claims_unary_poisson_5000qps --mode=all --ref-peer=go --repeats=2 --order-seed=210022 --warmup=1 --duration=2 --claim-check --log-dir=work/sb21/optimized/qps-all > work/sb21/optimized/qps-all.log 2>&1
all_rc=$?
bash scripts/grpc-qps-interop.sh --skip-build --scenarios=rpc-bench/scenarios/claims.json --scenario=claims_unary_poisson_5000qps --mode=ref_pair --ref-peer=go --repeats=1 --order-seed=210022 --warmup=1 --duration=2 --claim-check --log-dir=work/sb21/optimized/qps-ref > work/sb21/optimized/qps-ref.log 2>&1
ref_rc=$?
end_ns=$(date +%s%N)
python3 - "$start_ns" "$end_ns" "$all_rc" "$ref_rc" <<'PY' > work/sb21/optimized/lease.json
import json,sys
a,b,all_rc,ref_rc=map(int,sys.argv[1:])
print(json.dumps({'start_unix_ns':a,'end_unix_ns':b,'elapsed_s':(b-a)/1e9,'qps_all_exit':all_rc,'qps_ref_exit':ref_rc},indent=2))
PY
cat work/sb21/optimized/lease.json
exit "$((all_rc || ref_rc))"
