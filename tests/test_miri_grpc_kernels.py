"""A green Miri command must have selected actual shipping-kernel tests."""

import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "miri_grpc_kernels", Path(__file__).resolve().parents[1] / "scripts/miri-grpc-kernels.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


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
