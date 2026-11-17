#!/usr/bin/env python3
"""A local stand-in for the Messages API, to test the Writing Studio coach
without a key or network: ANTHROPIC_API_KEY=x ANTHROPIC_BASE_URL=http://127.0.0.1:18765.
Writes the last request to /tmp/quill-e2e/last_request.json."""
import json, http.server, os

class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        user = body["messages"][0]["content"]
        os.makedirs("/tmp/quill-e2e", exist_ok=True)
        with open("/tmp/quill-e2e/last_request.json", "w") as f:
            json.dump({"headers": dict(self.headers), "body": body}, f, indent=1)
        if "JSON array" in user:
            text = json.dumps([{"rule": "grammar.agreement", "instruction": "Fix the verb.", "sentence": "Technology help students learn.",
                                "answer": "Technology helps students learn.", "explanation": "Uncountable subjects take a singular verb."}])
        else:
            text = "## Hint 1\n- **“technology help”** — what verb form follows a singular subject?\n\n## Hint 2\nWhat evidence supports your second paragraph?"
        out = json.dumps({"content": [{"type": "text", "text": text}], "stop_reason": "end_turn"}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *a):
        pass

http.server.HTTPServer(("127.0.0.1", 18765), H).serve_forever()
