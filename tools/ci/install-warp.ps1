# Developer-only WARP from Microsoft's pinned NuGet package. Never install it
# into Windows or ship it with MUI. Clean before the Rust cache is saved.
param([switch]$Clean)
$ErrorActionPreference = 'Stop'
if ($Clean) {
    if ($env:MUI_CI_WARP_TARGETS) {
        foreach ($directory in ($env:MUI_CI_WARP_TARGETS | ConvertFrom-Json)) {
            Remove-Item -LiteralPath (Join-Path $directory 'd3d10warp.dll') -Force -ErrorAction SilentlyContinue
        }
    }
    exit 0
}
$version = '1.0.21'
$sha256 = 'ea44b77eb30eec14427193e20f40cdb2e4c31ed11ab39cb880538fdf4bac2681'
$folder = Join-Path $env:RUNNER_TEMP "mui-warp-$version"
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$archive = Join-Path $folder 'warp.zip'
Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.warp/$version/microsoft.direct3d.warp.$version.nupkg" -OutFile $archive
if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) {
    throw 'WARP package checksum mismatch'
}
Expand-Archive -LiteralPath $archive -DestinationPath $folder -Force
$source = Join-Path $folder 'build/native/bin/x64/d3d10warp.dll'
$directories = @()
foreach ($mapping in ($env:MUI_CI_WORKSPACES -split "`n")) {
    if (!$mapping.Trim()) { continue }
    $parts = $mapping.Trim() -split '\s*->\s*'
    $target = Join-Path (Join-Path $env:GITHUB_WORKSPACE $parts[0]) $parts[1]
    foreach ($profile in @('debug', 'release')) {
        foreach ($suffix in @('', 'examples', 'deps')) {
            $directory = Join-Path (Join-Path $target $profile) $suffix
            New-Item -ItemType Directory -Force -Path $directory | Out-Null
            Copy-Item -LiteralPath $source -Destination $directory -Force
            $directories += $directory
        }
    }
}
$serialized = ConvertTo-Json -InputObject $directories -Compress
"MUI_CI_WARP_TARGETS=$serialized" >> $env:GITHUB_ENV
$receipt = "Microsoft.Direct3D.WARP $version SHA256=$sha256 DLL=$((Get-Item $source).VersionInfo.FileVersion)"
Write-Output $receipt
"$receipt`n" >> $env:GITHUB_STEP_SUMMARY
