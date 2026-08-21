#!/usr/bin/env python3
"""Measure resident memory across a scripted Markdown LSP session.

The harness launches each server over stdio, samples its complete process tree
from ``/proc``, and records four milestones: initialized baseline, files-opened
settled state, post-edit settled state, and the sampled peak. It intentionally
uses no third-party Python packages so a Panache development shell can run it
without additional setup.
"""

import argparse
import json
import os
import platform
import shlex
import signal
import statistics
import subprocess
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

CLK_TCK = os.sysconf("SC_CLK_TCK")
IDLE_CPU_FRACTION = 0.05
MILESTONES = ("baseline", "settled", "edited", "peak")
SCHEMA_VERSION = 1
SAMPLE_INTERVAL_SECONDS = 0.15
SERVER_META = {
    "panache": (
        "Panache",
        "open-document analysis with lazily discovered project relationships",
    ),
    "marksman": (
        "Marksman",
        "workspace-wide Markdown indexing and cross-reference analysis",
    ),
}


# --- /proc sampling ---------------------------------------------------------


def parse_rss_kb(status):
    """Return VmRSS from one ``/proc/<pid>/status`` payload."""
    for line in status.splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1])
    return 0


def parse_cpu_ticks(stat):
    """Return user + system ticks from one ``/proc/<pid>/stat`` payload."""
    fields = stat[stat.rindex(")") + 2 :].split()
    return int(fields[11]) + int(fields[12])


def process_tree(root_pid):
    """Return every live PID in the process tree rooted at ``root_pid``."""
    pids = {root_pid}
    frontier = [root_pid]
    while frontier:
        pid = frontier.pop()
        try:
            tasks = list(Path(f"/proc/{pid}/task").iterdir())
        except OSError:
            continue
        for task in tasks:
            try:
                children = (task / "children").read_text().split()
            except OSError:
                continue
            for child in map(int, children):
                if child not in pids:
                    pids.add(child)
                    frontier.append(child)
    return pids


def read_process(pid):
    """Return RSS, PSS, and CPU ticks for one process, or ``None`` if gone."""
    try:
        status = Path(f"/proc/{pid}/status").read_text()
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None

    rss = parse_rss_kb(status)
    try:
        pss = 0
        for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
            if line.startswith("Pss:"):
                pss = int(line.split()[1])
                break
    except OSError:
        pss = rss

    return rss, pss, parse_cpu_ticks(stat)


def sample_tree(root_pid):
    """Sum RSS, PSS, CPU ticks, and process count over a live process tree."""
    rss = pss = cpu = count = 0
    for pid in process_tree(root_pid):
        reading = read_process(pid)
        if reading is None:
            continue
        rss += reading[0]
        pss += reading[1]
        cpu += reading[2]
        count += 1
    return rss, pss, cpu, count


class Sampler(threading.Thread):
    def __init__(self, pid, interval=SAMPLE_INTERVAL_SECONDS):
        super().__init__(daemon=True)
        self.pid = pid
        self.interval = interval
        self.stop_flag = threading.Event()
        self.samples = []
        self.peak_rss = 0
        self.peak_pss = 0
        self.peak_processes = 0
        self.started_at = time.monotonic()

    def run(self):
        while not self.stop_flag.is_set():
            rss, pss, cpu, count = sample_tree(self.pid)
            if count:
                elapsed = time.monotonic() - self.started_at
                self.samples.append((elapsed, rss, pss, cpu, count))
                self.peak_rss = max(self.peak_rss, rss)
                self.peak_pss = max(self.peak_pss, pss)
                self.peak_processes = max(self.peak_processes, count)
            self.stop_flag.wait(self.interval)

    def milestone(self):
        rss, pss, _, count = sample_tree(self.pid)
        if not count:
            raise RuntimeError("server process tree exited before a memory milestone")
        self.peak_rss = max(self.peak_rss, rss)
        self.peak_pss = max(self.peak_pss, pss)
        self.peak_processes = max(self.peak_processes, count)
        return {
            "rss_mb": round(rss / 1024, 1),
            "pss_mb": round(pss / 1024, 1),
            "processes": count,
        }

    def peak(self):
        return {
            "rss_mb": round(self.peak_rss / 1024, 1),
            "pss_mb": round(self.peak_pss / 1024, 1),
            "processes": self.peak_processes,
        }

    def is_quiet(self, seconds):
        if not self.samples:
            return False
        cutoff = self.samples[-1][0] - seconds
        window = [sample for sample in self.samples if sample[0] >= cutoff]
        span = window[-1][0] - window[0][0] if len(window) >= 3 else 0.0
        if span < seconds * 0.8:
            return False
        cpu_seconds = (window[-1][3] - window[0][3]) / CLK_TCK
        return cpu_seconds / span < IDLE_CPU_FRACTION


# --- minimal LSP client -----------------------------------------------------


class Client:
    def __init__(self, command, cwd, env, stderr_path):
        self.stderr_file = open(stderr_path, "wb")  # noqa: SIM115
        self.proc = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr_file,
            start_new_session=True,
        )
        if self.proc.stdin is None or self.proc.stdout is None:
            raise RuntimeError("failed to open language-server stdio")
        self.stdin = self.proc.stdin
        self.stdout = self.proc.stdout
        self.next_id = 1
        self.write_lock = threading.Lock()
        self.responses = {}
        self.notifications = []
        self.state = threading.Condition()
        self.alive = True
        threading.Thread(target=self._read_loop, daemon=True).start()

    def _read_loop(self):
        while True:
            length = None
            while True:
                line = self.stdout.readline()
                if not line:
                    with self.state:
                        self.alive = False
                        self.state.notify_all()
                    return
                line = line.strip()
                if not line:
                    break
                if line.lower().startswith(b"content-length:"):
                    length = int(line.split(b":", 1)[1])
            if length is None:
                continue
            try:
                message = json.loads(self.stdout.read(length))
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue
            with self.state:
                if "id" in message and ("result" in message or "error" in message):
                    self.responses[message["id"]] = message
                elif "method" in message:
                    self.notifications.append(message)
                    if "id" in message:
                        self._answer(message)
                self.state.notify_all()

    def _answer(self, request):
        if request["method"] == "workspace/configuration":
            items = (request.get("params") or {}).get("items") or []
            result = [None] * max(1, len(items))
        else:
            result = None
        self._send({"jsonrpc": "2.0", "id": request["id"], "result": result})

    def _send(self, message):
        payload = json.dumps(message).encode()
        with self.write_lock:
            try:
                self.stdin.write(b"Content-Length: %d\r\n\r\n" % len(payload))
                self.stdin.write(payload)
                self.stdin.flush()
            except (BrokenPipeError, ValueError, OSError):
                pass

    def notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def request(self, method, params, timeout):
        with self.state:
            request_id = self.next_id
            self.next_id += 1
        self._send(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        )
        deadline = time.monotonic() + timeout
        with self.state:
            while request_id not in self.responses:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not self.alive:
                    return None
                self.state.wait(min(1.0, remaining))
            return self.responses.pop(request_id)

    def count_published_diagnostics(self):
        with self.state:
            return sum(
                notification.get("method") == "textDocument/publishDiagnostics"
                for notification in self.notifications
            )

    def shutdown(self):
        if self.proc.poll() is None:
            response = self.request("shutdown", None, timeout=15)
            if response is not None and "error" not in response:
                self.notify("exit", None)
                try:
                    self.proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    pass
        self.kill()

    def kill(self):
        if self.proc.poll() is None:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
            except (ProcessLookupError, PermissionError, OSError):
                pass
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                pass
        self.stderr_file.close()


CAPABILITIES = {
    "general": {"positionEncodings": ["utf-16"]},
    "workspace": {
        "workspaceFolders": True,
        "configuration": True,
        "didChangeConfiguration": {"dynamicRegistration": True},
        "diagnostics": {"refreshSupport": True},
        "symbol": {"dynamicRegistration": True},
    },
    "textDocument": {
        "synchronization": {"dynamicRegistration": True, "didSave": True},
        "publishDiagnostics": {"relatedInformation": True, "versionSupport": True},
        "diagnostic": {"dynamicRegistration": True, "relatedDocumentSupport": True},
        "hover": {"contentFormat": ["markdown", "plaintext"]},
        "definition": {"dynamicRegistration": True},
        "documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
    },
}


def require_response(response, method, require_result=False):
    if response is None:
        raise RuntimeError(f"{method} timed out or the server exited")
    if "error" in response:
        raise RuntimeError(f"{method} failed: {response['error']}")
    if require_result and not response.get("result"):
        raise RuntimeError(f"{method} returned no result")
    return response


def wait_until_quiet(client, sampler, quiet_seconds, timeout, phase):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if client.proc.poll() is not None:
            raise RuntimeError(
                f"server exited during {phase} (rc={client.proc.returncode})"
            )
        if sampler.is_quiet(quiet_seconds):
            return
        time.sleep(0.5)
    raise RuntimeError(f"server did not become quiet during {phase} within {timeout}s")


def label_for_edit(index):
    return f"memory-{index:06d}"


def utf16_length(text):
    return len(text.encode("utf-16-le")) // 2


def position_for_offset(text, offset):
    line = text.count("\n", 0, offset)
    line_start = text.rfind("\n", 0, offset) + 1
    return {"line": line, "character": utf16_length(text[line_start:offset])}


def prepare_documents(files):
    documents = []
    edit_target = None
    for index, path in enumerate(files):
        text = Path(path).read_text(errors="replace")
        if index == 0:
            if not text.endswith("\n"):
                text += "\n"
            initial_label = label_for_edit(0)
            fixture = (
                "\n<!-- lsp-memory-benchmark -->\n"
                f"[Memory benchmark][{initial_label}]\n\n"
                f"[{initial_label}]: https://example.com\n"
            )
            text += fixture
            link_offset = text.index(initial_label, text.index("[Memory benchmark]"))
            definition_offset = text.index(
                initial_label, link_offset + len(initial_label)
            )
            edit_target = {
                "link": position_for_offset(text, link_offset),
                "definition": position_for_offset(text, definition_offset),
                "label_length": utf16_length(initial_label),
            }
        documents.append(
            {
                "path": str(Path(path)),
                "uri": Path(path).as_uri(),
                "text": text,
            }
        )
    if edit_target is None:
        raise RuntimeError("at least one document is required")
    edit_target["uri"] = documents[0]["uri"]
    return documents, edit_target


def pull_diagnostics(client, uris, timeout):
    for uri in uris:
        require_response(
            client.request(
                "textDocument/diagnostic",
                {"textDocument": {"uri": uri}},
                timeout=timeout,
            ),
            "textDocument/diagnostic",
        )


def exercise_shared_requests(client, uris):
    for uri in uris[:3]:
        require_response(
            client.request(
                "textDocument/documentSymbol",
                {"textDocument": {"uri": uri}},
                timeout=60,
            ),
            "textDocument/documentSymbol",
        )
        require_response(
            client.request(
                "textDocument/hover",
                {"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}},
                timeout=60,
            ),
            "textDocument/hover",
        )
    require_response(
        client.request("workspace/symbol", {"query": ""}, timeout=60),
        "workspace/symbol",
    )


def churn_reference_labels(client, target, edits, timeout):
    previous_length = target["label_length"]
    for edit in range(1, edits + 1):
        label = label_for_edit(edit)
        changes = []
        for key in ("definition", "link"):
            start = target[key]
            changes.append(
                {
                    "range": {
                        "start": start,
                        "end": {
                            "line": start["line"],
                            "character": start["character"] + previous_length,
                        },
                    },
                    "text": label,
                }
            )
        client.notify(
            "textDocument/didChange",
            {
                "textDocument": {"uri": target["uri"], "version": edit + 1},
                "contentChanges": changes,
            },
        )
        response = client.request(
            "textDocument/definition",
            {
                "textDocument": {"uri": target["uri"]},
                "position": {
                    "line": target["link"]["line"],
                    "character": target["link"]["character"] + 1,
                },
            },
            timeout=timeout,
        )
        require_response(response, "textDocument/definition", require_result=True)
        previous_length = utf16_length(label)


def isolated_environment(directory):
    env = os.environ.copy()
    config_home = Path(directory) / "config"
    cache_home = Path(directory) / "cache"
    config_home.mkdir()
    cache_home.mkdir()
    env["XDG_CONFIG_HOME"] = str(config_home)
    env["XDG_CACHE_HOME"] = str(cache_home)
    env["NO_COLOR"] = "1"
    return env


def run_session(
    spec, run_number, project, files, edits, settle_timeout, quiet_seconds, stderr_dir
):
    key, command = spec
    print(f"==> memory: {key} (run {run_number})", flush=True)
    stderr_path = Path(stderr_dir) / f"{key}-run-{run_number}.stderr.log"
    client = None
    sampler = None
    started_at = time.monotonic()
    with tempfile.TemporaryDirectory(prefix=f"panache-memory-{key}-") as state_dir:
        try:
            client = Client(
                command,
                cwd=str(project),
                env=isolated_environment(state_dir),
                stderr_path=str(stderr_path),
            )
            sampler = Sampler(client.proc.pid)
            sampler.start()

            initialized = require_response(
                client.request(
                    "initialize",
                    {
                        "processId": os.getpid(),
                        "clientInfo": {"name": "panache-memory-bench", "version": "1"},
                        "rootUri": project.as_uri(),
                        "rootPath": str(project),
                        "capabilities": CAPABILITIES,
                        "workspaceFolders": [
                            {"uri": project.as_uri(), "name": project.name}
                        ],
                        "initializationOptions": {},
                        "trace": "off",
                    },
                    timeout=settle_timeout,
                ),
                "initialize",
                require_result=True,
            )
            init_seconds = round(time.monotonic() - started_at, 2)
            capabilities = (initialized.get("result") or {}).get("capabilities", {})
            pull = bool(capabilities.get("diagnosticProvider"))
            client.notify("initialized", {})

            wait_until_quiet(
                client,
                sampler,
                quiet_seconds,
                settle_timeout,
                "initialization",
            )
            milestones = {"baseline": sampler.milestone()}
            baseline_seconds = round(time.monotonic() - started_at, 2)
            print(f"    baseline {milestones['baseline']['rss_mb']} MB RSS", flush=True)

            documents, edit_target = prepare_documents(files)
            uris = [document["uri"] for document in documents]
            for document in documents:
                client.notify(
                    "textDocument/didOpen",
                    {
                        "textDocument": {
                            "uri": document["uri"],
                            "languageId": "markdown",
                            "version": 1,
                            "text": document["text"],
                        }
                    },
                )
                time.sleep(0.2)

            if pull:
                pull_diagnostics(client, uris, settle_timeout)
            exercise_shared_requests(client, uris)
            wait_until_quiet(
                client, sampler, quiet_seconds, settle_timeout, "files-opened settle"
            )
            milestones["settled"] = sampler.milestone()
            settled_seconds = round(time.monotonic() - started_at, 2)
            print(f"    settled  {milestones['settled']['rss_mb']} MB RSS", flush=True)

            edit_started_at = time.monotonic()
            churn_reference_labels(client, edit_target, edits, timeout=30)
            if pull:
                pull_diagnostics(client, uris, settle_timeout)
            wait_until_quiet(
                client, sampler, quiet_seconds, settle_timeout, "post-edit settle"
            )
            milestones["edited"] = sampler.milestone()
            edit_seconds = round(time.monotonic() - edit_started_at, 2)
            total_seconds = round(time.monotonic() - started_at, 2)

            sampler.stop_flag.set()
            sampler.join(timeout=2)
            milestones["peak"] = sampler.peak()
            result = {
                "run": run_number,
                "milestones": milestones,
                "init_seconds": init_seconds,
                "baseline_seconds": baseline_seconds,
                "settled_seconds": settled_seconds,
                "edit_seconds": edit_seconds,
                "total_seconds": total_seconds,
                "diagnostic_mode": "pull" if pull else "push",
                "diagnostics_published": client.count_published_diagnostics(),
                "diagnostic_requests": len(uris) * 2 if pull else 0,
                "definition_requests": edits,
                "samples": len(sampler.samples),
            }
            print(
                f"    edited   {milestones['edited']['rss_mb']} MB RSS"
                f"  (peak {milestones['peak']['rss_mb']} MB, {total_seconds}s)",
                flush=True,
            )
            client.shutdown()
            client = None
            return result
        finally:
            if sampler is not None and sampler.is_alive():
                sampler.stop_flag.set()
                sampler.join(timeout=2)
            if client is not None:
                client.kill()


# --- aggregation and output ------------------------------------------------


def rounded_median(values, digits):
    return round(statistics.median(values), digits)


def aggregate_runs(runs):
    milestones = {}
    for milestone in MILESTONES:
        milestones[milestone] = {
            "rss_mb": rounded_median(
                [run["milestones"][milestone]["rss_mb"] for run in runs], 1
            ),
            "pss_mb": rounded_median(
                [run["milestones"][milestone]["pss_mb"] for run in runs], 1
            ),
            "processes": int(
                statistics.median(
                    [run["milestones"][milestone]["processes"] for run in runs]
                )
            ),
        }
    timing_keys = (
        "init_seconds",
        "baseline_seconds",
        "settled_seconds",
        "edit_seconds",
        "total_seconds",
    )
    return {
        "milestones": milestones,
        "timings": {
            key: rounded_median([run[key] for run in runs], 2) for key in timing_keys
        },
        "samples": int(statistics.median([run["samples"] for run in runs])),
    }


def add_panache_ratios(servers):
    panache = next(server for server in servers if server["key"] == "panache")
    baseline = panache["aggregate"]["milestones"]
    for server in servers:
        current = server["aggregate"]["milestones"]
        server["aggregate"]["relative_to_panache"] = {
            f"{milestone}_rss": round(
                current[milestone]["rss_mb"] / baseline[milestone]["rss_mb"], 1
            )
            for milestone in MILESTONES
        }


def host_metadata():
    cpu = "unknown"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    memory_gb = 0
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                memory_gb = int(line.split()[1]) // 1024 // 1024
                break
    except OSError:
        pass
    return {
        "os": platform.system(),
        "arch": platform.machine(),
        "cpu": cpu,
        "memory_gb": memory_gb,
    }


def parse_key_values(values, option):
    parsed = {}
    for value in values:
        key, separator, payload = value.partition("=")
        if not key or not separator or not payload:
            raise ValueError(f"{option} expects NAME=VALUE, got {value!r}")
        parsed[key] = payload
    return parsed


def parse_servers(values):
    servers = []
    for value in values:
        key, separator, command = value.partition("=")
        if not key or not separator or not command:
            raise ValueError(f"--server expects NAME=COMMAND, got {value!r}")
        servers.append((key, shlex.split(command)))
    return servers


def display_command(command):
    display = [Path(command[0]).name]
    hide_next = False
    for argument in command[1:]:
        if hide_next:
            display.append("<isolated-gfm-config>")
            hide_next = False
        else:
            display.append(argument)
            hide_next = argument == "--config"
    return " ".join(display)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", required=True)
    parser.add_argument("--files", nargs="+", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--server", action="append", default=[], required=True)
    parser.add_argument("--server-version", action="append", default=[])
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--edits", type=int, default=1000)
    parser.add_argument("--settle-timeout", type=float, default=120)
    parser.add_argument("--quiet-seconds", type=float, default=5)
    parser.add_argument("--stderr-dir", required=True)
    parser.add_argument("--corpus-name", required=True)
    parser.add_argument("--corpus-repo", required=True)
    parser.add_argument("--corpus-revision", required=True)
    args = parser.parse_args()

    if platform.system() != "Linux" or not Path("/proc/self/smaps_rollup").is_file():
        parser.error("the memory benchmark requires Linux with /proc/smaps_rollup")
    if args.runs < 1 or args.edits < 1:
        parser.error("--runs and --edits must both be positive")

    project = Path(args.project).absolute()
    files = [Path(path).absolute() for path in args.files]
    stderr_dir = Path(args.stderr_dir)
    stderr_dir.mkdir(parents=True, exist_ok=True)
    specs = parse_servers(args.server)
    versions = parse_key_values(args.server_version, "--server-version")
    keys = [key for key, _ in specs]
    if set(keys) != {"panache", "marksman"}:
        parser.error("exactly panache and marksman server commands are required")

    runs_by_server = {key: [] for key in keys}
    session_index = 0
    for repetition in range(args.runs):
        order = specs if repetition % 2 == 0 else list(reversed(specs))
        for spec in order:
            session_index += 1
            key = spec[0]
            runs_by_server[key].append(
                run_session(
                    spec,
                    repetition + 1,
                    project,
                    files,
                    args.edits,
                    args.settle_timeout,
                    args.quiet_seconds,
                    stderr_dir,
                )
            )
            if session_index < args.runs * len(specs):
                time.sleep(2)

    servers = []
    for key, command in specs:
        label, doing = SERVER_META.get(key, (key, ""))
        runs = runs_by_server[key]
        servers.append(
            {
                "key": key,
                "label": label,
                "doing": doing,
                "version": versions.get(key, "unknown"),
                "command": display_command(command),
                "runs": runs,
                "aggregate": aggregate_runs(runs),
            }
        )
    add_panache_ratios(servers)

    documents, _ = prepare_documents(files)
    payload = {
        "schema_version": SCHEMA_VERSION,
        "meta": {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "host": host_metadata(),
            "runs": args.runs,
            "quiet_seconds": args.quiet_seconds,
            "settle_timeout_seconds": args.settle_timeout,
            "edit_count": args.edits,
        },
        "corpus": {
            "name": args.corpus_name,
            "repo": args.corpus_repo,
            "revision": args.corpus_revision,
        },
        "session": {
            "project": project.name,
            "files": [str(path.relative_to(project)) for path in files],
            "file_count": len(files),
            "disk_bytes": sum(path.stat().st_size for path in files),
            "opened_bytes": sum(
                len(document["text"].encode()) for document in documents
            ),
            "edited_file": str(files[0].relative_to(project)),
        },
        "servers": servers,
    }
    output = Path(args.out)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"==> wrote {output}")


if __name__ == "__main__":
    main()
