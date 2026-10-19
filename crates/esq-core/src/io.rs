//! Native format (.esq, JSON) and mesh exports (OBJ, STL).

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use glam::DVec3;

use crate::model::{MaterialId, Model};
use crate::tess::face_triangles;
use crate::walk::visit;

pub fn save_esq(model: &Model, path: &Path) -> Result<()> {
    let json = serde_json::to_vec(model)?;
    let tmp = path.with_extension("esq.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("écriture de {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_esq(path: &Path) -> Result<Model> {
    let bytes = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
    load_esq_bytes(&bytes)
}

/// [`load_esq`] from the file's bytes. The keys of the file are checked before use
/// (`crate::repair`): a dangling one made the topology rebuild panic, a definition
/// containing itself made every walk of the tree recurse.
pub fn load_esq_bytes(bytes: &[u8]) -> Result<Model> {
    let mut model: Model = serde_json::from_slice(bytes).context("fichier .esq invalide")?;
    if model.format_version > 1 {
        bail!("fichier créé par une version plus récente (format {})", model.format_version);
    }
    model.check_after_reading().map_err(anyhow::Error::msg)?;
    model.rebuild_topology();
    model.sanitize_texture_names();
    model.migrate_legacy_images();
    // Snaps and spaces get their ids (spaces saved by older versions become entities).
    model.ensure_snap_ids();
    Ok(model)
}

/// Every visible face of the model as world triangles, with its material.
pub fn world_triangles(model: &Model) -> Vec<([DVec3; 3], Option<MaterialId>)> {
    let mut out = Vec::new();
    visit(model, |p| {
        for (f, face) in p.ents.faces.iter() {
            if face.hidden || !model.layers.get(face.layer).is_none_or(|l| l.visible) {
                continue;
            }
            let mat = face.front.or(p.inherited);
            for t in face_triangles(p.ents, f) {
                out.push((t.map(|v| p.xform.transform_point3(v)), mat));
            }
        }
    });
    out
}

pub fn export_stl(model: &Model, path: &Path) -> Result<()> {
    let tris = world_triangles(model);
    let mut buf = Vec::with_capacity(84 + tris.len() * 50);
    let mut header = [0u8; 80];
    header[..18].copy_from_slice(b"Esquisse STL (mm) ");
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for ([a, b, c], _) in &tris {
        let n = (*b - *a).cross(*c - *a).normalize_or_zero();
        for v in [n, *a, *b, *c] {
            for x in [v.x, v.y, v.z] {
                buf.extend_from_slice(&(x as f32).to_le_bytes());
            }
        }
        buf.extend_from_slice(&[0, 0]);
    }
    std::fs::write(path, buf)?;
    Ok(())
}

pub fn export_obj(model: &Model, path: &Path) -> Result<()> {
    let tris = world_triangles(model);
    let mtl_path = path.with_extension("mtl");
    let mut obj = String::new();
    let mut mtl = String::new();
    writeln!(obj, "# Esquisse (unités : mm)")?;
    writeln!(obj, "mtllib {}", mtl_path.file_name().and_then(|s| s.to_str()).unwrap_or("model.mtl"))?;
    writeln!(mtl, "newmtl defaut\nKd 1 1 1")?;
    for (_, m) in model.materials.iter() {
        let c = m.color.map(|x| x as f32 / 255.0);
        writeln!(mtl, "newmtl {}\nKd {} {} {}\nd {}", sanitize(&m.name), c[0], c[1], c[2], m.opacity)?;
    }
    let mut current: Option<Option<MaterialId>> = None;
    for (i, ([a, b, c], mat)) in tris.iter().enumerate() {
        for v in [a, b, c] {
            writeln!(obj, "v {} {} {}", v.x, v.y, v.z)?;
        }
        if current != Some(*mat) {
            let name = mat.and_then(|m| model.materials.get(m)).map(|m| sanitize(&m.name));
            writeln!(obj, "usemtl {}", name.unwrap_or_else(|| "defaut".into()))?;
            current = Some(*mat);
        }
        let k = i * 3 + 1;
        writeln!(obj, "f {} {} {}", k, k + 1, k + 2)?;
    }
    std::fs::write(path, obj)?;
    std::fs::write(mtl_path, mtl)?;
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Definition, Entities, Instance};

    /// A `.esq` whose edges and face name a vertex the file does not have: rebuilding the
    /// topology indexed it and the opening panicked.
    #[test]
    fn a_dangling_vertex_key_does_not_panic() {
        let mut m = Model::new();
        let v: Vec<_> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        m.root.add_face_raw(v.clone(), vec![]).unwrap();
        m.root.verts.remove(v[1]);
        let back = load_esq_bytes(&serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(back.root.faces.is_empty());
        assert!(back.root.edges.values().all(|e| e.v.iter().all(|x| back.root.verts.contains_key(*x))));
    }

    /// The deepest nesting of a model, walked iteratively.
    pub fn depth(m: &Model) -> usize {
        let mut deepest = 0;
        let mut stack: Vec<(&crate::model::Entities, usize)> = vec![(&m.root, 0)];
        while let Some((e, d)) = stack.pop() {
            deepest = deepest.max(d);
            for i in e.instances.values() {
                if let Some(def) = m.defs.get(i.def) {
                    stack.push((&def.entities, d + 1));
                }
            }
        }
        deepest
    }

    /// A chain of 100 000 nested definitions and a cycle A → B → A, opened then walked the
    /// ways Esquisse walks a model (display, exports): the nesting is cut at 256 levels and
    /// the cycle broken when the file is read, so no recursive walk overflows the stack.
    #[test]
    fn deep_chains_and_cycles_are_cut_when_read() {
        for (name, m) in
            [("chaîne", crate::repair::samples::chain(100_000)), ("cycle", crate::repair::samples::cycle())]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let bytes = serde_json::to_vec(&m).unwrap();
            std::thread::spawn(move || {
                let back = load_esq_bytes(&bytes).unwrap();
                let dir = std::env::temp_dir().join(format!("esq-profond-{}", std::process::id()));
                std::fs::create_dir_all(&dir).unwrap();
                let tris = world_triangles(&back).len();
                export_obj(&back, &dir.join("p.obj")).unwrap();
                export_stl(&back, &dir.join("p.stl")).unwrap();
                let mut visits = 0;
                crate::walk::visit_all(&back, true, |_| visits += 1);
                let _ = std::fs::remove_dir_all(&dir);
                let _ = tx.send((depth(&back), tris, visits));
            });
            let (d, tris, visits) = rx.recv_timeout(std::time::Duration::from_secs(120)).expect(name);
            assert!(d <= crate::repair::MAX_DEPTH, "{name} : {d}");
            assert!(tris > 0 && visits > 1, "{name}");
        }
    }

    /// A definition holding two instances of itself: every walk of the tree (display,
    /// picking, export) made 2^64 visits down to its depth limit. The cycle is cut when
    /// the file is read.
    #[test]
    fn a_definition_that_contains_itself_is_cut_when_read() {
        let mut m = Model::new();
        let d = m.defs.insert(Definition {
            name: "Boucle".into(),
            is_group: false,
            description: String::new(),
            entities: Entities::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        let i = Instance {
            def: d,
            xform: glam::DMat4::IDENTITY,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        };
        m.defs[d].entities.instances.insert(i.clone());
        m.defs[d].entities.instances.insert(i.clone());
        m.root.instances.insert(i);
        let back = load_esq_bytes(&serde_json::to_vec(&m).unwrap()).unwrap();
        let mut n = 0;
        crate::walk::visit(&back, |_| n += 1);
        assert_eq!(n, 2);
    }
}
