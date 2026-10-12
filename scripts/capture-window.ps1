param([Parameter(Mandatory)][long]$WindowHandle,[Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SpacefoldScreenCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
}
'@
[SpacefoldScreenCapture]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
[SpacefoldScreenCapture]::SetForegroundWindow([IntPtr]$WindowHandle) | Out-Null
[SpacefoldScreenCapture]::SetWindowPos([IntPtr]$WindowHandle,[IntPtr](-1),0,0,0,0,0x0003) | Out-Null
Start-Sleep -Milliseconds 150
$taskRect=New-Object SpacefoldScreenCapture+Rect
[SpacefoldScreenCapture]::GetWindowRect([IntPtr]$WindowHandle,[ref]$taskRect) | Out-Null
$taskBitmap=New-Object System.Drawing.Bitmap(($taskRect.Right-$taskRect.Left),($taskRect.Bottom-$taskRect.Top))
$taskGraphics=[System.Drawing.Graphics]::FromImage($taskBitmap)
try {$taskGraphics.CopyFromScreen($taskRect.Left,$taskRect.Top,0,0,$taskBitmap.Size);$taskBitmap.Save([IO.Path]::GetFullPath($OutputPath),[System.Drawing.Imaging.ImageFormat]::Png)} finally {$taskGraphics.Dispose();$taskBitmap.Dispose();[SpacefoldScreenCapture]::SetWindowPos([IntPtr]$WindowHandle,[IntPtr](-2),0,0,0,0,0x0003) | Out-Null}
Write-Output $OutputPath
