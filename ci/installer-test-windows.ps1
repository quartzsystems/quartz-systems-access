# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.

# Installer test for Windows: install, double-click a session file, upgrade,
# repair and uninstall, silently, then check that nothing is left that was
# not there before (apart from user data). See TESTING.md.
#
#   ci\installer-test-windows.ps1 -Installer target\dist\QS400-Access-V1R2M0-windows-x86_64.exe [-Scope user|machine]
#
# -Scope machine must run as administrator. Exits 0 when every check passes.

param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [ValidateSet('user', 'machine')][string]$Scope = 'user'
)

$ErrorActionPreference = 'Stop'
$Installer = (Resolve-Path $Installer).Path
# The release in the file name, QS400-Access-VxRyMz-..., gives the version
# the binaries must carry, x.y.z (VxRyMz).
if ((Split-Path -Leaf $Installer) -notmatch '-V(\d+)R(\d+)M(\d+)-') { throw "no VxRyMz release in $Installer" }
$version = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
$productVersion = "$version (V$($Matches[1])R$($Matches[2])M$($Matches[3]))"
$failures = New-Object System.Collections.Generic.List[string]
function Check($ok, $what) {
    if ($ok) { Write-Host "ok    $what" } else { Write-Host "FAIL  $what"; $failures.Add($what) }
}

$hive = if ($Scope -eq 'user') { 'HKCU:' } else { 'HKLM:' }
$dir = if ($Scope -eq 'user') { "$env:LOCALAPPDATA\Programs\Quartz Systems\Access" } else { "$env:ProgramFiles\Quartz Systems\Access" }
$programs = if ($Scope -eq 'user') { [Environment]::GetFolderPath('Programs') } else { [Environment]::GetFolderPath('CommonPrograms') }
$desktop = if ($Scope -eq 'user') { [Environment]::GetFolderPath('Desktop') } else { [Environment]::GetFolderPath('CommonDesktopDirectory') }
$settingsDir = "$env:APPDATA\Quartz Systems\Access"

# What an install can touch: these keys and folders, listed in full.
$keys = @(
    "$hive\Software\Classes\.qsd", "$hive\Software\Classes\.qsp", "$hive\Software\Classes\.qkm", "$hive\Software\Classes\.qsm", "$hive\Software\Classes\.qdx",
    "$hive\Software\Classes\QuartzSystems.Access.Session.1",
    "$hive\Software\Classes\QuartzSystems.Access.PrinterSession.1",
    "$hive\Software\Classes\QuartzSystems.Access.KeyMap.1",
    "$hive\Software\Classes\QuartzSystems.Access.Macro.1",
    "$hive\Software\Classes\QuartzSystems.Access.Transfer.1",
    "$hive\Software\Microsoft\Windows\CurrentVersion\Uninstall\QuartzSystemsAccess"
)
# Folders listed in full, and folders listed one level deep (a desktop can
# hold large trees, and a shortcut only ever goes at its top).
$folders = @((Split-Path $dir), "$programs\Quartz Systems")
$shallow = @($desktop, $programs)

function Snapshot {
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($k in $keys) {
        if (Test-Path $k) {
            Get-ChildItem $k -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $lines.Add("key $($_.Name)") }
            $lines.Add("key $k")
        }
    }
    foreach ($f in $folders) {
        if (Test-Path $f) {
            $lines.Add("dir $f")
            Get-ChildItem $f -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object { $lines.Add("file $($_.FullName)") }
        }
    }
    foreach ($f in $shallow) {
        Get-ChildItem $f -Force -ErrorAction SilentlyContinue | ForEach-Object { $lines.Add("file $($_.FullName)") }
    }
    return $lines | Sort-Object
}

function Run($exe, [string[]]$arguments) {
    $p = Start-Process -FilePath $exe -ArgumentList $arguments -Wait -PassThru
    return $p.ExitCode
}

# A running copy would save its own settings over the restored ones when it
# closes; start with none running.
if (Get-Process qsaccess -ErrorAction SilentlyContinue) {
    Write-Host "QS/400 Access for Desktop is running; close it and run the test again"
    exit 2
}

# Settings are user data: keep a copy and put it back afterwards, so that
# the double-click below leaves no trace in Recent Sessions.
$backup = Join-Path $env:TEMP "qsa-test-settings-$PID"
if (Test-Path $settingsDir) { Copy-Item $settingsDir $backup -Recurse -Force }

$before = Snapshot
Check (-not (Test-Path "$dir\install-manifest.toml")) "nothing installed before the test"

# A damaged installer is refused before anything changes.
$damaged = Join-Path $env:TEMP "qsa-damaged-$PID.exe"
$bytes = [IO.File]::ReadAllBytes($Installer)
$bytes[$bytes.Length - 1000] = $bytes[$bytes.Length - 1000] -bxor 0xFF
[IO.File]::WriteAllBytes($damaged, $bytes)
Check ((Run $damaged @('--silent', "--scope=$Scope")) -eq 2) "a damaged installer exits 2"
Remove-Item $damaged
Check ((Compare-Object $before (Snapshot)) -eq $null) "a damaged installer changes nothing"

# Install.
Check ((Run $Installer @('--silent', "--scope=$Scope")) -eq 0) "silent install exits 0"
Check (Test-Path "$dir\qsaccess.exe") "qsaccess.exe installed"
Check (Test-Path "$dir\uninstall.exe") "uninstall.exe installed"
Check (Test-Path "$dir\install-manifest.toml") "install manifest written"
$v = (Get-Item "$dir\qsaccess.exe").VersionInfo
Check ($v.ProductName -eq 'QS/400 Access for Desktop' -and $v.CompanyName -eq 'Quartz Systems' -and $v.ProductVersion -eq $productVersion) "version information embedded"
$cmd = (Get-ItemProperty "$hive\Software\Classes\QuartzSystems.Access.Session.1\shell\open\command").'(default)'
Check ($cmd -eq "`"$dir\qsaccess.exe`" open `"%1`"") "open command for .qsd"
Check ((Get-ItemProperty "$hive\Software\Classes\.qsd").'(default)' -eq 'QuartzSystems.Access.Session.1') ".qsd names its ProgID"
$cmd = (Get-ItemProperty "$hive\Software\Classes\QuartzSystems.Access.PrinterSession.1\shell\open\command").'(default)'
Check ($cmd -eq "`"$dir\qsaccess.exe`" open `"%1`"") "open command for .qsp"
Check ((Get-ItemProperty "$hive\Software\Classes\.qsp").'(default)' -eq 'QuartzSystems.Access.PrinterSession.1') ".qsp names its ProgID"
Check ((Get-ItemProperty "$hive\Software\Classes\QuartzSystems.Access.PrinterSession.1\DefaultIcon").'(default)' -like '*application-x-qsaccess-printer*') ".qsp has its own icon"
Check (Test-Path "$programs\Quartz Systems\QS400 Access for Desktop.lnk") "Start menu shortcut"
Check (Test-Path "$programs\Quartz Systems\Session Manager.lnk") "Session Manager shortcut"
Check (Test-Path "$desktop\QS400 Access for Desktop.lnk") "desktop shortcut"
$un = Get-ItemProperty "$hive\Software\Microsoft\Windows\CurrentVersion\Uninstall\QuartzSystemsAccess"
Check ($un.DisplayName -eq 'QS/400 Access for Desktop' -and $un.Publisher -eq 'Quartz Systems' -and $un.DisplayVersion -eq $version) "Apps entry"

# Double-click a session file: the shell runs "qsaccess.exe open <file>".
# It names a configured system, so that no "Create system" question comes
# up: the first one there is, else a temporary one for the QS/400 test system.
$madeSettings = $false
$system = '10.42.70.150'
if (Test-Path "$settingsDir\systems.toml") {
    $m = Select-String -Path "$settingsDir\systems.toml" -Pattern '^name = "(.*)"' | Select-Object -First 1
    if ($m) { $system = $m.Matches[0].Groups[1].Value }
} else {
    New-Item -ItemType Directory -Force $settingsDir | Out-Null
    "[[system]]`nname = `"$system`"`ndefault_user = `"QATEST`"`n" | Set-Content -Encoding utf8 "$settingsDir\systems.toml"
    $madeSettings = $true
}
$qsd = Join-Path $env:TEMP "qsa-test-$PID.qsd"
@"
[qsaccess]
kind = "display"
version = 1

[system]
name = "$system"
host = "10.42.70.150"

[session]
title = "Installer test"
auto_connect = false
"@ | Set-Content -Encoding utf8 $qsd
if ($Scope -eq 'user') {
    Start-Process $qsd
    Start-Sleep 4
    $p = Get-CimInstance Win32_Process -Filter "Name = 'qsaccess.exe'" | Where-Object { $_.CommandLine -like "*open*qsa-test-$PID.qsd*" }
    Check ($p -ne $null) "double-clicking a .qsd runs qsaccess open"
    # A printer session file opens in the same running instance.
    $qsp = Join-Path $env:TEMP "qsa-test-$PID.qsp"
    "[qsaccess]`nkind = `"printer`"`nversion = 1`n`n[system]`nname = `"$system`"`nhost = `"10.42.70.150`"`n`n[printer]`ndevice_name = `"QSATSTPRT`"`n`n[session]`nauto_connect = false`n" | Set-Content -Encoding utf8 $qsp
    # The second qsaccess hands the file over and exits at once, so look
    # for the running one's "opening" line in qsaccess.log, not a process.
    # The file name alone: the shell may pass a short (8.3) TEMP path.
    $primary = @($p)[0].ProcessId
    $log = "$settingsDir\qsaccess.log"
    Start-Process $qsp
    $opened = $false
    for ($i = 0; $i -lt 20 -and -not $opened; $i++) {
        Start-Sleep -Milliseconds 500
        $opened = (Test-Path $log) -and (Select-String -Path $log -Pattern "\[$primary\] opening .*qsa-test-$PID\.qsp$" -Quiet)
    }
    Check $opened "double-clicking a .qsp opens it in the running qsaccess"
    Remove-Item $qsp
    Start-Process "$dir\qsaccess.exe" -ArgumentList '--quit' -Wait
    Start-Sleep 2
    Check ((Get-Process qsaccess -ErrorAction SilentlyContinue) -eq $null) "qsaccess --quit closes it"
}
Remove-Item $qsd

# Upgrade over the top, then repair.
Check ((Run $Installer @('--silent', "--scope=$Scope")) -eq 0) "upgrade over the top exits 0"
Check ((Run "$dir\uninstall.exe" @('--repair', '--silent')) -eq 0) "repair exits 0"
Start-Sleep 5
Check (Test-Path "$dir\qsaccess.exe") "repair keeps the program"
Check (Test-Path "$dir\uninstall.exe") "repair keeps the uninstaller"

# Uninstall.
Check ((Run "$dir\uninstall.exe" @('--uninstall', '--silent')) -eq 0) "silent uninstall exits 0"
Start-Sleep 6
$diff = Compare-Object $before (Snapshot)
if ($diff) { $diff | ForEach-Object { Write-Host "      $($_.SideIndicator) $($_.InputObject)" } }
Check ($diff -eq $null) "nothing left behind after uninstall"
Check ((Test-Path "$settingsDir\systems.toml") -or -not (Test-Path "$backup\systems.toml")) "user data kept"

if (Test-Path $backup) {
    Copy-Item "$backup\*" $settingsDir -Recurse -Force
    Remove-Item $backup -Recurse -Force
} elseif ($madeSettings) {
    Remove-Item $settingsDir -Recurse -Force
    $parent = Split-Path $settingsDir
    if (-not (Get-ChildItem $parent -Force -ErrorAction SilentlyContinue)) { Remove-Item $parent }
}

if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) check(s) failed"
    exit 1
}
Write-Host "every check passed"
exit 0
