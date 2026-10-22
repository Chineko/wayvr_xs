param(
    [string]$SourceDirectory = (Join-Path $PSScriptRoot '.upstream'),
    [string]$WindowsSdkVersion = '10.0',
    [string]$MSBuildPath
)
$ErrorActionPreference = 'Stop'
$revision = '031d74d69f08411a9a9b1030d3d428ac02b93549'
function CheckExit([string]$Action) { if ($LASTEXITCODE -ne 0) { throw "$Action failed ($LASTEXITCODE)." } }
if (-not (Test-Path -LiteralPath (Join-Path $SourceDirectory '.git'))) {
    git clone --depth 1 --branch v3.6 https://github.com/elvissteinjr/DesktopPlus.git $SourceDirectory
    CheckExit 'Clone Desktop+'
}
$head = git -C $SourceDirectory rev-parse HEAD
CheckExit 'Read upstream revision'
if ($head -ne $revision) { throw 'Unexpected Desktop+ revision; use a dedicated v3.6 checkout.' }
$patch = Join-Path $PSScriptRoot 'xs-native-controls.patch'
git -C $SourceDirectory apply --reverse --check $patch 2>$null
if ($LASTEXITCODE -ne 0) {
    git -C $SourceDirectory apply --check $patch
    CheckExit 'Validate XS patch'
    git -C $SourceDirectory apply $patch
    CheckExit 'Apply XS patch'
}
python (Join-Path $PSScriptRoot 'generate_profiles.py') --check
CheckExit 'Validate profiles'
Copy-Item -Path (Join-Path $PSScriptRoot 'profiles/*.ini') -Destination (Join-Path $SourceDirectory 'assets/profiles')
$tools = Join-Path $PSScriptRoot '.tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
$nuget = Join-Path $tools 'nuget.exe'
if (-not (Test-Path -LiteralPath $nuget)) {
    Invoke-WebRequest https://dist.nuget.org/win-x86-commandline/v6.13.2/nuget.exe -OutFile $nuget
}
$env:NUGET_PACKAGES = Join-Path $tools 'packages'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $tools 'http-cache'
& $nuget restore (Join-Path $SourceDirectory 'src/DesktopPlus.sln') -PackagesDirectory (Join-Path $SourceDirectory 'src/packages') -NonInteractive -Verbosity quiet
CheckExit 'NuGet restore'
if (-not $MSBuildPath) {
    $msbuild = Get-Command msbuild.exe -ErrorAction SilentlyContinue
    if ($msbuild) { $MSBuildPath = $msbuild.Source }
    else {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $MSBuildPath = & $vswhere -latest -products '*' -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
    }
}
if (-not $msbuildPath) { throw 'Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK.' }
& $msbuildPath (Join-Path $SourceDirectory 'src/DesktopPlus.sln') /m:1 /nr:false /nologo /v:minimal /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 "/p:WindowsTargetPlatformVersion=$WindowsSdkVersion" /p:UseMultiToolTask=false "/p:ForceImportBeforeCppTargets=$(Join-Path $PSScriptRoot 'release.targets')"
CheckExit 'Build Desktop+ XS'
# Profile files are generated after the upstream project was authored; copy them explicitly.
$release = Join-Path $SourceDirectory 'src/x64/Release'
New-Item -ItemType Directory -Force -Path (Join-Path $release 'profiles') | Out-Null
Copy-Item -Path (Join-Path $PSScriptRoot 'profiles/*.ini') -Destination (Join-Path $release 'profiles')
python (Join-Path $PSScriptRoot 'package.py') --source $SourceDirectory
CheckExit 'Package Desktop+ XS'
