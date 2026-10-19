//! Outils Ruby (zone « tools ») branchés sur la vraie boucle d'outils : `Model#select_tool`
//! remplace l'outil de l'application par un RubyTool, les clics et la boîte de mesures
//! repartent vers Ruby, ce que l'outil dessine arrive dans la liste de dessin de la vue.

use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, Model};
use esq_ruby::RunOutput;
use serde_json::Value as J;

use super::Harness;
use super::ruby::interp;
use crate::camera::Camera;
use crate::ruby_host::tools::ToolsCx;
use crate::ruby_host::view::{ViewCx, ViewState};
use crate::ruby_tools::{State, ruby_str, tool_status};
use crate::style::ViewStyle;
use crate::tools::{Tool, ToolId};

/// L'hôte est global : un seul test à la fois le prête à Ruby (même verrou que la zone vue).
use super::ruby::HOST_LOCK as LOCK;

struct TestHost {
    tool: Box<dyn Tool>,
    st: State,
    vcb: String,
    message: Option<(String, f64)>,
    cam: Camera,
    style: ViewStyle,
    active: Option<usize>,
    vs: ViewState,
}

impl esq_ruby::Host for TestHost {
    fn call(&mut self, model: &mut Model, _sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        if op.starts_with("tools.") {
            let mut cx = ToolsCx { tool: &mut self.tool, st: &mut self.st, vcb: &mut self.vcb };
            return cx.call(op, args);
        }
        // Barre d'état et boîte de mesures (zones modèle et ui), comme dans l'application.
        if op == "model.status" {
            let t = args["text"].as_str().unwrap_or("");
            if !tool_status(self.tool.as_ref(), 0, t) {
                self.message = Some((t.to_string(), 0.0));
            }
            return Ok(J::Null);
        }
        if op == "ui.vcb" {
            for (pos, key) in [(1, "label"), (2, "value")] {
                if let Some(t) = args[key].as_str() {
                    tool_status(self.tool.as_ref(), pos, t);
                }
            }
            return Ok(J::Null);
        }
        let mut cx = ViewCx {
            cam: &mut self.cam,
            style: &mut self.style,
            active_scene: &mut self.active,
            anim: (2.0, 1.0),
            vs: &mut self.vs,
        };
        cx.call(model, op, args)
    }
}

fn host() -> TestHost {
    TestHost {
        tool: ToolId::Select.make(),
        st: State::default(),
        vcb: String::new(),
        message: None,
        cam: Camera { size: DVec2::new(800.0, 600.0), ..Default::default() },
        style: ViewStyle::default(),
        active: None,
        vs: ViewState::default(),
    }
}

fn run(h: &mut TestHost, m: &mut Model, code: &str) -> Option<RunOutput> {
    let i = interp()?;
    let mut sel = Vec::new();
    let o = esq_ruby::with_host(h, || i.run(code, "outil.rb", m, &mut sel)).unwrap();
    assert!(o.ok, "erreur Ruby: {}\n{}", o.error, o.out);
    Some(o)
}

/// Ce que `ruby_tools::frame` fait à chaque image : la file d'événements de l'outil part vers Ruby.
fn deliver(h: &mut TestHost, m: &mut Model) -> Option<RunOutput> {
    let sh = h.tool.ruby().expect("outil Ruby actif");
    let (token, events) = {
        let mut s = sh.borrow_mut();
        (s.token, std::mem::take(&mut s.queue))
    };
    let code = format!("Esq::Tools.deliver({token}, JSON.parse({}), true)", ruby_str(&J::Array(events).to_string()));
    run(h, m, &code)
}

const TOOL: &str = r#"
Esq::Tools.reset!; Esq::Events.reset!
class EsqLineTool
  attr_reader :log
  def initialize; @log = []; @pts = []; end
  def activate
    @log << :activate
    Sketchup.set_status_text('Cliquez deux points')
    Sketchup.vcb_label = 'Longueur'
  end
  def deactivate(view); @log << :deactivate; end
  def enableVCB?; true; end
  def onLButtonDown(flags, x, y, view)
    @pts << [x / 10.0, y / 10.0, 0]
    if @pts.size == 2
      Sketchup.active_model.entities.add_line(*@pts)
      @pts.clear
    end
    Sketchup.vcb_value = @pts.size.to_s
    view.invalidate
  end
  def onUserText(text, view); @log << [:text, text]; end
  def onCancel(reason, view); @log << [:cancel, reason]; @pts.clear; end
  def onReturn(view); @log << :return; end
  def draw(view)
    view.drawing_color = 'red'
    view.draw_line([0, 0, 0], [1, 0, 0])
  end
end
$esq_tool = EsqLineTool.new
"#;

#[test]
fn select_tool_puts_a_ruby_tool_in_the_application() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = host();
    let mut m = Model::new();
    if run(&mut h, &mut m, TOOL).is_none() {
        return;
    }
    run(&mut h, &mut m, "Sketchup.active_model.select_tool($esq_tool)");
    assert_eq!(h.tool.id(), ToolId::Ruby);
    assert_eq!(h.tool.status(), "Cliquez deux points");
    assert_eq!(h.tool.vcb_label_text(), "Longueur");
    let sh = h.tool.ruby().unwrap();
    assert!(sh.borrow().vcb, "enableVCB? → boîte de mesures");
    assert_eq!(sh.borrow().id, 50000);
    assert_eq!(h.st.reported, Some((ToolId::Ruby, sh.borrow().token)));

    // Deux clics dans la vraie boucle d'outils (Harness), puis livraison à Ruby.
    let mut hn = Harness::new();
    std::mem::swap(&mut hn.tool, &mut h.tool);
    hn.with(|t, cx| {
        t.press(cx, DVec2::new(10.0, 20.0));
        t.release(cx, DVec2::new(10.0, 20.0));
        t.press(cx, DVec2::new(50.0, 20.0));
    });
    let typed = hn.with(|t, cx| t.vcb(cx, "120"));
    hn.with(|t, cx| t.cancel(cx));
    hn.with(|t, cx| t.enter(cx));
    assert!(hn.tool.busy(), "Échap va à onCancel, l'outil reste actif");
    std::mem::swap(&mut hn.tool, &mut h.tool);
    assert!(typed.is_ok());
    deliver(&mut h, &mut m);
    assert_eq!(m.root.edges.len(), 1, "le second clic trace une arête");
    let e = m.root.edges.values().next().unwrap();
    let a = m.root.verts[e.v[0]].pos;
    assert!((a - DVec3::new(25.4, 50.8, 0.0)).length() < 1e-6 || (a - DVec3::new(127.0, 50.8, 0.0)).length() < 1e-6);
    assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "0");
    assert!(h.st.redraw, "view.invalidate demande un nouveau dessin");
    assert!(!h.vs.draw.is_empty(), "draw(view) remplit la liste de dessin de la vue");
    let o = run(&mut h, &mut m, "$esq_tool.log").unwrap();
    assert_eq!(o.result, "[:activate, [:text, \"120\"], [:cancel, 0], :return]");

    // Tools#push_tool suspend l'outil, pop_tool le reprend ; select_tool(nil) revient à Sélectionner.
    run(&mut h, &mut m, "Sketchup.active_model.tools.push_tool(Object.new)");
    assert_eq!(h.st.suspended.len(), 1);
    let o = run(&mut h, &mut m, "t = Sketchup.active_model.tools; [t.active_tool_name, t.active_tool_id]").unwrap();
    assert_eq!(o.result, "[\"RubyTool\", 50001]");
    run(&mut h, &mut m, "Sketchup.active_model.tools.pop_tool");
    assert!(h.st.suspended.is_empty());
    assert_eq!(h.tool.ruby().unwrap().borrow().token, sh.borrow().token);
    run(&mut h, &mut m, "Sketchup.active_model.select_tool(nil)");
    assert_eq!(h.tool.id(), ToolId::Select);
    let o = run(
        &mut h,
        &mut m,
        "t = Sketchup.active_model.tools; [t.active_tool_name, t.active_tool_id, $esq_tool.log.last]",
    )
    .unwrap();
    assert_eq!(o.result, "[\"SelectionTool\", 21022, :deactivate]");
    assert!(h.vs.draw.is_empty(), "l'outil parti, son dessin s'efface");
}

#[test]
fn native_tool_names_and_status_without_ruby_tool() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = host();
    let mut m = Model::new();
    h.tool = ToolId::PushPull.make();
    let Some(o) = run(
        &mut h,
        &mut m,
        "Esq::Tools.reset!
         t = Sketchup.active_model.tools
         Sketchup.set_status_text('Bonjour')
         [t.active_tool_name, t.active_tool_id, t.active_tool]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"PushPullTool\", 21041, nil]");
    assert_eq!(h.message.as_ref().map(|m| m.0.as_str()), Some("Bonjour"));
}

#[test]
fn overlays_reach_the_application() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = host();
    let mut m = Model::new();
    let Some(_) = run(
        &mut h,
        &mut m,
        "Esq::Overlays.reset!; Esq::Overlays.prefs.clear
         class EsqOv < Sketchup::Overlay
           def draw(view); view.draw_points([[0, 0, 0]], 8, 2, 'blue'); end
         end
         $esq_ov = EsqOv.new('esq.test.app', 'Repère', description: 'Un point bleu au centre')
         Sketchup.active_model.overlays.add($esq_ov)",
    ) else {
        return;
    };
    assert_eq!(h.st.overlays, vec![("esq.test.app".to_string(), "Repère".to_string(), false)]);
    // Window › Overlays shows its description and where it comes from.
    let (desc, _source) = &h.st.overlay_info["esq.test.app"];
    assert_eq!(desc, "Un point bleu au centre");
    run(&mut h, &mut m, "$esq_ov.enabled = true; Esq::Tools.render_all");
    assert!(h.st.overlays[0].2);
    assert_eq!(h.vs.draw.len(), 1, "la surcouche activée dessine");
    run(
        &mut h,
        &mut m,
        "Sketchup.active_model.overlays.remove($esq_ov); Esq::Overlays.prefs.clear; Esq::Tools.render_all",
    );
    assert!(h.st.overlays.is_empty());
    assert!(h.vs.draw.is_empty());
}
