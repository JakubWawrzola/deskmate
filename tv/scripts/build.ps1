<#
.SYNOPSIS
  Builds DeskmateTV: crypto test vectors from the integration's crypto.py,
  unit tests and a signed release APK in dist\.

.DESCRIPTION
  Needs JDK 17+ (JAVA_HOME), the Android SDK (local.properties: sdk.dir) and
  Python with the `cryptography` package. The signing key is read from
  _prywatne\keystore.properties (never committed). Keep a backup of the key:
  updates only install over a build signed with the same certificate.
#>
param([string]$JavaHome = "")
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if ($JavaHome) { $env:JAVA_HOME = $JavaHome }
elseif (-not $env:JAVA_HOME -and (Test-Path "C:\Program Files\Android\openjdk\jdk-21.0.8")) {
    $env:JAVA_HOME = "C:\Program Files\Android\openjdk\jdk-21.0.8"
}
if ($env:JAVA_HOME) { $env:PATH = "$env:JAVA_HOME\bin;$env:PATH" }
if (-not (Test-Path (Join-Path $Root "_prywatne\keystore.properties"))) {
    throw "Missing _prywatne\keystore.properties - without it the release APK would be unsigned."
}
Push-Location $Root
try {
    python -I tools\gen_vectors.py
    .\gradlew.bat testDebugUnitTest assembleRelease --console=plain
    if ($LASTEXITCODE -ne 0) { throw "gradle failed" }
    $version = (Select-String -Path app\build.gradle.kts -Pattern 'versionName = "(.+)"').Matches[0].Groups[1].Value
    New-Item -ItemType Directory -Force dist | Out-Null
    $out = "dist\DeskmateTV-$version.apk"
    Copy-Item app\build\outputs\apk\release\app-release.apk $out -Force
    $hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLower()
    Write-Host "OK: $out  sha256=$hash"
} finally {
    Pop-Location
}
