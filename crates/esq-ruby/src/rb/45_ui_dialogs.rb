# Esquisse — boîtes de dialogue de l'API UI : HtmlDialog, WebDialog, InputBox, Notification,
# et la distribution des événements (rappels JavaScript, minuteries, fermetures) que l'application
# remet au Ruby (`Esq.ui_pump`).
#
# Les fenêtres web sont de vraies fenêtres WebView2 ouvertes par l'application (`host.ui.html_*`).

module Esq
  class << self
    # Distribue un événement de l'hôte. Une exception dans un rappel ne bloque pas les autres.
    def dispatch_ui_event(ev)
      case ev['t']
      when 'cb' then (d = @dialogs[ev['d']]) && d.__callback(ev)
      when 'close_request' then (d = @dialogs[ev['d']]) && d.__close_request
      when 'closed' then (d = @dialogs.delete(ev['d'])) && d.__closed
      when 'timer' then fire_timer(ev['id'])
      when 'notif' then (n = @notifs[ev['id']]) && n.__action(ev['action'])
      end
    rescue Exception => e # rubocop:disable Lint/RescueException
      warn "[ui] #{e.class}: #{e.message}"
    end

    def fire_timer(id)
      t = @timers[id] or return
      @timers.delete(id) unless t[:repeat]
      begin
        t[:proc].call
      rescue Exception => e # rubocop:disable Lint/RescueException
        warn "[minuterie] #{e.class}: #{e.message}"
      end
    end

    # Appelé par l'application à chaque image quand des événements attendent.
    def ui_pump
      evs = ui_host('poll', {}, []) || []
      evs.each { |ev| dispatch_ui_event(ev) }
      evs.size
    end

    # Attente (fenêtre modale) : traite les événements jusqu'à ce que le bloc dise stop.
    def modal_loop(dialog)
      until dialog.__closed?
        evs = ui_host('html_wait', { id: dialog.__id, ms: 100 }, nil)
        break if evs.nil? # pas d'application
        evs.each { |ev| dispatch_ui_event(ev) }
      end
    end

    def h(s)
      s.to_s.gsub('&', '&amp;').gsub('<', '&lt;').gsub('>', '&gt;').gsub('"', '&quot;').gsub("'", '&#39;')
    end
  end
end

module UI
  # Contexte passé aux rappels d'un HtmlDialog (`sketchup.nom(...)` côté JavaScript).
  class ActionCallbackContext
    def initialize(dialog, promise = nil)
      @dialog = dialog
      @promise = promise
      @settled = false
    end

    def progress(value)
      @dialog.execute_script(
        "window.dispatchEvent(new CustomEvent('sketchup-progress',{detail:{id:#{@promise.to_i},value:#{JSON.generate([value])[1..-2]}}}))"
      )
      nil
    end

    def resolve(value = nil); __settle(true, value); end
    def reject(value = nil); __settle(false, value); end

    def __settled?; @settled; end

    def __settle(ok, value)
      return nil if @settled || @promise.nil?
      @settled = true
      @dialog.execute_script("window.__esq_settle(#{@promise.to_i},#{ok},#{JSON.generate([value])[1..-2]})")
      nil
    end
  end

  class HtmlDialog
    STYLE_WINDOW = 1
    STYLE_DIALOG = 0
    STYLE_UTILITY = 2
    CEF_VERSION = '155.0.4283.45 (WebView2)'
    CHROME_VERSION = '155.0.4283.45'

    DEFAULTS = { dialog_title: '', preferences_key: '', scrollable: true, resizable: true,
                 use_content_size: false, width: 250, height: 250, left: nil, top: nil,
                 min_width: -1, min_height: -1, max_width: -1, max_height: -1, style: 0,
                 sketchup_js_api: false }.freeze

    def initialize(props = nil)
      unless props.nil?
        Esq.unexpected_type(props, 'Hash') unless props.is_a?(Hash)
      end
      @opts = DEFAULTS.dup
      (props || {}).each do |k, v|
        next unless DEFAULTS.key?(k)
        @opts[k] = case DEFAULTS[k]
                   when true, false then v ? true : false
                   when Integer then Esq.num2int(v)
                   when String then v.to_s
                   else v.nil? ? nil : Esq.num2int(v)
                   end
      end
      @id = Esq.next_dialog_id
      @callbacks = {}
      @content = nil
      @can_close = nil
      @on_closed = nil
      @closed = true
      @opened = false
      @geo = nil
    end

    def __id; @id; end
    def __closed?; @closed; end

    def add_action_callback(name, &blk)
      raise ArgumentError, 'Method requires block argument for callback' unless blk
      @callbacks[name.to_s] = blk
      true
    end

    def execute_script(script)
      Esq.ui_host('html_exec', { id: @id, js: script.to_s }) if @opened
      nil
    end

    def set_can_close(&blk)
      @can_close = blk
      Esq.ui_host('html_hook', { id: @id, can_close: !blk.nil? }) if @opened
      nil
    end

    def set_on_closed(&blk)
      @on_closed = blk
      nil
    end

    def set_file(path); __load(:file, path.to_s); end
    def set_html(html); __load(:html, html.to_s); end
    def set_url(url); __load(:url, url.to_s); end

    def show; __open(false); nil; end

    def show_modal
      __open(true)
      Esq.modal_loop(self)
      nil
    end

    def close
      Esq.ui_host('html_close', { id: @id }) if @opened
      nil
    end

    def hide
      Esq.ui_host('html_hide', { id: @id }) if @opened
      nil
    end

    def visible?
      return false unless @opened
      g = Esq.ui_host('html_geo', { id: @id }, nil)
      g && g['visible'] ? true : false
    end

    def bring_to_front
      Esq.ui_host('html_front', { id: @id }) if @opened
      nil
    end

    def center
      Esq.ui_host('html_center', { id: @id }) if @opened
      @opts[:left] = @opts[:top] = nil unless @opened
      nil
    end

    def get_position
      g = __geo
      [g['x'], g['y']]
    end

    def get_size
      g = __geo
      [g['w'], g['h']]
    end

    def get_content_size
      g = __geo
      [g['cw'], g['ch']]
    end

    def set_position(x, y)
      x = Esq.num2int(x)
      y = Esq.num2int(y)
      @opts[:left] = x
      @opts[:top] = y
      Esq.ui_host('html_pos', { id: @id, x: x, y: y }) if @opened
      nil
    end

    def set_size(w, h)
      w = Esq.num2int(w)
      h = Esq.num2int(h)
      @opts[:width] = w
      @opts[:height] = h
      @opts[:use_content_size] = false
      Esq.ui_host('html_size', { id: @id, w: w, h: h, content: false }) if @opened
      nil
    end

    def set_content_size(w, h)
      w = Esq.num2int(w)
      h = Esq.num2int(h)
      @opts[:width] = w
      @opts[:height] = h
      @opts[:use_content_size] = true
      Esq.ui_host('html_size', { id: @id, w: w, h: h, content: true }) if @opened
      nil
    end

    # --- interne -------------------------------------------------------------------------------
    def __geo
      g = @opened ? Esq.ui_host('html_geo', { id: @id }, nil) : nil
      g || { 'x' => @opts[:left] || 0, 'y' => @opts[:top] || 0, 'w' => @opts[:width], 'h' => @opts[:height],
             'cw' => @opts[:width], 'ch' => @opts[:height] }
    end

    def __load(kind, value)
      @content = [kind.to_s, value]
      Esq.ui_host('html_load', { id: @id, kind: kind.to_s, value: value }) if @opened && !@closed
      nil
    end

    def __open(modal)
      return Esq.ui_host('html_front', { id: @id }) if @opened && !@closed
      Esq.dialogs[@id] = self
      @closed = false
      @opened = true
      Esq.ui_host('html_open', { id: @id, opts: @opts, content: @content, modal: modal,
                              can_close_hook: !@can_close.nil?, kind: 'html' })
    end

    def __callback(ev)
      blk = @callbacks[ev['n']] or return
      ctx = ActionCallbackContext.new(self, ev['i'])
      args = ev['a'].is_a?(Array) ? ev['a'] : []
      args = [ev['p']] if ev.key?('p')
      blk.call(ctx, *args)
      ctx.resolve(nil) if ev['i']
    end

    def __close_request
      ok = @can_close ? (@can_close.call ? true : false) : true
      Esq.ui_host('html_close', { id: @id, force: true }) if ok
    end

    def __closed
      @closed = true
      @on_closed&.call
    end
  end

  # Ancienne boîte web de SketchUp (basée sur le navigateur système) : mêmes fenêtres, autre API.
  class WebDialog
    WD_DEFAULTS = { dialog_title: '', preferences_key: '', scrollable: false, resizable: true,
                    use_content_size: false, width: 250, height: 250, left: 0, top: 0 }.freeze

    def initialize(*args)
      @opts = HtmlDialog::DEFAULTS.dup.merge(WD_DEFAULTS)
      if args.first.is_a?(Hash)
        args.first.each do |k, v|
          next unless WD_DEFAULTS.key?(k) || HtmlDialog::DEFAULTS.key?(k)
          @opts[k] = v
        end
      else
        keys = %i[dialog_title scrollable preferences_key width height left top resizable]
        args.first(8).each_with_index do |v, i|
          next if v.nil?
          @opts[keys[i]] = v
        end
      end
      @opts[:dialog_title] = @opts[:dialog_title].to_s
      @opts[:preferences_key] = @opts[:preferences_key].to_s
      %i[width height left top].each { |k| @opts[k] = Esq.num2int(@opts[k]) }
      @opts[:left] = @opts[:top] = nil if @opts[:left] == 0 && @opts[:top] == 0
      @opts[:min_width] = @opts[:min_height] = @opts[:max_width] = @opts[:max_height] = 0 unless args.first.is_a?(Hash)
      @id = Esq.next_dialog_id
      @callbacks = {}
      @content = nil
      @on_close = nil
      @closed = true
      @opened = false
      @nav_buttons = true
      @bg = nil
    end

    def __id; @id; end
    def __closed?; @closed; end

    %i[min_width min_height max_width max_height].each do |m|
      define_method(m) { @opts[m] }
      define_method(:"#{m}=") do |v|
        @opts[m] = Esq.num2int(v)
        Esq.ui_host('html_limits', { id: @id, opts: @opts }) if @opened
      end
    end

    def navigation_buttons_enabled?; @nav_buttons; end
    def navigation_buttons_enabled=(v); @nav_buttons = v ? true : false; end

    def add_action_callback(name, &blk)
      raise ArgumentError, 'Required proc not found' unless blk
      @callbacks[name.to_s] = blk
      nil
    end

    def allow_actions_from_host(_host); nil; end
    def set_full_security; nil; end
    def get_default_dialog_color; '#F0F0F0'; end

    def set_background_color(color)
      @bg = color.to_s
      Esq.ui_host('html_bg', { id: @id, color: @bg }) if @opened
      nil
    end

    def screen_scale_factor; Esq.ui_host('scale_factor', { window: nil }, 1.0).to_f; end

    def set_size(w, h)
      w = Esq.num2int(w)
      h = Esq.num2int(h)
      @opts[:width] = w
      @opts[:height] = h
      Esq.ui_host('html_size', { id: @id, w: w, h: h, content: false }) if @opened
      nil
    end

    def set_position(x, y)
      x = Esq.num2int(x)
      y = Esq.num2int(y)
      @opts[:left] = x
      @opts[:top] = y
      Esq.ui_host('html_pos', { id: @id, x: x, y: y }) if @opened
      nil
    end

    def set_url(url); __load(:url, url.to_s); end
    def set_html(html); __load(:html, html.to_s); end

    def set_file(path, _ignore = nil); __load(:file, path.to_s); end

    def post_url(url, data)
      form = "<form id='f' method='post' action='#{Esq.h(url)}'>" +
             data.to_s.split('&').map do |kv|
               k, v = kv.split('=', 2)
               "<input type='hidden' name='#{Esq.h(__urldecode(k.to_s))}' value='#{Esq.h(__urldecode(v.to_s))}'>"
             end.join + '</form>'
      __load(:html, "<html><body>#{form}<script>document.getElementById('f').submit()</script></body></html>")
      true
    end

    def show; __open(false); nil; end

    def show_modal
      __open(true)
      Esq.modal_loop(self)
      nil
    end

    def visible?
      return false unless @opened && !@closed
      g = Esq.ui_host('html_geo', { id: @id }, nil)
      g && g['visible'] ? true : false
    end

    def close
      Esq.ui_host('html_close', { id: @id }) if @opened && !@closed
      nil
    end

    def set_on_close(&blk)
      @on_close = blk
      nil
    end

    def bring_to_front
      Esq.ui_host('html_front', { id: @id }) if @opened
      nil
    end

    def execute_script(script)
      Esq.ui_host('html_exec', { id: @id, js: script.to_s }) if @opened
      nil
    end

    def get_element_value(name)
      js = "(function(){var e=document.getElementById(#{JSON.generate(name.to_s)});" \
           "if(!e){return ''}var v=e.value;return String(v===undefined?e.innerHTML:v)})()"
      r = @opened ? Esq.ui_host('html_eval', { id: @id, js: js }, nil) : nil
      r.nil? ? '' : r.to_s
    end

    # Capture de la fenêtre dans un fichier (PNG ou BMP selon l'extension).
    def write_image(*args)
      raise ArgumentError, 'File name required' if args.empty?
      return false unless @opened && !@closed
      Esq.ui_host('html_capture', { id: @id, path: args[0].to_s }, false) ? true : false
    end

    # --- interne -------------------------------------------------------------------------------
    def __urldecode(s)
      s.gsub('+', ' ').gsub(/%(\h\h)/) { $1.hex.chr }
    end
    private :__urldecode

    def __load(kind, value)
      @content = [kind.to_s, value]
      Esq.ui_host('html_load', { id: @id, kind: kind.to_s, value: value }) if @opened && !@closed
      nil
    end

    def __open(modal)
      return Esq.ui_host('html_front', { id: @id }) if @opened && !@closed
      Esq.dialogs[@id] = self
      @closed = false
      @opened = true
      Esq.ui_host('html_open', { id: @id, opts: @opts, content: @content, modal: modal,
                              can_close_hook: false, kind: 'web', bg: @bg })
    end

    def __callback(ev)
      blk = @callbacks[ev['n']] or return
      params = ev.key?('p') ? ev['p'].to_s : (ev['a'].is_a?(Array) && ev['a'].first.is_a?(String) ? ev['a'].first : '')
      blk.call(self, params)
    end

    def __close_request; Esq.ui_host('html_close', { id: @id, force: true }); end

    def __closed
      @closed = true
      @on_close&.call
    end
  end

  # ------------------------------------------------------------------------------------------
  # UI::InputBox — formulaire modal construit d'après la description des champs.
  class InputBox
    module ValueType
      STRING = 0
      INTEGER = 1
      FLOAT = 2
      LENGTH = 3
    end

    def initialize(*args, **kw)
      title = args.first.is_a?(String) ? args.first : nil
      opts = args.first.is_a?(Hash) ? args.first.merge(kw) : kw
      @title = (opts[:title] || title || '').to_s
      @accept = (opts[:accept_label] || 'OK').to_s
      @cancel = (opts[:cancel_label] || 'Cancel').to_s
      @fields = []
    end

    def clone; __dup_fields(super); end
    def dup; __dup_fields(super); end

    def add_checkbox(*args, **kw)
      f = __field(:check, args, kw)
      f[:value] = kw.key?(:value) ? (kw[:value] ? true : false) : (kw[:default] ? true : false)
      @fields << f
      nil
    end

    def add_description(text)
      @fields << { kind: :desc, label: text.to_s }
      nil
    end

    def add_dropdown(*args, **kw)
      f = __field(:select, args, kw)
      f[:options] = __options(args, kw)
      f[:value] = (kw[:value] || kw[:default] || f[:options].first).to_s
      __check_type(kw, f)
      @fields << f
      nil
    end

    def add_image(path)
      @fields << { kind: :image, label: path.to_s }
      nil
    end

    def add_listbox(*args, **kw)
      f = __field(:list, args, kw)
      f[:options] = __options(args, kw)
      f[:value] = (kw[:value] || kw[:default] || f[:options].first).to_s
      __check_type(kw, f)
      @fields << f
      nil
    end

    def add_separator(*_args, **_kw)
      @fields << { kind: :sep }
      nil
    end

    def add_slider(*args, **kw)
      f = __field(:slider, args, kw)
      range = kw[:range] || args[1]
      unless range.is_a?(Range) || range.nil?
        raise TypeError, "Invalid type for input field: #{range.class}"
      end
      range ||= (0..100)
      f[:range] = range
      f[:value] = kw[:value] || kw[:default] || range.begin
      f[:type] = (range.begin.is_a?(Float) || range.end.is_a?(Float) || f[:value].is_a?(Float)) ? ValueType::FLOAT : ValueType::INTEGER
      @fields << f
      nil
    end

    def add_textbox(*args, **kw)
      f = __field(:text, args, kw)
      v = kw.key?(:value) ? kw[:value] : (kw.key?(:default) ? kw[:default] : args[1])
      v = '' if v.nil?
      f[:value] = v
      f[:type] = kw[:type] || __type_of(v)
      @fields << f
      nil
    end

    # Ouvre la boîte (modale). Hash { :"Étiquette" => valeur } si acceptée, nil si annulée.
    def prompt
      shown = @fields.reject { |f| f[:kind] == :sep || f[:kind] == :desc || f[:kind] == :image }
      raw = Esq::Forms.run(@title, @fields, @accept, @cancel)
      return nil if raw.nil?
      result = {}
      shown.each_with_index do |f, i|
        result[f[:label].to_sym] = Esq::Forms.coerce(f, raw[i])
      end
      result
    end

    def __fields; @fields; end

    private

    def __dup_fields(copy)
      copy.instance_variable_set(:@fields, @fields.map(&:dup))
      copy
    end

    def __field(kind, args, kw)
      label = kw[:label] || args[0]
      raise ArgumentError, 'Field label must be non-empty' if label.nil? || label.to_s.empty?
      { kind: kind, label: label.to_s }
    end

    def __options(args, kw)
      o = kw[:options] || kw[:list] || args[1]
      o = [] if o.nil?
      raise TypeError, "Invalid type for input field: #{o.class}" unless o.is_a?(Array)
      o.map(&:to_s)
    end

    def __check_type(kw, f)
      f[:type] = kw[:type] || ValueType::STRING
    end

    def __type_of(v)
      return ValueType::LENGTH if defined?(::Length) && v.is_a?(::Length)
      case v
      when Integer then ValueType::INTEGER
      when Float then ValueType::FLOAT
      else ValueType::STRING
      end
    end
  end

  # ------------------------------------------------------------------------------------------
  class Notification
    def initialize(extension, message = nil, icon_name = nil, icon_tooltip = nil)
      if defined?(::SketchupExtension) && !extension.is_a?(::SketchupExtension)
        Esq.unexpected_type(extension, 'SketchupExtension')
      end
      @extension = extension
      @message = message
      @icon_name = icon_name
      @icon_tooltip = icon_tooltip
      @on_accept = @on_dismiss = nil
      @accept_title = @dismiss_title = nil
      @shown = false
      @id = Esq.next_dialog_id
    end

    attr_reader :message, :icon_name, :icon_tooltip, :on_accept_title, :on_dismiss_title

    def message=(v); @message = v; end
    def icon_name=(v); @icon_name = v; end
    def icon_tooltip=(v); @icon_tooltip = v; end

    def on_accept(title = nil, &blk)
      __check(blk)
      @on_accept_title = title
      @on_accept = blk
      true
    end

    def on_dismiss(title = nil, &blk)
      __check(blk)
      @on_dismiss_title = title
      @on_dismiss = blk
      true
    end

    def show
      @shown = true
      Esq.notifs[@id] = self
      Esq.ui_host('notify', { id: @id, message: @message.to_s, icon: @icon_name.to_s, tooltip: @icon_tooltip.to_s,
                           accept: @on_accept ? @on_accept_title.to_s : nil,
                           dismiss: @on_dismiss ? @on_dismiss_title.to_s : nil })
      true
    end

    def __action(which)
      Esq.notifs.delete(@id)
      (which == 'accept' ? @on_accept : @on_dismiss)&.call
    end

    private

    def __check(blk)
      raise LocalJumpError, 'no block given (yield)' unless blk
      raise 'cannot change the on_accept block, the notification already has been shown.)' if @shown
    end
  end
end

# ---------------------------------------------------------------- formulaires (InputBox, UI.inputbox)
module Esq
  module Forms
    class << self
      # Appelé quand le formulaire est affiché (utilisé par les tests pour le remplir).
      attr_accessor :on_ready

      # Conversion d'une valeur saisie vers le type du champ.
      def coerce(f, raw)
        case f[:kind]
        when :check then raw ? true : false
        when :select, :list then raw.to_s
        when :slider
          f[:type] == UI::InputBox::ValueType::FLOAT ? raw.to_f : raw.to_f.round
        else
          case f[:type]
          when UI::InputBox::ValueType::INTEGER then raw.to_s.strip.to_i
          when UI::InputBox::ValueType::FLOAT then raw.to_s.strip.tr(',', '.').to_f
          when UI::InputBox::ValueType::LENGTH
            begin
              raw.to_s.to_l
            rescue ArgumentError
              f[:value]
            end
          else raw.to_s
          end
        end
      end

      def fmt(v)
        case v
        when Float then v == v.floor && v.abs < 1e15 ? v.to_i.to_s : v.to_s
        else v.to_s
        end
      end

      def field_html(f, i)
        l = Esq.h(f[:label])
        case f[:kind]
        when :desc then "<p class='d'>#{l}</p>"
        when :sep then '<hr>'
        when :image then "<p><img src='#{Esq.h('file:///' + f[:label].tr('\\', '/'))}' style='max-width:100%'></p>"
        when :check then "<label class='r'><span></span><input type='checkbox' name='f#{i}'#{f[:value] ? ' checked' : ''}> #{l}</label>"
        when :select
          opts = f[:options].map { |o| "<option#{o == f[:value] ? ' selected' : ''}>#{Esq.h(o)}</option>" }.join
          "<label class='r'><span>#{l}</span><select name='f#{i}'>#{opts}</select></label>"
        when :list
          opts = f[:options].map { |o| "<option#{o == f[:value] ? ' selected' : ''}>#{Esq.h(o)}</option>" }.join
          "<label class='r'><span>#{l}</span><select name='f#{i}' size='5'>#{opts}</select></label>"
        when :slider
          r = f[:range]
          step = f[:type] == UI::InputBox::ValueType::FLOAT ? 'any' : 1
          v = Esq.h(f[:value])
          "<label class='r'><span>#{l}</span><input type='range' name='f#{i}' min='#{Esq.h(r.begin)}' " \
            "max='#{Esq.h(r.end)}' step='#{step}' value='#{v}' " \
            "oninput='this.nextElementSibling.textContent=this.value'><output>#{v}</output></label>"
        else
          "<label class='r'><span>#{l}</span><input type='text' name='f#{i}' value='#{Esq.h(fmt(f[:value]))}'></label>"
        end
      end

      def page(title, fields, accept, cancel)
        idx = -1
        rows = fields.map do |f|
          idx += 1 unless %i[sep desc image].include?(f[:kind])
          field_html(f, idx)
        end.join("\n")
        <<~HTML
          <!doctype html><html><head><meta charset="utf-8"><title>#{Esq.h(title)}</title>
          <style>
            body{font:13px system-ui,Segoe UI,sans-serif;margin:0;padding:14px 16px;background:#f3f3f3;color:#222}
            .r{display:flex;align-items:center;gap:10px;margin:7px 0}.r span{flex:0 0 38%}
            .r input[type=text],.r select{flex:1;padding:4px 6px;border:1px solid #aaa;border-radius:3px;font:inherit}
            .r input[type=range]{flex:1}.d{margin:8px 0;color:#444}hr{border:0;border-top:1px solid #ccc;margin:10px 0}
            .b{display:flex;justify-content:flex-end;gap:8px;margin-top:14px}
            button{min-width:80px;padding:5px 12px;border:1px solid #888;border-radius:3px;background:#fff;font:inherit}
            button.p{background:#2b6fd6;border-color:#2b6fd6;color:#fff}
          </style></head><body>
          <form id="f" onsubmit="return false">
          #{rows}
          <div class="b"><button type="button" class="p" id="ok">#{Esq.h(accept)}</button><button type="button" id="no">#{Esq.h(cancel)}</button></div>
          </form>
          <script>
            function vals(){var out=[];var els=document.getElementById('f').elements;
              for(var i=0;i<els.length;i++){var e=els[i];if(!e.name)continue;
                out.push(e.type==='checkbox'?e.checked:e.value);}
              return out;}
            document.getElementById('ok').onclick=function(){sketchup.submit(JSON.stringify(vals()))};
            document.getElementById('no').onclick=function(){sketchup.cancel()};
            window.addEventListener('load',function(){sketchup.ready()});
            document.addEventListener('keydown',function(e){
              if(e.key==='Escape')sketchup.cancel();
              if(e.key==='Enter'&&e.target.tagName!=='BUTTON'&&e.target.tagName!=='SELECT')document.getElementById('ok').click();});
          </script></body></html>
        HTML
      end

      # Ouvre le formulaire dans un HtmlDialog modal. Renvoie le tableau de valeurs brutes ou nil.
      def run(title, fields, accept, cancel)
        rows = fields.size
        height = 100 + rows * 38 + fields.count { |f| f[:kind] == :list } * 80
        dlg = UI::HtmlDialog.new(dialog_title: title, width: 460, height: height, resizable: false,
                                 style: UI::HtmlDialog::STYLE_DIALOG, scrollable: false)
        result = nil
        dlg.add_action_callback('submit') do |_c, json|
          result = JSON.parse(json)
          dlg.close
        end
        dlg.add_action_callback('cancel') { |_c| dlg.close }
        dlg.add_action_callback('ready') { |_c| on_ready&.call(dlg) }
        dlg.set_html(page(title, fields, accept, cancel))
        dlg.show_modal
        result
      end

      # UI.inputbox(prompts, defaults, list, title)
      def legacy_inputbox(prompts, defaults, lists, title)
        fields = prompts.each_with_index.map do |p, i|
          d = i < defaults.size ? defaults[i] : ''
          opts = i < lists.size ? lists[i].to_s : ''
          if opts.empty?
            { kind: :text, label: p, value: d, type: type_of(d) }
          else
            choices = opts.split('|')
            { kind: :select, label: p, options: choices, value: d.to_s, type: 0 }
          end
        end
        raw = run(title.to_s, fields, 'OK', 'Cancel')
        return false if raw.nil?
        fields.each_with_index.map { |f, i| coerce(f, raw[i]) }
      end

      def type_of(v)
        return UI::InputBox::ValueType::LENGTH if defined?(::Length) && v.is_a?(::Length)
        case v
        when Float then UI::InputBox::ValueType::FLOAT
        when Integer then UI::InputBox::ValueType::INTEGER
        else UI::InputBox::ValueType::STRING
        end
      end
    end
  end
end
