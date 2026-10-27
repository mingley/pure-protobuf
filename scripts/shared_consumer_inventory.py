"""Classify the pinned upstream suites and the repository's local regression suite."""

import argparse
from pathlib import Path
import tomllib


UPSTREAM_SUITES = (
    "accessors_map_test", "accessors_proto3_test", "accessors_repeated_test",
    "accessors_test", "bad_names_test", "child_parent_test", "edition2023_test",
    "enum_test", "fields_with_imported_types_test", "import_public_test",
    "message_copy_merge_test", "message_generics_test", "nested_types_test",
    "package_test", "proto_macro_test", "serialization_test", "simple_nested_test",
    "threading_test", "utf8_test",
)
LOCAL_SUITES = ("arena_recursion",)
EXCLUDED_UPSTREAM_SUITES = (
    "ctype_cord_test", "extensions_test", "gtest_matchers_test",
    "no_internal_access_test", "package_disambiguation_test",
)
EXPECTED_SUITES = UPSTREAM_SUITES + LOCAL_SUITES
EXPECTED_PATHS = {name: f"tests/{name}.rs" for name in EXPECTED_SUITES}


def upstream_source(upstream: Path, suite: str) -> Path:
    return upstream / ("utf8/utf8_test.rs" if suite == "utf8_test" else suite + ".rs")


def integration_targets(root: Path) -> dict[str, str]:
    consumer = root / "rust_out_shared"
    try:
        manifest = tomllib.loads((consumer / "Cargo.toml").read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ValueError(f"invalid consumer Cargo.toml: {error}") from error
    package = manifest.get("package", {})
    if package.get("edition", "2015") == "2015" and manifest.get("test") and "autotests" not in package:
        raise ValueError("declare autotests explicitly for edition 2015 with explicit test targets")
    autotests = package.get("autotests", True)
    if not isinstance(autotests, bool):
        raise ValueError("consumer package.autotests must be a boolean")
    targets = {}
    if autotests:
        paths = list((consumer / "tests").glob("*.rs"))
        paths.extend((consumer / "tests").glob("*/main.rs"))
        for path in paths:
            if not path.is_file():
                continue
            name = path.stem if path.parent == consumer / "tests" else path.parent.name
            if name in targets:
                raise ValueError(f"duplicate consumer target identity: {name}")
            targets[name] = path.relative_to(consumer).as_posix()
    explicit = set()
    for target in manifest.get("test", []):
        name = target.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError("explicit consumer test target needs a name")
        if name in explicit:
            raise ValueError(f"duplicate explicit consumer target: {name}")
        explicit.add(name)
        if not target.get("harness", True) or target.get("required-features"):
            raise ValueError(f"consumer target {name} needs its unconditional standard test harness")
        if not target.get("test", True):
            targets.pop(name, None)
            continue
        path = target.get("path", targets.get(name, f"tests/{name}.rs"))
        if not isinstance(path, str):
            raise ValueError(f"consumer target {name} needs a source path")
        targets[name] = path
    return targets


def validate_inventory(root: Path, upstream: Path) -> None:
    groups = (UPSTREAM_SUITES, LOCAL_SUITES, EXCLUDED_UPSTREAM_SUITES)
    flattened = sum(groups, ())
    if len(set(flattened)) != len(flattened):
        raise ValueError("duplicate suite classification")
    targets = integration_targets(root)
    missing = set(EXPECTED_SUITES) - targets.keys()
    unexpected = targets.keys() - set(EXPECTED_SUITES)
    if missing or unexpected:
        raise ValueError(f"consumer inventory mismatch: missing={sorted(missing)}, unexpected={sorted(unexpected)}")
    for name, path in targets.items():
        if path != EXPECTED_PATHS[name]:
            raise ValueError(f"consumer target path mismatch for {name}: {path}")
        if not (root / "rust_out_shared" / path).is_file():
            raise ValueError(f"missing consumer source for {name}")
    required = set(UPSTREAM_SUITES + EXCLUDED_UPSTREAM_SUITES)
    paths = [path for path in upstream.rglob("*_test.rs") if path.is_file()]
    actual = {path.stem for path in paths}
    if len(actual) != len(paths):
        raise ValueError("duplicate upstream suite identity")
    missing = required - actual
    unexpected = actual - required
    if missing or unexpected:
        raise ValueError(f"upstream inventory mismatch: missing={sorted(missing)}, unexpected={sorted(unexpected)}")
    for suite in required:
        if not upstream_source(upstream, suite).is_file():
            raise ValueError(f"missing upstream source for {suite}")


def validate_results(crates: dict[str, tuple[str, int, int]]) -> None:
    missing = set(EXPECTED_SUITES) - set(crates)
    unexpected = set(crates) - set(EXPECTED_SUITES)
    if missing or unexpected:
        raise ValueError(f"executed suite mismatch: missing={sorted(missing)}, unexpected={sorted(unexpected)}")
    for name, (status, passed, failed) in crates.items():
        if status != "ok" or failed != 0 or passed <= 0:
            raise ValueError(f"suite {name} did not pass a nonempty test set")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--upstream", type=Path)
    parser.add_argument("--count", action="store_true")
    args = parser.parse_args()
    if args.count:
        print(len(EXPECTED_SUITES))
        return
    upstream = args.upstream or args.root / "third_party/protobuf/rust/test/shared"
    try:
        validate_inventory(args.root, upstream)
    except ValueError as error:
        parser.exit(1, f"shared consumer inventory: {error}\n")
    print(f"Inventory OK: {len(UPSTREAM_SUITES)} upstream suites, {len(LOCAL_SUITES)} local regression suite, {len(EXCLUDED_UPSTREAM_SUITES)} documented upstream exclusions.")


if __name__ == "__main__":
    main()
