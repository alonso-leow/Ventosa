import json, pathlib
import sys
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ws import ROOT  # noqa: E402
H = ROOT / "recording"
d = json.loads((H / "clips.json").read_text())
md = ["# Clip index (state edited by the teleprompter)\n"]; tot = {}
for vid, v in d["videos"].items():
    md.append(f"\n## {vid} · {v['title']}  \n[script](../{v['file']})\n")
    md.append("| clip | t | beat source | status | take | file |\n|---|---|---|---|---|---|")
    for l in v["lines"]:
        kept = next((t["file"] for t in reversed(l["takes"]) if t["status"] == "kept"), "")
        md.append(f"| {l['id']} | {l['t']} | {l['source'][:28]} | {l['status']} | {l['take']} | {kept} |")
        tot[l["status"]] = tot.get(l["status"], 0) + 1
md.insert(1, "Totals: " + ", ".join(f"{k} {n}" for k, n in sorted(tot.items())) + "\n")
(H / "index.md").write_text("\n".join(md) + "\n")
