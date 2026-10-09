import http.server, os, sys, threading
HERE = os.path.dirname(os.path.abspath(__file__))
CONF = os.path.expanduser("~/.config/bramley")
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8791
dev = open(os.path.join(CONF, "developer-token")).read().strip()
page = open(os.path.join(HERE, "index.html")).read().replace("__DEV_TOKEN__", dev).encode()
done = threading.Event()

class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (self.command, self.path.split("?")[0]))
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(page)
    def do_POST(self):
        if not self.path.rstrip("/").endswith("token"):
            self.send_response(404); self.end_headers(); return
        n = int(self.headers.get("Content-Length") or 0)
        tok = self.rfile.read(min(n, 8192)).decode().strip()
        if not tok or len(tok) < 50:
            self.send_response(400); self.end_headers(); return
        fd = os.open(os.path.join(CONF, "music-user-token"), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(tok + "\n")
        self.send_response(204); self.end_headers()
        sys.stderr.write("music-user-token saved (%d chars)\n" % len(tok))
        done.set()

srv = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
sys.stderr.write("listening on 127.0.0.1:%d\n" % PORT)
done.wait(timeout=3600)
srv.shutdown()
