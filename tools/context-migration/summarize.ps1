[CmdletBinding()]
param([string]$Directory = 'target/context-migration/step9-bench')
$ErrorActionPreference = 'Stop'
$directoryPath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../../$Directory"))
$settings = Get-Content -Raw (Join-Path $directoryPath 'settings.json') | ConvertFrom-Json
$results = foreach ($file in Get-ChildItem $directoryPath -Filter '*.csv') {
    $rows = @(Import-Csv $file)
    if ($rows.Count -ne $settings.samples) { throw "Incomplete samples in $file" }
    $work = @($rows.work_us | ForEach-Object { [double]$_ } | Sort-Object)
    $frame = @($rows.frame_us | ForEach-Object { [double]$_ } | Sort-Object)
    [pscustomobject][ordered]@{
        run = $file.BaseName
        frames = $rows.Count
        workP50Us = $work[[int][Math]::Floor($rows.Count * 0.50)]
        workP95Us = $work[[int][Math]::Floor($rows.Count * 0.95)]
        frameP50Us = $frame[[int][Math]::Floor($rows.Count * 0.50)]
        frameP95Us = $frame[[int][Math]::Floor($rows.Count * 0.95)]
        allocationCallsPerFrame = ($rows | Measure-Object alloc_calls -Average).Average
        allocatedBytesPerFrame = ($rows | Measure-Object allocated_bytes -Average).Average
        retainedBytes = ($rows | Measure-Object allocated_bytes -Sum).Sum - ($rows | Measure-Object freed_bytes -Sum).Sum
        imageSha256 = (Get-FileHash ([IO.Path]::ChangeExtension($file.FullName, '.png'))).Hash
    }
}
if ($results.Count -ne 4 * $settings.runs) { throw 'Missing benchmark runs' }
foreach ($mode in @('game', 'stress')) {
    $images = @($results | Where-Object { $_.run.StartsWith("$mode-") } | Group-Object imageSha256)
    if ($images.Count -ne 1) { throw "$mode images differ between runs or engine versions" }
}
$results | ConvertTo-Json | Set-Content (Join-Path $directoryPath 'summary.json')
$results | Select-Object -ExcludeProperty imageSha256 | Format-Table -AutoSize
Write-Output 'All baseline/current screenshots match within each workload.'
