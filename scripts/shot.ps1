# Save a Calen window to a PNG (used for the README screenshots).
#
#   scripts/shot.ps1 -Out docs/screenshots/main.png              # largest window
#   scripts/shot.ps1 -Out docs/screenshots/flyout.png -Pick smallest
#
# Brings the window to the front and copies it from the screen. (PrintWindow
# isn't used: with the CPU renderer it left parts of the window black.)
# Run Calen with a scratch CALEN_DATA_DIR and --seed so only
# sample events show.
param(
    [Parameter(Mandatory)] [string] $Out,
    [ValidateSet('largest', 'smallest')] [string] $Pick = 'largest',
    [string] $ProcessName = 'calen',
    [int] $Id = 0
)
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Shot {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    public static List<IntPtr> Windows(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@
[void][Shot]::SetProcessDPIAware()

$proc = if ($Id) { Get-Process -Id $Id } else { Get-Process -Name $ProcessName | Select-Object -First 1 }
$wins = foreach ($h in [Shot]::Windows([uint32]$proc.Id)) {
    $r = New-Object Shot+RECT
    [void][Shot]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -gt 50 -and $hh -gt 50) { [pscustomobject]@{ Handle = $h; X = $r.L; Y = $r.T; W = $w; H = $hh; Area = $w * $hh } }
}
if (-not $wins) { throw "no visible $ProcessName window" }
$win = if ($Pick -eq 'largest') { $wins | Sort-Object Area -Descending | Select-Object -First 1 }
       else { $wins | Sort-Object Area | Select-Object -First 1 }

$bmp = New-Object System.Drawing.Bitmap $win.W, $win.H
$g = [System.Drawing.Graphics]::FromImage($bmp)
[void][Shot]::SetForegroundWindow($win.Handle)
Start-Sleep -Milliseconds 600
$g.CopyFromScreen($win.X, $win.Y, 0, 0, $bmp.Size)
$g.Dispose()
$dir = Split-Path -Parent $Out
if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
$bmp.Save($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"saved $Out ($($win.W)x$($win.H))"
