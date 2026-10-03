#!/usr/bin/env python3
"""Capture one unchanged additional test against the earlier readiness library."""
import datetime
import hashlib
import json
import pathlib
import shutil
import subprocess


ROOT = pathlib.Path('/workspace/scratch/work/deadline-admission')
PROTO = ROOT / 'work/h2-admission-prototype'
OLD = PROTO / 'queue-reserved-corrected-seven'
NEW = PROTO / 'additive-only-eighteen'
OUT = PROTO / 'mixed-waker-red'
EXPECTED_OLD = '4136a19e90d8fda7a140abbe18245aad86887ec8f098164351432d5be070c785'
TEST = 'additional::mixed_legacy_pending_cancel_releases_only_new_task_and_request'


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def run(argv, name):
    with (OUT / (name + '.stdout')).open('wb') as stdout:
        with (OUT / (name + '.stderr')).open('wb') as stderr:
            proc = subprocess.run(argv, cwd=ROOT, stdout=stdout, stderr=stderr)
    return {'argv': argv, 'cwd': str(ROOT), 'exit_code': proc.returncode}


OUT.mkdir(exist_ok=False)
old_pins = json.loads((OLD / 'pins.json').read_text())
new_pins = json.loads((NEW / 'pins.json').read_text())
library = OLD / 'libh2_admission.rlib'
assert sha(library) == EXPECTED_OLD == old_pins['rlib_sha256']
inputs = {
    str(library): EXPECTED_OLD,
    str(OLD / 'pins.json'): sha(OLD / 'pins.json'),
    str(OLD / 'source.tar.gz'): old_pins['artifacts_sha256']['source.tar.gz'],
    str(NEW / 'pins.json'): sha(NEW / 'pins.json'),
    str(NEW / 'source.tar.gz'): new_pins['artifacts_sha256']['source.tar.gz'],
    str(NEW / 'admission_tests.rs'): new_pins['source_sha256']['new_admission_tests.rs'],
    str(NEW / 'admission_additional_tests.rs'): new_pins['source_sha256']['admission_additional_tests.rs'],
}
for key, pin in old_pins['externs'].items():
    assert pin == new_pins['externs'][key]
    inputs[pin['path']] = pin['sha256']
# Preserve the coordinator's source inventory separately from the linked OLD
# source inventory. The current source is not compiled or mislabeled here.
for key, expected in new_pins['source_sha256'].items():
    if key.startswith('vendor/'):
        inputs[str(PROTO / key)] = expected
before = {path: sha(path) for path in inputs}
assert before == inputs, 'input drift before build'
compiler = pathlib.Path(subprocess.check_output(['rustup', 'which', 'rustc'], text=True).strip())
pins = {
    'started_utc': utc(),
    'purpose': 'specific mixed old/new pending waker red before additive-only readiness fix',
    'linked_backend_snapshot': 'queue-reserved-corrected-seven; retains old Sender.poll_ready',
    'harness_snapshot': 'unchanged additive-only-eighteen composed harness',
    'root_current_source_pins_reference': str(NEW / 'pins.json'),
    'root_current_source_is_linked': False,
    'linked_backend_source_pins': old_pins['source_sha256'],
    'root_current_source_pins': new_pins['source_sha256'],
    'compiler': {
        'command_shim': shutil.which('rustc'),
        'actual_path': str(compiler),
        'actual_sha256': sha(compiler),
        'version_verbose': subprocess.check_output(['rustc', '-Vv'], text=True),
    },
    'externs': old_pins['externs'],
    'inputs_sha256_before': before,
    'shipping_graph_changed': False,
}
argv = list(new_pins['test_build']['argv'])
argv[argv.index('-o') + 1] = str(OUT / 'admission-tests')
for i, value in enumerate(argv):
    if value.startswith('h2='):
        argv[i] = 'h2=' + str(library)
pins['test_build'] = run(argv, 'test-build')
assert pins['test_build']['exit_code'] == 0, 'test build failed'
executable = OUT / 'admission-tests'
pins['executable_sha256_before'] = sha(executable)
pins['tests'] = run([
    'timeout', '45', str(executable), '--exact', TEST,
    '--test-threads=1', '--nocapture',
], 'test')
pins['executable_sha256_after'] = sha(executable)
pins['inputs_sha256_after'] = {path: sha(path) for path in inputs}
pins['input_drift'] = pins['inputs_sha256_after'] != before
pins['executable_drift'] = pins['executable_sha256_after'] != pins['executable_sha256_before']
pins['finished_utc'] = utc()
pins['artifacts_sha256'] = {p.name: sha(p) for p in OUT.iterdir() if p.is_file()}
(OUT / 'pins.json').write_text(json.dumps(pins, indent=2) + '\n')
assert pins['tests']['exit_code'] == 101, 'expected assertion red was not captured'
assert 'new task retained on legacy stream' in (OUT / 'test.stderr').read_text()
assert '0 passed; 1 failed' in (OUT / 'test.stdout').read_text()
assert not pins['input_drift'] and not pins['executable_drift']
print(json.dumps({
    'library_sha256': EXPECTED_OLD,
    'test_exit_code': pins['tests']['exit_code'],
    'input_drift': pins['input_drift'],
    'executable_sha256': pins['executable_sha256_before'],
    'output': str(OUT),
}, indent=2))
