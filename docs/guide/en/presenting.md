# Shadows, scenes and sections

[Guide](../../../README.en.md#documentation) · [Français](../presenter.md)

## Shadows

![Shadows on 21 June at 6:30 pm; Shadows panel with time, date and lighting](../../media/ombres.jpg)

The **Shadows** panel sets the time, date and time zone; sunrise and sunset are shown under the
time slider. The sun follows the model's geo-location (**Fenêtre › Infos sur le modèle ›
Géolocalisation**, Window › Model Info › Geo-location). Shadows fall on faces, on the ground,
or from edges; the Light and Dark sliders set the lighting.

## Scenes

![Scene tabs at the top of the view; Scenes panel with the saved properties](../../media/scenes.jpg)

A scene keeps a point of view and, as chosen, the visible tags, hidden objects, active section
planes, style, fog, shadows, environment and axes. The tabs switch from one scene to the next
with a transition; **Affichage › Animation** (View › Animation) plays them in turn, and
**Fichier › Exporter › Animation** (File › Export › Animation) makes a video (ffmpeg needed) or
an image sequence.

## Section planes

![Plan cut through the house at 1.40 m: walls, partition and openings filled, furnished rooms](../../media/section.jpg)

The **Section Plane** tool cuts the model; the cut is filled (colour set by the style) and
exported as DWG, DXF or SVG (**Fichier › Exporter › Tranche de section**, File › Export ›
Section Slice). Each group or component can have its own active plane, and a scene keeps the
active planes.

## Cameras

**Caméra** (Camera) offers standard views, perspective, parallel projection and two-point
perspective, field of view, **Position Camera**, **Walk** and **Look Around** to move through the
model, and **Match Photo** to model over a photograph.
