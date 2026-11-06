$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$letter=$env:TOOLBOKS_OPTIMIZE_VOLUME
if($letter -notmatch '^[A-Z]:$'){throw 'Invalid drive letter.'}
$volume=Get-CimInstance Win32_Volume -Filter ("DriveLetter='"+$letter+"'")
if(-not $volume -or $volume.DriveType -ne 3){throw 'This volume is not a local fixed drive.'}
if($volume.DirtyBitSet){throw 'Windows reports that this volume needs a filesystem check.'}
$result=Invoke-CimMethod -InputObject $volume -MethodName DefragAnalysis
$code=[uint32]$result.ReturnValue
if($code -ne 0){
    $reason=switch($code){1{'Administrator access is required.'};2{'Analysis is not supported for this volume.'};3{'The filesystem needs a check.'};4{'Not enough free space.'};5{'Windows detected a filesystem problem.'};6{'Analysis was cancelled.'};7{'The cancellation request arrived too late.'};8{'Windows is already optimizing a drive.'};9{'Windows could not connect to the optimization engine.'};default{'The Windows analysis engine could not complete the request.'}}
    throw ($reason+' (Windows code '+$code+')')
}
$a=$result.DefragAnalysis
if(-not $a){throw 'Windows returned no fragmentation report.'}
@{device_id=[string]$volume.DeviceID;letter=$letter;total_percent=$a.TotalPercentFragmentation;file_percent=$a.FilePercentFragmentation;free_percent=$a.FreeSpacePercentFragmentation;fragmented_files=$a.TotalFragmentedFiles;total_files=$a.TotalFiles;recommended=[bool]$result.DefragRecommended;checked_at=(Get-Date).ToString('yyyy-MM-dd HH:mm:ss')} | ConvertTo-Json -Compress
