# Composants tiers / Third-party components

[Français](#français) · [English](#english)

## Français

Le code d'Esquisse est sous licence MIT (`LICENSE-MIT`). Ce fichier recense ce qui vient
d'ailleurs : ce que le dépôt contient, ce que la compilation télécharge et ce que le paquet
Windows livre à côté de l'exécutable.

### Dans ce dépôt

| Chemin | Contenu | Auteur | Licence |
|---|---|---|---|
| `tools/content/ifc/*.xsd` et `content/Classifications/*.skc` (dossier `Schemas`) | Schémas XML IFC 2x3 TC1 et IFC 4 ADD2 TC1, sans modification (empreintes dans `tools/content/ifc/SOURCE.txt`) | buildingSMART International Ltd. | CC BY-ND 4.0 ; l'en-tête du schéma IFC 4 autorise l'usage dans un logiciel avec attribution. Voir la réserve ci-dessous. |
| `crates/esq-skp/tests/reel/*.skp` | Cinq modèles enregistrés par SketchUp, données de test, copiés sans modification | Christopher Kulla, Anton Synytsia, DeqianBai, LinkItONEDevGroup, TQEC Community | MIT ou Apache-2.0 ; dépôt d'origine, ligne de copyright et texte de chaque licence dans `crates/esq-skp/tests/reel/LICENCES.md` |
| `tools/licences/` | Textes de licence joints au runtime Ruby du paquet (Ruby BSD, GPL-3.0, LGPL-3.0) | Ruby, Free Software Foundation | textes de licence |

Tout le reste du contenu livré (`content/` : matières, textures, styles, environnements,
modèles types, composants, motifs de hachures) est fabriqué par le code d'Esquisse
(`crates/esq-app/src/content_gen`) et suit la licence MIT. Aucun fichier de Trimble
(bibliothèque, matière, style, composant, icône, modèle type) n'est dans le dépôt ni dans le
paquet.

Réserve sur les schémas IFC : la page de copyright de 2021 des documentations IFC 2x3 TC1 et
IFC 4 ADD2 TC1 annonce CC BY-NC-ND 4.0, alors que les dépôts publics de buildingSMART et les
documentations récentes déclarent CC BY-ND 4.0. Une diffusion commerciale d'Esquisse devrait
faire confirmer ce point par buildingSMART.

### Images de la documentation

Les captures de `docs/media` sont rendues par Esquisse. Trois montrent un contenu tiers, crédité
sous l'image : `galerie-skp.jpg` (Open Source Tiny House V2, © 2014 One Fold Media, MIT,
github.com/EddieOne/open-source-tiny-home), `galerie-extension.jpg` (extension Wave Function
Collapse, © 2022 Trimble Inc., MIT, github.com/SketchUp/wave-function-collapse, tuiles de Kenney,
CC0) et `extensions.jpg` (extension sketchup-shapes, © 2014 Trimble Navigation Ltd., MIT). Ni ces
modèles ni ces extensions ne sont dans le dépôt.

### Récupéré à la compilation (non versionné)

- **Bibliothèques Rust** : 307 crates en dépendances normales de l'exécutable pour
  `x86_64-pc-windows-msvc` (liste ci-dessous, d'après `cargo metadata`). Toutes sous licences
  permissives ; quand une crate offre un choix, Esquisse la reçoit sous la licence permissive
  (`self_cell` : Apache-2.0). Les polices par défaut d'egui (`epaint_default_fonts`) sont sous
  OFL-1.1 et Ubuntu Font Licence ; les certificats de `webpki-roots` sous CDLA-Permissive-2.0 ;
  les données ICU sous Unicode-3.0. Les textes complets sont réunis par
  `crates/esq-app/examples/licences.rs` dans `licences\rust-crates.txt` du paquet.
- **Runtime Ruby 3.2** : distribution RubyInstaller (`github.com/oneclick/rubyinstaller2`),
  copiée par `tools/ruby-dev.ps1` (développement, CI) ou `tools/package.ps1` (paquet). Ruby est
  sous licence Ruby ou BSD-2-Clause ; RubyInstaller sous licence BSD-3-Clause ; ses DLL
  d'accompagnement : OpenSSL (Apache-2.0), libffi (MIT), GMP (LGPL-3.0 ou GPL-2.0, DLL
  distincte et remplaçable), winpthreads (MIT), LibYAML (MIT), zlib. Le détail et les sources
  figurent dans `tools/licences/ruby-runtime.txt`.

### Utilisé par les tests, hors du dépôt

Ces tests sont sautés quand leurs données sont absentes ; la CI n'en a pas besoin.

- La suite officielle de tests de l'API Ruby de SketchUp (TestUp) et les extensions d'exemple
  de l'organisation SketchUp sur GitHub, sous licence MIT, clonées hors du dépôt
  (`ESQ_RUBY_REEL`). Manifeste : `crates/esq-ruby/tests/data/ruby-reel.json`.
- Un corpus de 1 967 fichiers `.skp` réels de dépôts GitHub publics sous licence déclarée,
  téléchargés hors du dépôt par `tools/skp-reel.ps1` (`ESQ_SKP_REEL`). Manifeste avec la
  source, la licence et l'auteur de chaque fichier : `crates/esq-skp/tests/data/skp-reel.json`.
- Le SDK C de SketchUp (`SketchUpAPI.dll`), s'il est installé, comme référence de lecture et
  d'écriture. Esquisse ne le charge pas en usage normal et ne le redistribue pas.

### Marques

SketchUp, LayOut, 3D Warehouse, Extension Warehouse et Trimble sont des marques de Trimble Inc.
AutoCAD et FBX sont des marques d'Autodesk, Inc. Google Earth est une marque de Google LLC. Ces noms désignent les formats et les
logiciels avec lesquels Esquisse est compatible. Esquisse est un projet indépendant : il n'est ni
affilié à ces sociétés, ni approuvé ou soutenu par elles.

## English

Esquisse's own code is under the MIT licence (`LICENSE-MIT`). This file lists what comes from
elsewhere: what the repository holds, what the build downloads and what the Windows package
ships next to the executable.

### In this repository

| Path | Content | Author | Licence |
|---|---|---|---|
| `tools/content/ifc/*.xsd` and `content/Classifications/*.skc` (`Schemas` folder) | IFC 2x3 TC1 and IFC 4 ADD2 TC1 XML schemas, unmodified (checksums in `tools/content/ifc/SOURCE.txt`) | buildingSMART International Ltd. | CC BY-ND 4.0; the IFC 4 schema header allows use in software development with attribution. See the note below. |
| `crates/esq-skp/tests/reel/*.skp` | Five models saved by SketchUp, test data, copied unmodified | Christopher Kulla, Anton Synytsia, DeqianBai, LinkItONEDevGroup, TQEC Community | MIT or Apache-2.0; origin, copyright line and licence text of each in `crates/esq-skp/tests/reel/LICENCES.md` |
| `tools/licences/` | Licence texts shipped with the package's Ruby runtime (Ruby BSD, GPL-3.0, LGPL-3.0) | Ruby, Free Software Foundation | licence texts |

All other shipped content (`content/`: materials, textures, styles, environments, templates,
components, hatch patterns) is generated by Esquisse's code
(`crates/esq-app/src/content_gen`) and is under the MIT licence. No Trimble file (library,
material, style, component, icon, template) is in the repository or in the package.

Note on the IFC schemas: the 2021 copyright page of the IFC 2x3 TC1 and IFC 4 ADD2 TC1
documentation states CC BY-NC-ND 4.0, while buildingSMART's public repositories and recent
documentation state CC BY-ND 4.0. A commercial distribution of Esquisse should have
buildingSMART confirm this point.

### Documentation images

The screenshots in `docs/media` are rendered by Esquisse. Three show third-party content, credited
under the image: `galerie-skp.jpg` (Open Source Tiny House V2, © 2014 One Fold Media, MIT,
github.com/EddieOne/open-source-tiny-home), `galerie-extension.jpg` (Wave Function Collapse
extension, © 2022 Trimble Inc., MIT, github.com/SketchUp/wave-function-collapse, tiles by Kenney,
CC0) and `extensions.jpg` (sketchup-shapes extension, © 2014 Trimble Navigation Ltd., MIT). Neither
these models nor these extensions are in the repository.

### Fetched at build time (not in the repository)

- **Rust libraries**: 307 crates are normal dependencies of the executable for
  `x86_64-pc-windows-msvc` (list below, from `cargo metadata`). All are under permissive
  licences; where a crate offers a choice, Esquisse takes the permissive one (`self_cell`:
  Apache-2.0). egui's default fonts (`epaint_default_fonts`) are under OFL-1.1 and the Ubuntu
  Font Licence; `webpki-roots` certificates under CDLA-Permissive-2.0; ICU data under
  Unicode-3.0. `crates/esq-app/examples/licences.rs` gathers the full texts into
  `licences\rust-crates.txt` in the package.
- **Ruby 3.2 runtime**: the RubyInstaller distribution (`github.com/oneclick/rubyinstaller2`),
  copied by `tools/ruby-dev.ps1` (development, CI) or `tools/package.ps1` (package). Ruby is
  under the Ruby licence or BSD-2-Clause; RubyInstaller under BSD-3-Clause; its companion DLLs:
  OpenSSL (Apache-2.0), libffi (MIT), GMP (LGPL-3.0 or GPL-2.0, a separate, replaceable DLL),
  winpthreads (MIT), LibYAML (MIT), zlib. Details and sources: `tools/licences/ruby-runtime.txt`.

### Used by tests, outside the repository

These tests are skipped when their data is missing; CI does not need them.

- SketchUp's official Ruby API test suite (TestUp) and the sample extensions of the SketchUp
  organisation on GitHub, under the MIT licence, cloned outside the repository
  (`ESQ_RUBY_REEL`). Manifest: `crates/esq-ruby/tests/data/ruby-reel.json`.
- A corpus of 1,967 real `.skp` files from public GitHub repositories with a stated licence,
  downloaded outside the repository by `tools/skp-reel.ps1` (`ESQ_SKP_REEL`). Manifest with the
  source, licence and author of each file: `crates/esq-skp/tests/data/skp-reel.json`.
- SketchUp's C SDK (`SketchUpAPI.dll`), when installed, as a reference reader and writer.
  Esquisse does not load it in normal use and does not redistribute it.

### Trademarks

SketchUp, LayOut, 3D Warehouse, Extension Warehouse and Trimble are trademarks of Trimble Inc.
AutoCAD and FBX are trademarks of Autodesk, Inc. Google Earth is a trademark of Google LLC. These names identify the formats and
software Esquisse is compatible with. Esquisse is an independent project: it is not affiliated
with, endorsed or sponsored by these companies.

## Crates Rust / Rust crates

<details>
<summary>307 crates (nom, version, licence / name, version, licence)</summary>

| Crate | Version | Licence |
|---|---|---|
| accesskit | 0.25.1 | MIT OR Apache-2.0 |
| accesskit_consumer | 0.39.1 | MIT OR Apache-2.0 |
| accesskit_windows | 0.35.1 | MIT OR Apache-2.0 |
| accesskit_winit | 0.34.1 | Apache-2.0 |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| ahash | 0.8.12 | MIT OR Apache-2.0 |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.4 | BSD-3-Clause |
| anstream | 1.0.0 | MIT OR Apache-2.0 |
| anstyle | 1.0.14 | MIT OR Apache-2.0 |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 |
| ash | 0.38.0+1.3.281 | MIT OR Apache-2.0 |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| bit_field | 0.10.3 | Apache-2.0 OR MIT |
| bitflags | 1.3.2 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bit-set | 0.10.0 | Apache-2.0 OR MIT |
| bit-vec | 0.9.1 | Apache-2.0 OR MIT |
| brotli | 8.0.4 | BSD-3-Clause AND MIT |
| brotli-decompressor | 5.0.3 | BSD-3-Clause OR MIT |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| bytes | 1.12.1 | MIT |
| cadkit-core | 0.3.0 | MIT OR Apache-2.0 |
| cadkit-dwg | 0.3.0 | MIT OR Apache-2.0 |
| cadkit-dxf | 0.3.0 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| codespan-reporting | 0.13.1 | Apache-2.0 |
| color | 0.3.3 | Apache-2.0 OR MIT |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 |
| cookie | 0.18.2 | MIT OR Apache-2.0 |
| core_detect | 1.0.0 | MIT OR Apache-2.0 |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| defmt | 1.2.0 | MIT OR Apache-2.0 |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 |
| deranged | 0.5.9 | MIT OR Apache-2.0 |
| derive_more | 2.1.1 | MIT |
| derive_more-impl | 2.1.1 | MIT |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |
| document-features | 0.2.12 | MIT OR Apache-2.0 |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| earcutr | 0.5.0 | ISC |
| ecolor | 0.37.0 | MIT OR Apache-2.0 |
| eframe | 0.37.0 | MIT OR Apache-2.0 |
| egui | 0.37.0 | MIT OR Apache-2.0 |
| egui-wgpu | 0.37.0 | MIT OR Apache-2.0 |
| egui-winit | 0.37.0 | MIT OR Apache-2.0 |
| either | 1.19.0 | MIT OR Apache-2.0 |
| emath | 0.37.0 | MIT OR Apache-2.0 |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| env_filter | 2.0.0 | MIT OR Apache-2.0 |
| env_logger | 0.11.11 | MIT OR Apache-2.0 |
| epaint | 0.37.0 | MIT OR Apache-2.0 |
| epaint_default_fonts | 0.37.0 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| euclid | 0.22.14 | MIT OR Apache-2.0 |
| exr | 1.74.2 | BSD-3-Clause |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| fax | 0.2.7 | MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| fearless_simd | 0.4.1 | Apache-2.0 OR MIT |
| fearless_simd | 0.7.0 | Apache-2.0 OR MIT |
| fearless_simd | 1.1.0 | Apache-2.0 OR MIT |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 OR MIT |
| foldhash | 0.2.0 | Zlib |
| font-types | 0.12.7 | MIT OR Apache-2.0 |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| getrandom | 0.3.4 | MIT OR Apache-2.0 |
| glam | 0.34.1 | MIT OR Apache-2.0 |
| glifo | 0.3.0 | Apache-2.0 OR MIT |
| glifo | 0.4.0 | Apache-2.0 OR MIT |
| glow | 0.17.0 | MIT OR Apache-2.0 OR Zlib |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 |
| gpu-allocator | 0.28.0 | MIT OR Apache-2.0 |
| guillotiere | 0.7.0 | MIT OR Apache-2.0 |
| half | 2.7.1 | MIT OR Apache-2.0 |
| harfrust | 0.12.0 | MIT |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| hayro | 0.8.0 | Apache-2.0 OR MIT |
| hayro-ccitt | 0.4.0 | Apache-2.0 OR MIT |
| hayro-cmap | 0.1.0 | Apache-2.0 OR MIT |
| hayro-interpret | 0.8.0 | Apache-2.0 OR MIT |
| hayro-jbig2 | 0.3.1 | Apache-2.0 OR MIT |
| hayro-jpeg2000 | 0.4.1 | Apache-2.0 OR MIT |
| hayro-postscript | 0.1.0 | Apache-2.0 OR MIT |
| hayro-syntax | 0.8.0 | Apache-2.0 OR MIT |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| icu_collections | 2.3.0 | Unicode-3.0 |
| icu_locale_core | 2.3.0 | Unicode-3.0 |
| icu_normalizer | 2.3.0 | Unicode-3.0 |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 |
| icu_properties | 2.3.0 | Unicode-3.0 |
| icu_properties_data | 2.3.0 | Unicode-3.0 |
| icu_provider | 2.3.1 | Unicode-3.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |
| image | 0.25.10 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| itertools | 0.14.0 | MIT OR Apache-2.0 |
| itertools | 0.15.0 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| jiff | 0.2.38 | Unlicense OR MIT |
| jiff-core | 0.1.1 | Unlicense OR MIT |
| keyring | 3.6.3 | MIT OR Apache-2.0 |
| khronos-egl | 6.0.0 | MIT OR Apache-2.0 |
| kurbo | 0.13.1 | Apache-2.0 OR MIT |
| lebe | 0.5.3 | BSD-3-Clause |
| libc | 0.2.190 | MIT OR Apache-2.0 |
| libloading | 0.8.9 | ISC |
| libloading | 0.9.0 | ISC |
| libm | 0.2.16 | MIT |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT |
| litemap | 0.8.3 | Unicode-3.0 |
| litrs | 1.0.1 | MIT OR Apache-2.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| logos | 0.16.1 | MIT OR Apache-2.0 |
| logos-codegen | 0.16.1 | MIT OR Apache-2.0 |
| logos-derive | 0.16.1 | MIT OR Apache-2.0 |
| lz4_flex | 0.14.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT |
| naga | 30.0.1 | MIT OR Apache-2.0 |
| naga-types | 30.0.1 | MIT OR Apache-2.0 |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT |
| num-complex | 0.4.6 | MIT OR Apache-2.0 |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| openusd | 0.8.0 | MIT |
| ordered-float | 5.5.0 | MIT |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| paste | 1.0.15 | MIT OR Apache-2.0 |
| peniko | 0.6.1 | Apache-2.0 OR MIT |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_generator | 0.13.1 | MIT |
| phf_macros | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| pic-scale | 0.7.12 | BSD-3-Clause OR Apache-2.0 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| png | 0.18.1 | MIT OR Apache-2.0 |
| pollster | 1.0.1 | Apache-2.0 OR MIT |
| polycool | 0.4.0 | MIT OR Apache-2.0 |
| potential_utf | 0.1.6 | Unicode-3.0 |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 |
| presser | 0.3.1 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| profiling | 1.0.18 | MIT OR Apache-2.0 |
| pulp | 0.22.3 | MIT |
| pulp-wasm-simd-flag | 0.1.1 | MIT |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| quick-error | 2.0.1 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| range-alloc | 0.1.5 | MIT OR Apache-2.0 |
| raw-cpuid | 11.6.0 | MIT |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 |
| read-fonts | 0.43.3 | MIT OR Apache-2.0 |
| reborrow | 0.5.5 | MIT |
| regex | 1.13.1 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-lite | 0.1.9 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| renderdoc-sys | 1.1.0 | MIT OR Apache-2.0 |
| rfd | 0.17.2 | MIT |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| roxmltree | 0.21.1 | MIT OR Apache-2.0 |
| rustc-hash | 1.1.0 | Apache-2.0 OR MIT |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.15 | ISC |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| self_cell | 1.3.0 | Apache-2.0 OR GPL-2.0-only |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| skrifa | 0.44.0 | MIT OR Apache-2.0 |
| skrifa | 0.46.2 | MIT OR Apache-2.0 |
| slotmap | 1.1.1 | Zlib |
| smallvec | 1.16.3 | MIT OR Apache-2.0 |
| smol_str | 0.2.2 | MIT OR Apache-2.0 |
| spirv | 0.4.0+sdk-1.4.341.0 | Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 |
| strum | 0.28.0 | MIT |
| strum_macros | 0.28.0 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| synstructure | 0.14.0 | MIT |
| tao | 0.37.1 | Apache-2.0 |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| tiff | 0.11.3 | MIT |
| time | 0.3.55 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.32 | MIT OR Apache-2.0 |
| tinystr | 0.8.4 | Unicode-3.0 |
| tracing | 0.1.44 | MIT |
| tracing-attributes | 0.1.31 | MIT |
| tracing-core | 0.1.36 | MIT |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 |
| twox-hash | 2.1.5 | MIT |
| typed-path | 0.12.3 | MIT OR Apache-2.0 |
| type-map | 0.5.1 | MIT OR Apache-2.0 |
| unicode-general-category | 1.1.0 | Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-properties | 0.1.4 | MIT OR Apache-2.0 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 |
| untrusted | 0.9.0 | ISC |
| ureq | 3.4.2 | MIT OR Apache-2.0 |
| ureq-proto | 0.6.4 | MIT OR Apache-2.0 |
| url | 2.5.8 | MIT OR Apache-2.0 |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 |
| uuid | 1.28.0 | Apache-2.0 OR MIT |
| vello_common | 0.2.0 | Apache-2.0 OR MIT |
| vello_common | 0.3.0 | Apache-2.0 OR MIT |
| vello_cpu | 0.2.0 | Apache-2.0 OR MIT |
| vello_cpu | 0.3.0 | Apache-2.0 OR MIT |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 |
| web-time | 1.1.0 | MIT OR Apache-2.0 |
| webview2-com | 0.39.1 | MIT |
| webview2-com-macros | 0.8.1 | MIT |
| webview2-com-sys | 0.39.1 | MIT |
| weezl | 0.1.12 | MIT OR Apache-2.0 |
| wgpu | 30.0.1 | MIT OR Apache-2.0 |
| wgpu-core | 30.0.1 | MIT OR Apache-2.0 |
| wgpu-core-deps-windows-linux-android | 30.0.1 | MIT OR Apache-2.0 |
| wgpu-hal | 30.0.1 | MIT OR Apache-2.0 |
| wgpu-naga-bridge | 30.0.1 | MIT OR Apache-2.0 |
| wgpu-types | 30.0.1 | MIT OR Apache-2.0 |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| windows | 0.62.2 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 |
| windows-core | 0.62.2 | MIT OR Apache-2.0 |
| windows-future | 0.3.2 | MIT OR Apache-2.0 |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 |
| windows-result | 0.4.1 | MIT OR Apache-2.0 |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 |
| windows-version | 0.1.7 | MIT OR Apache-2.0 |
| winit | 0.30.13 | Apache-2.0 |
| writeable | 0.6.4 | Unicode-3.0 |
| wry | 0.57.0 | Apache-2.0 OR MIT |
| yoke | 0.8.3 | Unicode-3.0 |
| yoke-derive | 0.8.4 | Unicode-3.0 |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.8 | Unicode-3.0 |
| zeroize | 1.9.1 | Apache-2.0 OR MIT |
| zerotrie | 0.2.5 | Unicode-3.0 |
| zerovec | 0.11.8 | Unicode-3.0 |
| zerovec-derive | 0.11.6 | Unicode-3.0 |
| zip | 8.5.0 | MIT |
| zlib-rs | 0.6.8 | Zlib |
| zmij | 1.0.23 | MIT |
| zopfli | 0.8.4 | Apache-2.0 |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| zune-inflate | 0.2.54 | MIT OR Apache-2.0 OR Zlib |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

</details>
