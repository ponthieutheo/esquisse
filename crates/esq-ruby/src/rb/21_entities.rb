module Sketchup
  # Entity, Drawingelement, Vertex, Edge, Face, Loop and Entities live in 41_entities_extra.rb;
  # the first two only exist here so that the instance classes below can inherit from them.
  class Entity; end
  class Drawingelement < Entity; end

  class Instance < Drawingelement
    def info; Esq.call('inst.info', 'h' => @handle); end
    def set(h); Esq.call('inst.set', { 'h' => @handle }.merge(h.transform_keys { |k| k.to_s })); end
    def transformation; Geom::Transformation.new(info['xform']); end
    def transformation=(t); set(xform: t.to_a); end
    def transform!(t); self.transformation = t * transformation; self; end
    def move!(t); self.transformation = t; self; end
    def name; info['name']; end
    def name=(n); set(name: n); end
    def locked?; info['locked']; end
    def locked=(v); set(locked: v); end
    def definition; ComponentDefinition.from_ctx(info['defctx']); end
    def explode; Esq.call('inst.explode', 'h' => @handle).map { |h| Entity.wrap(h) }; end
    def origin; transformation.origin; end
    def make_unique; self; end
  end

  class ComponentInstance < Instance
    def to_component; self; end
  end

  class Group < Instance
    def entities; Entities.new(info['defctx']); end
    def definition; ComponentDefinition.from_ctx(info['defctx']); end
    def to_component; self; end
  end

  class ComponentDefinition
    attr_reader :ctx
    def self.from_ctx(ctx); new(ctx); end
    def initialize(ctx); @ctx = ctx; end
    def entities; Entities.new(@ctx); end
    def ==(o); o.is_a?(ComponentDefinition) && o.ctx == @ctx; end
    alias eql? ==
    def hash; @ctx.hash; end
    def name; (Esq.call('def.list').find { |d| d['ctx'] == @ctx } || {})['name']; end
    def group?; false; end
    def valid?; true; end
    def count_instances; 0; end
    def bounds; bb = Geom::BoundingBox.new; entities.each { |e| bb.add(e.bounds) if e.respond_to?(:bounds) }; bb; end
  end

  class DefinitionList
    include Enumerable
    def each(&b); Esq.call('def.list').map { |d| ComponentDefinition.from_ctx(d['ctx']) }.each(&b); end
    def [](k)
      return to_a[k] if k.is_a?(Integer)
      find { |d| d.name == k }
    end
    def add(name); ComponentDefinition.from_ctx(Esq.call('def.add', 'name' => name)['ctx']); end
    def length; to_a.size; end
    alias size length
  end
end
