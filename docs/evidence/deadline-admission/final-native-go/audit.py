#!/usr/bin/env python3
"""Replay the cc4 protocol evidence without executing the retained binaries."""
import ast
import collections
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import sys
import tarfile
import types

SOURCE = 'cc4db05383776ad39e8af34dee12a164b35ffcf2'
DOC_BASE = 'b8a8bd9171f58b5ec67b487d34e6b78f284fb4f7'
GO_PIN = 'dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef'
BASE_CASES = {
    'empty_unary', 'large_unary', 'client_streaming', 'server_streaming',
    'ping_pong', 'empty_stream', 'cancel_after_begin',
    'cancel_after_first_response', 'timeout_on_sleeping_server',
    'custom_metadata', 'status_code_and_message', 'special_status_message',
    'unimplemented_method', 'unimplemented_service',
}
COMPRESSION_CASES = {
    'client_compressed_unary', 'server_compressed_unary',
    'client_compressed_streaming', 'server_compressed_streaming',
}
DIRECTIONS = {
    'kernel_client_to_kernel_server', 'kernel_client_to_go_server',
    'go_client_to_kernel_server',
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


class JoinedParts(io.RawIOBase):
    def __init__(self, paths):
        self.paths = iter(paths)
        self.current = None

    def readable(self):
        return True

    def read(self, size=-1):
        if size < 0:
            raise ValueError('The audit reads the compressed archive in bounded chunks')
        output = bytearray()
        while len(output) < size:
            if self.current is None:
                path = next(self.paths, None)
                if path is None:
                    break
                self.current = path.open('rb')
            block = self.current.read(size - len(output))
            if block:
                output.extend(block)
            else:
                self.current.close()
                self.current = None
        return bytes(output)

    def close(self):
        if self.current is not None:
            self.current.close()
        super().close()


def go_build_info(data):
    """Read Go 1.18+ inline build info from the ELF .go.buildinfo section.

    The section/header/string format is documented in Go's debug/buildinfo
    source. This reader checks this archive's ELF64 little-endian format and
    parses only the named section; it does not search arbitrary binary strings.
    """
    import struct
    assert data[:6] == b'\x7fELF\x02\x01', 'Expected ELF64 little-endian Go artifact'
    section_offset = struct.unpack_from('<Q', data, 40)[0]
    section_size, section_count, names_index = struct.unpack_from('<HHH', data, 58)
    assert section_size == 64 and names_index < section_count
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, section_offset + i * section_size)
                for i in range(section_count)]
    names_header = sections[names_index]
    names = data[names_header[4]:names_header[4] + names_header[5]]
    info = None
    for header in sections:
        name = names[header[0]:].split(b'\0', 1)[0]
        if name == b'.go.buildinfo':
            assert info is None, 'Duplicate .go.buildinfo section'
            info = data[header[4]:header[4] + header[5]]
    assert info is not None and info[:14] == b'\xff Go buildinf:'
    assert info[14:16] == b'\x08\x02', 'Expected inline, little-endian build info'

    def string(pos):
        length = shift = 0
        for _ in range(10):
            value = info[pos]
            pos += 1
            length |= (value & 127) << shift
            if value < 128:
                assert pos + length <= len(info)
                return info[pos:pos + length], pos + length
            shift += 7
        raise AssertionError('Invalid varint in build info')

    version, pos = string(32)
    module, _ = string(pos)
    assert version == b'go1.25.3'
    assert module[:16] == bytes.fromhex('3077af0c9274080241e1c107e6d618e6')
    assert module[-16:] == bytes.fromhex('f932433186182072008242104116d8f2')
    return version.decode(), module[16:-16].decode()


def main():
    directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent
    archive = json.loads((directory / 'archive.json').read_text())
    paths = [directory / part['name'] for part in archive['parts']]
    combined = hashlib.sha256()
    for part, path in zip(archive['parts'], paths):
        digest = hashlib.sha256()
        size = 0
        with path.open('rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                size += len(block)
                digest.update(block)
                combined.update(block)
        assert size == part['bytes'] <= 25 * 1024**2
        assert digest.hexdigest() == part['sha256']
    assert combined.hexdigest() == archive['compressed_sha256']

    data = {}
    manifest = None
    observed = {}
    with JoinedParts(paths) as joined, gzip.GzipFile(fileobj=joined) as compressed:
        with tarfile.open(fileobj=compressed, mode='r|') as bundle:
            for member in bundle:
                assert member.isfile() and not member.name.startswith('/')
                assert '..' not in Path(member.name).parts
                assert member.name not in observed, 'Duplicate archive member'
                stream = bundle.extractfile(member)
                content = stream.read()
                assert len(content) == member.size
                observed[member.name] = {'bytes': len(content), 'sha256': sha(content)}
                if manifest is None:
                    assert member.name == 'manifest.json', 'Manifest must be first'
                    manifest = json.loads(content)
                else:
                    assert manifest['members'][member.name] == observed[member.name]
                    data[member.name] = content
    assert set(observed) == {'manifest.json', *manifest['members']}
    # The sealed initial auditor is retained unchanged after its first-build
    # pin-set assumption failed. This corrected external auditor has its own
    # Git/outer-index pin, and the original remains an immutable member.
    assert sha(data['audit.py']) == archive['embedded_auditor_sha256']
    assert sha(Path(__file__).read_bytes()) == archive['auditor_sha256']
    for name, item in archive['audit_correction_raw'].items():
        raw = (directory / name).read_bytes()
        assert len(raw) == item['bytes'] and sha(raw) == item['sha256']
    assert (directory / 'audit-first.exit').read_text() == '1\n'
    assert manifest['source'] == SOURCE and manifest['docs_base'] == DOC_BASE

    def load(name):
        return json.loads(data[name])

    record = load('runtime/record.json')
    assert record['provider_source'] == SOURCE
    assert record['before'] == record['after']
    assert record['before']['head'] == SOURCE and not record['before']['status']
    assert record['status'] == 'failed_or_incomplete'
    assert record['runtime_gomaxprocs'] is None
    assert record['global_free_reserve'] == 2 * 1024**3
    assert sha(data['runtime/run-final-native-go.py']) == record['runner_sha256']
    overrides = record['environment_overrides']
    for key in ('GOMAXPROCS', 'GOFLAGS', 'CASE_TIMEOUT_SEC', 'STARTUP_TIMEOUT_SEC',
                'MAX_ATTEMPTS', 'SELF_ONLY', 'GRPC_INTEROP_CASES'):
        assert key not in overrides
    runner = data['runtime/run-final-native-go.py'].decode()
    ast.parse(runner)
    assert "'GOMAXPROCS'" in runner and 'env.pop(k)' in runner
    harness = data['source/scripts/grpc-interop.sh'].decode()
    assert 'CASE_TIMEOUT_SEC="${CASE_TIMEOUT_SEC:-${GRPC_INTEROP_CASE_TIMEOUT:-15}}"' in harness
    assert 'STARTUP_TIMEOUT_SEC="${STARTUP_TIMEOUT_SEC:-${GRPC_INTEROP_STARTUP_TIMEOUT:-5}}"' in harness
    assert 'MAX_ATTEMPTS="${MAX_ATTEMPTS:-${GRPC_INTEROP_MAX_ATTEMPTS:-2}}"' in harness
    for name, expected in [('BASE_CASES', BASE_CASES), ('COMPRESSION_CASES', COMPRESSION_CASES)]:
        group = re.search(r'^' + name + r'=\((.*?)^\)', harness, re.M | re.S)
        assert group is not None and set(group[1].split()) == expected

    native_pins = load('build/native/037-final-native-interop-pins.json')
    native_meta = load('build/native/037-final-native-interop-build185.meta.json')
    assert sha(data['build/native/037-final-native-interop-pins.json']) == record['native_build_pins_sha256']
    assert sha(data['build/native/037-final-native-interop-build185.meta.json']) == record['native_build_meta_sha256'] == native_pins['build_meta_sha256']
    assert native_pins['source'] == native_meta['before']['head'] == SOURCE
    assert native_pins['toolchain'] == native_meta['toolchain'] == '1.85.0'
    assert native_meta['before'] == native_meta['after'] and native_meta['pins_unchanged']
    assert native_meta['exit'] == 0 and native_meta['stop_reason'] is None
    assert '--release' not in native_meta['argv']
    assert sha(data['build/native/capture-final.py']) == native_meta['before']['tools']['capture_script']['sha256']
    assert native_meta['before']['tools']['rustc']['version'].startswith('rustc 1.85.0 ')
    assert native_meta['before']['tools']['cargo']['version'].startswith('cargo 1.85.0 ')
    assert sha(data['build/native/037-final-native-interop-build185.stdout.log']) == native_meta['stdout_sha256']
    assert sha(data['build/native/037-final-native-interop-build185.stderr.log']) == native_meta['stderr_sha256']
    emitted = [json.loads(line) for line in data['build/native/037-final-native-interop-build185.stdout.log'].decode().splitlines()]
    for name, pin in native_pins['selected'].items():
        assert pin['compiler_artifact'] in emitted
        artifact = data['binaries/' + name + '-185']
        assert artifact[:4] == b'\x7fELF' and len(artifact) == pin['bytes']
        assert sha(artifact) == pin['sha256'] == record['before']['executables'][pin['path']]
        assert pin['compiler_artifact']['profile']['opt_level'] == '0'
    for name, expected in native_meta['before']['files'].items():
        assert sha(data['source/' + name]) == expected == record['before']['files'][name]

    go_record = load('build/go/retry/build-record.json')
    verify = load('build/go/retry/module-verification.json')
    first = load('build/go/first/build-record.json')
    assert sha(data['build/go/retry/build-record.json']) == record['go_build_record_sha256']
    assert sha(data['build/go/retry/module-verification.json']) == record['go_module_verification_sha256']
    assert sha(data['build/go/build-final-go-peer.py']) == go_record['runner_sha256']
    assert go_record['status'] == verify['status'] == 'pass'
    assert go_record['input_sha256_before'] == go_record['input_sha256_after']
    assert first['status'] == 'failed_or_stopped' and first['phases'][-1]['peer'] == 'client' and first['phases'][-1]['exit'] == 1
    assert first['input_sha256_before'] == first['input_sha256_after']
    assert all(go_record['input_sha256_before'][name] == expected
               for name, expected in first['input_sha256_before'].items())
    for phase in first['phases']:
        for suffix in ('build.stdout', 'build.stderr', 'resources.jsonl'):
            item = data['build/go/first/' + phase['peer'] + '-' + suffix]
            assert sha(item) == phase[suffix]['sha256'] and len(item) == phase[suffix]['bytes']
    assert b'Forbidden' in data['build/go/first/client-build.stderr']
    assert sha(data['build/go/first/cached-api-preload.json']) == go_record['preload_record_sha256']
    preload = load('build/go/first/cached-api-preload.json')
    assert (preload['module'], preload['version'], preload['go_sum_h1']) in {
        tuple(row) for row in verify['unique_runtime_module_pins']
    }
    for name, item in preload['files'].items():
        if not name.endswith('.zip'):
            retained = data['build/go/preload-inputs/' + name]
            assert sha(retained) == item['sha256'] and len(retained) == item['bytes']
    assert verify['exit'] == 0 and verify['argv'][-2:] == ['mod', 'verify']
    assert sha(data['build/go/retry/module-verify.stdout']) == verify['stdout_sha256']
    assert sha(data['build/go/retry/module-verify.stderr']) == verify['stderr_sha256']
    assert data['build/go/retry/module-verify.stdout'] == b'all modules verified\n'
    sums = {tuple(line.split()) for line in data['source/tests/interop/go/go.sum'].decode().splitlines()}
    tuples = set()
    for phase in go_record['phases']:
        assert phase['exit'] == 0 and phase['guard_stop'] is None
        peer = phase['peer']
        binary = data['binaries/go-interop-' + peer]
        path = phase['argv'][phase['argv'].index('-o') + 1]
        assert sha(binary) == phase['binary_sha256'] == record['before']['executables'][path]
        retained = data['build/go/retry/' + peer + '-module-info.txt']
        assert sha(retained) == phase['module_info_sha256']
        version, embedded = go_build_info(binary)
        assert 'build\tCGO_ENABLED=1\n' in embedded
        assert retained.decode().splitlines()[0] == path + ': ' + version
        assert retained.decode().splitlines()[1:] == ['\t' + line for line in embedded.splitlines()]
        for line in embedded.splitlines():
            words = line.split()
            assert not words or words[0] != '=>', 'Unexpected module replacement'
            if words and words[0] in ('mod', 'dep'):
                assert len(words) == 4
                tuples.add(tuple(words[1:]))
        for suffix in ('build.stdout', 'build.stderr', 'resources.jsonl'):
            item = data['build/go/retry/' + peer + '-' + suffix]
            assert sha(item) == phase[suffix]['sha256'] and len(item) == phase[suffix]['bytes']
    assert tuples == {tuple(row) for row in verify['unique_runtime_module_pins']}
    assert len(tuples) == verify['runtime_module_pin_count'] == 32 and tuples <= sums
    assert verify['all_runtime_module_pins_in_frozen_go_sum']
    assert any(module == 'google.golang.org/grpc' and GO_PIN[:12] in version for module, version, _ in tuples)
    for name in ('tests/interop/go/go.mod', 'tests/interop/go/go.sum'):
        assert sha(data['source/' + name]) == go_record['input_sha256_before'][name] == record['before']['files'][name]

    registry = load('source/tests/interop/cases.json')
    module = types.ModuleType('_sealed_interop_report')
    module.__file__ = '/sealed/source/scripts/interop-report.py'
    sys.modules[module.__name__] = module
    exec(compile(data['source/scripts/interop-report.py'], module.__file__, 'exec'), module.__dict__)
    validator = module.ReportValidator(registry)
    assert [phase['transport'] for phase in record['phases']] == ['plaintext', 'tls', 'mtls']
    for phase, transport, expected_exit, expected_counts in zip(
        record['phases'], ('http2_cleartext', 'http2_tls', 'http2_mtls'), (0, 0, 1),
        ({'passed': 46, 'unsupported': 8}, {'passed': 46, 'unsupported': 8}, {'passed': 18, 'unsupported': 36}),
    ):
        label = phase['transport']
        assert phase['exit'] == expected_exit and phase['guard_stop'] is None
        assert phase['pins_unchanged'] and phase['before'] == phase['after'] == record['before']
        assert phase['os_bound_seconds'] == 1800
        assert phase['argv'][2:5] == ['--skip-build', '--log-dir', '/workspace/pure-protobuf/work/final-native-go-cc4-20261003/' + label + '/logs']
        assert phase['argv'][5:] == ([] if label == 'plaintext' else ['--use-' + label])
        for name, item in phase['artifacts'].items():
            raw = data['runtime/' + label + '/' + name]
            assert len(raw) == item['bytes'] and sha(raw) == item['sha256']
        rows = load('runtime/' + label + '/logs/results.json')['results']
        assert len(rows) == 54
        assert dict(collections.Counter(row['status'] for row in rows)) == expected_counts
        coordinates = {(row['case'], row['peer'], row['direction'], row['transport']) for row in rows}
        expected = {(case, 'pbrs-grpc' if direction == 'kernel_client_to_kernel_server' else 'grpc-go', direction, transport)
                    for case in BASE_CASES | COMPRESSION_CASES for direction in DIRECTIONS}
        assert coordinates == expected
        for row in rows:
            assert row['attempt_count'] == len(row['attempts']) == 1 and not row['is_flaky']
            assert row['first_attempt_status'] == row['status']
            native = row['peer'] == 'pbrs-grpc'
            should_pass = native or (label != 'mtls' and row['case'] in BASE_CASES)
            assert row['status'] == ('passed' if should_pass else 'unsupported')
            if native:
                assert row['peer_pin'] is None
            else:
                assert row['peer_pin'] == GO_PIN
            if should_pass:
                assert row['exit_code'] == row['attempts'][0]['exit_code'] == 0
                assert 0 <= row['duration_ms'] < 15000
                log = Path(row['stdout_log']).name
                assert log.endswith('-attempt1.log')
                assert 'runtime/' + label + '/logs/' + log in data
            else:
                assert row['stdout_log'] is row['stderr_log'] is None
                assert row['attempts'][0]['stdout_log'] is None
                assert row['duration_ms'] == 0
        validation = validator.validate_results(
            [module.TestResult.from_dict(row) for row in rows], suite='standard_interop',
            profile='native', require_matrix=True, required_directions=DIRECTIONS,
            strict_retries=True,
        )
        assert validation.is_valid == (label != 'mtls')
        if label == 'mtls':
            assert len(validation.errors) == 56
            assert len(validation.missing_matrix_rows) == 28
            assert b'FAIL: interop qualification failed' in data['runtime/mtls/stderr.log']
        report = load('runtime/' + label + '/logs/report.json')
        assert report['overall_passed'] and report['target_suite'] is report['target_profile'] is None
        assert report['summary']['passed'] == expected_counts['passed']
        assert report['summary']['unsupported'] == expected_counts['unsupported']
        samples = [json.loads(line) for line in data['runtime/' + label + '/resources.jsonl'].decode().splitlines()]
        assert samples and min(sample['global_free_bytes'] for sample in samples) >= record['global_free_reserve']
        print(f'{label}: {expected_counts}; executed cases passed on first attempts; matrix validator exit expectation {expected_exit}')

    provenance = load('source/provenance.json')
    assert provenance['source'] == SOURCE and provenance['docs_base'] == DOC_BASE
    assert provenance['source_is_ancestor_of_docs_base_exit'] == 1
    assert provenance['source_scope'] == 'isolated cc4 candidate; not current-main qualification'
    assert b'AssertionError' in data['build/packaging-first/package-final-native-go.first.stderr']
    for name in ('scripts/grpc-interop.sh', 'scripts/interop-report.py', 'tests/interop/cases.json'):
        assert sha(data['source/' + name]) == record['before']['files'][name]
    print(f'PASS: {len(manifest["members"])} member hashes, four ELF pins, exact 32 Go module tuples, frozen cases/default bounds, source scopes and fail-closed mTLS matrix replay')
    print('This audit verifies bounded protocol evidence; it does not qualify release performance, full mTLS/C++ matrices, long soaks, or current-main runtime behavior.')


if __name__ == '__main__':
    main()
