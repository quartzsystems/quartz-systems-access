# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.

# ci/version.sh from PowerShell, run with Git for Windows' bash (found next to
# git.exe). Plain `bash` on Windows is usually WSL's, which is not wanted.
#
#   ci\version.ps1 prepare 1.1.1
#   ci\version.ps1 check
#
# Takes the same arguments as ci/version.sh.

$ErrorActionPreference = 'Stop'
$git = (Get-Command git.exe -ErrorAction SilentlyContinue).Source
if (-not $git) { throw 'git is not on PATH: install Git for Windows' }
# git.exe is in <Git>\cmd or <Git>\bin (or <Git>\mingw64\bin).
$bash = $null
$dir = Split-Path $git
while ($dir -and -not $bash) {
    $candidate = Join-Path $dir 'bin\bash.exe'
    if (Test-Path $candidate) { $bash = $candidate }
    $dir = Split-Path $dir
}
if (-not $bash) { throw "no Git Bash found above $git" }
& $bash (Join-Path $PSScriptRoot 'version.sh') @args
exit $LASTEXITCODE
