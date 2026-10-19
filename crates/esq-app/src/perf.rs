//! Work counters: how much the view, the inference and the undo
//! history do, counted rather than timed, so that tests can bound it while other programs
//! slow the machine down. Per thread: tests running side by side do not see each other's.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Builds of the scene's mesh (`scene::build`).
    pub mesh_builds: u64,
    /// Builds of the picking cache (`PickCache::build`).
    pub pick_builds: u64,
    /// Triangles, edges and points the inference and the picking looked at.
    pub pick_visits: u64,
    /// Bytes of model copied into the undo history.
    pub undo_bytes: u64,
    /// Faces triangulated for the view and for picking.
    pub tessellations: u64,
    /// Updates of some parts of the scene's mesh (`scene::update_parts`).
    pub mesh_updates: u64,
    /// Shadow maps drawn (`Viewport3d::render`).
    pub shadow_passes: u64,
    /// Attached annotations put in step with the model, and the entities they looked up
    /// (`esq_core::attach`).
    pub attach_annots: u64,
    pub attach_lookups: u64,
}

thread_local! {
    static COUNTERS: Cell<Counters> = const {
        Cell::new(Counters {
            mesh_builds: 0,
            pick_builds: 0,
            pick_visits: 0,
            undo_bytes: 0,
            tessellations: 0,
            mesh_updates: 0,
            shadow_passes: 0,
            attach_annots: 0,
            attach_lookups: 0,
        })
    };
}

/// Adds to the counters of this thread.
pub fn count(f: impl FnOnce(&mut Counters)) {
    COUNTERS.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    });
}

/// The counters of this thread.
pub fn get() -> Counters {
    COUNTERS.with(|c| c.get())
}

impl Counters {
    /// What was counted since `before`.
    pub fn since(self, before: Counters) -> Counters {
        let Counters {
            mesh_builds,
            pick_builds,
            pick_visits,
            undo_bytes,
            tessellations,
            mesh_updates,
            shadow_passes,
            attach_annots,
            attach_lookups,
        } = self;
        Counters {
            mesh_builds: mesh_builds - before.mesh_builds,
            pick_builds: pick_builds - before.pick_builds,
            pick_visits: pick_visits - before.pick_visits,
            undo_bytes: undo_bytes - before.undo_bytes,
            tessellations: tessellations - before.tessellations,
            mesh_updates: mesh_updates - before.mesh_updates,
            shadow_passes: shadow_passes - before.shadow_passes,
            attach_annots: attach_annots - before.attach_annots,
            attach_lookups: attach_lookups - before.attach_lookups,
        }
    }

    /// Adds what another thread counted (work done there for this one).
    pub fn add(&mut self, d: Counters) {
        let Counters {
            mesh_builds,
            pick_builds,
            pick_visits,
            undo_bytes,
            tessellations,
            mesh_updates,
            shadow_passes,
            attach_annots,
            attach_lookups,
        } = d;
        self.mesh_builds += mesh_builds;
        self.pick_builds += pick_builds;
        self.pick_visits += pick_visits;
        self.undo_bytes += undo_bytes;
        self.tessellations += tessellations;
        self.mesh_updates += mesh_updates;
        self.shadow_passes += shadow_passes;
        self.attach_annots += attach_annots;
        self.attach_lookups += attach_lookups;
    }
}

/// Tests: the allocator counts the allocations of each thread (`alloc::count`).
#[cfg(test)]
pub mod alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static N: Cell<u64> = const { Cell::new(0) };
    }

    pub struct Counting;

    fn note() {
        // No destructor, no allocation: safe from inside the allocator.
        let _ = N.try_with(|n| n.set(n.get() + 1));
    }

    // SAFETY: every call is passed on to the system's allocator unchanged.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            note();
            unsafe { System.alloc(l) }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            note();
            unsafe { System.alloc_zeroed(l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
            note();
            unsafe { System.realloc(p, l, size) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
    }

    /// Allocations made by this thread so far.
    pub fn count() -> u64 {
        N.try_with(|n| n.get()).unwrap_or(0)
    }
}

#[cfg(test)]
#[global_allocator]
static ALLOC: alloc::Counting = alloc::Counting;

#[cfg(test)]
mod tests {
    use esq_core::glam::{DMat4, DVec2, DVec3};
    use esq_core::model::{Definition, Instance, Model};

    /// An image of an orbit over a model of 3 000 copies of a component, without anything
    /// changing: what the view draws over the 3D image (snaps, spaces, lights, guide points,
    /// texts) does not walk the model, nor allocate for each copy.
    #[test]
    fn an_orbit_image_without_change_allocates_almost_nothing() {
        let mut m = Model::new();
        let mut e = esq_core::model::Entities::default();
        let c = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| e.add_vertex_raw(q * 100.0));
        e.add_face_raw(c.to_vec(), Vec::new());
        let d = m.defs.insert(Definition {
            name: "Carré".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for i in 0..3000 {
            m.root.instances.insert(Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new((i % 60) as f64 * 150.0, (i / 60) as f64 * 150.0, 0.0)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        let mut app = crate::app::EsquisseApp::bare(None, None);
        app.doc = crate::doc::Document::new(m);
        app.cam.size = DVec2::new(800.0, 600.0);
        let ctx = eframe::egui::Context::default();
        let rect = eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(800.0, 600.0));
        let painter = eframe::egui::Painter::new(ctx, eframe::egui::LayerId::background(), rect);
        let frame = |app: &mut crate::app::EsquisseApp| {
            app.cam.orbit_with(4.0, 0.0, true);
            app.follow_eye();
            app.ensure_overlay_pick();
            let o = crate::tools::Overlay {
                painter: painter.clone(),
                origin: eframe::egui::Pos2::ZERO,
                cam: &app.cam,
                doc: &app.doc,
                units: app.doc.model.units,
            };
            app.paint_model_overlay(&o);
        };
        frame(&mut app);
        let before = super::alloc::count();
        for _ in 0..10 {
            frame(&mut app);
        }
        let n = super::alloc::count() - before;
        eprintln!("{n} allocations pour 10 images");
        assert!(n < 10 * 100, "{n} allocations pour 10 images");
    }
}

/// About how many bytes a copy of `e` takes: its slots and the lists they own (attributes
/// and texts left out).
pub fn entities_bytes(e: &esq_core::model::Entities) -> u64 {
    use esq_core::model::{Annotation, Edge, Face, Guide, Instance, SectionPlane, Vertex};
    use std::mem::size_of;
    let id = size_of::<esq_core::model::VertexId>() as u64;
    let slots = |n: usize, s: usize| (n * (s + 8)) as u64;
    slots(e.verts.capacity(), size_of::<Vertex>())
        + e.verts.values().map(|v| v.edges.len() as u64 * id).sum::<u64>()
        + slots(e.edges.capacity(), size_of::<Edge>())
        + e.edges.values().map(|x| x.faces.len() as u64 * id).sum::<u64>()
        + slots(e.faces.capacity(), size_of::<Face>())
        + e.faces
            .values()
            .map(|f| (f.outer.len() + f.holes.iter().map(Vec::len).sum::<usize>()) as u64 * id)
            .sum::<u64>()
        + slots(e.instances.capacity(), size_of::<Instance>())
        + slots(e.guides.capacity(), size_of::<Guide>())
        + slots(e.annots.capacity(), size_of::<Annotation>())
        + slots(e.sections.capacity(), size_of::<SectionPlane>())
}

/// About how many bytes a copy of the model's geometry takes (`entities_bytes` of each
/// collection).
#[cfg_attr(not(test), allow(dead_code))]
pub fn model_bytes(m: &esq_core::model::Model) -> u64 {
    entities_bytes(&m.root) + m.defs.values().map(|d| entities_bytes(&d.entities)).sum::<u64>()
}
