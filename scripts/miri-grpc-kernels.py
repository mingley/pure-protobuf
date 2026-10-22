#!/usr/bin/env python3
"""Run the embedded HTTP/2 safety checks, rejecting empty test selections."""

from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "h2_backend::proto::streams::lock::tests::",
    "h2_backend::proto::streams::store::tests::",
    "h2_backend::hpack::header::",
    "h2_backend::frame::tests::",
    "h2_backend::header_pool::tests::",
    "h2_backend::server::cache_tests::",
    "h2_backend::frame::headers::test::cache_",
    "rpc_cancel::tests::",
    "lazy_ext::tests::",
    "request::tests::publishing_connect_info_does_not_clone_user_extensions",
    "static_copy::tests::",
    "server::http_name::tests::",
    "grpc_prefix::tests::",
)


def validate_inventory(output, groups=GROUPS):
    tests = [line.removesuffix(": test") for line in output.splitlines()
             if line.endswith(": test")]
    missing = [group for group in groups if not any(test.startswith(group) for test in tests)]
    if missing:
        raise ValueError("missing embedded kernel tests: " + ", ".join(missing))
    return tests


def main():
    command = ["cargo", "miri", "test", "--locked", "-p", "pbrs-grpc", "--lib"]
    inventory = subprocess.run([*command, "--", "--list"], cwd=ROOT,
                               check=False, capture_output=True, text=True)
    sys.stdout.write(inventory.stdout)
    sys.stderr.write(inventory.stderr)
    inventory.check_returncode()
    validate_inventory(inventory.stdout)
    for group in GROUPS:
        subprocess.run([*command, group, "--", "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
