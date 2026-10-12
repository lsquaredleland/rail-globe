"""Phase 2 'Done when' check: run the hand-made answer list against a region's output.
Usage: python3 check.py [slug ...]   (default: all three). Exit code 1 if anything fails."""
import json, re, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"


def load(slug):
    st = [json.loads(l)["properties"] for l in open(DATA / slug / "stations.geojsonl")]
    cx = {json.loads(l)["properties"]["id"]: json.loads(l)["properties"] for l in open(DATA / slug / "complexes.geojsonl")}
    lines = json.load(open(DATA / slug / "lines.json"))
    return st, cx, lines


def check_station(rule, st, cx):
    cands = [s for s in st if any(re.search(rule["match"], n or "") for n in (s["name"], s["name_en"]))
             and (not rule.get("mode") or rule["mode"] in s["modes"])]
    if not cands:
        return False, "not found"
    s = max(cands, key=lambda s: s["score"])
    errs = []
    if "tier_max" in rule and s["tier"] > rule["tier_max"]:
        errs.append(f'tier {s["tier"]} (want <= {rule["tier_max"]})')
    if "tier_min" in rule and s["tier"] < rule["tier_min"]:
        errs.append(f'tier {s["tier"]} (want >= {rule["tier_min"]})')
    if "terminus" in rule and s["terminus"] != rule["terminus"]:
        errs.append(f'terminus={s["terminus"]} (want {rule["terminus"]})')
    if "complex_modes" in rule:
        have = set(cx[s["complex"]]["modes"])
        if not set(rule["complex_modes"]) <= have:
            errs.append(f'complex modes {sorted(have)} (want {rule["complex_modes"]})')
    return not errs, f'{s["name"]} T{s["tier"]} score={s["score"]}' + (" | " + "; ".join(errs) if errs else "")


def check_line(rule, lines):
    c = [l for l in lines if re.search(rule["match"], l["name"] or "") and (not rule.get("network") or l["network"] == rule["network"])]
    if not c:
        return False, "not found"
    bad = [l for l in c if l["type"] != rule["type"]]
    return not bad, f'{c[0]["name"]} -> {c[0]["type"]}' + (f' | {len(bad)} of {len(c)} not {rule["type"]}' if bad else "")


def main(slugs):
    failed = 0
    for slug in slugs:
        ans = json.load(open(HERE / "answers" / f"{slug}.json"))
        st, cx, lines = load(slug)
        print(f"== {slug}")
        for kind, fn, args in (("station", check_station, (st, cx)), ("line", check_line, (lines,))):
            for rule in ans[kind + "s"]:
                ok, msg = fn(rule, *args)
                failed += not ok
                print(f'  {"PASS" if ok else "FAIL"} {kind:7} {rule["match"]:40} {msg}')
    print(f"\n{'ALL PASS' if not failed else str(failed) + ' FAILED'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or ["sf-bay-area", "chengdu", "paris"]))
