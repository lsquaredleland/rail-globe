//! Numbers the pipeline and the app must agree on, kept in one file (shared/constants.json at the repo root; the app imports it too):
//! the speed colour bands of app/src/mapStyle.ts, and the zoom bands of the construction dashes (property `b`).

use serde_json::Value;
use std::sync::LazyLock;

const RAW: &str = include_str!("../../../../shared/constants.json");

static JSON: LazyLock<Value> = LazyLock::new(|| serde_json::from_str(RAW).expect("shared/constants.json is not valid JSON"));

static SPEED_BAND_MINS: LazyLock<Vec<i64>> = LazyLock::new(|| {
    JSON["speed_band_mins"].as_array().expect("constants.json: speed_band_mins").iter().map(|v| v.as_i64().expect("constants.json: speed_band_mins entry")).collect()
});

/// lower bound (km/h) of each speed colour band, fastest first; the last is 0
pub(crate) fn speed_band_mins() -> &'static [i64] {
    &SPEED_BAND_MINS
}

/// the speed band a speed falls in: the lower bound of the first band it reaches
pub(crate) fn speed_band_min(kmh: i64) -> i64 {
    speed_band_mins().iter().find(|&&b| kmh >= b).copied().unwrap_or(0)
}

/// position of the speed band a speed falls in (0 = fastest band)
pub(crate) fn speed_band_index(kmh: i64) -> usize {
    speed_band_mins().iter().position(|&b| kmh >= b).unwrap_or(speed_band_mins().len() - 1)
}

/// zoom bands of the construction dashes: one copy per band of `step` zoom levels from `first`; the last band is shown from `last` to the deepest zoom
pub(crate) struct ConBand {
    pub first: f64,
    pub step: f64,
    pub last: f64,
}

pub(crate) fn con_band() -> ConBand {
    let c = &JSON["con_band"];
    let n = |k: &str| c[k].as_f64().unwrap_or_else(|| panic!("constants.json: con_band.{k}"));
    ConBand { first: n("first"), step: n("step"), last: n("last") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_constants_parse() {
        assert_eq!(speed_band_mins(), [400, 350, 300, 250, 200, 160, 120, 80, 0]);
        let b = con_band();
        assert_eq!((b.first, b.step, b.last), (4.0, 0.5, 14.5));
        assert_eq!(speed_band_min(250), 250);
        assert_eq!(speed_band_min(249), 200);
        assert_eq!(speed_band_min(5), 0);
        assert_eq!(speed_band_index(400), 0);
        assert_eq!(speed_band_index(79), 8);
    }
}
