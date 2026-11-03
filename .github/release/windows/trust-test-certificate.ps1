[CmdletBinding()]
param(
    [string] $CertificateBase64 = $env:TEST_CERTIFICATE,
    [string] $ExpectedSha256 = $env:TEST_CERTIFICATE_SHA256
)

$ErrorActionPreference = 'Stop'
if ($ExpectedSha256 -notmatch '^[0-9A-Fa-f]{64}$') {
    throw 'Configure the SignPath test certificate SHA256 fingerprint first'
}
$bytes = [Convert]::FromBase64String($CertificateBase64)
$actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
if ($actual -ne $ExpectedSha256) { throw 'Test certificate fingerprint mismatch' }
$path = Join-Path $env:RUNNER_TEMP 'signpath-test.cer'
[IO.File]::WriteAllBytes($path, $bytes)
# A CurrentUser root import can display a trust dialog that a hosted runner
# cannot answer. This script is used only on disposable Windows test VMs.
# https://learn.microsoft.com/en-us/powershell/module/pki/import-certificate#example-3
$imported = @(Import-Certificate -FilePath $path -CertStoreLocation Cert:\LocalMachine\Root -Confirm:$false)
if ($imported.Count -ne 1 -or
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$imported[0].RawData)) -ne $ExpectedSha256) {
    throw 'Imported test certificate fingerprint mismatch'
}
Write-Output 'The pinned test certificate is trusted on this temporary runner.'
