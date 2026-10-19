//! Tools › Comment: the comments of a model go through a .skp in a dictionary of the model's
//! attributes and come back the same, twice over (Esquisse → .skp → Esquisse → .skp), next to
//! the model's own dictionaries.

use esq_core::comments::{Comment, Comments, SKP_DICT};
use esq_core::glam::DVec3;
use esq_core::model::{AttrValue, Model, SceneCamera, set_attr};

#[test]
fn comments_survive_a_skp_round_trip() {
    let mut m = Model::new();
    set_attr(m.attr_store.entry("model".into()).or_default(), "fiche", "auteur", AttrValue::Text("Théo".into()));
    let mut c = Comment::new(DVec3::new(1200.0, 300.0, 2500.0), "Groupe Mur", "Théo", "Linteau à reprendre ?", 1_000);
    c.camera = Some(SceneCamera {
        eye: DVec3::new(6000.0, -5000.0, 4000.0),
        target: DVec3::new(1200.0, 300.0, 2500.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    });
    c.reply("Alice", "Oui, en béton « préfa »", None, 2_000);
    let mut done = Comment::new(DVec3::ZERO, "", "Alice", "Arête à adoucir", 3_000);
    done.resolved = true;
    let mut comments = Comments::default();
    comments.add(c);
    comments.add(done);
    m.comments = comments;

    let dir = std::env::temp_dir();
    let a = dir.join(format!("esq-comments-a-{}.skp", std::process::id()));
    let b = dir.join(format!("esq-comments-b-{}.skp", std::process::id()));
    esq_skp::export_skp(&m, &a).unwrap();
    let back = esq_skp::import_skp(&a).unwrap();
    esq_skp::export_skp(&back, &b).unwrap();
    let again = esq_skp::import_skp(&b).unwrap();
    // SketchUp reads the file the same (with its SDK, when installed).
    let sdk = esq_skp::available().then(|| esq_skp::sdk_import_skp_view(&b).unwrap().0);
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);

    for got in [&back, &again].into_iter().chain(sdk.as_ref()) {
        assert_eq!(got.comments, m.comments);
        // The dictionary is the comments' own: the model's attributes keep theirs only.
        assert!(!got.attr_store["model"].contains_key(SKP_DICT));
        assert_eq!(got.attr_store["model"]["fiche"]["auteur"], AttrValue::Text("Théo".into()));
    }
}
