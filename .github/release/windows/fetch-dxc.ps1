# Microsoft's DirectX Shader Compiler for the Windows package. wgpu compiles the
# game's DirectX 12 shaders with dxcompiler.dll when it sits beside vera20k.exe,
# and with Windows' 2015 FXC otherwise (wgpu's `Dx12Compiler::Auto`); naga's
# HLSL is written for DXC, and FXC's code is slower, which matters on the
# integrated graphics chips that Windows' two-second timeout kills first. One
# pinned release, checked by hash, for Release and the signing diagnostic.
param([Parameter(Mandatory = $true)][string]$Package)
$ErrorActionPreference = 'Stop'
$release = 'v1.9.2609'
$asset = 'dxc_2026_09_29.zip'
$expectedSha256 = 'ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1'
$zip = Join-Path $env:RUNNER_TEMP $asset
$unpacked = Join-Path $env:RUNNER_TEMP 'dxc'
Invoke-WebRequest -Uri "https://github.com/microsoft/DirectXShaderCompiler/releases/download/$release/$asset" -OutFile $zip
if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $expectedSha256) {
    throw 'DirectX Shader Compiler download checksum mismatch'
}
if (Test-Path $unpacked) { Remove-Item $unpacked -Recurse -Force }
Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
$dll = Join-Path $unpacked 'bin\x64\dxcompiler.dll'
if (-not (Test-Path $dll -PathType Leaf)) { throw 'The DirectX Shader Compiler release has no bin\x64\dxcompiler.dll' }
Copy-Item $dll (Join-Path $Package 'dxcompiler.dll')
# Its license notices, in the one file the installer and README name.
$licenses = @(Get-ChildItem $unpacked -Filter 'LICENSE*' -File | Sort-Object Name)
if ($licenses.Count -eq 0) { throw 'The DirectX Shader Compiler release has no LICENSE files' }
$text = "DirectX Shader Compiler $release (dxcompiler.dll), from`r`n" +
    "https://github.com/microsoft/DirectXShaderCompiler/releases/tag/$release`r`n"
foreach ($license in $licenses) {
    $text += "`r`n==== $($license.Name) ====`r`n" + (Get-Content $license.FullName -Raw)
}
[IO.File]::WriteAllText((Join-Path $Package 'LICENSE-DXC.txt'), $text)
