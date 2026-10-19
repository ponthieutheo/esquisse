# Sécurité : ce qu'Esquisse lit des fichiers et des extensions

Esquisse ouvre des fichiers venus de n'importe où (modèles téléchargés, bibliothèques partagées,
extensions). Ce document recense les points d'entrée, les trouvailles de l'audit d'octobre 2026
(gravité, preuve, état) et **les règles que tout code nouveau suit**.

Modèle de menace : un fichier (modèle, matière, style, archive, page d'un dialogue) est une donnée
hostile possible ; le code Ruby d'une extension est du code (l'utilisateur l'a installé), mais ce
qu'il lit d'un modèle ne l'est pas. Un fichier ne doit jamais : faire contacter un serveur ou un
périphérique, écrire hors du dossier prévu, lancer un programme, faire paniquer, déborder la pile,
tourner sans fin, ni réserver une mémoire hors de proportion avec sa taille.

Hors périmètre de cet audit : le lecteur et l'écrivain `.skp` natifs (`crates/esq-skp/src/native/`,
audités à part ; ils suivent les mêmes règles : budget de leur ZIP, limite de 256 niveaux).

## Règles communes (à suivre dans tout code nouveau)

Elles sont dans `esq-core` : `esq_core::untrusted`, `esq_core::zip`, `esq_core::repair`.

1. **Chemin venu d'un fichier** : jamais ouvert tel quel. `untrusted::read_referenced`,
   `referenced_is_file`, `referenced_modified` (ou `local_path`) n'acceptent qu'un chemin absolu
   sur un lecteur local : pas d'UNC (`\\serveur\x`, `//serveur/x`, `\/x`), pas de `\\?\`, `\\.\`,
   `\??\`, pas de `file://serveur/`, pas de nom de périphérique (`CON`, `NUL`, `COM1`, `LPT1`,
   `CONIN$`…, avec ou sans extension). Ouvrir un chemin UNC fait envoyer par Windows le hachage
   NTLM de l'utilisateur au serveur. Les refus sont notés (`untrusted::refused_paths`) : les tests
   prouvent l'absence d'accès par ce journal, jamais par une vraie connexion SMB.
2. **Nom de texture ou de fichier cité par un fichier** : réduit à un nom nu
   (`esq_formats::tex::safe_name`, `esq_core::meta::base_name`) et cherché à côté du fichier
   ouvert ou dans son paquet, jamais utilisé comme chemin.
3. **Entrée d'archive écrite sur le disque** : `untrusted::join_under(racine, nom)`, qui refuse
   `..`, chemin absolu, lecteur (`C:`), flux (`x:y`), nom réservé, point ou espace final
   (Windows les retire : `.. ` deviendrait `..`), caractères interdits ; un lien symbolique
   (`zip::Header::is_symlink`) est refusé. Tout est vérifié avant la première écriture.
4. **ZIP** : un seul lecteur, `esq_core::zip` (décalages vérifiés, ZIP64, au plus 2^20
   entrées). Pas de nouveau lecteur ZIP : `.skp` mis à part, tout paquet passe par lui.
5. **Décompression** : toujours dans un `untrusted::Budget` partagé par tout ce que le fichier
   produit (`Budget::for_input` : 100 fois la taille du fichier plus 256 Mo, au plus 4 Go), et
   jamais au-delà de la taille annoncée (`Budget::inflate`). Jamais
   `miniz_oxide::inflate::decompress_to_vec` sans limite.
6. **Taille ou nombre lu dans un fichier** : vérifié contre les données présentes avant toute
   réservation (`Vec::with_capacity`, `vec![x; n]`, `(0..n).collect()`) ; arithmétique vérifiée
   (`checked_*`, `saturating_*`) ; un angle de révolution ou d'arc borné à un tour.
7. **Index lu dans un fichier** : `get()`, jamais `[i]` ; découpage d'une chaîne par octets
   (`as_bytes().get(..)`) ou par caractères, jamais `&s[i..i + 2]` après un caractère quelconque.
8. **Graphe lu dans un fichier** (nœuds, définitions, références) : chaque nœud parcouru une fois
   (ensemble des vus), la définition réservée avant la lecture de son contenu, pas de récursion
   sans limite ; une limite de profondeur seule ne suffit pas (la fan-out reste exponentielle).
9. **Modèle lu** (`.esq`, `.skp`, modèle type, composant chargé, `Model#import`,
   `DefinitionList#load`) : `Model::check_after_reading` (`esq_core::repair`) avant tout usage,
   déjà appelé par `esq_core::io::load_esq` et `esq_skp::import_skp_view` : arêtes et faces aux
   sommets absents retirées, cycles de définitions coupés, imbrication coupée à `MAX_DEPTH` = 256
   niveaux (la limite du lecteur `.skp` natif ; SketchUp n'en documente pas, les vrais modèles
   restent sous vingt) et la partie profonde, devenue inutilisée, retirée ; refus au-delà de
   `MAX_PLACEMENTS` = 10^8 placements. Le contrôle est itératif. Les parcours de l'arbre (rendu,
   exports, solides, Ruby…) peuvent alors rester récursifs : un nouveau parcours n'a pas à
   redoubler la limite, mais tout nouveau chemin qui fabrique un modèle à partir d'un fichier
   doit passer par `check_after_reading`. Les tags, matières et instances de définitions absentes
   restent à `esq_core::validity`, que l'application lance après l'ouverture et qui prévient
   l'utilisateur.
10. **XML** : pas d'entités déclarées (`<!ENTITY` refusé par le lecteur COLLADA) ; profondeur
    bornée (256) pour les arbres construits et parcourus récursivement.
11. **Lancement de programme** : aucun argument venu d'un fichier. `UI.openURL` ne passe que
    `http(s):`, `mailto:` et `file:` vers un document local (ni partage réseau, ni programme :
    `.exe`, `.bat`, `.lnk`, `.ps1`, flux `::$DATA`…). Pas de `tar`, `cmd`, `powershell` pour lire
    un fichier.
12. **Dialogues web** : navigation et nouvelles fenêtres vers un chemin réseau refusées.
13. **FFI Ruby** : rien qui puisse lever une exception Ruby ne s'appelle depuis une fonction Rust
    ayant des destructeurs en attente : vérifier la classe (`rb_obj_is_kind_of`), puis appeler
    sous `rb_protect` (les rappels sont `extern "C-unwind"` sans destructeur).
14. **Tests** : aucun test ne touche le réseau ; un piège se prouve par un fichier fabriqué, et
    un blocage par un fil avec délai (`recv_timeout`), pas par l'attente.
15. **Image envoyée au GPU** : jamais plus grande que ce que la carte prend (sinon egui
    s'arrête sur une assertion et wgpu refuse la texture : l'application meurt, même sur un
    fichier légitime avec une texture de 10 000 px). Texture egui : `gpu_img::load_texture`,
    seul chemin permis (test `no_texture_is_loaded_around_the_check`) ; texture wgpu faite
    d'une image : `gpu_img::fit(…, gpu_img::device_max_side(&device))` avant `create_texture`.

## Points d'entrée

| Point d'entrée | Ce qu'il accepte | Lecteur |
|---|---|---|
| Fichier › Ouvrir, modèles types, composants | `.esq` (JSON), `.skp` | `esq_core::io::load_esq`, `esq_skp::import_skp_view` (+ `repair`) |
| Fichier › Importer (3D) | 3DS, DAE, DEM/DDF, DWG/DXF, glTF/GLB, IFC/IFCZIP, KMZ, STL, TRB, USD/USDA/USDC/USDZ | `esq_formats::*` (cadkit pour DWG/DXF, openusd pour USD) |
| Fichier › Importer (2D), outil Image | BMP, JPEG, PNG, TGA, TIFF, PSD, PDF | `imgfile` (crate `image`, PSD maison, hayro pour PDF) |
| Fenêtre › Matières, Styles, Environnements, Hachures | `.skm`, `.style`, `.ske`/`.hdr`/`.exr`, `.skh`, `.pat` | `skm`, `style_file`, `environments`, `skh` |
| Infos sur le modèle › Classements, outil Classer | `.skc` | `mi_classif`, `tools::classifier` |
| Chemins nommés par un modèle | panorama d'environnement, `.skc` d'un classement, image d'une définition | `untrusted::read_referenced` |
| Ruby : extensions | `.rb` du dossier Plugins, `Sketchup.install_from_archive` (`.rbz`) | `esq-ruby` (`misc::extract_archive`) |
| Ruby : `Classifications#load_schema` | `.skc` | `misc.zip_texts` |
| Ruby : `UI::HtmlDialog`, `UI::WebDialog` | pages, `skp:` | `ruby_host/ui/web.rs` (WebView2) |
| Ruby : `UI.openURL` | URL | `ruby_host/ui.rs` |
| Pont natif Ruby | `Esq.__native(op, json)` | `esq-ruby/src/lib.rs`, `ffi.rs` |

## Trouvailles

Gravité : **élevée** = fuite de données, écriture hors du dossier, programme lancé ou mort de
l'application par un petit fichier ; **moyenne** = panique, blocage ou mémoire excessive ;
**basse** = débordement de l'arithmétique en version de débogage, cas marginaux.

| # | Gravité | Entrée | Problème | Preuve (test) | État |
|---|---|---|---|---|---|
| 1 | élevée | Environnement d'un modèle | panorama lu à son chemin dès l'affichage : `\\serveur\x.hdr` → hachage NTLM envoyé ; « Emplacement du soleil » lisait le chemin brut (et paniquait sur `sun_pos` vide) | `environments::a_network_path_in_a_model_is_never_opened` | corrigé |
| 2 | élevée | Classement d'un modèle | existence du `.skc` testée à chaque image pendant la sélection : `\\serveur\x.skc` → NTLM | `mi_classif::a_network_schema_path_is_never_looked_at` | corrigé |
| 3 | élevée | USD | sous-couches, références, charges utiles résolues par openusd (`exists`, `canonicalize`) : `@\\serveur\x.usda@` → NTLM ; nom nu cherché dans le dossier courant | `usd::network_layers_are_never_opened` | corrigé (résolveur enveloppe) |
| 4 | élevée | `UI.openURL` | tout `file:` passé à `url.dll,FileProtocolHandler` : partage réseau ouvert, programmes lancés (`.exe`, `.bat`, `.lnk`, `::$DATA`) | `open_url_refuses_network_shares_and_programs` | corrigé |
| 5 | élevée | `install_from_archive` | archive confiée au `tar` de Windows : tout format (pas seulement ZIP), décompression sans limite, noms non contrôlés | `rbz_archives_cannot_write_outside_plugins` (`..`, absolu, lecteur, UNC, `CON`, flux, lien) | corrigé |
| 6 | élevée | `.style`, `.skm`, `.ske`, `.skh`, `.skc`, KMZ, IFCZIP, USDZ | six lecteurs ZIP sans borne de décompression ; 300 entrées sur le même mégaoctet compressé = 300 Mo pour quelques dizaines de ko | `a_style_file_bomb_is_refused`, `a_kmz_bomb_is_refused`, `zip::bombs_are_refused` | corrigé (lecteur commun) |
| 7 | élevée | PSD | 59 To réservés d'après l'en-tête (30 octets) : mort à l'allocation | `photoshop_headers_cannot_ask_for_terabytes` | corrigé |
| 8 | élevée | glTF | `count` d'un accessor : 96 Go réservés d'un coup | `a_huge_declared_count_is_refused` | corrigé |
| 9 | élevée | COLLADA | `<vcount>` de 2^63 : liste de cette taille ; entités XML (« billion laughs » au premier niveau, non bornées par roxmltree) | `counts_and_offsets_from_the_file_are_bounded`, `entity_declarations_are_refused` | corrigé |
| 10 | élevée | DAE, IFC, KMZ (construction) | composant qui se contient lui-même : récursion jusqu'au débordement de pile (processus tué) | `a_definition_that_contains_itself_is_cut`, `a_node_that_instances_itself_ends` | corrigé |
| 11 | élevée | IFC, TRB | angle de révolution / d'arc de 10^12 : milliards d'anneaux ou de segments | `a_huge_revolution_angle_is_one_turn`, `a_huge_arc_angle_is_one_turn` | corrigé |
| 12 | élevée | `.esq` (et `.skp` par le même contrôle) | clé de sommet orpheline : panique à l'ouverture ; définition qui se contient deux fois : 2^64 visites à chaque parcours ; chaîne de 100 000 définitions imbriquées : vérifié sans la coupe, l'ouverture ne finit pas en 300 s et l'export fait mourir le processus (`STATUS_STACK_OVERFLOW`, même sur une pile de 8 Mo) | `a_dangling_vertex_key_does_not_panic`, `a_definition_that_contains_itself_is_cut_when_read`, `deep_chains_and_cycles_are_cut_when_read`, `tests::securite::deep_and_cyclic_models_{open_and_show,export,go_to_ruby}` (chaîne de 100 000 niveaux et cycle A → B → A : ouverture, vue 3D hors écran, tous les exports 3D, script Ruby récursif) | corrigé (`repair`) |
| 13 | moyenne | `.skc` (outil Classer, `load_schema` Ruby) | deux types parents l'un de l'autre : boucle sans fin | `a_cycle_of_parent_types_ends` (Rust et Ruby) | corrigé |
| 14 | moyenne | glTF | normales plus courtes que les positions : panique ; nœuds listant deux fois le suivant : 2^64 visites | `short_normals_do_not_panic`, `nodes_listed_many_times_are_entered_once` | corrigé |
| 15 | moyenne | 3DS | face texturée sans points : panique ; keyframer : nœuds de même id que leur parent → 41^64 visites (et parcours quadratique) | `a_textured_face_without_points_does_not_panic`, `keyframer_nodes_naming_themselves_as_parent_end` | corrigé |
| 16 | moyenne | IFC | représentation qui se référence : relue à chaque élément, 2^32 lectures | `a_map_that_maps_itself_ends` | corrigé |
| 17 | moyenne | IFC (STEP) | `\X\aé`, `\S\é` : caractère coupé, panique | `escapes_before_multibyte_characters_do_not_panic` | corrigé |
| 18 | moyenne | DAE, glTF, 3DS, USD, KMZ (textures) | `%` devant un caractère accentué dans un nom : panique | `references_are_reduced_to_bare_names` | corrigé |
| 19 | moyenne | `.style`, `.skm`, `.ske` | attribut délimité par « é » : panique ; document imbriqué 200 000 fois : débordement de pile | `hostile_xml_is_refused_without_panic` | corrigé |
| 19 bis | moyenne | `.skh` | même découpage de l'attribut délimité par « é » : panique | `skh::a_non_ascii_quote_does_not_panic` | corrigé |
| 20 | moyenne | Dialogues web | navigation et `window.open` vers `file://serveur/` → NTLM | `dialogs_never_go_to_network_shares` | corrigé (ressources : voir « ouvert ») |
| 21 | moyenne | Pont Ruby (FFI) | `Esq.__native(1, nil)` ou une chaîne avec octet nul : exception Ruby levée à travers un cadre Rust (comportement indéfini ; prouvé : le TypeError traversait le rappel) | `the_native_bridge_refuses_what_is_not_a_string` | corrigé |
| 22 | basse | Dialogues web | `set_background_color("#aé1234")` : panique du fil de toutes les fenêtres web | `colors_parse` | corrigé |
| 23 | basse | Importations 3D | coordonnée de 1e300 ou `inf` : débordement de la grille de soudure (débogage) | `huge_coordinates_do_not_overflow_the_weld_grid` | corrigé |
| 24 | basse | `.skh` (`patData`) | nombre de tirets de 3·10^38 : `2 * n` déborde (débogage) | `pat_files_and_sketchup_pat_data` | corrigé |
| 25 | moyenne | Enregistrement `.skp` (écrivain natif, `esq-skp/src/native/write/`) | plan et archive MFC faits récursivement sur les définitions imbriquées : sur la pile de 1 Mo du fil principal, une version de débogage débordait vers 130 niveaux (256 passaient en `--release`) ; un script pouvait imbriquer bien plus | `native_writer::deep_nesting_is_written_from_a_small_stack` (fil de 1 Mo, débogage : 256 niveaux écrits et relus dans les 15 versions, 4 096 écrits, 4 097 refusés), `tests::securite::the_deepest_model_kept_saves_on_a_small_stack` (plus ignoré) | corrigé : écriture sur un fil dédié de 256 Mio de pile (`WRITE_STACK`, réservée, non engagée), imbrication bornée à 4 096 niveaux (`MAX_NESTING`, erreur propre au-delà) ; une panique du fil devient une erreur |

| 26 | moyenne | Images affichées : vignettes de styles et de composants, icônes d'extensions, textures de `View#draw` (Ruby), outil Positionner la texture, filigranes, panoramas | image envoyée au GPU à sa taille : au-delà du côté maximal de la carte (8 192 ou 16 384), assertion d'egui en débogage et texture refusée par wgpu (mort de l'application) ; un panorama très haut gardait toute sa hauteur | `gpu_img::a_texture_never_exceeds_the_cards_side`, `gpu_img::no_texture_is_loaded_around_the_check`, `environments::a_panorama_of_any_shape_fits_the_gpu` | corrigé (`gpu_img`) |

## Ce qui reste ouvert

- **Mutations déterministes** (troncatures, octets inversés sur les lecteurs) : faites pour le
  lecteur `.skp` natif seulement (`esq-skp/tests/native_mutations.rs`, 296 786 cas sur les
  1 969 fichiers réels, aucun problème : `docs/skp-format.md` §6) ; les autres lecteurs ont des
  tests de fichiers piégés fabriqués à la main.
- **Ressources d'une page web** (`<img src="file://serveur/x">` dans une page d'extension qui
  affiche des données d'un modèle) : wry n'expose pas de filtre des requêtes de ressources ;
  seule la navigation est filtrée. À traiter si wry ajoute `WebResourceRequested`.
- **Lecteurs tiers** : `image` (limite de 512 Mo par image, par défaut), hayro (PDF : page rendue
  au plus 10 000 px de côté, comme SketchUp), cadkit (DWG/DXF), openusd (USDC), roxmltree :
  non audités en profondeur.
- **DEM** : une grille de 10^8 points (800 Mo) peut être demandée par un petit fichier ; c'est
  la limite du programme de SketchUp (« … rangées dépassent la limite du programme »), gardée.
- **Noms réservés à l'export** : une texture nommée `CON.png` s'écrit sous ce nom à côté d'un
  export ; sans effet sous Windows 11, à renommer si un Windows plus ancien est visé.
- **Beaucoup de définitions** : certains panneaux comptent les instances définition par
  définition (quadratique : `count_used_instances` en Ruby, statistiques) ; un modèle de 10^5
  définitions non imbriquées reste lent (minutes), sans débordement.
- **Modèle construit par une extension** : un script Ruby peut imbriquer 100 000 groupes par
  l'API ; c'est du code installé par l'utilisateur, hors du modèle de menace, mais les parcours
  récursifs ne sont pas protégés contre lui (seule la lecture d'un fichier l'est).
