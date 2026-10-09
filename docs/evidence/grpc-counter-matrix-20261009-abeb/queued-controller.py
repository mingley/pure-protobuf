"""Run synthetic endpoint counters after the resource campaign has ended."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path('/workspace/pure-protobuf')
SOURCE = ROOT
OUT = ROOT / 'work/counter-sweeps-abeb'
OUT.mkdir(exist_ok=False)
PIN = 'abeb7bcf2ee653bff899ba14f505ced9b373f963'
PUBLISHER = ROOT / 'scripts/grpc-counter-capsule.py'
PUBLISHER_SHA = hashlib.sha256(PUBLISHER.read_bytes()).hexdigest()
state = dict(source=PIN, state='waiting_for_resource_campaign', qualified=False,
             expected_captures_per_phase=7200, phases=[],
             publisher_sha256=PUBLISHER_SHA,
             limits=['synthetic steady-state counter coverage only',
                     'read-all corpora, saturation, cold/idle, wakeups and dedicated-host timing remain open'])
(OUT / 'controller.py').write_bytes(Path(__file__).read_bytes())


def save():
    temporary = OUT / 'status.json.tmp'
    temporary.write_text(json.dumps(state, indent=2) + '\n')
    temporary.replace(OUT / 'status.json')


save()


def publish(phase, directory):
    if hashlib.sha256(PUBLISHER.read_bytes()).hexdigest() != PUBLISHER_SHA:
        raise RuntimeError('counter capsule publisher changed')
    def command(args, cwd=ROOT):
        subprocess.run(args, cwd=cwd, check=True)
    command(['git', 'fetch', 'origin', 'main'])
    checkout = OUT / (phase + '-publication')
    relative = Path('docs/evidence/grpc-counter-matrix-20261009-abeb') / phase
    command(['git', 'worktree', 'add', '--detach', '--no-checkout', str(checkout), 'origin/main'])
    command(['git', 'sparse-checkout', 'init', '--no-cone'], checkout)
    command(['git', 'sparse-checkout', 'set', '/' + str(relative) + '/'], checkout)
    command(['git', 'checkout', 'HEAD'], checkout)
    command(['python3', str(PUBLISHER), '--source', str(directory), '--output',
             str(checkout / relative), '--expected-source', PIN], checkout)
    command(['git', 'add', str(relative)], checkout)
    command(['git', 'diff', '--cached', '--check'], checkout)
    command(['git', 'commit', '-m', f'bench(grpc): retain {phase} counter sweep outcome'], checkout)
    for attempt in range(3):
        result = subprocess.run(['git', 'push', 'origin', 'HEAD:main'], cwd=checkout)
        if result.returncode == 0:
            revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=checkout, text=True).strip()
            command(['git', 'worktree', 'remove', str(checkout)])
            return dict(commit=revision, path=str(relative))
        command(['git', 'fetch', 'origin', 'main'], checkout)
        command(['git', 'rebase', 'origin/main'], checkout)
    raise RuntimeError('main rejected publication; local commit and all captures retained')


try:
    while True:
        try:
            prerequisites = json.loads((ROOT / 'work/resource-009-prerequisites/status.json').read_text())
        except (OSError, json.JSONDecodeError):
            prerequisites = {}
        if prerequisites.get('state') in ['finished', 'failed', 'blocked']:
            try:
                resource = json.loads((ROOT / 'work/campaign-v5-009/controller-status.json').read_text())
            except (OSError, json.JSONDecodeError):
                resource = {}
            if not prerequisites.get('benchmark_ready'):
                raise RuntimeError('resource prerequisites ended without a verified benchmark')
            if resource.get('state') != 'finished':
                resource = dict(state='not_started', reason=prerequisites.get('error'), qualified=False)
            break
        time.sleep(3)
    state['resource_outcome'] = resource
    driver = SOURCE / 'scripts/grpc-counter-campaign.py'
    state['driver_sha256'] = hashlib.sha256(driver.read_bytes()).hexdigest()
    os.environ['VALGRIND_LIB'] = str(ROOT / 'work/toolchain/valgrind/usr/libexec/valgrind')
    for phase in ['native', 'syscalls', 'callgrind']:
        state['state'] = 'capturing_' + phase
        save()
        # Keep room for terminal records and retain a visible failure rather
        # than start another phase when the workspace is nearly full.
        disk = os.statvfs(OUT)
        if disk.f_bavail * disk.f_frsize < 1024 ** 3:
            raise RuntimeError('less than 1 GiB free before counter phase; earlier raw captures retained')
        directory = OUT / phase
        argv = ['python3', str(driver), '--source-checkout', str(SOURCE),
                '--binary', str(ROOT / 'work/benchmark-abeb/rpc-bench'),
                '--build-record', str(ROOT / 'work/benchmark-abeb/build.json'),
                '--output', str(directory), '--phase', phase]
        if phase == 'syscalls':
            argv += ['--strace', '/usr/bin/strace']
        if phase == 'callgrind':
            argv += ['--valgrind', str(ROOT / 'work/toolchain/valgrind/usr/bin/valgrind')]
        with (OUT / (phase + '.stdout')).open('w') as stdout, (OUT / (phase + '.stderr')).open('w') as stderr:
            result = subprocess.run(argv, cwd=SOURCE, stdout=stdout, stderr=stderr)
        record = json.loads((directory / 'campaign.json').read_text())
        state['phases'].append(dict(phase=phase, command=argv, exit_code=result.returncode,
                                    state=record.get('state'), passed=record.get('passed'),
                                    report=str(directory / 'campaign.json')))
        save()
        state['phases'][-1]['publication'] = publish(phase, directory)
        save()
        if record.get('state') != 'finished':
            raise RuntimeError(f'{phase} did not finish; partial and failed captures retained')
    state['state'] = 'finished'
    state['passed'] = all(phase['passed'] for phase in state['phases'])
    save()
except BaseException as error:
    state.update(state='failed', passed=False, error=dict(type=type(error).__name__, message=str(error)))
    save()
    raise
