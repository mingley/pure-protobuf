"""Bounded idle-process functional proof; this measures no RPC workload."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

root = next(p for p in Path(__file__).resolve().parents if (p / 'scripts/rpc-bench-matrix.py').is_file())
spec = importlib.util.spec_from_file_location('matrix_capacity', root / 'scripts/rpc-bench-matrix.py')
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)
child_code = '''import os,sys,threading
os.sched_setaffinity(0,{int(sys.argv[1])})
stop=threading.Event()
threads=[threading.Thread(target=stop.wait) for _ in range(int(sys.argv[2]))]
for t in threads:t.start()
print("READY",flush=True)
sys.stdin.readline()
stop.set()
for t in threads:t.join()
'''
children = []
cpu = min(os.sched_getaffinity(0))
try:
    for workers in (1, 7):
        child = subprocess.Popen([sys.executable, '-c', child_code, str(cpu), str(workers)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 text=True)
        children.append(child)
        assert child.stdout.readline().strip() == 'READY'
    monitor = matrix.ProcessResourceMonitor(children[0].pid, poll_interval_s=0.05)
    monitor.set_client_pid(children[1].pid)
    monitor.start()
    time.sleep(0.2)
    client, server, saturation = monitor.stop()
    assert client['thread_count'] == 8
    assert client['cpu_capacity']['affinity_cpu_count'] == 1
    assert server['cpu_capacity']['affinity_cpu_count'] == 1
    assert client['cpu_capacity']['verified'] is False
    assert server['cpu_capacity']['verified'] is False
    assert client['cpu_capacity']['effective_cpu_capacity'] is None
    assert saturation['headroom_verified'] is False
    assert saturation['spare_capacity_pct'] is None
    assert saturation['status'] == 'FAIL (UNVERIFIED)'
    files = ['scripts/rpc-bench-matrix.py', 'bench/stack-matrix/run.py',
             'tests/interop/test_bench_matrix.py', 'tests/interop/test_stack_matrix.py']
    proof = {'kind': 'idle-process functional fixture; no RPC or headroom qualification',
             'claim_eligible': False,
             'source_commit': subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
             'source_clean': subprocess.run(['git','diff','--quiet','HEAD','--'],cwd=root).returncode == 0,
             'source_sha256': {name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in files},
             'python_version': platform.python_version(),
             'python_binary': str(Path(sys.executable).resolve()),
             'python_binary_sha256': hashlib.sha256(Path(sys.executable).read_bytes()).hexdigest(),
             'kernel_release': platform.release(), 'sampler_poll_interval_seconds':0.05,
             'client_resources':client, 'server_resources':server, 'saturation':saturation,
             'host_capacity':matrix.collect_process_cpu_capacity(os.getpid())}
    print(json.dumps(proof,indent=2))
finally:
    for child in children:
        child.communicate(input='\n',timeout=5)
