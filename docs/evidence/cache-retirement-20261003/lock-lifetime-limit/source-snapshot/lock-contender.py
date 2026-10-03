"""Independent Linux x86_64 lock contender. Exit0 only for the requested observation."""
import argparse
import ctypes
import errno
import fcntl
import json
import os
import sys


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--path", required=True)
    p.add_argument("--kind", choices=("posix", "flock", "ofd"), required=True)
    p.add_argument("--expect", choices=("blocked", "acquired"), required=True)
    args = p.parse_args()
    fd = os.open(args.path, os.O_RDWR | os.O_NOFOLLOW)
    observed = "error"
    try:
        try:
            if args.kind == "posix":
                fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            elif args.kind == "flock":
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            else:
                class Lock(ctypes.Structure):
                    _fields_ = [("type", ctypes.c_short), ("whence", ctypes.c_short),
                                ("start", ctypes.c_longlong), ("length", ctypes.c_longlong), ("pid", ctypes.c_int)]
                if not (sys.platform == "linux" and os.uname().machine == "x86_64"
                        and ctypes.sizeof(ctypes.c_void_p) == 8 and ctypes.sizeof(Lock) == 32
                        and [getattr(Lock, x).offset for x in ("type", "whence", "start", "length", "pid")]
                        == [0, 2, 8, 16, 24] and getattr(fcntl, "F_OFD_SETLK", 37) == 37):
                    raise RuntimeError("unqualified Linux x86_64 OFD ABI")
                fcntl.fcntl(fd, 37, bytes(Lock(fcntl.F_WRLCK, os.SEEK_SET, 0, 0, 0)))
            observed = "acquired"
        except OSError as exc:
            if exc.errno not in (errno.EACCES, errno.EAGAIN):
                raise
            observed = "blocked"
        st = os.fstat(fd)
        print(json.dumps({"kind": args.kind, "expected": args.expect, "observed": observed,
                          "inode": st.st_ino, "device": st.st_dev, "ofd_command": 37}))
        return 0 if observed == args.expect else 3
    finally:
        os.close(fd)


if __name__ == "__main__":
    sys.exit(main())
