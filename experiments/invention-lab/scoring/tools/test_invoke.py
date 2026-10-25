"""invoke.py against a fake organization server, with lab-exec mocked: a run
starts at most once, only after the server agrees, runs the server's frozen
plan, and never runs anything when the start is refused.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
Nothing is executed: every lab subprocess is mocked.
"""
import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

MODULE = importlib.util.spec_from_file_location("invoke_under_test",
                                                Path(__file__).with_name("invoke.py"))
invoke = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(invoke)


class Org(http.server.BaseHTTPRequestHandler):
    """Records every request; answers from `responses` by path suffix."""
    requests, responses = [], {}

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])) or b"null")
        Org.requests.append((self.path, self.headers["authorization"], body))
        status, reply = next(v for k, v in Org.responses.items() if self.path.endswith(k))
        data = json.dumps(reply).encode()
        self.send_response(status)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


class InvokeTests(unittest.TestCase):
    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Org)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.url = f"http://127.0.0.1:{self.server.server_port}/api/v1"
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.package = self.root / "pkg.json"
        self.package.write_bytes(b'{"package":{"id":"m","version":"0.1.0"}}')
        digest = hashlib.sha256(self.package.read_bytes()).hexdigest()
        self.frozen = {"id": "invoke", "slots": [{"slot_id": "use", "method": {"sha256": digest}}]}
        Org.requests = []
        Org.responses = {
            "/method-runs": (201, {"id": "mrun_1", "status": "prepared", "plan": self.frozen}),
            "/start": (200, {"id": "mrun_1", "status": "started", "plan": self.frozen}),
            "/receipt": (200, {"id": "mrun_1", "status": "completed",
                               "receipt": {"outcome": "output_generated"}}),
        }
        (self.root / "req.json").write_text("{}")
        (self.root / "plan.json").write_text(json.dumps({"id": "proposal"}))
        os.environ[invoke.TOKEN] = "operator-token"
        self.addCleanup(os.environ.pop, invoke.TOKEN, None)
        self.dir = self.root / "run"

    def prepare(self):
        invoke.main(["prepare", "--org", self.url, "--key", "k1", "--request",
                     str(self.root / "req.json"), "--plan", str(self.root / "plan.json"),
                     "--out", str(self.dir)])

    def run_args(self):
        return ["run", str(self.dir), "--package", str(self.package), "--lab-exec", "lab-exec",
                "--sandbox", "none"]

    def fake_lab_exec(self, receipt=True):
        calls = []

        def run(args, stdout, stderr):
            calls.append(args)
            if receipt:
                os.makedirs(args[5])
                Path(args[5], "receipt.json").write_text(json.dumps({"outcome": "output_generated"}))
            return type("Done", (), {"returncode": 0})()
        return calls, run

    def test_prepare_start_run_and_receipt_in_order(self):
        self.prepare()
        calls, run = self.fake_lab_exec()
        with patch.object(invoke.subprocess, "run", run):
            invoke.main(self.run_args())
        paths = [p for p, _, _ in Org.requests]
        self.assertEqual(paths, ["/api/v1/method-runs", "/api/v1/method-runs/mrun_1/start",
                                 "/api/v1/method-runs/mrun_1/receipt"])
        self.assertTrue(all(a == "Bearer operator-token" for _, a, _ in Org.requests))
        # lab-exec ran once, on the plan the server froze.
        self.assertEqual(len(calls), 1)
        self.assertEqual(json.loads(Path(calls[0][2]).read_text()), self.frozen)
        self.assertEqual(calls[0][3], "use")
        self.assertEqual(Org.requests[2][2], {"outcome": "output_generated"})
        # Never started twice.
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(self.run_args())
        self.assertEqual(len(calls), 1)
        self.assertEqual(len(Org.requests), 3)

    def test_a_refused_start_runs_nothing(self):
        self.prepare()
        Org.responses["/start"] = (409, {"error": "method_run_status"})
        calls, run = self.fake_lab_exec()
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(self.run_args())
        self.assertEqual(calls, [])
        self.assertTrue((self.dir / "start-refused.json").exists())
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(self.run_args())
        self.assertEqual(len(Org.requests), 2, "no second start after a refusal")

    def test_another_package_is_refused_before_any_start(self):
        self.prepare()
        self.package.write_bytes(b"{}")
        calls, run = self.fake_lab_exec()
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(self.run_args())
        self.assertEqual((calls, len(Org.requests)), ([], 1))

    def test_no_receipt_means_nothing_is_posted(self):
        self.prepare()
        calls, run = self.fake_lab_exec(receipt=False)
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(self.run_args())
        self.assertEqual(len(calls), 1)
        self.assertEqual(len(Org.requests), 2)

    def test_prepare_refusals_and_missing_token(self):
        Org.responses["/method-runs"] = (422, {"error": "method_run_binding"})
        with self.assertRaises(SystemExit):
            self.prepare()
        self.assertTrue((self.dir / "prepare-refused.json").exists())
        self.assertFalse((self.dir / "run.json").exists())
        os.environ.pop(invoke.TOKEN)
        with self.assertRaises(SystemExit):
            invoke.main(["prepare", "--org", self.url, "--key", "k2", "--request",
                         str(self.root / "req.json"), "--plan", str(self.root / "plan.json"),
                         "--out", str(self.root / "other")])
        self.assertEqual(len(Org.requests), 1)

    def scored_run(self):
        self.prepare()
        calls, run = self.fake_lab_exec()
        with patch.object(invoke.subprocess, "run", run):
            invoke.main(self.run_args())
        Org.responses["/result"] = (200, {"id": "mrun_1", "status": "completed",
                                          "result": {"attachment": {"kind": "attached"}}})

    def fake_lab_score(self, evaluation=True):
        calls = []

        def run(args, stdout, stderr):
            calls.append(args)
            if evaluation:
                os.makedirs(args[5])
                Path(args[5], "evaluation.json").write_text(json.dumps({"evaluation": 1}))
            return type("Done", (), {"returncode": 0})()
        return calls, run

    def test_score_runs_lab_score_once_and_posts_the_evaluation(self):
        self.scored_run()
        calls, run = self.fake_lab_score()
        with patch.object(invoke.subprocess, "run", run):
            invoke.main(["score", str(self.dir), "--lab-score", "lab-score"])
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1:4], ["score", str(self.dir / "plan.json"), "use"])
        self.assertEqual(Org.requests[-1][0], "/api/v1/method-runs/mrun_1/result")
        self.assertEqual(Org.requests[-1][2], {"evaluation": 1})
        self.assertTrue((self.dir / "result.json").exists())
        # Never scored twice.
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(["score", str(self.dir), "--lab-score", "lab-score"])
        self.assertEqual(len(calls), 1)

    def test_score_needs_an_accepted_receipt_and_an_evaluation(self):
        self.prepare()
        calls, run = self.fake_lab_score()
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(["score", str(self.dir), "--lab-score", "lab-score"])
        self.assertEqual(calls, [])
        self.scored_run_after_prepare()
        calls, run = self.fake_lab_score(evaluation=False)
        before = len(Org.requests)
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(["score", str(self.dir), "--lab-score", "lab-score"])
        self.assertEqual(len(Org.requests), before, "nothing posted without an evaluation")

    def scored_run_after_prepare(self):
        calls, run = self.fake_lab_exec()
        with patch.object(invoke.subprocess, "run", run):
            invoke.main(self.run_args())
        Org.responses["/result"] = (200, {"result": {"attachment": {"kind": "attached"}}})

    def test_a_refused_result_is_kept(self):
        self.scored_run()
        Org.responses["/result"] = (422, {"error": "method_run_binding"})
        calls, run = self.fake_lab_score()
        with patch.object(invoke.subprocess, "run", run), self.assertRaises(SystemExit):
            invoke.main(["score", str(self.dir), "--lab-score", "lab-score"])
        self.assertTrue((self.dir / "result-refused.json").exists())


if __name__ == "__main__":
    unittest.main()
