#!/usr/bin/env python3
"""Separate first-analysis cost from document cost in fresh Quarto LSP processes."""

import argparse
import hashlib
import itertools
import json
import math
import os
import shlex
import statistics
import tempfile
import time
from pathlib import Path

from lsp_memory import (
    CAPABILITIES,
    Client,
    isolated_environment,
    notify_and_diagnose,
    require_response,
)


def summarize(samples):
    return {
        "samples": len(samples),
        "median_ms": statistics.median(samples),
        "p95_ms": sorted(samples)[math.ceil(len(samples) * 0.95) - 1],
        "max_ms": max(samples),
    }


def run_process(command, project, documents, order, stderr_path):
    with tempfile.TemporaryDirectory(prefix="panache-first-open-") as state:
        env = isolated_environment(state)
        config = Path(state) / "panache.toml"
        config.write_text('flavor = "quarto"\n')
        started = time.perf_counter_ns()
        client = Client(
            [*command, "--config", str(config), "lsp"], project, env, stderr_path
        )
        try:
            initialized = require_response(
                client.request(
                    "initialize",
                    {
                        "processId": os.getpid(),
                        "rootUri": project.as_uri(),
                        "capabilities": CAPABILITIES,
                        "workspaceFolders": [
                            {"uri": project.as_uri(), "name": project.name}
                        ],
                    },
                    timeout=30,
                ),
                "initialize",
                require_result=True,
            )
            initialize_ms = (time.perf_counter_ns() - started) / 1_000_000
            if not initialized["result"]["capabilities"].get("diagnosticProvider"):
                raise RuntimeError("first-open benchmark requires pull diagnostics")
            client.notify("initialized", {})
            samples = []
            for position, index in enumerate(order, 1):
                document = documents[index]
                diagnostics, elapsed = notify_and_diagnose(
                    client,
                    "textDocument/didOpen",
                    {"textDocument": document},
                    True,
                    30,
                )
                since_launch = (time.perf_counter_ns() - started) / 1_000_000
                if any(item.get("severity") == 1 for item in diagnostics):
                    raise RuntimeError(f"corpus document has errors: {diagnostics}")
                samples.append(
                    {
                        "document": Path(document["uri"]).name,
                        "position": position,
                        "elapsed_ms": elapsed,
                        "since_launch_ms": since_launch,
                        "diagnostics": diagnostics,
                    }
                )
            return {"initialize_ms": initialize_ms, "opens": samples}
        finally:
            client.shutdown()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", required=True, help="command prefix, without lsp")
    parser.add_argument("--project", required=True, type=Path)
    parser.add_argument("--files", nargs=3, required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=48)
    parser.add_argument("--warmups", type=int, default=3)
    args = parser.parse_args()
    if args.runs <= 0 or args.runs % 6 or args.warmups < 0:
        parser.error("runs must be a positive multiple of six; warmups must be >= 0")
    project = args.project.resolve()
    files = [path.resolve() for path in args.files]
    documents = [
        {
            "uri": path.as_uri(),
            "languageId": "quarto",
            "version": 1,
            "text": path.read_text(),
        }
        for path in files
    ]
    orders = list(itertools.permutations(range(3)))
    runs = []
    args.out.parent.mkdir(parents=True, exist_ok=True)
    stderr_dir = args.out.with_suffix(".logs")
    stderr_dir.mkdir(exist_ok=True)
    for run in range(-args.warmups, args.runs):
        order = orders[max(run, 0) % len(orders)]
        record = run_process(
            shlex.split(args.server),
            project,
            documents,
            order,
            stderr_dir / f"process-{run + 1}.stderr.log",
        )
        if run >= 0:
            runs.append({"run": run + 1, "order": order, **record})
    summary = {
        f"{path.name}:position-{position}": summarize(
            [
                sample["elapsed_ms"]
                for run in runs
                for sample in run["opens"]
                if sample["document"] == path.name and sample["position"] == position
            ]
        )
        for path in files
        for position in range(1, 4)
    }
    result = {
        "server": args.server,
        "warmup_processes": args.warmups,
        "corpus_sha256": {
            path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in files
        },
        "initialize": summarize([run["initialize_ms"] for run in runs]),
        "first_diagnostics_since_launch": summarize(
            [run["opens"][0]["since_launch_ms"] for run in runs]
        ),
        "summary": summary,
        "runs": runs,
    }
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(
        json.dumps(
            {key: value for key, value in result.items() if key != "runs"}, indent=2
        )
    )


if __name__ == "__main__":
    main()
