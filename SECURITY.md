# Sécurité / Security

## Français

Esquisse ouvre des fichiers venus de n'importe où (modèles, bibliothèques, archives) et charge
des extensions Ruby. Les règles qu'il applique à ces données sont décrites dans
[`docs/securite.md`](docs/securite.md).

Pour signaler une faille, utilisez le signalement privé de GitHub (onglet **Security**, puis
**Report a vulnerability**) plutôt qu'une issue publique. Joignez si possible un fichier qui la
reproduit et la version (ou le commit) concernée. Seule la dernière version de la branche
`main` reçoit des correctifs.

Une extension Ruby a les mêmes droits que l'application : n'installez que des extensions dont
vous connaissez la source.

## English

Esquisse opens files from anywhere (models, libraries, archives) and loads Ruby extensions.
The rules it applies to such data are described (in French) in
[`docs/securite.md`](docs/securite.md).

To report a vulnerability, please use GitHub's private reporting (**Security** tab, then
**Report a vulnerability**) rather than a public issue. If you can, attach a file that
reproduces it and the affected version or commit. Only the latest `main` branch receives fixes.

A Ruby extension has the same rights as the application: only install extensions whose source
you trust.
