#!/usr/bin/env python3
"""Unqualified CG-19 codegen/consumer cost diagnostic; no network or new dependencies."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import re
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPORA = {"small": (6, 2), "100": (100, 5), "1000": (1000, 20)}
DEFAULT_SEED = 190019
REFERENCE_REVISION = "35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03"
REFERENCE_PROTOC_SHA256 = "e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940"
REFERENCE_RUNTIME_VERSION = "4.35.1-release"
REFERENCE_RUNTIME_CHECKSUM = "a169648cc34d6f327fea8919ca63f38261fb26405fde8879745dc0a483db328e"
REFERENCE_MACROS_CHECKSUM = "fddf7218148e92862511304cfde3ef07ce109e1f03e830601833b9f0cb901a75"
REFERENCE_RUST_OPT = "experimental-codegen=enabled,kernel=upb"
FIELD_TYPES = (
    "string",
    "bytes",
    "uint64",
    "int32",
    "bool",
    "repeated uint32",
    "repeated string",
    "map<string, uint32>",
)

# SB-09 message-generator matrix. "pbrs" keeps the exact CG-19 flow;
# peers run the same phases on the same corpora under cases/<case>/gen/<name>/.
MESSAGE_GENERATORS = ("pbrs", "prost", "buffa", "v4")
# SB-09 stub matrix, compared separately on service-carrying corpora.
STUB_GENERATORS = ("pbrs-native", "pbrs-tonic", "tonic-build")
# case -> (message count, file count); one service per file (unary + streaming).
STUB_CORPORA = {"svc-small": (6, 2), "svc-100": (100, 5)}
# SB-09 realistic corpora: pinned third-party protos, fetched once into
# target/codegen-bench/vendor and hash-verified on every run. Every
# generator compiles the FULL file closure as inputs (subset + support):
# pbrs/buffa/v4 need support messages for cross-file paths, and prost
# compiles the closure on its own; the generated-code volume each tool
# needs is part of what the matrix measures. Consumers roundtrip the
# SUBSET top-level messages only (nested types excluded, documented).
OTEL_REF = "8654ab7a5a43ca25fe8046e59dcd6935c3f76de0"  # opentelemetry-proto v1.7.0
GAPI_REF = "9415ba048aa587b1b2df2b96fc00aa009c831597"  # googleapis (no tags; pinned commit)
ENVOY_REF = "b579d07d3ad7ee11d32b105e91a5a39ad24718d7"  # envoyproxy/envoy v1.39.1
UDPA_REF = "e8cd3a4bb307e2c810cffff99f93e96e6d7fee85"  # cncf/udpa (archived; pinned commit)
XDS_REF = "7f1daf1720fc185f3b63f70d25aefaeef83d88d7"  # cncf/xds (pinned commit)
PGV_REF = "92b9a7df69ca9f71bfc492f7a90adf4d36eab569"  # bufbuild/protoc-gen-validate v1.3.3
VENDOR_BASE = "https://raw.githubusercontent.com"
# Destination relpath -> (repo, ref, repo_path, sha256).
VENDOR_FILES = {
    # OTLP v1.7.0 (in-repo closure, no WKT).
    "opentelemetry/proto/trace/v1/trace.proto": (
        "open-telemetry/opentelemetry-proto", OTEL_REF,
        "opentelemetry/proto/trace/v1/trace.proto",
        "94b0201460115874b71a0316ea7f9329f222a1afec9f811251f3a98c25bb5b45",
    ),
    "opentelemetry/proto/metrics/v1/metrics.proto": (
        "open-telemetry/opentelemetry-proto", OTEL_REF,
        "opentelemetry/proto/metrics/v1/metrics.proto",
        "f54b7bdc4effc6c8cc9b01dff316e3aca19341479bec42cb72adc53e23e3663a",
    ),
    "opentelemetry/proto/logs/v1/logs.proto": (
        "open-telemetry/opentelemetry-proto", OTEL_REF,
        "opentelemetry/proto/logs/v1/logs.proto",
        "91e42ca14a09f7de338a7870583319445fcf5b9d9e7090effa4a52958f603ca8",
    ),
    "opentelemetry/proto/common/v1/common.proto": (
        "open-telemetry/opentelemetry-proto", OTEL_REF,
        "opentelemetry/proto/common/v1/common.proto",
        "f9eba928880a84964aedf178c34d0ac6245eb4a520d7cab383f932b4bcbca4ad",
    ),
    "opentelemetry/proto/resource/v1/resource.proto": (
        "open-telemetry/opentelemetry-proto", OTEL_REF,
        "opentelemetry/proto/resource/v1/resource.proto",
        "be315021ab29992f38555a42fd4e0b15a761b13359c5f8d54c918eec55586715",
    ),
    # googleapis (annotations carry only options/extensions, no messages).
    "google/api/annotations.proto": (
        "googleapis/googleapis", GAPI_REF, "google/api/annotations.proto",
        "e79ea741cb605a65e78ca322174764a4af9fde1962c1631e12b84c4934ba9a6c",
    ),
    "google/api/http.proto": (
        "googleapis/googleapis", GAPI_REF, "google/api/http.proto",
        "4a4d9be6a5c7f1989c93c25c71b48ff1b401645790b8b978ad34d579e29c4a2a",
    ),
    "google/api/httpbody.proto": (
        "googleapis/googleapis", GAPI_REF, "google/api/httpbody.proto",
        "3bc84638659531d3bd6154e4a867ee763b41c6ad15d1d707e0aacf41df8f1901",
    ),
    "google/rpc/status.proto": (
        "googleapis/googleapis", GAPI_REF, "google/rpc/status.proto",
        "3b5c712455570ac4342dd3c521c4c11011652ae9a0fbca75ba22fcc45c6e1991",
    ),
    "google/rpc/code.proto": (
        "googleapis/googleapis", GAPI_REF, "google/rpc/code.proto",
        "9993be65e050c30ced246951659dbe0a13663b77cf57bcaa4c0ed4248480fb80",
    ),
    # Envoy v1.39.1 (served under api/, dest strips that prefix).
    "envoy/annotations/deprecation.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/annotations/deprecation.proto",
        "bc72a2deefc60cacdb0f49380e1c92028078e219b02993d6772e2854a8929b36",
    ),
    "envoy/config/core/v3/address.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/address.proto",
        "33ec035d4e2a4b0fb20ab4a1b7790661404f7330ff322a74868fc0d7848fe79b",
    ),
    "envoy/config/core/v3/backoff.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/backoff.proto",
        "f347a6a36616ea6735ff031b546d10cc79aaae17da3a60e8174479c7f75ac53e",
    ),
    "envoy/config/core/v3/base.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/base.proto",
        "5a6a07551e0db451d6984d905aa45269f3134e00b2160ca19dc8875245edab7c",
    ),
    "envoy/config/core/v3/extension.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/extension.proto",
        "860106c686cab11106159f462243ce4009cbc405f197df7df8ab423af71a82a7",
    ),
    "envoy/config/core/v3/http_uri.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/http_uri.proto",
        "ef5c59234f0be07a23327d695afc65435ef5052941a63184795b0d300fccf1e6",
    ),
    "envoy/config/core/v3/socket_option.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/config/core/v3/socket_option.proto",
        "78937443cba81f3a60a18b71debc9962b27b484ec55ea79892cfb1deabda9058",
    ),
    "envoy/service/discovery/v3/ads.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/service/discovery/v3/ads.proto",
        "05afb7539d786271333895f54c25d12773a61515c19257047ce3bc060715eced",
    ),
    "envoy/service/discovery/v3/discovery.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/service/discovery/v3/discovery.proto",
        "c875f8e55176477fa1d1072499e300b383b397a2deabcdfa678f006e09e5dfa7",
    ),
    "envoy/type/v3/percent.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/type/v3/percent.proto",
        "b5b5ea7f6264f465ce4b398601e855b62395e3c83e4bb7e0d9d4ebae788210ca",
    ),
    "envoy/type/v3/semantic_version.proto": (
        "envoyproxy/envoy", ENVOY_REF, "api/envoy/type/v3/semantic_version.proto",
        "71cc18d6cbbccd0e3a027d3499bbdbe0a7e74cdf125565f36843736027cc8cc5",
    ),
    "udpa/annotations/status.proto": (
        "cncf/udpa", UDPA_REF, "udpa/annotations/status.proto",
        "564149dd66c6c92c8e56237e02960cc0629cbd7a0cfd0df846d3d6dc4e114aef",
    ),
    "udpa/annotations/versioning.proto": (
        "cncf/udpa", UDPA_REF, "udpa/annotations/versioning.proto",
        "657c807e024a3fcd6b47c56e86a5d27b5b64f9e56c540164eee6b1ae9c8d2d02",
    ),
    "udpa/annotations/migrate.proto": (
        "cncf/udpa", UDPA_REF, "udpa/annotations/migrate.proto",
        "e2091c88268446db1bc5e9ff45a1e9ab35a526983d8810d2e03081b95bde67aa",
    ),
    "xds/core/v3/context_params.proto": (
        "cncf/xds", XDS_REF, "xds/core/v3/context_params.proto",
        "912801a580c04553d75a0099d16206a668dd609277c860b6d05d86f8cbddc11c",
    ),
    "xds/annotations/v3/status.proto": (
        "cncf/xds", XDS_REF, "xds/annotations/v3/status.proto",
        "0145b2d9437ace6c1201453cd57d44c96e2e89894e502c2d869e8ee2b59b8c19",
    ),
    "validate/validate.proto": (
        "bufbuild/protoc-gen-validate", PGV_REF, "validate/validate.proto",
        "68c9625ebe0668605a37670db1759ceb03864bc07c52eee919b049347cbc018c",
    ),
}
# WKT support files, copied from the repo's pinned third_party/protobuf
# checkout (no fetch); every one must declare package google.protobuf.
REALISTIC_WKT = ("any", "descriptor", "duration", "struct", "wrappers", "timestamp")
REALISTIC_CORPORA = {
    "otlp": {
        "inputs": [
            "opentelemetry/proto/trace/v1/trace.proto",
            "opentelemetry/proto/metrics/v1/metrics.proto",
            "opentelemetry/proto/logs/v1/logs.proto",
            "opentelemetry/proto/common/v1/common.proto",
            "opentelemetry/proto/resource/v1/resource.proto",
        ],
        "support": [],
        "wkt": (),
        "packages": {
            "opentelemetry/proto/trace/v1/trace.proto": "opentelemetry.proto.trace.v1",
            "opentelemetry/proto/metrics/v1/metrics.proto": "opentelemetry.proto.metrics.v1",
            "opentelemetry/proto/logs/v1/logs.proto": "opentelemetry.proto.logs.v1",
            "opentelemetry/proto/common/v1/common.proto": "opentelemetry.proto.common.v1",
            "opentelemetry/proto/resource/v1/resource.proto": "opentelemetry.proto.resource.v1",
        },
        "messages": {
            "opentelemetry/proto/trace/v1/trace.proto": [
                "TracesData", "ResourceSpans", "ScopeSpans", "Span", "Status",
            ],
            "opentelemetry/proto/metrics/v1/metrics.proto": [
                "MetricsData", "ResourceMetrics", "ScopeMetrics", "Metric", "Gauge", "Sum",
                "Histogram", "ExponentialHistogram", "Summary", "NumberDataPoint",
                "HistogramDataPoint", "ExponentialHistogramDataPoint", "SummaryDataPoint",
                "Exemplar",
            ],
            "opentelemetry/proto/logs/v1/logs.proto": [
                "LogsData", "ResourceLogs", "ScopeLogs", "LogRecord",
            ],
            "opentelemetry/proto/common/v1/common.proto": [
                "AnyValue", "ArrayValue", "KeyValueList", "KeyValue",
                "InstrumentationScope", "EntityRef",
            ],
            "opentelemetry/proto/resource/v1/resource.proto": ["Resource"],
        },
        "prost_omitted_packages": [],
        "exclude": {},
    },
    "googleapis": {
        "inputs": [
            "google/api/annotations.proto",
            "google/api/http.proto",
            "google/api/httpbody.proto",
        ],
        "support": [],
        "wkt": ("any", "descriptor"),
        "packages": {
            "google/api/annotations.proto": "google.api",
            "google/api/http.proto": "google.api",
            "google/api/httpbody.proto": "google.api",
        },
        "messages": {
            "google/api/annotations.proto": [],
            "google/api/http.proto": ["Http", "HttpRule", "CustomHttpPattern"],
            "google/api/httpbody.proto": ["HttpBody"],
        },
        "prost_omitted_packages": [],
        "exclude": {},
    },
    "envoy-core": {
        "inputs": [
            "envoy/config/core/v3/socket_option.proto",
            "envoy/config/core/v3/address.proto",
            "envoy/config/core/v3/backoff.proto",
            "envoy/config/core/v3/extension.proto",
            "envoy/config/core/v3/http_uri.proto",
            "envoy/type/v3/percent.proto",
            "envoy/type/v3/semantic_version.proto",
        ],
        "support": [
            "envoy/annotations/deprecation.proto",
            "udpa/annotations/status.proto",
            "udpa/annotations/versioning.proto",
            "udpa/annotations/migrate.proto",
            "validate/validate.proto",
        ],
        "wkt": ("any", "descriptor", "duration", "wrappers", "timestamp"),
        "packages": {
            "envoy/config/core/v3/socket_option.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/address.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/backoff.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/extension.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/http_uri.proto": "envoy.config.core.v3",
            "envoy/type/v3/percent.proto": "envoy.type.v3",
            "envoy/type/v3/semantic_version.proto": "envoy.type.v3",
            "envoy/annotations/deprecation.proto": "envoy.annotations",
            "udpa/annotations/status.proto": "udpa.annotations",
            "udpa/annotations/versioning.proto": "udpa.annotations",
            "udpa/annotations/migrate.proto": "udpa.annotations",
            "validate/validate.proto": "validate",
        },
        "messages": {
            "envoy/config/core/v3/socket_option.proto": ["SocketOption", "SocketOptionsOverride"],
            "envoy/config/core/v3/address.proto": [
                "Pipe", "EnvoyInternalAddress", "SocketAddress", "TcpKeepalive",
                "ExtraSourceAddress", "BindConfig", "Address", "CidrRange",
            ],
            "envoy/config/core/v3/backoff.proto": ["BackoffStrategy"],
            "envoy/config/core/v3/extension.proto": ["TypedExtensionConfig"],
            "envoy/config/core/v3/http_uri.proto": ["HttpUri"],
            "envoy/type/v3/percent.proto": ["Percent", "FractionalPercent"],
            "envoy/type/v3/semantic_version.proto": ["SemanticVersion"],
        },
        # deprecation.proto carries only options, so prost emits no file for it.
        "prost_omitted_packages": ["envoy.annotations"],
        "exclude": {},
    },
    "envoy-discovery": {
        "inputs": [
            "envoy/service/discovery/v3/discovery.proto",
            "envoy/service/discovery/v3/ads.proto",
        ],
        "support": [
            "envoy/annotations/deprecation.proto",
            "envoy/config/core/v3/address.proto",
            "envoy/config/core/v3/backoff.proto",
            "envoy/config/core/v3/base.proto",
            "envoy/config/core/v3/extension.proto",
            "envoy/config/core/v3/http_uri.proto",
            "envoy/config/core/v3/socket_option.proto",
            "envoy/type/v3/percent.proto",
            "envoy/type/v3/semantic_version.proto",
            "udpa/annotations/status.proto",
            "udpa/annotations/versioning.proto",
            "udpa/annotations/migrate.proto",
            "xds/core/v3/context_params.proto",
            "xds/annotations/v3/status.proto",
            "validate/validate.proto",
            "google/rpc/status.proto",
            "google/rpc/code.proto",
        ],
        "wkt": ("any", "descriptor", "duration", "struct", "wrappers", "timestamp"),
        "packages": {
            "envoy/service/discovery/v3/discovery.proto": "envoy.service.discovery.v3",
            "envoy/service/discovery/v3/ads.proto": "envoy.service.discovery.v3",
            "envoy/annotations/deprecation.proto": "envoy.annotations",
            "envoy/config/core/v3/address.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/backoff.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/base.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/extension.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/http_uri.proto": "envoy.config.core.v3",
            "envoy/config/core/v3/socket_option.proto": "envoy.config.core.v3",
            "envoy/type/v3/percent.proto": "envoy.type.v3",
            "envoy/type/v3/semantic_version.proto": "envoy.type.v3",
            "udpa/annotations/status.proto": "udpa.annotations",
            "udpa/annotations/versioning.proto": "udpa.annotations",
            "udpa/annotations/migrate.proto": "udpa.annotations",
            "xds/core/v3/context_params.proto": "xds.core.v3",
            "xds/annotations/v3/status.proto": "xds.annotations.v3",
            "validate/validate.proto": "validate",
            "google/rpc/status.proto": "google.rpc",
            "google/rpc/code.proto": "google.rpc",
        },
        "messages": {
            "envoy/service/discovery/v3/discovery.proto": [
                "ResourceLocator", "ResourceName", "ResourceError", "DiscoveryRequest",
                "DiscoveryResponse", "DeltaDiscoveryRequest", "DeltaDiscoveryResponse",
                "DynamicParameterConstraints", "Resource",
            ],
            "envoy/service/discovery/v3/ads.proto": ["AdsDummy"],
        },
        "prost_omitted_packages": ["envoy.annotations"],
        # v4 --rust_out re-exports every input file into one flat namespace;
        # PackageVersionStatus is an enum in both udpa/annotations and
        # xds/annotations/v3, so the v4 consumer cannot compile (E0659).
        "exclude": {
            "v4": "flat namespace collision: PackageVersionStatus in udpa/annotations "
                  "and xds/annotations/v3 (E0659)",
        },
    },
}
# Peer generator pins. The harness resolves these --offline from the local
# registry and records lockfile hashes per cell; a missing crate fails closed.
PROST_VERSION = "0.14.4"
PROST_TYPES_VERSION = "0.14.4"
BUFFA_VERSION = "0.9.1"
PROST013_VERSION = "0.13.5"
TONIC_BUILD_VERSION = "0.13.1"
TONIC013_VERSION = "0.13.1"
TONIC014_VERSION = "0.14.6"
TOKIO_VERSION = "1.48.0"
TOKIO_STREAM_VERSION = "0.1.17"
HTTP_VERSION = "1.3.1"
# Matrix peers whose generation is byte-and-mtime deterministic like pbrs;
# every other peer verifies bytes and counts mtime rewrites instead.
STRICT_PEERS = frozenset({"pbrs-native", "pbrs-tonic"})
PBRS_STUB_ENV = {"pbrs-native": "native", "pbrs-tonic": "tonic"}
# Seeded corpora share one package, so prost emits one file for every case.
PROST_PACKAGE_FILE = "bench.cg19.rs"
# generator -> snapshot entrypoint. pbrs and the v4 reference keep their
# existing dedicated checks; matrix peers verify bytes on unchanged inputs
# and count mtime rewrites instead of failing. Expected output sets come
# from peer_expected_files: buffa emits per-input files, prost one per package.
PEER_ENTRYPOINT = {
    "prost": PROST_PACKAGE_FILE,
    "buffa": "mod.rs",
    "v4": "generated.rs",
    "pbrs-native": "mod.rs",
    "pbrs-tonic": "mod.rs",
    "tonic-build": PROST_PACKAGE_FILE,
}
BUFFA_PACKAGE_FILE = "bench.cg19.mod.rs"


def realistic_pbrs_expected(names: list[str]) -> frozenset[str]:
    """pbrs emits mod.rs, a path-mirrored file per input, and a flat
    basename file only when that stem is unique across inputs
    (descriptors.rs: stem_counts == 1); colliding basenames such as the
    three status.proto files on envoy-discovery get mirrored files only."""
    stems = [name.removesuffix(".proto") for name in names]
    basenames = [Path(stem).name for stem in stems]
    counts: dict[str, int] = {}
    for base in basenames:
        counts[base] = counts.get(base, 0) + 1
    files = {"mod.rs"}
    for stem, base in zip(stems, basenames):
        files.add(f"{stem}.rs")
        if counts[base] == 1:
            files.add(f"{base}.rs")
    return frozenset(files)


def realistic_buffa_core(
    names: list[str], case: str, content: dict[str, dict[str, bool]],
) -> frozenset[str]:
    """buffa emits mod.rs, per-input files named by the dotted input path,
    and one per-package sidecar. Per-input outputs depend on content:
    .rs for files defining messages or enums, .__view.rs for files
    defining messages, .__ext.rs for extension-only files. Oneof
    auxiliary files are validated separately (see BUFFA_AUX_SUFFIXES)."""
    files = {"mod.rs"}
    for name in names:
        dotted = name.removesuffix(".proto").replace("/", ".")
        kinds = content[name]
        if kinds["messages"] or kinds["enums"]:
            files.add(f"{dotted}.rs")
        if kinds["messages"]:
            files.add(f"{dotted}.__view.rs")
        if kinds["extends"] and not (kinds["messages"] or kinds["enums"]):
            files.add(f"{dotted}.__ext.rs")
    for package in realistic_closure_packages(case):
        files.add(f"{package}.mod.rs")
    return frozenset(files)


# buffa auxiliary outputs beyond the core set; every extra file must be
# one of these suffixes on a core per-input stem, never anything else.
BUFFA_AUX_SUFFIXES = (".__oneof.rs", ".__view_oneof.rs", ".__ext.rs")


def assert_buffa_realistic_outputs(
    case: str, generator: str, before: dict, expected: frozenset[str],
) -> None:
    missing = sorted(expected - before.keys())
    if missing:
        raise BenchmarkError(f"{case}/{generator}: missing core outputs: {missing}")
    core_stems = {name.removesuffix(".rs") for name in expected if name != "mod.rs"}
    for extra in sorted(before.keys() - expected):
        stem = next(
            (
                extra.removesuffix(suffix)
                for suffix in BUFFA_AUX_SUFFIXES
                if extra.endswith(suffix)
            ),
            None,
        )
        if stem is None or stem not in core_stems:
            raise BenchmarkError(
                f"{case}/{generator}: unexpected auxiliary output {extra}"
            )


def realistic_expected_files(generator: str, names: list[str], case: str) -> frozenset[str]:
    if generator == "prost":
        return frozenset(prost_package_files(case))
    if generator == "v4":
        return frozenset(
            {
                v4_entrypoint_rel(case),
                *(name.removesuffix(".proto") + ".u.pb.rs" for name in names),
            }
        )
    if generator == "buffa":
        raise BenchmarkError(
            "buffa realistic outputs need per-file content classes; "
            "use realistic_buffa_core instead"
        )
    if generator == "pbrs":
        return realistic_pbrs_expected(names)
    raise BenchmarkError(f"generator {generator} does not run on realistic case {case}")


def peer_expected_files(
    generator: str, names: list[str], case: str | None = None,
) -> frozenset[str]:
    if case in REALISTIC_CORPORA:
        assert case is not None
        return realistic_expected_files(generator, names, case)
    if generator == "prost":
        return frozenset({PROST_PACKAGE_FILE})
    if generator == "buffa":
        files = {"mod.rs", BUFFA_PACKAGE_FILE}
        for name in names:
            stem = name.removesuffix(".proto")
            files.add(f"{stem}.rs")
            files.add(f"{stem}.__view.rs")
        return frozenset(files)
    if generator == "v4":
        return frozenset(
            {"generated.rs", *(name.removesuffix(".proto") + ".u.pb.rs" for name in names)}
        )
    if generator in ("pbrs-native", "pbrs-tonic"):
        return frozenset(
            {"mod.rs", *(name.removesuffix(".proto") + ".rs" for name in names)}
        )
    if generator == "tonic-build":
        return frozenset({PROST_PACKAGE_FILE})
    raise BenchmarkError(f"no snapshot table for generator: {generator}")


def v4_generation_command(
    protoc: str, proto_dir: Path, generated: Path, names: list[str],
) -> list[str]:
    return [
        protoc, f"--proto_path={proto_dir}",
        f"--rust_out={generated}", f"--rust_opt={REFERENCE_RUST_OPT}", *names,
    ]


def resolve_pinned_protoc(candidate: Path | None = None) -> Path:
    if candidate is None:
        candidate = ROOT / "target" / "pinned-protoc-build" / "protoc"
    if not candidate.is_file() or not os.access(candidate, os.X_OK):
        raise BenchmarkError(
            "v4 generator needs the pinned protoc "
            f"(missing {candidate}); run scripts/build-pinned-protoc.sh"
        )
    try:
        version = subprocess.check_output(
            [str(candidate), "--version"], text=True, timeout=30,
        ).strip()
        help_text = subprocess.check_output(
            [str(candidate), "--help"], text=True, timeout=30,
        )
    except subprocess.SubprocessError as exc:
        raise BenchmarkError(f"pinned protoc --version failed: {exc}") from exc
    digest = sha256(candidate)
    if version != "libprotoc 35.1" or "--rust_out=OUT_DIR" not in help_text:
        raise BenchmarkError(
            "pinned protoc is not v35.1 Rust generator "
            f"(version={version!r} sha256={digest}); run scripts/build-pinned-protoc.sh"
        )
    try:
        source_head = subprocess.check_output(
            ["git", "-C", str(ROOT / "third_party" / "protobuf"), "rev-parse", "HEAD"],
            text=True,
            timeout=30,
        ).strip()
    except subprocess.SubprocessError as exc:
        raise BenchmarkError(f"pinned protobuf source check failed: {exc}") from exc
    if source_head != REFERENCE_REVISION:
        raise BenchmarkError(
            "pinned protoc is not v35.1 Rust generator "
            f"(version={version!r} sha256={digest} source={source_head}); "
            "run scripts/build-pinned-protoc.sh"
        )
    return candidate


class BenchmarkError(Exception):
    """An incomplete or invalid measurement, never a successful comparison."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relative(path: Path, run_dir: Path) -> str:
    return path.relative_to(run_dir).as_posix()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def render_proto(seed: int, messages: int, files: int, index: int) -> str:
    if messages % files or not 0 <= index < files:
        raise ValueError("invalid corpus distribution")
    per_file = messages // files
    lines = ['syntax = "proto3";', "package bench.cg19;"]
    if index:
        lines.append(f'import "part_{index - 1:02d}.proto";')
    lines.append("")
    for offset in range(per_file):
        number = index * per_file + offset
        lines.append(f"message Message{number:04d} {{")
        for field in range(1, 5):
            key = f"cg19-v1:{seed}:{messages}:{number}:{field}".encode("ascii")
            kind = FIELD_TYPES[hashlib.sha256(key).digest()[0] % len(FIELD_TYPES)]
            lines.append(f"  {kind} field_{field} = {field};")
        if index and offset == 0:
            lines.append(f"  Message{number - per_file:04d} previous = 5;")
        lines.extend(("}", ""))
    return "\n".join(lines)


def render_proto_with_services(seed: int, messages: int, files: int, index: int) -> str:
    base = render_proto(seed, messages, files, index)
    first = index * (messages // files)
    return base + (
        f"\nservice Service{index:02d} {{\n"
        f"  rpc Get (Message{first:04d}) returns (Message{first + 1:04d});\n"
        f"  rpc Watch (Message{first:04d}) returns (stream Message{first + 1:04d});\n"
        "}\n"
    )


def stub_services(case: str) -> list[tuple[str, int, int]]:
    messages, files = STUB_CORPORA[case]
    per_file = messages // files
    return [
        (f"Service{index:02d}", index * per_file, index * per_file + 1)
        for index in range(files)
    ]


def render_consumer(messages: int, marker: int, reference: bool = False) -> str:
    runtime = "protobuf" if reference else "pbrs"
    generated = "generated" if reference else "bench::cg19"
    lines = [
        ('#[path = "../generated/generated.rs"] mod generated;'
         if reference else
         'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));'),
        "",
        f"fn roundtrip<T: {runtime}::Parse + {runtime}::Serialize>(msg: T) -> usize {{",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip({generated}::Message{number:04d}::new());"
        for number in range(messages)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def manifest(name: str, reference: bool = False) -> str:
    if reference:
        dependency = f'protobuf = "={REFERENCE_RUNTIME_VERSION}"'
    elif name == "cg19-generator":
        dependency = f"pbrs = {{ path = {json.dumps(str(ROOT))} }}"
    else:
        profile = os.environ.get("SB09_PBRS_RUNTIME_PROFILE", "default")
        if profile == "default":
            dependency = f"pbrs = {{ path = {json.dumps(str(ROOT))} }}"
        elif profile == "minimal":
            dependency = (
                f"pbrs = {{ default-features = false, path = {json.dumps(str(ROOT))} }}"
            )
        elif profile == "json-text":
            dependency = (
                "pbrs = { default-features = false, features = [\"json\", \"text\"], "
                f"path = {json.dumps(str(ROOT))} }}"
            )
        else:
            raise BenchmarkError(
                "SB09_PBRS_RUNTIME_PROFILE must be default, minimal, or json-text"
            )
    return (
        f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2024"\n'
        'publish = false\n\n[workspace]\n\n[dependencies]\n'
        f"{dependency}\n\n"
        '[profile.release]\nopt-level = 3\nlto = "thin"\ncodegen-units = 1\n'
    )


def pbrs_profile(env: dict[str, str], generator: str = "pbrs") -> dict:
    """Report the existing driver/manifest knobs without changing their defaults."""
    keys = (
        "SB09_PBRS_EMIT_REFLECTION", "SB09_PBRS_EMIT_JSON", "SB09_PBRS_EMIT_TEXT",
        "SB09_PBRS_SHARED_DESCRIPTOR_SET", "SB09_PBRS_RUNTIME_PROFILE", "SB09_PBRS_STUBS",
    )
    raw = {key: env.get(key) for key in keys}

    def boolean(key: str, default: bool) -> bool:
        value = raw[key]
        if value is None:
            return default
        if value.lower() in ("1", "true"):
            return True
        if value.lower() in ("0", "false"):
            return False
        raise BenchmarkError(f"expected boolean {key}, got {value!r}")

    reflection = boolean("SB09_PBRS_EMIT_REFLECTION", True)
    runtime = env.get("SB09_PBRS_RUNTIME_PROFILE", "default") if generator == "pbrs" else "default"
    if runtime not in ("default", "minimal", "json-text"):
        raise BenchmarkError("SB09_PBRS_RUNTIME_PROFILE must be default, minimal, or json-text")
    ordinary_stubs = PBRS_STUB_ENV.get(generator, "none")
    stubs = PBRS_STUB_ENV.get(generator, env.get("SB09_PBRS_STUBS") or "none")
    resolved = {
        "emit_reflection": reflection,
        "emit_json": boolean("SB09_PBRS_EMIT_JSON", reflection),
        "emit_text": boolean("SB09_PBRS_EMIT_TEXT", reflection),
        "shared_descriptor_set": boolean("SB09_PBRS_SHARED_DESCRIPTOR_SET", False),
        "runtime_profile": runtime,
        "stubs": stubs,
    }
    ordinary = {
        "emit_reflection": True, "emit_json": True, "emit_text": True,
        "shared_descriptor_set": False, "runtime_profile": "default", "stubs": ordinary_stubs,
    }
    return {
        "raw": raw, "resolved": resolved,
        "kind": "ordinary-default" if resolved == ordinary else "nondefault-diagnostic",
    }


def pbrs_helper_provenance(snapshot: dict, profile: dict, target_count: int) -> dict:
    """Reject missing/stale helper output rather than silently mislabel a profile."""
    name = "__pbrs_shared_descriptors.rs"
    resolved = profile["resolved"]
    expected = (
        resolved["shared_descriptor_set"] and resolved["emit_reflection"] and target_count > 1
    )
    if (name in snapshot) != expected:
        raise BenchmarkError(f"shared descriptor helper presence does not match requested profile: {name}")
    if not expected:
        return {"active": False, "path": None, "sha256": None, "bytes": 0}
    _, digest, size = snapshot[name]
    return {"active": True, "path": name, "sha256": digest, "bytes": size}


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def render_consumer_prost(messages: int, marker: int) -> str:
    lines = [
        f'#[path = "../generated/{PROST_PACKAGE_FILE}"] mod bench_cg19;',
        "",
        "fn roundtrip<M: prost::Message + Default>(msg: M) -> usize {",
        "    let wire = msg.encode_to_vec();",
        '    let parsed = M::decode(std::hint::black_box(&wire[..])).expect("parse generated message");',
        '    std::hint::black_box(parsed.encode_to_vec()).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip(bench_cg19::Message{number:04d}::default());"
        for number in range(messages)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def peer_manifest(package: str, generator: str, case: str | None = None) -> str:
    if generator == "v4":
        return manifest(package, reference=True)
    if generator in STUB_GENERATORS:
        return stub_manifest(package, generator)
    if generator == "prost":
        dependencies = [f'prost = "={PROST_VERSION}"']
        if case in REALISTIC_CORPORA and REALISTIC_CORPORA[case]["wkt"]:
            # prost extern-maps WKT to ::prost_types instead of emitting them.
            dependencies.append(f'prost-types = "={PROST_TYPES_VERSION}"')
    elif generator == "buffa":
        dependencies = [f'buffa = "={BUFFA_VERSION}"']
    else:
        raise BenchmarkError(f"unknown peer generator: {generator}")
    return (
        f'[package]\nname = "{package}"\nversion = "0.0.0"\nedition = "2024"\n'
        'publish = false\n\n[workspace]\n\n[dependencies]\n'
        + "\n".join(dependencies) + "\n\n"
        '[profile.release]\nopt-level = 3\nlto = "thin"\ncodegen-units = 1\n'
    )


def stub_manifest(package: str, generator: str) -> str:
    tonic013 = (
        f'prost = "={PROST013_VERSION}"\n'
        f'tonic = {{ version = "={TONIC013_VERSION}", default-features = false,'
        ' features = ["transport", "codegen", "prost"] }'
    )
    tonic014 = (
        f'tonic = {{ version = "={TONIC014_VERSION}", default-features = false,'
        ' features = ["transport", "codegen"] }'
    )
    runtime = (
        f'tokio = {{ version = "={TOKIO_VERSION}", features = ["sync", "rt"] }}\n'
        f'tokio-stream = "={TOKIO_STREAM_VERSION}"'
    )
    if generator == "pbrs-native":
        dependencies = [
            f'pbrs = {{ path = "{ROOT}" }}',
            f'pbrs-grpc = {{ path = "{ROOT / "pbrs-grpc"}" }}',
        ]
    elif generator == "pbrs-tonic":
        dependencies = [
            f'pbrs = {{ path = "{ROOT}" }}',
            f'protobuf-tonic = {{ path = "{ROOT / "protobuf-tonic"}" }}',
            tonic014,
            runtime,
            # pbrs-tonic stubs name http:: directly (paths, responses, headers).
            f'http = "={HTTP_VERSION}"',
        ]
    elif generator == "tonic-build":
        dependencies = [tonic013, runtime]
    else:
        raise BenchmarkError(f"unknown stub generator: {generator}")
    return (
        f'[package]\nname = "{package}"\nversion = "0.0.0"\nedition = "2024"\n'
        'publish = false\n\n[workspace]\n\n[dependencies]\n'
        + "\n".join(dependencies) + "\n\n"
        '[profile.release]\nopt-level = 3\nlto = "thin"\ncodegen-units = 1\n'
    )


def peer_driver_manifest(generator: str) -> str:
    if generator == "prost":
        dependency = f'prost-build = "={PROST_VERSION}"'
    elif generator == "buffa":
        dependency = f'buffa-build = "={BUFFA_VERSION}"'
    elif generator == "tonic-build":
        dependency = f'tonic-build = "={TONIC_BUILD_VERSION}"'
    else:
        raise BenchmarkError(f"unknown peer generator: {generator}")
    return (
        f'[package]\nname = "sb09-{generator}-driver"\nversion = "0.0.0"\nedition = "2021"\n'
        'publish = false\n\n[workspace]\n\n[dependencies]\n'
        f"{dependency}\n"
    )


def vendor_dir() -> Path:
    return ROOT / "target" / "codegen-bench" / "vendor"


def ensure_vendor(names: list[str]) -> Path:
    """Fetch-once, hash-verified vendor cache for realistic corpora.
    Cached files are re-verified on every call; any mismatch or fetch
    failure aborts the run instead of measuring unknown inputs."""
    base = vendor_dir()
    for name in names:
        repo, ref, repo_path, expected = VENDOR_FILES[name]
        dest = base / name
        if dest.is_file() and sha256(dest) == expected:
            continue
        url = f"{VENDOR_BASE}/{repo}/{ref}/{repo_path}"
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                data = response.read(8_000_001)
        except (OSError, ValueError) as exc:
            raise BenchmarkError(f"vendor fetch failed for {name} ({url}): {exc}") from exc
        if len(data) > 8_000_000:
            raise BenchmarkError(f"vendor file unexpectedly large: {name}")
        actual = hashlib.sha256(data).hexdigest()
        if actual != expected:
            raise BenchmarkError(
                f"vendor hash mismatch for {name}: got {actual}, want {expected}"
            )
        dest.parent.mkdir(parents=True, exist_ok=True)
        tmp = dest.with_suffix(".proto.download")
        tmp.write_bytes(data)
        tmp.replace(dest)
    return base


def prepare_realistic_corpus(
    case_dir: Path, case: str, generators: tuple[str, ...],
) -> tuple[list[str], dict]:
    spec = REALISTIC_CORPORA[case]
    vendor = ensure_vendor([*spec["inputs"], *spec["support"]])
    consumer = case_dir / "consumer"
    names = realistic_generation_names(case)
    wkt_names = {f"google/protobuf/{stem}.proto" for stem in spec["wkt"]}
    input_names = set(spec["inputs"])
    file_entries: dict[str, list] = {"inputs": [], "support": []}
    content: dict[str, dict[str, bool]] = {}
    digest = hashlib.sha256()
    for name in names:
        if name in wkt_names:
            src = ROOT / "third_party" / "protobuf" / "src" / name
            if not src.is_file():
                raise BenchmarkError(f"pinned WKT missing from checkout: {src}")
        else:
            src = vendor / name
        path = consumer / "proto" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        data = src.read_bytes()
        path.write_bytes(data)
        digest.update(f"{name}\n".encode("ascii"))
        digest.update(data)
        entry = {
            "path": relative(path, case_dir),
            "sha256": sha256(path),
            "bytes": len(data),
        }
        file_entries["inputs" if name in input_names else "support"].append(entry)
        text = data.decode("utf-8")
        package = re.search(r"^package ([\w.]+);", text, re.M)
        if package is None:
            raise BenchmarkError(f"{case}: {name} declares no package")
        if name in wkt_names:
            if package.group(1) != "google.protobuf":
                raise BenchmarkError(f"{case}: {name} is not in package google.protobuf")
        else:
            if package.group(1) != spec["packages"][name]:
                raise BenchmarkError(
                    f"{case}: {name} package {package.group(1)} != spec {spec['packages'][name]}"
                )
        if name in input_names:
            actual = re.findall(r"^message (\w+)", text, re.M)
            if actual != spec["messages"][name]:
                raise BenchmarkError(
                    f"{case}: {name} messages {actual} != spec {spec['messages'][name]}"
                )
        content[name] = {
            "messages": re.search(r"^message \w+", text, re.M) is not None,
            "enums": re.search(r"^enum \w+", text, re.M) is not None,
            "extends": re.search(r"^extend [\w.]+", text, re.M) is not None,
        }
    entries = realistic_entries(case)
    metadata: dict = {
        "messages": len(entries),
        "proto_file_count": len(spec["inputs"]),
        "support_file_count": len(spec["support"]) + len(spec["wkt"]),
        "sha256": digest.hexdigest(),
        "inputs": file_entries["inputs"],
        "support": file_entries["support"],
        "content": content,
        "packages": {name: spec["packages"][name] for name in spec["inputs"] + spec["support"]},
        "entries": [[package, message] for package, message in entries],
        "vendor": sorted(
            {f"{VENDOR_FILES[name][0]}@{VENDOR_FILES[name][1]}"
             for name in spec["inputs"] + spec["support"]}
        ),
    }
    if "pbrs" in generators:
        write_text(consumer / "Cargo.toml", manifest(f"cg19-consumer-{case}"))
        write_text(
            consumer / "src" / "main.rs", render_consumer_entries(case, 0, "pbrs"),
        )
    return names, metadata


def prepare_corpus(
    case_dir: Path, case: str, seed: int, generators: tuple[str, ...] = ("pbrs",),
) -> tuple[list[str], dict]:
    if case in REALISTIC_CORPORA:
        return prepare_realistic_corpus(case_dir, case, generators)
    if case in STUB_CORPORA:
        messages, files = STUB_CORPORA[case]
        render = render_proto_with_services
    else:
        messages, files = CORPORA[case]
        render = render_proto
    consumer = case_dir / "consumer"
    names = []
    inputs = []
    digest = hashlib.sha256()
    for index in range(files):
        name = f"part_{index:02d}.proto"
        path = consumer / "proto" / name
        write_text(path, render(seed, messages, files, index))
        digest.update(f"{name}\n".encode("ascii"))
        digest.update(path.read_bytes())
        names.append(name)
        inputs.append(
            {"path": relative(path, case_dir), "sha256": sha256(path), "bytes": path.stat().st_size}
        )
    if "pbrs" in generators:
        write_text(consumer / "Cargo.toml", manifest(f"cg19-consumer-{case}"))
        write_text(consumer / "src" / "main.rs", render_consumer(messages, 0))
    metadata: dict = {
        "messages": messages, "proto_file_count": files, "sha256": digest.hexdigest(), "inputs": inputs
    }
    if case in STUB_CORPORA:
        metadata["services"] = [name for name, _, _ in stub_services(case)]
    return names, metadata


def render_consumer_for(
    messages: int, marker: int, generator: str,
    services: list[tuple[str, int, int]] | None = None,
    case: str | None = None,
) -> str:
    if case in REALISTIC_CORPORA:
        assert case is not None
        return render_consumer_entries(case, marker, generator)
    if generator == "prost":
        return render_consumer_prost(messages, marker)
    if generator == "buffa":
        return render_consumer_buffa(messages, marker)
    if generator == "v4":
        return render_consumer(messages, marker, reference=True)
    if generator in STUB_GENERATORS:
        if not services:
            raise BenchmarkError(f"stub generator {generator} needs services")
        if generator == "pbrs-native":
            return render_consumer_pbrs_native(messages, marker, services)
        if generator == "pbrs-tonic":
            return render_consumer_pbrs_tonic(messages, marker, services)
        return render_consumer_tonic_build(messages, marker, services)
    raise BenchmarkError(f"no consumer renderer for generator: {generator}")


def render_consumer_pbrs_native(
    messages: int, marker: int, services: list[tuple[str, int, int]],
) -> str:
    lines = [
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));',
        "",
        "fn roundtrip<T: pbrs::Parse + pbrs::Serialize>(msg: T) -> usize {",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
    ]
    for name, _, _ in services:
        # Native traits carry default unimplemented bodies; an empty impl
        # still compiles the full server dispatch for the service.
        lines.append(f"struct {name}Svc;")
        lines.append(f"impl bench::cg19::{name} for {name}Svc {{}}")
        lines.append("")
    lines.extend(("fn main() {", "    let mut total = 0usize;"))
    lines.extend(
        f"    total += roundtrip(bench::cg19::Message{number:04d}::new());"
        for number in range(messages)
    )
    for index, (name, _, _) in enumerate(services):
        lines.append(f"    let server{index:02d} = bench::cg19::{name}Server::new({name}Svc);")
        lines.append(
            '    let channel = pbrs_grpc::Channel::connect_lazy("127.0.0.1:1")'
            '.expect("lazy channel");'
        )
        lines.append(f"    let client{index:02d} = bench::cg19::{name}Client::new(channel);")
        lines.append(f"    std::hint::black_box((&server{index:02d}, &client{index:02d}));")
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_pbrs_tonic(
    messages: int, marker: int, services: list[tuple[str, int, int]],
) -> str:
    lines = [
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));',
        "",
        "fn roundtrip<T: pbrs::Parse + pbrs::Serialize>(msg: T) -> usize {",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
    ]
    for name, req, resp in services:
        lines.append(f"struct {name}Svc;")
        lines.append(f"impl bench::cg19::{name} for {name}Svc {{")
        lines.append(
            f"    async fn get(&self, _request: tonic::Request<bench::cg19::Message{req:04d}>)"
        )
        lines.append(
            "        -> std::result::Result<"
            f"tonic::Response<bench::cg19::Message{resp:04d}>, tonic::Status>"
        )
        lines.append('        { unimplemented!("stub smoke") }')
        lines.append(
            "    type WatchStream = tokio_stream::wrappers::ReceiverStream<"
            f"std::result::Result<bench::cg19::Message{resp:04d}, tonic::Status>>;"
        )
        lines.append(
            f"    async fn watch(&self, _request: tonic::Request<bench::cg19::Message{req:04d}>)"
        )
        lines.append(
            "        -> std::result::Result<tonic::Response<Self::WatchStream>, tonic::Status>"
        )
        lines.append('        { unimplemented!("stub smoke") }')
        lines.append("}")
        lines.append("")
    lines.extend(('#[tokio::main(flavor = "current_thread")]',
                      "async fn main() {", "    let mut total = 0usize;"))
    lines.extend(
        f"    total += roundtrip(bench::cg19::Message{number:04d}::new());"
        for number in range(messages)
    )
    for index, (name, _, _) in enumerate(services):
        lines.append(f"    let server{index:02d} = bench::cg19::{name}Server::new({name}Svc);")
        lines.append(
            '    let channel = tonic::transport::Channel::from_static("http://127.0.0.1:1")'
            ".connect_lazy();"
        )
        lines.append(f"    let client{index:02d} = bench::cg19::{name}Client::new(channel);")
        lines.append(f"    std::hint::black_box((&server{index:02d}, &client{index:02d}));")
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def tonics_snake(name: str) -> str:
    # Mirrors tonic-build's naive CamelCase splitting for our ServiceNN names.
    out = []
    for position, char in enumerate(name):
        if char.isupper() and position:
            out.append("_")
        out.append(char.lower())
    return "".join(out)


def render_consumer_tonic_build(
    messages: int, marker: int, services: list[tuple[str, int, int]],
) -> str:
    lines = [
        f'#[path = "../generated/{PROST_PACKAGE_FILE}"] mod bench_cg19;',
        "",
        "fn roundtrip<M: prost::Message + Default>(msg: M) -> usize {",
        "    let wire = msg.encode_to_vec();",
        '    let parsed = M::decode(std::hint::black_box(&wire[..])).expect("parse generated message");',
        '    std::hint::black_box(parsed.encode_to_vec()).len()',
        "}",
        "",
    ]
    for name, req, resp in services:
        snake = tonics_snake(name)
        lines.append(f"struct {name}Svc;")
        lines.append("#[tonic::async_trait]")
        lines.append(f"impl bench_cg19::{snake}_server::{name} for {name}Svc {{")
        lines.append(
            f"    async fn get(&self, _request: tonic::Request<bench_cg19::Message{req:04d}>)"
        )
        lines.append(
            "        -> std::result::Result<"
            f"tonic::Response<bench_cg19::Message{resp:04d}>, tonic::Status>"
        )
        lines.append('        { unimplemented!("stub smoke") }')
        lines.append(
            "    type WatchStream = tokio_stream::wrappers::ReceiverStream<"
            f"std::result::Result<bench_cg19::Message{resp:04d}, tonic::Status>>;"
        )
        lines.append(
            f"    async fn watch(&self, _request: tonic::Request<bench_cg19::Message{req:04d}>)"
        )
        lines.append(
            "        -> std::result::Result<tonic::Response<Self::WatchStream>, tonic::Status>"
        )
        lines.append('        { unimplemented!("stub smoke") }')
        lines.append("}")
        lines.append("")
    lines.extend(('#[tokio::main(flavor = "current_thread")]',
                      "async fn main() {", "    let mut total = 0usize;"))
    lines.extend(
        f"    total += roundtrip(bench_cg19::Message{number:04d}::default());"
        for number in range(messages)
    )
    for index, (name, _, _) in enumerate(services):
        snake = tonics_snake(name)
        lines.append(
            f"    let server{index:02d} = bench_cg19::{snake}_server::{name}Server::new({name}Svc);"
        )
        lines.append(
            '    let channel = tonic::transport::Channel::from_static("http://127.0.0.1:1")'
            ".connect_lazy();"
        )
        lines.append(
            f"    let client{index:02d} = bench_cg19::{snake}_client::{name}Client::new(channel);"
        )
        lines.append(f"    std::hint::black_box((&server{index:02d}, &client{index:02d}));")
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_buffa(messages: int, marker: int) -> str:
    lines = [
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));',
        "",
        "fn roundtrip<M: buffa::Message + Default>(msg: M) -> usize {",
        "    let mut wire = Vec::new();",
        "    msg.encode(&mut wire);",
        '    let parsed = M::decode(&mut &wire[..]).expect("parse generated message");',
        "    let mut wire2 = Vec::new();",
        "    std::hint::black_box(parsed).encode(&mut wire2);",
        "    wire2.len()",
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip(bench::cg19::Message{number:04d}::default());"
        for number in range(messages)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def prepare_peer_consumer(
    case_dir: Path, case: str, messages: int, generator: str,
    services: list[tuple[str, int, int]] | None = None,
) -> tuple[Path, str]:
    consumer = case_dir / "gen" / generator / "consumer"
    package = f"sb09-{generator}-consumer-{case}"
    write_text(consumer / "Cargo.toml", peer_manifest(package, generator, case))
    write_text(
        consumer / "src" / "main.rs",
        render_consumer_for(messages, 0, generator, services, case),
    )
    return consumer, package


# Rust 2021 strict keywords plus the 2018 additions; mirrors the escaping
# pbrs (mod_ident), prost, and buffa all apply to package segments.
RUST_KEYWORDS = frozenset({
    "as", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "typeof", "unsized", "virtual",
    "yield", "try", "union", "gen",
})
RUST_UNESCAPABLE = frozenset({"crate", "self", "Self", "super", "true", "false", "_"})


def rust_mod_ident(segment: str) -> str:
    if segment in RUST_UNESCAPABLE:
        return f"{segment}_"
    if segment in RUST_KEYWORDS:
        return f"r#{segment}"
    return segment


def realistic_entries(case: str) -> list[tuple[str, str]]:
    """(package, message) consumer entries in spec order for a realistic case."""
    spec = REALISTIC_CORPORA[case]
    return [
        (spec["packages"][name], message)
        for name in spec["inputs"]
        for message in spec["messages"][name]
    ]


def realistic_entry_paths(case: str) -> list[str]:
    return [
        "::".join([*(rust_mod_ident(part) for part in package.split(".")), message])
        for package, message in realistic_entries(case)
    ]


def realistic_generation_names(case: str) -> list[str]:
    spec = REALISTIC_CORPORA[case]
    return [
        *spec["inputs"], *spec["support"],
        *(f"google/protobuf/{stem}.proto" for stem in spec["wkt"]),
    ]


def realistic_closure_packages(case: str) -> set[str]:
    spec = REALISTIC_CORPORA[case]
    packages = {spec["packages"][name] for name in spec["inputs"] + spec["support"]}
    if spec["wkt"]:
        packages.add("google.protobuf")
    return packages


def prost_package_file(package: str) -> str:
    return ".".join(rust_mod_ident(part) for part in package.split(".")) + ".rs"


def prost_package_files(case: str) -> list[str]:
    """Exact prost outputs: one file per closure package except WKT
    (extern-mapped to ::prost_types) and spec-declared omitted packages."""
    spec = REALISTIC_CORPORA[case]
    packages = realistic_closure_packages(case) - {"google.protobuf"}
    packages -= set(spec["prost_omitted_packages"])
    return sorted(prost_package_file(package) for package in packages)


def prost_package_tree(packages: list[str]) -> str:
    """Crate-root module tree including one prost package file per leaf.
    prost cross-references packages through super:: chains, so every
    package segment must be exactly one module level."""
    root: dict = {}
    for package in packages:
        node = root
        for part in package.split("."):
            node = node.setdefault(part, {})
        node[None] = prost_package_file(package)
    lines: list[str] = []

    def emit(node: dict, indent: int) -> None:
        pad = "    " * indent
        if None in node:
            lines.append(
                f'{pad}include!(concat!(env!("CARGO_MANIFEST_DIR"), '
                f'"/generated/{node[None]}"));'
            )
        for part in sorted(key for key in node if key is not None):
            lines.append(f"{pad}pub mod {rust_mod_ident(part)} {{")
            emit(node[part], indent + 1)
            lines.append(f"{pad}}}")

    for part in sorted(root):
        lines.append(f"pub mod {rust_mod_ident(part)} {{")
        emit(root[part], 1)
        lines.append("}")
    return "\n".join(lines)


def v4_entrypoint_rel(case: str) -> str:
    """v4 --rust_out writes its single generated.rs into the FIRST input's
    directory; the harness verifies the prediction against a post-generation
    search and fails closed on any mismatch."""
    first = REALISTIC_CORPORA[case]["inputs"][0]
    parent = str(Path(first).parent)
    return f"{parent}/generated.rs" if parent != "." else "generated.rs"


def render_consumer_entries_pbrs(case: str, marker: int) -> str:
    lines = [
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));',
        "",
        "fn roundtrip<T: pbrs::Parse + pbrs::Serialize>(msg: T) -> usize {",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip({path}::new());" for path in realistic_entry_paths(case)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_entries_buffa(case: str, marker: int) -> str:
    lines = [
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/mod.rs"));',
        "",
        "fn roundtrip<M: buffa::Message + Default>(msg: M) -> usize {",
        "    let mut wire = Vec::new();",
        "    msg.encode(&mut wire);",
        '    let parsed = M::decode(&mut &wire[..]).expect("parse generated message");',
        "    let mut wire2 = Vec::new();",
        "    std::hint::black_box(parsed).encode(&mut wire2);",
        "    wire2.len()",
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip({path}::default());"
        for path in realistic_entry_paths(case)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_entries_prost(case: str, marker: int) -> str:
    spec = REALISTIC_CORPORA[case]
    packages = sorted(
        realistic_closure_packages(case)
        - {"google.protobuf"}
        - set(spec["prost_omitted_packages"])
    )
    lines = [
        prost_package_tree(packages),
        "",
        "fn roundtrip<M: prost::Message + Default>(msg: M) -> usize {",
        "    let wire = msg.encode_to_vec();",
        '    let parsed = M::decode(std::hint::black_box(&wire[..])).expect("parse generated message");',
        '    std::hint::black_box(parsed.encode_to_vec()).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip({path}::default());"
        for path in realistic_entry_paths(case)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_entries_v4(case: str, marker: int) -> str:
    lines = [
        f'#[path = "../generated/{v4_entrypoint_rel(case)}"] mod generated;',
        "",
        "fn roundtrip<T: protobuf::Parse + protobuf::Serialize>(msg: T) -> usize {",
        '    let wire = msg.serialize().expect("serialize generated message");',
        '    let parsed = T::parse(std::hint::black_box(&wire)).expect("parse generated message");',
        '    std::hint::black_box(parsed.serialize().expect("serialize parsed message")).len()',
        "}",
        "",
        "fn main() {",
        "    let mut total = 0usize;",
    ]
    lines.extend(
        f"    total += roundtrip(generated::{message}::new());"
        for _, message in realistic_entries(case)
    )
    lines.extend(
        (
            f"    println!(\"{{}}\", std::hint::black_box(total + {marker}));",
            "}",
            "",
        )
    )
    return "\n".join(lines)


def render_consumer_entries(case: str, marker: int, generator: str) -> str:
    if generator == "pbrs":
        return render_consumer_entries_pbrs(case, marker)
    if generator == "prost":
        return render_consumer_entries_prost(case, marker)
    if generator == "buffa":
        return render_consumer_entries_buffa(case, marker)
    if generator == "v4":
        return render_consumer_entries_v4(case, marker)
    raise BenchmarkError(f"no realistic consumer renderer for generator: {generator}")


def snapshot_generated(
    out: Path, entrypoint: str = "mod.rs", min_files: int = 2,
) -> dict[str, tuple[int, str, int]]:
    if not (out / entrypoint).is_file():
        raise BenchmarkError(f"generation omitted {out / entrypoint}")
    files = sorted(out.rglob("*.rs"))
    if len(files) < min_files:
        raise BenchmarkError(f"generation produced fewer than {min_files} Rust files in {out}")
    return {
        path.relative_to(out).as_posix(): (path.stat().st_mtime_ns, sha256(path), path.stat().st_size)
        for path in files
    }


def assert_unchanged(before: dict, after: dict) -> None:
    if before.keys() != after.keys():
        raise BenchmarkError(
            f"unchanged generation changed the output file set: "
            f"removed={sorted(before.keys() - after.keys())}, "
            f"added={sorted(after.keys() - before.keys())}"
        )
    for path in sorted(before):
        if before[path] != after[path]:
            raise BenchmarkError(
                f"unchanged generation altered {path}: "
                f"(mtime_ns, sha256, bytes) {before[path]} -> {after[path]}"
            )


def assert_same_bytes(before: dict, after: dict) -> int:
    if before.keys() != after.keys():
        raise BenchmarkError("reference unchanged generation changed the output file set")
    changed = 0
    for path in sorted(before):
        if before[path][1:] != after[path][1:]:
            raise BenchmarkError(f"reference unchanged generation changed bytes in {path}")
        changed += before[path][0] != after[path][0]
    return changed


def tree_digest(snapshot: dict[str, tuple[int, str, int]]) -> str:
    entries = [(path, details[1]) for path, details in sorted(snapshot.items())]
    return hashlib.sha256(json.dumps(entries, separators=(",", ":")).encode("utf-8")).hexdigest()


def parse_time_rss(stderr: str, system: str) -> int:
    if system == "Darwin":
        match = re.search(r"(?m)^\s*(\d+)\s+maximum resident set size\b", stderr)
        scale = 1
    elif system == "Linux":
        match = re.search(r"(?m)^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$", stderr)
        scale = 1024
    else:
        raise BenchmarkError(f"RSS measurement unsupported on {system}")
    if match is None:
        raise BenchmarkError(f"missing {system} maximum resident set size in time log")
    return int(match.group(1)) * scale


def tree_rss(ps_output: str, root_pid: int) -> int:
    children: dict[int, list[int]] = {}
    sizes: dict[int, int] = {}
    for row in ps_output.splitlines():
        columns = row.split()
        if len(columns) != 3:
            raise BenchmarkError(f"invalid ps RSS row: {row!r}")
        pid, parent, rss_kib = map(int, columns)
        children.setdefault(parent, []).append(pid)
        sizes[pid] = rss_kib * 1024
    if root_pid not in sizes:
        return 0  # The timed process finished between sampling and ps.
    pending = [root_pid]
    seen = set()
    total = 0
    while pending:
        pid = pending.pop()
        if pid in seen:
            continue
        seen.add(pid)
        total += sizes[pid]
        pending.extend(children.get(pid, ()))
    return total


def _terminate_own_group(proc: subprocess.Popen) -> None:
    # Every measured command starts in a new session; this group belongs only to it.
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()


def log_paths(stem: Path, run_dir: Path) -> tuple[Path, Path, dict]:
    stdout = stem.with_name(stem.name + ".stdout.log")
    stderr = stem.with_name(stem.name + ".stderr.log")
    return stdout, stderr, {
        "stdout_log": relative(stdout, run_dir),
        "stderr_log": relative(stderr, run_dir),
    }


def timed_command(
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    stem: Path,
    run_dir: Path,
    timeout: int,
    sample_ms: int,
) -> dict:
    system = platform.system()
    time_flag = {"Darwin": "-l", "Linux": "-v"}.get(system)
    if time_flag is None or not Path("/usr/bin/time").is_file():
        raise BenchmarkError(f"requires /usr/bin/time on Linux or macOS (found {system})")
    ps = shutil.which("ps")
    if ps is None:
        raise BenchmarkError("requires ps to sample cargo/rustc child RSS")
    stdout, stderr, paths = log_paths(stem, run_dir)
    stem.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        started = time.perf_counter_ns()
        try:
            proc = subprocess.Popen(
                ["/usr/bin/time", time_flag, *command],
                cwd=cwd,
                env=env,
                stdout=out,
                stderr=err,
                start_new_session=True,
            )
        except OSError as exc:
            raise BenchmarkError(f"cannot start {command[0]}: {exc}; logs: {paths}") from exc
        stop = threading.Event()
        samples: list[int] = []
        sampling_errors: list[str] = []

        def sample() -> None:
            while not stop.is_set():
                try:
                    result = subprocess.run(
                        [ps, "-A", "-o", "pid=,ppid=,rss="],
                        capture_output=True,
                        text=True,
                        check=True,
                        timeout=10,
                        env={**os.environ, "LC_ALL": "C"},
                    )
                    rss = tree_rss(result.stdout, proc.pid)
                    if rss:
                        samples.append(rss)
                except (
                    OSError,
                    ValueError,
                    BenchmarkError,
                    subprocess.CalledProcessError,
                    subprocess.TimeoutExpired,
                ) as exc:
                    sampling_errors.append(f"process-tree RSS sampling failed: {exc}")
                    return
                stop.wait(sample_ms / 1000)

        sampler = threading.Thread(target=sample, daemon=True)
        sampler.start()
        try:
            exit_code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            _terminate_own_group(proc)
            raise BenchmarkError(f"{command[0]} exceeded {timeout}s; logs: {paths}") from exc
        finally:
            stop.set()
            sampler.join()
        elapsed_ns = time.perf_counter_ns() - started
    direct_peak = parse_time_rss(stderr.read_text(encoding="utf-8", errors="replace"), system)
    sampled_peak = max(samples) if samples else None
    return {
        "command": command,
        "exit_code": exit_code,
        "elapsed_ns": elapsed_ns,
        "peak_rss_bytes": max(direct_peak, sampled_peak or 0),
        "direct_command_peak_rss_bytes": direct_peak,
        "process_tree_sample_peak_rss_bytes": sampled_peak,
        "process_tree_samples": len(samples),
        "process_tree_sampling_error": sampling_errors[0] if sampling_errors else None,
        "rss_peak_is_lower_bound": True,
        **paths,
    }


def plain_command(
    command: list[str], cwd: Path, env: dict[str, str], stem: Path,
    run_dir: Path, timeout: int,
) -> dict:
    stdout, stderr, paths = log_paths(stem, run_dir)
    stem.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        try:
            proc = subprocess.Popen(
                command, cwd=cwd, env=env, stdout=out, stderr=err, start_new_session=True,
            )
        except OSError as exc:
            raise BenchmarkError(f"cannot start {command[0]}: {exc}; logs: {paths}") from exc
        try:
            exit_code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            _terminate_own_group(proc)
            raise BenchmarkError(f"{command[0]} exceeded {timeout}s; logs: {paths}") from exc
    return {
        "command": command, "cwd": str(cwd), "exit_code": exit_code,
        "timeout_seconds": timeout, **paths,
    }


def write_report(report: dict, run_dir: Path) -> None:
    path = run_dir / "summary.json"
    temporary = run_dir / ".summary.json.tmp"
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def phase(
    report: dict,
    target: dict,
    name: str,
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    run_dir: Path,
    timeout: int,
    sample_ms: int,
    stem: Path,
    *,
    raw_output: bool = False,
) -> dict:
    _out, _err, paths = log_paths(stem, run_dir)
    target[name] = {"status": "running", "command": command, **paths}
    write_report(report, run_dir)
    try:
        measurement = (
            plain_command(command, cwd, env, stem, run_dir, timeout)
            if raw_output else
            timed_command(command, cwd, env, stem, run_dir, timeout, sample_ms)
        )
    except BenchmarkError as exc:
        target[name].update({"status": "error", "error": str(exc)})
        write_report(report, run_dir)
        raise
    target[name] = {**measurement, "status": "passed" if measurement["exit_code"] == 0 else "failed"}
    write_report(report, run_dir)
    if measurement["exit_code"] != 0:
        raise BenchmarkError(f"{name} exited {measurement['exit_code']}; see {paths['stderr_log']}")
    return measurement


def tool_version(executable: str, flags: list[str], name: str, run_dir: Path) -> dict:
    resolved = shutil.which(executable)
    if resolved is None:
        raise BenchmarkError(f"required tool not found: {executable}")
    path = Path(os.path.abspath(resolved))
    stdout, stderr, logs = log_paths(run_dir / "logs" / "environment" / name, run_dir)
    stdout.parent.mkdir(parents=True, exist_ok=True)
    with stdout.open("wb") as out, stderr.open("wb") as err:
        try:
            result = subprocess.run([str(path), *flags], stdout=out, stderr=err, timeout=30)
        except subprocess.TimeoutExpired as exc:
            raise BenchmarkError(f"{name} --version timed out; logs: {logs}") from exc
    if result.returncode:
        raise BenchmarkError(f"{name} --version exited {result.returncode}; logs: {logs}")
    return {
        "executable": str(path),
        "resolved_target": str(path.resolve()) if path.is_symlink() else None,
        "executable_sha256": sha256(path),
        "version": stdout.read_text(encoding="utf-8").strip(),
        **logs,
    }


def assert_consumer_built(measurement: dict, package: str, verb: str, run_dir: Path) -> None:
    log = (run_dir / measurement["stderr_log"]).read_text(encoding="utf-8", errors="replace")
    if not re.search(rf"(?m)^\s*{verb} {re.escape(package)} v0\.0\.0\b", log):
        raise BenchmarkError(
            f"{verb.lower()} was a no-op or built the wrong consumer; see "
            f"{measurement['stderr_log']}"
        )


def reference_pin(protoc: dict) -> dict:
    if protoc["version"] != "libprotoc 35.1" or protoc["executable_sha256"] != REFERENCE_PROTOC_SHA256:
        raise BenchmarkError(
            "reference protoc must be the pinned libprotoc 35.1 binary "
            f"({REFERENCE_PROTOC_SHA256}); got {protoc['version']!r}, "
            f"{protoc['executable_sha256']}"
        )
    checked = ROOT / "tonic-bench" / "checked_v4" / "manifest.json"
    pins = json.loads(checked.read_text(encoding="utf-8"))
    expected = {
        "protobuf_source_revision": REFERENCE_REVISION,
        "protoc_version": "libprotoc 35.1",
        "protoc_sha256": REFERENCE_PROTOC_SHA256,
        "runtime_version": REFERENCE_RUNTIME_VERSION,
    }
    if any(pins.get(key) != value for key, value in expected.items()):
        raise BenchmarkError(f"reference pins disagree with {checked}")
    if (ROOT / "vendor" / "google" / "PIN").read_text().strip() != "v35.1":
        raise BenchmarkError("vendor/google/PIN is not v35.1")
    if (ROOT / "vendor" / "google" / "SHA").read_text().strip() != REFERENCE_REVISION:
        raise BenchmarkError("vendor/google/SHA disagrees with pinned reference revision")
    checkout = ROOT / "third_party" / "protobuf"
    head = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True, timeout=30
    ).strip()
    if head != REFERENCE_REVISION:
        raise BenchmarkError(f"reference source checkout at {head}, expected {REFERENCE_REVISION}")
    dirty = subprocess.check_output(
        ["git", "-C", str(checkout), "status", "--porcelain", "--untracked-files=no"],
        text=True, timeout=30,
    )
    if dirty:
        raise BenchmarkError(f"reference source checkout has tracked changes: {checkout}")
    return {
        "source_revision": head,
        "source_checkout": str(checkout),
        "checked_pin_manifest_sha256": sha256(checked),
        "generator_options": REFERENCE_RUST_OPT,
    }


def reference_lock(path: Path) -> dict:
    try:
        import tomllib
    except ModuleNotFoundError as exc:
        raise BenchmarkError("reference lock verification requires Python 3.11+") from exc
    packages = tomllib.loads(path.read_text(encoding="utf-8")).get("package")
    if not isinstance(packages, list):
        raise BenchmarkError(f"reference lockfile lacks packages: {path}")
    if any(package.get("name") == "pbrs" for package in packages):
        raise BenchmarkError(f"reference consumer unexpectedly depends on pbrs: {path}")
    result = {"lock_sha256": sha256(path), "packages": {}}
    for name, checksum in (
        ("protobuf", REFERENCE_RUNTIME_CHECKSUM),
        ("protobuf-macros", REFERENCE_MACROS_CHECKSUM),
    ):
        matches = [package for package in packages if package.get("name") == name]
        if len(matches) != 1 or any(
            matches[0].get(key) != value for key, value in (
                ("version", REFERENCE_RUNTIME_VERSION),
                ("source", "registry+https://github.com/rust-lang/crates.io-index"),
                ("checksum", checksum),
            )
        ):
            raise BenchmarkError(f"reference {name} version/source/checksum is not pinned in {path}")
        result["packages"][name] = {
            "version": REFERENCE_RUNTIME_VERSION, "source": matches[0]["source"], "checksum": checksum,
        }
    return result


def source_hashes(reference: bool = False) -> dict[str, str]:
    paths = [
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "build.rs",
        ROOT / "proto" / "person.proto",
        ROOT / "vendor" / "google" / "conformance_fds.bin",
        ROOT / "scripts" / "codegen-bench.sh",
        Path(__file__),
        Path(__file__).with_name("generator.rs"),
        *(ROOT / "src").rglob("*.rs"),
    ]
    if reference:
        paths.extend([
            ROOT / "tonic-bench" / "Cargo.lock",
            ROOT / "tonic-bench" / "checked_v4" / "manifest.json",
            ROOT / "vendor" / "google" / "PIN",
            ROOT / "vendor" / "google" / "SHA",
        ])
    return {
        path.relative_to(ROOT).as_posix(): sha256(path)
        for path in sorted(paths)
    }


def provenance(run_dir: Path, jobs: int, sample_ms: int, reference_protoc: Path | None = None) -> dict:
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True
    ).splitlines()
    cargo = tool_version(os.environ.get("CARGO", "cargo"), ["--version", "--verbose"], "cargo", run_dir)
    rustc = tool_version(os.environ.get("RUSTC", "rustc"), ["--version", "--verbose"], "rustc", run_dir)
    protoc = tool_version(
        str(reference_protoc) if reference_protoc else os.environ.get("PROTOC", "protoc"),
        ["--version"], "protoc", run_dir,
    )
    reference = None
    tools = {"cargo": cargo, "rustc": rustc, "protoc": protoc}
    if reference_protoc is not None:
        overrides = sorted(
            key for key in os.environ if key.startswith("CC_") or key in ("TARGET_CC", "HOST_CC")
        )
        if overrides:
            raise BenchmarkError(f"reference C compiler overrides are unsupported: {overrides}")
        reference = reference_pin(protoc)
        tools["cc"] = tool_version(os.environ.get("CC", "cc"), ["--version"], "cc", run_dir)
    return {
        "repository": {
            "head": head,
            "tracked_changes": dirty,
            "source_sha256": source_hashes(reference_protoc is not None),
            "source_scope": (
                "src/**/*.rs, build.rs, proto/person.proto, vendored conformance FDS, "
                "Cargo.toml/Cargo.lock, harness, generator and wrapper"
                + (", vendored Google pin, checked generator pin and tonic-bench/Cargo.lock"
                   if reference_protoc is not None else "")
            ),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "cpu_count": os.cpu_count(),
            "python": sys.version.split()[0],
        },
        "tools": tools,
        **({"reference_source": reference} if reference is not None else {}),
        "cache": {
            "cargo_home": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
            "inherited_target_dir": os.environ.get("CARGO_TARGET_DIR"),
            "inherited_cargo_incremental": os.environ.get("CARGO_INCREMENTAL"),
            "targets": "shared integration-consumers bootstrap; fresh isolated per-corpus targets",
            "registry": "existing local Cargo registry; warmth not controlled",
            "cargo_offline": True,
            "cargo_locked_after_offline_lockfile": True,
            "build_jobs": jobs,
            "check_incremental": True,
            "release_incremental": False,
            "rustflags": os.environ.get("RUSTFLAGS"),
            "rustc_wrapper": os.environ.get("RUSTC_WRAPPER"),
            "rustc_workspace_wrapper": os.environ.get("RUSTC_WORKSPACE_WRAPPER"),
            "sccache_dir": os.environ.get("SCCACHE_DIR"),
            "rustup_toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
            **({"c_compiler_env": {
                key: os.environ.get(key) for key in (
                    "CC", "CFLAGS", "CPPFLAGS", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET",
                    "CRATE_CC_NO_DEFAULTS", "CARGO_ENCODED_RUSTFLAGS",
                )
            }, "cargo_profile_overrides": {
                key: value for key, value in os.environ.items() if key.startswith("CARGO_PROFILE_")
            }} if reference_protoc is not None else {}),
        },
        "measurement": {
            "rss": "max of OS time direct-command peak and sampled process-tree RSS sum",
            "rss_sample_ms": sample_ms,
            "rss_peaks_are_lower_bounds": True,
            "generation_includes": "protoc descriptor compilation + Config::compile_protos Rust emission",
            "check_clean": "fresh per-corpus target; local registry/compiler wrapper may be warm",
            "check_incremental": "same target after source marker changes; consumer rechecked",
            "release": "same target, distinct release profile: opt-level=3, thin LTO, codegen-units=1",
        },
    }


def copy_generator(shared: Path, run_dir: Path) -> tuple[Path, str]:
    if not shared.is_file():
        raise BenchmarkError(f"bootstrap did not produce {shared}")
    before = sha256(shared)
    generator = run_dir / "bin" / "cg19-generator"
    generator.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(shared, generator)
    copied = sha256(generator)
    if copied != before or sha256(shared) != before:
        raise BenchmarkError(f"shared generator changed during copy to {generator}")
    if not os.access(generator, os.X_OK):
        raise BenchmarkError(f"copied generator is not executable: {generator}")
    return generator, copied


def copy_peer_generator(shared: Path, run_dir: Path, generator: str) -> tuple[Path, str]:
    if not shared.is_file():
        raise BenchmarkError(f"bootstrap did not produce {shared}")
    before = sha256(shared)
    binary = run_dir / "bin" / f"sb09-{generator}-driver"
    binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(shared, binary)
    copied = sha256(binary)
    if copied != before or sha256(shared) != before:
        raise BenchmarkError(f"shared {generator} driver changed during copy to {binary}")
    if not os.access(binary, os.X_OK):
        raise BenchmarkError(f"copied {generator} driver is not executable: {binary}")
    return binary, copied


def build_peer_driver(
    report: dict, run_dir: Path, generator: str, cargo: str, boot_env: dict[str, str],
    boot_target: Path, timeout: int, sample_ms: int,
) -> Path:
    driver = run_dir / f"driver-{generator}"
    (driver / "src").mkdir(parents=True)
    source = Path(__file__).with_name(f"{generator}_driver.rs")
    if not source.is_file():
        raise BenchmarkError(f"missing peer driver source: {source}")
    shutil.copyfile(source, driver / "src" / "main.rs")
    write_text(driver / "Cargo.toml", peer_driver_manifest(generator))
    boot_log = run_dir / "logs" / "setup"
    phase(
        report, report["setup"], f"driver_lock_{generator}",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(driver / "Cargo.toml")],
        ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / f"driver-lock-{generator}",
    )
    report["setup"][f"driver_lock_{generator}_sha256"] = sha256(driver / "Cargo.lock")
    write_report(report, run_dir)
    binary_name = f"sb09-{generator}-driver"
    phase(
        report, report["setup"], f"driver_build_{generator}",
        [
            cargo, "build", "--offline", "--locked", "--manifest-path", str(driver / "Cargo.toml"),
            "--target-dir", str(boot_target), "--bin", binary_name,
        ],
        ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / f"driver-build-{generator}",
    )
    shared = boot_target / "debug" / binary_name
    binary, digest = copy_peer_generator(shared, run_dir, generator)
    report["setup"][f"{generator}_driver_binary_path"] = relative(binary, run_dir)
    report["setup"][f"{generator}_driver_binary_sha256"] = digest
    report["setup"][f"{generator}_driver_source_sha256"] = sha256(source)
    write_report(report, run_dir)
    return binary


def release_smoke(
    report: dict, phases: dict, binary: Path, consumer: Path, env: dict[str, str],
    run_dir: Path, timeout: int, sample_ms: int, logs: Path,
) -> None:
    phase(
        report, phases, "release_smoke", [str(binary)], consumer, env, run_dir,
        min(timeout, 15), sample_ms, logs / "release-smoke", raw_output=True,
    )
    smoke = phases["release_smoke"]
    stdout_path = run_dir / smoke["stdout_log"]
    stderr_path = run_dir / smoke["stderr_log"]
    stdout = stdout_path.read_bytes()
    stderr = stderr_path.read_bytes()
    smoke.update({
        "expected_stdout": "1\n",
        "expected_stderr": "",
        "stdout_sha256": sha256(stdout_path),
        "stderr_sha256": sha256(stderr_path),
        "output_verified": stdout == b"1\n" and stderr == b"",
    })
    if not smoke["output_verified"]:
        smoke["status"] = "failed"
        smoke["error"] = (
            f"release smoke expected stdout b'1\\n' and empty stderr, "
            f"got {len(stdout)} stdout bytes and {len(stderr)} stderr bytes; "
            f"see {smoke['stdout_log']} and {smoke['stderr_log']}"
        )
        write_report(report, run_dir)
        raise BenchmarkError(smoke["error"])
    write_report(report, run_dir)


def measure_consumer_build(
    report: dict, cell: dict, case: str, consumer: Path, target_dir: Path, package: str,
    reference: bool, cargo: str, check_env: dict[str, str], run_dir: Path,
    timeout: int, sample_ms: int, logs: Path, generator: str = "pbrs",
    messages: int | None = None, services: list[tuple[str, int, int]] | None = None,
) -> None:
    if target_dir.exists():
        raise BenchmarkError(f"clean check target already exists: {target_dir}")
    cargo_args = [
        "--offline", "--locked", "--manifest-path", str(consumer / "Cargo.toml"),
        "--target-dir", str(target_dir), "--bin", package,
    ]
    checked = phase(
        report, cell["phases"], "check_clean", [cargo, "check", *cargo_args],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "check-clean",
    )
    assert_consumer_built(checked, package, "Checking", run_dir)
    main_rs = consumer / "src" / "main.rs"
    old_mtime = main_rs.stat().st_mtime_ns
    time.sleep(max(0, (old_mtime + 1_100_000_000 - time.time_ns()) / 1_000_000_000))
    if case in REALISTIC_CORPORA:
        rewrite = render_consumer_entries(case, 1, generator)
    else:
        rewrite_messages = CORPORA[case][0] if messages is None else messages
        if generator == "pbrs":
            rewrite = render_consumer(rewrite_messages, 1, reference)
        else:
            rewrite = render_consumer_for(rewrite_messages, 1, generator, services)
    write_text(main_rs, rewrite)
    if main_rs.stat().st_mtime_ns <= old_mtime:
        raise BenchmarkError(f"incremental source mtime did not advance: {main_rs}")
    incremental = phase(
        report, cell["phases"], "check_incremental", [cargo, "check", *cargo_args],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "check-incremental",
    )
    assert_consumer_built(incremental, package, "Checking", run_dir)
    release = phase(
        report, cell["phases"], "build_release",
        [cargo, "build", "--release", *cargo_args],
        ROOT, {**check_env, "CARGO_INCREMENTAL": "0"},
        run_dir, timeout, sample_ms, logs / "build-release",
    )
    assert_consumer_built(release, package, "Compiling", run_dir)
    binary = target_dir / "release" / package
    if not binary.is_file() or not binary.stat().st_size:
        raise BenchmarkError(f"release build omitted a nonempty binary at {binary}")
    cell["release_binary"] = {
        "path": relative(binary, run_dir),
        "size_bytes": binary.stat().st_size,
        "sha256": sha256(binary),
    }
    write_report(report, run_dir)
    release_smoke(
        report, cell["phases"], binary, consumer, check_env, run_dir, timeout, sample_ms, logs,
    )


def measure_pbrs_cell(
    report: dict, case: str, names: list[str], corpus: dict, rep: int,
    case_dir: Path, pbrs_binary: Path, cargo: str, protoc: str,
    base_env: dict[str, str], run_dir: Path, timeout: int, sample_ms: int,
    reference_protoc: Path | None, generation_only: bool,
) -> None:
    consumer = case_dir / "consumer"
    generated = consumer / "generated"
    target_dir = case_dir / f"target-r{rep}"
    package = f"cg19-consumer-{case}"
    cell = {
        "case": case,
        "generator": "pbrs",
        "repeat": rep,
        "corpus": corpus,
        "target_dir": relative(target_dir, run_dir),
        "phases": {},
    }
    cell["profile"] = pbrs_profile(base_env)
    report["cells"].append(cell)
    write_report(report, run_dir)
    check_env = {**base_env, "CARGO_TARGET_DIR": str(target_dir), "CARGO_INCREMENTAL": "1"}
    logs = run_dir / "logs" / case / f"r{rep}"
    phase(
        report, cell["phases"], "consumer_lock",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "consumer-lock",
    )
    cell["consumer_lock_sha256"] = sha256(consumer / "Cargo.lock")
    generation = [
        str(pbrs_binary), str(consumer / "proto"), str(generated), protoc, *names
    ]
    phase(
        report, cell["phases"], "generation", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation",
    )
    before = snapshot_generated(generated)
    cell["shared_descriptor_helper"] = pbrs_helper_provenance(before, cell["profile"], len(names))
    if case in REALISTIC_CORPORA:
        expected_files = realistic_pbrs_expected(names)
        if cell["shared_descriptor_helper"]["active"]:
            expected_files |= {cell["shared_descriptor_helper"]["path"]}
        if before.keys() != expected_files:
            raise BenchmarkError(
                f"{case}: unexpected pbrs Rust outputs: "
                f"missing={sorted(expected_files - before.keys())}, "
                f"extra={sorted(before.keys() - expected_files)}"
            )
    else:
        expected_files = {"mod.rs", *(name.removesuffix(".proto") + ".rs" for name in names)}
        missing = expected_files - before.keys()
        if missing:
            raise BenchmarkError(f"{case}: missing generated Rust outputs: {sorted(missing)}")
    phase(
        report, cell["phases"], "generation_unchanged", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation-unchanged",
    )
    assert_unchanged(before, snapshot_generated(generated))
    cell["output"] = {
        "rust_file_count": len(before),
        "rust_bytes": sum(item[2] for item in before.values()),
        "rust_tree_sha256": tree_digest(before),
        "unchanged_generation_mtimes_preserved": True,
        "unchanged_generation_verified_files": len(before),
    }
    write_report(report, run_dir)
    if generation_only:
        return
    measure_consumer_build(
        report, cell, case, consumer, target_dir, package, False, cargo, check_env,
        run_dir, timeout, sample_ms, logs,
    )
    if reference_protoc is not None:
        if case in REALISTIC_CORPORA:
            # The legacy CG-19 reference flow is single-directory only; the
            # v4 matrix cell is the comparison on realistic corpora.
            cell["reference"] = {"skipped": "single-dir reference flow; see v4 matrix cell"}
        else:
            measure_reference(
                report, cell, case, names, rep, case_dir, cargo, protoc, base_env,
                run_dir, timeout, sample_ms,
            )
            compare_cell(report, cell, run_dir)


def measure_peer_cell(
    report: dict, case: str, names: list[str], corpus: dict, generator: str, rep: int,
    case_dir: Path, generator_binary: Path, cargo: str, protoc: str,
    base_env: dict[str, str], run_dir: Path, timeout: int, sample_ms: int,
    generation_only: bool,
) -> None:
    if case in REALISTIC_CORPORA:
        messages = len(realistic_entries(case))
        services: list[tuple[str, int, int]] | None = None
    elif case in STUB_CORPORA:
        messages = STUB_CORPORA[case][0]
        services = stub_services(case)
    else:
        messages = CORPORA[case][0]
        services = None
    consumer, package = prepare_peer_consumer(case_dir, case, messages, generator, services)
    generated = consumer / "generated"
    generated.mkdir(parents=True, exist_ok=True)
    target_dir = case_dir / "gen" / generator / f"target-r{rep}"
    cell = {
        "case": case,
        "generator": generator,
        "repeat": rep,
        "corpus": corpus,
        "protoc": protoc,
        "target_dir": relative(target_dir, run_dir),
        "phases": {},
    }
    if generator in PBRS_STUB_ENV:
        cell["profile"] = pbrs_profile(base_env, generator)
    report["cells"].append(cell)
    write_report(report, run_dir)
    check_env = {**base_env, "CARGO_TARGET_DIR": str(target_dir), "CARGO_INCREMENTAL": "1"}
    logs = run_dir / "logs" / case / "gen" / generator / f"r{rep}"
    phase(
        report, cell["phases"], "consumer_lock",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "consumer-lock",
    )
    cell["consumer_lock_sha256"] = sha256(consumer / "Cargo.lock")
    if generator == "v4":
        cell["runtime_lock"] = reference_lock(consumer / "Cargo.lock")
    write_report(report, run_dir)
    proto_dir = case_dir / "consumer" / "proto"
    gen_env = base_env
    if generator in PBRS_STUB_ENV:
        generation = [
            str(generator_binary), str(proto_dir), str(generated), protoc, *names,
        ]
        gen_env = {**base_env, "SB09_PBRS_STUBS": PBRS_STUB_ENV[generator]}
    elif generator == "v4":
        generation = v4_generation_command(str(generator_binary), proto_dir, generated, names)
    else:
        generation = [str(generator_binary), str(proto_dir), str(generated), *names]
    phase(
        report, cell["phases"], "generation", generation, ROOT, gen_env,
        run_dir, timeout, sample_ms, logs / "generation",
    )
    entrypoint = PEER_ENTRYPOINT[generator]
    if case in REALISTIC_CORPORA:
        if generator == "v4":
            entrypoint = v4_entrypoint_rel(case)
        elif generator == "prost":
            entrypoint = prost_package_files(case)[0]
    if generator == "buffa" and case in REALISTIC_CORPORA:
        expected = realistic_buffa_core(names, case, corpus["content"])
        before = snapshot_generated(generated, entrypoint, min_files=1)
        assert_buffa_realistic_outputs(case, generator, before, expected)
    else:
        expected = peer_expected_files(generator, names, case)
        before = snapshot_generated(generated, entrypoint, min_files=1)
        if generator in PBRS_STUB_ENV:
            cell["shared_descriptor_helper"] = pbrs_helper_provenance(before, cell["profile"], len(names))
            if cell["shared_descriptor_helper"]["active"]:
                expected |= {cell["shared_descriptor_helper"]["path"]}
        if before.keys() != expected:
            raise BenchmarkError(
                f"{case}/{generator}: unexpected Rust outputs: "
                f"missing={sorted(expected - before.keys())}, extra={sorted(before.keys() - expected)}"
            )
    if generator == "v4" and case in REALISTIC_CORPORA:
        found = sorted(
            path.relative_to(generated).as_posix()
            for path in generated.rglob("generated.rs")
        )
        if found != [entrypoint]:
            raise BenchmarkError(
                f"{case}/v4: generated.rs search {found} != predicted [{entrypoint}]"
            )
    phase(
        report, cell["phases"], "generation_unchanged", generation, ROOT, gen_env,
        run_dir, timeout, sample_ms, logs / "generation-unchanged",
    )
    if generator in STRICT_PEERS:
        assert_unchanged(before, snapshot_generated(generated, entrypoint, min_files=1))
        cell["output"] = {
            "rust_file_count": len(before),
            "rust_bytes": sum(item[2] for item in before.values()),
            "rust_tree_sha256": tree_digest(before),
            "unchanged_generation_verified_files": len(before),
        }
    else:
        rewritten = assert_same_bytes(
            before, snapshot_generated(generated, entrypoint, min_files=1),
        )
        cell["output"] = {
            "rust_file_count": len(before),
            "rust_bytes": sum(item[2] for item in before.values()),
            "rust_tree_sha256": tree_digest(before),
            "unchanged_generation_bytes_verified": True,
            "unchanged_generation_mtimes_preserved": rewritten == 0,
            "unchanged_generation_rewritten_files": rewritten,
        }
    write_report(report, run_dir)
    if generation_only:
        return
    measure_consumer_build(
        report, cell, case, consumer, target_dir, package, False, cargo, check_env,
        run_dir, timeout, sample_ms, logs, generator=generator, messages=messages,
        services=services,
    )


def measure_reference(
    report: dict, cell: dict, case: str, names: list[str], rep: int, case_dir: Path, cargo: str,
    protoc: str, base_env: dict[str, str], run_dir: Path, timeout: int, sample_ms: int,
) -> dict:
    consumer = case_dir / f"reference-r{rep}" / "consumer"
    generated = consumer / "generated"
    generated.mkdir(parents=True, exist_ok=True)
    package = f"cg19-reference-{case}"
    write_text(consumer / "Cargo.toml", manifest(package, reference=True))
    write_text(consumer / "src" / "main.rs", render_consumer(CORPORA[case][0], 0, reference=True))
    target_dir = case_dir / f"reference-r{rep}" / "target"
    reference = {
        "corpus_sha256": cell["corpus"]["sha256"],
        "proto_inputs": [item["path"] for item in cell["corpus"]["inputs"]],
        "target_dir": relative(target_dir, run_dir),
        "phases": {},
    }
    cell["reference"] = reference
    write_report(report, run_dir)
    check_env = {**base_env, "CARGO_TARGET_DIR": str(target_dir), "CARGO_INCREMENTAL": "1"}
    logs = run_dir / "logs" / case / f"reference-r{rep}"
    phase(
        report, reference["phases"], "consumer_lock",
        [cargo, "generate-lockfile", "--offline", "--manifest-path", str(consumer / "Cargo.toml")],
        ROOT, check_env, run_dir, timeout, sample_ms, logs / "consumer-lock",
    )
    report["reference"]["runtime"] = reference_lock(consumer / "Cargo.lock")
    reference["consumer_lock_sha256"] = report["reference"]["runtime"]["lock_sha256"]
    write_report(report, run_dir)
    generation = [
        protoc, f"--proto_path={case_dir / 'consumer' / 'proto'}",
        f"--rust_out={generated}", f"--rust_opt={REFERENCE_RUST_OPT}", *names,
    ]
    phase(
        report, reference["phases"], "generation", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation",
    )
    before = snapshot_generated(generated, "generated.rs")
    expected = {"generated.rs", *(name.removesuffix(".proto") + ".u.pb.rs" for name in names)}
    if before.keys() != expected:
        raise BenchmarkError(
            f"{case}: unexpected reference Rust outputs: "
            f"missing={sorted(expected - before.keys())}, extra={sorted(before.keys() - expected)}"
        )
    phase(
        report, reference["phases"], "generation_unchanged", generation, ROOT, base_env,
        run_dir, timeout, sample_ms, logs / "generation-unchanged",
    )
    rewritten = assert_same_bytes(before, snapshot_generated(generated, "generated.rs"))
    reference["output"] = {
        "rust_file_count": len(before),
        "rust_bytes": sum(item[2] for item in before.values()),
        "rust_tree_sha256": tree_digest(before),
        "unchanged_generation_bytes_verified": True,
        "unchanged_generation_mtimes_preserved": rewritten == 0,
        "unchanged_generation_rewritten_files": rewritten,
    }
    write_report(report, run_dir)
    measure_consumer_build(
        report, reference, case, consumer, target_dir, package, True, cargo, check_env,
        run_dir, timeout, sample_ms, logs,
    )
    return reference


def compare_cell(report: dict, cell: dict, run_dir: Path) -> None:
    reference = cell["reference"]
    if cell["corpus"]["sha256"] != reference["corpus_sha256"]:
        raise BenchmarkError("paired consumers did not use the same corpus")
    pairs = {
        "output.rust_bytes": (cell["output"]["rust_bytes"], reference["output"]["rust_bytes"]),
        "release_binary.size_bytes": (
            cell["release_binary"]["size_bytes"], reference["release_binary"]["size_bytes"],
        ),
    }
    for phase_name in ("generation", "generation_unchanged", "check_clean",
                       "check_incremental", "build_release"):
        for metric in ("elapsed_ns", "peak_rss_bytes"):
            pairs[f"{phase_name}.{metric}"] = (
                cell["phases"][phase_name][metric],
                reference["phases"][phase_name][metric],
            )
    rows = []
    for metric, (pbrs, pinned) in pairs.items():
        if (not isinstance(pbrs, int) or isinstance(pbrs, bool) or pbrs < 0
                or not isinstance(pinned, int) or isinstance(pinned, bool) or pinned < 0):
            raise BenchmarkError(f"invalid paired measurement for {cell['case']}: {metric}")
        rows.append({
            "case": cell["case"], "repeat": cell.get("repeat", 0),
            "metric": metric, "pbrs": pbrs, "reference": pinned,
            "pbrs_loses": pbrs > pinned,
            "rss_peak_is_lower_bound": metric.endswith("peak_rss_bytes"),
        })
    if report["comparison"]["losing_cells"] is None:
        report["comparison"]["losing_cells"] = []
    report["comparison"]["metrics"].extend(rows)
    report["comparison"]["losing_cells"].extend(row for row in rows if row["pbrs_loses"])
    report["comparison"]["status"] = "partial"
    write_report(report, run_dir)


MATRIX_PHASES = (
    "generation", "generation_unchanged", "check_clean", "check_incremental", "build_release",
)
MATRIX_PHASE_METRICS = ("elapsed_ns", "peak_rss_bytes")


def matrix_metrics(cell: dict) -> dict[str, int]:
    metrics = {"output.rust_bytes": cell["output"]["rust_bytes"]}
    if "release_binary" in cell:
        metrics["release_binary.size_bytes"] = cell["release_binary"]["size_bytes"]
    for phase_name in MATRIX_PHASES:
        if phase_name not in cell["phases"]:
            continue
        for metric in MATRIX_PHASE_METRICS:
            metrics[f"{phase_name}.{metric}"] = cell["phases"][phase_name][metric]
    return metrics


def summarize(values: list[int]) -> dict:
    return {
        "n": len(values),
        "min": min(values),
        "median": statistics.median(values),
        "mean": statistics.mean(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else None,
    }


def generators_for_case(
    case: str, generators: tuple[str, ...], stub_generators: tuple[str, ...],
) -> tuple[str, ...]:
    if case in STUB_CORPORA:
        return stub_generators
    if case in REALISTIC_CORPORA:
        excluded = REALISTIC_CORPORA[case]["exclude"]
        return tuple(g for g in generators if g not in excluded)
    return generators


def excluded_cells(
    cases: list[str], generators: tuple[str, ...],
) -> list[dict[str, str]]:
    return [
        {
            "case": case,
            "generator": generator,
            "reason": REALISTIC_CORPORA[case]["exclude"][generator],
        }
        for case in cases
        if case in REALISTIC_CORPORA
        for generator in generators
        if generator in REALISTIC_CORPORA[case]["exclude"]
    ]


def plan_execution(
    cases: list[str], generators: tuple[str, ...], stub_generators: tuple[str, ...],
    repeats: int, seed: int,
) -> list[tuple[str, str, int]]:
    work = [
        (case, generator, rep)
        for case in cases
        for generator in generators_for_case(case, generators, stub_generators)
        for rep in range(repeats)
    ]
    if not work:
        raise BenchmarkError(
            "no applicable generators for the selected cases: message cases need "
            "--generators, stub cases need --stub-generators"
        )
    return random.Random(seed).sample(work, len(work))


def compute_matrix(report: dict) -> None:
    groups: dict[tuple[str, str], list[dict]] = {}
    for cell in report["cells"]:
        groups.setdefault((cell["case"], cell["generator"]), []).append(cell)
    tabulated = {}
    for (case, generator), cells in sorted(groups.items()):
        per_metric: dict[str, list[int]] = {}
        for cell in cells:
            for metric, value in matrix_metrics(cell).items():
                per_metric.setdefault(metric, []).append(value)
        tabulated[f"{case}/{generator}"] = {
            "case": case,
            "generator": generator,
            "repeats": sorted(cell["repeat"] for cell in cells),
            "metrics": {
                metric: summarize(values) for metric, values in sorted(per_metric.items())
            },
        }
    losses = []
    for case in sorted({case for case, _ in groups}):
        baseline = tabulated.get(f"{case}/pbrs")
        if baseline is None:
            continue
        for (cell_case, generator) in sorted(groups):
            if cell_case != case or generator == "pbrs":
                continue
            peer = tabulated[f"{case}/{generator}"]
            for metric, summary in peer["metrics"].items():
                pbrs_median = baseline["metrics"][metric]["median"]
                gen_median = summary["median"]
                losses.append({
                    "case": case,
                    "generator": generator,
                    "metric": metric,
                    "pbrs_median": pbrs_median,
                    "generator_median": gen_median,
                    "generator_loses": gen_median > pbrs_median,
                })
    report["matrix"] = {
        "repeats": report.get("repeats"),
        "cells": tabulated,
        "losses": losses,
    }


def run_cases(
    report: dict, run_dir: Path, cases: list[str], seed: int, jobs: int, timeout: int,
    sample_ms: int, reference_protoc: Path | None = None,
    generators: tuple[str, ...] = ("pbrs",), repeats: int = 5,
    stub_generators: tuple[str, ...] = ("pbrs-native",), generation_only: bool = False,
) -> None:
    unknown = [name for name in generators if name not in MESSAGE_GENERATORS]
    if unknown:
        raise BenchmarkError(f"unknown generators: {unknown}")
    unknown_stubs = [name for name in stub_generators if name not in STUB_GENERATORS]
    if unknown_stubs:
        raise BenchmarkError(f"unknown stub generators: {unknown_stubs}")
    if repeats < 1:
        raise BenchmarkError(f"repeats must be at least 1, got {repeats}")
    details = provenance(run_dir, jobs, sample_ms, reference_protoc)
    details.setdefault("measurement", {})["generation_only"] = generation_only
    report["environment"] = details
    report["generators"] = list(generators)
    report["stub_generators"] = list(stub_generators)
    report["repeats"] = repeats
    if reference_protoc is not None:
        report["reference"].update({
            "status": "ready",
            "generator": "protobuf v35.1 built-in --rust_out (kernel=upb)",
            "revision": REFERENCE_REVISION,
            "protoc": details["tools"]["protoc"],
            "c_compiler": details["tools"]["cc"],
            "source": details["reference_source"],
            "runtime": None,
            "reason": "independent pinned-host and repeated paired qualification not performed",
        })
    write_report(report, run_dir)
    cargo = details["tools"]["cargo"]["executable"]
    protoc = details["tools"]["protoc"]["executable"]
    base_env = os.environ.copy()
    cleared = sorted(key for key in base_env if key.startswith("PURE_PROTOBUF_"))
    for key in cleared:
        del base_env[key]
    details["cache"]["cleared_codegen_env_keys"] = cleared
    protoc_link = run_dir / "bin" / "protoc"
    protoc_link.parent.mkdir()
    protoc_link.symlink_to(protoc)
    details["tools"]["core_build_script_protoc"] = relative(protoc_link, run_dir)
    base_env.update(
        {
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TERM_COLOR": "never",
            "CARGO_BUILD_JOBS": str(jobs),
            "RUSTC": details["tools"]["rustc"]["executable"],
            "PROTOC": protoc,
            "PATH": str(protoc_link.parent) + os.pathsep + base_env.get("PATH", ""),
            "LC_ALL": "C",
        }
    )
    if reference_protoc is not None:
        base_env["CC"] = details["tools"]["cc"]["executable"]
        details["cache"]["paired_cold_targets"] = (
            "separate initially nonexistent cases/<case>/target and cases/<case>/reference/target; "
            "serial Cargo jobs=2; shared registry and compiler-wrapper caches are not cleared"
        )
        details["measurement"]["reference_generation_includes"] = (
            "same proto inputs parsed by pinned protoc and emitted by built-in Rust --rust_out"
        )
        details["measurement"]["reference_check_clean"] = (
            "fresh target including protobuf 4.35.1-release and its C/upb build; "
            "no pbrs or ABI shim; local registry/compiler-wrapper caches may be warm"
        )
    write_report(report, run_dir)

    peer_drivers: dict[str, Path] = {}
    applicable = {
        generator
        for case in cases
        for generator in generators_for_case(case, generators, stub_generators)
    }
    report["pbrs_profiles"] = {
        generator: pbrs_profile(base_env, generator)
        for generator in sorted(applicable)
        if generator == "pbrs" or generator in PBRS_STUB_ENV
    }
    if any(profile["kind"] == "nondefault-diagnostic" for profile in report["pbrs_profiles"].values()):
        qualification = report.setdefault("qualification", {"qualified": False, "reasons": []})
        qualification["reasons"].append("nondefault_pbrs_profile_diagnostic")
    if not applicable:
        raise BenchmarkError(
            "no applicable generators for the selected cases: message cases need "
            "--generators, stub cases need --stub-generators"
        )
    need_pbrs_driver = "pbrs" in applicable or bool(applicable & PBRS_STUB_ENV.keys())
    boot_target = ROOT / "target" / "integration-consumers"
    boot_jobs = min(jobs, 2)
    details["cache"].update({
        "bootstrap_target_dir": str(boot_target),
        "bootstrap_build_jobs": boot_jobs,
        "bootstrap_cargo_incremental": base_env.get("CARGO_INCREMENTAL"),
        "bootstrap_shared": True,
        "bootstrap_included_in_measurements": False,
        "corpus_targets": "fresh cases/<case>/target; never use bootstrap target",
    })
    write_report(report, run_dir)
    boot_env = {**base_env, "CARGO_TARGET_DIR": str(boot_target), "CARGO_BUILD_JOBS": str(boot_jobs)}
    boot_log = run_dir / "logs" / "setup"
    if need_pbrs_driver:
        driver = run_dir / "driver"
        (driver / "src").mkdir(parents=True)
        shutil.copyfile(Path(__file__).with_name("generator.rs"), driver / "src" / "main.rs")
        write_text(driver / "Cargo.toml", manifest("cg19-generator"))
        phase(
            report, report["setup"], "driver_lock",
            [cargo, "generate-lockfile", "--offline", "--manifest-path", str(driver / "Cargo.toml")],
            ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / "driver-lock",
        )
        report["setup"]["driver_lock_sha256"] = sha256(driver / "Cargo.lock")
        write_report(report, run_dir)
        phase(
            report, report["setup"], "driver_build",
            [
                cargo, "build", "--offline", "--locked", "--manifest-path", str(driver / "Cargo.toml"),
                "--target-dir", str(boot_target), "--bin", "cg19-generator",
            ],
            ROOT, boot_env, run_dir, timeout, sample_ms, boot_log / "driver-build",
        )
        shared_generator = boot_target / "debug" / "cg19-generator"
        pbrs_binary, digest = copy_generator(shared_generator, run_dir)
        report["setup"]["shared_generator_binary_path"] = str(shared_generator)
        report["setup"]["generator_binary_path"] = relative(pbrs_binary, run_dir)
        report["setup"]["generator_binary_sha256"] = digest
        write_report(report, run_dir)
    for peer in generators:
        if peer == "pbrs" or peer not in applicable:
            continue
        if peer == "v4":
            pinned = resolve_pinned_protoc()
            report["setup"]["v4_protoc"] = str(pinned)
            report["setup"]["v4_protoc_sha256"] = sha256(pinned)
            write_report(report, run_dir)
            peer_drivers[peer] = pinned
            continue
        peer_drivers[peer] = build_peer_driver(
            report, run_dir, peer, cargo, boot_env, boot_target, timeout, sample_ms,
        )
    for peer in stub_generators:
        if peer not in applicable:
            continue
        if peer in PBRS_STUB_ENV:
            peer_drivers[peer] = pbrs_binary
        elif peer == "tonic-build":
            peer_drivers[peer] = build_peer_driver(
                report, run_dir, peer, cargo, boot_env, boot_target, timeout, sample_ms,
            )

    order = plan_execution(cases, generators, stub_generators, repeats, seed)
    report["execution_order"] = [f"{case}/{generator}/r{rep}" for case, generator, rep in order]
    write_report(report, run_dir)
    for case, generator, rep in order:
        case_dir = run_dir / "cases" / case
        names, corpus = prepare_corpus(case_dir, case, seed, (generator,))
        if generator == "pbrs":
            measure_pbrs_cell(
                report, case, names, corpus, rep, case_dir, pbrs_binary,
                cargo, protoc, base_env, run_dir, timeout, sample_ms,
                reference_protoc, generation_only,
            )
        else:
            measure_peer_cell(
                report, case, names, corpus, generator, rep, case_dir,
                peer_drivers[generator], cargo, protoc, base_env,
                run_dir, timeout, sample_ms, generation_only,
            )

    before = details["repository"]["source_sha256"]
    after = source_hashes(reference_protoc is not None)
    if before != after:
        changed = sorted(path for path in before.keys() | after.keys() if before.get(path) != after.get(path))
        raise BenchmarkError(f"source changed during measurement: {changed}")
    if reference_protoc is not None:
        if sha256(Path(protoc)) != REFERENCE_PROTOC_SHA256:
            raise BenchmarkError("pinned reference protoc changed during measurement")
        report["reference"]["status"] = "measured"
        report["comparison"]["status"] = "diagnostic"
    compute_matrix(report)
    write_report(report, run_dir)


def positive_int(value: str) -> int:
    number = int(value)
    if not 1 <= number <= 3600:
        raise argparse.ArgumentTypeError("must be between 1 and 3600")
    return number


def qualification_reasons(
    reference_mode: bool, has_peer: bool, repeats: int,
) -> list[str]:
    if reference_mode:
        reasons = ["no_independent_pinned_host_qualification"]
        if repeats < 5:
            reasons.extend(["single_run_diagnostic", "no_paired_replicates_or_uncertainty"])
        return reasons
    reasons = []
    if not has_peer:
        reasons.append("no_equivalent_reference_peer")
    if repeats < 5:
        reasons.append("single_run_diagnostic")
    return reasons


def build_jobs(value: str) -> int:
    try:
        number = int(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be an integer between 1 and 8") from exc
    if not 1 <= number <= 8:
        raise argparse.ArgumentTypeError("must be between 1 and 8")
    return number


def seed_arg(value: str) -> int:
    number = int(value)
    if not 0 <= number <= 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("seed must fit in 32 bits")
    return number


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--case", choices=["all", *CORPORA, *STUB_CORPORA, *REALISTIC_CORPORA],
        default="all",
    )
    parser.add_argument("--seed", type=seed_arg, default=DEFAULT_SEED)
    parser.add_argument("--out", type=Path, help="new directory under target/codegen-bench")
    inherited_jobs = os.environ.get("CARGO_BUILD_JOBS")
    try:
        default_jobs = build_jobs(inherited_jobs) if inherited_jobs is not None else 2
    except argparse.ArgumentTypeError as exc:
        parser.error(f"invalid CARGO_BUILD_JOBS: {exc}")
    parser.add_argument("--jobs", type=build_jobs, default=default_jobs)
    parser.add_argument("--timeout-seconds", type=positive_int, default=900)
    parser.add_argument("--rss-sample-ms", type=positive_int, default=100)
    parser.add_argument(
        "--reference-protoc", type=Path,
        help="opt into the SHA-pinned v35.1 upb peer for one explicit "
             "--case small|100|1000 --seed 190019 --jobs 2",
    )
    parser.add_argument(
        "--require-qualified", action="store_true",
        help="fail fast: even a local pinned-reference diagnostic lacks independent qualification",
    )
    parser.add_argument(
        "--generators", default="pbrs",
        help=f"comma-separated message generators to measure: {','.join(MESSAGE_GENERATORS)}",
    )
    parser.add_argument(
        "--repeats", type=positive_int, default=5,
        help="repeats per cell in seeded-random execution order (default: 5)",
    )
    parser.add_argument(
        "--stub-generators", default="pbrs-native",
        help=f"comma-separated stub generators for svc cases: {','.join(STUB_GENERATORS)}",
    )
    parser.add_argument(
        "--generation-only", action="store_true",
        help="measure generation and unchanged-output verification only; skip cargo check/build phases",
    )
    args = parser.parse_args(argv)
    generators = tuple(part for part in args.generators.split(",") if part)
    unknown = [name for name in generators if name not in MESSAGE_GENERATORS]
    if not generators or unknown:
        parser.error(
            f"--generators must be a comma-separated subset of {','.join(MESSAGE_GENERATORS)}"
        )
    stub_generators = tuple(part for part in args.stub_generators.split(",") if part)
    unknown_stubs = [name for name in stub_generators if name not in STUB_GENERATORS]
    if not stub_generators or unknown_stubs:
        parser.error(
            "--stub-generators must be a comma-separated subset of "
            f"{','.join(STUB_GENERATORS)}"
        )
    if args.reference_protoc is not None:
        if args.case == "all" or args.seed != DEFAULT_SEED or args.jobs != 2:
            parser.error(
                "--reference-protoc requires one explicit --case small|100|1000 "
                "--seed 190019 --jobs 2"
            )
        args.reference_protoc = Path(os.path.abspath(args.reference_protoc))
    allowed = (ROOT / "target" / "codegen-bench").resolve()
    run_dir = (
        args.out if args.out is not None
        else allowed / f"{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{os.getpid()}"
    ).resolve()
    if not run_dir.is_relative_to(allowed) or run_dir == allowed:
        parser.error("--out must be a new directory under target/codegen-bench")
    if run_dir.exists():
        parser.error(f"output already exists: {run_dir} (refusing to overwrite evidence)")
    run_dir.mkdir(parents=True)
    cases = (
        list(CORPORA) + list(STUB_CORPORA) + list(REALISTIC_CORPORA)
        if args.case == "all" else [args.case]
    )
    measured = {
        generator
        for case in cases
        for generator in generators_for_case(case, generators, stub_generators)
    }
    has_peer = any(generator != "pbrs" for generator in measured)
    report = {
        "schema_version": "cg19/1",
        "status": "pending",
        "started_at_utc": utc_now(),
        "seed": args.seed,
        "cases_requested": cases,
        "environment": None,
        "setup": {},
        "cells": [],
        "excluded": excluded_cells(cases, generators),
        "reference": {
            "status": "missing",
            "generator": None,
            "revision": None,
            "reason": "No equivalent pinned generator and consumer with matched reflection/API work is wired.",
        } if args.reference_protoc is None else {
            "status": "requested",
            "generator": "protobuf v35.1 built-in --rust_out (kernel=upb)",
            "revision": REFERENCE_REVISION,
            "requested_protoc": str(args.reference_protoc),
            "reason": "Pinned binary, source checkout and runtime lock have not been verified yet.",
        },
        "comparison": {"status": "not_run", "losing_cells": None}
        if args.reference_protoc is None else
        {"status": "not_run", "metrics": [], "losing_cells": None},
        "qualification": {
            "qualified": False,
            "reasons": qualification_reasons(
                args.reference_protoc is not None, has_peer, args.repeats,
            ),
        },
        "errors": [],
    }
    if len(cases) != len(CORPORA) + len(STUB_CORPORA) + len(REALISTIC_CORPORA):
        report["qualification"]["reasons"].append("partial_corpus_matrix")
    if args.generation_only:
        report["qualification"]["reasons"].append("generation_only")
    write_report(report, run_dir)
    if args.require_qualified:
        report["status"] = "unqualified"
        report["finished_at_utc"] = utc_now()
        write_report(report, run_dir)
        print(f"UNQUALIFIED: independent paired-host evidence is missing; {run_dir / 'summary.json'}",
              file=sys.stderr)
        return 2
    try:
        run_kwargs = {
            "generators": generators,
            "repeats": args.repeats,
            "stub_generators": stub_generators,
        }
        if args.generation_only:
            run_kwargs["generation_only"] = True
        run_cases(
            report, run_dir, cases, args.seed, args.jobs, args.timeout_seconds, args.rss_sample_ms,
            args.reference_protoc, **run_kwargs,
        )
    except (BenchmarkError, OSError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired, ValueError) as exc:
        report["status"] = "error"
        report["errors"].append(str(exc))
        if args.reference_protoc is not None and report["reference"]["status"] != "measured":
            report["reference"]["status"] = "incomplete"
            report["reference"]["reason"] = str(exc)
        report["qualification"]["reasons"].append("incomplete_measurement")
        report["finished_at_utc"] = utc_now()
        write_report(report, run_dir)
        print(f"CG-19 ERROR: {exc}; {run_dir / 'summary.json'}", file=sys.stderr)
        return 1
    report["status"] = "unqualified"
    report["finished_at_utc"] = utc_now()
    write_report(report, run_dir)
    if args.reference_protoc is None:
        print(f"UNQUALIFIED (no equivalent reference): {run_dir / 'summary.json'}")
    else:
        print(f"UNQUALIFIED (local pinned-reference diagnostic only): {run_dir / 'summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
