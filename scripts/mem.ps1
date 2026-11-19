# Measure Calen's memory: A = main window open, B = closed to the tray.
# Usage: pwsh scripts/mem.ps1 [-Exe target\release\calen.exe] [-Backend wgpu|tiny-skia]
param(
    [string]$Exe = "target\release\calen.exe",
    [string]$Backend = "",
    [int]$Settle = 10,
    [int]$TrayWait = 30
)
$data = Join-Path $env:TEMP ("calen-mem-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $data | Out-Null
$env:CALEN_DATA_DIR = $data
if ($Backend) { $env:ICED_BACKEND = $Backend } else { Remove-Item Env:ICED_BACKEND -ErrorAction SilentlyContinue }

function Sample($p, $label) {
    $p.Refresh()
    [pscustomobject]@{
        State      = $label
        Backend    = $(if ($Backend) { $Backend } else { "default" })
        PrivateMB  = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
        WorkingMB  = [math]::Round($p.WorkingSet64 / 1MB, 1)
    }
}

$p = Start-Process -FilePath $Exe -ArgumentList "--seed" -PassThru
Start-Sleep -Seconds $Settle
$a = Sample $p "A: window open"
$null = $p.CloseMainWindow()   # same as clicking close -> goes to the tray
Start-Sleep -Seconds $TrayWait
$b = Sample $p "B: in tray"
Stop-Process -Id $p.Id -Force
Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
$a, $b | Format-Table -AutoSize
