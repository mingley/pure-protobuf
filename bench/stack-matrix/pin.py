"""SB-11 CPU pinning: taskset on Linux, explicit unsupported elsewhere.

Pinning is part of the cell definition, not a silent best effort: when the
host cannot pin, the report records why and the numbers stay diagnostic.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from typing import Any, Dict, List, Tuple


def describe(cpus: int, offset: int = 0) -> Dict[str, Any]:
    """Describe how a `cpus`-wide pin at `offset` would apply here."""
    if cpus <= 0:
        raise ValueError(f"cpus must be positive, got {cpus}")
    if sys.platform != "linux":
        return {
            "supported": False,
            "method": "none",
            "reason": f"CPU pinning needs Linux taskset/cpusets; this host is {sys.platform}",
        }
    taskset = shutil.which("taskset")
    if taskset is None:
        return {
            "supported": False,
            "method": "none",
            "reason": "taskset not found on PATH",
        }
    total = os.cpu_count() or 0
    if offset + cpus > total:
        return {
            "supported": False,
            "method": "none",
            "reason": f"pin {offset}-{offset + cpus - 1} exceeds {total} CPUs; refusing to clamp silently",
        }
    return {
        "supported": True,
        "method": f"taskset -c {offset}-{offset + cpus - 1}",
        "cpus": list(range(offset, offset + cpus)),
        "taskset": taskset,
    }


def wrap(cmd: List[str], cpus: int, offset: int = 0) -> Tuple[List[str], Dict[str, Any]]:
    """Prefix `cmd` with taskset when supported; return (cmd, pin_record)."""
    state = describe(cpus, offset)
    if not state["supported"]:
        return list(cmd), state
    assert state["taskset"] is not None
    pinned = [state["taskset"], "-c", f"{offset}-{offset + cpus - 1}"] + list(cmd)
    return pinned, state


def actual_affinity(pid: int) -> Dict[str, Any]:
    """Read back the kernel's affinity for a live PID (Linux only)."""
    taskset = shutil.which("taskset")
    if sys.platform != "linux" or taskset is None:
        return {"supported": False, "reason": "taskset unavailable"}
    try:
        out = subprocess.run(
            [taskset, "-pc", str(pid)],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError) as e:
        return {"supported": False, "reason": f"taskset -pc failed: {e}"}
    if out.returncode != 0:
        return {"supported": False, "reason": out.stderr.strip()[-200:]}
    return {"supported": True, "affinity": out.stdout.strip()[-200:]}
