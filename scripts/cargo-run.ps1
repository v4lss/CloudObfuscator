$cargoArgs = @($args)
if ($cargoArgs.Count -eq 0) { $cargoArgs = @("build") }
$retries = 10

$ErrorActionPreference = "Continue"
$mingw = "C:\Users\Vals\AppData\Local\Temp\opencode\tools\w64devkit\bin"
if (Test-Path -LiteralPath $mingw) { $env:PATH = "$env:USERPROFILE\.cargo\bin;$mingw;$env:PATH" }
else { $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH" }

function Format-Output($lines) {
    $lines |
        Where-Object { $_ -notmatch 'rerun-if-env-changed' } |
        Where-Object { $_ -notmatch '^\s*(CC|CFLAGS|AR|HOST_CC|CRATE_CC|CC_FORCE|CC_ENABLE|CC_x86|HOST_CFLAGS|CFLAGS_x86)=' } |
        Where-Object { $_ -notmatch 'NativeCommandError|FullyQualifiedErrorId|CategoryInfo|At line|^\s*\+|cargo.exe :' }
}

for ($i = 1; $i -le $retries; $i++) {
    $output = & cargo @cargoArgs 2>&1
    $code = $LASTEXITCODE
    if ($code -eq 0) {
        Format-Output $output | Select-Object -Last 40
        exit 0
    }
    $text = ($output | Out-String)
    if ($text -match 'os error 32' -or $text -match 'Blocking waiting for file lock') {
        Write-Host "transient file lock (attempt $i/$retries), retrying..."
        Get-Process -Name cargo, rustc -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 4
        continue
    }
    Format-Output $output | Select-Object -Last 90
    exit $code
}

Write-Host "failed after $retries attempts"
exit 1
