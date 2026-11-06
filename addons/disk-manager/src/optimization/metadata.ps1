$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$principal=New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
$elevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$history=@()
$historyWarning=$null
try {
    $events=Get-WinEvent -FilterHashtable @{LogName='Application';ProviderName='Microsoft-Windows-Defrag';Id=258} -MaxEvents 512 -ErrorAction Stop
    foreach($event in $events){
        [xml]$xml=$event.ToXml()
        $data=@($xml.Event.EventData.Data | ForEach-Object { if($_ -is [string]){$_}else{$_.InnerText} })
        if($data.Count -ge 2){$history+=@{operation=[string]$data[0];volume=[string]$data[1];time=$event.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss');utc=$event.TimeCreated.ToUniversalTime().ToString('o')}}
    }
} catch { if($_.FullyQualifiedErrorId -notlike 'NoMatchingEventsFound*'){$historyWarning=$_.Exception.Message} }
$volumes=@(Get-CimInstance Win32_Volume -Filter 'DriveType=3' | Where-Object { $_.DriveLetter -match '^[A-Za-z]:$' } | ForEach-Object {
    $letter=[string]$_.DriveLetter
    $records=@($history | Where-Object { $_.operation -in @('defragmentation','defragment','defrag','retrim','trim') } | Where-Object { $_.volume -eq $letter -or $_.volume -eq ($letter+'\') -or $_.volume.EndsWith('('+$letter+')',[StringComparison]::OrdinalIgnoreCase) })
    @{letter=$letter;device_id=[string]$_.DeviceID;label=[string]$_.Label;filesystem=[string]$_.FileSystem;capacity=[uint64]$_.Capacity;free=[uint64]$_.FreeSpace;dirty=[bool]$_.DirtyBitSet;history=@($records | Select-Object -First 20)}
})
@{elevated=$elevated;volumes=$volumes;history_warning=$historyWarning;checked_at=(Get-Date).ToString('yyyy-MM-dd HH:mm:ss')} | ConvertTo-Json -Depth 8 -Compress
