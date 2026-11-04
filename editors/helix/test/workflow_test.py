#!/usr/bin/env python3
"""Check the workflow proof's LSP response matching and unsaved text replay."""
import unittest

from workflow import document_text, response_pairs


def event(direction, message, pid=7):
    return {"direction": direction, "server_pid": pid, "message": message}


class WorkflowEvidenceTest(unittest.TestCase):
    def test_response_requires_same_request_document_method_and_server(self):
        def request(identifier, method="textDocument/hover", uri="file:///main.rill"):
            return event("client", {"id": identifier, "method": method,
                                    "params": {"textDocument": {"uri": uri}}})

        trace = [request(0), event("server", {"id": 0, "result": "old"}),
                 request(1, uri="file:///other.rill"),
                 event("server", {"id": 1, "result": "wrong document"}),
                 request(2, method="textDocument/completion"),
                 event("server", {"id": 2, "result": "wrong method"}),
                 request(3), event("server", {"id": 3, "result": "wrong server"}, pid=9),
                 event("server", {"id": 3, "result": "actual popup"})]
        pairs = response_pairs(trace, "textDocument/hover", "file:///main.rill", after=2)
        self.assertEqual([(request["id"], reply["result"]) for request, reply in pairs],
                         [(3, "actual popup")])

    def test_replay_applies_sequential_utf16_edits_and_ignores_other_documents(self):
        uri = "file:///main.rill"
        trace = [event("client", {"method": "textDocument/didOpen", "params": {
            "textDocument": {"uri": uri, "text": "// 😀\nfn main(){0}\n"}}}),
            event("client", {"method": "textDocument/didChange", "params": {
                "textDocument": {"uri": uri}, "contentChanges": [
                    {"range": {"start": {"line": 0, "character": 3},
                               "end": {"line": 0, "character": 5}}, "text": "π"},
                    {"range": {"start": {"line": 1, "character": 10},
                               "end": {"line": 1, "character": 11}}, "text": "42"}]}}),
            event("client", {"method": "textDocument/didChange", "params": {
                "textDocument": {"uri": "file:///other.rill"},
                "contentChanges": [{"text": "unrelated"}]}})]
        self.assertEqual(document_text(trace, uri), "// π\nfn main(){42}\n")
        trace.append(event("client", {"method": "textDocument/didChange", "params": {
            "textDocument": {"uri": uri}, "contentChanges": [{"text": "replacement\n"}]}}))
        self.assertEqual(document_text(trace, uri), "replacement\n")


if __name__ == "__main__":
    unittest.main()
