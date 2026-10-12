param([Parameter(Mandatory)][int]$ProcessId,[switch]$Restore,[long]$WindowHandle=0)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SpacefoldNativeState {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 public delegate bool EnumProc(IntPtr h,IntPtr data);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder text,int max);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point p);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int mode);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h,uint message,IntPtr wParam,IntPtr lParam);
 public static int Hit(IntPtr h,int x,int y){return SendMessage(h,0x84,IntPtr.Zero,new IntPtr((y<<16)|(x&0xffff))).ToInt32();}
 public static IntPtr FindMain(uint pid){IntPtr found=IntPtr.Zero;EnumWindows((h,data)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var title=new System.Text.StringBuilder(256);GetWindowText(h,title,256);if(title.ToString().EndsWith("Spacefold",StringComparison.Ordinal)){found=h;return false;}}return true;},IntPtr.Zero);return found;}
 public static string Title(IntPtr h){var text=new System.Text.StringBuilder(256);GetWindowText(h,text,256);return text.ToString();}
}
'@
[SpacefoldNativeState]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$taskProcess=Get-Process -Id $ProcessId
$taskHandle=[IntPtr]$WindowHandle
if($taskHandle -eq 0){$taskHandle=[SpacefoldNativeState]::FindMain($ProcessId)}
if($taskHandle -eq 0){throw 'Native window handle not found'}
if($Restore){[SpacefoldNativeState]::ShowWindow($taskHandle,9) | Out-Null}
$taskRect=New-Object SpacefoldNativeState+Rect
[SpacefoldNativeState]::GetWindowRect($taskHandle,[ref]$taskRect) | Out-Null
$taskStyle=[SpacefoldNativeState]::GetWindowLongPtr($taskHandle,-16).ToInt64()
$taskClient=New-Object SpacefoldNativeState+Rect
$taskOrigin=New-Object SpacefoldNativeState+Point
[SpacefoldNativeState]::GetClientRect($taskHandle,[ref]$taskClient) | Out-Null
[SpacefoldNativeState]::ClientToScreen($taskHandle,[ref]$taskOrigin) | Out-Null
$taskMiddleX=[int](($taskRect.Left+$taskRect.Right)/2)
$taskMiddleY=[int](($taskRect.Top+$taskRect.Bottom)/2)
[ordered]@{
 pid=$ProcessId; handle=$taskHandle.ToInt64(); title=[SpacefoldNativeState]::Title($taskHandle); dpi=[SpacefoldNativeState]::GetDpiForWindow($taskHandle)
 minimized=[SpacefoldNativeState]::IsIconic($taskHandle); maximized=[SpacefoldNativeState]::IsZoomed($taskHandle)
 style=$taskStyle; hasCaption=(($taskStyle -band 0x00c00000) -ne 0); hasResizeFrame=(($taskStyle -band 0x00040000) -ne 0)
 bounds=[ordered]@{left=$taskRect.Left;top=$taskRect.Top;right=$taskRect.Right;bottom=$taskRect.Bottom}
 client=[ordered]@{x=$taskOrigin.X;y=$taskOrigin.Y;width=$taskClient.Right;height=$taskClient.Bottom}
 frameOffsets=[ordered]@{left=$taskOrigin.X-$taskRect.Left;top=$taskOrigin.Y-$taskRect.Top;bottom=$taskRect.Bottom-($taskOrigin.Y+$taskClient.Bottom)}
 edgeHitTest=[ordered]@{left=[SpacefoldNativeState]::Hit($taskHandle,$taskRect.Left+1,$taskMiddleY);right=[SpacefoldNativeState]::Hit($taskHandle,$taskRect.Right-2,$taskMiddleY);top=[SpacefoldNativeState]::Hit($taskHandle,$taskMiddleX,$taskRect.Top+1);bottom=[SpacefoldNativeState]::Hit($taskHandle,$taskMiddleX,$taskRect.Bottom-2)}
} | ConvertTo-Json -Depth 4 -Compress
