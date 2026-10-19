//! Lengths typed in the Measurements box and shown in the UI, and the model's unit settings
//! (Window › Model Info › Units: SketchUp's `UnitsOptions`, kept in `Model::meta.options`
//! where the Ruby API reads them too).

use std::cell::RefCell;

use serde_json::{Value, json};

use crate::model::{Model, Units};

const INCH: f64 = 25.4;

impl Units {
    pub fn factor(self) -> f64 {
        match self {
            Units::Millimeters => 1.0,
            Units::Centimeters => 10.0,
            Units::Meters => 1000.0,
            Units::Inches => INCH,
            Units::Feet => INCH * 12.0,
            Units::Yards => INCH * 36.0,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Units::Millimeters => "mm",
            Units::Centimeters => "cm",
            Units::Meters => "m",
            Units::Inches => "\"",
            Units::Feet => "'",
            Units::Yards => "yd",
        }
    }

    /// SketchUp's `LengthUnit` code: 0 inches, 1 feet, 2 mm, 3 cm, 4 m, 5 yards.
    pub fn code(self) -> i64 {
        match self {
            Units::Inches => 0,
            Units::Feet => 1,
            Units::Millimeters => 2,
            Units::Centimeters => 3,
            Units::Meters => 4,
            Units::Yards => 5,
        }
    }

    pub fn from_code(code: i64) -> Option<Units> {
        Some(match code {
            0 => Units::Inches,
            1 => Units::Feet,
            2 => Units::Millimeters,
            3 => Units::Centimeters,
            4 => Units::Meters,
            5 => Units::Yards,
            _ => return None,
        })
    }

    /// Name in the unit lists of Model Info (SketchUp's French labels).
    pub fn name(self) -> &'static str {
        match self {
            Units::Inches => "Pouces",
            Units::Feet => "Pieds",
            Units::Yards => "Verges",
            Units::Millimeters => "Millimètres",
            Units::Centimeters => "Centimètres",
            Units::Meters => "Mètres",
        }
    }

    pub const ALL: [Units; 6] =
        [Units::Millimeters, Units::Centimeters, Units::Meters, Units::Inches, Units::Feet, Units::Yards];

    /// In the order of SketchUp's list (inches, feet, yards, mm, cm, m).
    pub const SKETCHUP_ORDER: [Units; 6] =
        [Units::Inches, Units::Feet, Units::Yards, Units::Millimeters, Units::Centimeters, Units::Meters];
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    if let Some((a, b)) = s.split_once('/') {
        return Some(a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?);
    }
    s.parse().ok()
}

/// Parses "1200", "1,5m", "50 cm", "3'", "2'6\"", "-40". Bare numbers use `default`.
/// Returns millimetres.
pub fn parse_length(s: &str, default: Units) -> Option<f64> {
    let s = s.trim().trim_start_matches('~').trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some((ft, rest)) = s.split_once('\'') {
        let feet = parse_number(ft)?;
        let rest = rest.trim().trim_end_matches('"');
        let inches = if rest.is_empty() { 0.0 } else { parse_number(rest)? };
        let sign = if feet < 0.0 || ft.trim().starts_with('-') { -1.0 } else { 1.0 };
        return Some((feet.abs() * 12.0 + inches) * INCH * sign);
    }
    for (suffix, f) in [("mm", 1.0), ("cm", 10.0), ("km", 1e6), ("yd", INCH * 36.0), ("m", 1000.0), ("\"", INCH)] {
        if let Some(n) = s.strip_suffix(suffix) {
            return parse_number(n).map(|x| x * f);
        }
    }
    parse_number(&s).map(|x| x * default.factor())
}

/// Shows a length in the model units: with the model's Units settings when the application
/// made them current ([`set_display`]), else French style with a few decimals ("1 250 mm",
/// "1,25 m").
pub fn format_length(mm: f64, units: Units) -> String {
    if let Some(d) = display().filter(|d| d.unit == units) {
        return d.length(mm);
    }
    let v = mm / units.factor();
    let decimals = match units {
        Units::Millimeters => 1,
        Units::Centimeters => 2,
        Units::Meters => 3,
        Units::Inches | Units::Feet | Units::Yards => 2,
    };
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".into();
    }
    format!("{} {}", s.replace('.', ","), units.suffix())
}

// ------------------------------------------------------------------------------------------
// Model Info › Units

/// Length formats of Model Info › Units (`LengthFormat`).
pub const FORMATS: [&str; 4] = ["Décimal", "Architecture", "Génie", "Fractionnel"];
/// Area units (`AreaUnit`): suffix, square millimetres per unit.
pub const AREA_UNITS: [(&str, f64); 6] = [
    ("in²", INCH * INCH),
    ("ft²", 144.0 * INCH * INCH),
    ("mm²", 1.0),
    ("cm²", 100.0),
    ("m²", 1e6),
    ("yd²", 1296.0 * INCH * INCH),
];
/// Volume units (`VolumeUnit`): suffix, cubic millimetres per unit (6 litres, 7 US gallons).
pub const VOLUME_UNITS: [(&str, f64); 8] = [
    ("in³", INCH * INCH * INCH),
    ("ft³", 1728.0 * INCH * INCH * INCH),
    ("mm³", 1.0),
    ("cm³", 1e3),
    ("m³", 1e9),
    ("yd³", 46656.0 * INCH * INCH * INCH),
    ("L", 1e6),
    ("gal", 231.0 * INCH * INCH * INCH),
];
/// Choices of Model Info › Units › Enable angle snapping (degrees).
pub const SNAP_ANGLES: [f64; 10] = [0.1, 0.5, 1.0, 3.0, 5.0, 7.5, 10.0, 15.0, 30.0, 45.0];

/// Default `LengthPrecision` of a unit when the model has none (as the Ruby API reports it).
pub fn default_precision(u: Units) -> u32 {
    match u {
        Units::Millimeters => 0,
        Units::Centimeters => 1,
        _ => 3,
    }
}

/// How the model shows lengths, areas, volumes and angles, and how drawing snaps to them:
/// Model Info › Units. Read from and written to `Model::meta.options["UnitsOptions"]` (and
/// `Model::units` for the length unit), the values the Ruby API's `model.options` gives.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitsDisplay {
    pub unit: Units,
    /// 0 decimal, 1 architectural, 2 engineering, 3 fractional.
    pub format: u8,
    /// Decimal places, or the power of two of the denominator for fractions.
    pub precision: u32,
    /// "Display units format" unticked (`SuppressUnitsDisplay`).
    pub suppress: bool,
    /// Force display of 0" (`ForceInchDisplay`).
    pub force_inch: bool,
    /// Enable length snapping, and the length (mm).
    pub length_snap: bool,
    pub snap_length: f64,
    pub area_unit: usize,
    pub area_precision: u32,
    pub volume_unit: usize,
    pub volume_precision: u32,
    pub angle_precision: u32,
    pub angle_snap: bool,
    /// Degrees.
    pub snap_angle: f64,
}

impl Default for UnitsDisplay {
    fn default() -> Self {
        UnitsDisplay::of(&Model::new())
    }
}

const TOL_F: f64 = 0.001;
const EPS: f64 = 1e-6;

/// "n/d" of a fraction of an inch, with the smallest power-of-two denominator up to 64.
fn fraction_parts(frac: f64) -> Option<(i64, i64)> {
    (1..=6).find_map(|k| {
        let d = 1i64 << k;
        let n = (d as f64 * frac) as i64;
        (d as f64 * frac - (n as f64) < EPS).then_some((n, d))
    })
}

fn opt<'a>(m: &'a Model, key: &str) -> Option<&'a Value> {
    m.meta.options.get("UnitsOptions")?.get(key)
}

impl UnitsDisplay {
    /// The settings of a model.
    pub fn of(m: &Model) -> UnitsDisplay {
        let int = |k: &str| opt(m, k).and_then(Value::as_i64);
        let num = |k: &str| opt(m, k).and_then(Value::as_f64);
        let flag = |k: &str, d: bool| opt(m, k).and_then(Value::as_bool).unwrap_or(d);
        let unit = m.units;
        let precision = int("LengthPrecision").map_or(default_precision(unit), |p| p.clamp(0, 6) as u32);
        UnitsDisplay {
            unit,
            format: int("LengthFormat").map_or(0, |f| f.clamp(0, 3) as u8),
            precision,
            suppress: flag("SuppressUnitsDisplay", false),
            force_inch: flag("ForceInchDisplay", false),
            length_snap: flag("LengthSnapEnabled", false),
            // SketchUp keeps it in inches.
            snap_length: num("LengthSnapLength").unwrap_or(1.0) * INCH,
            area_unit: int("AreaUnit").map_or(4, |u| u.clamp(0, 5) as usize),
            area_precision: int("AreaPrecision").map_or(precision, |p| p.clamp(0, 6) as u32),
            volume_unit: int("VolumeUnit").map_or(4, |u| u.clamp(0, 7) as usize),
            volume_precision: int("VolumePrecision").map_or(precision, |p| p.clamp(0, 6) as u32),
            angle_precision: int("AnglePrecision").map_or(1, |p| p.clamp(0, 3) as u32),
            angle_snap: flag("AngleSnapEnabled", true),
            snap_angle: num("SnapAngle").unwrap_or(15.0),
        }
    }

    /// Writes the settings into a model (as `model.options["UnitsOptions"]` would).
    pub fn write(&self, m: &mut Model) {
        m.units = self.unit;
        let o = m.meta.options.entry("UnitsOptions".into()).or_default();
        let mut put = |k: &str, v: Value| {
            o.insert(k.to_string(), v);
        };
        put("LengthUnit", json!(self.unit.code()));
        put("LengthFormat", json!(self.format));
        put("LengthPrecision", json!(self.precision));
        put("SuppressUnitsDisplay", json!(self.suppress));
        put("ForceInchDisplay", json!(self.force_inch));
        put("LengthSnapEnabled", json!(self.length_snap));
        put("LengthSnapLength", json!(self.snap_length / INCH));
        put("AreaUnit", json!(self.area_unit));
        put("AreaPrecision", json!(self.area_precision));
        put("VolumeUnit", json!(self.volume_unit));
        put("VolumePrecision", json!(self.volume_precision));
        put("AnglePrecision", json!(self.angle_precision));
        put("AngleSnapEnabled", json!(self.angle_snap));
        put("SnapAngle", json!(self.snap_angle));
    }

    /// The precisions a format offers: "0", "0,0"… for decimal ones, "1\"", "1/2\""… for
    /// fractions of an inch.
    pub fn precision_choices(format: u8) -> Vec<String> {
        match format {
            1 | 3 => (0..=6).map(|k| if k == 0 { "1\"".into() } else { format!("1/{}\"", 1 << k) }).collect(),
            _ => {
                let suffix = if format == 2 { "'" } else { "" };
                (0..=6)
                    .map(|k| if k == 0 { format!("0{suffix}") } else { format!("0,{}{suffix}", "0".repeat(k)) })
                    .collect()
            }
        }
    }

    /// Units a format allows: architectural and fractional are in inches, engineering in feet.
    pub fn format_unit(format: u8) -> Option<Units> {
        match format {
            1 | 3 => Some(Units::Inches),
            2 => Some(Units::Feet),
            _ => None,
        }
    }

    fn step(fmt: u8, prec: u32) -> f64 {
        if prec == 0 {
            return if fmt == 2 { 12.0 } else { 1.0 };
        }
        match fmt {
            0 => 1.0 / 10f64.powi(prec as i32),
            2 => 12.0 / 10f64.powi(prec as i32),
            _ => 1.0 / (1u64 << prec) as f64,
        }
    }

    /// Body of a length (no sign, no "~"), in the formatted unit (inches for fractions).
    fn body(&self, value: f64, fmt: u8, force_frac: bool) -> String {
        let prec = self.precision;
        let whole = (value + EPS) as i64;
        let frac = if prec == 0 { 0.0 } else { value - whole as f64 };
        let mut stp = if fmt == 0 || fmt == 2 { 1.0 / 10f64.powi(prec as i32) } else { 1.0 / (1u64 << prec) as f64 };
        if (fmt == 0 || fmt == 2) && prec == 0 {
            stp = 0.1;
        }
        let fraction = |with_inch: bool| -> String {
            let mark = if with_inch { "\"" } else { "" };
            match fraction_parts(frac) {
                None => String::new(),
                Some((n, d)) if whole < 1 => format!("{n}/{d}{mark}"),
                Some((n, d)) => format!("{whole} {n}/{d}{mark}"),
            }
        };
        let s = match fmt {
            1 => {
                if frac - stp > -TOL_F || force_frac {
                    fraction(true)
                } else {
                    format!("{whole}\"")
                }
            }
            3 => {
                let t = if frac - stp <= -TOL_F { whole.to_string() } else { fraction(false) };
                if self.suppress { t } else { format!("{t}\"") }
            }
            2 => {
                if frac - stp <= -stp && !force_frac {
                    format!("{whole}'")
                } else {
                    format!("{value:.p$}'", p = prec as usize)
                }
            }
            _ => {
                let mut t = format!("{value:.p$}", p = prec as usize);
                if !self.suppress {
                    if self.unit.code() > 1 {
                        t.push(' ');
                    }
                    t.push_str(self.unit.suffix());
                }
                t
            }
        };
        s.replace('.', ",")
    }

    /// A length (mm) as SketchUp shows it: rounded to the precision, "~" in front when the
    /// rounding changed it.
    pub fn length(&self, mm: f64) -> String {
        if !mm.is_finite() {
            return mm.to_string();
        }
        let v = mm / INCH;
        let fmt = self.format;
        let prec = self.precision;
        let neg = v < 0.0;
        let x = if fmt == 0 { mm / self.unit.factor() } else { v };
        let a = x.abs();
        let r = if prec == 0 {
            (a + 0.5).floor()
        } else {
            let st = Self::step(fmt, prec);
            ((a / st) + 0.5).floor() * st
        };
        let exact = (a - r).abs() < TOL_F;
        let b = match fmt {
            1 => {
                let feet = ((r + EPS) / 12.0) as i64;
                let inch = r - feet as f64 * 12.0;
                if feet == 0 {
                    if inch < TOL_F { "0\"".to_string() } else { self.body(inch, 1, false) }
                } else if inch < TOL_F {
                    if self.force_inch { format!("{feet}' 0\"") } else { format!("{feet}'") }
                } else {
                    format!("{feet}' {}", self.body(inch, 1, false))
                }
            }
            2 => self.body(r / 12.0, 2, false),
            _ => self.body(r, fmt, false),
        };
        let mut out = String::new();
        if !exact {
            out.push('~');
        }
        if neg && r != 0.0 {
            out.push('-');
        }
        out + &b
    }

    fn number(v: f64, prec: u32) -> String {
        let s = format!("{v:.p$}", p = prec as usize);
        let s = if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') { s[1..].to_string() } else { s };
        let approx = (v - s.parse::<f64>().unwrap_or(v)).abs() > 1e-9 * v.abs().max(1.0);
        let s = s.replace('.', ",");
        if approx { format!("~ {s}") } else { s }
    }

    /// An area (mm²) in the model's area unit.
    pub fn area(&self, mm2: f64) -> String {
        let (suffix, per) = AREA_UNITS[self.area_unit.min(5)];
        format!("{} {suffix}", Self::number(mm2 / per, self.area_precision))
    }

    /// A volume (mm³) in the model's volume unit.
    pub fn volume(&self, mm3: f64) -> String {
        let (suffix, per) = VOLUME_UNITS[self.volume_unit.min(7)];
        format!("{} {suffix}", Self::number(mm3 / per, self.volume_precision))
    }

    /// An angle (degrees) at the model's angle precision.
    pub fn angle(&self, deg: f64) -> String {
        format!("{}°", Self::number(deg, self.angle_precision))
    }

    /// A length rounded to the length snapping increment, when it is on.
    pub fn snap_length(&self, mm: f64) -> f64 {
        if self.length_snap && self.snap_length > 1e-9 {
            (mm / self.snap_length).round() * self.snap_length
        } else {
            mm
        }
    }

    /// The protractor's snapping step (radians), or `None` when angle snapping is off.
    pub fn angle_step(&self) -> Option<f64> {
        (self.angle_snap && self.snap_angle > 1e-9).then(|| self.snap_angle.to_radians())
    }
}

thread_local! {
    static DISPLAY: RefCell<Option<UnitsDisplay>> = const { RefCell::new(None) };
}

/// Makes a model's Units settings the ones [`format_length`], [`format_area`]… use on this
/// thread (the application does it every frame for the open model).
pub fn set_display(d: Option<UnitsDisplay>) {
    DISPLAY.with(|c| *c.borrow_mut() = d);
}

/// The Units settings made current by [`set_display`].
pub fn display() -> Option<UnitsDisplay> {
    DISPLAY.with(|c| c.borrow().clone())
}

/// An area (mm²) as the current settings show it (Model Info › Units › Area), or in the
/// square of `units` ("0,6 m²", "6000 cm²") when those settings are for other units.
pub fn format_area(mm2: f64, units: Units) -> String {
    match display() {
        Some(d) if d.unit == units => d.area(mm2),
        _ => {
            let f = units.factor();
            let mut s = format!("{:.2}", mm2 / (f * f));
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            if s == "-0" {
                s = "0".into();
            }
            let unit = match units {
                Units::Inches => "po",
                Units::Feet => "pi",
                u => u.suffix(),
            };
            format!("{} {unit}²", s.replace('.', ","))
        }
    }
}

/// A volume (mm³) as the current settings show it.
pub fn format_volume(mm3: f64, units: Units) -> String {
    match display() {
        Some(d) if d.unit == units => d.volume(mm3),
        _ => {
            let k = units.factor();
            format!("{} {}³", format!("{:.2}", mm3 / (k * k * k)).replace('.', ","), units.suffix())
        }
    }
}

/// An angle (degrees) as the current settings show it (one decimal otherwise).
pub fn format_angle(deg: f64) -> String {
    match display() {
        Some(d) => d.angle(deg),
        None => format!("{deg:.1}°").replace('.', ","),
    }
}

/// The protractor's snapping step of the current settings (15° by default; `None` = off).
pub fn angle_step() -> Option<f64> {
    match display() {
        Some(d) => d.angle_step(),
        None => Some(crate::angle::DEFAULT_SNAP_STEP),
    }
}

/// A drawn length rounded to the length snapping increment of the current settings.
pub fn snap_length(mm: f64) -> f64 {
    display().map_or(mm, |d| d.snap_length(mm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_areas_in_square_units() {
        assert_eq!(format_area(600_000.0, Units::Meters), "0,6 m²");
        assert_eq!(format_area(600_000.0, Units::Centimeters), "6000 cm²");
        assert_eq!(format_area(144.0 * 25.4 * 25.4, Units::Feet), "1 pi²");
    }

    #[test]
    fn parses_common_inputs() {
        let mm = Units::Millimeters;
        assert_eq!(parse_length("1200", mm), Some(1200.0));
        assert_eq!(parse_length("1,5m", mm), Some(1500.0));
        assert_eq!(parse_length("50 cm", mm), Some(500.0));
        assert_eq!(parse_length("2'6\"", mm), Some(30.0 * 25.4));
        assert_eq!(parse_length("12", Units::Centimeters), Some(120.0));
        assert_eq!(parse_length("-40", mm), Some(-40.0));
        assert_eq!(parse_length("abc", mm), None);
        assert_eq!(parse_length("2yd", mm), Some(72.0 * 25.4));
        assert_eq!(parse_length("~ 12 mm", mm), Some(12.0));
    }

    #[test]
    fn formats() {
        assert_eq!(format_length(1250.0, Units::Meters), "1,25 m");
        assert_eq!(format_length(1200.0, Units::Millimeters), "1200 mm");
    }

    fn st(unit: Units, format: u8, precision: u32) -> UnitsDisplay {
        UnitsDisplay { unit, format, precision, ..UnitsDisplay::default() }
    }

    #[test]
    fn sketchup_length_formats() {
        // Decimal: fixed decimals, a space before metric units, "~" when rounded.
        assert_eq!(st(Units::Millimeters, 0, 0).length(1200.0), "1200 mm");
        assert_eq!(st(Units::Meters, 0, 3).length(1250.0), "1,250 m");
        assert_eq!(st(Units::Meters, 0, 2).length(1254.0), "~1,25 m");
        assert_eq!(st(Units::Centimeters, 0, 1).length(-125.0), "-12,5 cm");
        assert_eq!(st(Units::Inches, 0, 3).length(25.4 * 1.5), "1,500\"");
        assert_eq!(st(Units::Yards, 0, 2).length(914.4 * 2.0), "2,00 yd");
        // Architectural: feet and fractions of an inch.
        assert_eq!(st(Units::Inches, 1, 4).length(25.4 * 30.5), "2' 6 1/2\"");
        assert_eq!(st(Units::Inches, 1, 4).length(25.4 * 24.0), "2'");
        let force = UnitsDisplay { force_inch: true, ..st(Units::Inches, 1, 4) };
        assert_eq!(force.length(25.4 * 24.0), "2' 0\"");
        // Fractional inches, engineering feet.
        assert_eq!(st(Units::Inches, 3, 3).length(25.4 * 1.25), "1 1/4\"");
        assert_eq!(st(Units::Feet, 2, 2).length(25.4 * 18.0), "1,50'");
        // Units not shown.
        let bare = UnitsDisplay { suppress: true, ..st(Units::Millimeters, 0, 3) };
        assert_eq!(bare.length(63.5), "63,500");
    }

    #[test]
    fn areas_volumes_angles_and_snaps() {
        let d =
            UnitsDisplay { area_unit: 4, area_precision: 2, volume_unit: 6, volume_precision: 1, ..Default::default() };
        assert_eq!(d.area(1.5e6), "1,50 m²");
        assert_eq!(d.volume(2.5e6), "2,5 L");
        assert_eq!(d.area(1.2345e6), "~ 1,23 m²");
        let d = UnitsDisplay { angle_precision: 2, ..d };
        assert_eq!(d.angle(45.0), "45,00°");
        let d = UnitsDisplay { length_snap: true, snap_length: 5.0, ..d };
        assert_eq!(d.snap_length(12.6), 15.0);
        let d = UnitsDisplay { angle_snap: false, ..d };
        assert_eq!(d.angle_step(), None);
    }

    #[test]
    fn settings_are_kept_in_the_model_options() {
        let mut m = Model::new();
        let d = UnitsDisplay::of(&m);
        assert_eq!((d.unit, d.format, d.precision, d.snap_angle), (Units::Millimeters, 0, 0, 15.0));
        let new = UnitsDisplay { unit: Units::Yards, format: 0, precision: 4, snap_angle: 7.5, ..d };
        new.write(&mut m);
        assert_eq!(m.units, Units::Yards);
        let o = &m.meta.options["UnitsOptions"];
        assert_eq!((o["LengthUnit"].as_i64(), o["LengthPrecision"].as_i64()), (Some(5), Some(4)));
        assert_eq!(UnitsDisplay::of(&m), new);
    }

    #[test]
    fn the_current_settings_drive_format_length() {
        let d = st(Units::Meters, 0, 3);
        set_display(Some(d));
        assert_eq!(format_length(1250.0, Units::Meters), "1,250 m");
        // Another unit than the model's keeps the plain style.
        assert_eq!(format_length(1250.0, Units::Millimeters), "1250 mm");
        set_display(None);
        assert_eq!(format_length(1250.0, Units::Meters), "1,25 m");
    }
}
