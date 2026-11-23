"""HTTP Basic auth for every Ventosa web app (stdlib only).

Reads the same identity file as the desktop app ($VENTOSA_IDENTITY, else
$XDG_CONFIG_HOME/ventosa/identity.json): roles + users, passwords stored as
pbkdf2$<iterations>$<salt hex>$<hash hex> (PBKDF2-HMAC-SHA256).

No file or no users = open (single-user mode, localhost only by default).
A user passes when enabled, not expired, and the role has graph control or
lists the app id (or "*"). Every refusal and first success per user is
appended to $XDG_DATA_HOME/ventosa/audit.jsonl.

Bind address: $VENTOSA_BIND (default 127.0.0.1; 0.0.0.0 for a local network,
only with users set up).
"""
import base64, hashlib, hmac, json, os, pathlib, time

import threading
LOCAL = threading.local()  # the user of the request this thread serves (events carry it)
_cache = {"mtime": None, "data": {"roles": [], "users": []}}
_seen = set()


def _xdg(var, default):
    return pathlib.Path(os.environ.get(var) or (pathlib.Path.home() / default))


def identity_path():
    env = os.environ.get("VENTOSA_IDENTITY")
    return pathlib.Path(env) if env else _xdg("XDG_CONFIG_HOME", ".config") / "ventosa" / "identity.json"


def audit_path():
    return _xdg("XDG_DATA_HOME", ".local/share") / "ventosa" / "audit.jsonl"


def load():
    p = identity_path()
    try:
        m = p.stat().st_mtime
    except OSError:
        return {"roles": [], "users": []}
    if m != _cache["mtime"]:
        try:
            _cache["data"] = json.loads(p.read_text())
            _cache["mtime"] = m
        except (OSError, ValueError):
            pass
    return _cache["data"]


_ok = {}


def verify(password, stored):
    """PBKDF2 check; a success is remembered (by a hash of the password) so polling pages don't pay it per request."""
    memo = (hashlib.sha256(password.encode()).hexdigest(), stored)
    if _ok.get(memo):
        return True
    try:
        kind, iters, salt, h = stored.split("$")
        if kind != "pbkdf2":
            return False
        out = hashlib.pbkdf2_hmac("sha256", password.encode(), bytes.fromhex(salt), int(iters), len(bytes.fromhex(h)))
        good = hmac.compare_digest(out, bytes.fromhex(h))
        if good:
            _ok[memo] = True
        return good
    except (ValueError, TypeError):
        return False


def audit(actor, action, target, ok):
    p = audit_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "a") as f:
        f.write(json.dumps({"at": int(time.time()), "actor": actor, "action": action, "target": target, "ok": ok, "via": "web"}) + "\n")


def role_of(data, user):
    roles = data.get("roles") or []
    return next((r for r in roles if r.get("id") == user.get("role")), {})


def allowed(role, app_id):
    if app_id == "portal" or role.get("graph"):
        return True
    apps = role.get("apps") or []
    return "*" in apps or app_id in apps


def check(header, app_id):
    """(ok, user name or None, reason). Open mode when no users exist."""
    data = load()
    users = data.get("users") or []
    if not users:
        return True, None, "open"
    if not header or not header.startswith("Basic "):
        return False, None, "login required"
    try:
        name, _, pw = base64.b64decode(header[6:]).decode().partition(":")
    except (ValueError, UnicodeDecodeError):
        return False, None, "bad header"
    u = next((x for x in users if x.get("name") == name), None)
    if not u or not verify(pw, u.get("pass", "")):
        audit(name, "login", app_id, False)
        return False, name, "unknown user or wrong password"
    if not u.get("enabled", True):
        return False, name, "account disabled"
    if u.get("expires") and u["expires"] < time.time():
        return False, name, "account expired"
    if not allowed(role_of(data, u), app_id):
        audit(name, "open", app_id, False)
        return False, name, f"no access to {app_id}"
    if (name, app_id) not in _seen:
        _seen.add((name, app_id))
        audit(name, "open", app_id, True)
    return True, name, "ok"


def apps_for(name):
    """App ids a user may open ("*" when the role has graph control or all apps)."""
    data = load()
    u = next((x for x in data.get("users") or [] if x.get("name") == name), None)
    if not u:
        return ["*"] if not data.get("users") else []
    r = role_of(data, u)
    return ["*"] if r.get("graph") or "*" in (r.get("apps") or []) else list(r.get("apps") or [])


def guard(handler, app_id):
    """Call from parse_request: False (and a 401/403 sent) when the request may not pass."""
    ok, name, why = check(handler.headers.get("Authorization"), app_id)
    if ok:
        handler.auth_user = name
        LOCAL.user = name
        return True
    body = json.dumps({"error": why}).encode()
    code = 403 if why.startswith("no access") or why.startswith("account") else 401
    handler.send_response(code)
    if code == 401:
        handler.send_header("WWW-Authenticate", 'Basic realm="ventosa", charset="UTF-8"')
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(body)))
    handler.end_headers()
    handler.wfile.write(body)
    return False


def bind():
    return os.environ.get("VENTOSA_BIND", "127.0.0.1")
