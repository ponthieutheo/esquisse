//! Toolbar icons, drawn with vector shapes (no image assets).

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::tools::ToolId;

const INK: Color32 = Color32::from_rgb(40, 44, 52);
const ACCENT: Color32 = Color32::from_rgb(214, 96, 44);
const BLUE: Color32 = Color32::from_rgb(52, 110, 220);

pub fn draw(p: &Painter, r: Rect, tool: ToolId) {
    let s = r.width() / 24.0;
    let at = |x: f32, y: f32| -> Pos2 { r.min + vec2(x * s, y * s) };
    let ink = Stroke::new(1.6 * s, INK);
    let thin = Stroke::new(1.1 * s, INK);
    let acc = Stroke::new(1.8 * s, ACCENT);
    match tool {
        ToolId::Select => {
            let pts = vec![
                at(7.0, 3.0),
                at(7.0, 19.0),
                at(11.0, 15.0),
                at(14.0, 21.0),
                at(16.5, 20.0),
                at(13.5, 14.0),
                at(19.0, 14.0),
            ];
            p.add(Shape::convex_polygon(pts.clone(), Color32::WHITE, ink));
        }
        ToolId::Line => {
            p.line_segment([at(4.0, 20.0), at(18.0, 6.0)], ink);
            p.line_segment([at(15.5, 3.5), at(20.5, 8.5)], acc);
            p.circle_filled(at(4.0, 20.0), 1.6 * s, ACCENT);
        }
        ToolId::Rect => {
            p.rect_stroke(Rect::from_min_max(at(4.0, 6.0), at(20.0, 18.0)), 0.0, ink, eframe::egui::StrokeKind::Middle);
            p.circle_filled(at(20.0, 18.0), 1.6 * s, ACCENT);
        }
        ToolId::Circle => {
            p.circle_stroke(at(12.0, 12.0), 8.0 * s, ink);
            p.line_segment([at(12.0, 12.0), at(20.0, 12.0)], acc);
        }
        ToolId::Polygon => {
            let pts: Vec<Pos2> = (0..6)
                .map(|i| {
                    let a = i as f32 / 6.0 * std::f32::consts::TAU;
                    at(12.0 + 8.0 * a.cos(), 12.0 + 8.0 * a.sin())
                })
                .collect();
            p.add(Shape::closed_line(pts, ink));
        }
        ToolId::Arc => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    at(12.0 + 8.0 * a.cos(), 17.0 + 8.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, ink));
            p.circle_filled(at(4.0, 17.0), 1.5 * s, ACCENT);
            p.circle_filled(at(20.0, 17.0), 1.5 * s, ACCENT);
        }
        ToolId::PushPull => {
            p.add(Shape::convex_polygon(
                vec![at(3.0, 16.0), at(13.0, 16.0), at(21.0, 11.0), at(11.0, 11.0)],
                Color32::from_rgb(220, 226, 236),
                thin,
            ));
            p.line_segment([at(12.0, 13.5), at(12.0, 3.0)], acc);
            p.add(Shape::convex_polygon(vec![at(9.0, 6.0), at(15.0, 6.0), at(12.0, 2.0)], ACCENT, Stroke::NONE));
            p.line_segment([at(3.0, 16.0), at(3.0, 21.0)], thin);
            p.line_segment([at(13.0, 16.0), at(13.0, 21.0)], thin);
        }
        ToolId::FollowMe => {
            // A small square profile swept along a bent path.
            let path = vec![at(4.0, 19.0), at(14.0, 19.0), at(19.0, 14.0), at(19.0, 4.0)];
            p.add(Shape::line(path, Stroke::new(1.2 * s, ACCENT)));
            let prof = vec![at(3.0, 14.0), at(8.0, 14.0), at(8.0, 21.0), at(3.0, 21.0)];
            p.add(Shape::convex_polygon(prof, Color32::WHITE, ink));
            let band = vec![at(8.0, 14.0), at(13.0, 14.0), at(14.0, 13.0), at(14.0, 4.0)];
            p.add(Shape::line(band, thin));
        }
        ToolId::Component => {
            p.rect_stroke(Rect::from_min_max(at(5.0, 8.0), at(15.0, 18.0)), 0.0, ink, eframe::egui::StrokeKind::Middle);
            p.line_segment([at(5.0, 8.0), at(10.0, 4.0)], thin);
            p.line_segment([at(15.0, 8.0), at(20.0, 4.0)], thin);
            p.line_segment([at(15.0, 18.0), at(20.0, 14.0)], thin);
            p.line_segment([at(10.0, 4.0), at(20.0, 4.0)], thin);
            p.line_segment([at(20.0, 4.0), at(20.0, 14.0)], thin);
        }
        ToolId::Move => {
            let c = at(12.0, 12.0);
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                let tip = c + vec2(dx, dy) * 9.0 * s;
                p.line_segment([c, tip], ink);
                let side = vec2(-dy, dx) * 2.5 * s;
                let back = tip - vec2(dx, dy) * 3.0 * s;
                p.add(Shape::convex_polygon(vec![tip, back + side, back - side], ACCENT, Stroke::NONE));
            }
        }
        ToolId::Rotate => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -0.3 + i as f32 / 20.0 * 4.9;
                    at(12.0 + 8.0 * a.cos(), 12.0 + 8.0 * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, ink));
            p.add(Shape::convex_polygon(
                vec![end + vec2(-3.0, -1.0) * s, end + vec2(2.5, -2.5) * s, end + vec2(1.0, 3.0) * s],
                ACCENT,
                Stroke::NONE,
            ));
        }
        ToolId::Scale => {
            p.rect_stroke(
                Rect::from_min_max(at(3.0, 9.0), at(15.0, 21.0)),
                0.0,
                thin,
                eframe::egui::StrokeKind::Middle,
            );
            p.rect_stroke(
                Rect::from_min_max(at(3.0, 3.0), at(21.0, 21.0)),
                0.0,
                Stroke::new(1.0 * s, Color32::GRAY),
                eframe::egui::StrokeKind::Middle,
            );
            p.line_segment([at(13.0, 11.0), at(20.0, 4.0)], acc);
            for q in [at(3.0, 3.0), at(21.0, 3.0), at(21.0, 21.0), at(3.0, 21.0)] {
                p.rect_filled(Rect::from_center_size(q, vec2(3.0, 3.0) * s), 0.0, BLUE);
            }
        }
        ToolId::Offset => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 4.0), at(21.0, 20.0)), 0.0, ink, eframe::egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(7.0, 8.0), at(17.0, 16.0)), 0.0, acc, eframe::egui::StrokeKind::Middle);
        }
        ToolId::Eraser => {
            p.add(Shape::convex_polygon(
                vec![at(4.0, 15.0), at(13.0, 6.0), at(20.0, 13.0), at(11.0, 22.0)],
                Color32::from_rgb(240, 150, 170),
                ink,
            ));
            p.line_segment([at(8.5, 10.5), at(15.5, 17.5)], thin);
        }
        ToolId::Paint => {
            p.add(Shape::convex_polygon(
                vec![at(5.0, 10.0), at(12.0, 3.0), at(19.0, 10.0), at(12.0, 17.0)],
                Color32::from_rgb(230, 230, 236),
                ink,
            ));
            p.add(Shape::convex_polygon(vec![at(5.0, 10.0), at(19.0, 10.0), at(12.0, 17.0)], ACCENT, Stroke::NONE));
            p.circle_filled(at(20.0, 18.0), 2.0 * s, ACCENT);
        }
        ToolId::Tape => {
            p.rect_stroke(
                Rect::from_min_max(at(3.0, 9.0), at(15.0, 19.0)),
                2.0 * s,
                ink,
                eframe::egui::StrokeKind::Middle,
            );
            p.circle_stroke(at(9.0, 14.0), 2.5 * s, thin);
            p.line_segment([at(15.0, 17.0), at(22.0, 17.0)], Stroke::new(2.4 * s, Color32::from_rgb(230, 190, 40)));
        }
        ToolId::Orbit => {
            p.circle_stroke(at(12.0, 12.0), 3.5 * s, ink);
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * 5.4;
                    at(12.0 + 9.0 * a.cos(), 12.0 + 4.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, acc));
        }
        ToolId::Pan => {
            for (i, h) in [7.0, 5.0, 4.5, 6.0].iter().enumerate() {
                let x = 7.5 + i as f32 * 3.0;
                p.line_segment([at(x, 13.0), at(x, 13.0 - h)], Stroke::new(2.2 * s, INK));
            }
            p.add(Shape::convex_polygon(
                vec![at(6.0, 12.0), at(18.5, 12.0), at(17.0, 20.0), at(8.0, 20.0)],
                Color32::from_rgb(250, 220, 190),
                ink,
            ));
        }
        ToolId::Freehand => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let x = 3.0 + i as f32 * 0.75;
                    at(x, 13.0 + 5.0 * (x * 0.55).sin() - (x - 12.0) * 0.25)
                })
                .collect();
            p.add(Shape::line(pts, ink));
            p.line_segment([at(17.0, 4.0), at(21.5, 8.5)], acc);
        }
        ToolId::ArcCenter | ToolId::Pie => {
            let c = at(6.0, 18.0);
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 * i as f32 / 16.0;
                    c + vec2(a.cos(), a.sin()) * 14.0 * s
                })
                .collect();
            if tool == ToolId::Pie {
                let mut poly = vec![c];
                poly.extend(pts.iter().copied());
                p.add(Shape::convex_polygon(poly, Color32::from_rgb(220, 226, 236), Stroke::NONE));
                p.line_segment([c, pts[0]], ink);
                p.line_segment([c, *pts.last().unwrap()], ink);
            } else {
                p.line_segment([c, pts[0]], Stroke::new(1.0 * s, Color32::GRAY));
            }
            p.add(Shape::line(pts, ink));
            p.circle_filled(c, 1.6 * s, ACCENT);
        }
        ToolId::Arc3 => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    at(12.0 + 8.0 * a.cos(), 16.0 + 8.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts.clone(), ink));
            for q in [pts[0], pts[8], pts[16]] {
                p.circle_filled(q, 1.5 * s, ACCENT);
            }
        }
        ToolId::RotRect => {
            let q = vec![at(4.0, 13.0), at(13.0, 4.0), at(20.0, 11.0), at(11.0, 20.0)];
            p.add(Shape::closed_line(q, ink));
            p.circle_filled(at(4.0, 13.0), 1.6 * s, ACCENT);
            p.circle_filled(at(13.0, 4.0), 1.6 * s, ACCENT);
        }
        ToolId::Lasso => {
            let pts: Vec<Pos2> = (0..=28)
                .map(|i| {
                    let a = i as f32 / 28.0 * 5.6;
                    at(12.0 + 8.0 * a.cos(), 9.0 + 5.5 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, ink));
            p.add(Shape::line(vec![at(17.0, 13.0), at(15.0, 17.0), at(18.0, 21.0)], thin));
        }
        ToolId::TagTool => {
            let q = vec![at(3.0, 9.0), at(13.0, 9.0), at(20.0, 14.0), at(13.0, 19.0), at(3.0, 19.0)];
            p.add(Shape::convex_polygon(q, Color32::from_rgb(250, 220, 150), ink));
            p.circle_stroke(at(14.0, 14.0), 1.4 * s, thin);
        }
        ToolId::Flip => {
            p.line_segment([at(12.0, 2.0), at(12.0, 22.0)], Stroke::new(1.0 * s, BLUE));
            p.add(Shape::convex_polygon(
                vec![at(10.0, 6.0), at(10.0, 18.0), at(3.0, 18.0)],
                Color32::from_rgb(220, 226, 236),
                ink,
            ));
            p.add(Shape::convex_polygon(vec![at(14.0, 6.0), at(21.0, 18.0), at(14.0, 18.0)], ACCENT, ink));
        }
        ToolId::Protractor => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    at(12.0 + 9.0 * a.cos(), 17.0 + 9.0 * a.sin())
                })
                .collect();
            let mut poly = pts.clone();
            poly.push(at(21.0, 17.0));
            p.add(Shape::line(poly, ink));
            p.line_segment([at(3.0, 17.0), at(21.0, 17.0)], ink);
            p.line_segment([at(12.0, 17.0), at(19.0, 9.0)], acc);
        }
        ToolId::AxesTool => {
            let o = at(8.0, 17.0);
            p.line_segment([o, at(21.0, 20.0)], Stroke::new(1.8 * s, Color32::from_rgb(220, 30, 30)));
            p.line_segment([o, at(17.0, 9.0)], Stroke::new(1.8 * s, Color32::from_rgb(20, 150, 40)));
            p.line_segment([o, at(8.0, 3.0)], Stroke::new(1.8 * s, Color32::from_rgb(30, 50, 230)));
        }
        ToolId::North => {
            p.circle_stroke(at(12.0, 13.0), 8.0 * s, thin);
            p.add(Shape::convex_polygon(
                vec![at(12.0, 4.0), at(15.5, 14.0), at(12.0, 12.0), at(8.5, 14.0)],
                ACCENT,
                ink,
            ));
        }
        ToolId::Fov => {
            p.add(Shape::convex_polygon(
                vec![at(3.0, 12.0), at(21.0, 4.0), at(21.0, 20.0)],
                Color32::from_rgb(220, 226, 236),
                ink,
            ));
            p.circle_filled(at(3.0, 12.0), 1.8 * s, ACCENT);
        }
        ToolId::ZoomWindow => {
            p.rect_stroke(
                Rect::from_min_max(at(2.0, 2.0), at(16.0, 13.0)),
                0.0,
                Stroke::new(1.2 * s, BLUE),
                eframe::egui::StrokeKind::Middle,
            );
            p.circle_stroke(at(11.0, 12.0), 5.0 * s, ink);
            p.line_segment([at(14.5, 15.5), at(21.0, 22.0)], Stroke::new(2.8 * s, ACCENT));
        }
        ToolId::PositionCamera => {
            p.add(Shape::convex_polygon(vec![at(12.0, 21.0), at(7.0, 13.0), at(17.0, 13.0)], ACCENT, Stroke::NONE));
            p.circle_stroke(at(12.0, 8.0), 4.5 * s, ink);
            p.circle_filled(at(12.0, 8.0), 1.6 * s, INK);
        }
        ToolId::LookAround => {
            p.add(Shape::convex_polygon(
                vec![at(3.0, 12.0), at(12.0, 6.0), at(21.0, 12.0), at(12.0, 18.0)],
                Color32::WHITE,
                ink,
            ));
            p.circle_filled(at(12.0, 12.0), 3.0 * s, INK);
            p.line_segment([at(4.0, 4.0), at(9.0, 4.0)], acc);
            p.line_segment([at(15.0, 20.0), at(20.0, 20.0)], acc);
        }
        ToolId::Walk => {
            for (x, y) in [(8.0, 7.0), (15.0, 15.0)] {
                p.add(Shape::convex_polygon(
                    vec![at(x - 2.5, y - 4.0), at(x + 2.5, y - 4.0), at(x + 2.0, y + 4.0), at(x - 2.0, y + 4.0)],
                    Color32::from_rgb(90, 96, 108),
                    Stroke::NONE,
                ));
            }
        }
        ToolId::Dimension => {
            p.line_segment([at(3.0, 18.0), at(3.0, 8.0)], thin);
            p.line_segment([at(21.0, 18.0), at(21.0, 8.0)], thin);
            p.line_segment([at(4.0, 11.0), at(20.0, 11.0)], ink);
            for (x, d) in [(4.0, 1.0), (20.0, -1.0)] {
                p.add(Shape::convex_polygon(
                    vec![at(x, 11.0), at(x + 4.0 * d, 9.0), at(x + 4.0 * d, 13.0)],
                    INK,
                    Stroke::NONE,
                ));
            }
            p.line_segment([at(9.0, 6.0), at(15.0, 6.0)], acc);
        }
        ToolId::Text => {
            p.line_segment([at(3.0, 21.0), at(9.0, 14.0)], thin);
            p.circle_filled(at(3.0, 21.0), 1.6 * s, INK);
            p.rect_stroke(
                Rect::from_min_max(at(9.0, 4.0), at(22.0, 14.0)),
                1.0 * s,
                ink,
                eframe::egui::StrokeKind::Middle,
            );
            p.line_segment([at(12.0, 7.5), at(19.0, 7.5)], acc);
            p.line_segment([at(12.0, 10.5), at(17.0, 10.5)], thin);
        }
        ToolId::SectionPlane => {
            // A slanted plane with an arrow through it.
            let plane = vec![at(6.0, 4.0), at(16.0, 7.0), at(16.0, 21.0), at(6.0, 18.0)];
            p.add(Shape::convex_polygon(plane, Color32::from_rgba_unmultiplied(102, 110, 141, 50), ink));
            p.line_segment([at(2.0, 12.0), at(20.0, 12.5)], acc);
            p.add(Shape::convex_polygon(vec![at(22.5, 12.5), at(18.0, 9.5), at(18.0, 15.5)], ACCENT, Stroke::NONE));
        }
        ToolId::Ruby => {
            // A ruby: a faceted gem.
            let gem = vec![at(5.0, 9.0), at(9.0, 4.0), at(15.0, 4.0), at(19.0, 9.0), at(12.0, 21.0)];
            p.add(Shape::convex_polygon(gem, Color32::from_rgb(200, 40, 50), ink));
            p.line_segment([at(5.0, 9.0), at(19.0, 9.0)], thin);
        }
        ToolId::MatchPhoto => {
            // A photo frame with a hill and a sun.
            p.rect_stroke(Rect::from_min_max(at(3.0, 5.0), at(21.0, 19.0)), 1.0, ink, eframe::egui::StrokeKind::Middle);
            p.add(Shape::line(
                vec![at(3.5, 17.0), at(9.0, 11.0), at(13.0, 15.0), at(16.0, 12.0), at(20.5, 17.0)],
                thin,
            ));
            p.circle_filled(at(16.5, 8.5), 1.7 * s, ACCENT);
        }
        ToolId::Image => {
            // A picture lying in perspective, its corner on the cursor's point.
            let quad = vec![at(4.0, 18.0), at(16.0, 20.0), at(20.0, 8.0), at(8.0, 6.0)];
            p.add(Shape::convex_polygon(quad, Color32::from_rgb(220, 232, 246), ink));
            p.add(Shape::line(vec![at(6.5, 16.5), at(11.0, 11.0), at(14.0, 15.0), at(18.0, 10.5)], thin));
            p.circle_filled(at(4.0, 18.0), 1.6 * s, ACCENT);
        }
        ToolId::OuterShell
        | ToolId::SolidIntersect
        | ToolId::SolidUnion
        | ToolId::SolidSubtract
        | ToolId::SolidTrim
        | ToolId::SolidSplit => {
            // Two overlapping squares; the part the tool keeps is filled.
            let a = Rect::from_min_max(at(3.0, 8.0), at(15.0, 20.0));
            let b = Rect::from_min_max(at(9.0, 3.0), at(21.0, 15.0));
            let both = a.intersect(b);
            let fill = Color32::from_rgb(240, 170, 120);
            let light = Color32::from_rgb(220, 226, 236);
            let k = eframe::egui::StrokeKind::Middle;
            match tool {
                ToolId::SolidIntersect => {
                    p.rect_filled(both, 0.0, fill);
                }
                ToolId::SolidUnion | ToolId::OuterShell => {
                    p.rect_filled(a, 0.0, fill);
                    p.rect_filled(b, 0.0, fill);
                }
                ToolId::SolidSubtract | ToolId::SolidTrim => {
                    p.rect_filled(b, 0.0, fill);
                    p.rect_filled(both, 0.0, Color32::WHITE);
                    if tool == ToolId::SolidTrim {
                        p.rect_filled(a, 0.0, light);
                    }
                }
                _ => {
                    p.rect_filled(a, 0.0, light);
                    p.rect_filled(b, 0.0, fill);
                    p.rect_filled(both, 0.0, Color32::from_rgb(140, 180, 230));
                }
            }
            if tool == ToolId::OuterShell {
                let outline = vec![
                    a.left_top(),
                    pos2(b.left(), a.top()),
                    b.left_top(),
                    b.right_top(),
                    b.right_bottom(),
                    pos2(a.right(), b.bottom()),
                    a.right_bottom(),
                    a.left_bottom(),
                ];
                p.add(Shape::closed_line(outline, ink));
            } else {
                p.rect_stroke(a, 0.0, thin, k);
                p.rect_stroke(b, 0.0, thin, k);
            }
        }
        ToolId::FromContours => {
            // Contour lines at different heights over a triangulated surface.
            p.add(Shape::line(vec![at(3.0, 18.0), at(9.0, 12.0), at(14.0, 14.0), at(21.0, 7.0)], acc));
            p.add(Shape::line(vec![at(3.0, 21.0), at(10.0, 17.0), at(15.0, 19.0), at(21.0, 12.0)], ink));
            p.line_segment([at(9.0, 12.0), at(10.0, 17.0)], thin);
            p.line_segment([at(14.0, 14.0), at(15.0, 19.0)], thin);
            p.line_segment([at(9.0, 12.0), at(15.0, 19.0)], thin);
        }
        ToolId::AddDetail => {
            // A triangle cut in three around a new vertex that rises.
            let (a, b, c, v) = (at(3.0, 19.0), at(21.0, 19.0), at(12.0, 8.0), at(12.0, 15.0));
            p.add(Shape::closed_line(vec![a, b, c], ink));
            for corner in [a, b, c] {
                p.line_segment([v, corner], thin);
            }
            p.line_segment([v, at(12.0, 3.0)], acc);
            p.circle_filled(at(12.0, 3.0), 2.0 * s, ACCENT);
        }
        ToolId::FlipEdge => {
            // A quad whose diagonal turns.
            let quad = vec![at(4.0, 6.0), at(18.0, 4.0), at(20.0, 18.0), at(6.0, 20.0)];
            p.add(Shape::closed_line(quad, ink));
            p.line_segment([at(18.0, 4.0), at(6.0, 20.0)], Stroke::new(1.1 * s, Color32::from_gray(150)));
            p.line_segment([at(4.0, 6.0), at(20.0, 18.0)], acc);
        }
        ToolId::FromScratch => {
            // A grid of squares with a diagonal in each.
            for i in 0..=3 {
                let k = 4.0 + 5.0 * i as f32;
                p.line_segment([at(k, 4.0), at(k, 19.0)], thin);
                p.line_segment([at(4.0, k), at(19.0, k)], thin);
            }
            p.line_segment([at(9.0, 4.0), at(4.0, 9.0)], acc);
            p.line_segment([at(14.0, 9.0), at(9.0, 14.0)], acc);
        }
        ToolId::Smoove => {
            // A mesh line lifted into a hill by an arrow.
            let hill: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let bump = (1.0 - (2.0 * t - 1.0).powi(2)).max(0.0);
                    at(2.0 + 20.0 * t, 19.0 - 11.0 * bump * bump)
                })
                .collect();
            p.add(Shape::line(hill, ink));
            p.line_segment([at(2.0, 20.0), at(22.0, 20.0)], thin);
            p.line_segment([at(12.0, 7.0), at(12.0, 2.5)], acc);
            p.line_segment([at(9.5, 5.0), at(12.0, 2.5)], acc);
            p.line_segment([at(14.5, 5.0), at(12.0, 2.5)], acc);
        }
        ToolId::Stamp => {
            // A flat pad pressed into sloping ground.
            p.add(Shape::line(vec![at(2.0, 8.0), at(8.0, 14.0), at(16.0, 14.0), at(22.0, 20.0)], ink));
            p.rect_stroke(Rect::from_min_max(at(8.0, 8.0), at(16.0, 14.0)), 0.0, acc, eframe::egui::StrokeKind::Middle);
            p.line_segment([at(12.0, 2.0), at(12.0, 8.0)], thin);
            p.line_segment([at(9.5, 5.5), at(12.0, 8.0)], thin);
            p.line_segment([at(14.5, 5.5), at(12.0, 8.0)], thin);
        }
        ToolId::Drape => {
            // A straight line above a hill, dropped onto it.
            let hill: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let bump = (1.0 - (2.0 * t - 1.0).powi(2)).max(0.0);
                    at(2.0 + 20.0 * t, 20.0 - 9.0 * bump)
                })
                .collect();
            p.add(Shape::line(hill, ink));
            p.line_segment([at(3.0, 4.0), at(21.0, 4.0)], acc);
            for x in [6.0f32, 12.0, 18.0] {
                let bump = (1.0 - (2.0 * (x - 2.0) / 20.0 - 1.0).powi(2)).max(0.0);
                p.line_segment([at(x, 5.0), at(x, 20.0 - 9.0 * bump - 1.0)], thin);
            }
        }
        ToolId::Zoom => {
            p.circle_stroke(at(10.0, 10.0), 6.0 * s, ink);
            p.line_segment([at(14.5, 14.5), at(21.0, 21.0)], Stroke::new(3.0 * s, ACCENT));
        }
        ToolId::Divide
        | ToolId::ShowFaceEdges
        | ToolId::Classifier
        | ToolId::PhotoPoint
        | ToolId::AddSnap
        | ToolId::Light
        | ToolId::Igloo => crate::tools::outils_icon(p, r, tool),
        ToolId::Comment => crate::comments::tool_icon(p, r),
        ToolId::SampleMaterial => {
            // An eyedropper over a drop of paint.
            p.line_segment([at(5.0, 19.0), at(15.0, 9.0)], Stroke::new(2.6 * s, INK));
            p.add(Shape::convex_polygon(
                vec![at(14.0, 6.0), at(18.0, 2.5), at(21.5, 6.0), at(18.0, 10.0)],
                ACCENT,
                ink,
            ));
            p.circle_filled(at(4.0, 20.5), 1.8 * s, ACCENT);
        }
        ToolId::PositionTexture => {
            // A tile of texture held by a pushpin.
            p.rect_stroke(
                Rect::from_min_max(at(3.0, 7.0), at(17.0, 21.0)),
                0.0,
                thin,
                eframe::egui::StrokeKind::Middle,
            );
            p.line_segment([at(3.0, 14.0), at(17.0, 14.0)], thin);
            p.line_segment([at(10.0, 7.0), at(10.0, 21.0)], thin);
            p.line_segment([at(17.0, 7.0), at(14.0, 10.0)], ink);
            p.circle_filled(at(19.0, 5.0), 3.0 * s, Color32::from_rgb(220, 30, 30));
        }
    }
}

pub fn zoom_extents(p: &Painter, r: Rect) {
    let s = r.width() / 24.0;
    let at = |x: f32, y: f32| r.min + vec2(x * s, y * s);
    p.circle_stroke(at(10.0, 10.0), 5.5 * s, Stroke::new(1.6 * s, INK));
    p.line_segment([at(14.0, 14.0), at(20.0, 20.0)], Stroke::new(2.8 * s, ACCENT));
    for (a, b) in
        [((2.0, 6.0), (2.0, 2.0)), ((2.0, 2.0), (6.0, 2.0)), ((22.0, 2.0), (18.0, 2.0)), ((22.0, 2.0), (22.0, 6.0))]
    {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], Stroke::new(1.4 * s, BLUE));
    }
    let _ = pos2(0.0, 0.0);
}
