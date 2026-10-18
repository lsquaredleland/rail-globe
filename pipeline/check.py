"""Phase 2 'Done when' check: run the hand-made answer list against a region's output.
Usage: python3 check.py [--data DIR] [slug ...]   (default: the three pilots in data/). Exit code 1 if anything fails.
Without --data a slug is read from data/world/<slug> when that folder exists (a world slug), else from data/<slug> (the pilots);
the legacy top-level data/{br,ca,ch,co,de,it,mx,us,ve} are never read that way. With --data DIR every slug is read from DIR/<slug>.
Answer entries: "forced": true marks an entry that only restates a config override (main_terminals, main_station_re,
type_overrides), so it cannot catch a regression of the generic rules; the summary counts forced and independent
entries apart. "_known_failure": "reason" marks a real, understood bug: printed KNOWN (not a failure); if it starts
to pass it prints NOW PASSING (not a failure either; remove the marker).
       python3 check.py --exemptions [--data DIR] [slug ...]   only the stale-exemption check (default: every region in DIR with a
       classify_report.json): fails when an exemption in pipeline/config matched fewer features than its expect_min, or when a
       config exemption id is missing from the report (stale build). A region with no report prints NO REPORT and fails
       only if its config lists exemptions.
       See docs/12-config-and-exemptions.md."""
import json, re, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"


def data_for(slug, data=None):
    """The folder that holds <slug>: DIR when --data was given, else data/world for a world slug, else data/ (pilots)."""
    if data:
        return data
    return DATA / "world" if (DATA / "world" / slug).is_dir() else DATA


def load(slug, data=None):
    DATA_ = data_for(slug, data)
    st = [json.loads(l)["properties"] for l in open(DATA_ / slug / "stations.geojsonl")]
    cx = {json.loads(l)["properties"]["id"]: json.loads(l)["properties"] for l in open(DATA_ / slug / "complexes.geojsonl")}
    lines = json.load(open(DATA_ / slug / "lines.json"))
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


def config_exemption_ids(slug):
    """Exemption ids in pipeline/config/<slug>.json and its extends chain (exemptions and +exemptions)."""
    ids, name, seen = [], slug, set()
    while name and name not in seen:
        seen.add(name)
        p = HERE / "config" / f"{name}.json"
        if not p.exists():
            break
        cfg = json.load(open(p))
        ids += [e["id"] for k in ("exemptions", "+exemptions") for e in cfg.get(k, [])]
        name = cfg.get("extends")
    return ids


def check_exemptions(slug, data=None):
    """Read <data>/<slug>/classify_report.json; returns the number of failing exemptions (stale, or missing from the report)."""
    path = data_for(slug, data) / slug / "classify_report.json"
    want = config_exemption_ids(slug)
    if not path.exists():
        if want:
            print(f"  FAIL exempt  NO REPORT: {slug} config lists {len(want)} exemption(s) ({', '.join(want)}) but {path} does not exist")
            return len(want)
        print(f"  NO REPORT {slug}: no classify_report.json and no exemptions in its config (nothing to check)")
        return 0
    rep = json.load(open(path)).get("exemptions") or {}
    stale = 0
    for ex_id, e in rep.items():
        ok = e["expect_min_met"]
        stale += not ok
        print(f'  {"PASS" if ok else "FAIL"} exempt  {ex_id:40} matched {e["matched"]} ({e["tracks"]} track, {e["lines"]} line, {e["km"]} km), expect_min {e["expect_min"]}')
    for ex_id in want:
        if ex_id not in rep:
            stale += 1
            print(f"  FAIL exempt  {ex_id:40} MISSING from {path} (region {slug}): config lists it, report does not; rebuild with `rail-extract cut {slug}`")
    return stale


def pop_data(args):
    data = None
    if "--data" in args:
        i = args.index("--data")
        data = Path(args[i + 1])
        del args[i:i + 2]
    return data


def main_exemptions(args):
    data = pop_data(args)
    slugs = args or sorted(p.parent.name for p in (data or DATA / "world").glob("*/classify_report.json"))
    if not args and not data:
        slugs += [p for p in ("sf-bay-area", "chengdu", "paris") if (DATA / p / "classify_report.json").exists()]
    failed = 0
    for slug in slugs:
        print(f"== {slug} exemptions")
        failed += check_exemptions(slug, data)
    print(f"\n{'ALL PASS' if not failed else str(failed) + ' STALE/MISSING'}")
    return 1 if failed else 0


def main(args):
    data = pop_data(args)
    slugs = args or ["sf-bay-area", "chengdu", "paris"]
    failed = known = now = 0
    tot = {"forced": 0, "independent": 0}
    for slug in slugs:
        ans = json.load(open(HERE / "answers" / f"{slug}.json"))
        st, cx, lines = load(slug, data)
        print(f"== {slug}")
        n = {"forced": 0, "independent": 0}
        for kind, fn, args_ in (("station", check_station, (st, cx)), ("line", check_line, (lines,))):
            for rule in ans[kind + "s"]:
                ok, msg = fn(rule, *args_)
                kf = rule.get("_known_failure")
                cls = "forced" if rule.get("forced") else "independent"
                n[cls] += 1
                if ok and kf:
                    tag, now = "NOW PASSING", now + 1
                    msg += f" | remove _known_failure: {kf}"
                elif ok:
                    tag = "PASS"
                elif kf:
                    tag, known = "KNOWN", known + 1
                    msg += f" | known: {kf}"
                else:
                    tag, failed = "FAIL", failed + 1
                print(f'  {tag:11} {kind:7} {cls[0].upper()} {rule["match"]:40} {msg}')
        failed += check_exemptions(slug, data)
        tot["forced"] += n["forced"]
        tot["independent"] += n["independent"]
        print(f'  ({n["independent"]} independent, {n["forced"]} forced entries)')
    print(f"\n{tot['independent']} independent + {tot['forced']} forced entries; {known} KNOWN failures, {now} NOW PASSING")
    print(f"{'ALL PASS' if not failed else str(failed) + ' FAILED'}")
    return 1 if failed else 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--exemptions"]:
        sys.exit(main_exemptions(sys.argv[2:]))
    sys.exit(main(sys.argv[1:]))
