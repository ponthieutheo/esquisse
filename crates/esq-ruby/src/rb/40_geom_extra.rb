# Esquisse — zone « geom », part 2: 2D geometry (Point2d, Vector2d, Transformation2d, Bounds2d),
# Geom::PolygonMesh, Geom::LatLong and Geom::UTM. See 10_geom.rb for the 3D classes.

module Esq
  module G
    module_function

    # Anything convertible to a 2D point: [x, y]
    def pt2(v)
      case v
      when Geom::Point2d then v.__a
      when Array
        raise ArgumentError, 'wrong number of values in array' unless v.size == 2
        [num(v[0]), num(v[1])]
      else raise TypeError, 'Cannot convert argument to Geom::Point2d'
      end
    end

    def point2_like?(v)
      v.is_a?(Geom::Point2d) || (v.is_a?(Array) && v.size == 2 && numbers?(v))
    end

    def vec2(v)
      case v
      when Geom::Vector2d then v.__a
      when Array
        raise ArgumentError, 'wrong number of values in array' unless v.size == 2
        [num(v[0]), num(v[1])]
      else raise TypeError, 'Cannot convert argument to Geom::Vector2d'
      end
    end

    def xform2(t)
      return t if t.is_a?(Geom::Transformation2d)
      raise TypeError, 'wrong type - expected Geom::Transformation2d'
    end

    def len2(a); Math.sqrt(a[0] * a[0] + a[1] * a[1]); end

    def unit2(a)
      l = len2(a)
      l.zero? ? [0.0, 0.0] : [a[0] / l, a[1] / l]
    end

    def near2?(a, b)
      (a[0] - b[0]).abs < TOL && (a[1] - b[1]).abs < TOL && len2([a[0] - b[0], a[1] - b[1]]) < TOL
    end
  end
end

module Geom
  class Point2d
    def initialize(*a)
      case a.size
      when 0 then @x = @y = 0.0
      when 1 then @x, @y = G.pt2(a[0])
      when 2
        @x = G.num(a[0])
        @y = G.num(a[1])
      else raise ArgumentError, "wrong # of arguments (#{a.size})"
      end
    end

    def __a; [@x, @y]; end

    def x; Length.new(@x); end
    def y; Length.new(@y); end
    def x=(v); @x = G.num(v); end
    def y=(v); @y = G.num(v); end

    def [](i); Length.new(__a[G.index2(i)]); end

    def []=(i, v)
      i = G.index2(i)
      f = G.num(v)
      i.zero? ? @x = f : @y = f
      v
    end

    def to_a; [@x, @y]; end

    def set!(*a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      raise ArgumentError, 'wrong number arguments' if a.empty?
      case a.size
      when 1 then @x, @y = G.pt2(a[0])
      when 2
        x = G.num(a[0])
        @y = G.num(a[1])
        @x = x
      else raise ArgumentError, "wrong # of arguments (#{a.size})"
      end
      self
    end

    def clone; Point2d.new(@x, @y); end

    def ==(o)
      return true if equal?(o)
      return false unless o.is_a?(Point2d) || (o.is_a?(Array) && o.size == 2 && G.numbers?(o))
      G.near2?(__a, G.pt2(o))
    end

    def distance(o)
      Length.new(G.len2(G.pt2(o).zip(__a).map { |a, b| a - b }))
    end

    def vector_to(o)
      b = G.pt2(o)
      Vector2d.new(b[0] - @x, b[1] - @y)
    end

    def +(v)
      d = G.vec2(v)
      Point2d.new(@x + d[0], @y + d[1])
    end

    def -(o)
      if o.is_a?(Vector2d)
        Point2d.new(@x - o.x, @y - o.y)
      else
        b = G.pt2(o)
        Vector2d.new(@x - b[0], @y - b[1])
      end
    end

    def __offset_vec(args, err)
      raise ArgumentError, 'Wrong number of arguments: requires 1 or 2' if args.empty?
      v = G.vec2(args[0])
      if args.size > 1
        l = G.len2(v)
        raise err, (err == TypeError ? 'Zero length vector)' : 'Zero distance is not allowed') if l.zero?
        k = G.num(args[1]) / l
        v = [v[0] * k, v[1] * k]
      end
      v
    end
    private :__offset_vec

    def offset(*args)
      v = __offset_vec(args, ArgumentError)
      Point2d.new(@x + v[0], @y + v[1])
    end

    def offset!(*args)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      v = __offset_vec(args, TypeError)
      @x += v[0]
      @y += v[1]
      self
    end

    def transform(t)
      G.xform2(t) * self
    end

    def transform!(t)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y = (G.xform2(t) * self).__a
      self
    end

    def inspect; "Point2d(#{G.g(@x)}, #{G.g(@y)})"; end

    def to_s
      st = Esq::Units.settings
      "(#{__a.map { |v| Esq::Units.format_length(v, st, precision: 6, strip: true) }.join(', ')})"
    end
  end

  class Vector2d
    def initialize(*a)
      case a.size
      when 0 then @x = @y = 0.0
      when 1 then @x, @y = G.vec2(a[0])
      when 2
        if G.point2_like?(a[0])
          p = G.pt2(a[0])
          q = G.pt2(a[1])
          @x = q[0] - p[0]
          @y = q[1] - p[1]
        else
          @x = G.num(a[0])
          @y = G.num(a[1])
        end
      else raise ArgumentError, "wrong # of arguments(#{a.size})"
      end
    end

    def __a; [@x, @y]; end

    def x; @x; end
    def y; @y; end
    def x=(v); @x = G.num(v); end
    def y=(v); @y = G.num(v); end

    def [](i)
      i = G.int(i)
      i += 2 if i < 0
      raise IndexError, 'index out of range' unless (0..1).cover?(i)
      __a[i]
    end

    def []=(i, v)
      j = G.int(i)
      j += 2 if j < 0
      raise IndexError, 'index out of range' unless (0..1).cover?(j)
      f = G.num(v)
      j.zero? ? @x = f : @y = f
      v
    end

    def to_a; [@x, @y]; end

    def set!(*a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      raise ArgumentError, 'wrong number arguments' if a.empty?
      raise ArgumentError, "wrong # of arguments(#{a.size})" if a.size > 2
      @x, @y = Vector2d.new(*a).__a
      self
    end

    def clone; Vector2d.new(@x, @y); end

    def ==(o)
      return true if equal?(o)
      return false unless o.is_a?(Vector2d) || (o.is_a?(Array) && o.size == 2 && G.numbers?(o))
      G.near2?(__a, G.vec2(o))
    end

    def +(o); b = G.vec2(o); Vector2d.new(@x + b[0], @y + b[1]); end
    def -(o); b = G.vec2(o); Vector2d.new(@x - b[0], @y - b[1]); end

    def dot(o); b = G.vec2(o); @x * b[0] + @y * b[1]; end
    alias % dot

    def cross(o); b = G.vec2(o); @x * b[1] - @y * b[0]; end

    # SketchUp: `*` is the cross product. Extension: `vector * number` scales the vector.
    def *(o)
      return Vector2d.new(@x * o.to_f, @y * o.to_f) if o.is_a?(Numeric)
      cross(o)
    end

    def -@; Vector2d.new(-@x, -@y); end # extension

    def length; Length.new(G.len2(__a)); end

    def length=(l)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      cur = G.len2(__a)
      raise TypeError, 'Cannot set the length of a zero length vector' if cur.zero?
      k = G.num(l) / cur
      @x *= k
      @y *= k
      l
    end

    def normalize; Vector2d.new(G.unit2(__a)); end

    def normalize!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y = G.unit2(__a)
      self
    end

    def reverse; Vector2d.new(-@x, -@y); end

    def reverse!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x = -@x
      @y = -@y
      self
    end

    def valid?; G.len2(__a) > 0.0; end
    def unit_vector?; (G.len2(__a) - 1.0).abs < G::TOL; end

    def __nonzero!(o)
      v = G.vec2(o)
      raise ArgumentError, 'Cannot create unit vector from zero length vector' if v.all?(&:zero?) || G.len2(__a).zero?
      v
    end
    private :__nonzero!

    def parallel?(o)
      v = __nonzero!(o)
      a = G.unit2(__a)
      b = G.unit2(v)
      G.same_dir?(a, b) || G.same_dir?(a, [-b[0], -b[1]])
    end

    def perpendicular?(o)
      v = __nonzero!(o)
      a = G.unit2(__a)
      b = G.unit2(v)
      (a[0] * b[0] + a[1] * b[1]).abs < G::TOL
    end

    def same_direction?(o)
      v = __nonzero!(o)
      G.same_dir?(G.unit2(__a), G.unit2(v))
    end

    def angle_between(o)
      v = G.vec2(o)
      lp = G.len2(__a) * G.len2(v)
      return 0.0 if lp.zero?
      Math.acos([[(@x * v[0] + @y * v[1]) / lp, 1.0].min, -1.0].max)
    end

    def transform(t); G.xform2(t) * self; end

    def transform!(t)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y = (G.xform2(t) * self).__a
      self
    end

    def inspect; "Vector2d(#{G.g(@x)}, #{G.g(@y)})"; end
    def to_s; "(#{G.g(@x)}, #{G.g(@y)})"; end
  end

  # Affine 2D transformation stored as [xaxis.x, xaxis.y, yaxis.x, yaxis.y, tx, ty].
  class Transformation2d
    IDENT = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0].freeze

    def self.__from(m)
      t = allocate
      t.instance_variable_set(:@m, m)
      t
    end

    def self.translation(v)
      d = v.is_a?(Geom::Point2d) ? G.pt2(v) : G.vec2(v)
      __from([1.0, 0.0, 0.0, 1.0, d[0], d[1]])
    end

    def self.rotation(point, angle)
      o = G.pt2(point)
      a = G.num(angle)
      c = Math.cos(a)
      s = Math.sin(a)
      __from([c, s, -s, c, o[0] - (c * o[0] - s * o[1]), o[1] - (s * o[0] + c * o[1])])
    end

    # scaling(s), scaling(x, y), scaling(point, s), scaling(point, x, y)
    def self.scaling(*a)
      case a.size
      when 1 then s = [G.num(a[0])] * 2
      when 2
        if a[0].is_a?(Numeric)
          s = [G.num(a[0]), G.num(a[1])]
        else
          o = G.pt2(a[0])
          s = [G.num(a[1])] * 2
        end
      when 3
        o = G.pt2(a[0])
        s = [G.num(a[1]), G.num(a[2])]
      else raise ArgumentError, 'Wrong number of arguments'
      end
      m = [s[0], 0.0, 0.0, s[1], 0.0, 0.0]
      if o
        m[4] = o[0] - s[0] * o[0]
        m[5] = o[1] - s[1] * o[1]
      end
      __from(m)
    end

    def initialize(*a)
      @m = IDENT.dup
      return if a.empty?
      raise ArgumentError, 'Wrong number of arguments' unless a.size == 1
      set!(a[0])
    end

    def to_a; @m.dup; end
    def clone; Transformation2d.__from(@m.dup); end

    def set!(a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      case a
      when Transformation2d then @m = a.to_a
      when Array
        if a.size == 6 && G.numbers?(a)
          @m = a.map(&:to_f)
        elsif a.size == 2 && G.numbers?(a)
          @m = Transformation2d.translation(a).to_a
        else raise TypeError, 'Not a valid argument type'
        end
      else raise TypeError, 'Not a valid argument type'
      end
      self
    end

    def ==(o)
      o.is_a?(Transformation2d) && to_a.zip(o.to_a).all? { |a, b| (a - b).abs < G::TOL }
    end

    def identity?; @m.each_with_index.all? { |v, i| (v - IDENT[i]).abs < G::TOL }; end

    def *(o)
      m = @m
      case o
      when Point2d
        x, y = o.__a
        Point2d.new(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
      when Vector2d
        x, y = o.__a
        Vector2d.new(m[0] * x + m[2] * y, m[1] * x + m[3] * y)
      when Transformation2d
        b = o.to_a
        Transformation2d.__from([m[0] * b[0] + m[2] * b[1], m[1] * b[0] + m[3] * b[1],
                                 m[0] * b[2] + m[2] * b[3], m[1] * b[2] + m[3] * b[3],
                                 m[0] * b[4] + m[2] * b[5] + m[4], m[1] * b[4] + m[3] * b[5] + m[5]])
      when Array
        raise TypeError, 'Not a valid argument type' unless o.size == 2 && G.numbers?(o)
        (self * Point2d.new(o)).to_a
      else raise TypeError, 'Not a valid argument type'
      end
    end

    def __inv
      m = @m
      det = m[0] * m[3] - m[1] * m[2]
      raise ArgumentError, 'Transformation is not invertible' if det.abs < 1e-12
      i = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det]
      [i[0], i[1], i[2], i[3], -(i[0] * m[4] + i[2] * m[5]), -(i[1] * m[4] + i[3] * m[5])]
    end
    private :__inv

    def inverse; Transformation2d.__from(__inv); end

    def invert!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @m = __inv
      self
    end
  end

  # A rectangle in 2D (view coordinates): upper-left and lower-right corners.
  class Bounds2d
    def initialize(*a)
      @p = [0.0, 0.0, 0.0, 0.0]
      set!(*a) unless a.empty?
    end

    # Comme SketchUp : une autre Bounds2d, deux coins (haut gauche, bas
    # droit), ou x, y, largeur, hauteur, en arguments ou dans un tableau.
    def __args(a)
      if a.size == 1
        return a[0].instance_variable_get(:@p).dup if a[0].is_a?(Bounds2d)
        raise TypeError, 'wrong argument type' unless a[0].is_a?(Array)
        a = a[0]
      end
      case a.size
      when 2 then G.pt2(a[0]) + G.pt2(a[1])
      when 4
        x, y, w, h = a.map { |v| Esq.num2dbl(v) }
        [x, y, x + w, y + h]
      else raise ArgumentError, 'Wrong number of arguments'
      end
    end
    private :__args

    def set!(*a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @p = __args(a)
      self
    end

    def ==(o)
      o.is_a?(Bounds2d) && G.near2?(@p[0, 2], o.to_a[0].__a) && G.near2?(@p[2, 2], o.to_a[1].__a)
    end

    def upper_left; Point2d.new(@p[0], @p[1]); end
    def lower_right; Point2d.new(@p[2], @p[3]); end
    def width; @p[2] - @p[0]; end
    def height; @p[3] - @p[1]; end
    def to_a; [upper_left, lower_right]; end
  end

  # Gelées comme dans SketchUp.
  ORIGIN_2D = Point2d.new(0, 0).freeze
  X_AXIS_2D = Vector2d.new(1, 0).freeze
  Y_AXIS_2D = Vector2d.new(0, 1).freeze
  IDENTITY_2D = Transformation2d.new.freeze

  # ---------------------------------------------------------------- PolygonMesh
  # Points and polygons are numbered from 1. A polygon lists point indices; a negative index marks a
  # hidden/soft edge in SketchUp (the sign is kept, the index is the absolute value).
  class PolygonMesh
    NO_SMOOTH_OR_HIDE = 0
    HIDE_BASED_ON_INDEX = 1
    SOFTEN_BASED_ON_INDEX = 2
    AUTO_SOFTEN = 4
    SMOOTH_SOFT_EDGES = 8
    MESH_POINTS = 0
    MESH_UVQ_FRONT = 1
    MESH_UVQ_BACK = 2
    MESH_NORMALS = 4

    def initialize(npoints = 0, npolygons = nil)
      a = G.int(npoints)
      b = npolygons.nil? ? a : G.int(npolygons)
      raise RangeError, 'Number of points and/or polygons must be 0 or greater' if a < 0 || b < 0
      @pts = []
      @polys = []
      @uvq = { true => [], false => [] }
      @normals = []
    end

    def count_points; @pts.size; end
    def count_polygons; @polys.size; end

    def __pidx(i)
      n = G.int(i).abs
      n.zero? || n > @pts.size ? nil : n
    end
    private :__pidx

    def add_point(*a)
      raise ArgumentError, 'Not enough arguments: Point required' if a.empty?
      p = G.pt(a[0])
      i = @pts.index { |q| G.near?(p, q) }
      return i + 1 if i
      @pts << p
      @pts.size
    end

    def point_index(pt)
      p = G.pt(pt)
      i = @pts.index { |q| G.near?(p, q) }
      i ? i + 1 : 0
    end

    def point_at(i)
      n = __pidx(i) or return nil
      Point3d.new(@pts[n - 1])
    end

    def set_point(i, pt)
      p = G.pt(pt)
      n = G.int(i).abs
      raise IndexError, 'Index out of range' if n.zero? || n > @pts.size
      @pts[n - 1] = p
      self
    end

    def points; @pts.map { |p| Point3d.new(p) }; end

    def add_polygon(*a)
      raise ArgumentError, 'Not enough points - at least 3 required' if a.empty?
      # Un seul tableau : ce sont ses éléments, points ou indices ;
      # add_polygon([1, 2, 3]) prend donc trois indices, pas un point.
      a = a[0] if a.size == 1 && a[0].is_a?(Array)
      a = a[0] if a[0].is_a?(Array) && !G.point_array?(a[0])
      raise ArgumentError, 'Not enough points - at least 3 required' if a.size < 3
      idx = a.map do |e|
        if G.point_like?(e) || G.positioned?(e)
          add_point(e)
        else
          n = G.int(e)
          raise ArgumentError, "point index #{n} out of range" if n.zero? || n.abs > @pts.size
          n
        end
      end
      @polys << idx
      @polys.size
    end

    def polygon_at(i)
      n = G.int(i)
      n < 1 || n > @polys.size ? nil : @polys[n - 1].dup
    end

    def polygon_points_at(i)
      n = G.int(i)
      return nil if n < 1 || n > @polys.size
      @polys[n - 1].map { |k| Point3d.new(@pts[k.abs - 1]) }
    end

    def polygons; @polys.map(&:dup); end

    def normal_at(i)
      n = __pidx(i) or return nil
      return Vector3d.new(@normals[n - 1]) if @normals[n - 1]
      sum = [0.0, 0.0, 0.0]
      @polys.each do |poly|
        next unless poly.any? { |k| k.abs == n }
        ps = poly.map { |k| @pts[k.abs - 1] }
        ps.each_with_index do |p, j|
          q = ps[(j + 1) % ps.size]
          sum[0] += (p[1] - q[1]) * (p[2] + q[2])
          sum[1] += (p[2] - q[2]) * (p[0] + q[0])
          sum[2] += (p[0] - q[0]) * (p[1] + q[1])
        end
      end
      Vector3d.new(G.unit(sum))
    end

    # Used by Face#mesh / Entities#fill_from_mesh to attach per-point data.
    def __set_normal(i, v); @normals[i - 1] = G.vec(v); end

    def uvs(front)
      Array.new(@pts.size) { |k| Point3d.new(@uvq[front ? true : false][k] || [0.0, 0.0, 0.0]) }
    end

    def uv_at(i, front)
      n = G.int(i).abs
      return nil if n.zero? || n > @pts.size
      Point3d.new(@uvq[front ? true : false][n - 1] || [0.0, 0.0, 0.0])
    end

    def set_uv(i, uvq, front)
      n = G.int(i).abs
      uv = G.pt(uvq)
      raise RangeError, 'Index cannot be zero - starts at 1' if n.zero?
      raise RangeError, 'Index out of range' if n > @pts.size
      @uvq[front ? true : false][n - 1] = uv
      nil
    end

    def transform!(t)
      t = G.xform(t)
      @pts = @pts.map { |p| (t * Point3d.new(p)).__a }
      @normals = @normals.map { |v| v && G.unit((t * Vector3d.new(v)).__a) }
      self
    end
  end

  # ---------------------------------------------------------------- LatLong / UTM
  module WGS84
    A = 6_378_137.0
    F = 1 / 298.257223563
    K0 = 0.9996
    E2 = F * (2 - F)
    E = Math.sqrt(E2)
    N = F / (2 - F)
    # Krüger series coefficients (order 6 in n)
    ALPHA = [N / 2 - 2 * N**2 / 3 + 5 * N**3 / 16 + 41 * N**4 / 180,
             13 * N**2 / 48 - 3 * N**3 / 5 + 557 * N**4 / 1440,
             61 * N**3 / 240 - 103 * N**4 / 140,
             49_561 * N**4 / 161_280].freeze
    BETA = [N / 2 - 2 * N**2 / 3 + 37 * N**3 / 96 - N**4 / 360,
            N**2 / 48 + N**3 / 15 - 437 * N**4 / 1440,
            17 * N**3 / 480 - 37 * N**4 / 840,
            4397 * N**4 / 161_280].freeze
    AA = A / (1 + N) * (1 + N**2 / 4 + N**4 / 64)
    BANDS = 'CDEFGHJKLMNPQRSTUVWX'.freeze
  end

  class LatLong
    def initialize(*a)
      case a.size
      when 0 then @lat = @lon = 0.0
      when 1
        v = a[0]
        if v.is_a?(LatLong)
          @lat, @lon = v.to_a
        elsif v.is_a?(Array)
          raise ArgumentError, 'wrong number of values in array' unless v.size == 2
          __set(v[0], v[1])
        else raise ArgumentError, 'wrong type - expected Sketchup::LatLong'
        end
      when 2 then __set(a[0], a[1])
      else raise ArgumentError, "wrong # of arguments(#{a.size} for 3)"
      end
    end

    def __set(lat, lon)
      lat = G.num(lat)
      lon = G.num(lon)
      raise ArgumentError, 'Invalid Latitude Value' if lat.abs > 90
      raise ArgumentError, 'Invalid Longitude Value' if lon.abs > 180
      @lat = lat
      @lon = lon
    end
    private :__set

    def latitude; @lat; end
    def longitude; @lon; end
    def to_a; [@lat, @lon]; end

    def to_s
      lat = format('%.5f', @lat.abs) + (@lat >= 0 ? 'N' : 'S')
      lon = format('%.5f', @lon.abs) + (@lon >= 0 ? 'E' : 'W')
      "LatLong(#{lat}, #{lon})"
    end

    def to_utm
      w = WGS84
      if @lat < -80 || @lat > 84
        return __ups
      end
      zone = ((@lon + 180) / 6).floor + 1
      zone = 60 if zone > 60
      lon0 = (zone - 1) * 6 - 180 + 3
      phi = @lat * Math::PI / 180
      dl = (@lon - lon0) * Math::PI / 180
      t = Math.sinh(Math.atanh(Math.sin(phi)) - w::E * Math.atanh(w::E * Math.sin(phi)))
      xi = Math.atan2(t, Math.cos(dl))
      eta = Math.atanh(Math.sin(dl) / Math.sqrt(1 + t * t))
      x = eta
      y = xi
      w::ALPHA.each_with_index do |al, j|
        k = 2 * (j + 1)
        x += al * Math.cos(k * xi) * Math.sinh(k * eta)
        y += al * Math.sin(k * xi) * Math.cosh(k * eta)
      end
      easting = 500_000.0 + w::K0 * w::AA * x
      northing = w::K0 * w::AA * y
      northing += 10_000_000.0 if @lat < 0
      idx = ((@lat + 80) / 8).floor
      idx = 19 if idx > 19
      UTM.new(zone, w::BANDS[idx], easting, northing)
    end

    def __ups
      w = WGS84
      north = @lat > 0
      phi = (north ? @lat : -@lat) * Math::PI / 180
      lam = @lon * Math::PI / 180
      e = w::E
      t = Math.tan(Math::PI / 4 - phi / 2) / ((1 - e * Math.sin(phi)) / (1 + e * Math.sin(phi)))**(e / 2)
      rho = 2 * w::A * 0.994 * t / Math.sqrt((1 + e)**(1 + e) * (1 - e)**(1 - e))
      x = 2_000_000.0 + rho * Math.sin(lam)
      y = north ? 2_000_000.0 - rho * Math.cos(lam) : 2_000_000.0 + rho * Math.cos(lam)
      letter = north ? (@lon >= 0 ? 'Z' : 'Y') : (@lon >= 0 ? 'B' : 'A')
      UTM.new(0, letter, x, y)
    end
    private :__ups
  end

  class UTM
    def initialize(*a)
      case a.size
      when 0
        @zone = 0
        @letter = 'A'
        @x = @y = 0.0
      when 1
        v = a[0]
        if v.is_a?(UTM)
          @zone, @letter, @x, @y = v.to_a
        elsif v.is_a?(Array)
          raise ArgumentError, 'wrong number of values in array' unless v.size == 4
          __set(*v)
        elsif v.is_a?(LatLong)
          @zone, @letter, @x, @y = v.to_utm.to_a
        else raise ArgumentError, 'wrong type - expected Sketchup::UTM'
        end
      when 4 then __set(*a)
      else raise ArgumentError, "wrong # of arguments(#{a.size} for 3)"
      end
    end

    def __set(zone, letter, x, y)
      @zone = G.int(zone)
      @letter = letter.to_s[0] || 'A'
      @x = G.num(x)
      @y = G.num(y)
    end
    private :__set

    def zone_number; @zone; end
    def zone_letter; @letter.dup; end
    def x; @x; end
    def y; @y; end
    def to_a; [@zone, @letter.dup, @x, @y]; end
    def to_s; format('UTM(%d %s %.5f %.5f)', @zone, @letter, @x, @y); end

    def to_latlong
      w = WGS84
      north = @letter >= 'N'
      if @zone.zero?
        return __from_ups(north)
      end
      e = @x - 500_000.0
      n = north ? @y : @y - 10_000_000.0
      xi = n / (w::K0 * w::AA)
      eta = e / (w::K0 * w::AA)
      xi_p = xi
      eta_p = eta
      w::BETA.each_with_index do |b, j|
        k = 2 * (j + 1)
        xi_p -= b * Math.sin(k * xi) * Math.cosh(k * eta)
        eta_p -= b * Math.cos(k * xi) * Math.sinh(k * eta)
      end
      chi = Math.asin(Math.sin(xi_p) / Math.cosh(eta_p))
      # conformal -> geodetic latitude (series in n)
      n1 = w::N
      d = [2 * n1 - 2 * n1**2 / 3 - 2 * n1**3 + 116 * n1**4 / 45,
           7 * n1**2 / 3 - 8 * n1**3 / 5 - 227 * n1**4 / 45,
           56 * n1**3 / 15 - 136 * n1**4 / 35,
           4279 * n1**4 / 630]
      phi = chi
      d.each_with_index { |dj, j| phi += dj * Math.sin(2 * (j + 1) * chi) }
      lon0 = (@zone - 1) * 6 - 180 + 3
      lam = Math.atan2(Math.sinh(eta_p), Math.cos(xi_p))
      LatLong.new(phi * 180 / Math::PI, lon0 + lam * 180 / Math::PI)
    end

    def __from_ups(_north)
      w = WGS84
      north = @letter >= 'Y'
      dx = @x - 2_000_000.0
      dy = north ? 2_000_000.0 - @y : @y - 2_000_000.0
      rho = Math.sqrt(dx * dx + dy * dy)
      e = w::E
      t = rho * Math.sqrt((1 + e)**(1 + e) * (1 - e)**(1 - e)) / (2 * w::A * 0.994)
      chi = Math::PI / 2 - 2 * Math.atan(t)
      phi = chi
      6.times { phi = Math::PI / 2 - 2 * Math.atan(t * ((1 - e * Math.sin(phi)) / (1 + e * Math.sin(phi)))**(e / 2)) }
      lam = rho.zero? ? 0.0 : Math.atan2(dx, dy)
      lat = phi * 180 / Math::PI
      LatLong.new(north ? lat : -lat, lam * 180 / Math::PI)
    end
    private :__from_ups
  end
end

# SketchUp defines these at the top level.
ORIGIN_2D = Geom::ORIGIN_2D unless defined?(ORIGIN_2D)
X_AXIS_2D = Geom::X_AXIS_2D unless defined?(X_AXIS_2D)
Y_AXIS_2D = Geom::Y_AXIS_2D unless defined?(Y_AXIS_2D)
IDENTITY_2D = Geom::IDENTITY_2D unless defined?(IDENTITY_2D)
