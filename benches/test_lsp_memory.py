import importlib.util
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("lsp_memory.py")


def load_harness():
    spec = importlib.util.spec_from_file_location("lsp_memory", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load lsp_memory.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ProcParsingTests(unittest.TestCase):
    def test_rss_parser_reads_kibibytes(self):
        harness = load_harness()
        status = "Name:\tmarksman\nVmRSS:\t  12345 kB\nThreads:\t8\n"
        self.assertEqual(harness.parse_rss_kb(status), 12345)

    def test_cpu_parser_handles_spaces_and_parentheses_in_comm(self):
        harness = load_harness()
        prefix = "812 (server worker (one)) "
        fields = ["S"] + ["0"] * 10 + ["17", "19"] + ["0"] * 20
        self.assertEqual(harness.parse_cpu_ticks(prefix + " ".join(fields)), 36)


class AggregationTests(unittest.TestCase):
    def test_aggregate_runs_uses_medians(self):
        harness = load_harness()
        runs = [
            {
                "milestones": {
                    "baseline": {"rss_mb": 10.0, "pss_mb": 9.0, "processes": 1},
                    "settled": {"rss_mb": 20.0, "pss_mb": 18.0, "processes": 1},
                    "edited": {"rss_mb": 21.0, "pss_mb": 19.0, "processes": 1},
                    "peak": {"rss_mb": 22.0, "pss_mb": 20.0, "processes": 1},
                },
                "init_seconds": 1.0,
                "baseline_seconds": 2.0,
                "settled_seconds": 3.0,
                "edit_seconds": 4.0,
                "total_seconds": 5.0,
                "samples": 50,
            },
            {
                "milestones": {
                    "baseline": {"rss_mb": 12.0, "pss_mb": 11.0, "processes": 1},
                    "settled": {"rss_mb": 24.0, "pss_mb": 22.0, "processes": 1},
                    "edited": {"rss_mb": 25.0, "pss_mb": 23.0, "processes": 1},
                    "peak": {"rss_mb": 27.0, "pss_mb": 25.0, "processes": 1},
                },
                "init_seconds": 1.2,
                "baseline_seconds": 2.2,
                "settled_seconds": 3.2,
                "edit_seconds": 4.2,
                "total_seconds": 5.2,
                "samples": 52,
            },
            {
                "milestones": {
                    "baseline": {"rss_mb": 11.0, "pss_mb": 10.0, "processes": 1},
                    "settled": {"rss_mb": 22.0, "pss_mb": 20.0, "processes": 1},
                    "edited": {"rss_mb": 23.0, "pss_mb": 21.0, "processes": 1},
                    "peak": {"rss_mb": 25.0, "pss_mb": 23.0, "processes": 1},
                },
                "init_seconds": 1.1,
                "baseline_seconds": 2.1,
                "settled_seconds": 3.1,
                "edit_seconds": 4.1,
                "total_seconds": 5.1,
                "samples": 51,
            },
        ]

        aggregate = harness.aggregate_runs(runs)

        self.assertEqual(aggregate["milestones"]["settled"]["rss_mb"], 22.0)
        self.assertEqual(aggregate["milestones"]["edited"]["pss_mb"], 21.0)
        self.assertEqual(aggregate["timings"]["edit_seconds"], 4.1)
        self.assertEqual(aggregate["samples"], 51)

    def test_ratios_use_matching_panache_milestones(self):
        harness = load_harness()
        servers = [
            {
                "key": "panache",
                "aggregate": {
                    "milestones": {
                        "baseline": {"rss_mb": 10.0},
                        "settled": {"rss_mb": 20.0},
                        "edited": {"rss_mb": 25.0},
                        "peak": {"rss_mb": 30.0},
                    }
                },
            },
            {
                "key": "marksman",
                "aggregate": {
                    "milestones": {
                        "baseline": {"rss_mb": 20.0},
                        "settled": {"rss_mb": 50.0},
                        "edited": {"rss_mb": 75.0},
                        "peak": {"rss_mb": 120.0},
                    }
                },
            },
        ]

        harness.add_panache_ratios(servers)

        self.assertEqual(
            servers[1]["aggregate"]["relative_to_panache"]["edited_rss"], 3.0
        )
        self.assertEqual(
            servers[1]["aggregate"]["relative_to_panache"]["peak_rss"], 4.0
        )


class OutputTests(unittest.TestCase):
    def test_required_result_rejects_an_empty_definition(self):
        harness = load_harness()

        with self.assertRaisesRegex(RuntimeError, "returned no result"):
            harness.require_response(
                {"jsonrpc": "2.0", "id": 1, "result": []},
                "textDocument/definition",
                require_result=True,
            )

    def test_display_command_hides_ephemeral_config_path(self):
        harness = load_harness()

        command = harness.display_command(
            [
                "/tmp/build/panache",
                "--config",
                "/tmp/panache-lsp-memory.1234/panache.toml",
                "lsp",
            ]
        )

        self.assertEqual(command, "panache --config <isolated-gfm-config> lsp")


if __name__ == "__main__":
    unittest.main()
