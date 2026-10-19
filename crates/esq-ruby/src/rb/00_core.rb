# Esquisse — SketchUp Ruby API, built on the native bridge `Esq.__native(op, json)`.
# Lengths are inches (SketchUp's convention); the bridge converts to the model's units.
require 'json'
require 'stringio'

module Esq
  @commands = []
  @console_lines = []
  @defaults = {}
  @loaded_flags = {}
  class << self
    attr_reader :commands, :defaults, :loaded_flags
    attr_accessor :plugin_dirs

    def call(op, args = {})
      r = JSON.parse(__native(op, JSON.generate(args)))
      raise(r['err']) if r.key?('err')
      r['ok']
    end

    # Runs the pending job. Returns a JSON string {ok, result, out, err}.
    def __run
      job = JSON.parse(__native('take', ''))['ok']
      out = StringIO.new
      old_out, old_err = $stdout, $stderr
      $stdout = $stderr = out
      res = { 'ok' => true }
      begin
        value = TOPLEVEL_BINDING.eval(job['code'], job['file'], 1)
        res['result'] = value.inspect
      rescue Exception => e # rubocop:disable Lint/RescueException
        res['ok'] = false
        res['err'] = "#{e.class}: #{e.message}\n" + (e.backtrace || []).reject { |l| l.start_with?('eval:') || l.include?('__run') }.first(12).join("\n")
      ensure
        $stdout, $stderr = old_out, old_err
      end
      res['out'] = out.string
      JSON.generate(res)
    end

    def invoke_command(id)
      cmd = @commands.find { |c| c[:id] == id } or return false
      cmd[:proc]&.call
      true
    end

    def commands_json
      JSON.generate(@commands.map { |c| { 'id' => c[:id], 'menu' => c[:menu], 'title' => c[:title] } })
    end

    def load_plugins(dir)
      n = 0
      return n unless File.directory?(dir)
      $LOAD_PATH.unshift(dir) unless $LOAD_PATH.include?(dir)
      Dir.glob(File.join(dir, '*.rb')).sort.each do |f|
        begin
          load f
          n += 1
        rescue Exception => e # rubocop:disable Lint/RescueException
          warn "[plugins] #{File.basename(f)}: #{e.class}: #{e.message}"
        end
      end
      n
    end
  end
end

# Units, Numeric and String extensions: see 10_geom.rb.
