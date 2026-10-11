<#
.SYNOPSIS
  Installs DeskmateTV on an Android / Google TV over network ADB and pairs it
  with Home Assistant (Deskmate Link).

.DESCRIPTION
  One run does everything that needs ADB, so debugging can be switched off
  on the TV afterwards:
    1. adb connect <TV>:5555 (confirm "Allow USB debugging?" on the TV)
    2. installs the APK, then reinstalls it with itself as the installer of
       record (later self-updates from Home Assistant need no clicks)
    3. grants: accessibility service, notification access (now playing),
       WRITE_SECURE_SETTINGS (watchdog), install packages (self-update),
       battery optimisation exemption
    4. pairing: a DMP1 pairing code from Home Assistant (given, or created
       through the REST config flow with an admin long-lived token)
    5. verification: services enabled, connected to Home Assistant

  The Deskmate Link integration in Home Assistant must already include
  DeskmateTV support (this repository, custom_components/deskmate_link).

.EXAMPLE
  .\install.ps1 -TvIp 192.168.1.40 -HaUrl http://192.168.1.10:8123 -PairingCode DMP1.eyJrIjoi...
.EXAMPLE
  .\install.ps1 -TvIp 192.168.1.40 -HaUrl http://192.168.1.10:8123 -TokenFile .\ha-token.txt
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$TvIp,
    [Parameter(Mandatory = $true)][string]$HaUrl,
    [int]$Port = 5555,
    [string]$WsUrl = "",
    [string]$Apk = "",
    [string]$PairingCode = "",
    [string]$TokenFile = "",
    [string]$Node = "tv",
    [string]$Name = "DeskmateTV",
    [string]$Assistant = "",
    [string]$Adb = "",
    [switch]$SkipInstall,
    [switch]$SkipPairing
)

$ErrorActionPreference = "Stop"
$Pkg = "com.wawrzola.deskmatetv"
$A11y = "$Pkg/$Pkg.service.TvAccessibilityService"
$Listener = "$Pkg/$Pkg.service.MediaListenerService"
$Root = Split-Path -Parent $PSScriptRoot
$HaUrl = $HaUrl.TrimEnd("/")
if (-not $WsUrl) { $WsUrl = $HaUrl -replace '^http', 'ws' }
if (-not $Adb) {
    $sdkAdb = if ($env:ANDROID_HOME) { Join-Path $env:ANDROID_HOME "platform-tools\adb.exe" } else { "" }
    $Adb = if ($sdkAdb -and (Test-Path $sdkAdb)) { $sdkAdb } else { (Get-Command adb -ErrorAction SilentlyContinue).Source }
}
if (-not $Adb) { throw "adb not found. Install Android platform-tools or pass -Adb <path to adb.exe>." }
if (-not $Apk) {
    $Apk = Get-ChildItem (Join-Path $Root "dist") -Filter "DeskmateTV-*.apk" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
$Serial = "${TvIp}:$Port"

function Step($n, $text) { Write-Host ""; Write-Host "[$n] $text" -ForegroundColor Cyan }
function Ok($text) { Write-Host "    OK  $text" -ForegroundColor Green }
function Warn($text) { Write-Host "    !!  $text" -ForegroundColor Yellow }
function Shell([string]$cmd) {
    $out = & $Adb -s $Serial shell $cmd 2>&1 | Out-String
    return $out.Trim()
}

# --- 1. ADB connection
Step 1 "ADB connection to $Serial"
& $Adb connect $Serial | Out-Host
$state = ""
for ($i = 0; $i -lt 120; $i++) {
    $state = (& $Adb -s $Serial get-state 2>&1 | Out-String).Trim()
    if ($state -eq "device") { break }
    if ($i -eq 0 -or $i % 15 -eq 0) {
        Warn "state: $state - on the TV confirm 'Allow USB debugging?' (tick 'Always allow')"
    }
    Start-Sleep -Seconds 1
    if ($state -match "offline|not found|cannot") { & $Adb connect $Serial | Out-Null }
}
if ($state -ne "device") { throw "The TV is not reachable over ADB ($state). Check debugging and the IP." }
$sdk = Shell "getprop ro.build.version.sdk"
$model = Shell "getprop ro.product.model"
$release = Shell "getprop ro.build.version.release"
Ok "connected: $model, Android $release (SDK $sdk)"
if ([int]$sdk -lt 28) { throw "DeskmateTV needs Android 9+ (SDK 28)." }
if ([int]$sdk -lt 33) { Warn "Android < 13: D-pad keys need a remote integration in Home Assistant." }

# --- 2. install
if (-not $SkipInstall) {
    Step 2 "Installing the APK"
    if (-not $Apk -or -not (Test-Path $Apk)) { throw "No APK. Build it with scripts\build.ps1 (output in dist\)." }
    Write-Host "    file: $Apk"
    & $Adb -s $Serial install -r $Apk | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "adb install failed" }
    # Reinstalling with -i makes the app its own installer of record, so
    # self-updates pushed from Home Assistant install without a confirmation.
    & $Adb -s $Serial install -r -i $Pkg $Apk | Out-Host
    if ($LASTEXITCODE -ne 0) { Warn "installer of record not set (the first self-update may show a confirmation; the app clicks it itself)" }
    $where = Shell "pm path $Pkg"
    Ok "installed ($where)"
    if ($where -match "/mnt/expand") { Warn "the app landed on adopted storage despite internalOnly" }
}

# --- 3. permissions
Step 3 "Permissions"
Shell "pm grant $Pkg android.permission.WRITE_SECURE_SETTINGS" | Out-Null
Ok "WRITE_SECURE_SETTINGS (service watchdog)"
Shell "appops set $Pkg REQUEST_INSTALL_PACKAGES allow" | Out-Null
Ok "install packages (self-update)"
Shell "cmd notification allow_listener $Listener" | Out-Null
Ok "notification access (now playing)"
$cur = Shell "settings get secure enabled_accessibility_services"
if ($cur -eq "null" -or -not $cur) { $cur = "" }
if ($cur -notmatch [regex]::Escape($A11y) -and $cur -notmatch "$Pkg/\.service\.TvAccessibilityService") {
    $new = if ($cur) { "${cur}:$A11y" } else { $A11y }
    Shell "settings put secure enabled_accessibility_services '$new'" | Out-Null
}
Shell "settings put secure accessibility_enabled 1" | Out-Null
Ok "accessibility service"
Shell "dumpsys deviceidle whitelist +$Pkg" | Out-Null
Ok "battery optimisation exemption"
# First start clears the "stopped" state and shows the diagnostics screen
Shell "am start -n $Pkg/.ui.StatusActivity" | Out-Null
Start-Sleep -Seconds 2

# --- 4. pairing
if (-not $SkipPairing) {
    Step 4 "Pairing with Home Assistant"
    if (-not $PairingCode) {
        if (-not $TokenFile) { throw "Pass -PairingCode (HA: Add integration > Deskmate Link) or -TokenFile." }
        $token = (Get-Content $TokenFile -Raw).Trim()
        $headers = @{ Authorization = "Bearer $token" }
        $flow = Invoke-RestMethod -Method Post -Uri "$HaUrl/api/config/config_entries/flow" -Headers $headers `
            -ContentType "application/json" -Body '{"handler":"deskmate_link","show_advanced_options":false}'
        $flow = Invoke-RestMethod -Method Post -Uri "$HaUrl/api/config/config_entries/flow/$($flow.flow_id)" -Headers $headers `
            -ContentType "application/json" -Body '{}'
        if ($flow.step_id -ne "show_key") { throw "Unexpected Home Assistant flow step: $($flow.step_id)" }
        $PairingCode = $flow.description_placeholders.code
        $done = Invoke-RestMethod -Method Post -Uri "$HaUrl/api/config/config_entries/flow/$($flow.flow_id)" -Headers $headers `
            -ContentType "application/json" -Body '{}'
        Ok "pairing code from Home Assistant ($($done.type) $($done.reason))"
    }
    if ($PairingCode -notmatch '^DMP1\.[A-Za-z0-9_-]+$') { throw "The pairing code must look like DMP1.xxxx" }
    $extra = if ($Assistant) { " --es assistant '$Assistant'" } else { "" }
    $res = Shell "am broadcast -a $Pkg.PROVISION -n $Pkg/.service.ControlReceiver -f 0x10000020 --es code $PairingCode --es node $Node --es name '$Name' --es url $WsUrl$extra"
    if ($res -notmatch 'result=-1') { throw "The TV rejected the pairing: $res" }
    Ok (($res -split 'data=')[-1])
}

# --- 5. verification
Step 5 "Verification"
$status = $null
for ($i = 0; $i -lt 30; $i++) {
    $raw = Shell "am broadcast -a $Pkg.STATUS -n $Pkg/.service.ControlReceiver -f 0x10000020"
    if ($raw -match 'data="(.*)"') {
        try { $status = $Matches[1] | ConvertFrom-Json } catch { $status = $null }
    }
    if ($status -and $status.connected -and $status.a11y) { break }
    Start-Sleep -Seconds 2
}
if (-not $status) { throw "No STATUS answer from DeskmateTV." }
$checks = [ordered]@{
    "accessibility service running" = $status.a11y
    "notification access"           = $status.media
    "watchdog (WRITE_SECURE)"       = $status.write_secure
    "self-update allowed"           = $status.install_allowed
    "connected to Home Assistant"   = $status.connected
}
$allOk = $true
foreach ($k in $checks.Keys) {
    if ($checks[$k]) { Ok $k } else { Warn "$k - NO"; $allOk = $false }
}
if ($status.last_error) { Warn "last Link error: $($status.last_error)" }
Write-Host "    version $($status.version), SDK $($status.sdk), D-pad: $($status.dpad), node: $($status.node)"

Write-Host ""
if ($allOk) {
    Write-Host "DONE. You can switch USB debugging off on the TV now." -ForegroundColor Green
} else {
    Write-Host "Installed with warnings - see above. Keep ADB on until fixed." -ForegroundColor Yellow
}
& $Adb disconnect $Serial | Out-Null
