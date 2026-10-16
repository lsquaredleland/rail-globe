import sys, re, json, collections
from urllib.parse import unquote
ops = collections.defaultdict(lambda: {"st": 0, "ln": 0, "other": 0, "names": collections.Counter()})
for line in open(sys.argv[1], errors="replace"):
    if "wikidata=" not in line: continue
    parts = line.rstrip("\n").split(" ")
    t = next((p[1:] for p in parts if p.startswith("T")), None)
    if not t: continue
    tags = {}
    for kv in t.split(","):
        k, _, v = kv.partition("=")
        tags[unquote(k)] = unquote(v)
    q = tags.get("operator:wikidata") or tags.get("brand:wikidata")
    if not q: continue
    kind = "st" if (tags.get("railway") in ("station", "halt", "tram_stop") or tags.get("public_transport") in ("station", "stop_position") ) else "ln" if (tags.get("type") in ("route", "route_master") or "railway" in tags) else "other"
    nm = tags.get("operator") or tags.get("brand") or ""
    for qid in re.split(r"\s*;\s*", q):
        if re.fullmatch(r"Q\d+", qid):
            o = ops[qid]; o[kind] += 1; o["names"][nm] += 1
out = {q: {"st": o["st"], "ln": o["ln"], "other": o["other"], "name": o["names"].most_common(1)[0][0]} for q, o in ops.items()}
json.dump(out, open(sys.argv[2], "w"), ensure_ascii=False)
print(len(out), "operators;", sum(1 for o in out.values() if o["st"]), "on stations;", sum(1 for o in out.values() if o["ln"]), "on lines/routes")
for q, o in sorted(out.items(), key=lambda x: -(x[1]["st"] + x[1]["ln"]))[:12]: print(q, o)
