# Organiser le modèle

[Guide](../../README.md#documentation) · [English](en/organising.md)

## Groupes et composants

![Panneau Composants : les composants du modèle, ici le mobilier du contenu livré](../media/composants.jpg)

- **Créer un groupe** isole des faces et des arêtes ; **Créer un composant** en fait une
  définition partagée : modifier une copie modifie toutes les autres, sauf celle que l'on a
  rendue unique.
- Un double-clic ouvre le groupe ou le composant pour l'éditer ; Échap le referme. Le reste du
  modèle peut être estompé ou masqué pendant l'édition.
- Un composant peut se coller à une face, découper son ouverture dans un mur (portes,
  fenêtres) ou rester tourné vers la caméra (personnages et arbres en 2D).
- Le panneau **Composants** parcourt les composants du modèle et ceux du contenu livré, les
  place, modifie leurs réglages et donne leurs statistiques. Un fichier `.skp` ou `.esq`
  s'insère comme composant par **Fichier › Importer**.
- **Infos sur l'entité** montre et modifie ce qui est sélectionné : nom, étiquette, matière,
  volume d'un solide, ombres projetées et reçues, et les réglages propres à chaque type
  d'entité.

## Étiquettes

![Panneau Étiquettes : étiquettes rangées en dossiers](../media/etiquettes.jpg)

Les étiquettes (les calques de SketchUp) règlent la visibilité de ce qui leur est attribué. Le
panneau les range en dossiers, leur donne une couleur et un type de tirets, et peut colorer le
modèle par étiquette (**Affichage › Colorer par étiquette**). L'outil Étiquette attribue une
étiquette d'un clic.

## Structure

![Panneau Structure : groupes et composants du modèle](../media/structure.jpg)

Le panneau **Structure** montre l'arbre des groupes et des composants ; un filtre retrouve un
élément par son nom, et l'œil le masque ou l'affiche.

## Classer

L'outil **Classificateur** attribue un type IFC (mur, dalle, fenêtre…) à un groupe ou à un
composant, d'après les classements IFC 2x3 et IFC 4 livrés (**Fenêtre › Infos sur le modèle ›
Classements**). Ces types sont gardés dans le `.skp`.
