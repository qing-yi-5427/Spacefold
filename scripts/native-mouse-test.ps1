param([Parameter(Mandatory)][long]$WindowHandle,[Parameter(Mandatory)][int]$X,[Parameter(Mandatory)][int]$Y,[int]$DeltaX=90,[int]$DeltaY=60)
$ErrorActionPreference='Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Threading;
public static class SpacefoldNativeMouse {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct MouseInput {public int X,Y;public uint Data,Flags,Time;public UIntPtr Extra;}
 [StructLayout(LayoutKind.Explicit)] public struct InputUnion {[FieldOffset(0)] public MouseInput Mouse;}
 [StructLayout(LayoutKind.Sequential)] public struct Input {public uint Type;public InputUnion Union;}
 [DllImport("user32.dll")] public static extern uint SendInput(uint count,Input[] inputs,int size);
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
 private static void Send(uint flags,int x=0,int y=0){var input=new Input{Type=0,Union=new InputUnion{Mouse=new MouseInput{Flags=flags,X=x,Y=y}}};if(SendInput(1,new[]{input},Marshal.SizeOf(typeof(Input)))!=1)throw new Exception("SendInput failed");}
 private static void Move(int x,int y){int left=GetSystemMetrics(76),top=GetSystemMetrics(77),width=GetSystemMetrics(78),height=GetSystemMetrics(79);Send(0x8000|0x4000|1,(int)Math.Round((x-left)*65535.0/(width-1)),(int)Math.Round((y-top)*65535.0/(height-1)));}
 public static void Drag(IntPtr h,int x,int y,int dx,int dy){Point original;GetCursorPos(out original);SetForegroundWindow(h);try{Move(x,y);Thread.Sleep(150);Send(2);Thread.Sleep(150);for(int i=1;i<=12;i++){Move(x+dx*i/12,y+dy*i/12);Thread.Sleep(35);}Send(4);Thread.Sleep(200);}finally{Send(4);Move(original.X,original.Y);}}
}
'@
[SpacefoldNativeMouse]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
[SpacefoldNativeMouse]::Drag([IntPtr]$WindowHandle,$X,$Y,$DeltaX,$DeltaY)
Write-Output 'Native SendInput gesture completed on the dedicated Spacefold test window; pointer restored.'
