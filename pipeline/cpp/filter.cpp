// Phase 1 filter in C++ (libosmium). Same rules as filter.py (docs/03-data-spec.md); filter.py runs this after a prefilter.
// Usage: rail_filter <input.osm.pbf> <outdir>
// Writes <outdir>/features.geojsonl, stop_positions.jsonl, relations_all.jsonl, filter_meta.json
#include <cstdio>
#include <cstring>
#include <map>
#include <set>
#include <string>
#include <vector>

#include <osmium/area/assembler.hpp>
#include <osmium/area/multipolygon_manager.hpp>
#include <osmium/handler/node_locations_for_ways.hpp>
#include <osmium/index/map/flex_mem.hpp>
#include <osmium/io/any_input.hpp>
#include <osmium/relations/manager_util.hpp>
#include <osmium/visitor.hpp>

using std::string;

static const std::set<string> TRACK = {"rail", "subway", "light_rail", "tram", "monorail", "funicular", "narrow_gauge", "construction", "proposed"};
static const std::set<string> SERVICE_KEEP = {"yard", "siding", "spur", "crossover"};
static const char* MODE_FLAGS[] = {"train", "subway", "light_rail", "tram", "monorail"};
static const char* KEEP[] = {
    "railway", "public_transport", "building", "landuse", "station", "service", "usage", "railway:traffic_mode",
    "highspeed", "name", "official_name", "int_name", "ref", "wikidata", "uic_ref", "railway:ref",
    "operator", "operator:short", "network", "gauge", "electrified", "voltage", "frequency", "tracks",
    "maxspeed", "maxspeed:freight", "construction", "proposed", "bridge", "tunnel", "light_rail", "subway", "train", "start_date",
    "construction:railway", "construction:public_transport", "construction:station"};
static const char* REL_TAGS[] = {"name", "ref", "colour", "operator", "network", "route", "service", "from", "to", "wikidata",
                                 "public_transport", "type", "route_master"};
static const std::set<string> ROUTE_MODES = {"train", "subway", "light_rail", "tram", "monorail", "funicular"};

static string jstr(const char* s) {
    string o = "\"";
    for (const unsigned char* p = reinterpret_cast<const unsigned char*>(s); *p; ++p) {
        switch (*p) {
            case '"': o += "\\\""; break;
            case '\\': o += "\\\\"; break;
            case '\n': o += "\\n"; break;
            case '\r': o += "\\r"; break;
            case '\t': o += "\\t"; break;
            default:
                if (*p < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", *p); o += b; }
                else o += static_cast<char>(*p);
        }
    }
    return o + "\"";
}
static string jstr(const string& s) { return jstr(s.c_str()); }

// coordinate rounded to 5 decimals, shortest form (like Python's round + json)
static string num(double v) {
    char b[32];
    snprintf(b, sizeof b, "%.5f", v);
    string s = b;
    if (s.find('.') != string::npos) {
        while (s.back() == '0') s.pop_back();
        if (s.back() == '.') s.pop_back();
    }
    if (s == "-0") s = "0";
    return s;
}
static string pt(double lon, double lat) { return "[" + num(lon) + "," + num(lat) + "]"; }

static bool is(const osmium::TagList& t, const char* k, const char* v) {
    const char* x = t.get_value_by_key(k);
    return x && !strcmp(x, v);
}
static string val(const osmium::TagList& t, const char* k) {
    const char* x = t.get_value_by_key(k);
    return x ? x : "";
}
static bool has(const osmium::TagList& t, const char* k) { return t.get_value_by_key(k) != nullptr; }
static bool passenger_mode(const osmium::TagList& t) {
    for (auto m : MODE_FLAGS) if (is(t, m, "yes")) return true;
    return false;
}

struct Data {
    std::map<unsigned long long, std::set<string>> way_routes;
    std::map<string, string> route_operator;
    std::map<std::pair<char, unsigned long long>, string> member_area;
};

class Handler : public osmium::handler::Handler {
public:
    Handler(Data& d, FILE* f, FILE* s) : d_(d), feat_(f), stop_(s) {}
    std::map<string, long> counts;
    std::vector<std::tuple<string, string, bool>> stations;
    std::set<unsigned long long> way_ids;
    std::set<string> used_areas;

    // a named station or stop that is being built: railway=construction + construction=station|halt, or the lifecycle
    // prefix construction:railway=station|halt|stop / construction:public_transport=station|stop_position
    static bool under_construction_station(const osmium::TagList& t) {
        if (!has(t, "name")) return false;
        if (is(t, "railway", "construction") && (is(t, "construction", "station") || is(t, "construction", "halt"))) return true;
        string cr = val(t, "construction:railway"), cp = val(t, "construction:public_transport");
        return cr == "station" || cr == "halt" || cr == "stop" || cp == "station" || cp == "stop_position";
    }

    static const char* classify_poly(const osmium::TagList& t) {
        if (is(t, "landuse", "railway")) return "area:landuse_railway";
        if (is(t, "railway", "yard")) return "area:yard";
        if (is(t, "building", "train_station")) return "area:station_building";
        if (is(t, "public_transport", "station") && (has(t, "railway") || passenger_mode(t))) return "area:station";
        if (is(t, "railway", "station") || is(t, "railway", "halt")) return "area:station";
        if (is(t, "railway", "platform") || (is(t, "public_transport", "platform") && passenger_mode(t))) return "area:platform";
        return nullptr;
    }

    void emit(const char* type, unsigned long long id, const osmium::TagList& t, const string& geom, const string& label) {
        string osm_id = string(type) + "/" + std::to_string(id);
        string p = "{";
        bool first = true;
        auto add = [&](const string& k, const string& v) { p += (first ? "" : ",") + jstr(k) + ":" + v; first = false; };
        for (auto k : KEEP) if (const char* v = t.get_value_by_key(k)) add(k, jstr(v));
        for (const auto& tag : t) if (!strncmp(tag.key(), "name:", 5)) add(tag.key(), jstr(tag.value()));
        add("osm_id", jstr(osm_id));
        if (!strcmp(type, "way")) {
            way_ids.insert(id);
            auto it = d_.way_routes.find(id);
            if (it != d_.way_routes.end() && !it->second.empty()) {
                string r = "[";
                bool f2 = true;
                std::set<string> ops;
                for (const auto& rid : it->second) {
                    r += (f2 ? "" : ",") + jstr(rid); f2 = false;
                    auto o = d_.route_operator.find(rid);
                    if (o != d_.route_operator.end() && !o->second.empty()) ops.insert(o->second);
                }
                add("_routes", r + "]");
                if (!ops.empty()) {
                    string o = "[";
                    bool f3 = true;
                    for (const auto& x : ops) { o += (f3 ? "" : ",") + jstr(x); f3 = false; }
                    add("_operators", o + "]");
                }
            }
        }
        auto sa = d_.member_area.find({type[0], id});
        const char* rw = t.get_value_by_key("railway");
        const char* ptv = t.get_value_by_key("public_transport");
        if (sa != d_.member_area.end() &&
            ((rw && (!strcmp(rw, "station") || !strcmp(rw, "halt") || !strcmp(rw, "platform"))) ||
             (ptv && (!strcmp(ptv, "station") || !strcmp(ptv, "platform"))))) {
            add("_stop_area", jstr(sa->second));
            used_areas.insert(sa->second);
        }
        p += "}";
        fputs(("{\"type\":\"Feature\",\"properties\":" + p + ",\"geometry\":" + geom + "}\n").c_str(), feat_);
        counts[label]++;
    }
    bool props_has_stop_area(const char* type, unsigned long long id) { return d_.member_area.count({type[0], id}) > 0; }

    void node(const osmium::Node& n) {
        const auto& t = n.tags();
        if (t.empty() || has(t, "entrance")) return;
        if (under_construction_station(t)) {
            emit("node", n.id(), t, "{\"type\":\"Point\",\"coordinates\":" + pt(n.location().lon(), n.location().lat()) + "}", "station:construction");
            return;
        }
        if (is(t, "public_transport", "stop_position") || is(t, "railway", "stop")) {
            if (passenger_mode(t) || is(t, "railway", "stop")) {
                string modes = "{";
                bool f = true;
                for (auto m : MODE_FLAGS) if (const char* v = t.get_value_by_key(m)) { modes += (f ? "" : ",") + jstr(m) + ":" + jstr(v); f = false; }
                modes += "}";
                auto name = t.get_value_by_key("name"), ref = t.get_value_by_key("ref");
                string row = "{\"osm_id\":\"node/" + std::to_string(n.id()) + "\",\"lon\":" + num(n.location().lon()) + ",\"lat\":" + num(n.location().lat()) +
                             ",\"name\":" + (name ? jstr(name) : "null") + ",\"modes\":" + modes + ",\"ref\":" + (ref ? jstr(ref) : "null") + "}\n";
                fputs(row.c_str(), stop_);
                auto sa = d_.member_area.find({'n', static_cast<unsigned long long>(n.id())});
                if (sa != d_.member_area.end()) used_areas.insert(sa->second);
                counts["stop_position"]++;
            }
            return;
        }
        bool station = is(t, "railway", "station") || is(t, "railway", "halt") ||
                       (is(t, "public_transport", "station") && (has(t, "railway") || passenger_mode(t)));
        if (!station) return;
        if (is(t, "railway", "abandoned") || is(t, "railway", "disused") || is(t, "railway", "proposed")) return;
        emit("node", n.id(), t, "{\"type\":\"Point\",\"coordinates\":" + pt(n.location().lon(), n.location().lat()) + "}",
             "station:" + (has(t, "railway") ? val(t, "railway") : string("pt")));
        stations.emplace_back("node/" + std::to_string(n.id()), val(t, "name"), props_has_stop_area("node", n.id()) &&
                              (is(t, "railway", "station") || is(t, "railway", "halt") || is(t, "public_transport", "station")));
        names_[stations.size() - 1] = has(t, "name");
    }
    std::map<size_t, bool> names_;  // station has a name tag (distinguishes missing from empty)

    // linestring from a way; false when it has no usable geometry
    static bool line(const osmium::Way& w, string& out) {
        string c;
        double plon = 0, plat = 0;
        int n = 0;
        for (const auto& nr : w.nodes()) {
            if (!nr.location().valid()) return false;
            double lon = nr.location().lon(), lat = nr.location().lat();
            if (n && lon == plon && lat == plat) continue;
            c += (n ? "," : "") + pt(lon, lat);
            plon = lon; plat = lat; n++;
        }
        if (n < 2) return false;
        out = "{\"type\":\"LineString\",\"coordinates\":[" + c + "]}";
        return true;
    }

    void way(const osmium::Way& w) {
        const auto& t = w.tags();
        if (t.empty() || has(t, "entrance")) return;
        string rw = val(t, "railway");
        if (rw == "platform_edge") return;
        string g;
        if (TRACK.count(rw)) {
            string svc = val(t, "service");
            if (!line(w, g)) { counts["dropped:bad_geometry"]++; return; }
            emit("way", w.id(), t, g, "track:" + rw + (SERVICE_KEEP.count(svc) ? ":" + svc : ""));
            return;
        }
        if (rw == "siding" || rw == "spur" || rw == "crossover") {
            if (!line(w, g)) return;
            emit("way", w.id(), t, g, "track:" + rw);
            return;
        }
        const char* lab = classify_poly(t);
        if (!lab) return;
        string label = lab;
        bool closed = w.is_closed() && w.nodes().size() >= 4;
        if (closed && (label != "area:platform" || is(t, "area", "yes")) && !is(t, "area", "no")) {
            string c;
            for (const auto& nr : w.nodes()) {
                if (!nr.location().valid()) return;
                c += (c.empty() ? "" : ",") + pt(nr.location().lon(), nr.location().lat());
            }
            g = "{\"type\":\"Polygon\",\"coordinates\":[[" + c + "]]}";
        } else if (label == "area:platform") {
            if (!line(w, g)) return;
            label = "platform:line";
        } else {
            return;
        }
        emit("way", w.id(), t, g, label);
    }

    void area(const osmium::Area& a) {
        if (a.from_way()) return;
        const auto& t = a.tags();
        const char* lab = classify_poly(t);
        if (!lab) return;
        string polys;
        bool fp = true;
        try {
            for (const auto& outer : a.outer_rings()) {
                string rings;
                auto ring = [&](const osmium::NodeRefList& r) {
                    string c;
                    for (const auto& nr : r) c += (c.empty() ? "" : ",") + pt(nr.lon(), nr.lat());
                    rings += (rings.empty() ? "[" : ",[") + c + "]";
                };
                ring(outer);
                for (const auto& inner : a.inner_rings(outer)) ring(inner);
                polys += (fp ? "[" : ",[") + rings + "]";
                fp = false;
            }
        } catch (...) { counts["dropped:bad_multipolygon"]++; return; }
        if (fp) { counts["dropped:bad_multipolygon"]++; return; }
        emit("relation", a.orig_id(), t, "{\"type\":\"MultiPolygon\",\"coordinates\":[" + polys + "]}", lab);
    }

private:
    Data& d_;
    FILE* feat_;
    FILE* stop_;
};

// pass 1: relations side table
class RelHandler : public osmium::handler::Handler {
public:
    RelHandler(Data& d, FILE* out) : d_(d), out_(out) {}
    void relation(const osmium::Relation& r) {
        const auto& t = r.tags();
        string typ = val(t, "type"), route = val(t, "route"), ptype = val(t, "public_transport"), kind;
        if (typ == "public_transport" && ptype == "stop_area") kind = "stop_area";
        else if (typ == "public_transport" && ptype == "stop_area_group") kind = "stop_area_group";
        else if (typ == "route" && ROUTE_MODES.count(route)) kind = "route";
        else if (typ == "route_master" && ROUTE_MODES.count(val(t, "route_master"))) kind = "route_master";
        else if (typ == "route" && route == "railway") kind = "railway_line";  // named infrastructure line, not a service
        else return;
        string rid = "relation/" + std::to_string(r.id());
        string tags = "{";
        bool f = true;
        for (auto k : REL_TAGS) if (const char* v = t.get_value_by_key(k)) { tags += (f ? "" : ",") + jstr(k) + ":" + jstr(v); f = false; }
        for (const auto& tag : t) if (!strncmp(tag.key(), "name:", 5)) { tags += (f ? "" : ",") + jstr(tag.key()) + ":" + jstr(tag.value()); f = false; }
        tags += "}";
        string members = "[";
        bool fm = true;
        for (const auto& m : r.members()) {
            if (kind == "railway_line" && m.type() == osmium::item_type::way) continue;  // only stations and stops are needed, not the track
            const char* ty = m.type() == osmium::item_type::node ? "node" : m.type() == osmium::item_type::way ? "way" : "relation";
            members += (fm ? "" : ",") + string("{\"type\":\"") + ty + "\",\"id\":" + std::to_string(m.ref()) + ",\"role\":" + jstr(m.role()) + "}";
            fm = false;
        }
        members += "]";
        fputs(("{\"id\":" + jstr(rid) + ",\"kind\":\"" + kind + "\",\"tags\":" + tags + ",\"members\":" + members + "}\n").c_str(), out_);
        if (kind == "route") {
            d_.route_operator[rid] = val(t, "operator");
            for (const auto& m : r.members())
                if (m.type() == osmium::item_type::way && (!*m.role() || !strcmp(m.role(), "route"))) d_.way_routes[m.ref()].insert(rid);
        } else if (kind == "stop_area") {
            for (const auto& m : r.members()) {
                char c = m.type() == osmium::item_type::node ? 'n' : m.type() == osmium::item_type::way ? 'w' : 'r';
                d_.member_area[{c, static_cast<unsigned long long>(m.ref())}] = rid;
            }
        }
    }
private:
    Data& d_;
    FILE* out_;
};

int main(int argc, char** argv) {
    if (argc < 3) { fprintf(stderr, "usage: rail_filter <input.osm.pbf> <outdir>\n"); return 2; }
    string in = argv[1], out = argv[2];
    Data d;
    FILE* rel = fopen((out + "/relations_all.jsonl").c_str(), "w");
    FILE* feat = fopen((out + "/features.geojsonl").c_str(), "w");
    FILE* stop = fopen((out + "/stop_positions.jsonl").c_str(), "w");
    if (!rel || !feat || !stop) { perror("open"); return 1; }

    {   // pass 1: relations only
        osmium::io::Reader reader{in, osmium::osm_entity_bits::relation};
        RelHandler rh{d, rel};
        osmium::apply(reader, rh);
        reader.close();
    }

    // pass 2: nodes, ways and assembled multipolygons
    using index_type = osmium::index::map::FlexMem<osmium::unsigned_object_id_type, osmium::Location>;
    using loc_handler = osmium::handler::NodeLocationsForWays<index_type>;
    osmium::area::Assembler::config_type cfg;
    osmium::area::MultipolygonManager<osmium::area::Assembler> mp{cfg};
    osmium::relations::read_relations(osmium::io::File{in}, mp);
    index_type index;
    loc_handler locs{index};
    locs.ignore_errors();
    Handler h{d, feat, stop};
    osmium::io::Reader reader{in, osmium::io::read_meta::no};
    osmium::apply(reader, locs, h, mp.handler([&](osmium::memory::Buffer&& b) { osmium::apply(b, h); }));
    reader.close();
    fclose(rel); fclose(feat); fclose(stop);

    FILE* m = fopen((out + "/filter_meta.json").c_str(), "w");
    string s = "{\"counts\":{";
    bool f = true;
    for (const auto& kv : h.counts) { s += (f ? "" : ",") + jstr(kv.first) + ":" + std::to_string(kv.second); f = false; }
    s += "},\"stations\":[";
    f = true;
    size_t i = 0;
    for (const auto& st : h.stations) {
        s += (f ? "" : ",") + string("[") + jstr(std::get<0>(st)) + "," + (h.names_[i] ? jstr(std::get<1>(st)) : "null") + "," + (std::get<2>(st) ? "true" : "false") + "]";
        f = false; i++;
    }
    s += "],\"way_ids\":[";
    f = true;
    for (auto id : h.way_ids) { s += (f ? "" : ",") + std::to_string(id); f = false; }
    s += "],\"used_areas\":[";
    f = true;
    for (const auto& a : h.used_areas) { s += (f ? "" : ",") + jstr(a); f = false; }
    s += "]}\n";
    fputs(s.c_str(), m);
    fclose(m);
    return 0;
}
