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
  # Les pixels comme SketchUp les garde (SUImageRep) : lignes depuis le haut, BGR ou BGRA (ou un
  # octet de gris), chaque ligne suivie de `row_padding` octets. Une image chargée a 24 bits
  # par pixel, 32 si un de ses pixels n'est pas opaque, 8 si elle est en gris sans
  # transparence (bridge/model/images.rs), et des lignes alignées sur 4 octets.
  class ImageRep
    # (rb_scan_args « 01 ») : sans argument (ou nil), une image vide.
    def initialize(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      @w = @h = @bpp = @pad = 0
      @data = nil
      load_file(args[0]) unless args.empty? || args[0].nil?
    end

    def width; @w; end
    def height; @h; end
    def bits_per_pixel; @bpp; end
    def row_padding; @pad; end
    # SUImageRepGetDataSize : les octets des pixels, marges des lignes comprises.
    def size; @data ? @data.bytesize : 0; end
    # nil pour une image vide.
    def data; @data && @data.dup; end

    # StringValue (TypeError), ArgumentError si le fichier ne se lit pas.
    def load_file(path)
      path = Esq.str_arg(path)
      __assign(Esq.call('img.load', 'path' => path))
      nil
    end

    # RuntimeError si l'image ne s'écrit pas (vide, dossier absent, type
    # inconnu).
    def save_file(path)
      path = Esq.str_arg(path)
      raise RuntimeError, 'failed to save image' unless @data
      Esq.call('img.save', __fields.merge('path' => path))
      nil
    end

    def self.__from(r); new.__assign(r); end

    def __assign(r)
      @w, @h, @bpp, @pad = r['w'], r['h'], r['bpp'], r['pad']
      @data = r['data'].unpack1('m0')
      self
    end

    def __fields; { 'w' => @w, 'h' => @h, 'bpp' => @bpp, 'pad' => @pad, 'data' => [@data || ''].pack('m0') }; end

    # quatre entiers non signés (TypeError, RangeError), une chaîne
    # (TypeError), dont la taille vaut (bpp × largeur / 8 + marge) × hauteur ; puis
    # SUImageRepSetData : données vides, taille nulle ou bpp hors de 8/24/32 lèvent ArgumentError.
    def set_data(width, height, bits_per_pixel, row_padding, pixel_data)
      w = Esq::Conv.num2uint(width)
      h = Esq::Conv.num2uint(height)
      bpp = Esq::Conv.num2uint(bits_per_pixel)
      pad = Esq::Conv.num2uint(row_padding)
      raise TypeError, 'expected binary string' unless pixel_data.is_a?(String)
      if pixel_data.bytesize != ((bpp * w) / 8 + pad) * h
        raise ArgumentError, 'width, height, bits per pixel and row padding must equate to the size of the binary string'
      end
      raise ArgumentError, 'invalid pixel data' if pixel_data.empty?
      unless w > 0 && h > 0 && [8, 24, 32].include?(bpp)
        raise ArgumentError, 'invalid width, height, or bits per pixel. Width or height must be greater than 0. Bits per pixel must be 8/24/32.'
      end
      @w, @h, @bpp, @pad = w, h, bpp, pad
      @data = pixel_data.b
      self
    end

    # La couleur du pixel (x, y) depuis le haut.
    def __pixel(x, y)
      if @bpp == 8
        g = @data.getbyte(y * (@w + @pad) + x).to_i
        return [g, g, g, 255]
      end
      px = @bpp / 8
      o = y * (@w * px + @pad) + x * px
      [@data.getbyte(o + 2), @data.getbyte(o + 1), @data.getbyte(o), px == 4 ? @data.getbyte(o + 3) : 255]
    end

    # SUImageRepGetDataAsColors : nil pour une image vide.
    def colors
      return nil unless @data
      out = Array.new(@w * @h)
      i = 0
      @h.times do |y|
        @w.times do |x|
          out[i] = Sketchup::Color.new(*__pixel(x, y))
          i += 1
        end
      end
      out
    end

    # (rb_scan_args « 21 », rb_num2dbl) puis le lecteur de SketchUp:
    #  u et v se répètent, v monte depuis le bas ; le pixel sous le point, ou
    # le mélange des quatre plus proches de son centre (bilinéaire), tronqué. nil si vide.
    def color_at_uv(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 2..3)" unless (2..3).cover?(args.size)
      u = Esq::Conv.num2dbl(args[0])
      v = Esq::Conv.num2dbl(args[1])
      bilinear = args.size > 2 && args[2] ? true : false
      return nil unless @data && @w > 0 && @h > 0
      fu = u % 1.0
      fv = -v % 1.0
      x = (@w * fu).to_i
      x = @w - 1 if x >= @w
      y = (@h * fv).to_i
      y = @h - 1 if y >= @h
      return Sketchup::Color.new(*__pixel(x, y)) unless bilinear
      near = lambda do |d, i, n|
        if d < 0.5 then [0.5 - d, i - 1 < 0 ? n - 1 : i - 1]
        elsif d > 0.5 then [d - 0.5, i + 1 > n - 1 ? 0 : i + 1]
        else [0.5, i]
        end
      end
      wx, x1 = near.(@w * fu - x, x, @w)
      wy, y1 = near.(@h * fv - y, y, @h)
      c00, c10, c01, c11 = __pixel(x, y), __pixel(x1, y), __pixel(x, y1), __pixel(x1, y1)
      mix = (0..3).map do |k|
        f = (c00[k] * (1 - wx) * (1 - wy) + c10[k] * wx * (1 - wy) + c01[k] * (1 - wx) * wy + c11[k] * wx * wy) / 255.0
        f <= 0.0 ? 0 : (f >= 1.0 ? 255 : (f * 255.0).to_i)
      end
      Sketchup::Color.new(*mix)
    end

    def self.__solid(w, h, rgb)
      row = w * 3
      pad = (4 - row % 4) % 4
      line = ([rgb[2], rgb[1], rgb[0]] * w).pack('C*') + ("\0".b * pad)
      new.set_data(w, h, 24, pad, line * h)
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

    # La matière de la texture.
    def parent; @mat; end

    # Le chemin par StringValue (TypeError) ; la colorisation n'est pas encore appliquée.
    def write(path, colorize = false)
      path = Esq.str_arg(path)
      _ = colorize
      begin
        d = Esq.call('mat.tex_data', 'id' => @mat.id, 'slot' => @slot) or return false
        Esq.call('img.convert', 'path' => path, 'data' => d['data'])
      rescue StandardError
        false
      end
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

    def __check_material(m); Esq.material_setter_check!(m); end
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
    def entityID; Esq.object_pid(:material, @id.to_i); end
    alias persistent_id entityID
    def parent; Sketchup.active_model.materials; end
    def erase!; raise NoMethodError, "undefined method `erase!' for #{inspect}"; end

    def name; __info['name']; end
    # StringValue ; le même nom ne change rien, celui d'une autre matière lève
    # ArgumentError.
    def name=(n)
      s = Esq.str_arg(n)
      __update('name' => s) unless s == name
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
    # rb_num2dbl, ramené entre 0 et 1.
    def alpha=(a)
      __update('alpha' => Esq::Conv.num2dbl(a).clamp(0.0, 1.0))
      a
    end
    def use_alpha?; alpha < 1.0; end
    def color; Sketchup::Color.new(*__info['color']); end
    # nil ou ce dont Color.new fait une couleur ; (0, 0, 0, 0) retire la
    # colorisation d'une texture colorisée (le pont fait le reste).
    def color=(c)
      rgba = if c.nil? then [0, 0, 0, 0]
             else (c.is_a?(Sketchup::Color) ? c : Sketchup::Color.new(c)).to_a
             end
      __update('color' => rgba)
      c
    end
    def materialType; __info['type']; end
    alias material_type materialType
    def owner_type; __info['owner_image'] ? OWNER_IMAGE : OWNER_MANAGER; end
    def colorize_deltas; __info['colorize_deltas']; end
    def colorize_type; __info['colorize_type']; end
    # un entier non signé (TypeError, RangeError), au plus 2, sinon
    # ArgumentError.
    def colorize_type=(t)
      v = Esq::Conv.num2uint(t)
      raise ArgumentError, 'Invalid colorization type' unless v < 3
      __update('colorize_type' => v)
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
    # le chemin par StringValue.
    def save_as(path)
      path = Esq.str_arg(path)
      ok = Esq.host('model.write_skm', { 'path' => path, 'material' => Esq.call('mat.dump', 'id' => @id) }, nil)
      return ok ? true : false unless ok.nil?
      Esq.call('mat.export', 'id' => @id, 'path' => path) ? true : false
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

    # Une ImageRep n'a pas de nom de fichier : SketchUp prend un format par défaut, « .jpg »
    # (« .png » pour une image avec transparence) ; les pixels sont gardés sans perte (PNG).
    def __load_image_rep(slot, rep)
      png = Esq.call('img.encode', rep.__fields)
      name = rep.bits_per_pixel == 32 ? '.png' : '.jpg'
      Esq.call('mat.tex_set', 'id' => @id, 'slot' => slot, 'data' => png, 'name' => name)
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

    # un nom (nil s'il n'y en a pas), sinon un index par rb_num2long
    # (TypeError), négatif depuis la fin.
    def [](k)
      list = Esq.call('mat.list')
      if k.is_a?(String)
        i = list.find { |m| m['name'] == k }
        return i && Material.__for(i)
      end
      n = Esq::Conv.num2long(k)
      n += list.size if n.negative?
      raise IndexError, "index #{n} out of range [0, #{list.size})" if n.negative? || n >= list.size
      Material.__for(list[n])
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

    # faux pour nil, 0 ou une matière effacée ; la matière
    # d'une image lève ArgumentError ; un nom, sinon le nom d'une couleur
    # (ArgumentError « Cannot find material named … » à défaut) ; autre chose, la matière de
    # cette couleur.
    def remove(mat)
      return false if mat.nil? || (mat.is_a?(Integer) && mat.zero?)
      m = case mat
          when Material
            return false unless mat.valid?
            raise ArgumentError, 'Cannot assign image material to model entities' if mat.owner_type == Material::OWNER_IMAGE
            mat
          when String
            self[mat] || __by_color(Sketchup::Color.new(mat)) || raise(ArgumentError, "Cannot find material named #{mat}")
          else
            __by_color(Sketchup::Color.new(mat))
          end
      return false unless m
      Esq.call('mat.remove', 'id' => m.id)
      Esq.emit(@observers, :onMaterialRemove, self, m)
      true
    rescue ArgumentError => e
      raise e unless mat.is_a?(String) && !e.message.start_with?('Cannot find material')
      raise ArgumentError, "Cannot find material named #{mat}"
    end

    # La matière d'une couleur, aucune pour (0, 0, 0, 0).
    def __by_color(c)
      return nil if c.to_a == [0, 0, 0, 0]
      find { |x| x.color.to_a.first(3) == c.to_a.first(3) }
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
    # le chemin par StringValue ; un échec lève RuntimeError.
    def load(path)
      path = Esq.str_arg(path)
      data = Esq.host('model.read_skm', { 'path' => path }, nil)
      m = Material.__for(Esq.call('mat.import', 'path' => path, 'material' => data))
      Esq.emit(@observers, :onMaterialAdd, self, m)
      m
    end

    # un nom par StringValue.
    def unique_name(base)
      Esq.call('mat.unique_name', 'name' => Esq.str_arg(base))
    end

    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end
  end

  # ------------------------------------------------------------------ styles de ligne
  # Les douze styles de ligne de SketchUp 2026, dans l'ordre de son gestionnaire (les noms
  # qu'écrit esq-skp, `native::write::plan::LINE_STYLES`). Une étiquette sans style garde la
  # chaîne vide dans le modèle : `Layer#line_style` rend nil.
  class LineStyle < Entity
    NAMES = ['Solid Basic', 'Short dash', 'Dash', 'Dot', 'Dash dot', 'Dash double-dot', 'Dash triple-dot',
             'Double-dash dot', 'Double-dash double-dot', 'Double-dash triple-dot', 'Long-dash dash',
             'Long-dash double-dash'].freeze
    CACHE = {}
    private_class_method :new

    # Un seul objet Ruby par style (SketchUp garde l'enveloppe de chaque objet natif).
    def self.__for(name)
      CACHE[name] ||= new(name)
    end

    def initialize(name); @name = name; super("s:0:#{NAMES.index(name)}"); end
    def valid?; true; end
    def name; @name.dup; end
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
    def names; LineStyle::NAMES.map(&:dup); end
    def to_a; LineStyle::NAMES.map { |n| LineStyle.__for(n) }; end
    def each
      return to_enum(:each) unless block_given?
      to_a.each { |s| yield s }
      self
    end

    # un nom (nil s'il n'existe pas), sinon un index (rb_num2long : TypeError
    # pour nil ou un autre objet), négatif depuis la fin.
    def [](k)
      return (LineStyle::NAMES.include?(k) ? LineStyle.__for(k) : nil) if k.is_a?(String)
      i = Esq::Conv.num2long(k)
      n = size
      j = i < 0 ? i + n : i
      raise IndexError, "index #{j} out of range [0, #{n})" if j < 0 || j >= n
      LineStyle.__for(LineStyle::NAMES[j])
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
    def entityID; Esq.object_pid(:tag, @uid); end
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
    # une étiquette du chemin actif ne se cache pas ; cachée, l'étiquette active
    # cède la place à l'étiquette par défaut, et la sélection perd ce qu'elle ne montre plus.
    def visible=(v)
      __update('visible' => v ? true : false)
      v
    end
    def color
      c = __info['color']
      Sketchup::Color.new(*(c || [128, 128, 128]))
    end
    # la valeur passe par la conversion de Sketchup::Color (nil : TypeError, un
    # nom inconnu : ArgumentError) ; l'étiquette garde une couleur opaque (SketchUp 2020+).
    def color=(c)
      c = Sketchup::Color.new(c) unless c.is_a?(Sketchup::Color)
      __update('color' => c.to_a.first(3))
      c
    end
    def page_behavior; __info['page_behavior']; end
    # un entier non signé (TypeError, RangeError) ; seuls les bits 0x01, 0x10 et
    # 0x20 existent, et pas 0x10 avec 0x20.
    def page_behavior=(v)
      __update('page_behavior' => Esq::Conv.num2uint(v))
      v
    end
    # nil quand l'étiquette n'a pas de style de ligne.
    def line_style
      n = __info['line_style']
      LineStyle::NAMES.include?(n) ? LineStyle.__for(n) : nil
    end
    # nil enlève le style (SULayerClearLineStyle).
    def line_style=(ls)
      Esq::Conv.wrong_type('LineStyle') unless ls.nil? || ls.is_a?(LineStyle)
      __update('line_style' => ls ? ls.name : '')
      ls
    end
    def folder
      f = __info['folder']
      f ? Sketchup.active_model.layers.__folder(f) : nil
    end
    def folder=(f)
      raise TypeError, 'parent must be a Sketchup::LayerFolder or nil' unless f.nil? || (f.is_a?(LayerFolder) && f.valid?)
      __update('folder' => f && f.uid)
      f
    end
    include Comparable
    # l'autre côté passe par la conversion en étiquette (un nom, nil pour
    # l'étiquette par défaut ; le reste lève ArgumentError) ; les noms affichés se comparent
    # sans la casse.
    def <=>(other)
      o = Sketchup.active_model.layers.__tag_arg(other)
      display_name.downcase <=> o.display_name.downcase
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
    def entityID; Esq.object_pid(:tag, @uid); end
    alias persistent_id entityID
    # le dossier qui le contient, sinon le gestionnaire des étiquettes.
    def parent; folder || Sketchup.active_model.layers; end
    def erase!; raise NoMethodError, "undefined method `erase!' for #{inspect}"; end

    # Un dossier, comme les méthodes de SketchUp le convertissent.
    def self.__arg(f)
      Esq::Conv.wrong_type('LayerFolder') unless f.is_a?(LayerFolder)
      Esq::Conv.deleted('LayerFolder') unless f.valid?
      f
    end

    # Un argument « dossier existant ou nom d'un nouveau » (commun à
    # Layers#add_folder et LayerFolder#add_folder) : un dossier existant y est déplacé.
    def self.__add_to(parent_uid, arg)
      unless arg.is_a?(String) || (arg.is_a?(LayerFolder) && arg.valid?)
        raise TypeError, 'Must provide an existing Sketchup::LayerFolder or a name for a new folder'
      end
      if arg.is_a?(String)
        raise ArgumentError, 'Folder name cannot be empty' if arg.empty?
        return __notify(:onLayerFolderAdded, __for(Esq.call('folder.add', 'name' => arg, 'parent' => parent_uid)))
      end
      arg.__update('parent' => parent_uid)
      arg
    end

    def name; __info['name']; end
    # une chaîne (rb_unexpected_type sinon), non vide ; les noms en double sont
    # permis.
    def name=(n)
      Esq::Conv.str!(n)
      raise ArgumentError, 'Folder names cannot be empty.' if n.empty?
      __update('name' => n)
      n
    end
    alias display_name name
    # Le drapeau du dossier lui-même : ses étiquettes et ses sous-dossiers
    # gardent le leur.
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
      raise TypeError, 'parent must be a Sketchup::LayerFolder or nil' unless f.nil? || (f.is_a?(LayerFolder) && f.valid?)
      __update('parent' => f && f.uid)
      f
    end

    def add_folder(arg)
      __info
      LayerFolder.__add_to(@uid, arg)
    end
    # un dossier enfant direct ; ce qu'il contenait remonte dans celui-ci.
    def remove_folder(f)
      LayerFolder.__arg(f)
      raise ArgumentError, 'folder is not a child' unless f.folder == self
      Esq.call('folder.remove', 'uid' => f.uid)
      LayerFolder.__notify(:onLayerFolderRemoved, f)
      nil
    end

    # LayersObserver#onLayerFolderAdded / onLayerFolderRemoved (layers, folder).
    def self.__notify(event, folder)
      layers = Sketchup.active_model.layers
      Esq.emit(layers.__observers, event, layers, folder)
      folder
    end

    # Parcours d'une liste qui peut changer pendant qu'on la parcourt : le
    # nombre est relu à chaque tour ; s'il a baissé sous l'index courant, avertissement et arrêt.
    def self.__each_live(list)
      n = list.call.size
      i = 0
      while i < n
        cur = list.call
        if cur.size <= i
          warn "don't modify the collection while iterating"
          break
        end
        yield cur[i]
        i += 1
      end
      nil
    end

    def folders; Esq.call('folder.list').select { |f| f['parent'] == @uid }.map { |f| LayerFolder.__for(f) }; end
    def count_folders; folders.size; end
    def each_folder(&b)
      return to_enum(:each_folder) unless b
      LayerFolder.__each_live(-> { folders }, &b)
    end

    # une étiquette (ArgumentError sinon), pas l'étiquette par défaut.
    def add_layer(layer)
      raise ArgumentError, 'argument must be a Sketchup::Layer' unless layer.is_a?(Layer) && layer.valid?
      layer.folder = self
      nil
    end
    # l'étiquette revient à la racine.
    def remove_layer(layer)
      raise ArgumentError, 'argument must be a Sketchup::Layer' unless layer.is_a?(Layer) && layer.valid?
      raise ArgumentError, 'layer is not a child' unless layer.folder == self
      layer.folder = nil
      nil
    end
    def layers; Esq.call('layer.list').select { |l| l['folder'] == @uid }.map { |l| Layer.__for(l) }; end
    def count_layers; layers.size; end
    alias length count_layers
    alias size count_layers
    def each_layer(&b)
      return to_enum(:each_layer) unless b
      LayerFolder.__each_live(-> { layers }, &b)
    end
    alias each each_layer
    include Enumerable

    # nil pour ce qui n'est pas un dossier valide ; les noms se comparent sans
    # la casse.
    def <=>(other)
      return nil unless other.is_a?(LayerFolder) && other.valid?
      name.downcase <=> other.name.downcase
    end
    def ==(o); equal?(o) || (o.is_a?(LayerFolder) && o.uid == @uid && o.valid?); end
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
    # Tous les dossiers, à toute profondeur.
    def __all_folders; Esq.call('folder.list').map { |f| LayerFolder.__for(f) }; end
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

    # L'étiquette d'un nom : « Layer0 », sans la casse, est celle par défaut,
    # qui répond aussi à son nom affiché « Untagged ».
    def __named(name)
      list = __list
      i = name.casecmp?(DEFAULT_NAME) || name == 'Untagged' ? list[0] : list.find { |l| l['name'] == name }
      i && Layer.__for(i)
    end

    # un nom (nil s'il n'existe pas), sinon un index (rb_num2long), négatif
    # depuis la fin.
    def [](k)
      return __named(k) if k.is_a?(String)
      list = __list
      i = Esq::Conv.num2long(k)
      j = i < 0 ? i + list.size : i
      raise IndexError, "index #{j} out of range [0, #{list.size})" if j < 0 || j >= list.size
      Layer.__for(list[j])
    end
    alias at []

    # l'étiquette de ce nom si elle existe, sinon une nouvelle.
    def add(name)
      raise ArgumentError, 'tag name required' unless name.is_a?(String) && !name.empty?
      found = __named(name)
      return found if found
      i = Esq.call('layer.add', 'name' => name)
      l = Layer.__for(i)
      Esq.emit(@observers, :onLayerAdded, self, l) if i['created']
      l
    end
    alias add_layer add

    # un index, sinon la conversion en étiquette (un nom, nil = l'étiquette par
    # défaut ; ArgumentError pour le reste) ; un 2e argument vrai efface aussi la géométrie.
    def remove(*args)
      raise ArgumentError, 'must have 1 or 2 arguments' unless (1..2).cover?(args.size)
      target, del = args
      if target.is_a?(Integer)
        list = __list
        j = target < 0 ? target + list.size : target
        raise IndexError, "index #{j} out of range [0, #{list.size})" if j < 0 || j >= list.size
        target = Layer.__for(list[j])
      else
        target = __tag_arg(target)
      end
      raise ArgumentError, 'cannot remove default tag' if target == self[0]
      raise ArgumentError, "tag doesn't belong to this model" unless target.valid?
      Esq.call('layer.remove', 'uid' => target.uid, 'delete_geometry' => del ? true : false)
      Esq.emit(@observers, :onLayerRemoved, self, target)
      true
    end
    alias remove_layer remove

    # le nombre d'étiquettes retirées.
    def purge_unused
      Esq.call('layer.purge')
    end
    alias purge_unused_layers purge_unused

    # « Tag » par défaut ; un nom libre est rendu tel quel, un nom pris (ou
    # « Layer0 ») reçoit le premier numéro libre.
    def unique_name(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      base = args.empty? ? 'Tag' : Esq.str_arg(args[0])
      Esq.call('layer.unique_name', 'name' => base)
    end

    def layers; __list.select { |l| l['folder'].nil? }.map { |l| Layer.__for(l) }; end
    def count_layers; __list.count { |l| l['folder'].nil? }; end
    def each_layer(&b)
      return to_enum(:each_layer) unless b
      LayerFolder.__each_live(-> { layers }, &b)
    end

    def folders; Esq.call('folder.list').select { |f| f['parent'].nil? }.map { |f| LayerFolder.__for(f) }; end
    def count_folders; folders.size; end
    def each_folder(&b)
      return to_enum(:each_folder) unless b
      LayerFolder.__each_live(-> { folders }, &b)
    end
    def add_folder(arg); LayerFolder.__add_to(nil, arg); end
    # un dossier de la racine ; ce qu'il contenait remonte à la racine.
    def remove_folder(f)
      LayerFolder.__arg(f)
      raise ArgumentError, 'folder is not a child' unless f.folder.nil?
      Esq.call('folder.remove', 'uid' => f.uid)
      LayerFolder.__notify(:onLayerFolderRemoved, f)
      nil
    end
    # le nombre de dossiers retirés.
    def purge_unused_folders; Esq.call('folder.purge'); end

    def dump; each { |l| $stdout.puts("#{l.name} (#{l.visible? ? 'visible' : 'hidden'})") }; nil; end
    def add_observer(obs); @observers << obs unless @observers.include?(obs); true; end
    def remove_observer(obs); !@observers.delete(obs).nil?; end
  end

  # ------------------------------------------------------------------ ombres et options
  module ::Esq
    # The name an option or a provider is asked by: a String as is, otherwise an index read by
    # rb_num2long (TypeError for nil), negative from the end; nil past either end.
    module OptionKey
      def self.of(key, names)
        return key if key.is_a?(String)
        i = Esq::Conv.num2long(key)
        i += names.size if i.negative?
        i.negative? || i >= names.size ? nil : names[i]
      end
    end
  end

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

    # a name, or an index by rb_num2long (negative from the end).
    def [](key); __data[Esq::OptionKey.of(key, keys)]; end
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
    # a name, or an index by rb_num2long (negative from the end).
    def [](key)
      k = Esq::OptionKey.of(key, keys)
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
