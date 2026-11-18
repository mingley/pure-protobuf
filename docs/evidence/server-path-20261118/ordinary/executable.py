#!/usr/bin/env python3
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
for line in root.open():
    if not line.startswith("{"):
        continue
    message = json.loads(line)
    if message.get("reason") != "compiler-artifact":
        continue
    if message["target"]["name"] == "server_path_probe" and message["executable"]:
        executable = Path(message["executable"])
        assert executable.is_file(), executable
        print(executable)
        break
else:
    raise SystemExit("missing Cargo example executable")
