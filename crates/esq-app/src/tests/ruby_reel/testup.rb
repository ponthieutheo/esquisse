# encoding: utf-8
# Lanceur de la suite officielle de tests de l'API Ruby de SketchUp (dépôt
# github.com/SketchUp/testup-2, dossier « tests/SketchUp Ruby API », licence MIT) dans
# Esquisse. Écrit pour Esquisse : de TestUp, seul `sketchup_test_utilities.rb` est chargé (lu
# en entier : ni réseau ni programme lancé) ; la cale `testup/testcase` est la nôtre.
#
# Un test = un script (`EsqReel.run_one`), comme un clic dans la fenêtre de TestUp : le
# modèle actif reste celui de l'application d'un test à l'autre, les opérations de chaque test
# sont rendues après lui.

require 'json'
require 'timeout'

module EsqReel
  # Classe#méthode (ou Classe#*) → raison ; ces tests ne sont pas lancés.
  @skip = {}
  @tests = []
  @load_errors = []
  @saved_env = {}
  @guard = false
  @ui_log = Hash.new(0)
  @clipboard = nil

  # Ce qui, dans l'application, touche le bureau de l'utilisateur (boîtes natives, navigateur,
  # presse-papiers, sons) est simulé pendant la suite : OK aux boîtes de message, Annuler aux
  # boîtes de fichiers, un presse-papiers à nous. Les arguments passent d'abord par l'API
  # d'Esquisse, comme d'habitude ; seul l'appel final à l'hôte est remplacé.
  module UiGuard
    def ui_host(op, args = {}, fallback = nil)
      return super unless EsqReel.guard?
      EsqReel.ui_log[op] += 1 if EsqReel::UI_SIMULATED.include?(op)
      case op
      when 'messagebox' then EsqReel.messages << args[:text].to_s; IDOK
      when 'openpanel', 'savepanel', 'select_directory', 'beep', 'play_sound' then nil
      when 'openurl' then false
      when 'clip_get' then EsqReel.clipboard
      when 'clip_set' then EsqReel.clipboard = args[:text]; true
      else super
      end
    end
  end
  UI_SIMULATED = %w[messagebox openpanel savepanel select_directory openurl beep play_sound clip_get clip_set].freeze

  # Les formulaires (UI.inputbox, UI::InputBox) sont validés tels qu'ils s'ouvrent, comme un
  # utilisateur qui clique OK : les valeurs proposées, écrites comme dans le formulaire, sont
  # relues par Esquisse (Esq::Forms.coerce). Le titre et les libellés sont notés.
  module FormsOk
    def run(title, fields, accept, cancel)
      return super unless EsqReel.guard?
      EsqReel.forms << [title, fields.map { |f| f[:label] }]
      fields.reject { |f| %i[sep desc image].include?(f[:kind]) }.map do |f|
        case f[:kind]
        when :check then f[:value] ? true : false
        when :select, :list then f[:value].to_s
        when :slider then f[:value]
        else fmt(f[:value])
        end
      end
    end
  end

  @messages = []
  @forms = []

  class << self
    attr_reader :tests, :load_errors, :ui_log, :messages, :forms
    attr_accessor :clipboard

    def guard?; @guard; end

    # Une séance : ce que le code essayé écrit (Sketchup.temp_dir, ~, chemins relatifs) reste
    # dans root/tmp et root/home, le bureau est simulé.
    def start(root)
      @root = root
      %w[TMPDIR HOME].each { |k| @saved_env[k] = ENV[k] }
      ENV['TMPDIR'] = File.join(root, 'tmp')
      ENV['HOME'] = File.join(root, 'home')
      @saved_dir = Dir.pwd
      Dir.chdir(File.join(root, 'tmp'))
      Esq.singleton_class.prepend(UiGuard) unless Esq.singleton_class.include?(UiGuard)
      Esq::Forms.singleton_class.prepend(FormsOk) unless Esq::Forms.singleton_class.include?(FormsOk)
      @app_observers = Esq.observers[:app].dup
      fresh_observers
      @guard = true
    end

    def stop
      @guard = false
      fresh_observers
      Esq.observers[:app].replace(@app_observers) if @app_observers
      Dir.chdir(@saved_dir) if @saved_dir
      @saved_env.each { |k, v| ENV[k] = v }
    end

    # L'interpréteur est partagé par les tests, l'application est neuve à chaque essai : comme
    # Fichier › Nouveau (Esq::Events.model_replaced), on détache les observateurs laissés sur
    # le modèle d'avant, sans prévenir ceux de l'application. Sinon l'observateur que
    # TC_Sketchup_ComponentDefinition laisse sur une définition effacée se retrouve sur la
    # définition de même numéro du modèle suivant et la purge.
    def fresh_observers
      return unless defined?(Esq::Events) && Esq::Events.respond_to?(:model_replaced)
      app = Esq.observers[:app].dup
      Esq.observers[:app].clear
      Esq::Events.model_replaced('new')
    ensure
      Esq.observers[:app].replace(app) if app
    end

    # root : dossier temporaire du test (suite recopiée, cale, TMPDIR, HOME) ;
    # testup_src : dossier `src` du dépôt testup-2 ; files : fichiers TC_*.rb à charger ;
    # skip : { 'Classe#méthode' => raison }.
    def prepare(root, testup_src, files, skip)
      @skip = skip
      start(root)
      $esq_reel_testup_src = testup_src
      lib = File.join(root, 'lib')
      $LOAD_PATH.unshift(lib) unless $LOAD_PATH.include?(lib)
      require 'testup/testcase'
      @tests = []
      @load_errors = []
      files.each do |f|
        before = test_classes
        begin
          load f
        rescue Exception => e # rubocop:disable Lint/RescueException
          @load_errors << [File.basename(f), "#{e.class}: #{e.message}"[0, 300]]
          next
        end
        (test_classes - before).sort_by(&:name).each do |k|
          k.public_instance_methods(true).grep(/\Atest_/).map(&:to_s).sort.each do |m|
            @tests << [File.basename(f), k, m]
          end
        end
      end
      @tests.size
    end

    def test_classes
      ObjectSpace.each_object(Class).select { |k| k < TestUp::TestCase }
    end

    def label(i)
      f, k, m = @tests[i]
      JSON.generate([f, k.name, m])
    end

    def skip_reason(k, m)
      @skip["#{k.name}##{m}"] || @skip["#{k.name}#*"]
    end

    # [statut, message] : pass, fail, error, skip (sauté par le test), excluded (pas lancé).
    def run_one(i)
      _f, k, m = @tests[i]
      first = i.zero? || @tests[i - 1][1] != k
      last = i == @tests.size - 1 || @tests[i + 1][1] != k
      out = nil
      if (why = skip_reason(k, m))
        out = ['excluded', why]
      else
        begin
          k.setup_testcase if first && k.respond_to?(:setup_testcase)
          r = Timeout.timeout(60) { Minitest.run_one_method(k, m) }
          out = if r.skipped? then ['skip', r.failure.message.to_s[0, 200]]
                elsif r.passed? then ['pass', '']
                else
                  f = r.failure
                  where = f.respond_to?(:location) ? f.location.to_s.sub(/\A.*[\\\/]/, '') : ''
                  [r.error? ? 'error' : 'fail', "#{f.message.to_s.gsub(/\s+/, ' ')[0, 400]} [#{where}]"]
                end
        rescue Exception => e # rubocop:disable Lint/RescueException
          out = ['error', "#{e.class}: #{e.message.to_s.gsub(/\s+/, ' ')[0, 300]} (hors du test)"]
        end
      end
      begin
        k.teardown_testcase if last && k.respond_to?(:teardown_testcase) && !skip_reason(k, m)
      rescue Exception # rubocop:disable Lint/RescueException
        nil
      end
      JSON.generate(out)
    end

    def finish
      stop
      $esq_reel_testup_src = nil
      JSON.generate([@load_errors, @ui_log])
    end
  end
end

# La cale `testup/testcase` (écrite par le test Rust dans root/lib) : Minitest, puis les
# utilitaires de TestUp pour SketchUp.
module EsqReel
  def self.define_testcase
    require 'minitest'
    require 'stringio'
    require File.join($esq_reel_testup_src, 'testup', 'sketchup_test_utilities.rb')
    TestUp.module_eval do
      # Ce que `testup.rb` déclare (lu dans extension.json) ; l'extension est enregistrée
      # sans être chargée : sa fenêtre et son lanceur ne sont pas ceux qu'on teste.
      unless const_defined?(:EXTENSION, false)
        json = File.read(File.join($esq_reel_testup_src, 'testup', 'extension.json'))
        const_set(:EXTENSION, JSON.parse(json, symbolize_names: true).freeze)
        ex = SketchupExtension.new(TestUp::EXTENSION[:name], 'testup/core')
        ex.version = TestUp::EXTENSION[:version]
        ex.creator = TestUp::EXTENSION[:creator]
        Sketchup.register_extension(ex, false)
      end
      const_set(:TestCaseExtendable, Module.new do
        def test_methods
          public_instance_methods(true).grep(/^test_/i).sort
        end
      end) unless const_defined?(:TestCaseExtendable, false)
      unless const_defined?(:TestCase, false)
        k = Class.new(Minitest::Test)
        const_set(:TestCase, k)
        k.extend(TestUp::TestCaseExtendable)
        k.extend(TestUp::SketchUpTestUtilities)
        k.include(TestUp::SketchUpTestUtilities)
      end
    end
  end
end
