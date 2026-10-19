# Monte le dossier de distribution d'Esquisse, qui marche sur un PC sans SketchUp :
#
#   Esquisse\
#     esquisse.exe                compilé en release
#     ruby\                       le runtime Ruby 3.2 (DLL, bibliothèque standard, gems, licences)
#     Materials\ Styles\ Environments\ Templates\ Classifications\ HatchPatterns\
#                                 le contenu par défaut (dossier content\ du dépôt)
#     LICENCE.txt                 licence d'Esquisse (MIT)
#     licences\                   bibliothèques Rust, schémas IFC de buildingSMART
#     LISEZMOI.txt
#
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1 -Ruby <RubyInstaller 3.2 x64-ucrt>
#     -Ruby     racine d'un RubyInstaller 3.2 x64-ucrt (sinon celle de ESQ_RUBY_DLL)
#     -Out      dossier à créer (par défaut target\paquet\Esquisse ; un autre dossier doit être
#               vide ou absent : rien n'est jamais effacé hors de target\)
#     -NoBuild  reprend esquisse.exe déjà compilé par un passage précédent
#
# L'exécutable est compilé dans target\paquet-build avec --remap-path-prefix : il ne garde pas
# les chemins de la machine de compilation (dépôt, registre de Cargo) dans ses messages. Le CRT
# de Visual C++ y est lié statiquement (+crt-static) : un Windows neuf n'a pas forcément le
# redistribuable (VCRUNTIME140.dll) ; l'UCRT, elle, fait partie de Windows 10 et 11.
# Les licences des bibliothèques Rust viennent de `cargo metadata --offline` et des sources
# déjà téléchargées par Cargo (exemple crates\esq-app\examples\licences.rs) : sans réseau.

param([string]$Ruby = '', [string]$Out = '', [switch]$NoBuild)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'ruby-runtime.ps1')

$repo = Split-Path -Parent $PSScriptRoot
$rubyRoot = Resolve-RubyRoot $Ruby
$buildDir = Join-Path $repo 'target\paquet-build'
$triple = 'x86_64-pc-windows-msvc'
$defaultOut = Join-Path $repo 'target\paquet\Esquisse'
if (-not $Out) { $Out = $defaultOut }
$Out = [System.IO.Path]::GetFullPath($Out)

# Lance cargo : sa sortie d'erreur (la progression) ne doit pas passer pour une erreur de
# PowerShell 5.1 quand la sortie du script est redirigée ; seul son code de retour compte.
function Invoke-Cargo([string[]]$CargoArgs, [string]$What) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & cargo @CargoArgs 2>&1 | ForEach-Object { "$_" }
    } finally {
        $ErrorActionPreference = $old
    }
    if ($LASTEXITCODE -ne 0) { throw "$What a échoué" }
}

# 1. Compilation en release, chemins de la machine retirés.
if (-not $NoBuild) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
    $sep = [char]0x1f
    # (Avec --target, ces options ne touchent que l'exécutable, pas les scripts de compilation.)
    $env:CARGO_ENCODED_RUSTFLAGS = "-Ctarget-feature=+crt-static$sep--remap-path-prefix=$repo=esquisse$sep--remap-path-prefix=$cargoHome=cargo"
    $env:CARGO_TARGET_DIR = $buildDir
    Push-Location $repo
    try {
        Invoke-Cargo @('build', '--release', '-p', 'esq-app', '--bin', 'esquisse', '--target', $triple) 'cargo build'
    } finally {
        Pop-Location
        Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
}
$exe = Join-Path $buildDir "$triple\release\esquisse.exe"
if (-not (Test-Path -LiteralPath $exe)) { throw "$exe introuvable (lancer sans -NoBuild)" }
$imports = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($exe))
if ($imports.Contains('VCRUNTIME140')) { throw "$exe dépend encore de VCRUNTIME140.dll (CRT non statique)" }

# 2. Dossier de sortie neuf.
if (Test-Path -LiteralPath $Out) {
    $inTarget = $Out.StartsWith((Join-Path $repo 'target\paquet') + '\', [System.StringComparison]::OrdinalIgnoreCase)
    if ($inTarget) {
        Remove-Item -LiteralPath $Out -Recurse -Force
    } elseif (Get-ChildItem -LiteralPath $Out -Force | Select-Object -First 1) {
        throw "$Out n'est pas vide : choisissez un dossier vide ou absent."
    }
}
New-Item -ItemType Directory -Force $Out | Out-Null

# 3. L'exécutable, Ruby, le contenu.
Copy-Item -LiteralPath $exe -Destination (Join-Path $Out 'esquisse.exe')
Copy-RubyRuntime -From $rubyRoot -To (Join-Path $Out 'ruby') -Repo $repo
foreach ($d in Get-ChildItem -LiteralPath (Join-Path $repo 'content') -Directory) {
    Copy-Item -LiteralPath $d.FullName -Destination (Join-Path $Out $d.Name) -Recurse
}

# 4. Licences.
$lic = Join-Path $Out 'licences'
New-Item -ItemType Directory -Force $lic | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE-MIT') -Destination (Join-Path $Out 'LICENCE.txt')
Copy-Item -LiteralPath (Join-Path $repo 'tools\content\ifc\SOURCE.txt') -Destination (Join-Path $lic 'buildingSMART-IFC.txt')
Push-Location $repo
try {
    Invoke-Cargo @('run', '--quiet', '-p', 'esq-app', '--example', 'licences', '--', (Join-Path $lic 'rust-crates.txt')) 'la liste des licences Rust'
} finally {
    Pop-Location
}

# 5. Lisez-moi.
$readme = @"
Esquisse — modeleur 3D libre (licence MIT), compatible avec SketchUp.

Lancer esquisse.exe. Rien n'est à installer : tout ce qu'il faut est dans ce dossier.

  esquisse.exe        l'application
  ruby\               Ruby 3.2 (RubyInstaller), pour les extensions ; ses licences dans ruby\licences
  Materials\          bibliothèque de matières (Fenêtre › Matières)
  Styles\             bibliothèque de styles (Fenêtre › Styles)
  Environments\       environnements (Fenêtre › Environnements)
  Templates\          modèles types (fenêtre de bienvenue)
  Classifications\    IFC 2x3 et IFC 4 de buildingSMART (Infos sur le modèle › Classements)
  HatchPatterns\      motifs de hachures des coupes
  LICENCE.txt         licence d'Esquisse
  licences\           licences des bibliothèques et des schémas inclus

Les préférences et les fichiers de l'utilisateur vont dans %APPDATA%\Esquisse.
La lecture et l'écriture des fichiers .skp passent encore par SketchUpAPI.dll quand SketchUp
est installé ; sans lui, Esquisse enregistre au format .esq.
"@
$utf8bom = New-Object System.Text.UTF8Encoding $true
[System.IO.File]::WriteAllText((Join-Path $Out 'LISEZMOI.txt'), $readme.Replace("`n", "`r`n").Replace("`r`r", "`r"), $utf8bom)

# 6. Bilan.
"Paquet : $Out"
"  esquisse.exe  $([math]::Round((Get-Item -LiteralPath (Join-Path $Out 'esquisse.exe')).Length / 1MB, 1)) Mo"
"  ruby          $(Get-FolderMB (Join-Path $Out 'ruby')) Mo"
foreach ($d in 'Materials', 'Styles', 'Environments', 'Templates', 'Classifications', 'HatchPatterns', 'licences') {
    $p = Join-Path $Out $d
    if (Test-Path -LiteralPath $p) { "  {0,-13} {1} Mo" -f $d, (Get-FolderMB $p) }
}
"  total         $(Get-FolderMB $Out) Mo"
