use esq_core::glam::DVec3;
use esq_core::model::{Entities, FaceId, Model};
use esq_core::sandbox::*;
use esq_core::tess::face_area;

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn total_area(e: &Entities) -> f64 {
    e.faces.keys().map(|f| face_area(e, f)).sum()
}

fn all_up(e: &Entities) -> bool {
    e.faces.values().all(|f| f.normal.z > 0.0)
}

/// Two triangles sharing a diagonal: a square of side 100 cut from (100,0) to (0,100).
fn square() -> Entities {
    let mut e = Entities::default();
    e.add_mesh(
        &[p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0), p(0.0, 100.0, 0.0), p(100.0, 100.0, 0.0)],
        &[[0, 1, 2], [2, 1, 3]],
        &[],
    );
    e
}

#[test]
fn grid_has_two_triangles_per_square_and_soft_diagonals() {
    let e = grid_entities(3, 2, 120.0);
    assert_eq!(e.verts.len(), 12);
    assert_eq!(e.faces.len(), 12);
    assert_eq!(e.edges.len(), 9 + 8 + 6);
    assert_eq!(e.edges.values().filter(|x| x.soft).count(), 6);
    assert!(all_up(&e));
    assert!((total_area(&e) - 360.0 * 240.0).abs() < 1e-6);
    // Every soft edge is a diagonal: it joins two corners that differ in x and y.
    for edge in e.edges.values().filter(|x| x.soft) {
        let (a, b) = (e.pos(edge.v[0]), e.pos(edge.v[1]));
        assert!((a.x - b.x).abs() > 1.0 && (a.y - b.y).abs() > 1.0);
    }
}

#[test]
fn grid_count_rounds_down_to_whole_squares() {
    assert_eq!(grid_count(3048.0 * 2.9, 3048.0), 2);
    assert_eq!(grid_count(3048.0 * 3.0, 3048.0), 3);
    assert_eq!(grid_count(100.0, 3048.0), 0);
}

#[test]
fn grid_frame_follows_the_axes() {
    let m = grid_frame(p(10.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(0.0, 7.0, 0.0)).unwrap();
    assert!(m.transform_vector3(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-9));
    assert!(m.transform_point3(p(1.0, 1.0, 0.0)).abs_diff_eq(p(11.0, 1.0, 0.0), 1e-9));
    assert!(grid_frame(DVec3::ZERO, DVec3::X, DVec3::X * 2.0).is_none());
}

#[test]
fn same_place_keeps_the_highest_point() {
    let pts = vec![p(0.0, 0.0, 5.0), p(0.0, 0.0, 9.0), p(0.0, 0.01, 2.0), p(50.0, 0.0, 1.0)];
    let hi = remove_non_functional(pts.clone(), true);
    assert_eq!(hi.len(), 2);
    assert!(hi.iter().any(|q| q.z == 9.0));
    let lo = remove_non_functional(pts, false);
    assert_eq!(lo.len(), 2);
    assert!(lo.iter().any(|q| q.z == 2.0));
}

#[test]
fn delaunay_of_a_square_with_a_centre_makes_four_triangles_facing_up() {
    let pts = [p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0), p(100.0, 100.0, 0.0), p(0.0, 100.0, 0.0), p(50.0, 50.0, 10.0)];
    let tris = triangulate_delaunay(&pts);
    assert_eq!(tris.len(), 4);
    for t in &tris {
        let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
        assert!((b - a).cross(c - a).z > 0.0);
    }
}

#[test]
fn delaunay_circumcircles_are_empty() {
    // A deterministic scatter.
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / ((1u64 << 31) as f64) * 1000.0
    };
    let pts: Vec<DVec3> = (0..60).map(|_| p(next(), next(), next() * 0.1)).collect();
    let tris = triangulate_delaunay(&pts);
    assert!(tris.len() > 60);
    for t in &tris {
        let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
        let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        let ux = ((a.x * a.x + a.y * a.y) * (b.y - c.y)
            + (b.x * b.x + b.y * b.y) * (c.y - a.y)
            + (c.x * c.x + c.y * c.y) * (a.y - b.y))
            / d;
        let uy = ((a.x * a.x + a.y * a.y) * (c.x - b.x)
            + (b.x * b.x + b.y * b.y) * (a.x - c.x)
            + (c.x * c.x + c.y * c.y) * (b.x - a.x))
            / d;
        let r = ((a.x - ux).powi(2) + (a.y - uy).powi(2)).sqrt();
        for (i, q) in pts.iter().enumerate() {
            if t.contains(&i) {
                continue;
            }
            let dist = ((q.x - ux).powi(2) + (q.y - uy).powi(2)).sqrt();
            assert!(dist >= r - 1e-6, "point {i} inside the circle of {t:?}");
        }
    }
}

#[test]
fn contours_become_a_terrain_group_keeping_heights() {
    let mut m = Model::new();
    for (z, y) in [(0.0, 0.0), (1000.0, 2000.0)] {
        m.root.add_polyline(&[p(0.0, y, z), p(1000.0, y, z), p(2000.0, y, z), p(3000.0, y, z)], false, false);
    }
    let edges: Vec<_> = m.root.edges.keys().collect();
    let g = m.terrain_from_contours(None, &edges).unwrap();
    let def = m.defs[m.root.instances[g].def].clone();
    assert!(def.is_group);
    // 8 ends, plus one break point on each edge (each is as long as the average).
    assert_eq!(def.entities.verts.len(), 14);
    assert!(def.entities.faces.len() >= 6);
    assert!(all_up(&def.entities));
    assert!(def.entities.verts.values().any(|v| v.pos.z == 1000.0));
    // All edges visible.
    assert!(def.entities.edges.values().all(|e| !e.soft && !e.hidden));
}

#[test]
fn long_contour_edges_get_break_points() {
    let mut e = Entities::default();
    e.add_polyline(&[p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0)], false, false);
    e.add_polyline(&[p(0.0, 10.0, 0.0), p(1000.0, 10.0, 0.0)], false, false);
    let edges: Vec<_> = e.edges.keys().collect();
    // Average length 550: the long edge is cut once (1000 / 550 = 1), the short one not.
    let pts = contour_points(&e, &edges);
    assert_eq!(pts.len(), 5);
    assert!(pts.iter().any(|q| (q.x - 500.0).abs() < 1e-9 && q.y == 10.0));
}

#[test]
fn contours_need_a_selection() {
    let mut m = Model::new();
    assert_eq!(m.terrain_from_contours(None, &[]).unwrap_err(), MSG_NO_CONTOURS);
}

#[test]
fn subdividing_one_triangle_cuts_it_in_four_and_its_neighbour_in_two() {
    let mut e = square();
    let first: FaceId = e.faces.keys().next().unwrap();
    let area = total_area(&e);
    assert_eq!(e.subdivide_triangles(&[first]), 1);
    assert_eq!(e.faces.len(), 4 + 2);
    assert!((total_area(&e) - area).abs() < 1e-6);
    assert!(all_up(&e));
    // Every edge borders at most two faces and none is left cracked: 6 triangles, 3 midpoints.
    assert_eq!(e.verts.len(), 4 + 3);
    assert!(e.edges.values().all(|x| x.faces.len() <= 2));
    assert!(e.faces.values().all(|f| f.outer.len() == 3));
}

#[test]
fn subdividing_both_triangles_makes_eight() {
    let mut e = square();
    let all: Vec<FaceId> = e.faces.keys().collect();
    assert_eq!(e.subdivide_triangles(&all), 2);
    assert_eq!(e.faces.len(), 8);
    assert_eq!(e.verts.len(), 4 + 5);
}

#[test]
fn subdividing_makes_soft_edges_visible() {
    let mut e = grid_entities(1, 1, 100.0);
    assert!(e.edges.values().any(|x| x.soft));
    let all: Vec<FaceId> = e.faces.keys().collect();
    e.subdivide_triangles(&all);
    assert!(e.edges.values().all(|x| !x.soft));
}

#[test]
fn detail_on_a_face_joins_the_new_point_to_the_three_corners() {
    let mut e = square();
    let f = e.faces.keys().next().unwrap();
    let v = e.add_detail_vertex(DetailTarget::Face(f), p(20.0, 20.0, 0.0)).unwrap();
    assert_eq!(e.faces.len(), 4);
    assert_eq!(e.verts[v].edges.len(), 3);
    assert!(all_up(&e));
}

#[test]
fn detail_on_an_edge_cuts_both_faces_in_two() {
    let mut e = square();
    let diag = e.edges.iter().find(|(_, x)| x.faces.len() == 2).map(|(k, _)| k).unwrap();
    let v = e.add_detail_vertex(DetailTarget::Edge(diag), p(50.0, 50.0, 0.0)).unwrap();
    assert_eq!(e.faces.len(), 4);
    assert_eq!(e.verts[v].edges.len(), 4);
    assert!(all_up(&e));
    assert!((total_area(&e) - 10000.0).abs() < 1e-6);
}

#[test]
fn detail_on_a_vertex_adds_nothing() {
    let mut e = square();
    let corner = e.find_vertex(p(0.0, 0.0, 0.0)).unwrap();
    assert_eq!(e.add_detail_vertex(DetailTarget::Vertex(corner), DVec3::ZERO), Ok(corner));
    assert_eq!(e.faces.len(), 2);
}

#[test]
fn detail_refuses_faces_that_are_not_triangles() {
    let mut e = Entities::default();
    let f = e.add_mesh(&[p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)], &[[0, 1, 2]], &[]).pop().unwrap();
    // Make it a quad by hand.
    let extra = e.add_vertex_raw(p(0.0, 1.0, 0.0));
    e.faces[f].outer.push(extra);
    assert_eq!(e.add_detail_vertex(DetailTarget::Face(f), p(0.5, 0.4, 0.0)), Err(MSG_DETAIL_INVALID.to_string()));
}

#[test]
fn flip_swaps_the_diagonal_and_keeps_attributes() {
    let mut e = square();
    let diag = e.edges.iter().find(|(_, x)| x.faces.len() == 2).map(|(k, _)| k).unwrap();
    e.edges[diag].smooth = true;
    let mat = {
        let mut m = Model::new();
        m.add_material("Herbe", [0, 200, 0])
    };
    let f0 = e.edges[diag].faces[0];
    e.faces[f0].front = Some(mat);
    assert_eq!(e.can_flip_edge(diag), Ok(()));
    e.flip_edge(diag).unwrap();
    assert_eq!(e.faces.len(), 2);
    let a = e.find_vertex(p(0.0, 0.0, 0.0)).unwrap();
    let d = e.find_vertex(p(100.0, 100.0, 0.0)).unwrap();
    let new_diag = e.find_edge(a, d).expect("the other diagonal exists");
    assert!(e.edges[new_diag].smooth);
    assert_eq!(e.edges[new_diag].faces.len(), 2);
    assert!(all_up(&e));
    assert!(e.faces.values().all(|f| f.front == Some(mat)));
    assert!((total_area(&e) - 10000.0).abs() < 1e-6);
}

#[test]
fn flip_refuses_a_concave_quad_and_boundary_edges() {
    // The diagonal of a dart: the other diagonal would fall outside the quad.
    let mut e = Entities::default();
    e.add_mesh(
        &[p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0), p(100.0, 100.0, 0.0), p(80.0, 5.0, 0.0)],
        &[[0, 1, 3], [1, 2, 3]],
        &[],
    );
    let shared = e.edges.iter().find(|(_, x)| x.faces.len() == 2).map(|(k, _)| k).unwrap();
    assert_eq!(e.can_flip_edge(shared), Err(FlipError::Invalid));
    let boundary = e.edges.iter().find(|(_, x)| x.faces.len() == 1).map(|(k, _)| k).unwrap();
    assert_eq!(e.can_flip_edge(boundary), Err(FlipError::NotTriangles));
}
