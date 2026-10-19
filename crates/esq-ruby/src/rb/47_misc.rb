# Esquisse — zone « misc » : Sketchup::Http, SKSocket, Licensing, Amplitude, Test, extensions.
# Comportements relevés dans SketchUp 2026 (messages d'erreur, valeurs de retour, constantes).

module Esq
  # Utilitaires de conversion à la manière de l'API C de Ruby (rb_num2long, rb_unexpected_type…).
  module Conv
    module_function

    def type_name(o)
      case o
      when nil then 'nil'
      when true then 'true'
      when false then 'false'
      else o.class.to_s
      end
    end

    # TypeError « wrong argument type X (expected String) » (rb_unexpected_type).
    def unexpected(o, expected)
      raise TypeError, "wrong argument type #{type_name(o)} (expected #{expected})"
    end

    def str!(o)
      o.is_a?(String) ? o : unexpected(o, 'String')
    end

    def num2long(o)
      case o
      when Integer then o
      when Float
        raise FloatDomainError, o.to_s if o.nan? || o.infinite?
        o.to_i
      when nil then raise TypeError, 'no implicit conversion from nil to integer'
      when true, false then raise TypeError, "no implicit conversion of #{o} into Integer"
      else
        raise TypeError, "no implicit conversion of #{o.class} into Integer" unless o.respond_to?(:to_int)
        o.to_int
      end
    end

    def num2dbl(o)
      case o
      when Float then o
      when Integer then o.to_f
      when nil then raise TypeError, "can't convert nil into Float"
      when true, false then raise TypeError, "can't convert #{o} into Float"
      when String then raise TypeError, 'no implicit conversion to float from string'
      else
        raise TypeError, "can't convert #{o.class} into Float" unless o.respond_to?(:to_f)
        o.to_f
      end
    end

    def truthy(o)
      !(o.nil? || o == false)
    end
  end

  @http_requests = {}
  @http_seq = 0
  @socket_listeners = []
  @licenses = {}
  @registered_extensions = []
  @support_paths = nil
  @plugin_report = []

  class << self
    attr_reader :http_requests, :socket_listeners, :licenses, :registered_extensions, :plugin_report

    def next_http_id
      @http_seq += 1
    end

    # Remet au code Ruby ce que les threads réseau ont reçu (réponses HTTP, données de socket) :
    # les blocs s'exécutent ici, sur le thread Ruby. Renvoie le nombre d'événements traités.
    def pump
      events = Esq.call('misc.poll')
      events.each do |ev|
        begin
          dispatch_event(ev)
        rescue Exception => e # rubocop:disable Lint/RescueException
          warn "Error: #<#{e.class}: #{e.message}>"
          (e.backtrace || []).first(5).each { |l| warn l }
        end
      end
      events.size
    end

    def dispatch_event(ev)
      case ev['type']
      when 'http_done'
        req = @http_requests.delete(ev['id']) or return
        req.__finish(ev)
      when 'download', 'upload'
        req = @http_requests[ev['id']] or return
        req.__progress(ev['type'], ev['current'], ev['total'])
      when 'sock_data'
        data = ev['data'].unpack1('m').force_encoding(Encoding::UTF_8)
        @socket_listeners.dup.each { |l| l.call(data) }
      end
    end

    # --- chemins ---------------------------------------------------------------------------
    def paths
      @paths ||= begin
        p = Esq.call('misc.paths')
        plugins = p['plugins'].tr('\\', '/')
        root = File.dirname(plugins)
        { plugins: plugins, root: root, tools: "#{root}/Tools", exe_dir: p['exe_dir'].to_s.tr('\\', '/'),
          content: p['content'].to_s.tr('\\', '/') }
      end
    end

    # Dossiers où chercher les fichiers de support (équivalent de l'arborescence d'installation) :
    # le contenu livré avec Esquisse (Materials, Styles, Templates…) y est.
    def support_roots
      env = ENV['ESQ_RESOURCES']
      [env, paths[:root], paths[:content], paths[:exe_dir]].compact.reject(&:empty?).uniq
    end

    # Plugins et Tools sont dans le chemin de chargement, comme dans SketchUp.
    def prepare_paths
      return if @paths_prepared
      @paths_prepared = true
      [paths[:tools], paths[:plugins]].each do |d|
        $LOAD_PATH.unshift(d) if File.directory?(d) && !$LOAD_PATH.include?(d)
      end
    end

    # Charge les fichiers .rb du dossier Plugins (un par un : une erreur n'arrête pas les autres).
    def load_plugins(dir)
      prepare_paths
      n = 0
      return n unless File.directory?(dir)
      return n if Sketchup.plugins_disabled?
      $LOAD_PATH.unshift(dir) unless $LOAD_PATH.include?(dir)
      @plugin_report = []
      Dir.glob(File.join(dir, '*.rb')).sort_by { |f| File.basename(f).downcase }.each do |f|
        begin
          load f
          n += 1
          @plugin_report << { 'file' => File.basename(f), 'ok' => true }
        rescue Exception => e # rubocop:disable Lint/RescueException
          warn "[plugins] #{File.basename(f)}: #{e.class}: #{e.message}"
          @plugin_report << { 'file' => File.basename(f), 'ok' => false, 'error' => "#{e.class}: #{e.message}" }
        end
      end
      # AppObserver#onExtensionsLoaded: once every extension of the folder is loaded.
      Esq.emit(Esq.observers[:app], :onExtensionsLoaded)
      n
    end

    # Pour le gestionnaire d'extensions de l'application.
    def extensions_json
      JSON.generate(@registered_extensions.map do |e|
        { 'name' => e.name, 'version' => e.version, 'creator' => e.creator, 'description' => e.description,
          'loaded' => e.loaded?, 'enabled' => e.__enabled?, 'load_on_start' => e.load_on_start?,
          'path' => e.extension_path.to_s }
      end)
    end

    def plugin_report_json
      JSON.generate(@plugin_report)
    end

    def set_extension_enabled(name, on)
      ext = @registered_extensions.find { |e| e.name == name } or return false
      on ? ext.check : ext.uncheck
      true
    end

    # Installe une licence locale (pas de serveur de licences : sert aux tests et aux hôtes).
    def set_license(id, state, days = 0, error = '')
      @licenses[id] = [state, days, error]
    end
  end

  # check/uncheck n'ont pas besoin du document : on prépare simplement le chemin de chargement
  # avant chaque script.
  module PrepareRun
    def __run
      Esq.prepare_paths
      super
    end
  end
  singleton_class.prepend(PrepareRun)
end

# ------------------------------------------------------------------ Sketchup::Http
module Sketchup
  module Http
    GET = 'GET'
    POST = 'POST'
    PUT = 'PUT'
    DELETE = 'DELETE'
    HEAD = 'HEAD'
    OPTIONS = 'OPTIONS'
    PATCH = 'PATCH'
    STATUS_UNKNOWN = 0
    STATUS_SUCCESS = 1
    STATUS_PENDING = 2
    STATUS_CANCELED = 3
    STATUS_FAILED = 4

    class Response
      class << self
        def new(*)
          raise TypeError, "allocator undefined for #{self}"
        end

        def allocate
          raise TypeError, "allocator undefined for #{self}"
        end

        def __make(status, headers, body) # :nodoc:
          r = Class.instance_method(:allocate).bind_call(self)
          r.instance_variable_set(:@status_code, status)
          r.instance_variable_set(:@headers, headers)
          r.instance_variable_set(:@body, body)
          r
        end
      end

      def status_code; @status_code; end
      def body; @body.dup; end

      def headers; @headers.dup; end
    end

    class Request
      def initialize(url, verb = nil)
        Esq::Conv.str!(url)
        unless verb.nil?
          raise TypeError, 'invalid Http verb argument)' unless verb.is_a?(String)
        end
        @url = url
        @verb = verb || GET.dup
        @headers = {}
        @body = ''
        @status = STATUS_UNKNOWN
        @block = nil
        @download_cb = nil
        @upload_cb = nil
        @id = nil
      end

      def url; @url.dup; end

      def method; @verb; end

      def method=(v)
        @verb = Esq::Conv.str!(v)
      end

      def headers; @headers; end

      def headers=(h)
        raise TypeError, "wrong argument type #{Esq::Conv.type_name(h)} (expected Hash)" unless h.is_a?(Hash)
        @headers = h
      end

      def body; @body; end

      def body=(b)
        @body = Esq::Conv.str!(b)
      end

      def status; @status; end

      def start(&block)
        raise 'start can only be called once per request)' unless @status == STATUS_UNKNOWN
        @block = block if block
        seen = {}
        hdrs = {}
        @headers.each do |k, v|
          Esq::Conv.str!(k)
          Esq::Conv.str!(v)
          raise ArgumentError, 'Invalid headers - duplicate entries' if seen[k.downcase]
          seen[k.downcase] = true
          hdrs[k] = v
        end
        @id = Esq.next_http_id
        @status = STATUS_PENDING
        Esq.http_requests[@id] = self
        Esq.call('misc.http_start', id: @id, method: @verb, url: @url, headers: hdrs, body: [@body].pack('m0'))
        true
      end

      def cancel
        if @status == STATUS_PENDING
          @status = STATUS_CANCELED
          Esq.http_requests.delete(@id)
          Esq.call('misc.http_cancel', id: @id)
        end
        true
      end

      def set_download_progress_callback(&block)
        raise LocalJumpError, 'no block given' unless block
        @download_cb = block
        true
      end

      def set_upload_progress_callback(&block)
        raise LocalJumpError, 'no block given' unless block
        @upload_cb = block
        true
      end

      def __progress(kind, current, total) # :nodoc:
        cb = kind == 'download' ? @download_cb : @upload_cb
        cb&.call(current, total)
      end

      def __finish(ev) # :nodoc:
        if ev['ok']
          body = ev['body'].unpack1('m')
          ctype = ev['headers'].find { |k, _| k.casecmp?('Content-Type') }&.last.to_s
          enc = ctype =~ /charset=\s*"?([\w.:-]+)/i ? (Encoding.find($1) rescue nil) : nil
          body.force_encoding(enc || Encoding::UTF_8)
          headers = ev['headers'].sort_by { |k, _| k.downcase }.to_h
          @status = STATUS_SUCCESS
          resp = Response.__make(ev['status'], headers, body)
        else
          @status = STATUS_FAILED
          resp = Response.__make(0, {}, '')
        end
        @block&.call(self, resp)
      end

      def inspect; "#<#{self.class}:0x#{'%016x' % (object_id * 8)}>"; end
    end
  end

  # ---------------------------------------------------------------- Sketchup::Licensing
  module Licensing
    LICENSED = 0
    EXPIRED = 1
    TRIAL = 2
    TRIAL_EXPIRED = 3
    NOT_LICENSED = 4

    class ExtensionLicense
      class << self
        def new(*)
          raise NoMethodError, "undefined method `new' for #{self}:Class"
        end

        def __make(state, days, error) # :nodoc:
          l = allocate
          l.instance_variable_set(:@state, state)
          l.instance_variable_set(:@days, days)
          l.instance_variable_set(:@error, error)
          l
        end
      end

      def licensed?; [LICENSED, TRIAL].include?(@state); end
      def state; @state; end
      def days_remaining; @days; end
      def error_description; @error.dup; end
    end

    def self.get_extension_license(id)
      raise TypeError, "no implicit conversion of #{Esq::Conv.type_name(id)} into String" unless id.respond_to?(:to_str)
      st, days, err = Esq.licenses[id.to_str] || [NOT_LICENSED, 0, '']
      ExtensionLicense.__make(st, days, err)
    end
    freeze
  end

  # ---------------------------------------------------------------- Sketchup::Amplitude
  # Aperçu des guides : sans module d'aperçu actif, toutes les fonctions renvoient le module.
  module Amplitude
    def self.get_all_guides; self; end
    def self.open_preview; self; end
    def self.close_preview; self; end
    def self.update_preview(_s); self; end
  end
end

# ------------------------------------------------------------------ SKSocket
module SKSocket
  @port = 0

  def self.add_socket_listener(&block)
    Esq.socket_listeners << block
    Esq.socket_listeners.size
  end

  def self.connect(host, port)
    Esq.socket_listeners.clear
    Esq.call('misc.sock_disconnect')
    host = Esq::Conv.str!(host)
    @port = Esq::Conv.num2long(port)
    ok = Esq.call('misc.sock_connect', host: host, port: @port)
    ok ? "Connected to #{@port}" : "NOT connected to #{@port}"
  end

  def self.disconnect
    Esq.call('misc.sock_disconnect')
    "Disconnected from #{@port}"
  end

  def self.write(data)
    data = Esq::Conv.str!(data)
    msg = "SKSocket: #{data}"
    $stdout.puts(msg)
    Esq.call('misc.sock_write', data: [data].pack('m0'))
    msg
  end
end

# ------------------------------------------------------------------ Test (module interne)
module Test
  module ExtensionNotificationType
    EXTENSION_INSTALLED = 0
    EXTENSION_UNINSTALLED = 1
    EXTENSION_UPDATED = 2
    EXTENSION_ENABLED = 3
    EXTENSION_DISABLED = 4
  end

  module ExtensionResult
    RESULT_SUCCEEDED = 0
    RESULT_FAILED = 1
    RESULT_FAILED_REMOVE_LICENSE = 2
    RESULT_FAILED_REMOVE_FILES = 3
    RESULT_FAILED_NO_ARCHIVE_HANDLER = 4
    RESULT_FAILED_WRITE_ACCESS = 5
    RESULT_FAILED_DOWNLOAD = 6
    RESULT_FAILED_UPDATE_LICENSE = 7
    RESULT_FAILED_UPDATE_VERSION_ID = 8
    RESULT_FAILED_ARCHIVE_READ = 9
  end

  @auto_drag_distance = 6
  @auto_click_time = 0.35
  @show_fps = false
  @suppress_warnings = false
  @min_feature_culling_size = 0
  @tooltips = false
  @bubble_lines = false
  @cv_enabled = true
  @notifications = []

  class << self
    attr_reader :notifications

    def show_error_dialog; nil; end

    def repair_model(*_a); true; end

    # Compare le modèle à un fichier : aucune différence à signaler pour un modèle intact.
    def diff(*_a); []; end

    def time_display(*_a); nil; end

    def time_pick(count = 1000)
      n = Esq::Conv.num2long(count)
      t = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      n.times { Sketchup.active_model.active_view.invalidate rescue nil }
      Process.clock_gettime(Process::CLOCK_MONOTONIC) - t
    end

    def AutoDragDistance; @auto_drag_distance; end

    def AutoDragDistance=(v)
      old = @auto_drag_distance
      @auto_drag_distance = Esq::Conv.num2long(v)
      old
    end

    def AutoClickTime; @auto_click_time; end

    def AutoClickTime=(v)
      old = @auto_click_time
      @auto_click_time = Esq::Conv.num2dbl(v)
      old
    end

    def CorruptModel; false; end

    def show_fps(v)
      @show_fps = Esq::Conv.num2long(v) != 0 if v.is_a?(Integer)
      @show_fps
    end

    def FindFile(dir, name)
      d = Esq::Conv.str!(dir)
      n = Esq::Conv.str!(name)
      hit = Dir.glob(File.join(d, '**', n)).first
      hit ? hit.tr('\\', '/') : false
    end

    def project_textures_from_photo; false; end

    def flip_images_for_origami(_a, _b); nil; end

    def eat_mem(n)
      (@eaten ||= []) << ("\0" * Esq::Conv.num2long(n))
      nil
    end

    def suppress_warnings?; @suppress_warnings; end

    def suppress_warnings=(v)
      @suppress_warnings = Esq::Conv.truthy(v)
    end

    def min_feature_culling_size?; @min_feature_culling_size; end

    def min_feature_culling_size=(v)
      old = @min_feature_culling_size
      @min_feature_culling_size = Esq::Conv.num2long(v)
      old
    end

    def ToolTips=(v)
      old = @tooltips
      @tooltips = Esq::Conv.truthy(v)
      old
    end

    def draw_horizontal_bubble_lines?; @bubble_lines; end

    def draw_horizontal_bubble_lines=(v)
      old = @bubble_lines
      @bubble_lines = Esq::Conv.truthy(v)
      old
    end

    def cv_enabled?; @cv_enabled; end

    def cv_enabled=(v)
      @cv_enabled = Esq::Conv.truthy(v)
      nil
    end

    def export_animation(*_a); nil; end

    def unregister_procedure(name)
      Esq::Conv.str!(name)
      defined?(Esq::Procedures) ? Esq::Procedures.unregister(name.to_str) : false
    end

    # send_extension_notification(type, result, extension_id, sync_list: false)
    def send_extension_notification(*args, **opts)
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 3)" unless args.size == 3
      type = Esq::Conv.num2long(args[0])
      raise ArgumentError, "Invalid extension notification type: #{type}" unless (0..4).cover?(type)
      result = Esq::Conv.num2long(args[1])
      raise ArgumentError, "Invalid extension notification result: #{result}" unless (0..9).cover?(result)
      id = Esq::Conv.str!(args[2])
      @notifications << { type: type, result: result, id: id, sync_list: Esq::Conv.truthy(opts[:sync_list]) }
      nil
    end
  end
end

# ------------------------------------------------------------------ SketchupExtension
class SketchupExtension
  attr_accessor :name, :description, :version, :copyright, :creator, :id, :version_id

  def initialize(name, path)
    raise TypeError, "no implicit conversion of #{Esq::Conv.type_name(name)} into String" unless name.respond_to?(:to_str)
    raise TypeError, "no implicit conversion of #{Esq::Conv.type_name(path)} into String" unless path.respond_to?(:to_str)
    @name = name.to_str
    @path = path.to_str
    @description = ''
    @version = ''
    @copyright = ''
    @creator = ''
    @id = ''
    @version_id = ''
    @loaded = false
    @enabled = true
    @registered = false
    @load_on_start = false
    @loading = false
    @origin_dir = File.dirname(caller_locations(1, 1)&.first&.absolute_path || File.join(Dir.pwd, 'x'))
  end

  # Chemin absolu du fichier principal (avec son extension .rb), relatif au fichier appelant
  # ou au chemin de chargement.
  def extension_path
    return @resolved if @resolved
    cands = []
    if @path =~ %r{\A([A-Za-z]:)?[/\\]}
      cands << @path
    else
      cands << File.join(@origin_dir, @path)
      $LOAD_PATH.each { |d| cands << File.join(d.to_s, @path) }
    end
    cands = cands.flat_map { |c| File.extname(c).empty? ? ["#{c}.rb", c] : [c, "#{c}.rb"] }
    hit = cands.find { |c| File.file?(c) }
    hit ? (@resolved = File.expand_path(hit).tr('\\', '/')) : nil
  end

  def check
    @enabled = true
    return true if @loaded
    return true if @loading
    @loading = true
    begin
      p = extension_path
      raise LoadError, "cannot load such file -- #{@path}" unless p
      Kernel.require(p)
      @loaded = true
    ensure
      @loading = false
    end
    true
  end

  def uncheck
    @enabled = false
    true
  end

  def loaded?; @loaded && @enabled; end
  def registered?; @registered; end
  def load_on_start?; @load_on_start; end
  def __enabled?; @enabled; end

  def get_extension_license
    Sketchup::Licensing.get_extension_license(@id)
  end

  def register_from_sketchup # :nodoc:
    @registered = true
  end

  def __set_load_on_start(v) # :nodoc:
    @load_on_start = v
  end
end

module Sketchup
  class ExtensionsManager
    include Enumerable

    class << self
      def new(*)
        raise TypeError, "allocator undefined for #{self}"
      end

      def __instance # :nodoc:
        @instance ||= Class.instance_method(:allocate).bind_call(self)
      end
    end

    def size; Esq.registered_extensions.size; end
    alias length size

    def [](key)
      list = Esq.registered_extensions
      if key.is_a?(String)
        key.empty? ? nil : list.find { |e| e.name == key }
      else
        i = Esq::Conv.num2long(key)
        i += list.size if i < 0
        i >= 0 && i < list.size ? list[i] : nil
      end
    end

    def each
      raise LocalJumpError, 'no block given (yield)' unless block_given?
      Esq.registered_extensions.dup.each { |e| yield e }
      nil
    end

    def keys; Esq.registered_extensions.map(&:name); end
  end

  # La classe de 1re itération n'existe pas dans SketchUp.
  remove_const(:Extensions) if const_defined?(:Extensions, false)
  remove_const(:Extension) if const_defined?(:Extension, false)

  class << self
    def extensions; ExtensionsManager.__instance; end

    # register_extension(extension, load_at_start = false) : true à l'enregistrement, false si une
    # extension de même nom l'était déjà.
    def register_extension(*args)
      raise ArgumentError, 'Too few arguments: extension required' if args.empty?
      ext = args[0]
      load_at_start = args.size > 1 && Esq::Conv.truthy(args[1])
      unless ext.respond_to?(:name) && (ext.respond_to?(:load) || ext.respond_to?(:load, true))
        raise ArgumentError, 'An extension must respond to "name" and "load"'
      end
      name = ext.name.to_s
      ext.register_from_sketchup if ext.respond_to?(:register_from_sketchup, true)
      if Esq.registered_extensions.any? { |e| e.name == name }
        old = Esq.registered_extensions.find { |e| e.name == name }
        load_at_start ? old.check : old.uncheck if old.respond_to?(:check)
        return false
      end
      ext.__set_load_on_start(load_at_start) if ext.respond_to?(:__set_load_on_start)
      Esq.registered_extensions << ext
      ext.check if load_at_start && ext.respond_to?(:check)
      ext.uncheck if !load_at_start && ext.respond_to?(:uncheck)
      true
    end

    def find_support_file(*args)
      raise ArgumentError, 'File name missing' if args.empty?
      name = Esq::Conv.str!(args[0])
      sub = args[1].nil? ? '' : Esq::Conv.str!(args[1])
      return name.tr('\\', '/') if name =~ %r{\A([A-Za-z]:)?[/\\]} && File.exist?(name)
      Esq.support_roots.each do |r|
        p = File.join(r, sub, name)
        return File.expand_path(p).tr('\\', '/') if File.exist?(p)
      end
      nil
    end

    def find_support_files(*args)
      raise ArgumentError, 'File name missing' if args.empty?
      ext = Esq::Conv.str!(args[0]).sub(/\A\*?\./, '')
      sub = args[1].nil? ? '' : Esq::Conv.str!(args[1])
      out = []
      Esq.support_roots.each do |r|
        dir = File.join(r, sub)
        next unless File.directory?(dir)
        Dir.glob(File.join(dir, '**', ext == '*' ? '*' : "*.#{ext}"), File::FNM_CASEFOLD).sort_by(&:downcase).each do |f|
          out << File.expand_path(f).tr('\\', '/') if File.file?(f)
        end
      end
      out.uniq
    end

    def get_resource_path(name)
      name = Esq::Conv.str!(name)
      loc = respond_to?(:get_locale) ? get_locale.to_s : 'en-US'
      Esq.support_roots.each do |r|
        [loc, 'en-US', ''].uniq.each do |l|
          p = File.join(r, 'Resources', l, name)
          return File.expand_path(p).tr('\\', '/') if File.exist?(p)
        end
      end
      ''
    end

    def install_from_archive(*args)
      raise ArgumentError, 'Too few arguments: path required' if args.empty?
      if args.size < 2 || Esq::Conv.truthy(args[1])
        answer = UI.messagebox("Installer cette extension ?\n#{args[0]}", MB_YESNO)
        raise Interrupt, 'User canceled the installation.' if [IDNO, IDCANCEL].include?(answer)
      end
      path = Esq::Conv.str!(args[0])
      raise Exception, "Could not find: #{path}" unless File.file?(path)
      Esq.prepare_paths
      dest = Esq.paths[:plugins]
      before = Dir.glob(File.join(dest, '*.rb')).to_a
      r = Esq.call('misc.unzip', archive: File.expand_path(path), dest: dest)
      raise Exception, 'Could not unzip archive.' unless r['ok']
      (Dir.glob(File.join(dest, '*.rb')).to_a - before).sort.each { |f| load f }
      true
    end

    def is_online
      Esq.call('misc.online') ? true : false
    end

    # SketchUp keeps the flag in its preferences (PREFS /
    # RubyManager_DisablePlugins) and loads no plugin at the next start when it is set.
    def plugins_disabled?
      @plugins_disabled = read_default('PREFS', 'RubyManager_DisablePlugins', false) ? true : false if @plugins_disabled.nil?
      @plugins_disabled
    end

    def plugins_disabled=(v)
      on = Esq::Conv.truthy(v)
      if on != plugins_disabled?
        @plugins_disabled = on
        write_default('PREFS', 'RubyManager_DisablePlugins', on)
      end
      on
    end

    # Comme Kernel#require, avec le dossier Plugins dans le chemin de chargement.
    def require(name)
      Esq.prepare_paths
      Kernel.require(Esq::Conv.str!(name))
    end

    # Charge un fichier avec ou sans extension .rb, cherché dans le chemin de chargement.
    def load(name)
      Esq.prepare_paths
      n = Esq::Conv.str!(name)
      dirs = n =~ %r{\A([A-Za-z]:)?[/\\]} ? [''] : ['', *$LOAD_PATH.map(&:to_s)]
      dirs.each do |d|
        [n, "#{n}.rb"].each do |f|
          p = d.empty? ? f : File.join(d, f)
          return Kernel.load(p) if File.file?(p)
        end
      end
      raise LoadError, "cannot load such file -- #{n}"
    end
  end
end

# ---------------------------------------------------------------- Tools de SketchUp
# SketchUp charge au démarrage ses fichiers Ruby du dossier Tools (sketchup.rb, extensions.rb,
# langhandler.rb) ; les extensions commencent presque toutes par `require 'sketchup.rb'` et
# `require 'extensions.rb'`, qui y rendent false (déjà chargés). Ce qu'ils définissent est
# dans le prélude d'Esquisse : les demander rend false aussi, au lieu de LoadError.
module Esq
  module ToolsRequire
    TOOLS = %w[sketchup extensions langhandler].freeze

    def self.tool?(name)
      n = name.respond_to?(:to_path) ? name.to_path : name
      n.is_a?(String) && TOOLS.include?(File.basename(n, '.rb').downcase) && !n.include?('/') && !n.include?('\\')
    end

    private

    def require(name)
      return false if Esq::ToolsRequire.tool?(name)
      super
    end
  end
end
Kernel.prepend(Esq::ToolsRequire)
Kernel.singleton_class.prepend(Module.new do
  def require(name)
    return false if Esq::ToolsRequire.tool?(name)
    super
  end
end)

# LanguageHandler (Tools/langhandler.rb dans SketchUp) : les chaînes traduites d'une extension,
# lues dans <dossier de l'appelant>/<nom de l'appelant>/Resources/<langue>/<fichier>, sinon en
# anglais (en-US), sinon dans les ressources de l'application. Écrit pour Esquisse d'après la
# documentation de l'API et les tests de TestUp (TC_LanguageHandler).
class LanguageHandler
  def initialize(strings_file_name)
    @strings = {}
    caller_file = caller_locations(1, 1).first&.absolute_path.to_s.tr('\\', '/')
    @resource_dir = File.join(File.dirname(caller_file), File.basename(caller_file, '.*'), 'Resources')
    path = strings_file_name.is_a?(String) ? GetResourcePath(strings_file_name) : ''
    __parse(path) unless path.empty?
  end

  def GetString(key)
    @strings.key?(key) ? @strings[key] : key
  end
  alias [] GetString

  def GetStrings
    @strings
  end
  alias strings GetStrings

  def GetResourcePath(file_name)
    raise ArgumentError, 'file name must be a String' unless file_name.is_a?(String)
    [Sketchup.get_locale.to_s, 'en-US'].uniq.each do |loc|
      p = File.join(@resource_dir, loc, file_name)
      return p if File.file?(p)
    end
    Sketchup.get_resource_path(file_name).to_s
  end
  alias resource_path GetResourcePath

  # Un fichier .strings : des lignes "clé"="valeur"; ; commentaires // et /* … */. Les
  # guillemets ne s'échappent pas : la clé va jusqu'au dernier `"="` de la ligne.
  def __parse(path)
    raw = File.binread(path)
    raw = raw.byteslice(3..) if raw.start_with?("\xEF\xBB\xBF".b) # BOM
    text = raw.force_encoding('UTF-8')
    return unless text.valid_encoding?
    text = text.gsub(%r{/\*.*?\*/}m, '')
    text.each_line do |line|
      next if line.lstrip.start_with?('//')
      m = /\A\s*"(.*)"\s*=\s*"(.*)"\s*;/.match(line) or next
      @strings[m[1]] = m[2]
    end
  rescue SystemCallError
    nil
  end
  private :__parse
end
