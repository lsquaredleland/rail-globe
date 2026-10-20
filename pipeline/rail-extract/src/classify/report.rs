//! features.typed.geojsonl and classify_report.json.

use super::*;

/// features.typed.geojsonl: the copy of features.geojsonl where each track gets its type fields at the end of its properties.
/// Also the track counts by type and by type and source.
pub(super) fn write_typed(
    feats: &[u8],
    tracks: &[TrackIn],
    decided: &mut [Option<Decided>],
    dropped: &[bool],
    con_mode: &[Option<&str>],
) -> (Vec<u8>, Vec<(String, i64)>, std::collections::BTreeMap<(String, String), i64>) {
    let mut typed = Vec::with_capacity(feats.len() + tracks.len() * 64);
    let mut at = 0;
    let (mut counts, mut sources): (Vec<(String, i64)>, std::collections::BTreeMap<(String, String), i64>) = (Vec::new(), Default::default());
    for (ti, tr) in tracks.iter().enumerate() {
        let Some(Decided { t, s, own }) = decided[ti].take() else { continue };
        let p = &tr.props;
        let rw = p.g("railway").unwrap_or("");
        if dropped[ti] {
            typed.extend_from_slice(&feats[at..tr.start]);
            at = tr.end;
            continue;
        }
        match con_mode[ti] {
            None => typed.extend_from_slice(&feats[at..tr.pos]),
            Some(m) => typed.extend_from_slice(&as_construction(&feats[at..tr.pos], m)),
        }
        at = tr.pos;
        if rw == "construction" {
            typed.extend_from_slice(b",\"construction_type\":");
            match p.g("construction") {
                Some(c) => js(&mut typed, c.as_bytes()),
                None => typed.extend_from_slice(b"null"),
            }
        }
        typed.extend_from_slice(b",\"type\":");
        js(&mut typed, t.as_bytes());
        typed.extend_from_slice(b",\"type_source\":");
        js(&mut typed, s.as_bytes());
        typed.extend_from_slice(b",\"own_type\":");
        match own.filter(|_| rw != "construction" && rw != "proposed") {
            Some(o) => js(&mut typed, o.as_bytes()),
            None => typed.extend_from_slice(b"null"),
        }
        bump(&mut counts, &t);
        *sources.entry((t, s.to_string())).or_default() += 1;
    }
    typed.extend_from_slice(&feats[at..]);
    (typed, counts, sources)
}

/// classify_report.json, and the warnings for exemptions that matched fewer features than their expect_min.
pub(super) fn report_json(
    slug: &str,
    cfg: &Cfg,
    ex_stats: &[ExStats],
    lines: &Lines,
    counts: &[(String, i64)],
    sources: &std::collections::BTreeMap<(String, String), i64>,
) -> (Vec<String>, Vec<u8>) {
    let mut stale = Vec::new();
    let mut ex_report = Vec::new();
    for (x, st) in cfg.exemptions.iter().zip(ex_stats) {
        let matched = st.tracks + st.lines;
        let met = matched >= x.expect_min;
        if !met {
            stale.push(format!("stale exemption \"{}\" matched {matched} features, expected at least {}", x.id, x.expect_min));
        }
        ex_report.push((
            x.id.clone(),
            J::obj(vec![
                ("matched", J::Int(matched as i64)),
                ("tracks", J::Int(st.tracks as i64)),
                ("lines", J::Int(st.lines as i64)),
                ("km", J::Raw(format!("{:.1}", st.km))),
                ("expect_min", J::Int(x.expect_min as i64)),
                ("expect_min_met", J::Raw(met.to_string())),
            ]),
        ));
    }
    let mut report = J::obj(vec![
        ("region", J::s(slug)),
        ("track_by_type", J::Obj(most_common(counts))),
        ("track_by_type_and_source", J::Obj(sources.iter().map(|((t, s), n)| (format!("{t}/{s}"), J::Int(*n))).collect())),
        ("lines", J::Int(lines.ids.len() as i64)),
        ("lines_by_type", J::Obj(most_common(&lines.by_type))),
        ("lines_by_type_source", J::Obj(most_common(&lines.by_type_src))),
    ]);
    if let (J::Obj(fields), false) = (&mut report, ex_report.is_empty()) {
        fields.push(("exemptions".into(), J::Obj(ex_report)));
    }
    let mut rep = Vec::new();
    report.write_pretty(&mut rep, 0);
    (stale, rep)
}
