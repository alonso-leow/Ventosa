#!/usr/bin/env python3
"""Ventosa dashboard app: http://127.0.0.1:<ports.dashboard>/dashboard.html
GET /api/dashboard?days=N   uploads per platform, tokens per video and step, phase KPIs (dashboard.py)
"""
import os, pathlib, sys, urllib.parse
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import common  # noqa: E402
import dashboard  # noqa: E402
from ws import WEB, WS  # noqa: E402


class H(common.Handler):
    app_id = "dashboard"
    app_dir = WEB

    def do_GET(s):
        u = urllib.parse.urlparse(s.path)
        if u.path == "/api/dashboard":
            q = urllib.parse.parse_qs(u.query)
            try:
                days = int(q.get("days", ["30"])[0])
            except ValueError:
                days = 30
            return s.send(dashboard.build(days))
        return common.Handler.do_GET(s)


if __name__ == "__main__":
    port = int(os.environ.get("VENTOSA_PORT") or WS["ports"].get("dashboard", 8768))
    common.serve(H, port, "dashboard.html")
