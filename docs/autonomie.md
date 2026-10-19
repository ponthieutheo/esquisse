# Autonomie : Esquisse sans SketchUp installé

But : Esquisse tourne sur un PC où SketchUp n'a jamais été installé, avec tout ce qu'il faut
livré à côté de l'exécutable et aucun fichier de Trimble. Les `.skp` sont lus et écrits
nativement, pour toutes les versions de SketchUp 3 à 2026 (`esq-skp/src/native`, écriture
`esq-skp/src/native/write`, `docs/skp-format.md` §8) : `SketchUpAPI.dll` n'est plus chargée
du tout en usage normal, seulement avec `ESQ_SKP_SDK=1` (le SDK lit et écrit alors à la place
d'Esquisse, pour comparer) et par les tests qui s'en servent d'oracle.

## 1. Inventaire : ce qu'Esquisse va chercher hors de son dossier

### Retiré (lisait l'installation de SketchUp ou les outils du développeur)

| Où | Lisait | Maintenant |
|---|---|---|
| `esq-app/src/environments.rs` `localized` | `C:\Program Files\SketchUp\SketchUp 2026\SketchUp\resources\fr\Environments.strings` | nos traductions, `crates/esq-app/assets/noms-sketchup.fr.strings` (intégré à l'exe, `i18n.rs`) |
| `esq-app/src/style_file.rs` `sketchup_names` | `…\resources\fr\Styles.strings` | idem |
| `esq-app/src/environments.rs` `SKETCHUP_LIBRARY` → `env_panel.rs` | `C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\Environments` (bibliothèque du panneau) | `Environments` du contenu livré, puis le dossier de Préférences › Fichiers › Environnements, puis `%APPDATA%\Esquisse\Environments` ; la constante ne sert plus qu'aux tests (oracle) |
| `esq-app/src/styles_panel.rs` `sketchup_library` | `%PROGRAMDATA%\SketchUp\SketchUp 2026\SketchUp\Styles` | `Styles` du contenu livré (une collection par dossier), puis Préférences › Fichiers › Styles, puis `%APPDATA%\Esquisse\Styles` |
| `esq-app/src/dialogs/mi_classif.rs` `import_dir` | `C:\Program Files\SketchUp\SketchUp 2026\SketchUp\ShippedClassifications` | Préférences › Fichiers › Classements, sinon `Classifications` du contenu livré (aussi pour l'outil Classificateur) |
| `esq-ruby/src/ffi.rs` `candidate_dlls` | `%USERPROFILE%\tools\ruby32\*\bin\x64-ucrt-ruby320.dll` (le RubyInstaller du développeur) | `ESQ_RUBY_DLL`, puis `ruby\bin\x64-ucrt-ruby320.dll` à côté de l'exe ; les programmes de test (`target\<profil>\deps`) regardent aussi `target\<profil>\ruby` |
| `esq-app/src/ui.rs` `setup_fonts` | `C:\Windows\Fonts` écrit en dur | `%WINDIR%` (ou `%SystemRoot%`)`\Fonts` |

Il n'y avait pas de bibliothèque de matières, de modèles types livrés ni de motifs de hachures
lus dans SketchUp : le panneau Matières ne montrait que le modèle, les modèles types « Simple »
sont construits par le code (`prefs::builtin_templates`). Le contenu livré les ajoute (§ 2).

### Ce qui reste, et pourquoi

| Quoi | Où | Pourquoi |
|---|---|---|
| `SketchUpAPI.dll` | `esq-skp/src/lib.rs` `find_sketchup_api` : `ESQ_SKETCHUP_API`, sinon `%ProgramFiles%\SketchUp\*\SketchUp\` | seulement avec `ESQ_SKP_SDK=1` (lecture et écriture des `.skp` par le SDK, pour comparer) et dans les tests (`native_oracle`, `native_writer`…, sautés sans SketchUp). Ouvrir, enregistrer (toutes les versions), Enregistrer une copie, Ruby `Model#save`/`save_copy`, `ComponentDefinition#save_as`/`save_copy` : natifs. Seule ligne autorisée par le test `autonomie`. |
| Polices du système | `fonts.rs` : `%WINDIR%\Fonts`, `%LOCALAPPDATA%\Microsoft\Windows\Fonts` ; `ui.rs` : Segoe UI | textes 3D et interface ; présentes sur tout Windows |
| Edge WebView2 | `wry` (boîtes `UI::HtmlDialog`) | composant de Windows 11 ; données dans `%APPDATA%\Esquisse\WebView2` (un dossier temporaire du processus dans les tests) |
| ffmpeg | `export_anim.rs` : `ESQUISSE_FFMPEG`, le PATH, `%LOCALAPPDATA%\Microsoft\WinGet\Packages\Gyan.FFmpeg*` | export vidéo seulement (programme externe admis par le brief) ; non livré |
| Programmes de Windows | `powershell` (impression), `cmd /C start`, `explorer`, `rundll32`, `curl`, `tar` (Ruby : `Sketchup::Http`, `.rbz`) | fournis par Windows 10/11 |
| Données de l'utilisateur | `%APPDATA%\Esquisse` : préférences, récents, modèles types, styles, environnements, `Plugins`, `sandbox.ini`… | les fichiers de l'utilisateur, créés par Esquisse |
| Fichiers de support Ruby | `Sketchup.find_support_file` : `ESQ_RESOURCES`, `%APPDATA%\Esquisse`, le contenu livré, le dossier de l'exe | comme l'arborescence d'installation de SketchUp |

Variables reconnues : `ESQ_CONTENT` (dossier du contenu), `ESQ_RUBY_DLL`, `ESQ_SKETCHUP_API`,
`ESQUISSE_FFMPEG`, `ESQ_RESOURCES`, `ESQ_OFFSCREEN`.

### Le garde-fou

`crates/esq-core/tests/autonomie.rs` échoue si le code livré (Rust de `crates/*/src`, Ruby de
`esq-ruby/src/rb`, `tools/package.ps1`, `ruby-dev.ps1`, `ruby-runtime.ps1`) contient un chemin
vers `Program Files`, `ProgramData`, `C:\Users\…`, `USERPROFILE`, `…\SketchUp 2026\`,
`ShippedClassifications`/`ShippedExtensions`, `\ruby32` ou `\rubyinstaller-`. Le code de test
est ignoré (éléments et modules `#[cfg(test)]`, fichiers de ces modules, dossiers `tests/` et
`examples/`), ainsi que les commentaires. Les tests qui se servent de SketchUp comme oracle
(sautés s'il est absent) restent permis : `environments.rs`, `style_file.rs`, `skm.rs`,
`skh.rs`, `tests/dialogs_classif.rs`, `esq-ruby/src/tests/instances/paths.rs`,
`content_gen::templates_table_matches_sketchup`, les tests de `esq-skp`.

Second garde-fou du même fichier, `sketchups_sdk_is_loaded_only_with_esq_skp_sdk` : dans le
code livré, ce qui atteint le SDK (`api()`, `available()`, `api_version()`, `space_api()`,
`find_sketchup_api`, `sdk_import_skp_view`, `sdk_export_skp_version`, la chaîne
`SketchUpAPI.dll`) n'apparaît que dans `esq-skp`, dans les fonctions du SDK lui-même (son
chargeur, son lecteur et son écrivain de référence) ou dans les trois portes de l'application
(`read_guid`, `import_skp_view`, `export_skp_with`) après `sdk_reader_forced()`
(`ESQ_SKP_SDK=1`). Un appel ajouté ailleurs, ou avant la porte, le fait échouer
(`the_sdk_checker_sees_ungated_calls` le vérifie sur un faux dépôt).

## 2. Le contenu par défaut (`content/`, 7,6 Mo, 454 fichiers)

Tout est fabriqué par `crates/esq-app/src/content_gen/` (module de test) et versionné :
`ESQ_WRITE_CONTENT=1 cargo test -p esq-app content_gen::write_content -- --nocapture`
le refait à l'identique (vérifié : deux générations donnent les mêmes octets ; on peut ne
refaire qu'une partie : `ESQ_WRITE_CONTENT=styles,templates`). Le test
`the_content_has_sketchups_collections_and_reads_back` vérifie à chaque `cargo test` les
collections, les nombres, que chaque fichier se relit et la taille totale (< 40 Mo).
Les dossiers portent les noms anglais de SketchUp (`Materials\Brick`…) ; l'interface les
affiche en français par `noms-sketchup.fr.strings`. Aucun téléchargement CC0 n'a été
nécessaire : textures, panoramas et vignettes sont calculés.

| Dossier | Nombre (= SketchUp 2026) | Format | Fabrication | Licence |
|---|---|---|---|---|
| `Materials` | 300 en 14 collections (Asphalte et béton 6, Brique et parement 7, Tissus 6, Verre 4, Sol 6, Métal 6, Motifs 122, Enduit 4, Plastique 4, Toiture 4, Couleurs unies 106, Pierre 5, Carrelage 4, Bois 6) | `.skm` (lu par `skm.rs`, écrit comme SketchUp) | couleurs et réglages PBR choisis ici ; 54 textures procédurales 256 px raccordables (bruits périodiques, appareils de briques, veinage…) en JPEG ; 122 motifs 128 px en PNG | MIT (Esquisse) |
| `Styles` | 111 en 8 collections (Occlusion ambiante 7, Styles variés 13, Jeux de couleurs 16, Styles par défaut 14, Modélisation d'après photo 3, Arêtes esquissées 38, Lignes droites 10, « Lauréats » 10) | `.style` | réglages à nous ; papiers (filigranes) calculés ; vignettes rendues par le moteur d'Esquisse | MIT |
| `Environments` | 18 | `.ske` (panorama HDR 1024 × 512 + réglages + soleil associé + vignette) | ciel, soleil, nuages, étoiles, sol, relief, ville ou studio calculés | MIT |
| `Templates` | 21 | `.esq` | noms, unités, options d'unités, options de rendu et ombres des modèles types de SketchUp : des faits, relevés une fois par le SDK dans `tools/content/modeles-types.json` (`ESQ_WRITE_TEMPLATE_FACTS=1 … content_gen::write_template_facts`) ; rien d'autre de leurs fichiers (ni géométrie, ni personnage, ni matière) | MIT |
| `Classifications` | 2 : IFC 2x3 (TC1) et IFC 4 (ADD2 TC1) | `.skc` | schémas XML de buildingSMART **inchangés** (`tools/content/ifc/`) ; liste des types (tout élément non abstrait dérivé d'`IfcProduct`), descriptions et vignette à nous | schémas : © buildingSMART International, CC BY-ND 4.0 — voir ci-dessous ; le reste MIT |
| `HatchPatterns\Default Hatching` | 12 (ANSI31–38, CLAY, CROSS, EARTH, STEEL) | `.skh` | familles de lignes écrites ici d'après ce que désigne chaque nom de hachure | MIT |
| `Components\Components Sampler` | 12 (SketchUp en livre davantage : nos composants sont les nôtres, pas les siens) | `.esq` + vignette `.png` | mobilier, porte et fenêtre (collées aux murs, découpent leur ouverture), escalier, clôture, lampadaire, personnage et arbre 2D (face à la caméra) construits ici de boîtes et de contours ; vignettes rendues par le moteur d'Esquisse | MIT |

Noms français : `crates/esq-app/assets/noms-sketchup.fr.strings` — 239 entrées pour les
styles (noms, descriptions, 8 collections), 18 environnements, 14 collections de matières,
les modèles types et leurs unités. Les clés sont les noms anglais de SketchUp (des faits, nécessaires pour
reconnaître ses fichiers) ; les traductions sont les nôtres.

### Schémas IFC : source et licence

Source : `github.com/buildingSMART/Standards-builds` (miroir public de
`standards.buildingsmart.org`, qui oppose un défi Cloudflare au téléchargement), téléchargés
une fois le 16/10/2026 ; empreintes SHA-256 dans `tools/content/ifc/SOURCE.txt`.
Licence : les dépôts de buildingSMART (`buildingSMART/IFC`, `IFC4.x-development`) et la
documentation IFC 4.3/4.4 déclarent CC BY-ND 4.0 ; l'en-tête du schéma IFC 4 permet l'usage
« in software development » avec attribution et sans modification. Les schémas sont donc
livrés tels quels, avec l'attribution (`Schemas/LICENCE-buildingSMART.txt` dans chaque
`.skc`, `licences\buildingSMART-IFC.txt` dans le paquet). Réserve : la page copyright de 2021
des documentations IFC2x3 TC1 et IFC4 ADD2 TC1 indique CC BY-NC-ND 4.0 ; une diffusion
commerciale d'Esquisse devrait faire confirmer ce point par buildingSMART.

## 3. Ruby livré et paquet

- `tools/package.ps1 -Ruby <RubyInstaller 3.2 x64-ucrt>` (ou `ESQ_RUBY_DLL`) monte
  `target\paquet\Esquisse` : `esquisse.exe` (release, compilé dans `target\paquet-build` pour
  `x86_64-pc-windows-msvc` avec le CRT de Visual C++ lié statiquement — sinon il faut le
  redistribuable `VCRUNTIME140.dll`, absent d'un Windows neuf — et `--remap-path-prefix` pour
  ne pas emporter les chemins de la machine ; le script refuse un exe qui importe encore
  `VCRUNTIME140`), `ruby\` (DLL, DLL
  d'accompagnement, configuration OpenSSL, bibliothèque standard, gems sans cache ni
  documentation, licences), le contenu, `LICENCE.txt`, `licences\`, `LISEZMOI.txt`.
  Il n'efface jamais rien hors de `target\paquet` (un autre `-Out` doit être vide).
- Licences des bibliothèques Rust (`licences\rust-crates.txt`) : méthode fiable et sans
  réseau — `crates/esq-app/examples/licences.rs` lit `cargo metadata --offline
  --filter-platform x86_64-pc-windows-msvc`, suit les seules dépendances *normales* d'esq-app
  (pas celles des scripts de compilation ni des tests), et recopie les fichiers
  `LICENSE*`/`COPYING*`/`NOTICE*` des sources que Cargo a déjà téléchargées ; les textes
  identiques ne sont écrits qu'une fois.
- Licences de Ruby (`ruby\licences`) : `COPYING.html` et `LEGAL.html` de la distribution,
  `BSDL.txt` (texte BSD de Ruby, `tools/licences/ruby-BSDL.txt`), la licence de RubyInstaller,
  et `LISEZMOI.txt` (`tools/licences/ruby-runtime.txt`) qui liste les DLL d'accompagnement
  (OpenSSL Apache-2.0, libffi MIT, GMP LGPLv3/GPLv2, winpthreads, LibYAML MIT, zlib) avec
  leurs sources, plus les textes complets de la LGPL v3 et de la GPL v3 (pour GMP ;
  `tools/licences/LGPL-3.0.txt` et `GPL-3.0.txt`, copiés de gnu.org).
- Développement : Esquisse ne cherche plus le Ruby du développeur. Une fois par copie du
  dépôt (et après `cargo clean`) : `powershell -File tools\ruby-dev.ps1 -Ruby <RubyInstaller>`
  copie le runtime dans `target\debug\ruby` (ou poser `ESQ_RUBY_DLL`). Sans cela, les tests
  Ruby sont sautés (« Ruby indisponible »).

## 4. Preuve (16/10/2026, sur le PC de développement)

Le paquet `target\paquet\Esquisse` lancé avec un environnement vidé : seules les variables
système gardées (`SystemRoot`, `windir`…), `PATH` = `System32;Windows;Wbem;PowerShell`,
`APPDATA`, `LOCALAPPDATA`, `USERPROFILE`, `TEMP` vers des dossiers vides,
`ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`, `ProgramData` vers des dossiers vides,
ni `ESQ_RUBY_DLL` ni `ESQ_SKETCHUP_API`, `ESQ_OFFSCREEN=1`. Piloté par UI Automation
(AccessKit), capturé par PrintWindow ; une extension de test déposée dans le
`%APPDATA%\Esquisse\Plugins` du bac à sable.

- Fenêtre de bienvenue : les 24 modèles types (5 « Simple » intégrés + 19 livrés) ; choisir
  « Architecture — Mètres » crée le modèle avec ses unités et son style.
- Ruby : l'extension se charge au démarrage avec le Ruby livré (`RbConfig prefix` = dossier
  `ruby` du paquet, Ruby 3.2.11, zlib 1.3.2, OpenSSL 3.6.1 chargés) ; son menu Extensions ›
  « Preuve : dessiner » dessine une boîte (6 faces, matière) et un texte, et
  `Sketchup.find_support_file('Brique rouge.skm', 'Materials/Brick')` trouve le contenu livré.
- Matières : la collection « Bois » montre ses 6 matières ; « Parquet chêne » devient la
  matière courante (Pot de peinture), ajoutée au modèle.
- Environnements : la collection « Environnements » montre les 18 panoramas (noms
  accentués) ; « Ciel de midi » s'affiche en fond de la vue.
- Styles : la collection « Arêtes esquissées » ; « Crayon sur papier crème » s'applique
  (papier en filigrane, arêtes tremblées).
- Enregistrement : `Model#save` écrit `preuve.esq` (385 Ko, avec la matière, le panorama et
  le style) ; un second lancement avec ce fichier en argument le rouvre à l'identique.
- Rien n'a été écrit dans les dossiers réels de l'utilisateur (dates comparées avant/après).
- `esquisse.exe` n'importe aucune DLL du CRT et ne contient aucun chemin de la machine de
  compilation ; il ne contient « Program Files » que pour `SketchUpAPI.dll`, cherchée
  seulement avec `ESQ_SKP_SDK=1` (depuis l'écrivain natif du 17/10/2026).

Ce que cette simulation ne prouve pas :
- Les chemins écrits en dur (`C:\Program Files\…`) ne dépendent pas des variables : c'est le
  test `autonomie` qui garantit qu'il n'en reste pas dans le code livré, pas la simulation.
- SketchUp reste installé ici : un code qui le trouverait par une autre voie (registre,
  dossiers connus de Windows par `SHGetKnownFolderPath`, chemin absolu construit autrement)
  ne serait pas pris en défaut. Même chose pour ce que des bibliothèques lisent par les API
  de Windows plutôt que par les variables (WebView2, boîtes de fichiers).
- Les composants de Windows présents ici le seraient sur un autre PC Windows 10/11, mais pas
  forcément : Edge WebView2 (boîtes `UI::HtmlDialog`), pilotes graphiques (wgpu/DX12),
  polices Segoe UI. Le redistribuable Visual C++ n'est plus nécessaire (CRT statique).
- Un vrai poste neuf (machine virtuelle, Windows Sandbox) n'a pas été essayé : il ouvrirait une
  fenêtre devant l'utilisateur de ce PC.

Effets de bord connus : au premier démarrage de Ruby, RubyInstaller crée
`%ProgramData%\gemrc` (fichier de configuration de RubyGems, 371 octets, « security
measure ») — c'est le comportement de son `rubygems/defaults/operating_system.rb`.
