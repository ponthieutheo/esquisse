# Drawing and modelling

[Guide](../../../README.en.md#documentation) · [Français](../dessiner.md)

## Tools, status bar and Measurements box

![Push/Pull tool active: the status bar describes the tool, the "Distance" box waits for a value](../../media/outils.jpg)

Each tool tells in the status bar what it expects and what its modifier keys do (Ctrl, Alt,
Shift, arrow keys). The Measurements box, bottom right, takes what is typed during or right
after an operation, without clicking in it:

| Input | Effect |
|---|---|
| `2,5` or `2,5m`, `80cm`, `6'4"` | a length, in the model's unit or the one typed |
| `4;3` | both sides of a rectangle |
| `24s` | the number of segments of a circle, arc or polygon |
| `45` | an angle, with Rotate or the Protractor |
| `5x` or `/5` after a copy | an array of copies, or copies spread evenly |

**Drawing**: Line, Freehand, Arc (centre, two-point, three-point, pie), Rectangle, Rotated
Rectangle, Circle, Polygon. A closed planar outline becomes a face; a line across a face splits
it; a shape drawn inside a face makes a hole.

**Editing**: Push/Pull (Ctrl keeps the starting face, a double-click repeats the last distance),
Follow Me, Offset, Move (Ctrl copies), Rotate, Scale, Flip, Eraser (Shift hides, Ctrl softens).

**Inference**: endpoints, midpoints, points on an edge or a face, red, green and blue axes.
Arrow keys lock an axis; Shift locks the current inference.

**Construction**: Tape Measure and Protractor (guides), Axes, Dimensions, Text, 3D Text.

## Solid tools

![A plate drilled with Subtract and a part made with Union; Entity Info shows the solid group's volume](../../media/solides.jpg)

A closed group or component is a solid: Entity Info shows its volume. The tools of **Outils ›
Solides** (Tools › Solids) combine two solids: Intersect, Union, Subtract, Trim and Split;
**Outils › Enveloppe externe** (Outer Shell) keeps only the skin. The result is still a solid.

## Sandbox: terrain

![Smoothed terrain and the Dessiner › Bac à sable (Draw › Sandbox) menu](../../media/terrain.jpg)

- **Dessiner › Bac à sable › À partir des contours** (Draw › Sandbox › From Contours) makes a
  terrain from selected contour lines.
- **À partir de zéro** (From Scratch) draws a flat grid to shape.
- The tools of **Outils › Bac à sable** (Tools › Sandbox), Smoove, Stamp, Drape, Add Detail and
  Flip Edge, deform the terrain, print an object's footprint on it or project edges onto it.

## Also

- **Édition** (Edit): cut, copy, paste, paste in place, hide and lock, make a group or a
  component, Intersect Faces (with the model, the selection or the context), Weld Edges.
- **Right-click**: depending on the selection, the context menu has SketchUp's entries
  (reverse or orient faces, divide an edge, intersect, explode, make unique…); the Soften Edges
  panel smooths a mesh.
- Undo and redo apply to everything.
