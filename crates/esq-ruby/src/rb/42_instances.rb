# Esquisse — instances : groupes, composants, définitions,
# comportement, dictionnaires d'attributs, classifications, chemins d'instances, Sketchup::Set.
# Comportements relevés dans SketchUp.exe (messages d'erreur, valeurs de retour, unités : pouces).

module Esq
  class << self
    # Comme `call`, mais les erreurs « !Genre|message » du pont deviennent la bonne exception.
    def icall(op, args = {}, what = nil)
      call(op, args)
    rescue RuntimeError => e
      msg = e.message
      raise unless msg.start_with?('!')
      kind, text = msg[1..].split('|', 2)
      klass = { 'Arg' => ArgumentError, 'Type' => TypeError, 'Runtime' => RuntimeError, 'Index' => IndexError,
                'IO' => IOError }[kind]
      raise TypeError, "reference to deleted #{what || 'Entity'}", cause: nil if kind == 'Gone'
      raise e unless klass
      raise klass, text, cause: nil
    end

    # Texte venant d'un argument Ruby (nom de dictionnaire, clé…) : comme SketchUp, `to_s`.
    def text_of(v)
      case v
      when String then v
      when true then 'true'
      when false then 'false'
      when nil then ''
      else
        begin
          v.to_s
        rescue StandardError
          ''
        end
      end
    end

    # Argument qui doit être une chaîne (TypeError sinon, comme `StringValue`).
    def str_arg(v)
      return v if v.is_a?(String)
      return v.to_str if v.respond_to?(:to_str)
      raise TypeError, "no implicit conversion #{v.nil? ? 'from nil to' : "of #{v.class} into"} String"
    end

    def to_long(v)
      return v if v.is_a?(Integer)
      return v.to_i if v.is_a?(Float)
      return v.to_int if v.respond_to?(:to_int)
      raise TypeError, "no implicit conversion #{v.nil? ? 'from nil to integer' : "of #{v.class} into Integer"}"
    end

    # Identifiant stable de 128 bits (version 4) dérivé d'une poignée.
    def guid_bits(*parts)
      s = parts.join(':')
      h = (0..3).map do |k|
        s.bytes.inject((0x811c9dc5 ^ (k * 0x9e3779b1)) & 0xffffffff) { |a, b| ((a ^ b) * 0x01000193) & 0xffffffff }
      end
      n = h.inject(0) { |acc, w| (acc << 32) | w }
      n = (n & ~(0xf << 76)) | (0x4 << 76)
      (n & ~(0x3 << 62)) | (0x2 << 62)
    end

    # GUID d'une définition : « %08x-%04x-%04x-%02x%02x-%02x… » en minuscules, comme SketchUp.
    def guid_of(*parts)
      hex = format('%032x', guid_bits(*parts))
      "#{hex[0, 8]}-#{hex[8, 4]}-#{hex[12, 4]}-#{hex[16, 4]}-#{hex[20, 12]}"
    end

    IFC64 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$'

    # GUID d'une instance : les 128 bits en 22 chiffres de base 64 (forme compressée IFC).
    def ifc_guid_of(*parts)
      ifc_of_bits(guid_bits(*parts))
    end

    # Les 128 bits en 22 chiffres de base 64, le premier de 2 bits.
    def ifc_of_bits(n)
      (0...22).map { |i| IFC64[(n >> (6 * (21 - i))) & 63] }.join
    end

    # Le GUID lu dans le fichier (« 3f2504e0-4f89-11d3-9a0c-0305e82c3301 ») sous la forme
    # compressée de SketchUp : les 128 bits dans l'ordre où le GUID s'écrit.
    def ifc_of_guid(g)
      hex = g.to_s.delete('-')
      return nil unless hex =~ /\A\h{32}\z/
      ifc_of_bits(hex.to_i(16))
    end

    # La conversion de SketchUp (pour transformation=, transform! et move!) :
    # une transformation, un point ou un vecteur (translation), 16 nombres, un nombre (échelle).
    def xform_arg(x)
      G.xform(x)
    end

    def singular?(t)
      a = t.to_a
      det = a[0] * (a[5] * a[10] - a[6] * a[9]) - a[4] * (a[1] * a[10] - a[2] * a[9]) + a[8] * (a[1] * a[6] - a[2] * a[5])
      det.abs < 1e-12
    end

    # Calculé en pouces, côté pont.
    def point_arg(p)
      return p.to_a if p.is_a?(Geom::Point3d)
      return p.map(&:to_f) if p.is_a?(Array) && p.size == 3 && p.all? { |v| v.is_a?(Numeric) }
      nil
    end
  end

  # ------------------------------------------------------------------ valeurs d'attributs
  # Les valeurs voyagent en JSON étiqueté : {t: 's', v: 'texte'}, {t: 'i', v: 3}, …
  module Attr
    RESERVED_DELETE = %w[GSU_ContributorsInfo SU_LiveComponent SU_InstanceSet SU_DefinitionSet SU_PC].freeze
    RESERVED_MODIFY = %w[GSU_ContributorsInfo SU_LiveComponent SU_PC].freeze

    module_function

    def enc(v)
      # SketchUp garde les longueurs à part (type « Length ») : à tester avant Float.
      return { 't' => 'l', 'v' => v.to_f } if defined?(::Length) && ::Length.is_a?(Class) && v.is_a?(::Length)
      case v
      when String then { 't' => 's', 'v' => v }
      when true, false then { 't' => 'b', 'v' => v }
      when nil then { 't' => 'n' }
      when Integer then { 't' => 'i', 'v' => v }
      when Float
        v.finite? ? { 't' => 'f', 'v' => v } : { 't' => 'nf', 'v' => v.nan? ? 'nan' : (v.positive? ? 'inf' : '-inf') }
      when Sketchup::Color then { 't' => 'c', 'v' => v.to_a }
      when Time then { 't' => 'time', 'v' => [v.to_i, v.nsec] }
      when Geom::Point3d then { 't' => 'p', 'v' => v.to_a }
      when Geom::Vector3d then { 't' => 'vec', 'v' => v.to_a }
      when Array then { 't' => 'a', 'v' => v.map { |x| enc(x) } }
      else
        { 't' => 'n' } # Hash, Symbol… : SketchUp les range comme une valeur vide
      end
    end

    def dec(j)
      case j['t']
      when 's', 'b', 'i' then j['v']
      when 'f' then j['v'].to_f
      when 'l' then j['v'].to_f.to_l
      when 'nf' then { 'nan' => Float::NAN, 'inf' => Float::INFINITY, '-inf' => -Float::INFINITY }[j['v']]
      when 'c' then Sketchup::Color.new(*j['v'])
      when 'time' then Time.at(j['v'][0], j['v'][1], :nsec)
      when 'p' then Geom::Point3d.new(*j['v'])
      when 'vec' then Geom::Vector3d.new(*j['v'])
      when 'a' then j['v'].map { |x| dec(x) }
      end
    end
  end

  # ------------------------------------------------------------------ attributs d'une entité
  # Méthodes `*_attribute` de Sketchup::Entity ; `attr_owner` désigne le propriétaire côté pont
  # (instance, définition, ou « h:<poignée> » pour tout le reste).
  module AttrHolder
    def attr_owner
      h = respond_to?(:handle) ? handle : nil
      h = instance_variable_get(:@handle) if h.nil? && instance_variable_defined?(:@handle)
      "h:#{h}"
    end

    # Le propriétaire, après avoir vérifié qu'une arête ou une face existe encore.
    def attr_target
      o = attr_owner
      if o.match?(/\Ah:[efgasnp]:/) && respond_to?(:valid?) && !valid?
        raise TypeError, "reference to deleted #{Esq.attr_typename(self)}"
      end
      o
    end
    private :attr_target

    def attribute_dictionaries
      names = Esq.icall('attr.names', { 'owner' => attr_target }, Esq.attr_typename(self))
      names && Sketchup::AttributeDictionaries.new(attr_target)
    end

    def attribute_dictionary(*args)
      raise ArgumentError, 'Dictionary name required' if args.empty?
      name = Esq.text_of(args[0])
      owner = attr_target
      what = Esq.attr_typename(self)
      found = Esq.icall('attr.dict', { 'owner' => owner, 'dict' => name }, what)
      return Sketchup::AttributeDictionary.new(owner, name) if found
      if args.size > 1 && args[1]
        if Esq::Attr::RESERVED_DELETE.include?(name)
          raise ArgumentError, 'Cannot create a dictionary with a reserved name.'
        end
        Esq.icall('attr.create', { 'owner' => owner, 'dict' => name }, what)
        return Sketchup::AttributeDictionary.new(owner, name)
      end
      nil
    end

    def get_attribute(*args)
      raise ArgumentError, 'Dictionary name and key required' if args.size < 2
      r = Esq.icall('attr.get', { 'owner' => attr_target, 'dict' => Esq.text_of(args[0]), 'key' => Esq.text_of(args[1]) },
                    Esq.attr_typename(self))
      v = r['found'] ? Esq::Attr.dec(r['v']) : nil
      v.nil? ? args[2] : v
    end

    def set_attribute(*args)
      raise ArgumentError, 'Dictionary name, key and value required' if args.size < 3
      dict = Esq.text_of(args[0])
      key = Esq.text_of(args[1])
      raise ArgumentError, 'Key cannot be empty' if key.empty?
      raise ArgumentError, 'Cannot modify internal attribute dictionaries.' if Esq::Attr::RESERVED_MODIFY.include?(dict)
      Esq.icall('attr.set', { 'owner' => attr_target, 'dict' => dict, 'key' => key, 'v' => Esq::Attr.enc(args[2]) },
                Esq.attr_typename(self))
      args[2]
    end

    def delete_attribute(*args)
      raise ArgumentError, 'Dictionary name required' if args.empty?
      dict = Esq.text_of(args[0])
      owner = attr_target
      what = Esq.attr_typename(self)
      return false unless Esq.icall('attr.dict', { 'owner' => owner, 'dict' => dict }, what)
      if args.size < 2
        raise ArgumentError, 'Cannot delete internal attribute dictionaries.' if Esq::Attr::RESERVED_DELETE.include?(dict)
        Esq.icall('attr.delete_dict', { 'owner' => owner, 'dict' => dict }, what)
        true
      else
        raise ArgumentError, 'Cannot modify internal attribute dictionaries.' if Esq::Attr::RESERVED_MODIFY.include?(dict)
        r = Esq.icall('attr.delete_key', { 'owner' => owner, 'dict' => dict, 'key' => Esq.text_of(args[1]),
                                           'prune' => !Esq::Attr::RESERVED_DELETE.include?(dict) }, what)
        r['found'] ? true : false
      end
    end
  end

  def self.attr_typename(obj)
    obj.respond_to?(:esq_typename, true) ? obj.send(:esq_typename) : obj.class.name.to_s.split('::').last
  end

end

module Sketchup
  Entity.prepend(Esq::AttrHolder)

  # ====================================================================== AttributeDictionaries
  class AttributeDictionaries < Entity
    include Enumerable
    def initialize(owner)
      @owner = owner
      @handle = "ads:#{owner}"
    end

    def attr_owner; "ads:#{@owner}"; end

    def esq_typename; 'Attribute'; end
    private :esq_typename

    def names
      Esq.icall('attr.names', { 'owner' => @owner }, 'Attribute') || []
    end
    private :names

    def valid?; @owner == 'model' || !!Esq.icall('attr.names', { 'owner' => @owner }); rescue StandardError; false; end

    def size; names.size; end
    alias length size

    def [](name)
      s = Esq.str_arg(name)
      names.include?(s) ? AttributeDictionary.new(@owner, s) : nil
    end

    def each
      return enum_for(:each) unless block_given?
      names.each { |n| yield AttributeDictionary.new(@owner, n) }
      nil
    end

    def delete(dict)
      name = if dict.is_a?(String) then dict
             elsif dict.is_a?(AttributeDictionary) then dict.name
             else raise ArgumentError, 'wrong argument type.  Expected String or AttributeDictionary'
             end
      return self unless names.include?(name)
      raise ArgumentError, 'Cannot delete internal attribute dictionaries' if Esq::Attr::RESERVED_DELETE.include?(name)
      Esq.icall('attr.delete_dict', { 'owner' => @owner, 'dict' => name }, 'Attribute')
      self
    end

    # The entity whose dictionaries these are (the model for the model's own).
    def parent
      m = Sketchup.active_model
      case @owner
      when /\Ah:([efvcgasnp]:\d+:\d+)\z/ then Entity.wrap(Regexp.last_match(1))
      when /\Ai:\d+:\d+\z/ then Entity.wrap(@owner)
      when /\Ad:0:(\d+)\z/ then ComponentDefinition.from_ctx(Regexp.last_match(1))
      when /\Ab:0:(\d+)\z/ then ComponentDefinition.from_ctx(Regexp.last_match(1)).behavior
      when /\Amat:(.*)\z/m then m.materials[Regexp.last_match(1)]
      when /\Alayer:(.*)\z/m then m.layers[Regexp.last_match(1)]
      when 'dl' then m.definitions
      else m
      end
    end
  end

  # ====================================================================== AttributeDictionary
  class AttributeDictionary < Entity
    include Enumerable
    def initialize(owner, name)
      @owner = owner
      @name = name
      @handle = "ad:#{owner}|#{name}"
    end

    def attr_owner; "ad:#{@owner}|#{@name}"; end

    def esq_typename; 'Attribute'; end
    private :esq_typename

    def pairs
      Esq.icall('attr.dict', { 'owner' => @owner, 'dict' => @name }, 'Attribute') or
        raise TypeError, 'reference to deleted Attribute'
    end
    private :pairs

    def key_arg(k)
      s = Esq.text_of(k)
      raise ArgumentError, 'String expected' if s.empty?
      s
    end
    private :key_arg

    def valid?
      !Esq.icall('attr.dict', { 'owner' => @owner, 'dict' => @name }).nil?
    rescue StandardError
      false
    end

    def name; pairs; @name; end
    def size; pairs.size; end
    alias length size
    def empty?; pairs.empty?; end
    def keys; pairs.map { |k, _| k }; end
    def values; pairs.map { |_, v| Esq::Attr.dec(v) }; end

    def [](key)
      k = key_arg(key)
      r = Esq.icall('attr.get', { 'owner' => @owner, 'dict' => @name, 'key' => k }, 'Attribute')
      r['found'] ? Esq::Attr.dec(r['v']) : nil
    end

    def []=(key, value)
      raise FrozenError.new("can't modify frozen #{self.class}: #{inspect}", receiver: self) if frozen?
      pairs
      k = key_arg(key)
      return self if Esq::Attr::RESERVED_MODIFY.include?(@name)
      Esq.icall('attr.set', { 'owner' => @owner, 'dict' => @name, 'key' => k, 'v' => Esq::Attr.enc(value) }, 'Attribute')
      self
    end

    def each
      return enum_for(:each) unless block_given?
      pairs.each { |k, v| yield k, Esq::Attr.dec(v) }
      nil
    end
    alias each_pair each

    def each_key
      return enum_for(:each_key) unless block_given?
      keys.each { |k| yield k }
      nil
    end

    def delete_key(key)
      pairs
      k = key_arg(key)
      return nil if Esq::Attr::RESERVED_MODIFY.include?(@name)
      r = Esq.icall('attr.delete_key', { 'owner' => @owner, 'dict' => @name, 'key' => k, 'prune' => false }, 'Attribute')
      r['found'] ? Esq::Attr.dec(r['v']) : nil
    end

    def parent; AttributeDictionaries.new(@owner); end
  end

  # ====================================================================== Behavior
  class Behavior < Entity
    def initialize(ctx)
      @ctx = ctx
      @handle = "b:0:#{ctx}"
    end

    def attr_owner; "b:0:#{@ctx}"; end

    def info
      Esq.icall('def.info', { 'ctx' => @ctx }, 'ComponentDefinition')['behavior']
    end
    private :info

    def put(k, v)
      Esq.icall('def.set', { 'ctx' => @ctx, 'behavior' => { k => v } }, 'ComponentDefinition')
      self
    end
    private :put

    def valid?; Esq.call('valid', 'h' => "d:0:#{@ctx}"); end

    def always_face_camera?; info['always_face_camera']; end
    def always_face_camera=(v); put('always_face_camera', v ? true : false); end
    def cuts_opening?; info['cuts_opening']; end
    def cuts_opening=(v); put('cuts_opening', v ? true : false); end
    def is2d?; info['is2d']; end
    def is2d=(v); put('is2d', v ? true : false); end
    def shadows_face_sun?; info['shadows_face_sun']; end
    def shadows_face_sun=(v); put('shadows_face_sun', v ? true : false); end
    def no_scale_mask?; info['no_scale_mask']; end
    def no_scale_mask=(v); put('no_scale_mask', Esq.to_long(v)); end
    def snapto; info['snapto']; end

    def snapto=(v)
      n = Esq.to_long(v)
      raise ArgumentError, 'Snapto value out of range 0..3' unless (0..3).cover?(n)
      put('snapto', n)
    end

    def parent; ComponentDefinition.from_ctx(@ctx); end
  end

  # ====================================================================== Set
  class Set
    include Enumerable
    def inspect; to_s; end
    def initialize
      @items = {}.compare_by_identity
    end

    def ordered
      xs = @items.keys
      return xs unless xs.all? { |x| x.is_a?(Integer) }
      xs.sort_by { |x| x >= 0 ? [0, x] : [1, x] }
    end
    private :ordered

    def insert(*objs); objs.each { |o| @items[o] = true }; self; end

    def delete(obj)
      @items.delete(obj) ? obj : nil
    end

    def include?(obj); @items.key?(obj); end
    alias contains? include?
    def size; @items.size; end
    alias length size
    def empty?; @items.empty?; end
    def clear; @items.clear; self; end

    def each
      return enum_for(:each) unless block_given?
      ordered.each { |x| yield x }
      nil
    end

    def to_a; ordered; end
  end

  # ====================================================================== InstancePath
  class InstancePath
    include Enumerable
    def inspect; to_s; end

    def initialize(path)
      raise TypeError, "wrong argument type #{path.class} (expected Array)" unless path.is_a?(Array)
      @path = []
      @leaf = nil
      raise ArgumentError, 'invalid instance path' unless fill(path)
    end

    def instance?(e); e.is_a?(ComponentInstance) || e.is_a?(Group); end
    private :instance?

    # Remplit depuis un tableau d'entités comme SketchUp : avant la dernière
    # place, seulement des groupes ou des instances de composant ; à la dernière, une entité,
    # rangée comme instance ou comme feuille ; aucun élément effacé. L'emboîtement n'est pas
    # vérifié ici mais par valid? : [instance, instance] se construit et n'est pas valide.
    def fill(path)
      path.each_with_index do |e, i|
        return false unless e.is_a?(Entity) && e.valid?
        if instance?(e)
          @path << e
        elsif i == path.size - 1
          @leaf = e
        else
          return false
        end
      end
      true
    end
    private :fill

    def initialize_copy(src)
      super
      @path = src.instance_variable_get(:@path).dup
    end

    def clear; @path.clear; @leaf = nil; self; end
    def clone; self.class.allocate.tap { |c| c.instance_variable_set(:@path, @path.dup); c.instance_variable_set(:@leaf, @leaf) }; end

    def copy(other)
      raise TypeError, 'wrong argument type (expected Sketchup::InstancePath)' unless other.is_a?(InstancePath)
      @path = other.instance_variable_get(:@path).dup
      @leaf = other.instance_variable_get(:@leaf)
      self
    end

    # Les éléments du chemin ; comme SketchUp, TypeError si l'un a été effacé.
    def elements
      a = @leaf ? @path + [@leaf] : @path.dup
      raise TypeError, 'invalid instance path - deleted elements' unless a.all?(&:valid?)
      a
    end
    private :elements

    def to_a; elements; end
    def size; elements.size; end
    alias length size
    def empty?; size.zero?; end

    def each
      return enum_for(:each) unless block_given?
      elements.each { |e| yield e }
      nil
    end

    def [](i)
      i = Esq.to_long(i)
      raise IndexError, 'index out of bounds' if i.negative? || i >= size
      to_a[i]
    end

    def include?(e); a = elements; e.is_a?(Entity) && a.include?(e); end
    def root; elements.first; end
    def leaf; elements; @leaf; end

    def leaf=(e)
      if e.nil?
        @leaf = nil
      else
        raise TypeError, 'wrong argument type (expected Sketchup::Entity)' unless e.is_a?(Entity)
        @leaf = e
      end
      e
    end

    def push(e)
      raise TypeError, 'wrong argument type (expected Sketchup::Entity)' unless e.is_a?(Entity)
      raise ArgumentError, 'cannot push to a path that already has a leaf entity' if @leaf
      instance?(e) ? @path << e : @leaf = e
      self
    end

    def pop(*args)
      return nil if empty?
      n = args.empty? ? 1 : Esq.to_long(args[0])
      raise ArgumentError, 'negative array size' if n.negative?
      popped = nil
      [n, size].min.times do
        e = @leaf || @path.last
        popped ||= e
        @leaf ? @leaf = nil : @path.pop
      end
      popped
    end

    # deux chemins dont l'un a un élément effacé ne sont égaux que s'ils sont
    # le même objet.
    def ==(other)
      return false unless other.is_a?(InstancePath)
      mine = @leaf ? @path + [@leaf] : @path
      theirs = other.instance_variable_get(:@path) + [other.instance_variable_get(:@leaf)].compact
      return equal?(other) unless mine.all?(&:valid?) && theirs.all?(&:valid?)
      mine == theirs
    end

    # non vide, sans élément effacé, chaque instance dans la
    # définition de la précédente, la feuille dans celle de la dernière ; une feuille seule doit
    # être hors de toute définition.
    def valid?
      all = @leaf ? @path + [@leaf] : @path
      return false if all.empty? || !all.all?(&:valid?)
      return !@leaf.respond_to?(:ctx) || @leaf.ctx == '0' if @path.empty?
      @path.each_cons(2) { |a, b| return false unless b.ctx == a.definition.ctx }
      @leaf.nil? || !@leaf.respond_to?(:ctx) || @leaf.ctx == @path.last.definition.ctx
    end

    def persistent_id_path; to_a.map(&:persistent_id).join('.'); end

    def transformation(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)" if args.size > 1
      elements
      idx = args.empty? ? nil : Esq.to_long(args[0])
      raise IndexError, 'index out of bounds' if idx && (idx.negative? || idx >= size)
      xs = idx ? @path.first(idx + 1) : @path
      xs.inject(Geom::Transformation.new) { |acc, e| acc * e.transformation }
    end
  end
end

module Esq
  # Un argument de chemin d'instance comme SketchUp le convertit.
  def self.instance_path_arg(arg)
    unless arg.is_a?(Sketchup::InstancePath) || arg.is_a?(Array)
      raise TypeError, 'Argument must be type of Sketchup::InstancePath or an Array representing an instance path'
    end
    path = arg.is_a?(Array) ? (Sketchup::InstancePath.new(arg) rescue nil) : arg
    raise ArgumentError, 'Provided instance path is not valid.' unless path&.valid?
    path
  end

  # Une étiquette cachée, ou rangée dans un dossier caché à quelque niveau que ce soit
  def self.tag_hidden?(layer)
    return false unless layer
    return true unless layer.visible?
    f = layer.folder
    while f
      return true unless f.visible?
      f = f.folder
    end
    false
  end
end

# =========================================================================================
# Groupes et instances de composant
# =========================================================================================
module Sketchup
  # Méthodes communes (Group et ComponentInstance ont la même interface dans SketchUp).
  module InstanceCommon
    def attr_owner; @handle; end

    def esq_typename; self.class.name.split('::').last; end
    private :esq_typename

    def info; Esq.icall('inst.info', { 'h' => @handle }, esq_typename); end

    def set(h)
      Esq.icall('inst.set', { 'h' => @handle }.merge(h.transform_keys(&:to_s)), esq_typename)
    end
    private :set

    def definition; ComponentDefinition.from_ctx(info['defctx']); end

    def name; info['name']; end

    # Valeurs de retour relevées dans SketchUp.exe (visibles avec `send`) : le composant renvoie
    # lui-même, le groupe son nouveau nom.
    def name=(n)
      set(name: Esq.str_arg(n))
      is_a?(Group) ? name : self
    end

    def locked?; info['locked']; end

    def locked=(v)
      set(locked: v ? true : false)
      v ? true : false
    end

    # Celui du fichier (SketchUp le garde), sinon un GUID stable tiré de la poignée.
    def guid; Esq.ifc_of_guid(info['guid']) || Esq.ifc_guid_of('inst', @handle); end

    def transformation; Geom::Transformation.new(info['xform']); end

    def transformation=(t)
      t = Esq.xform_arg(t)
      raise ArgumentError, 'Unable to set transformation: transformation matrix is not invertible' if Esq.singular?(t)
      set(xform: t.to_a)
      self
    end

    def transform!(t)
      t = Esq.xform_arg(t)
      raise ArgumentError, 'Unable to apply transformation: transformation matrix is not invertible' if Esq.singular?(t)
      set(xform: (t * transformation).to_a)
      self
    end

    def move!(t)
      set(xform: Esq.xform_arg(t).to_a)
      self
    end

    def explode
      hs = Esq.icall('inst.explode', { 'h' => @handle }, esq_typename)
      hs ? hs.map { |h| Entity.wrap(h) } : false
    end

    def make_unique
      Esq.icall('inst.make_unique', { 'h' => @handle }, esq_typename)
      self
    end

    def glued_to
      h = info['glued']
      h && Entity.wrap(h)
    end

    def glued_to=(target)
      to = if target.nil? then nil
           elsif target.is_a?(Entity) && target.respond_to?(:handle) then target.handle
           else raise ArgumentError, 'wrong type - expected Sketchup::Face, Sketchup::Group, SketchUp::Image or Sketchup::ComponentInstance'
           end
      Esq.icall('inst.glue', { 'h' => @handle, 'to' => to }, esq_typename)
      nil
    end

    def hatch_pattern
      n = info['hatch']
      return nil unless n
      m = Sketchup.active_model
      m.respond_to?(:hatch_patterns) ? m.hatch_patterns[n] : nil
    end

    def hatch_pattern=(h)
      set(hatch: h.nil? ? nil : (h.respond_to?(:name) ? h.name : h.to_s))
      h
    end

    def manifold?; Esq.icall('inst.manifold', { 'h' => @handle }, esq_typename); end
    def volume; Esq.icall('inst.volume', { 'h' => @handle }, esq_typename); end

    def equals?(other)
      other = Esq.instance_arg(other)
      Esq.icall('inst.same_geometry', { 'a' => @handle, 'b' => other.handle }, esq_typename)
    end

    def show_differences(*args)
      raise ArgumentError, "'show_differences' requires at least 1 argument." if args.empty?
      equals?(args[0])
    end

    %i[intersect union subtract trim split outer_shell].each do |op|
      define_method(op) do |other|
        other = Esq.instance_arg(other)
        made = Esq.icall('inst.solid', { 'op' => op.to_s, 'a' => @handle, 'b' => other.handle }, esq_typename)
        next nil if made.nil?
        res = made.map { |h| Entity.wrap(h) }
        op == :split ? res : res.first
      end
    end

    def local_bounds
      bb = Geom::BoundingBox.new
      r = Esq.icall('inst.local_bounds', { 'h' => @handle }, esq_typename)
      bb.add(Geom::Point3d.new(*r[0]), Geom::Point3d.new(*r[1])) if r
      bb
    end
  end

  class ComponentInstance
    include InstanceCommon
    def definition=(d)
      raise TypeError, 'wrong argument type (expected Sketchup::ComponentDefinition)' unless d.is_a?(ComponentDefinition)
      raise TypeError, 'reference to deleted ComponentDefinition' unless d.valid?
      set(defctx: d.ctx)
      self
    end
  end

  class Group
    include InstanceCommon

    def entities
      Esq.icall('inst.make_unique', { 'h' => @handle }, 'Group') if info['count'].to_i > 1
      Entities.new(info['defctx'])
    end

    def description; definition.description; end

    def description=(d)
      definition.description = d
      self
    end

    def copy
      Group.new(Esq.icall('inst.copy', { 'h' => @handle }, 'Group'))
    end

    def to_component
      ComponentInstance.new(Esq.icall('inst.to_component', { 'h' => @handle }, 'Group'))
    end
  end

end

module Esq
  def self.instance_arg(o)
    return o if o.is_a?(Sketchup::ComponentInstance) || o.is_a?(Sketchup::Group)
    raise TypeError, 'wrong argument type (expected Sketchup::Group)'
  end
end

# =========================================================================================
# Définitions
# =========================================================================================
module Sketchup
  remove_const(:ComponentDefinition) if const_defined?(:ComponentDefinition, false)
  remove_const(:DefinitionList) if const_defined?(:DefinitionList, false)

  class ComponentDefinition < Drawingelement
    attr_reader :ctx

    def self.from_ctx(ctx); new(ctx); end

    def initialize(ctx)
      @ctx = ctx.to_s
      @handle = "d:0:#{@ctx}"
    end

    def esq_typename; 'ComponentDefinition'; end
    private :esq_typename

    def attr_owner; @handle; end

    def info; Esq.icall('def.info', { 'ctx' => @ctx }, 'ComponentDefinition'); end
    private :info

    def put(h)
      Esq.icall('def.set', { 'ctx' => @ctx }.merge(h.transform_keys(&:to_s)), 'ComponentDefinition')
    end
    private :put

    def ==(o); o.is_a?(ComponentDefinition) && o.ctx == @ctx; end
    alias eql? ==
    def hash; @ctx.hash; end

    def <=>(other)
      raise TypeError, 'wrong argument type (expected Sketchup::ComponentDefinition)' unless other.is_a?(ComponentDefinition)
      name <=> other.name
    end

    def parent; model.definitions; end
    def valid?; Esq.call('valid', 'h' => @handle); end

    # --- identité
    def name; info['name']; end

    def name=(n)
      put(name: Esq.str_arg(n))
      nil
    end

    def description; info['description']; end

    def description=(d)
      put(description: Esq.str_arg(d))
      self
    end

    # Celui du fichier (SketchUp le garde), sinon un GUID stable tiré de la définition.
    def guid
      g = info['guid'].to_s
      g =~ /\A\h{8}-\h{4}-\h{4}-\h{4}-\h{12}\z/ ? g.downcase : Esq.guid_of('def', @ctx)
    end
    def path; info['path']; end

    def load_time
      Time.at(info['loaded_at'])
    end

    # --- nature
    def group?; info['is_group']; end
    def image?; info['is_image'] ? true : false; end
    def internal?; info['path'].empty?; end

    # Vrai quand chaque instance est rangée dans une définition elle-même cachée ou chargée d'un fichier.
    def hidden?
      list = instances
      return false if list.empty?
      list.all? do |i|
        next false if i.ctx == '0'
        parent = ComponentDefinition.from_ctx(i.ctx)
        parent.hidden? || !parent.path.empty?
      end
    end

    def live_component?; !attribute_dictionary('SU_LiveComponent').nil?; end
    def procedural?; !attribute_dictionary('SU_PC').nil?; end
    def manifold?; Esq.icall('def.manifold', { 'ctx' => @ctx }, 'ComponentDefinition'); end

    def entities; info; Entities.new(@ctx); end
    def behavior; info; Behavior.new(@ctx); end

    def count_instances; info['count']; end
    def count_used_instances; info['used']; end

    def instances
      Esq.icall('def.instances', { 'ctx' => @ctx }, 'ComponentDefinition').map { |h| Entity.wrap(h) }
    end

    def insertion_point; info; Geom::Point3d.new(0, 0, 0); end

    def insertion_point=(p)
      xyz = Esq.point_arg(p) or raise ArgumentError, 'Wrong argument type - expected Point3d.'
      Esq.icall('def.set_origin', { 'ctx' => @ctx, 'p' => xyz }, 'ComponentDefinition')
      self
    end

    def invalidate_bounds; info; true; end
    def refresh_thumbnail; info; nil; end

    # --- vignette
    def thumbnail_camera
      t = info['thumb']
      cam = Sketchup::Camera.new rescue nil
      return cam unless cam && t
      cam.set(Geom::Point3d.new(*t[0]), Geom::Point3d.new(*t[1]), Geom::Vector3d.new(*t[2]))
      cam
    end

    def thumbnail_camera=(cam)
      raise TypeError, 'wrong argument type. Expected Sketchup::Camera' unless defined?(Sketchup::Camera) && cam.is_a?(Sketchup::Camera)
      put(thumb: [cam.eye.to_a, cam.target.to_a, cam.up.to_a])
      nil
    end

    def save_thumbnail(path)
      info
      v = Sketchup.active_model.active_view
      return false unless v.respond_to?(:write_image)
      v.write_image(path, 256, 256, true) ? true : false
    rescue StandardError
      false
    end

    # --- fichiers
    def save_as(*args)
      raise ArgumentError, 'wrong number of arguments (given 0, expected 1..2)' if args.empty?
      Esq.icall('def.save', { 'ctx' => @ctx, 'path' => Esq.str_arg(args[0]), 'version' => args[1], 'relink' => true }, 'ComponentDefinition')
    end

    def save_copy(*args)
      raise ArgumentError, 'wrong number of arguments (given 0, expected 1..2)' if args.empty?
      Esq.icall('def.save', { 'ctx' => @ctx, 'path' => Esq.str_arg(args[0]), 'version' => args[1] }, 'ComponentDefinition')
    end

    # --- composants dynamiques / procéduraux
    def attach_procedure(name, params)
      raise TypeError, 'Cannot attach a procedure to this definition.' if group? || image?
      raise ArgumentError, 'Failed to attach procedure.'
    end

    def set_procedure_parameters(_params)
      raise TypeError, 'This definition is not procedural.'
    end

    def control_entities
      raise TypeError, 'Control elements can only be accessed for procedural components.'
    end

    # --- fusion de la géométrie coplanaire
    def merge(*args)
      list = args[0]
      hs = case list
           when nil then nil
           when Array then list.map { |e| e.respond_to?(:handle) ? e.handle : nil }.compact
           else raise RuntimeError, 'Error : merge - First argument must be an entity array or nil'
           end
      Esq.icall('def.merge', { 'ctx' => @ctx, 'hs' => hs }, 'ComponentDefinition') ? true : false
    end

    # --- classifications
    APPLIED = 'AppliedSchemaTypes'

    # Les noms passent par StringValue : nil, un nombre ou un
    # point lèvent TypeError ; une image rend false avant de les lire.
    def add_classification(schema, type)
      return false if image?
      schema = Esq.str_arg(schema)
      type = Esq.str_arg(type)
      s = Esq::Classif.schema(schema) or return false
      return false unless s.type?(type)
      set_attribute(APPLIED, schema, type)
      true
    end

    def remove_classification(*args)
      raise ArgumentError, 'Invalid number of arguments.' unless args.size == 1 || args.size == 2
      return false if image?
      schema = Esq.str_arg(args[0])
      type = args.size > 1 ? Esq.str_arg(args[1]) : nil
      applied = get_attribute(APPLIED, schema)
      return false if applied.nil? || (type && type != applied)
      delete_attribute(APPLIED, schema)
      delete_attribute(schema)
      true
    end

    def get_classification_value(path)
      p = Esq::Classif.path(path)
      return nil unless get_attribute(APPLIED, p[0])
      get_attribute(p[0], Esq::Classif.value_key(self, p))
    end

    def set_classification_value(path, value)
      p = Esq::Classif.path(path)
      return false unless get_attribute(APPLIED, p[0])
      set_attribute(p[0], Esq::Classif.value_key(self, p), value)
      true
    end

    # --- Drawingelement : une définition n'est pas dessinée comme telle
    def hidden=(v); info; v; end
    def visible?; !hidden?; end
    def visible=(v); info; v; end
    def layer; model.layers[0]; end
    def layer=(l); info; l; end
    def material; nil; end
    def material=(m); info; m; end
    def casts_shadows?; true; end
    def casts_shadows=(v); v; end
    def receives_shadows?; true; end
    def receives_shadows=(v); v; end

    def erase!
      model.definitions.remove(self)
      nil
    end

    def bounds
      bb = Geom::BoundingBox.new
      entities.each { |e| bb.add(e.bounds) if e.respond_to?(:bounds) }
      bb
    end
  end

  class DefinitionList < Entity
    include Enumerable
    def initialize
      @handle = 'dl:0:0'
    end

    def esq_typename; 'DefinitionList'; end
    private :esq_typename

    def attr_owner; 'dl'; end

    def all; Esq.icall('defs.all', {}, 'DefinitionList').map { |d| ComponentDefinition.from_ctx(d['ctx']) }; end
    private :all

    def valid?; true; end
    def length; all.size; end
    alias size length
    def parent; model; end

    def each
      return enum_for(:each) unless block_given?
      all.each { |d| yield d }
      self
    end

    def [](k)
      if k.is_a?(String)
        if k =~ /\A\h{8}-\h{4}-\h{4}-\h{4}-\h{12}\z/
          d = all.find { |x| x.guid.casecmp?(k) }
          return d if d
        end
        return all.find { |d| d.name == k }
      end
      i = Esq.to_long(k)
      list = all
      j = i.negative? ? i + list.size : i
      raise IndexError, "index #{j} out of range [0, #{list.size})" if j.negative? || j >= list.size
      list[j]
    end
    alias at []

    def add(*args)
      name = args.empty? ? 'Composant' : Esq.str_arg(args[0])
      ComponentDefinition.from_ctx(Esq.icall('defs.add', { 'name' => name }, 'DefinitionList'))
    end

    def unique_name(name)
      Esq.icall('defs.unique_name', { 'name' => Esq.str_arg(name) }, 'DefinitionList')
    end

    def purge_unused
      Esq.icall('defs.purge', {}, 'DefinitionList')
      self
    end

    def remove(defn)
      raise TypeError, 'wrong argument type (expected Sketchup::ComponentDefinition)' unless defn.is_a?(ComponentDefinition)
      raise TypeError, 'reference to deleted ComponentDefinition' unless defn.valid?
      Esq.icall('defs.remove', { 'ctx' => defn.ctx }, 'DefinitionList')
      true
    end

    def load(*args)
      opts = args.last.is_a?(Hash) ? args.pop : {}
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 1)" unless args.size == 1
      _ = opts
      path = Esq.str_arg(args[0])
      ComponentDefinition.from_ctx(Esq.icall('defs.load', { 'path' => path }, 'DefinitionList'))
    end

    # URL → fichier temporaire → load. Avec un rappel (onPercentChange/onSuccess/onFailure),
    # les échecs lui sont signalés au lieu d'être levés.
    def load_from_url(*args)
      raise RuntimeError, 'Usage: load_from_url url [callback]' if args.empty?
      url = Esq.text_of(args[0])
      cb = args[1]
      fail_with = lambda do |msg|
        raise IOError, msg unless cb
        cb.onFailure(msg) if cb.respond_to?(:onFailure)
        nil
      end
      require 'net/http'
      require 'uri'
      require 'tmpdir'
      uri = begin
        URI.parse(url)
      rescue StandardError
        nil
      end
      return fail_with.call('Invalid component URL') unless uri.is_a?(URI::HTTP)
      cb.onPercentChange(0.0) if cb.respond_to?(:onPercentChange)
      return fail_with.call('Download cancelled') if cb.respond_to?(:cancelled?) && cb.cancelled?
      res = begin
        Net::HTTP.get_response(uri)
      rescue StandardError
        nil
      end
      return fail_with.call('Invalid component URL') unless res.is_a?(Net::HTTPSuccess)
      tmp = File.join(Dir.tmpdir, "esq_dl_#{Process.pid}_#{rand(1 << 30)}.skp")
      File.binwrite(tmp, res.body)
      cb.onPercentChange(100.0) if cb.respond_to?(:onPercentChange)
      d = begin
        load(tmp)
      rescue IOError
        return fail_with.call('Invalid component file')
      ensure
        File.delete(tmp) if File.exist?(tmp)
      end
      cb.onSuccess if cb.respond_to?(:onSuccess)
      d
    end

    # Importe un fichier d'un autre format comme définition (importeurs intégrés : OBJ, STL).
    def import(*args)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 1..2)" unless (1..2).cover?(args.size)
      path = Esq.str_arg(args[0])
      raise IOError, 'File not found' unless File.file?(path)
      ext = File.extname(path).downcase.delete('.')
      raise ArgumentError, 'Skp files not supported. Use DefinitionList#load instead.' if ext == 'skp'
      importer = Esq::MeshImport.for(ext) or raise ArgumentError, 'No importer found for file extension'
      d = add(File.basename(path, '.*'))
      begin
        importer.call(path, d.entities)
      rescue StandardError
        remove(d)
        raise IOError, 'Failed converting file to SKP'
      end
      d.send(:put, path: path)
      d
    end
  end
end

# Importeurs de maillages intégrés (OBJ, STL), pour DefinitionList#import.
module Esq
  module MeshImport
    def self.for(ext)
      { 'obj' => method(:obj), 'stl' => method(:stl) }[ext]
    end

    def self.faces(ents, polys)
      polys.each do |pts|
        next if pts.size < 3
        begin
          ents.add_face(*pts)
        rescue StandardError
          nil
        end
      end
    end

    def self.obj(path, ents)
      v = []
      polys = []
      File.foreach(path) do |l|
        w = l.split
        case w[0]
        when 'v' then v << Geom::Point3d.new(*w[1, 3].map(&:to_f))
        when 'f' then polys << w[1..].map { |t| v[t.split('/')[0].to_i.then { |i| i.negative? ? v.size + i : i - 1 }] }
        end
      end
      faces(ents, polys)
    end

    def self.stl(path, ents)
      data = File.binread(path)
      polys = []
      if data[0, 5] == 'solid' && data.include?('facet')
        pts = []
        data.each_line do |l|
          w = l.split
          next unless w[0] == 'vertex'
          pts << Geom::Point3d.new(*w[1, 3].map(&:to_f))
          polys << pts.shift(3) if pts.size == 3
        end
      else
        n = data.byteslice(80, 4).unpack1('V')
        n.times do |i|
          f = data.byteslice(84 + i * 50, 50).unpack('e12')
          polys << [Geom::Point3d.new(*f[3, 3]), Geom::Point3d.new(*f[6, 3]), Geom::Point3d.new(*f[9, 3])]
        end
      end
      faces(ents, polys)
    end
  end
end

# =========================================================================================
# Classifications (schémas .skc)
# =========================================================================================
module Esq
  module Classif
    Schema = Struct.new(:name, :namespace, :path, :types) do
      def type?(t); types.key?(t); end
    end

    # Les schémas chargés sont rangés dans le modèle (propriétaire « classifications » du pont) :
    # ils sont enregistrés avec lui, comme dans SketchUp, et chaque modèle a les siens.
    OWNER = { 'owner' => 'classifications', 'dict' => 'schemas' }.freeze

    class << self
      def schemas
        pairs = Esq.icall('attr.dict', OWNER) || []
        pairs.to_h do |name, v|
          ns, path, *types = Esq::Attr.dec(v)
          [name, Schema.new(name, ns, path, types.to_h { |t| [t, true] })]
        end
      end

      def schema(name); schemas[name]; end

      def store(s)
        Esq.icall('attr.set', OWNER.merge('key' => s.name, 'v' => Esq::Attr.enc([s.namespace, s.path, *s.types.keys])))
      end

      def path(p)
        raise TypeError, "wrong argument type #{p.class} (expected Array)" unless p.is_a?(Array)
        p.map { |s| Esq.text_of(s) }
      end

      def value_key(defn, p)
        rest = p[1..] || []
        applied = defn.get_attribute(Sketchup::ComponentDefinition::APPLIED, p[0])
        rest = rest[1..] if rest.first && rest.first == applied
        rest.join('/')
      end

      # Un .skc est une archive zip : documentProperties.xml (titre), Schemas/*.xsd et *.xsd.filter.
      # Lue par le lecteur ZIP commun (esq_core::zip : décalages vérifiés, budget de
      # décompression) : l'ancien lecteur Ruby gonflait sans limite (bombe de décompression).
      def zip_entries(path)
        r = Esq.call('misc.zip_texts', path: path.to_s)
        raise IOError, r['err'].to_s if r['err']
        r['files']
      end

      def load(path)
        files = zip_entries(path)
        props = files['documentProperties.xml'].to_s.dup.force_encoding('UTF-8')
        title = props[%r{<dp:title>(.*?)</dp:title>}m, 1]
        return false if title.nil? || title.empty?
        xsd = files.find { |n, _| n.end_with?('.xsd') && n.start_with?('Schemas/') && !n.end_with?('ex.xsd') }
        ns = xsd && xsd[1].dup.force_encoding('UTF-8')[/targetNamespace="([^"]+)"/, 1]
        filt = files.find { |n, _| n.end_with?('.xsd.filter') && !n.end_with?('ex.xsd.filter') }
        types = {}
        if filt
          # Un bloc { … } après un type liste les attributs affichés, pas des types.
          depth = 0
          filt[1].dup.force_encoding('UTF-8').each_line do |l|
            break if l.start_with?('// Blacklist')
            s = l.strip
            if s.start_with?('{') then depth += 1
            elsif s.start_with?('}') then depth = [depth - 1, 0].max
            elsif depth.zero? && s =~ /\A(\w+)\z/ then types[Regexp.last_match(1)] = true
            end
          end
        end
        if xsd
          parents = {}
          xsd[1].dup.force_encoding('UTF-8').scan(/<xs:element substitutionGroup="\w+:(\w+)" name="(\w+)"/) { |p, n| parents[n] = p }
          # Chaque parent une fois : deux types parents l'un de l'autre bouclaient sans fin.
          types.keys.each do |t|
            while (t = parents[t]) && !types.key?(t)
              types[t] = true
            end
          end
        end
        store(Schema.new(title, ns.to_s, path, types))
        # Le fichier .skc part avec le modèle : le .skp le porte, comme dans SketchUp.
        Esq.call('misc.keep_skc', name: title, path: path.to_s)
        true
      rescue StandardError
        false
      end

      def unload(name); Esq.icall('attr.delete_key', OWNER.merge('key' => name, 'prune' => true)); end
    end
  end
end

module Sketchup
  class ClassificationSchema
    def initialize(s); @s = s; end
    def inspect; to_s; end
    def name; @s.name.dup; end
    def namespace; @s.namespace.dup; end

    def <=>(other)
      other.is_a?(ClassificationSchema) ? (name <=> other.name) : nil
    end
  end

  class Classifications
    include Enumerable
    def inspect; to_s; end
    def keys; Esq::Classif.schemas.keys.sort; end
    def size; Esq::Classif.schemas.size; end
    alias length size

    def [](k)
      if k.is_a?(String)
        s = Esq::Classif.schema(k)
        return s && ClassificationSchema.new(s)
      end
      i = Esq.to_long(k)
      n = size
      i += n if i.negative?
      return nil if i.negative? || i >= n
      ClassificationSchema.new(Esq::Classif.schema(keys[i]))
    end

    def each
      return enum_for(:each) unless block_given?
      keys.each { |n| yield ClassificationSchema.new(Esq::Classif.schema(n)) }
      nil
    end

    def load_schema(path)
      Esq::Classif.load(Esq.str_arg(path))
    end

    def unload_schema(name)
      Esq::Classif.unload(Esq.text_of(name))
      true
    end
  end

  # ====================================================================== Modèle, matières, étiquettes
  module ModelAttributes
    include Esq::AttrHolder
    def attr_owner; 'model'; end
    def esq_typename; 'Model'; end
    private :esq_typename

    # Jamais nil pour un modèle ouvert (SketchUp ne rend nil que sans modèle) : ceux de
    # SketchUp sortent d'un gabarit qui porte déjà des dictionnaires (GeoReference,
    # GSU_ContributorsInfo), et TestUp, comme des extensions, écrit
    # `model.attribute_dictionaries.delete(nom)` sans vérifier.
    def attribute_dictionaries
      super || Sketchup::AttributeDictionaries.new(attr_target)
    end

    def classifications; @classifications ||= Classifications.new; end
    def model_attribute_dictionaries; attribute_dictionaries; end
  end
  Model.prepend(ModelAttributes)

  class Material
    include Esq::AttrHolder
    def attr_owner; "mat:#{name}"; end
  end

  class Layer
    include Esq::AttrHolder
    def attr_owner; "layer:#{name}"; end
  end
end

SnapTo_Arbitrary = 0 unless defined?(SnapTo_Arbitrary)
SnapTo_Horizontal = 1 unless defined?(SnapTo_Horizontal)
SnapTo_Vertical = 2 unless defined?(SnapTo_Vertical)
SnapTo_Sloped = 3 unless defined?(SnapTo_Sloped)
