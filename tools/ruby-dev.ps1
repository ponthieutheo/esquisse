# Met un runtime Ruby 3.2 là où les compilations de développement le cherchent :
# target\debug\ruby (esquisse.exe de debug, tests de `cargo test`, exemples) et, avec
# -Release, target\release\ruby. Esquisse ne cherche plus Ruby ailleurs que dans son propre
# dossier (ou ESQ_RUBY_DLL) : à lancer une fois par copie du dépôt, et après un
# `cargo clean`.
#
#   powershell -File tools\ruby-dev.ps1 -Ruby <dossier de RubyInstaller 3.2 x64-ucrt>
#   (sans -Ruby : la racine du Ruby de ESQ_RUBY_DLL)

param([string]$Ruby = '', [switch]$Release)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'ruby-runtime.ps1')

$repo = Split-Path -Parent $PSScriptRoot
$from = Resolve-RubyRoot $Ruby
$profiles = @('debug')
if ($Release) { $profiles += 'release' }
foreach ($p in $profiles) {
    $to = Join-Path $repo "target\$p\ruby"
    Copy-RubyRuntime -From $from -To $to -Repo $repo
    "Ruby 3.2 copié dans $to ($(Get-FolderMB $to) Mo)"
}
