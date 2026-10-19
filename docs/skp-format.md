# Le format .skp (SketchUp 3 à 2026)

Ce document décrit le format des fichiers `.skp` tel qu'Esquisse le lit et l'écrit sans
SketchUp (`crates/esq-skp/src/native/`, écriture au §8). Sources : les fichiers eux-mêmes (corpus fabriqué par le SDK, voir §7), et le code de
lecture/écriture de SketchUp.exe et SketchUpAPI.dll, étudié pour l'interopérabilité
(SketchUp 2026, version 26.2.243). Rien de ce document ne recopie du code de Trimble.

Conventions : petit-boutiste partout ; longueurs en **pouces** (×25,4 pour les millimètres
d'Esquisse) ; angles en radians sauf mention ; « id » = identifiant persistant d'un objet.

## 1. Enveloppe

| Octets | Contenu |
|---|---|
| `FF FE FF 0E` + 14 × UTF-16 | `CString` MFC « SketchUp Model » |
| `FF FE FF nn` + nn × UTF-16 | `CString` de la version qui a écrit le fichier : `{26.2.243}`, `{20.0.1}`, `{3.0.xxx}` |
| … | corps, selon la version (§1.1 ou §1.2) |

Une `CString` MFC (`CArchive << CString`) : `FF FE FF` (marque Unicode), puis la longueur en
unités UTF-16 sur 1 octet ; si cet octet vaut `FF`, sur 2 octets ; si ceux-ci valent `FFFF`,
sur 4. Sans la marque `FF FE FF`, la chaîne est en octets (ANSI) avec le même préfixe.

### 1.1 SketchUp 2021 et suivants

Après la version : un marqueur de 13 octets puis une archive ZIP.

```
56 46 46 08 00 01 00 11 00  xx xx xx xx   50 4B 03 04 …
 V  F  F                    (4 octets variables d'un fichier à l'autre)
```

Les 9 premiers octets sont constants dans tout le corpus (2024 à 2026). Les 4 suivants sont
une somme de contrôle des entrées de l'archive (vérifiée sur tout le
corpus, §8.2) ; le lecteur ne la vérifie pas. L'archive commence au premier `PK\x03\x04` ;
ses offsets sont relatifs à ce début. Méthodes : stockée (0) et deflate (8) ; les en-têtes
utilisent ZIP64 (version 0x2D) ; dates nulles (1980-01-00).

Entrées observées :

| Entrée | Contenu |
|---|---|
| `model.dat` | le modèle (§2) |
| `meta/meta.dat` | description du fichier, GUID du modèle (§2.1) |
| `meta/model_thumbnail.png`, `meta/preview_thumbnail.png` | vignettes (absentes des fichiers du SDK) |
| `materials/<dossier>/material.xml` | une matière (§2.6) |
| `materials/<dossier>/<image>` | sa texture et ses cartes PBR (fichiers d'origine) |
| `materials/<dossier>/thumbnail.jpg` | vignette de la matière |
| `styles/<nom>/style.xml` | un style (options de rendu, §2.7) |
| `thumbnails/<nom>.png` | vignettes des définitions |
| `matched_photos/…`, `environments/…`, `hatchpatterns/…`, `foreground_images/…`, `classifications/…`, `scene_thumbnails/…`, `watermarks/…` | ressources des classes du même nom (noms relevés dans les écrivains de SketchUp.exe) |

### 1.2 SketchUp 3 à 2020

Après la version : 16 octets (GUID du modèle), une `CString` (vide dans le corpus), 4 octets
(horodatage), puis une archive MFC (`CArchive`, §3). Pas de ZIP : textures et images sont
dans l'archive.

## 2. Le modèle de SketchUp 2021+ (`model.dat`)

### 2.1 Enregistrements

`model.dat` et `meta/meta.dat` sont des arbres d'enregistrements :

```
tag:u16  longueur:u32  données[longueur]
```

Les données sont soit une valeur, soit une suite d'enregistrements (le type dépend du tag : il
faut connaître le schéma, des données de valeur peuvent ressembler à des enregistrements).
Chaque classe écrit un enregistrement « classe » (son tag, ex. arête = 3000 = 0x0BB8) qui
contient d'abord les enregistrements de ses classes de base, puis ses champs, chacun dans son
propre enregistrement. Un lecteur doit ignorer les tags inconnus (versions futures) et
accepter l'absence d'un champ (valeur par défaut).

Encodage des valeurs :

| Valeur | Encodage |
|---|---|
| booléen | 1 octet |
| entier | 4 octets (certains champs « petits » sur 1 octet : drapeaux 0x7D3) — lire selon la longueur |
| double | 8 octets IEEE |
| flottant | 4 octets IEEE |
| point, vecteur | 3 doubles |
| plan | 4 doubles (a, b, c, d) : a·x + b·y + c·z + d = 0, (a,b,c) unitaire |
| couleur | 4 octets R, G, B, A |
| chaîne | UTF-8 brut, sans zéro final (la longueur de l'enregistrement fait foi) |
| GUID | 16 octets, structure GUID Windows (Data1 u32, Data2 u16, Data3 u16, Data4[8]) |
| id (référence) | 1 à 8 octets, petit-boutiste, longueur minimale (18 → `12`, 45175 → `77 B0`) |
| liste d'ids | suite de (longueur:u8, id sur ce nombre d'octets) : `01 12 01 15` = [18, 21] |
| heure | 4 octets, secondes depuis 1970 (`ShadowTime`) |

Ids : chaque entité porte le sien (enregistrement 0x5DE). Le prochain id libre est dans
0x1F5 → 0x3E8 → 0x3E9. Les références sont des ids ; un objet est toujours défini avant ou
après ses références indifféremment (le lecteur fait deux passes : définitions d'abord,
collection racine ensuite).

`meta/meta.dat` : enregistrement 0x64 contenant : 0x75 version (« 26.2.243 »), 0x76 majeure
(u16), 0x77, 0x73, 0x74 (u16), **0x66 GUID du modèle** (`SUModelGetGuid`), 0x6D nom de l'unité
(« Millimeter »), 0x6F chemin d'origine, 0x78 historique des clients (0xC8 → 0xC9 → 0xCA :
0xCB nom du client, 0xCC heure, 0xCD 8 octets).

### 2.2 Sections de `model.dat`

Racine : 0x1F4 (= 500). Ordre d'écriture :

| Tag | Contenu | Lu |
|---|---|---|
| 0x063 | version du schéma (0) | — |
| 0x20A | nom de l'entrée meta (`meta/meta.dat`) | — |
| 0x209 | propriétés du modèle : un conteneur d'attributs (§2.4) avec le dictionnaire `ModelProperties` (`Name`, `Description`, `IsClassified`…) | nom, description |
| 0x200 | gestionnaire d'options 0x61A8 (§2.5) | oui |
| 0x1F5 | 0x3E8 → 0x3E9 prochain id | — |
| 0x1FA | caméra du modèle 0x34BC | oui |
| 0x1FB | options de rendu 0x733C (§2.7) | oui |
| 0x204 | ombres 0x6590 | oui |
| 0x1FC | axes de dessin 0x4650 | oui |
| 0x208 | styles de trait 0x4074 | noms |
| 0x211 | motifs de hachures 0x7D00 (§10.11) | oui |
| 0x1F7 | matières 0x30D4 | oui |
| 0x1F8 | étiquettes 0x3A98 | oui |
| 0x1FD | polices 0x4E20 (§10.3) | oui |
| 0x1FE | style de texte 0x57E4 (§10.3) | oui |
| 0x1FF | style de cote 0x5FB4 (écrivain, §10.3) | oui |
| 0x216 | 0x7DC8 (écrit si la fonction « ModelingGrid » est active : éteinte) | non |
| 0x201 | photos adaptées 0x2904 (§10.9) | oui |
| 0x202 | id de la photo active (§10.9) | oui |
| 0x203 | filigranes du modèle 0x2CEC `CWatermarkManager` (§10) | oui |
| 0x1F9 | définitions (§2.3.6) | oui |
| 0x1F6 | collection racine 0x1388 (§2.3.4) | oui |
| 0x20C, 0x214 | booléens du modèle (§10.13 ; 0x214 : « faces retournées mises à jour ») | 0x20C |
| 0x20D | entier du modèle (§10.13) | oui |
| 0x206 | styles 0x6978 `CSkpStyleManager` (§10) | oui |
| 0x210 | environnements 0x7918 (§10.10) | oui |
| 0x213 | images d'avant-plan 0x7D64 (fonction « ForegroundImage », §5) | vide |
| 0x207 | scènes 0x6D60 (§2.3.11) | oui |
| 0x205 | classifications 0x6784 (§10.4) | oui |
| 0x217 | 0x7DFA (fonction « ClassificationTags », éteinte) | non |
| 0x215 | (écrit si la fonction « ProceduralComponents » est active : éteinte) | non |
| 0x20E | comportement par défaut 0x1B58 | non |
| 0x20F | entier (§10.13) | oui |

### 2.3 Classes

Notation : `tag nom : type` ; « id » = référence. Les classes de base sont les premiers
enregistrements.

#### 2.3.1 Entité, élément de dessin

- **0x5DC CEntity** (écrit seulement si l'entité a un id ou des dictionnaires) : 0x5DE id ;
  0x5DD conteneur d'attributs (§2.4).
- **0x7D0 CDrawingElement** : 0x5DC ; 0x7D1 matière (id) ; 0x7D2 étiquette (id ; absente = étiquette
  par défaut) ; 0x7D3 drapeaux (1 octet).

Drapeaux 0x7D3 (bits, `CDrawingElement`, valeur par défaut 6) :

| Bit | Sens |
|---|---|
| 0x01 | caché |
| 0x02 | projette des ombres |
| 0x04 | reçoit des ombres |
| 0x08 | arête adoucie (soft) |
| 0x10 | arête lissée (smooth) |
| 0x20 | instance verrouillée |

#### 2.3.2 Géométrie

- **0x9C4 CVertex** : 0x5DC ; 0x9C5 position (point).
- **0xBB8 CEdge** : 0x7D0 ; 0xBB9 sommet de départ (id) ; 0xBBA sommet d'arrivée (id) ; 0xBBB
  courbe (id, si l'arête en fait partie).
- **0xDAC CFace** : 0x7D0 (0x7D1 = matière **avant**) ; 0xDAF matière arrière (id) ; 0xDAD plan
  (la normale de la face) ; 0xDAE boucles : suite de **0x1194 CLoop**, la première est la boucle
  extérieure, les suivantes les trous.
- **0x1194 CLoop** : 0x5DC ; 0x1195 suite de **0xFA0 CEdgeUse** : 0x5DC ; 0xFA1 arête (id) ;
  0xFA2 inversée (booléen). Sommets d'une boucle, dans l'ordre : le départ de chaque arête
  (son arrivée si inversée). Le SDK rend la boucle extérieure dans le sens trigonométrique
  autour de la normale.
- **0x4A38 CCurve** : 0x5DC ; 0x4A39 nombre de segments (u32) ; 0x4A3A booléen « polygone »
  (vrai pour toute courbe simple, vrai pour un arc dessiné comme polygone, faux pour un
  arc/cercle) ; 0x4A3B première arête (id) ; 0x4A3C dernière arête (id).
- **0x4C2C CArcCurve** : 0x4A38 ; 0x4C2D arc, 16 doubles : centre (3), plan
  (4 : normale, d), axe x (3, longueur = rayon), axe y (3, longueur = rayon), rayon (1),
  angle de départ (1), angle de fin (1).
- **0x4844 CPolyline3d** (liste 0x1395) : non lue.

#### 2.3.3 Placement de texture d'une face

Un élément 0x2710 **CFaceTextureCoords** du conteneur d'attributs de la face (§2.4) :
0x36B0 (base) ; 0x2711 côté avant, 0x2712 côté arrière ; chacun 0x2713 :

| Tag | Contenu |
|---|---|
| 0x2714 | drapeaux : bit 0 positionnée, bit 1 projetée |
| 0x2715 | matrice M 3×3 (9 doubles : ses colonnes, voir le calcul) |
| 0x2716 | direction de projection (vecteur) |
| 0x2717 | punaises : suite de 0x2718 {0x2719 (2 doubles), 0x271A (2 doubles)} |

Si le conteneur en a plusieurs, SketchUp prend la première.

Calcul : le UV helper de SketchUpAPI.dll (`SUFaceGetUVHelper` → : repère,
 matrice ; `SUUVHelperGetFrontUVQ` →). Vérifié
sur tout le corpus réel (§9) et celui du SDK, au point près hors de la face aussi, et par les
tests `uv_tile_at` de TestUp :
1. Repère 2D du côté. Côté projeté (bit 1, direction 0x2716 de longueur ≥ 1e-10) : le plan
   passant par l'**origine** de normale la direction de projection. Sinon le plan de la face
   (n, d), origine O = −d·n. Les deux côtés d'une face prennent ce même repère (l'envers n'est
   pas retourné). Axes (règle de l'« axe arbitraire ») : Ax = Y × n si n est
   la verticale (chaque composante à 0,001 près de (0, 0, ±1)), sinon Ax = Z × n, normalisé ;
   Ay = n × Ax. Une face tournée vers le bas a donc Ax = −X, Ay = +Y.
2. Point P du modèle → (x, y) = ((P−O)·Ax, (P−O)·Ay) : les coordonnées ne changent pas le long
   de la normale du repère (la projection pour un côté projeté).
3. M (9 doubles : les colonnes de la matrice ; SketchUp la garde telle que lue, +0x38, et son
   inverse, +0x80, calculé à la lecture) envoie les coordonnées de texture
   en pouces vers le repère : [x y w]ᵀ = M·[s t 1]ᵀ. Donc [S T Q]ᵀ = M⁻¹·[x y 1]ᵀ ;
   u = S/(Q·largeur), v = T/(Q·hauteur), largeur et hauteur = taille d'un motif en pouces
   (`xScale`, `yScale` du material.xml).
4. Côté ni positionné ni projeté : M = identité. Matière sans texture, ou pas de matière : M =
   identité, 1 pouce par motif, repère du plan de la face (placement et projection ignorés).

M est affine quand sa dernière ligne vaut (0, 0, 1) ; une texture déformée (4 punaises) la rend
projective. Le lecteur faisait la règle 1 sans le repère de la projection : c'était l'écart des
36 fichiers réels du §9.

`Face#position_material` / `SUFacePositionMaterial` :
chaque point du modèle est pris dans ce repère (la hauteur au-dessus du plan tombe : le long de
la projection pour un côté projeté), chaque point UV en longueur de texture (u × largeur,
v × hauteur ; la 3e coordonnée est ignorée) ; M vient d'exactement ces paires : 1 paire une
translation, 2 une similitude, 3 une affinité, 4 une perspective (le quadrilatère des paires
0, 1, 3, 2) ; des points dégénérés (trois alignés, quadrilatère aplati) sont refusés
(« Could not compute valid matrix from points »). `uv_tile_at` : le point de
référence projeté sur le plan de la face, la tuile de texture qui le contient, ses coins (0, 0),
(1, 0), (1, 1), (0, 1) ramenés dans le plan du repère (celui de la projection, par l'origine,
pour un côté projeté), les points UV avec 0 pour 3e coordonnée.

Esquisse garde ce placement comme la fonction (u·q, v·q, q) du point, entière
(`esq_core::meta::UvMap`, `FaceTex`) ; `esq_core::texpos` fait le repère (`Frame::of_side`),
la lecture (`map_from_sketchup`), l'écriture (`sketchup_matrix`) et `position_material`
(`map_from_pairs`), `meta::side_uv` le placement par défaut. Le lecteur et l'écrivain natifs,
le lecteur et l'écrivain par le SDK, le rendu, les exports, les outils de texture et l'API Ruby
s'en servent.

Quand la face change (mesuré avec le SDK : `tests/textures_uv.rs`,
`placements_follow_their_faces_as_in_sketchup`) : déplacée ou tournée
(`SUEntitiesTransform`, le code des outils Déplacer et Rotation), le placage la suit, projeté ou
non (la fonction composée avec l'inverse de la transformation, la projection transformée :
`instance_ops::transform_entities`) ; retournée (`Face#reverse!`, Inverser les faces,
`SUFaceReverse` →, qui ne fait que changer le signe du plan et l'ordre des
boucles), matières et placages restent sur leur côté, et la matrice gardée dans le repère
retourné (axe x opposé, même origine et même axe y) donne un placage en miroir ; un côté
projeté ne change pas (`Entities::flip_face`). Esquisse retourne autrement les faces de ses
opérations (Pousser/tirer, Suivez-moi, importations : `reverse_face`) : chaque matière y part
avec son placage sur l'autre côté, la texture reste en place.

#### 2.3.4 Collection d'entités (0x1388 CEntities)

0x7D0 (le 0x5DC porte l'id de la **définition** quand la collection en est une ; celui de la
collection racine porte les **dictionnaires du modèle**, `SUModelGetAttributeDictionaries`).
Puis des listes, chacune présente si non vide :

| Tag | Contient |
|---|---|
| 0x1389 | sommets 0x9C4 |
| 0x1396 | courbes 0x4A38 |
| 0x1397 | arcs 0x4C2C |
| 0x138A | arêtes 0xBB8 |
| 0x138B | faces 0xDAC |
| 0x139C | boucles libres 0x1194 |
| 0x138C | instances 0x1964 |
| 0x138D | groupes 0x1D4C |
| 0x1390 | images 0x1F40 |
| 0x1391 | lignes de construction 0x4269 |
| 0x1392 | points de construction 0x426C |
| 0x139D | ancrages 0x7530 (§8.4) |
| 0x1395 | polylignes 0x4844 |
| 0x1393 | plans de section 0x445C |
| 0x1398 | textes 0x55F0 |
| 0x1399 | cotes linéaires 0x5BCC |
| 0x139A | cotes radiales 0x5DC0 |
| 0x13A2 | espaces 0x7E2C (§8.4) |
| 0x13A3 | lumières 0x7E5E (non lues) |
| 0x139E | (vide dans le corpus) |
| 0x138E | ordre des entités : liste d'ids |
| 0x139B | 0x639F `CRelationshipMap` → 0x63A0 → 0x639C `CRelationship` (collages, §10.12) |
| 0x139F | booléen |
| 0x1394 | plan de section actif (id) |
| 0x13A0 | boîte englobante d'une définition (6 doubles : min, max) |

#### 2.3.5 Instances, groupes, images

- **0x1964 CComponentInstance** : 0x7D0 ; 0x1965 nom ; 0x1966 transformation, 13 doubles :
  la partie 3×3 **par lignes** (t0..t8), la translation (t9..t11, pouces), l'échelle w (t12).
  En matrice colonne (SUTransformation) : colonnes (t0,t3,t6,0), (t1,t4,t7,0), (t2,t5,t8,0),
  (t9,t10,t11,t12). 0x1967 définition (id) ; 0x1968 GUID de l'instance ; 0x1969 (option).
- **0x1D4C CGroup** : 0x1964.
- **0x1F40 CImage** : 0x1964 (la définition est une définition d'image, §2.3.6). Le nom de
  fichier et la taille en pixels sont ceux de la texture de la face de la définition.

#### 2.3.6 Définitions

0x1F9 → 0x063, 0x1770 → 0x063, 0x1771 → 0x063 puis les **0x157C CComponentDefinition** :

| Tag | Contenu |
|---|---|
| 0x1388 | entités (son 0x5DC : id et dictionnaires de la définition) |
| 0x157D | GUID |
| 0x157E | nom |
| 0x157F | description |
| 0x1580 | chaîne (chemin ?) |
| 0x1582 | booléen |
| 0x1583 | genre : 0 composant, 1 groupe, 2 image |
| 0x1581 | heure (4 octets) |
| 0x1B58 | comportement (§ ci-dessous) |
| 0x1585 | caméra de la vignette 0x251C → 0x251D → 0x34BC |
| 0x1586 | grille de modélisation (id, si l'option « ModelingGrid » est active) |

**0x1B58 CComponentBehavior** (7000) : 0x1B59 plan d'accroche (0 quelconque, 1 horizontal,
2 vertical, 3 incliné) ; 0x1B5A masque d'échelle ; 0x1B5B se colle (booléen) ; 0x1B5C découpe
une ouverture ; 0x1B5D toujours face à la caméra ; 0x1B5E ombres face au soleil. Pour le SDK,
`component_snap` = 0 si 0x1B5B est faux, sinon 1 + 0x1B59.

#### 2.3.7 Construction, sections, annotations

- **0x4268 CConstructionGeometry** : 0x7D0.
- **0x4269 CConstructionLine** : 0x4268 ; 0x426A 8 doubles : point, direction (unitaire),
  paramètres t₀, t₁ des deux bouts (±1e30 = infini) ; 0x426B u16 (0xFFFF). Le SDK rend une
  ligne infinie (point, direction) ou un segment (P+t₀d, (t₁−t₀)d).
- **0x426C CConstructionPoint** : 0x4268 ; 0x426D position ; 0x426E vecteur ; 0x426F booléen.
- **0x445C CSectionPlane** : 0x7D0 ; 0x445D plan ; 0x445E nom ; 0x445F symbole. Actif : 0x1394
  de la collection.
- **0x5208 point d'attache** : 0x5209 type (u32, voir §3.5 : 0 aucun, 1 point libre, 2 sommet,
  3 point de construction, 4 milieu, 5 sur une arête, 6 sur une ligne de construction, 7 sur
  une face, 9 intersection, 10 centre d'un arc, 11 sur un plan de section, 12, 13 instance) ;
  0x520A point (paramètres pour les types 5 à 7) ; 0x520B, 0x520C les deux entités, chacune
  0x53FC {0x53FD entité (id), 0x53FE chemin d'instances (liste d'ids, la plus extérieure
  d'abord)}. Le point se calcule au chargement (§3.5). Esquisse garde l'attache et l'écrit
  (§3.5) : 0x53FD puis 0x53FE dans 0x520B, 0x520C vide.
- **0x55F0 CText** : 0x7D0 ; 0x55F1 texte ; 0x55FF chaîne ; 0x55F2, 0x55F3 position à l'écran
  (fractions de la vue) ; 0x55F4 → 0x5208 point ; 0x55F5 vecteur de la ligne de rappel ; 0x55F6
  booléen ; 0x55F7 vecteur ; 0x55F8 booléen ; 0x55F9 police (id) ; 0x55FA type de flèche ;
  0x55FB entier ; 0x55FC booléen ; 0x55FD type de ligne de rappel (0 aucune, 1 vue, 2 punaise) ;
  0x55FE entier.
- **0x59D8 CDimension** : 0x7D0 ; 0x59D9 texte (`<>` = mesure) ; 0x59DA police (id) ; 0x59DB
  booléen ; 0x59DC type de flèche.
- **0x5BCC CDimensionLinear** : 0x59D8 ; 0x5BCD départ → 0x5208 ; 0x5BCE arrivée → 0x5208 ;
  0x5BCF normale ; 0x5BD0 axe x ; 0x5BD1 entier ; 0x5BD3 position x ; 0x5BD2 position y
  (décalage de la ligne de cote le long de normale × axe x) ; 0x5BD4 entier.

#### 2.3.8 Matières (0x1F7 → 0x30D4 → 0x30D5)

**0x32C8 CMaterial** (13000) : 0x5DC (id, **sans** dictionnaires) ; 0x32CC nom du dossier
(`materials/<nom>/`, nom assaini) ; 0x32CA appartient à un autre objet (étiquette, image :
n'apparaît pas dans la liste du SDK) ; 0x32CB chaîne ; 0x32CD entier (texture) ; 0x32CE
vignette. Tout le reste est dans `material.xml` (§2.6). 0x30D6 (fin du gestionnaire) : id de la
matière courante.

#### 2.3.9 Étiquettes (0x1F8 → 0x3A98)

0x3A99 liste de **0x3C8C CLayer** : 0x5DC ; 0x3C8D nom ; 0x3C8E cachée ; 0x3C8F matière
propre 0x32C8 (sa couleur est la couleur de l'étiquette) ; 0x3C90 comportement dans les scènes
(u32, bits de `Layer#page_behavior` : 1 cachée par défaut, 0x10 visible dans les nouvelles
scènes, 0x20 cachée dans les nouvelles scènes) ; 0x3C91 style de trait (id). La première est
l'étiquette par défaut (« Layer0 »).
0x3A9A étiquette active (id). 0x3A9B → **0x3E80 CLayerGroup** racine (dossiers) : 0x5DC ;
0x3E81 nom ; 0x3E83 sous-dossiers 0x3E80 ; 0x3E84 étiquettes (liste d'ids) ; 0x3E82 caché ;
0x3E85 caché dans les nouvelles scènes.

Styles de trait (0x208 → 0x4074 → 0x4075) : **0x4076 CCustomLineStyle** : 0x5DC ; 0x4077 nom ;
0x4078 motif (« 12.0, -6.0 ») ; 0x407A, 0x4079 doubles ; 0x407B couleur ; 0x407C booléen.

#### 2.3.10 Caméra, axes, ombres

- **0x34BC CCamera** : 0x34BD œil ; 0x34BE cible ; 0x34BF haut ; 0x34C0, 0x34C1 doubles ;
  0x34C2 perspective ; 0x34C4 champ de vision (degrés) ; 0x34C3 hauteur de la vue parallèle ;
  0x34C5…0x34CE réglages (non lus). Pour une caméra parallèle, le SDK ne donne pas de champ
  de vision : le lecteur d'Esquisse garde 35°.
- **0x4650 axes** : 0x7D0 ; 0x4651 origine ; 0x4652 x ; 0x4653 y ; 0x4654 z.
- **0x6590 CShadowInfo** (26000), noms du SDK (`SUShadowInfoGetValue`) :

| Tag | Clé | Type |
|---|---|---|
| 0x6591 | ShadowTime | heure |
| 0x6592 | DaylightSavings | booléen |
| 0x6593 | City | chaîne |
| 0x6594 | Country | chaîne |
| 0x6595 | Longitude | double |
| 0x6596 | Latitude | double |
| 0x6597 | TZOffset | double |
| 0x6598 | vecteur du nord → NorthAngle = atan2(x, y) en degrés, ramené dans [0, 360) | vecteur |
| 0x6599 | DisplayShadows | booléen |
| 0x659A | DisplayNorth | booléen |
| 0x659B | DisplayOnAllFaces | booléen |
| 0x659C | DisplayOnGroundPlane | booléen |
| 0x659D | EdgesCastShadows | booléen |
| 0x659E | Light | entier |
| 0x659F | Dark | entier |
| 0x65A0 | UseSunForAllShading | booléen |

#### 2.3.11 Scènes (0x207 → 0x6D60 CPageList)

0x6D61 liste de **0x7148 CViewPage** (29000) ; 0x6D62 scène sélectionnée (id).

| Tag | Contenu |
|---|---|
| 0x6F54 | CSketchUpPage : 0x5DC (id, dictionnaires) ; 0x6F55 nom ; 0x6F56 description |
| 0x7149 | drapeaux de ce que la scène mémorise (`SUSceneGetFlags`) |
| 0x714A | caméra 0x34BC (si drapeau 0x01) |
| 0x714B | (drapeau 0x10) |
| 0x714C | style (id) ; écrit même sans le drapeau 0x02 (le SDK le rend, `SUSceneGetStyle`) |
| 0x714D | options de rendu 0x733C (drapeau 0x02) |
| 0x714E | ombres 0x6590 (drapeau 0x04) |
| 0x715A, 0x715B | (drapeau 0x200 : réglages 0x7B0F…) |
| 0x714F | axes 0x4650 (drapeau 0x08) |
| 0x7150 | étiquettes cachées, liste d'ids (drapeau 0x20) ; 0x7159 dossiers |
| 0x7151 | entités cachées (drapeau 0x40) |
| 0x7152 | incluse dans l'animation |
| 0x7153 | chaîne |
| 0x7154, 0x7155 | durées (−1 = celles du modèle) |
| 0x7156 | id |
| 0x715C | image d'avant-plan |
| 0x7157 | booléen |
| 0x7158 | vignette |

### 2.4 Attributs (dictionnaires)

0x5DD → **0x36B1 CAttributeContainer** → 0x36B2 éléments : dictionnaires **0x36B3
CAttributeNamed** et, pour une face, son placement de texture 0x2710 (§2.3.3).

0x36B3 : 0x36B0 (base, sous-dictionnaires ?) ; 0x36B4 nom ; 0x36B5 paires (0x36B6 clé, 0x38A4
valeur) — clés triées ; 0x36B7 u32.

**0x38A4 valeur typée** : un seul enregistrement enfant (aucun = vide, `nil`), de tag
0x38A3 + type de variante de SketchUp (le même numéro que l'attribut `type` du XML, §2.6) :

| Tag | Variante | Type `SUTypedValue` | Encodage |
|---|---|---|---|
| 0x38A5 | 2 | octet | 1 |
| 0x38A6 | 3 | entier 16 | 2 |
| 0x38A7 | 4 | entier 32 | 4 |
| 0x38A8 | 5 | flottant | 4 |
| 0x38A9 | 6 | double | 8 |
| 0x38AA | 7 | booléen | 1 |
| 0x38AB | 8 | couleur | 4 |
| 0x38AC | 9 | heure | 4 |
| 0x38AD | 10 | chaîne | UTF-8 |
| 0x38AE | 11 | tableau | suite de 0x38A4 |
| 0x38AF–0x38B3 | 12–16 | double (longueurs, angles…) | 8 |
| 0x38B4 | 17 | point (vecteur 3D pour le SDK) | 24 |
| 0x38B5 | 18 | vecteur 3D | 24 |
| 0x38B6 | 19 | vecteur unitaire (vecteur 3D pour le SDK) | 24 |
| 0x38B7 | 20 | transformation | — |

Une clé de valeur vide n'est pas rendue par le SDK. Les types 12–19 se trouvent dans les
fichiers réels (§9, attributs écrits par des extensions) ; le type 20 dans aucun fichier.

### 2.5 Options du modèle (Infos sur le modèle)

0x200 → 0x61A8 → 0x61A9 → fournisseurs **0x61AA** : 0x61AB nom (`UnitsOptions`,
`PageOptions`, `SlideshowOptions`, `NamedOptions`, `PrintOptions`…) ; 0x61AC paires (0x61AD
clé, 0x38A4 valeur). Unités du modèle : `UnitsOptions/LengthUnit` (0 pouce, 1 pied, 2 mm,
3 cm, 4 m, 5 yard).

### 2.6 `material.xml`

```xml
<materialDocument …><mat:material name="Brique" type="1" workflow="0" colorRed="150" colorGreen="80"
  colorBlue="60" colorizeType="1" trans="0" useTrans="0" pbrPromoState="0" hasTexture="1">
  <mat:texture textureFilename="brique.png" xScale="19.68…" yScale="9.84…" avgColor="4278668656">
    <mat:images><mat:image id="1" path="./brique.png" file_name="brique.png"/></mat:images>
  </mat:texture>
  <mat:pbrMR xScale="…" yScale="…">
    <mat:enable_metalness>0</mat:enable_metalness> … <mat:normal_texture id="2" path="./normale.png" file_name="normale.png"/>
    <mat:metallicFactor>0.80000001</mat:metallicFactor> …
  </mat:pbrMR></mat:material></materialDocument>
```

- `type` : 0 couleur, 1 texture, 2 texture colorisée ; `trans` = 1 − opacité ; `useTrans` ;
  `xScale`/`yScale` = taille d'un motif en pouces ; `avgColor` = couleur moyenne 0xAABBGGRR.
- Dictionnaires de la matière (pas dans `model.dat`) : `<n0:AttributeDictionaries count>` →
  `<n0:AttributeDictionary name count>` → `<n0:Attribute key type [count]>valeur</n0:Attribute>`;
    `type` = variante (§2.4) ; couleur en décimal
  0xAABBGGRR ; chaîne en CDATA ; tableau : enfants `<value type>` ; vecteur :
  `<vector x y z/>` (point : `<point>`, unitaire : `<unitVector>`) ; heure en secondes. Le
  texte d'une chaîne est celui de sa section CDATA s'il y en a une, avec les fins de ligne
  ramenées à `\n` (comme tout le XML).
- Chemin d'une image : relatif au dossier de la matière quand il commence par `./` ; un chemin
  sans `./` qui contient `/` (fichiers réels : `materials/<nom>/<fichier>`) se cherche d'abord
  depuis la racine de l'archive. Un nom de texture sans extension d'image
  (`textureFilename="Brique"`) reçoit celle du format de l'image (`.png`, `.jpg`, `.bmp`,
  `.gif`, `.tif`), comme le rend le SDK, après le nom tel qu'écrit : ses points et espaces
  finaux restent (`S.` → `S..jpg`, `S. ` → `S. .jpg`). Taille en pixels : PNG, JPEG, BMP, GIF,
  TIFF (étiquettes 256/257 de la première IFD), TGA (en-tête de 18 octets, sans signature).
- Cartes PBR : `metallic_texture`, `roughness_texture`, `normal_texture`, `occlusion_texture`
  (fichiers du même dossier). Facteurs en simple précision. Sans `pbrMR`, valeurs par défaut :
  facteurs 1, `normalMapStyle` 1.
- Les décalages de colorisation ne sont pas écrits : `SUMaterialGetColorizeDeltas` les
  calcule (SketchUpAPI.dll) à partir de la couleur moyenne de l'image et de
  la couleur de la matière, en TSL (composantes divisées par 255 en simple précision,
  comparaisons à 0,001 près) : luminosité = L(couleur) − L(moyenne) ; saturation et teinte
  selon que l'une ou l'autre est un gris (code de `colorize_deltas` dans `native/build.rs`).
  La couleur moyenne rendue pour une texture colorisée est alors la couleur de la matière.
- Décodage des images : SketchUp 2026 passe par OpenImageIO 2.5 et, pour le JPEG, libjpeg-turbo
  3.0.3 (chaînes de SketchUpAPI.dll). `native::jpeg` refait ce décodage sans bibliothèque de
  Trimble : IDCT entière lente (ISLOW), suréchantillonnage « fancy » (triangle, bords répétés ;
  h2v1 et h2v2 seulement si la largeur réduite dépasse 2 pixels, sinon répétition), conversion
  YCbCr → RVB en virgule fixe (16 bits, tables de libjpeg), tables de Huffman par défaut de
  l'annexe K si le fichier n'en a pas, balayages progressifs, marqueurs de reprise, données
  manquantes complétées par des zéros, lissage des blocs seulement pour une image progressive
  inachevée. Les moyennes de couleur (colorisation, `avgColor` d'une matière lue en SketchUp 3
  à 2020) sont exactement celles du SDK (`tests/jpeg_sdk.rs` compare au pixel près au décodage
  du SDK, `SUImageRepLoadFile`). L'algorithme vient de l'Independent JPEG Group (mention dans
  le module).

### 2.7 Options de rendu (0x733C CRenderingOptions)

Noms du SDK (`SURenderingOptionsGetKeys`) → enregistrement : table `RENDERING` de
`native/vff.rs` (relevée dans la table d'accesseurs de `CRenderingOptions` de SketchUp.exe,
 et son écrivain). Particularités :
- `DrawHidden` = `DrawHiddenGeometry` (0x7380) ou `DrawHiddenObjects` (0x7381) ;
- `DisplaySectionPlanes` = bit 0 de 0x7375, `DisplaySectionCuts` = bit 1 ;
- `BackgroundColor`, `ForegroundColor` à 0 0 0 0 : couleurs système (blanc, noir) ;
  `FogColor` à 0 : la couleur d'arrière-plan ;
- non écrits (valeur après chargement) : `SectionCutDrawEdges` vrai, `ShowViewName` vrai,
  `BandColor` (0,0,0,127) ; absents des fichiers d'avant 2025 : options `AmbientOcclusion…`
  (faux, 10, 0, 1, faux, noir).

`styles/<nom>/style.xml` : les mêmes réglages par numéro d'élément (`<sty:item id="1000">`) ;
non lus (le modèle et les scènes ont leurs propres 0x733C).

## 3. Le modèle de SketchUp 3 à 2020 (archive MFC)

Lu par `native/mfc.rs` (l'archive) et `native/legacy.rs` (les classes), d'après la branche de
lecture du `Serialize` de chaque classe de SketchUp.exe (emplacement 2 de sa vtable). Le
lecteur lit les 680 fichiers 3–2020 du corpus jusqu'au dernier octet, identiques au SDK.

### 3.1 L'archive

Une archive `CArchive` de MFC : pas de longueurs ni d'enregistrements ; chaque objet est écrit
par son `Serialize`, champ après champ, dans un ordre qui dépend du **schéma** de sa classe.
Une classe inconnue, ou un champ mal compté, rend la suite illisible : toutes les classes qui
peuvent paraître doivent être lues, même celles qui n'intéressent pas Esquisse (styles,
polices, filigranes…).

Types : u8/bool (1 octet), u16, u32/i32, f64 ; *point* et *vecteur* = 3 f64 ; *plan* = 4 f64
(a, b, c, d) ; *couleur* = 4 octets R, V, B, A ; *str* = `CString` (§1) ; *GUID* = 16 octets
(u32, u16, u16, 8 octets) ; *transformation* = 13 f64 (3×3 par lignes, translation, échelle ;
comme 0x1966, §2.3.5) ; *id compact* = un octet de masque puis un octet par bit à 1 (les
octets non nuls de l'id, poids faible d'abord) ; **obj** = une référence d'objet :

| u16 | Sens |
|---|---|
| `00 00` | nul |
| `FF FF` | nouvel objet d'une classe déclarée ici : schéma (u16, inutilisé), longueur du nom (u16, < 0x40), nom ASCII, puis les champs de l'objet |
| `0x8000 \| i` | nouvel objet de la classe d'index i, puis ses champs |
| `i` (< 0x7FFF) | l'objet d'index i, déjà lu |
| `7F FF` + u32 | index ≥ 0x7FFF sur 4 octets (bit 31 : classe) |

Table des index : elle commence à 1 ; chaque déclaration de classe prend un index, chaque
nouvel objet le suivant, **avant** ses champs (un objet peut se désigner lui-même). Un objet
est écrit entier à sa première référence, où qu'elle soit : une instance lit sa définition en
place si elle ne l'a pas encore été, et ainsi de suite — la lecture est récursive aussi loin
que le fichier l'imbrique. Les membres « en place » (les gestionnaires du modèle et des
composants, le comportement, la caméra ancienne…) n'ont ni étiquette ni index : leur
`Serialize` est appelé directement.

Le schéma qu'un `Serialize` consulte est celui de la **table des versions** du fichier,
 0 pour une classe qu'elle ne nomme pas — pas celui écrit avec la classe.

### 3.2 En-tête et table des versions

Après §1.2 (GUID, `CString` vide, u32 horodatage) : l'objet `CVersionMap` (nouvelle classe,
index 1, objet 2), suite de paires (str classe, u32 schéma) close par la chaîne
« End-Of-Version-Map » et u32 0. Noms de SketchUp 3 renommés : `CESketchPage` →
`CSketchUpPage`, `CESketchDoc` → `CSketchUpModel`, `CAnnotation` → `CText`. Le GUID de
l'en-tête est celui du modèle (`Skp.read_guid`).

Schémas écrits par chaque version (fichiers du SDK, `SUModelSaveToFileWithVersion`) :

| Classe | 3 | 4 | 5 | 6 | 7 | 8 | 2013 | 2014 | 2015 | 2016 | 2017 | 2018 | 2019 | 2020 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| CArcCurve | 1 | 1 | 1 | 1 | 1 | 1 | 2 | 2 | 3 | 3 | 3 | 3 | 3 | 3 |
| CAttribute | – | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| CAttributeContainer | – | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| CAttributeNamed | – | 0 | 0 | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CBackgroundImage | – | – | – | 1 | 9 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 |
| CCamera | – | 4 | 4 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 |
| CComponent | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 | 11 |
| CComponentBehavior | 3 | 4 | 4 | 4 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 |
| CComponentDefinition | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 10 | 11 | 11 | 11 |
| CComponentInstance | 3 | 3 | 4 | 4 | 4 | 4 | 4 | 5 | 5 | 5 | 5 | 6 | 6 | 6 |
| CConstructionLine | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CCurve | 3 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| CCustomLineStyle | – | – | – | – | – | – | – | – | – | – | – | – | 2 | 3 |
| CDib | 2 | 2 | 2 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 |
| CDimension | 0 | 0 | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CDimensionLinear | 4 | 4 | 4 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| CDimensionRadial | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| CDimensionStyle | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| CDrawingElement | 8 | 8 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 |
| CEdge | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| CEdgeUse, CLoop, CGroup, CImage, CSketchUpPage, CWatermark | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CEntity | 2 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 5 | 5 | 5 | 5 |
| CFace | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 |
| CFaceTextureCoords | – | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| CLayer | 1 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 3 | 3 |
| CLayerGroup | – | – | – | – | – | – | – | – | – | – | – | – | – | 2 |
| CLayerManager | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 7 |
| CLineStyleManager | – | – | – | – | – | – | – | – | – | – | – | – | 1 | 1 |
| CMaterial | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 |
| CMaterialManager | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| CPageList | – | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CRenderingOptions | 22 | 25 | 28 | 32 | 35 | 36 | 36 | 36 | 36 | 36 | 36 | 37 | 38 | 39 |
| CSchemaFile, CSchemaZipFile | – | – | – | – | – | – | – | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CSectionPlane | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 3 | 3 | 3 |
| CShadowInfo | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 | 7 |
| CSketchUpModel | 10 | 12 | 12 | 18 | 22 | 22 | 22 | 24 | 24 | 25 | 26 | 28 | 29 | 29 |
| CSkFont | 0 | 0 | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CSkpStyle | – | – | – | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 2 |
| CSkpStyleManager, CWatermarkManager | – | – | – | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| CText | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 | 9 |
| CTextStyle | 4 | 4 | 4 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 | 5 |
| CTexture | 4 | 4 | 4 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| CThumbnail | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| CViewPage | 6 | 9 | 9 | 11 | 11 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 12 | 13 |

Schéma 0 partout : CConstructionGeometry, CConstructionPoint, CDefinitionList, CFontManager,
CPolyline3d, CRelationship, CRelationshipMap, CSketchCS, CVertex, CSchemaFilterFile.

### 3.3 Le modèle (`CSketchUpModel`), schéma M

Lu après la table des versions, dans cet ordre :

1. M > 6 : u32, u32 ; M > 0x11 : u32 ; M ≥ 0x1A : u64.
2. M > 3 : obj (aperçu, `CDib`), bool.
3. `CComponentBehavior` en place ; str description.
4. M ≥ 0x15 : `COptionsManager` en place ; obj : le `CAttributeContainer` du modèle ; obj.
5. Caméra : M ≤ 10 en place au format ancien, sinon obj `CCamera` (§3.4 ; le choix est un
   argument passé par registre, `R8B = M <= 10`, non visible au premier abord).
6. `CRenderingOptions` en place.
7. u32 n + n obj (ignorés) ; M ≥ 0x11 : u32.
8. Le `CComponent` racine en place : ses gestionnaires de matières, d'étiquettes et sa liste
   de définitions sont ceux du modèle.
9. M > 1 : `CShadowInfo` en place.
10. Scènes : M < 0xC : u32 n + n obj `CViewPage` ; sinon `CPageList` en place.
11. M < 6 : u32 n + n str ; M < 3 : `CMaterialManager` en place.
12. `CSketchCS` en place (axes de dessin) ; 16 × u32.
13. 5 ≤ M ≤ 20 : `COptionsManager` en place (une deuxième fois pour 6 ≤ M ≤ 20).
14. M > 7 : `CDimensionStyle` en place.
15. M ≥ 9 : (M ≥ 10 : `CTextStyle` en place) ; `CFontManager` en place ; M ≥ 0x1D :
    `CLineStyleManager` en place ; si M ≥ 0x1D, M = 0xD ou M ≥ 0xF : obj (`CBackgroundImage`,
    photo adaptée).
16. M ≥ 0xE : `CSkpStyleManager` en place ; M > 0xF : `CWatermarkManager` en place ;
    M = 0x13 : bool ; M ≥ 0x14 : u32 ; M ≥ 0x16 : bool ; M ≥ 0x17 : u32 n + n obj
    (classifications, `CSchemaFile`) ; M > 0x1B : u32.

L'archive se termine là (le lecteur vérifie qu'il ne reste aucun octet : `skp_dump --check`).

### 3.4 Classes

S = le schéma de la classe (table des versions). « → » : champ gardé par Esquisse.

**Entités**
- `CEntity` : S = 1 : u32 ; S < 3 : rien ; sinon obj → conteneur d'attributs ; S = 4 : u64 id ;
  S ≥ 5 : id compact.
- `CDrawingElement` : CEntity ; S ≠ 2 : (S > 4 : obj → matière ; sinon couleur, dont SketchUp
  fait une matière) ; S = 0 : aller en B. A : bool → caché ; S ≥ 6 : bool → projette, bool →
  reçoit les ombres ; S ≥ 7 : bool → doux ; S ≥ 8 : bool → lisse ; S > 8 : bool → verrouillé ;
  S ≥ 8 : obj → étiquette, fin. B (S < 8) : lisse = doux ; S = 2 : bool, si vrai obj (ignoré),
  fin ; S ≥ 4 : obj → étiquette. Par défaut : projette et reçoit les ombres, étiquette par
  défaut. Drapeaux comme 0x7D3 (§2.3.1).
- `CAttributeContainer` : CEntity ; obj jusqu'à nul : `CAttributeNamed`, `CFaceTextureCoords`.
- `CAttribute` : CEntity ; u32.
- `CAttributeNamed` : CAttribute ; str → nom ; puis (str clé, valeur) jusqu'à une clé vide ;
  S > 0 : u32.
- Valeur : u8 type puis 0, 1 : vide ; 2 : u8 ; 3 : i16 ; 4 : i32 ; 5 : f32 ;
  6, 0xC–0x10 : f64 ; 7 : bool ; 8 : couleur (R, V, B, un octet ignoré ; alpha 255) ; 9 : i32
  heure ; 10 : str ; 11 : i32 n + n valeurs ; 0x11 : point ; 0x12, 0x13 : vecteur ; 0x14 :
  transformation. Les mêmes numéros qu'en 2021+ (tag = 0x38A3 + type, §2.4).
- `CFaceTextureCoords` : CAttribute ; 9 f64 → matrice avant (par colonnes, dans l'ordre de
  0x2715 : les deux formats l'écrivent pareil) ; S ≠ 0 : vecteur → projection
  avant ; S > 1 : matrice arrière, projection arrière ; S > 2 : 2 × (i32 n + n × 4 f64 :
  épingles) ; S ≥ 4 : u32 → drapeaux avant, u32 → arrière (bit 0 positionnée, bit 1 projetée) ;
  S < 4 : positionnée si la matrice n'est pas l'identité.

**Géométrie**
- `CVertex` : CEntity ; point.
- `CEdge` : CDrawingElement ; obj → départ, obj → arrivée (`CVertex`) ; S ∈ {1, 2} : obj → courbe.
- `CEdgeUse` : CEntity ; obj → arête ; bool → inversée ; obj (ignoré) ; S = 0 : obj, obj.
- `CLoop` : CEntity ; bool, bool ; obj → premier `CEdgeUse` ; S ≠ 0 et non nul : obj → suivants
  jusqu'à nul (l'ordre de la boucle).
- `CFace` : CDrawingElement ; plan ; u32 n (≤ 0xFFFF) ; n obj → `CLoop` (l'extérieure d'abord) ;
  S = 1 : u32, si ≠ 0 obj → matière des deux côtés ; S > 2 : obj → matière arrière ; sinon
  arrière = avant.
- `CCurve` : CEntity ; S = 4 : bool → polygone, u32 → segments ; S = 3 : obj, obj, bool, u32 ;
  S = 2 : bool ; S = 0 : obj, obj.
- `CArcCurve` : CCurve ; arc : point → centre, vecteur → normale (unitaire),
  vecteur → axe x (longueur = rayon), f64 → angle de départ, f64 → angle de fin ; S ≠ 0 :
  vecteur → axe y (sinon normale × axe x). Normale retournée si (x × y) · n < 0.

**Composants**
- `CComponent` : CDrawingElement ; S ≥ 9 : `CMaterialManager`, `CLayerManager`,
  `CDefinitionList` en place ; u32 n + n obj → entités ; `CRelationshipMap` en place ; obj →
  plan de section actif ; S = 10 : `CFontManager` en place. S < 9 : u32 n + n obj ; S < 7 :
  transformation ; S ≠ 0 : `CRelationshipMap` ; S > 1 : `CDefinitionList` ; 3 ≤ S < 6 : u32, si
  ≠ 0 obj ; S > 3 : `CMaterialManager` ; S > 4 : `CLayerManager` ; S > 7 : obj → section active.
  Les gestionnaires d'un composant autre que la racine sont lus puis jetés
  (/bb0/b30`). Les sommets, boucles, courbes ne sont pas dans la liste.
- `CComponentDefinition` : CComponent ; GUID → ; S > 2 : str → nom, str → description ;
  S < 4 : u32 n + n str, sinon str (chemin) ; u32 horodatage ; S > 8 : bool ; S > 4 : point ;
  S < 6 : u32 n + n obj ; S > 1 : `CComponentBehavior` en place ; S > 7 : u32 → genre (0
  composant, 1 groupe, 2 image ; avant : 0) ; S > 9 : obj (`CThumbnail`).
- `CComponentInstance`, `CGroup`, `CImage` (le même `Serialize`, au schéma de
  `CComponentInstance`) : CDrawingElement ; obj → définition ; transformation → ; S < 2 : u32 ;
  S ∈ {1, 2} : u32, si ≠ 0 obj ; S > 3 : str → nom ; S > 4 : GUID. Verrouillé : drapeau 0x20.
- `CComponentBehavior` : CEntity ; S < 3 : colle = vrai, bool → découpe, S < 2 : fin, i32 →
  plan ; sinon bool → colle, bool → découpe, i32 → plan ; S > 3 : u8 (bit 0 → toujours face à
  la caméra ; bit 1 → ombres face au soleil, fichiers ≥ 5, vrai pour 5.0 build < 0x97) ;
  S ≥ 5 : i32 → masque d'échelle. Comme 0x1B58 (§2.3.6).
- `CDefinitionList` : u32 n + n obj → définitions (dans l'ordre du modèle).
- `CRelationshipMap` : u32 n + n obj `CRelationship` (CEntity ; obj → ce à quoi le composant est
  collé ; obj → le composant, §10.12).

**Matières, étiquettes**
- `CMaterialManager` : S ≠ 0 : CEntity ; u32 n + n obj → `CMaterial` ; S > 1 : obj
  (courante) ; S = 3 : obj.
- `CMaterial` : CEntity ; str → nom ; S < 3 : bool, si vrai u32 ; bool texture ; si texture :
  S < 4 obj `CTexture`, sinon `CTexture` en place ; 2 ≤ S ≤ 4 : u32 ; S > 5 : bool → possédée
  (couleur d'étiquette, image : hors liste, comme 0x32CA) ; S > 6 : couleur → ; S > 7 : str ;
  S ≥ 9 : i32 → type (0 couleur, 1 texture, 2 colorisée ; avant : 1) ; S ≥ 10 : i32 → type de
  colorisation ; S ≥ 11 : f64 → **transparence** (opacité = 1 − valeur) ; S ≥ 12 : bool →
  utiliser l'opacité (avant : transparence ≠ 0). Sans texture : type 0.
- `CTexture` : S > 4 : CEntity ; S < 6 : (S < 2 : obj image ; sinon bool, si vrai `CDib` en
  place) ; S ≥ 6 : obj `CDib` ; f64, f64 → taille d'un motif (pouces) ; str → nom de fichier ;
  S ≠ 0 : couleur (moyenne de l'image ; pour une texture colorisée, la couleur de
  colorisation, `+0x30` si non nulle, à l'écriture) ; S = 3 : 8 octets.
- `CDib` : S = 0 : rien ; format (S = 1 : 4, sinon u32) ; u32 n + n octets → le fichier image
  d'origine (PNG, JPEG… : ce que rend `SUTextureWriteOriginalToFile`) ; S ≥ 3 et format 1 : u32.
- `CLayer` : CEntity ; str → nom ; bool → caché ; S = 0 : couleur, obj ; sinon `CMaterial` en
  place (sa couleur → couleur de l'étiquette) ; S ≥ 2 : u32 → comportement dans les scènes ;
  S > 2 : obj → style de trait.
- `CLayerManager` : S ≠ 0 : CEntity ; i32 n + n obj → `CLayer` (la première est l'étiquette par
  défaut) ; S < 3 : str (nom de l'active) ; sinon (S = 3 ou n > 0) : obj → active ; S = 6 :
  u32 n + n obj ; S ≥ 7 : `CLayerGroup` racine en place (dossiers).
- `CLayerGroup` (schéma 1 à 3, sinon erreur) : CEntity ; str → nom ; i32 n + n obj →
  sous-dossiers ; i32 n + n obj → étiquettes ; S > 1 : bool → caché ; S ≥ 3 : bool → caché
  dans les nouvelles scènes.
- `CLineStyleManager` (en place) : CEntity ; u32 n + n obj `CCustomLineStyle`.
- `CCustomLineStyle` : CEntity ; S < 3 : CEntity encore (écrit deux fois jusqu'en 2019) ;
  str → nom ; S = 1 : u16, sinon str ; f64 ; S > 1 : f64, puis S < 4 : 8 octets, sinon couleur,
  u8.

**Construction, annotations**
- `CConstructionLine` : CDrawingElement ; S < 1 : point, vecteur (infinie) ; sinon point,
  vecteur, f64 t0, f64 t1 (±1e30 : infinie), u32 (motif). Comme 0x426A.
- `CConstructionPoint` : CDrawingElement ; point → ; point ; bool.
- `CSectionPlane` : CDrawingElement ; plan → ; S ≥ 3 : str → nom, str → symbole.
- `CText` : CDrawingElement ; S = 0 : fin ; S > 3 : obj (`CTextStyle`) ; S < 5 : obj ; f64,
  f64 → position à l'écran ; S < 5 : point attaché, sinon point d'attache (ci-dessous) ;
  S < 6 : vecteur, puis S > 4 : vecteur → flèche, sinon point ; S ≥ 6 : vecteur → flèche ;
  vecteur ; 3 ≤ S ≤ 7 : f64 ; i32 → type de guide (0 aucun, 1 selon la vue, 2 punaise) ; i32 ;
  bool ; bool ; i32 → type de flèche ; S > 1 : bool ; str → texte ; S > 7 : bool ; S > 8 : i32.
- Point d'attache (= 0x5208) : i32 → type (0 : non attaché) ; i32 version ;
  point → ; obj (entité) ; version > 0 : obj ; u32 n + n obj (chemin) ; version ≥ 4 : u32 n + n
  obj.
- `CDimension` : CDrawingElement ; str → texte ; S > 0 : obj (style), bool, i32 → type de
  flèche.
- `CDimensionLinear` : S = 0 : obj, obj (ancien format) ; sinon S ≥ 2 : CDimension ; point
  d'attache → départ, → arrivée ; vecteur → normale ; S < 4 : f64 (angle), sinon vecteur → axe
  x ; u32 ; f64 → décalage ; S ≥ 3 : f64 (S ≥ 5 : → position le long de la cote) ; S > 5 : u32.
- `CDimensionRadial` : CDimension ; obj (arc) ; f64, f64, bool ; arc nul : arc en place (axe y
  si S > 1). Non gardée (comme en 2021+).
- `CPolyline3d` : CDrawingElement ; u32 n + n points.

**Vue, scènes, réglages**
- `CCamera` : point → œil, point → cible, vecteur → haut ; f64, f64 ; bool → perspective ;
  f64 → champ (degrés) ; f64 ; point ; f64 ; bool ; S ≥ 2 : bool ; S > 2 : str ; S > 3 : f64 ;
  S > 4 : bool, 3 × f64. **Format ancien** (en place : modèle M ≤ 10, `CViewPage` S < 7,
  `CThumbnail` S = 0) : point œil, vecteur direction (cible = œil + direction), vecteur haut,
  f64, f64, bool perspective, f64 champ, f64, point.
- `CPageList` (en place) : CEntity ; u32 n + n obj → `CViewPage` ; obj → scène choisie.
- `CViewPage` : (`CSketchUpPage` : CEntity si le schéma de CEntity ≠ 0 ; str → nom ; str →
  description) ; u32 → drapeaux (comme 0x7149) ; &1 : caméra → (en place si S < 7, sinon
  obj) ; &2 : `CRenderingOptions` en place →, S > 9 : obj (style) ; &4 : `CShadowInfo` en
  place, S > 1 : bool (ombres affichées) ; &8 : `CSketchCS` en place, bool ; &0x10 : u32 n + n
  obj (entités cachées) ; S ≠ 0 et &0x20 : u32 n + n obj → étiquettes cachées ; S > 0xE et
  &0x20 : u32 n + n obj (dossiers cachés) ; S = 4 : (&0x40 : obj) bool → animation ; S > 4 :
  (&0x40 : u32 n + n obj) bool → animation ; 2 < S < 4 : bool → animation ; 6 ≤ S ≤ 10 : bool ;
  S ≥ 8 : f64 ; S > 8 : f64 ; S > 9 : obj (photo adaptée) ; S > 0xB : bool, bool image, si
  vrai : bool, si vrai i32 n + n octets.
- `CRenderingOptions`, noms SDK par tag 2021+ (§2.7) : CEntity ; i32 →
  0x733D ; 0x11 ≤ S ≤ 0x15 : i32 (1 : 0x733E, 2 : 0x733F), sinon bool → 0x733E ; S > 0x15 :
  bool → 0x733F ; bool → 0x734B ; S < 0x26 : (S ≤ 8 : bool, sinon i32) → 0x7341 ; sinon bool →
  0x734C, i32 → 0x7341 ; couleurs → 0x7357, 0x7358, 0x7359 ; S ≥ 0xB : couleur → 0x735B, bool
  → ombres affichées ; sinon S ≠ 0 : bool → ombres affichées ; S = 2 : bool → 0x7340 ; S > 3 :
  bool → 0x7349 ; S > 4 : bool → 0x7347 ; S ≥ 6 : bool → 0x7340, i32 → 0x7348 ; S ≥ 7 : bool →
  0x734D, i32 → 0x734E, bool → 0x734F, i32 → 0x7350, (S > 0x19 : bool → 0x7351, i32 → 0x7352,
  bool → 0x7353, i32 → 0x7354, S > 0x1B : bool → 0x7355), (S < 0x27 : un bool pour 0x7380 et
  0x7381, sinon un bool chacun) ; S ≥ 8 : u32 ; S > 9 : couleurs → 0x735C, 0x735D ; S > 0xB :
  f64 → 0x736C, f64 → 0x736D, bool → 0x736E, bool → 0x736F ; S > 0x1C : bool → 0x735F,
  couleur → 0x7360, bool → 0x7361, f64 → 0x7362, f64 → 0x7363 ; S > 0x1D : i32 → 0x7364 ;
  S > 0x1E : i32 → 0x7342, bools → 0x7343 à 0x7346 ; ciel et sol : S ≥ 0x28 : bool → 0x7382,
  couleurs → 0x7365, 0x7366 ; 0xC < S < 0x28 et S ≠ 0xD : couleur → 0x7365, S ≥ 0x18 :
  couleur → 0x7366 ; puis (S > 0xC, S ≠ 0xD) couleur → 0x7367, bools → 0x7368 à 0x736A ;
  S = 0xD : couleurs ciel et sol (gardées si non nulles), 3 × i32 (−1 : inchangé) → 0x7368 à
  0x736A ; S > 0xC : i32 → 0x736B ; S ≥ 0xF : couleurs → 0x7370 à 0x7372, i32 → 0x7374 (borné à
  1–20), u32 → 0x7375, puis S ≥ 0x25 : couleur → 0x7373, bool → 0x7376, i32 → 0x7377 ; S = 0x10 :
  bool → 0x733F ; S = 0x12 : bool → 0x7377 ; 0x12 < S < 0x25 : i32 → 0x7377 ; S > 0x13 : bool →
  0x7379, f64 → 0x737A ; S > 0x14 : bool → 0x737B ; S ∈ {0x17, 0x18} : bool ; S > 0x1A :
  couleur → 0x735A ; S > 0x1F : bool → 0x735E ; S = 0x21 : bool, fin ; S > 0x22 : f64 →
  0x7378 ; S > 0x23 : bool → 0x7356, bool → 0x737C, f64 → 0x737D, bool → 0x737E, f64 → 0x737F.
  Les options qu'un schéma ne contient pas gardent la valeur du constructeur (
  table `rendering_defaults` de `native/legacy.rs` ; ex. horizon 0 0 0 0, verrouillé rouge,
  ciel (140, 165, 255), sol (63, 147, 63) : fractions × 255 tronquées).
- `CShadowInfo` : CEntity ; i32 → heure (0x6591) ; bool → heure d'été ; str → ville ; str →
  pays ; S ≠ 0 : f64 → longitude, f64 → latitude, f64 → fuseau ; S > 1 : vecteur → nord, bool →
  nord affiché ; S = 3 : bool ; S > 4 : bool → sur toutes les faces, bool → au sol, bool →
  arêtes, i32 → clair, i32 → sombre ; S > 5 : bool → soleil pour l'ombrage. « Ombres
  affichées » n'y est pas : c'est le booléen de `CRenderingOptions`.
- `CSketchCS` : CDrawingElement ; point → origine ; vecteurs → axes x, y, z.
- `COptionsManager` (en place) : u32 version ; u32 n ; n × (str fournisseur ; (str clé, valeur)
  jusqu'à une clé vide). Version < 3 : `UnitsOptions/LengthUnit` vaut aussi `AreaUnit` et
  `VolumeUnit`.

**Parcourues sans être gardées**
- `CThumbnail` : CEntity ; caméra (en place si S = 0, sinon obj) ; obj `CDib`.
- `CFontManager` (en place) : CEntity ; u32 n + n obj `CSkFont` (CEntity ; str ; bool ; bool ;
  u32 ; S ≠ 0 : bool, f64).
- `CTextStyle` (en place ou obj) : CEntity ; S = 3 : str, u32, bool, bool, str ; sinon obj
  (`CSkFont`) ; u32, u32, bool, u32, bool ; S ≥ 2 : couleur ; S ≥ 5 : couleur, obj.
- `CDimensionStyle` (en place) : CEntity ; 4 octets ; S > 0 : obj ; bool, bool, u32 ; S > 2 :
  u32 ; u32 ; i32 → flèches ; S > 2 : u32 ; bool ; couleur ; bool ; bool ; f64 ; bool ; f64 ;
  S > 1 : couleur → des cotes, couleur ; S ≥ 4 : u32.
- `CSkpStyleManager` (en place) : CEntity ; i32 n ; n < 1 : fin ; n obj `CSkpStyle` ; obj ;
  obj ; S > 1 : bool.
- `CSkpStyle` : CEntity ; GUID ; str nom ; propriétés : u32 version ; str ;
  str ; version 1 : bool, si vrai `CDib` en place ; i32 n ; n × (u32 clé ; clé 0x3F5 : image
  (i32 version ; < 2 : 2 × u32, sinon 2 × u64 ; u32, bool, u32, u32 ; i32 n +
  n × u32 ; i32 n + n obj) ; clé 0x1389 : version > 2 : u32 n + n obj ; autre clé : valeur
  (u32 ignoré, u32 type ; taille : 0 → 0, 1–2 → 1, 3 → 2, 4–6 → 4, 7 et
  0xF → 8, 9–0xB → 24, 0xC → 104, 0xE → 4)).
- `CWatermarkManager` (en place) : CEntity ; S < 2 : i32 n + n obj ; i32 n + n obj
  `CWatermark` (CEntity ; bool ; str ; i32 ; u32 ; 5 × bool ; f64 ; f64 ; str ; obj `CDib`).
- `CBackgroundImage` : CEntity ; S < 2 : str ; sinon str, (S < 5 : bool, sinon i32 genre), obj
  image, S ≥ 3 : 3 × u32, S ≥ 4 : i32 ; bool ; f64 ; u32 n + n points ; S > 5 : vecteur ; S > 6 :
  f64 ; S = 8 : u32, i32 n, si n > 0 : n × 2 points, fin ; S = 0 : bool, si vrai `CDib` en
  place ; S = 1 : obj ; S ≥ 10 : u32.
- `CSchemaFile` : str ; str ; i32 n + n octets ; u32 n + n obj ; u32 n + n obj ; S ≠ 0 : bool,
  si vrai obj (`CSchemaFilterFile` : str ; i32 n + n octets). `CSchemaZipFile` : str ; str ;
  i32 n + n octets.

### 3.5 Ce que SketchUp change au chargement

Le SDK rend le modèle après ces corrections ; le lecteur natif les reproduit (sinon il
différerait de l'oracle), l'écrivain doit savoir qu'il ne les retrouvera pas en relisant :

- un objet n'a qu'un propriétaire : une entité n'entre dans une collection que si elle n'a
  pas encore de parent (`CComponent::Serialize`, `plVar4[3] == 0`) ; le lecteur applique la
  même règle aux conteneurs, dictionnaires, boucles, dossiers et listes du modèle (§6) ;
- scènes : drapeau 0x200 ajouté ; les étiquettes cachées sont une liste dès le drapeau 0x20 ;
  géométrie cachée (0x10) : ses deux sous-drapeaux 0x80 (objets cachés) et 0x100
  (géométrie cachée) mis tous deux s'il n'y en a aucun, ôtés sans 0x10
  (`CViewPage::Serialize`) ;
- courbes (après la lecture du composant racine, sur les courbes rencontrées
  pendant la lecture) : chaque courbe est parcourue depuis sa première arête lue (la courbe
  est lue en place à la première arête qui la cite : le plus petit index de l'archive), vers
  l'arrière puis vers l'avant le long de ses arêtes (un sommet où trois
  arêtes de la courbe se rencontrent arrête le parcours). Une courbe dont le parcours n'atteint
  pas toutes ses arêtes est **dissoute** : ses arêtes n'ont plus de courbe. Le parcours fixe
  la première et la dernière arête : les deux bouts d'une courbe ouverte ; pour une courbe
  fermée, la première arête rencontrée vers l'arrière dont le sommet arrière porte une autre
  arête, non adoucie, d'aucune courbe (ou d'une autre), sinon la première arête lue. Le sens
  d'une arête le long d'un arc : celui des angles de ses sommets (à π près).
  Sans première et dernière arêtes dans le fichier (`CCurve` de schéma ≠ 3), le repère et les
  angles d'un arc sont recalculés depuis ses sommets (`CArcCurve`, emplacement 25 de la vtable,
   appelé avec (1, 0)) : axe x tourné vers le premier sommet (au rayon), normale
  rendue perpendiculaire à cet axe (n' = d × unité(n × d)), axe y = n' × x, angle de départ 0 ;
  angle de fin = celui du dernier sommet (atan2((x × d)·n, x·d) ramené dans
  [0, 2π]) ou, pour un arc fermé, **0 si son premier sommet porte deux arêtes** (un cercle
  seul), **2π sinon** (le bout d'un cylindre) — quels que soient les angles du fichier (vérifié
  sur un cercle seul écrit par le SDK et par l'écrivain natif dans les 15 versions : le SDK lit
  une fin à 0 de SketchUp 3 à 2020, à 2π en 2021+, comme le lecteur natif). Un arc
  elliptique (axes x et y de longueurs différentes ou non perpendiculaires : un cercle mis à
  l'échelle) est recalculé dans le repère où il est un cercle unité — celui de ses axes x, y et
  de sa normale, depuis son centre — puis ramené (axes par [x y n], normale par l'inverse
  transposée). Avec une dernière arête dans le fichier (schéma 3), rien n'est recalculé ; si
  le parcours finit ailleurs qu'à cette arête, il est refait depuis elle et doit revenir à la
  première, sinon la courbe est dissoute. Les fichiers 2021+ gardent leurs première et dernière
  arêtes : pas de parcours (il dissoudrait des cercles fermés que SketchUp garde) ;
- fichiers d'avant 7.1 : un côté de face sans matière prend celle de l'autre côté si elle est
  transparente ou si sa texture a des pixels non opaques ;
- matières : couleur nulle → (64, 64, 64) ; texturée (type 1) : couleur = moyenne de la
  texture si S < 12 ou couleur nulle ; colorisée et S < 13 : moyenne **recalculée depuis
  l'image** (somme de chaque canal sur tous les pixels / nombre de pixels, tronquée)
   valeurs PBR : facteurs 1, style de normale 1 ;
- SketchUp 3 (`CDimension` absent ou 0) : le style de cote du modèle s'applique aux cotes:
    flèches, et couleur → une matière de couleur (une existante sans texture
  de même couleur, sinon une nouvelle nommée « * ») ;
- définitions de même nom (sans tenir compte de la casse : `towupper`) :
  chacune après la première, dans l'ordre de la liste, prend le nom que fait
  `CDefinitionList::GenerateUniqueName` : un « #chiffres »
  final ôté, puis le nom lui-même s'il est libre, sinon le nom sans ses chiffres finaux suivi
  de « #1 », « #2 »… le premier libre (« AI9-BLK2 » en double → « AI9-BLK#1 ») ; observé sur
  des fichiers de SketchUp 5 (le format 2021+ n'en a montré aucun) ;
- plans de section sans nom (schéma < 3, avant SketchUp 2018) : « Section n » et « n », ceux
  du modèle d'abord puis ceux de chaque définition dans l'ordre de la liste des définitions
  (pas dans l'ordre de l'archive, où les définitions viennent avant les entités du modèle) ;
- définitions de schéma < 11 (avant 2018) : dictionnaire vide des attributs avancés
  `SU_DefinitionSet` {Price, Size, Url} aux composants ; toute instance d'un composant (pas
  d'un groupe ni d'une image) qui n'a pas `SU_InstanceSet` reçoit {Owner, Status} vides
  (`CComponentInstance::Serialize`), quelle que soit la version ;
- noms des imports Google Earth (`CAttributeNamed`, `CLayer`)
   les étiquettes, noms et clés de dictionnaire « Google Earth Snapshot » et
  « Google Earth Terrain » deviennent « Location Snapshot » et « Location Terrain » ;
- dictionnaire `GeoReference` du modèle (géolocalisation d'un ancien SketchUp) : les clés
  absentes sont ajoutées (fin de `CSketchUpModel::Serialize`) :
  `UsesGeoReferencing` vrai, `LocationSource` « Google Earth », `Longitude` et `Latitude` des
  ombres ;
- gestionnaire d'options : les valeurs d'un fournisseur que SketchUp n'a plus (`AxisOptions`
  de SketchUp 3 à 6 ; restent `UnitsOptions`, `PageOptions`, `SlideshowOptions`,
  `NamedOptions`, `PrintOptions`) sont lues puis laissées, sauf
  `AxisOptions/DisplayAxes` d'un gestionnaire de version < 2 : il devient `DisplaySketchAxes`
  des options de rendu du modèle. Une scène (`CViewPage`, drapeau 0x08) garde après son repère
  d'esquisse un booléen : son propre `DisplaySketchAxes` ;
- `CComponent::Serialize` : un composant (définition, instance, groupe) qui a
  deux dictionnaires « 3DWarehouse » ou « TempShadowInfo » ne garde que le dernier lu ;
- polices des textes et des cotes qui ne sont pas dans la liste des polices du modèle
  (SketchUp 8) : `SUModelGetFonts` les rend après celles de la liste, dans l'ordre où les
  annotations les citent ;
- modèle de schéma < 0x15 (SketchUp 3 à 6, sans dictionnaires du modèle) :
  `GSU_ContributorsInfo` {LastModifiedByKey "", VersionKey 1000} ;
- le dictionnaire `ModelProperties` du modèle donne nom et description et n'est pas rendu
  comme dictionnaire ;
- options de rendu absentes du schéma : valeurs du constructeur (§3.4) ; `UnitsOptions` d'un
  gestionnaire de version < 3 : voir §3.4.

Dans les deux formats (voir aussi §9) :

- points d'attache des textes et des cotes (0x5208) : SketchUp garde
  l'entité et le chemin d'instances, et calcule le point au chargement selon
  le type : 1 le point du fichier ; 2 un sommet ; 3 un point de construction ; 4 le milieu
  d'une arête ; 5 un point d'une arête (à la fraction x du point du fichier) ; 6 un point
  d'une ligne de construction (paramètre x, sur le point et la direction qu'elle garde) ; 7 un
  point d'une face (fractions x, y de l'étendue de sa boucle extérieure, dans le repère de sa
  première utilisation d'arête : origine à son départ, x le long d'elle, z la normale,
   mesurées depuis cette origine) ; 10 le centre d'un arc ; 11 le point projeté
  sur un plan de section ; 9 l'intersection de deux entités, chacune déplacée par son chemin
  (deux lignes : le milieu de leurs points les plus proches, à 0,001" près ; une ligne et un
  plan : leur point commun) ; 8 un point d'une image (fractions x, y de sa taille — un motif de
  sa texture, en pouces —, par sa transformation) ; 12 et 13 le point du fichier, ou avec un
  chemin l'origine de ses instances ; 0 : l'origine. Sauf 9, 12 et 13, le point est ensuite
  déplacé par les transformations du chemin, la plus extérieure d'abord. Le point écrit dans le
  fichier est souvent (0, 0, 0) ;
- ce qu'Esquisse garde de ces attaches (`doc::Connection`, `build::attach_annotations`,
  `esq_core::attach`) : l'entité et le chemin par les clés qu'ils ont reçues, et le paramètre
  recalculé depuis le point placé par le lecteur (types 2 à 7, 10, 11, 12 et 13 avec un
  chemin) ; une intersection (9), une image (8), un point libre restent libres. Corpus réel
  (`tests/attaches.rs`, `real_files_keep_their_attaches`) : 179 fichiers, 4 765 bouts attachés
  (type 2 : 4 405, 4 : 106, 5 : 81, 10 : 67, 7 : 48, 3 : 32, 12 : 20, 6 : 5, 8 : 1), tous
  gardés (4 759) sauf l'image et 5 centres d'arcs dont l'arc n'a plus d'arête dans le modèle lu
  (courbe dissoute au chargement, ci-dessus, ou sans arête), dans 5 fichiers. Une cote
  radiale, sans point d'attache dans le fichier, est attachée à son arc (centre et point) ;
- ce que l'écrivain en écrit (`plan::Conn`) : le type, le paramètre (5 : la fraction depuis le
  départ de l'arête écrite ; 6 : les pouces le long de la ligne écrite ; 7 : les fractions dans
  le repère de la face écrite), l'entité et le chemin par leurs ids ; en MFC, l'entité et les
  instances sont déjà dans l'archive (la géométrie, les instances et les définitions qu'elles
  placent précèdent textes et cotes dans `CComponent`), sinon le point est écrit libre. Relu par
  le SDK dans les 15 versions : mêmes entités, mêmes chemins, mêmes points ;
- le SDK (`SketchUpAPI.dll` 26.2) numérote `SURefType` avec deux types de plus avant `Face`
  que les en-têtes anciens : arête 6, face 11, sommet 26, point de construction 28, ligne de
  construction 29. Une cote faite par `SUDimensionLinearCreate` avec des chemins avant d'être
  dans une collection est détachée en y entrant, ses bouts à l'origine, puis
  effacée ; le SDK ne calcule un point qu'à la demande : un bout détaché sans
  qu'on l'ait demandé avant garde (0, 0, 0) ;
- Check Validity passe sur le modèle à l'ouverture et
  corrige ce qu'il trouve (mode 1). Arcs (`CArcCurve`, emplacement 16,
   `native/curves.rs`) : dissous si leur sommet de départ ou de fin n'est pas
  là où le rayon issu du centre vers le point de l'angle de départ (de fin) coupe la chaîne de
  leurs arêtes (à 0,001" près), si leur axe x n'a pas la longueur du rayon, si
  leur plan n'est pas celui du centre et de x × y (sans orientation) ou si cette normale est
  nulle ; et, constaté sur les fichiers réels sans que la fonction soit trouvée, un arc ouvert
  dont l'angle de fin est inférieur à celui de départ (fichiers 2021+, qui gardent les angles
  écrits : `MuPiBox`) — un cercle fermé de 0 à −2π reste (`vckb`). Faces (`CFace`,
  emplacement 16, `native/faces.rs`) : une boucle de moins de 3 arêtes, ou
  dont les arêtes ne se suivent pas (une utilisation donnée deux fois), est ôtée (intérieure) ou fait effacer la face (extérieure) ; une boucle qui tourne dans le
  mauvais sens autour de la normale (aire signée depuis son premier sommet, :
  l'extérieure positive, les intérieures négatives) est retournée (même
  première utilisation, sens et drapeaux inversés), ou, pour une face d'une seule boucle, c'est
  le plan qui l'est ; un plan non fini fait effacer la face ; un sommet à 0,001" ou plus du
  plan : plan des moindres carrés des sommets de la boucle extérieure (
  le plus grand des trois déterminants 2×2 de la covariance, refusé sous
  1e-10), orienté par la boucle ; s'il n'y en a pas ou s'il ne tient pas tous les sommets,
  SketchUp découpe la face et efface les morceaux de moins de 0,001 pouce
  carré : une face aussi petite est effacée, le découpage d'une plus grande
  n'est pas refait (la face reste) ;
- unités (`SUModelGetUnits`) : le format décide d'abord (architectural et fractionnaire :
  pouces ; ingénieur : pieds), l'unité du fichier ensuite (décimal). L'écrivain garde format,
  précision et unité lus quand ils donnent l'unité du modèle ;
- taille d'une image en pixels (`SUImageGetPixelDimensions`) : celle d'un motif de la texture
  de sa matière, en pouces, tronquée — pas celle de l'image (elles diffèrent quand l'image a été
  remplacée : un BMP de 384 pixels dans un motif de 386") ;
- transformations d'instance : le 13e nombre est une échelle qui **multiplie** le point
  transformé ; `SUTransformation`, qui le divise, en garde l'inverse. À la lecture
  (en MFC comme en 2021+) : presque
  l'identité (partie 3×3, translation et échelle à moins de 0,001) devient
  l'identité, translation nulle, échelle gardée ; presque une rotation (lignes de longueur 1
  et perpendiculaires à 0,001 près) en devient une exactement : l'identité si
  la partie 3×3 l'est à 1e-10 près, sinon les axes (colonnes) rendus unitaires puis
  y = unité(z × x), x = unité(y × z) — pour un miroir x = unité(z × y), y = unité(x × z) ;
- caméras lues comme objet (`CCamera`, 0x34BC ; `SetOrientation`) : la
  cible n'est pas plus près de l'œil que le dixième de la plus grande de leurs distances à
  l'origine (elle est repoussée le long de la visée). Une caméra au format ancien (œil et
  direction, en place, §3.4) garde cible = œil + direction ;
- une scène qui ne garde pas sa caméra (drapeau 0x01) a celle que SketchUp crée par défaut :
  œil à 1" au-dessus de l'origine, visée vers le bas, y en haut, perspective, 30°.

Non reproduit (aucun fichier du corpus) : l'heure des ombres des schémas < 7 ramenée au fuseau
local, la matière que SketchUp fait d'une couleur d'élément (`CDrawingElement` S ≤ 4).

## 4. Défauts du SDK (à ne pas reproduire)

- `SUEntityGetAttributeDictionaries(n)` parcourt les n premiers éléments du conteneur,
  dictionnaires ou non, et range chaque dictionnaire à l'index de son élément : les
  dictionnaires d'une face qui suivent son placement de texture sont perdus. Le lecteur natif
  les garde ; `native::Options::sdk_quirks` reproduit le défaut pour la comparaison.
- `SUCameraGetPerspectiveFrustumFOV` échoue sur une caméra parallèle.
- Valeurs d'attribut : un point (variante 17) et un vecteur unitaire (19) sont rendus comme
  des vecteurs 3D ; un tableau vide d'un dictionnaire de SketchUp 3 à 2020 n'est pas rendu
  (sa clé disparaît). Le lecteur natif garde le point (le vecteur unitaire reste un vecteur)
  et le tableau ; `sdk_quirks` reproduit le SDK.

## 5. Ce que le lecteur natif ne lit pas encore

Archives MFC (3–2020) : toutes les classes rencontrées sont parcourues (61 sur 61 dans le
corpus, `class_coverage`) et gardées (§10), sauf `CSchemaFile` (le schéma XSD d'une
classification de SketchUp 2014 : son nom est lu, pas converti en `.skc`).

Format 2021+ : toutes les classes que SketchUp 2026 écrit sont lues et réécrites (§10), sauf
celles de fonctions désactivées dans SketchUp 26.2 et absentes des corpus : images d'avant-plan
(0x213 → 0x7D64 → 0x7D6E, scène 0x715C ; fonction « ForegroundImage » ; 283 fichiers réels ont
un gestionnaire vide, réécrit vide), 0x215 (« ProceduralComponents »),
0x216 et la définition 0x1586 (« ModelingGrid »), étiquettes de
classification (0x217, « ClassificationTags »), lumières (0x13A3 → 0x7E5E, « LightElement »).
Le comportement par défaut du modèle (0x20E) est le même dans les 454 fichiers 2021+ du
corpus, réécrit tel quel.

Lus par le lecteur natif mais pas par le SDK : ancrages (0x7530) et espaces (0x7E2C, avec
leur contour et leurs trous), §8.4 ; avec `sdk_quirks`, le lecteur natif les laisse aussi
(le SDK n'en liste aucun, quel que soit l'auteur du fichier). Les chemins d'instances (0x53FC)
sont lus pour les points d'attache (§3.5).

## 6. Fichiers hostiles (limites du lecteur natif)

- Archive : taille déclarée d'une entrée ≤ 2 Gio et ≤ 1032 × sa taille compressée (le
  maximum de deflate) ; chaque entrée lue par son nom est décompressée une seule fois (cache),
  et l'archive entière ne peut décompresser plus de 100 × sa taille + 256 Mio ; CRC vérifié.
- `material.xml` : lu et analysé une fois par dossier, les images partagées (`Arc`) : des
  milliers de matières qui nomment le même dossier ne coûtent qu'une lecture.
- Enregistrements : aucune allocation d'après une longueur lue avant de l'avoir vérifiée ;
  valeurs typées et tableaux imbriqués sur 64 niveaux au plus, dossiers d'étiquettes 32.
  Les autres lectures ne s'appellent pas elles-mêmes (définitions et collections sont des
  listes plates, liées par ids).
- Construction du modèle : récursive sur les définitions imbriquées, sur un fil dédié de
  256 Mio de pile (réservée, non engagée) ; 4 096 niveaux au plus (10 000 mesurés sans
  dépassement), une définition déjà ouverte n'est pas rouverte (cycles) ; une panique du fil
  devient une erreur. Temps linéaire : les noms rendus uniques (« Groupe#n » des groupes, noms
  des images et de leurs matières) viennent d'un index des noms pris et du prochain numéro à
  essayer par base, pas d'un parcours du modèle par numéro essayé (qui était cubique : 150 s
  pour 3 000 groupes imbriqués) ; la texture d'une définition d'image est cherchée une fois.
- Budget par fichier (`native::Budget`, un seul pour tout le chargement d'un fichier) :
  octets produits ≤ 32 × la taille + 64 Mio (les fichiers réels en font ~10 ×), pixels
  décodés ≤ 16 Mpx + 32 par octet, 256 Mpx au plus. Au-delà des octets : refusé ; au-delà des
  pixels : les images ne sont plus décodées. Les copies d'un `material.xml` partagé (2021+) et
  toutes les copies du lecteur MFC sont comptées.
- Archive MFC : chaque compte et chaque longueur lus sont vérifiés contre ce qui reste du
  fichier avant toute allocation ou boucle ; un objet référencé plusieurs fois n'est donné
  qu'à un propriétaire (§3.5), si bien qu'un objet partagé n'est jamais copié par référence ;
  imbrication des objets ≤ 20 000 et pile mesurée (adresse d'une variable locale) ≤ 224 Mio
  du fil de 256 Mio ; 10 000 classes au plus ; table des index de 8 octets par entrée ; objets
  du lecteur de 32 octets au plus (les gros en boîte). Images : taille lue dans l'en-tête,
  ≤ 16 384 de côté et 64 Mpx, pixels pris au budget avant tout décodage, 256 Mio d'allocation
  au plus par image, résultat gardé par image.
- Dictionnaires d'une archive MFC : un dictionnaire n'appartient qu'à un conteneur et un
  conteneur qu'à une entité ; leurs valeurs y sont déplacées, pas copiées (comptées une fois,
  à la lecture). Un vrai fichier de SketchUp 7 (`Skyline.skp`, 1,8 Mo : sept définitions
  portant chacune un tableau `serial_data` de quelques milliers de valeurs) dépassait le budget
  quand chaque conteneur recopiait ses dictionnaires.
- Tests : `tests/native_robust.rs` (troncatures, corruptions, imbrication de 100 000, 3 000
  groupes imbriqués), `tests/native_budget.rs` (fichiers fabriqués pour copier, décoder ou
  allouer : pic de mémoire mesuré par un compteur d'allocations), tests de `native::build`
  (travail linéaire prouvé par des compteurs : collections construites, noms essayés) et
  `tests/native_mutations.rs` : mutations déterministes (graine fixe, tirée du contenu de
  chaque fichier) des fichiers réels — troncature, 1 à 8 octets inversés (complément),
  ordre de 2 à 16 octets renversé, bloc de 1 octet à 64 Kio recopié ailleurs, tailles et
  décalages du ZIP faussés (2021+), `model.dat` abîmé derrière le CRC (longueurs
  d'enregistrements gonflées, octets inversés, blocs retirés ou doublés : 2021+), comptes et
  longueurs de l'archive MFC gonflés (3–2020). Chaque cas doit rendre `Ok` ou `Err` : ni panique (crochet de
  panique), ni plantage, ni boucle (chien de garde : 20 s + 20 fois la lecture du fichier
  intact), mémoire sous le budget (compteur d'allocations). Mesuré le 17/10/2026 en
  `--release` sur les 1 969 fichiers du corpus réel et ceux du dépôt : **296 786 cas**
  (103 159 lus, 193 627 refusés), **aucun problème** ; pointe la plus haute 29,4 % du budget,
  cas le plus long 3,7 s. Le test court (fichiers du dépôt, 28 cas chacun) tourne toujours.

## 7. Outils

- `cargo run -p esq-skp --example skp_corpus -- [dossier]` : le corpus (un petit modèle par
  type d'objet, dans les 15 versions que le SDK sait écrire), dans
  `%USERPROFILE%\tools\skp-corpus\gen\<version>\`.
- `cargo run -p esq-skp --example skp_dump -- fichier.skp [entrée|--extract dossier]` :
  l'arbre des enregistrements de `model.dat` ; pour une archive MFC, la table des versions et
  le résultat de la lecture (octets restants, ou l'erreur et les octets autour) ;
  `--batch dossier fichiers…` ; `--check fichiers…` : une ligne par fichier (lu, octets
  restants). `ESQ_SKP_TRACE=1` : chaque objet MFC et chaque étape du modèle, avec sa
  position, sur la sortie d'erreur.
- `cargo test -p esq-skp --test native_oracle -- --nocapture` : comparaison au SDK sur tout le
  corpus et couverture ; `ESQ_ORACLE_DIFF=classe,classe` montre les différences.
- `powershell -File tools\skp-reel.ps1 [dossier]` : télécharge le corpus de fichiers réels
  (§9) d'après `crates/esq-skp/tests/data/skp-reel.json` (fichiers absents ou dont le SHA-256
  diffère) dans `ESQ_SKP_REEL` ou `%USERPROFILE%\tools\skp-reel`.
- `cargo test -p esq-skp --test native_reel -- --nocapture` : comparaison au SDK des fichiers
  réels de `tests/data/reel` et des 4 plus petits de chaque version du corpus ;
  `cargo test --release -p esq-skp --test native_reel -- --ignored --nocapture` : tout le
  corpus et le compte par famille de versions (`ESQ_REEL_LIST=1` : une ligne par fichier,
  `ESQ_REEL_FILTER=texte`, `ESQ_REEL_FILES=liste`, `ESQ_ORACLE_DIFF=classe`).
- `cargo test -p esq-skp --test native_mutations` : mutations déterministes (§6) des fichiers
  de `tests/data` ; `cargo test --release -p esq-skp --test native_mutations -- --ignored
  --nocapture` : tout le corpus réel (`ESQ_MUT_CASES` : cas au plus par fichier,
  `ESQ_MUT_FILTER` : fichiers dont le chemin contient le texte).

## 8. Écriture (`native::write`)

`native::write::write(modèle, vue, version, extras)` : le modèle devient d'abord les objets de
SketchUp (`plan.rs`, comme l'écrivain du SDK les fait : ids, tags, matières des étiquettes,
courbes, placements de textures, images, styles), puis le fichier de la version demandée :
`vff.rs` (2021+) ou `legacy.rs` (archive MFC de SketchUp 3 à 2020). C'est le chemin de
`esq_skp::export_skp*` (Fichier › Enregistrer, Enregistrer sous, Enregistrer une copie,
Ruby `Model#save`/`save_copy` avec leur version, `ComponentDefinition#save_as`/`save_copy`,
Composants › Enregistrer sous) ; le SDK n'écrit plus qu'avec `ESQ_SKP_SDK=1`
(`sdk_export_skp_version`, l'oracle des tests). Le fichier est écrit à côté puis renommé : un
échec laisse l'ancien intact. Toute valeur non finie, toute définition qui se contient est une
erreur propre ; le reste (noms vides, unicode, matière sans image ou à image illisible,
placage dégénéré, 300 niveaux d'imbrication…) s'écrit et se relit
(`strange_models_give_an_error_not_a_panic`). Le plan et l'archive MFC sont récursifs sur les
définitions imbriquées : ils tournent sur un fil dédié de 256 Mio de pile (`WRITE_STACK`), et
l'imbrication est bornée à 4 096 niveaux (`MAX_NESTING`, erreur au-delà). Depuis un fil de
1 Mo, en débogage, 256 niveaux s'écrivent et se relisent dans les 15 versions, 4 096
s'écrivent, 4 097 sont refusés (`deep_nesting_is_written_from_a_small_stack`).

### 8.1 Versions

`SkpVersion` = les 15 valeurs de `SUModelVersion` (SketchUp 3 à 8, 2013 à 2020, et 2021 =
le format courant) ; `SUModelSaveToFileWithVersion` refuse toute valeur ≥ 15.
Ruby : `Sketchup::Model::VERSION_3` = 3 … `VERSION_2021` = 17 (`SkpVersion::from_ruby`).
La boîte Enregistrer sous de SketchUp 2026 (`SaveFileWidget`) ne propose que
« SketchUp Models (*.skp) » (sa liste de types, n'a que cette entrée)
 les versions antérieures ne s'atteignent que par Ruby et le SDK. Esquisse
les propose toutes dans sa boîte (`skpsave.rs`).

### 8.2 SketchUp 2021+ (`vff.rs`, `zip.rs`, `xml.rs`)

- En-tête : `CString` « SketchUp Model », `{26.2.243}`, marqueur `VFF 08 00 01 00 11 00`,
  puis la somme : CRC-32 (zlib) de [¬crc des entrées d'index pair] ++ [crc − 0x2A (s'il dépasse
  0x2A) des entrées d'index impair], crc des entrées dans l'ordre de l'archive, petit-boutiste.
- ZIP comme SketchUp : en-tête local version 45, drapeaux 6, deflate (niveau 6), heure 0,
  date 0x0020, champ ZIP64 (id 1, 16 octets nuls) ; répertoire central version 0 / 20 ;
  ZIP64 complet au-delà de 4 Gio. Ordre des entrées : vignettes, `meta/meta.dat`, matières
  (`material.xml` puis images), styles, `model.dat` en dernier.
- `meta.dat` : version, GUID, références des vignettes (0x2328 › 0x2329/0x232A) quand il y en
  a, nom, description, unité, historique (« SketchUp », heure).
- `model.dat` dans l'ordre des écrivains de SketchUp.exe ; ids : 1 style,
  2–3 polices (Arial, Tahoma), 4 Layer0 et 5 sa matière, 6–17 les 12 styles de ligne, puis à
  la suite. Structure identique à celle du SDK (toutes les suites d'enregistrements), sauf la
  boîte des définitions qu'il ne met pas.
- `material.xml` comme SketchUp (`%.17g`/`%.8g` du C, `pbrMR` toujours) ; `style.xml` :
  le style par défaut, comme le SDK (les options de rendu du modèle sont dans `model.dat`).
- Vignette : `meta/model_thumbnail.png` et `meta/preview_thumbnail.png`, 256 × 256 comme
  SketchUp (mesuré sur ses modèles types), rendues hors écran à l'enregistrement par
  l'application (`EsquisseApp::skp_thumbnail` : la vue courante, ou le modèle d'un script) ;
  aucune sans GPU, ni pour `ComponentDefinition#save_as` (pas de vue).

### 8.3 SketchUp 3 à 2020 (`legacy.rs`, `mfc.rs`, `versions.rs`)

Archive `CArchive` : classe nouvelle `FFFF schéma longueur nom` (schéma déclaré = celui de
SketchUp 2026, la table de version donne le vrai), index dès 1, un objet prend son index avant
ses champs, référence `0x8000|index` (au-delà de 0x7FFF : `7FFF` + u32). Table de versions :
celle que SketchUp 2026 écrit pour chaque version (`versions.rs`). Chaque classe suit la
branche d'enregistrement de son `Serialize` au schéma de la version. Ce que SketchUp 2026 fait
en écrivant une version ancienne, reproduit : dossiers d'étiquettes à partir de 2020, styles de
ligne à partir de 2019, `SU_DefinitionSet`/`SU_InstanceSet` retirés avant 2018 (conteneur vide
gardé), pas de dictionnaires ni de placements de textures en SketchUp 3, pas d'étiquettes
cachées dans ses scènes, rendu « monochrome » (6) écrit 2, étiquette sans couleur écrite en
gris 64,64,64, motif des repères 0xFFFF à partir de 2019 (0 avant), images en `CDib` (JPEG
format 1 + qualité 90, PNG format 4, tout autre format converti en PNG), un Layer0 privé par
définition, la vignette du modèle en `CDib`. Ancrages, espaces et lumières n'existent dans
aucune de ces versions (classes absentes des tables) : perdus, comme par SketchUp.
SketchUp 3 garde une courbe par sa première et sa dernière arête et suit les autres à partir
de la fin de la première : une courbe dont la première arête part à l'envers est perdue (par
l'écrivain du SDK aussi) ; Esquisse la liste alors depuis l'autre bout, ou retourne cette
seule arête.

### 8.4 Ce que le SDK ne sait pas écrire

| | Écrit (SketchUp.exe) | Lu (SketchUp.exe) |
|---|---|---|
| ancrage 0x7530 (dans 0x139D) | : élément, 7531 position, 7532 direction, 7533 droite, 7534 haut (3 doubles chacun), 7535 u32 | (liste lue depuis, cas 0x139D) : les mêmes champs aux mêmes places |
| espace 0x7E2C (dans 0x13A2) | : élément, 7E2D nom, 7E2E et 7E2F u32, 7E30 point d'orientation, 7E31 plan du plafond (4 doubles), 7E32 contour (n × 3 doubles), 7E33 › 7E34 un trou chacun | : les mêmes champs aux mêmes places |
| scène sélectionnée 0x6D62 (dans 28000) | : référence de la page | : référence, refusée si l'entité n'est pas une `CViewPage` (type 0x28) |

Le lecteur natif les relit (le contour et les trous des espaces corrigés à cette occasion :
il prenait le premier point de chaque trou pour le contour). Le SDK relit la scène sélectionnée
(`SUModelGetActiveScene`) mais ne liste aucun ancrage ni espace d'un fichier, quel qu'en soit
l'auteur : `SUEntitiesEntityListFill` (`18084e20`) ne connaît que les types 4, 5, 6, 0xB, 0xC,
0xD, 0x14, 0x1C, 0x1D, 0x22, 0x23, 0x2F, 0x32 et 0x33 et rend une liste vide pour les autres.
Les lumières (0x13A3 › 0x7E5E) ne sont lues par SketchUp 2026 que si la fonction
« LightElement » est active : ni écrites ni lues par Esquisse.

### 8.5 Écarts voulus avec l'écrivain du SDK

L'écrivain du SDK change ce qu'il écrit ; l'écrivain natif garde le modèle : sens des arêtes
(le SDK reprend celui des boucles des faces), arcs (le SDK les recommence à l'angle 0 et
l'axe x vers le premier point), polygones (le SDK en fait des arcs), couleurs placées sur les
faces (perdues par le SDK), flèche par défaut des cotes, nom de la texture d'une image (le SDK
écrit son chemin temporaire), une définition par groupe, attributs avancés vides ajoutés
(`SU_DefinitionSet` Price/Size/Url, `SU_InstanceSet` Owner/Status) aux composants faits hors
de SketchUp. Repris du SDK : le haut d'une caméra mis d'équerre avec la visée.

### 8.6 Preuves (`tests/native_writer.rs`, `fidelity.rs`, `roundtrip.rs`, `versions.rs`, `comments.rs`)

- Corpus (`the_corpus_is_written_in_every_version_as_the_sdk_writes_it`) : chaque fichier lu
  nativement est écrit dans une version par les deux écrivains ; les deux fichiers relus par
  le SDK et par le lecteur natif doivent donner, classe par classe, le même modèle, ou le nôtre
  celui d'où il vient (où le SDK perd, §8.5). Toutes les versions sur tous les fichiers avec
  `ESQ_SKP_WRITE_ALL=1` ; par défaut tous en 2021+ et un sur sept dans chaque autre.
  Mesuré le 17/10/2026, 773 fichiers × 15 versions = 11 595 écritures (15 min) : toutes
  ouvertes par le SDK et relues nativement, les 21 classes et la caméra conformes dans les
  11 595 cas, par les deux lecteurs. Tout à fait identiques à la relecture du fichier du SDK
  (sans aucune classe où le nôtre garde ce que le SDK perd) : en 2021+ 688 fichiers sur 773 ;
  dans les autres versions, sur l'échantillon par défaut, 104 à 109 sur 110–111 relus par le
  SDK, 97 à 109 relus nativement.
- Modèle fait dans Esquisse (géométrie, composants sur trois niveaux, matières texturées et
  teintées, placage à la main, étiquettes en dossiers, scènes, attributs, texte, cote, coupe,
  repères, image, courbes, ancrage, espace) dans les 15 versions : le SDK l'ouvre, avec les
  mêmes faces, composants, instances et annotations, et le lit comme le fichier de son propre
  écrivain (aux écarts du §8.5 près).
- Placages (`tests/textures_uv.rs`) : côtés positionnés, projetés selon une direction qui n'est
  pas la normale, déformés (4 paires), projetés et déformés, sur l'envers, sur une face tournée
  vers le bas, écrits dans les 14 versions qui ont des placages : relus nativement et par le
  SDK, le placage est celui de départ à 1e-7 motif près, aux sommets, au centre et hors de la
  face (le long de la normale, et le long de la projection pour un côté projeté).
- Avec MFC : archive identique à l'octet près à celle du SDK (hors GUID, heures,
  ids compacts, doubles recalculés, ordre des polices) pour 644 des 672 fichiers MFC du corpus
  (chacun dans sa version) ; les 28 autres sont `image` et `kitchen`, où le SDK nomme la
  texture d'une image d'après son fichier temporaire.
- Couverture (`coverage_of_sketchups_classes_per_version`, sur le modèle fait dans Esquisse) :
  classes dont les champs sont écrits / classes de SketchUp dans la version (2021+ : les
  classes des écrivains de SketchUp.exe, `vff::CLASSES` ; MFC : la table de version) —
  2021+ 56/71, 2020 52/61, 2019 51/60, 2014–2018 49/58, 6–8 et 2013 49/55, 4–5 46/51, 3 40/45.
  Jamais écrites, faute d'équivalent dans Esquisse : photo adaptée (`CMatchPhoto`,
  `CBackgroundImage`), relations, filigranes, classifications (`CSchema…`, 0x6784), cotes
  radiales, polylignes 3D, environnements, hachures, images d'avant-plan, composants
  procéduraux, lumières ; ni vignettes des définitions, des matières ou des scènes
  (`thumbnails/`, `thumbnail.jpg`, `scene_thumbnails/`), que le SDK n'écrit pas non plus.

### 8.7 Arcs écrits tels que lus (`tests/writer_reel.rs`)

Un arc (`CArcCurve`) n'a pas toujours ses sommets sur lui : `tests/reel/stool-depth-8.skp`
(SketchUp 8) a deux quarts de cercle de rayon 157,39 mm dont les 6 sommets du milieu sont à
158,75 mm du centre (157,39 / cos 7,5° : un polygone circonscrit) ; d'autres fichiers ont des
arcs elliptiques (axes x et y de longueurs différentes : un arc mis à l'échelle), des arcs d'une
seule arête, ou des arcs coupés par une arête (un sommet sur une corde, à l'intérieur du cercle).
SketchUp les garde et les écrit tels quels, le SDK les relit comme des arcs ; l'écrivain natif,
qui ne reprenait l'arc du fichier que si tous les sommets étaient sur un cercle, en faisait des
courbes simples (`kind=curve`).

- Le lecteur garde l'arc entier, hors `sdk_quirks` (le SDK ne l'expose pas) : rayon (`r`),
  longueurs des axes (`rx`, `ry`), axe y (`yx`, `yy`, `yz`), en plus du centre, de la normale,
  de l'axe x et des angles, et l'empreinte de ses sommets (`fp`, `build::curve_fingerprint` :
  leurs positions exactes, sans ordre). Pour un polygone (`kind=polygon`) aussi.
- L'écrivain (`Plan::curves`, `ExactArc`) écrit l'arc tel que lu tant que ses sommets sont ceux
  de la lecture (même empreinte), sommets dans l'ordre de l'arc (depuis l'angle de départ) ; un
  arc d'une arête ou dont les arêtes ne font pas une chaîne reste un arc. Sinon (géométrie
  modifiée dans Esquisse), comme avant : l'arc du fichier si les sommets sont dessus, sinon le
  cercle qui passe par eux (`arc_fit`), sinon une courbe.
- Un cercle d'un fichier d'avant 2021 dont la fin vaut 0 (§3.5) s'écrit avec cette fin, comme
  le fait la conversion de SketchUp (`sdk_resave`), et non plus 2π.

Preuve sur tout le corpus réel (`every_real_file_keeps_its_curves`, `--ignored`, en 2026) : la
classe « courbes » que le SDK relit dans le fichier réécrit est celle de l'original pour 1 961
fichiers sur 1 965 ; les 4 autres (SketchUp 6 à 8) se relisent comme le modèle lu, le lecteur
natif y recalculant un cercle que le SDK ne recalcule pas (écart du lecteur, pas de l'écrivain).
Avant : 1 449 sur 1 965.
## 9. Fichiers réels (`tests/native_reel.rs`)

Le corpus du §7 est fait par le SDK ; celui-ci est fait par l'application SketchUp, de
SketchUp 3 à 2026, chez de vrais utilisateurs.

- **Sources** : 1 969 fichiers .skp distincts (par contenu) de 168 dépôts GitHub publics sous
  licence claire (MIT 77 dépôts, GPL 48, Creative Commons 17, Apache-2.0 9, BSD 6, AGPL 5,
  autres 6), trouvés par recherche de chemins (Sourcegraph, API GitHub en lecture seule) et
  téléchargés depuis `raw.githubusercontent.com`. Ni 3D Warehouse ni service de Trimble ;
  aucun compte, rien d'envoyé. Les plus gros apports : sparkfun/3D_Models (453),
  hew3d/openskp (434), brunurb/Google-Sketchup (249). Les fichiers ne sont que des données :
  rien n'y est exécuté.
- **Manifeste** `crates/esq-skp/tests/data/skp-reel.json` : par source la page, l'adresse
  brute, la licence et l'auteur ; par fichier le chemin, le SHA-256, la taille, la version de
  l'en-tête (`{N.M.build}`), l'origine (« version antérieure » : `{N.0.1}`, un Enregistrer sous
  ou le SDK ; « build 0 »), l'écart avec le SDK s'il en reste un. `tools/skp-reel.ps1` les
  retélécharge (SHA-256 vérifié) dans `%USERPROFILE%\tools\skp-reel`.
- **Dans le dépôt** (`crates/esq-skp/tests/reel`, 226 Ko, licences MIT et Apache-2.0) :
  `su2sf_pointlight-5.skp` (SketchUp 5), `version_text-6.skp` (6), `stool-depth-8.skp` (8),
  `TOP2_CLEAN_NEW-2015.skp`, `template-2026.skp` ; hors de `tests/data`, le corpus du SDK que
  d'autres tests parcourent. `tests/reel/LICENCES.md` donne pour chacun le dépôt, le chemin,
  l'auteur, la ligne de copyright et la licence ; les textes des licences MIT de chaque dépôt,
  la licence Apache 2.0 et le `NOTICE` de tqec sont à côté.
- **Familles** (ouverts par le SDK) : 3–7 : 299 fichiers ; 8 : 123 ; 2013–2020 : 1 091 ;
  2021+ : 454. Le SDK refuse 2 fichiers (`SU99.skp`, version `{99.1.0}` ; `skp-vff-hard-block`,
  fabriqué pour le tester).

**Compteur** (fichiers identiques au SDK, classe par classe, / ouverts par le SDK) :

| Famille | Avant (main au 17/10) | skp-reel | placage (textures-uv) | skp-reel-2 | les deux (18/10) |
|---|---|---|---|---|---|
| 3–7 | 78 / 299 | 277 / 299 | 285 / 299 | 289 / 299 | 299 / 299 |
| 8 | 54 / 123 | 113 / 123 | 120 / 123 | 115 / 123 | 123 / 123 |
| 2013–2020 | 563 / 1 091 | 1 059 / 1 091 | 1 067 / 1 091 | 1 078 / 1 091 | 1 090 / 1 091 |
| 2021+ | 321 / 454 | 446 / 454 | 451 / 454 | 448 / 454 | 454 / 454 |
| Total | 1 016 / 1 967 | 1 895 / 1 967 | 1 923 / 1 967 | 1 930 / 1 967 | **1 966 / 1 967** (lus : 1 967) |

Classe « textures de faces » : 1 931 / 1 967 avant le nouveau placage des textures, **1 967 / 1 967**
après. Une ligne de cette classe donne, par côté, les trois lignes (u, v, q) du placage, mises à
l'échelle q = 1 au premier point de la face, chacune par son gradient et sa valeur en ce point
(`native::canon`) ; le test les compare au 1e-9 près de leur taille plus 2,5e-9
(`tests/oracle`, `texture_lines_close`) : un placage loin de son origine (des dizaines de
milliers de motifs) montre l'arrondi des doubles à la 9e décimale, que le SDK et le lecteur
n'arrondissent pas pareil. 18 fichiers ne sont égaux qu'ainsi. Le lecteur par le SDK lit la
fonction du UV helper en quatre points hors d'un même plan (elle est affine) ; le lecteur natif
la calcule (§2.3.3).

Ce que les fichiers réels ont appris (§3.5, §2.3.7, §2.4, §2.6) : parcours et dissolution des
courbes, arcs recalculés (elliptiques compris), points d'attache (images comprises),
transformations réglées, échelle inverse, caméras, scènes, fournisseurs d'options (axes des
SketchUp 3 à 5), noms Google Earth, `GeoReference`, `SU_InstanceSet`, noms des définitions en
double, numéros des sections, `material.xml` (chemins, CDATA, extension, points finaux), types
de valeurs 12–19, repère de texture (seuil 1e-3, échantillonnage du SDK), budget des
dictionnaires, décodage JPEG de SketchUp (§2.6), Check Validity des arcs et des faces, unités
par le format, taille des images en pixels, dictionnaires en double d'un composant, polices
des annotations hors de la liste du modèle.

Avec skp-reel-2, sur les 72 fichiers du manifeste, 35 n'ont plus d'écart : matières 26/26
(décodeur JPEG 21 — le décodeur de la crate `image` arrondissait autrement, 1/255 sur la
moyenne —, image d'origine telle que stockée dans l'oracle 2, nom de texture 1, taille des images
2), arêtes et courbes 8/8 (Check Validity des arcs, arcs elliptiques, première et dernière arêtes
de SketchUp 3, arcs ouverts à angles décroissants), définitions 5/5 (taille des images 2,
dictionnaires « 3DWarehouse » en double 3), faces 6/7 (Check Validity des faces 4, taille des
images 2), styles 2/2 (axes de SketchUp 3 à 5), unités 1/1, annotations 1/1 (point d'attache sur
une image). Le test compare désormais classe par classe : il a montré une classe que le
manifeste ne listait pas (« styles nommés » de `Sky+Dome-Tutorial.skp`). Deux régressions de
main hors manifeste corrigées à la source : `Foldarap.skp` (f59f125 : le lecteur du SDK des
tests ne réservait pas les noms des composants, §10.14) et `legacy_annotations.skp` (9e030ca :
polices des annotations absentes de la liste du modèle).

**Écarts restants** (1 fichier, listé au manifeste ; le test le tolère, il refuse tout autre
écart, et toute tolérance qui ne sert plus) : `sparkfun 13656.skp` (arêtes, faces), des faces
non planes que SketchUp découpe en triangles (nouvelles arêtes douces et
cachées) ; le découpage n'est pas refait.

Éliminé en chemin : le décodeur JPEG de la crate `image` (arrondis), le recalcul des arcs des
fichiers 2021+ (il dissoudrait des cercles fermés que SketchUp garde, `piBrick`), une règle
« fin < départ » pour tous les arcs (elle dissout les cercles fermés de `vckb`), garder toute
face qu'aucun plan ne tient (les triangles minuscules de `ur5.skp` sont effacés : moins de
0,001 pouce carré). Non expliqué, gardé : rien en dehors de `13656.skp`.

## 10. Ce que le modèle garde au-delà de la géométrie (skp-complet)

Lu ET écrit dans toutes les versions, avec les conversions que fait SketchUp. Preuves :
`tests/skp_complet.rs` (mesures au §10.15), le recensement du SDK `esq_skp::sdk_census` (ce que l'API C
expose de chaque classe) et la conversion de SketchUp lui-même (`esq_skp::sdk_resave` : le SDK
ouvre le fichier et l'enregistre dans une version, avec le code de SketchUp.exe).

### 10.1 Styles

Un style est un ensemble d'**éléments** numérotés (1000 arêtes, 2000 faces, 4000 arrière-plan,
5000 filigranes, 7000 modélisation, 8000 photo adaptée, 8100 occlusion ambiante), chacun une
valeur typée (1 booléen, 2 octet, 3 court, 4 entier, 5 non signé, 6 flottant, 7 double, 9–11
vecteurs, 12 transformation, 13 objet, 14 heure, 15 64 bits). La plupart sont des options de rendu
(table `ITEMS` de `native/style.rs`, d'après le gestionnaire qui change un élément quand une
option change) ; 1013 = arêtes esquissées (objet) ; 5001 = filigranes (objet) ;
7010 = « géométrie ou objets cachés » (avant que 7017 et 7018 soient séparés) ; 7013 = plans et
coupes (bits 0 et 1).

**2021+** : 0x206 → 0x6978 `CSkpStyleManager` : 0x6979 liste de 0x6B6C `CSkpStyle` {0x5DC id,
0x6B6D GUID, 0x6B6E le `.style` d'origine (`SUStyleGetPath`), 0x6B6F dossier de l'archive,
0x6B70 ids des filigranes du modèle qu'il montre} ; 0x697A id du style sélectionné ; 0x697B →
0x6B6C le style **actif** (copie du sélectionné qui porte les réglages non enregistrés, sans id ;
dossier « `<sélectionné>_1` ») ; 0x697C booléen « actif modifié »
(`SUStylesGetActiveStyleChanged`). Nom, description et éléments sont dans
`styles/<dossier>/style.xml` (`<sty:style name desc>`, `<sty:item id>` → `<t:variant type>`),
le format des fichiers `.style`. 1013 : `<n0:nprEdge dropoutLength fadeFactor width useHalo
haloLength numProtos>`, `<n0:lengths>`, images `./Stroke_<p>_<longueur>.png` du dossier. 5001 :
`<n1:wmlist count>` → `<n1:screenimages>` → `<n1:screenimage>` ; « `<MODEL SPACE>` » marque la
place du modèle (avant : derrière lui). SketchUp ne fait un filigrane que d'un élément qui a
`info_found`, `info_filename`, `info_time`, `position`, `tiled`, `stretched`, `maintainAR`,
`background`, `intensForAlpha`, `alphaScale` et `scale` (lus l'un après l'autre, le suivant
seulement si le précédent est là) ; `fittingType` (0 mosaïque, 1 étiré, 2
placé) et `stretchType` sont facultatifs ; l'image (`<n1:image path>`) est un fichier de la
racine de l'archive (`watermarks/…`). Écrivain, mêmes attributs dans le même
ordre. Le SDK liste les styles dans l'ordre d'un ensemble de pointeurs : l'ordre n'est pas
une donnée.

**3–2020** : `CSkpStyleManager` en place : CEntity, i32 n, n obj `CSkpStyle`,
obj sélectionné, obj actif (jamais un de la liste : un actif qui en est un est jeté), schéma > 1 :
bool modifié. `CSkpStyle` : CEntity, GUID, chaîne (le `.style` d'origine),
propriétés : u32 version (3 à l'écriture), nom, description, (version 1 : une
vignette `CDib`), i32 n, n × (u32 élément ; 1013 : version, deux doubles (deux
flottants avant la version 2), u32 largeur, bool halo, u32, u32, i32 n + n u32 longueurs, i32 n +
n obj `CDib` ; 5001 (version > 2) : u32 n + n obj `CWatermark` ou nul pour la place du modèle ;
autre : u32 1, u32 type, la valeur). Ne s'écrivent pas dans une archive :
7019, 7020 et l'occlusion ambiante (8100, 8102, 8103, 8105–8107). Avant le
schéma 2 de `CSkpStyle` (SketchUp 2020), 7010 tient lieu de 7017 et 7018 (lecture : les deux en
prennent la valeur ; écriture : 7010 = 7017 ou 7018). Un élément de type 5 se lit comme un
entier (pour 4 et 5) : un style d'archive converti en 2021+ a ses couleurs en
type 4, comme SketchUp les écrit. Le style d'une scène : obj `CSkpStyle` de `CViewPage` (schéma
> 9, drapeau 0x02).

**Chargement** : SketchUp remplit d'abord un style des éléments d'un style neuf:
  un style lu garde ceux que son fichier n'a pas (ex. l'occlusion ambiante et
4007 d'un style d'archive) ; `native::style::fill_defaults`. Un fichier sans styles (SketchUp 3
à 5) en reçoit à l'ouverture : le style actif des options de rendu du modèle ;
un style par scène qui garde ses options de rendu, et pour la scène sélectionnée (de ses
options, sinon de celles du modèle), un seul pour des réglages égaux ; le sélectionné est celui
de la scène sélectionnée, sinon un style des réglages de l'actif ; noms « Style », « Style1 »…
(`native::style::of_doc`). Les GUID de ces styles changent d'une ouverture à l'autre.

**Dans Esquisse** : `view_ext["styles"]` = `{list: [{id, name, description, path, ro, wm, guid,
skp_items, skp_folder, skp_like}], selected, skp: {changed, active, ro0, wm0}}` : `ro` les options
de rendu des éléments, `wm` les filigranes (le `Watermark` de l'application, `skp` : fichier,
heure, nom d'image), `skp_items` tout autre élément tel que lu (arêtes esquissées, 7013, 5001
exact), `skp_like` le type de chaque élément lu (gardé à l'écriture). L'actif et le drapeau
« modifié » s'écrivent tels que lus tant que la vue garde les réglages lus (`ro0`, `wm0`) ;
sinon l'actif est le sélectionné avec les réglages de la vue. La scène nomme son style
(`props["style"]`, comme `Page#style`) ; la montrer, si elle garde style et brouillard (0x02),
sélectionne son style et ses filigranes (`dialogs/scene_props.rs`, `Page#__apply`). Le panneau
Styles montre traduits les noms de SketchUp (« [Simple Style] »).

### 10.2 Filigranes du modèle

**2021+** : 0x203 → 0x2CEC `CWatermarkManager` : 0x2CED liste de 0x2EE0 `CWatermark` {0x5DC id ;
0x2EE1 fichier d'origine ; 0x2EE2 trouvé ; 0x2EE3 heure ; 0x2EE4 nom ; 0x2EE5 position 0–8 ;
0x2EE6 → 0x2328 {0x2329 u32 4, 0x232A chemin dans l'archive} l'image ; 0x2EE7 mosaïque ; 0x2EE8
étiré ; 0x2EE9 proportions gardées ; 0x2EEE ajustement ; 0x2EEF type d'étirement ; 0x2EEA
derrière le modèle ; 0x2EEB échelle ; 0x2EEC luminosité = transparence ; 0x2EED opacité} ; 0x2CEE
u32 : combien sont derrière le modèle (la place du modèle dans la liste).

**3–2020** : `CWatermarkManager` en place : CEntity, (schéma < 2 : u32 n + n obj), i32 n + n obj
`CWatermark` ou nul (la place du modèle). `CWatermark` : CEntity, bool trouvé,
chaîne fichier, u32 heure, u32 position, bool mosaïque, bool étiré, bool proportions (étiré
seulement), bool derrière, bool masque, f64 opacité, f64 échelle, chaîne nom, obj `CDib`.

Dans Esquisse : `view_ext["watermarks"]` (ce que montre la vue) ; un modèle fait dans Esquisse
qui a des filigranes reçoit un style « Style » qui les porte.

### 10.3 Polices, style de texte, style de cote

**2021+** : 0x1FD → 0x4E20 `CFontManager` → 0x4E21 liste de 0x5014 `CSkFont` {0x5DC id, 0x5015
nom, 0x5016 gras, 0x5017 italique, 0x5018 points, 0x5019 taille dans le modèle, 0x501A hauteur
(pouces)}. 0x1FE → 0x57E4 `CTextStyle` {0x57E5 police (id), 0x57E6 seconde
police, 0x57E7 u32, 0x57E8 u32, 0x57E9 bool, 0x57EA u32, 0x57EB bool, 0x57EC couleur, 0x57ED
couleur}. 0x1FF → 0x5FB4 `CDimensionStyle` {0x5FB5 police, 0x5FB6 texte 3D,
0x5FB7 préfixe des cotes radiales, 0x5FB8 décalage des lignes d'attache, 0x5FB9 dépassement,
0x5FBA épaisseur, 0x5FBB flèche, 0x5FBC taille de flèche, 0x5FBD surligner les cotes détachées,
0x5FBE leur couleur, 0x5FBF bool, 0x5FC0 cacher hors du plan, 0x5FC1 sa valeur, 0x5FC2 cacher
les petites, 0x5FC3 sa valeur, 0x5FC4 couleur, 0x5FC5 couleur du texte, 0x5FC6 u32} (les champs
nommés sont ceux que le SDK expose, vérifiés par `SUDimensionStyleGet…`). Un texte : 0x55F9 id de
sa police ; une cote : 0x59DA (absent : celle du style).

**3–2020** : `CFontManager` : CEntity, i32 n + n obj `CSkFont` (celui du modèle, dans
`CSketchUpModel`). `CSkFont` : CEntity, chaîne nom, bool **italique**, bool
gras, u32 points, (schéma ≠ 0 : bool taille dans le modèle, f64 hauteur ; sinon hauteur 10, celle
du constructeur). `CTextStyle` : CEntity, (schéma 3 : la police en place — nom,
u32 points, deux bool, chaîne ; sinon obj `CSkFont`), u32, u32, bool, u32, bool, schéma ≥ 2 :
couleur, schéma ≥ 5 : couleur et obj `CSkFont`. `CDimensionStyle` (`CSketchUpModel` de schéma
> 7) : CEntity, u32, schéma > 0 : obj `CSkFont`, bool 3D, bool préfixe, u32 décalage, schéma > 2 :
u32 dépassement, u32 épaisseur, i32 flèche, schéma > 2 : u32 taille, bool surligner, couleur,
bool, bool hors du plan, f64, bool petites, f64, schéma > 1 : deux couleurs, schéma ≥ 4 : u32.
`CText` de schéma > 3 : obj `CSkFont` ; `CDimension` de schéma > 0 : obj `CSkFont` (nul : celle du
style), bool, i32 flèche.

**SDK** : `SUDimensionGetFont` rend la police du style pour une cote qui n'en a pas
(dans SketchUpAPI.dll) : il ne distingue pas une police propre égale à celle du
style ; `sdk_quirks` les confond aussi. La couleur d'un texte ou d'une cote est sa matière
(nommée « * » quand SketchUp la fait d'une couleur).

**Dans Esquisse** : `view_ext["skp_fonts"]` (la liste lue), `view_ext["skp_text_style"]`,
`view_ext["skp_dim_style"]` (polices par leur rang dans la liste) ; ce qu'en montre Infos sur le
modèle : `view_ext["TextStyle"]` (police et couleur des textes à repère = première police,
des textes d'écran = seconde ; flèche, type de repère : correspondance supposée, le SDK
n'expose pas le style de texte) et `view_ext["DimensionStyle"]` (police, flèche, préfixe, cacher,
surligner). Une annotation porte sa police (`font_name`, `font_size`, `font_bold`,
`font_italic`, `font_height` en mm pour une taille dans le modèle) et sa couleur (`material`,
`color`). L'écrivain écrit la liste lue, plus les polices que demandent les annotations et Infos
sur le modèle ; un modèle fait dans Esquisse reçoit Arial et Tahoma 12, comme du SDK. Perdu :
l'alignement et la position du texte des cotes d'Infos sur le modèle (`align_screen`,
`position`) ne sont pas reportés dans le style de cote (gardé tel que lu).

### 10.4 Classifications

**2021+** : 0x205 → liste de 0x6784 `CClassifications` {0x6785 le fichier d'où le schéma a été
chargé, 0x6786 son nom, 0x6787 son `.skc` dans l'archive (`classifications/<nom>.skc`)}. Un
`.skc` est une archive zip : `document.xml`, `documentProperties.xml`, `references.xml`,
`doc_thumbnail.png`, `Schemas/*.xsd` et leurs `.xsd.filter`.

**3–2020** : `CSketchUpModel` de schéma ≥ 0x17 (SketchUp 2014) : i32 n + n obj `CSchemaZipFile`:
  chaîne fichier d'origine (souvent le dossier), chaîne nom, i32 n + n octets
du `.skc`. `CSchemaFile` (lecture seule : SketchUp 2014 avant le zip) : deux
chaînes, le `.xsd` en clair, deux listes de `CSchemaFile` (les schémas qu'il importe), (schéma
≠ 0) bool + obj `CSchemaFilterFile` (chaîne, octets du filtre).

Les types appliqués sont des attributs de la définition (`AppliedSchemaTypes` : schéma → type,
et un dictionnaire par schéma, dont les valeurs sont dans des dictionnaires imbriqués).

**Chargement d'un `.skc`** : SketchUp lit `documentProperties.xml` élément après élément
(`title`, `description`, `creator`, `keywords`, `lastModifiedBy`, `revision`, `created`,
`modified`, `thumbnail`, `generator`, l'ordre de son écrivain) et ne charge pas
le schéma s'il en manque un (vérifié : un `.skc` sans `keywords`… n'apparaît pas dans
`SUModelGetClassifications`).

**Dans Esquisse** : le schéma dans la table du modèle (`classifications` › `schemas`,
`esq_core::classify`) et son `.skc` dans `Model::files["skc:<nom>"]` ; Importer… (Infos sur le
modèle, outil Classer, `Classifications#load_schema`) garde le fichier, un .skp le porte.
Perdu : un schéma sans son fichier (chargé avant ce changement) n'est pas écrit ; les
`CSchemaFile` de SketchUp 2014 (un `.xsd` en clair, pas de zip) ne sont pas convertis.

### 10.5 Dictionnaires imbriqués

Un dictionnaire (`CAttributeNamed`) est une entité : son `CEntity` a lui aussi un conteneur de
dictionnaires, ceux qui sont imbriqués dans lui (`AttributeDictionary#attribute_dictionaries`
en Ruby ; l'API C ne les expose pas). SketchUp y range les valeurs d'une classification
(dictionnaire du schéma → propriété → valeur, `attribute_type`, `is_hidden`, `value`…).

**2021+** : 0x36B3 → 0x36B0 (vide, ou 0x5DC → 0x5DD : le conteneur des dictionnaires
imbriqués). **3–2020** : le `CEntity` en tête de `CAttributeNamed` (obj `CAttributeContainer`).

**Dans Esquisse**, comme le pont Ruby les range : les dictionnaires imbriqués dans le
dictionnaire `D` de l'entité `O` sont dans `Model::attr_store["ad:<O>|<D>"]` (`O` : `model`,
`d:0:<définition>`, `i:<contexte>:<instance>`, `h:e:…`, `mat:<nom>`, `layer:<nom>`…), un étage
de plus dans `ad:ad:<O>|<D>|<E>`. Le lecteur les lit (`doc::NEST`, `build::split_nested`),
l'écrivain les écrit (`plan::dicts_with`, 32 étages au plus). Un dictionnaire sans valeurs mais
qui en contient d'autres est gardé. Les dictionnaires de la définition d'un groupe sont lus
aussi (ils étaient perdus par les deux lecteurs), le nom d'une définition de groupe non (comme
le lecteur du SDK, Esquisse les renomme).

Preuve : `ifc-silly-sample-scene.skp` (SketchUp 2024, 110 977 dictionnaires imbriqués) se relit
identique après réécriture dans sa version et en 2026 ; `nested_dictionaries_go_through_every_version`
(modèle, définition et instance, trois étages, SketchUp 4 à 2026).

### 10.6 Cotes radiales

**2021+** : liste 0x139A d'une collection → 0x5DC0 `CDimensionRadial` : 0x59D8 la
cote (élément, texte, police, flèches, comme une cote linéaire), 0x5DC1 l'id d'une **arête** de
l'arc mesuré (0 : aucun, alors 0x5DC2 l'arc lui-même, 16 doubles comme 0x4C2D :),
 0x5DC3 l'angle de son point sur l'arc, 0x5DC4 le décalage du repère, 0x5DC5
diamètre. **3–2020** (schéma 2 partout) : `CDimension`, obj `CEdge` ou nul,
f64 angle, f64 décalage, bool diamètre, sans arête l'arc en place (centre, normale, axe x,
angles, schéma > 1 : axe y). Valeurs d'une cote neuve : angle 0, décalage 0, rayon.
 SketchUp en fait les trois points du repère (`SUDimensionRadialGetLeaderPoints`,
de SketchUpAPI.dll : point de l'arc à l'angle, décalé le long du rayon ; le point
opposé pour un diamètre). Une cote de SketchUp 3 à 8 sans style à elle prend les flèches et la
matière du style de cote du modèle, comme une linéaire.

**Dans Esquisse**, comme le pont Ruby (`Entities#add_dimension_radial`) : une annotation
linéaire du centre au point de l'arc, `radial` = l'arc (`c:<collection>:<courbe>`,
`DimensionRadial#arc_curve`), `diameter`, `arrow_type`, et ce que SketchUp lit (`skp_angle`,
`skp_leader` en mm ; `skp_arc` sans arête). L'écrivain écrit l'arête de l'arc la plus proche du
point, l'angle et le décalage lus tant que le point est toujours celui de l'angle, sinon l'angle
du point et pas de décalage (avant : la cote était écrite linéaire). Le lecteur du SDK ne lit
pas les cotes radiales (`sdk_quirks` les laisse).

### 10.7 Polylignes 3D

**2021+** : liste 0x1395 → 0x4844 `CPolyline3d` {0x7D0 élément, 0x4845 les points (n × 3
doubles)}. **3–2020** (schéma 0 partout) : `CDrawingElement`, i32 n, n points.
Le SDK les lit (`SUEntitiesGetPolyline3ds`) mais n'en ajoute pas.

**Dans Esquisse** : `Entities::polylines` (`esq_core::model::Polyline3d` : points en mm,
étiquette, cachée), dessinées comme des arêtes, dans le cadrage de Zoom étendu ; elles suivent
leur collection quand on éclate un groupe, et les étiquettes renumérotées. Pas de sélection ni
d'outil (SketchUp n'en a pas non plus), pas de classe Ruby (l'API Ruby n'en a pas).

### 10.8 Vignettes, composants non placés

**Définitions** : 2021+ 0x157C → 0x1585 → 0x251C `CThumbnail` {0x251D → 0x34BC la caméra,
0x251E → 0x2328 {0x2329 u32 4, 0x232A `thumbnails/<nom>.png`} l'image dans l'archive} ; 3–2020 :
dernier champ de `CComponentDefinition` (schéma > 9) : obj `CThumbnail` (CEntity, caméra en place
au schéma 0, sinon obj `CCamera`, obj `CDib` ou nul). **Matières** (2021+ seulement) :
`materials/<dossier>/thumbnail.jpg` (ou `.png`), à côté de `material.xml`, et 0x32CE → 0x2134
son chemin (l'écrivain ne l'écrivait pas).
L'aperçu du modèle (`meta/model_thumbnail.png`, `preview_thumbnail.png`) est celui que
l'application rend à l'enregistrement. Le SDK n'expose que les vignettes des styles et des
environnements.

**Dans Esquisse** : l'image dans `Model::files` (`build::def_thumbnail_key` : `thumb:def:<nom>`,
`build::mat_thumbnail_key` : `thumb:mat:<nom>`), la caméra dans `DefExt::thumbnail_camera`
(`ComponentDefinition#thumbnail_camera`). L'écrivain les réécrit telles que lues (une définition
renommée perd la sienne, SketchUp la refera) ; sans elles, la caméra du SDK et pas d'image.

**Composants non placés** (« Dans le modèle » de Composants jusqu'à la purge) : ni le lecteur
du SDK ni le natif ne les gardaient, ni l'écrivain ne les écrivait ; ils sont lus (hors
`sdk_quirks`) et écrits. Les définitions de groupes et d'images partent avec leur dernière
instance, comme dans SketchUp.

Preuves : `stool-depth-8.skp` garde la vignette de son composant non placé, les 92 vignettes de
`ifc-silly-sample-scene.skp` se relisent identiques ; `thumbnails_and_unplaced_components_go_through_every_version`.

### 10.9 Photos adaptées ; temps, vignette et nom des scènes

**2021+** : 0x201 → 0x2904 `CMatchPhoto` {0x5DC id ; 0x2905 → 0x2AF8 l'image {0x2AF9 le fichier
d'origine, 0x2AFA type, 0x2AFB → 0x2328 son fichier `matched_photos/…` de l'archive, 0x2AFC
largeur, 0x2AFD hauteur, 0x2AFE u32, 0x2AFF heure} ; 0x2906 bool ; 0x2907 double ; 0x2908 les
12 extrémités des lignes tracées sur la photo (2 lignes par axe, rouge, vert, bleu ; en hauteurs
d'image) ; 0x2909 décalage de l'origine (3 doubles) ; 0x290A double ; 0x290B u32} ; 0x202 l'id
de la photo active. La scène : 0x7156 l'id de sa photo. **3–2020** : `CBackgroundImage`,
 obj de `CViewPage` (schéma > 9) et du modèle (la photo active) : CEntity,
l'image (chaîne, type — bool avant le schéma 5 —, obj `CDib`, au schéma ≥ 3
largeur, hauteur, u32, au schéma ≥ 4 heure), bool, double, i32 n + n points, schéma > 5 le
décalage, schéma > 6 un double (rendu négatif au chargement s'il est positif), schéma ≥ 10 u32 ;
au schéma < 2 (SketchUp 6) chaîne, bool, double, points puis l'image.

L'unité des points est la hauteur de l'image : `esq_core::matchphoto::solve` sur ces lignes
(× hauteur) redonne la caméra de la scène (`fcafb5b5_match-photo.skp` : champ vertical 98,744°
pour 98,737°, même visée). Non établi : le sens de l'axe vertical (cette photo est symétrique ;
Esquisse prend y vers le bas).

**Fin de la scène** : 0x7152 animée, 0x7153 une chaîne (le nom de la photo
d'une scène de photo adaptée), 0x7154 et 0x7155 ses temps de transition et de pause (−1 : ceux du
modèle), 0x7156 la photo, 0x715C les images d'avant-plan, 0x7157 bool, 0x7158 → 0x2134
`scene_thumbnails/…` sa vignette (ou 0x2135 l'image). MFC (schéma ≥ 8) : les deux temps, la
photo (> 9), puis (> 0xB) bool 0x7157, bool « a une vignette », image (bool,
i32 n, octets).

**Dans Esquisse** : `Scene::photo` (`esq_core::photo::MatchPhoto` : image, taille, lignes, caméra
tirée de celle de la scène, champ vertical), et `props["skp_photo"]` ce que SketchUp garde, réécrit
tel quel tant que l'image et les lignes sont celles lues (sinon refait depuis Esquisse) ;
`props["transition"]`, `props["delay"]` (`Page#transition_time`, `#delay_time`),
`props["skp_page"]` (0x7153, 0x7157), la vignette dans `Model::files["thumb:scene:<uid>"]`.
Avant : photo, temps, vignettes perdus, temps écrits −1.

### 10.10 Environnements

**2021+ seulement** (SketchUp 2025 les a ajoutés ; une version 3–2020 n'en a pas, la conversion
de SketchUp les perd aussi) : 0x210 → 0x7918 `CEnvironmentManager` {0x7919 la liste de 0x7B0C
`CEnvironment` {0x5DC id, 0x7B0D son dossier `environments/<dossier>/` de
l'archive, 0x7B0E → 0x2134 sa vignette `thumbnail.jpg`}, 0x791A l'id du courant}. Le dossier
garde `environment.xml` (les attributs du `document.xml` d'un `.ske` : `name`, `description`,
`ibl_enabled`, `rotation_angle`, `skybox_enabled`, `skybox_gain`, `reflection_gain`,
`shadow_light_enabled`, `shadow_light_positionX` en fraction de tour, `…Y` en fraction de quart de
tour, `ibl_filename` ; `<ibl_image path="./nom">`) et le panorama `.hdr` ou `.exr`. La scène
(drapeau 0x200) : 0x715A l'id de son environnement, 0x715B ses réglages (
0x7B0F reflets, 0x7B10 ciel, 0x7B11 rotation f32, 0x7B12 et 0x7B13 expositions, 0x7B14 soleil lié,
0x7B15 et 0x7B16 position du soleil). Le lecteur ne cherche le panorama que dans l'archive, dans
le dossier de l'environnement (`./nom`, sans `..`), pris au budget.

**Dans Esquisse** : `view_ext["envs"]` (`Sketchup::Environments` ; ids 1, 2… dans l'ordre du
fichier, position du soleil en degrés, `skp_folder` et `skp_file` pour réécrire les mêmes noms),
`view_ext["env_current"]`, le panorama dans `Model::files["env:<id>"]` (un `.ske` livré y est
gardé entier : l'écrivain en sort le panorama de `ref/`), la vignette dans `["thumb:env:<id>"]` ;
la scène : `props["env"]` le nom (par 0x715A, sinon l'environnement aux mêmes réglages, sinon le
courant), `props["skp_env"]` ses réglages tels que lus. L'écrivain du SDK les crée par
`SUEnvironmentCreateFromFile` (le fichier passe par le dossier temporaire) et
`SUSceneSetEnvironment`. Avant : environnements perdus en lecture comme en écriture, et les
`.ske` livrés par Esquisse refusés par SketchUp (`documentProperties` sans `keywords`,
`lastModifiedBy`, `revision`, `created`, `modified`).

Preuves (corpus du SDK ; aucun fichier réel n'en a) : un fichier fait par le SDK (deux
environnements, une scène sur le second) réécrit nativement en 2026 et 2021 donne le même
recensement du SDK, et en 2017 le même que la conversion de SketchUp ;
`environments_go_through_a_2021_file` (natif et SDK, `Studio blanc.ske` livré).

### 10.11 Motifs de hachures

**2026 seulement** (SketchUp.exe 26.2 ; `SketchUpAPI.dll` 26.2 n'en lit ni n'en écrit aucun : ni le
SDK ni sa conversion ne les gardent ; avant 2021 il n'y en a pas). Le gestionnaire 0x211 → 0x7D00
`HatchPatternManager` → 0x7D01 la liste de 0x7D0A
`HatchPattern` {0x5DC id, 0x7D15 son dossier
`hatchpatterns/<dossier>/` (le nom, `/` remplacé par `_`), 0x7D0B son nom, 0x7D10 → 0x2134 sa
vignette `thumbnail.jpg` (ou 0x2135 l'image)}. Le dossier garde `hatchpattern.xml`
(racine `hatchpatternDocument`, élément `hp:hatchpattern` avec `name` puis les
réglages) et, pour un motif image, son image (`<textureName id path
file_name>`). Les réglages (`HatchPatternSettings`) en attributs : `colorRed`, `colorGreen`,
`colorBlue`, `hasTexture`, `xScale`, `yScale`, `rotationAngle` (flottants), `hasPat`, `patName`,
`patData` (base64 des flottants du .pat : par famille angle, origine, décalage, nombre de paires
trait/blanc, les paires ; une ligne pleine est la paire 1, 0) — ceux d'un fichier `.skh`, de
l'élément 7019 d'un style (`hatch_pattern_settings`) et, en ligne, des options de rendu :
0x7388 → 0x7D0A {0x7D0C couleur RGBA, 0x7D0D → 0x2328 l'image
(0x232B ses octets), 0x7D12 son nom, 0x7D0E et 0x7D0F les échelles, 0x7D14 la rotation (f32),
0x7D11 les flottants du .pat s'il en a un, 0x7D13 le nom du .pat}, écrit avant 0x738C
(`DefaultSectionFillStyleEnabled`) ; une scène qui garde ses options de rendu a les siens.
Références par id : matière 0x32CF, instance (et groupe) 0x1969, plan de section 0x4460
(lus seulement si la fonction « SSC » est
active).

**Dans Esquisse** : `ModelMeta::hatch_patterns` (`Sketchup::HatchPatterns` : un .pat devient son
texte, chaque nombre le plus court qui redonne le même flottant), `MaterialExt::hatch` (uid),
`InstanceExt::hatch` (nom), la propriété `hatch` d'un plan de section (nom),
`view_ext["ro_hatch"]` (le motif des options de rendu, par nom, quand ses réglages sont ceux
d'un motif du modèle, sinon `view_ext["skp_ro_hatch"]` les réglages lus ;
`props["skp_ro_hatch"]` pour une scène) ; `Model::files["skp_hatch:<uid>"]` le
`hatchpattern.xml` lu, dont l'écrivain reprend les flottants tant que le motif n'a pas changé,
`["thumb:hatch:<uid>"]` la vignette. Avant : tout perdu en lecture comme en écriture (motifs,
références, réglages des options de rendu).

Preuves : aucun fichier réel ni du SDK n'a de motif ; `Sree.skp` (Components Sampler de SketchUp
2026) se réécrit avec les mêmes octets pour 0x7388 et 0x211 (`hatch_records_are_written_as_sketchup_writes_them`) ;
`hatch_patterns_go_through_a_2026_file` (un .pat aux nombres qu'un flottant ne tient pas, une image,
une matière, un groupe, un plan de section, le style ; relu identique, réécrit avec les mêmes
nombres ; le SDK ouvre le fichier).

### 10.12 Collages (relations)

Une relation (`CRelationship`) lie ce à quoi un composant (ou
un groupe) est collé et ce composant : `ComponentInstance#glued_to` rend la première extrémité de
la relation de l'instance. **2021+** : dans la collection, 0x139B → 0x639F
`CRelationshipMap` {entité, 0x63A0 la liste de 0x639C {entité, 0x639D l'id de la cible (face,
instance, groupe), 0x639E l'id du composant}} ; **3–2020** : la `CRelationshipMap` en place du
`CComponent`, après ses éléments (u32 n + n obj `CRelationship` : CEntity, les deux objets).
Le corpus réel en a dans 159 fichiers (138 archives MFC, 1177 relations ; 21 fichiers 2021+).

**Dans Esquisse** : `InstanceExt::glued_to` (`GlueRef::Face` ou `GlueRef::Instance` de la même
collection) ; le SDK n'en expose rien (`sdk_quirks`). Avant : lues puis jetées, jamais écrites.

Preuves : `glued_components_go_through_every_version` (une instance collée à une face, une à une
autre instance, un groupe collé : relus dans chaque version, et gardés par la réécriture du SDK
dans la même version comme vers 2026) ; classe « collages » de `skp_complet_all`. La conversion
de SketchUp de `8L16.skp` (deux groupes collés à une face) n'en garde aucune ; Esquisse les garde.

### 10.13 Valeurs du modèle dont l'usage n'est pas connu

`CSketchUpModel` écrit un booléen en 0x2FF (0x20C), un u32 en 0x328 (0x20D) et un
u32 en 0x24C (0x20F) ; l'archive MFC les a aussi : u32 0x24C au schéma ≥ 0x14
(au schéma 0x13 un booléen : faux → 2, vrai → 0), booléen 0x2FF au schéma ≥ 0x16 (vrai au schéma
0x15, faux avant), u32 0x328 au schéma > 0x1B (le plus grand de celui du modèle et de celui lu).
Un modèle neuf a vrai, 1, 0. Dans le corpus 2021+ : 0x20C faux dans 4 fichiers, 0x20D vaut 2 à 4
dans 28. 0x214 (0x301) dit que les faces retournées ont été mises à jour (`UpdateFaceReversal`,
fait au chargement) : faux partout, écrit faux.

**Dans Esquisse** : `view_ext["skp_model"]` quand elles ne sont pas celles d'un modèle neuf ;
les deux écrivains les réécrivent telles que lues. Avant : écrites vrai, 1, 0. Preuve : classe
« valeurs du modèle » de `skp_complet_all`.

### 10.14 Provenance des définitions, GUID des instances

Une définition garde son GUID (0x157D ; MFC après son `CComponent`), le fichier d'où elle a été
chargée (0x1580 ; MFC au schéma ≥ 4, avant une liste de noms) et quand (0x1581, secondes depuis
1970) ; une instance son GUID (0x1968 ; MFC au schéma > 4). La réécriture de SketchUp (`sdk_resave`,
dans la même version comme vers 2026) garde les GUID des définitions et donne au modèle un GUID
neuf (`meta/meta.dat` 0x66) ; l'écrivain natif fait de même.

**Dans Esquisse** : `DefExt::guid`, `DefExt::path`, `DefExt::loaded_at` (même 0),
`InstanceExt::guid` (le lecteur du SDK n'en donne pas : `sdk_quirks`) ; le nom et la description
d'une définition de groupe (« Lines », « Group#3 » : Esquisse nomme les siennes « Groupe#N »)
dans `view_ext["skp_group_names"]`, par GUID, que l'écrivain reprend (un groupe d'Esquisse :
« Group », comme `SUGroupCreate`). Les noms des composants du fichier sont réservés avant de nommer
les groupes (le lecteur du SDK des tests fait de même, `SUModelGetComponentDefinitions`). En Ruby,
`ComponentInstance#guid` rend le GUID du fichier sous la forme IFC (22 caractères base 64)
 et `ComponentDefinition#guid` en minuscules avec tirets ; une
copie n'a plus celui de l'original. Avant : GUID neufs, chemin vide, heure de l'enregistrement,
groupes nommés « Group ».
Preuve : classes « provenance » et « polices des textes » (le SDK nomme le propriétaire d'un texte
par sa définition) de `skp_complet_all`.

### 10.15 Mesures

`skp_complet_all` (`--ignored --release`, 49 min) sur les 1967 fichiers réels distincts
(`tools/skp-reel` et `tests/reel`) : chacun lu nativement, réécrit dans sa version et en 2026, relu
par le SDK (`sdk_census`) et nativement ; une classe est gardée si elle est celle du fichier, ou
celle de la conversion de SketchUp lui-même (`sdk_resave`). Fichiers qui ont la classe / gardée
dans leur version / gardée en 2026 (18/10/2026) :

| Classe | Fichiers | Même version | 2026 |
|---|---|---|---|
| styles | 1967 | 1965 | 1965 |
| style actif | 1967 | 1967 | 1965 |
| styles des scènes | 230 | 229 | 229 |
| filigranes et éléments des styles | 1854 | 1853 | 1853 |
| polices | 1967 | 1964 | 1966 |
| polices des textes | 96 | 94 | 94 |
| style de cote | 1967 | 1967 | 1967 |
| classifications | 784 | 784 | 784 |
| dictionnaires imbriqués | 7 | 7 | 7 |
| cotes radiales | 23 | 23 | 23 |
| polylignes | 4 | 4 | 4 |
| vignettes | 1692 | 1690 | 1690 |
| collages | 145 | 145 | 145 |
| provenance | 1818 | 1818 | 1818 |
| valeurs du modèle | 245 | 245 | 245 |

Aucune écriture en échec (avant les corrections du 18/10 : 7, dont 5 fichiers que SketchUp 2026
refusait). Avant ce travail, l'écrivain natif ne gardait aucune de ces classes au-delà du style
courant ; à mi-parcours (styles seuls) : styles 1963, style actif 1965, styles des scènes 227.
Restent (12 fichiers) : deux styles en double d'un même fichier (`Sky+Dome-Tutorial.skp`), le
drapeau « affiche les filigranes » d'un style sans image de filigrane (3 fichiers), une police
qu'aucun texte n'utilise ou celle d'un texte de SketchUp 8 (`legacy_annotations.skp`), la
vignette de la matière d'une image (Esquisse refait les images, 2 fichiers).

skp-reel-2 (18/10, `skp_complet_all` avec le décodeur JPEG, les unités, les axes et les polices
des annotations) : mêmes compteurs, sauf classifications 781/784 en même version (`piBrick`,
`Garg`, `attic-staircase`), instable : relancé seul, `piBrick` passe ; un autre processus du
SDK tournait en même temps (un autre agent). Les 12 fichiers restent ;
pour la vignette de la matière d'une image, garder les octets ne suffit pas : SketchUp nomme
cette matière d'après le fichier (« Image »), l'écrivain l'écrit « <auto> ».

Corpus du SDK : les 75 fichiers des nouveaux cas de `skp_corpus` (`styles_files`,
`classification`, `radial`, `match_photo`, `environments`, dans les 15 versions) :
`native_reader_matches_the_sdk` les lit comme le SDK (126 fichiers sur 126 avec ceux du dépôt et
ceux livrés avec SketchUp) ; `skp_complet_all` sur eux et `tests/reel` (80 fichiers) : styles
80/80, classifications 8/8 (les versions qui en ont), cotes radiales 15/15, environnements 1/1
(2021+), tout gardé dans la version et en 2026.
