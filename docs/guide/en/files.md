# Files: `.skp`, import and export

[Guide](../../../README.en.md#documentation) · [Français](../fichiers.md)

## The `.skp` format

![A stool saved by SketchUp 8, opened in Esquisse](../../media/fichier-skp.jpg)

*Model: `stool-depth.skp` from the DeqianBai/Python-Project repository, Apache-2.0 licence
(`crates/esq-skp/tests/reel`).*

- Esquisse reads and writes `.skp` files itself, without SketchUp or its SDK: files from
  SketchUp 3 to 2020 (MFC archives) as well as 2021 to 2026 (ZIP archives).
- On a corpus of 1,967 real files saved by SketchUp 3 to 2026, 1,966 are read exactly as
  SketchUp's SDK reads them, object class by object class.
- **Enregistrer** (Save) writes the current version (2026); **Enregistrer sous** (Save As)
  offers the older versions, like SketchUp. A model opened then saved keeps what SketchUp put in
  it: textures, styles, scenes, attributes, dimensions, snaps, spaces, classifications.
- Damaged files are refused cleanly or repaired on opening (**Fenêtre › Infos sur le modèle ›
  Statistiques › Corriger**, Window › Model Info › Statistics › Fix Problems).
- `.esq` is Esquisse's native format: readable JSON.

## Import and export

![File › Export menu: 3D model, 2D graphic, section slice, animation](../../media/exporter.jpg)

| | Formats |
|---|---|
| 3D import | 3DS, COLLADA (`.dae`), DEM (`.dem`, `.ddf`), AutoCAD (`.dwg`, `.dxf`), glTF (`.glb`, `.gltf`), IFC (`.ifc`, `.ifczip`), Google Earth (`.kmz`), STL, Trimble (`.trb`), USD (`.usd`, `.usdz`) |
| Image import | BMP, JPEG, PNG, PSD, TGA, TIFF, PDF; as an image, as a face texture or as a matched photo |
| 3D export | 3DS, COLLADA, DWG and DXF, FBX, IFC, KMZ, OBJ (with `.mtl` and textures), STL, VRML, XSI, glTF and GLB, USDZ |
| 2D export | PNG, JPEG, BMP, TIFF, vector PDF, EPS, hidden-line DWG and DXF |
| Section export | section slice as DWG, DXF or SVG |
| Animation export | MP4, WebM, AVI (ffmpeg needed) or an image sequence |

Each format has its options, the same as in SketchUp (**Options...** button of the export or
import dialog). DWG and DXF are written in the chosen version, from R12 to 2018. **Fichier ›
Imprimer** (File › Print) prints the view to scale or tiled.

## Templates

**Fichier › Nouveau à partir du modèle type** (File › New from Template) and the welcome window
offer the 21 bundled templates, plus the user's own: **Fichier › Enregistrer comme modèle type**
(File › Save As Template) adds one. The default template is chosen in **Fenêtre › Préférences ›
Modèle type** (Window › Preferences › Template).
