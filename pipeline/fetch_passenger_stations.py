"""China's passenger stations: every station that sells tickets on 12306 (about 3,400 names).
Usage: python3 fetch_passenger_stations.py     -> config/cn-passenger-stations.txt (one name per line, "站" and spaces dropped)
Used by classify/ (config key "passenger_stations_file"): an OSM station counts as a passenger station in China only when its name is in
this list. A bare railway=station tag is also used for freight yards and siding stops there, so tags alone cannot tell them apart.
12306 is the national ticketing service; the list is a public file of the ticketing site. Re-run it when the timetable changes."""
import re
import urllib.request
from pathlib import Path

URL = "https://kyfw.12306.cn/otn/resources/js/framework/station_name.js"
req = urllib.request.Request(URL, headers={"User-Agent": "Mozilla/5.0 (rail-globe pipeline)"})
text = urllib.request.urlopen(req, timeout=60).read().decode("utf-8")
names = sorted({n.replace("站", "").strip() for n in re.findall(r"@[a-z]+\|([^|]+)\|[A-Z]+\|", text)})
out = Path(__file__).resolve().parent / "config" / "cn-passenger-stations.txt"
out.write_text("\n".join(names) + "\n", encoding="utf-8")
print(len(names), "stations ->", out)
