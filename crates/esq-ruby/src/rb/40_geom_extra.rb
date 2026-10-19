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
        # SUVector2DSetLength: neither the vector nor the distance under 1e-10.
        d = G.num(args[1])
        l = G.len2(v)
        raise err, (err == TypeError ? 'Zero length vector)' : 'Zero distance is not allowed') if l < 1e-10 || d.abs < 1e-10
        k = d / l
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

    # a Vector2d first is copied whatever follows it.
    def set!(*a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      raise ArgumentError, 'wrong number arguments' if a.empty?
      return (@x, @y = a[0].__a) && self if a[0].is_a?(Vector2d)
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

    # `*` is the cross product: a number is no vector (TypeError).
    def cross(o); b = G.vec2(o); @x * b[1] - @y * b[0]; end
    alias * cross

    def -@; Vector2d.new(-@x, -@y); end # extension

    def length; Length.new(G.len2(__a)); end

    # SUVector2DSetLength: neither a length nor a vector shorter than the
    # tolerance; the coordinates are scaled by length / current length.
    def length=(l)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      l = G.num(l)
      cur = Math.sqrt(@y * @y + @x * @x)
      raise ArgumentError, 'Cannot set the length of a zero length vector' if l.abs < 1e-10 || cur < 1e-10
      k = l / cur
      @x *= k
      @y *= k
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

    # SUVector2DAngleBetween: both made unit, then the arc cosine of their
    # dot product, clamped to [-1, 1] (a zero vector stays zero).
    def angle_between(o)
      v = G.vec2(o)
      l = Math.sqrt(@x * @x + @y * @y)
      sx, sy = l.zero? ? [0.0, 0.0] : [@x / l, @y / l]
      m = Math.sqrt(v[1] * v[1] + v[0] * v[0])
      ox, oy = m.zero? ? [0.0, 0.0] : [v[0] / m, v[1] / m]
      Math.acos((oy * sy + ox * sx).clamp(-1.0, 1.0))
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
  # SketchUp keeps 6 doubles [xx, xy, yx, yy, tx, ty] (x' = xx·x + xy·y + tx) but computes in
  # single precision (CTransformation2d: SUTransformation2DRotation, …Multiply, …GetInverse
  # turn the doubles into floats and back).
  class Transformation2d
    IDENT = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0].freeze

    # A double rounded to the nearest float (one float operation is the rounded double one).
    def self.f(x); [x].pack('f').unpack1('f'); end

    def self.__from(m)
      t = allocate
      t.instance_variable_set(:@m, m)
      t
    end

    def self.translation(v)
      d = v.is_a?(Geom::Point2d) ? G.pt2(v) : G.vec2(v)
      __from([1.0, 0.0, 0.0, 1.0, f(d[0]), f(d[1])])
    end

    # cosf, sinf of the angle as a float, about the point as floats.
    def self.rotation(point, angle)
      o = G.pt2(point)
      x = f(o[0])
      y = f(o[1])
      a = f(G.num(angle))
      c = f(Math.cos(a))
      s = f(Math.sin(a))
      __from([c, -s, s, c, f(x - f(f(x * c) - f(y * s))), f(y - f(f(x * s) + f(y * c)))])
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
      s = s.map { |v| f(v) }
      m = [s[0], 0.0, 0.0, s[1], 0.0, 0.0]
      if o
        x = f(o[0])
        y = f(o[1])
        m[4] = f(x - f(s[0] * x))
        m[5] = f(y - f(s[1] * y))
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

    # A point or a vector: the matrix as floats, the coordinates as doubles.
    def *(o)
      m = @m.map { |v| Transformation2d.f(v) }
      case o
      when Point2d
        x, y = o.__a
        Point2d.new(m[1] * y + m[0] * x + m[4], m[3] * y + m[2] * x + m[5])
      when Vector2d
        x, y = o.__a
        Vector2d.new(m[1] * y + m[0] * x, m[3] * y + m[2] * x)
      when Transformation2d
        # in floats.
        a = m
        b = o.to_a.map { |v| Transformation2d.f(v) }
        g = ->(v) { Transformation2d.f(v) }
        Transformation2d.__from([g.(g.(b[2] * a[1]) + g.(b[0] * a[0])), g.(g.(b[3] * a[1]) + g.(b[1] * a[0])),
                                 g.(g.(a[3] * b[2]) + g.(a[2] * b[0])), g.(g.(a[3] * b[3]) + g.(a[2] * b[1])),
                                 g.(g.(g.(b[5] * a[1]) + g.(b[4] * a[0])) + a[4]),
                                 g.(g.(g.(b[5] * a[3]) + g.(b[4] * a[2])) + a[5])])
      when Array
        raise TypeError, 'Not a valid argument type' unless o.size == 2 && G.numbers?(o)
        (self * Point2d.new(o)).to_a
      else raise TypeError, 'Not a valid argument type'
      end
    end

    # in floats; not when the determinant is under 0.001⁴.
    def __inv
      g = ->(v) { Transformation2d.f(v) }
      a = @m.map { |v| g.(v) }
      det = g.(g.(a[3] * a[0]) - g.(a[2] * a[1]))
      raise ArgumentError, 'Transformation is not invertible' unless 1e-12 <= det.abs
      k = g.(1.0 / det)
      [g.(k * a[3]), g.(-a[1] * k), g.(-a[2] * k), g.(k * a[0]),
       g.(g.(g.(a[5] * a[1]) - g.(a[4] * a[3])) * k), g.(g.(g.(a[4] * a[2]) - g.(a[5] * a[0])) * k)]
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
  # The UTM conversions of SketchUp: Snyder's transverse
  # Mercator series on the WGS 84 ellipsoid of its datum table (SketchUp forward,
  # inverse, the meridian arc), computed in its order.
  module WGS84
    A = 6_378_137.0
    F = 1.0 / 298.257223563
    K0 = 0.9996
    E2 = (F + F) - F * F
    E = Math.sqrt(E2)
    D2R = 0.017453292519943295

    module_function

    def arc(phi)
      return 0.0 if phi == 0.0
      e2 = E2
      d1 = e2 * 3.0
      d3 = 1.0 - e2 * 0.25
      d4 = d1 * e2
      d5 = d4 * 0.015625
      d9 = e2 * 5.0 * e2 * e2 * 0.00390625
      d2 = e2 * 45.0 * e2 * e2 * 0.0009765625
      s2 = Math.sin(phi + phi)
      d4 *= 0.03125
      d1 *= 0.125
      s4 = Math.sin(phi * 4.0)
      d10 = e2 * 15.0 * e2 * 0.00390625
      s6 = Math.sin(phi * 6.0)
      ((((d3 - d5) - d9) * phi - s2 * (d4 + d1 + d2)) + s4 * (d10 + d2) - s6 * ((e2 * 35.0 * e2 * e2) / 3072.0)) * A
    end

    # Latitude and longitude (degrees) to x, y around the meridian cm.
    def forward(lat, lon, cm)
      e2 = E2
      ep2 = e2 / (1.0 - e2)
      phi = lat * D2R
      n = A / Math.sqrt(1.0 - (Math.sin(phi)**2.0) * e2)
      tn = Math.tan(phi)
      t = tn**2.0
      cs = Math.cos(phi)
      c = (cs**2.0) * ep2
      a = (lon * D2R - cm * D2R) * cs
      x = ((a**5.0 * (((5.0 - t * 18.0) + t * t + c * 72.0) - ep2 * 58.0)) / 120.0 +
           (((1.0 - t) + c) * a * a * a) / 6.0 + a) * n * K0
      q4 = (a**4.0 * ((5.0 - t) + c * 9.0 + c * 4.0 * c)) / 24.0
      q6 = (a**6.0 * (((61.0 - t * 58.0) + t * t + c * 600.0) - ep2 * 330.0)) / 720.0
      y = ((arc(phi) - arc(0.0)) + (q4 + a * a * 0.5 + q6) * tn * n) * K0
      [x, y]
    end

    # x, y around the meridian cm to latitude and longitude (degrees).
    def inverse(x, y, cm)
      e2 = E2
      d5 = 1.0 - e2
      ep2 = e2 / d5
      sq = Math.sqrt(d5)
      e1 = (1.0 - sq) / (sq + 1.0)
      mu = (arc(0.0) + y / K0) / ((((1.0 - e2 * 0.25) - e2 * 3.0 * e2 * 0.015625) - e2 * 5.0 * e2 * e2 * 0.00390625) * A)
      e14 = e1**4.0
      e13 = e1**3.0
      phi = Math.sin(mu + mu) * (e1 * 3.0 * 0.5 - e13 * 27.0 * 0.03125) + mu +
            Math.sin(mu * 4.0) * (e1 * 21.0 * e1 * 0.0625 - e14 * 55.0 * 0.03125) +
            Math.sin(mu * 6.0) * ((e13 * 151.0) / 96.0) + Math.sin(mu * 8.0) * e14 * 1097.0 * 0.001953125
      cs = Math.cos(phi)
      c = (cs**2.0) * ep2
      tn = Math.tan(phi)
      t = tn**2.0
      w = 1.0 - (Math.sin(phi)**2.0) * e2
      n = A / Math.sqrt(w)
      c3 = c * 3.0 * c
      d = x / (n * K0)
      q4 = (d**4.0 * (((t * 3.0 + 5.0 + c * 10.0) - c * 4.0 * c) - ep2 * 9.0)) / 24.0
      q6 = (d**6.0 * ((((t * 90.0 + 61.0 + c * 298.0) + t * 45.0 * t) - ep2 * 252.0) - c3)) / 720.0
      lat = (phi - ((n * tn) / ((A * d5) / w**1.5)) * ((d * d * 0.5 - q4) + q6)) / D2R
      q3 = (d**3.0 * (t + t + 1.0 + c)) / 6.0
      q5 = (d**5.0 * ((((5.0 - (c + c)) + t * 28.0) - c3) + ep2 * 8.0 + t * 24.0 * t)) / 120.0
      lon = (((d - q3) + q5) / cs + cm * D2R) / D2R
      [lat, lon]
    end
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

    # two numbers (rb_num2dbl), kept as given (TestUp test_latitude_integers
    # makes one of 123).
    def __set(lat, lon)
      @lat = G.num(lat)
      @lon = G.num(lon)
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
      return __ups if @lat < -80.0 || @lat > 84.0
      # the band from C, I and O left out (80° to 84° gives Y as in SketchUp);
      # the meridian of the 6° zone; the south is 10 000 km less the distance.
      i = (@lat + 80.0).to_i / 8
      letter = i + 'C'.ord
      letter += 1 if letter >= 'I'.ord
      letter += 1 if letter >= 'O'.ord
      cm = (@lon / 6.0).floor * 6.0 + 3.0
      x, y = WGS84.forward(@lat, @lon, cm)
      y = 10_000_000.0 - y if @lat < 0.0
      UTM.new((cm.to_i + 183) / 6, letter.chr, x + 500_000.0, y)
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
        else
          # a UTM or an array, nothing else (rb_check_type).
          raise TypeError, "wrong argument type #{v.nil? ? 'nil' : v.class} (expected Array)"
        end
      when 4 then __set(*a)
      else raise ArgumentError, "wrong # of arguments(#{a.size} for 3)"
      end
    end

    # The zone by rb_num2long, the band by StringValue, the coordinates by rb_num2dbl.
    def __set(zone, letter, x, y)
      @zone = G.int(zone)
      raise TypeError, "no implicit conversion of #{letter.nil? ? 'nil' : letter.class} into String" unless letter.is_a?(String)
      @letter = letter[0] || 'A'
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
      north = @letter >= 'N'
      return __from_ups(north) if @zone.zero?
      # the south counted from 10 000 km; the meridian of the zone.
      y = north ? @y : 10_000_000.0 - @y
      lat, lon = WGS84.inverse(@x - 500_000.0, y, (@zone * 6 - 183).to_f)
      LatLong.new(lat, lon)
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
