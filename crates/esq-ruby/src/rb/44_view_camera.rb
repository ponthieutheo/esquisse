# encoding: utf-8
# Esquisse — 44_view_camera : View, Camera, Page(s), Style(s), RenderingOptions, ShadowInfo,
# InputPoint, PickHelper, Snap, Space, HLR, Environment(s) (la vue).
#
# La caméra et la fenêtre 3D vivent dans l'application (hôte : `host.view.*`) ; les scènes, les
# ombres et l'état libre du modèle (styles, environnements, options de rendu) vivent dans le
# modèle (ponts `scene.*`, `shadow.*`, `vx.*`). Les messages d'erreur et les valeurs de retour
# reprennent ceux de SketchUp.

module Esq
  class << self
    # Appelle l'hôte ; `fallback` si l'application n'est pas là (script lancé sans fenêtre).
    def host(op, args = {}, fallback = nil)
      call("host.#{op}", args)
    rescue RuntimeError => e
      raise unless e.message.to_s.start_with?("l'application n'est pas disponible")
      fallback
    end

    # Équivalent de rb_warn : avertissement sur la console Ruby, sans exception.
    def warn_su(msg)
      $stderr.puts("warning: #{msg}")
      nil
    end

    def vx_get(key, default = nil)
      v = call('vx.get', 'k' => key)
      v.nil? ? default : v
    end

    def vx_set(key, value)
      call('vx.set', 'k' => key, 'v' => value)
    end

    def num!(v)
      return v.to_f if v.is_a?(Numeric)
      raise TypeError, "can't convert #{v.nil? ? 'nil' : v.class} into Float"
    end

    def int!(v)
      return v.to_i if v.is_a?(Numeric)
      raise TypeError, "no implicit conversion of #{v.nil? ? 'nil' : v.class} into Integer"
    end

    def point_a(v)
      raise TypeError, 'Cannot convert argument to Point3d' unless v.respond_to?(:to_a) && v.to_a.size >= 3 && v.to_a.first(3).all? { |c| c.is_a?(Numeric) }
      v.to_a.first(3).map(&:to_f)
    end
  end
end

# ---------------------------------------------------------------- constantes globales
GL_POINTS = 0 unless defined?(GL_POINTS)
GL_LINES = 1 unless defined?(GL_LINES)
GL_LINE_LOOP = 2 unless defined?(GL_LINE_LOOP)
GL_LINE_STRIP = 3 unless defined?(GL_LINE_STRIP)
GL_TRIANGLES = 4 unless defined?(GL_TRIANGLES)
GL_TRIANGLE_STRIP = 5 unless defined?(GL_TRIANGLE_STRIP)
GL_TRIANGLE_FAN = 6 unless defined?(GL_TRIANGLE_FAN)
GL_QUADS = 7 unless defined?(GL_QUADS)
GL_QUAD_STRIP = 8 unless defined?(GL_QUAD_STRIP)
GL_POLYGON = 9 unless defined?(GL_POLYGON)
MK_LBUTTON = 1 unless defined?(MK_LBUTTON)
MK_RBUTTON = 2 unless defined?(MK_RBUTTON)
MK_SHIFT = 4 unless defined?(MK_SHIFT)
MK_CONTROL = 8 unless defined?(MK_CONTROL)
MK_MBUTTON = 0x10 unless defined?(MK_MBUTTON)
MK_ALT = 0x20 unless defined?(MK_ALT)
MK_COMMAND = 0 unless defined?(MK_COMMAND)
PAGE_USE_CAMERA = 1
PAGE_USE_RENDERING_OPTIONS = 2
PAGE_USE_SHADOWINFO = 4
PAGE_USE_SKETCHCS = 8
PAGE_USE_HIDDEN = 0x10
PAGE_USE_LAYER_VISIBILITY = 0x20
PAGE_USE_SECTION_PLANES = 0x40
PAGE_USE_HIDDEN_GEOMETRY = 0x80
PAGE_USE_HIDDEN_OBJECTS = 0x100
PAGE_USE_ENVIRONMENT = 0x200
PAGE_USE_ALL = 0xfff
PAGE_NO_CAMERA = 0xffe

module Sketchup
  # ================================================================ Camera
  class Camera
    # Ce que lisent/écrivent les caméras « liées » (celle de la vue, celle d'une scène) : une
    # table {eye, target, up, persp, fov, height, aspect, image_width, desc, is2d, c2d, s2d}
    # en pouces et en degrés. Une caméra libre (Camera.new) garde ses valeurs ici.
    EPS = 0.001

    def self.deserialize(str)
      raise TypeError, 'wrong argument type (expected String)' unless str.is_a?(String)
      h = begin
        JSON.parse(str)
      rescue StandardError
        nil
      end
      unless h.is_a?(Hash) && h['eye'].is_a?(Array) && h['target'].is_a?(Array) && h['up'].is_a?(Array)
        raise 'Failed to deserialize camera from string.'
      end
      c = new
      c.__load(h)
      c
    end

    # Caméra liée à la vue (:view) ou à une scène ([:page, uid]).
    def self.__linked(src)
      c = new
      c.instance_variable_set(:@src, src)
      c.__pull
      c
    end

    def initialize(*args)
      @src = nil
      @eye = [0.0, 0.0, 1.0]
      @target = [0.0, 0.0, 0.0]
      @up = [0.0, 1.0, 0.0]
      @persp = true
      @fov = 30.0
      @height = 1.0
      @aspect = 0.0
      @image_width = 0.0
      @desc = ''
      @is2d = false
      @c2d = [0.0, 0.0]
      @s2d = 1.0
      set(*args) unless args.empty?
    end

    # ---- liaison
    def __pull
      return unless @src
      h = @src == :view ? Esq.host('view.camera_get', {}, nil) : Esq.page_camera(@src[1])
      __load(h) if h
    end

    def __push
      return unless @src
      if @src == :view
        Esq.host('view.camera_set', __dump)
      else
        Esq.page_camera_set(@src[1], __dump)
      end
    end

    def __load(h)
      @eye = h['eye'].first(3).map(&:to_f)
      @target = h['target'].first(3).map(&:to_f)
      @up = h['up'].first(3).map(&:to_f)
      @persp = h.fetch('persp', true) ? true : false
      @fov = h.fetch('fov', 30.0).to_f
      @height = h.fetch('height', 1.0).to_f
      @aspect = h.fetch('aspect', 0.0).to_f
      @image_width = h.fetch('image_width', 0.0).to_f
      @desc = h.fetch('desc', '').to_s
      @is2d = h.fetch('is2d', false) ? true : false
      @c2d = (h['c2d'] || [0.0, 0.0]).first(2).map(&:to_f)
      @s2d = h.fetch('s2d', 1.0).to_f
    end

    def __dump
      { 'eye' => @eye, 'target' => @target, 'up' => @up, 'persp' => @persp, 'fov' => @fov, 'height' => @height,
        'aspect' => @aspect, 'image_width' => @image_width, 'desc' => @desc, 'is2d' => @is2d, 'c2d' => @c2d, 's2d' => @s2d }
    end

    # ---- lecture
    def eye; __pull; Geom::Point3d.new(*@eye); end
    def target; __pull; Geom::Point3d.new(*@target); end
    def up; __pull; Geom::Vector3d.new(*@up); end

    def direction
      __pull
      Geom::Vector3d.new(*__dir)
    end

    def xaxis
      __pull
      Geom::Vector3d.new(*__norm(__cross(__dir, @up)))
    end

    def yaxis
      __pull
      x = __norm(__cross(__dir, @up))
      Geom::Vector3d.new(*__norm(__cross(x, __dir)))
    end

    def zaxis
      __pull
      Geom::Vector3d.new(*__dir)
    end

    def perspective?; __pull; @persp; end

    def fov
      __pull
      Esq.warn_su('fov is only valid for perspective cameras') unless @persp
      @fov
    end

    def fov_is_height?; __pull; @aspect < EPS; end

    def focal_length
      __pull
      Esq.warn_su('focal_length is only valid for perspective cameras') unless @persp
      __focal
    end

    def image_width
      __pull
      Esq.warn_su('image_width is only valid for perspective cameras') unless @persp
      @image_width
    end

    def height
      __pull
      Esq.warn_su('height is not valid for perspective views') if @persp
      @height
    end

    def aspect_ratio; __pull; @aspect; end
    def description; __pull; @desc.dup; end
    def is_2d?; __pull; @is2d; end
    def center_2d; __pull; Geom::Point3d.new(@c2d[0], @c2d[1], 0.0); end
    def scale_2d; __pull; @s2d; end
    def serialize; __pull; JSON.generate(__dump); end

    # ---- écriture
    def set(*args)
      raise ArgumentError, 'Eye, Target and Up are required' if args.size < 3
      eye = Esq.point_a(args[0])
      t = args[1]
      target = t.is_a?(Geom::Vector3d) ? [eye[0] + t.x, eye[1] + t.y, eye[2] + t.z] : Esq.point_a(t)
      up = Esq.point_a(args[2])
      dir = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]]
      if __len(__cross(dir, up)) < EPS
        if dir.all? { |c| c.abs < EPS } && __len(dir) < EPS
          raise ArgumentError, 'Eye and target points must be different'
        end
        raise ArgumentError, 'Up vector cannot be parallel to view direction'
      end
      __pull
      @eye = eye
      @target = target
      @up = __norm(up)
      __set_persp(args[3] ? true : false) if args.size > 3
      if args.size > 4
        v = Esq.num!(args[4])
        if @persp
          @fov = v.clamp(1.0, 120.0)
        else
          @height = v
        end
      end
      __push
      self
    end

    def fov=(value)
      __pull
      if !@persp
        Esq.warn_su('fov= is only valid for perspective views')
      else
        v = Esq.num!(value)
        if v < 1.0 || v > 120.0
          Esq.warn_su('fov must be between 1 and 120 degrees')
          v = v.clamp(1.0, 120.0)
        end
        @fov = v
        __push
      end
      value
    end

    def focal_length=(value)
      __pull
      if !@persp
        Esq.warn_su('focal_length= is only valid for perspective views')
      else
        v = Esq.num!(value)
        if v < 1.0 || v > 3000.0
          Esq.warn_su('focal_length must be between 1 and 3000 mm')
          v = v.clamp(1.0, 3000.0)
        end
        w = @image_width < EPS ? 36.0 : @image_width
        @fov = 2.0 * Math.atan((w * 0.5) / v) * 180.0 / Math::PI
        __push
      end
      value
    end

    def image_width=(value)
      __pull
      if !@persp
        Esq.warn_su('image_width= is only valid for perspective views')
      else
        v = Esq.num!(value)
        if v < 0.0
          Esq.warn_su('image_width must be greater than 0.0 mm')
          v = 0.0
        end
        @image_width = v
        __push
      end
      value
    end

    def height=(value)
      __pull
      if @persp
        Esq.warn_su('height= is not valid for perspective views')
      else
        v = Esq.num!(value)
        if v < EPS
          Esq.warn_su('Height too small')
          v = EPS
        end
        @height = v
        __push
      end
      value
    end

    def aspect_ratio=(value)
      __pull
      v = Esq.num!(value)
      v = 0.0 if v < 0.0
      if (v - @aspect).abs >= EPS
        @aspect = v
        __push
      end
      value
    end

    def perspective=(value)
      __pull
      __set_persp(value ? true : false)
      __push
      value
    end

    def description=(value)
      __pull
      @desc = value.to_s
      __push
      value
    end

    def ==(o)
      return false unless o.is_a?(Camera)
      __dump == o.__dump
    end

    def inspect
      "#<Sketchup::Camera:0x#{(object_id * 8).to_s(16).rjust(16, '0')}>"
    end

    private

    def __dir; __norm([@target[0] - @eye[0], @target[1] - @eye[1], @target[2] - @eye[2]]); end
    def __len(v); Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]); end
    def __cross(a, b); [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]; end

    def __norm(v)
      l = __len(v)
      l.zero? ? [0.0, 0.0, 0.0] : [v[0] / l + 0.0, v[1] / l + 0.0, v[2] / l + 0.0]
    end

    def __focal
      return 0.0 unless @persp
      w = @image_width < EPS ? 36.0 : @image_width
      return 0.0 if @fov >= 180.0
      (w * 0.5) / Math.tan(@fov * Math::PI / 180.0 * 0.5)
    end

    # Passer de la perspective au parallèle (et inversement) garde l'image : la hauteur visible
    # à la cible remplace le champ de vision, ou l'œil recule pour retrouver ce champ.
    def __set_persp(on)
      return if on == @persp
      k = 2.0 * Math.tan(@fov * Math::PI / 180.0 * 0.5)
      d = [@target[0] - @eye[0], @target[1] - @eye[1], @target[2] - @eye[2]]
      if @persp
        @height = __len(d) * k
        @is2d = false
      else
        dir = __norm(d)
        dist = @height / k
        @eye = [@target[0] - dir[0] * dist, @target[1] - dir[1] * dist, @target[2] - dir[2] * dist]
      end
      @persp = on
    end
  end

end

module Esq
  class << self
    def page_camera(uid)
      s = call('scene.list').find { |x| x['uid'] == uid } or raise TypeError, 'reference to deleted Camera'
      c = s['camera']
      extra = (s['props'] || {})['cam'] || {}
      { 'eye' => c['eye'], 'target' => c['target'], 'up' => c['up'], 'persp' => c['persp'], 'fov' => c['fov'],
        'height' => extra.fetch('height', 1.0), 'aspect' => extra.fetch('aspect', 0.0),
        'image_width' => extra.fetch('image_width', 0.0), 'desc' => extra.fetch('desc', ''),
        'is2d' => extra.fetch('is2d', false), 'c2d' => extra.fetch('c2d', [0.0, 0.0]), 's2d' => extra.fetch('s2d', 1.0) }
    end

    def page_camera_set(uid, h)
      call('scene.set', 'uid' => uid,
                        'camera' => { 'eye' => h['eye'], 'target' => h['target'], 'up' => h['up'], 'fov' => h['fov'], 'persp' => h['persp'] },
                        'props' => { 'cam' => { 'height' => h['height'], 'aspect' => h['aspect'], 'image_width' => h['image_width'],
                                                'desc' => h['desc'], 'is2d' => h['is2d'], 'c2d' => h['c2d'], 's2d' => h['s2d'] } })
    end
  end
end

module Esq
  class << self
    # Classes dont SketchUp retire l'allocateur (rb_undef_alloc_func) : pas de `new`.
    def undef_alloc(klass)
      klass.singleton_class.send(:define_method, :new) do |*|
        raise TypeError, "allocator undefined for #{klass.name}"
      end
    end

    def view_size
      Esq.host('view.size', {}, nil) || { 'w' => 1280, 'h' => 800, 'dw' => 1280, 'dh' => 800 }
    end

    def color_a(c)
      c = Sketchup::Color.new(c) unless c.is_a?(Sketchup::Color)
      c.to_a
    end

    # Les options d'un texte de la vue (draw_text, text_bounds), vérifiées comme SketchUp:
    #  un Hash ; font une chaîne de moins de 32 caractères ; un seul de size,
    # pixel_size et point_size, entier ; align dans 0..2, vertical_align dans 0..3 ; color une
    # couleur. Rend les options aux clés en symboles.
    def text_options(options)
      return {} if options.nil?
      raise TypeError, "wrong argument type #{options.class} (expected Hash)" unless options.is_a?(Hash)
      o = options.to_h { |k, v| [k.to_sym, v] }
      if o.key?(:font) && Esq.str_arg(o[:font]).length >= 32
        raise ArgumentError, 'Font names must be less than 32 characters in length'
      end
      sizes = %i[size pixel_size point_size].select { |k| o.key?(k) }
      raise ArgumentError, "Cannot mix 'size', 'pixel_size' and  'point_size'" if sizes.size > 1
      sizes.each { |k| Esq.num2int(o[k]) }
      if o.key?(:align) && !(0..2).cover?(Esq.num2int(o[:align]))
        raise ArgumentError, 'invalid text alignment'
      end
      if o.key?(:vertical_align) && !(0..3).cover?(Esq.num2int(o[:vertical_align]))
        raise ArgumentError, 'invalid vertical text alignment'
      end
      if o.key?(:color) && ![Sketchup::Color, String, Integer, Array].any? { |k| o[:color].is_a?(k) }
        raise TypeError, "wrong argument type #{o[:color].class} (expected Sketchup::Color)"
      end
      o
    end

    def points_a(args)
      list = args.size == 1 && args[0].is_a?(Array) && !args[0].empty? && !args[0][0].is_a?(Numeric) ? args[0] : args
      list.map { |p| point_a(p) }
    end

    # Ce que View#draw prend pour un point : un Point3d ou un tableau de deux ou
    # trois nombres.
    def draw_point?(v)
      v.is_a?(Geom::Point3d) || (v.is_a?(Array) && (2..3).cover?(v.size) && v.all? { |c| c.is_a?(Numeric) })
    end

    def draw_point(v)
      a = v.to_a
      [a[0].to_f, a[1].to_f, (a[2] || 0).to_f]
    end

    # La conversion de point des arguments de SketchUp : un tableau de deux ou
    # trois nombres, un Point3d, un sommet ou un point de saisie ; sinon ArgumentError.
    def sketchup_point(v)
      case v
      when Geom::Point3d then v.to_a
      when Array
        raise ArgumentError, 'wrong number of values in array' unless (2..3).cover?(v.size)
        [Esq.num2dbl(v[0]), Esq.num2dbl(v[1]), v.size == 3 ? Esq.num2dbl(v[2]) : 0.0]
      when Sketchup::Vertex, Sketchup::InputPoint then v.position.to_a
      else raise ArgumentError, 'Cannot convert argument to Geom::Point3d'
      end
    end

    # Ids of the textures loaded by View#load_texture (one table for every view, as in SketchUp).
    def view_textures; @view_textures ||= {}; end
  end
end

module Sketchup
  # ================================================================ View
  class View
    CORNER_TOP_LEFT = 0
    CORNER_TOP_RIGHT = 1
    CORNER_BOTTOM_LEFT = 2
    CORNER_BOTTOM_RIGHT = 3

    Esq.undef_alloc(self)

    def self.__instance
      @__instance ||= allocate
    end

    def model; Sketchup.active_model; end

    # ---- taille de la fenêtre 3D
    def vpwidth; Esq.view_size['w'].to_f; end
    def vpheight; Esq.view_size['h'].to_f; end
    def device_width; Esq.view_size['dw'].to_i; end
    def device_height; Esq.view_size['dh'].to_i; end

    def center
      s = Esq.view_size
      [(s['w'].to_i / 2).to_f, (s['h'].to_i / 2).to_f]
    end

    def corner(index)
      i = Esq.int!(index)
      raise IndexError, 'index out of range [0,3]' unless i >= 0 && i < 4
      s = Esq.view_size
      x = (i & 1).zero? ? 0 : s['w'].to_i
      y = (i & 2).zero? ? 0 : s['h'].to_i
      [x.to_f, y.to_f]
    end

    # ---- caméra
    def camera; Camera.__linked(:view); end

    def camera=(value)
      if value.is_a?(Array)
        raise ArgumentError, 'Wrong number of arguments: 2 expected' if value.size < 2
        if value[0].is_a?(Camera)
          Esq.num!(value[1])
          Esq.host('view.camera_set', value[0].__dump)
        else
          cam = camera
          cam.set(*value)
        end
      elsif value.is_a?(Camera)
        Esq.host('view.camera_set', value.__dump)
      else
        raise ArgumentError, 'Invalid Camera'
      end
      value
    end

    def field_of_view
      cam = camera
      cam.__dump['fov']
    end

    def field_of_view=(value)
      v = Esq.num!(value)
      cam = camera
      if cam.perspective?
        h = cam.__dump
        h['fov'] = v.clamp(1.0, 120.0)
        Esq.host('view.camera_set', h)
      end
      value
    end

    def zoom_extents
      Esq.host('view.zoom_extents')
      self
    end

    def zoom(*args)
      if args.size == 1 && args[0].is_a?(Float)
        f = args[0]
        return nil if f < 1.0e-6
        Esq.host('view.zoom', 'factor' => f)
      else
        items = args.size == 1 ? args[0] : args
        items = [items] unless items.is_a?(Array)
        bb = Geom::BoundingBox.new
        items.flatten.each do |e|
          if e.respond_to?(:bounds)
            bb.add(e.bounds)
          elsif e.respond_to?(:to_a) && e.to_a.first.is_a?(Numeric)
            bb.add(e)
          end
        end
        unless bb.empty?
          Esq.host('view.zoom_box', 'min' => bb.min.to_a, 'max' => bb.max.to_a)
        end
      end
      self
    end

    # ---- projection
    def screen_coords(point)
      r = Esq.host('view.screen_coords', 'pt' => Esq.point_a(point))
      r && Geom::Point3d.new(*r)
    end

    def pickray(*args)
      args = args[0] if args.size == 1 && args[0].is_a?(Array)
      raise ArgumentError, 'Wrong number of arguments: x and y required' unless args.size == 2
      x, y = Esq.num!(args[0]), Esq.num!(args[1])
      r = Esq.host('view.pickray', 'x' => x, 'y' => y)
      return [Geom::Point3d.new(0, 0, 0), Geom::Vector3d.new(0, 0, 1)] unless r
      [Geom::Point3d.new(*r['origin']), Geom::Vector3d.new(*r['dir'])]
    end

    def pixels_to_model(pixels, point)
      px = Esq.num!(pixels).round
      s = Esq.host('view.pixel_size', 'pt' => Esq.point_a(point), 'px' => px)
      s.nil? ? 0.0 : s.to_f
    end

    def guess_target(*args)
      pt = nil
      if args.size == 1 && args[0].respond_to?(:to_a)
        a = args[0].to_a
        pt = [a[0].to_f, a[1].to_f]
      elsif args.size >= 2
        pt = [Esq.num!(args[0]), Esq.num!(args[1])]
      end
      r = Esq.host('view.guess_target', 'x' => pt && pt[0], 'y' => pt && pt[1])
      r ? Geom::Point3d.new(*r) : camera.target
    end

    # ---- picking et inférences
    def inputpoint(*args)
      raise ArgumentError, 'Wrong number of arguments: x and y required' if args.size < 2
      ip = InputPoint.new
      ip.pick(self, args[0], args[1], args[2])
      ip
    end

    def pick_helper(*args)
      return PickHelper.__new(self) if args.empty?
      raise ArgumentError, 'Wrong number of arguments: x and y required' if args.size == 1
      ph = PickHelper.__new(self)
      ph.do_pick(*args)
      ph
    end

    def lock_inference(*args)
      case args.size
      when 0
        @lock = nil
        false
      when 1, 2
        ips = args.first(2)
        ips.each do |ip|
          raise TypeError, 'wrong argument type (expected Sketchup::InputPoint)' unless ip.is_a?(InputPoint)
        end
        return false unless ips.all?(&:valid?)
        @lock = ips.map { |ip| ip.clone }
        true
      else
        raise ArgumentError, 'Wrong number of arguments: 0, 1 or 2'
      end
    end

    def inference_locked?; !@lock.nil?; end
    def __lock; @lock; end

    # ---- dessin (dans les rappels draw d'un outil)
    # SketchUp: the points, then options `normals:`, `uvs:`
    # (one per point) and `texture:` (an id of #load_texture, used with uvs only).
    def draw(mode, *points)
      __draw(mode, points, false)
    end

    def draw2d(mode, *points)
      __draw(mode, points, true)
    end

    # Comme SketchUp : le mode et au moins un argument de
    # points ; le nombre de points est vérifié tel qu'il est donné, puis les éléments qui ne sont
    # pas des points (un Point3d ou un tableau de nombres) sont ignorés, de même
    # que les normales et les uvs qui ne se convertissent pas.
    def __draw(mode, points, two_d)
      opts = points.size > 1 && points.last.is_a?(Hash) ? points.pop : nil
      raise ArgumentError, "wrong number of arguments (given #{points.size + 1}, expected 2+)" if points.empty?
      m = Esq.int!(mode)
      list = points.size == 1 && points[0].is_a?(Array) && !Esq.draw_point?(points[0]) ? points[0] : points
      __check_count(m, list.size)
      keep = list.each_index.select { |i| Esq.draw_point?(list[i]) }
      pts = keep.map { |i| Esq.draw_point(list[i]) }
      a = { 'mode' => m, 'pts' => pts, 'style' => __style }
      a['two_d'] = true if two_d
      __draw_options(opts, list.size, m, keep).each { |k, v| a[k] = v } if opts
      Esq.host('view.draw', a)
      self
    end
    private :__draw

    # Point counts of each mode, with SketchUp's messages.
    def __check_count(m, n)
      many = ->(k) { raise ArgumentError, "Incorrect number of points: must be multiple of #{k}" }
      least = ->(k) { raise ArgumentError, "Not enough points: at least #{k} required" }
      case m
      when GL_POINTS then nil
      when GL_LINES then many.(2) if n.odd?
      when GL_LINE_LOOP, GL_LINE_STRIP then least.(2) if n < 2
      when GL_TRIANGLES then many.(3) if n % 3 != 0
      when GL_TRIANGLE_STRIP, GL_TRIANGLE_FAN, GL_POLYGON then least.(3) if n < 3
      when GL_QUADS then many.(4) if n % 4 != 0
      when GL_QUAD_STRIP
        least.(4) if n < 4
        many.(2) if n.odd?
      else raise ArgumentError, 'Invalid DrawingHelper drawing mode'
      end
    end
    private :__check_count

    def __draw_options(opts, n, mode, keep)
      out = {}
      normals = opts[:normals]
      unless normals.nil?
        Esq.unexpected_type(normals, 'Array') unless normals.is_a?(Array)
        raise ArgumentError, "normals must match number of points. Expected #{n}, got #{normals.size}" if normals.size != n
        unless [GL_LINES, GL_LINE_LOOP, GL_LINE_STRIP].include?(mode)
          # Une normale qui n'est pas un vecteur, ou nulle, prend la normale par défaut.
          out['normals'] = keep.map do |i|
            v = normals[i]
            c = v.is_a?(Geom::Vector3d) || Esq.draw_point?(v) ? Esq.draw_point(v) : nil
            c.nil? || c.all?(&:zero?) ? [0.0, 0.0, 1.0] : c
          end
        end
      end
      uvs = opts[:uvs]
      unless uvs.nil?
        Esq.unexpected_type(uvs, 'Array') unless uvs.is_a?(Array)
        raise ArgumentError, "uvs must match number of points. Expected #{n}, got #{uvs.size}" if uvs.size != n
        out['uvs'] = keep.map do |i|
          uv = uvs[i]
          next [0.0, 0.0] unless Esq.draw_point?(uv)
          c = uv.to_a
          q = c.size == 3 && c[2].to_f != 0.0 ? c[2].to_f : 1.0
          [c[0].to_f / q, c[1].to_f / q]
        end
        tex = opts[:texture]
        unless tex.nil?
          id = Esq::Conv.num2long(tex)
          raise ArgumentError, 'invalid texture ID' unless Esq.view_textures.key?(id)
          out['texture'] = id
        end
      end
      out
    end
    private :__draw_options

    def draw_lines(*points)
      raise ArgumentError, 'Wrong number of points: at least two required' if points.size < 2 && !(points.size == 1 && points[0].is_a?(Array))
      pts = Esq.points_a(points)
      raise ArgumentError, 'Wrong number of points: at least two required' if pts.size < 2
      Esq.host('view.draw', 'mode' => GL_LINES, 'pts' => pts, 'style' => __style)
      self
    end
    alias draw_line draw_lines

    def draw_polyline(*points)
      pts = Esq.points_a(points)
      raise ArgumentError, 'Wrong number of points: at least two required' if pts.size < 2
      Esq.host('view.draw', 'mode' => GL_LINE_STRIP, 'pts' => pts, 'style' => __style)
      self
    end

    def draw_points(points, size = 6, style = 3, color = nil)
      raise ArgumentError, 'Wrong number of arguments: at least 1 point required' if points.nil?
      pts = Esq.points_a([points])
      st = __style
      st['color'] = Esq.color_a(color) if color
      Esq.host('view.draw_points', 'pts' => pts, 'size' => Esq.int!(size), 'kind' => Esq.int!(style), 'style' => st)
      self
    end

    def draw_text(point, text, options = {})
      opts = {}
      Esq.text_options(options).each { |k, v| opts[k.to_s] = v.is_a?(Sketchup::Color) ? v.to_a : v }
      Esq.host('view.draw_text', 'pt' => Esq.sketchup_point(point), 'text' => text.to_s, 'options' => opts, 'style' => __style)
      self
    end

    # La boîte du texte que dessinerait draw_text (tailles estimées), placée selon align
    # (TextAlignLeft/Center/Right : le point à gauche, au milieu, à droite) et vertical_align
    # (TextVerticalAlignBoundsTop/Baseline/CapHeight/Center : le point en haut, sur la ligne de
    # base, à la hauteur des capitales, au milieu).
    def text_bounds(point, text, options = {})
      opts = Esq.text_options(options)
      size = (opts[:size] || opts[:pixel_size] || opts[:point_size] || 12).to_f
      lines = text.to_s.split("\n", -1)
      lines = [''] if lines.empty?
      w = (lines.map(&:length).max * size * 0.55).ceil
      h = (lines.size * size * 1.25).ceil
      # Like draw_text, the point is already in screen pixels.
      x, y = Esq.sketchup_point(point)
      x -= case opts[:align].to_i
           when TextAlignCenter then w / 2.0
           when TextAlignRight then w
           else 0
           end
      y -= case opts[:vertical_align].to_i
           when TextVerticalAlignBaseline then size * 0.9
           when TextVerticalAlignCapHeight then size * 0.7
           when TextVerticalAlignCenter then h / 2.0
           else 0
           end
      Geom::Bounds2d.new(x, y, w, h)
    end

    def drawing_color=(color)
      @color = Esq.color_a(color)
      color
    end

    def line_width=(width)
      w = Esq.num!(width)
      raise ArgumentError, 'Line width must be positive' if w < 0.0
      @line_width = w
      width
    end

    def line_stipple=(stipple)
      s = stipple.to_s
      @stipple = s
      stipple
    end

    def set_color_from_line(*args)
      raise ArgumentError, 'Wrong number of arguments: 2 points required' if args.size < 2
      a, b = Esq.point_a(args[0]), Esq.point_a(args[1])
      d = [(b[0] - a[0]).abs, (b[1] - a[1]).abs, (b[2] - a[2]).abs]
      tol = 1.0e-4 * [d.max, 1.0].max
      axis = d.each_index.select { |i| d[i] > tol }
      @color = case axis
               when [0] then [255, 0, 0, 255]
               when [1] then [0, 128, 0, 255]
               when [2] then [0, 0, 255, 255]
               else [0, 0, 0, 255]
               end
      self
    end

    def __style
      { 'color' => @color || [0, 0, 0, 255], 'width' => @line_width || 1.0, 'stipple' => @stipple || '' }
    end

    # ---- textures: the application keeps the pixels and
    # draws them with View#draw(…, texture:, uvs:); ids are those of the application.
    def load_texture(image_rep)
      raise TypeError, 'wrong argument type' unless image_rep.is_a?(Sketchup::ImageRep)
      raise ArgumentError, 'invalid ImageRep' if image_rep.width.to_i <= 0 || image_rep.height.to_i <= 0 || image_rep.data.empty?
      id = Esq.host('view.load_texture', image_rep.__fields, nil)
      id ||= (Esq.view_textures.keys.max || 0) + 1
      Esq.view_textures[id] = true
      id
    end

    def release_texture(id)
      id = Esq::Conv.num2long(id)
      return false unless Esq.view_textures.delete(id)
      Esq.host('view.release_texture', { 'id' => id }, nil)
      true
    end

    # ---- rafraîchissement
    def invalidate
      Esq.host('view.invalidate')
      self
    end

    def refresh
      Esq.host('view.invalidate')
      self
    end

    def show_frame(*args)
      raise ArgumentError, 'Wrong number of arguments: 0 or 1' if args.size > 1
      delay = args.empty? ? 0.0 : Esq.num!(args[0])
      Esq.host('view.invalidate')
      sleep(delay) if delay > 0
      self
    end

    def average_refresh_time; (Esq.host('view.refresh_times', {}, nil) || [0.016, 0.016])[1].to_f; end

    def last_refresh_time(*args)
      t = (Esq.host('view.refresh_times', {}, nil) || [0.016, 0.016])[0].to_f
      t
    end

    def tooltip=(text)
      if text.nil?
        Esq.host('view.tooltip', 'text' => nil)
      else
        raise ArgumentError, 'String expected' unless text.is_a?(String)
        Esq.host('view.tooltip', 'text' => text)
      end
      @tooltip = text
      text
    end

    def graphics_engine
      Esq.host('view.graphics_engine', {}, nil) ? :graphics_engine_classic : :graphics_engine_2024
    end

    # ---- animation
    def animation=(anim)
      raise RuntimeError, 'Cannot change the animation in a frame change callback' if @in_frame
      @animation.stop if @animation.respond_to?(:stop)
      UI.stop_timer(@animation_timer) if @animation_timer && UI.respond_to?(:stop_timer)
      @animation_timer = nil
      @animation = anim
      if anim
        anim.onStart(self) if anim.respond_to?(:onStart)
        if UI.respond_to?(:start_timer)
          @animation_timer = UI.start_timer(0.0, true) do
            @in_frame = true
            begin
              keep = anim.respond_to?(:nextFrame) ? anim.nextFrame(self) : false
            ensure
              @in_frame = false
            end
            unless keep
              UI.stop_timer(@animation_timer) if @animation_timer
              @animation_timer = nil
              @animation = nil
              anim.stop if anim.respond_to?(:stop)
            end
          end
        end
      end
      anim
    end

    def dynamic=(value); value; end

    # ---- observateurs (l'émission des événements est de la zone « tools »)
    def add_observer(observer)
      @observers ||= []
      return false if observer.nil? || @observers.any? { |o| o.equal?(observer) }
      @observers << observer
      true
    end

    def remove_observer(observer)
      @observers ||= []
      !@observers.reject! { |o| o.equal?(observer) }.nil?
    end

    def __observers; (@observers ||= []).dup; end

    # ---- image
    def write_image(*args)
      raise ArgumentError, 'Too few arguments. Filename or option hash required' if args.empty?
      raise ArgumentError, 'Too many arguments.' if args.size > 5
      s = Esq.view_size
      w = s['dw'].to_i
      h = s['dh'].to_i
      scale = 1.0
      antialias = false
      compression = 1.0
      transparent = false
      framebuffer = false
      if args[0].is_a?(Hash)
        o = args[0]
        framebuffer = o[:source].to_s == 'framebuffer'
        unless framebuffer
          w = o[:width].to_i if o[:width].is_a?(Integer)
          h = o[:height].to_i if o[:height].is_a?(Integer)
        end
        file = o[:filename]
        if o.key?(:scale_factor) && !o[:scale_factor].nil?
          scale = Esq.num!(o[:scale_factor])
          if scale < 0.1 || scale > 10.0
            raise ArgumentError, format(':scale_factor must be between %f and %f', 0.1, 10.0)
          end
        end
        antialias = o[:antialias] ? true : false
        compression = Esq.num!(o[:compression]) unless o[:compression].nil?
        transparent = o[:transparent] ? true : false
      else
        file = args[0]
        w = Esq.int!(args[1]) if args.size > 1
        h = Esq.int!(args[2]) if args.size > 2
        antialias = args[3] ? true : false if args.size > 3
        compression = Esq.num!(args[4]) if args.size > 4
      end
      unless w > 0 && h > 0 && w <= 16000 && h <= 16000
        raise ArgumentError, 'Invalid width or height value. Valid range is 1 to 16000'
      end
      raise ArgumentError, 'Filename not specified.' if file.nil? || file.to_s.empty?
      r = Esq.host('view.write_image', 'path' => File.expand_path(file.to_s), 'w' => w, 'h' => h, 'scale' => scale,
                                       'antialias' => antialias, 'compression' => compression, 'transparent' => transparent)
      r ? true : false
    end

    def inspect; "#<Sketchup::View:0x#{(object_id * 8).to_s(16).rjust(16, '0')}>"; end
  end

  class Model
    def active_view; View.__instance; end
  end
end

module Esq
  class << self
    # The constraint View#lock_inference sets: [:line, point, vector] for two input points (the
    # line through them) or one on an edge (the edge's line), [:plane, point, normal] for one
    # on a face (the face's plane); nil otherwise.
    def inference_lock(lock)
      return nil if lock.nil? || lock.empty?
      if lock.size == 2
        a, b = lock.map(&:position)
        d = a.vector_to(b)
        return d.length > 1e-9 ? [:line, a, d] : nil
      end
      ip = lock[0]
      t = ip.transformation || Geom::Transformation.new
      if (e = ip.edge)
        p, d = e.line
        [:line, t * p, t * d]
      elsif (f = ip.face)
        [:plane, ip.position, t * f.normal]
      end
    end

    # An inference (InputPoint data) moved onto the locked line or plane: where the pick ray
    # comes closest to the line, or crosses the plane (else the inferred point, projected).
    def locked_inference(view, d, x, y)
      kind, p, v = inference_lock(view.__lock)
      return d unless kind
      pos = Geom::Point3d.new(*d['pos'])
      o, r = view.pickray(x, y)
      w = p - o
      q = if kind == :line
            a = v.dot(v); b = v.dot(r); c = r.dot(r); dd = v.dot(w); e = r.dot(w)
            den = a * c - b * b
            s = den.abs < 1e-12 ? -(pos.vector_to(p)).dot(v) / a : (b * e - c * dd) / den
            p.offset(v, s * v.length)
          else
            den = v.dot(r)
            if den.abs < 1e-12
              pos.project_to_plane([p, v])
            else
              o.offset(r, v.dot(w) / den * r.length)
            end
          end
      d.merge('pos' => q.to_a, 'dof' => kind == :line ? 1 : 2)
    end

    def entity_of(handle)
      return nil if handle.nil?
      Sketchup::Entity.wrap(handle)
    end

    def instance_path_of(handles)
      return nil if handles.nil?
      ents = handles.map { |h| entity_of(h) }
      defined?(Sketchup::InstancePath) ? Sketchup::InstancePath.new(ents) : ents
    end
  end
end

module Sketchup
  # ================================================================ InputPoint
  class InputPoint
    TYPE_ERR = 'wrong argument type (expected Sketchup::InputPoint)'.freeze

    def initialize(*args)
      @d = nil
      return if args.empty?
      a = args[0]
      if a.is_a?(InputPoint)
        @d = a.__data&.dup
      elsif Esq.point_value?(a)
        # un point (Point3d ou tableau de nombres) ou un sommet ; le reste est ignoré.
        @d = { 'pos' => Esq.sketchup_point(a).map(&:to_f), 'kind' => 'free', 'dof' => 3, 'tip' => '', 'path' => [], 'face' => nil, 'edge' => nil }
      elsif a.is_a?(Sketchup::Vertex)
        # Sur un sommet : `vertex` le retrouve par sa position parmi ceux d'un de ses bords.
        e = a.edges.first
        @d = { 'pos' => a.position.to_a, 'kind' => 'vertex', 'dof' => 0, 'tip' => '', 'path' => [], 'face' => nil,
               'edge' => nil, 'vertex' => e && e.handle }
      end
    end

    def __data; @d; end

    def clear; @d = nil; self; end

    def copy!(other)
      raise TypeError, TYPE_ERR unless other.is_a?(InputPoint)
      @d = other.__data&.dup
      self
    end

    def valid?; !@d.nil?; end
    def degrees_of_freedom; @d ? @d['dof'].to_i : 0; end

    def depth
      return nil unless @d
      @d['path'].size + (@d['face'] || @d['edge'] ? 1 : 0)
    end

    def display?; @d ? true : false; end

    def draw(view)
      return nil unless @d
      Esq.host('view.draw_points', 'pts' => [@d['pos']], 'size' => 8, 'kind' => 2, 'style' => { 'color' => [0, 0, 0, 255] }) if view
      self
    end

    def edge; @d && Esq.entity_of(@d['edge']); end
    def face; @d && Esq.entity_of(@d['face']); end

    def vertex
      return nil unless @d && @d['vertex']
      e = Esq.entity_of(@d['vertex'])
      pos = position
      e.vertices.find { |v| v.position == pos }
    end

    # The pick path ends with the element the point lies on (InputPoint+0x48).
    def instance_path; @d && Esq.instance_path_of(@d['path'] + [@d['leaf']].compact); end

    def pick(*args)
      raise ArgumentError, 'Too few arguments: view, x and y required' if args.size < 3
      view = args[0]
      raise ArgumentError, 'Invalid view in InputPoint::pick' unless view.is_a?(View)
      x, y = Esq.num!(args[1]), Esq.num!(args[2])
      ref = args[3]
      ref_pos = nil
      unless ref.nil?
        raise TypeError, TYPE_ERR unless ref.is_a?(InputPoint)
        ref_pos = ref.__data && ref.__data['pos']
      end
      r = begin
        Esq.call('pick.infer', 'x' => x, 'y' => y, 'ref' => ref_pos)
      rescue RuntimeError => e
        raise unless e.message.start_with?("l'application n'est pas disponible")
        nil
      end
      r = Esq.locked_inference(view, r, x, y) if r && view.inference_locked?
      changed = r != @d
      @d = r
      changed
    end

    def position
      @d ? Geom::Point3d.new(*@d['pos']) : Geom::Point3d.new(0, 0, 0)
    end

    def ==(other)
      return false unless other.is_a?(InputPoint)
      return other.__data.nil? if @d.nil?
      return false if other.__data.nil?
      @d['pos'] == other.__data['pos'] && @d['face'] == other.__data['face'] && @d['edge'] == other.__data['edge']
    end

    def tooltip; @d ? @d['tip'].to_s : ''; end

    def transformation
      return nil unless @d
      return Geom::Transformation.new if @d['path'].empty?
      path = @d['path'].map { |h| Esq.entity_of(h) }
      path.map(&:transformation).inject(Geom::Transformation.new) { |a, b| a * b }
    end

    def initialize_copy(src)
      super
      @d = src.__data&.dup
    end
  end

  # ================================================================ PickHelper
  class PickHelper
    PICK_INSIDE = 2
    PICK_CROSSING = 3
    NO_VIEW = 'Invalid PickHelper - no view defined'.freeze

    Esq.undef_alloc(self)

    def self.__new(view)
      ph = allocate
      ph.instance_variable_set(:@view, view)
      ph.instance_variable_set(:@hits, [])
      ph
    end

    def view; @view; end

    def init(*args)
      raise ArgumentError, 'Wrong number of arguments: x and y required' if args.size < 2
      raise RuntimeError, NO_VIEW unless @view
      @pt = [Esq.num!(args[0]), Esq.num!(args[1]), args.size > 2 ? Esq.num!(args[2]) : 0.0]
      @hits = []
      self
    end

    def do_pick(*args)
      raise ArgumentError, 'Wrong number of arguments: x and y required' if args.size < 2
      raise RuntimeError, NO_VIEW unless @view
      init(*args)
      @hits = Esq.call('pick.list', 'x' => @pt[0], 'y' => @pt[1], 'aperture' => @pt[2]) rescue (@hits = [])
      @hits = [] unless @hits.is_a?(Array)
      @hits.size
    end

    def count; @hits.size; end

    def __hit(i)
      i = Esq.int!(i)
      i >= 0 && i < @hits.size ? @hits[i] : nil
    end

    def all_picked; @hits.each_index.map { |i| element_at(i) }; end
    def best_picked; element_at(0); end

    def element_at(i)
      h = __hit(i) or return nil
      Esq.entity_of(h['path'].first || h['leaf'])
    end

    def leaf_at(i)
      h = __hit(i) or return nil
      Esq.entity_of(h['leaf'])
    end

    def path_at(i)
      h = __hit(i) or return nil
      p = (h['path'] + [h['leaf']]).map { |x| Esq.entity_of(x) }
      p.empty? ? nil : p
    end

    def depth_at(i)
      h = __hit(i) or return nil
      h['path'].size + 1
    end

    def transformation_at(i)
      h = __hit(i) or return nil
      h['path'].map { |x| Esq.entity_of(x).transformation }.inject(Geom::Transformation.new) { |a, b| a * b }
    end

    def picked_edge
      h = @hits.find { |x| x['leaf'].to_s.start_with?('e:') }
      h && Esq.entity_of(h['leaf'])
    end

    def picked_face
      h = @hits.find { |x| x['leaf'].to_s.start_with?('f:') }
      h && Esq.entity_of(h['leaf'])
    end

    def picked_element(*args)
      args.empty? ? element_at(0) : element_at(args[0])
    end

    def test_point(*args)
      raise ArgumentError, 'Wrong number of arguments: point required' if args.empty?
      raise RuntimeError, NO_VIEW unless @view
      raise ArgumentError, 'Wrong number of arguments: point, x and y required' if args.size == 2
      pt = Esq.point_a(args[0])
      x, y = args.size > 2 ? [Esq.num!(args[1]), Esq.num!(args[2])] : [@pt && @pt[0], @pt && @pt[1]]
      ap = args.size > 3 ? Esq.num!(args[3]) : (@pt ? @pt[2] : 0.0)
      s = @view.screen_coords(pt)
      return false if s.nil? || x.nil?
      ap = 6.0 if ap <= 0
      Math.hypot(s.x - x, s.y - y) <= ap
    end

    def pick_segment(*args)
      raise ArgumentError, 'Wrong number of arguments: Array of Points required' if args.empty?
      raise RuntimeError, NO_VIEW unless @view
      xy = nil
      if args[0].is_a?(Array) && args[0].first.is_a?(Array) || args[0].is_a?(Array) && args[0].first.respond_to?(:x)
        pts = args[0].map { |p| Esq.point_a(p) }
        rest = args[1..]
      else
        pts = args.take_while { |a| !a.is_a?(Numeric) }.map { |p| Esq.point_a(p) }
        rest = args[pts.size..]
      end
      raise ArgumentError, 'At least two Points are needed.' if pts.size < 2
      unless rest.empty?
        raise ArgumentError, 'Both x and y are needed.' if rest.size < 2
        @pt = [Esq.num!(rest[0]), Esq.num!(rest[1]), rest.size > 2 ? Esq.num!(rest[2]) : 0.0]
      end
      raise RuntimeError, 'Invalid PickHelper - no view defined' unless @pt
      ap = @pt[2] > 0 ? @pt[2] : 6.0
      scr = pts.map { |p| @view.screen_coords(p) }
      scr.each_with_index do |s, i|
        return i if s && Math.hypot(s.x - @pt[0], s.y - @pt[1]) <= ap
      end
      (1...pts.size).each do |i|
        a, b = scr[i - 1], scr[i]
        next unless a && b
        return -i if Esq.seg_dist(@pt[0], @pt[1], a.x, a.y, b.x, b.y) <= ap
      end
      false
    end

    def window_pick(p1, p2, flag)
      raise RuntimeError, NO_VIEW unless @view
      f = Esq.int!(flag)
      unless f == PICK_INSIDE || f == PICK_CROSSING
        raise ArgumentError, 'Invalid pick option. Use Sketchup::PickHelper::PICK_INSIDE or Sketchup::PickHelper::PICK_CROSSING.'
      end
      a, b = p1.to_a, p2.to_a
      # Among the entities being edited, an instance as a whole.
      ctx = (Sketchup.active_model.active_path || []).map(&:handle)
      @hits = Esq.call('pick.rect', 'min' => [[a[0], b[0]].min.to_f, [a[1], b[1]].min.to_f],
                                    'max' => [[a[0], b[0]].max.to_f, [a[1], b[1]].max.to_f], 'crossing' => f == PICK_CROSSING,
                                    'ctx' => ctx) rescue []
      @hits = [] unless @hits.is_a?(Array)
      @hits.size
    end

    # a box, the pick option, maybe a transformation (rb_scan_args « 21 »); the
    # entities being edited, an instance as a whole.
    def boundingbox_pick(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 2..3)" unless (2..3).cover?(args.size)
      bb = args[0]
      raise TypeError, 'Wrong type - expected BoundingBox' unless bb.is_a?(Geom::BoundingBox)
      f = Esq.int!(args[1])
      unless f == PICK_INSIDE || f == PICK_CROSSING
        raise ArgumentError, 'Invalid pick option. Use Sketchup::PickHelper::PICK_INSIDE or Sketchup::PickHelper::PICK_CROSSING.'
      end
      t = args[2].nil? ? nil : Esq::G.xform(args[2])
      mn, mx = bb.min.to_a, bb.max.to_a
      if t
        corners = [mn[0], mx[0]].product([mn[1], mx[1]], [mn[2], mx[2]]).map { |c| (t * Geom::Point3d.new(*c)).to_a }
        mn = corners.transpose.map(&:min)
        mx = corners.transpose.map(&:max)
      end
      m = Sketchup.active_model
      path = m.active_path || []
      @hits = Esq.call('pick.bbox', 'min' => mn, 'max' => mx, 'crossing' => f == PICK_CROSSING,
                                    'ctx' => m.active_entities.ctx, 'xform' => m.edit_transform.to_a,
                                    'path' => path.map(&:handle))
      @hits.size
    end
  end

  # ================================================================ Snap
  # An entity of its Entities collection (`Entities#add_snap`), stored in the model: handle
  # `n:<ctx>:<id>`; position, direction and up are read from and written to the model.
  class Snap < Drawingelement
    Esq.undef_alloc(self)

    def self.__wrap(handle)
      s = allocate
      s.instance_variable_set(:@handle, handle)
      s
    end

    def position; Geom::Point3d.new(info['pos']); end
    def direction; Geom::Vector3d.new(info['dir']); end
    def up; Geom::Vector3d.new(info['up']); end

    # set(position) keeps the orientation; set(position, direction[, up]) orients the snap.
    def set(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 1..3)" if args.empty? || args.size > 3
      a = { 'h' => @handle, 'pos' => Esq.point_a(args[0]) }
      a['dir'] = Esq.point_a(args[1]) if args.size > 1
      a['up'] = Esq.point_a(args[2]) if args.size > 2
      Esq.call('snap.set', a)
      self
    end
  end

  # ================================================================ Space
  # An entity of its Entities collection (`Entities#add_space`, like add_cpoint in SketchUp.exe),
  # stored in the model (`Entities.spaces`), handle `p:<ctx>:<id>`; `each`, `erase!`, `hidden=`,
  # `layer=`, `transform_entities`, the selection work on it.
  class Space < Drawingelement
    Esq.undef_alloc(self)

    def self.__create(ctx, name, orientation, perimeter, ceiling)
      __wrap(Esq.call('space.add', 'ctx' => ctx, 'name' => name, 'orientation' => orientation,
                                   'perimeter' => perimeter, 'ceiling' => ceiling))
    end

    def self.__wrap(handle)
      s = allocate
      s.instance_variable_set(:@handle, handle)
      s
    end

    def __doc; info; end

    def __set(args)
      Esq.call('space.set', args.merge('h' => @handle))
    end
    private :__set

    def name; __doc['name'].dup; end
    def name=(v); __set('name' => v.to_s); v; end
    def orientation_point; Geom::Point3d.new(*__doc['orientation']); end
    def orientation_point=(v); __set('orientation' => Esq.point_a(v)); v; end
    def perimeter_points; __doc['perimeter'].map { |p| Geom::Point3d.new(*p) }; end

    def perimeter_points=(pts)
      raise TypeError, 'wrong argument type (expected Array)' unless pts.is_a?(Array)
      raise ArgumentError, 'Perimeter points must have at least 3 points.' if pts.size < 3
      __set('perimeter' => pts.map { |p| Esq.point_a(p) })
      pts
    end

    def ceiling_plane
      __doc['ceiling'] || [0.0, 0.0, 1.0, 0.0]
    end

    def ceiling_plane=(plane)
      pl = plane.to_a
      raise ArgumentError, 'Could not set ceiling plane' unless pl.size == 4 && pl.all? { |c| c.is_a?(Numeric) } && Math.sqrt(pl[0]**2 + pl[1]**2 + pl[2]**2) > 1e-9
      __set('ceiling' => pl.map(&:to_f))
      plane
    end

    def add_hole(pts)
      raise TypeError, 'wrong argument type (expected Array)' unless pts.is_a?(Array)
      raise ArgumentError, 'Hole must have at least 3 points.' if pts.size < 3
      __set('hole' => pts.map { |p| Esq.point_a(p) })
      true
    end
  end

  class Entities
    # SketchUp (arity 4): name, perimeter points (an Array, at least 3), the
    # orientation point, and the height of the ceiling (a horizontal plane at that height).
    def add_space(name, perimeter_points, orientation_point, height)
      check_frozen!
      name = Esq.str_arg(name)
      Esq.unexpected_type(perimeter_points, 'Array') unless perimeter_points.is_a?(Array)
      raise ArgumentError, 'Perimeter points must have at least 3 points.' if perimeter_points.size < 3
      pts = perimeter_points.map { |p| Esq.point_a(p) }
      h = Esq.num2dbl(height)
      Space.__create(@ctx, name, Esq.point_a(orientation_point), pts, [0.0, 0.0, 1.0, 0.0 - h])
    end
  end

  # ================================================================ HLR
  class HLR < Entity
    def self.use_new_edgevisibility(value)
      @new_edgevis = value ? true : false
    end

    def initialize
      @calculated = false
      @faces = 0
    end

    def calculate
      @faces = Esq.call('hlr.count')
      @calculated = true
    end

    def num_faces; @faces; end

    def cleanup
      @faces = 0
      @calculated = false
      true
    end
  end
end

module Esq
  class << self
    def seg_dist(px, py, ax, ay, bx, by)
      dx, dy = bx - ax, by - ay
      l2 = dx * dx + dy * dy
      t = l2.zero? ? 0.0 : (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0)
      Math.hypot(px - (ax + t * dx), py - (ay + t * dy))
    end
  end
end

module Esq
  class << self
    def scenes
      call('scene.list')
    end

    def default_transition; (host('view.anim_settings', {}, nil) || {})['transition'] || 2.0; end
    def default_delay; (host('view.anim_settings', {}, nil) || {})['delay'] || 1.0; end

    def handle_of(entity)
      entity.respond_to?(:handle) ? entity.handle : nil
    end

    # The model's collection, then those of every definition (groups included).
    def all_entities(model)
      [model.entities] + model.definitions.map(&:entities)
    end
  end
end

module Sketchup
  # ================================================================ Page (scène)
  class Page < Entity
    FLAG = {
      camera: PAGE_USE_CAMERA, rendering: PAGE_USE_RENDERING_OPTIONS, shadow: PAGE_USE_SHADOWINFO,
      axes: PAGE_USE_SKETCHCS, hidden: PAGE_USE_HIDDEN, layers: PAGE_USE_LAYER_VISIBILITY,
      sections: PAGE_USE_SECTION_PLANES, hidden_geometry: PAGE_USE_HIDDEN_GEOMETRY,
      hidden_objects: PAGE_USE_HIDDEN_OBJECTS, environment: PAGE_USE_ENVIRONMENT, style: 0x400, point_cloud: 0x800
    }.freeze

    Esq.undef_alloc(self)

    def self.__wrap(uid)
      p = allocate
      p.instance_variable_set(:@uid, uid)
      p
    end

    def uid; @uid; end
    def handle; "g:0:#{@uid}"; end

    def __doc
      Esq.scenes.find { |s| s['uid'] == @uid } or raise TypeError, 'reference to deleted Page'
    end

    def __props; __doc['props']; end

    def __set(h)
      Esq.call('scene.set', { 'uid' => @uid }.merge(h))
    end

    def __prop_set(k, v)
      __set('props' => { k => v })
    end

    def __flags; __props.fetch('flags', PAGE_USE_ALL); end

    def __flag_set(bit, on)
      f = __flags
      __prop_set('flags', on ? (f | bit) : (f & ~bit))
    end

    def valid?; Esq.scenes.any? { |s| s['uid'] == @uid }; end
    def deleted?; !valid?; end
    def entityID; Esq.object_pid(:page, @uid); end
    alias persistent_id entityID
    def ==(o); o.is_a?(Page) && o.uid == @uid; end
    alias eql? ==
    def hash; @uid.hash; end
    def parent; Sketchup.active_model.pages; end
    def model; Sketchup.active_model; end
    def typename; 'Page'; end

    def name; __doc['name'].dup; end
    def label; __doc['name'].dup; end

    def name=(value)
      n = value.to_s
      return value if n == __doc['name']
      n = Sketchup.active_model.pages.unique_name(n) if Esq.scenes.any? { |s| s['name'] == n && s['uid'] != @uid }
      __set('name' => n)
      value
    end

    def description; __doc['description'].dup; end
    def description=(v); __set('description' => v.to_s); v; end

    def transition_time; __props.fetch('transition', -1.0).to_f; end

    def transition_time=(v)
      t = Esq.num!(v)
      t = -1.0 if t < 0
      __prop_set('transition', t)
      t
    end

    def delay_time; __props.fetch('delay', -1.0).to_f; end

    def delay_time=(v)
      t = Esq.num!(v)
      t = -1.0 if t < 0
      __prop_set('delay', t)
      t
    end

    def include_in_animation?; __props.fetch('animate', true) ? true : false; end
    def include_in_animation=(v); __prop_set('animate', v ? true : false); v; end

    FLAG.each do |sym, bit|
      meth = case sym
             when :camera then 'camera'
             when :rendering then 'rendering_options'
             when :shadow then 'shadow_info'
             when :axes then 'axes'
             when :hidden then 'hidden'
             when :layers then 'hidden_layers'
             when :sections then 'section_planes'
             when :hidden_geometry then 'hidden_geometry'
             when :hidden_objects then 'hidden_objects'
             when :environment then 'environment'
             when :style then 'style'
             else 'point_cloud_data'
             end
      define_method("use_#{meth}?") { (__flags & bit) != 0 }
      define_method("use_#{meth}=") { |v| __flag_set(bit, v ? true : false); v }
    end

    # ---- capture et application
    def update(*args)
      flags = args.empty? ? PAGE_USE_ALL : Esq.int!(args[0])
      m = Sketchup.active_model
      patch = {}
      props = { 'flags' => __flags | flags }
      if (flags & PAGE_USE_CAMERA) != 0
        h = Esq.host('view.camera_get', {}, nil)
        if h
          c = Camera.new
          c.__load(h)
          Esq.page_camera_set(@uid, h)
        end
      end
      if (flags & PAGE_USE_RENDERING_OPTIONS) != 0
        props['ro'] = m.rendering_options.__snapshot
      end
      if (flags & PAGE_USE_SHADOWINFO) != 0
        props['shadow'] = m.shadow_info.__snapshot
      end
      if (flags & PAGE_USE_LAYER_VISIBILITY) != 0
        patch['hidden_layers'] = m.layers.reject(&:visible?).map(&:name)
        props['hidden_folders'] = Esq.call('folder.list').reject { |f| f['flag'] }.map { |f| f['uid'] }
      end
      if (flags & PAGE_USE_HIDDEN) != 0
        props['hidden_ents'] = m.entities.select { |e| e.respond_to?(:hidden?) && e.hidden? }.map { |e| e.handle }
      end
      if (flags & PAGE_USE_SECTION_PLANES) != 0
        props['sections'] = Esq.all_entities(m).map(&:active_section_plane).compact.map(&:handle)
      end
      props['axes'] = Esq.call('axes.get') if (flags & PAGE_USE_SKETCHCS) != 0
      props['style'] = m.styles.selected_style.name if (flags & (0x400 | PAGE_USE_RENDERING_OPTIONS)) != 0 && m.styles.selected_style
      props['env'] = (m.environments.current && m.environments.current.name) if (flags & PAGE_USE_ENVIRONMENT) != 0
      __set(patch.merge('props' => props))
      true
    end

    def __apply(instant = true)
      flags = __flags
      m = Sketchup.active_model
      props = __props
      if (flags & PAGE_USE_RENDERING_OPTIONS) != 0 && props['ro']
        m.rendering_options.__restore(props['ro'])
      end
      if (flags & PAGE_USE_SHADOWINFO) != 0 && props['shadow']
        m.shadow_info.__restore(props['shadow'])
      end
      # The style of a scene that saves its style and fog (`Style and Fog` in the Scenes panel).
      if (flags & (0x400 | PAGE_USE_RENDERING_OPTIONS)) != 0 && props['style']
        st = m.styles[props['style']]
        m.styles.__select_raw(st) if st
      end
      if (flags & PAGE_USE_ENVIRONMENT) != 0 && props.key?('env')
        m.environments.__current_raw(props['env'])
      end
      Esq.call('axes.set', 'm' => props['axes']) if (flags & PAGE_USE_SKETCHCS) != 0 && props['axes']
      if (flags & PAGE_USE_SECTION_PLANES) != 0 && props['sections']
        want = active_section_planes
        Esq.all_entities(m).each do |ents|
          sp = want.find { |s| s.ctx == ents.ctx }
          ents.active_section_plane = sp unless ents.active_section_plane == sp
        end
      end
      # The folders' own switches: the tags inside got their state from the scene's tags (shown
      # again by the folder, a tag the scene hides would show).
      if (flags & PAGE_USE_LAYER_VISIBILITY) != 0 && props['hidden_folders']
        hidden = props['hidden_folders']
        Esq.call('folder.list').each do |f|
          want = !hidden.include?(f['uid'])
          Esq.call('folder.update', 'uid' => f['uid'], 'flag' => want) unless f['flag'] == want
        end
      end
      if (flags & PAGE_USE_HIDDEN) != 0 && props['hidden_ents']
        m.entities.each do |e|
          next unless e.respond_to?(:hidden=)
          e.hidden = props['hidden_ents'].include?(e.handle)
        end
      end
    end

    # ---- éléments enregistrés
    def camera; Camera.__linked([:page, @uid]); end
    def rendering_options; RenderingOptions.__for_page(@uid); end
    def shadow_info; ShadowInfo.__for_page(@uid); end

    def style
      n = __props['style'] or return nil
      Sketchup.active_model.styles[n]
    end

    def environment
      n = __props['env'] or return nil
      Sketchup.active_model.environments[n]
    end

    def environment=(env)
      unless env.nil? || env.is_a?(Environment)
        raise TypeError, 'wrong argument type (expected Sketchup::Environment)'
      end
      __prop_set('env', env && env.name)
      env
    end

    # The drawing axes the scene saved (#update with PAGE_USE_SKETCHCS), else the model's.
    def axes
      __doc
      Axes.__for_page(@uid)
    end

    def hidden_entities
      return nil if (__flags & PAGE_USE_HIDDEN).zero?
      (__props['hidden_ents'] || []).map { |h| Esq.entity_of(h) }
    end

    def layers
      return nil if (__flags & PAGE_USE_LAYER_VISIBILITY).zero?
      names = __doc['hidden_layers'] || []
      Sketchup.active_model.layers.select { |l| names.include?(l.name) }
    end

    # The tag folders the scene hides (by uid, in the scene's props).
    def layer_folders
      return nil if (__flags & PAGE_USE_LAYER_VISIBILITY).zero?
      uids = __props['hidden_folders'] || []
      Esq.call('folder.list').select { |f| uids.include?(f['uid']) }.map { |f| LayerFolder.__for(f) }
    end

    # The section planes the scene keeps active (one per collection), saved by #update.
    def active_section_planes
      (__props['sections'] || []).filter_map { |h| e = Esq.entity_of(h) rescue nil; e if e && e.valid? }
    end

    def set_visibility(item, visible)
      unless item.is_a?(Layer) || (defined?(LayerFolder) && item.is_a?(LayerFolder))
        raise TypeError, 'expected Sketchup::Layer or Sketchup::LayerFolder'
      end
      if item.is_a?(Layer)
        names = (__doc['hidden_layers'] || []).dup
        if visible
          names.delete(item.name)
        else
          names << item.name unless names.include?(item.name)
        end
        __set('hidden_layers' => names)
      else
        uids = (__props['hidden_folders'] || []).dup
        if visible
          uids.delete(item.uid)
        else
          uids << item.uid unless uids.include?(item.uid)
        end
        __prop_set('hidden_folders', uids)
      end
      __flag_set(PAGE_USE_LAYER_VISIBILITY, true)
      self
    end

    # The scene's own list of hidden elements, the one #update fills and selecting the scene
    # restores (SketchUp): top-level elements and instances only.
    def set_drawingelement_visibility(entity, visible)
      raise TypeError, 'Not a Sketchup::Drawingelement.' unless entity.is_a?(Drawingelement)
      return false unless entity.parent.is_a?(Model) || entity.is_a?(Instance) || entity.is_a?(Image)
      list = (__props['hidden_ents'] || []).dup
      if visible
        list.delete(entity.handle)
      else
        list << entity.handle unless list.include?(entity.handle)
      end
      __prop_set('hidden_ents', list)
      true
    end

    def get_drawingelement_visibility(entity)
      raise TypeError, 'Not a Sketchup::Drawingelement.' unless entity.is_a?(Drawingelement)
      !(__props['hidden_ents'] || []).include?(entity.handle)
    end

    def inspect; "#<Sketchup::Page:0x#{(object_id * 8).to_s(16).rjust(16, '0')}>"; end
  end

  # ================================================================ Pages
  class Pages < Entity
    include Enumerable

    ImageEmbedded = 0
    ImageEmbeddedAndLinked = 1
    ImageLinked = 2
    UnitsPixels = 0
    UnitsNormalizedY = 1
    UnitsNormalizedX = 2

    Esq.undef_alloc(self)

    def self.__instance; @__instance ||= allocate; end
    def handle; 'g:0:0'; end
    def ==(o); o.is_a?(Pages); end

    @frame_observers = []
    class << self
      def add_frame_change_observer(obs)
        @frame_observers << obs
        @frame_observers.size - 1
      end

      def remove_frame_change_observer(id)
        i = Esq.int!(id)
        return false unless i >= 0 && i < @frame_observers.size && @frame_observers[i]
        @frame_observers[i] = nil
        true
      end

      def __frame_observers; @frame_observers.compact; end
    end

    def size; Esq.scenes.size; end
    alias length size
    def parent; Sketchup.active_model; end

    def [](key)
      list = Esq.scenes
      if key.is_a?(String)
        s = list.find { |x| x['name'] == key }
        return s && Page.__wrap(s['uid'])
      end
      i = Esq.int!(key)
      i += list.size if i < 0
      return nil if i < 0 || i >= list.size
      Page.__wrap(list[i]['uid'])
    end

    def each
      return enum_for(:each) unless block_given?
      Esq.scenes.each { |s| yield Page.__wrap(s['uid']) }
      nil
    end

    def unique_name(base)
      b = base.to_s
      names = Esq.scenes.map { |s| s['name'] }
      return b unless names.include?(b)
      (1..).each do |i|
        n = "#{b}##{i}"
        return n unless names.include?(n)
      end
    end

    def add(*args)
      name = args[0].nil? ? '' : args[0].to_s
      name = unique_name(name) if !name.empty? && Esq.scenes.any? { |s| s['name'] == name }
      flags = args.size > 1 ? Esq.int!(args[1]) : PAGE_USE_ALL
      flags |= PAGE_USE_HIDDEN if (flags & 0x180) != 0
      index = args.size > 2 ? Esq.int!(args[2]) : size
      index = [[index, 0].max, size].min
      cam = Esq.host('view.camera_get', {}, nil) || Camera.new.__dump
      h = { 'camera' => { 'eye' => cam['eye'], 'target' => cam['target'], 'up' => cam['up'], 'fov' => cam['fov'], 'persp' => cam['persp'] },
            'index' => index,
            'props' => { 'flags' => 0, 'animate' => true, 'transition' => -1.0, 'delay' => -1.0,
                         'cam' => { 'height' => cam['height'], 'aspect' => cam['aspect'], 'image_width' => cam['image_width'],
                                    'desc' => '', 'is2d' => cam['is2d'], 'c2d' => cam['c2d'], 's2d' => cam['s2d'] } } }
      h['name'] = name unless name.empty?
      uid = Esq.call('scene.add', h)
      page = Page.__wrap(uid)
      page.update(flags)
      __new_page_visibility(page) if (flags & PAGE_USE_LAYER_VISIBILITY) != 0
      Esq.host('view.scene_added', 'index' => index)
      page
    end

    # A new scene hides the tags marked LAYER_IS_HIDDEN_ON_NEW_PAGES and shows those marked
    # LAYER_IS_VISIBLE_ON_NEW_PAGES whatever their current visibility, and hides the folders
    # that are not visible on new pages.
    def __new_page_visibility(page)
      m = Sketchup.active_model
      hidden = (page.__doc['hidden_layers'] || []).dup
      m.layers.each do |l|
        pb = l.page_behavior.to_i
        if (pb & LAYER_IS_HIDDEN_ON_NEW_PAGES) != 0
          hidden |= [l.name]
        elsif (pb & LAYER_IS_VISIBLE_ON_NEW_PAGES) != 0
          hidden -= [l.name]
        end
      end
      folders = (page.__props['hidden_folders'] || []) | Esq.call('folder.list').reject { |f| f['visible_on_new_pages'] }.map { |f| f['uid'] }
      page.__set('hidden_layers' => hidden, 'props' => { 'hidden_folders' => folders })
    end
    private :__new_page_visibility

    def add_matchphoto_page(*args)
      raise ArgumentError, 'Invalid number of arguments: 1 to 9 expected' unless args.size >= 1 && args.size <= 9
      raise ArgumentError, 'Null image name encountered' if args[0].nil?
      image = args[0].to_s
      raise ArgumentError, 'Image name is empty' if image.empty?
      cam = args[1]
      name = args.size > 2 && args[2] ? args[2].to_s : ''
      flags = args.size > 3 && !args[3].nil? ? Esq.int!(args[3]) : -1
      page = add(name.empty? ? nil : name, flags < 0 ? PAGE_USE_ALL : flags)
      page.camera.set(*cam.__dump.values_at('eye', 'target', 'up')) if cam.is_a?(Camera)
      page.__prop_set('photo', image)
      page
    end

    def erase(page)
      raise TypeError, 'wrong argument type (expected Sketchup::Page)' unless page.is_a?(Page)
      before = size
      idx = Esq.scenes.index { |s| s['uid'] == page.uid }
      if idx
        Esq.call('scene.remove', 'uid' => page.uid)
        Esq.host('view.scene_removed', 'index' => idx)
      end
      size != before
    end

    def reorder(page, index)
      raise TypeError, 'wrong argument type (expected Sketchup::Page)' unless page.is_a?(Page)
      raise ArgumentError, 'Page does not exist in the page list.' unless page.valid?
      i = Esq.int!(index)
      i += size if i < 0
      raise IndexError, 'Invalid index for reordering scene.' unless i >= 0 && i <= size - 1
      Esq.call('scene.move', 'uid' => page.uid, 'index' => i)
      nil
    end

    def selected_page
      i = Esq.host('view.active_scene', {}, :none)
      i = @selected if i == :none
      return nil unless i.is_a?(Integer)
      list = Esq.scenes
      i < list.size ? Page.__wrap(list[i]['uid']) : nil
    end

    def selected_page=(page)
      raise TypeError, 'wrong argument type (expected Sketchup::Page)' unless page.is_a?(Page)
      i = Esq.scenes.index { |s| s['uid'] == page.uid } or raise TypeError, 'reference to deleted Page'
      flags = page.__flags
      Esq.host('view.go_to_scene', 'index' => i, 'camera' => (flags & PAGE_USE_CAMERA) != 0,
                                   'layers' => (flags & PAGE_USE_LAYER_VISIBILITY) != 0)
      @selected = i
      page.__apply
      page
    end

    def slideshow_time
      t = 0.0
      first = true
      Esq.scenes.each do |s|
        pr = s['props']
        next unless pr.fetch('animate', true)
        tr = pr.fetch('transition', -1.0)
        tr = Esq.default_transition if tr < 0
        dl = pr.fetch('delay', -1.0)
        dl = Esq.default_delay if dl < 0
        t += tr unless first
        t += dl
        first = false
      end
      t
    end

    def show_frame_at(time)
      t = Esq.num!(time)
      list = Esq.scenes.select { |s| s['props'].fetch('animate', true) }
      return nil if list.empty?
      acc = 0.0
      prev = nil
      list.each do |s|
        pr = s['props']
        tr = pr.fetch('transition', -1.0)
        tr = Esq.default_transition if tr < 0
        dl = pr.fetch('delay', -1.0)
        dl = Esq.default_delay if dl < 0
        if prev
          acc += tr
          if t - acc < Geom.tolerance && tr > 0
            ratio = (t - (acc - tr)) / tr
            __show(prev, s, ratio)
            return [Page.__wrap(prev['uid']), ratio]
          end
        end
        acc += dl
        if t - acc < Geom.tolerance
          __show(s, s, 0.0)
          return [Page.__wrap(s['uid']), 0.0]
        end
        prev = s
      end
      nil
    end

    def __show(a, b, ratio)
      ca, cb = a['camera'], b['camera']
      lerp = ->(p, q) { p.each_index.map { |k| p[k] + (q[k] - p[k]) * ratio } }
      cam = Camera.new
      h = cam.__dump
      h['eye'] = lerp.call(ca['eye'], cb['eye'])
      h['target'] = lerp.call(ca['target'], cb['target'])
      h['up'] = lerp.call(ca['up'], cb['up'])
      h['fov'] = ca['fov'] + (cb['fov'] - ca['fov']) * ratio
      h['persp'] = ca['persp']
      Esq.host('view.camera_set', h)
    end

    def add_observer(obs)
      @observers ||= []
      return false if obs.nil? || @observers.any? { |o| o.equal?(obs) }
      @observers << obs
      true
    end

    def remove_observer(obs)
      @observers ||= []
      !@observers.reject! { |o| o.equal?(obs) }.nil?
    end

    def __observers; (@observers ||= []).dup; end
  end

  class Model
    def pages; Pages.__instance; end
  end
end

module Sketchup
  # ================================================================ RenderingOptions
  class RenderingOptions < Entity
    include Enumerable

    ROP = {
      'ROPAssign' => 0, 'ROPDrawHidden' => 0x1c, 'ROPDrawHiddenGeometry' => 0x1d, 'ROPDrawHiddenObjects' => 0x1e,
      'ROPEditComponent' => 0x1f, 'ROPSectionDisplayTurnedOff' => 0x1b, 'ROPSetBackgroundColor' => 0xc,
      'ROPSetConstructionColor' => 0xf, 'ROPSetDepthQueEdges' => 0x21, 'ROPSetDepthQueWidth' => 0x23,
      'ROPSetDisplayColorByLayer' => 0x10, 'ROPSetDisplayDims' => 0x3c, 'ROPSetDisplayFog' => 0x14,
      'ROPSetDisplayInstanceAxes' => 0xb, 'ROPSetDisplaySketchAxes' => 0x27, 'ROPSetDisplayText' => 0x3b,
      'ROPSetDisplayWatermarks' => 0x3e, 'ROPSetDrawBackEdges' => 0x40, 'ROPSetDrawGround' => 0x38,
      'ROPSetDrawHorizon' => 0x37, 'ROPSetDrawUnderground' => 0x39, 'ROPSetEdgeColorMode' => 5,
      'ROPSetEdgeDisplayMode' => 4, 'ROPSetEdgeType' => 0x29, 'ROPSetExtendEdges' => 8, 'ROPSetExtendLines' => 0x12,
      'ROPSetFaceColor' => 0x28, 'ROPSetFogColor' => 0x17, 'ROPSetFogDist' => 0x18, 'ROPSetFogHint' => 0x19,
      'ROPSetFogUseBkColor' => 0x3d, 'ROPSetForegroundColor' => 0xd, 'ROPSetGroundColor' => 0x36,
      'ROPSetGroundTransparency' => 0x3a, 'ROPSetHorizonColor' => 0x47, 'ROPSetHideConstructionGeometry' => 0x34,
      'ROPSetHighlightColor' => 0xe, 'ROPSetJitterEdges' => 6, 'ROPSetLineEndEdges' => 0x22, 'ROPSetLineEndWidth' => 0x24,
      'ROPSetLineExtension' => 0x13, 'ROPSetLineStyleEdges' => 7, 'ROPSetLockedColor' => 0x26,
      'ROPSetMaterialTransparency' => 0x2b, 'ROPSetModelTransparency' => 0x2a,
      'ROPSetPhotomatchBackgroundOpacity' => 0x42, 'ROPSetPhotomatchDrawBackground' => 0x41,
      'ROPSetPhotomatchDrawOverlay' => 0x43, 'ROPSetPhotomatchOverlayOpacity' => 0x44, 'ROPSetProfileEdges' => 9,
      'ROPSetProfilesOnlyEdges' => 0x25, 'ROPSetProfileWidth' => 10, 'ROPSetRenderMode' => 1,
      'ROPSetSectionActiveColor' => 0x2c, 'ROPSetSectionCutFilled' => 0x33, 'ROPSetSectionCutWidth' => 0x32,
      'ROPSetSectionDefaultCutColor' => 0x2e, 'ROPSetSectionDefaultFillColor' => 0x2f, 'ROPSetSectionDisplayMode' => 0x1a,
      'ROPSetSectionInactiveColor' => 0x2d, 'ROPSetSkyColor' => 0x35, 'ROPSetTexture' => 3,
      'ROPSetTransparencyObsolete' => 2, 'ROPSetXRayOpacity' => 0x3f, 'ROPTransparencySortMethod' => 0x20,
      'ROPSetAOEnabled' => 0x48, 'ROPSetAODistance' => 0x49, 'ROPSetAOIntensity' => 0x4b, 'ROPSetModelingGrid' => 0x46,
      'ROPSetHideSpaces' => 0x4e, 'ROPSetAOColorEnabled' => 0x4c, 'ROPSetAOColor' => 0x4d, 'ROPSetAOMultiplier' => 0x4a,
      'ROPSetFaceColorMode' => 9999
    }.freeze
    ROP.each { |n, v| const_set(n, v) }

    # Clé de l'option de rendu → identifiant ROP envoyé aux observateurs.
    ROP_OF_KEY = {
      'RenderMode' => 1, 'Texture' => 3, 'EdgeDisplayMode' => 4, 'EdgeColorMode' => 5, 'JitterEdges' => 6,
      'DrawProfilesOnly' => 0x25, 'DrawSilhouettes' => 9, 'SilhouetteWidth' => 10, 'DisplayInstanceAxes' => 0xb,
      'BackgroundColor' => 0xc, 'ForegroundColor' => 0xd, 'HighlightColor' => 0xe, 'ConstructionColor' => 0xf,
      'DisplayColorByLayer' => 0x10, 'ExtendLines' => 0x12, 'LineExtension' => 0x13, 'DisplayFog' => 0x14,
      'FogColor' => 0x17, 'FogStartDist' => 0x18, 'FogEndDist' => 0x18, 'FogUseBkColor' => 0x3d,
      'DisplaySectionPlanes' => 0x1a, 'DrawHidden' => 0x1c, 'DrawHiddenGeometry' => 0x1d, 'DrawHiddenObjects' => 0x1e,
      'TransparencySort' => 0x20, 'DrawDepthQue' => 0x21, 'DrawLineEnds' => 0x22, 'DepthQueWidth' => 0x23,
      'LineEndWidth' => 0x24, 'LockedColor' => 0x26, 'DisplaySketchAxes' => 0x27, 'EdgeType' => 0x29,
      'ModelTransparency' => 0x2a, 'MaterialTransparency' => 0x2b, 'SectionActiveColor' => 0x2c,
      'SectionInactiveColor' => 0x2d, 'SectionDefaultCutColor' => 0x2e, 'SectionDefaultFillColor' => 0x2f,
      'SectionCutWidth' => 0x32, 'SectionCutFilled' => 0x33, 'HideConstructionGeometry' => 0x34, 'SkyColor' => 0x35,
      'GroundColor' => 0x36, 'DrawHorizon' => 0x37, 'DrawGround' => 0x38, 'DrawUnderground' => 0x39,
      'GroundTransparency' => 0x3a, 'DisplayText' => 0x3b, 'DisplayDims' => 0x3c, 'DisplayWatermarks' => 0x3e,
      'DrawBackEdges' => 0x40, 'HorizonColor' => 0x47, 'AmbientOcclusion' => 0x48, 'AmbientOcclusionDistance' => 0x49,
      'AmbientOcclusionMultiplier' => 0x4a, 'AmbientOcclusionIntensity' => 0x4b, 'AmbientOcclusionColorEnabled' => 0x4c,
      'AmbientOcclusionColor' => 0x4d
    }.freeze

    C = ->(r, g, b) { [r, g, b, 255] }
    # nom => [type, valeur par défaut], dans l'ordre alphabétique des clés de SketchUp
    SPEC = {
      'AmbientOcclusion' => [:bool, false], 'AmbientOcclusionColor' => [:color, C.(0, 0, 0)],
      'AmbientOcclusionColorEnabled' => [:bool, false], 'AmbientOcclusionDistance' => [:float, 23.6220472441],
      'AmbientOcclusionIntensity' => [:float, 1.0], 'AmbientOcclusionMultiplier' => [:float, 1.0],
      'BackgroundColor' => [:color, C.(255, 255, 255)], 'BandColor' => [:color, C.(255, 255, 255)],
      'ConstructionColor' => [:color, C.(0, 0, 0)], 'DefaultSectionFillStyleEnabled' => [:bool, false],
      'DepthQueWidth' => [:int, 4], 'DisplayColorByLayer' => [:bool, false], 'DisplayDims' => [:bool, true],
      'DisplayFog' => [:bool, false], 'DisplayInstanceAxes' => [:bool, false], 'DisplaySectionCuts' => [:bool, true],
      'DisplaySectionPlanes' => [:bool, false], 'DisplaySketchAxes' => [:bool, true], 'DisplayText' => [:bool, true],
      'DisplayWatermarks' => [:bool, true], 'DrawBackEdges' => [:bool, false], 'DrawDepthQue' => [:bool, false],
      'DrawGround' => [:bool, false], 'DrawHidden' => [:bool, false], 'DrawHiddenGeometry' => [:bool, false],
      'DrawHiddenObjects' => [:bool, false], 'DrawHorizon' => [:bool, true], 'DrawLineEnds' => [:bool, false],
      'DrawProfilesOnly' => [:bool, false], 'DrawSilhouettes' => [:bool, true], 'DrawUnderground' => [:bool, false],
      'EdgeColorMode' => [:int, 0], 'EdgeDisplayMode' => [:int, 1], 'EdgeType' => [:int, 1],
      'ExtendLines' => [:bool, false], 'FaceBackColor' => [:color, C.(164, 164, 164)],
      'FaceFrontColor' => [:color, C.(255, 255, 255)], 'FogColor' => [:color, C.(204, 204, 204)],
      'FogEndDist' => [:float, 8000.0], 'FogStartDist' => [:float, 0.0], 'FogUseBkColor' => [:bool, true],
      'ForegroundColor' => [:color, C.(0, 0, 0)], 'GroundColor' => [:color, C.(153, 153, 153)],
      'GroundTransparency' => [:int, 0], 'HideConstructionGeometry' => [:bool, false],
      'HighlightColor' => [:color, C.(0, 0, 255)], 'HorizonColor' => [:color, C.(204, 214, 224)],
      'InactiveFade' => [:float, 0.7], 'InactiveHidden' => [:bool, false], 'InstanceFade' => [:float, 0.7],
      'InstanceHidden' => [:bool, false], 'JitterEdges' => [:bool, false], 'LineEndWidth' => [:int, 1],
      'LineExtension' => [:int, 6], 'LockedColor' => [:color, C.(255, 128, 128)],
      'MaterialTransparency' => [:bool, true], 'ModelTransparency' => [:bool, false], 'RenderMode' => [:int, 2],
      'SectionActiveColor' => [:color, C.(0, 0, 0)], 'SectionCutDrawEdges' => [:bool, true],
      'SectionCutFilled' => [:bool, true], 'SectionCutWidth' => [:int, 3],
      'SectionDefaultCutColor' => [:color, C.(0, 0, 0)], 'SectionDefaultFillColor' => [:color, C.(128, 128, 128)],
      'SectionInactiveColor' => [:color, C.(0, 0, 0)], 'ShowViewName' => [:bool, false],
      'SilhouetteWidth' => [:int, 3], 'SkyColor' => [:color, C.(204, 214, 224)], 'Texture' => [:bool, true],
      'TransparencySort' => [:int, 0]
    }.freeze

    Esq.undef_alloc(self)

    def self.__instance; @__instance ||= allocate; end
    def self.__for_page(uid); r = allocate; r.instance_variable_set(:@page, uid); r; end
    def self.keys; SPEC.keys.sort; end
    def self.each_key(&block); keys.each(&block); nil; end

    def handle; @page ? "g:0:ro#{@page}" : 'g:0:ro'; end
    def ==(o); o.is_a?(RenderingOptions) && o.instance_variable_get(:@page) == @page; end

    def keys; self.class.keys; end
    def size; SPEC.size; end
    alias length size

    def each_key(&block)
      return enum_for(:each_key) unless block
      keys.each(&block)
      nil
    end

    def each_pair
      return enum_for(:each_pair) unless block_given?
      keys.each { |k| yield k, self[k] }
      nil
    end
    alias each each_pair

    def __store
      if @page
        s = Esq.scenes.find { |x| x['uid'] == @page } or raise TypeError, 'reference to deleted Page'
        (s['props']['ro'] || {})
      else
        Esq.vx_get('ro', {})
      end
    end

    def __save(h)
      if @page
        Esq.call('scene.set', 'uid' => @page, 'props' => { 'ro' => h })
      else
        Esq.vx_set('ro', h)
      end
    end

    def [](key)
      k = key.to_s
      spec = SPEC[k] or return nil
      v = nil
      unless @page
        r = Esq.host('view.ro_get', { 'key' => k }, nil)
        v = r['value'] if r && r['mapped']
      end
      v = __store.fetch(k, spec[1]) if v.nil?
      spec[0] == :color ? Color.new(*v) : v
    end

    def []=(key, value)
      k = key.to_s
      spec = SPEC[k]
      v = spec && __coerce(spec[0], value)
      raise ArgumentError, 'Rendering option could not be set to the given value' if v.nil?
      if @page
        h = __store.dup
        h[k] = v
        __save(h)
      else
        Styles.__materialize
        mapped = Esq.host('view.ro_set', { 'key' => k, 'value' => v }, nil)
        if mapped && mapped['mapped'] && mapped['ok'] == false
          raise ArgumentError, 'Rendering option could not be set to the given value'
        end
        unless mapped && mapped['mapped']
          h = __store.dup
          h[k] = v
          __save(h)
        end
        __notify(k)
      end
      value
    end

    def __coerce(type, v)
      case type
      when :bool then (v == true || v == false) ? v : nil
      when :int then v.is_a?(Numeric) ? v.round : nil
      when :float then v.is_a?(Numeric) ? v.to_f : nil
      when :color
        return nil unless v.is_a?(Color) || v.is_a?(Array) || v.is_a?(String)
        c = begin
          v.is_a?(Color) ? v : Color.new(v)
        rescue StandardError
          nil
        end
        c && c.to_a
      end
    end

    def __notify(key)
      id = ROP_OF_KEY[key] or return
      __observers.each { |o| o.onRenderingOptionsChanged(self, id) if o.respond_to?(:onRenderingOptionsChanged) }
    end

    def __snapshot
      h = {}
      keys.each do |k|
        v = self[k]
        h[k] = v.is_a?(Color) ? v.to_a : v
      end
      h
    end

    def __restore(h)
      h.each do |k, v|
        next unless SPEC.key?(k)
        self[k] = v
      end
    end

    def hatch_pattern
      n = Esq.vx_get('ro_hatch') or return nil
      defined?(HatchPatterns) ? Sketchup.active_model.hatch_patterns[n] : nil
    end

    def hatch_pattern=(pattern)
      Esq.vx_set('ro_hatch', pattern && pattern.name)
      pattern
    end

    def add_observer(obs)
      @observers ||= []
      return false if obs.nil? || @observers.any? { |o| o.equal?(obs) }
      @observers << obs
      true
    end

    def remove_observer(obs)
      @observers ||= []
      !@observers.reject! { |o| o.equal?(obs) }.nil?
    end

    def __observers; (@observers ||= []).dup; end
    def inspect; "#<Sketchup::RenderingOptions:0x#{(object_id * 8).to_s(16).rjust(16, '0')}>"; end
  end

  # ================================================================ ShadowInfo
  class ShadowInfo < Entity
    include Enumerable

    KEYS = %w[City Country Dark DayOfYear DaylightSavings DisplayNorth DisplayOnAllFaces DisplayOnGroundPlane
              DisplayShadows EdgesCastShadows Latitude Light Longitude NorthAngle ShadowTime ShadowTime_time_t
              SunDirection SunRise SunRise_time_t SunSet SunSet_time_t TZOffset UseSunForAllShading].freeze

    Esq.undef_alloc(self)

    def self.__instance; @__instance ||= allocate; end
    def self.__for_page(uid); r = allocate; r.instance_variable_set(:@page, uid); r; end
    def self.keys; KEYS.dup; end
    def self.each_key(&block); KEYS.each(&block); nil; end

    def handle; @page ? "g:0:sh#{@page}" : 'g:0:sh'; end
    def ==(o); o.is_a?(ShadowInfo) && o.instance_variable_get(:@page) == @page; end

    def keys; KEYS.dup; end
    def size; KEYS.size; end
    alias length size

    def each_key(&block)
      return enum_for(:each_key) unless block
      KEYS.each(&block)
      nil
    end

    def each_pair
      return enum_for(:each_pair) unless block_given?
      KEYS.each { |k| yield k, self[k] }
      nil
    end
    alias each each_pair

    def __live
      Esq.call('shadow.get')
    end

    def __data
      if @page
        s = Esq.scenes.find { |x| x['uid'] == @page } or raise TypeError, 'reference to deleted Page'
        snap = s['props']['shadow']
        return __live.merge(snap) if snap
      end
      __live
    end

    def [](key)
      k = key.to_s
      return nil unless KEYS.include?(k)
      d = __data
      case k
      when 'ShadowTime' then Time.at(d['ShadowTime_time_t']).utc
      when 'SunRise' then Time.at(d['SunRise_time_t']).utc
      when 'SunSet' then Time.at(d['SunSet_time_t']).utc
      when 'SunDirection' then Geom::Vector3d.new(*d['SunDirection'])
      else d[k]
      end
    end

    def []=(key, value)
      k = key.to_s
      v = value.is_a?(Time) ? value.to_i : value
      v = v.to_f if %w[Latitude Longitude NorthAngle TZOffset].include?(k) && v.is_a?(Numeric)
      if @page
        s = Esq.scenes.find { |x| x['uid'] == @page } or raise TypeError, 'reference to deleted Page'
        snap = (s['props']['shadow'] || __live).dup
        snap[k == 'ShadowTime' ? 'ShadowTime_time_t' : k] = v if KEYS.include?(k)
        Esq.call('scene.set', 'uid' => @page, 'props' => { 'shadow' => snap })
      else
        begin
          ok = Esq.call('shadow.set', 'k' => k, 'v' => v)
        rescue RuntimeError
          ok = false
        end
        __notify if ok
      end
      value
    end

    def __notify
      __observers.each { |o| o.onShadowInfoChanged(self, 0) if o.respond_to?(:onShadowInfoChanged) }
    end

    def __snapshot
      d = __live
      d.reject { |k, _| %w[DayOfYear SunDirection SunRise_time_t SunSet_time_t].include?(k) }
    end

    def __restore(h)
      h.each { |k, v| Esq.call('shadow.set', 'k' => k, 'v' => v) }
    end

    def add_observer(obs)
      @observers ||= []
      return false if obs.nil? || @observers.any? { |o| o.equal?(obs) }
      @observers << obs
      true
    end

    def remove_observer(obs)
      @observers ||= []
      !@observers.reject! { |o| o.equal?(obs) }.nil?
    end

    def __observers; (@observers ||= []).dup; end
  end

  # ================================================================ Style / Styles
  class Style < Entity
    Esq.undef_alloc(self)

    def self.__wrap(id)
      s = allocate
      s.instance_variable_set(:@id, id)
      s
    end

    def __id; @id; end
    def handle; "g:0:st#{@id}"; end
    def __doc; Styles.__doc(@id.abs); end
    def __update; Styles.__update { |st| d = st['list'].find { |x| x['id'] == @id }; yield d if d }; end
    def valid?; !Styles.__doc(@id, false).nil?; end
    def ==(o); o.is_a?(Style) && o.__id == @id; end
    alias eql? ==
    def hash; @id.hash; end
    def entityID; Esq.object_pid(:style, @id); end

    def name; __doc['name'].dup; end

    def name=(v)
      __update { |d| d['name'] = v.to_s }
      v
    end

    def description; __doc['description'].dup; end
    def description=(v); __update { |d| d['description'] = v.to_s }; v; end
    def path; __doc['path'].dup; end

    def duplicate
      parent = Styles.__doc_list.any? { |d| d['id'] == @id }
      raise RuntimeError, 'Style has no parent Styles collection.' unless parent
      src = __doc
      id = nil
      Styles.__update do |st|
        id = (st['list'].map { |d| d['id'] }.max || 0) + 1
        st['list'] << src.merge('id' => id, 'name' => Styles.__unique(src['name'], st['list']), 'path' => '')
      end
      Style.__wrap(id)
    end
  end

  class Styles < Entity
    include Enumerable
    Esq.undef_alloc(self)

    def self.__instance; @__instance ||= allocate; end
    def handle; 'g:0:sts'; end
    def ==(o); o.is_a?(Styles); end

    # The model's styles; a model that has none yet has the default style, written to the model
    # only when something changes (reading the styles is not a change of the model).
    def self.__state
      Esq.vx_get('styles') ||
        { 'list' => [{ 'id' => 1, 'name' => "Style par défaut", 'description' => '', 'path' => '',
                       'ro' => Sketchup.active_model.rendering_options.__snapshot }],
          'selected' => 1 }
    end

    # Writes the default style to the model if it is not there yet: done before the rendering
    # options change, so that the default style keeps the settings it was made with.
    def self.__materialize
      Esq.vx_set('styles', __state) if Esq.vx_get('styles').nil?
    end

    def self.__doc_list; __state['list']; end

    def self.__doc(id, strict = true)
      d = __doc_list.find { |x| x['id'] == id }
      raise TypeError, 'reference to deleted Style' if d.nil? && strict
      d
    end

    def self.__update
      st = __state
      yield st
      Esq.vx_set('styles', st)
    end

    def self.__unique(base, list)
      names = list.map { |d| d['name'] }
      return base unless names.include?(base)
      (1..).each { |i| n = "#{base} #{i}"; return n unless names.include?(n) }
    end

    def size; Styles.__doc_list.size; end
    alias length size
    def parent; Sketchup.active_model; end

    def [](key)
      list = Styles.__doc_list
      if key.is_a?(String)
        d = list.find { |x| x['name'] == key }
        return d && Style.__wrap(d['id'])
      end
      i = Esq.int!(key)
      i += list.size if i < 0
      return nil if i < 0 || i >= list.size
      Style.__wrap(list[i]['id'])
    end

    def each
      return enum_for(:each) unless block_given?
      Styles.__doc_list.each { |d| yield Style.__wrap(d['id']) }
      nil
    end

    def selected_style
      st = Styles.__state
      Style.__wrap(st['selected'])
    end

    def active_style_changed
      st = Styles.__state
      d = st['list'].find { |x| x['id'] == st['selected'] }
      d['ro'] != Sketchup.active_model.rendering_options.__snapshot
    end

    def active_style
      active_style_changed ? Style.__wrap(-Styles.__state['selected']) : selected_style
    end

    def __select_raw(style)
      Styles.__update { |st| st['selected'] = style.__id }
      doc = style.__doc
      Sketchup.active_model.rendering_options.__restore(doc['ro'])
      Esq.host('view.watermarks_set', { 'wm' => doc['wm'] || [] }, nil)
    end

    def selected_style=(style)
      raise TypeError, 'wrong argument type (expected Sketchup::Style)' unless style.is_a?(Style)
      raise ArgumentError, 'Can not set the selected style to the active style.' if style.__id < 0
      style.__doc
      __select_raw(style)
      nil
    end

    def update_selected_style
      snap = Sketchup.active_model.rendering_options.__snapshot
      Styles.__update { |st| st['list'].find { |x| x['id'] == st['selected'] }['ro'] = snap }
      nil
    end

    def add_style(*args)
      path = args[0]
      select = args.size > 1 && args[1] ? true : false
      return nil unless path.is_a?(String) && !path.empty? && File.file?(path)
      # The style file's own settings (read by the application), else the current ones.
      f = Esq.host('view.style_file', { 'path' => File.expand_path(path) }, nil)
      base = f && !f['name'].to_s.empty? ? f['name'] : File.basename(path, '.*')
      id = nil
      Styles.__update do |st|
        id = (st['list'].map { |d| d['id'] }.max || 0) + 1
        st['list'] << { 'id' => id, 'name' => Styles.__unique(base, st['list']), 'description' => f ? f['description'].to_s : '',
                        'path' => File.expand_path(path),
                        'ro' => f ? f['ro'] : Sketchup.active_model.rendering_options.__snapshot, 'wm' => f ? f['wm'] : [] }
      end
      style = Style.__wrap(id)
      __select_raw(style) if select
      style
    end

    def remove_style(style)
      raise TypeError, 'wrong argument type (expected Sketchup::Style)' unless style.is_a?(Style)
      raise ArgumentError, 'Style not found in the Styles collection.' unless style.valid?
      raise ArgumentError, 'The Styles collection must contain at least one style.' if size <= 1
      Styles.__update do |st|
        st['list'].reject! { |d| d['id'] == style.__id }
        st['selected'] = st['list'].first['id'] unless st['list'].any? { |d| d['id'] == st['selected'] }
      end
      nil
    end

    def purge_unused
      used = Esq.scenes.map { |s| s['props']['style'] }.compact
      Styles.__update do |st|
        st['list'].reject! { |d| d['id'] != st['selected'] && !used.include?(d['name']) && st['list'].size > 1 }
      end
      nil
    end
  end

  # ================================================================ Environment / Environments
  class Environment < Entity
    Esq.undef_alloc(self)

    def self.__wrap(id)
      e = allocate
      e.instance_variable_set(:@id, id)
      e
    end

    def __id; @id; end
    def handle; "g:0:en#{@id}"; end
    def __doc; Environments.__list.find { |d| d['id'] == @id } or raise TypeError, 'reference to deleted Environment'; end

    def __update
      list = Environments.__list
      d = list.find { |x| x['id'] == @id } or raise TypeError, 'reference to deleted Environment'
      yield d
      Environments.__save(list)
    end

    def valid?; Environments.__list.any? { |d| d['id'] == @id }; end
    def ==(o); o.is_a?(Environment) && o.__id == @id; end
    alias eql? ==
    def hash; @id.hash; end
    def entityID; Esq.object_pid(:environment, @id); end

    def name; __doc['name'].dup; end

    def name=(v)
      n = v.to_s
      raise ArgumentError, 'Environment name cannot be empty.' if n.empty?
      if Environments.__list.any? { |d| d['name'] == n && d['id'] != @id }
        raise RuntimeError, 'Environment name must be unique.'
      end
      __update { |d| d['name'] = n }
      v
    end

    def description; __doc['description'].dup; end
    def description=(v); __update { |d| d['description'] = v.to_s }; v; end
    def path; __doc['path'].dup; end
    def use_as_skydome?; __doc['skydome'] ? true : false; end
    def use_as_skydome=(v); __update { |d| d['skydome'] = v ? true : false }; v; end
    def use_for_reflections?; __doc['reflections'] ? true : false; end
    def use_for_reflections=(v); __update { |d| d['reflections'] = v ? true : false }; v; end
    def rotation; __doc['rotation'].to_f; end

    def rotation=(v)
      f = Esq.num!(v)
      raise RangeError, 'Rotation is out of range.' unless f >= 0.0 && f <= 360.0
      __update { |d| d['rotation'] = f }
      v
    end

    def skydome_exposure; __doc['skydome_exposure'].to_f; end

    def skydome_exposure=(v)
      f = Esq.num!(v)
      raise RangeError, 'Skydome exposure is out of range.' unless f >= 0.0 && f <= 20.0
      __update { |d| d['skydome_exposure'] = f }
      v
    end

    def reflection_exposure; __doc['reflection_exposure'].to_f; end

    def reflection_exposure=(v)
      f = Esq.num!(v)
      raise RangeError, 'Reflection exposure is out of range.' unless f >= 0.0 && f <= 10.0
      __update { |d| d['reflection_exposure'] = f }
      v
    end

    def linked_sun?; __doc['linked_sun'] ? true : false; end
    def linked_sun=(v); __update { |d| d['linked_sun'] = v ? true : false }; v; end

    def linked_sun_position
      a = __doc['sun_pos']
      defined?(Geom::Point2d) ? Geom::Point2d.new(*a) : a.dup
    end

    def linked_sun_position=(v)
      a = v.to_a
      raise RangeError, 'Linked sun position values are out of range.' unless a.size >= 2 && a[0].is_a?(Numeric) && a[1].is_a?(Numeric) &&
                                                                              a[0] >= 0.0 && a[0] <= 360.0 && a[1] >= 0.0 && a[1] <= 90.0
      __update { |d| d['sun_pos'] = [a[0].to_f, a[1].to_f] }
      v
    end

    def thumbnail
      p = __doc['path']
      unless defined?(ImageRep) && !p.empty? && File.file?(p)
        raise RuntimeError, 'Invalid thumbnail image data.'
      end
      ir = ImageRep.new
      ir.load_file(p)
      ir
    end

    def write_hdr(file)
      f = file.to_s
      raise ArgumentError, 'The file name is empty.' if f.empty?
      p = __doc['path']
      raise ArgumentError, 'The image is invalid.' unless !p.empty? && File.file?(p)
      begin
        File.binwrite(f, File.binread(p))
      rescue SystemCallError
        raise RuntimeError, 'Failed to write HDR file.'
      end
      f
    end
  end

  class Environments < Entity
    include Enumerable
    Esq.undef_alloc(self)

    def self.__instance; @__instance ||= allocate; end
    def handle; 'g:0:ens'; end
    def ==(o); o.is_a?(Environments); end
    def self.__list; Esq.vx_get('envs', []); end
    def self.__save(list); Esq.vx_set('envs', list); end

    def size; Environments.__list.size; end
    alias length size

    def each
      return enum_for(:each) unless block_given?
      Environments.__list.each { |d| yield Environment.__wrap(d['id']) }
      self
    end

    def [](name)
      d = Environments.__list.find { |x| x['name'] == name.to_s }
      d && Environment.__wrap(d['id'])
    end

    def add(*args)
      raise ArgumentError, 'Wrong number of arguments: requires 1 or 2' unless args.size == 1 || args.size == 2
      list = Environments.__list
      if args.size == 1
        path = args[0].to_s
        name = File.basename(path, '.*')
        name = 'Environment' if name.empty?
        n = name
        (1..).each { |i| break unless list.any? { |d| d['name'] == n }; n = "#{name} #{i}" }
        name = n
      else
        name = args[0].to_s
        path = args[1].to_s
        raise ArgumentError, 'Environment name cannot be empty.' if name.empty?
        raise ArgumentError, 'Environment name must be unique.' if list.any? { |d| d['name'] == name }
      end
      raise ArgumentError, 'Failed to load image from path.' unless File.file?(path)
      id = (list.map { |d| d['id'] }.max || 0) + 1
      list << { 'id' => id, 'name' => name, 'description' => '', 'path' => File.expand_path(path), 'skydome' => true,
                'reflections' => true, 'rotation' => 0.0, 'skydome_exposure' => 1.0, 'reflection_exposure' => 1.0,
                'linked_sun' => false, 'sun_pos' => [0.0, 45.0] }
      Environments.__save(list)
      Environment.__wrap(id)
    end

    def remove(env)
      raise TypeError, "wrong argument type #{env.class} (expected Data)" unless env.is_a?(Environment)
      return false unless env.valid?
      list = Environments.__list
      list.reject! { |d| d['id'] == env.__id }
      Environments.__save(list)
      Esq.vx_set('env_current', nil) if Esq.vx_get('env_current') == env.__id
      true
    end

    def current
      id = Esq.vx_get('env_current')
      id && valid_id?(id) ? Environment.__wrap(id) : nil
    end

    def valid_id?(id); Environments.__list.any? { |d| d['id'] == id }; end
    private :valid_id?

    def current=(env)
      unless env.nil? || env.is_a?(Environment)
        raise TypeError, 'Expected an Environment object.'
      end
      if env && !env.valid?
        raise ArgumentError, 'Environment must be in the manager before it can be selected.'
      end
      Esq.vx_set('env_current', env && env.__id)
      env
    end

    def __current_raw(name)
      e = name && self[name]
      Esq.vx_set('env_current', e && e.__id)
    end

    def purge_unused
      used = Esq.scenes.map { |s| s['props']['env'] }.compact
      cur = Esq.vx_get('env_current')
      list = Environments.__list
      list.reject! { |d| d['id'] != cur && !used.include?(d['name']) }
      Environments.__save(list)
      self
    end

    def add_observer(obs)
      @observers ||= []
      return false if obs.nil? || @observers.any? { |o| o.equal?(obs) }
      @observers << obs
      true
    end

    def remove_observer(obs)
      @observers ||= []
      !@observers.reject! { |o| o.equal?(obs) }.nil?
    end

    def __observers; (@observers ||= []).dup; end
  end

  class Model
    def rendering_options; RenderingOptions.__instance; end
    def shadow_info; ShadowInfo.__instance; end
    def styles; Styles.__instance; end
    def environments; Environments.__instance; end
  end

  # The Entity side of the view zone's objects, which have no geometry handle (`handle` is a
  # method of theirs, `@handle` is unset): Entity's valid?, typename, entityID, parent… would
  # ask the geometry bridge about a nil handle.
  module ViewEntity
    def valid?; true; end
    def deleted?; !valid?; end
    def typename; self.class.name.split('::').last; end
    def entityID; handle.bytes.inject(7) { |a, b| (a * 31 + b) & 0x7fffffff }; end
    def persistent_id; entityID; end
    def parent; Sketchup.active_model; end
    def model; Sketchup.active_model; end
  end

  [Styles, Pages, Environments, RenderingOptions, ShadowInfo].each { |k| k.include(ViewEntity) }

  # The rendering options and shadows a scene keeps belong to that scene.
  module PageBound
    def valid?; @page.nil? || Esq.scenes.any? { |s| s['uid'] == @page }; end
    def parent; @page ? Page.__wrap(@page) : Sketchup.active_model; end
  end
  RenderingOptions.include(PageBound)
  ShadowInfo.include(PageBound)

  class Style
    def persistent_id; entityID; end
    def parent; Sketchup.active_model.styles; end
  end

  class Environment
    def persistent_id; entityID; end
    def parent; Sketchup.active_model.environments; end
  end
end
