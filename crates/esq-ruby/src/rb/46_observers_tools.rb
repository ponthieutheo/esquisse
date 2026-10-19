# encoding: utf-8
# Esquisse — 46_observers_tools (observateurs et outils)
#
#   1. les classes d'observateurs (Sketchup::*Observer), vides comme dans SketchUp ;
#   2. le bus d'événements (Esq::Events) : add_observer / remove_observer des classes du modèle,
#      et l'émission réelle des événements, déduite en comparant l'état du modèle avant/après
#      (opération native `tools.state`) à chaque appel qui modifie le modèle, et à chaque
#      changement venu de l'interface (Esq::Events.app_sync, appelé par l'application). View,
#      Pages, RenderingOptions, ShadowInfo et Environments gardent leurs propres listes
#      (44_view_camera.rb) : les événements leur sont livrés par ces listes ;
#   3. Sketchup::Tools / Model#select_tool et le protocole d'outil Ruby ;
#   4. Sketchup::Overlay / OverlaysManager, Sketchup::Procedure, la barre d'état.

module Sketchup
  # ------------------------------------------------------------------ observateurs
  # SketchUp appelle seulement les rappels que l'utilisateur écrit (il teste `respond_to?`
  # avant chaque appel). Seuls AppObserver (onQuit, onNewModel, onOpenModel) et
  # DimensionObserver (onTextChanged) en définissent, qui renvoient false.
  class AppObserver
    def onQuit; false; end
    def onNewModel(_model); false; end
    def onOpenModel(_model); false; end
  end
  class ModelObserver; end
  class SelectionObserver; end
  class EntityObserver; end
  class InstanceObserver < EntityObserver; end
  class DefinitionObserver < EntityObserver; end
  class EntitiesObserver; end
  class PagesObserver < EntitiesObserver; end
  class DefinitionsObserver; end
  class LayersObserver; end
  class MaterialsObserver; end
  class ViewObserver; end
  class ToolsObserver; end
  class RenderingOptionsObserver; end
  class ShadowInfoObserver; end
  class EnvironmentsObserver; end
  class OptionsProviderObserver; end
  class DimensionObserver
    def onTextChanged(_dimension); false; end
  end
end

module Esq
  class << self
    # `host.tools.<op>` (ruby_host/tools.rs) ; `fallback` hors de l'application (tests).
    def tools_host(op, args = {}, fallback = nil)
      call("host.tools.#{op}", args)
    rescue RuntimeError => e
      msg = e.message.to_s.dup.force_encoding(Encoding::UTF_8)
      raise unless msg.start_with?("l'application n'est pas disponible", 'opération hôte')
      fallback
    end
  end

  # Conversions comme celles du code natif de SketchUp.
  module ToolConv
    module_function

    def type_name(o)
      case o
      when nil then 'nil'
      when true then 'true'
      when false then 'false'
      else o.class.name
      end
    end

    # StringValue : String, ou objet qui répond à to_str.
    def str(o)
      return o if o.is_a?(String)
      return o.to_str if o.respond_to?(:to_str)
      raise TypeError, "no implicit conversion of #{type_name(o)} into String"
    end

    # NUM2LONG : entier (un Float est tronqué).
    def long(o)
      return o if o.is_a?(Integer)
      return o.to_i if o.is_a?(Float)
      raise TypeError, 'no implicit conversion from nil to integer' if o.nil?
      return o.to_int if o.respond_to?(:to_int)
      raise TypeError, "no implicit conversion of #{type_name(o)} into Integer"
    end

    def truthy(o); !(o.nil? || o == false); end
  end

  # Bus d'événements : qui observe quoi, comparaison d'états, livraison des callbacks.
  module Events
    Reg = Struct.new(:kind, :key, :source, :observer)
    # Remplacé par l'objet auquel l'observateur a été ajouté (premier argument de la plupart
    # des callbacks : `onElementAdded(entities, entity)`).
    SRC = Object.new.freeze

    # Objets qui gardent eux-mêmes leur liste d'observateurs (zones « vue » et « modèle ») :
    # genre → classe dont `__instance` est l'objet, ou méthode du modèle qui le donne.
    EXT = { view: :View, pages: :Pages, rendering_options: :RenderingOptions, shadow: :ShadowInfo,
            environments: :Environments, model: :model, selection: :selection, layers: :layers,
            materials: :materials }.freeze
    # Ceux dont les changements faits en Ruby sont déjà annoncés par la zone propriétaire : on
    # n'émet que les changements venus de l'interface.
    OWN_RUBY_EVENTS = %i[selection layers materials shadow rendering_options].freeze

    @regs = []
    @last = nil
    @busy = false
    @dirty = false
    @op_depth = 0
    @op_changed = false
    @objs = {}
    @undoing = false
    @ro_last = nil
    @ext_count = 0

    class << self
      attr_reader :regs

      def reset!
        @regs = []
        @last = nil
        @busy = false
        @dirty = false
        @op_depth = 0
        @op_changed = false
        @objs = {}
        @ro_last = nil
        EXT.each_key do |k|
          src = ext_source(k)
          ext_list(src).dup.each { |o| src.remove_observer(o) } if src
        end
        @ext_count = 0
        ent_lists.clear
        @ent_objs = {}
        notify_host
      end

      def watching?
        !@regs.empty? || @ext_count.positive? || ent_lists.any? { |k, l| k.is_a?(String) && !l.empty? }
      end

      def watching(kind)
        return !ext_observers(kind).empty? if EXT.key?(kind)
        return !ent_observers('dl:0:0').empty? if kind == :definitions
        @regs.any? { |r| r.kind == kind }
      end

      # ---------------------------------------------------------------- listes de la zone entités
      # Entity#add_observer et Entities#add_observer (41_entities_extra.rb) rangent leurs
      # observateurs dans Esq.observers : poignée de l'entité, ou « entities:<ctx> ». Les
      # définitions ont la poignée « d:0:<ctx> », la liste des définitions « dl:0:0 ».
      def ent_lists
        Esq.respond_to?(:observers) ? Esq.observers : {}
      end

      def ent_observers(key)
        l = ent_lists
        l.key?(key) ? l[key].dup : []
      end

      # Clés des entités observées (Esq.observers[:app] est la liste des AppObserver).
      def ent_watch_keys
        ent_lists.select { |k, l| k.is_a?(String) && !l.empty? }.keys
      end

      def app_observers
        ent_lists.key?(:app) ? ent_lists[:app].dup : []
      end

      # L'objet Ruby d'une poignée observée, gardé pour pouvoir le passer après sa disparition.
      def ent_obj(h)
        @ent_objs ||= {}
        @ent_objs[h] ||= begin
          if h.start_with?('d:0:') then Sketchup::ComponentDefinition.from_ctx(h.split(':')[2])
          elsif h == 'dl:0:0' then model.definitions
          else wrap(h)
          end
        rescue StandardError
          nil
        end
      end

      # Avant / après Entity#add_observer et Entities#add_observer de la zone entités.
      def before_ent_add
        flush unless @busy
      end

      def after_ent_add
        ent_watch_keys.each { |h| ent_obj(h) unless h.start_with?('entities:') }
        rebaseline unless @busy
        notify_host
      end

      def regs_of(kind, key = nil); @regs.select { |r| r.kind == kind && (key.nil? || r.key == key) }; end

      # ---------------------------------------------------------------- classes
      def klass(name)
        Sketchup.const_defined?(name, false) ? Sketchup.const_get(name, false) : nil
      end

      def is?(src, name)
        k = klass(name)
        k ? src.is_a?(k) : false
      end

      def ext_instance(name)
        k = klass(name)
        k.respond_to?(:__instance) ? k.__instance : nil
      rescue StandardError
        nil
      end

      # L'objet observé d'un genre qui garde sa liste.
      def ext_source(kind)
        n = EXT[kind]
        return nil unless n
        n.to_s[0] =~ /[a-z]/ ? (n == :model ? model : model.public_send(n)) : ext_instance(n)
      rescue StandardError
        nil
      end

      def ext_list(src)
        return [] if src.nil?
        l = src.respond_to?(:__observers) ? src.__observers : src.instance_variable_get(:@observers)
        l.is_a?(Array) ? l : []
      end

      # [[objet observé, observateur]…] pour un objet qui garde sa liste.
      def ext_observers(kind)
        src = ext_source(kind)
        ext_list(src).dup.map { |o| [src, o] }
      end

      # Une liste propre à une classe a changé (add_observer / remove_observer).
      def ext_changed(added)
        @ext_count = EXT.keys.sum { |k| ext_observers(k).size }
        if added && !@busy
          flush
          rebaseline
        end
        notify_host
      end

      # [genre d'observateur, clé de la source] d'un objet auquel on ajoute un observateur.
      def classify(src)
        return [:tools, 'tools'] if is?(src, :Tools)
        [nil, nil]
      end

      def handle_of(e)
        h = e.respond_to?(:handle) ? e.handle : e.instance_variable_get(:@handle)
        h.nil? ? "obj:#{e.object_id}" : h.to_s
      end

      def ctx_of(e)
        if e.respond_to?(:ctx) then e.ctx.to_s
        elsif (v = e.instance_variable_get(:@ctx)) then v.to_s
        else '0'
        end
      end

      def options_name(p); p.respond_to?(:name) ? p.name.to_s : p.object_id.to_s; end

      # ---------------------------------------------------------------- enregistrement
      # Renvoie true si l'observateur a été ajouté, false s'il l'était déjà.
      def add(src, obs)
        kind, key = classify(src)
        return false if kind.nil?
        return false if @regs.any? { |r| r.kind == kind && r.key == key && r.observer.equal?(obs) }
        flush unless @busy
        @regs << Reg.new(kind, key, src, obs)
        rebaseline unless @busy
        notify_host
        true
      end

      # Sketchup.add_observer (liste Esq.observers[:app] de la zone modèle) : un AppObserver qui
      # attend la notification du modèle de démarrage la reçoit (les extensions se chargent ici
      # quand ce modèle existe déjà).
      def app_added(obs)
        notify_host
        return unless obs.respond_to?(:expectsStartupModelNotification)
        return unless ToolConv.truthy(deliver(obs, :expectsStartupModelNotification))
        deliver(obs, :onNewModel, Sketchup.active_model)
      end

      # Renvoie true si l'observateur était enregistré.
      def remove(src, obs)
        kind, key = classify(src)
        n = @regs.size
        @regs.reject! { |r| r.kind == kind && r.key == key && r.observer.equal?(obs) }
        notify_host
        n != @regs.size
      end

      # Dit à l'application ce qui est observé, pour qu'elle n'émette que ce qui sert.
      def notify_host
        Esq.call('host.tools.observing', 'kinds' => observed_kinds.split(','))
      rescue StandardError
        nil
      end

      def frame_observers
        k = klass(:Pages)
        k.respond_to?(:__frame_observers) ? k.__frame_observers : []
      rescue StandardError
        []
      end

      # ---------------------------------------------------------------- livraison
      def report(e)
        $stderr.puts "Error: #<#{e.class}: #{e.message}>"
        (e.backtrace || []).first(8).each { |l| $stderr.puts l }
      end

      # Appelle `obs.name(*args)` si l'observateur sait répondre ; une exception est affichée
      # dans la console (comme SketchUp) et ne remonte pas.
      def deliver(obs, name, *args)
        return nil unless obs.respond_to?(name)
        obs.public_send(name, *args)
      rescue Exception => e # rubocop:disable Lint/RescueException
        report(e)
        nil
      end

      # Livre à tous les observateurs de (kind, key) ; SRC dans args = l'objet observé.
      def fire(kind, key, name, *args)
        pairs = case kind
                when *EXT.keys then ext_observers(kind)
                when :entities then ent_observers("entities:#{key}").map { |o| [entities_obj(key), o] }
                when :definitions then ent_observers('dl:0:0').map { |o| [ent_obj('dl:0:0'), o] }
                when :definition then ent_observers("d:0:#{key}").map { |o| [ent_obj("d:0:#{key}"), o] }
                else regs_of(kind, key).map { |r| [r.source, r.observer] }
                end
        pairs.each do |src, obs|
          deliver(obs, name, *args.map { |a| a.equal?(SRC) ? src : a })
        end
      end

      # Les AppObserver (Sketchup.add_observer).
      def fire_app(name, *args)
        app_observers.each { |o| deliver(o, name, *args) }
      end

      # ---------------------------------------------------------------- états
      def model; Sketchup.active_model; end

      def spec
        s = {}
        keys = ent_watch_keys
        ctxs = keys.select { |k| k.start_with?('entities:') }.map { |k| k.sub('entities:', '') }
        s['ents'] = ctxs unless ctxs.empty?
        hs = keys.reject { |k| k.start_with?('entities:') || k == 'dl:0:0' }
        s['handles'] = hs unless hs.empty?
        s['sel'] = true if watching(:selection)
        s['layers'] = true if watching(:layers)
        s['mats'] = true if watching(:materials)
        s['defs'] = true if watching(:definitions)
        ds = keys.select { |k| k.start_with?('d:0:') }.map { |k| k.split(':')[2] }
        s['definstances'] = ds unless ds.empty?
        s['pages'] = true if watching(:pages)
        s['shadow'] = true if watching(:shadow)
        s['envs'] = true if watching(:environments)
        s['rev'] = true if watching(:model)
        s
      end

      def snapshot
        sp = spec
        return {} if sp.empty?
        Esq.call('tools.state', sp)
      rescue StandardError
        {}
      end

      def ro_snapshot
        return nil unless watching(:rendering_options)
        ro = ext_instance(:RenderingOptions)
        ro.respond_to?(:__snapshot) ? ro.__snapshot : nil
      rescue StandardError
        nil
      end

      def rebaseline
        @last = snapshot
        @ro_last = ro_snapshot
        cache_objects
      end

      # Garde les objets Ruby de ce qui peut disparaître : on devra les passer aux observateurs
      # après coup (onLayerRemoved, onComponentRemoved…).
      def cache_objects
        @objs = {}
        if watching(:layers)
          (model.layers.to_a rescue []).each { |l| @objs["layer:#{l.name}"] = l }
        end
        if watching(:materials)
          (model.materials.to_a rescue []).each { |m| @objs["mat:#{m.name}"] = m }
        end
        if watching(:definitions)
          (model.definitions.to_a rescue []).each { |d| @objs["def:#{ctx_of(d)}"] = d }
        end
        ((@last || {})['definstances'] || {}).each_value do |list|
          list.each { |h| @objs["inst:#{h}"] ||= wrap(h) }
        end
        keys = ent_watch_keys
        (@ent_objs ||= {}).select! { |h, _| keys.include?(h) }
        keys.each { |h| ent_obj(h) unless h.start_with?('entities:') }
      end

      def wrap(h)
        Sketchup::Entity.wrap(h)
      rescue StandardError
        nil
      end

      # entityID d'une entité disparue (même calcul que Entity#entityID).
      def id_of(h)
        Sketchup::Entity.new(h).send(:unique_id)
      rescue StandardError
        h.split(':').last.to_i
      end

      def entities_obj(ctx)
        Sketchup::Entities.new(ctx)
      end

      def page(key)
        k = klass(:Page)
        return nil unless k.respond_to?(:__wrap) && !key.to_s.empty?
        key.start_with?('name:') ? (model.pages[key[5..]] rescue nil) : k.__wrap(key.to_i)
      rescue StandardError
        nil
      end

      def environment(id)
        k = klass(:Environment)
        id && k.respond_to?(:__wrap) ? k.__wrap(id) : nil
      end

      # ---------------------------------------------------------------- comparaison
      def diff(old, cur, ev, from_app)
        diff_entities(old, cur, ev)
        diff_handles(old, cur, ev)
        diff_definitions(old, cur, ev)
        diff_definstances(old, cur, ev)
        diff_pages(old, cur, ev)
        diff_environments(old, cur, ev)
        # Sélection, étiquettes, matières, ombres : ce que Ruby change est annoncé par la zone
        # propriétaire (Selection#add, Materials#add, ShadowInfo#[]=…) ; ici, l'interface.
        diff_materials(old, cur, ev, from_app)
        return unless from_app
        diff_selection(old, cur, ev)
        diff_layers(old, cur, ev)
        diff_shadow(old, cur, ev)
      end

      def diff_entities(old, cur, ev)
        (cur['ents'] || {}).each do |ctx, now|
          was = (old['ents'] || {})[ctx]
          next if was.nil? || now.nil?
          (was.keys - now.keys).each { |h| ev << -> { fire(:entities, ctx, :onElementRemoved, SRC, id_of(h)) } }
          (now.keys - was.keys).each do |h|
            ev << -> { (e = wrap(h)) && fire(:entities, ctx, :onElementAdded, SRC, e) }
          end
          now.each do |h, f|
            next unless was.key?(h) && was[h] != f
            ev << -> { (e = wrap(h)) && fire(:entities, ctx, :onElementModified, SRC, e) }
          end
        end
        # The section plane that cuts a collection changed (activated, turned off, erased).
        (cur['active_sections'] || {}).each do |ctx, now|
          olds = old['active_sections'] || {}
          next unless olds.key?(ctx) && olds[ctx] != now
          ev << -> { fire(:entities, ctx, :onActiveSectionPlaneChanged, SRC) }
        end
      end

      # Valeur d'une poignée : empreinte, ou [empreinte, empreinte du texte] (cotes, textes).
      def diff_handles(old, cur, ev)
        (cur['handles'] || {}).each do |h, now|
          next unless (old['handles'] || {}).key?(h)
          was = old['handles'][h]
          if was && now.nil?
            ev << -> { fire_entity(h, :onEraseEntity) }
          elsif was && now && was != now
            ev << -> { fire_entity(h, :onChangeEntity) }
            if was.is_a?(Array) && now.is_a?(Array) && was[1] != now[1]
              ev << -> { fire_entity(h, :onTextChanged) }
            end
          end
        end
      end

      def fire_entity(h, name)
        e = ent_obj(h)
        ent_observers(h).each { |o| deliver(o, name, e) }
      end

      def diff_selection(old, cur, ev)
        was = old['sel']
        now = cur['sel']
        return if was.nil? || now.nil? || was == now
        added = now - was
        removed = was - now
        if now.empty?
          ev << lambda {
            fire(:selection, 'selection', :onSelectionCleared, SRC)
            fire(:selection, 'selection', :onSelectedCleared, SRC)
          }
        elsif added.size + removed.size == 1
          h = (added + removed).first
          if added.size == 1
            ev << -> { (e = wrap(h)) && fire(:selection, 'selection', :onSelectionAdded, SRC, e) }
          else
            ev << lambda {
              e = wrap(h)
              fire(:selection, 'selection', :onSelectionRemoved, SRC, e)
              fire(:selection, 'selection', :onSelectedRemoved, SRC, e)
            }
          end
        else
          ev << -> { fire(:selection, 'selection', :onSelectionBulkChange, SRC) }
        end
      end

      # Listes [clé, empreinte, …] comparées par clé.
      def diff_keyed(was, now, ev, on_add, on_del, on_chg)
        return if was.nil? || now.nil?
        wm = was.map { |a| [a[0], a] }.to_h
        nm = now.map { |a| [a[0], a] }.to_h
        (wm.keys - nm.keys).each { |k| ev << -> { on_del.call(k, wm[k]) } }
        (nm.keys - wm.keys).each { |k| ev << -> { on_add.call(k, nm[k]) } }
        nm.each { |k, a| ev << -> { on_chg.call(k, wm[k], a) } if wm.key?(k) && wm[k] != a }
      end

      def diff_layers(old, cur, ev)
        live = ->(n) { (model.layers[n] rescue nil) || @objs["layer:#{n}"] }
        diff_keyed(old['layers'], cur['layers'], ev,
                   ->(n, _) { fire(:layers, 'layers', :onLayerAdded, SRC, live.call(n)) },
                   ->(n, _) { fire(:layers, 'layers', :onLayerRemoved, SRC, @objs["layer:#{n}"]) },
                   ->(n, _, _) { fire(:layers, 'layers', :onLayerChanged, SRC, live.call(n)) })
        # Tag folders made, removed or changed in the interface (Tags panel).
        folder = ->(uid) { (model.layers.__folder(uid) rescue nil) || Sketchup::LayerFolder.__for('uid' => uid) }
        diff_keyed(old['folders'], cur['folders'], ev,
                   ->(u, _) { fire(:layers, 'layers', :onLayerFolderAdded, SRC, folder.call(u)) },
                   ->(u, _) { fire(:layers, 'layers', :onLayerFolderRemoved, SRC, folder.call(u)) },
                   ->(u, _, _) { fire(:layers, 'layers', :onLayerFolderChanged, SRC, folder.call(u)) })
      end

      # Hors interface (`from_app` faux), seul onMaterialRefChange : la zone modèle annonce elle-même
      # les ajouts, suppressions et modifications faits en Ruby.
      def diff_materials(old, cur, ev, from_app = true)
        undoing = @undoing
        live = ->(n) { (model.materials[n] rescue nil) || @objs["mat:#{n}"] }
        diff_keyed(old['mats'], cur['mats'], ev,
                   ->(n, _) { fire(:materials, 'materials', :onMaterialAdd, SRC, live.call(n)) if from_app },
                   ->(n, _) { fire(:materials, 'materials', :onMaterialRemove, SRC, @objs["mat:#{n}"]) if from_app },
                   lambda { |n, a, b|
                     m = live.call(n)
                     if a[1] != b[1] && from_app
                       fire(:materials, 'materials', undoing ? :onMaterialUndoRedo : :onMaterialChange, SRC, m)
                     end
                     fire(:materials, 'materials', :onMaterialRefChange, SRC, m) if a[2] != b[2]
                   })
      end

      def diff_definitions(old, cur, ev)
        live = ->(c) { (model.definitions.find { |d| ctx_of(d) == c } rescue nil) || @objs["def:#{c}"] }
        diff_keyed(old['defs'], cur['defs'], ev,
                   ->(c, _) { fire(:definitions, 'definitions', :onComponentAdded, SRC, live.call(c)) },
                   ->(c, _) { fire(:definitions, 'definitions', :onComponentRemoved, SRC, @objs["def:#{c}"]) },
                   lambda { |c, a, b|
                     d = live.call(c)
                     fire(:definitions, 'definitions', :onComponentPropertiesChanged, SRC, d) if a[2] != b[2]
                     fire(:definitions, 'definitions', :onComponentTypeChanged, SRC, d) if a[3] != b[3]
                   })
      end

      def diff_definstances(old, cur, ev)
        (cur['definstances'] || {}).each do |ctx, now|
          was = (old['definstances'] || {})[ctx]
          next if was.nil?
          (was - now).each do |h|
            ev << -> { fire(:definition, ctx, :onComponentInstanceRemoved, SRC, @objs["inst:#{h}"] || wrap(h)) }
          end
          (now - was).each do |h|
            ev << -> { (i = wrap(h)) && fire(:definition, ctx, :onComponentInstanceAdded, SRC, i) }
          end
        end
      end

      def diff_pages(old, cur, ev)
        diff_keyed(old['pages'], cur['pages'], ev,
                   ->(k, _) { fire(:pages, nil, :onElementAdded, SRC, page(k)) },
                   ->(k, _) { fire(:pages, nil, :onElementRemoved, SRC, page(k)) },
                   ->(_k, _, _) { fire(:pages, nil, :onContentsModified, SRC) })
        a = old['pages']
        b = cur['pages']
        return if a.nil? || b.nil? || a.map(&:first) == b.map(&:first)
        # Même ensemble de scènes dans un autre ordre : le contenu de Pages a changé.
        ev << -> { fire(:pages, nil, :onContentsModified, SRC) } if a.map(&:first).sort == b.map(&:first).sort
      end

      def diff_environments(old, cur, ev)
        was = old['envs']
        now = cur['envs']
        return if was.nil? || now.nil?
        diff_keyed(was['list'], now['list'], ev,
                   ->(id, _) { fire(:environments, nil, :onEnvironmentAdd, SRC, environment(id)) },
                   ->(id, _) { fire(:environments, nil, :onEnvironmentRemove, SRC, environment(id)) },
                   ->(id, _, _) { fire(:environments, nil, :onEnvironmentChange, SRC, environment(id)) })
        return if was['current'] == now['current']
        ev << -> { fire(:environments, nil, :onEnvironmentSetCurrent, SRC, environment(now['current'])) }
      end

      def diff_shadow(old, cur, ev)
        was = old['shadow']
        now = cur['shadow']
        return if was.nil? || now.nil? || was == now
        # Même « type » que ShadowInfo#[]= (44_view_camera.rb).
        ev << -> { fire(:shadow, nil, :onShadowInfoChanged, SRC, 0) }
      end

      # Options de rendu changées dans l'interface (les changements Ruby sont annoncés par
      # RenderingOptions#[]= lui-même).
      def diff_ro(ev)
        was = @ro_last
        now = ro_snapshot
        @ro_last = now
        return if was.nil? || now.nil?
        ro = ext_instance(:RenderingOptions)
        map = ro.class.const_defined?(:ROP_OF_KEY) ? ro.class::ROP_OF_KEY : {}
        now.each do |k, v|
          next if was[k] == v
          id = map[k] or next
          ev << -> { fire(:rendering_options, nil, :onRenderingOptionsChanged, SRC, id) }
        end
      end

      # ---------------------------------------------------------------- vidage
      # Compare l'état courant au précédent et livre ce qui a changé. `hints` : ce que
      # l'application sait déjà (annuler, rétablir, valider…), voir app_sync.
      def flush(hints = [], from_app = false)
        return unless watching?
        if @busy
          @dirty = true
          return
        end
        @busy = true
        begin
          3.times do
            @dirty = false
            cur = snapshot
            ev = []
            old = @last
            changed = false
            if old
              diff(old, cur, ev, from_app)
              changed = !ev.empty? || (old['rev'] && cur['rev'] && old['rev'] != cur['rev'])
            end
            if from_app && hints.include?('ro')
              diff_ro(ev)
            elsif watching(:rendering_options)
              @ro_last = ro_snapshot
            end
            @last = cur
            cache_objects
            run_events(ev, changed, hints)
            hints = []
            from_app = false
            break unless @dirty
          end
        ensure
          @busy = false
        end
      end

      def model_event(name, *args)
        fire(:model, 'model', name, SRC, *args)
      end

      def run_events(ev, changed, hints)
        explicit = @op_depth > 0
        commit = hints.include?('commit')
        undo = hints.include?('undo')
        again = hints.include?('redo')
        wrap = commit || (changed && !explicit && !undo && !again)
        model_event(:onTransactionStart) if wrap
        @undoing = undo || again
        ev.each(&:call)
        @undoing = false
        @op_changed = true if changed && explicit
        model_event(changed ? :onTransactionCommit : :onTransactionEmpty) if wrap
        model_event(:onTransactionUndo) if undo
        model_event(:onTransactionRedo) if again
        hints.each do |h|
          case h
          when 'path' then model_event(:onActivePathChanged)
          when 'save'
            model_event(:onPreSaveModel)
            model_event(:onSaveModel)
            model_event(:onPostSaveModel)
          when 'erase_all' then model_event(:onEraseAll)
          when 'explode' then model_event(:onExplode)
          when 'place' then model_event(:onPlaceComponent)
          when 'view'
            v = ext_instance(:View)
            fire(:view, nil, :onViewChanged, v) if v
          when /\Aopen:(.+)\z/ then fire_entity(Regexp.last_match(1), :onOpen)
          when /\Aclose:(.+)\z/ then fire_entity(Regexp.last_match(1), :onClose)
          when /\Aframe:(\d*):(\d+):([\d.eE+-]+)\z/
            from = page(Regexp.last_match(1))
            to = page(Regexp.last_match(2))
            pct = Regexp.last_match(3).to_f
            frame_observers.each { |o| deliver(o, :frameChange, from, to, pct) }
          end
        end
      end

      # Appelé après chaque appel natif qui peut avoir modifié le modèle.
      READ_ONLY = /(\A|\.)(info|get|list|valid\??|names?|count|bounds|query|state|find|raytest|read|exists|is_\w+|has_\w+|take|ping|size)\z|\Apick\./

      def after_call(op)
        return unless watching?
        return if op.start_with?('host.', 'tools.') || READ_ONLY.match?(op)
        flush
      end

      # Appelé par l'application avec ce qui s'est passé dans l'interface depuis la dernière
      # fois : 'commit', 'undo', 'redo', 'path', 'open:<poignée>', 'close:<poignée>', 'save',
      # 'view' (caméra), 'ro' (options de rendu), 'frame:<uid>:<uid>:<0..1>' (transition)…
      # Renvoie les genres observés (l'application s'en sert pour ne rien demander d'inutile).
      def app_sync(hints = [])
        flush(hints, true) if watching? || !hints.empty?
        notify_host
        observed_kinds
      end

      def observed_kinds
        kinds = @regs.map(&:kind).uniq.map(&:to_s)
        EXT.each_key { |k| kinds << k.to_s unless ext_observers(k).empty? }
        kinds << 'app' unless app_observers.empty?
        keys = ent_watch_keys
        kinds << 'entities' if keys.any? { |k| k.start_with?('entities:') }
        kinds << 'entity' if keys.any? { |k| !k.start_with?('entities:') }
        kinds << 'frame' if frame_observers.any?
        kinds.join(',')
      end

      # Événements de l'application elle-même (nouveau modèle, ouverture, fermeture…).
      def app_event(name, *args)
        fire_app(name.to_sym, *args)
      end

      # Un autre modèle remplace le modèle actif (Fichier › Nouveau / Ouvrir dans l'interface) :
      # les observateurs attachés à l'ancien modèle (modèle, sélection, matières, étiquettes,
      # entités) ne reçoivent plus rien ; ceux de l'application sont prévenus.
      def model_replaced(how)
        @regs.reject! { |r| r.kind != :tools }
        %i[model selection layers materials].each do |k|
          src = ext_source(k)
          ext_list(src).clear if src
        end
        ent_lists.delete_if { |k, _| k.is_a?(String) }
        @ent_objs = {}
        @ext_count = EXT.keys.sum { |k| ext_observers(k).size }
        @last = nil
        @op_depth = 0
        rebaseline
        notify_host
        app_event(how == 'open' ? :onOpenModel : :onNewModel, model)
      end

      # Model#start_operation / commit_operation / abort_operation : la zone modèle annonce
      # onTransactionStart / Commit / Abort ; ici on suit seulement si une opération est ouverte
      # (pas de transaction implicite par appel) et on livre les événements des éléments avant
      # la validation.
      def operation_started
        @op_depth += 1
        @op_changed = false
      end

      def before_commit
        flush
      end

      def operation_ended
        @op_depth -= 1 if @op_depth > 0
        @op_changed = false
      end
    end
  end

  # Ajoute add_observer / remove_observer aux classes observables.
  module Observable
    def add_observer(obs); Esq::Events.add(self, obs); end
    def remove_observer(obs); Esq::Events.remove(self, obs); end
  end

  # Classes qui gardent leur propre liste : on suit seulement ses changements.
  module ExtObservable
    def add_observer(obs)
      r = super
      Esq::Events.ext_changed(r)
      r
    end

    def remove_observer(obs)
      r = super
      Esq::Events.ext_changed(false)
      r
    end
  end

  # Entity#add_observer / Entities#add_observer (zone entités, liste Esq.observers) : on prend
  # l'état de référence au moment de l'ajout.
  module EntObservable
    def add_observer(obs)
      Esq::Events.before_ent_add
      r = super
      Esq::Events.after_ent_add
      r
    end

    def remove_observer(obs)
      r = super
      Esq::Events.notify_host
      r
    end
  end

  module FrameObservable
    def add_frame_change_observer(obs)
      r = super
      Esq::Events.notify_host
      r
    end

    def remove_frame_change_observer(id)
      r = super
      Esq::Events.notify_host
      r
    end
  end

  # Un appel natif qui peut changer le modèle déclenche la comparaison d'états.
  module CallHook
    def call(op, args = {})
      r = super
      Esq::Events.after_call(op) if Esq::Events.watching?
      r
    end
  end
  singleton_class.prepend(CallHook)

  # Modèle : opérations.
  module ModelHooks
    def start_operation(*a)
      r = super
      Esq::Events.operation_started
      r
    end

    def commit_operation
      Esq::Events.before_commit
      r = super
      Esq::Events.operation_ended
      r
    end

    def abort_operation
      r = super
      Esq::Events.operation_ended
      r
    end
  end

  # Sketchup.add_observer (zone modèle) : notification du modèle de démarrage.
  module AppObservable
    def add_observer(obs)
      r = super
      Esq::Events.app_added(obs)
      r
    end

    def remove_observer(obs)
      r = super
      Esq::Events.notify_host
      r
    end
  end

  # Entities#clear! : onEraseEntities, sans un onElementRemoved par élément.
  module EntitiesHooks
    def clear!
      Esq::Events.fire(:entities, Esq::Events.ctx_of(self), :onEraseEntities, Esq::Events::SRC) if Esq::Events.watching?
      r = super
      Esq::Events.rebaseline if Esq::Events.watching?
      r
    end
  end

  # View#invalidate / refresh : l'outil et les surcouches seront redessinés.
  module InvalidateHooks
    def invalidate
      r = super
      Esq::Tools.host('invalidate')
      r
    end

    def refresh
      r = super
      Esq::Tools.host('invalidate')
      r
    end
  end

  # ====================================================================== outils Ruby
  # La pile des outils Ruby (Model#select_tool, Tools#push_tool / pop_tool) et la livraison de
  # leurs événements. L'application garde l'outil actif (un RubyTool natif qui ne fait que
  # transmettre) ; ici on garde les objets Ruby, désignés côté application par un jeton.
  module Tools
    SELECT = ['SelectionTool', 21_022].freeze
    MOUSE = %w[onMouseMove onLButtonDown onLButtonUp onLButtonDoubleClick onRButtonDown onRButtonUp
               onRButtonDoubleClick onMButtonDown onMButtonUp onMButtonDoubleClick].freeze
    # Après ces événements SketchUp redemande enableVCB? à l'outil.
    RECHECK = %w[onLButtonDown onLButtonUp onLButtonDoubleClick onRButtonDown].freeze

    @registry = {}
    @ids = {}
    @stack = []
    @ruby_active = false
    @next_token = 0
    @next_id = 50_000
    @free_ids = []
    @native = SELECT
    @cursor = 0

    class << self
      attr_reader :registry
      attr_accessor :cursor

      def view; Sketchup.active_model.active_view; end

      # Opération hôte de la zone outils (Esq.tools_host) ; nil hors de l'application.
      def host(op, args = {})
        Esq.tools_host(op, args)
      end

      def view_host(op, args = {})
        Esq.call("host.view.#{op}", args)
      rescue StandardError
        nil
      end

      def reset!
        @registry = {}
        @ids = {}
        @stack = []
        @ruby_active = false
        @native = SELECT
        @free_ids = []
        @next_id = 50_000
      end

      # Identifiants d'outils Ruby : 50000, 50001… ; ceux des outils abandonnés sont réutilisés.
      def alloc_id
        return @free_ids.pop unless @free_ids.empty?
        @next_id += 1
        @next_id - 1
      end

      def register(tool)
        @next_token += 1
        @registry[@next_token] = tool
        @ids[@next_token] = alloc_id
        @next_token
      end

      def release(token)
        @registry.delete(token)
        id = @ids.delete(token)
        @free_ids << id if id
      end

      def active_token; @ruby_active ? @stack.last : nil; end
      def active; (t = active_token) ? @registry[t] : nil; end
      def id_of(token); @ids[token]; end

      # [nom, identifiant] de l'outil actif.
      def current_info
        return ['RubyTool', @ids[active_token]] if @ruby_active
        h = host('active')
        h.is_a?(Hash) && h['name'] ? [h['name'], h['id']] : @native
      end

      # Appelle un rappel facultatif de l'outil ; une exception est affichée, pas propagée.
      def call(tool, name, *args)
        return nil unless tool.respond_to?(name)
        tool.public_send(name, *args)
      rescue Exception => e # rubocop:disable Lint/RescueException
        Esq::Events.report(e)
        nil
      end

      def caps(token)
        t = @registry[token] or return
        vcb = t.respond_to?(:enableVCB?) ? ToolConv.truthy(call(t, :enableVCB?)) : t.respond_to?(:onUserText)
        middle = t.respond_to?(:onMButtonDown) && t.respond_to?(:onMButtonUp)
        host('caps', 'token' => token, 'vcb' => vcb, 'middle' => middle)
      end

      def activate(token)
        call(@registry[token], :activate)
        caps(token)
      end

      def notify_changed
        name, id = current_info
        Esq::Events.fire(:tools, 'tools', :onActiveToolChanged, Esq::Events::SRC, name, id)
        render_all
      end

      # Model#select_tool : remplace toute la pile ; nil = l'outil Sélectionner.
      def select(tool)
        if (old = active)
          call(old, :deactivate, view)
        end
        @stack.each { |tk| release(tk) }
        @stack = []
        @ruby_active = false
        if tool.nil?
          @native = SELECT
          host('select_native')
        else
          tk = register(tool)
          @stack = [tk]
          @ruby_active = true
          host('select', 'token' => tk, 'id' => @ids[tk])
          activate(tk)
        end
        notify_changed
      end

      # Tools#push_tool : l'outil actif est suspendu, le nouveau l'emporte.
      def push(tool)
        if (cur = active)
          call(cur, :suspend, view)
        end
        tk = register(tool)
        @stack << tk
        @ruby_active = true
        host('push', 'token' => tk, 'id' => @ids[tk])
        activate(tk)
        notify_changed
        true
      end

      # Tools#pop_tool : l'outil Ruby actif est désactivé (et renvoyé), le précédent reprend.
      def pop
        unless @ruby_active
          host('pop')
          return nil
        end
        tk = @stack.pop
        t = @registry[tk]
        call(t, :deactivate, view)
        release(tk)
        host('pop')
        if @stack.empty?
          @ruby_active = false
        else
          call(active, :resume, view)
          caps(active_token)
        end
        notify_changed
        t
      end

      # L'utilisateur a choisi un autre outil dans l'interface : l'outil Ruby actif est désactivé.
      def app_changed(name, id)
        if (t = active)
          call(t, :deactivate, view)
        end
        @stack.each { |tk| release(tk) }
        @stack = []
        @ruby_active = false
        @native = [name, id]
        Esq::Events.fire(:tools, 'tools', :onActiveToolChanged, Esq::Events::SRC, name, id)
        render_all
      end

      # Un outil natif a changé d'état (par exemple la Ligne après son premier clic).
      def app_state_changed(name, id, state)
        Esq::Events.fire(:tools, 'tools', :onToolStateChanged, Esq::Events::SRC, name, id, state)
      end

      # Événements venus de l'application : [[nom, args…], …] (coordonnées en pixels).
      # Renvoie true si l'outil a traité la molette (onMouseWheel a renvoyé true).
      def deliver(token, events, draw = false)
        wheel = false
        events.each do |e|
          break unless token == active_token
          t = @registry[token]
          name = e[0].to_s
          a = e[1..] || []
          v = view
          if MOUSE.include?(name)
            @last_mouse = [a[0].to_i, a[1].to_f, a[2].to_f]
            call(t, name.to_sym, a[0].to_i, a[1].to_f, a[2].to_f, v)
            caps(token) if RECHECK.include?(name) && t.respond_to?(:enableVCB?)
            set_cursor(t) if name == 'onMouseMove'
          else
            case name
            when 'onMouseWheel'
              wheel = call(t, :onMouseWheel, a[0].to_i, a[1].to_i, a[2].to_f, a[3].to_f, v) == true
            when 'onKeyDown', 'onKeyUp'
              call(t, name.to_sym, a[0].to_i, a[1].to_i, a[2].to_i, v)
            when 'onUserText' then call(t, :onUserText, a[0].to_s, v)
            when 'onReturn'
              call(t, :onReturn, v)
              caps(token) if t.respond_to?(:enableVCB?)
            when 'onCancel' then call(t, :onCancel, a[0].to_i, v)
            when 'onMouseEnter', 'onMouseLeave' then call(t, name.to_sym, v)
            end
          end
        end
        render_all if draw
        wheel
      end

      def set_cursor(t)
        return unless t.respond_to?(:onSetCursor)
        handled = ToolConv.truthy(call(t, :onSetCursor))
        host('cursor', 'id' => handled ? @cursor.to_i : 0)
      end

      # Extents (getExtents) en pouces, ou nil.
      def extents_of(obj)
        return nil unless obj.respond_to?(:getExtents)
        bb = call(obj, :getExtents)
        return nil unless bb.is_a?(Geom::BoundingBox) && !bb.empty?
        [bb.min.to_a, bb.max.to_a]
      end

      # Redessine ce que les outils et les surcouches dessinent sur la vue : on vide la liste
      # de dessin de la vue (44_view_camera.rb), puis draw(view) de l'outil actif et de chaque
      # surcouche activée.
      def render_all
        view_host('draw_clear')
        ext = []
        if (t = active)
          call(t, :draw, view)
          ext << extents_of(t)
        end
        ext.concat(Esq::Overlays.render)
        host('extents', 'list' => ext.compact)
        true
      end

      # Dernière position de la souris livrée à l'outil (drapeaux, x, y) : getMenu la reçoit.
      attr_reader :last_mouse

      # Menu contextuel de l'outil actif (getMenu), rempli dans le menu « Context/Tool » de la
      # zone ui (Esq.context_menu_json, comme les gestionnaires de UI.add_context_menu_handler).
      # Renvoie true si l'outil a fourni son menu : il remplace alors celui de l'application.
      def fill_context_menu
        t = active
        return false unless t.respond_to?(:getMenu) && defined?(Sketchup::Menu) && Sketchup::Menu.respond_to?(:__new)
        menu = Sketchup::Menu.__new('Context/Tool')
        f, x, y = @last_mouse || [0, 0.0, 0.0]
        res = t.method(:getMenu).arity == 1 ? call(t, :getMenu, menu) : call(t, :getMenu, menu, f, x, y, view)
        return true if ToolConv.truthy(res)
        items = Esq.instance_variable_get(:@menu_items)
        items&.reject! { |i| i[:menu].to_s.start_with?('Context/Tool') }
        false
      end
    end
  end

  # Esq.context_menu_json (zone ui) : le menu de l'outil Ruby actif vient d'abord.
  module ToolContextMenu
    def context_menu_json
      Esq::Tools.fill_context_menu
      super
    end
  end

  # Registre des procédures (Sketchup.register_procedure, Test.unregister_procedure).
  module Procedures
    @list = {}
    class << self
      def list; @list; end

      def register(p)
        id = p.__id_str
        return false if id.empty? || @list.key?(id)
        @list[id] = p
        true
      end

      def unregister(id)
        !@list.delete(id).nil?
      end
    end
  end

  # ====================================================================== surcouches
  # Sketchup::Overlay ajoutées au modèle (model.overlays) : dessinées à chaque image, et
  # prévenues des mouvements de la souris quand elles sont activées.
  module Overlays
    @list = []
    @prefs = {}
    class << self
      attr_reader :list, :prefs

      def reset!
        @list.each { |o| o.__manager = nil }
        @list = []
      end

      def find(id); @list.find { |o| o.overlay_id == id }; end

      def sync_host
        Esq::Tools.host('overlays', 'list' => @list.map { |o| { 'id' => o.overlay_id, 'name' => o.name, 'enabled' => o.enabled?, 'description' => o.description, 'source' => o.source } })
        Esq::Tools.host('invalidate')
      end

      def add(o)
        return false if find(o.overlay_id)
        raise ArgumentError, 'name cannot be an empty string' if o.name.empty?
        raise ArgumentError, 'overlay already added to a model' if o.__manager
        @list << o
        o.__manager = self
        # L'état choisi par l'utilisateur (fenêtre Surcouches) est retenu ; désactivée sinon.
        if @prefs[o.overlay_id]
          o.__enabled = true
          Esq::Tools.call(o, :start)
        end
        sync_host
        true
      end

      def remove(o)
        return false unless find(o.overlay_id).equal?(o)
        Esq::Tools.call(o, :stop)
        @list.delete(o)
        o.__manager = nil
        sync_host
        true
      end

      def set_enabled(o, on)
        return false if o.enabled? == on
        return false unless find(o.overlay_id)
        o.__enabled = on
        on ? Esq::Tools.call(o, :start) : Esq::Tools.call(o, :stop)
        @prefs[o.overlay_id] = on
        sync_host
        true
      end

      # Bascule depuis l'application (fenêtre Surcouches).
      def toggle(id, on)
        o = find(id) or return false
        set_enabled(o, on)
      end

      # Appelé par l'application : événements souris pour les surcouches activées, puis draw.
      def deliver(events, draw = false)
        v = Esq::Tools.view
        @list.select(&:enabled?).each do |o|
          events.each do |e|
            name = e[0].to_s
            a = e[1..] || []
            case name
            when 'onMouseMove', 'onMouseEnter'
              Esq::Tools.call(o, name.to_sym, a[0].to_i, a[1].to_f, a[2].to_f, v)
            when 'onMouseLeave'
              Esq::Tools.call(o, :onMouseLeave, v)
            end
          end
        end
        Esq::Tools.render_all if draw
        true
      end

      # draw(view) de chaque surcouche activée ; renvoie leurs extents.
      def render
        v = Esq::Tools.view
        @list.select(&:enabled?).map do |o|
          Esq::Tools.call(o, :draw, v)
          Esq::Tools.extents_of(o)
        end
      end
    end
  end

  # Sketchup.register_procedure : comme SketchUp, TypeError si ce n'est pas une Sketchup::Procedure,
  # false pour un identifiant vide ou déjà enregistré (la liste Esq.procedures de la zone modèle
  # reçoit les procédures acceptées).
  module ProcedureHooks
    def register_procedure(p)
      unless p.is_a?(Sketchup::Procedure)
        raise TypeError, "wrong argument type #{ToolConv.type_name(p)} (expected su_proc_t)"
      end
      ok = Esq::Procedures.register(p)
      Esq.procedures << p if ok && Esq.respond_to?(:procedures)
      ok
    end
  end

  # UI.set_cursor : l'outil actif l'appelle dans onSetCursor.
  module CursorHooks
    def set_cursor(id)
      r = defined?(super) ? super : true
      Esq::Tools.cursor = id
      r
    end
  end
end

module Sketchup
  # Model#tools : les outils du modèle (pas de Tools.new).
  class Tools
    class << self
      def new(*)
        raise TypeError, "allocator undefined for #{self}"
      end

      def __make
        allocate
      end
    end

    def model; Sketchup.active_model; end
    def active_tool; Esq::Tools.active; end
    def active_tool_id; Esq::Tools.current_info[1]; end
    def active_tool_name; Esq::Tools.current_info[0]; end
    def push_tool(tool); Esq::Tools.push(tool); end
    def pop_tool; Esq::Tools.pop; end
    def inspect; "#<Sketchup::Tools:0x#{format('%016x', object_id)}>"; end
  end

  # Une procédure (composants procéduraux de SketchUp 2026) : identifiant, nom, description,
  # version. Elle n'expose aucune méthode en Ruby.
  class Procedure
    def initialize(id, name, description, version)
      @id = Esq::ToolConv.str(id)
      @name = Esq::ToolConv.str(name)
      @description = Esq::ToolConv.str(description)
      @version = Esq::ToolConv.long(version)
      nil
    end

    # (SketchUp nomme ici EntitiesBuilder : message repris tel quel.)
    def clone
      raise TypeError, 'Cannot duplicate or clone Sketchup::EntitiesBuilder.'
    end

    def dup
      raise TypeError, 'Cannot duplicate or clone Sketchup::EntitiesBuilder.'
    end

    def __id_str; @id.to_s; end
  end

  # Surcouche de la vue (SketchUp 2023.0+) : sous-classée par les extensions.
  class Overlay
    def initialize(*args, **kw)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 2)" unless args.size == 2
      bad = kw.keys - [:description]
      raise ArgumentError, "unknown keyword: #{bad.first.inspect}" unless bad.empty?
      id = Esq::ToolConv.str(args[0])
      name = Esq::ToolConv.str(args[1])
      desc = kw.key?(:description) ? Esq::ToolConv.str(kw[:description]) : ''
      raise ArgumentError, 'overlay id cannot be an empty string' if id.empty?
      raise ArgumentError, 'overlay name cannot be an empty string' if name.empty?
      @overlay_id = id
      @name = name
      @description = desc
      @source = Overlay.__source(caller_locations(1, 8))
      @enabled = false
      @manager = nil
      self
    end

    # « Extension: <nom> » de l'extension dont le dossier contient le code appelant.
    def self.__source(locs)
      files = (locs || []).map { |l| (l.absolute_path || l.path).to_s.tr('\\', '/') }
      exts = (Sketchup.extensions.to_a rescue [])
      exts.each do |e|
        p = (e.respond_to?(:extension_path) ? e.extension_path : nil) or next
        root = p.sub(/\.rb\z/, '')
        if files.any? { |f| f == p || f.start_with?("#{root}/") }
          return "Extension: #{e.name}"
        end
      end
      'Extension: unknown'
    end

    def overlay_id; @overlay_id.to_s; end
    def name; @name.to_s; end
    def description; @description.to_s; end

    def description=(d)
      @description = Esq::ToolConv.str(d)
      nil
    end

    def source; @source.to_s; end
    def enabled?; @enabled ? true : false; end

    def enabled=(v)
      raise RuntimeError, 'overlays must be added to a model to toggle them on/off' unless @manager
      Esq::Overlays.set_enabled(self, Esq::ToolConv.truthy(v))
      nil
    end

    def valid?; true; end

    def clone
      raise TypeError, 'Cannot duplicate or clone Sketchup::Overlay.'
    end

    def dup
      raise TypeError, 'Cannot duplicate or clone Sketchup::Overlay.'
    end

    def __manager; @manager; end
    def __manager=(m); @manager = m; end
    def __enabled=(v); @enabled = v; end
    def __source_set?; !@source.nil? && !@source.empty?; end
  end

  # model.overlays
  class OverlaysManager
    include Enumerable

    class << self
      def new(*)
        raise TypeError, "allocator undefined for #{self}"
      end

      def __make
        allocate
      end
    end

    def add(overlay)
      ov = Esq::OverlaysManagerCheck.overlay!(overlay)
      raise ArgumentError, 'invalid overlay object. Did you forget to call super in initialize?' unless ov.__source_set?
      raise ArgumentError, 'overlay_id cannot be an empty string' if ov.overlay_id.empty?
      Esq::Overlays.add(ov)
    end

    def remove(overlay)
      Esq::Overlays.remove(Esq::OverlaysManagerCheck.overlay!(overlay))
    end

    def each
      raise LocalJumpError, 'no block given (yield)' unless block_given?
      Esq::Overlays.list.dup.each { |o| yield o }
      nil
    end

    def size; Esq::Overlays.list.size; end
    alias length size

    def [](index)
      i = index.is_a?(Integer) ? index : Esq::ToolConv.long(index)
      i.negative? ? nil : Esq::Overlays.list[i]
    end
    alias at []
  end
end

module Esq
  module OverlaysManagerCheck
    def self.overlay!(o)
      return o if o.is_a?(Sketchup::Overlay)
      raise TypeError, "wrong argument type #{ToolConv.type_name(o)} (expected sketchup_model_service)"
    end
  end

  # Modèle : outils et surcouches.
  module ModelToolHooks
    def tools; @__tools ||= Sketchup::Tools.__make; end
    def overlays; @__overlays ||= Sketchup::OverlaysManager.__make; end

    def select_tool(tool)
      Esq::Tools.select(tool)
      self
    end
  end

  # Installe les mixins dès que les classes existent (elles peuvent être définies après ce fichier).
  # Observateurs gardés ici (Esq::Events.regs).
  OBSERVABLE = %i[Tools].freeze
  # Observateurs gardés par la zone entités (Esq.observers).
  ENT_OBSERVABLE = %i[Entity Entities].freeze
  # Observateurs gardés par l'objet lui-même (zones vue et modèle).
  EXT_OBSERVABLE = %i[View Pages RenderingOptions ShadowInfo Environments Model Selection Layers Materials].freeze

  def self.install_observables(mod = Sketchup)
    OBSERVABLE.each do |n|
      next unless mod.const_defined?(n, false)
      k = mod.const_get(n, false)
      k.prepend(Observable) unless k.include?(Observable)
    end
    ENT_OBSERVABLE.each do |n|
      next unless mod.const_defined?(n, false)
      k = mod.const_get(n, false)
      k.prepend(EntObservable) unless k.include?(EntObservable)
      k.prepend(EntitiesHooks) if n == :Entities && !k.include?(EntitiesHooks)
    end
    EXT_OBSERVABLE.each do |n|
      next unless mod.const_defined?(n, false)
      k = mod.const_get(n, false)
      k.prepend(ExtObservable) if k.method_defined?(:add_observer) && !k.include?(ExtObservable)
      k.prepend(InvalidateHooks) if n == :View && !k.include?(InvalidateHooks)
      k.singleton_class.prepend(FrameObservable) if n == :Pages && k.respond_to?(:add_frame_change_observer) && !k.singleton_class.include?(FrameObservable)
      if n == :Model
        k.prepend(ModelHooks) unless k.include?(ModelHooks)
        k.prepend(ModelToolHooks) unless k.include?(ModelToolHooks)
      end
    end
  end

  Sketchup.singleton_class.prepend(AppObservable) unless Sketchup.singleton_class.include?(AppObservable)
  Sketchup.singleton_class.prepend(ProcedureHooks) unless Sketchup.singleton_class.include?(ProcedureHooks)
  UI.singleton_class.prepend(CursorHooks) if defined?(UI) && !UI.singleton_class.include?(CursorHooks)
  if Esq.respond_to?(:context_menu_json) && !Esq.singleton_class.include?(ToolContextMenu)
    Esq.singleton_class.prepend(ToolContextMenu)
  end
  install_observables

  # Classes définies plus tard (par d'autres fichiers du prélude ou une extension).
  module LateObservables
    def const_added(name)
      super
      if Esq::OBSERVABLE.include?(name) || Esq::EXT_OBSERVABLE.include?(name) || Esq::ENT_OBSERVABLE.include?(name)
        Esq.install_observables(self)
      end
    end
  end
  Sketchup.singleton_class.prepend(LateObservables)
end

{ GL_POINTS: 0, GL_LINES: 1, GL_LINE_LOOP: 2, GL_LINE_STRIP: 3, GL_TRIANGLES: 4, GL_TRIANGLE_STRIP: 5,
  GL_TRIANGLE_FAN: 6, GL_QUADS: 7, GL_QUAD_STRIP: 8, GL_POLYGON: 9,
  SB_PROMPT: 0, SB_VCB_LABEL: 1, SB_VCB_VALUE: 2,
  CONSTRAIN_MODIFIER_KEY: 16, COPY_MODIFIER_KEY: 17, ALT_MODIFIER_KEY: 18,
  CONSTRAIN_MODIFIER_MASK: 4, COPY_MODIFIER_MASK: 8, ALT_MODIFIER_MASK: 32,
  MK_LBUTTON: 1, MK_RBUTTON: 2, MK_SHIFT: 4, MK_CONTROL: 8, MK_MBUTTON: 16, MK_ALT: 32,
  VK_SHIFT: 16, VK_CONTROL: 17, VK_MENU: 18, VK_ALT: 18, VK_COMMAND: 17,
  VK_LEFT: 37, VK_UP: 38, VK_RIGHT: 39, VK_DOWN: 40, VK_DELETE: 46, VK_INSERT: 45,
  VK_HOME: 36, VK_END: 35, VK_PRIOR: 33, VK_NEXT: 34, VK_SPACE: 32 }.each do |n, v|
  Object.const_set(n, v) unless Object.const_defined?(n)
end
