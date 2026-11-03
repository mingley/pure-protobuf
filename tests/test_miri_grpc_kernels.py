"""A green Miri command must have selected actual shipping-kernel tests."""

import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "miri_grpc_kernels", Path(__file__).resolve().parents[1] / "scripts/miri-grpc-kernels.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

zero_spec = importlib.util.spec_from_file_location(
    "miri_zero_defaults", Path(__file__).resolve().parents[1] / "scripts/miri-zero-defaults.py")
zero_runner = importlib.util.module_from_spec(zero_spec)
zero_spec.loader.exec_module(zero_runner)


class KernelInventoryTests(unittest.TestCase):
    def test_wrong_package_or_empty_selection_is_rejected(self):
        for output in ["0 tests, 0 benchmarks\n", "core::parse: test\n"]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                runner.validate_inventory(output)

    def test_missing_group_and_benchmark_only_match_are_rejected(self):
        present = "".join(group + "case: test\n" for group in runner.GROUPS[:-1])
        with self.assertRaises(ValueError):
            runner.validate_inventory(present + runner.GROUPS[-1] + "case: benchmark\n")
        self.assertEqual(len(runner.validate_inventory(
            present + runner.GROUPS[-1] + "case: test\n")), len(runner.GROUPS))


class ZeroDefaultInventoryTests(unittest.TestCase):
    def test_library_and_runtime_controls_are_required(self):
        for required in (zero_runner.LIBRARY_TESTS, zero_runner.RUNTIME_TESTS):
            complete = "".join(name + ": test\n" for name in required)
            self.assertEqual(zero_runner.validate_inventory(complete, required), list(required))
            for invalid in ("0 tests, 0 benchmarks\n", complete.split("\n", 1)[1],
                            complete + required[0] + ": test\n"):
                with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                    zero_runner.validate_inventory(invalid, required)

    def test_inventory_cannot_substitute_for_executing_required_controls(self):
        output = "required::default_case: test\nother::zeroed: test\n"
        with self.assertRaises(ValueError):
            zero_runner.validate_inventory(output, ("required::default_case",), ("zeroed",))
        self.assertEqual(
            zero_runner.validate_inventory(output, ("required::default_case",), ("required::",)),
            ["required::default_case", "other::zeroed"])

    def test_actual_results_must_match_inventory_without_ignores(self):
        valid = "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 123 filtered out;\n"
        zero_runner.validate_result(valid, total=127, selected=4)
        for invalid in (
            valid.replace("4 passed", "0 passed"),
            valid.replace("4 passed", "3 passed").replace("0 ignored", "1 ignored"),
            valid.replace("123 filtered", "122 filtered"),
            valid + valid,
            "test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 123 filtered out;\n",
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                zero_runner.validate_result(invalid, total=127, selected=4)
        with self.assertRaises(ValueError):
            zero_runner.validate_result(
                valid.replace("4 passed", "0 passed").replace("123 filtered", "127 filtered"),
                total=127, selected=0)
