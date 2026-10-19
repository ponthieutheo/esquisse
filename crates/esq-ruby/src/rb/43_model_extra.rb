# encoding: utf-8
# Matières, étiquettes, textures, images, motifs de hachure, styles de ligne, ombres, options.
# Le modèle.

FILE_WRITE_OK = 0 unless defined?(FILE_WRITE_OK)
FILE_WRITE_FAILED_INVALID_TYPE = 1 unless defined?(FILE_WRITE_FAILED_INVALID_TYPE)
FILE_WRITE_FAILED_UNKNOWN = 2 unless defined?(FILE_WRITE_FAILED_UNKNOWN)
LAYER_VISIBLE_BY_DEFAULT = 0 unless defined?(LAYER_VISIBLE_BY_DEFAULT)
LAYER_HIDDEN_BY_DEFAULT = 1 unless defined?(LAYER_HIDDEN_BY_DEFAULT)
LAYER_USES_DEFAULT_VISIBILITY_ON_NEW_PAGES = 0 unless defined?(LAYER_USES_DEFAULT_VISIBILITY_ON_NEW_PAGES)
LAYER_IS_VISIBLE_ON_NEW_PAGES = 0x10 unless defined?(LAYER_IS_VISIBLE_ON_NEW_PAGES)
LAYER_IS_HIDDEN_ON_NEW_PAGES = 0x20 unless defined?(LAYER_IS_HIDDEN_ON_NEW_PAGES)

module Sketchup
  # ------------------------------------------------------------------ images
  class ImageRep
    def initialize(*args)
      @w = 0
      @h = 0
      @bpp = 32
      @pad = 4
      @data = ''.b
      load_file(args[0]) if args[0].is_a?(String)
    end

    def width; @w; end
    def height; @h; end
    def size; [@w, @h]; end
    def bits_per_pixel; @bpp; end
    def row_padding; @pad; end
    def data; @data.dup; end

    def load_file(path)
      raise TypeError, 'wrong argument type' unless path.is_a?(String)
      __assign(Esq.call('img.load', 'path' => path))
    end

    def save_file(path)
      raise TypeError, 'wrong argument type' unless path.is_a?(String)
      Esq.call('img.save', __fields.merge('path' => path))
      self
    end

    def self.__from(r); new.__assign(r); end

    def __assign(r)
      @w, @h, @bpp, @pad = r['w'], r['h'], r['bpp'], r['pad']
      @data = r['data'].unpack1('m0')
      self
    end

    def __fields; { 'w' => @w, 'h' => @h, 'bpp' => @bpp, 'pad' => @pad, 'data' => [@data].pack('m0') }; end

    def set_data(width, height, bits_per_pixel, row_padding, pixel_data)
      raise TypeError, 'expected binary string' unless pixel_data.is_a?(String)
      unless width.to_i > 0 && height.to_i > 0 && [8, 24, 32].include?(bits_per_pixel)
        raise ArgumentError, 'invalid width, height, or bits per pixel. Width or height must be greater than 0. Bits per pixel must be 8/24/32.'
      end
      pad = row_padding.to_i
      raise ArgumentError, 'invalid pixel data' if pad < 1
      row = ((width * bits_per_pixel / 8) + pad - 1) / pad * pad
      if pixel_data.bytesize != row * height
        raise ArgumentError, 'width, height, bits per pixel and row padding must equate to the size of the binary string'
      end
      @w, @h, @bpp, @pad = width, height, bits_per_pixel, pad
      @data = pixel_data.b
      self
    end

    def colors
      px = @bpp / 8
      row = ((@w * px) + @pad - 1) / @pad * @pad
      out = []
      @h.times do |y|
        @w.times do |x|
          o = y * row + x * px
          b = @data.getbyte(o)
          if px == 1
            out << Sketchup::Color.new(b, b, b)
          else
            out << Sketchup::Color.new(@data.getbyte(o + 2), @data.getbyte(o + 1), b, px == 4 ? @data.getbyte(o + 3) : 255)
          end
        end
      end
      out
    end

    # Couleur au point (u, v) de l'image (0..1, origine en haut à gauche), interpolée.
    def color_at_uv(*args)
      u, v = args[0].respond_to?(:x) ? [args[0].x, args[0].y] : [args[0], args[1]]
      raise ArgumentError, 'invalid image' if @w.zero?
      u = u.to_f % 1.0
      v = v.to_f % 1.0
      x = (u * (@w - 1))
      y = (v * (@h - 1))
      x0, y0 = x.floor, y.floor
      x1, y1 = [x0 + 1, @w - 1].min, [y0 + 1, @h - 1].min
      fx, fy = x - x0, y - y0
      cs = colors
      px = ->(i, j) { cs[j * @w + i].to_a }
      mix = lambda do |c|
        (px.(x0, y0)[c] * (1 - fx) * (1 - fy) + px.(x1, y0)[c] * fx * (1 - fy) +
         px.(x0, y1)[c] * (1 - fx) * fy + px.(x1, y1)[c] * fx * fy).round
      end
      Sketchup::Color.new(mix.(0), mix.(1), mix.(2), mix.(3))
    end

    def self.__solid(w, h, rgb)
      row = (w * 3 + 3) / 4 * 4
      line = ([rgb[2], rgb[1], rgb[0]] * w).pack('C*').ljust(row, "\0".b)
      new.set_data(w, h, 24, 4, line * h)
    end
  end

  # ------------------------------------------------------------------ textures
  class Texture < Entity
    def initialize(mat, slot = 'base')
      @mat = mat
      @slot = slot
      super("x:#{mat.id}:#{slot}")
    end

    def __info
      i = @mat.__info
      @slot == 'base' ? i['texture'] : i['pbr']["#{@slot}_texture"]
    end

    def valid?; !!(@mat.valid? && __info); end
    def width; __info['width']; end
    def height; __info['height']; end
    def image_width; __info['w']; end
    def image_height; __info['h']; end
    def filename; __info['filename']; end
    def average_color; Sketchup::Color.new(*__info['avg']); end

    def size=(size)
      raise ArgumentError, 'Too few arguments - at least 1 required' if size.nil?
      w, h = size.is_a?(Array) ? size : [size, nil]
      raise ArgumentError, 'Texture size cannot be 0' if w.to_f == 0.0 || (h && h.to_f == 0.0)
      i = __info
      h ||= w.to_f * i['h'] / i['w']
      Esq.call('mat.tex_size', 'id' => @mat.id, 'w' => w.to_f, 'h' => h.to_f)
      @mat.__changed
      size
    end

    def write(path, colorize = false)
      d = Esq.call('mat.tex_data', 'id' => @mat.id, 'slot' => @slot) or return false
      Esq.call('img.convert', 'path' => path.to_s, 'data' => d['data'])
    rescue StandardError
      false
    end

    # Décodée en mémoire : le nom de fichier de la texture (venu du modèle) ne sert jamais de chemin.
    def image_rep(colorize = false)
      d = Esq.call('mat.tex_data', 'id' => @mat.id, 'slot' => @slot)
      raise RuntimeError, 'Invalid image texture data' unless d
      ImageRep.__from(Esq.call('img.decode', 'data' => d['data']))
    end
  end

  class TextureWriter
    class << self
      private :new
    end

    def initialize; @items = []; end

    def count; @items.size; end
    alias length count

    def load(*args)
      raise ArgumentError, 'Wrong number of arguments: 1 or 2 required' unless (1..2).cover?(args.size)
      ent, front = args[0], args.size == 2 ? args[1] : true
      mat = __material(ent, front)
      raise ArgumentError, 'TextureWriter does not support the loading of the passed object type.' if mat.nil? && !__supported?(ent)
      @items << [ent, front ? true : false, mat]
      @items.size
    end

    def handle(*args)
      raise ArgumentError, 'Wrong number of arguments: 1 or 2 required' unless (1..2).cover?(args.size)
      ent, front = args[0], args.size == 2 ? args[1] : true
      raise ArgumentError, 'TextureWriter does not support objects of the passed type.' unless __supported?(ent)
      i = @items.index { |e, f, _m| e == ent && f == (front ? true : false) }
      i ? i + 1 : 0
    end

    def filename(handle)
      it = @items[handle.to_i - 1] or return nil
      t = it[2] && it[2].texture
      t ? t.filename : nil
    end

    def write(*args)
      raise ArgumentError, 'Wrong number of arguments: 2 or 3 required' unless (2..3).cover?(args.size)
      ent, front, path = args.size == 3 ? args : [args[0], true, args[1]]
      if ent.is_a?(Integer)
        it = @items[ent - 1] or return FILE_WRITE_FAILED_INVALID_TYPE
        mat = it[2]
      else
        raise ArgumentError, 'TextureWriter does not support objects of the passed type.' unless __supported?(ent)
        mat = __material(ent, front)
      end
      return FILE_WRITE_FAILED_UNKNOWN unless mat && mat.texture
      mat.texture.write(path.to_s) ? FILE_WRITE_OK : FILE_WRITE_FAILED_UNKNOWN
    end

    def write_all(*args)
      raise ArgumentError, 'Wrong number of arguments: 2 required' unless args.size == 2
      dir, ext = args
      status = FILE_WRITE_OK
      @items.each_with_index do |(_e, _f, mat), i|
        next unless mat && mat.texture
        base = File.basename(mat.texture.filename, '.*')
        r = mat.texture.write(File.join(dir.to_s, "#{base}#{i + 1}.#{ext.to_s.delete('.')}"))
        status = FILE_WRITE_FAILED_UNKNOWN unless r
      end
      status
    end

    private

    def __supported?(ent)
      ent.is_a?(Sketchup::Drawingelement)
    end

    def __material(ent, front)
      return nil unless __supported?(ent)
      if ent.respond_to?(:back_material) && !front
        ent.back_material
      else
        ent.material
      end
    end
  end

  # Coordonnées de texture homogènes (u·q, v·q, q) d'un point, comme le UV helper de SketchUp :
  # la position du côté s'il est texturé et positionné ou projeté, sinon la projection par
  # défaut dans le repère de la face (`esq_core::meta::side_uv`).
  class UVHelper
    def initialize(face, front = true, back = true, _texture_writer = nil)
      raise TypeError, 'wrong argument type (expected Sketchup::Face)' unless face.is_a?(Sketchup::Face)
      @face = face
      @front = front
      @back = back
    end

    def get_front_UVQ(point); __uvq(point, true); end
    def get_back_UVQ(point); __uvq(point, false); end

    private

    # nil quand l'argument n'est pas un point.
    def __uvq(point, front)
      return nil unless point.is_a?(Geom::Point3d) || Esq.point_value?(point)
      Geom::Point3d.new(*Esq.call('uv.at', 'h' => @face.handle, 'front' => front, 'pt' => Esq.xyz(point)))
    end
  end

  # Placage des textures d'une face, d'après les méthodes Ruby de SketchUp.exe ;
  # le modèle de placage est celui d'esq-core.
  class Face
    # avant et arrière par vérité Ruby ; un 3e argument doit être un TextureWriter.
    def get_UVHelper(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..3)" if args.size > 3
      front = args.size > 0 ? (args[0] ? true : false) : true
      back = args.size > 1 ? (args[1] ? true : false) : true
      if args.size > 2 && !args[2].is_a?(Sketchup::TextureWriter)
        raise TypeError, 'wrong argument type (expected Sketchup::TextureWriter)'
      end
      UVHelper.new(self, front, back, args[2])
    end

    # (matière, points, côté[, projection]). La matière passe par la conversion de
    # `material=` (une matière d'image est refusée) ; sans texture, rien ne change et la méthode
    # rend false. Les points UV ne gardent que u et v.
    def position_material(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 3..4)" unless (3..4).cover?(args.size)
      material, points, on_front, projection = args
      __check_material(material)
      name = Esq.material_name(material)
      m = name && Sketchup.active_model.materials[name]
      return false unless m && m.texture
      raise ArgumentError, 'Array of points required' unless points.is_a?(Array)
      raise ArgumentError, 'Wrong number of points: must be between 2 and 8' unless (1..4).cover?(points.size / 2)
      raise ArgumentError, 'Wrong number of points: must be an even number' if points.size.odd?
      pairs = points.each_slice(2).flat_map do |p, uv|
        q = Esq.xyz(uv)
        [Esq.xyz(p), [q[0], q[1]]]
      end
      a = { 'h' => @handle, 'front' => on_front ? true : false, 'pairs' => pairs, 'material' => m.name }
      a['projection'] = Esq.xyz(projection, 'Geom::Vector3d') unless projection.nil?
      Esq.call('uv.set', a)
      self
    end

    def texture_positioned?(front)
      Esq.call('uv.state', 'h' => @handle, 'front' => front ? true : false)['positioned']
    end

    def texture_projected?(front)
      !Esq.call('uv.state', 'h' => @handle, 'front' => front ? true : false)['projection'].nil?
    end

    def get_texture_projection(front)
      v = Esq.call('uv.state', 'h' => @handle, 'front' => front ? true : false)['projection']
      v && Geom::Vector3d.new(*v)
    end

    def clear_texture_position(front)
      Esq.call('uv.clear', 'h' => @handle, 'front' => front ? true : false)
      nil
    end

    def clear_texture_projection(front)
      Esq.call('uv.clear', 'h' => @handle, 'front' => front ? true : false, 'projection_only' => true)
      nil
    end

    # coins de la tuile de texture sous le point (point du modèle, point UV
    # (u, v, 0), …), comme position_material les prend ; nil si ce côté n'a pas de texture.
    def uv_tile_at(point, front)
      p = Esq.xyz(point)
      r = Esq.call('uv.tile', 'h' => @handle, 'front' => front ? true : false, 'pt' => p)
      r && r.map { |q| Geom::Point3d.new(q) }
    end

    def material=(m)
      __check_material(m)
      super
    end

    def back_material=(m)
      __check_material(m)
      Esq.call('face.set', 'h' => @handle, 'back' => Esq.material_name(m))
      m
    end

    private

    # ni matière d'image, ni matière d'étiquette sur une entité du modèle.
    def __check_material(m)
      return unless m.is_a?(Sketchup::Material)
      raise ArgumentError, 'Cannot assign image material to model entities' if m.owner_type == Sketchup::Material::OWNER_IMAGE
    end
  end
  # ------------------------------------------------------------------ matières
  class Material < Entity
    WORKFLOW_CLASSIC = 0
    WORKFLOW_PBR_METALLIC_ROUGHNESS = 1
    NORMAL_STYLE_OPENGL = 0
    NORMAL_STYLE_DIRECTX = 1
    MATERIAL_SOLID = 0
    MATERIAL_TEXTURED = 1
    MATERIAL_COLORIZED_TEXTURED = 2
    COLORIZE_SHIFT = 0
    COLORIZE_TINT = 1
    OWNER_MANAGER = 0
    OWNER_IMAGE = 1
    OWNER_LAYER = 2

    CACHE = {}

    def self.__for(info)
      m = (CACHE[info['id']] ||= allocate)
      m.__init(info)
      m
    end

    def __init(info)
      @id = info['id']
      @handle = "m:0:#{@id}"
      @cache = info
    end
    private :__init
    public :__init

    attr_reader :id

    def __info
      @cache = Esq.call('mat.get', 'id' => @id)
    end

    def __update(fields)
      @cache = Esq.call('mat.update', { 'id' => @id }.merge(fields))
      __changed
      @cache
    end

    def __changed; Esq.emit(Sketchup.active_model.materials.__observers, :onMaterialChange, Sketchup.active_model.materials, self); end

    def valid?
      __info
      true
    rescue TypeError
      false
    end
    def deleted?; !valid?; end
    def entityID; @id.to_i; end
    alias persistent_id entityID
    def parent; Sketchup.active_model.materials; end
    def erase!; raise NoMethodError, "undefined method `erase!' for #{inspect}"; end

    def name; __info['name']; end
    def name=(n)
      __update('name' => n.to_s)
      n
    end
    def display_name
      n = name
      n =~ /\A\[(.*)\]\z/ ? $1 : n
    end
    def <=>(other)
      raise ArgumentError, 'Cannot get material from argument' unless other.is_a?(Material)
      name <=> other.name
    end
    def ==(other); other.is_a?(Material) && other.id == @id; end
    alias eql? ==
    def hash; @id.hash; end
    def inspect; "#<Sketchup::Material:#{format('0x%016x', object_id * 8)}>"; end

    def alpha; __info['alpha']; end
    def alpha=(a)
      __update('alpha' => a.to_f.clamp(0.0, 1.0))
      a
    end
    def use_alpha?; alpha < 1.0; end
    def color; Sketchup::Color.new(*__info['color']); end
    def color=(c)
      c = Sketchup::Color.new(c) unless c.is_a?(Sketchup::Color)
      __update('color' => c.to_a.first(3))
      c
    end
    def materialType; __info['type']; end
    alias material_type materialType
    def owner_type; __info['owner_image'] ? OWNER_IMAGE : OWNER_MANAGER; end
    def colorize_deltas; __info['colorize_deltas']; end
    def colorize_type; __info['colorize_type']; end
    def colorize_type=(t)
      raise ArgumentError, 'Invalid colorization type' unless [COLORIZE_SHIFT, COLORIZE_TINT].include?(t)
      __update('colorize_type' => t)
      t
    end

    def texture
      __info['texture'] ? Texture.new(self, 'base') : nil
    end

    def texture=(tex)
      case tex
      when nil
        Esq.call('mat.tex_set', 'id' => @id, 'slot' => 'base')
      when String
        __load_texture('base', tex)
      when Array
        raise ArgumentError, 'Texture filename or nil required' unless tex[0].is_a?(String)
        __load_texture('base', tex[0], tex[1], tex[2])
      when ImageRep
        __load_image_rep('base', tex)
      else
        raise ArgumentError, 'Texture filename or nil required'
      end
      __info
      __changed
      tex
    end

    def hatch_pattern
      h = __info['hatch']
      h ? Sketchup.active_model.hatch_patterns.__for_uid(h) : nil
    end
    def hatch_pattern=(hp)
      raise TypeError, 'wrong argument type (expected Sketchup::HatchPattern)' unless hp.nil? || hp.is_a?(HatchPattern)
      __update('hatch' => hp && hp.uid)
      hp
    end

    def duplicate
      Material.__for(Esq.call('mat.duplicate', 'id' => @id)).tap do |m|
        Esq.emit(Sketchup.active_model.materials.__observers, :onMaterialAdd, Sketchup.active_model.materials, m)
      end
    end

    # A SketchUp .skm, written by the application; without it, Esquisse's earlier format.
    def save_as(path)
      ok = Esq.host('model.write_skm', { 'path' => path.to_s, 'material' => Esq.call('mat.dump', 'id' => @id) }, nil)
      return ok ? true : false unless ok.nil?
      Esq.call('mat.export', 'id' => @id, 'path' => path.to_s) ? true : false
    end

    def write_thumbnail(path, size)
      s = size.to_i
      s = 1 if s < 1
      t = __info['texture']
      if t
        d = Esq.call('mat.tex_data', 'id' => @id, 'slot' => 'base')
        return Esq.call('img.convert', 'path' => path.to_s, 'data' => d['data']) ? true : false
      end
      ImageRep.__solid(s, s, __info['color']).save_file(path.to_s)
      true
    rescue StandardError
      false
    end

    # ---- PBR
    def workflow; __info['workflow']; end

    def __pbr(k); __info['pbr'][k]; end
    def __set_pbr(h); __update('pbr' => h); end

    def metalness_enabled?; __pbr('metalness_enabled'); end
    def metalness_enabled=(v); __set_pbr('metalness_enabled' => v ? true : false); v; end
    def metallic_factor; __pbr('metallic_factor'); end
    def metallic_factor=(v)
      __range01(v)
      __set_pbr('metallic_factor' => v.to_f)
      v
    end
    def metallic_texture; __pbr('metallic_texture') ? Texture.new(self, 'metallic') : nil; end
    def metallic_texture=(t); __set_slot('metallic', t); end

    def roughness_enabled?; __pbr('roughness_enabled'); end
    def roughness_enabled=(v); __set_pbr('roughness_enabled' => v ? true : false); v; end
    def roughness_factor; __pbr('roughness_factor'); end
    def roughness_factor=(v)
      __range01(v)
      __set_pbr('roughness_factor' => v.to_f)
      v
    end
    def roughness_texture; __pbr('roughness_texture') ? Texture.new(self, 'roughness') : nil; end
    def roughness_texture=(t); __set_slot('roughness', t); end

    def normal_enabled?; __pbr('normal_enabled'); end
    def normal_enabled=(v)
      if v && !__pbr('normal_texture')
        raise ArgumentError, 'Failed to enable normal mapping: normal texture not set.'
      end
      __set_pbr('normal_enabled' => v ? true : false)
      v
    end
    def normal_texture; __pbr('normal_texture') ? Texture.new(self, 'normal') : nil; end
    def normal_texture=(t); __set_slot('normal', t); end
    def normal_scale; __pbr('normal_scale'); end
    def normal_scale=(v)
      f = v.to_f
      raise RangeError, 'normal scale cannot be Float::Infinity' if f.infinite?
      raise RangeError, 'normal scale cannot be negative' if f < 0
      __set_pbr('normal_scale' => f)
      v
    end
    def normal_style; __pbr('normal_style'); end
    def normal_style=(v)
      unless [NORMAL_STYLE_OPENGL, NORMAL_STYLE_DIRECTX].include?(v)
        raise ArgumentError, "expected NORMAL_STYLE_OPENGL or NORMAL_STYLE_DIRECTX, got #{v.to_i}"
      end
      __set_pbr('normal_style' => v)
      v
    end

    def ao_enabled?; __pbr('ao_enabled'); end
    def ao_enabled=(v)
      if v && !__pbr('ao_texture')
        raise ArgumentError, 'Failed to enable ambient occlusion: ambient occlusion texture not set.'
      end
      __set_pbr('ao_enabled' => v ? true : false)
      v
    end
    def ao_texture; __pbr('ao_texture') ? Texture.new(self, 'ao') : nil; end
    def ao_texture=(t); __set_slot('ao', t); end
    def ao_strength; __pbr('ao_strength'); end
    def ao_strength=(v)
      __range01(v)
      __set_pbr('ao_strength' => v.to_f)
      v
    end

    private

    def __range01(v)
      f = v.to_f
      raise RangeError, format('expected value between %g and %g, got %g', 0, 1, f) if f < 0 || f > 1 || f.nan?
    end

    def __load_texture(slot, path, w = nil, h = nil)
      args = { 'id' => @id, 'slot' => slot, 'path' => path }
      args['w'] = w.to_f if w
      args['h'] = h.to_f if h
      Esq.call('mat.tex_set', args)
    rescue ArgumentError
      nil # fichier illisible : la matière reste inchangée, comme dans SketchUp
    end

    def __load_image_rep(slot, rep)
      png = Esq.call('img.encode', rep.__fields)
      Esq.call('mat.tex_set', 'id' => @id, 'slot' => slot, 'data' => png, 'name' => 'image.png')
    end

    def __set_slot(slot, t)
      case t
      when nil then Esq.call('mat.tex_set', 'id' => @id, 'slot' => slot)
      when String then __load_texture(slot, t)
      when ImageRep then __load_image_rep(slot, t)
      else raise ArgumentError, 'Texture filename or nil required'
      end
      if t.nil?
        key = { 'normal' => 'normal_enabled', 'ao' => 'ao_enabled' }[slot]
        __set_pbr(key => false) if key
      end
      __changed
      t
    end
  end

  class Materials < Entity
    include Enumerable

    def initialize
      @observers = []
      super('M:0:0')
    end
    def __observers; @observers; end
    def valid?; true; end

    def each(&b)
      return to_enum(:each) unless b
      Esq.call('mat.list').map { |i| Material.__for(i) }.each(&b)
      self
    end
    def length; Esq.call('mat.list').size; end
    alias size length
    def count(*a, &b); a.empty? && !b ? length : super; end

    def [](k)
      list = Esq.call('mat.list')
      case k
      when Integer
        raise IndexError, "index #{k} out of range [0, #{list.size})" if k < 0 || k >= list.size
        Material.__for(list[k])
      when String
        i = list.find { |m| m['name'] == k }
        i && Material.__for(i)
      end
    end
    alias at []

    def add(*args)
      raise ArgumentError, "wrong # of arguments(#{args.size} for 0 or 1)" if args.size > 1
      raise ArgumentError, 'Material name required' if args.size == 1 && !args[0].is_a?(String)
      i = Esq.call('mat.add', args.empty? ? {} : { 'name' => args[0] })
      m = Material.__for(i)
      Esq.emit(@observers, :onMaterialAdd, self, m)
      m
    end

    def remove(mat)
      raise ArgumentError, 'Cannot get material from argument' unless mat.is_a?(Material)
      Esq.call('mat.remove', 'id' => mat.id)
      Esq.emit(@observers, :onMaterialRemove, self, mat)
      true
    end

    def purge_unused
      before = length
      Esq.call('mat.purge')
      before != length
    end

    def current
      i = Esq.call('mat.current')
      i && Material.__for(i)
    end

    def current=(mat)
      if mat.nil?
        Esq.call('mat.set_current', {})
      elsif mat.is_a?(Material)
        Esq.call('mat.set_current', 'id' => mat.id)
      elsif mat.is_a?(String)
        m = self[mat] or raise ArgumentError, 'Cannot get material from argument'
        Esq.call('mat.set_current', 'id' => m.id)
      else
        raise ArgumentError, 'Cannot get material from argument'
      end
      Esq.emit(@observers, :onMaterialSetCurrent, self, current)
      mat
    end

    # A SketchUp .skm read by the application, else a file of Esquisse's earlier format.
    def load(path)
      raise ArgumentError, 'wrong argument type' unless path.is_a?(String)
      data = Esq.host('model.read_skm', { 'path' => path }, nil)
      m = Material.__for(Esq.call('mat.import', 'path' => path, 'material' => data))
      Esq.emit(@observers, :onMaterialAdd, self, m)
      m
    end

    def unique_name(base = nil)
      Esq.call('mat.unique_name', 'name' => base.nil? ? 'Material' : base.to_s)
    end

    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end
  end

  # ------------------------------------------------------------------ styles de ligne
  class LineStyle < Entity
    NAMES = ['Solid Line', 'Dotted Line', 'Short Dashes Line', 'Dashes Line', 'Dash Dot Dash Line'].freeze
    attr_reader :name
    def initialize(name); @name = name; super("s:0:#{NAMES.index(name)}"); end
    def valid?; true; end
    def ==(o); o.is_a?(LineStyle) && o.name == @name; end
    alias eql? ==
    def hash; @name.hash; end
  end

  class LineStyles < Entity
    include Enumerable
    def initialize; super('S:0:0'); end
    def valid?; true; end
    def size; LineStyle::NAMES.size; end
    alias length size
    def names; LineStyle::NAMES.dup; end
    def to_a; LineStyle::NAMES.map { |n| LineStyle.new(n) }; end
    def each(&b)
      return to_enum(:each) unless b
      to_a.each(&b)
      self
    end
    def [](k)
      case k
      when Integer
        raise IndexError, "index #{k} out of range [0, #{size})" if k < 0 || k >= size
        LineStyle.new(LineStyle::NAMES[k])
      when String
        LineStyle::NAMES.include?(k) ? LineStyle.new(k) : nil
      end
    end
    alias at []
  end

  # ------------------------------------------------------------------ motifs de hachure
  class HatchPattern < Entity
    attr_reader :uid
    def initialize(info)
      @uid = info['uid']
      @cache = info
      super("h:0:#{@uid}")
    end
    def __info; @cache = Esq.call('hatch.list').find { |h| h['uid'] == @uid } or raise TypeError, 'reference to deleted HatchPattern'; end
    def valid?; Esq.call('hatch.list').any? { |h| h['uid'] == @uid }; end
    def name; __info['name']; end
    def name=(n)
      Esq.call('hatch.update', 'uid' => @uid, 'name' => n.to_s)
      n
    end
    def width; __info['width']; end
    def height; __info['height']; end
    def size=(s)
      raise ArgumentError, 'Too few arguments - at least 1 required' if s.nil?
      raise ArgumentError, 'Parameter must be an array' unless s.is_a?(Array)
      raise ArgumentError, 'Texture sizes must be positive values' unless s.size >= 2 && s.all? { |v| v.to_f > 0 }
      Esq.call('hatch.update', 'uid' => @uid, 'size' => s.first(2).map(&:to_f))
      s
    end
    def write(path); Esq.call('hatch.write', 'uid' => @uid, 'path' => path.to_s) ? true : false; end
    def ==(o); o.is_a?(HatchPattern) && o.uid == @uid; end
    alias eql? ==
    def hash; @uid.hash; end
  end

  class HatchPatterns < Entity
    include Enumerable
    def initialize; super('H:0:0'); end
    def valid?; true; end
    def __for_uid(uid)
      i = Esq.call('hatch.list').find { |h| h['uid'] == uid }
      i && HatchPattern.new(i)
    end
    def each(&b)
      return to_enum(:each) unless b
      Esq.call('hatch.list').map { |i| HatchPattern.new(i) }.each(&b)
      self
    end
    def size; Esq.call('hatch.list').size; end
    alias length size
    def [](k)
      list = Esq.call('hatch.list')
      i = k.is_a?(Integer) ? list[k] : list.find { |h| h['name'] == k.to_s }
      i && HatchPattern.new(i)
    end
    def add(name, path)
      HatchPattern.new(Esq.call('hatch.add', 'name' => name.to_s, 'path' => path.to_s))
    end
    def remove(hp)
      raise TypeError, 'wrong argument type (expected Sketchup::HatchPattern)' unless hp.is_a?(HatchPattern)
      Esq.call('hatch.remove', 'uid' => hp.uid)
      true
    end
    def purge_unused; Esq.call('hatch.purge') > 0; end
  end

  # ------------------------------------------------------------------ étiquettes
  class Layer < Entity
    attr_reader :uid

    def self.__for(info)
      (CACHE[info['uid']] ||= allocate).tap { |l| l.__init(info) }
    end
    CACHE = {}

    def __init(info)
      @uid = info['uid']
      @handle = "l:0:#{@uid}"
      @cache = info
    end
    public :__init

    def __info
      @cache = Esq.call('layer.list').find { |l| l['uid'] == @uid } or raise TypeError, 'reference to deleted Layer'
    end

    def __update(h)
      @cache = Esq.call('layer.update', { 'uid' => @uid }.merge(h))
      Esq.emit(Sketchup.active_model.layers.__observers, :onLayerChanged, Sketchup.active_model.layers, self)
      @cache
    end

    def valid?; Esq.call('layer.list').any? { |l| l['uid'] == @uid }; end
    def deleted?; !valid?; end
    def entityID; @uid; end
    alias persistent_id entityID
    def parent; Sketchup.active_model.layers; end
    def erase!; raise NoMethodError, "undefined method `erase!' for #{inspect}"; end
    def inspect; "#<Sketchup::Layer:#{format('0x%016x', object_id * 8)}>"; end

    def name; __info['name']; end
    def name=(n)
      raise ArgumentError, 'tag name required' unless n.is_a?(String) && !n.empty?
      __update('name' => n)
      n
    end
    def display_name; name == Layers::DEFAULT_NAME ? 'Untagged' : name; end
    def visible?; __info['visible']; end
    def visible=(v)
      __update('visible' => v ? true : false)
      v
    end
    def color
      c = __info['color']
      Sketchup::Color.new(*(c || [128, 128, 128]))
    end
    def color=(c)
      c = Sketchup::Color.new(c) unless c.nil? || c.is_a?(Sketchup::Color)
      __update('color' => c && c.to_a.first(3))
      c
    end
    def page_behavior; __info['page_behavior']; end
    def page_behavior=(v)
      raise ArgumentError, 'Invalid page behavior' unless v.is_a?(Integer)
      __update('page_behavior' => v)
      v
    end
    def line_style
      n = __info['line_style']
      LineStyle.new(LineStyle::NAMES.include?(n) ? n : LineStyle::NAMES[0])
    end
    def line_style=(ls)
      raise ArgumentError, 'wrong argument type (expected Sketchup::LineStyle)' unless ls.is_a?(LineStyle)
      __update('line_style' => ls.name)
      ls
    end
    def folder
      f = __info['folder']
      f ? Sketchup.active_model.layers.__folder(f) : nil
    end
    def folder=(f)
      raise TypeError, 'parent must be a Sketchup::LayerFolder or nil' unless f.nil? || f.is_a?(LayerFolder)
      __update('folder' => f && f.uid)
      f
    end
    def <=>(other)
      return nil unless other.is_a?(Layer)
      name <=> other.name
    end
    def ==(o); o.is_a?(Layer) && o.uid == @uid; end
    alias eql? ==
    def hash; @uid.hash; end
  end

  class LayerFolder < Entity
    attr_reader :uid
    include Comparable

    def self.__for(info)
      (CACHE[info['uid']] ||= allocate).tap { |l| l.__init(info) }
    end
    CACHE = {}

    def __init(info)
      @uid = info['uid']
      @handle = "o:0:#{@uid}"
    end
    public :__init

    def __info
      Esq.call('folder.list').find { |f| f['uid'] == @uid } or raise TypeError, 'reference to deleted LayerFolder'
    end
    def __update(h)
      Esq.call('folder.update', { 'uid' => @uid }.merge(h))
      Esq.emit(Sketchup.active_model.layers.__observers, :onLayerFolderChanged, Sketchup.active_model.layers, self)
    end

    def valid?; Esq.call('folder.list').any? { |f| f['uid'] == @uid }; end
    def entityID; @uid; end
    alias persistent_id entityID
    def parent; Sketchup.active_model.layers; end
    def erase!; raise NoMethodError, "undefined method `erase!' for #{inspect}"; end

    def name; __info['name']; end
    def name=(n)
      raise ArgumentError, 'Folder names cannot be empty.' unless n.is_a?(String) && !n.empty?
      __update('name' => n)
      n
    end
    alias display_name name
    def visible?; __info['visible']; end
    def visible=(v)
      __update('visible' => v ? true : false)
      v
    end
    def visible_on_new_pages?; __info['visible_on_new_pages']; end
    def visible_on_new_pages=(v)
      __update('visible_on_new_pages' => v ? true : false)
      v
    end
    def folder
      p = __info['parent']
      p ? Sketchup.active_model.layers.__folder(p) : nil
    end
    def folder=(f)
      raise TypeError, 'parent must be a Sketchup::LayerFolder or nil' unless f.nil? || f.is_a?(LayerFolder)
      __update('parent' => f && f.uid)
      f
    end

    def add_folder(name)
      raise ArgumentError, 'Folder names cannot be empty.' unless name.is_a?(String) && !name.empty?
      LayerFolder.__notify(:onLayerFolderAdded, LayerFolder.__for(Esq.call('folder.add', 'name' => name, 'parent' => @uid)))
    end
    def remove_folder(f)
      raise ArgumentError, 'folder is not a child' unless f.is_a?(LayerFolder) && f.folder == self
      Esq.call('folder.remove', 'uid' => f.uid)
      LayerFolder.__notify(:onLayerFolderRemoved, f)
      true
    end

    # LayersObserver#onLayerFolderAdded / onLayerFolderRemoved (layers, folder).
    def self.__notify(event, folder)
      layers = Sketchup.active_model.layers
      Esq.emit(layers.__observers, event, layers, folder)
      folder
    end
    def folders; Esq.call('folder.list').select { |f| f['parent'] == @uid }.map { |f| LayerFolder.__for(f) }; end
    def count_folders; folders.size; end
    def each_folder(&b)
      return to_enum(:each_folder) unless b
      folders.each(&b)
      self
    end

    def add_layer(layer)
      raise ArgumentError, 'argument must be a Sketchup::Layer' unless layer.is_a?(Layer)
      layer.folder = self
      layer
    end
    def remove_layer(layer)
      raise ArgumentError, 'argument must be a Sketchup::Layer' unless layer.is_a?(Layer)
      raise ArgumentError, 'layer is not a child' unless layer.folder == self
      layer.folder = nil
      true
    end
    def layers; Esq.call('layer.list').select { |l| l['folder'] == @uid }.map { |l| Layer.__for(l) }; end
    def count_layers; layers.size; end
    alias length count_layers
    alias size count_layers
    def each_layer(&b)
      return to_enum(:each_layer) unless b
      layers.each(&b)
      self
    end
    alias each each_layer
    include Enumerable

    def <=>(other)
      return nil unless other.is_a?(LayerFolder)
      name <=> other.name
    end
    def ==(o); o.is_a?(LayerFolder) && o.uid == @uid; end
    alias eql? ==
    def hash; @uid.hash; end
  end

  class Layers < Entity
    include Enumerable
    DEFAULT_NAME = 'Layer0'

    def initialize
      @observers = []
      super('L:0:0')
    end
    def __observers; @observers; end
    def valid?; true; end

    def __list; Esq.call('layer.list'); end
    def __folder(uid)
      i = Esq.call('folder.list').find { |f| f['uid'] == uid }
      i && LayerFolder.__for(i)
    end
    def __active
      Layer.__for(__list.find { |l| l['active'] } || __list[0])
    end
    def __activate(layer)
      Esq.call('layer.set_active', 'uid' => layer.uid)
      Esq.emit(@observers, :onCurrentLayerChanged, self, layer)
    end

    # Ce que SketchUp accepte là où il attend une étiquette (`Model#active_layer=`,
    # `Drawingelement#layer=`) : une étiquette valide ; nil ou "Layer0" pour
    # l'étiquette par défaut ; le nom d'une étiquette du modèle. Le reste lève ArgumentError.
    def __tag_arg(v)
      case v
      when Layer
        return v if v.valid?
      when nil
        return self[0]
      when String
        return self[0] if v == DEFAULT_NAME
        l = self[v]
        return l if l
        raise ArgumentError, "Cannot find tag named \"#{v}\""
      end
      raise ArgumentError, 'No implicit conversion to tag'
    end

    def each(&b)
      return to_enum(:each) unless b
      __list.map { |l| Layer.__for(l) }.each(&b)
      self
    end
    def length; __list.size; end
    alias size length

    def [](k)
      list = __list
      case k
      when Integer
        raise IndexError, "index #{k} out of range [0, #{list.size})" if k < 0 || k >= list.size
        Layer.__for(list[k])
      when String
        i = list.find { |l| l['name'] == k }
        i && Layer.__for(i)
      end
    end
    alias at []

    def add(name)
      raise ArgumentError, 'tag name required' unless name.is_a?(String) && !name.empty?
      i = Esq.call('layer.add', 'name' => name)
      l = Layer.__for(i)
      Esq.emit(@observers, :onLayerAdded, self, l) if i['created']
      l
    end
    alias add_layer add

    def remove(*args)
      raise ArgumentError, 'must have 1 or 2 arguments' unless (1..2).cover?(args.size)
      target, del = args
      list = __list
      if target.is_a?(Integer)
        raise IndexError, "index #{target} out of range [0, #{list.size})" if target < 0 || target >= list.size
        target = Layer.__for(list[target])
      end
      raise ArgumentError, "tag doesn't belong to this model" unless target.is_a?(Layer) && target.valid?
      Esq.call('layer.remove', 'uid' => target.uid, 'delete_geometry' => del ? true : false)
      Esq.emit(@observers, :onLayerRemoved, self, target)
      true
    end
    alias remove_layer remove

    def purge_unused
      n = Esq.call('layer.purge')
      n > 0
    end
    alias purge_unused_layers purge_unused

    def unique_name(base = nil)
      Esq.call('layer.unique_name', 'name' => base.nil? ? 'Tag1' : base.to_s)
    end

    def layers; __list.select { |l| l['folder'].nil? }.map { |l| Layer.__for(l) }; end
    def count_layers; __list.count { |l| l['folder'].nil? }; end
    def each_layer(&b)
      return to_enum(:each_layer) unless b
      layers.each(&b)
      self
    end

    def folders; Esq.call('folder.list').select { |f| f['parent'].nil? }.map { |f| LayerFolder.__for(f) }; end
    def count_folders; folders.size; end
    def each_folder(&b)
      return to_enum(:each_folder) unless b
      folders.each(&b)
      self
    end
    def add_folder(name)
      raise ArgumentError, 'Folder names cannot be empty.' unless name.is_a?(String) && !name.empty?
      LayerFolder.__notify(:onLayerFolderAdded, LayerFolder.__for(Esq.call('folder.add', 'name' => name)))
    end
    def remove_folder(f)
      raise ArgumentError, 'folder is not a child' unless f.is_a?(LayerFolder) && f.valid? && f.folder.nil?
      Esq.call('folder.remove', 'uid' => f.uid)
      LayerFolder.__notify(:onLayerFolderRemoved, f)
      true
    end
    def purge_unused_folders; Esq.call('folder.purge') > 0; end

    def dump; each { |l| $stdout.puts("#{l.name} (#{l.visible? ? 'visible' : 'hidden'})") }; nil; end
    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end
  end

  # ------------------------------------------------------------------ ombres et options
  class OptionsProvider
    include Enumerable
    class << self
      private :new
    end
    attr_reader :name

    def initialize(name)
      @name = name
      @observers = []
    end

    def __data; Esq.call('opt.get', 'provider' => @name); end
    def size; __data.size; end
    alias length size
    def keys; __data.keys; end
    def [](key); __data[key.to_s]; end
    def []=(key, value)
      Esq.call('opt.set', 'provider' => @name, 'key' => key.to_s, 'value' => value)
      Esq.emit(@observers, :onOptionsProviderChanged, self, key.to_s)
      value
    end
    def each_pair
      return to_enum(:each_pair) unless block_given?
      __data.each { |k, v| yield k, v }
      self
    end
    alias each each_pair
    def each_key(&b)
      return to_enum(:each_key) unless b
      keys.each(&b)
      self
    end
    def each_value(&b)
      return to_enum(:each_value) unless b
      __data.values.each(&b)
      self
    end
    def has_key?(key); __data.key?(key.to_s); end
    alias key? has_key?
    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end
  end

  class OptionsManager
    include Enumerable
    class << self
      private :new
    end
    def initialize; @providers = {}; end
    def keys; Esq.call('opt.providers'); end
    def size; keys.size; end
    alias length size
    def [](key)
      k = key.is_a?(Integer) ? keys[key] : key.to_s
      return nil unless k && keys.include?(k)
      @providers[k] ||= OptionsProvider.send(:new, k)
    end
    def each
      return to_enum(:each) unless block_given?
      keys.each { |k| yield self[k] }
      self
    end
  end

  # ------------------------------------------------------------------ importateurs
  class Importer
    ImportSuccess = 0
    ImportFail = 1
    ImportCanceled = 2
    ImportFileNotFound = 4

    def description; nil; end
    def file_extension; nil; end
    def id; nil; end
    def supports_options?; false; end
    def do_options; nil; end
    def show_summary; nil; end
    def load_file(_path, _status = false); ImportFail; end
  end

  module Skp
    # SketchUp: the GUID saved in the model file, read by SketchUpAPI.dll for a
    # .skp (Esquisse's own .esq keeps it in its metadata). "file not found", "invalid model"
    # (ArgumentError) or "unknown error" (RuntimeError).
    def self.read_guid(path)
      # Ruby's own implicit conversion (rb_string_value_ptr): TypeError for a non-String.
      path = String.new(path) unless path.is_a?(String)
      raise ArgumentError, 'file not found' unless File.file?(path)
      if path =~ /\.esq\z/i
        g = (JSON.parse(File.binread(path))['meta'] || {})['guid'] rescue nil
        return g if g.is_a?(String) && !g.empty?
        raise ArgumentError, 'invalid model'
      end
      Esq.call('meta.read_guid', 'path' => path)
    end
  end
end
