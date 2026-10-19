# Contribuer / Contributing

## Français

Les rapports de bogues et les propositions sont les bienvenus dans les issues. Pour un écart
avec SketchUp, décrivez ce que fait SketchUp, ce que fait Esquisse, et joignez si possible un
petit fichier `.skp` ou `.esq` qui le montre (seulement un fichier que vous avez le droit de
partager).

Avant une demande de fusion :

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Une fonction par commit, avec son test.
- Pas de fichier de Trimble dans le dépôt (bibliothèques, matières, styles, icônes, modèles
  types, textes d'aide), ni de code repris de SketchUp : le comportement se réécrit.
- Tout code qui lit un fichier ou une donnée d'extension suit les règles de
  [`docs/securite.md`](docs/securite.md).
- En contribuant, vous acceptez que votre contribution soit publiée sous la licence MIT du
  projet.

## English

Bug reports and suggestions are welcome in the issues. For a difference with SketchUp,
describe what SketchUp does, what Esquisse does, and if you can attach a small `.skp` or `.esq`
file that shows it (only a file you are allowed to share).

Before a pull request, run the three commands above (format, Clippy, tests).

- One feature per commit, with its test.
- No Trimble files in the repository (libraries, materials, styles, icons, templates, help
  texts) and no code taken from SketchUp: behaviour is rewritten.
- Any code that reads a file or extension data follows the rules of
  [`docs/securite.md`](docs/securite.md).
- By contributing, you agree that your contribution is published under the project's MIT
  licence.
