$ErrorActionPreference = 'Stop'
# One pinned compiler for Release and the signing diagnostic.
$installer = Join-Path $env:RUNNER_TEMP 'innosetup-7.1.0-x64.exe'
$compilerDir = Join-Path $env:RUNNER_TEMP 'innosetup-7.1.0'
Invoke-WebRequest -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_0/innosetup-7.1.0-x64.exe' -OutFile $installer
$expectedSha256 = '0362a383ed217d4c4239b5933866dd96d3eb2102737da92f80f6057a4b40df2f'
if ((Get-FileHash $installer -Algorithm SHA256).Hash -ne $expectedSha256) {
    throw 'Inno Setup download checksum mismatch'
}
$install = Start-Process $installer -Wait -PassThru -ArgumentList @(
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/TASKS=',
    "/DIR=`"$compilerDir`"")
if ($install.ExitCode -ne 0) { throw "Inno Setup installation failed: $($install.ExitCode)" }
$iscc = Join-Path $compilerDir 'ISCC.exe'
if (-not (Test-Path $iscc -PathType Leaf)) { throw 'Inno Setup compiler was not installed' }
"INNO_SETUP_ISCC=$iscc" >> $env:GITHUB_ENV
