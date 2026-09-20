import importlib.util
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

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

    def test_quiet_window_excludes_earlier_phases_and_short_waits(self):
        harness = load_harness()
        sampler = harness.Sampler(1)
        sampler.samples = [(t, 1, 1, 0, 1) for t in (0.0, 2.5, 4.0)]
        self.assertIsNone(sampler.quiet_since(5.0))
        sampler.samples.append((5.0, 1, 1, 0, 1))
        self.assertEqual(sampler.quiet_since(5.0), 0.0)
        self.assertIsNone(sampler.quiet_since(5.0, not_before=1.0))

    def test_busy_window_is_not_ready(self):
        harness = load_harness()
        sampler = harness.Sampler(1)
        sampler.samples = [
            (t, 1, 1, int(t * harness.CLK_TCK), 1) for t in (0.0, 2.5, 5.0)
        ]
        self.assertIsNone(sampler.quiet_since(5.0))


class LatencyTests(unittest.TestCase):
    def test_edit_latency_includes_the_change_notification(self):
        harness = load_harness()
        clock_ns = [0]

        def notify(*args):
            clock_ns[0] += 2_000_000

        def request(*args, **kwargs):
            clock_ns[0] += 5_000_000
            return {"result": [{"uri": "file:///a.md", "range": {}}]}

        client = Mock()
        client.notify.side_effect = notify
        client.request.side_effect = request
        target = {
            "uri": "file:///a.md",
            "label_length": 13,
            "link": {"line": 0, "character": 4},
            "definition": {"line": 2, "character": 1},
        }
        with patch.object(
            harness.time, "perf_counter_ns", side_effect=lambda: clock_ns[0]
        ):
            record = harness.churn_reference_labels(client, target, edits=2, timeout=30)
        self.assertEqual(record["median_ms"], 7.0)
        self.assertEqual(record["samples"], 2)

    def test_warmups_are_excluded_and_results_are_counted(self):
        harness = load_harness()
        client = Mock()
        client.request.return_value = {
            "result": [{"name": "Chapter", "children": [{"name": "Section"}]}]
        }
        with patch.object(
            harness.time,
            "perf_counter_ns",
            side_effect=[0, 1_000_000, 2_000_000, 5_000_000],
        ):
            record = harness.benchmark_requests(
                client,
                "document_symbol",
                "Document symbols",
                "textDocument/documentSymbol",
                [{"textDocument": {"uri": "file:///a.md"}}],
                runs=2,
                warmups=1,
                timeout=30,
            )
        self.assertEqual(client.request.call_count, 3)
        self.assertEqual(record["samples"], 2)
        self.assertEqual(record["median_ms"], 2.0)
        self.assertEqual(record["p95_ms"], 3.0)
        self.assertEqual(record["result_count_min"], 2)
        self.assertEqual(record["empty_results"], 0)

    def test_timeouts_do_not_become_fast_samples(self):
        harness = load_harness()
        client = Mock()
        client.request.return_value = None
        with self.assertRaisesRegex(RuntimeError, "timed out"):
            harness.benchmark_requests(
                client,
                "hover",
                "Hover",
                "textDocument/hover",
                [{}],
                runs=1,
                warmups=0,
                timeout=30,
            )

    def test_empty_responses_are_visible_in_result_counts(self):
        harness = load_harness()
        client = Mock()
        client.request.return_value = {"result": None}
        record = harness.benchmark_requests(
            client,
            "hover",
            "Hover",
            "textDocument/hover",
            [{}],
            runs=2,
            warmups=1,
            timeout=30,
        )
        self.assertEqual(record["empty_results"], 2)
        self.assertEqual(record["result_count_min"], 0)
        self.assertEqual(record["samples"], 2)

    def test_protocol_errors_abort_measurement(self):
        harness = load_harness()
        client = Mock()
        client.request.return_value = {
            "error": {"code": -32601, "message": "Unknown method"}
        }
        with self.assertRaisesRegex(RuntimeError, "failed"):
            harness.benchmark_requests(
                client,
                "hover",
                "Hover",
                "textDocument/hover",
                [{}],
                runs=1,
                warmups=0,
                timeout=30,
            )

    def test_pooling_uses_all_samples_instead_of_run_percentiles(self):
        harness = load_harness()
        records = [
            {
                "key": "hover",
                "label": "Hover",
                "method": "textDocument/hover",
                "targets": 1,
                "empty_results": 0,
                "_latencies_ms": [1.0, 2.0, 3.0],
                "_result_counts": [1, 1, 1],
                "_result_files": [],
                "_payload_bytes": [10, 10, 10],
            },
            {
                "key": "hover",
                "label": "Hover",
                "method": "textDocument/hover",
                "targets": 1,
                "empty_results": 1,
                "_latencies_ms": [100.0],
                "_result_counts": [0],
                "_result_files": [],
                "_payload_bytes": [4],
            },
        ]
        pooled = harness.aggregate_latencies(
            [{"request_latencies": [record]} for record in records]
        )[0]
        self.assertEqual(pooled["samples"], 4)
        self.assertEqual(pooled["median_ms"], 2.5)
        self.assertEqual(pooled["p95_ms"], 100.0)
        self.assertEqual(pooled["empty_results"], 1)
        self.assertEqual(pooled["result_count_min"], 0)
        self.assertFalse(any(key.startswith("_") for key in pooled))

    def test_navigation_counts_work_across_files(self):
        harness = load_harness()
        self.assertEqual(
            harness.result_summary(
                "textDocument/definition",
                [
                    {"uri": "file:///a.md"},
                    {"targetUri": "file:///b.md"},
                ],
            ),
            (2, 2),
        )
        self.assertEqual(
            harness.result_summary(
                "textDocument/rename",
                {
                    "changes": {"file:///a.md": [{}, {}]},
                    "documentChanges": [
                        {"textDocument": {"uri": "file:///b.md"}, "edits": [{}]}
                    ],
                },
            ),
            (3, 2),
        )


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

        for run in runs:
            run.update(
                {
                    "workspace_ready_seconds": 0.123456,
                    "documents_ready_seconds": 0.234567,
                    "edit_work_seconds": 0.345678,
                    "request_latencies": [],
                }
            )
        aggregate = harness.aggregate_runs(runs)

        self.assertEqual(aggregate["milestones"]["settled"]["rss_mb"], 22.0)
        self.assertEqual(aggregate["milestones"]["edited"]["pss_mb"], 21.0)
        self.assertEqual(aggregate["timings"]["edit_seconds"], 4.1)
        self.assertEqual(aggregate["timings"]["documents_ready_seconds"], 0.234567)
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
