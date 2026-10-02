#!/usr/bin/env python3
"""Transparent Valgrind wrapper retaining raw N/2N child evidence (SB-31).

Place a symlink named `valgrind` to this executable first on PATH. Set
PBRS_REPLAY_VALGRIND to the real executable's absolute path and PBRS_REPLAY_RAW
to a JSONL destination. Children remain sequential under the usual collector.
The wrapper retains complete tool stderr (including I refs and child JSON),
forwards both streams and preserves the original exit status and arguments.
"""

import json
import os
import subprocess
import sys


def main():
    command = [os.environ["PBRS_REPLAY_VALGRIND"], *sys.argv[1:]]
    result = subprocess.run(command, capture_output=True, check=False)
    if "--tool=callgrind" in sys.argv[1:]:
        record = {
            "command": command,
            "returncode": result.returncode,
            "stdout": result.stdout.decode("utf-8"),
            "stderr": result.stderr.decode("utf-8"),
        }
        with open(os.environ["PBRS_REPLAY_RAW"], "a", encoding="utf-8") as output:
            output.write(json.dumps(record) + "\n")
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
