# encoding: utf-8
# Sketchup::Model et Sketchup::Selection (zone « model »).
# View, RenderingOptions, ShadowInfo : zone « view » (44_view_camera.rb).

module Sketchup
  # Déclarées ici avec leur superclasse (Entity, comme dans SketchUp) : d'autres zones les
  # rouvrent avant 43_model_extra.rb, qui les complète.
  class Material < Entity; end
  class Materials < Entity; end
  class Layer < Entity; end
  class Layers < Entity; end
  class LayerFolder < Entity; end
  class Texture < Entity; end
  class HatchPattern < Entity; end
  class HatchPatterns < Entity; end
  class LineStyle < Entity; end
  class LineStyles < Entity; end

  class Selection
    include Enumerable

    def initialize; @observers = []; end
    def each(&b)
      return to_enum(:each) unless b
      Esq.call('sel.get').map { |h| Entity.wrap(h) }.each(&b)
      self
    end
    def length; Esq.call('sel.get').size; end
    alias size length
    alias nitems length
    def empty?; length.zero?; end
    def [](i); Esq.call('sel.get')[i.to_i].then { |h| h && Entity.wrap(h) }; end
    alias at []
    def first; self[0]; end
    def model; Sketchup.active_model; end
    def single_object?; length == 1; end
    def contains?(e); to_a.include?(e); end
    alias include? contains?
    def is_curve?; Esq.call('sel.kind')['curve']; end
    def is_surface?; Esq.call('sel.kind')['surface']; end

    # Ajoute des entités (isolées, en tableau, ou une autre sélection) ; renvoie le nombre ajouté.
    def add(*ents)
      list = flat(ents)
      cur = Esq.call('sel.get')
      new = list.map(&:handle).uniq - cur
      return 0 if new.empty?
      set(cur + new)
      # What the document's selection took (only entities of the active context).
      new &= Esq.call('sel.get')
      return 0 if new.empty?
      Esq.emit(@observers, :onSelectionBulkChange, self)
      Esq.emit(@observers, :onSelectionAdded, self, list.first) if new.size == 1
      new.size
    end

    def remove(*ents)
      list = flat(ents)
      cur = Esq.call('sel.get')
      gone = list.map(&:handle) & cur
      return 0 if gone.empty?
      set(cur - gone)
      Esq.emit(@observers, :onSelectionBulkChange, self)
      Esq.emit(@observers, :onSelectionRemoved, self, list.first) if gone.size == 1
      gone.size
    end

    # Inverse la sélection de chacune des entités données ; renvoie le nombre d'entités traitées.
    def toggle(*ents)
      list = flat(ents)
      cur = Esq.call('sel.get')
      list.map(&:handle).uniq.each { |h| cur.include?(h) ? cur.delete(h) : cur << h }
      set(cur)
      Esq.emit(@observers, :onSelectionBulkChange, self)
      list.size
    end

    def clear
      had = !empty?
      set([])
      Esq.emit(@observers, :onSelectionCleared, self) if had
      nil
    end

    def invert
      Esq.call('sel.invert')
      Esq.emit(@observers, :onSelectionBulkChange, self)
      nil
    end

    # Retire et renvoie la première entité.
    def shift
      e = first
      return nil unless e
      remove(e)
      e
    end

    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end

    private

    def flat(ents)
      out = []
      ents.each do |e|
        case e
        when Array, Selection then out.concat(flat(e.to_a))
        when Entity then out << e
        else raise ArgumentError, "Cannot add #{e.class} to the selection"
        end
      end
      out
    end

    def set(hs); Esq.call('sel.set', 'hs' => hs); self; end
  end

  class Model
    VERSION_3 = 3
    VERSION_4 = 4
    VERSION_5 = 5
    VERSION_6 = 6
    VERSION_7 = 7
    VERSION_8 = 8
    VERSION_2013 = 9
    VERSION_2014 = 10
    VERSION_2015 = 11
    VERSION_2016 = 12
    VERSION_2017 = 13
    VERSION_2018 = 14
    VERSION_2019 = 15
    VERSION_2020 = 16
    VERSION_2021 = 17
    ProTrial = 1
    ProLicensed = 2
    MakeTrial = 4
    Make = 6
    LOAD_STATUS_SUCCESS = 0
    LOAD_STATUS_SUCCESS_MORE_RECENT = 1

    def initialize
      @observers = []
    end

    # ---- collections
    def entities; @entities ||= Entities.new('0'); end
    def active_entities
      ctx = Esq.host('model.path', {}, nil)
      ctx && ctx['ctx'] && ctx['ctx'] != '0' ? Entities.new(ctx['ctx']) : entities
    end
    def selection; @selection ||= Selection.new; end
    def definitions; @definitions ||= DefinitionList.new; end
    def materials; @materials ||= Materials.new; end
    def layers; @layers ||= Layers.new; end
    alias tags layers
    def hatch_patterns; @hatch_patterns ||= HatchPatterns.new; end
    def line_styles; @line_styles ||= LineStyles.new; end
    def options; @options ||= OptionsManager.send(:new); end
    def tools; @tools ||= (defined?(Tools) ? Tools.new : nil); end
    def overlays; @overlays ||= (defined?(Sketchup::OverlaysManager) ? Sketchup::OverlaysManager.new : nil); end
    def behavior; defined?(Behavior) ? Behavior.new('0') : nil; end

    def tags=(layers_or_names)
      raise ArgumentError, 'String expected' unless layers_or_names.is_a?(String)
      layers.add(layers_or_names)
    end

    # ---- identité du modèle
    def name; Esq.call('meta.get')['name']; end
    def name=(v)
      raise ArgumentError, 'String expected' unless v.is_a?(String)
      Esq.call('meta.set', 'name' => v)
      v
    end
    def description; Esq.call('meta.get')['description']; end
    def description=(v)
      raise ArgumentError, 'String expected' unless v.is_a?(String)
      Esq.call('meta.set', 'description' => v)
      v
    end
    def guid; Esq.call('meta.get')['guid']; end
    def path
      i = Esq.host('model.info', {}, nil)
      i ? i['path'].to_s : ''
    end
    def title
      p = path
      p.empty? ? '' : File.basename(p, '.*')
    end
    def modified?; (i = Esq.host('model.info', {}, nil)) ? i['dirty'] ? true : false : false; end
    def valid?; true; end
    def number_faces; Esq.call('meta.get')['number_faces']; end
    def mipmapping?; Esq.call('meta.get')['mipmapping']; end
    def mipmapping=(v); Esq.call('meta.set', 'mipmapping' => v ? true : false); v; end
    def get_product_family; ProLicensed; end

    # ---- étiquette et chemin actifs
    def active_layer; layers.__active; end
    # nil ou "Layer0" : l'étiquette par défaut ; TestUp le fait avant chaque test.
    def active_layer=(layer)
      l = layers.__tag_arg(layer)
      layers.__activate(l)
      l
    end

    def active_path
      r = Esq.host('model.path', {}, nil)
      hs = r ? r['path'] : []
      hs.empty? ? nil : hs.map { |h| Entity.wrap(h) }
    end

    # nil ou un tableau vide ferment tout ; sinon un chemin d'instance
    # (TypeError pour un autre objet ou un élément effacé, ArgumentError pour un
    # chemin invalide) dont la feuille, si ce n'est pas une instance, est laissée de côté. Ni
    # instance verrouillée, ni composant dynamique, ni autre
    # instance verrouillée d'une définition ouverte. Le changement est une opération : hors de
    # toute opération, onTransactionStart puis onTransactionCommit ; dans une opération, celle-ci
    # est validée (si elle a changé quelque chose) et rouverte avant, validée et rouverte après.
    def active_path=(path)
      list = []
      unless path.nil? || (path.is_a?(Array) && path.empty?)
        unless path.is_a?(InstancePath) || path.is_a?(Array)
          raise TypeError, 'Argument must be type of Sketchup::InstancePath or an Array representing an instance path'
        end
        gone = path.to_a.find { |e| e.is_a?(Sketchup::Entity) && !e.valid? }
        raise TypeError, "reference to deleted #{gone.class.name.split('::').last}" if gone
        ip = Esq.instance_path_arg(path)
        list = ip.to_a.take_while { |e| e.is_a?(Sketchup::Group) || e.is_a?(Sketchup::ComponentInstance) }
        raise ArgumentError, 'Cannot open instance path with locked instances' if list.any?(&:locked?)
        if list.any? { |e| e.definition.live_component? }
          raise ArgumentError, 'Cannot open instance path with live components'
        end
        if list.any? { |e| e.definition.instances.any? { |i| i != e && i.locked? } }
          raise ArgumentError, 'Cannot open instance path with locked sibling instances'
        end
      end
      before = active_path || []
      op = Esq.call('op.state')
      if op && Esq::Events.op_changed?
        commit_operation
        start_operation(op['name'], true, nil, true)
      end
      Esq.emit(@observers, :onTransactionStart, self) unless op
      Esq.host('model.set_path', { 'path' => list.map(&:handle) }, nil)
      if op
        @__path_in_op = true
        commit_operation
        start_operation(op['name'], op['transparent'], nil, true)
      else
        Esq.emit(@observers, :onTransactionCommit, self)
      end
      (before - list).reverse_each { |i| Esq::Events.fire_entity(i.handle, :onClose) }
      (list - before).each { |i| Esq::Events.fire_entity(i.handle, :onOpen) }
      Esq.emit(@observers, :onActivePathChanged, self)
      path
    end

    def edit_transform
      (active_path || []).inject(Geom::Transformation.new) { |t, i| t * i.transformation }
    end

    def active_section_planes
      active_entities.grep(Sketchup::SectionPlane).select { |s| s.respond_to?(:active?) && s.active? }
    rescue StandardError
      []
    end

    # Comme SketchUp : le chemin est un InstancePath ou un tableau qui en décrit
    # un, sa feuille un Drawingelement. Chaque élément, feuille comprise, cache
    # tout s'il a une étiquette cachée ou dans un dossier caché, ou s'il est masqué sans que
    # l'option de son genre ne montre les masqués : DrawHiddenObjects pour les groupes, instances
    # de composant et plans de coupe (types 5, 0x14 et 0x1f), DrawHiddenGeometry pour le reste
    def drawing_element_visible?(path)
      path = Esq.instance_path_arg(path)
      leaf = path.leaf
      raise ArgumentError, 'Leaf must be a type of Sketchup::Drawingelement' if leaf && !leaf.is_a?(Sketchup::Drawingelement)
      ro = rendering_options
      path.to_a.all? do |e|
        next false if Esq.tag_hidden?(e.layer)
        next true unless e.hidden?
        object = e.is_a?(Sketchup::Group) || e.is_a?(Sketchup::ComponentInstance) || e.is_a?(Sketchup::SectionPlane)
        ro[object ? 'DrawHiddenObjects' : 'DrawHiddenGeometry'] ? true : false
      end
    end

    # ---- opérations annulables
    def start_operation(*args)
      raise ArgumentError, 'Description required' if args.empty? || !args[0].is_a?(String)
      warn 'third argument of Sketchup::Model.start_operation is deprecated', uplevel: 1 if args.size == 3
      transparent = args[3] ? true : false
      r = Esq.call('op.start', 'name' => args[0], 'transparent' => transparent)
      warn r['warn'], uplevel: 1 if r['warn']
      Esq.emit(@observers, :onTransactionStart, self)
      true
    end

    # Une opération qui n'a rien changé s'annonce vide (onTransactionEmpty) ; le changement de
    # chemin actif compte (SketchUp valide l'opération qui le contient).
    def commit_operation
      changed = @__path_in_op || !defined?(Esq::Events) || Esq::Events.op_changed?
      @__path_in_op = false
      Esq.call('op.commit')
      Esq.emit(@observers, changed ? :onTransactionCommit : :onTransactionEmpty, self)
      true
    end

    def abort_operation
      Esq.call('op.abort')
      Esq.emit(@observers, :onTransactionAbort, self)
      true
    rescue RuntimeError
      true
    end

    # ---- fichiers
    def save(*args)
      path = args[0]
      cur = self.path
      raise ArgumentError, 'File name must be specified for unsaved models.' if (path.nil? || path.to_s.empty?) && cur.empty?
      r = Esq.call('host.model.save', 'path' => (path || cur).to_s, 'version' => args[1])
      r['ok'] ? true : false
    end

    def save_copy(*args)
      raise ArgumentError, 'File name must be specified for model copy.' if args.empty?
      path = args[0].to_s
      raise ArgumentError, 'File name cannot be empty.' if path.empty?
      raise ArgumentError, 'Model must be saved before copying.' if self.path.empty?
      raise ArgumentError, 'Cannot save copy to current model file.' if File.expand_path(path) == File.expand_path(self.path)
      Esq.call('host.model.save_copy', 'path' => path, 'version' => args[1])['ok'] ? true : false
    end

    def save_thumbnail(path); Esq.host('model.thumbnail', { 'image' => path.to_s }, false) ? true : false; end

    # un argument au plus (rb_error_arity).
    def close(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      raise RuntimeError, 'Only the active model can be closed' unless equal?(Sketchup.active_model)
      Esq.host('model.close', { 'discard' => args[0] ? true : false }, nil)
      Esq::Events.entities_replaced if defined?(Esq::Events)
      nil
    end
    def close_active; close; end

    # Extensions of the exporters (SketchUp raises for any other); .skp and .esq are Esquisse's.
    EXPORT_EXT = %w[skp esq 3ds dae dwg dxf fbx ifc kmz obj stl wrl xsi glb usdz
                    png jpg jpeg bmp tif tiff tga pdf eps].freeze

    # The second argument of #export / #import as SketchUp reads it: a Hash gives the
    # exporter's (importer's) options, looked up by Symbol only, and :show_summary when it
    # is true or false; anything else is only "show the summary" (truthiness).
    def __format_options(arg)
      return [{}, arg ? true : false] unless arg.is_a?(Hash)
      opts = {}
      arg.each do |k, v|
        next unless k.is_a?(Symbol)
        case v
        when true, false, Integer, String then opts[k.to_s] = v
        when Float then opts[k.to_s] = v if v.finite?
        end
      end
      [opts, arg[:show_summary] == true]
    end
    private :__format_options

    # SketchUp: ArgumentError without a path or for an extension no exporter has; then
    # true when the exporter wrote the file, false otherwise.
    def export(*args)
      raise ArgumentError, 'Not enough arguments given' if args.empty?
      path = Esq.str_arg(args[0])
      ext = File.extname(path).delete('.').downcase
      raise ArgumentError, 'Unsupported file extension' unless EXPORT_EXT.include?(ext)
      opts, show = __format_options(args[1])
      r = Esq.host('model.export', { 'path' => path, 'options' => opts, 'show' => show }, nil)
      r.is_a?(Hash) && r['ok'] ? true : false
    end

    # SketchUp: an importer registered by Ruby gets #load_file(path, show_summary) and the
    # import succeeds when it answers ImportSuccess (nil is a failure); the others read the
    # file into a component placed like File › Import does. false when no importer has the
    # extension or the file cannot be read.
    def import(*args)
      raise ArgumentError, 'Not enough arguments given' if args.empty?
      path = Esq.str_arg(args[0])
      opts, show = __format_options(args[1])
      ext = File.extname(path).delete('.').downcase
      imp = Esq.importers.find { |i| i.file_extension.to_s.downcase.delete('.') == ext }
      if imp
        r = imp.load_file(path, show)
        code = case r
               when nil then Importer::ImportFail
               when Integer then r
               when Float then r.to_i
               else raise TypeError, "no implicit conversion of #{[true, false].include?(r) ? r : r.class} into Integer"
               end
        return code == Importer::ImportSuccess
      end
      r = Esq.host('model.import', { 'path' => path, 'options' => opts, 'show' => show }, nil)
      r.is_a?(Hash) && r['ok'] ? true : false
    end

    def merge(*args)
      first = args[0]
      unless first.nil? || first.is_a?(Array)
        raise ArgumentError, 'Error : merge - First argument must be an entity array or nil'
      end
      (args[1..] || []).each { |p| import(p.to_s) }
      true
    end

    # SketchUp: a Component tool for the definition; `repeat` keeps placing
    # copies, else one click places it, selects it and hands over to Move. Returns the model.
    def place_component(definition, repeat = false)
      unless definition.is_a?(ComponentDefinition)
        raise TypeError, 'wrong argument type (expected Sketchup::ComponentDefinition)'
      end
      raise TypeError, 'reference to deleted ComponentDefinition' unless definition.valid?
      r = Esq.host('tools.place_component', { 'definition' => definition.ctx, 'repeat' => repeat ? true : false }, :none)
      raise NotImplementedError, 'There is no tool manager' if r == :none
      raise ArgumentError, 'Invalid component - cannot place' unless r
      self
    end

    def select_tool(tool)
      r = Esq.host('tools.select', { 'tool' => tool.class.name }, :none)
      raise NotImplementedError, 'There is no tool manager' if r == :none
      true
    end

    # ---- recherche d'entités
    # Les arguments de find_entity_by_id et find_entity_by_persistent_id 
    # au moins un ; un seul tableau donne la liste à chercher (non vide) et un
    # tableau en réponse ; une seule valeur, la réponse seule.
    def __id_list(args)
      raise ArgumentError, 'Must have at least one argument.' if args.empty?
      return [args, false] unless args.size == 1
      return [[args[0]], true] unless args[0].is_a?(Array)
      raise ArgumentError, 'Must have at least one argument.' if args[0].empty?
      [args[0], false]
    end
    private :__id_list

    # Tout ce qui a un identifiant : les entités de chaque collection (sommets et courbes des
    # arêtes compris), les définitions, les étiquettes et leurs dossiers, les matières et leurs
    # textures, les scènes, les styles.
    def __identified
      out = []
      seen = {}
      add = ->(e) { out << e if e && !seen.key?(e.handle) && (seen[e.handle] = true) }
      ([entities] + definitions.map(&:entities)).each do |es|
        es.each do |e|
          add.(e)
          next unless e.is_a?(Sketchup::Edge)
          add.(e.start)
          add.(e.end)
          add.(e.curve)
        end
      end
      out.concat(definitions.to_a)
      out.concat(layers.to_a)
      layers.__all_folders.each { |f| out << f } if layers.respond_to?(:__all_folders)
      materials.each do |m|
        out << m
        t = m.texture
        out << t if t
      end
      out.concat(pages.to_a) if respond_to?(:pages)
      out.concat(styles.to_a) if respond_to?(:styles)
      out
    end
    private :__identified

    # des entityID (Integer) ou des GUID (String), pas les deux ; le genre de
    # la première valeur décide (TypeError sinon, et pour une valeur d'un autre genre).
    def find_entity_by_id(*args)
      ids, single = __id_list(args)
      found =
        if ids[0].is_a?(Integer)
          ids = ids.map { |i| Esq::Conv.num2long(i) }
          table = {}
          __identified.each { |e| table[e.entityID] ||= e }
          ids.map { |i| table[i] }
        elsif ids[0].is_a?(String)
          ids = ids.map { |g| Esq.str_arg(g) }
          table = {}
          ([entities] + definitions.map(&:entities)).each do |es|
            es.each { |e| table[e.guid] ||= e if e.is_a?(Sketchup::Group) || e.is_a?(Sketchup::ComponentInstance) }
          end
          definitions.each { |d| table[d.guid] ||= d }
          ids.map { |g| g.empty? ? nil : table[g] }
        else
          raise TypeError, 'Must be either entityID Integer or GUID String'
        end
      single ? found[0] : found
    end
    alias_method :__find_by_id, :find_entity_by_id

    # (rb_scan_args « *: ») puis : sans mots-clés tout est cherché,
    # sinon seulement les genres demandés (entities: ce que contiennent le modèle et les
    # définitions, layers:, layer_folders:, materials:, pages:, styles:, definitions:).
    def find_entity_by_persistent_id(*args)
      scope = args.size > 1 && args.last.is_a?(Hash) ? args.pop : nil
      ids, single = __id_list(args)
      ids = ids.map { |i| Esq::Conv.num2long(i) }
      want = ->(k) { scope.nil? || scope[k] }
      pool = []
      if want.(:entities)
        ([entities] + definitions.map(&:entities)).each { |es| pool.concat(es.to_a) }
      end
      pool.concat(layers.to_a) if want.(:layers)
      pool.concat(layers.__all_folders) if want.(:layer_folders) && layers.respond_to?(:__all_folders)
      pool.concat(materials.to_a) if want.(:materials)
      pool.concat(pages.to_a) if want.(:pages) && respond_to?(:pages)
      pool.concat(styles.to_a) if want.(:styles) && respond_to?(:styles)
      pool.concat(definitions.to_a) if want.(:definitions)
      table = {}
      pool.each { |e| table[e.persistent_id] ||= e }
      found = ids.map { |i| table[i] }
      single ? found[0] : found
    end

    # une chaîne (StringValue) de persistent_id séparés par des points, chaque
    # élément pris dans la collection du précédent.
    def instance_path_from_pid_path(pid_path)
      pid_path = Esq.str_arg(pid_path)
      raise ArgumentError, 'invalid pid path' unless pid_path =~ /\A\d+(\.\d+)*\z/
      ents = entities
      chain = []
      pid_path.split('.').each do |pid|
        raise ArgumentError, 'unable to locate full pid path' if ents.nil?
        e = ents.find { |x| x.persistent_id == pid.to_i }
        raise ArgumentError, 'unable to locate full pid path' unless e
        chain << e
        ents = e.is_a?(Sketchup::Group) || e.is_a?(Sketchup::ComponentInstance) ? e.definition.entities : nil
      end
      raise ArgumentError, 'unable to find an instance path' if chain.empty?
      InstancePath.new(chain)
    end

    def raytest(*args)
      ray = args[0]
      raise ArgumentError, 'wrong number of arguments' unless ray.is_a?(Array) && ray.size == 2
      wrap = args[1] ? true : false
      r = Esq.call('meta.raytest', 'origin' => Geom::Point3d.new(*ray[0]).to_a, 'dir' => Geom::Vector3d.new(*ray[1].to_a).to_a)
      return nil unless r
      path = r['path'].map { |h| Entity.wrap(h) }
      [Geom::Point3d.new(*r['point']), wrap ? [path] : path]
    end

    def add_note(*args)
      raise ArgumentError, 'Wrong number of arguments - 3 expected' unless args.size == 3
      text, x, y = args
      raise ArgumentError, 'Coordinates must be between 0 and 1' if x < 0 || x > 1 || y < 0 || y > 1
      h = Esq.call('meta.add_note', 'text' => text.to_s, 'x' => x, 'y' => y)
      defined?(Sketchup::Text) ? Entity.wrap(h) : nil
    end

    def bounds
      bb = Geom::BoundingBox.new
      entities.each { |e| bb.add(e.bounds) if e.respond_to?(:bounds) }
      bb
    end

    # ---- image de diffusion
    def add_diffusion_image(path, opacity)
      raise ArgumentError, 'Opacity must be between 0.0 and 1.0.' if opacity < 0 || opacity > 1
      raise ArgumentError, 'File path cannot be empty.' if path.to_s.empty?
      raise ArgumentError, 'Failed to add diffusion image.' unless File.file?(path.to_s)
      Esq.call('meta.set', 'diffusion' => path.to_s, 'diffusion_opacity' => opacity.to_f)
      true
    end
    def clear_diffusion_image; Esq.call('meta.set', 'diffusion' => nil); true; end
    def diffusion_image_opacity; Esq.call('meta.get')['diffusion_opacity']; end
    def diffusion_image_opacity=(v)
      raise ArgumentError, 'Opacity must be between 0.0 and 1.0' if v < 0 || v > 1
      Esq.call('meta.set', 'diffusion_opacity' => v.to_f)
      v
    end

    # ---- géolocalisation
    def georeferenced?; Esq.call('meta.get')['georeferenced']; end
    def list_datums; %w[WGS84 NAD83 NAD27 ETRS89 GDA94]; end
    def get_datum; Esq.call('meta.get')['datum']; end
    def set_datum(datum)
      raise ArgumentError, 'unknown datum' unless list_datums.include?(datum.to_s)
      Esq.call('meta.set', 'datum' => datum.to_s)
      true
    end
    # Calculées depuis la géolocalisation du modèle (celle du modèle type tant qu'aucune n'a été
    # choisie, comme dans SketchUp).
    def point_to_latlong(point)
      Esq::Geo.point_to_latlong(self, Geom::Point3d.new(*point.to_a))
    end
    def latlong_to_point(ll)
      Esq::Geo.latlong_to_point(self, ll)
    end
    def point_to_utm(point)
      lat, lon = Esq::Geo.ll_parts(point_to_latlong(point))
      Esq::Geo.utm(*Esq::Geo.to_utm(lat, lon))
    end
    def utm_to_point(utm)
      latlong_to_point(Esq::Geo.latlong(*Esq::Geo.from_utm(*Esq::Geo.utm_parts(utm))))
    end

    # Pointeur du modèle pour l'API C (skpdoc(true)) ; sans argument, l'ancien SDK C++ : nil.
    def skpdoc(*args)
      if args.size > 1 || (args.size == 1 && ![true, false].include?(args[0]))
        raise ArgumentError, 'Method requires at most 1 boolean argument'
      end
      return object_id if args[0] == true
      warn 'The C++ SDK was completely removed after SketchUp 2019. Migrate to the C API for continued use.', uplevel: 1
      nil
    end

    # ---- observateurs
    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end

    private

    # Toutes les entités du modèle (racine et définitions).
    def all_entities
      out = entities.to_a
      definitions.each { |d| out.concat(d.entities.to_a) }
      out
    end
  end

end


# ---------------------------------------------------------------------------- géodésie
# Conversions entre le modèle (pouces), la latitude/longitude et l'UTM (WGS84). L'origine du
# modèle est à la géolocalisation enregistrée (ShadowInfo) ; le nord est tourné de NorthAngle.
module Esq
  module Geo
    A = 6_378_137.0
    F = 1 / 298.257223563
    K0 = 0.9996
    E2 = F * (2 - F)
    EP2 = E2 / (1 - E2)
    BANDS = 'CDEFGHJKLMNPQRSTUVWXX'

    module_function

    def latlong(lat, lon)
      defined?(Geom::LatLong) ? Geom::LatLong.new(lat, lon) : [lat, lon]
    end

    def utm(zone, letter, x, y)
      defined?(Geom::UTM) ? Geom::UTM.new(zone, letter, x, y) : [zone, letter, x, y]
    end

    def ll_parts(ll)
      ll.respond_to?(:latitude) ? [ll.latitude.to_f, ll.longitude.to_f] : ll.to_a.map(&:to_f)
    end

    def utm_parts(u)
      return [u.zone_number.to_i, u.zone_letter.to_s, u.x.to_f, u.y.to_f] if u.respond_to?(:zone_number)
      a = u.to_a
      [a[0].to_i, a[1].to_s, a[2].to_f, a[3].to_f]
    end

    # [zone, lettre, x (est, m), y (nord, m)] ; `zone` force le fuseau.
    def to_utm(lat, lon, zone = nil)
      zone ||= ((lon + 180) / 6).floor % 60 + 1
      letter = BANDS[((lat.clamp(-80, 84) + 80) / 8).floor.clamp(0, 20)]
      lon0 = ((zone - 1) * 6 - 180 + 3) * Math::PI / 180
      phi = lat * Math::PI / 180
      n = A / Math.sqrt(1 - E2 * Math.sin(phi)**2)
      t = Math.tan(phi)**2
      c = EP2 * Math.cos(phi)**2
      a = Math.cos(phi) * (lon * Math::PI / 180 - lon0)
      m = A * ((1 - E2 / 4 - 3 * E2**2 / 64 - 5 * E2**3 / 256) * phi -
               (3 * E2 / 8 + 3 * E2**2 / 32 + 45 * E2**3 / 1024) * Math.sin(2 * phi) +
               (15 * E2**2 / 256 + 45 * E2**3 / 1024) * Math.sin(4 * phi) - (35 * E2**3 / 3072) * Math.sin(6 * phi))
      x = K0 * n * (a + (1 - t + c) * a**3 / 6 + (5 - 18 * t + t**2 + 72 * c - 58 * EP2) * a**5 / 120) + 500_000
      y = K0 * (m + n * Math.tan(phi) * (a**2 / 2 + (5 - t + 9 * c + 4 * c**2) * a**4 / 24 +
                                         (61 - 58 * t + t**2 + 600 * c - 330 * EP2) * a**6 / 720))
      y += 10_000_000 if lat < 0
      [zone, letter, x, y]
    end

    def from_utm(zone, letter, x, y)
      e1 = (1 - Math.sqrt(1 - E2)) / (1 + Math.sqrt(1 - E2))
      x -= 500_000
      y -= 10_000_000 if letter.upcase < 'N'
      mu = y / K0 / (A * (1 - E2 / 4 - 3 * E2**2 / 64 - 5 * E2**3 / 256))
      phi1 = mu + (3 * e1 / 2 - 27 * e1**3 / 32) * Math.sin(2 * mu) + (21 * e1**2 / 16 - 55 * e1**4 / 32) * Math.sin(4 * mu) +
             (151 * e1**3 / 96) * Math.sin(6 * mu) + (1097 * e1**4 / 512) * Math.sin(8 * mu)
      n1 = A / Math.sqrt(1 - E2 * Math.sin(phi1)**2)
      t1 = Math.tan(phi1)**2
      c1 = EP2 * Math.cos(phi1)**2
      r1 = A * (1 - E2) / (1 - E2 * Math.sin(phi1)**2)**1.5
      d = x / (n1 * K0)
      lat = phi1 - (n1 * Math.tan(phi1) / r1) * (d**2 / 2 - (5 + 3 * t1 + 10 * c1 - 4 * c1**2 - 9 * EP2) * d**4 / 24 +
                                                  (61 + 90 * t1 + 298 * c1 + 45 * t1**2 - 252 * EP2 - 3 * c1**2) * d**6 / 720)
      lon0 = ((zone - 1) * 6 - 180 + 3) * Math::PI / 180
      lon = lon0 + (d - (1 + 2 * t1 + c1) * d**3 / 6 +
                    (5 - 2 * c1 + 28 * t1 - 3 * c1**2 + 8 * EP2 + 24 * t1**2) * d**5 / 120) / Math.cos(phi1)
      [lat * 180 / Math::PI, lon * 180 / Math::PI]
    end

    # Décalage (est, nord) en mètres d'un point du modèle par rapport à l'origine.
    def info; Esq.call('meta.get'); end

    def point_to_latlong(_model, p)
      i = info
      zone, letter, x0, y0 = to_utm(i['latitude'], i['longitude'])
      a = i['north_angle'].to_f * Math::PI / 180
      dx, dy = p.x * 0.0254, p.y * 0.0254
      east = dx * Math.cos(a) + dy * Math.sin(a)
      north = -dx * Math.sin(a) + dy * Math.cos(a)
      latlong(*from_utm(zone, letter, x0 + east, y0 + north))
    end

    def latlong_to_point(_model, ll)
      i = info
      zone, _l, x0, y0 = to_utm(i['latitude'], i['longitude'])
      lat, lon = ll_parts(ll)
      # Toujours dans le fuseau de l'origine, pour que les distances restent continues.
      _z, _l2, x, y = to_utm(lat, lon, zone)
      y -= 10_000_000 if lat < 0 && i['latitude'] >= 0
      y += 10_000_000 if lat >= 0 && i['latitude'] < 0
      east, north = x - x0, y - y0
      a = i['north_angle'].to_f * Math::PI / 180
      dx = east * Math.cos(a) - north * Math.sin(a)
      dy = east * Math.sin(a) + north * Math.cos(a)
      Geom::Point3d.new(dx / 0.0254, dy / 0.0254, 0)
    end
  end
end
