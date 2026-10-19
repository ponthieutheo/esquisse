# Ruby and extensions

[Guide](../../../README.en.md#documentation) · [Français](../ruby.md)

## SketchUp extensions

![SketchUp's "3D Shapes" sample extension, loaded as is: its menu and its shapes](../../media/extensions.jpg)

*Extension: [sketchup-shapes](https://github.com/SketchUp/sketchup-shapes), © Trimble
Navigation Ltd., MIT licence, unmodified.*

- Esquisse embeds Ruby 3.2, SketchUp 2026's version, and SketchUp's whole Ruby API: the 1,855
  methods of its classes (`Sketchup`, `Geom`, `UI`, entities, tools, observers,
  `UI::HtmlDialog`…) exist; the list is in [API coverage](../../ruby-coverage.md).
- Extensions go in Esquisse's `Plugins` folder (`%APPDATA%\Esquisse\Plugins`) or are installed
  from an `.rbz` (**Extensions › Gestionnaire d'extensions**, Extension Manager), then add their
  menus, toolbars and tools as in SketchUp.
- SketchUp's official Ruby API test suite (TestUp) passes 2,895 of 3,020 tests; sample
  extensions published by SketchUp on GitHub (3D Shapes, Weld, Draw Mesh) run unmodified.
- Encrypted extensions (`.rbe`, `.rbs`), common among commercial ones, do not load.

## Ruby Console

![Ruby Console with the output of a script that queries the model](../../media/console-ruby.jpg)

**Extensions › Développeur › Console Ruby** (Extensions › Developer › Ruby Console) runs Ruby on
the open model: Enter runs the line, Shift+Enter starts a new line, the arrow keys recall the
history. Output from scripts and extensions (`puts`) shows there.

```ruby
model = Sketchup.active_model
face = model.active_entities.add_face([0, 0, 0], [2.m, 0, 0], [2.m, 1.m, 0], [0, 1.m, 0])
face.pushpull(-2.5.m)
```
