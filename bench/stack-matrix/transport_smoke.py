"""Real separate-process shape/compression wiring; deliberately no performance claims.

Usage: python3 bench/stack-matrix/transport_smoke.py --binary PATH --out-dir DIR
Runs all native/tonic-pbrs/tonic-prost directions in plaintext and verified TLS.
Required official-peer gaps remain explicit in the report.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import socket
import ssl
import sys
import threading
import time

import cells
import peertls
import run


class FrameCapture:
    """Observe actual gRPC message compressed flags without decoding HPACK."""
    def __init__(self, upstream):
        self.flags = {"request": [], "response": []}
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen()
        self.port = self.listener.getsockname()[1]
        self.upstream = upstream
        threading.Thread(target=self.accept, daemon=True).start()

    def accept(self):
        while True:
            try:
                client, _ = self.listener.accept()
                server = socket.create_connection(("127.0.0.1", self.upstream))
            except OSError:
                return
            threading.Thread(target=self.copy, args=(client, server, "request"), daemon=True).start()
            threading.Thread(target=self.copy, args=(server, client, "response"), daemon=True).start()

    def copy(self, source, target, direction):
        buf = bytearray()
        streams = {}
        preface = direction == "request"
        try:
            while chunk := source.recv(65536):
                target.sendall(chunk)
                buf.extend(chunk)
                if preface:
                    if len(buf) < 24:
                        continue
                    del buf[:24]
                    preface = False
                while len(buf) >= 9:
                    length = int.from_bytes(buf[:3], "big")
                    if len(buf) < 9 + length:
                        break
                    kind, flags = buf[3], buf[4]
                    stream = int.from_bytes(buf[5:9], "big") & 0x7FFFFFFF
                    payload = buf[9:9 + length]
                    del buf[:9 + length]
                    if kind != 0:  # HTTP/2 DATA only
                        continue
                    if flags & 8:
                        payload = payload[1:len(payload) - payload[0]]
                    data = streams.setdefault(stream, bytearray())
                    data.extend(payload)
                    while len(data) >= 5:
                        size = int.from_bytes(data[1:5], "big")
                        if len(data) < 5 + size:
                            break
                        self.flags[direction].append(data[0])
                        del data[:5 + size]
        except OSError:
            pass
        finally:
            try:
                target.shutdown(socket.SHUT_WR)
            except OSError:
                pass
            source.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    binary = str(args.binary.resolve())
    peers = ["native", "tonic-pbrs", "tonic-prost"]
    results = []
    for server in peers:
        for tls in (False, True):
            tls_spec = peertls.server_spec(server, run.REPO_ROOT) if tls else None
            for compression in ("identity", "gzip"):
                port = run.bench_matrix.find_free_port()
                cmd = [binary, "load-server", f"--port={port}", "--timeout-secs=120",
                       f"--transport={'native' if server == 'native' else 'tonic'}",
                       f"--codec={'prost' if server == 'tonic-prost' else 'pbrs'}",
                       f"--compression={compression}"]
                if tls_spec:
                    cmd.extend(tls_spec.server_args)
                tag = f"{server}-{'tls' if tls else 'plain'}-{compression}"
                server_log = args.out_dir / f"{tag}.server.log"
                with server_log.open("w") as log:
                    proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
                    capture = None
                    tls_probe = None
                    try:
                        run.bench_matrix.wait_for_server_readiness(proc, host="127.0.0.1", port=port, timeout_secs=15)
                        if not tls:
                            capture = FrameCapture(port)
                        elif ssl.HAS_TLSv1_3:
                            context = ssl.create_default_context(cafile=tls_spec.ca_file)
                            # The repository's test CA omits KeyUsage. Python 3.13+
                            # enables an extra X.509 strictness policy by default;
                            # keep chain and hostname verification, using the same
                            # test-root validation policy as earlier Python releases.
                            context.verify_flags &= ~ssl.VERIFY_X509_STRICT
                            context.minimum_version = ssl.TLSVersion.TLSv1_3
                            context.maximum_version = ssl.TLSVersion.TLSv1_3
                            context.set_alpn_protocols(["h2"])
                            probe_log_start = server_log.stat().st_size
                            with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
                                with context.wrap_socket(raw, server_hostname=tls_spec.server_name) as secure:
                                    tls_probe = {"version": secure.version(), "cipher": secure.cipher()[0],
                                                 "alpn": secure.selected_alpn_protocol(),
                                                 "supported": True,
                                                 "source": "independent verified Python TLS probe, not load-session telemetry"}
                                    tls_probe["matches_contract"] = (
                                        tls_probe["version"] == "TLSv1.3"
                                        and tls_probe["cipher"] == "TLS_AES_128_GCM_SHA256"
                                        and tls_probe["alpn"] == "h2")
                                    if server != "native" and not tls_probe["matches_contract"]:
                                        raise RuntimeError(f"tonic TLS provider does not match its cipher contract: {tls_probe}")
                            # Settle the independent probe's server observation before
                            # taking offsets for actual load sessions.
                            if server == "native":
                                deadline = time.monotonic() + 2.0
                                while "TLS_SERVER " not in server_log.read_text()[probe_log_start:]:
                                    if time.monotonic() >= deadline:
                                        raise RuntimeError("native TLS probe observation was not emitted")
                                    time.sleep(0.005)
                        else:
                            tls_probe = {"supported": False, "reason": "Python SSL lacks TLS 1.3; Rust endpoints still verify TLS"}
                        for client in peers:
                            for shape in cells.SHAPES:
                                cell = cells.Cell("client", server, client, shape, "1kib", tls, 1, compression)
                                path = args.out_dir / f"{tag}-{client}-{shape}.json"
                                counts = {key: len(value) for key, value in capture.flags.items()} if capture else {}
                                server_log_start = server_log.stat().st_size
                                cmd = run.load_command(binary, client, f"127.0.0.1:{capture.port if capture else port}", cell,
                                                       50.0, 0.15, 210021, path, tls_spec)
                                cmd = [arg for arg in cmd if not arg.startswith("--stream-msgs=")]
                                cmd.append("--stream-msgs=3")
                                completed = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
                                metrics = json.loads(path.read_text()) if path.exists() else {}
                                success = (completed.returncode == 0 and metrics.get("successful_rpcs", 0) > 0
                                           and metrics.get("failed_rpcs") == 0
                                           and metrics.get("offered_rpcs") == metrics.get("successful_rpcs"))
                                result = {**cells.as_dict(cell), "server_peer": server,
                                          "status": "pass" if success else "fail", "metrics_file": path.name}
                                if tls_probe:
                                    result["server_tls_probe"] = tls_probe
                                    observations = {}
                                    sources = {"client": completed.stderr,
                                               "server": server_log.read_text()[server_log_start:]}
                                    for role, source in sources.items():
                                        prefix = f"TLS_{role.upper()} "
                                        observations[role] = [json.loads(line[len(prefix):]) for line in source.splitlines()
                                                              if line.startswith(prefix)]
                                    result["load_tls_handshakes"] = observations
                                    seen = observations["client"] + observations["server"]
                                    result["observed_load_tls_matches_contract"] = (
                                        all(item["version"] == "TLSv1.3" and item["alpn"] == "h2"
                                            and item["cipher"] == "TLS13_AES_128_GCM_SHA256" for item in seen)
                                        if seen else None)
                                    if client == "native" and len(observations["client"]) != 1:
                                        success = False
                                    if server == "native" and len(observations["server"]) != 1:
                                        success = False
                                    result["status"] = "pass" if success else "fail"
                                if capture:
                                    calls = metrics.get("successful_rpcs", 0)
                                    expected_counts = {
                                        "request": calls * (3 if shape in ("bidi", "client_stream") else 1),
                                        "response": calls * (3 if shape in ("bidi", "server_stream") else 1),
                                    }
                                    deadline = time.monotonic() + 2.0
                                    while (any(len(capture.flags[key]) - counts[key] < count
                                               for key, count in expected_counts.items())
                                           and time.monotonic() < deadline):
                                        time.sleep(0.005)
                                    observed = {key: value[counts[key]:] for key, value in capture.flags.items()}
                                    expected = 1 if compression == "gzip" else 0
                                    wire_ok = all(len(flags) == expected_counts[key] and flags and set(flags) == {expected}
                                                  for key, flags in observed.items())
                                    result["wire_message_flags"] = {key: {"count": len(flags), "flags": sorted(set(flags))}
                                                                     for key, flags in observed.items()}
                                    success = success and wire_ok
                                    result["status"] = "pass" if success else "fail"
                                if not success:
                                    result["error"] = completed.stderr[-1000:]
                                results.append(result)
                                print(f"[{result['status']}] {tag}/{client}/{shape}", flush=True)
                    finally:
                        if capture:
                            capture.listener.close()
                        proc.terminate()
                        try:
                            proc.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            proc.kill()
                            proc.wait()
    report = {"schema": "sb24-transport-smoke/1", "claim_eligible": False,
              "qualification": "diagnostic-loopback", "binary": binary, "cells": results,
              "required_gaps": ["grpc-go/grpc-c++ equivalent open-loop clients",
                                "official peer response compression and runtime effective-settings export",
                                "dedicated-host CPU windows and headroom qualification"]}
    (args.out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return int(any(row["status"] != "pass" for row in results))


if __name__ == "__main__":
    sys.exit(main())
