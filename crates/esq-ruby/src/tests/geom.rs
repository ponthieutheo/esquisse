//! Tests de la zone « geom » : Length, Point3d, Vector3d, Transformation, BoundingBox, Point2d,
//! Vector2d, Transformation2d, Bounds2d, PolygonMesh, LatLong, UTM, extensions d'Array/Numeric/String
//! et fonctions du module Geom. Chaque assertion épingle un comportement lu dans SketchUp.exe.
use super::*;

/// Runs `code`, returns the `inspect` of its value (or None when Ruby is unavailable).
fn val(code: &str) -> Option<String> {
    must(code).map(|(o, _)| o.result)
}

/// Runs `code` which must raise; returns "Class: message".
fn err(code: &str) -> Option<String> {
    let wrapped = format!("begin; {code}; 'no error'; rescue Exception => e; \"#{{e.class}}: #{{e.message}}\"; end");
    val(&wrapped).map(|s| s.trim_matches('"').replace("\\\"", "\""))
}

macro_rules! check {
    ($code:expr, $want:expr) => {
        if let Some(v) = val($code) {
            assert_eq!(v, $want, "code: {}", $code);
        }
    };
}

macro_rules! raises {
    ($code:expr, $want:expr) => {
        if let Some(v) = err($code) {
            assert_eq!(v, $want, "code: {}", $code);
        }
    };
}

// ------------------------------------------------------------------ Length & units
#[test]
fn numeric_units_return_lengths() {
    check!(
        "[1.m.class, 1.mm.class, 2.inch.class, 3.feet.class, 1.m.is_a?(Float), 1.m.is_a?(Length), 1.m.is_a?(Numeric)]",
        "[Length, Length, Length, Length, true, true, true]"
    );
    check!(
        "[1.inch.to_f, 1.feet.to_f, 1.yard.to_f, 1.mile.to_f, 25.4.mm.to_f.round(9), 2.54.cm.to_f.round(9)]",
        "[1.0, 12.0, 36.0, 63360.0, 1.0, 1.0]"
    );
    check!("[1.m.to_f.round(6), 1.km.to_f.round(4)]", "[39.370079, 39370.0787]");
    // to_* give plain Floats in the inverse direction
    check!(
        "[1.to_mm, 1.to_cm.round(6), 1.to_m, 1.to_km, 24.to_feet, 72.to_yard, 63360.to_mile, 5.to_inch].map { |v| [v.class, v.round(6)] }",
        "[[Float, 25.4], [Float, 2.54], [Float, 0.0254], [Float, 2.5e-05], [Float, 2.0], [Float, 2.0], [Float, 1.0], [Float, 5.0]]"
    );
    check!(
        "[180.degrees.round(10), 90.degrees.class, Math::PI.radians.round(10), 1.radians.round(6)]",
        "[3.1415926536, Float, 180.0, 57.29578]"
    );
}

#[test]
fn length_behaves_as_a_float() {
    check!("(1.m + 1.m).class", "Float");
    check!("[(2.m * 3).class, (1.0 + 1.m).class, (1.m / 2).class, -1.m.abs.class]", "[Float, Float, Float, Float]");
    check!(
        "[1.m.round(2), 1.5.inch.floor, 1.5.inch.ceil, 1.5.inch.to_i, 2.5.inch.zero?, 0.inch.zero?]",
        "[39.37, 1, 2, 1, false, true]"
    );
    check!("Math.sqrt(4.inch)", "2.0");
    check!("Length.new(3).inspect", "\"3.0\"");
    check!("[Length.new(3).hash == 3.0.hash, Length.new(3).frozen?]", "[true, true]");
    check!(
        "[1.m === 1.m, 1.m === 39.37007874015748, Float === 1.m, Length === 1.m, Length === 1.0]",
        "[true, true, false, true, false]"
    );
    check!("JSON.generate([1.mm])", "\"[0.03937007874015748]\"");
    check!("[1.m.kind_of?(Float), 1.m.instance_of?(Length)]", "[true, true]");
}

#[test]
fn length_comparisons_use_the_tolerance() {
    check!(
        "[1.inch == 1.0005, 1.inch == 1.002, 1.inch < 1.0005, 1.inch <= 1.0005, 1.inch > 1.002, 1.inch >= 1.0005, 2.inch > 1]",
        "[true, false, false, true, false, true, true]"
    );
    check!("[1.inch <=> 1, 1.inch <=> 2, 2.inch <=> 1, 1.inch <=> 'a', 1.inch <=> nil]", "[0, -1, 1, nil, nil]");
    check!("[0.5, 3, 1.inch, 2.inch].max.class", "Integer");
    raises!("1.inch == 'a'", "ArgumentError: comparison of Length with String failed");
    raises!("1.inch < nil", "ArgumentError: comparison of Length with nil failed");
    raises!("1.inch >= :a", "ArgumentError: comparison of Length with Symbol failed");
}

#[test]
fn length_constants() {
    check!("[Length::Decimal, Length::Architectural, Length::Engineering, Length::Fractional]", "[0, 1, 2, 3]");
    check!(
        "[Length::Inches, Length::Feet, Length::Millimeter, Length::Centimeter, Length::Meter, Length::Yard]",
        "[0, 1, 2, 3, 4, 5]"
    );
    check!(
        "[Length::SquareInches, Length::SquareFeet, Length::SquareMillimeter, Length::SquareCentimeter, Length::SquareMeter, Length::SquareYard]",
        "[0, 1, 2, 3, 4, 5]"
    );
    check!(
        "[Length::CubicInches, Length::CubicFeet, Length::CubicMillimeter, Length::CubicCentimeter, Length::CubicMeter, Length::CubicYard, Length::Liter, Length::USGallon]",
        "[0, 1, 2, 3, 4, 5, 6, 7]"
    );
}

#[test]
fn length_to_s_follows_the_model_units() {
    // The default model is in millimetres, precision 0: SketchUp rounds, marks rounded values with
    // "~" and puts a space before metric units.
    check!(
        "[1.m.to_s, 25.4.mm.to_s, 1.5.mm.to_s, 0.to_l.to_s, -2.cm.to_s, -25.4.mm.to_s]",
        "[\"1000 mm\", \"~ 25 mm\", \"~ 2 mm\", \"0 mm\", \"-20 mm\", \"~ -25 mm\"]"
    );
    // Point3d#to_s: precision 6, trailing zeros and "~" stripped
    check!("Geom::Point3d.new(1.m, 2.mm, 0).to_s", "\"(1000 mm, 2 mm, 0 mm)\"");
    check!("Geom::Point3d.new(1.m, 25.4.mm, 0.5.mm).to_s", "\"(1000 mm, 25.4 mm, 0.5 mm)\"");
}

#[test]
fn string_to_l_parses_lengths() {
    check!(
        "['1m', '12\"', '3\\'', '10cm', '5mm', '2 ft', '1km', '1yd'].map { |s| s.to_l.to_f.round(6) }",
        "[39.370079, 12.0, 36.0, 3.937008, 0.19685, 24.0, 39370.07874, 36.0]"
    );
    // bare numbers use the model unit (mm), commas are decimal separators
    check!("['10', '1,5', '.5', '-3'].map { |s| s.to_l.to_f.round(6) }", "[0.393701, 0.059055, 0.019685, -0.11811]");
    check!(
        "[\"1'6\\\"\", \"1'-6\\\"\", \"1' 6\", \"2'\", \"1/2\\\"\", \"3 1/2\\\"\"].map { |s| s.to_l.to_f }",
        "[18.0, 18.0, 18.0, 24.0, 0.5, 3.5]"
    );
    check!("'1m'.to_l.class", "Length");
    // what Length#to_s writes reads back
    check!(
        "['~ 25 mm', '~ -25 mm', '~25 mm', '1000 mm', \"1' 6 1/2\\\"\"].map { |s| s.to_l.to_f.round(6) }",
        "[0.984252, -0.984252, 0.984252, 39.370079, 18.5]"
    );
    check!("'-1\\'6\"'.to_l.to_f", "-18.0");
    raises!("'abc'.to_l", "ArgumentError: Cannot convert \"abc\" to Length");
    raises!("'1xyz'.to_l", "ArgumentError: Cannot convert \"1xyz\" to Length");
}

#[test]
fn length_formats() {
    // Esq::Units.format_length(inches, settings)
    let s = |fmt: i32, unit: i32, prec: i32, v: &str| {
        format!("Esq::Units.format_length({v}, {{unit: {unit}, format: {fmt}, precision: {prec}, suppress: false}})")
    };
    // decimal: fixed number of decimals, unit after a space for metric units
    check!(&s(0, 0, 3, "1.5"), "\"1.500\\\"\"");
    check!(&s(0, 1, 3, "18"), "\"1.500'\"");
    check!(&s(0, 3, 2, "1"), "\"2.54 cm\"");
    check!(&s(0, 4, 3, "39.37007874"), "\"1.000 m\"");
    check!(&s(0, 2, 0, "1"), "\"~ 25 mm\"");
    check!(&s(0, 2, 0, "-1"), "\"~ -25 mm\"");
    check!(&s(0, 0, 3, "-1.5"), "\"-1.500\\\"\"");
    check!(&s(0, 0, 3, "-0.0001"), "\"0.000\\\"\"");
    // architectural: feet and fractional inches separated by a space
    check!(&s(1, 0, 4, "18.5"), "\"1' 6 1/2\\\"\"");
    check!(&s(1, 0, 4, "24"), "\"2'\"");
    check!(&s(1, 0, 4, "6"), "\"6\\\"\"");
    check!(&s(1, 0, 4, "0.5"), "\"1/2\\\"\"");
    check!(&s(1, 0, 4, "0"), "\"0\\\"\"");
    check!(&s(1, 0, 4, "18.53"), "\"~ 1' 6 1/2\\\"\"");
    check!(
        "Esq::Units.format_length(24, {unit: 0, format: 1, precision: 4, suppress: false, force_inch: true})",
        "\"2' 0\\\"\""
    );
    // engineering: decimal feet
    check!(&s(2, 0, 2, "18"), "\"1.50'\"");
    check!(&s(2, 0, 2, "18.5"), "\"~ 1.54'\"");
    // fractional inches
    check!(&s(3, 0, 4, "18.5"), "\"18 1/2\\\"\"");
    check!(&s(3, 0, 4, "0"), "\"0\\\"\"");
    check!(&s(3, 0, 4, "0.25"), "\"1/4\\\"\"");
    check!("Esq::Units.format_length(2.5, {unit: 2, format: 0, precision: 3, suppress: true})", "\"63.500\"");
    // strip (Point3d#to_s): no "~", no trailing zeros
    check!(
        "Esq::Units.format_length(1.5, {unit: 0, format: 0, precision: 3, suppress: false}, strip: true)",
        "\"1.5\\\"\""
    );
    // SketchUp n'ôte que le « ~ » de « ~ 25 mm » : l'espace reste.
    check!(
        "Esq::Units.format_length(1, {unit: 2, format: 0, precision: 0, suppress: false}, strip: true)",
        "\" 25 mm\""
    );
}

// ------------------------------------------------------------------ Point3d
#[test]
fn point3d_construction() {
    check!(
        "[Geom::Point3d.new, Geom::Point3d.new(1, 2), Geom::Point3d.new(1, 2, 3), Geom::Point3d.new([4, 5, 6]), Geom::Point3d.new([4, 5])].map(&:to_a)",
        "[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [4.0, 5.0, 0.0]]"
    );
    check!("Geom::Point3d.new(Geom::Point3d.new(1, 2, 3)).to_a", "[1.0, 2.0, 3.0]");
    check!("Geom::Point3d.new(1.m, 0, 0).x.class", "Length");
    raises!("Geom::Point3d.new(1, 2, 3, 4)", "ArgumentError: wrong # of arguments(4 for 3)");
    raises!("Geom::Point3d.new([1])", "ArgumentError: wrong number of values in array");
    raises!("Geom::Point3d.new([1, 2, 3, 4])", "ArgumentError: wrong number of values in array");
    raises!("Geom::Point3d.new(5)", "ArgumentError: Cannot convert argument to Geom::Point3d");
    raises!(
        "Geom::Point3d.new(Geom::Vector3d.new(1, 2, 3))",
        "ArgumentError: Cannot convert argument to Geom::Point3d"
    );
    raises!("Geom::Point3d.new('a', 2, 3)", "TypeError: no implicit conversion to float from string");
    raises!("Geom::Point3d.new(nil, 2, 3)", "TypeError: can't convert nil into Float");
}

#[test]
fn point3d_accessors() {
    check!(
        "p = Geom::Point3d.new(1, 2, 3); [p.x.class, p.y.to_f, p[2].class, p[-1].to_f, p[0].to_f]",
        "[Length, 2.0, Length, 3.0, 1.0]"
    );
    check!("p = Geom::Point3d.new(1, 2, 3); p.x = 9; p[1] = 8; p[-1] = 7; p.to_a", "[9.0, 8.0, 7.0]");
    check!("p = Geom::Point3d.new; (p.x = 5).class", "Integer");
    raises!("Geom::Point3d.new[3]", "IndexError: index out of range [0,2]");
    raises!("Geom::Point3d.new[-4]", "IndexError: index out of range [0,2]");
    raises!("p = Geom::Point3d.new; p[3] = 1", "IndexError: index out of range [0,2]");
    raises!("Geom::Point3d.new.x = 'a'", "TypeError: no implicit conversion to float from string");
    raises!(
        "p = Geom::Point3d.new; p.freeze; p.x = 1",
        "FrozenError: can't modify frozen Geom::Point3d: Point3d(0, 0, 0)"
    );
}

#[test]
fn point3d_set_clone_equality() {
    check!("p = Geom::Point3d.new(1, 2, 3); [p.set!(4, 5, 6).equal?(p), p.to_a]", "[true, [4.0, 5.0, 6.0]]");
    // set! assigns only the coordinates given
    check!("p = Geom::Point3d.new(1, 2, 3); p.set!([7, 8]); p.to_a", "[7.0, 8.0, 3.0]");
    check!("v = Geom::Vector3d.new(1, 2, 3); v.set!(7); v.to_a", "[7.0, 2.0, 3.0]");
    check!("p = Geom::Point3d.new(1, 2, 3); p.set!(Geom::Point3d.new(9, 9, 9)); p.to_a", "[9.0, 9.0, 9.0]");
    check!("p = Geom::Point3d.new(1, 2, 3); p.set!(5); p.to_a", "[5.0, 2.0, 3.0]");
    raises!("Geom::Point3d.new.set!", "ArgumentError: wrong number of arguments");
    check!(
        "p = Geom::Point3d.new(1, 2, 3); q = p.clone; q.x = 9; [p.x.to_f, q.x.to_f, p.equal?(q), p.freeze.frozen?, p.clone.frozen?]",
        "[1.0, 9.0, false, true, false]"
    );
    check!(
        "a = Geom::Point3d.new(1, 2, 3); [a == Geom::Point3d.new(1, 2, 3), a == Geom::Point3d.new(1.0005, 2, 3), a == Geom::Point3d.new(1.01, 2, 3), a == [1, 2, 3], a == [1, 2], a == 5, a == nil, a == Geom::Vector3d.new(1, 2, 3)]",
        "[true, true, false, true, false, false, false, false]"
    );
    check!(
        "[Geom::Point3d.new(0, 0, 0) < Geom::Point3d.new(1, 0, 0), Geom::Point3d.new(1, 0, 0) < Geom::Point3d.new(1, 0, 0), Geom::Point3d.new(1, 1, 0) < Geom::Point3d.new(1, 0, 5), Geom::Point3d.new(1, 1, 0) < [1, 2, 0]]",
        "[true, false, false, true]"
    );
}

#[test]
fn point3d_inspect_to_s() {
    check!("Geom::Point3d.new(1, 2.5, 3).inspect", "\"Point3d(1, 2.5, 3)\"");
    check!("Geom::Point3d.new(1.23456789, 1e-12, -0.0).inspect", "\"Point3d(1.23457, 0, 0)\"");
    check!("Geom::Point3d.new(1000000, 0, 0).inspect", "\"Point3d(1e+06, 0, 0)\"");
}

#[test]
fn point3d_arithmetic() {
    check!("(Geom::Point3d.new(1, 2, 3) + Geom::Vector3d.new(1, 1, 1)).inspect", "\"Point3d(2, 3, 4)\"");
    check!("(Geom::Point3d.new(1, 2, 3) + [1, 1, 1]).inspect", "\"Point3d(2, 3, 4)\"");
    check!("(Geom::Point3d.new(1, 2, 3) - Geom::Vector3d.new(1, 1, 1)).inspect", "\"Point3d(0, 1, 2)\"");
    check!("(Geom::Point3d.new(4, 5, 6) - Geom::Point3d.new(1, 2, 3)).inspect", "\"Vector3d(3, 3, 3)\"");
    check!("(Geom::Point3d.new(4, 5, 6) - [1, 2, 3]).inspect", "\"Vector3d(3, 3, 3)\"");
    raises!("Geom::Point3d.new + Geom::Point3d.new", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    check!("Geom::Point3d.new(1, 2, 3).vector_to([4, 6, 3]).inspect", "\"Vector3d(3, 4, 0)\"");
    check!("Geom::Point3d.new(0, 0, 0).distance([3, 4, 0]).class", "Length");
    check!("Geom::Point3d.new(0, 0, 0).distance(Geom::Point3d.new(3, 4, 0)).to_f", "5.0");
    raises!("Geom::Point3d.new.distance(5)", "ArgumentError: Cannot convert argument to Geom::Point3d");
    check!("Geom::Point3d.linear_combination(0.5, [0, 0, 0], 0.5, [2, 4, 6]).inspect", "\"Point3d(1, 2, 3)\"");
}

#[test]
fn point3d_offset() {
    check!("Geom::Point3d.new(1, 1, 1).offset([0, 0, 1]).inspect", "\"Point3d(1, 1, 2)\"");
    check!("Geom::Point3d.new(0, 0, 0).offset(Geom::Vector3d.new(3, 0, 4), 10).inspect", "\"Point3d(6, 0, 8)\"");
    check!("p = Geom::Point3d.new; [p.offset!([1, 2, 3]).equal?(p), p.to_a]", "[true, [1.0, 2.0, 3.0]]");
    raises!("Geom::Point3d.new.offset", "ArgumentError: Wrong number of arguments: requires 1 or 2");
    raises!("Geom::Point3d.new.offset([0, 0, 0], 5)", "TypeError: Zero length vector");
    raises!("Geom::Point3d.new.offset!([0, 0, 0], 5)", "TypeError: Zero length vector");
    raises!("Geom::Point3d.new.offset(Geom::Point3d.new)", "ArgumentError: Cannot convert argument to Geom::Vector3d");
}

#[test]
fn point3d_lines_and_planes() {
    let line = "[Geom::Point3d.new(0, 0, 0), Geom::Vector3d.new(0, 0, 1)]";
    check!(&format!("Geom::Point3d.new(3, 4, 5).distance_to_line({line})"), "5.0");
    check!(
        "Geom::Point3d.new(3, 4, 5).distance_to_line(Geom::Point3d.new(0, 0, 0), Geom::Vector3d.new(0, 0, 2))",
        "5.0"
    );
    check!("Geom::Point3d.new(3, 4, 5).distance_to_line([0, 0, 0], [0, 0, 7])", "5.0");
    check!("Geom::Point3d.new(0, 0, 5).on_line?([[0, 0, 0], [0, 0, 1]])", "true");
    check!("Geom::Point3d.new(0.0005, 0, 5).on_line?([[0, 0, 0], [0, 0, 1]])", "true");
    check!("Geom::Point3d.new(1, 0, 5).on_line?([[0, 0, 0], [0, 0, 1]])", "false");
    check!(&format!("Geom::Point3d.new(3, 4, 5).project_to_line({line}).inspect"), "\"Point3d(0, 0, 5)\"");
    check!("Geom::Point3d.new(3, 4, 5).distance_to_plane([[0, 0, 0], [0, 0, 1]])", "5.0");
    check!("Geom::Point3d.new(3, 4, -5).distance_to_plane([0, 0, 1, 0])", "5.0");
    check!("Geom::Point3d.new(3, 4, 5).distance_to_plane([0, 0, 0], [0, 0, 3])", "5.0");
    check!("Geom::Point3d.new(3, 4, 5).distance_to_plane([0, 0, 0], [1, 0, 0], [0, 1, 0])", "5.0");
    check!("Geom::Point3d.new(3, 4, 5).distance_to_plane(0, 0, 1, -5)", "0.0");
    check!("Geom::Point3d.new(3, 4, 5).on_plane?([0, 0, 1, -5])", "true");
    check!("Geom::Point3d.new(3, 4, 5).on_plane?([0, 0, 1, 0])", "false");
    check!("Geom::Point3d.new(3, 4, 5).project_to_plane([0, 0, 1, 0]).inspect", "\"Point3d(3, 4, 0)\"");
    raises!("Geom::Point3d.new.distance_to_line(1)", "TypeError: wrong argument type Integer (expected Array)");
    raises!("Geom::Point3d.new.distance_to_line(1, 2)", "ArgumentError: Cannot convert argument to Geom::Point3d");
    raises!("Geom::Point3d.new.distance_to_plane(1, 2, 3, 4, 5)", "ArgumentError: Wrong number of arguments for plane");
}

#[test]
fn point3d_transform() {
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); Geom::Point3d.new(1, 1, 1).transform(t).inspect",
        "\"Point3d(2, 3, 4)\""
    );
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); p = Geom::Point3d.new(1, 1, 1); [p.transform!(t).equal?(p), p.to_a]",
        "[true, [2.0, 3.0, 4.0]]"
    );
    raises!("Geom::Point3d.new.transform('a')", "TypeError: no implicit conversion to Transformation");
    // a point, a vector, 16 numbers or a number stand for a transformation
    check!(
        "p = Geom::Point3d.new(1, 1, 1); [p.transform([1, 2, 3]).to_a, p.transform(2).to_a, p.transform(Geom::Vector3d.new(0, 0, 1)).to_a, Geom::Vector3d.new(1, 0, 0).transform(3).to_a, [1, 1, 1].transform([0, 0, 1])]",
        "[[2.0, 3.0, 4.0], [2.0, 2.0, 2.0], [1.0, 1.0, 2.0], [3.0, 0.0, 0.0], [1.0, 1.0, 2.0]]"
    );
}

// ------------------------------------------------------------------ Vector3d
#[test]
fn vector3d_construction() {
    check!(
        "[Geom::Vector3d.new, Geom::Vector3d.new(1, 2), Geom::Vector3d.new(1, 2, 3), Geom::Vector3d.new([4, 5, 6]), Geom::Vector3d.new([4, 5]), Geom::Vector3d.new(Geom::Vector3d.new(7, 8, 9))].map(&:to_a)",
        "[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [4.0, 5.0, 0.0], [7.0, 8.0, 9.0]]"
    );
    // two points: the vector from the first to the second
    check!("Geom::Vector3d.new(Geom::Point3d.new(1, 1, 1), Geom::Point3d.new(2, 3, 4)).to_a", "[1.0, 2.0, 3.0]");
    check!("Geom::Vector3d.new([1, 1, 1], [2, 3, 4]).to_a", "[1.0, 2.0, 3.0]");
    raises!("Geom::Vector3d.new(1, 2, 3, 4)", "ArgumentError: wrong # of arguments(4 for 3)");
    raises!("Geom::Vector3d.new(Geom::Point3d.new)", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    raises!("Geom::Vector3d.new(5)", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    raises!("Geom::Vector3d.new([1])", "ArgumentError: wrong number of values in array");
}

#[test]
fn vector3d_accessors() {
    check!(
        "v = Geom::Vector3d.new(1, 2, 3); [v.x, v.y, v.z, v[0], v[-1], v.x.class, v.length.class, v.to_a]",
        "[1.0, 2.0, 3.0, 1.0, 3.0, Float, Length, [1.0, 2.0, 3.0]]"
    );
    check!("v = Geom::Vector3d.new; v.x = 4; v[1] = 5; v[-1] = 6; v.to_a", "[4.0, 5.0, 6.0]");
    check!("v = Geom::Vector3d.new(1, 2, 3); [v.set!(4, 5, 6).equal?(v), v.to_a]", "[true, [4.0, 5.0, 6.0]]");
    check!("v = Geom::Vector3d.new; v.set!([7, 8, 9]); v.to_a", "[7.0, 8.0, 9.0]");
    check!("v = Geom::Vector3d.new; v.set!(Geom::Vector3d.new(1, 1, 1)); v.to_a", "[1.0, 1.0, 1.0]");
    raises!("Geom::Vector3d.new[3]", "IndexError: index out of range [0,2]");
    raises!(
        "v = Geom::Vector3d.new; v.freeze; v.x = 1",
        "FrozenError: can't modify frozen Geom::Vector3d: Vector3d(0, 0, 0)"
    );
    check!("v = Geom::Vector3d.new(1, 2, 3); w = v.clone; w.x = 9; [v.x, w.x]", "[1.0, 9.0]");
}

#[test]
fn vector3d_products() {
    // SketchUp: `*` is the cross product, `%` the dot product
    check!("(Geom::Vector3d.new(1, 0, 0) * Geom::Vector3d.new(0, 1, 0)).to_a", "[0.0, 0.0, 1.0]");
    check!("Geom::Vector3d.new(1, 2, 3) % Geom::Vector3d.new(4, 5, 6)", "32.0");
    check!("Geom::Vector3d.new(1, 2, 3).dot([4, 5, 6])", "32.0");
    check!("Geom::Vector3d.new(1, 0, 0).cross([0, 1, 0]).to_a", "[0.0, 0.0, 1.0]");
    check!("Geom::Vector3d.new(1, 0, 0).dot(Geom::Vector3d.new(0, 1, 0)).class", "Float");
    raises!("Geom::Vector3d.new.dot(5)", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    raises!("Geom::Vector3d.new.cross(Geom::Point3d.new)", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    raises!("Geom::Vector3d.new.dot([1])", "ArgumentError: wrong number of values in array");
    check!("(Geom::Vector3d.new(1, 2, 3) + Geom::Vector3d.new(1, 1, 1)).to_a", "[2.0, 3.0, 4.0]");
    check!("(Geom::Vector3d.new(1, 2, 3) - [1, 1, 1]).to_a", "[0.0, 1.0, 2.0]");
    raises!("Geom::Vector3d.new(1, 2, 3) * 2", "ArgumentError: Cannot convert argument to Geom::Vector3d");
    check!("Geom::Vector3d.linear_combination(2, [1, 0, 0], 3, Geom::Vector3d.new(0, 1, 0)).to_a", "[2.0, 3.0, 0.0]");
    check!("Geom::Vector3d.linear_combination(1, [1, 0, 0], 2, [0, 1, 0], 3, [0, 0, 1]).to_a", "[1.0, 2.0, 3.0]");
    raises!(
        "Geom::Vector3d.linear_combination(1, [1, 0, 0], 2)",
        "ArgumentError: wrong number arguments: 4 or 6 required"
    );
}

#[test]
fn vector3d_length_and_normalize() {
    check!(
        "v = Geom::Vector3d.new(3, 4, 0); [v.length.to_f, v.valid?, v.unitvector?, v.normalize.length.to_f, v.normalize.unitvector?]",
        "[5.0, true, false, 1.0, true]"
    );
    check!("v = Geom::Vector3d.new(3, 4, 0); v.length = 10; v.to_a", "[6.0, 8.0, 0.0]");
    check!("v = Geom::Vector3d.new(3, 4, 0); [v.normalize!.equal?(v), v.to_a]", "[true, [0.6, 0.8, 0.0]]");
    check!(
        "v = Geom::Vector3d.new(1, 2, 3); [v.reverse.to_a, v.reverse!.equal?(v), v.to_a]",
        "[[-1.0, -2.0, -3.0], true, [-1.0, -2.0, -3.0]]"
    );
    check!(
        "[Geom::Vector3d.new.valid?, Geom::Vector3d.new(0, 0, 0.5).valid?, Geom::Vector3d.new.normalize.to_a]",
        "[false, true, [0.0, 0.0, 0.0]]"
    );
    raises!("Geom::Vector3d.new.length = 5", "TypeError: Cannot set the length of a zero length vector)");
}

#[test]
fn vector3d_relations() {
    check!(
        "x = Geom::Vector3d.new(1, 0, 0); [x.parallel?([2, 0, 0]), x.parallel?([-1, 0, 0]), x.parallel?([0, 1, 0]), x.perpendicular?([0, 1, 0]), x.perpendicular?([1, 1, 0])]",
        "[true, true, false, true, false]"
    );
    check!(
        "x = Geom::Vector3d.new(1, 0, 0); [x.samedirection?([2, 0, 0]), x.samedirection?([-1, 0, 0]), x.samedirection?([0, 1, 0])]",
        "[true, false, false]"
    );
    check!("Geom::Vector3d.new(1, 0, 0).angle_between([0, 1, 0]).round(10)", "1.5707963268");
    check!("Geom::Vector3d.new(1, 0, 0).angle_between([-1, 0, 0]).round(10)", "3.1415926536");
    raises!(
        "Geom::Vector3d.new(1, 0, 0).parallel?([0, 0, 0])",
        "ArgumentError: Cannot create unit vector from zero length vector"
    );
    raises!(
        "Geom::Vector3d.new(1, 0, 0).perpendicular?(Geom::Vector3d.new)",
        "ArgumentError: Cannot create unit vector from zero length vector"
    );
    raises!(
        "Geom::Vector3d.new(1, 0, 0).samedirection?(5)",
        "ArgumentError: Cannot convert argument to Geom::Vector3d"
    );
    check!(
        "a = Geom::Vector3d.new(1, 2, 3); [a == Geom::Vector3d.new(1, 2, 3), a == [1, 2, 3], a == Geom::Vector3d.new(1, 2, 4), a == 7, a == Geom::Point3d.new(1, 2, 3)]",
        "[true, true, false, false, false]"
    );
    check!(
        "[Geom::Vector3d.new(0, 0, 0) < Geom::Vector3d.new(1, 0, 0), Geom::Vector3d.new(1, 0, 0) < [1, 0, 0]]",
        "[true, false]"
    );
    check!("Geom::Vector3d.new(1, 2, 3).inspect", "\"Vector3d(1, 2, 3)\"");
    check!("Geom::Vector3d.new(1, 2.5, 1e-12).inspect", "\"Vector3d(1, 2.5, 0)\"");
    check!("Geom::Vector3d.new(1, 2.5, 3).to_s", "\"(1, 2.5, 3)\"");
}

#[test]
fn vector3d_axes_and_transform() {
    check!("Geom::Vector3d.new(0, 0, 1).axes.map(&:to_a)", "[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]");
    check!(
        "x, y, z = Geom::Vector3d.new(1, 1, 1).axes; [(x % y).abs < 1e-9, (y % z).abs < 1e-9, x.unitvector?, y.unitvector?, z.unitvector?]",
        "[true, true, true, true, true]"
    );
    raises!("Geom::Vector3d.new.axes", "ArgumentError: Cannot create unit vector from zero length vector");
    // vectors ignore the translation of a transformation
    check!(
        "Geom::Vector3d.new(1, 0, 0).transform(Geom::Transformation.translation([5, 5, 5])).to_a",
        "[1.0, 0.0, 0.0]"
    );
    check!(
        "v = Geom::Vector3d.new(1, 0, 0); [v.transform!(Geom::Transformation.scaling(2)).equal?(v), v.to_a]",
        "[true, [2.0, 0.0, 0.0]]"
    );
    check!(
        "[X_AXIS.to_a, Y_AXIS.to_a, Z_AXIS.to_a, ORIGIN.to_a, Geom::X_AXIS.equal?(X_AXIS)]",
        "[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0], true]"
    );
}

// ------------------------------------------------------------------ Transformation
#[test]
fn transformation_basics() {
    check!(
        "t = Geom::Transformation.new; [t.identity?, t.to_a.size, t.origin.to_a, IDENTITY.identity?]",
        "[true, 16, [0.0, 0.0, 0.0], true]"
    );
    check!("Geom::Transformation.new([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1]).origin.to_a", "[5.0, 6.0, 7.0]");
    check!("Geom::Transformation.new([5, 6, 7]).origin.to_a", "[5.0, 6.0, 7.0]");
    check!("Geom::Transformation.new(Geom::Point3d.new(5, 6, 7)).origin.to_a", "[5.0, 6.0, 7.0]");
    check!("Geom::Transformation.new(Geom::Transformation.translation([1, 2, 3])).origin.to_a", "[1.0, 2.0, 3.0]");
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); u = t.clone; u.set!(Geom::Transformation.new); [t.identity?, u.identity?]",
        "[false, true]"
    );
    check!(
        "Geom::Transformation.new.set!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1]).origin.to_a",
        "[1.0, 1.0, 1.0]"
    );
    raises!("Geom::Transformation.new(1, 2, 3, 4, 5)", "ArgumentError: Wrong number of arguments: 0, 1 or 3");
    raises!("Geom::Transformation.new('a')", "TypeError: no implicit conversion to float from string");
    check!("Geom::Transformation.new(2).to_a.values_at(0, 5, 10, 15)", "[2.0, 2.0, 2.0, 1.0]");
}

#[test]
fn transformation_constructors() {
    // (origin, zaxis)
    check!(
        "t = Geom::Transformation.new([1, 2, 3], [0, 0, 1]); [t.origin.to_a, t.xaxis.to_a, t.yaxis.to_a, t.zaxis.to_a]",
        "[[1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]"
    );
    // (origin, xaxis, yaxis): y is made perpendicular to x
    check!(
        "t = Geom::Transformation.new([0, 0, 0], [1, 0, 0], [1, 1, 0]); [t.xaxis.to_a, t.yaxis.to_a, t.zaxis.to_a]",
        "[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]"
    );
    // (origin, axis, angle) = rotation
    check!(
        "t = Geom::Transformation.new(ORIGIN, Z_AXIS, 90.degrees); (t * Geom::Point3d.new(1, 0, 0)).to_a.map { |v| v.round(9) }",
        "[0.0, 1.0, 0.0]"
    );
    // (xaxis, yaxis, zaxis, origin)
    check!(
        "t = Geom::Transformation.new([0, 1, 0], [-1, 0, 0], [0, 0, 1], [5, 5, 5]); [(t * Geom::Point3d.new(1, 0, 0)).to_a]",
        "[[5.0, 6.0, 5.0]]"
    );
    check!("Geom::Transformation.axes([1, 2, 3], [1, 0, 0], [0, 1, 0], [0, 0, 1]).origin.to_a", "[1.0, 2.0, 3.0]");
    check!(
        "t = Geom::Transformation.axes([0, 0, 0], [0, 1, 0], [0, 0, 1]); [t.xaxis.to_a, t.zaxis.to_a]",
        "[[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]"
    );
    raises!(
        "Geom::Transformation.axes([0, 0, 0], [1, 0, 0])",
        "ArgumentError: Wrong number of arguments: 3 or 4 required"
    );
    raises!(
        "Geom::Transformation.axes([0, 0, 0], [1, 0, 0], [2, 0, 0])",
        "ArgumentError: Zero length vector for zaxis"
    );
}

#[test]
fn transformation_rotation_scaling_translation() {
    check!(
        "t = Geom::Transformation.rotation([1, 1, 0], Z_AXIS, 90.degrees); (t * Geom::Point3d.new(2, 1, 0)).to_a.map { |v| v.round(9) }",
        "[1.0, 2.0, 0.0]"
    );
    check!("Geom::Transformation.translation(Geom::Vector3d.new(1, 2, 3)).origin.to_a", "[1.0, 2.0, 3.0]");
    check!("Geom::Transformation.translation([1, 2, 3]).origin.to_a", "[1.0, 2.0, 3.0]");
    check!("Geom::Transformation.scaling(2).to_a.values_at(0, 5, 10, 15)", "[2.0, 2.0, 2.0, 1.0]");
    check!("Geom::Transformation.scaling(2, 3, 4).to_a.values_at(0, 5, 10)", "[2.0, 3.0, 4.0]");
    check!("t = Geom::Transformation.scaling([1, 1, 1], 2); (t * Geom::Point3d.new(2, 2, 2)).to_a", "[3.0, 3.0, 3.0]");
    check!(
        "t = Geom::Transformation.scaling([1, 1, 1], 2, 3, 4); (t * Geom::Point3d.new(2, 2, 2)).to_a",
        "[3.0, 4.0, 5.0]"
    );
    raises!("Geom::Transformation.scaling", "ArgumentError: Wrong number of arguments");
    raises!("Geom::Transformation.scaling(1, 2, 3, 4, 5)", "ArgumentError: Wrong number of arguments");
}

#[test]
fn transformation_multiplication() {
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); (t * Geom::Point3d.new(1, 1, 1)).inspect",
        "\"Point3d(2, 3, 4)\""
    );
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); (t * Geom::Vector3d.new(1, 1, 1)).inspect",
        "\"Vector3d(1, 1, 1)\""
    );
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); (t * [1, 1, 1]).map { |v| [v.class, v] }",
        "[[Float, 2.0], [Float, 3.0], [Float, 4.0]]"
    );
    // order: a * b applies b first
    check!(
        "a = Geom::Transformation.translation([1, 0, 0]); b = Geom::Transformation.scaling(2); ((a * b) * Geom::Point3d.new(1, 0, 0)).to_a",
        "[3.0, 0.0, 0.0]"
    );
    check!(
        "a = Geom::Transformation.translation([1, 0, 0]); b = Geom::Transformation.scaling(2); ((b * a) * Geom::Point3d.new(1, 0, 0)).to_a",
        "[4.0, 0.0, 0.0]"
    );
    // planes
    check!("t = Geom::Transformation.translation([0, 0, 5]); t * [0, 0, 1, 0]", "[0.0, 0.0, 1.0, -5.0]");
    check!("t = Geom::Transformation.translation([0, 0, 5]); t * [[0, 0, 0], [0, 0, 1]]", "[0.0, 0.0, 1.0, -5.0]");
    raises!("Geom::Transformation.new * 5", "ArgumentError: Not a valid argument type");
    raises!("Geom::Transformation.new * 'a'", "ArgumentError: Not a valid argument type");
    raises!("Geom::Transformation.new * [1, 2, 3, 4, 5]", "ArgumentError: Not a valid argument type");
}

#[test]
fn transformation_inverse() {
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]) * Geom::Transformation.rotation(ORIGIN, Z_AXIS, 0.3); (t * t.inverse).to_a.map { |v| v.round(9) + 0.0 } == Geom::Transformation.new.to_a",
        "true"
    );
    check!(
        "t = Geom::Transformation.translation([1, 2, 3]); i = t.clone; [i.invert!.equal?(i), i.origin.to_a]",
        "[true, [-1.0, -2.0, -3.0]]"
    );
    raises!("Geom::Transformation.scaling(0).inverse", "ArgumentError: Transformation is not invertible");
    raises!("Geom::Transformation.scaling(0).invert!", "ArgumentError: Transformation is not invertible");
    check!(
        "t = Geom::Transformation.scaling(2, 3, 4); [t.xaxis.to_a, t.yaxis.to_a, t.zaxis.to_a]",
        "[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]"
    );
}

#[test]
fn transformation_interpolate() {
    check!(
        "a = Geom::Transformation.translation([0, 0, 0]); b = Geom::Transformation.translation([10, 0, 0]); Geom::Transformation.interpolate(a, b, 0.25).origin.to_a",
        "[2.5, 0.0, 0.0]"
    );
    check!(
        "a = Geom::Transformation.new; b = Geom::Transformation.rotation(ORIGIN, Z_AXIS, 90.degrees); t = Geom::Transformation.interpolate(a, b, 0.5); (t * Geom::Point3d.new(1, 0, 0)).to_a.map { |v| v.round(6) }",
        "[0.707107, 0.707107, 0.0]"
    );
    // weights near 0/1 copy the ends; outside 0..1 SketchUp leaves the identity
    check!(
        "a = Geom::Transformation.translation([1, 0, 0]); b = Geom::Transformation.translation([9, 0, 0]); [0.0005, 0.9995, 1.5, -0.1].map { |w| Geom::Transformation.interpolate(a, b, w).origin.x.to_f }",
        "[1.0, 9.0, 0.0, 0.0]"
    );
    raises!("Geom::Transformation.interpolate('a', 2, 0.5)", "TypeError: no implicit conversion to Transformation");
    check!("Geom::Transformation.new.set!([1, 2, 3]).origin.to_a", "[1.0, 2.0, 3.0]");
}

// ------------------------------------------------------------------ BoundingBox
#[test]
fn bounding_box_basics() {
    check!(
        "b = Geom::BoundingBox.new; [b.empty?, b.valid?, b.min.x.to_f, b.max.x.to_f, b.width.to_f, b.diagonal.to_f, b.center.to_a]",
        "[true, false, 1.0e+30, -1.0e+30, 0.0, 0.0, [0.0, 0.0, 0.0]]"
    );
    check!(
        "b = Geom::BoundingBox.new; b.add([1, 2, 3], [4, 6, 9]); [b.empty?, b.valid?, b.min.to_a, b.max.to_a, b.width.to_f, b.height.to_f, b.depth.to_f, b.width.class, b.diagonal.to_f.round(6), b.center.to_a]",
        "[false, true, [1.0, 2.0, 3.0], [4.0, 6.0, 9.0], 3.0, 4.0, 6.0, Length, 7.81025, [2.5, 4.0, 6.0]]"
    );
    check!("b = Geom::BoundingBox.new; [b.add([1, 2, 3]).equal?(b), b.width.to_f, b.empty?]", "[true, 0.0, false]");
    check!("b = Geom::BoundingBox.new; b.add([1, 2, 3]); b.clear.equal?(b) && b.empty?", "true");
    raises!("Geom::BoundingBox.new.add", "ArgumentError: Wrong number of values - at least one required");
    raises!("Geom::BoundingBox.new.add(5)", "ArgumentError: Cannot convert argument to Geom::Point3d");
    raises!("Geom::BoundingBox.new(1)", "ArgumentError: wrong number of arguments (given 1, expected 0)");
}

#[test]
fn bounding_box_add_forms() {
    check!(
        "b = Geom::BoundingBox.new; b.add([[0, 0, 0], [1, 1, 1]]); [b.min.to_a, b.max.to_a]",
        "[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]"
    );
    check!(
        "b = Geom::BoundingBox.new; b.add([0, 0, 0]); b.add([-1, 2, 3]); [b.min.to_a, b.max.to_a]",
        "[[-1.0, 0.0, 0.0], [0.0, 2.0, 3.0]]"
    );
    check!(
        "a = Geom::BoundingBox.new.add([0, 0, 0], [1, 1, 1]); b = Geom::BoundingBox.new.add([5, 5, 5]); b.add(a); [b.min.to_a, b.max.to_a]",
        "[[0.0, 0.0, 0.0], [5.0, 5.0, 5.0]]"
    );
    check!(
        "b = Geom::BoundingBox.new.add(Geom::Point3d.new(1, 2, 3), Geom::Point3d.new(-1, -2, -3)); [b.min.to_a, b.max.to_a]",
        "[[-1.0, -2.0, -3.0], [1.0, 2.0, 3.0]]"
    );
    check!("b = Geom::BoundingBox.new.add([1, 1, 1]); b.add(Geom::BoundingBox.new); b.min.to_a", "[1.0, 1.0, 1.0]");
}

#[test]
fn bounding_box_corners_contains_intersect() {
    check!(
        "b = Geom::BoundingBox.new.add([0, 0, 0], [1, 2, 3]); (0..7).map { |i| b.corner(i).to_a }",
        "[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 3.0], [1.0, 0.0, 3.0], [0.0, 2.0, 3.0], [1.0, 2.0, 3.0]]"
    );
    check!(
        "b = Geom::BoundingBox.new.add([0, 0, 0], [1, 2, 3]); [b.corner(Geom::BoundingBox::CORNER_RIGHT_BACK_TOP).to_a, b.corner(Geom::BoundingBox::CORNER_LEFT_FRONT_BOTTOM).to_a]",
        "[[1.0, 2.0, 3.0], [0.0, 0.0, 0.0]]"
    );
    check!(
        "[Geom::BoundingBox::CORNER_LEFT_FRONT_BOTTOM, Geom::BoundingBox::CORNER_RIGHT_FRONT_BOTTOM, Geom::BoundingBox::CORNER_LEFT_BACK_BOTTOM, Geom::BoundingBox::CORNER_RIGHT_BACK_BOTTOM, Geom::BoundingBox::CORNER_LEFT_FRONT_TOP, Geom::BoundingBox::CORNER_RIGHT_FRONT_TOP, Geom::BoundingBox::CORNER_LEFT_BACK_TOP, Geom::BoundingBox::CORNER_RIGHT_BACK_TOP]",
        "[0, 1, 2, 3, 4, 5, 6, 7]"
    );
    raises!("Geom::BoundingBox.new.corner(8)", "IndexError: Invalid corner index");
    raises!("Geom::BoundingBox.new.corner(-1)", "IndexError: Invalid corner index");
    check!(
        "b = Geom::BoundingBox.new.add([0, 0, 0], [2, 2, 2]); [b.contains?([1, 1, 1]), b.contains?([2, 2, 2]), b.contains?([3, 1, 1]), b.contains?(Geom::Point3d.new(0, 0, 0))]",
        "[true, true, false, true]"
    );
    check!(
        "b = Geom::BoundingBox.new.add([0, 0, 0], [2, 2, 2]); [b.contains?(Geom::BoundingBox.new.add([1, 1, 1], [2, 2, 2])), b.contains?(Geom::BoundingBox.new.add([1, 1, 1], [3, 3, 3])), b.contains?(Geom::BoundingBox.new)]",
        "[true, false, false]"
    );
    check!(
        "a = Geom::BoundingBox.new.add([0, 0, 0], [2, 2, 2]); b = Geom::BoundingBox.new.add([1, 1, 1], [3, 3, 3]); i = a.intersect(b); [i.min.to_a, i.max.to_a, i.class]",
        "[[1.0, 1.0, 1.0], [2.0, 2.0, 2.0], Geom::BoundingBox]"
    );
    check!(
        "a = Geom::BoundingBox.new.add([0, 0, 0], [1, 1, 1]); b = Geom::BoundingBox.new.add([2, 2, 2], [3, 3, 3]); a.intersect(b).empty?",
        "true"
    );
    raises!("Geom::BoundingBox.new.intersect(5)", "TypeError: Wrong type - expected BoundingBox");
}

// ------------------------------------------------------------------ Array as point/vector
#[test]
fn array_point_and_vector_helpers() {
    check!("a = [1, 2, 3]; [a.x, a.y, a.z, [].x, [1].y]", "[1, 2, 3, nil, nil]");
    check!("a = [1, 2, 3]; a.x = 5; a.y = 6; a.z = 7; a", "[5, 6, 7]");
    check!("[[0, 0, 0].distance([3, 4, 0]).class, [0, 0, 0].distance([3, 4, 0]).to_f]", "[Length, 5.0]");
    check!("[1, 2, 3].vector_to([4, 6, 3]).class", "Geom::Vector3d");
    check!("[1, 2, 3].dot([4, 5, 6])", "32.0");
    check!("[1, 0, 0].cross([0, 1, 0]).class", "Geom::Vector3d");
    check!("[1, 0, 0].cross([0, 1, 0]).to_a", "[0.0, 0.0, 1.0]");
    check!("[3, 4, 0].normalize", "[0.6, 0.8, 0.0]");
    check!("a = [3, 4, 0]; [a.normalize!.equal?(a), a]", "[true, [0.6, 0.8, 0.0]]");
    check!("[1, 1, 1].offset([0, 0, 2])", "[1.0, 1.0, 3.0]");
    check!("a = [1, 1, 1]; a.offset!([0, 0, 2]); [a.map(&:class).uniq, a.map(&:to_f)]", "[[Length], [1.0, 1.0, 3.0]]");
    check!("[1, 1, 1].transform(Geom::Transformation.translation([1, 1, 1]))", "[2.0, 2.0, 2.0]");
    check!(
        "a = [1, 1, 1]; a.transform!(Geom::Transformation.translation([1, 1, 1])); [a.map(&:class).uniq, a.map(&:to_f)]",
        "[[Length], [2.0, 2.0, 2.0]]"
    );
    check!(
        "[[3, 4, 5].distance_to_line([[0, 0, 0], [0, 0, 1]]).class, [3, 4, 5].distance_to_line([[0, 0, 0], [0, 0, 1]]).to_f]",
        "[Length, 5.0]"
    );
    check!("[3, 4, 5].distance_to_plane([0, 0, 1, 0]).to_f", "5.0");
    check!("[[0, 0, 5].on_line?([[0, 0, 0], [0, 0, 1]]), [0, 1, 5].on_plane?([0, 0, 1, -5])]", "[true, true]");
    check!("[3, 4, 5].project_to_line([[0, 0, 0], [0, 0, 1]])", "[0.0, 0.0, 5.0]");
    check!("[3, 4, 5].project_to_plane([0, 0, 1, 0])", "[3.0, 4.0, 0.0]");
    check!("[1, 2].vector_to([4, 6]).class", "Geom::Vector2d");
    check!("[3, 4].normalize", "[0.6, 0.8]");
    check!("[1, 0].cross([0, 1])", "1.0");
    check!("[1, 2].dot([3, 4])", "11.0");
    check!("[[0, 0].distance([3, 4]).to_f, [1, 1].offset([1, 2])]", "[5.0, [2.0, 3.0]]");
    raises!("[1, 0, 0].offset", "ArgumentError: Wrong number of arguments: requires 1 or 2");
    raises!("[0, 0, 0].offset([0, 0, 0], 3)", "TypeError: Zero length vector");
}

// ------------------------------------------------------------------ Geom module functions
#[test]
fn geom_intersections() {
    check!("Geom.intersect_line_line([[0, 0, 0], [1, 0, 0]], [[1, -1, 0], [0, 1, 0]]).inspect", "\"Point3d(1, 0, 0)\"");
    check!("Geom.intersect_line_line([[0, 0, 0], [1, 0, 0]], [[0, 1, 0], [1, 0, 0]])", "nil");
    check!("Geom.intersect_line_line([[0, 0, 0], [1, 0, 0]], [[0, 0, 1], [0, 1, 0]])", "nil");
    check!(
        "Geom.intersect_line_line([Geom::Point3d.new(0, 0, 0), Geom::Vector3d.new(1, 0, 0)], [Geom::Point3d.new(2, 0, 0), Geom::Point3d.new(2, 5, 0)]).inspect",
        "\"Point3d(2, 0, 0)\""
    );
    check!("Geom.intersect_line_plane([[0, 0, 5], [0, 0, 1]], [0, 0, 1, 0]).inspect", "\"Point3d(0, 0, 0)\"");
    check!("Geom.intersect_line_plane([[1, 1, 5], [1, 0, 0]], [ORIGIN, Z_AXIS])", "nil");
    check!("Geom.intersect_line_plane([[1, 1, 5], [0, 0, -1]], [[0, 0, 2], Z_AXIS]).inspect", "\"Point3d(1, 1, 2)\"");
    check!(
        "l = Geom.intersect_plane_plane([0, 0, 1, 0], [1, 0, 0, 0]); [l[0].on_line?(l), l[1].to_a.map(&:abs), l[0].on_plane?([0, 0, 1, 0]), l[0].on_plane?([1, 0, 0, 0])]",
        "[true, [0.0, 1.0, 0.0], true, true]"
    );
    check!("Geom.intersect_plane_plane([0, 0, 1, 0], [0, 0, 1, -3])", "nil");
    check!(
        "a, b = Geom.closest_points([[0, 0, 0], [1, 0, 0]], [[0, 0, 5], [0, 1, 0]]); [a.to_a, b.to_a]",
        "[[0.0, 0.0, 0.0], [0.0, 0.0, 5.0]]"
    );
    check!("a, b = Geom.closest_points([[0, 0, 0], [1, 0, 0]], [[0, 3, 0], [1, 0, 0]]); [a.distance(b).to_f]", "[3.0]");
    raises!("Geom.intersect_line_line(1, 2)", "TypeError: wrong argument type Integer (expected Array)");
    // nearly-meeting lines: the midpoint of their closest points
    check!(
        "p = Geom.intersect_line_line([[0, 0, 0], [1, 0, 0]], [[1, -1, 0.0004], [0, 1, 0]]); [p.x.to_f, p.y.to_f, p.z.to_f.round(6)]",
        "[1.0, 0.0, 0.0002]"
    );
    check!("Geom.intersect_line_line([[0, 0, 0], [1, 0, 0]], [[1, -1, 0.002], [0, 1, 0]])", "nil");
    raises!(
        "Geom.intersect_line_line([[0, 0, 0], [0, 0, 0]], [[1, -1, 0], [0, 1, 0]])",
        "ArgumentError: Cannot create unit vector from zero length vector"
    );
}

#[test]
fn geom_fit_plane_and_combination() {
    check!(
        "pl = Geom.fit_plane_to_points([0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]); pl.map { |v| v.round(9) + 0.0 }",
        "[0.0, 0.0, 1.0, 0.0]"
    );
    check!(
        "pl = Geom.fit_plane_to_points([[0, 0, 3], [1, 0, 3], [0, 1, 3]]); pl.map { |v| v.round(9) + 0.0 }",
        "[0.0, 0.0, 1.0, -3.0]"
    );
    check!(
        "pl = Geom.fit_plane_to_points([0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]); pl.map { |v| v.round(9) + 0.0 }",
        "[0.0, 0.0, -1.0, 0.0]"
    );
    raises!(
        "Geom.fit_plane_to_points([0, 0, 0], [1, 0, 0])",
        "ArgumentError: At least three points needed to fit a plane"
    );
    check!("Geom.linear_combination(0.25, [0, 0, 0], 0.75, [4, 4, 4]).inspect", "\"Point3d(3, 3, 3)\"");
    check!(
        "Geom.linear_combination(2, Geom::Vector3d.new(1, 0, 0), 3, Geom::Vector3d.new(0, 1, 0)).inspect",
        "\"Vector3d(2, 3, 0)\""
    );
}

#[test]
fn geom_point_in_polygon_2d() {
    let sq = "[[0, 0, 0], [4, 0, 0], [4, 4, 0], [0, 4, 0]]";
    check!(
        &format!(
            "[Geom.point_in_polygon_2D([2, 2, 0], {sq}, true), Geom.point_in_polygon_2D([5, 2, 0], {sq}, true), Geom.point_in_polygon_2D([-1, -1, 9], {sq}, false)]"
        ),
        "[true, false, false]"
    );
    check!(
        &format!(
            "[Geom.point_in_polygon_2D([0, 2, 0], {sq}, true), Geom.point_in_polygon_2D([0, 2, 0], {sq}, false), Geom.point_in_polygon_2D([4, 4, 0], {sq}, true), Geom.point_in_polygon_2D([4, 4, 0], {sq}, false)]"
        ),
        "[true, false, true, false]"
    );
    check!(
        "Geom.point_in_polygon_2D(Geom::Point3d.new(1, 1, 7), [Geom::Point3d.new(0, 0, 0), Geom::Point3d.new(3, 0, 0), Geom::Point3d.new(0, 3, 0)], true)",
        "true"
    );
    // the border is 0.001" thick
    check!(
        &format!(
            "[Geom.point_in_polygon_2D([0.0005, 2, 0], {sq}, false), Geom.point_in_polygon_2D([-0.0005, 2, 0], {sq}, true), Geom.point_in_polygon_2D([0.002, 2, 0], {sq}, false), Geom.point_in_polygon_2D([4.0004, 4.0004, 0], {sq}, true)]"
        ),
        "[false, true, true, true]"
    );
    raises!("Geom.point_in_polygon_2D([0, 0, 0], 5, true)", "TypeError: wrong argument type Integer (expected Array)");
}

#[test]
fn geom_tesselate() {
    check!(
        "t = Geom.tesselate([0, 0, 0], [1, 0, 0], [1, 1, 0]) rescue t = Geom.tesselate([[0, 0, 0], [1, 0, 0], [1, 1, 0]]); t.size",
        "3"
    );
    check!(
        "t = Geom.tesselate([[0, 0, 0], [4, 0, 0], [4, 4, 0], [0, 4, 0]]); [t.size, t.all? { |p| p.is_a?(Geom::Point3d) }]",
        "[6, true]"
    );
    // area of the triangles = area of the square with a hole
    check!(
        "t = Geom.tesselate([[0, 0, 0], [6, 0, 0], [6, 6, 0], [0, 6, 0]], [[2, 2, 0], [2, 4, 0], [4, 4, 0], [4, 2, 0]]); a = t.each_slice(3).sum { |p, q, r| ((q - p).cross(r - p)).length.to_f / 2 }; a.round(9)",
        "32.0"
    );
    // triangles keep the orientation of the outer loop
    check!(
        "t = Geom.tesselate([[0, 0, 0], [4, 0, 0], [4, 4, 0], [0, 4, 0]]); t.each_slice(3).all? { |p, q, r| ((q - p).cross(r - p)).z > 0 }",
        "true"
    );
    check!(
        "t = Geom.tesselate([[0, 0, 0], [0, 4, 0], [4, 4, 0], [4, 0, 0]]); t.each_slice(3).all? { |p, q, r| ((q - p).cross(r - p)).z < 0 }",
        "true"
    );
    raises!(
        "Geom.tesselate([[0, 0, 0], [1, 0, 0]])",
        "ArgumentError: Loop cannot contain less than 3 points. Loop 0 had 2 points"
    );
    raises!(
        "Geom.tesselate([[0, 0, 0], [1, 0, 0], [0, 1, 0]], [[0, 0, 0]])",
        "ArgumentError: Loop cannot contain less than 3 points. Loop 1 had 1 points"
    );
    raises!("Geom.tesselate(5)", "TypeError: wrong argument type Integer (expected Array)");
}

// ------------------------------------------------------------------ 2D classes
#[test]
fn point2d_vector2d_basics() {
    check!(
        "[Geom::Point2d.new.to_a, Geom::Point2d.new(1, 2).to_a, Geom::Point2d.new([3, 4]).to_a, Geom::Point2d.new(Geom::Point2d.new(5, 6)).to_a]",
        "[[0.0, 0.0], [1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]"
    );
    check!(
        "p = Geom::Point2d.new(1, 2); [p.x.class, p.y.to_f, p[0].class, p[-1].to_f, p.inspect]",
        "[Length, 2.0, Length, 2.0, \"Point2d(1, 2)\"]"
    );
    check!("p = Geom::Point2d.new; p.x = 3; p[1] = 4; p.to_a", "[3.0, 4.0]");
    raises!("Geom::Point2d.new(1, 2, 3)", "ArgumentError: wrong # of arguments (3)");
    raises!("Geom::Point2d.new([1, 2, 3])", "ArgumentError: wrong number of values in array");
    raises!("Geom::Point2d.new(5)", "TypeError: Cannot convert argument to Geom::Point2d");
    raises!("Geom::Point2d.new.set!(1, 2, 3)", "ArgumentError: wrong # of arguments (3)");
    raises!("Geom::Point2d.new.distance(5)", "TypeError: Cannot convert argument to Geom::Point2d");
    raises!("Geom::Point2d.new[2]", "IndexError: index out of range [0,1]");
    raises!("Geom::Point2d.new[-3]", "IndexError: index out of range [0,1]");
    check!(
        "a = Geom::Point2d.new(1, 2); [a == Geom::Point2d.new(1, 2), a == [1, 2], a == Geom::Point2d.new(1, 3), a == 5]",
        "[true, true, false, false]"
    );
    check!("(Geom::Point2d.new(1, 2) + Geom::Vector2d.new(1, 1)).inspect", "\"Point2d(2, 3)\"");
    check!("(Geom::Point2d.new(1, 2) - Geom::Vector2d.new(1, 1)).inspect", "\"Point2d(0, 1)\"");
    check!("(Geom::Point2d.new(4, 6) - Geom::Point2d.new(1, 2)).inspect", "\"Vector2d(3, 4)\"");
    check!(
        "[Geom::Point2d.new(0, 0).distance([3, 4]).class, Geom::Point2d.new(0, 0).distance([3, 4]).to_f]",
        "[Length, 5.0]"
    );
    check!("Geom::Point2d.new(1, 1).vector_to([4, 5]).inspect", "\"Vector2d(3, 4)\"");
    check!("Geom::Point2d.new(1, 1).offset([3, 4], 10).to_a", "[7.0, 9.0]");
    check!("p = Geom::Point2d.new(1, 1); [p.offset!([1, 0]).equal?(p), p.to_a]", "[true, [2.0, 1.0]]");
    check!("p = Geom::Point2d.new(1, 1); p.set!(5, 6); p.to_a", "[5.0, 6.0]");
    check!("p = Geom::Point2d.new(1, 1); p.set!([7, 8]); p.to_a", "[7.0, 8.0]");
    check!("p = Geom::Point2d.new(1, 1); q = p.clone; q.x = 9; [p.x.to_f, q.x.to_f]", "[1.0, 9.0]");
    raises!("Geom::Point2d.new.offset", "ArgumentError: Wrong number of arguments: requires 1 or 2");
    raises!("Geom::Point2d.new.offset([0, 0], 3)", "ArgumentError: Zero distance is not allowed");
    raises!("Geom::Point2d.new.offset!([0, 0], 3)", "TypeError: Zero length vector)");
    check!("Geom::Point2d.new(1000, 2000).to_s", "\"(25400 mm, 50800 mm)\"");
    check!(
        "[ORIGIN_2D.to_a, X_AXIS_2D.to_a, Y_AXIS_2D.to_a, IDENTITY_2D.identity?]",
        "[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], true]"
    );
}

#[test]
fn vector2d_basics() {
    check!(
        "[Geom::Vector2d.new.to_a, Geom::Vector2d.new(1, 2).to_a, Geom::Vector2d.new([3, 4]).to_a, Geom::Vector2d.new(Geom::Vector2d.new(5, 6)).to_a, Geom::Vector2d.new(Geom::Point2d.new(1, 1), Geom::Point2d.new(4, 5)).to_a]",
        "[[0.0, 0.0], [1.0, 2.0], [3.0, 4.0], [5.0, 6.0], [3.0, 4.0]]"
    );
    check!(
        "v = Geom::Vector2d.new(3, 4); [v.x, v.y, v[0], v[-1], v.length.class, v.length.to_f, v.inspect, v.to_s]",
        "[3.0, 4.0, 3.0, 4.0, Length, 5.0, \"Vector2d(3, 4)\", \"(3, 4)\"]"
    );
    raises!("Geom::Vector2d.new(1, 2, 3)", "ArgumentError: wrong # of arguments(3)");
    raises!("Geom::Vector2d.new[2]", "IndexError: index out of range");
    raises!("Geom::Vector2d.new(5)", "TypeError: Cannot convert argument to Geom::Vector2d");
    check!("v = Geom::Vector2d.new; v.set!([1, 1], [4, 5]); v.to_a", "[3.0, 4.0]");
    check!("(Geom::Vector2d.new(1, 2) + [1, 1]).to_a", "[2.0, 3.0]");
    check!("(Geom::Vector2d.new(1, 2) - Geom::Vector2d.new(1, 1)).to_a", "[0.0, 1.0]");
    check!(
        "[Geom::Vector2d.new(1, 2) % Geom::Vector2d.new(3, 4), Geom::Vector2d.new(1, 2).dot([3, 4]), Geom::Vector2d.new(1, 2) * Geom::Vector2d.new(3, 4), Geom::Vector2d.new(1, 2).cross([3, 4])]",
        "[11.0, 11.0, -2.0, -2.0]"
    );
    check!("v = Geom::Vector2d.new(3, 4); v.length = 10; v.to_a", "[6.0, 8.0]");
    check!(
        "v = Geom::Vector2d.new(3, 4); [v.normalize.to_a, v.normalize!.equal?(v), v.to_a, v.unit_vector?, v.valid?]",
        "[[0.6, 0.8], true, [0.6, 0.8], true, true]"
    );
    check!(
        "v = Geom::Vector2d.new(1, 2); [v.reverse.to_a, v.reverse!.equal?(v), v.to_a]",
        "[[-1.0, -2.0], true, [-1.0, -2.0]]"
    );
    check!(
        "x = Geom::Vector2d.new(1, 0); [x.parallel?([3, 0]), x.parallel?([0, 1]), x.perpendicular?([0, 2]), x.same_direction?([2, 0]), x.same_direction?([-2, 0])]",
        "[true, false, true, true, false]"
    );
    check!("Geom::Vector2d.new(1, 0).angle_between([0, 1]).round(9)", "1.570796327");
    check!("v = Geom::Vector2d.new; v.set!(3, 4); v.set!([5, 6]); v.to_a", "[5.0, 6.0]");
    check!(
        "[Geom::Vector2d.new(1, 2) == [1, 2], Geom::Vector2d.new(1, 2) == Geom::Vector2d.new(2, 2)]",
        "[true, false]"
    );
    raises!("Geom::Vector2d.new.length = 3", "ArgumentError: Cannot set the length of a zero length vector");
    raises!(
        "Geom::Vector2d.new(1, 0).parallel?([0, 0])",
        "ArgumentError: Cannot create unit vector from zero length vector"
    );
    check!("Geom::Vector2d.new(1, 0).transform(Geom::Transformation2d.translation([5, 5])).to_a", "[1.0, 0.0]");
}

#[test]
fn transformation2d_basics() {
    check!("t = Geom::Transformation2d.new; [t.identity?, t.to_a]", "[true, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]]");
    check!("(Geom::Transformation2d.translation([1, 2]) * Geom::Point2d.new(1, 1)).inspect", "\"Point2d(2, 3)\"");
    check!(
        "(Geom::Transformation2d.translation(Geom::Vector2d.new(1, 2)) * Geom::Vector2d.new(1, 1)).inspect",
        "\"Vector2d(1, 1)\""
    );
    // In single precision like SketchUp: cosf(90°) is -4.37e-8 (SUTransformation2DRotation).
    check!(
        "(Geom::Transformation2d.rotation([0, 0], 90.degrees) * Geom::Point2d.new(1, 0)).to_a.map { |v| v.round(9) }",
        "[-4.4e-08, 1.0]"
    );
    check!(
        "(Geom::Transformation2d.rotation([1, 1], 90.degrees) * Geom::Point2d.new(2, 1)).to_a.map { |v| v.round(6) }",
        "[1.0, 2.0]"
    );
    check!("(Geom::Transformation2d.scaling(2) * [1, 1])", "[2.0, 2.0]");
    check!("(Geom::Transformation2d.scaling(2, 3) * Geom::Point2d.new(1, 1)).to_a", "[2.0, 3.0]");
    check!("(Geom::Transformation2d.scaling([1, 1], 2) * Geom::Point2d.new(2, 2)).to_a", "[3.0, 3.0]");
    check!("(Geom::Transformation2d.scaling([1, 1], 2, 3) * Geom::Point2d.new(2, 2)).to_a", "[3.0, 4.0]");
    check!(
        "a = Geom::Transformation2d.translation([1, 0]); b = Geom::Transformation2d.scaling(2); ((a * b) * Geom::Point2d.new(1, 0)).to_a",
        "[3.0, 0.0]"
    );
    check!(
        "t = Geom::Transformation2d.translation([1, 2]); u = t.inverse; [(u * t).identity?, t.clone.invert!.to_a == u.to_a, t == t.clone, t == u]",
        "[true, true, true, false]"
    );
    check!("Geom::Transformation2d.new([1, 0, 0, 1, 5, 6]).to_a", "[1.0, 0.0, 0.0, 1.0, 5.0, 6.0]");
    check!(
        "t = Geom::Transformation2d.new; t.set!(Geom::Transformation2d.translation([1, 1])); t.to_a",
        "[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]"
    );
    check!(
        "p = Geom::Point2d.new(1, 1); [p.transform!(Geom::Transformation2d.translation([1, 1])).equal?(p), p.to_a]",
        "[true, [2.0, 2.0]]"
    );
    raises!("Geom::Transformation2d.scaling", "ArgumentError: Wrong number of arguments");
    raises!("Geom::Transformation2d.new * 5", "TypeError: Not a valid argument type");
    raises!("Geom::Transformation2d.scaling(0).inverse", "ArgumentError: Transformation is not invertible");
    raises!("Geom::Point2d.new.transform(5)", "TypeError: wrong type - expected Geom::Transformation2d");
}

#[test]
fn bounds2d_basics() {
    check!(
        "b = Geom::Bounds2d.new; [b.upper_left.to_a, b.lower_right.to_a, b.width, b.height]",
        "[[0.0, 0.0], [0.0, 0.0], 0.0, 0.0]"
    );
    // Quatre nombres : x, y, largeur, hauteur.
    check!(
        "b = Geom::Bounds2d.new; b.set!(1, 2, 10, 5); [b.upper_left.to_a, b.lower_right.to_a, b.width, b.height, b.to_a.map(&:to_a)]",
        "[[1.0, 2.0], [11.0, 7.0], 10.0, 5.0, [[1.0, 2.0], [11.0, 7.0]]]"
    );
    check!(
        "b = Geom::Bounds2d.new; b.set!(Geom::Point2d.new(1, 2), Geom::Point2d.new(3, 4)); b.to_a.map(&:to_a)",
        "[[1.0, 2.0], [3.0, 4.0]]"
    );
    check!(
        "a = Geom::Bounds2d.new; a.set!(1, 2, 3, 4); b = Geom::Bounds2d.new; b.set!(1, 2, 3, 4); [a == b, a == Geom::Bounds2d.new, a.set!(0, 0, 1, 1).equal?(a)]",
        "[true, false, true]"
    );
    check!("Geom::Bounds2d.new(1, 2, 3, 4).width", "3.0");
}

// ------------------------------------------------------------------ PolygonMesh
#[test]
fn polygon_mesh_points_and_polygons() {
    check!(
        "m = Geom::PolygonMesh.new; [m.add_point([0, 0, 0]), m.add_point([1, 0, 0]), m.add_point([1, 1, 0]), m.add_point([1, 0, 0]), m.add_point([1, 0.0005, 0]), m.count_points]",
        "[1, 2, 3, 2, 2, 3]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_point([0, 0, 0]); m.add_point([1, 0, 0]); [m.point_index([1, 0, 0]), m.point_index([9, 9, 9]), m.point_index(Geom::Point3d.new(0, 0, 0))]",
        "[2, 0, 1]"
    );
    check!("m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0])", "1");
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([[0, 0, 0], [1, 0, 0], [1, 1, 0]]); m.add_polygon([0, 0, 0], [1, 1, 0], [0, 1, 0]); [m.count_points, m.count_polygons, m.polygons, m.polygon_at(1), m.polygon_at(2), m.polygon_at(0), m.polygon_at(3)]",
        "[4, 2, [[1, 2, 3], [1, 3, 4]], [1, 2, 3], [1, 3, 4], nil, nil]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]); m.add_polygon(1, -2, 3); m.polygon_at(2)",
        "[1, -2, 3]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); m.polygon_points_at(1).map(&:to_a)",
        "[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); [m.polygon_points_at(0), m.polygon_points_at(2)]",
        "[nil, nil]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); [m.point_at(1).to_a, m.point_at(-2).to_a, m.point_at(0), m.point_at(4), m.points.size, m.points[2].class]",
        "[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], nil, nil, 3, Geom::Point3d]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_point([0, 0, 0]); [m.set_point(1, [5, 5, 5]).equal?(m), m.point_at(1).to_a]",
        "[true, [5.0, 5.0, 5.0]]"
    );
    raises!("m = Geom::PolygonMesh.new; m.set_point(1, [0, 0, 0])", "IndexError: Index out of range");
    raises!("m = Geom::PolygonMesh.new; m.add_point", "ArgumentError: Not enough arguments: Point required");
    raises!("Geom::PolygonMesh.new.add_polygon", "ArgumentError: Not enough points - at least 3 required");
    raises!(
        "Geom::PolygonMesh.new.add_polygon([0, 0, 0], [1, 0, 0])",
        "ArgumentError: Not enough points - at least 3 required"
    );
    raises!("m = Geom::PolygonMesh.new; m.add_polygon(1, 2, 3)", "ArgumentError: point index 1 out of range");
    raises!("Geom::PolygonMesh.new(-1)", "RangeError: Number of points and/or polygons must be 0 or greater");
    check!("[Geom::PolygonMesh.new(10).count_points, Geom::PolygonMesh.new(10, 5).count_polygons]", "[0, 0]");
}

#[test]
fn polygon_mesh_uvs_normals_transform() {
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); [m.uvs(true).size, m.uvs(false).size, m.uv_at(1, true).to_a, m.uv_at(0, true), m.uv_at(4, true)]",
        "[3, 3, [0.0, 0.0, 0.0], nil, nil]"
    );
    // The side is required (TestUp TC_Geom_PolygonMesh#test_uv_at_too_few_arguments,
    // #test_uv_at_invalid_arguments, #test_uvs_too_few_arguments, #test_set_uv_too_few_arguments).
    for (code, want) in [
        ("m.uvs", "ArgumentError: wrong number of arguments (given 0, expected 1)"),
        ("m.set_uv(1, [0, 0, 1])", "ArgumentError: wrong number of arguments (given 2, expected 3)"),
        ("m.set_uv(1)", "ArgumentError: wrong number of arguments (given 1, expected 3)"),
        ("m.uv_at(1)", "ArgumentError: wrong number of arguments (given 1, expected 2)"),
        ("m.uv_at([1, true])", "ArgumentError: wrong number of arguments (given 1, expected 2)"),
        ("m.uv_at(1, true, true)", "ArgumentError: wrong number of arguments (given 3, expected 2)"),
        ("m.uv_at('1', true)", "TypeError: no implicit conversion of String into Integer"),
        ("m.uv_at(nil, true)", "TypeError: no implicit conversion from nil to integer"),
    ] {
        raises!(&format!("m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); {code}"), want);
    }
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); [m.set_uv(2, [0.5, 0.25, 1], true), m.uv_at(2, true).to_a, m.uv_at(2, false).to_a, m.uvs(true)[1].to_a]",
        "[nil, [0.5, 0.25, 1.0], [0.0, 0.0, 0.0], [0.5, 0.25, 1.0]]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); m.set_uv(1, [1, 1, 1], false); [m.uv_at(1, false).to_a, m.uv_at(-1, false).to_a]",
        "[[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]]"
    );
    raises!(
        "m = Geom::PolygonMesh.new; m.add_point([0, 0, 0]); m.set_uv(0, [0, 0, 1], true)",
        "RangeError: Index cannot be zero - starts at 1"
    );
    raises!(
        "m = Geom::PolygonMesh.new; m.add_point([0, 0, 0]); m.set_uv(2, [0, 0, 1], true)",
        "RangeError: Index out of range"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); m.normal_at(1).to_a",
        "[0.0, 0.0, 1.0]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); [m.normal_at(0), m.normal_at(9)]",
        "[nil, nil]"
    );
    check!(
        "m = Geom::PolygonMesh.new; m.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0]); t = Geom::Transformation.translation([0, 0, 5]); [m.transform!(t).equal?(m), m.point_at(1).to_a]",
        "[true, [0.0, 0.0, 5.0]]"
    );
    check!(
        "[Geom::PolygonMesh::NO_SMOOTH_OR_HIDE, Geom::PolygonMesh::HIDE_BASED_ON_INDEX, Geom::PolygonMesh::SOFTEN_BASED_ON_INDEX, Geom::PolygonMesh::AUTO_SOFTEN, Geom::PolygonMesh::SMOOTH_SOFT_EDGES]",
        "[0, 1, 2, 4, 8]"
    );
    check!(
        "[Geom::PolygonMesh::MESH_POINTS, Geom::PolygonMesh::MESH_UVQ_FRONT, Geom::PolygonMesh::MESH_UVQ_BACK, Geom::PolygonMesh::MESH_NORMALS]",
        "[0, 1, 2, 4]"
    );
}

// ------------------------------------------------------------------ LatLong / UTM
#[test]
fn latlong_and_utm() {
    check!(
        "l = Geom::LatLong.new(48.8566, 2.3522); [l.latitude, l.longitude, l.to_a, l.to_s]",
        "[48.8566, 2.3522, [48.8566, 2.3522], \"LatLong(48.85660N, 2.35220E)\"]"
    );
    check!("Geom::LatLong.new(-33.9, -70.65).to_s", "\"LatLong(33.90000S, 70.65000W)\"");
    check!("Geom::LatLong.new([10, 20]).to_a", "[10.0, 20.0]");
    check!("Geom::LatLong.new(Geom::LatLong.new(10, 20)).to_a", "[10.0, 20.0]");
    check!("Geom::LatLong.new.to_a", "[0.0, 0.0]");
    // SketchUp keeps what it is given (TestUp test_latitude_integers).
    check!("Geom::LatLong.new(91, 181).to_a", "[91.0, 181.0]");
    raises!("Geom::LatLong.new([1])", "ArgumentError: wrong number of values in array");
    raises!("Geom::LatLong.new(1, 2, 3)", "ArgumentError: wrong # of arguments(3 for 3)");
    // Paris -> UTM 31U, known reference values
    check!(
        "u = Geom::LatLong.new(48.8566, 2.3522).to_utm; [u.zone_number, u.zone_letter, u.x.round(0), u.y.round(0), u.to_a.size]",
        "[31, \"U\", 452483, 5411717, 4]"
    );
    // In the south SketchUp gives 10 000 km less the (negative) distance to the equator,
    // not 10 000 km plus it, and reads it back the same way.
    check!(
        "u = Geom::LatLong.new(-33.8688, 151.2093).to_utm; [u.zone_number, u.zone_letter, u.x.round(0), u.y.round(0)]",
        "[56, \"H\", 334369, 13749052]"
    );
    check!(
        "u = Geom::UTM.new(31, 'U', 452483.0, 5411717.0); l = u.to_latlong; [l.latitude.round(3), l.longitude.round(3)]",
        "[48.857, 2.352]"
    );
    check!(
        "u = Geom::LatLong.new(0, 3).to_utm; [u.zone_number, u.zone_letter, u.x.round(3), u.y.round(3)]",
        "[31, \"N\", 500000.0, 0.0]"
    );
    check!(
        "u = Geom::LatLong.new(45, 3).to_utm; [u.zone_number, u.zone_letter, u.x.round(3), u.y.round(2)]",
        "[31, \"T\", 500000.0, 4982950.4]"
    );
    check!(
        "l = Geom::LatLong.new(-33.8688, 151.2093); b = l.to_utm.to_latlong; [(b.latitude - l.latitude).abs < 1e-7, (b.longitude - l.longitude).abs < 1e-7]",
        "[true, true]"
    );
    check!("Geom::UTM.new(31, 'U', 452000.5, 5411722.25).to_s", "\"UTM(31 U 452000.50000 5411722.25000)\"");
    check!("Geom::UTM.new([31, 'U', 1.0, 2.0]).to_a", "[31, \"U\", 1.0, 2.0]");
    check!("Geom::UTM.new(Geom::UTM.new(31, 'U', 1.0, 2.0)).to_a", "[31, \"U\", 1.0, 2.0]");
    raises!(
        "Geom::UTM.new(Geom::LatLong.new(48.8566, 2.3522))",
        "TypeError: wrong argument type Geom::LatLong (expected Array)"
    );
    check!("Geom::UTM.new.to_a", "[0, \"A\", 0.0, 0.0]");
    raises!("Geom::UTM.new(1, 2)", "ArgumentError: wrong # of arguments(2 for 3)");
    // polar stereographic outside 80S..84N
    check!("u = Geom::LatLong.new(-85, 10).to_utm; [u.zone_number, u.zone_letter]", "[0, \"B\"]");
    check!(
        "l = Geom::LatLong.new(-85, 10); b = l.to_utm.to_latlong; [(b.latitude - l.latitude).abs < 1e-6, (b.longitude - l.longitude).abs < 1e-6]",
        "[true, true]"
    );
    check!(
        "l = Geom::LatLong.new(87, -40); b = l.to_utm.to_latlong; [l.to_utm.zone_letter, (b.latitude - l.latitude).abs < 1e-6, (b.longitude - l.longitude).abs < 1e-6]",
        "[\"Y\", true, true]"
    );
}

// ------------------------------------------------------------------ module Geom & positions
#[test]
fn geom_module_functions_exist() {
    // the module functions of Geom (not counted by api_coverage)
    check!(
        "%i[closest_points fit_plane_to_points intersect_line_line intersect_line_plane intersect_plane_plane linear_combination point_in_polygon_2D tesselate].reject { |m| Geom.respond_to?(m) }",
        "[]"
    );
}

#[test]
fn vertices_stand_for_points() {
    check!(
        "f = Sketchup.active_model.entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]); v = f.vertices.max_by { |x| x.position.x + x.position.y }; [Geom::Point3d.new(v).to_a, Geom::BoundingBox.new.add(f.vertices).max.to_a, Geom::Point3d.new(0, 0, 0).distance(v).to_f.round(6)]",
        "[[10.0, 10.0, 0.0], [10.0, 10.0, 0.0], 14.142136]"
    );
    raises!(
        "c = Sketchup.active_model.entities.add_cpoint([1, 2, 3]); Geom::Point3d.new(c)",
        "ArgumentError: Cannot convert argument to Geom::Point3d"
    );
    raises!("Geom::BoundingBox.new.add([[[1, 2, 3]]])", "ArgumentError: wrong number of values in array");
}

#[test]
fn face_mesh_is_a_geom_polygon_mesh() {
    // Face#mesh (zone entities) fills this zone's PolygonMesh: normals come from its polygons.
    check!(
        "f = Sketchup.active_model.entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]); m = f.mesh(Geom::PolygonMesh::MESH_NORMALS); [m.method(:uv_at).owner, m.count_points, m.count_polygons, (1..m.count_points).map { |i| m.normal_at(i).parallel?(f.normal) }.uniq]",
        "[Geom::PolygonMesh, 4, 2, [true]]"
    );
}

#[test]
fn bounding_box_add_accepts_plain_arrays() {
    let Some((o, _)) = must(
        "b = Geom::BoundingBox.new
         b.add([0, 0, 0], [1, 2, 3])
         c = Geom::BoundingBox.new.add([[4, 5, 6], [-1, 0, 0]])
         [b.max.to_a, c.min.to_a, c.max.to_a]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[1.0, 2.0, 3.0], [-1.0, 0.0, 0.0], [4.0, 5.0, 6.0]]");
}
