param([Parameter(Mandatory)][string]$FixtureFile,[switch]$CloseProperties)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$taskFull=[IO.Path]::GetFullPath($FixtureFile)
$taskName=[IO.Path]::GetFileName($taskFull)
$taskParent=[IO.Path]::GetDirectoryName($taskFull)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type 'using System;using System.Runtime.InteropServices;public static class SpacefoldShellClose{[DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);}'
$taskShell=New-Object -ComObject Shell.Application
$taskSelected=$false
foreach($taskWindow in $taskShell.Windows()){
  try{
    if($taskWindow.Document.Folder.Self.Path -ieq $taskParent){
      foreach($taskItem in $taskWindow.Document.SelectedItems()){if($taskItem.Path -ieq $taskFull){$taskSelected=$true}}
    }
  }catch{}
}
$taskRoot=[System.Windows.Automation.AutomationElement]::RootElement
$taskWindows=$taskRoot.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition)
$taskProperties=$false
$taskAssociated=$false
$taskTitles=@()
foreach($taskWindow in $taskWindows){
  $taskTitle=$taskWindow.Current.Name
  if([string]::IsNullOrEmpty($taskTitle)){continue}
  if($taskTitle.Contains($taskName)){
    $taskTitles+=$taskTitle
    if($taskTitle.Contains(([string][char]0x5c5e)+([string][char]0x6027)) -or $taskTitle.Contains('Properties')){
      $taskProperties=$true
      if($CloseProperties){
        [SpacefoldShellClose]::PostMessage([IntPtr]$taskWindow.Current.NativeWindowHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null
      }
    }else{$taskAssociated=$true}
  }
}
[ordered]@{selectedByExplorer=$taskSelected;propertiesWindow=$taskProperties;associatedWindow=$taskAssociated;fixtureTitles=$taskTitles} | ConvertTo-Json -Compress
