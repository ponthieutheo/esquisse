# Esquisse — SketchUp's geometry classes (zone « geom »): Length and unit helpers, Geom::Point3d,
# Vector3d, Transformation, BoundingBox, the Array/Numeric/String extensions and the module
# functions of Geom. Written from the behaviour read in SketchUp.exe (messages, return types,
# conversions); lengths are inches like everywhere in SketchUp's Ruby API.
#
# Internal helpers live in Esq::G (conversions that SketchUp does in C) and Esq::Units
# (Length#to_s / String#to_l, which follow the model's unit settings).

module Esq
  module G
    TOL = 0.001 # SketchUp's global tolerance, in inches

    module_function

    # rb_num2dbl
    def num(v)
      case v
      when Float then v
      when Numeric then v.to_f
      when nil then raise TypeError, "can't convert nil into Float"
      when true, false then raise TypeError, "can't convert #{v} into Float"
      when String then raise TypeError, 'no implicit conversion to float from string'
      else raise TypeError, "can't convert #{v.class} into Float"
      end
    end

    # rb_num2long
    def int(v)
      case v
      when Integer then v
      when Numeric then v.to_i
      when nil then raise TypeError, 'no implicit conversion from nil to integer'
      when true, false then raise TypeError, "no implicit conversion of #{v} into Integer"
      when String then raise TypeError, 'no implicit conversion of String into Integer'
      else raise TypeError, "no implicit conversion of #{v.class} into Integer"
      end
    end

    def numbers?(a)
      a.all? { |e| e.is_a?(Numeric) }
    end

    # An Array that can stand for a point (2 or 3 numbers).
    def point_array?(o)
      o.is_a?(Array) && (o.size == 2 || o.size == 3) && numbers?(o)
    end

    # Anything convertible to a point: [x, y, z]
    def pt(v)
      case v
      when Geom::Point3d then v.__a
      when Array
        raise ArgumentError, 'wrong number of values in array' unless v.size == 2 || v.size == 3
        [num(v[0]), num(v[1]), v.size == 3 ? num(v[2]) : 0.0]
      else
        if positioned?(v) && (pos = v.position).is_a?(Geom::Point3d) # Vertex, InputPoint
          pos.__a
        else
          raise ArgumentError, 'Cannot convert argument to Geom::Point3d'
        end
      end
    end

    # SketchUp also takes the position of a Vertex or an InputPoint where a point is expected.
    def positioned?(v)
      (defined?(Sketchup::Vertex) && v.is_a?(Sketchup::Vertex)) ||
        (defined?(Sketchup::InputPoint) && v.is_a?(Sketchup::InputPoint))
    end

    # Anything convertible to a vector: [x, y, z]
    def vec(v)
      case v
      when Geom::Vector3d then v.__a
      when Array
        raise ArgumentError, 'wrong number of values in array' unless v.size == 2 || v.size == 3
        [num(v[0]), num(v[1]), v.size == 3 ? num(v[2]) : 0.0]
      else raise ArgumentError, 'Cannot convert argument to Geom::Vector3d'
      end
    end

    def point_like?(v)
      v.is_a?(Geom::Point3d) || point_array?(v)
    end

    def add(a, b); [a[0] + b[0], a[1] + b[1], a[2] + b[2]]; end
    # A vector argument made a unit vector: a zero one (every coordinate within
    # 1e-10, SUVector3DIsValid) raises ArgumentError.
    def unit_arg(v)
      a = vec(v)
      raise ArgumentError, 'Cannot create unit vector from zero length vector' if a.all? { |c| c.abs <= 1e-10 }
      unit(a)
    end

    # u × a + v × b, as a Vector3d (u and v: vectors).
    def combine(u, a, v, b); Geom::Vector3d.new(add(scale(u.to_a, a), scale(v.to_a, b))); end
    def sub(a, b); [a[0] - b[0], a[1] - b[1], a[2] - b[2]]; end
    def dot(a, b); a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; end
    def scale(a, k); [a[0] * k, a[1] * k, a[2] * k]; end
    def cross(a, b)
      [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    end
    def len(a); Math.sqrt(dot(a, a)); end

    # set!(x[, y[, z]]) or set!([x, y, z]): assigns the coordinates given, in order (at most 3).
    def set_partial(obj, args)
      args = args[0] if args[0].is_a?(Array)
      args.first(3).each_with_index { |v, i| obj[i] = num(v) }
    end

    # Unit vectors equal within the tolerance, component by component.
    def same_dir?(u, v)
      u.each_index.all? { |i| (u[i] - v[i]).abs <= TOL }
    end

    def unit(a)
      l = len(a)
      l.zero? ? [0.0, 0.0, 0.0] : [a[0] / l, a[1] / l, a[2] / l]
    end

    # SUPoint3DGetEqual: every coordinate and the distance within the tolerance
    def near?(a, b)
      d = sub(a, b)
      d[0].abs < TOL && d[1].abs < TOL && d[2].abs < TOL && dot(d, d) < TOL * TOL
    end

    # Index of Point3d/Vector3d (negative counts from the end).
    def index3(i)
      i = int(i)
      i += 3 if i < 0
      raise IndexError, 'index out of range [0,2]' unless (0..2).cover?(i)
      i
    end

    def index2(i)
      i = int(i)
      i += 2 if i < 0
      raise IndexError, 'index out of range [0,1]' unless (0..1).cover?(i)
      i
    end

    # A line: [point, vector] or [point, point], also given as separate arguments. -> [pt, unit dir]
    def line(*args)
      if args.size == 1
        a = args[0]
        raise TypeError, "wrong argument type #{a.class} (expected Array)" unless a.is_a?(Array)
        return line(*a)
      end
      raise ArgumentError, 'Wrong number of arguments for line' if args.size < 2
      p = pt(args[0])
      if args[1].is_a?(Geom::Vector3d) || point_array?(args[1])
        # a Vector3d (or an array, which counts as a vector here) gives the direction
        v = unit(vec(args[1]))
        raise ArgumentError, 'Cannot create unit vector from zero length vector' if v.all?(&:zero?)
        [p, v]
      else
        [p, unit(sub(pt(args[1]), p))]
      end
    end

    # A plane: [a, b, c, d], [point, normal], [p1, p2, p3] or the same as arguments. -> [a, b, c, d]
    def plane(*args)
      if args.size == 1
        a = args[0]
        raise TypeError, "wrong argument type #{a.class} (expected Array)" unless a.is_a?(Array)
        return plane(*a)
      end
      case args.size
      when 2
        n = unit(vec(args[1]))
        p = pt(args[0])
        [n[0], n[1], n[2], -dot(n, p)]
      when 3
        p0, p1, p2 = args.map { |x| pt(x) }
        n = unit(cross(sub(p1, p0), sub(p2, p0)))
        [n[0], n[1], n[2], -dot(n, p0)]
      when 4
        a, b, c, d = args.map { |x| num(x) }
        l = len([a, b, c])
        l.zero? ? [a, b, c, d] : [a / l, b / l, c / l, d / l]
      else raise ArgumentError, 'Wrong number of arguments for plane'
      end
    end

    # Argument of transform/transform!/interpolate/set!: a Transformation, or what can make one
    # (point or vector: translation, 16 numbers: matrix, number: uniform scale).
    def xform(t)
      case t
      when Geom::Transformation then t
      when Geom::Point3d, Geom::Vector3d then Geom::Transformation.translation(t)
      when Array
        if t.size == 16
          Geom::Transformation.new(t.map { |v| num(v) })
        else
          Geom::Transformation.translation(Geom::Vector3d.new(vec(t)))
        end
      when Numeric then Geom::Transformation.scaling(num(t))
      else raise TypeError, 'no implicit conversion to Transformation'
      end
    end

    # 4x4 column-major product
    def mul16(a, b)
      r = Array.new(16, 0.0)
      4.times do |c|
        4.times do |row|
          r[c * 4 + row] = a[row] * b[c * 4] + a[4 + row] * b[c * 4 + 1] + a[8 + row] * b[c * 4 + 2] + a[12 + row] * b[c * 4 + 3]
        end
      end
      r
    end

    def inv16(m)
      n = 4
      a = (0...n).map { |r| (0...n).map { |c| m[c * 4 + r] } + (0...n).map { |c| c == r ? 1.0 : 0.0 } }
      n.times do |i|
        p = (i...n).max_by { |r| a[r][i].abs }
        return nil if a[p][i].abs < 1e-12
        a[i], a[p] = a[p], a[i]
        d = a[i][i]
        a[i] = a[i].map { |v| v / d }
        n.times do |r|
          next if r == i
          f = a[r][i]
          a[r] = a[r].each_with_index.map { |v, k| v - f * a[i][k] }
        end
      end
      (0...n).flat_map { |c| (0...n).map { |r| a[r][n + c] } }
    end

    # "%g"-style number as C++ streams print them (6 significant digits).
    def g(v)
      format('%g', v)
    end

    def snap(v)
      v.abs < 1e-9 ? 0.0 : v
    end
  end

  # ---------------------------------------------------------------- units
  # Length#to_s and String#to_l follow the model's unit settings (Model Info > Units).
  module Units
    INCHES_PER = { 0 => 1.0, 1 => 12.0, 2 => 1.0 / 25.4, 3 => 1.0 / 2.54, 4 => 1.0 / 0.0254, 5 => 36.0 }.freeze
    SUFFIX = { 0 => '"', 1 => "'", 2 => 'mm', 3 => 'cm', 4 => 'm', 5 => 'yd' }.freeze
    UNIT_FACTOR = { 0 => 1.0, 1 => 1.0 / 12, 2 => 25.4, 3 => 2.54, 4 => 0.0254, 5 => 1.0 / 36 }.freeze
    BRIDGE_UNIT = { 'Millimeters' => 2, 'Centimeters' => 3, 'Meters' => 4, 'Inches' => 0, 'Feet' => 1 }.freeze

    module_function

    # Default precision of a decimal display, per unit, when the model has no UnitsOptions.
    DEFAULT_PRECISION = { 0 => 3, 1 => 3, 2 => 0, 3 => 1, 4 => 3, 5 => 3 }.freeze
    EPS = 1e-6

    # { unit:, format:, precision:, suppress:, force_inch: } — unit 0 in, 1 ft, 2 mm, 3 cm, 4 m,
    # 5 yd; format 0 decimal, 1 architectural, 2 engineering, 3 fractional.
    def settings
      s = { unit: 2, format: 0, precision: nil, suppress: false, force_inch: false }
      begin
        s[:unit] = BRIDGE_UNIT.fetch(Esq.call('model.info')['units'], 2)
      rescue StandardError
        nil
      end
      begin
        o = Sketchup.active_model.options['UnitsOptions']
        if o
          s[:unit] = o['LengthUnit'] if o['LengthUnit']
          s[:format] = o['LengthFormat'] if o['LengthFormat']
          s[:precision] = o['LengthPrecision'] if o['LengthPrecision']
          s[:suppress] = o['SuppressUnitsDisplay'] ? true : false
          s[:force_inch] = o['ForceInchDisplay'] ? true : false
        end
      rescue StandardError, NotImplementedError
        nil
      end
      s[:precision] ||= s[:format].zero? ? DEFAULT_PRECISION.fetch(s[:unit], 3) : 4
      s
    end

    # The decimal separator of the user's regional settings ("." unless the misc zone says otherwise).
    def decimal_separator
      if defined?(Sketchup::RegionalSettings) && Sketchup::RegionalSettings.respond_to?(:decimal_separator)
        Sketchup::RegionalSettings.decimal_separator.to_s
      else
        '.'
      end
    rescue StandardError
      '.'
    end

    # Rounding step of a format/precision (in the formatted unit; inches for the fractional ones).
    def step(fmt, prec)
      return fmt == 2 ? 12.0 : 1.0 if prec.zero?
      case fmt
      when 0 then 1.0 / 10**prec
      when 2 then 12.0 / 10**prec
      else 1.0 / (1 << prec)
      end
    end

    # "n/d" for a fraction of an inch, the smallest power-of-two denominator up to 64; nil if none.
    def fraction_parts(frac)
      (1..6).each do |k|
        d = 1 << k
        n = (d * frac).to_i
        return [n, d] if d * frac - n < EPS
      end
      nil
    end

    # Body of a length (no sign, no "~"), like SketchUp's formatter.
    def body(value, fmt, prec, unit, st)
      whole = (value + EPS).to_i
      frac = prec.zero? ? 0.0 : value - whole
      stp = fmt == 0 || fmt == 2 ? 1.0 / 10**[prec, 0].max : 1.0 / (1 << [prec, 0].max)
      stp = 0.1 if (fmt == 0 || fmt == 2) && prec.zero?
      s =
        case fmt
        when 1 # inches of an architectural length
          if frac - stp > -TOL_F || st[:force_frac]
            n, d = fraction_parts(frac)
            if n.nil? then ''
            elsif whole < 1 then "#{n}/#{d}\""
            else "#{whole} #{n}/#{d}\""
            end
          else
            "#{whole}\""
          end
        when 3 # fractional inches
          t = if frac - stp <= -TOL_F
                whole.to_s
              else
                n, d = fraction_parts(frac)
                if n.nil? then ''
                elsif whole < 1 then "#{n}/#{d}"
                else "#{whole} #{n}/#{d}"
                end
              end
          st[:suppress] ? t : "#{t}\""
        when 2 # engineering: decimal feet
          frac - stp <= -stp && !st[:force_frac] ? "#{whole}'" : format("%.#{prec}f'", value)
        else
          t = format("%.#{prec}f", value)
          unless st[:suppress]
            t += ' ' if unit > 1
            t += SUFFIX.fetch(unit, '"')
          end
          t
        end
      s.tr('.', decimal_separator)
    end
    TOL_F = 0.001

    # Formats a length (inches) like SketchUp's model length formatter: rounded to the precision,
    # "~" in front when that rounding changed the value, "-" for negative values.
    # strip: removes the "~" and the trailing zeros (used by Point3d#to_s, at precision 6).
    def format_length(v, st = nil, precision: nil, strip: false)
      v = v.to_f
      return v.to_s if v.nan? || v.infinite?
      st ||= settings
      st = st.merge(precision: precision) if precision
      fmt = st[:format].to_i
      prec = [st[:precision].to_i, 0].max
      unit = st[:unit].to_i
      neg = v.abs != v
      x = fmt.zero? ? v * UNIT_FACTOR.fetch(unit, 1.0) : v
      a = x.abs
      r = prec.zero? ? (a + 0.5).floor.to_f : ((a / step(fmt, prec)) + 0.5).floor * step(fmt, prec)
      exact = (a - r).abs < TOL_F
      b =
        case fmt
        when 1
          feet = ((r + EPS) / 12.0).to_i
          inch = r - feet * 12.0
          if feet.zero?
            inch < TOL_F ? '0"' : body(inch, 1, prec, unit, st)
          elsif inch < TOL_F
            st[:force_inch] ? "#{feet}' 0\"" : "#{feet}'"
          else
            "#{feet}' " + body(inch, 1, prec, unit, st)
          end
        when 2 then body(r / 12.0, 2, prec, unit, st)
        else body(r, fmt, prec, unit, st)
        end
      out = +''
      # L"~ " : le tilde de SketchUp est suivi d'une espace.
      out << '~ ' unless exact
      out << '-' if neg && r != 0.0
      out << b
      strip ? strip_length(out) : out
    end

    # SketchUp's "strip" variant: no leading "~", no trailing zeros nor bare decimal separator.
    def strip_length(s)
      s = s[1..] if s.start_with?('~')
      sep = decimal_separator
      i = s.rindex(/[0-9#{Regexp.escape(sep)}]/)
      return s if i.nil?
      head = s[0..i]
      tail = s[(i + 1)..]
      if s.include?(sep)
        head = head.sub(/0+\z/, '')
        head = head.chomp(sep)
      end
      head + tail
    end

    UNIT_RE = /\A\s*(\d+(?:\.\d*)?|\.\d+)(?:\s+(\d+)\s*\/\s*(\d+)|\s*\/\s*(\d+))?\s*(mm|cm|km|yd|ft|in|m|"|'|)\s*/i
    UNIT_INCHES = { 'mm' => 1.0 / 25.4, 'cm' => 1.0 / 2.54, 'm' => 1.0 / 0.0254, 'km' => 1000.0 / 0.0254,
                    'yd' => 36.0, 'ft' => 12.0, 'in' => 1.0, '"' => 1.0, "'" => 12.0 }.freeze

    # Parses a length typed by the user (inches), or nil when it is not a length.
    def parse_length(str)
      s = str.to_s.strip.tr(',', '.')
      return nil if s.empty?
      s = s[1..].lstrip if s.start_with?('~') # an approximate length as Length#to_s writes it
      sign = 1.0
      if s.start_with?('-')
        sign = -1.0
        s = s[1..].lstrip
      elsif s.start_with?('+')
        s = s[1..].lstrip
      end
      st = settings
      bare = st[:format].zero? ? INCHES_PER.fetch(st[:unit], 1.0) : 1.0
      total = 0.0
      prev_unit = nil
      terms = 0
      until s.empty?
        s = s.sub(/\A\s*-\s*(?=\d)/, '') if prev_unit == "'" || prev_unit == 'ft'
        m = UNIT_RE.match(s) or return nil
        n = m[1].to_f
        n += m[2].to_f / m[3].to_f if m[2] && m[3].to_f != 0
        n /= m[4].to_f if m[4] && m[4].to_f != 0
        u = m[5].downcase
        f = if u.empty?
              prev_unit == "'" || prev_unit == 'ft' ? 1.0 : bare
            else
              UNIT_INCHES[u]
            end
        total += n * f
        prev_unit = u.empty? ? 'in' : u
        terms += 1
        s = m.post_match
      end
      terms.zero? ? nil : sign * total
    end
  end
end

# ---------------------------------------------------------------- Length
# A float of inches that prints itself with the model's units. Like SketchUp's, it is
# a Numeric that forwards every Float method (so arithmetic gives plain Floats).
class Length < Numeric
  def initialize(obj)
    @length = obj.to_f
    freeze
  end

  # Une variable locale, pas une constante : `Length.constants` est celui de SketchUp.
  own = %i[to_s inspect <=> == < <= > >= is_a? kind_of? ===]
  (Float.instance_methods(true) - Object.instance_methods(true) + [:hash] - own).each do |m|
    define_method(m) { |*a, **k, &b| @length.__send__(m, *a, **k, &b) }
  end

  def ===(other)
    other.is_a?(Length) ? @length === other.to_f : @length === other
  end

  def is_a?(*args)
    super || @length.is_a?(*args)
  end
  alias kind_of? is_a?

  def to_json(*a)
    @length.to_json(*a)
  end

  def to_s
    Esq::Units.format_length(@length)
  end

  def inspect
    @length.inspect
  end

  def <=>(other)
    return 0 if equal?(other)
    return nil unless other.is_a?(Numeric)
    o = other.to_f
    return 0 if (@length - o).abs < Esq::G::TOL
    @length < o ? -1 : 1
  end

  def __cmp_fail(other)
    desc = other.nil? || other == true || other == false ? other.inspect : other.class.to_s
    raise ArgumentError, "comparison of Length with #{desc} failed"
  end

  def ==(other)
    __cmp_fail(other) unless other.is_a?(Numeric)
    (@length - other.to_f).abs < Esq::G::TOL
  end

  def <(other)
    __cmp_fail(other) unless other.is_a?(Numeric)
    @length < other.to_f && !(self == other)
  end

  def <=(other)
    __cmp_fail(other) unless other.is_a?(Numeric)
    @length < other.to_f || self == other
  end

  def >(other)
    __cmp_fail(other) unless other.is_a?(Numeric)
    @length > other.to_f && !(self == other)
  end

  def >=(other)
    __cmp_fail(other) unless other.is_a?(Numeric)
    @length > other.to_f || self == other
  end

  # Model-info constants: format, length/area/volume units.
  Decimal = 0
  Architectural = 1
  Engineering = 2
  Fractional = 3
  Inches = 0
  Feet = 1
  Millimeter = 2
  Centimeter = 3
  Meter = 4
  Yard = 5
  SquareInches = 0
  SquareFeet = 1
  SquareMillimeter = 2
  SquareCentimeter = 3
  SquareMeter = 4
  SquareYard = 5
  CubicInches = 0
  CubicFeet = 1
  CubicMillimeter = 2
  CubicCentimeter = 3
  CubicMeter = 4
  CubicYard = 5
  Liter = 6
  USGallon = 7
end

# ---------------------------------------------------------------- Numeric / String
class Numeric
  # Unit conversions return Lengths (inches); the `to_*` ones give Floats.
  def mm; Length.new(to_f / 25.4); end
  def cm; Length.new(to_f / 2.54); end
  def m; Length.new(to_f / 0.0254); end
  def km; Length.new(to_f / 2.54 * 100_000.0); end
  def inch; Length.new(to_f); end
  def feet; Length.new(to_f * 12.0); end
  def yard; Length.new(to_f * 36.0); end
  def mile; Length.new(to_f * 63_360.0); end
  def to_mm; to_f * 25.4; end
  def to_cm; to_f * 2.54; end
  def to_m; to_f * 0.0254; end
  def to_km; to_f * 2.54 / 100_000.0; end
  def to_inch; to_f; end
  def to_feet; to_f / 12.0; end
  def to_yard; to_f / 36.0; end
  def to_mile; to_f / 63_360.0; end
  def to_l; Length.new(to_f); end
  # Les facteurs de SUDegreesToRadians / SURadiansToDegrees (SketchUp.exe, 0x140d68570) : le
  # premier est arrondi à 15 décimales, d'où `90.degrees == 1.570796326794897`.
  def degrees; to_f * 0.0174532925199433; end
  def radians; to_f * 57.29577951308232; end
end

class String
  # "1m", "12\"", "3'" … → Length (bare numbers are in the model's unit)
  def to_l
    # Une chaîne vide vaut 0 pour l'analyseur de SketchUp (SULengthFormatterParseString).
    return Length.new(0.0) if empty?
    v = Esq::Units.parse_length(self)
    raise ArgumentError, "Cannot convert \"#{self}\" to Length" if v.nil?
    Length.new(v)
  end
end

# ---------------------------------------------------------------- Geom
module Geom
  def self.tolerance; Esq::G::TOL; end

  G = Esq::G

  class Point3d
    def initialize(*a)
      case a.size
      when 0 then @x = @y = @z = 0.0
      when 1 then @x, @y, @z = G.pt(a[0])
      when 2, 3
        @x = G.num(a[0])
        @y = G.num(a[1])
        @z = a.size == 3 ? G.num(a[2]) : 0.0
      else raise ArgumentError, "wrong # of arguments(#{a.size} for 3)"
      end
    end

    def __a; [@x, @y, @z]; end

    # each weight (rb_num2dbl) before its point; p2 * w2 + p1 * w1.
    def self.linear_combination(w1, p1, w2, p2)
      u = G.num(w1)
      a = G.pt(p1)
      v = G.num(w2)
      b = G.pt(p2)
      new((0..2).map { |i| b[i] * v + a[i] * u })
    end

    def x; Length.new(@x); end
    def y; Length.new(@y); end
    def z; Length.new(@z); end
    def x=(v); @x = G.num(v); end
    def y=(v); @y = G.num(v); end
    def z=(v); @z = G.num(v); end

    def [](i); Length.new(__a[G.index3(i)]); end

    def []=(i, v)
      i = G.index3(i)
      f = G.num(v)
      case i
      when 0 then @x = f
      when 1 then @y = f
      else @z = f
      end
      v
    end

    def to_a; [@x, @y, @z]; end

    def set!(*a)
      raise ArgumentError, 'wrong number of arguments' if a.empty?
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      if a[0].is_a?(Geom::Point3d)
        @x, @y, @z = a[0].__a
      else
        G.set_partial(self, a)
      end
      self
    end

    def clone; Point3d.new(@x, @y, @z); end

    def ==(o)
      return true if equal?(o)
      return false unless o.is_a?(Point3d) || G.point_array?(o)
      G.near?(__a, G.pt(o))
    end

    def <(o)
      a = __a
      b = G.pt(o)
      3.times do |i|
        d = a[i] - b[i]
        next if d.abs < G::TOL
        return d < 0
      end
      false
    end

    def distance(o)
      Length.new(G.len(G.sub(G.pt(o), __a)))
    end

    def distance_to_line(*line)
      p, d = G.line(*line)
      G.len(G.cross(G.sub(__a, p), d))
    end

    def distance_to_plane(*plane)
      a, b, c, d = G.plane(*plane)
      (a * @x + b * @y + c * @z + d).abs
    end

    def on_line?(*line)
      distance_to_line(*line) < G::TOL
    end

    def on_plane?(*plane)
      distance_to_plane(*plane) < G::TOL
    end

    def project_to_line(*line)
      p, d = G.line(*line)
      t = G.dot(G.sub(__a, p), d)
      Point3d.new(G.add(p, G.scale(d, t)))
    end

    def project_to_plane(*plane)
      a, b, c, d = G.plane(*plane)
      dist = a * @x + b * @y + c * @z + d
      Point3d.new(@x - a * dist, @y - b * dist, @z - c * dist)
    end

    def vector_to(o)
      Vector3d.new(G.sub(G.pt(o), __a))
    end

    def __offset_vec(args)
      raise ArgumentError, 'Wrong number of arguments: requires 1 or 2' if args.empty?
      v = G.vec(args[0])
      if args.size > 1
        l = G.len(v)
        raise TypeError, 'Zero length vector' if l.zero?
        v = G.scale(v, G.num(args[1]) / l)
      end
      v
    end

    def offset(*args)
      Point3d.new(G.add(__a, __offset_vec(args)))
    end

    def offset!(*args)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y, @z = G.add(__a, __offset_vec(args))
      self
    end

    def +(v)
      Point3d.new(G.add(__a, G.vec(v)))
    end

    def -(o)
      if o.is_a?(Vector3d)
        Point3d.new(G.sub(__a, o.__a))
      else
        Vector3d.new(G.sub(__a, G.pt(o)))
      end
    end

    def transform(t)
      G.xform(t) * self
    end

    def transform!(t)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y, @z = (G.xform(t) * self).__a
      self
    end

    def inspect
      "Point3d(#{G.g(G.snap(@x))}, #{G.g(G.snap(@y))}, #{G.g(G.snap(@z))})"
    end

    def to_s
      st = Esq::Units.settings
      "(#{__a.map { |v| Esq::Units.format_length(v, st, precision: 6, strip: true) }.join(', ')})"
    end
  end

  class Vector3d
    def initialize(*a)
      case a.size
      when 0 then @x = @y = @z = 0.0
      when 1 then @x, @y, @z = G.vec(a[0])
      when 2
        if G.point_like?(a[0])
          @x, @y, @z = G.sub(G.pt(a[1]), G.pt(a[0]))
        else
          @x = G.num(a[0])
          @y = G.num(a[1])
          @z = 0.0
        end
      when 3
        @x = G.num(a[0])
        @y = G.num(a[1])
        @z = G.num(a[2])
      else raise ArgumentError, "wrong # of arguments(#{a.size} for 3)"
      end
    end

    def __a; [@x, @y, @z]; end

    def self.linear_combination(*a)
      raise ArgumentError, 'wrong number arguments: 4 or 6 required' unless a.size == 4 || a.size == 6
      # each weight (rb_num2dbl) before its vector.
      r = [0.0, 0.0, 0.0]
      a.each_slice(2) do |w, v|
        k = G.num(w)
        r = G.add(r, G.scale(G.vec(v), k))
      end
      new(r)
    end

    def x; @x; end
    def y; @y; end
    def z; @z; end
    def x=(v); @x = G.num(v); end
    def y=(v); @y = G.num(v); end
    def z=(v); @z = G.num(v); end

    def [](i); __a[G.index3(i)]; end

    def []=(i, v)
      i = G.index3(i)
      f = G.num(v)
      case i
      when 0 then @x = f
      when 1 then @y = f
      else @z = f
      end
      v
    end

    def to_a; [@x, @y, @z]; end

    def set!(*a)
      raise ArgumentError, 'wrong number of arguments' if a.empty?
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      if a[0].is_a?(Vector3d)
        @x, @y, @z = a[0].__a
      else
        G.set_partial(self, a)
      end
      self
    end

    def clone; Vector3d.new(@x, @y, @z); end

    def ==(o)
      return true if equal?(o)
      return false unless o.is_a?(Vector3d) || G.point_array?(o)
      G.near?(__a, G.vec(o))
    end

    def <(o)
      a = __a
      b = G.vec(o)
      3.times do |i|
        d = a[i] - b[i]
        next if d.abs < G::TOL
        return d < 0
      end
      false
    end

    def +(o); Vector3d.new(G.add(__a, G.vec(o))); end
    def -(o); Vector3d.new(G.sub(__a, G.vec(o))); end

    # `*` is the cross product and `%` the dot product: a number is no vector
    # (ArgumentError).
    def *(o); Vector3d.new(G.cross(__a, G.vec(o))); end

    def cross(o); Vector3d.new(G.cross(__a, G.vec(o))); end
    def dot(o); G.dot(__a, G.vec(o)); end
    alias % dot
    

    # Extension (not in SketchUp): unary minus.
    def -@; Vector3d.new(-@x, -@y, -@z); end

    def length; Length.new(G.len(__a)); end

    # SUVector3DSetLength: any length, but not of a vector shorter than 1e-10;
    # the coordinates are scaled by length / current length.
    def length=(l)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      l = G.num(l)
      cur = Math.sqrt(@y * @y + @x * @x + @z * @z)
      raise TypeError, 'Cannot set the length of a zero length vector)' if cur < 1e-10
      k = l / cur
      @x *= k
      @y *= k
      @z *= k
    end

    def normalize; Vector3d.new(G.unit(__a)); end

    def normalize!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y, @z = G.unit(__a)
      self
    end

    def reverse; Vector3d.new(-@x, -@y, -@z); end

    def reverse!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y, @z = -@x, -@y, -@z
      self
    end

    # not when every coordinate is within 1e-10 of zero.
    def valid?; !(@x.abs <= 1e-10 && @y.abs <= 1e-10 && @z.abs <= 1e-10); end

    def unitvector?; (G.len(__a) - 1.0).abs < G::TOL; end

    def __nonzero!(o)
      v = G.vec(o)
      raise ArgumentError, 'Cannot create unit vector from zero length vector' if v.all?(&:zero?) || G.len(__a).zero?
      v
    end

    def angle_between(o)
      v = G.vec(o)
      lp = G.len(__a) * G.len(v)
      return 0.0 if lp.zero?
      Math.acos([[G.dot(__a, v) / lp, 1.0].min, -1.0].max)
    end

    # Unit vectors compared component by component, like SketchUp's IsParallelTo/IsSameDirectionAs.
    def parallel?(o)
      v = G.unit(__nonzero!(o))
      u = G.unit(__a)
      G.same_dir?(u, v) || G.same_dir?(u, G.scale(v, -1.0))
    end

    def perpendicular?(o)
      v = __nonzero!(o)
      G.dot(G.unit(__a), G.unit(v)).abs < G::TOL
    end

    def samedirection?(o)
      v = __nonzero!(o)
      G.same_dir?(G.unit(__a), G.unit(v))
    end

    def transform(t); G.xform(t) * self; end

    def transform!(t)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @x, @y, @z = (G.xform(t) * self).__a
      self
    end

    # Two vectors perpendicular to this one (the « arbitrary axis algorithm »): [x, y, z].
    def axes
      z = G.unit(__a)
      raise ArgumentError, 'Cannot create unit vector from zero length vector' if z.all?(&:zero?)
      x = if z[0].abs < 1.0 / 64 && z[1].abs < 1.0 / 64
            G.unit(G.cross([0.0, 1.0, 0.0], z))
          else
            G.unit(G.cross([0.0, 0.0, 1.0], z))
          end
      [Vector3d.new(x), Vector3d.new(G.cross(z, x)), Vector3d.new(z)]
    end

    def inspect
      "Vector3d(#{G.g(G.snap(@x))}, #{G.g(G.snap(@y))}, #{G.g(G.snap(@z))})"
    end

    def to_s
      "(#{G.g(@x)}, #{G.g(@y)}, #{G.g(@z)})"
    end
  end

  # Column-major 4x4 like SketchUp: x axis = 0..2, y axis = 4..6, z axis = 8..10, origin = 12..14.
  class Transformation
    IDENT = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0].freeze

    def self.__from(m)
      t = allocate
      t.instance_variable_set(:@m, m)
      t
    end

    def self.__axes(origin, x, y, z)
      __from([x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, origin[0], origin[1], origin[2], 1.0])
    end

    def self.translation(v)
      t = v.is_a?(Vector3d) || v.is_a?(Array) && v.size == 3 && !v[0].is_a?(Array) ? G.vec(v) : G.pt(v)
      __from([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0], t[1], t[2], 1.0])
    end

    def self.rotation(point, vector, angle)
      o = G.pt(point)
      u = G.unit(G.vec(vector))
      raise ArgumentError, 'Zero length vector' if u.all?(&:zero?)
      a = G.num(angle)
      c = Math.cos(a)
      s = Math.sin(a)
      t = 1 - c
      r = [t * u[0] * u[0] + c, t * u[0] * u[1] + s * u[2], t * u[0] * u[2] - s * u[1], 0.0,
           t * u[0] * u[1] - s * u[2], t * u[1] * u[1] + c, t * u[1] * u[2] + s * u[0], 0.0,
           t * u[0] * u[2] + s * u[1], t * u[1] * u[2] - s * u[0], t * u[2] * u[2] + c, 0.0,
           0.0, 0.0, 0.0, 1.0]
      # translation part: o - R * o
      r[12] = o[0] - (r[0] * o[0] + r[4] * o[1] + r[8] * o[2])
      r[13] = o[1] - (r[1] * o[0] + r[5] * o[1] + r[9] * o[2])
      r[14] = o[2] - (r[2] * o[0] + r[6] * o[1] + r[10] * o[2])
      __from(r)
    end

    # scaling(s), scaling(point, s), scaling(x, y, z), scaling(point, x, y, z)
    def self.scaling(*a)
      case a.size
      when 1 then s = [G.num(a[0])] * 3
      when 2 then o = G.pt(a[0]); s = [G.num(a[1])] * 3
      when 3 then s = a.map { |v| G.num(v) }
      when 4 then o = G.pt(a[0]); s = a[1..].map { |v| G.num(v) }
      else raise ArgumentError, 'Wrong number of arguments'
      end
      m = [s[0], 0.0, 0.0, 0.0, 0.0, s[1], 0.0, 0.0, 0.0, 0.0, s[2], 0.0, 0.0, 0.0, 0.0, 1.0]
      if o
        m[12] = o[0] - s[0] * o[0]
        m[13] = o[1] - s[1] * o[1]
        m[14] = o[2] - s[2] * o[2]
      end
      __from(m)
    end

    # axes(origin, xaxis, yaxis[, zaxis])
    # each axis is made a unit vector (ArgumentError for a zero
    # vector), z defaults to x × y.
    def self.axes(*a)
      raise ArgumentError, 'Wrong number of arguments: 3 or 4 required' if a.size < 3
      o = G.pt(a[0])
      x = G.unit_arg(a[1])
      y = G.unit_arg(a[2])
      if a.size > 3
        z = G.unit_arg(a[3])
      else
        z = G.cross(x, y)
        raise ArgumentError, 'Zero length vector for zaxis' if z.all? { |c| c.abs <= 1e-10 }
      end
      __axes(o, x, y, z)
    end

    # SUTransformationInterpolate: a weight outside 0..1 leaves
    # the identity, one within 0.001 of 0 or 1 a copy of t1 or t2; otherwise the origins are
    # mixed and the rotations of the 3×3 parts slerped, scale
    # left out. A mirror's 3×3 reads as some rotation, as in SketchUp.
    def self.interpolate(t1, t2, weight)
      a = G.xform(t1).to_a
      b = G.xform(t2).to_a
      w = G.num(weight)
      return new if w < 0 || w > 1
      copy = ->(m) { __from(m[0, 3] + [0.0] + m[4, 3] + [0.0] + m[8, 3] + [0.0] + m[12, 3] + [1.0 / (1.0 / m[15])]) }
      return copy.(a) if w.abs < G::TOL
      return copy.(b) if (w - 1.0).abs < G::TOL
      x, y, z, s = G.su_slerp(G.su_quat(a), G.su_quat(b), w)
      u = 1.0 - w
      o = [b[12] * w + u * a[12] + 0.0, u * a[13] + b[13] * w + 0.0, b[14] * w + u * a[14] + 0.0]
      __from([1.0 - 2.0 * (z * z + y * y), 2.0 * (x * y + s * z), 2.0 * (x * z - s * y), 0.0,
              2.0 * (x * y - s * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (z * y + s * x), 0.0,
              2.0 * (x * z + s * y), 2.0 * (z * y - s * x), 1.0 - 2.0 * (x * x + y * y), 0.0,
              o[0], o[1], o[2], 1.0])
    end

    def initialize(*a)
      case a.size
      when 0 then @m = IDENT.dup
      when 1 then @m = __from_arg(a[0])
      when 2
        o = G.pt(a[0])
        z = G.unit(G.vec(a[1]))
        raise ArgumentError, 'Zero length vector' if z.all?(&:zero?)
        x, y, z = Vector3d.new(z).axes.map(&:__a)
        @m = Transformation.__axes(o, x, y, z).to_a
      when 3
        o = G.pt(a[0])
        if a[2].is_a?(Numeric)
          @m = Transformation.rotation(o, a[1], a[2]).to_a
        else
          x = G.unit(G.vec(a[1]))
          t = G.vec(a[2])
          y = G.cross(G.cross(x, t), x)
          y = G.unit(y)
          @m = Transformation.__axes(o, x, y, G.cross(x, y)).to_a
        end
      when 4
        x = G.vec(a[0])
        y = G.vec(a[1])
        z = G.vec(a[2])
        o = G.pt(a[3])
        @m = Transformation.__axes(o, x, y, z).to_a
      else raise ArgumentError, 'Wrong number of arguments: 0, 1 or 3'
      end
    end

    # One argument: a Transformation (copy), 16 numbers, a point/vector/[x, y, z] (translation) or a
    # number (uniform scale).
    def __from_arg(a)
      case a
      when Transformation then a.to_a
      when Array
        if a.size == 16 && G.numbers?(a)
          a.map(&:to_f)
        elsif a.size < 4
          Transformation.translation(a).to_a
        else
          Transformation.scaling(G.num(a)).to_a # raises like rb_num2dbl
        end
      when Geom::Point3d, Geom::Vector3d then Transformation.translation(a).to_a
      else Transformation.scaling(G.num(a)).to_a
      end
    end
    private :__from_arg

    def to_a; @m.dup; end

    def clone; Transformation.__from(@m.dup); end

    def set!(a)
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @m = G.xform(a).to_a
      self
    end

    # Within SketchUp's tolerance (0.001), like SUTransformationIsIdentity.
    def identity?
      @m.each_with_index.all? { |v, i| (v - IDENT[i]).abs < G::TOL }
    end

    def origin; Point3d.new(@m[12], @m[13], @m[14]); end
    def xaxis; Vector3d.new(G.unit(@m[0, 3])); end
    def yaxis; Vector3d.new(G.unit(@m[4, 3])); end
    def zaxis; Vector3d.new(G.unit(@m[8, 3])); end

    def *(o)
      case o
      when Point3d
        m = @m
        w = m[3] * o.__a[0] + m[7] * o.__a[1] + m[11] * o.__a[2] + m[15]
        x, y, z = o.__a
        Point3d.new((m[0] * x + m[4] * y + m[8] * z + m[12]) / w,
                    (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
                    (m[2] * x + m[6] * y + m[10] * z + m[14]) / w)
      when Vector3d
        m = @m
        x, y, z = o.__a
        Vector3d.new(m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z)
      when Transformation
        Transformation.__from(G.mul16(@m, o.to_a))
      when Array
        if o.size == 3 && G.numbers?(o)
          (self * Point3d.new(o)).to_a
        elsif o.size == 4 || o.size == 2
          __plane(G.plane(o))
        else raise ArgumentError, 'Not a valid argument type'
        end
      else raise ArgumentError, 'Not a valid argument type'
      end
    end

    # Planes transform with the inverse transpose.
    def __plane(pl)
      inv = G.inv16(@m) or raise ArgumentError, 'Transformation is not invertible'
      a, b, c, d = pl
      [inv[0] * a + inv[1] * b + inv[2] * c + inv[3] * d,
       inv[4] * a + inv[5] * b + inv[6] * c + inv[7] * d,
       inv[8] * a + inv[9] * b + inv[10] * c + inv[11] * d,
       inv[12] * a + inv[13] * b + inv[14] * c + inv[15] * d].map(&:to_f)
    end
    private :__plane

    def inverse
      Transformation.__from(G.inv16(@m) || raise(ArgumentError, 'Transformation is not invertible'))
    end

    def invert!
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
      @m = G.inv16(@m) || raise(ArgumentError, 'Transformation is not invertible')
      self
    end
  end

  class BoundingBox
    CORNER_LEFT_FRONT_BOTTOM = 0
    CORNER_RIGHT_FRONT_BOTTOM = 1
    CORNER_LEFT_BACK_BOTTOM = 2
    CORNER_RIGHT_BACK_BOTTOM = 3
    CORNER_LEFT_FRONT_TOP = 4
    CORNER_RIGHT_FRONT_TOP = 5
    CORNER_LEFT_BACK_TOP = 6
    CORNER_RIGHT_BACK_TOP = 7

    EMPTY_MIN = 1e30

    def initialize
      @min = [EMPTY_MIN] * 3
      @max = [-EMPTY_MIN] * 3
    end

    def initialize_copy(o)
      @min = o.__min.dup
      @max = o.__max.dup
    end

    def __min; @min; end
    def __max; @max; end

    def empty?; @min[0] > @max[0]; end
    def valid?; !empty?; end

    def clear
      @min = [EMPTY_MIN] * 3
      @max = [-EMPTY_MIN] * 3
      self
    end

    def min; Point3d.new(@min); end
    def max; Point3d.new(@max); end
    def center; Point3d.new((0..2).map { |i| (@min[i] + @max[i]) / 2.0 }); end
    def width; Length.new(@max[0] > @min[0] ? @max[0] - @min[0] : 0.0); end
    def height; Length.new(@max[1] > @min[1] ? @max[1] - @min[1] : 0.0); end
    def depth; Length.new(@max[2] > @min[2] ? @max[2] - @min[2] : 0.0); end

    def diagonal
      return Length.new(0.0) if empty?
      Length.new(G.len(G.sub(@max, @min)))
    end

    def corner(i)
      i = G.int(i)
      raise IndexError, 'Invalid corner index' unless (0..7).cover?(i)
      Point3d.new(i & 1 == 0 ? @min[0] : @max[0], i & 2 == 0 ? @min[1] : @max[1], i & 4 == 0 ? @min[2] : @max[2])
    end

    def add(*args)
      raise ArgumentError, 'Wrong number of values - at least one required' if args.empty?
      if args.size == 1 && args[0].is_a?(Array) && !args[0].empty? && __splat?(args[0][0])
        return add(*args[0])
      end
      args.each do |a|
        if a.is_a?(BoundingBox)
          unless a.empty?
            __extend(a.__min)
            __extend(a.__max)
          end
        else
          __extend(G.pt(a))
        end
      end
      self
    end

    def __splat?(e)
      e.is_a?(BoundingBox) || G.positioned?(e) || G.point_like?(e)
    end
    private :__splat?

    def __extend(p)
      3.times do |i|
        @min[i] = p[i] if p[i] < @min[i]
        @max[i] = p[i] if p[i] > @max[i]
      end
    end
    private :__extend

    def contains?(o)
      if o.is_a?(BoundingBox)
        return false if o.empty? || empty?
        return (0..2).all? { |i| o.__min[i] >= @min[i] && o.__max[i] <= @max[i] }
      end
      # The point is read first, empty box or not (TestUp test_contains_Query_invalid_argument_*).
      p = G.pt(o)
      return false if empty?
      (0..2).all? { |i| p[i] >= @min[i] && p[i] <= @max[i] }
    end

    def intersect(o)
      raise TypeError, 'Wrong type - expected BoundingBox' unless o.is_a?(BoundingBox)
      r = BoundingBox.new
      return r if empty? || o.empty?
      lo = (0..2).map { |i| [@min[i], o.__min[i]].max }
      hi = (0..2).map { |i| [@max[i], o.__max[i]].min }
      return r if (0..2).any? { |i| lo[i] > hi[i] }
      r.add(lo, hi)
    end
  end

  # ---------------------------------------------------------------- module functions
  class << self
    def closest_points(l1, l2)
      p1, d1 = G.line(l1)
      p2, d2 = G.line(l2)
      w = G.sub(p1, p2)
      b = G.dot(d1, d2)
      d = G.dot(d1, w)
      e = G.dot(d2, w)
      den = 1.0 - b * b
      if den.abs < 1e-12
        s = 0.0
        t = e
      else
        s = (b * e - d) / den
        t = (e - b * d) / den
      end
      [Point3d.new(G.add(p1, G.scale(d1, s))), Point3d.new(G.add(p2, G.scale(d2, t)))]
    end

    def intersect_line_line(l1, l2)
      p1, d1 = G.line(l1)
      p2, d2 = G.line(l2)
      return nil if G.len(G.cross(d1, d2)) < 1e-9
      a, b = closest_points([Point3d.new(p1), Vector3d.new(d1)], [Point3d.new(p2), Vector3d.new(d2)])
      # the midpoint of the closest points, when they meet within the tolerance
      G.near?(a.__a, b.__a) ? Point3d.new(G.scale(G.add(a.__a, b.__a), 0.5)) : nil
    end

    def intersect_line_plane(line, plane)
      p, d = G.line(line)
      a, b, c, e = G.plane(plane)
      den = a * d[0] + b * d[1] + c * d[2]
      return nil if den.abs < 1e-12
      t = -(a * p[0] + b * p[1] + c * p[2] + e) / den
      Point3d.new(G.add(p, G.scale(d, t)))
    end

    def intersect_plane_plane(p1, p2)
      a1, b1, c1, d1 = G.plane(p1)
      a2, b2, c2, d2 = G.plane(p2)
      # the direction n1 × n2 (none below 1e-10); the point is the one where
      # the coordinate of the direction's largest component is 0.
      dir = G.cross([a1, b1, c1], [a2, b2, c2])
      return nil if Math.sqrt(dir[1] * dir[1] + dir[0] * dir[0] + dir[2] * dir[2]) < 1e-10
      u = G.unit(dir)
      ax, ay, az = u.map(&:abs)
      pt = if ax < ay || ax < az
             if ay < az
               [(b1 * d2 - b2 * d1) / dir[2], (d1 * a2 - d2 * a1) / dir[2], 0.0]
             else
               [(c2 * d1 - c1 * d2) / dir[1], 0.0, (d2 * a1 - d1 * a2) / dir[1]]
             end
           else
             [0.0, (c1 * d2 - c2 * d1) / dir[0], (b2 * d1 - b1 * d2) / dir[0]]
           end
      [Point3d.new(pt), Vector3d.new(u)]
    end

    # one argument must be an array of the points (TypeError otherwise); the
    # plane is SketchUp's least squares fit: the centred sums,
    # the coordinate whose 2×2 determinant is the largest taken as 1, turned along the fan
    # normal of the points. Points that fix no plane leave [0, 0, 1, 0].
    def fit_plane_to_points(*args)
      if args.size == 1
        a = args[0]
        raise TypeError, "wrong argument type #{a.nil? ? 'nil' : a.class} (expected Array)" unless a.is_a?(Array)
        return fit_plane_to_points(*a)
      end
      raise ArgumentError, 'At least three points needed to fit a plane' if args.size < 3
      ps = args.map { |p| G.pt(p) }
      plane = [0.0, 0.0, 1.0, 0.0]
      tx = ty = tz = 0.0
      ps.each { |p| tx += p[0]; ty += p[1]; tz += p[2] }
      inv = 1.0 / ps.size
      n = sx = sy = sz = sxx = syy = szz = sxy = sxz = syz = 0.0
      ps.each do |p|
        n += 1.0
        dx = p[0] - tx * inv
        dz = p[2] - tz * inv
        dy = p[1] - ty * inv
        sx += dx
        sxx += dx * dx
        syy += dy * dy
        sy += dy
        sz += dz
        sxz += dz * dx
        syz += dz * dy
        szz += dz * dz
        sxy += dy * dx
      end
      fit = G.fit_plane_sums(n, sx, sy, sz, sxx, syy, szz, sxy, sxz, syz)
      return plane unless fit
      plane = fit
      plane[3] -= tx * inv * plane[0] + plane[1] * ty * inv + plane[2] * tz * inv
      o = ps[0]
      nw = [0.0, 0.0, 0.0]
      prev = G.sub(ps[1], o)
      ps[2..].each do |p|
        cur = G.sub(p, o)
        nw[0] += prev[1] * cur[2] - prev[2] * cur[1]
        nw[1] += prev[2] * cur[0] - prev[0] * cur[2]
        nw[2] += prev[0] * cur[1] - prev[1] * cur[0]
        prev = cur
      end
      plane = plane.map { |c| -c } if nw[1] * plane[1] + nw[0] * plane[0] + nw[2] * plane[2] < 0.0
      plane
    end

    def linear_combination(w1, x1, w2, x2)
      w1 = G.num(w1)
      w2 = G.num(w2)
      if G.point_like?(x1)
        Point3d.new(G.add(G.scale(G.pt(x1), w1), G.scale(G.pt(x2), w2)))
      else
        Vector3d.new(G.add(G.scale(G.vec(x1), w1), G.scale(G.vec(x2), w2)))
      end
    end

    # Is the point (x, y only) inside the polygon? check_border: points on the border count as inside.
    def point_in_polygon_2D(point, polygon, check_border)
      px, py = G.pt(point)
      raise TypeError, "wrong argument type #{polygon.class} (expected Array)" unless polygon.is_a?(Array)
      pts = polygon.map { |p| G.pt(p) }
      near = ->(x, y) { (x - px).abs < G::TOL && (y - py).abs < G::TOL && (x - px)**2 + (y - py)**2 < G::TOL**2 }
      inside = false
      n = pts.size
      n.times do |i|
        ax, ay = pts[i]
        bx, by = pts[(i + 1) % n]
        # on a vertex or an edge (within the tolerance): the border
        return check_border ? true : false if near.call(ax, ay)
        l2 = (bx - ax)**2 + (by - ay)**2
        if l2 >= 1e-12
          t = ((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / l2
          return check_border ? true : false if t > 0 && t < 1 && near.call(ax + t * (bx - ax), ay + t * (by - ay))
        end
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax
          inside = !inside
        end
      end
      inside
    end

    # Triangulates polygons with holes: tesselate(outer, hole1, ...) -> flat list of triangle corners.
    def tesselate(*loops)
      raise ArgumentError, 'wrong number of arguments (given 0, expected 1+)' if loops.empty?
      data = loops.each_with_index.map do |l, i|
        raise TypeError, "wrong argument type #{l.class} (expected Array)" unless l.is_a?(Array)
        if l.size < 3
          raise ArgumentError, "Loop cannot contain less than 3 points. Loop #{i} had #{l.size} points"
        end
        l.map { |p| G.pt(p) }
      end
      begin
        tris = Esq.call('geom.tesselate', { 'loops' => data })
      rescue RuntimeError => e
        raise RuntimeError, "Unable to tesselate polygon; #{e.message}"
      end
      tris.flat_map { |t| t.map { |p| Point3d.new(p) } }
    end
  end

  # Gelées, comme dans SketchUp (`rb_obj_freeze` avant `rb_define_global_const`)
  # `ORIGIN.x = 1` lève FrozenError au lieu de déplacer
  # l'origine de toutes les extensions ; `clone` et `dup` rendent une copie modifiable.
  ORIGIN = Point3d.new(0, 0, 0).freeze
  X_AXIS = Vector3d.new(1, 0, 0).freeze
  Y_AXIS = Vector3d.new(0, 1, 0).freeze
  Z_AXIS = Vector3d.new(0, 0, 1).freeze
  IDENTITY = Transformation.new.freeze
end

# SketchUp defines these at the top level.
ORIGIN = Geom::ORIGIN unless defined?(ORIGIN)
X_AXIS = Geom::X_AXIS unless defined?(X_AXIS)
Y_AXIS = Geom::Y_AXIS unless defined?(Y_AXIS)
Z_AXIS = Geom::Z_AXIS unless defined?(Z_AXIS)
IDENTITY = Geom::IDENTITY unless defined?(IDENTITY)

module Esq
  module G
    module_function

    # the quaternion [x, y, z, w] of the 3×3 part of a column-major 4×4, read
    # as a rotation whatever it is (a mirror gives some rotation), then made unit.
    def su_quat(m)
      p0, p1, p2, p3, p4, p5, p6, p7, p8 = m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]
      root = ->(v) { v < 0 ? Float::NAN : Math.sqrt(v) }
      tr = p0 + 1.0 + p4 + p8
      if tr > 1e-9
        s = root.(tr) * 2.0
        x, y, z, w = (p7 - p5) / s, (p2 - p6) / s, (p3 - p1) / s, s * 0.25
      elsif p0 <= p4 || p0 <= p8
        if p4 <= p8
          s = root.(((p8 + 1.0) - p0) - p4) * 2.0
          x, y, z, w = (p2 + p6) / s, (p5 + p7) / s, s * 0.25, (p3 - p1) / s
        else
          s = root.(((p4 + 1.0) - p0) - p8) * 2.0
          x, y, z, w = (p1 + p3) / s, s * 0.25, (p5 + p7) / s, (p2 - p6) / s
        end
      else
        s = root.(((p0 + 1.0) - p4) - p8) * 2.0
        x, y, z, w = s * 0.25, (p1 + p3) / s, (p2 + p6) / s, (p7 - p5) / s
      end
      n = root.(y * y + x * x + z * z + w * w)
      [x / n, y / n, z / n, w / n]
    end

    # SketchUp's slerp of two quaternions [x, y, z, w].
    def su_slerp(q1, q2, t)
      dot = q2[1] * q1[1] + q2[0] * q1[0] + q2[2] * q1[2] + q2[3] * q1[3]
      if dot < 0.0
        dot = -dot
        q1 = q1.map { |c| -c }
      end
      u = 1.0 - t
      if dot + 1.0 <= 1e-6
        s0 = Math.sin(u * 1.570796326794896)
        s1 = Math.sin(t * 1.570796326794896)
        return [s0 * q1[0] - s1 * q2[1], s1 * q2[0] + s0 * q1[1], s0 * q1[2] - s1 * q2[3], s1 * q2[2] + s0 * q1[3]]
      end
      a = u
      b = t
      if 1e-6 < 1.0 - dot
        om = Math.acos(dot)
        so = Math.sin(om)
        a = Math.sin(om * u) / so
        b = Math.sin(om * t) / so
      end
      [q2[0] * b + a * q1[0], a * q1[1] + q2[1] * b, q2[2] * b + a * q1[2], a * q1[3] + q2[3] * b]
    end

    # the plane of centred sums (n points), the coordinate of the largest 2×2
    # determinant taken as 1 and the two others solved; nil when every determinant is below
    # 1e-10 (points that fix no plane).
    def fit_plane_sums(n, sx, sy, sz, sxx, syy, szz, sxy, sxz, syz)
      cxx = sxx - sx * sx / n
      cyy = syy - sy * sy / n
      czz = szz - sz * sz / n
      cxy = sxy - sy * sx / n
      cxz = sxz - sz * sx / n
      cyz = syz - sz * sy / n
      dz = cyy * cxx - cxy * cxy
      dy = czz * cxx - cxz * cxz
      dx = czz * cyy - cyz * cyz
      if dz.abs < dy.abs || dz.abs < dx.abs
        if dy.abs < dx.abs
          return nil if dx.abs < 1e-10
          a = 1.0
          b = -(cxy * czz - cyz * cxz) / dx
          c = -(cxz * cyy - cyz * cxy) / dx
        else
          return nil if dy.abs < 1e-10
          a = -(cxy * czz - cyz * cxz) / dy
          b = 1.0
          c = -(cyz * cxx - cxz * cxy) / dy
        end
      else
        return nil if dz.abs < 1e-10
        a = -(cxz * cyy - cyz * cxy) / dz
        b = -(cyz * cxx - cxz * cxy) / dz
        c = 1.0
      end
      len = Math.sqrt(b * b + a * a + c * c)
      [a / len, b / len, c / len, -(sy * b + sx * a + sz * c) / n / len]
    end
  end
end

# ---------------------------------------------------------------- Array as a point / vector
class Array
  def __g2?; size == 2; end
  private :__g2?

  def x; self[0]; end
  def y; self[1]; end
  def z; self[2]; end
  def x=(v); self[0] = v; end
  def y=(v); self[1] = v; end
  def z=(v); self[2] = v; end

  # 2D only when both sides are: a 2D array
  # with a Point3d or a Vector3d is read as 3D (z = 0).
  def __p2?(o); o.is_a?(Geom::Point2d) || (o.is_a?(Array) && o.size == 2 && Esq::G.numbers?(o)); end
  def __v2?(o); o.is_a?(Geom::Vector2d) || (o.is_a?(Array) && o.size == 2 && Esq::G.numbers?(o)); end
  private :__p2?, :__v2?

  def distance(o)
    return Geom::Point2d.new(self).distance(o) if __p2?(self) && __p2?(o)
    Length.new(Esq::G.len(Esq::G.sub(Esq::G.pt(o), Esq::G.pt(self))))
  end

  def distance_to_line(*line)
    Length.new(Geom::Point3d.new(self).distance_to_line(*line))
  end

  def distance_to_plane(*plane)
    Length.new(Geom::Point3d.new(self).distance_to_plane(*plane))
  end

  def on_line?(*line); Geom::Point3d.new(self).on_line?(*line); end
  def on_plane?(*plane); Geom::Point3d.new(self).on_plane?(*plane); end
  def project_to_line(*line); Geom::Point3d.new(self).project_to_line(*line).to_a; end
  def project_to_plane(*plane); Geom::Point3d.new(self).project_to_plane(*plane).to_a; end

  def vector_to(o)
    return Geom::Point2d.new(self).vector_to(o) if __g2?
    Geom::Point3d.new(self).vector_to(o)
  end

  def dot(o)
    return Geom::Vector2d.new(self).dot(o) if __v2?(self) && __v2?(o)
    Geom::Vector3d.new(Esq::G.vec(self)).dot(o)
  end

  def cross(o)
    return Geom::Vector2d.new(self).cross(o) if __g2?
    Geom::Vector3d.new(self).cross(o)
  end

  def normalize
    return Geom::Vector2d.new(self).normalize.to_a if __g2?
    Geom::Vector3d.new(self).normalize.to_a
  end

  def normalize!
    n = normalize
    n.each_with_index { |v, i| self[i] = v }
    self
  end

  def offset(*args)
    raise ArgumentError, 'Wrong number of arguments: requires 1 or 2' if args.empty?
    return Geom::Point2d.new(self).offset(*args).to_a if __p2?(self) && __v2?(args[0])
    Geom::Point3d.new(Esq::G.pt(self)).offset(*args).to_a
  end

  def offset!(*args)
    o = offset(*args)
    o.each_with_index { |v, i| self[i] = Length.new(v) }
    self
  end

  def transform(t)
    return Geom::Point2d.new(self).transform(t).to_a if __g2?
    Geom::Point3d.new(self).transform(t).to_a
  end

  def transform!(t)
    o = transform(t)
    o.each_with_index { |v, i| self[i] = Length.new(v) }
    self
  end
end
