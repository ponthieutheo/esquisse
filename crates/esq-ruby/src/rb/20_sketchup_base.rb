# encoding: utf-8
# Module Sketchup (fonctions de l'application), Sketchup::Color, extensions.
# Le modèle. Les messages d'erreur reprennent ceux de SketchUp.

module Esq
  class << self
    # Les erreurs typées du pont (« ArgumentError: … ») : voir Esq.raise_native (zone entities).

    # Appelle l'hôte (application) ; renvoie le défaut si l'hôte n'est pas joignable.
    def host(op, args = {}, default = nil)
      call("host.#{op}", args)
    rescue RuntimeError => e
      raise if e.message !~ /application n'est pas disponible|non implémentée|inconnue/
      default
    end

    # Notifie des observateurs : un observateur qui lève n'arrête pas les autres.
    def emit(list, meth, *args)
      list.dup.each do |o|
        begin
          o.__send__(meth, *args) if o.respond_to?(meth)
        rescue StandardError => e
          warn "[observateur] #{e.class}: #{e.message}"
        end
      end
      nil
    end

    def observers; @observers ||= Hash.new { |h, k| h[k] = [] }; end
    # Session settings of the Sketchup module (the saved ones go through read/write_default).
    def prefs; @prefs ||= { fix_shadow_strings: false }; end
    def importers; @importers ||= []; end
    def procedures; @procedures ||= []; end

    # %APPDATA%/Esquisse : le dossier qui contient Plugins.
    def support_dir
      @support_dir ||= File.dirname(call('misc.paths')['plugins'].tr('\\', '/'))
    end

    # Sketchup.write_default : ESQ_DEFAULTS_FILE permet aux tests de ne pas toucher au vrai fichier.
    def defaults_file
      ENV['ESQ_DEFAULTS_FILE'] || File.join(support_dir, 'defaults.json')
    end

    def defaults
      @defaults_loaded ||= begin
        f = defaults_file
        (@defaults.merge!(JSON.parse(File.read(f))) { |_k, mine, _| mine }) if f && File.file?(f)
        true
      rescue StandardError
        true
      end
      @defaults
    end

    def save_defaults
      f = defaults_file or return
      Dir.mkdir(File.dirname(f)) unless File.directory?(File.dirname(f))
      File.write(f, JSON.generate(@defaults))
    rescue StandardError
      nil
    end
  end
end

SB_PROMPT = 0
SB_VCB_LABEL = 1
SB_VCB_VALUE = 2
# Identifiants de commandes acceptés par Sketchup.send_action (valeurs de SketchUp.exe).
SU_ID_FILE_NEW = 0x1c201
SU_ID_FILE_OPEN = 0x1c203
SU_ID_FILE_SAVE = 0x1c207
SU_ID_FILE_SAVE_COPY = 0xa521
SU_ID_FILE_SAVEASTEMPLATE = 0xbcef
SU_ID_FILE_REVERT = 0xa7db
SU_ID_FILE_SENDTOLAYOUT = 0xb417
SU_ID_IMPORT = 0xab5b
SU_ID_FILE_PRINT = 0x1c20f

module Sketchup
  VERSION = '26.0.0'

  ACTION_IDS = {
    SU_ID_FILE_NEW => 'fileNew:', SU_ID_FILE_OPEN => 'fileOpen:', SU_ID_FILE_SAVE => 'fileSave:',
    SU_ID_FILE_SAVE_COPY => 'fileSaveCopy:', SU_ID_FILE_SAVEASTEMPLATE => 'fileSaveAsTemplate:',
    SU_ID_FILE_REVERT => 'fileRevert:', SU_ID_FILE_SENDTOLAYOUT => 'fileSendToLayout:',
    SU_ID_IMPORT => 'fileImport:', SU_ID_FILE_PRINT => 'filePrint:',
    # CMD_NEW, CMD_OPEN, CMD_SAVE, CMD_UNDO, CMD_REDO (the other CMD_* go to the application).
    0xe100 => 'fileNew:', 0xe101 => 'fileOpen:', 0xe103 => 'fileSave:', 0xe12b => 'editUndo:', 0xe12c => 'editRedo:'
  }.freeze

  # Actions (« send_action ») reconnues et leur nom affiché.
  ACTION_NAMES = {
    'selectSelectionTool:' => 'Select', 'selectEraseTool:' => 'Eraser', 'selectPaintTool:' => 'Paint Bucket',
    'selectLineTool:' => 'Line', 'selectRectangleTool:' => 'Rectangle', 'selectCircleTool:' => 'Circle',
    'selectArcTool:' => 'Arc', 'selectPolygonTool:' => 'Polygon', 'selectMoveTool:' => 'Move',
    'selectRotateTool:' => 'Rotate', 'selectScaleTool:' => 'Scale', 'selectPushPullTool:' => 'Push/Pull',
    'selectOffsetTool:' => 'Offset', 'selectFollowMeTool:' => 'Follow Me', 'selectTapeMeasureTool:' => 'Tape Measure',
    'selectProtractorTool:' => 'Protractor', 'selectAxisTool:' => 'Axes', 'selectTextTool:' => 'Text',
    'selectDimensionTool:' => 'Dimension', 'selectSectionPlaneTool:' => 'Section Plane',
    'selectOrbitTool:' => 'Orbit', 'selectPanTool:' => 'Pan', 'selectZoomTool:' => 'Zoom',
    'selectZoomWindowTool:' => 'Zoom Window', 'viewZoomExtents:' => 'Zoom Extents',
    'editUndo:' => 'Undo', 'editRedo:' => 'Redo', 'editCut:' => 'Cut', 'editCopy:' => 'Copy',
    'editPaste:' => 'Paste', 'editDelete:' => 'Delete', 'selectAll:' => 'Select All',
    'selectNone:' => 'Select None', 'viewTop:' => 'Top', 'viewBottom:' => 'Bottom', 'viewFront:' => 'Front',
    'viewBack:' => 'Back', 'viewLeft:' => 'Left', 'viewRight:' => 'Right', 'viewIso:' => 'Iso',
    'viewPerspective:' => 'Perspective', 'viewShowAxes:' => 'Axes', 'viewShadows:' => 'Shadows',
    'fileSave:' => 'Save', 'fileOpen:' => 'Open...', 'fileNew:' => 'New', 'fileSaveAs:' => 'Save As...',
    'fileClose:' => 'Close'
  }.freeze

  class << self
    # ---- version, plate-forme
    def version; '26.0.0'; end
    def version_number(*); 26_000_000; end
    def app_name; 'SketchUp Pro'; end
    def platform; :platform_win; end
    def is_64bit?; true; end
    def is_pro?; true; end
    # Vrai seulement si l'interrupteur de fonction « MDI » est actif : comme
    # SketchUp pour Windows, Esquisse n'a qu'un modèle par fenêtre. TestUp
    # (`discard_all_models`) ferme les modèles jusqu'à n'en plus avoir quand c'est vrai.
    def mdi?; false; end
    def os_language; get_locale; end
    def get_locale; 'fr-FR'; end
    def device_id
      require 'digest'
      Digest::SHA256.hexdigest("#{ENV['COMPUTERNAME']}#{ENV['USERNAME']}")[0, 32]
    end
    def has_feature?(name); name.to_s == 'ExtensionManager' ? true : false; end
    def signed_in?; false; end
    def sign_in(*_a); nil; end

    # ---- modèle actif
    def active_model; @model ||= Model.new; end
    def add_observer(obs); Esq.observers[:app] << obs unless Esq.observers[:app].include?(obs); true; end
    def remove_observer(obs); !Esq.observers[:app].delete(obs).nil?; end
    def file_new
      Esq.call('host.model.new')
      Esq.emit(Esq.observers[:app], :onNewModel, active_model)
      true
    end
    # Comme SketchUp (rb_scan_args « 1: ») : un chemin, puis des mots-clés
    # (with_status:, show_version_warning_dialog:). Sans with_status: vrai, l'ancienne forme
    # avertit sur $stderr et rend true/false ; avec, le code LOAD_STATUS_* ou false.
    def open_file(*args)
      kw = args.size > 1 && args.last.is_a?(Hash) ? args.pop : {}
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 1)" unless args.size == 1
      with_status = kw[:with_status] ? true : false
      warn 'Deprecated overload, use with_status: true overload instead', uplevel: 1 unless with_status
      path = File.expand_path(Esq.str_arg(args[0])) # StringValue
      status = File.file?(path) ? Esq.host('model.open', { 'path' => path }, { 'status' => -1 })['status'] : -1
      ok = status == Model::LOAD_STATUS_SUCCESS || status == Model::LOAD_STATUS_SUCCESS_MORE_RECENT
      Esq.emit(Esq.observers[:app], ok ? :onOpenModel : :onFileOpenFailed, ok ? active_model : path)
      return ok unless with_status
      ok ? status : false
    end
    def undo; Esq.host('model.undo', {}, false) ? true : false; end
    def redo; Esq.host('model.redo', {}, false) ? true : false; end
    def quit; Esq.host('model.quit'); nil; end
    def focus; nil; end
    def send_to_layout(_path); false; end
    # Les arguments de SketchUp : le modèle, la largeur et la hauteur (entiers,
    # ou nombres avec logical_pixels: true). Esquisse ne laisse pas une extension redimensionner
    # sa fenêtre : false, la réponse de SketchUp quand le redimensionnement échoue.
    def resize_viewport(*args)
      kw = args.size > 3 && args.last.is_a?(Hash) ? args.pop : {}
      raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 3)" unless args.size == 3
      model, w, h = args
      raise TypeError, 'wrong argument type (expected Sketchup::Model)' unless model.is_a?(Model)
      conv = kw[:logical_pixels] ? :num2dbl : :num2int
      Esq.send(conv, w)
      Esq.send(conv, h)
      false
    end
    def save_thumbnail(skp, image)
      Esq.host('model.thumbnail', { 'skp' => skp.to_s, 'image' => image.to_s }, false) ? true : false
    end

    # ---- barre d'état
    # SB_PROMPT : la barre d'état de l'application ; SB_VCB_LABEL / SB_VCB_VALUE : la boîte
    # de mesures (tenue par l'outil Ruby actif, zone « tools »).
    def set_status_text(*args)
      text, position = args[0].to_s, (args[1] || SB_PROMPT)
      case position
      when SB_VCB_LABEL then self.vcb_label = text
      when SB_VCB_VALUE then self.vcb_value = text
      else
        r = Esq.host('model.status', { 'text' => text }, :none)
        $stdout.puts("[statut] #{text}") if r == :none
      end
      nil
    end
    def status_text=(t); set_status_text(t, 0); t; end
    def vcb_label=(t); Esq.host('ui.vcb', { 'label' => t.to_s }, nil); t; end
    def vcb_value=(t); Esq.host('ui.vcb', { 'value' => t.to_s }, nil); t; end

    # ---- préférences enregistrées
    def read_default(*args)
      raise ArgumentError, 'Too few arguments - at least 2 required' if args.size < 2
      section, key, default = args
      v = Esq.defaults.fetch("#{section}/#{key}", nil)
      v.nil? ? default : v
    end
    def write_default(section, key, value)
      return false if section.to_s.empty? || key.to_s.empty?
      value = value.to_a if value.is_a?(Sketchup::Color) # les couleurs se rangent en tableau
      Esq.defaults["#{section}/#{key}"] = value
      Esq.save_defaults
      true
    end

    # Break edges: SketchUp's Preferences/IntersectEdges, read by the geometry
    # engine; new edges split those they cross only when it is on. Saved with the preferences.
    def break_edges?; Esq.call('meta.break_edges') ? true : false; end
    def break_edges=(v)
      on = v ? true : false
      Esq.call('meta.break_edges', 'on' => on)
      Esq.host('model.intersect_edges', { 'on' => on }, nil)
      on
    end

    # A switch of SketchUp's renderer (a workaround for shadow artefacts), for the session only:
    # nothing to fix in Esquisse's renderer.
    def fix_shadow_strings?; Esq.prefs[:fix_shadow_strings] ? true : false; end
    def fix_shadow_strings=(v); Esq.prefs[:fix_shadow_strings] = v ? true : false; end

    # PREFS/RubyManager_DebugMode, saved.
    def debug_mode?
      Esq.prefs[:debug_mode] = read_default('PREFS', 'RubyManager_DebugMode', false) ? true : false if Esq.prefs[:debug_mode].nil?
      Esq.prefs[:debug_mode]
    end
    def debug_mode=(v)
      on = v ? true : false
      if on != debug_mode?
        Esq.prefs[:debug_mode] = on
        write_default('PREFS', 'RubyManager_DebugMode', on)
      end
      on
    end

    # Level of detail of SketchUp's renderer (LODPreferences/*), saved with SketchUp's defaults;
    # every setter answers the value it replaced.
    # Esquisse's renderer draws everything at full detail: they change nothing on screen.
    LOD_PREFS = { use_lod: ['LODEnabled', true], max_draw_time: ['MaxDrawTime', 0.1667],
                  required_tool_lod: ['RequiredToolLOD', 3], required_camera_lod: ['RequiredCameraLOD', 1],
                  full_detail_render_delay_min: ['FullDetailRenderDelayMin', 1.0],
                  full_detail_render_delay_max: ['FullDetailRenderDelayMax', 5.0] }.freeze
    def __lod(k)
      key, default = LOD_PREFS[k]
      v = read_default('LODPreferences', key, default)
      default.is_a?(Float) ? v.to_f : (default == true ? (v ? true : false) : v.to_i)
    end
    def __lod_set(k, v)
      old = __lod(k)
      write_default('LODPreferences', LOD_PREFS[k][0], v)
      old
    end
    private :__lod, :__lod_set
    def use_lod?; __lod(:use_lod); end
    def use_lod=(v); __lod_set(:use_lod, v ? true : false); end
    %i[max_draw_time full_detail_render_delay_min full_detail_render_delay_max].each do |k|
      define_method(k) { __lod(k) }
      define_method("#{k}=") { |v| __lod_set(k, Esq::Conv.num2dbl(v)) }
    end
    %i[required_tool_lod required_camera_lod].each do |k|
      define_method(k) { __lod(k) }
      define_method("#{k}=") { |v| __lod_set(k, Esq::Conv.num2long(v)) }
    end

    # ---- fichiers de support (find_support_file, extensions, require/load : zone « misc »)
    # File › New's template, kept by the application.
    def template_dir
      (Esq.host('model.template', {}, nil) || {})['dir'] || File.join(Esq.support_dir, 'Templates')
    end
    def template
      r = Esq.host('model.template', {}, nil)
      r ? r['path'] : (Esq.prefs[:template] || File.join(template_dir, 'Simple.skp'))
    end
    # An empty path brings the default template back; a missing file is refused (false).
    def template=(path)
      path = String.new(path) unless path.is_a?(String)
      return false unless path.empty? || File.exist?(path)
      r = Esq.host('model.set_template', { 'path' => path }, :none)
      Esq.prefs[:template] = (path.empty? ? nil : path) if r == :none
      r == :none ? true : r
    end
    def get_datfile_info(_a, _b); nil; end
    def get_i18n_datfile_info(_a, _b); nil; end
    def temp_dir; Dir.tmpdir.tr('\\', '/'); end
    def is_valid_filename?(name)
      s = name.to_s
      !s.empty? && s !~ /[\\\/:*?"<>|\x00-\x1f]/ && s !~ /[ .]\z/ && s !~ /\A(con|prn|aux|nul|com\d|lpt\d)(\..*)?\z/i
    end
    def register_importer(imp)
      unless %i[load_file description file_extension].all? { |m| imp.respond_to?(m) }
        raise ArgumentError, 'An extension must respond to "load_file", "description", and "file_extension"'
      end
      Esq.importers << imp unless Esq.importers.include?(imp)
      true
    end
    def register_procedure(proc_obj); Esq.procedures << proc_obj; true; end
    def file_loaded?(f); Esq.loaded_flags[f.to_s.tr('\\', '/')] ? true : false; end
    def file_loaded(f); Esq.loaded_flags[f.to_s.tr('\\', '/')] = true; end

    # ---- actions, raccourcis
    def send_action(action)
      unless $VERBOSE.nil?
        warn 'Sketchup.send_action is deprecated and not being maintained. If you are relying on this method, please log a request in our Issue Tracker.', uplevel: 1
      end
      a = ACTION_IDS[action] || action.to_s
      m = active_model
      case a
      when 'editUndo:' then self.undo
      when 'editRedo:' then self.redo
      when 'selectAll:', 'editSelectAll:' then m.selection.add(m.active_entities.to_a)
      when 'selectNone:', 'editSelectNone:' then m.selection.clear
      when 'editDelete:' then m.active_entities.erase_entities(m.selection.to_a) unless m.selection.empty?
      when 'fileNew:' then file_new
      when 'fileSave:' then m.path.empty? ? (return false) : m.save
      when 'viewZoomExtents:' then m.active_view.zoom_extents
      else return Esq.host('model.action', { 'action' => a }, false) ? true : false
      end
      true
    end
    def display_name_from_action(action); ACTION_NAMES[action.to_s] || ''; end
    def get_shortcuts
      Esq.host('model.shortcuts', {}, []).map { |k, v| "#{k}\t#{v}" }
    end
    def active_model_changed; end

    # ---- formats de longueur, d'aire, d'angle (selon UnitsOptions du modèle). Les longueurs
    # passent par le formateur de la zone geom (Esq::Units, 10_geom.rb).
    # Les valeurs passent par rb_num2dbl dans SketchUp : nil ou
    # une chaîne lèvent TypeError.
    def format_length(*args)
      raise ArgumentError, 'Too few arguments: length required' if args.empty?
      v = Esq.num2dbl(args[0])
      return Esq::Units.format_length(v) if args.size < 2
      # Une précision donnée : sa valeur absolue, 14 au plus. Positive, le « ~ » est ôté (pas
      # l'espace qui le suit) ; négative, les zéros de fin aussi.
      p = Esq.num2int(args[1])
      prec = [p.abs, 14].min
      if p >= 0
        s = Esq::Units.format_length(v, nil, precision: prec)
        s.start_with?('~') ? s[1..] : s
      else
        Esq::Units.format_length(v, nil, precision: prec, strip: true)
      end
    end
    def format_area(area); Esq::ModelUnits.value(Esq.num2dbl(area), :area); end
    def format_volume(vol); Esq::ModelUnits.value(Esq.num2dbl(vol), :volume); end
    def format_angle(rad); Esq::ModelUnits.angle(Esq.num2dbl(rad) * 180.0 / Math::PI); end
    def format_degrees(deg); Esq::ModelUnits.angle(Esq.num2dbl(deg)); end
    def parse_length(text)
      return text.to_f if text.is_a?(Numeric)
      Esq::Units.parse_length(text.to_s)
    end

    def create_texture_writer; TextureWriter.send(:new); end
  end
end


# ---------------------------------------------------------------------------- unités
module Esq
  # Aires, volumes et angles au format du modèle (UnitsOptions : AreaUnit, VolumeUnit,
  # LengthPrecision, AnglePrecision) ; « ~ » quand l'arrondi change la valeur, comme SketchUp.
  module ModelUnits
    SQ = "²"
    CU = "³"
    # Codes SketchUp -> [suffixe, pouces² ou pouces³ par unité]
    AREA = { 0 => ["in#{SQ}", 1.0], 1 => ["ft#{SQ}", 144.0], 2 => ["mm#{SQ}", 1 / 645.16], 3 => ["cm#{SQ}", 1 / 6.4516],
             4 => ["m#{SQ}", 1 / 0.00064516], 5 => ["yd#{SQ}", 1296.0] }.freeze
    VOLUME = { 0 => ["in#{CU}", 1.0], 1 => ["ft#{CU}", 1728.0], 2 => ["mm#{CU}", 1 / 16_387.064],
               3 => ["cm#{CU}", 1 / 16.387064], 4 => ["m#{CU}", 1 / 0.000016387064], 5 => ["yd#{CU}", 46_656.0],
               6 => ['L', 1 / 0.016387064], 7 => ['gal', 231.0] }.freeze

    module_function

    def opts; Sketchup.active_model.options['UnitsOptions']; end

    def num(v, prec)
      s = Kernel.format("%.#{[prec, 0].max}f", v)
      s = s.sub(/\A-(?=0(\.0*)?\z)/, '')
      approx = (v - s.to_f).abs > 1e-9 * [1.0, v.abs].max
      s = s.tr('.', Esq::Units.decimal_separator)
      approx ? "~ #{s}" : s
    end

    # Comme SketchUp (pour les aires et les volumes) : jamais de
    # « ~ », zéros de fin et séparateur final retirés. L'unité est celle d'AreaUnit/VolumeUnit en
    # décimal ; en architectural des pieds dès 1 ft² (1 ft³), sinon des pouces ; des pieds en
    # ingénierie, des pouces en fractionnaire.
    def value(v, kind)
      o = opts
      table, key = kind == :area ? [AREA, 'AreaUnit'] : [VOLUME, 'VolumeUnit']
      unit = case o['LengthFormat'].to_i
             when 0 then table.key?(o[key]) ? o[key] : 4
             when 1 then v >= table[1][1] ? 1 : 0
             when 2 then 1
             else 0
             end
      suffix, per = table[unit]
      # Model Info › Units has its own precision for areas and volumes (else the lengths').
      prec = o[kind == :area ? 'AreaPrecision' : 'VolumePrecision'] || o['LengthPrecision']
      s = Kernel.format("%.#{[prec.to_i, 0].max}f", v / per)
      s = s.sub(/0+\z/, '').chomp('.') if s.include?('.')
      "#{s.tr('.', Esq::Units.decimal_separator)} #{suffix}"
    end

    def angle(deg); num(deg, opts['AnglePrecision'].to_i); end
  end
end

module Sketchup
  # Séparateurs des réglages régionaux de Windows (GetLocaleInfoEx).
  module RegionalSettings
    def self.__seps; @seps ||= Esq.call('meta.regional'); end
    def self.decimal_separator; __seps['decimal']; end
    def self.list_separator; __seps['list']; end
  end
end

# ---------------------------------------------------------------------------- Color
module Sketchup
  class Color
    # Noms SVG/CSS, dans l'ordre alphabétique (SUColorGetNames).
    NAME_TABLE = {
      'AliceBlue' => [240, 248, 255], 'AntiqueWhite' => [250, 235, 215], 'Aqua' => [0, 255, 255],
      'Aquamarine' => [127, 255, 212], 'Azure' => [240, 255, 255], 'Beige' => [245, 245, 220],
      'Bisque' => [255, 228, 196], 'Black' => [0, 0, 0], 'BlanchedAlmond' => [255, 235, 205],
      'Blue' => [0, 0, 255], 'BlueViolet' => [138, 43, 226], 'Brown' => [165, 42, 42],
      'BurlyWood' => [222, 184, 135], 'CadetBlue' => [95, 158, 160], 'Chartreuse' => [127, 255, 0],
      'Chocolate' => [210, 105, 30], 'Coral' => [255, 127, 80], 'CornflowerBlue' => [100, 149, 237],
      'Cornsilk' => [255, 248, 220], 'Crimson' => [220, 20, 60], 'Cyan' => [0, 255, 255],
      'DarkBlue' => [0, 0, 139], 'DarkCyan' => [0, 139, 139], 'DarkGoldenrod' => [184, 134, 11],
      'DarkGray' => [169, 169, 169], 'DarkGreen' => [0, 100, 0], 'DarkGrey' => [169, 169, 169],
      'DarkKhaki' => [189, 183, 107], 'DarkMagenta' => [139, 0, 139], 'DarkOliveGreen' => [85, 107, 47],
      'DarkOrange' => [255, 140, 0], 'DarkOrchid' => [153, 50, 204], 'DarkRed' => [139, 0, 0],
      'DarkSalmon' => [233, 150, 122], 'DarkSeaGreen' => [143, 188, 143], 'DarkSlateBlue' => [72, 61, 139],
      'DarkSlateGray' => [47, 79, 79], 'DarkSlateGrey' => [47, 79, 79], 'DarkTurquoise' => [0, 206, 209],
      'DarkViolet' => [148, 0, 211], 'DeepPink' => [255, 20, 147], 'DeepSkyBlue' => [0, 191, 255],
      'DimGray' => [105, 105, 105], 'DimGrey' => [105, 105, 105], 'DodgerBlue' => [30, 144, 255],
      'FireBrick' => [178, 34, 34], 'FloralWhite' => [255, 250, 240], 'ForestGreen' => [34, 139, 34],
      'Fuchsia' => [255, 0, 255], 'Gainsboro' => [220, 220, 220], 'GhostWhite' => [248, 248, 255],
      'Gold' => [255, 215, 0], 'Goldenrod' => [218, 165, 32], 'Gray' => [128, 128, 128],
      'Green' => [0, 128, 0], 'GreenYellow' => [173, 255, 47], 'Grey' => [128, 128, 128],
      'Honeydew' => [240, 255, 240], 'HotPink' => [255, 105, 180], 'IndianRed' => [205, 92, 92],
      'Indigo' => [75, 0, 130], 'Ivory' => [255, 255, 240], 'Khaki' => [240, 230, 140],
      'Lavender' => [230, 230, 250], 'LavenderBlush' => [255, 240, 245], 'LawnGreen' => [124, 252, 0],
      'LemonChiffon' => [255, 250, 205], 'LightBlue' => [173, 216, 230], 'LightCoral' => [240, 128, 128],
      'LightCyan' => [224, 255, 255], 'LightGoldenrodYellow' => [250, 250, 210], 'LightGray' => [211, 211, 211],
      'LightGreen' => [144, 238, 144], 'LightGrey' => [211, 211, 211], 'LightPink' => [255, 182, 193],
      'LightSalmon' => [255, 160, 122], 'LightSeaGreen' => [32, 178, 170], 'LightSkyBlue' => [135, 206, 250],
      'LightSlateGray' => [119, 136, 153], 'LightSlateGrey' => [119, 136, 153], 'LightSteelBlue' => [176, 196, 222],
      'LightYellow' => [255, 255, 224], 'Lime' => [0, 255, 0], 'LimeGreen' => [50, 205, 50],
      'Linen' => [250, 240, 230], 'Magenta' => [255, 0, 255], 'Maroon' => [128, 0, 0],
      'MediumAquamarine' => [102, 205, 170], 'MediumBlue' => [0, 0, 205], 'MediumOrchid' => [186, 85, 211],
      'MediumPurple' => [147, 112, 219], 'MediumSeaGreen' => [60, 179, 113], 'MediumSlateBlue' => [123, 104, 238],
      'MediumSpringGreen' => [0, 250, 154], 'MediumTurquoise' => [72, 209, 204], 'MediumVioletRed' => [199, 21, 133],
      'MidnightBlue' => [25, 25, 112], 'MintCream' => [245, 255, 250], 'MistyRose' => [255, 228, 225],
      'Moccasin' => [255, 228, 181], 'NavajoWhite' => [255, 222, 173], 'Navy' => [0, 0, 128],
      'OldLace' => [253, 245, 230], 'Olive' => [128, 128, 0], 'OliveDrab' => [107, 142, 35],
      'Orange' => [255, 165, 0], 'OrangeRed' => [255, 69, 0], 'Orchid' => [218, 112, 214],
      'PaleGoldenrod' => [238, 232, 170], 'PaleGreen' => [152, 251, 152], 'PaleTurquoise' => [175, 238, 238],
      'PaleVioletRed' => [219, 112, 147], 'PapayaWhip' => [255, 239, 213], 'PeachPuff' => [255, 218, 185],
      'Peru' => [205, 133, 63], 'Pink' => [255, 192, 203], 'Plum' => [221, 160, 221],
      'PowderBlue' => [176, 224, 230], 'Purple' => [128, 0, 128], 'Red' => [255, 0, 0],
      'RosyBrown' => [188, 143, 143], 'RoyalBlue' => [65, 105, 225], 'SaddleBrown' => [139, 69, 19],
      'Salmon' => [250, 128, 114], 'SandyBrown' => [244, 164, 96], 'SeaGreen' => [46, 139, 87],
      'Seashell' => [255, 245, 238], 'Sienna' => [160, 82, 45], 'Silver' => [192, 192, 192],
      'SkyBlue' => [135, 206, 235], 'SlateBlue' => [106, 90, 205], 'SlateGray' => [112, 128, 144],
      'SlateGrey' => [112, 128, 144], 'Snow' => [255, 250, 250], 'SpringGreen' => [0, 255, 127],
      'SteelBlue' => [70, 130, 180], 'Tan' => [210, 180, 140], 'Teal' => [0, 128, 128],
      'Thistle' => [216, 191, 216], 'Tomato' => [255, 99, 71], 'Turquoise' => [64, 224, 208],
      'Violet' => [238, 130, 238], 'Wheat' => [245, 222, 179], 'White' => [255, 255, 255],
      'WhiteSmoke' => [245, 245, 245], 'Yellow' => [255, 255, 0], 'YellowGreen' => [154, 205, 50]
    }.freeze
    LOOKUP = NAME_TABLE.each_with_object({}) { |(k, v), h| h[k.downcase] = v }.freeze
    NAMES = NAME_TABLE.keys.freeze

    def self.names; NAMES.dup; end

    def self.__component(v)
      if v.is_a?(Float)
        return 0 unless v > 0.0
        return 255 if v >= 1.0
        (v * 255).to_i
      else
        i = Integer(v)
        i.clamp(0, 255)
      end
    end

    def initialize(*args)
      @red, @green, @blue, @alpha = 0, 0, 0, 255
      case args.size
      when 0
        nil
      when 1
        a = args[0]
        case a
        when Array
          raise ArgumentError, 'At least 3 values required' if a.size < 3
          set_components(a)
        when Integer
          @red = a & 0xff
          @green = (a >> 8) & 0xff
          @blue = (a >> 16) & 0xff
        when Sketchup::Color
          @red, @green, @blue, @alpha = a.to_a
        when String
          rgb = LOOKUP[a.downcase.delete(' ')] || (a =~ /\A#?(\h{2})(\h{2})(\h{2})\z/ && [$1.hex, $2.hex, $3.hex])
          raise ArgumentError, "Cannot find color named #{a}" unless rgb
          @red, @green, @blue = rgb
        else
          raise TypeError, 'no implicit conversion to Color'
        end
      else
        raise ArgumentError, 'wrong number of arguments' if args.size > 4
        set_components(args)
      end
    end

    def red; @red; end
    def green; @green; end
    def blue; @blue; end
    def alpha; @alpha; end
    def red=(v); @red = check_byte(v); end
    def green=(v); @green = check_byte(v); end
    def blue=(v); @blue = check_byte(v); end
    def alpha=(v)
      @alpha =
        if v.is_a?(Float)
          raise ArgumentError, 'Value must be between 0.0 and 1.0, or 0 and 255' if v < 0.0 || v > 1.0
          v <= 0.0 ? 0 : (v >= 1.0 ? 255 : (v * 255).to_i)
        else
          i = Integer(v)
          raise ArgumentError, 'Value must be between 0.0 and 1.0, or 0 and 255' if i < 0 || i > 255
          i
        end
      v
    end
    def to_a; [@red, @green, @blue, @alpha]; end
    def to_i; @red | (@green << 8) | (@blue << 16); end
    def to_s; "Color(#{@red}, #{@green}, #{@blue}, #{@alpha})"; end
    alias inspect to_s
    def ==(other); other.is_a?(Sketchup::Color) && to_a == other.to_a; end
    alias eql? ==
    def hash; to_a.hash; end

    # Mélange pondéré (SUColorBlend) : `weight` est le poids de self, 1 - weight celui de `other`.
    def blend(other, weight)
      raise ArgumentError, 'wrong type - expected Sketchup::Color' unless other.is_a?(Sketchup::Color)
      w = weight.to_f
      mix = ->(a, b) { (a * w + b * (1 - w)).round.clamp(0, 255) }
      Color.new(mix.(@red, other.red), mix.(@green, other.green), mix.(@blue, other.blue), mix.(@alpha, other.alpha))
    end

    private

    def check_byte(v)
      i = Integer(v)
      raise ArgumentError, 'Value must be between 0..255' if i < 0 || i > 255
      i
    end

    def set_components(a)
      vals = a.first(4).map { |v| Color.__component(v) }
      @red, @green, @blue = vals
      @alpha = vals[3] if vals.size > 3
    end
  end
end
