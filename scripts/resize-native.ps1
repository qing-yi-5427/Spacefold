param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][int]$Width,[int]$Height=800)
$ErrorActionPreference='Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SpacefoldWindow {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int hgt,uint flags);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
}
'@
[SpacefoldWindow]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$taskWindow=(Get-Process -Id $ProcessId).MainWindowHandle
if($taskWindow -eq 0){throw 'Native window handle not found'}
$taskClient=New-Object SpacefoldWindow+Rect
$taskFrame=New-Object SpacefoldWindow+Rect
[SpacefoldWindow]::GetClientRect($taskWindow,[ref]$taskClient) | Out-Null
[SpacefoldWindow]::GetWindowRect($taskWindow,[ref]$taskFrame) | Out-Null
$taskDpi=[SpacefoldWindow]::GetDpiForWindow($taskWindow)
$taskScale=$taskDpi/96.0
$taskFrameWidth=$taskFrame.Right-$taskFrame.Left-($taskClient.Right-$taskClient.Left)
$taskFrameHeight=$taskFrame.Bottom-$taskFrame.Top-($taskClient.Bottom-$taskClient.Top)
$taskNewWidth=[int][Math]::Round($Width*$taskScale)+$taskFrameWidth
$taskNewHeight=[int][Math]::Round($Height*$taskScale)+$taskFrameHeight
[SpacefoldWindow]::SetWindowPos($taskWindow,[IntPtr]::Zero,0,0,$taskNewWidth,$taskNewHeight,0x0006) | Out-Null
Write-Output "Native client requested ${Width}x${Height} CSS px; DPI=$taskDpi"
