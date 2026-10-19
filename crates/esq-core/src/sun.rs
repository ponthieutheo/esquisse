//! Shadow settings (Window › Shadows) and the sun's position for a place, date and time.

use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Shadow settings, stored like SketchUp's `ShadowInfo`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowInfo {
    /// View › Shadows.
    pub display: bool,
    /// Local date and time, in seconds since 1970-01-01 00:00 as if it were UTC
    /// (SketchUp's `ShadowTime`).
    pub time: i64,
    /// Hours from UTC.
    pub tz_offset: f64,
    /// Brightness of lit faces and of faces in shadow, 0–100.
    pub light: i32,
    pub dark: i32,
    pub use_sun_for_shading: bool,
    pub on_faces: bool,
    pub on_ground: bool,
    pub from_edges: bool,
    /// Model location (Window › Model Info › Geo-location), degrees.
    pub latitude: f64,
    pub longitude: f64,
    /// Angle of true north from the green axis, degrees, clockwise seen from above.
    pub north_angle: f64,
    pub city: String,
    pub country: String,
}

impl Default for ShadowInfo {
    /// The settings of SketchUp's templates: Boulder (Colorado), 8 November, 13:30.
    fn default() -> Self {
        ShadowInfo {
            display: false,
            time: 1_383_917_400,
            tz_offset: -7.0,
            light: 80,
            dark: 45,
            use_sun_for_shading: false,
            on_faces: true,
            on_ground: true,
            from_edges: false,
            latitude: 40.018309,
            longitude: -105.242139,
            north_angle: 0.0,
            city: "Boulder (CO)".into(),
            country: "USA".into(),
        }
    }
}

/// Year, month (1–12) and day (1–31) of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Days since 1970-01-01 of a calendar date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

impl ShadowInfo {
    /// (year, month, day, minutes after midnight) of the local shadow time.
    pub fn local(&self) -> (i64, u32, u32, f64) {
        let days = self.time.div_euclid(86_400);
        let secs = self.time.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        (y, m, d, secs as f64 / 60.0)
    }

    pub fn set_local(&mut self, y: i64, m: u32, d: u32, minutes: f64) {
        self.time = days_from_civil(y, m, d) * 86_400 + (minutes * 60.0).round() as i64;
    }

    pub fn day_of_year(&self) -> u32 {
        let (y, _, _, _) = self.local();
        (self.time.div_euclid(86_400) - days_from_civil(y, 1, 1) + 1) as u32
    }

    /// Solar declination (radians), equation of time (minutes) for the shadow date.
    fn solar_terms(&self) -> (f64, f64) {
        let (y, _, _, minutes) = self.local();
        let year_len = if is_leap(y) { 366.0 } else { 365.0 };
        let utc_hours = minutes / 60.0 - self.tz_offset;
        let g = std::f64::consts::TAU / year_len * (self.day_of_year() as f64 - 1.0 + (utc_hours - 12.0) / 24.0);
        let eqtime = 229.18
            * (0.000075 + 0.001868 * g.cos()
                - 0.032077 * g.sin()
                - 0.014615 * (2.0 * g).cos()
                - 0.040849 * (2.0 * g).sin());
        let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
            + 0.000907 * (2.0 * g).sin()
            - 0.002697 * (3.0 * g).cos()
            + 0.00148 * (3.0 * g).sin();
        (decl, eqtime)
    }

    /// Unit vector pointing at the sun, in model coordinates (red = east, green = north
    /// when the north angle is 0). Below the horizon, z is negative.
    pub fn sun_direction(&self) -> DVec3 {
        let (_, _, _, minutes) = self.local();
        let (decl, eqtime) = self.solar_terms();
        let lat = self.latitude.to_radians();
        let solar_minutes = minutes + eqtime + 4.0 * self.longitude - 60.0 * self.tz_offset;
        let ha = (solar_minutes / 4.0 - 180.0).to_radians();
        let cos_zen = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos()).clamp(-1.0, 1.0);
        let elev = std::f64::consts::FRAC_PI_2 - cos_zen.acos();
        // Azimuth from north, clockwise.
        let az = ha.sin().atan2(ha.cos() * lat.sin() - decl.tan() * lat.cos()) + std::f64::consts::PI;
        let local = DVec3::new(az.sin() * elev.cos(), az.cos() * elev.cos(), elev.sin());
        let north = -self.north_angle.to_radians();
        let (s, c) = north.sin_cos();
        DVec3::new(local.x * c - local.y * s, local.x * s + local.y * c, local.z)
    }

    /// Local sunrise and sunset, in minutes after midnight (None during polar day/night).
    /// Like SketchUp, the sun rises when its centre crosses the geometric horizon.
    pub fn sunrise_sunset(&self) -> Option<(f64, f64)> {
        let (decl, eqtime) = self.solar_terms();
        let lat = self.latitude.to_radians();
        let c = 90f64.to_radians().cos() / (lat.cos() * decl.cos()) - lat.tan() * decl.tan();
        if !(-1.0..=1.0).contains(&c) {
            return None;
        }
        let ha = c.acos().to_degrees();
        let noon = 720.0 - 4.0 * self.longitude - eqtime + 60.0 * self.tz_offset;
        Some((noon - 4.0 * ha, noon + 4.0 * ha))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trip() {
        let s = ShadowInfo::default();
        assert_eq!(s.local(), (2013, 11, 8, 810.0));
        assert_eq!(s.day_of_year(), 312);
        let mut t = s.clone();
        t.set_local(2013, 11, 8, 810.0);
        assert_eq!(t.time, s.time);
    }

    /// Directions SketchUp stores in its own files for the same settings.
    #[test]
    fn sun_matches_sketchup() {
        let s = ShadowInfo::default();
        let want = DVec3::new(-0.42414834, -0.77322708, 0.47140011);
        let got = s.sun_direction();
        assert!(got.angle_between(want).to_degrees() < 0.5, "{got:?}");

        let s = ShadowInfo { time: 1_782_048_600, latitude: 40.017, longitude: -105.283, ..ShadowInfo::default() };
        let want = DVec3::new(-0.33963735, -0.24340735, 0.90851491);
        let got = s.sun_direction();
        assert!(got.angle_between(want).to_degrees() < 0.5, "{got:?}");
    }

    #[test]
    fn sunrise_and_sunset_match_sketchup() {
        // SunRise / SunSet stored in SketchUp's template, as local times.
        let s = ShadowInfo::default();
        let (rise, set) = s.sunrise_sunset().unwrap();
        let want_rise = (1_383_892_994 % 86_400) as f64 / 60.0;
        let want_set = (1_383_929_157 % 86_400) as f64 / 60.0;
        assert!((rise - want_rise).abs() < 2.0, "{rise} vs {want_rise}");
        assert!((set - want_set).abs() < 2.0, "{set} vs {want_set}");
    }
}
