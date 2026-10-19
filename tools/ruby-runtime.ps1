# Fonctions communes à package.ps1 et ruby-dev.ps1 : copier le runtime Ruby 3.2 de
# RubyInstaller (build x64-ucrt, celui de SketchUp) dans un dossier « ruby » rangé comme
# Esquisse le cherche : ruby\bin\x64-ucrt-ruby320.dll et ruby\lib\ruby\3.2.0 (Ruby trouve sa
# bibliothèque standard à côté de sa DLL).

$RubyDll = 'x64-ucrt-ruby320.dll'

# La racine d'un RubyInstaller 3.2 x64-ucrt : celle donnée, sinon celle de ESQ_RUBY_DLL.
function Resolve-RubyRoot([string]$Ruby) {
    if (-not $Ruby -and $env:ESQ_RUBY_DLL) {
        $Ruby = Split-Path -Parent (Split-Path -Parent $env:ESQ_RUBY_DLL)
    }
    if (-not $Ruby) {
        throw "Indiquez le Ruby 3.2 à copier : -Ruby <dossier de RubyInstaller 3.2 x64-ucrt> (ou posez ESQ_RUBY_DLL)."
    }
    $dll = Join-Path $Ruby "bin\$RubyDll"
    if (-not (Test-Path -LiteralPath $dll)) {
        throw "$dll introuvable : ce n'est pas un RubyInstaller 3.2 x64-ucrt."
    }
    (Resolve-Path -LiteralPath $Ruby).Path
}

# Copie dans $To (un dossier nommé « ruby », recréé) la DLL de Ruby, ses DLL d'accompagnement,
# la configuration d'OpenSSL, la bibliothèque standard, les gems (sans leur cache .gem ni la
# documentation) et les licences. $Repo : la racine du dépôt (textes de licence d'Esquisse).
function Copy-RubyRuntime([string]$From, [string]$To, [string]$Repo) {
    if ((Split-Path -Leaf $To) -ne 'ruby') { throw "Destination inattendue (doit s'appeler ruby) : $To" }
    if (Test-Path -LiteralPath $To) { Remove-Item -LiteralPath $To -Recurse -Force }
    $bin = Join-Path $To 'bin'
    $lib = Join-Path $To 'lib\ruby'
    $gems = Join-Path $lib 'gems\3.2.0'
    $lic = Join-Path $To 'licences'
    New-Item -ItemType Directory -Force $bin, $gems, $lic | Out-Null
    Copy-Item -LiteralPath (Join-Path $From "bin\$RubyDll") -Destination $bin
    foreach ($d in 'ruby_builtin_dlls', 'lib', 'etc') {
        $src = Join-Path $From "bin\$d"
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination (Join-Path $bin $d) -Recurse }
    }
    foreach ($d in '3.2.0', 'site_ruby', 'vendor_ruby') {
        $src = Join-Path $From "lib\ruby\$d"
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination (Join-Path $lib $d) -Recurse }
    }
    foreach ($d in 'specifications', 'gems', 'extensions', 'plugins') {
        $src = Join-Path $From "lib\ruby\gems\3.2.0\$d"
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination (Join-Path $gems $d) -Recurse }
    }
    # Licences : celles de la distribution, plus le texte BSD de Ruby et la liste des DLL.
    Copy-Item -LiteralPath (Join-Path $From 'LICENSE.txt') -Destination (Join-Path $lic 'LICENSE-RubyInstaller.txt')
    foreach ($f in 'COPYING.html', 'LEGAL.html') {
        $src = Join-Path $From "share\doc\ruby\html\$f"
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination $lic }
    }
    Copy-Item -LiteralPath (Join-Path $Repo 'tools\licences\ruby-BSDL.txt') -Destination (Join-Path $lic 'BSDL.txt')
    Copy-Item -LiteralPath (Join-Path $Repo 'tools\licences\ruby-runtime.txt') -Destination (Join-Path $lic 'LISEZMOI.txt')
    # libgmp-10.dll est sous LGPL v3 : son texte, et celui de la GPL v3 qu'elle complète.
    foreach ($f in 'LGPL-3.0.txt', 'GPL-3.0.txt') {
        Copy-Item -LiteralPath (Join-Path $Repo "tools\licences\$f") -Destination $lic
    }
}

# Taille d'un dossier, en Mo.
function Get-FolderMB([string]$Dir) {
    $sum = (Get-ChildItem -LiteralPath $Dir -Recurse -File | Measure-Object -Property Length -Sum).Sum
    [math]::Round($sum / 1MB, 1)
}
