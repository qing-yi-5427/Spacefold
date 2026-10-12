param([int]$OwnerPid=0,[string]$DialogTitle,[ValidateSet('inspect','cancel','select')][string]$Action='inspect',[string]$FolderPath)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type 'using System;using System.Runtime.InteropServices;public static class SpacefoldDialog{[DllImport("user32.dll")]public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);[DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,string l);}'
$taskRoot=[System.Windows.Automation.AutomationElement]::RootElement
$taskWindow=$null
for($taskTry=0;$taskTry -lt 100;$taskTry++){
  $taskWindows=$taskRoot.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition)
  foreach($taskCandidate in $taskWindows){
    if(($OwnerPid -eq 0 -or $taskCandidate.Current.ProcessId -eq $OwnerPid) -and $taskCandidate.Current.Name -eq $DialogTitle){$taskWindow=$taskCandidate;break}
  }
  if($null -ne $taskWindow){break}
  Start-Sleep -Milliseconds 30
}
if($null -eq $taskWindow){throw "Owned dialog not found: $DialogTitle"}
$taskControls=$taskWindow.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
$taskRecords=@()
foreach($taskControl in $taskControls){if($taskControl.Current.ControlType -ne [System.Windows.Automation.ControlType]::ListItem){$taskRecords+=[ordered]@{name=$taskControl.Current.Name;id=$taskControl.Current.AutomationId;type=$taskControl.Current.ControlType.ProgrammaticName;class=$taskControl.Current.ClassName;handle=$taskControl.Current.NativeWindowHandle}}}
if($Action -eq 'cancel'){
  $taskCancel=$taskControls | Where-Object {$_.Current.AutomationId -eq '2'} | Select-Object -First 1
  if($null -eq $taskCancel){throw 'Cancel button not found'}
  [SpacefoldDialog]::SendMessage([IntPtr]$taskWindow.Current.NativeWindowHandle,0x111,[IntPtr]2,[IntPtr]::Zero) | Out-Null
}
if($Action -eq 'select'){
  $taskEdit=$taskControls | Where-Object {$_.Current.AutomationId -eq '1152'} | Select-Object -First 1
  if($null -eq $taskEdit -or $taskEdit.Current.NativeWindowHandle -eq 0){throw 'Owned folder filename HWND not found'}
  [SpacefoldDialog]::SendMessage([IntPtr]$taskEdit.Current.NativeWindowHandle,0xc,[IntPtr]::Zero,$FolderPath) | Out-Null
  $taskSelect=$taskControls | Where-Object {$_.Current.AutomationId -eq '1'} | Select-Object -First 1
  if($null -eq $taskSelect){throw 'Folder select button not found'}
  [SpacefoldDialog]::SendMessage([IntPtr]$taskWindow.Current.NativeWindowHandle,0x111,[IntPtr]1,[IntPtr]::Zero) | Out-Null
  Start-Sleep -Milliseconds 300
  [SpacefoldDialog]::SendMessage([IntPtr]$taskEdit.Current.NativeWindowHandle,0xc,[IntPtr]::Zero,'') | Out-Null
  [SpacefoldDialog]::SendMessage([IntPtr]$taskWindow.Current.NativeWindowHandle,0x111,[IntPtr]1,[IntPtr]::Zero) | Out-Null
}
[ordered]@{title=$DialogTitle;ownerPid=$taskWindow.Current.ProcessId;action=$Action;controls=$taskRecords} | ConvertTo-Json -Depth 4 -Compress
