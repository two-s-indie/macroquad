# Build isolated, instrumented copies; never edit the sibling game or switch branches.
[CmdletBinding()]
param(
    [string]$Baseline = '53e0ac7',
    [string]$OutputDirectory = 'target/context-migration/step9-bench',
    [ValidateRange(1, 25)][int]$Runs = 5,
    [ValidateRange(1, 100000)][int]$Samples = 5000,
    [ValidateRange(0, 100000)][int]$Warmup = 1000,
    [switch]$PrepareOnly,
    [switch]$RunOnly
)
$ErrorActionPreference = 'Stop'
if ($PrepareOnly -and $RunOnly) { throw 'Choose either PrepareOnly or RunOnly.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath((Join-Path $repo $OutputDirectory))
$game = [IO.Path]::GetFullPath((Join-Path $repo '../Where-s-the-Yokai/Where-s-the-Yokai'))
New-Item -ItemType Directory -Force $output | Out-Null
if (!$RunOnly) {
    $baselinePath = Join-Path $output 'baseline'
    if (Test-Path $baselinePath) { throw 'Use a fresh output directory when preparing another build.' }
    New-Item -ItemType Directory $baselinePath | Out-Null
    $archive = Join-Path $output 'baseline.tar'
    git -C $repo archive --format=tar "--output=$archive" $Baseline
    if ($LASTEXITCODE) { throw 'git archive failed' }
    tar -xf $archive -C $baselinePath
    if ($LASTEXITCODE) { throw 'Extracting baseline failed' }
    $source = Get-Content -Raw (Join-Path $game 'src/main.rs')
    foreach ($anchor in @('#[macroquad::main(window_conf)]', 'async fn main() {', '    loop {', '        next_frame().await;', 'let (mouse_x, mouse_y) = mouse_position();')) {
        if ([regex]::Matches($source, [regex]::Escape($anchor)).Count -ne 1) {
            throw "Game source changed: expected exactly one $anchor"
        }
    }
    $source = $source -replace '#!\[cfg_attr\([^\r\n]+\)\]\r?\n', ''
    $source = "mod probe;`n" + $source
    $source = $source.Replace('#[macroquad::main(window_conf)]', @'
fn bench_conf() -> Conf {
    let mut config = window_conf();
    config.platform.swap_interval = Some(0);
    config
}
#[macroquad::main(bench_conf)]
'@)
    $source = $source.Replace('async fn main() {', "async fn main() {`n    let mut probe = probe::Probe::new();")
    $source = $source.Replace('    loop {', "    loop {`n        probe.begin_frame();")
    $source = $source.Replace('let (mouse_x, mouse_y) = mouse_position();', @'
std::hint::black_box(mouse_position());
        let (mouse_x, mouse_y) = (400.0, 300.0);
'@)
    $source = $source.Replace('        next_frame().await;', @'
        probe.extra_work();
        if probe.finish_work() { return; }
        next_frame().await;
        probe.finish_frame();
'@)
    foreach ($variant in @('baseline', 'current')) {
        $engine = if ($variant -eq 'baseline') { $baselinePath } else { $repo }
        $engineToml = $engine.Replace('\', '/')
        $project = Join-Path $output "$variant-game"
        New-Item -ItemType Directory -Force (Join-Path $project 'src') | Out-Null
        Set-Content (Join-Path $project 'src/main.rs') $source
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'probe.rs') -Destination (Join-Path $project 'src/probe.rs')
        Copy-Item -LiteralPath (Join-Path $game 'build.rs') -Destination $project
        Copy-Item -LiteralPath (Join-Path $repo 'Cargo.lock') -Destination $project
        Set-Content (Join-Path $project 'Cargo.toml') @"
[package]
name = "context-migration-game-bench"
version = "0.1.0"
edition = "2024"
[dependencies]
macroquad = { path = "$engineToml" }
[patch.crates-io]
miniquad = { git = "https://github.com/two-s-indie/miniquad.git", branch = "game" }
[build-dependencies]
embed-manifest = "1.5"
"@
        $buildLog = Join-Path $output "$variant-build.log"
        cargo build --release --offline --manifest-path (Join-Path $project 'Cargo.toml') --target-dir (Join-Path $output 'build') *> $buildLog
        if ($LASTEXITCODE) { Get-Content $buildLog -Tail 60; throw "$variant build failed" }
        Copy-Item -LiteralPath (Join-Path $output 'build/release/context-migration-game-bench.exe') -Destination (Join-Path $output "$variant.exe")
        Write-Output "$variant release build ready"
    }
    $locks = @('baseline', 'current') | ForEach-Object { (Get-FileHash (Join-Path $output "$_-game/Cargo.lock")).Hash }
    if ($locks[0] -ne $locks[1]) { throw 'The compared dependency lockfiles differ' }
    $details = [ordered]@{
        baseline = (git -C $repo rev-parse $Baseline)
        current = (git -C $repo rev-parse HEAD)
        gameSourceSha256 = (Get-FileHash (Join-Path $game 'src/main.rs')).Hash
        lockSha256 = $locks[0]
        rust = (rustc --version)
    }
    $details | ConvertTo-Json | Set-Content (Join-Path $output 'builds.json')
}
if ($PrepareOnly) { return }
[ordered]@{ runs = $Runs; samples = $Samples; warmup = $Warmup; swapInterval = 0 } |
    ConvertTo-Json | Set-Content (Join-Path $output 'settings.json')
$savedEnvironment = @{}
foreach ($name in @('MQ_BENCH_SAMPLES', 'MQ_BENCH_WARMUP', 'MQ_BENCH_MODE', 'MQ_BENCH_OUTPUT')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
}
$env:MQ_BENCH_SAMPLES = "$Samples"
$env:MQ_BENCH_WARMUP = "$Warmup"
try {
    foreach ($mode in @('game', 'stress')) {
        $env:MQ_BENCH_MODE = $mode
        for ($run = 1; $run -le $Runs; $run++) {
            # Alternate order to reduce bias from clock/thermal/background drift.
            $order = if ($run % 2) { @('baseline', 'current') } else { @('current', 'baseline') }
            foreach ($variant in $order) {
                $env:MQ_BENCH_OUTPUT = Join-Path $output "$mode-$variant-$run"
                & (Join-Path $output "$variant.exe")
                if ($LASTEXITCODE) { throw "$mode $variant run $run failed" }
                if (!(Test-Path "$env:MQ_BENCH_OUTPUT.csv")) { throw 'Run ended before collecting samples' }
                Write-Output "$mode $variant run $run complete"
            }
        }
    }
} finally {
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
    }
}
