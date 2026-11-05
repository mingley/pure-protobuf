#!/usr/bin/env python3
"""Check captured byte-conversion allocations, source hashes and controls."""
import csv
import hashlib
import io
import json
from pathlib import Path
import shlex
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
BASE = '1c6ec8cacd547cce8c3a8e335320b49d34de40b4'
KINDS = {'vec_exact', 'vec_spare', 'cow_owned_exact', 'cow_owned_spare',
         'box_exact', 'cow_borrowed'}
LENGTHS = {0, 1, 16, 4096, 1_048_576}
MODELS = {'historical_expression', 'current_runtime'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read(name):
    path = Path(name)
    assert not path.is_absolute() and '..' not in path.parts, name
    return (HERE / path).read_bytes()


def artifact(events):
    found = [event for event in events
             if event.get('reason') == 'compiler-artifact'
             and event['target']['name'] == 'pbrs'
             and event['target']['kind'] == ['lib']]
    assert len(found) == 1
    result = found[0]
    assert result['features'] == []
    assert result['profile']['opt_level'] == '0'
    assert result['profile']['debuginfo'] == 0
    assert result['profile']['test'] is False
    return result


def main():
    manifest = json.loads(read('manifest.json'))
    assert manifest['schema'] == 'pbrs.byte-ownership-evidence.v1'
    assert manifest['base_commit'] == manifest['historical_source_commit'] == BASE
    assert set(manifest['files']) == {
        str(path.relative_to(HERE)) for path in (HERE / 'raw').rglob('*') if path.is_file()
    }
    for name, expected in manifest['files'].items():
        assert digest(read(name)) == expected, name

    report = json.loads(read('raw/report.json'))
    assert report['exit_code'] == 0
    assert report['source_inputs_unchanged'] is True
    assert report['compiled_inputs_unchanged'] is True
    assert report['source_before'] == report['source_after']
    assert report['compiled_inputs_before'] == report['compiled_inputs_after']
    assert report['base_commit'] == report['historical_source_commit'] == BASE
    assert report['host_machine'] == 'x86_64' and report['host_system'] == 'Linux'
    assert report['processes'] == 3 and report['rows_per_process'] == 60
    assert report['all_repeats_identical'] is True
    assert digest(read('raw/conversions.rs')) == report['harness_sha256']
    assert digest(read('raw/run.py')) == report['runner_sha256']
    assert len(report['compiler_binary']['sha256']) == 64
    assert len(report['binary_sha256']) == 64
    assert len(report['stages']) == 5
    for index, stage in enumerate(report['stages']):
        assert stage['exit_code'] == 0 and stage['disk_guard'] is False
        stdout = 'raw/' + (f'repeat-{index - 2}.csv' if index >= 2 else stage['stdout'])
        assert digest(read(stdout)) == stage['stdout_sha256']
        assert digest(read('raw/' + stage['stderr'])) == stage['stderr_sha256']
        if index >= 2:
            assert read('raw/' + stage['stderr']) == b''

    events = [json.loads(line) for line in read('raw/build.stdout').decode().splitlines()]
    assert events[-1] == {'reason': 'build-finished', 'success': True}
    current = artifact(events)
    assert manifest['build_artifact_reused'] is False
    assert current['fresh'] is manifest['build_artifact_reused']
    rlibs = {item['target']: item for item in report['rlibs']}
    assert set(rlibs) == {'pbrs', 'bytes', 'simdutf8'}
    for name, item in rlibs.items():
        assert len(item['sha256']) == 64
        matches = [event for event in events if event.get('reason') == 'compiler-artifact'
                   and event['target']['name'] == name]
        assert len(matches) == 1
        assert matches[0]['package_id'] == item['package_id']
        assert matches[0]['fresh'] is item['fresh']
        assert item['path'] in matches[0]['filenames']
    assert rlibs['bytes']['package_id'].endswith('#bytes@1.12.1')
    metadata = next(Path(name) for name in current['filenames'] if name.endswith('.rmeta'))
    expected_depfile = metadata.with_name(metadata.stem.removeprefix('lib') + '.d')
    assert str(expected_depfile) == rlibs['pbrs']['depfile']

    source_names = {'Cargo.toml', 'Cargo.lock', 'build.rs', 'clippy.toml'}
    deptext = read('raw/compiler-inputs.d').decode().replace('\\\n', '')
    recorded_root = Path(current['manifest_path']).parent
    for line in deptext.splitlines():
        if ': ' in line and not line.startswith('#'):
            for name in shlex.split(line.split(': ', 1)[1]):
                path = Path(name)
                source_names.add(str(path.relative_to(recorded_root)) if path.is_absolute() else name)
    assert source_names == set(report['compiled_inputs_before'])
    assert len(source_names) == 32
    for name, expected in report['compiled_inputs_before'].items():
        assert report['source_before'][name] == expected
        overlay = 'raw/overlay/' + name
        if overlay in manifest['files']:
            contents = read(overlay)
        else:
            assert not Path(name).is_absolute() and '..' not in Path(name).parts
            contents = subprocess.check_output(['git', 'show', f'{BASE}:{name}'], cwd=ROOT)
        assert digest(contents) == expected, name

    # Requested allocation bytes are specific to x86_64 and pinned bytes 1.12.1.
    captures = []
    expected_keys = {(kind, model, length) for kind in KINDS for model in MODELS for length in LENGTHS}
    for repeat in range(3):
        rows = list(csv.DictReader(io.StringIO(read(f'raw/repeat-{repeat}.csv').decode())))
        assert len(rows) == 60
        keys = set()
        for row in rows:
            kind, model, length = row['kind'], row['model'], int(row['len'])
            key = kind, model, length
            assert key not in keys
            keys.add(key)
            expected_capacity = max(length * 2, 4096) if kind.endswith('spare') else length
            assert int(row['input_capacity']) == expected_capacity
            if length == 0:
                conversion, clone, reused = (0, 0), (0, 0), 'not_applicable'
            elif model == 'historical_expression' or kind == 'cow_borrowed':
                conversion, clone, reused = (1, length), (1, 24), 'false'
            elif kind.endswith('spare'):
                conversion, clone, reused = (1, 24), (0, 0), 'true'
            else:
                conversion, clone, reused = (0, 0), (1, 24), 'true'
            assert (int(row['conversion_calls']), int(row['conversion_bytes'])) == conversion, key
            assert (int(row['clone_calls']), int(row['clone_bytes'])) == clone, key
            assert (int(row['share_calls']), int(row['share_bytes'])) == (0, 0), key
            assert row['payload_reused'] == reused, key
        assert keys == expected_keys
        captures.append(rows)
    assert all(capture == captures[0] for capture in captures)

    old_test = json.loads(read('raw/old-path-test.json'))
    assert old_test['exit_code'] == old_test['process_returncode'] == 101
    assert old_test['source_inputs_unchanged'] is False and old_test['disk_guard'] is False
    assert digest(read('raw/old-path-test.log')) == old_test['log_sha256']
    assert digest(read('raw/old-path-lazy.rs')) == old_test['inputs_sha256']['src/lazy.rs']
    old_source = read('raw/old-path-lazy.rs').decode()
    assert 'Self::owned(ProtoBytes::from(s.as_slice()))' in old_source
    assert 'fn bytes_from_vec_transfers_payload_with_exact_and_spare_capacity()' in old_source
    log = read('raw/old-path-test.log').decode()
    assert 'assertion `left == right` failed' in log
    assert 'test result: FAILED. 0 passed; 1 failed;' in log

    print(f'OK: 180 allocation rows, 32 bound inputs, {len(manifest["files"])} raw files, old-path copying control')


if __name__ == '__main__':
    main()
