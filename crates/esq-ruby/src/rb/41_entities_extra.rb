# Entities: Entity, Drawingelement, Edge, EdgeUse, Face, Vertex,
# Loop, Curve, ArcCurve, ConstructionPoint, ConstructionLine, Entities, EntitiesBuilder, Text,
# Dimension*, SectionPlane, Image, Axes. Loaded after the first-iteration 21_entities.rb and
# redefines what it had; the native side is bridge/entities.rs.

# ---------------------------------------------------------------- bridge errors and helpers
module Esq
  # Native calls that only read; the others may change the model.
  READ_OPS = /(\A|\.)(info|get|list|valid\??|names?|count|bounds|query|state|find|raytest|read|exists|is_\w+|has_\w+|take|ping|size)\z|\Apick\./

  class << self
    # Errors raised by the native side start with "ClassName: " when Ruby must raise that class.
    def call(op, args = {})
      @writes = writes + 1 unless READ_OPS.match?(op)
      r = JSON.parse(__native(op, JSON.generate(args)))
      raise_native(r['err']) if r.key?('err')
      r['ok']
    end

    # How many native calls could have changed the model so far (Entities#each looks again at
    # what it yields once it moves).
    def writes; @writes || 0; end

    def raise_native(msg)
      m = /\A(ArgumentError|TypeError|RuntimeError|RangeError|NotImplementedError|FrozenError|IndexError): (.*)\z/m.match(msg)
      raise Object.const_get(m[1]), m[2] if m
      raise msg
    end

    # [x, y, z] (inches) from a Point3d, Vector3d or Array. A point is read as SketchUp reads
    # one (Esq::G.pt): 2 or 3 numbers, a Point3d, a Vertex or an InputPoint.
    def xyz(p, what = 'Geom::Point3d')
      return G.pt(p) if what == 'Geom::Point3d'
      a = p.respond_to?(:to_a) ? p.to_a : nil
      unless a && a.size == 3 && a.all? { |v| v.is_a?(Numeric) }
        raise ArgumentError, "Cannot convert argument to #{what}"
      end
      a.map(&:to_f)
    end

    # Une valeur de point au sens : un Point3d ou un tableau de 2 ou 3 nombres.
    def point_value?(v)
      v.is_a?(Geom::Point3d) || (v.is_a?(Array) && (2..3).cover?(v.size) && v.all? { |x| x.is_a?(Numeric) })
    end

    # ---- attaches des textes et des cotes
    # Un chemin d'attache est gardé dans une propriété : les poignées de ses éléments séparées
    # par des espaces, la dernière celle de la feuille ; une entité du contexte de l'annotation
    # est un chemin d'un seul élément.
    def path_handles(path); path.to_a.map(&:handle).join(' '); end

    # Attache le bout `which` (0 départ ou point d'un texte, 1 arrivée) de l'annotation `h` à
    # la feuille des poignées `handles` au point `pt` (esq_core::attach) ; sans poignées, ou si
    # elles ne mènent pas à ce point, le bout reste libre. `mode` 'along' : sur un bord, le point
    # reste un point du bord (sinon un bout devient son sommet, son milieu le milieu).
    def attach_end(h, which, handles, pt, mode = 'dim')
      return unless handles.is_a?(String) && !handles.empty?
      call('annot.attach', 'h' => h, 'which' => which, 'handles' => handles, 'pt' => pt, 'mode' => mode)
    rescue ArgumentError
      nil
    end

    # Le chemin gardé, relu depuis le contexte `ctx` de l'annotation ; nil s'il ne tient plus.
    def attach_path(s, ctx)
      return nil unless s.is_a?(String) && !s.empty?
      ents = s.split(' ').map { |h| (Sketchup::Entity.wrap(h) rescue nil) }
      return nil unless ents.all? { |e| e && e.valid? } && ents.first.ctx == ctx
      ents.each_cons(2) do |a, b|
        return nil unless (a.is_a?(Sketchup::Group) || a.is_a?(Sketchup::ComponentInstance)) && b.ctx == a.definition.ctx
      end
      Sketchup::InstancePath.new(ents)
    rescue ArgumentError, TypeError
      nil
    end

    # Un bout de cote sans chemin, lu dans `args` à partir de `i` : un tableau
    # de deux éléments relu comme une suite, un InputPoint, un Vertex, un point, un
    # ConstructionPoint, ou un Edge ou une ConstructionLine suivi d'un point, projeté sur sa
    # droite. Rend [entité ou nil, point, indice suivant] ou nil.
    def dim_end(args, i)
      return nil unless i < args.size
      a = args[i]
      if a.is_a?(Array) && a.size == 2
        r = dim_end(a, 0)
        return r && [r[0], r[1], i + 1]
      end
      case a
      when Sketchup::InputPoint then return [a.vertex, a.position.to_a, i + 1]
      when Sketchup::Vertex, Sketchup::ConstructionPoint then return [a, a.position.to_a, i + 1]
      end
      return [nil, sketchup_point(a), i + 1] if point_value?(a)
      return nil unless i + 1 < args.size && point_value?(args[i + 1])
      pt = Geom::Point3d.new(sketchup_point(args[i + 1]))
      line = case a
             when Sketchup::Edge then [a.start.position, a.end.position]
             when Sketchup::ConstructionLine then [a.position, a.direction]
             end
      line && [a, pt.project_to_line(line).to_a, i + 2]
    end

    # Le décalage d'une cote sans chemin : un vecteur non nul, sinon nil.
    def dim_offset(v)
      a = xyz(v, 'Geom::Vector3d')
      a.any? { |x| x != 0.0 } ? a : nil
    rescue ArgumentError
      nil
    end

    # Un bout de cote attaché par un chemin : le point, en coordonnées du
    # modèle, doit tomber sur la feuille ; sur un bord, à un bout (la feuille devient ce sommet)
    # ou au milieu. Rend [poignées du chemin, point].
    def dim_connection(path, pt)
      leaf = path.leaf
      t = path.transformation
      p = Geom::Point3d.new(pt)
      off = 'Make sure the point is correctly transformed from local to global space.'
      case leaf
      when Sketchup::ConstructionPoint, Sketchup::Vertex
        w = leaf.position.transform(t)
        raise ArgumentError, off unless w == p
      when Sketchup::ConstructionLine
        w = p
        raise ArgumentError, off unless p.distance_to_line([leaf.position.transform(t), leaf.direction.transform(t)]) < Geom.tolerance
      when Sketchup::Edge
        a, b = leaf.start.position.transform(t), leaf.end.position.transform(t)
        if p == a then leaf, w = leaf.start, a
        elsif p == b then leaf, w = leaf.end, b
        elsif p == Geom.linear_combination(0.5, a, 0.5, b) then w = Geom.linear_combination(0.5, a, 0.5, b)
        else raise ArgumentError, 'Invalid point on edge. Point must be transformed and either be the start, mid or end point.'
        end
      else
        raise TypeError, 'Invalid start instance path leaf entity. Must be Sketchup::ConstructionPoint, Sketchup::ConstructionLine, Sketchup::Vertex, or Sketchup::Edge.'
      end
      [(path.to_a[0..-2] + [leaf]).map(&:handle).join(' '), w.to_a]
    end

    # Une valeur de matière comme SketchUp la convertit : nil ou 0 = aucune, une
    # Material, un nom de matière ou de couleur, sinon tout ce qui se convertit en couleur
    # (Color, tableau, entier) ; une couleur dont les quatre octets sont nuls = aucune.
    def material_name(m)
      case m
      when nil then nil
      when Sketchup::Material then m.name
      when Sketchup::Color
        return nil if m.to_a.all?(&:zero?)
        name = format('Color_%02X%02X%02X', m.red, m.green, m.blue)
        mats = Sketchup.active_model.materials
        mats.add(name).color = m unless mats[name]
        name
      when String
        mats = Sketchup.active_model.materials
        return m if mats[m]
        begin
          col = Sketchup::Color.new(m)
        rescue ArgumentError
          raise ArgumentError, "Cannot find material named #{m}"
        end
        mats.add(m).color = col
        m
      when Integer
        m.zero? ? nil : material_name(Sketchup::Color.new(m))
      else
        material_name(Sketchup::Color.new(m))
      end
    end

    # What material= and back_material= take: not a deleted material,
    # nor an image's or a tag's.
    def material_setter_check!(m)
      raise ArgumentError, 'Cannot assign deleted material' if m.is_a?(Sketchup::Material) && !m.valid?
      assignable_material!(m)
    end

    # neither an image's nor a tag's material goes on an entity of the model.
    def assignable_material!(m)
      return unless m.is_a?(Sketchup::Material)
      case m.owner_type
      when Sketchup::Material::OWNER_IMAGE then raise ArgumentError, 'Cannot assign image material to model entities'
      when Sketchup::Material::OWNER_LAYER then raise ArgumentError, 'Cannot assign layer material to model entities'
      end
    end

    # an instance of `defn` is on the path being edited.
    def open_definition?(defn)
      (Sketchup.active_model.active_path || []).any? { |i| i.definition == defn }
    end

    # What `glued_to=` glues to: nil ungules; anything else must be a live
    # entity (TypeError), the native side says which kind may take it.
    def glue_target(t)
      return nil if t.nil?
      expect(t, Sketchup::Entity)
      raise TypeError, 'reference to deleted Entity' unless t.valid?
      t.handle
    end

    def expect(obj, klass)
      raise TypeError, "wrong argument type (expected Sketchup::#{klass.name.split('::').last})" unless obj.is_a?(klass)
      obj
    end

    def observers; @observers ||= Hash.new { |h, k| h[k] = [] }; end
    def entity_attrs; @entity_attrs ||= {}; end
  end
end

# Alignments of Entities#add_3d_text (global constants, as SketchUp defines them).
TextAlignLeft = 0 unless defined?(TextAlignLeft)
TextAlignRight = 2 unless defined?(TextAlignRight)
TextAlignCenter = 1 unless defined?(TextAlignCenter)
TextVerticalAlignBoundsTop = 0 unless defined?(TextVerticalAlignBoundsTop)
TextVerticalAlignBaseline = 1 unless defined?(TextVerticalAlignBaseline)
TextVerticalAlignCapHeight = 2 unless defined?(TextVerticalAlignCapHeight)
TextVerticalAlignCenter = 3 unless defined?(TextVerticalAlignCenter)

# A polygon mesh as the API describes it; geom's own class wins when it is defined.
module Geom
  unless const_defined?(:PolygonMesh)
    class PolygonMesh
      def initialize(*); @points = []; @polygons = []; end
      def add_point(p)
        a = p.respond_to?(:to_a) ? p.to_a.map(&:to_f) : p
        i = @points.index { |q| q.to_a.zip(a).all? { |x, y| (x - y).abs < 1e-9 } }
        @points << Point3d.new(a) unless i
        (i || @points.size - 1) + 1
      end
      def add_polygon(*idx)
        idx = idx.flatten.map { |v| v.is_a?(Integer) ? v : add_point(v) }
        @polygons << idx
        @polygons.size
      end
      def points; @points.dup; end
      def polygons; @polygons.map(&:dup); end
      def count_points; @points.size; end
      def count_polygons; @polygons.size; end
      def point_at(i); @points[i - 1]; end
      def polygon_at(i); @polygons[i - 1]; end
      def polygon_points_at(i); polygon_at(i)&.map { |k| point_at(k.abs) }; end
    end
  end
end

# ---------------------------------------------------------------- Entity
module Sketchup
  class Entity
    KIND_CODE = { 'e' => 1, 'f' => 2, 'i' => 3, 'v' => 4, 'c' => 5, 'g' => 6, 'a' => 7, 's' => 8, 'l' => 9, 'u' => 10,
                  'n' => 11, 'p' => 12 }.freeze

    attr_reader :handle

    def initialize(handle); @handle = handle; end

    def self.wrap(handle, type = nil)
      case handle[0]
      when 'e' then Edge.new(handle)
      when 'f' then Face.new(handle)
      when 'v' then Vertex.new(handle)
      when 'n' then Snap.__wrap(handle)
      when 'p' then Space.__wrap(handle)
      else
        type ||= Esq.call('ent.type', 'h' => handle)
        Sketchup.const_get(type).new(handle)
      end
    end

    def ==(o); o.is_a?(Entity) && o.handle == @handle; end
    alias eql? ==
    def hash; @handle.hash; end

    def valid?; Esq.call('ent.valid', 'h' => @handle); end
    def deleted?; !valid?; end

    # Raises TypeError ("reference to deleted Edge") when the entity is gone.
    # Entities of other zones (definitions, pages, styles…) have their own handles and their
    # own `valid?`: ask them rather than the geometry bridge.
    def check!
      return Esq.call('ent.type', 'h' => @handle) if method(:valid?).owner == Sketchup::Entity
      raise TypeError, "reference to deleted #{self.class.name.split('::').last}" unless valid?
    end
    private :check!

    def entityID
      check!
      unique_id
    end

    # The id the entity had in the .skp it was read from (SketchUp 2021+ keeps them), else
    # one that is stable in the session.
    def persistent_id
      check!
      Esq.call('ent.pid', 'h' => @handle) || unique_id
    end

    def unique_id
      _k, cx, id = @handle.split(':')
      c = cx == '0' ? 0 : (cx.to_i & 0xffffffff) + 1
      ((c << 36) | ((id.to_i & 0xffffffff) << 4) | KIND_CODE.fetch(@handle[0], 0)) + 1
    end
    private :unique_id

    def typename
      check!
      self.class.name.split('::').last
    end

    def inspect
      addr = format('0x%016x', object_id << 3)
      valid? ? "#<#{self.class.name}:#{addr}>" : "#<Deleted Entity:#{addr}>"
    end
    alias to_s inspect

    def model; Sketchup.active_model; end

    def ctx; @handle.split(':')[1]; end

    def parent
      check!
      ctx == '0' ? model : ComponentDefinition.from_ctx(ctx)
    end

    # Observers: registered here, emitted by the observer zone.
    def add_observer(observer)
      return false unless valid?
      list = Esq.observers[@handle]
      list << observer unless list.include?(observer)
      true
    end

    def remove_observer(observer)
      !Esq.observers[@handle].delete(observer).nil?
    end

  end


  # ---------------------------------------------------------------- Drawingelement
  class Drawingelement < Entity
    def info; Esq.call('ent.get', 'h' => @handle); end

    def hidden?; info['hidden']; end
    def hidden=(v); Esq.call('ent.set', 'h' => @handle, 'hidden' => v ? true : false); end
    def visible?; !hidden?; end
    def visible=(v); self.hidden = !v; end

    def layer
      Sketchup.active_model.layers[info['layer']]
    end

    # A Layer, nil or "Layer0" (the default tag) or a tag name, as SketchUp converts it
    # (an unknown name or another type raises ArgumentError);
    # sent by index, since the model names the default tag otherwise.
    def layer=(l)
      layer = Sketchup.active_model.layers.__tag_arg(l)
      Esq.call('ent.set', 'h' => @handle, 'layer' => layer.__info['index'])
      layer
    end
    alias tag layer
    alias tag= layer=

    def material
      n = material_name
      n && Sketchup.active_model.materials[n]
    end

    def material=(m)
      Esq.material_setter_check!(m)
      write_material(Esq.material_name(m))
      m
    end

    def material_name
      i = info
      return i['front'] if i.key?('front')
      return i['material'] if i.key?('material')
      (i['props'] || {})['material']
    end
    private :material_name

    def write_material(name)
      if is_a?(Instance)
        set(material: name)
      else
        Esq.call('ent.set', 'h' => @handle, 'props' => { 'material' => name })
      end
    end
    private :write_material

    def casts_shadows?; prop_flag('casts'); end
    def casts_shadows=(v); set_prop_flag('casts', v); end
    def receives_shadows?; prop_flag('receives'); end
    def receives_shadows=(v); set_prop_flag('receives', v); end

    def prop_flag(k)
      v = (Esq.call('ent.get', 'h' => @handle)['props'] || {})[k]
      v.nil? ? true : v
    end
    private :prop_flag

    def set_prop_flag(k, v)
      Esq.call('ent.set', 'h' => @handle, 'props' => { k => v ? true : false })
    end
    private :set_prop_flag

    # an instance whose definition is being edited stays.
    def erase!
      if (is_a?(Group) || is_a?(ComponentInstance)) && valid? && Esq.open_definition?(definition)
        raise ArgumentError, 'cannot remove an instance in the active editing path'
      end
      Esq.call('ent.erase', 'hs' => [@handle])
      nil
    end

    def bounds; BoundingBoxOf.call(self); end
  end

  module BoundingBoxOf
    def self.call(ent)
      bb = Geom::BoundingBox.new
      case ent
      when Edge then bb.add(ent.start.position, ent.end.position)
      when Face then bb.add(*ent.vertices.map(&:position))
      when ConstructionPoint, Snap then bb.add(ent.position)
      when ConstructionLine then bb.add(*[ent.start, ent.end].compact)
      when Text then bb.add(ent.point) unless ent.info['kind'] == 'screen'
      when Dimension then bb.add(*ent.__points)
      when Image
        o = ent.origin
        t = ent.transformation
        x = t.xaxis.normalize
        y = t.yaxis.normalize
        w = ent.width
        h = ent.height
        bb.add(o, o.offset(x, w), o.offset(x, w).offset(y, h), o.offset(y, h))
      when Group, ComponentInstance
        local = Geom::BoundingBox.new
        ent.definition.entities.each { |e| local.add(e.bounds) if e.respond_to?(:bounds) }
        unless local.empty?
          mn = local.min
          mx = local.max
          [mn.x, mx.x].product([mn.y, mx.y], [mn.z, mx.z]).each { |c| bb.add(ent.transformation * Geom::Point3d.new(*c)) }
        end
      end
      bb
    end
  end

  # ---------------------------------------------------------------- Vertex
  class Vertex < Entity
    def initialize(handle); @handle = handle; end
    def info; Esq.call('ent.get', 'h' => @handle); end
    def position; Geom::Point3d.new(info['pos']); end
    def edges; info['edges'].map { |h| Edge.new(h) }; end
    def faces; info['faces'].map { |h| Face.new(h) }; end

    def loops
      faces.flat_map { |f| f.loops.select { |l| l.vertices.include?(self) } }
    end

    def common_edge(other)
      Esq.expect(other, Vertex)
      edges.find { |e| e.vertices.include?(other) }
    end

    def used_by?(ent)
      case ent
      when Edge then edges.include?(ent)
      when Face then ent.vertices.include?(self)
      else raise ArgumentError, 'Edge or Face required'
      end
    end

    # The curve this vertex lies inside of (two edges of one curve meet here), or nil.
    def curve_interior?
      es = edges
      return nil unless es.size == 2
      c0 = es[0].curve
      c0 && es[1].curve == c0 ? c0 : nil
    end
  end

  # ---------------------------------------------------------------- Edge
  class Edge < Drawingelement
    def initialize(handle); @handle = handle; end
    def start; Vertex.new(info['va']); end
    def end; Vertex.new(info['vb']); end
    def vertices; i = info; [Vertex.new(i['va']), Vertex.new(i['vb'])]; end

    def length(*args)
      t = args.first
      i = info
      a = Geom::Point3d.new(i['a'])
      b = Geom::Point3d.new(i['b'])
      return a.distance(b) if t.nil?
      (t * a).distance(t * b)
    end

    def line
      i = info
      a = Geom::Point3d.new(i['a'])
      [a, a.vector_to(Geom::Point3d.new(i['b'])).normalize]
    end

    def faces; info['faces'].map { |h| Face.new(h) }; end
    def smooth?; info['smooth']; end
    def soft?; info['soft']; end
    def smooth=(v); Esq.call('edge.set', 'h' => @handle, 'smooth' => v ? true : false); end
    def soft=(v); Esq.call('edge.set', 'h' => @handle, 'soft' => v ? true : false); end

    def curve
      c = info['curve']
      c && Entity.wrap(c)
    end

    def explode_curve
      Esq.call('edge.explode_curve', 'h' => @handle)
      self
    end

    def other_vertex(v)
      Esq.expect(v, Vertex)
      a, b = vertices
      return b if v == a
      return a if v == b
      nil
    end

    def used_by?(ent)
      case ent
      when Vertex then vertices.include?(ent)
      when Face then faces.include?(ent)
      else false
      end
    end

    def reversed_in?(face)
      Esq.expect(face, Face)
      i = info
      return nil unless i['faces'].include?(face.handle)
      face.info['loops'].each do |l|
        vs = l['verts']
        vs.each_index do |k|
          pair = [vs[k], vs[(k + 1) % vs.size]]
          return false if pair == [i['va'], i['vb']]
          return true if pair == [i['vb'], i['va']]
        end
      end
      nil
    end

    def common_face(other)
      Esq.expect(other, Edge)
      (faces & other.faces).first
    end

    def find_faces; Esq.call('edge.find_faces', 'h' => @handle); end

    def split(position)
      args = { 'h' => @handle }
      case position
      when Numeric then args['t'] = position.to_f
      else args['pt'] = Esq.xyz(position)
      end
      h = Esq.call('edge.split', args)
      h && Edge.new(h)
    end

    def all_connected
      Esq.call('ent.connected', 'h' => @handle).map { |h| Entity.wrap(h) }
    end
  end

  # ---------------------------------------------------------------- Face
  class Face < Drawingelement
    PointUnknown = 0
    PointInside = 1
    PointOnVertex = 2
    PointOnEdge = 4
    PointOnFace = 8
    PointOutside = 16
    PointNotOnPlane = 32

    def initialize(handle); @handle = handle; end

    def loops; info['loops'].each_index.map { |i| Loop.new(self, i) }; end
    def outer_loop; info; Loop.new(self, 0); end
    def vertices; info['loops'].flat_map { |l| l['verts'] }.map { |h| Vertex.new(h) }; end
    def edges; info['loops'].flat_map { |l| l['edges'].compact }.map { |h| Edge.new(h) }; end
    def normal; Geom::Vector3d.new(info['normal']); end

    def plane
      i = info
      n = i['normal']
      p = i['loops'][0]['pts'][0]
      [n[0], n[1], n[2], 0.0 - (n[0] * p[0] + n[1] * p[1] + n[2] * p[2])]
    end

    # une transformation, ou un tableau de 16 nombres.
    def area(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      i = info
      return i['area'] if args.empty?
      transform = case (t = args[0])
                  when Geom::Transformation then t
                  when Array
                    raise ArgumentError, 'Cannot convert argument to Geom::Transformation' unless t.size == 16 && t.all? { |x| x.is_a?(Numeric) }
                    Geom::Transformation.new(t)
                  else
                    raise TypeError, "wrong argument type (expected Geom::Transformation)"
                  end
      ring = lambda do |pts|
        q = pts.map { |p| (transform * Geom::Point3d.new(p)).to_a }
        s = [0.0, 0.0, 0.0]
        q.each_index do |k|
          a = q[k]
          b = q[(k + 1) % q.size]
          s[0] += a[1] * b[2] - a[2] * b[1]
          s[1] += a[2] * b[0] - a[0] * b[2]
          s[2] += a[0] * b[1] - a[1] * b[0]
        end
        Math.sqrt(s[0]**2 + s[1]**2 + s[2]**2) / 2.0
      end
      ls = i['loops']
      ring.call(ls[0]['pts']) - ls[1..].sum(0.0) { |l| ring.call(l['pts']) }
    end

    def material_name; info['front']; end
    private :material_name

    def write_material(name); Esq.call('face.set', 'h' => @handle, 'front' => name); end
    private :write_material

    def back_material; m = info['back']; m && Sketchup.active_model.materials[m]; end

    def back_material=(m)
      Esq.call('face.set', 'h' => @handle, 'back' => Esq.material_name(m))
      m
    end

    def pushpull(*args)
      raise ArgumentError, 'At least one argument (pushpull distance) required' if args.empty?
      # la distance passe par rb_num2dbl (TypeError hors nombre) ; avec copie, la
      # face reste celle de la base.
      d = args[0]
      raise TypeError, "no implicit conversion to float from #{d.nil? ? 'nil' : d.class.name.downcase}" unless d.is_a?(Numeric)
      dist = d.to_f
      return nil if dist.abs < 0.001
      copy = args.size > 1 && args[1] ? true : false
      r = Esq.call('face.pushpull', 'h' => @handle, 'd' => dist, 'keep' => copy)
      @handle = r if r && !copy
      nil
    end

    def reverse!
      Esq.call('face.reverse', 'h' => @handle)
      self
    end

    def classify_point(point)
      Esq.call('face.classify', 'h' => @handle, 'p' => Esq.xyz(point))
    end

    def coplanar_with?(other)
      return nil unless other.is_a?(Face)
      a = plane
      b = other.plane
      dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
      return false if dot.abs < 1.0 - 1e-9
      sign = dot > 0 ? 1.0 : -1.0
      (a[3] - sign * b[3]).abs < Geom.tolerance
    end

    def all_connected
      Esq.call('ent.connected', 'h' => @handle).map { |h| Entity.wrap(h) }
    end

    def followme(*edges)
      raise ArgumentError, 'Too few arguments - at least one Edge required' if edges.empty?
      # un tableau ne contient que des arêtes.
      edges.each do |a|
        next unless a.is_a?(Array)
        a.each { |x| raise TypeError, 'wrong argument type (expected Sketchup::Edge)' unless x.is_a?(Edge) }
      end
      es = edges.flatten.select { |e| e.is_a?(Edge) }
      es = edges.flatten.flat_map { |c| c.respond_to?(:edges) && !c.is_a?(Edge) ? c.edges : [] } if es.empty?
      Esq.call('face.followme', 'h' => @handle, 'edges' => es.map(&:handle))
    end

    # drapeaux en entier non signé (TypeError hors nombre, RangeError négatif ou
    # au-delà de 32 bits) ; UVQ avant (1), arrière (2), normales (4) ; les autres restent nuls.
    def mesh(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      f = args.empty? ? 0 : args[0]
      raise TypeError, "no implicit conversion of #{f.class} into Integer" unless f.is_a?(Numeric)
      flags = f.to_i
      raise RangeError, 'value cannot be negative' if flags.negative?
      raise RangeError, 'value is too large' if flags >= 2**32
      m = Esq.call('face.mesh', 'h' => @handle, 'flags' => flags)
      pm = Geom::PolygonMesh.new
      idx = m['pts'].map { |p| pm.add_point(p) }
      extra = m['uvq']
      idx.each_with_index do |i, k|
        pm.set_uv(i, extra['front'][k], true) if extra['front']
        pm.set_uv(i, extra['back'][k], false) if extra['back']
        pm.__set_normal(i, extra['normal'] || [0.0, 0.0, 0.0])
      end
      boundary = {}
      off = 0
      m['loops'].each do |n|
        n.times { |k| boundary[[off + k, off + (k + 1) % n]] = true }
        off += n
      end
      m['tris'].each do |a, b, c|
        corners = [a, b, c]
        sign = corners.each_index.map do |k|
          x = corners[k]
          y = corners[(k + 1) % 3]
          boundary[[x, y]] || boundary[[y, x]] ? 1 : -1
        end
        pm.add_polygon(*corners.each_index.map { |k| idx[corners[k]] * sign[k] })
      end
      pm
    end

    # The groups, components and images glued to this face (SketchUp's).
    def get_glued_instances
      Esq.call('face.glued', 'h' => @handle).map { |h, t| Entity.wrap(h, t) }
    end

    # Textures: `43_model_extra.rb` (UVHelper, position_material, uv_tile_at…).
    def set_texture_projection(_vector, _front)
      raise NotImplementedError, 'This method never worked properly and was removed in SketchUp 2021.1. Use face.position_material with the projection vector instead.'
    end
  end

  # ---------------------------------------------------------------- Loop and EdgeUse
  class Loop < Entity
    attr_reader :face, :index

    def initialize(face, index)
      @face = face
      @index = index
      _k, cx, id = face.handle.split(':')
      @handle = "l:#{cx}:#{id}.#{index}"
    end

    def check!; @face.info; end
    private :check!

    def unique_id; @face.send(:unique_id) * 1024 + @index + 1; end
    private :unique_id

    def ==(o); o.is_a?(Loop) && o.face == @face && o.index == @index; end
    alias eql? ==
    def hash; [@face.handle, @index].hash; end
    def valid?; @face.valid?; end
    def parent; @face.parent; end
    def ctx; @face.ctx; end

    def data; @face.info['loops'][@index]; end
    private :data

    def vertices; data['verts'].map { |h| Vertex.new(h) }; end
    def edges; data['edges'].compact.map { |h| Edge.new(h) }; end
    def edgeuses; data['edges'].each_index.map { |i| EdgeUse.new(self, i) }; end
    def outer?; @index.zero?; end

    def convex?
      d = data['pts']
      n = Geom::Vector3d.new(@face.info['normal'])
      sign = 0
      d.each_index do |i|
        a = Geom::Point3d.new(d[i])
        b = Geom::Point3d.new(d[(i + 1) % d.size])
        c = Geom::Point3d.new(d[(i + 2) % d.size])
        s = a.vector_to(b).cross(b.vector_to(c)).dot(n)
        next if s.abs < 1e-9
        s = s > 0 ? 1 : -1
        return false if sign != 0 && s != sign
        sign = s
      end
      true
    end
  end

  class EdgeUse < Entity
    attr_reader :loop, :index

    def initialize(loop, index)
      @loop = loop
      @index = index
      @handle = "u:#{loop.handle.split(':')[1]}:#{loop.handle.split(':')[2]}.#{index}"
    end

    def check!; @loop.send(:check!); end
    private :check!

    def unique_id; @loop.send(:unique_id) * 1024 + @index + 1; end
    private :unique_id

    def ==(o); o.is_a?(EdgeUse) && o.loop == @loop && o.index == @index; end
    alias eql? ==
    def hash; [@loop.hash, @index].hash; end
    def valid?; @loop.valid?; end
    def parent; @loop.parent; end
    def ctx; @loop.ctx; end

    def face; @loop.face; end
    def edge; Edge.new(@loop.send(:data)['edges'][@index]); end
    def next; EdgeUse.new(@loop, (@index + 1) % @loop.send(:data)['edges'].size); end
    def previous; EdgeUse.new(@loop, (@index - 1) % @loop.send(:data)['edges'].size); end

    def start_vertex; Vertex.new(@loop.send(:data)['verts'][@index]); end
    def end_vertex; Vertex.new(@loop.send(:data)['verts'][(@index + 1) % @loop.send(:data)['verts'].size]); end

    def reversed?
      e = edge.info
      e['va'] != @loop.send(:data)['verts'][@index]
    end

    def partners
      eh = edge.handle
      out = []
      edge.faces.each do |f|
        next if f == face
        f.info['loops'].each_with_index do |l, li|
          l['edges'].each_with_index { |h, k| out << EdgeUse.new(Loop.new(f, li), k) if h == eh }
        end
      end
      out
    end

    def start_vertex_normal; smooth_normal(start_vertex); end
    def end_vertex_normal; smooth_normal(end_vertex); end

    def smooth_normal(vertex)
      seen = { face.handle => true }
      sum = face.normal.to_a
      queue = [face]
      until queue.empty?
        f = queue.shift
        vertex.edges.each do |e|
          next unless e.faces.include?(f) && (e.smooth? || e.soft?)
          e.faces.each do |g|
            next if seen[g.handle] || !g.vertices.include?(vertex)
            seen[g.handle] = true
            sum = sum.zip(g.normal.to_a).map { |a, b| a + b }
            queue << g
          end
        end
      end
      Geom::Vector3d.new(sum).normalize
    end
    private :smooth_normal
  end

  # ---------------------------------------------------------------- Curve and ArcCurve
  class Curve < Entity
    def initialize(handle); @handle = handle; end
    def info; Esq.call('ent.get', 'h' => @handle); end
    def edges; info['edges'].map { |h| Edge.new(h) }; end
    # Walked edge by edge from the first one: a closed curve ends on the vertex
    # it started from, which comes twice.
    def vertices
      i = info
      vs = i['verts']
      vs += vs.first(1) if i['closed']
      vs.map { |h| Vertex.new(h) }
    end
    def count_edges; info['edges'].size; end
    def first_edge; Edge.new(info['edges'].first); end
    def last_edge; Edge.new(info['edges'].last); end

    def each_edge
      edges.each { |e| yield e }
      nil
    end

    def length
      pts = info['pts'].map { |p| Geom::Point3d.new(p) }
      sum = pts.each_cons(2).sum(0.0) { |a, b| a.distance(b) }
      sum += pts.last.distance(pts.first) if info['closed']
      sum
    end

    def is_polygon?; info['polygon'] ? true : false; end

    def move_vertices(points)
      raise TypeError, 'wrong argument type (expected Array)' unless points.is_a?(Array)
      Esq.call('curve.move_vertices', 'h' => @handle, 'pts' => points.map { |p| Esq.xyz(p) })
      true
    end
  end

  class ArcCurve < Curve
    def center; Geom::Point3d.new(info['center']); end
    def radius; info['radius']; end
    def normal; Geom::Vector3d.new(info['normal']); end
    def xaxis; Geom::Vector3d.new(info['xaxis']); end
    def yaxis; normal.cross(xaxis); end
    def start_angle; info['start']; end
    def end_angle; info['end']; end
    def circular?; info['circular'] ? true : false; end

    def plane
      n = info['normal']
      c = info['center']
      [n[0], n[1], n[2], 0.0 - (n[0] * c[0] + n[1] * c[1] + n[2] * c[2])]
    end
  end

  # ---------------------------------------------------------------- construction geometry
  class ConstructionPoint < Drawingelement
    def initialize(handle); @handle = handle; end
    def position; Geom::Point3d.new(info['pos']); end
  end

  class ConstructionLine < Drawingelement
    def initialize(handle); @handle = handle; end

    def position; Geom::Point3d.new(info['pos']); end

    # Each end of a construction line is a point or open (nil). Both ends: a segment; no end:
    # an infinite line; one end: a ray, kept as an infinite line through that end plus the
    # property "ray" naming the end that is closed.
    def ray; (info['props'] || {})['ray']; end
    private :ray

    def start
      i = info
      return Geom::Point3d.new(i['pos']) unless i['infinite']
      ray == 'start' ? Geom::Point3d.new(i['pos']) : nil
    end

    def end
      i = info
      return Geom::Point3d.new(i['pos']).offset(Geom::Vector3d.new(i['dir'])) unless i['infinite']
      ray == 'end' ? Geom::Point3d.new(i['pos']) : nil
    end

    def direction
      Geom::Vector3d.new(info['dir']).normalize
    end

    def set_ends(s, e, dir)
      if s && e
        Esq.call('guide.set', 'h' => @handle, 'pos' => s.to_a, 'mode' => 'segment', 'end' => e.to_a)
        r = nil
      else
        Esq.call('guide.set', 'h' => @handle, 'pos' => (s || e || position).to_a, 'mode' => 'line', 'dir' => dir.to_a)
        r = s ? 'start' : (e ? 'end' : nil)
      end
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'ray' => r })
    end
    private :set_ends

    def start=(p)
      q = p.nil? ? nil : Geom::Point3d.new(Esq.xyz(p))
      set_ends(q, self.end, direction)
      p
    end

    def end=(p)
      q = p.nil? ? nil : Geom::Point3d.new(Esq.xyz(p))
      set_ends(start, q, direction)
      p
    end

    def direction=(v)
      u = Geom::Vector3d.new(Esq.xyz(v, 'Geom::Vector3d')).normalize
      s = start
      e = self.end
      if s && e
        set_ends(s, s.offset(u, s.distance(e)), u)
      else
        set_ends(s, e, u)
      end
      v
    end

    def position=(p)
      Esq.call('guide.set', 'h' => @handle, 'pos' => Esq.xyz(p))
      p
    end

    def reverse!
      set_ends(self.end, start, direction.reverse)
      self
    end

    def stipple; (info['props'] || {})['stipple'] || ''; end

    def stipple=(s)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'stipple' => s.to_s })
      s
    end
  end

  # ---------------------------------------------------------------- Text
  class Text < Drawingelement
    def initialize(handle); @handle = handle; end

    def prop(k, default); v = (info['props'] || {})[k]; v.nil? ? default : v; end
    private :prop

    def text; info['text']; end
    def text=(s); Esq.call('annot.set', 'h' => @handle, 'text' => s.to_s); self; end
    alias set_text text=

    def point
      i = info
      i['kind'] == 'screen' ? Geom::Point3d.new(i['x'], i['y'], 0) : Geom::Point3d.new(i['point'])
    end

    def point=(p)
      return false if leader_type.zero?
      Esq.call('annot.set', 'h' => @handle, 'point' => Esq.xyz(p))
      true
    end

    def vector
      return nil if leader_type.zero?
      Geom::Vector3d.new(info['leader'])
    end

    def vector=(v)
      return false if leader_type.zero?
      Esq.call('annot.set', 'h' => @handle, 'leader' => Esq.xyz(v, 'Geom::Vector3d'))
      true
    end

    def leader_type
      prop('leader_type', info['kind'] == 'text' && info['leader'].any? { |c| c.abs > 1e-9 } ? 2 : 0).to_i
    end

    def leader_type=(t)
      t = Integer(t)
      if t.zero?
        warn 'ALeaderNone cannot be set. It is only used internally as a default state.'
        return nil
      end
      raise RangeError, 'Value out of range' unless [1, 2].include?(t)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'leader_type' => t.to_f })
      self
    end

    def has_leader?; leader_type != 0; end
    def display_leader?; prop('display_leader', true); end
    def display_leader=(v); Esq.call('ent.set', 'h' => @handle, 'props' => { 'display_leader' => v ? true : false }); end

    def arrow_type; prop('arrow_type', 0).to_i; end

    def arrow_type=(t)
      t = Integer(t)
      raise RangeError, 'Value out of range' unless (0..4).cover?(t)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'arrow_type' => t.to_f })
      self
    end

    def line_weight; prop('line_weight', 1).to_i; end

    def line_weight=(w)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'line_weight' => Integer(w).to_f })
      self
    end

    def font
      { name: prop('font_name', 'Arial'), size: prop('font_size', 12).to_i,
        bold: prop('font_bold', false), italic: prop('font_italic', false) }
    end

    def font=(h)
      raise TypeError, 'wrong argument type (expected Hash)' unless h.is_a?(Hash)
      props = {}
      props['font_name'] = h[:name].to_s if h.key?(:name)
      props['font_bold'] = h[:bold] ? true : false if h.key?(:bold)
      props['font_italic'] = h[:italic] ? true : false if h.key?(:italic)
      if h.key?(:size)
        size = Integer(h[:size])
        raise RangeError, format('Font size %d out of range (1..%d)', size, 1000) unless (1..1000).cover?(size)
        props['font_size'] = size.to_f
      end
      Esq.call('ent.set', 'h' => @handle, 'props' => props)
      self
    end

    # [InstancePath, Point3d] si le texte est attaché par un chemin valide, sinon nil.
    # L'attache est celle du modèle (esq_core::attach) : le texte suit ce à
    # quoi il est attaché.
    def attached_to
      path = Esq.attach_path(Esq.call('annot.attached', 'h' => @handle, 'which' => 0), ctx)
      path&.valid? ? [path, point] : nil
    end

    # un tableau [chemin, point] ; le chemin est un InstancePath ou un tableau
    # qui en décrit un.
    def attached_to=(arg)
      raise TypeError, "wrong argument type #{arg.class} (expected Array)" unless arg.is_a?(Array)
      raise ArgumentError, 'Incorrect number of elements.' unless arg.size == 2
      path = Esq.instance_path_arg(arg[0])
      pt = Esq.sketchup_point(arg[1])
      Esq.call('annot.set', 'h' => @handle, 'point' => pt)
      Esq.call('annot.attach', 'h' => @handle, 'which' => 0, 'handles' => Esq.path_handles(path), 'pt' => pt,
                               'mode' => 'along')
      arg
    end
  end

  # ---------------------------------------------------------------- Dimension
  class Dimension < Drawingelement
    ARROW_NONE = 0
    ARROW_SLASH = 1
    ARROW_DOT = 2
    ARROW_CLOSED = 3
    ARROW_OPEN = 4

    def initialize(handle); @handle = handle; end
    def prop(k, default); v = (info['props'] || {})[k]; v.nil? ? default : v; end
    private :prop

    def text
      t = info['text']
      t.to_s.empty? ? info['measure_text'] : t
    end

    def text=(s)
      Esq.call('annot.set', 'h' => @handle, 'text' => s.to_s)
      s
    end

    def arrow_type; prop('arrow_type', ARROW_CLOSED).to_i; end

    def arrow_type=(t)
      t = Integer(t)
      raise RangeError, 'Value out of range' unless (0..4).cover?(t)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'arrow_type' => t.to_f })
      t
    end

    def has_aligned_text?; prop('aligned', false); end

    def has_aligned_text=(v)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'aligned' => v ? true : false })
      v
    end

    def plane
      pts = __points
      a, b = pts[0], pts[1]
      off = info['offset']
      n = a.vector_to(b).cross(Geom::Vector3d.new(off))
      n = Geom::Vector3d.new(0, 0, 1) if n.length < 1e-9
      n = n.normalize
      [n.x, n.y, n.z, -(n.x * a.x + n.y * a.y + n.z * a.z)]
    end

    def __points
      i = info
      [Geom::Point3d.new(i['start']), Geom::Point3d.new(i['end'])]
    end
  end

  class DimensionLinear < Dimension
    TEXT_OUTSIDE_START = 0
    TEXT_CENTERED = 1
    TEXT_OUTSIDE_END = 2
    ALIGNED_TEXT_ABOVE = 0
    ALIGNED_TEXT_CENTER = 1
    ALIGNED_TEXT_OUTSIDE = 2

    # [entité attachée ou nil, point] : la feuille du chemin d'attache.
    def start; __end('start'); end
    def end; __end('end'); end

    # Un bout lu comme add_dimension_linear le lit sans chemin.
    def start=(*v); __set_end(v, 'start'); end
    def end=(*v); __set_end(v, 'end'); end

    # [InstancePath, point] si le bout est attaché par un chemin valide, sinon nil.
    def start_attached_to; __attached('start'); end
    def end_attached_to; __attached('end'); end

    # un tableau [chemin, point] résolu comme dans add_dimension_linear.
    def start_attached_to=(arg); __attach(arg, 'start'); end
    def end_attached_to=(arg); __attach(arg, 'end'); end

    # Le chemin de l'attache d'un bout, gardée par le modèle (esq_core::attach).
    def __path(which)
      Esq.attach_path(Esq.call('annot.attached', 'h' => @handle, 'which' => which == 'start' ? 0 : 1), ctx)
    end

    def __end(which)
      [__path(which)&.leaf, Geom::Point3d.new(info[which])]
    end

    def __attached(which)
      path = __path(which)
      path&.valid? ? [path, Geom::Point3d.new(info[which])] : nil
    end

    def __set_end(args, which)
      r = Esq.dim_end(args, 0) or raise ArgumentError, 'Invalid arguments'
      __store(which, r[0] && r[0].handle, r[1], 'along')
      args.size == 1 ? args[0] : args
    end

    def __attach(arg, which)
      raise TypeError, "wrong argument type #{arg.class} (expected Array)" unless arg.is_a?(Array)
      raise ArgumentError, 'Incorrect number of elements.' unless arg.size == 2
      path = Esq.instance_path_arg(arg[0])
      handles, pt = Esq.dim_connection(path, Esq.sketchup_point(arg[1]))
      __store(which, handles, pt, 'dim')
      arg
    end

    def __store(which, handles, pt, mode)
      other = info[which == 'start' ? 'end' : 'start']
      raise ArgumentError, 'Start and end cannot be the same point' if Geom::Point3d.new(pt) == Geom::Point3d.new(other)
      Esq.call('annot.set', 'h' => @handle, which => pt)
      Esq.attach_end(@handle, which == 'start' ? 0 : 1, handles, pt, mode)
    end
    private :__path, :__end, :__attached, :__set_end, :__attach, :__store

    def offset_vector; Geom::Vector3d.new(info['offset']); end

    def offset_vector=(v)
      a = Esq.xyz(v, 'Geom::Vector3d')
      raise ArgumentError, 'Invalid argument' if a.all?(&:zero?)
      Esq.call('annot.set', 'h' => @handle, 'offset' => a)
      v
    end

    def text_position; prop('text_position', TEXT_CENTERED).to_i; end

    def text_position=(v)
      v = Integer(v)
      raise RangeError, 'Value out of range' unless (0..2).cover?(v)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'text_position' => v.to_f })
      v
    end

    def aligned_text_position; prop('aligned_text_position', ALIGNED_TEXT_CENTER).to_i; end

    def aligned_text_position=(v)
      v = Integer(v)
      raise RangeError, 'Value out of range' unless (0..2).cover?(v)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'aligned_text_position' => v.to_f })
      v
    end
  end

  class DimensionRadial < Dimension
    def arc_curve
      h = prop('radial', nil)
      h && Entity.wrap(h)
    end

    def arc_curve=(c)
      raise ArgumentError, 'Invalid argument' unless c.is_a?(ArcCurve)
      i = info
      a = c.center
      u = Geom::Vector3d.new(i['end'].zip(i['start']).map { |p, q| p - q }).normalize
      b = a.offset(u, c.radius)
      Esq.call('annot.set', 'h' => @handle, 'start' => a.to_a, 'end' => b.to_a)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'radial' => c.handle })
      c
    end

    def leader_points
      i = info
      c = Geom::Point3d.new(i['start'])
      p = Geom::Point3d.new(i['end'])
      u = c.vector_to(p).normalize
      r = c.distance(p)
      [p, c.offset(u, r * 1.25), c.offset(u, r * 1.5)]
    end

    def leader_break_point; leader_points[1]; end

    def leader_break_point=(pt)
      i = info
      c = Geom::Point3d.new(i['start'])
      u = c.vector_to(Geom::Point3d.new(Esq.xyz(pt))).normalize
      arc = arc_curve
      r = arc ? arc.radius : c.distance(Geom::Point3d.new(i['end']))
      Esq.call('annot.set', 'h' => @handle, 'end' => c.offset(u, r).to_a)
      pt
    end
  end

  # ---------------------------------------------------------------- SectionPlane
  class SectionPlane < Drawingelement
    def initialize(handle); @handle = handle; end

    def get_plane
      i = info
      n = i['normal']
      p = i['pos']
      [n[0], n[1], n[2], 0.0 - (n[0] * p[0] + n[1] * p[1] + n[2] * p[2])]
    end

    def set_plane(*args)
      pos, normal = Esq.plane_args(args)
      Esq.call('section.set', 'h' => @handle, 'pos' => pos, 'normal' => normal)
      self
    end

    def name; info['name']; end

    def name=(s)
      raise TypeError, 'no implicit conversion into String' unless s.is_a?(String)
      Esq.call('section.set', 'h' => @handle, 'name' => s)
      s
    end

    def symbol; info['symbol']; end

    def symbol=(s)
      raise TypeError, 'no implicit conversion into String' unless s.is_a?(String)
      raise ArgumentError, 'invalid section plane symbol' if s.length > 3
      Esq.call('section.set', 'h' => @handle, 'symbol' => s)
      nil
    end

    def activate
      Esq.call('section.activate', 'h' => @handle)
      self
    end

    def active?; info['active']; end

    def hatch_pattern
      n = (info['props'] || {})['hatch']
      n && Sketchup.active_model.respond_to?(:hatch_patterns) ? Sketchup.active_model.hatch_patterns[n] : nil
    end

    def hatch_pattern=(h)
      Esq.call('ent.set', 'h' => @handle, 'props' => { 'hatch' => h.respond_to?(:name) ? h.name : h })
      h
    end
  end

  module ::Esq
    class << self
      # A plane given as [a, b, c, d], [point, normal], or two arguments (point, normal).
      def plane_args(args)
        args = args.first if args.size == 1 && args.first.is_a?(Array)
        if args.size == 4 && args.all? { |v| v.is_a?(Numeric) }
          a, b, c, d = args.map(&:to_f)
          len = Math.sqrt(a * a + b * b + c * c)
          raise ArgumentError, 'Invalid plane' if len.zero?
          n = [a / len, b / len, c / len]
          dist = -d / len
          [n.map { |v| v * dist }, n]
        elsif args.size == 2
          [xyz(args[0]), xyz(args[1], 'Geom::Vector3d')]
        else
          raise ArgumentError, 'Incorrect number of elements.'
        end
      end
    end
  end

  # ---------------------------------------------------------------- Image
  # An instance of an image definition (esq-core/src/image.rs): a rectangle painted with the
  # picture, one inch per pixel, placed and sized by the instance's transformation.
  class Image < Drawingelement
    def initialize(handle); @handle = handle; end

    def img; Esq.call('image.get', 'h' => @handle); end
    private :img

    def check!; img; end
    private :check!

    def path; img['path']; end
    def pixelwidth; img['px'][0]; end
    def pixelheight; img['px'][1]; end
    def width; img['width']; end
    def height; img['height']; end
    def origin; Geom::Point3d.new(img['origin']); end
    def normal; Geom::Vector3d.new(img['normal']); end
    def zrotation; img['zrotation']; end
    def transformation; Geom::Transformation.new(img['xform']); end

    # Resizing keeps the other side (not the proportions), as SketchUp does.
    def width=(w)
      Esq.call('image.size', 'h' => @handle, 'width' => Esq.num2dbl(w), 'height' => height)
      w
    end

    def height=(h)
      Esq.call('image.size', 'h' => @handle, 'width' => width, 'height' => Esq.num2dbl(h))
      h
    end

    # A width, or [width] / [width, height]; a null side follows the picture's proportions.
    def size=(v)
      if v.is_a?(Array)
        raise ArgumentError, 'Too few arguments - at least 1 required' if v.empty?
        w = Esq.num2dbl(v[0])
        h = v.size > 1 ? Esq.num2dbl(v[1]) : 0.0
      else
        w = Esq.num2dbl(v)
        h = 0.0
      end
      raise ArgumentError, "Image size cannot be 0\n" if w.abs <= Geom.tolerance && h.abs <= Geom.tolerance
      i = img
      pw, ph = i['px']
      w = h * pw / ph.to_f if w.abs <= Geom.tolerance
      h = w * ph / pw.to_f if h.abs <= Geom.tolerance
      Esq.call('image.size', 'h' => @handle, 'width' => w, 'height' => h)
      nil
    end

    def origin=(p)
      Esq.call('image.origin', 'h' => @handle, 'p' => Esq.xyz(p))
      p
    end

    def transformation=(t)
      m = Esq.matrix_arg(t)
      raise ArgumentError, 'Unable to set transformation: transformation matrix is not invertible' if Esq.singular_matrix?(m)
      Esq.call('image.xform', 'h' => @handle, 'm' => m)
      self
    end

    def transform!(t)
      m = Esq.matrix_arg(t)
      raise ArgumentError, 'Unable to apply transformation: transformation matrix is not invertible' if Esq.singular_matrix?(m)
      Esq.call('image.xform', 'h' => @handle, 'm' => (Geom::Transformation.new(m) * transformation).to_a)
      self
    end

    # Explodes into the textured face (and its edges).
    def explode
      check!
      hs = Esq.call('inst.explode', 'h' => @handle)
      hs ? hs.map { |h| Entity.wrap(h) } : false
    end

    def glued_to
      h = Esq.call('inst.info', 'h' => @handle)['glued']
      h && Entity.wrap(h)
    end

    def glued_to=(target)
      check!
      Esq.icall('inst.glue', { 'h' => @handle, 'to' => Esq.glue_target(target) }, 'Image')
      nil
    end

    def image_rep
      data = Esq.call('image.rep', 'h' => @handle)
      raise ArgumentError, 'Image lacks texture data' unless data
      ImageRep.__from(Esq.call('img.decode', 'data' => data))
    end
  end

  module ::Esq
    class << self
      # Ruby's rb_num2dbl: numbers only.
      def num2dbl(v)
        return v.to_f if v.is_a?(Numeric)
        raise TypeError, 'no implicit conversion to float from nil' if v.nil?
        raise TypeError, 'no implicit conversion to float from string' if v.is_a?(String)
        raise TypeError, "can't convert #{v.class} into Float"
      end

      # 16 numbers from a Geom::Transformation or an Array of 16.
      def matrix_arg(t)
        a = t.respond_to?(:to_a) ? t.to_a : nil
        raise TypeError, 'wrong argument type (expected Geom::Transformation)' unless a && a.size == 16 && a.all? { |v| v.is_a?(Numeric) }
        a.map(&:to_f)
      end

      def singular_matrix?(m)
        det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[4] * (m[1] * m[10] - m[2] * m[9]) + m[8] * (m[1] * m[6] - m[2] * m[5])
        det.abs < 1e-12
      end
    end
  end

  # ---------------------------------------------------------------- Axes
  class Axes < Entity
    def initialize(*); @handle = 'x:0:0'; end

    # The drawing axes a scene saved (Page#axes): read from and written to the scene.
    def self.__for_page(uid)
      a = new
      a.instance_variable_set(:@page, uid)
      a.instance_variable_set(:@handle, "x:0:p#{uid}")
      a
    end

    def __scene
      return nil unless @page
      Esq.call('scene.list').find { |s| s['uid'] == @page } or raise TypeError, 'reference to deleted Axes'
    end
    private :__scene

    def ==(o); o.is_a?(Axes) && o.instance_variable_get(:@page) == @page; end
    def valid?; @page.nil? || Esq.call('scene.list').any? { |s| s['uid'] == @page }; end
    def deleted?; !valid?; end
    def entityID; 0; end
    def persistent_id; 0; end
    def typename; 'Axes'; end
    def parent; @page ? model.pages.find { |p| p.uid == @page } : model; end

    def transformation
      s = __scene
      Geom::Transformation.new((s && s['props']['axes']) || Esq.call('axes.get'))
    end
    def origin; transformation.origin; end
    def xaxis; transformation.xaxis; end
    def yaxis; transformation.yaxis; end
    def zaxis; transformation.zaxis; end
    def axes; t = transformation; [t.xaxis, t.yaxis, t.zaxis]; end
    def to_a; t = transformation; [t.origin, t.xaxis, t.yaxis, t.zaxis]; end
    def sketch_plane; o = origin; n = zaxis; [n.x, n.y, n.z, 0.0 - (n.x * o.x + n.y * o.y + n.z * o.z)]; end

    def set(origin, x, y, z)
      o = Esq.xyz(origin)
      v = [x, y, z].map { |a| Geom::Vector3d.new(Esq.xyz(a, 'Geom::Vector3d')) }
      tol = 1e-6
      if [[0, 1], [0, 2], [1, 2]].any? { |i, j| v[i].normalize.dot(v[j].normalize).abs > tol }
        raise ArgumentError, 'Axes must be orthogonal'
      end
      u = v.map(&:normalize)
      m = [u[0].x, u[0].y, u[0].z, 0, u[1].x, u[1].y, u[1].z, 0, u[2].x, u[2].y, u[2].z, 0, o[0], o[1], o[2], 1]
      if @page
        __scene
        # A scene's axes change outside any undo operation (no transaction is heard).
        Esq::Events.unrecorded { Esq.call('scene.set', 'uid' => @page, 'props' => { 'axes' => m }) }
      else
        Esq.call('axes.set', 'm' => m)
      end
      self
    end
  end

  class Model
    def axes; @axes ||= Axes.new; end unless method_defined?(:axes)
  end

  # ---------------------------------------------------------------- EntitiesBuilder
  # SketchUp: made only by Entities#build (`new` is undefined).
  class EntitiesBuilder
    class << self
      undef_method :new
      def __make(entities); allocate.tap { |b| b.send(:initialize, entities) }; end
    end

    def initialize(entities); @entities = entities; @valid = true; end
    def __invalidate; @valid = false; end

    def check!
      raise RuntimeError, 'Invalid Sketchup::EntitiesBuilder object.' unless @valid
    end
    private :check!

    def valid?; @valid; end
    def entities; check!; @entities; end

    def clone; raise TypeError, 'Cannot duplicate or clone Sketchup::EntitiesBuilder.'; end
    alias dup clone

    # the points, or one array of them, and `holes:` (an array of such arrays);
    # the face keeps the orientation of its points.
    def add_face(*args, holes: nil)
      check!
      outer = __loop(args.size == 1 ? args[0] : args)
      unless holes.nil?
        raise TypeError, "wrong argument type #{holes.class} (expected Array)" unless holes.is_a?(Array)
        holes = holes.map { |h| __loop(h) }
      end
      Face.new(Esq.call('builder.face', 'ctx' => @entities.ctx, 'pts' => outer, 'holes' => holes || []))
    end

    # two points, or one array of two; ends that meet raise ArgumentError.
    def add_edge(*args)
      check!
      args = args[0] if __list?(args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 2)" unless args.size == 2
      pts = args.map { |p| Esq.xyz(p) }
      e = Esq.call('builder.edges', 'ctx' => @entities.ctx, 'pts' => pts).first
      raise ArgumentError, 'Duplicate points' unless e
      Edge.new(e)
    end
    alias add_line add_edge

    # an edge between each point and the next, nil where they meet.
    def add_edges(*args)
      check!
      args = args[0] if __list?(args)
      raise ArgumentError, format('At least 2 points required, got %d', args.size) if args.size < 2
      pts = args.map { |p| Esq.xyz(p) }
      Esq.call('builder.edges', 'ctx' => @entities.ctx, 'pts' => pts).map { |h| h && Edge.new(h) }
    end

    # the vertex at the point, or nil (none is made).
    def vertex_at(point)
      check!
      h = Esq.call('builder.find', 'ctx' => @entities.ctx, 'p' => Esq.xyz(point))
      h && Vertex.new(h)
    end

    # an array of points (TypeError for anything else).
    def __loop(pts)
      raise TypeError, "wrong argument type #{pts.class} (expected Array)" unless pts.is_a?(Array)
      pts.map { |p| Esq.xyz(p) }
    end

    # One argument that is an array of points rather than a point.
    def __list?(args)
      args.size == 1 && args[0].is_a?(Array) && !Esq::G.point_array?(args[0])
    end
    private :__loop, :__list?
  end

  # ---------------------------------------------------------------- Entities
  class Entities
    include Enumerable
    attr_reader :ctx

    def initialize(ctx = '0'); @ctx = ctx; end

    def ==(o); o.is_a?(Entities) && o.ctx == @ctx; end
    alias eql? ==
    def hash; @ctx.hash; end
    def inspect; format('#<Sketchup::Entities:0x%016x>', object_id << 3); end
    alias to_s inspect

    def check_frozen!
      raise FrozenError, "can't modify frozen Sketchup::Entities: #{inspect}" if frozen?
    end
    private :check_frozen!

    def list; Esq.call('ent.list', 'ctx' => @ctx); end
    private :list

    # SketchUp walks the collection with an iterator that steps over what is erased meanwhile:
    # once the block may have changed the model, what is left to yield is
    # looked at again.
    def each
      return to_enum(:each) unless block_given?
      w = Esq.writes
      list.each do |h, t|
        next if Esq.writes != w && !Esq.call('ent.valid', 'h' => h)
        yield Entity.wrap(h, t)
      end
      self
    end

    def length; list.size; end
    alias size length

    def [](i)
      return nil unless i.is_a?(Integer)
      l = list
      return nil if i >= l.size || i < -l.size
      h, t = l[i]
      Entity.wrap(h, t)
    end
    alias at []

    def model; Sketchup.active_model; end
    def parent; @ctx == '0' ? model : ComponentDefinition.from_ctx(@ctx); end

    def add_observer(observer)
      list = Esq.observers["entities:#{@ctx}"]
      list << observer unless list.include?(observer)
      true
    end

    def remove_observer(observer)
      !Esq.observers["entities:#{@ctx}"].delete(observer).nil?
    end

    # ---- edges, curves, faces
    def points_of(pts)
      pts = pts.first if pts.size == 1 && pts.first.is_a?(Array) && !pts.first.first.is_a?(Numeric)
      pts.map { |p| p.respond_to?(:position) ? Esq.xyz(p.position) : Esq.xyz(p) }
    end
    private :points_of

    def add_edges(*pts)
      check_frozen!
      p = points_of(pts)
      raise ArgumentError, 'Not enough points - at least 2 required' if p.size < 2
      r = Esq.call('ents.add_edges', 'ctx' => @ctx, 'pts' => p)
      r.empty? ? nil : r.map { |h| Edge.new(h) }
    end

    def add_line(*pts)
      es = add_edges(*pts)
      es && es.first
    end

    def add_curve(*pts)
      check_frozen!
      p = points_of(pts)
      raise ArgumentError, 'Not enough points - at least 2 required' if p.size < 2
      r = Esq.call('ent.add_curve', 'ctx' => @ctx, 'pts' => p, 'kind' => 'curve')
      r.empty? ? nil : r.map { |h| Edge.new(h) }
    end

    def circle_points(center, normal, radius, n, start_angle = 0.0, xaxis = nil)
      nrm = Geom::Vector3d.new(Esq.xyz(normal, 'Geom::Vector3d')).normalize
      x = xaxis ? Geom::Vector3d.new(xaxis).normalize : nrm.axes[0]
      y = nrm.cross(x)
      (0...n).map do |i|
        a = start_angle + 2 * Math::PI * i / n
        Geom::Point3d.new(Esq.xyz(center)).offset(Esq::G.combine(x, radius * Math.cos(a), y, radius * Math.sin(a)))
      end
    end
    private :circle_points

    def add_circle(center, normal, radius, numsegs = 24)
      check_frozen!
      numsegs = Integer(numsegs)
      nrm = Geom::Vector3d.new(Esq.xyz(normal, 'Geom::Vector3d')).normalize
      x = nrm.axes[0]
      pts = circle_points(center, normal, radius, numsegs, 0.0, x)
      Esq.call('ent.add_curve', 'ctx' => @ctx, 'pts' => pts.map(&:to_a), 'closed' => true, 'kind' => 'arc',
               'xaxis' => x.to_a, 'start' => 0.0, 'end' => 2 * Math::PI).map { |h| Edge.new(h) }
    end

    def add_ngon(center, normal, radius, numsides)
      check_frozen!
      pts = circle_points(center, normal, radius, Integer(numsides))
      Esq.call('ent.add_curve', 'ctx' => @ctx, 'pts' => pts.map(&:to_a), 'closed' => true, 'kind' => 'polygon').map { |h| Edge.new(h) }
    end

    def add_arc(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 6 required' if args.size < 6
      center, xaxis, normal, radius, start_angle, end_angle, numsegs = args
      numsegs = 12 if numsegs.nil?
      c =Geom::Point3d.new(Esq.xyz(center))
      n = Geom::Vector3d.new(Esq.xyz(normal, 'Geom::Vector3d')).normalize
      x = Geom::Vector3d.new(Esq.xyz(xaxis, 'Geom::Vector3d')).normalize
      y = n.cross(x)
      numsegs = [Integer(numsegs), 1].max
      pts = (0..numsegs).map do |i|
        a = start_angle + (end_angle - start_angle) * i / numsegs.to_f
        c.offset(Esq::G.combine(x, radius * Math.cos(a), y, radius * Math.sin(a)))
      end
      # an arc from 0 to within 0.001 of 2π is kept as a circle (its end angle
      # becomes its start, ArcCurve#circular?).
      kept_end = start_angle.to_f.abs < 0.001 && (end_angle.to_f - 2 * Math::PI).abs < 0.001 ? start_angle.to_f : end_angle.to_f
      Esq.call('ent.add_curve', 'ctx' => @ctx, 'pts' => pts.map(&:to_a), 'kind' => 'arc', 'xaxis' => x.to_a,
               'start' => start_angle.to_f, 'end' => kept_end).map { |h| Edge.new(h) }
    end

    def add_face(*args)
      check_frozen!
      args = args.first if args.size == 1 && args.first.is_a?(Array) && !args.first.first.is_a?(Numeric)
      if args.size == 1 && args.first.is_a?(Curve)
        args = args.first.edges
      end
      raise ArgumentError, 'Not enough edges - at least 3 required' if args.size < 3 && !args.first.is_a?(Curve)
      if args.first.is_a?(Edge)
        pts = edge_loop_points(args)
        raise ArgumentError, 'Not enough edges - at least 3 required' if pts.size < 3
        uniq = pts.map { |p| p.map { |v| v.round(6) } }
        raise ArgumentError, 'Duplicate points in array' if uniq.uniq.size != uniq.size
      else
        # Des points, comme SketchUp : un dernier point confondu avec le premier
        # ferme la boucle et tombe (l'extension Shapes dessine sa boîte ainsi) ; ensuite deux
        # points qui se suivent ne peuvent pas être confondus.
        pts = args.map { |p| p.respond_to?(:position) ? Esq.xyz(p.position) : Esq.xyz(p) }
        pts.pop if pts.size > 2 && Esq::G.near?(pts.first, pts.last)
        raise ArgumentError, 'Not enough points - at least 3 required' if pts.size < 3
        if pts.each_index.any? { |i| Esq::G.near?(pts[i], pts[(i + 1) % pts.size]) }
          raise ArgumentError, 'Duplicate points in array'
        end
      end
      n = Geom::Vector3d.new(0, 0, 0)
      p0 = Geom::Point3d.new(pts[0])
      (1...pts.size - 1).each { |i| n += p0.vector_to(Geom::Point3d.new(pts[i])).cross(p0.vector_to(Geom::Point3d.new(pts[i + 1]))) }
      raise ArgumentError, 'Points are not planar' if n.length < 1e-9
      nn = n.normalize
      unless pts.all? { |q| (p0.vector_to(Geom::Point3d.new(q)).dot(nn)).abs < Geom.tolerance }
        raise ArgumentError, 'Points are not planar'
      end
      Face.new(Esq.call('ent.add_face', 'ctx' => @ctx, 'pts' => pts))
    end

    def edge_loop_points(edges)
      chain = edges.map { |e| [e.start.position.to_a, e.end.position.to_a] }
      pts = chain.shift.dup
      until chain.empty?
        i = chain.index { |a, b| a == pts.last || b == pts.last }
        break unless i
        a, b = chain.delete_at(i)
        pts << (a == pts.last ? b : a)
      end
      pts.pop if pts.size > 1 && pts.first == pts.last
      pts
    end
    private :edge_loop_points

    def mesh_args(mesh, flags, mat, back)
      pts = mesh.points.map { |p| Esq.xyz(p) }
      polys = mesh.polygons
      { 'ctx' => @ctx, 'pts' => pts, 'polys' => polys, 'flags' => flags,
        'front' => Esq.material_name(mat), 'back' => Esq.material_name(back) }.compact
    end
    private :mesh_args

    # neither material may be an image's or a tag's.
    def add_faces_from_mesh(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - 1 to 4 required' if args.empty?
      mesh, flags, mat, back = args
      [mat, back].each { |m| Esq.assignable_material!(m) }
      Esq.call('ent.add_mesh', mesh_args(mesh, flags || 12, mat, back))
    end

    # only into an empty collection (nil otherwise, before anything else is
    # read); neither material may be an image's or a tag's.
    def fill_from_mesh(*args)
      check_frozen!
      return nil unless length.zero?
      raise ArgumentError, 'Too few arguments - 1 to 5 required' if args.empty?
      mesh, _weld, flags, mat, back = args
      [mat, back].each { |m| Esq.assignable_material!(m) }
      Esq.call('ent.add_mesh', mesh_args(mesh, flags || 12, mat, back))
      true
    end

    # ---- construction geometry
    def add_cpoint(point)
      check_frozen!
      ConstructionPoint.new(Esq.call('guide.add', 'ctx' => @ctx, 'mode' => 'point', 'pos' => Esq.xyz(point)))
    end

    def add_cline(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - 2 required' if args.size < 2
      start, second, stipple = args
      a = { 'ctx' => @ctx, 'pos' => Esq.xyz(start) }
      if second.is_a?(Geom::Vector3d)
        a['mode'] = 'line'
        a['dir'] = Esq.xyz(second, 'Geom::Vector3d')
      else
        a['mode'] = 'segment'
        a['end'] = Esq.xyz(second)
      end
      a['stipple'] = stipple.to_s if stipple
      ConstructionLine.new(Esq.call('guide.add', a))
    end

    # ---- text and dimensions
    # la position est un point (Point3d, tableau de nombres, Vertex,
    # InputPoint) ou un tableau [chemin, point] qui attache le texte à la feuille du chemin.
    def add_text(*args)
      check_frozen!
      raise ArgumentError, 'Wrong number of arguments: at least 2 required' if args.size < 2
      text = Esq.str_arg(args[0])
      where, vector = args[1], args[2]
      props = {}
      if Esq.point_value?(where) || where.is_a?(Vertex) || where.is_a?(InputPoint)
        pt = Esq.sketchup_point(where)
        v = vector ? Esq.xyz(vector, 'Geom::Vector3d') : [0.0, 0.0, 0.0]
        props['leader_type'] = vector ? 2.0 : 0.0
      else
        raise ArgumentError, 'Invalid arguments.' unless where.is_a?(Array)
        raise TypeError, 'Invalid elements in the array' unless where.size == 2 && Esq.point_value?(where[1])
        path = Esq.instance_path_arg(where[0])
        unless path.leaf
          raise TypeError, 'Invalid element. Leaf has to be either Sketchup::Vertex, Sketchup::Edge, Sketchup::ConstructionLine, or Sketchup::ConstructionPoint'
        end
        pt = Esq.sketchup_point(where[1])
        v = args.size > 2 ? Esq.xyz(vector, 'Geom::Vector3d') : [0.0, 0.0, 0.0]
        props['leader_type'] = 2.0
      end
      h = Esq.call('annot.add', 'ctx' => @ctx, 'kind' => 'text', 'text' => text, 'point' => pt, 'leader' => v, 'props' => props)
      Esq.attach_end(h, 0, Esq.path_handles(path), pt, 'along') if path
      Text.new(h)
    end

    # d'abord les formes sans chemin, sinon deux tableaux
    # [chemin, point] dont les points, en coordonnées du modèle, sont résolus sur la feuille.
    def add_dimension_linear(*args)
      check_frozen!
      raise ArgumentError, 'Wrong number of arguments: at least 3 required' if args.size < 3
      s = Esq.dim_end(args, 0)
      e = s && Esq.dim_end(args, s[2])
      off = e && Esq.dim_offset(args[e[2]])
      if off
        sh, sp = s[0] && s[0].handle, s[1]
        eh, ep = e[0] && e[0].handle, e[1]
      else
        s, e, off = args
        raise ArgumentError, 'Invalid arguments' unless s.is_a?(Array) && e.is_a?(Array)
        raise ArgumentError, 'Invalid elements in the array.' unless s.size == 2 && e.size == 2
        unless [s, e].all? { |a| a[0].is_a?(InstancePath) || a[0].is_a?(Array) }
          raise TypeError, "First element in the array is invalid. Must be Sketchup::InstancePath or an array containing Sketchup::ComponentInstance's and a valid leaf entity."
        end
        unless [s, e].all? { |a| Esq.point_value?(a[1]) }
          raise TypeError, 'Second element in the array is invalid. Must be a Point3d.'
        end
        spath, epath = Esq.instance_path_arg(s[0]), Esq.instance_path_arg(e[0])
        sp, ep = Esq.sketchup_point(s[1]), Esq.sketchup_point(e[1])
        off = Esq.xyz(off, 'Geom::Vector3d')
        raise ArgumentError, 'Invalid instance path.' unless spath.valid? && epath.valid?
      end
      raise ArgumentError, 'The start point and end point cannot be the same.' if Geom::Point3d.new(sp) == Geom::Point3d.new(ep)
      if spath
        raise TypeError, 'Invalid instance path leaf entity.' unless spath.leaf && epath.leaf
        sh, sp = Esq.dim_connection(spath, sp)
        eh, ep = Esq.dim_connection(epath, ep)
      end
      h = Esq.call('annot.add', 'ctx' => @ctx, 'kind' => 'linear', 'start' => sp, 'end' => ep, 'offset' => off,
                                'props' => {})
      Esq.attach_end(h, 0, sh, sp, spath ? 'dim' : 'along')
      Esq.attach_end(h, 1, eh, ep, spath ? 'dim' : 'along')
      DimensionLinear.new(h)
    end

    def add_dimension_radial(curve, leader)
      check_frozen!
      raise ArgumentError, 'Invalid argument' unless curve.is_a?(ArcCurve)
      c = curve.center
      u = Geom::Vector3d.new(Esq.xyz(leader, 'Geom::Vector3d')).normalize
      raise ArgumentError, 'Invalid argument' if u.length.zero?
      b = c.offset(u, curve.radius)
      h = Esq.call('annot.add', 'ctx' => @ctx, 'kind' => 'radial', 'start' => c.to_a, 'end' => b.to_a,
                                'props' => { 'radial' => curve.handle, 'arrow_type' => 3.0 })
      # Attachée à l'arc (son centre, et son point dans la direction du repère) : elle le suit.
      if curve.edges.first&.ctx == @ctx
        Esq.attach_end(h, 0, curve.handle, c.to_a)
        Esq.attach_end(h, 1, curve.handle, b.to_a, 'arc')
      end
      DimensionRadial.new(h)
    end

    def add_3d_text(*args)
      check_frozen!
      raise ArgumentError, 'Need at 3 arguments' if args.size < 3
      string, align, font, bold, italic, height, tolerance, z, filled, extrusion = args
      r = Esq.call('host.entities.text3d', 'ctx' => @ctx, 'text' => string.to_s, 'align' => Integer(align),
                   'font' => font.to_s, 'bold' => bold ? true : false, 'italic' => italic ? true : false,
                   'height' => height.nil? ? 1.0 : height.to_f, 'tolerance' => tolerance.to_f, 'z' => z.to_f,
                   'filled' => filled.nil? ? true : (filled ? true : false), 'extrusion' => extrusion.to_f)
      r ? true : false
    end

    # ---- section planes
    def add_section_plane(*args)
      check_frozen!
      pos, normal = Esq.plane_args(args)
      SectionPlane.new(Esq.call('section.add', 'ctx' => @ctx, 'pos' => pos, 'normal' => normal))
    end

    def active_section_plane
      h = Esq.call('ents.active_section', 'ctx' => @ctx)
      h && SectionPlane.new(h)
    end

    def active_section_plane=(plane)
      check_frozen!
      if plane.nil?
        Esq.call('ents.active_section', 'ctx' => @ctx, 'set' => true)
      else
        Esq.call('ents.active_section', 'ctx' => @ctx, 'h' => plane.handle)
      end
      plane
    end

    # ---- groups, components, images
    def add_group(*ents)
      check_frozen!
      ents = ents.flatten
      return Group.new(Esq.call('ents.add_group', 'ctx' => @ctx)) if ents.empty?
      Group.new(Esq.call('ents.make_group', 'ctx' => @ctx, 'hs' => ents.map(&:handle)))
    end

    def add_instance(defn, xform = nil)
      check_frozen!
      raise ArgumentError, 'Invalid component definition' unless defn.is_a?(ComponentDefinition)
      # Une transformation, ou ce dont Geom::Transformation.new en fait une (un point : une
      # translation, comme `add_instance(definition, ORIGIN)`).
      xform = Geom::Transformation.new(xform) unless xform.nil? || xform.is_a?(Geom::Transformation)
      m = xform.respond_to?(:to_a) ? xform.to_a : nil
      if m && m.size == 16 && (m[0] * (m[5] * m[10] - m[6] * m[9]) - m[4] * (m[1] * m[10] - m[2] * m[9]) + m[8] * (m[1] * m[6] - m[2] * m[5])).abs < 1e-12
        raise ArgumentError, 'Unable to add instance: transformation matrix is not invertible'
      end
      if @ctx != '0' && (defn.ctx == @ctx || uses_context?(defn.entities, @ctx))
        raise ArgumentError, 'Insertion would result in recursive definition.'
      end
      ComponentInstance.new(Esq.call('ents.add_instance', 'ctx' => @ctx, 'def' => "d:0:#{defn.ctx}", 'xform' => m))
    end

    def uses_context?(ents, ctx)
      ents.any? do |e|
        next false unless e.is_a?(Group) || e.is_a?(ComponentInstance)
        d = e.definition
        d.ctx == ctx || uses_context?(d.entities, ctx)
      end
    end
    private :uses_context?

    def add_image(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 3 required' if args.size < 3
      file, point, width, height = args
      file = file.to_str
      return nil if file.empty?
      p = Esq.xyz(point)
      w = Esq.num2dbl(width)
      h = height.nil? ? 0.0 : Esq.num2dbl(height)
      raise ArgumentError, "Image size cannot be 0\n" if w.abs <= Geom.tolerance && h.abs <= Geom.tolerance
      hnd = Esq.call('image.add', 'ctx' => @ctx, 'path' => file, 'origin' => p, 'width' => w, 'height' => h)
      unless hnd
        warn "Could not create Image for #{file}\n"
        return nil
      end
      Image.new(hnd)
    end

    # ---- snaps and spaces
    # A snap (position, direction, optional up) is an entity of this collection, stored in the
    # model (`Entities.snaps`) like a guide: `each` lists it, the Move tool uses it. Without an
    # up, the blue axis (green for a snap facing up or down), made perpendicular to the direction.
    def add_snap(*args)
      check_frozen!
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 2..3)" unless (2..3).cover?(args.size)
      pos = Esq.xyz(args[0])
      dir = Esq.xyz(args[1], 'Geom::Vector3d')
      up = args.size == 3 ? Esq.xyz(args[2], 'Geom::Vector3d') : nil
      Snap.__wrap(Esq.call('snap.add', 'ctx' => @ctx, 'pos' => pos, 'dir' => dir, 'up' => up))
    end

    # Entities#add_space is defined by the view zone (44_view_camera.rb) with the Space class.

    # ---- modification
    # SketchUp: entities, arrays of entities or a selection; an
    # instance whose definition is being edited cannot go.
    def erase_entities(*ents)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 1 required' if ents.empty?
      list = ents.flat_map { |e| e.is_a?(Array) || e.is_a?(Selection) ? e.to_a : [e] }
      list.each do |e|
        Esq.expect(e, Entity)
        raise TypeError, 'reference to deleted Entity' unless e.valid?
      end
      if list.any? { |e| (e.is_a?(Group) || e.is_a?(ComponentInstance)) && Esq.open_definition?(e.definition) }
        raise ArgumentError, 'cannot remove an instance in the active editing path'
      end
      hs = list.flat_map do |e|
        case e
        when Curve then e.edges.map(&:handle)
        when Vertex then []
        else [e.handle]
        end
      end
      Esq.call('ent.erase', 'hs' => hs)
      nil
    end

    def clear!
      check_frozen!
      hs = list.map(&:first)
      Esq.call('ent.erase', 'hs' => hs) unless hs.empty?
      self
    end

    def transform_entities(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 2 required' if args.size < 2
      t, *ents = args
      list = ents.flatten
      list.each { |e| Esq.expect(e, Entity) }
      Esq.call('ent.transform', 'ctx' => @ctx, 'hs' => list.map(&:handle), 'xform' => t.to_a)
      true
    end

    # SketchUp: without two arrays nothing happens; each entity moves by what
    # its value makes as a transformation (a vector or a point translates, a
    # Transformation is taken as is), the vertices all at once.
    def transform_by_vectors(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 2 required' if args.size < 2
      ents, vals = args
      return self unless ents.is_a?(Array) && vals.is_a?(Array)
      raise ArgumentError, 'second array must be at least as large as the first' if vals.size < ents.size
      xforms = ents.each_with_index.map do |e, i|
        Esq.expect(e, Entity)
        raise TypeError, 'reference to deleted Entity' unless e.valid?
        Esq::G.xform(vals[i]).to_a
      end
      Esq.call('ent.transform_by_vectors', 'ctx' => @ctx, 'hs' => ents.map(&:handle), 'xforms' => xforms)
      self
    end

    # SketchUp: the faces of this collection, placed by `t1` (recursing into its
    # groups and components with `recurse`), crossed with `entities2`; the edges go into `dest`
    # (an Entities, a group or component's definition, the model, or nil = this collection),
    # which `t2` places. Returns the new edges.
    def intersect_with(*args)
      check_frozen!
      raise ArgumentError, 'Too few arguments - at least 6 required' if args.size < 6
      recurse, t1, dest, t2, hidden = args
      dctx = case dest
             when Entities then dest.ctx
             when Group, ComponentInstance then dest.definition.ctx
             when Model then '0'
             else @ctx
             end
      items = args[5..].flatten.compact.select { |e| e.is_a?(Face) || e.is_a?(Group) || e.is_a?(ComponentInstance) }
      Esq.call('ent.intersect', 'ctx' => @ctx, 't1' => Esq.matrix_arg(t1), 'recurse' => recurse ? true : false,
                                'dest' => dctx, 't2' => Esq.matrix_arg(t2), 'hidden' => hidden ? true : false,
                                'hs' => items.map(&:handle))
         .map { |h| Edge.new(h) }
    end

    # One curve per connected run of the edges.
    def weld(edges)
      check_frozen!
      raise TypeError, "wrong argument type #{edges.class} (expected Array)" unless edges.is_a?(Array)
      edges.each { |e| Esq.expect(e, Edge) }
      Esq.call('ent.weld', 'ctx' => @ctx, 'hs' => edges.map(&:handle)).map { |h| Entity.wrap(h) }
    end

    def build
      check_frozen!
      raise LocalJumpError, 'no block given (yield)' unless block_given?
      raise RuntimeError, 'Cannot nest calls to entities.build' if Esq.building
      Esq.building = true
      builder = EntitiesBuilder.__make(self)
      begin
        yield builder
      ensure
        Esq.building = false
        builder.__invalidate
      end
      nil
    end

    def grep_faces; grep(Face); end
  end
end

module Esq
  class << self
    attr_accessor :building
  end
end
