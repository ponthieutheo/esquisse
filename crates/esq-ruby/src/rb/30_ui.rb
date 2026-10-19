# Esquisse — module UI (zone « ui ») : menus, commandes, barres d'outils, boîtes de message,
# minuteries, console. Les boîtes web sont dans 45_ui_dialogs.rb.
#
# Comportement relevé dans SketchUp.exe (fonctions natives des `rb_define_*`) : exceptions, retours,
# arités. Les fenêtres elles-mêmes sont ouvertes par l'application (`host.ui.*`).

# ---------------------------------------------------------------- constantes globales
{
  MB_OK: 0, MB_OKCANCEL: 1, MB_ABORTRETRYIGNORE: 2, MB_YESNOCANCEL: 3, MB_YESNO: 4, MB_RETRYCANCEL: 5,
  MB_MULTILINE: 0x10,
  VK_SPACE: 0x20, VK_PRIOR: 0x21, VK_NEXT: 0x22, VK_END: 0x23, VK_HOME: 0x24, VK_LEFT: 0x25, VK_UP: 0x26,
  VK_RIGHT: 0x27, VK_DOWN: 0x28, VK_INSERT: 0x2d, VK_DELETE: 0x2e, VK_SHIFT: 0x10, VK_CONTROL: 0x11,
  VK_ALT: 0x12, VK_COMMAND: 0x12, VK_MENU: 0x12,
  ALT_MODIFIER_KEY: 0x12, ALT_MODIFIER_MASK: 0x20, COPY_MODIFIER_KEY: 0x11, COPY_MODIFIER_MASK: 8,
  CONSTRAIN_MODIFIER_KEY: 0x10, CONSTRAIN_MODIFIER_MASK: 4,
  IDOK: 1, IDCANCEL: 2, IDABORT: 3, IDRETRY: 4, IDIGNORE: 5, IDYES: 6, IDNO: 7,
  MF_ENABLED: 0, MF_GRAYED: 1, MF_DISABLED: 2, MF_CHECKED: 8, MF_UNCHECKED: 0,
  TB_HIDDEN: 0, TB_VISIBLE: 1, TB_NEVER_SHOWN: -1,
  CMD_SELECT: 0x521e, CMD_PAINT: 0x5252, CMD_ERASE: 0x521b, CMD_RECTANGLE: 0x5266, CMD_LINE: 0x521c,
  CMD_CIRCLE: 0x5268, CMD_ARC: 0x5249, CMD_POLYGON: 0x5267, CMD_FREEHAND: 0x5227, CMD_PUSHPULL: 0x5231,
  CMD_TEXT: 0x539d, CMD_MOVE: 0x5238, CMD_ROTATE: 0x5289, CMD_EXTRUDE: 0x5415, CMD_SCALE: 0x52f4,
  CMD_OFFSET: 0x526c, CMD_MEASURE: 0x5220, CMD_PROTRACTOR: 0x5241, CMD_SKETCHCS: 0x5286,
  CMD_SECTION: 0x5359, CMD_DRAWOUTLINES: 0x5363, CMD_DRAWCUTS: 0x5364, CMD_ORBIT: 0x290c,
  CMD_DOLLY: 0x291b, CMD_ZOOM: 0x290d, CMD_ZOOM_WINDOW: 0x291e, CMD_ZOOM_EXTENTS: 0x291f,
  CMD_CAMERA_UNDO: 0x2921, CMD_WIREFRAME: 0x290e, CMD_HIDDENLINE: 0x290f, CMD_SHADED: 0x2910,
  CMD_TEXTURED: 0x292b, CMD_TRANSPARENT: 0x2911, CMD_WALK: 0x2918, CMD_PAN: 0x291d,
  CMD_MAKE_COMPONENT: 0x525b, CMD_DIMENSION: 0x53a2, CMD_VIEW_ISO: 0x290b, CMD_VIEW_TOP: 0x2905,
  CMD_VIEW_FRONT: 0x2906, CMD_VIEW_RIGHT: 0x2907, CMD_VIEW_BACK: 0x2909, CMD_VIEW_LEFT: 0x2908,
  CMD_VIEW_BOTTOM: 0x290a, CMD_VIEW_PERSPECTIVE: 0x2917, CMD_POSITION_CAMERA: 0x52b1,
  CMD_NEW: 0xe100, CMD_OPEN: 0xe101, CMD_SAVE: 0xe103, CMD_CUT: 0xe123, CMD_COPY: 0xe122,
  CMD_PASTE: 0xe125, CMD_DELETE: 0x521d, CMD_UNDO: 0xe12b, CMD_REDO: 0xe12c, CMD_PRINT: 0xe107,
  CMD_PAGE_NEW: 0x524b, CMD_PAGE_DELETE: 0x5256, CMD_PAGE_UPDATE: 0x524c, CMD_PAGE_NEXT: 0x2927,
  CMD_PAGE_PREVIOUS: 0x2928, CMD_RUBY_CONSOLE: 0x53e6, CMD_SKETCHAXES: 0x291a, CMD_SHOWHIDDEN: 0x52a2,
  CMD_SHOWHIDDENGEOMETRY: 0x52a3, CMD_SHOWHIDDENOBJECTS: 0x52a1, CMD_SHOWGUIDES: 0x55dc,
  CMD_SELECTION_ZOOM_EXT: 0x53dd, CMD_DISPLAY_FOV: 0x53f6
}.each { |k, v| Object.const_set(k, v) unless Object.const_defined?(k) }

# ---------------------------------------------------------------- plomberie interne
module Esq
  HOST_MISSING = "l'application n'est pas disponible"

  @cmds = {}          # id => UI::Command
  @menu_items = []    # { cmd: id|nil, menu: 'Tools/Sous-menu', sep: bool }
  @toolbar_defs = {}  # nom => { name:, items: [id|nil] }
  @next_cmd_id = 0
  @dialogs = {}
  @timers = {}
  @notifs = {}
  @cursors = {}
  @next_dialog_id = 0
  @context_handlers = []

  class << self
    attr_reader :cmds, :menu_items, :toolbar_defs, :dialogs, :timers, :notifs, :cursors, :context_handlers

    def next_cmd_id; @next_cmd_id += 1; end
    def next_dialog_id; @next_dialog_id += 1; end

    # Appel hôte ; sans application (tests, usage hors interface), renvoie `fallback`.
    def ui_host(op, args = {}, fallback = nil)
      call("host.ui.#{op}", args)
    rescue RuntimeError => e
      raise unless e.message.include?(HOST_MISSING)
      fallback
    end

    # NUM2INT / NUM2LONG de MRI : entiers, flottants, objets avec to_int.
    def num2int(v)
      case v
      when Integer then v
      when Float then v.to_i
      when nil then raise TypeError, 'no implicit conversion from nil to integer'
      else
        raise TypeError, "no implicit conversion of #{v.class} into Integer" unless v.respond_to?(:to_int)
        v.to_int
      end
    end

    def num2dbl(v)
      case v
      when Numeric then v.to_f
      when nil then raise TypeError, "can't convert nil into Float"
      when String then raise TypeError, "no implicit conversion to float from string"
      else raise TypeError, "can't convert #{v.class} into Float"
      end
    end

    def unexpected_type(v, expected)
      raise TypeError, "wrong argument type #{v.nil? ? 'nil' : v.class} (expected #{expected})"
    end

    # Drapeaux de validation d'une commande (MF_*). nil → actif.
    def command_flags(cmd)
      pr = cmd.get_validation_proc or return 0
      v = pr.call
      v.is_a?(Integer) ? v : 0
    rescue Exception => e # rubocop:disable Lint/RescueException
      warn "[validation] #{e.class}: #{e.message}"
      0
    end

    def command_info(cmd)
      { 'id' => cmd.__id, 'title' => cmd.menu_text, 'tooltip' => cmd.tooltip, 'status' => cmd.status_bar_text,
        'small_icon' => cmd.small_icon, 'large_icon' => cmd.large_icon, 'flags' => command_flags(cmd) }
    end

    # Entrées de menu (avec séparateurs), dans l'ordre d'ajout.
    def commands_json
      list = @menu_items.map do |it|
        if it[:sep]
          { 'id' => 0, 'menu' => it[:menu], 'sep' => true, 'title' => '' }
        else
          cmd = @cmds[it[:cmd]] or next
          command_info(cmd).merge('menu' => it[:menu], 'sep' => false)
        end
      end.compact
      JSON.generate(list)
    end

    def toolbars_json
      list = @toolbar_defs.values.map do |t|
        items = t[:items].map { |id| id && @cmds[id] ? command_info(@cmds[id]) : nil }
        { 'name' => t[:name], 'items' => items }
      end
      JSON.generate(list)
    end

    def invoke_command(id)
      cmd = @cmds[id] or return false
      cmd.proc&.call
      true
    end

    # Éléments du menu contextuel ajoutés par les gestionnaires (`UI.add_context_menu_handler`).
    # Renvoie le JSON des entrées { id, title, sep, flags, path }.
    def context_menu_json
      @ctx_items = []
      menu = Sketchup::Menu.__new('Context')
      @context_handlers.each do |h|
        begin
          h.call(menu)
        rescue Exception => e # rubocop:disable Lint/RescueException
          warn "[menu contextuel] #{e.class}: #{e.message}"
        end
      end
      JSON.generate(@menu_items.select { |i| i[:menu].start_with?('Context') }.map do |it|
        it[:sep] ? { 'id' => 0, 'sep' => true, 'menu' => it[:menu], 'title' => '' } :
          command_info(@cmds[it[:cmd]]).merge('menu' => it[:menu], 'sep' => false)
      end).tap { @menu_items.reject! { |i| i[:menu].start_with?('Context') } }
    end
  end
end

# ---------------------------------------------------------------- UI::Command
module UI
  class Command
    # Une commande « interne » (argument Integer) n'a ni bloc ni titre.
    def initialize(title, &blk)
      if title.is_a?(Integer)
        @proc = nil
      else
        raise ArgumentError, 'tried to create Proc object without a block' unless blk
        @proc = blk
        @menu_text = title.is_a?(String) ? title : ''
      end
      @menu_text ||= ''
      @tooltip = ''
      @status_bar_text = ''
      @small_icon = ''
      @large_icon = ''
      @validation = nil
      @extension = nil
      @id = Esq.next_cmd_id
      Esq.cmds[@id] = self
    end

    attr_reader :menu_text, :tooltip, :status_bar_text, :small_icon, :large_icon, :extension

    def __id; @id; end

    def proc; @proc; end

    def menu_text=(v); @menu_text = v.to_s; end
    def tooltip=(v); @tooltip = v.to_s; end
    def status_bar_text=(v); @status_bar_text = v.to_s; end
    def small_icon=(v); @small_icon = v.to_s; end
    def large_icon=(v); @large_icon = v.to_s; end

    def set_validation_proc(&blk)
      raise ArgumentError, 'tried to create Proc object without a block' unless blk
      @validation = blk
      self
    end

    def get_validation_proc; @validation; end

    def extension=(ext)
      unless ext.nil?
        if defined?(::SketchupExtension) && !ext.is_a?(::SketchupExtension)
          raise TypeError, 'expected SketchupExtension'
        end
      end
      @extension = ext
      nil
    end
  end
end

# ---------------------------------------------------------------- Sketchup::Menu
module Sketchup
  class Menu
    MENU_NAMES = { 1 => 'File', 2 => 'Edit', 3 => 'View', 4 => 'Camera', 5 => 'Draw', 6 => 'Tools',
                   7 => 'Window', 8 => 'Extensions', 9 => 'Help', 10 => 'Extensions/Developer' }.freeze

    class << self
      undef_method :new
      def __new(path); m = allocate; m.__send__(:__init, path); m; end
      def __for_key(key); (n = MENU_NAMES[key]) ? __new(n) : nil; end
    end

    def __init(path); @path = path; end
    def __path; @path; end

    def add_item(*args, &blk)
      return nil unless args.size == 1 || args.size == 2
      target = args[0]
      cmd = if target.is_a?(UI::Command)
              target
            else
              raise ArgumentError, 'tried to create Proc object without a block' unless blk
              UI::Command.new(target.to_s, &blk)
            end
      Esq.menu_items << { cmd: cmd.__id, menu: @path, sep: false }
      cmd.__id
    end

    def add_separator
      Esq.menu_items << { cmd: nil, menu: @path, sep: true }
      nil
    end

    def add_submenu(*args)
      raise ArgumentError, 'wrong number of arguments (given 0, expected 1..2)' if args.empty?
      name = args[0].to_s
      if args[1].is_a?(Hash)
        # `above:` / `below:` : identifiants de commande. L'ordre exact n'est pas modélisé.
      elsif args[1] && !args[1].is_a?(Integer)
        Esq.unexpected_type(args[1], 'Hash')
      end
      Sketchup::Menu.__new("#{@path}/#{name}")
    end

    def set_validation_proc(id, &blk)
      cmd = Esq.cmds[Esq.num2int(id)]
      return false unless cmd
      raise ArgumentError, 'tried to create Proc object without a block' unless blk
      cmd.set_validation_proc(&blk)
      true
    end
  end
end

# ---------------------------------------------------------------- UI::Toolbar
module UI
  class Toolbar
    include Enumerable

    class << self
      def new(name)
        n = name.to_s
        t = Esq.toolbar_defs[n] ||= { name: n, items: [] }
        obj = send(:allocate)
        obj.__bind(t)
        obj
      end
      private :allocate
    end

    def __bind(t); @t = t; end

    def add_item(cmd)
      raise TypeError, "wrong argument type #{cmd.class} (expected UI::Command)" unless cmd.is_a?(UI::Command)
      @t[:items] << cmd.__id
      self
    end

    def add_separator
      @t[:items] << nil
      self
    end

    def show; Esq.ui_host('toolbar_set_visible', { name: name, visible: true, plugin: true }); nil; end
    def hide; Esq.ui_host('toolbar_set_visible', { name: name, visible: false, plugin: true }); nil; end

    def restore
      show unless get_last_state == TB_HIDDEN
      nil
    end

    def get_last_state
      Esq.ui_host('toolbar_state', { name: name, plugin: true }, TB_NEVER_SHOWN)
    end

    def name; @t[:name]; end

    def visible?
      Esq.ui_host('toolbar_visible', { name: name, plugin: true }, false) ? true : false
    end

    def length; @t[:items].size; end
    alias size length

    def each
      return enum_for(:each) unless block_given?
      @t[:items].each { |id| yield(id ? Esq.cmds[id] : '|') }
      nil
    end
  end
end

# ---------------------------------------------------------------- module UI
module UI
  class << self
    def messagebox(*args)
      raise ArgumentError, 'Too few arguments: text required' if args.empty?
      text = args[0].is_a?(String) ? args[0] : args[0].to_s
      type = args.size > 1 ? Esq.num2int(args[1]) : 0
      if type == MB_MULTILINE
        title = args.size > 2 ? args[2].to_s : ''
        Esq.ui_host('messagebox', { text: text, type: type, title: title })
        return IDOK
      end
      r = Esq.ui_host('messagebox', { text: text, type: type, title: 'Esquisse' }, IDOK)
      r.is_a?(Integer) ? r : IDOK
    end

    # UI.inputbox(prompts, defaults = [], list = [], title = '') → tableau, ou false si annulé.
    def inputbox(*args)
      raise ArgumentError, 'Too few arguments' if args.empty?
      prompts = args[0]
      Esq.unexpected_type(prompts, 'Array') unless prompts.is_a?(Array)
      defaults = args.size > 1 ? args[1] : []
      Esq.unexpected_type(defaults, 'Array') unless defaults.is_a?(Array)
      list = []
      title = ''
      if args.size > 2
        if args[2].is_a?(Array)
          list = args[2]
          title = args[3].to_s if args.size > 3
        else
          title = args[2].to_s
        end
      end
      Esq::Forms.legacy_inputbox(prompts.map(&:to_s), defaults, list.map(&:to_s), title)
    end

    def menu(*args)
      key = 8
      unless args.empty?
        a = args[0]
        if a.is_a?(String)
          s = a.downcase
          key = case s
                when 'file' then 1
                when 'edit' then 2
                when 'view' then 3
                when 'camera' then 4
                when 'draw' then 5
                when 'tools' then 6
                when 'window' then 7
                when 'help' then 9
                when 'extensions', '' then 8
                when 'developer' then 10
                else
                  case s[0]
                  when 'f' then 1
                  when 'e' then 2
                  when 'v' then 3
                  when 'c' then 4
                  when 'd' then 5
                  when 't' then 6
                  when 'w' then 7
                  when 'h' then 9
                  when 'p' then 8
                  else raise ArgumentError, "Unknown menu #{a}"
                  end
                end
        else
          key = Esq.num2int(a)
        end
      end
      Sketchup::Menu.__for_key(key)
    end

    def toolbar(name)
      Toolbar.new(name)
    end

    def toolbar_names
      builtin = Esq.ui_host('toolbar_names', {}, [])
      (builtin + Esq.toolbar_defs.keys).uniq
    end

    def toolbar_visible?(name)
      Esq.ui_host('toolbar_visible', { name: name.to_s, plugin: Esq.toolbar_defs.key?(name.to_s) }, false) ? true : false
    end

    def set_toolbar_visible(name, visible)
      Esq.ui_host('toolbar_set_visible', { name: name.to_s, visible: visible ? true : false, plugin: Esq.toolbar_defs.key?(name.to_s) }, false) ? true : false
    end

    def refresh_toolbars; nil; end
    def refresh_inspectors; nil; end

    def add_context_menu_handler(&blk)
      raise ArgumentError, 'tried to create Proc object without a block' unless blk
      Esq.context_handlers << blk
      Esq.context_handlers.size
    end

    def beep; Esq.ui_host('beep'); nil; end

    def play_sound(path)
      Esq.ui_host('play_sound', { path: path.to_s })
      nil
    end

    def openURL(url)
      Esq.ui_host('openurl', { url: url.to_s }, false) ? true : false
    end

    def start_timer(*args, &blk)
      raise ArgumentError, 'tried to create Proc object without a block' unless blk
      raise ArgumentError, 'Too few arguments' if args.empty?
      secs = Esq.num2dbl(args[0])
      repeat = args.size > 1 && args[1] ? true : false
      id = Esq.ui_host('timer_add', { ms: (secs * 1000).to_i, repeat: repeat }, nil)
      id ||= (Esq.timers.keys.max || 0) + 1
      Esq.timers[id] = { proc: blk, repeat: repeat }
      id
    end

    def stop_timer(id)
      id = Esq.num2int(id)
      Esq.timers.delete(id)
      Esq.ui_host('timer_remove', { id: id })
      nil
    end

    def create_cursor(*args)
      raise ArgumentError, 'Too few arguments' if args.size < 3
      path = args[0].to_s
      x = Esq.num2int(args[1])
      y = Esq.num2int(args[2])
      id = Esq.ui_host('cursor_create', { path: path, x: x, y: y }, nil) || (Esq.cursors.size + 1)
      Esq.cursors[id] = [path, x, y]
      id
    end

    def set_cursor(id)
      id = Esq.num2int(id)
      Esq.ui_host('cursor_set', { id: id }, Esq.cursors.key?(id)) ? true : false
    end

    def scale_factor(*args)
      if args.size > 1
        raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..1)"
      end
      v = Esq.ui_host('scale_factor', { window: args[0] ? Esq.num2int(args[0]) : nil }, 1.0)
      v.to_f
    end

    def get_clipboard_data
      Esq.ui_host('clip_get')
    end

    # La chaîne passe par StringValue : nil ou un nombre lèvent
    # TypeError au lieu de vider le presse-papiers.
    def set_clipboard_data(text)
      Esq.ui_host('clip_set', { text: Esq.str_arg(text) }, false) ? true : false
    end

    def inspector_names
      %w[Materials Components Styles Scenes Shadows Layers SoftenEdges Outliner Instructor Fog MatchPhoto
         EntityInfo Overlays Environments]
    end

    def show_inspector(name)
      Esq.ui_host('inspector_show', { name: name.to_s }, false) ? true : false
    end

    def model_info_pages
      %w[Animation Classifications Components Credits Dimension File Location Rendering Statistics Text Units]
    end

    def preferences_pages
      %w[Applications Compatibility Drawing Files General OpenGL Shortcuts Template Workspace]
    end

    def show_model_info(*args)
      return true if args.empty?
      Esq.ui_host('modelinfo_show', { page: args[0].to_s })
      true
    end

    def show_preferences(*args)
      Esq.ui_host('prefs_show', { page: args.empty? ? nil : args[0].to_s })
      true
    end

    def show_extension_manager
      Esq.ui_host('extmgr_show')
      nil
    end

    def show_extension_warehouse(*_args)
      Esq.ui_host('openurl', { url: 'https://extensions.sketchup.com' })
      nil
    end

    # UI.openpanel(title = '', directory = '', filename_or_filter = '') → chemin ou nil
    def openpanel(title = '', directory = '', filter = '')
      r = Esq.ui_host('openpanel', { title: title.to_s, dir: directory.to_s, filename: filter.to_s })
      r.nil? ? nil : r.tr('\\', '/')
    end

    def savepanel(title = '', directory = '', filename = '')
      r = Esq.ui_host('savepanel', { title: title.to_s, dir: directory.to_s, filename: filename.to_s })
      r.nil? ? nil : r.tr('\\', '/')
    end

    # UI.select_directory(title:, directory:, select_multiple: false)
    def select_directory(*args)
      opts = args.last.is_a?(Hash) ? args.last : {}
      multi = opts[:select_multiple] ? true : false
      r = Esq.ui_host('select_directory', { title: opts[:title].to_s, directory: opts[:directory].to_s,
                                         multiple: multi })
      return nil if r.nil?
      r.is_a?(Array) ? r.map { |p| p.tr('\\', '/') } : r.tr('\\', '/')
    end
  end
end

# ---------------------------------------------------------------- Sketchup::Console
module Sketchup
  class Console
    class << self
      undef_method :new
      private :allocate
      def __make; send(:allocate); end
    end

    def write(obj)
      s = obj.is_a?(String) ? obj : obj.to_s
      return 0 if s.empty?
      Esq.ui_host('console', { action: 'write', text: s }) || ($stdout.print(s) && nil)
      s.length
    end
    alias_method :<<, :write

    def clear; Esq.ui_host('console', { action: 'clear' }); nil; end
    def show; Esq.ui_host('console', { action: 'show' }); nil; end
    def hide; Esq.ui_host('console', { action: 'hide' }); nil; end
    def visible?; Esq.ui_host('console', { action: 'visible' }, false) ? true : false; end
    def flush; self; end
    def sync; true; end
  end
end
SKETCHUP_CONSOLE = Sketchup::Console.__make unless defined?(SKETCHUP_CONSOLE)

class Object
  def file_loaded?(f); Sketchup.file_loaded?(f); end
  def file_loaded(f); Sketchup.file_loaded(f); end

  # Le reste de ce que Tools/sketchup.rb de SketchUp définit au niveau supérieur (documenté
  # dans l'API). Les extensions d'exemple s'en servent (Shapes : add_separator_to_menu).
  private

  # Un séparateur dans le menu nommé, une seule fois par menu quel que soit le nombre
  # d'extensions qui le demandent.
  def add_separator_to_menu(menu_name)
    $menu_separator_list ||= []
    return if $menu_separator_list.include?(menu_name)
    UI.menu(menu_name).add_separator
    $menu_separator_list << menu_name
    nil
  end

  # UI.inputbox, redemandé tant qu'une valeur saisie ne se convertit pas (message d'erreur).
  def inputbox(*args)
    loop do
      return UI.inputbox(*args)
    rescue ArgumentError => e
      UI.messagebox(e.message)
    end
  end

  # Ajoute le dossier au chemin de chargement et charge chacun de ses fichiers .rb.
  def require_all(dirname)
    dir = File.expand_path(dirname.to_s)
    $LOAD_PATH << dir unless $LOAD_PATH.include?(dir)
    Dir.glob(File.join(dir, '*.rb')).sort.each { |f| require f }
    nil
  end

  def show_ruby_panel; SKETCHUP_CONSOLE.show; end
end

Encoding.default_external = Encoding::UTF_8
Encoding.default_internal = Encoding::UTF_8
