"""Workspace resolution for the Ventosa engine (stdlib only).

The engine is product code; everything user-specific lives in the workspace:
`workspace.json` (brand, unit, platform, sponsor, ports, agent command) and the
data folders (labelling/, recording/, pipeline/, scripts/, assets/, <subjects>/).

Root: $VENTOSA_WORKSPACE, else the nearest folder with workspace.json walking up
from the current directory, else the current directory.
"""
import json, os, pathlib

ENGINE = pathlib.Path(__file__).resolve().parent
WEB = ENGINE.parent / "apps" / "web"


def _root():
    env = os.environ.get("VENTOSA_WORKSPACE")
    if env:
        return pathlib.Path(env).expanduser().resolve()
    here = pathlib.Path.cwd().resolve()
    for p in (here, *here.parents):
        if (p / "workspace.json").exists():
            return p
    return here


ROOT = _root()

DEFAULTS = {
    "name": ROOT.name,
    "brand": {"name": ROOT.name, "tagline": "", "accent": "#e5243b"},
    "unit": "video",                 # video | episode | post …  (UI wording)
    "id_prefix": "V",                # subject ids: V01, V02 … / E01 …
    "subjects_dir": "videos",        # one markdown file per subject, YAML frontmatter
    "platform": {"name": "YouTube", "title_max": 100, "min_chapters": 3},
    "sponsor": {"line": "", "lower_third": ""},   # empty line = no SP beat required
    "media": {"width": 1920, "height": 1080, "fps": 30, "audio_only": False,
              "edit_ext": [".mp4", ".webm", ".mov"]},
    "ports": {"labelling": 8766, "teleprompter": 8765},
    "rules": {"no_music": False, "max_animations": 7},
    "agent_command": "ventosa-graph attach {step} {vid}",
    "agent_env": "VENTOSA_AGENT",
}


def _merge(a, b):
    out = dict(a)
    for k, v in b.items():
        out[k] = _merge(a[k], v) if isinstance(v, dict) and isinstance(a.get(k), dict) else v
    return out


def load():
    f = ROOT / "workspace.json"
    try:
        return _merge(DEFAULTS, json.loads(f.read_text()))
    except FileNotFoundError:
        return dict(DEFAULTS)


WS = load()
SUBJECTS = ROOT / WS["subjects_dir"]
ID_RE = WS["id_prefix"] + r"\d+"


def agent():
    """Agent name: $VENTOSA_AGENT (or the workspace's agent_env, or legacy MVJ_AGENT), default 'main'."""
    for k in (WS.get("agent_env") or "VENTOSA_AGENT", "VENTOSA_AGENT", "MVJ_AGENT"):
        if os.environ.get(k):
            return os.environ[k]
    return "main"


def brand_payload():
    return {"name": WS["brand"]["name"], "tagline": WS["brand"].get("tagline", ""), "accent": WS["brand"]["accent"],
            "unit": WS["unit"], "platform": WS["platform"]["name"], "agent_command": WS["agent_command"],
            "workspace": WS["name"], "audio_only": bool(WS["media"].get("audio_only"))}
