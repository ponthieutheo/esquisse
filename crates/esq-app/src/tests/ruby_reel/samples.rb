# encoding: utf-8
# Les extensions d'exemple de l'organisation SketchUp sur GitHub (licence MIT), chargées dans
# Esquisse comme des extensions (un dossier Plugins temporaire) puis utilisées par leurs menus,
# un clic par script comme dans l'application. Écrit pour Esquisse ; chaque fichier d'une
# extension est lu en entier avant d'être chargé . Suppose testup.rb chargé
# (EsqReel.start : bureau simulé, formulaires validés tels quels).

module EsqReel
  # Ce qu'un outil demande à la vue de dessiner : [mode, nombre de points, options], puis
  # Esquisse le dessine comme d'habitude.
  module DrawLog
    def draw(mode, *pts, **opts)
      if EsqReel.guard?
        list = pts.size == 1 && pts[0].is_a?(Array) && !pts[0].first.is_a?(Numeric) ? pts[0] : pts
        EsqReel.draws << [mode, list.size, opts.keys.sort.map(&:to_s)]
      end
      super
    end
  end
  Sketchup::View.prepend(DrawLog)
  @draws = []

  class << self
    attr_reader :draws

    # Draw Mesh : un outil qui dessine dans la vue le maillage des faces du modèle, décalé à
    # côté. Rendu en JSON.
    def drawmesh_session
      m = Sketchup.active_model
      m.entities.clear!
      m.entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0])
      out = { 'tool' => JSON.parse(click('Draw Mesh', 'Draw Mesh'))['error'] }
      JSON.generate(out)
    end

    def drawmesh_draw
      m = Sketchup.active_model
      tool = m.tools.active_tool
      out = { 'tool' => tool.class.name }
      @draws.clear
      tool.draw(m.active_view)
      out['plain'] = @draws.dup
      flags = ->(t) { _, id = menu_commands('Draw Mesh').find { |x, _| x == t }; Esq.command_flags(Esq.cmds[id]) }
      before = [flags.('Toggle Light'), flags.('Toggle Edges')]
      out['toggle'] = [JSON.parse(click('Draw Mesh', 'Toggle Light'))['error'], JSON.parse(click('Draw Mesh', 'Toggle Edges'))['error']]
      out['flags'] = [before, [flags.('Toggle Light'), flags.('Toggle Edges')]]
      @draws.clear
      tool.draw(m.active_view)
      out['lit'] = @draws.dup
      b = tool.getExtents
      out['extents'] = [b.min.to_a, b.max.to_a].map { |p| p.map { |v| v.round(4) } }
      JSON.generate(out)
    end
    # Les commandes des menus dont le chemin finit par `suffix` : [[texte, id], ...].
    def menu_commands(suffix)
      Esq.menu_items.select { |i| i[:cmd] && i[:menu].to_s.end_with?(suffix) }
                    .map { |i| [Esq.cmds[i[:cmd]].menu_text, i[:cmd]] }
    end

    # Un clic sur la commande `text` d'un menu : ce qu'elle a laissé (les définitions créées),
    # les messages et formulaires vus, l'erreur levée. Rendu en JSON.
    def click(suffix, text)
      _, id = menu_commands(suffix).find { |t, _| t == text }
      return JSON.generate('error' => "commande introuvable : #{text}") unless id
      m = Sketchup.active_model
      before = m.definitions.to_a
      @messages.clear
      @forms.clear
      error = nil
      begin
        Esq.invoke_command(id)
      rescue Exception => e # rubocop:disable Lint/RescueException
        error = "#{e.class}: #{e.message}"[0, 200]
      end
      made = m.definitions.to_a - before
      JSON.generate('defs' => made.map { |d| describe(d) }, 'messages' => @messages.dup,
                    'forms' => @forms.map { |t, labels| [t, labels.size] }, 'error' => error)
    end

    # Un clic dans le menu contextuel (ce que les gestionnaires des extensions y mettent pour la
    # sélection courante) sur l'entrée `text`. Rend false si elle n'y est pas.
    def context_click(text)
      items = JSON.parse(Esq.context_menu_json)
      item = items.find { |i| i['title'] == text } or return false
      Esq.invoke_command(item['id'])
      true
    end

    # L'extension Weld : quatre arêtes bout à bout soudées par le menu contextuel, puis chaque
    # commande de son menu « Weld Debug ». Rendu en JSON.
    def weld_session
      m = Sketchup.active_model
      ents = m.entities
      ents.clear!
      edges = ents.add_edges([0, 0, 0], [10, 0, 0], [10, 10, 0], [20, 10, 0], [20, 20, 0])
      m.selection.clear
      m.selection.add(edges)
      @messages.clear
      out = { 'weld' => context_click('Weld') }
      curves = ents.grep(Sketchup::Edge).map(&:curve).compact.uniq
      out['curves'] = [curves.size, curves.map { |c| [c.class.name, c.edges.size] }]
      %w[Count\ Curves Mark\ Curve\ Endpoints Mark\ Edge\ PIDS Colors\ Curves Validate\ Model Dump\ PIDs].each do |t|
        r = JSON.parse(click('Weld Debug', t))
        out[t] = [r['error'], r['messages']]
      end
      out['cpoints'] = ents.grep(Sketchup::ConstructionPoint).size
      out['clines'] = ents.grep(Sketchup::ConstructionLine).map(&:stipple).sort
      out['texts'] = ents.grep(Sketchup::Text).size
      out['coloured'] = ents.grep(Sketchup::Edge).count { |e| e.material }
      out['edge colour mode'] = m.rendering_options['EdgeColorMode']
      JSON.generate(out)
    end

    # Ce que contient un ensemble d'entités : [faces, arêtes, [largeur, profondeur, hauteur]].
    def contents(ents)
      b = Geom::BoundingBox.new
      ents.each { |e| b.add(e.bounds) if e.respond_to?(:bounds) }
      [ents.grep(Sketchup::Face).size, ents.grep(Sketchup::Edge).size,
       [b.width, b.height, b.depth].map { |v| v.to_f.round(4) }]
    end

    # Le modèle vidé, puis une boîte de 10 × 20 × 30 mm à la racine.
    def box_10_20_30
      m = Sketchup.active_model
      m.entities.clear!
      f = m.entities.add_face([0, 0, 0], [10.mm, 0, 0], [10.mm, 20.mm, 0], [0, 20.mm, 0])
      f.reverse! if f.normal.z < 0
      f.pushpull(30.mm)
      contents(m.entities)
    end

    # L'extension STL : export de la boîte en ASCII et en binaire (millimètres), puis import de
    # chaque fichier dans un modèle vidé. Rendu en JSON.
    def stl_round_trip(dir)
      stl = CommunityExtensions::STL
      box = box_10_20_30
      ents = Sketchup.active_model.active_entities
      out = { 'box' => box }
      %w[ASCII Binary].each do |format|
        path = File.join(dir, "boite-#{format}.stl")
        stl::Exporter.export(path, ents, 'selection_only' => false, 'export_units' => 'Millimeters',
                                         'stl_format' => format)
        data = File.binread(path)
        out[format] = if format == 'ASCII'
                        xs = data.scan(/vertex (\S+) (\S+) (\S+)/).map { |v| v.map(&:to_f) }
                        { 'facets' => data.scan('facet normal').size, 'max' => xs.transpose.map { |c| c.max.round(4) } }
                      else
                        { 'size' => data.bytesize, 'count' => data[80, 4].unpack1('V') }
                      end
      end
      %w[ASCII Binary].each do |format|
        Sketchup.active_model.entities.clear!
        status = stl::Importer.new.load_file(File.join(dir, "boite-#{format}.stl"), false)
        ok = status == Sketchup::Importer::ImportSuccess
        out["import #{format}"] = [ok] + contents(Sketchup.active_model.entities)
      end
      out['messages'] = @messages.dup
      JSON.generate(out)
    end

    def describe(d)
      ents = d.entities
      edges = ents.grep(Sketchup::Edge)
      b = d.bounds
      { 'name' => d.name, 'faces' => ents.grep(Sketchup::Face).size, 'edges' => edges.size,
        'soft' => edges.count(&:soft?), 'curves' => edges.map(&:curve).compact.uniq.size,
        'class' => d.get_attribute('skpp', 'class'),
        'size' => [b.width, b.height, b.depth].map { |v| v.to_f.round(4) } }
    end
  end
end
