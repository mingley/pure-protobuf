import datetime
import json
import hashlib
import os
import pathlib
import subprocess
import shutil
import signal
import time
import sys

root = pathlib.Path(os.environ['TC30B_OUTBOUND_CWD'])
name, *argv = sys.argv[1:]
prefix = pathlib.Path(os.environ['TC30B_OUTBOUND_RECORDS']) / name

def git(*args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()

executable = pathlib.Path(argv[0])
executable_sha256 = hashlib.sha256(executable.read_bytes()).hexdigest() if executable.is_file() else None
record = {
    'schema': 'tc30b-outbound-command-v1',
    'name': name,
    'argv': argv,
    'executable_sha256_before': executable_sha256,
    'cwd': str(root),
    'source_before': git('rev-parse', 'HEAD'),
    'tracked_changes_before': git('status', '--porcelain'),
    'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'environment': {key: os.environ.get(key) for key in ['CARGO_BUILD_JOBS', 'CARGO_TARGET_DIR', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_DEV_DEBUG', 'CARGO_PROFILE_TEST_DEBUG', 'RUSTDOCFLAGS', 'RUSTUP_HOME', 'CARGO_HOME', 'PROTOC', 'PATH', 'RUSTFLAGS', 'RUSTC_WRAPPER']},
    'patch_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', '--binary'], cwd=root)).hexdigest(),
    'scope': 'correctness only; elapsed time is not comparative performance evidence',
}
guard = None
observations = []

def disk_sample():
    sizes = {}
    for item in json.loads(os.environ['TC30B_OUTBOUND_OWNED_TARGET_ROOTS']):
        cache = pathlib.Path(item)
        sizes[item] = int(subprocess.check_output(['du', '-sb', str(cache)], text=True).split()[0]) if cache.exists() else 0
    return {'free': shutil.disk_usage(root).free, 'owned_cache_bytes': sum(sizes.values()), 'owned_cache_roots': sizes}

initial = disk_sample()
assert initial['free'] >= 2 * 1024**3 and initial['owned_cache_bytes'] <= 2 * 1024**3, initial
observations.append(initial)
with prefix.with_suffix('.log').open('wb') as log:
    child = subprocess.Popen(argv, cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    while child.poll() is None:
        observation = disk_sample()
        free, size = observation['free'], observation['owned_cache_bytes']
        observations.append(observation)
        if free < 2 * 1024**3 or size > 2 * 1024**3:
            guard = {'reason': 'disk guard stop', 'free': free, 'owned_cache_bytes': size}
            os.killpg(child.pid, signal.SIGTERM)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
            break
        time.sleep(1)
    result = child.wait()
observations.append(disk_sample())
record['disk_observations'] = observations
record.update(disk_guard=guard, native_exit=result, minimum_free=min((x['free'] for x in observations), default=None), maximum_owned_cache=max((x['owned_cache_bytes'] for x in observations), default=None))
if guard:
    result = 125
record['executable_sha256_after'] = hashlib.sha256(executable.read_bytes()).hexdigest() if executable.is_file() else None
record.update(exit=result, finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), source_after=git('rev-parse', 'HEAD'), tracked_changes_after=git('status', '--porcelain'))
prefix.with_suffix('.json').write_text(json.dumps(record, indent=2) + '\n')
prefix.with_suffix('.exit').write_text(str(result) + '\n')
print(prefix.with_suffix('.log').read_text())
sys.exit(result)
