# Télécharge le corpus de fichiers .skp réels (écrits par l'application SketchUp) que liste
# crates\esq-skp\tests\data\skp-reel.json, pour le test native_reel (lecteur natif face au
# SDK) et native_mutations. Chaque fichier vient de l'URL de son entrée (ou de « brut » de sa
# source suivi de son chemin) ; son SHA-256 est vérifié ; un fichier déjà présent et intact
# n'est pas retéléchargé. Les fichiers ne sont que des données : rien n'y est exécuté.
#
#   powershell -File tools\skp-reel.ps1 [-Dest <dossier>]   (défaut : %USERPROFILE%\tools\skp-reel)

param([string]$Dest = '')
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Dest) {
    $Dest = if ($env:ESQ_SKP_REEL) { $env:ESQ_SKP_REEL } else { Join-Path $env:USERPROFILE 'tools\skp-reel' }
}
$manifest = Join-Path $repo 'crates\esq-skp\tests\data\skp-reel.json'
$json = [IO.File]::ReadAllText($manifest, [Text.Encoding]::UTF8) | ConvertFrom-Json
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

$ok = 0; $got = 0; $bad = @()
foreach ($f in $json.fichiers) {
    $to = Join-Path $Dest ($f.fichier -replace '/', '\')
    if ((Test-Path -LiteralPath $to) -and (Get-Sha256 $to) -eq $f.sha256) { $ok++; continue }
    $url = $f.url
    if (-not $url) {
        $src = $json.sources.($f.source)
        $url = $src.brut + (($f.chemin -split '/' | ForEach-Object { [Uri]::EscapeDataString($_) }) -join '/')
    }
    $dir = Split-Path -Parent $to
    New-Item -ItemType Directory -Force $dir | Out-Null
    $tmp = "$to.part"
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $tmp
        if ((Get-Sha256 $tmp) -ne $f.sha256) { throw "SHA-256 différent" }
        Move-Item -Force -LiteralPath $tmp -Destination $to
        $got++
    } catch {
        if (Test-Path -LiteralPath $tmp) { Remove-Item -Force -LiteralPath $tmp }
        $bad += "$($f.fichier) : $($_.Exception.Message)"
    }
}
"$ok fichiers déjà présents, $got téléchargés, $($bad.Count) en échec (dans $Dest)"
$bad | ForEach-Object { "  $_" }
