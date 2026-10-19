use super::*;

/// Début commun des scripts de test : un observateur qui note tout ce qu'il reçoit, et la remise
/// à zéro du bus (l'interpréteur Ruby est partagé entre les tests ; chaque script s'exécute d'un bloc).
const PRE: &str = r#"
Esq::Events.reset!
class Spy
  attr_reader :log
  def initialize(*names); @log = []; names.each { |n| define_singleton_method(n) { |*a| @log << [n, *a.map { |x| Spy.show(x) }] } }; end
  def self.show(x)
    case x
    when Sketchup::Entity then x.class.name.split('::').last
    when Integer, Float, String, Symbol, true, false, nil then x
    else x.class.name.split('::').last
    end
  end
end
"#;

fn script(body: &str) -> Option<RunOutput> {
    let code = format!("{PRE}\nbegin\n{body}\nensure\n  Esq::Events.reset!\nend");
    let (o, _) = must(&code)?;
    Some(o)
}

#[test]
fn observer_classes_exist_with_their_hierarchy() {
    let Some(o) = script(
        "%w[AppObserver ModelObserver SelectionObserver EntityObserver InstanceObserver DefinitionObserver
            EntitiesObserver PagesObserver DefinitionsObserver LayersObserver MaterialsObserver ViewObserver
            ToolsObserver RenderingOptionsObserver ShadowInfoObserver EnvironmentsObserver
            OptionsProviderObserver DimensionObserver].map { |n| Sketchup.const_get(n).superclass.name }",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Object\", \"Object\", \"Object\", \"Object\", \"Sketchup::EntityObserver\", \"Sketchup::EntityObserver\", \
         \"Object\", \"Sketchup::EntitiesObserver\", \"Object\", \"Object\", \"Object\", \"Object\", \"Object\", \
         \"Object\", \"Object\", \"Object\", \"Object\", \"Object\"]"
    );
}

#[test]
fn add_and_remove_observer_return_values() {
    let Some(o) = script(
        "t = Sketchup.active_model.tools
         s = Spy.new(:onActiveToolChanged)
         [t.add_observer(s), t.add_observer(s), t.remove_observer(s), t.remove_observer(s)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, false, true, false]");
}

/// Sélection changée dans l'interface (pas par Selection#add, que la zone modèle annonce
/// elle-même) : l'application appelle Esq::Events.app_sync.
#[test]
fn selection_observer_hears_interface_changes() {
    let Some(o) = script(
        "model = Sketchup.active_model
         a = model.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         b = model.entities.add_face([5,0,0],[6,0,0],[6,1,0],[5,1,0])
         spy = Spy.new(:onSelectionAdded, :onSelectionRemoved, :onSelectionCleared, :onSelectionBulkChange)
         model.selection.add_observer(spy)
         # Comme l'interface : la sélection change sans passer par Ruby (Esq.__native), puis app_sync.
         ui = ->(*hs) { Esq.__native('sel.set', JSON.generate('hs' => hs.map(&:handle))); Esq::Events.app_sync([]) }
         ui.(a)
         ui.(a, b)
         ui.(b)
         ui.()
         ui.(a, b)
         spy.log",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[:onSelectionAdded, \"Selection\", \"Face\"], [:onSelectionAdded, \"Selection\", \"Face\"], \
         [:onSelectionRemoved, \"Selection\", \"Face\"], [:onSelectionCleared, \"Selection\"], \
         [:onSelectionBulkChange, \"Selection\"]]"
    );
}

#[test]
fn entities_observer_added_modified_removed() {
    let Some(o) = script(
        "model = Sketchup.active_model
         ents = model.entities
         spy = Spy.new(:onElementAdded, :onElementModified, :onElementRemoved)
         ents.add_observer(spy)
         g = ents.add_group
         g.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         g.transformation = Geom::Transformation.translation([5,0,0])
         id = g.entityID
         ents.erase_entities(g)
         [spy.log.map { |e| e[0] }, spy.log.last[2] == id]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[:onElementAdded, :onElementModified, :onElementRemoved], true]");
}

#[test]
fn entity_observer_change_and_erase() {
    let Some(o) = script(
        "model = Sketchup.active_model
         g = model.entities.add_group
         spy = Spy.new(:onChangeEntity, :onEraseEntity)
         g.add_observer(spy)
         g.name = 'a'
         g.name = 'a'
         g.name = 'b'
         g.erase!
         spy.log",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[:onChangeEntity, \"Group\"], [:onChangeEntity, \"Group\"], [:onEraseEntity, \"Group\"]]");
}

#[test]
fn observer_exceptions_are_printed_not_raised() {
    let Some(o) = script(
        "ents = Sketchup.active_model.entities
         bad = Object.new
         def bad.onElementAdded(e, x); raise ArgumentError, 'oups'; end
         ents.add_observer(bad)
         ents.add_line([0,0,0],[1,0,0])
         :fini",
    ) else {
        return;
    };
    assert_eq!(o.result, ":fini");
    assert!(o.out.contains("Error: #<ArgumentError: oups>"), "{}", o.out);
}

#[test]
fn model_observer_transactions() {
    let Some(o) = script(
        "model = Sketchup.active_model
         spy = Spy.new(:onTransactionStart, :onTransactionCommit, :onTransactionEmpty, :onTransactionAbort, :onTransactionUndo)
         model.add_observer(spy)
         model.start_operation('x', true)
         model.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         model.commit_operation
         model.start_operation('vide', true)
         model.commit_operation
         model.entities.add_line([0,0,0],[9,0,0])
         Esq::Events.app_sync(['undo'])
         spy.log.map(&:first)",
    ) else {
        return;
    };
    // Opérations : annoncées par la zone modèle, vide quand elle n'a rien changé (TestUp
    // `TC_Sketchup_Model#test_active_path_Set_single_operation`) ; appel sans opération :
    // transaction implicite ;
    // annulation dans l'interface : onTransactionUndo.
    assert_eq!(
        o.result,
        "[:onTransactionStart, :onTransactionCommit, :onTransactionStart, :onTransactionEmpty, \
         :onTransactionStart, :onTransactionCommit, :onTransactionUndo]"
    );
}

#[test]
fn layers_materials_and_definitions_observers() {
    let Some(o) = script(
        "model = Sketchup.active_model
         ls = Spy.new(:onLayerAdded, :onLayerRemoved, :onLayerChanged)
         ms = Spy.new(:onMaterialAdd, :onMaterialChange, :onMaterialRefChange, :onMaterialRemove)
         model.layers.add_observer(ls)
         model.materials.add_observer(ms)
         model.layers.add('Murs')
         m = model.materials.add('Brique')
         m.color = 'red'
         f = model.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         f.material = m
         [ls.log.map(&:first), ms.log.map(&:first)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[:onLayerAdded], [:onMaterialAdd, :onMaterialChange, :onMaterialRefChange]]");
}

#[test]
fn definition_definitions_and_dimension_observers() {
    let Some(o) = script(
        "model = Sketchup.active_model
         dl = Spy.new(:onComponentAdded, :onComponentPropertiesChanged, :onComponentRemoved)
         model.definitions.add_observer(dl)
         d = model.definitions.add('Chaise')
         d.entities.add_line([0,0,0], [1,0,0])
         ds = Spy.new(:onComponentInstanceAdded, :onComponentInstanceRemoved, :onChangeEntity)
         d.add_observer(ds)
         i = model.entities.add_instance(d, Geom::Transformation.new)
         d.name = 'Fauteuil'
         i.erase!
         dim = model.entities.add_dimension_linear([0,0,0], [10,0,0], [0,5,0])
         dobs = Spy.new(:onTextChanged)
         dim.add_observer(dobs)
         dim.text = 'Dix'
         [dl.log.map(&:first), ds.log.map { |e| e[0..1] }, dobs.log]",
    ) else {
        return;
    };
    // The first copy gives the definition its advanced attributes (`SU_DefinitionSet`):
    // a change of the definition, then the copy added.
    assert_eq!(
        o.result,
        "[[:onComponentAdded, :onComponentPropertiesChanged], [[:onChangeEntity, \"ComponentDefinition\"], \
         [:onComponentInstanceAdded, \"ComponentDefinition\"], [:onChangeEntity, \"ComponentDefinition\"], \
         [:onComponentInstanceRemoved, \"ComponentDefinition\"]], [[:onTextChanged, \"DimensionLinear\"]]]"
    );
}

#[test]
fn tools_stack_select_push_pop() {
    let Some(o) = script(
        "Esq::Tools.reset!
         log = []
         mk = ->(n) { t = Object.new
           %i[activate deactivate suspend resume].each { |m| t.define_singleton_method(m) { |*a| log << [n, m, a.size] } }
           t }
         a = mk.('a'); b = mk.('b')
         model = Sketchup.active_model
         tools = model.tools
         obs = Spy.new(:onActiveToolChanged)
         tools.add_observer(obs)
         r1 = model.select_tool(a)
         id_a = tools.active_tool_id
         r2 = tools.push_tool(b)
         id_b = tools.active_tool_id
         top = tools.active_tool
         popped = tools.pop_tool
         back = tools.active_tool
         model.select_tool(nil)
         [r1.equal?(model), r2, id_a, id_b, top.equal?(b), popped.equal?(b), back.equal?(a), tools.active_tool,
          tools.active_tool_name, tools.active_tool_id, log, obs.log.map { |e| e[2..] }, tools.model.equal?(model)]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[true, true, 50000, 50001, true, true, true, nil, \"SelectionTool\", 21022, \
         [[\"a\", :activate, 0], [\"a\", :suspend, 1], [\"b\", :activate, 0], [\"b\", :deactivate, 1], \
         [\"a\", :resume, 1], [\"a\", :deactivate, 1]], \
         [[\"RubyTool\", 50000], [\"RubyTool\", 50001], [\"RubyTool\", 50000], [\"SelectionTool\", 21022]], true]"
    );
}

/// Les identifiants des outils abandonnés reviennent quand l'interface est au repos
/// (`Esq::Tools.idle`), les derniers libérés d'abord : pendant un script, chaque
/// outil choisi en a un neuf (TestUp `TC_Sketchup_Model#test_select_tool_diff_tools`).
#[test]
fn tools_cannot_be_instantiated_and_ids_are_reused() {
    let Some(o) = script(
        "Esq::Tools.reset!
         err = (Sketchup::Tools.new rescue $!)
         m = Sketchup.active_model
         m.select_tool(Object.new); a = m.tools.active_tool_id
         m.select_tool(Object.new); b = m.tools.active_tool_id
         m.select_tool(nil)
         Esq::Tools.idle
         m.select_tool(Object.new); c = m.tools.active_tool_id
         m.select_tool(nil)
         [err.class, err.message, a, b, c]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[TypeError, \"allocator undefined for Sketchup::Tools\", 50000, 50001, 50001]");
}

#[test]
fn ruby_tool_receives_events_with_sketchup_arguments() {
    let Some(o) = script(
        "Esq::Tools.reset!
         class T
           attr_reader :log
           def initialize; @log = []; end
           def onMouseMove(f, x, y, v); @log << [:move, f, x, y, v.class.name]; end
           def onLButtonDown(f, x, y, v); @log << [:down, f, x, y]; end
           def onKeyDown(k, r, f, v); @log << [:key, k, r, f]; end
           def onUserText(t, v); @log << [:text, t]; end
           def onCancel(r, v); @log << [:cancel, r]; end
           def onReturn(v); @log << [:return]; end
           def onMouseWheel(f, d, x, y, v); @log << [:wheel, d]; true; end
           def onMouseLeave(v); @log << [:leave]; end
         end
         t = T.new
         Sketchup.active_model.select_tool(t)
         tk = Esq::Tools.active_token
         w = Esq::Tools.deliver(tk, [['onMouseMove', 4, 10, 20], ['onLButtonDown', 1, 10, 20], ['onKeyDown', 17, 1, 8],
                                 ['onUserText', '12mm'], ['onReturn'], ['onCancel', 0], ['onMouseLeave'], ['onMouseWheel', 0, -1, 3, 4]])
         Sketchup.active_model.select_tool(nil)
         [w, t.log]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[true, [[:move, 4, 10.0, 20.0, \"Sketchup::View\"], [:down, 1, 10.0, 20.0], [:key, 17, 1, 8], \
         [:text, \"12mm\"], [:return], [:cancel, 0], [:leave], [:wheel, -1]]]"
    );
}

#[test]
fn tool_callback_errors_are_printed_and_tool_stays_active() {
    let Some(o) = script(
        "Esq::Tools.reset!
         t = Object.new
         def t.onLButtonDown(*); raise 'boum'; end
         Sketchup.active_model.select_tool(t)
         Esq::Tools.deliver(Esq::Tools.active_token, [['onLButtonDown', 1, 0, 0]])
         r = Sketchup.active_model.tools.active_tool.equal?(t)
         Sketchup.active_model.select_tool(nil)
         r",
    ) else {
        return;
    };
    assert_eq!(o.result, "true");
    assert!(o.out.contains("Error: #<RuntimeError: boum>"), "{}", o.out);
}

#[test]
fn render_calls_draw_and_get_extents_of_tool_and_enabled_overlays() {
    let Some(o) = script(
        "Esq::Tools.reset!; Esq::Overlays.reset!
         log = []
         t = Object.new
         t.define_singleton_method(:draw) { |v| log << [:tool, v.class.name]; v.draw_line([0,0,0], [1,0,0]) }
         t.define_singleton_method(:getExtents) { Geom::BoundingBox.new.tap { |b| b.add(Geom::Point3d.new(0,0,0), Geom::Point3d.new(2,3,4)) } }
         ov = Class.new(Sketchup::Overlay) { def draw(v); $ov_drawn = true; end }.new('esq.test.r', 'R')
         off = Class.new(Sketchup::Overlay) { def draw(v); $off_drawn = true; end }.new('esq.test.off', 'Off')
         m = Sketchup.active_model
         m.overlays.add(ov); m.overlays.add(off); ov.enabled = true
         m.select_tool(t)
         log.clear; $ov_drawn = false
         Esq::Tools.render_all
         ext = Esq::Tools.extents_of(t)
         m.select_tool(nil); m.overlays.remove(ov); m.overlays.remove(off); Esq::Overlays.prefs.clear
         [log, $ov_drawn, $off_drawn, ext]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[[:tool, \"Sketchup::View\"]], true, nil, [[0.0, 0.0, 0.0], [2.0, 3.0, 4.0]]]", "{}", o.out);
}

#[test]
fn view_pages_and_environment_observers_use_the_view_lists() {
    let Some(o) = script(
        "m = Sketchup.active_model
         vs = Spy.new(:onViewChanged)
         ps = Spy.new(:onElementAdded, :onElementRemoved, :onContentsModified)
         fs = Spy.new(:frameChange)
         m.active_view.add_observer(vs)
         m.pages.add_observer(ps)
         fid = Sketchup::Pages.add_frame_change_observer(fs)
         p1 = m.pages.add('Une')
         p2 = m.pages.add('Deux')
         m.pages.erase(p1)
         Esq::Events.app_sync(['view', \"frame:#{p2.uid}:#{p2.uid}:0.5\"])
         kinds = Esq::Events.app_sync([])
         m.active_view.remove_observer(vs)
         Sketchup::Pages.remove_frame_change_observer(fid)
         [vs.log, ps.log.map(&:first), fs.log, kinds.split(',').sort]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[[:onViewChanged, \"View\"]], [:onElementAdded, :onContentsModified, :onElementAdded, \
         :onContentsModified, :onElementRemoved], [[:frameChange, \"Page\", \"Page\", 0.5]], \
         [\"frame\", \"pages\", \"view\"]]"
    );
}

#[test]
fn procedures_register_and_unregister() {
    let Some(o) = script(
        "p = Sketchup::Procedure.new('esq.test.proc', 'Escalier', 'Un escalier', 1)
         e1 = (p.clone rescue $!)
         e2 = (Sketchup::Procedure.new(1, 'a', 'b', 1) rescue $!)
         e3 = (Sketchup.register_procedure('x') rescue $!)
         r = [Sketchup.register_procedure(p), Sketchup.register_procedure(p),
              Test.unregister_procedure('esq.test.proc'), Test.unregister_procedure('esq.test.proc')]
         [r, e1.message, e2.class, e2.message, e3.message]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[true, false, true, false], \"Cannot duplicate or clone Sketchup::EntitiesBuilder.\", TypeError, \
         \"no implicit conversion of Integer into String\", \"wrong argument type String (expected su_proc_t)\"]"
    );
}

#[test]
fn overlays_add_enable_remove() {
    let Some(o) = script(
        "Esq::Overlays.reset!
         class Ov < Sketchup::Overlay
           attr_reader :log
           def initialize; super('esq.test.ov', 'Mon calque', description: 'Essai'); @log = []; end
           def start; @log << :start; end
           def stop; @log << :stop; end
         end
         class Bad < Sketchup::Overlay
           def initialize; end
         end
         ov = Ov.new
         mgr = Sketchup.active_model.overlays
         e1 = (ov.send(:enabled=, true) rescue $!.message)
         errs = [(Sketchup::Overlay.new('', 'x') rescue $!.message), (Sketchup::Overlay.new('a', '') rescue $!.message),
                 (Sketchup::Overlay.new('a') rescue $!.message), (ov.clone rescue $!.message),
                 (mgr.add(Bad.new) rescue $!.message), (mgr.add('x') rescue $!.message), (mgr.each rescue $!.class)]
         a1 = mgr.add(ov)
         a2 = mgr.add(Ov.new)
         en0 = ov.enabled?
         ov.enabled = true
         ov.enabled = true
         en1 = ov.enabled?
         list = [mgr.size, mgr.length, mgr[0].equal?(ov), mgr.at(1), mgr[-1], mgr.to_a.size]
         r1 = mgr.remove(ov)
         r2 = mgr.remove(ov)
         Esq::Overlays.prefs.clear
         [ov.overlay_id, ov.name, ov.description, ov.source, ov.valid?, e1, errs, a1, a2, en0, en1, list, r1, r2, ov.log]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"esq.test.ov\", \"Mon calque\", \"Essai\", \"Extension: unknown\", false, \
         \"overlays must be added to a model to toggle them on/off\", \
         [\"overlay id cannot be an empty string\", \"overlay name cannot be an empty string\", \
         \"wrong number of arguments (given 1, expected 2)\", \"Cannot duplicate or clone Sketchup::Overlay.\", \
         \"invalid overlay object. Did you forget to call super in initialize?\", \
         \"wrong argument type String (expected sketchup_model_service)\", LocalJumpError], \
         true, false, false, true, [1, 1, true, nil, nil, 1], true, false, [:start, :stop]]"
    );
}

#[test]
fn tool_get_menu_goes_through_the_plugins_context_menu() {
    let Some(o) = script(
        "Esq::Tools.reset!
         $got = nil
         t = Object.new
         def t.getMenu(menu, flags, x, y, view)
           $got = [flags, x, y, view.class.name]
           menu.add_item('Terminer') { $done = true }
           true
         end
         Sketchup.active_model.select_tool(t)
         Esq::Tools.deliver(Esq::Tools.active_token, [['onRButtonDown', 2, 40, 50]])
         rows = JSON.parse(Esq.context_menu_json)
         r1 = rows.map { |r| [r['title'], r['menu']] }
         id = rows.find { |r| r['title'] == 'Terminer' }['id']
         Esq.invoke_command(id)
         # Un outil dont getMenu renvoie false laisse le menu habituel.
         u = Object.new
         def u.getMenu(menu); menu.add_item('Non') {}; false; end
         Sketchup.active_model.select_tool(u)
         r2 = JSON.parse(Esq.context_menu_json).map { |r| r['title'] }
         Sketchup.active_model.select_tool(nil)
         [r1, $got, $done, r2]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[[\"Terminer\", \"Context/Tool\"]], [2, 40.0, 50.0, \"Sketchup::View\"], true, []]");
}

#[test]
fn environment_observer_add_change_current_remove() {
    let path = std::env::temp_dir().join("esq_tools_env_test.hdr");
    std::fs::write(&path, b"#?RADIANCE\n").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    let Some(o) = script(&format!(
        "envs = Sketchup.active_model.environments
         s = Spy.new(:onEnvironmentAdd, :onEnvironmentChange, :onEnvironmentSetCurrent, :onEnvironmentRemove)
         envs.add_observer(s)
         e = envs.add('Ciel', '{p}')
         e.rotation = 90
         envs.current = e
         envs.remove(e)
         envs.remove_observer(s)
         s.log.map {{ |x| [x[0], x[1], x[2]] }}"
    )) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[:onEnvironmentAdd, \"Environments\", \"Environment\"], [:onEnvironmentChange, \"Environments\", \"Environment\"], \
         [:onEnvironmentSetCurrent, \"Environments\", \"Environment\"], [:onEnvironmentRemove, \"Environments\", \"Environment\"], \
         [:onEnvironmentSetCurrent, \"Environments\", nil]]"
    );
}
