# Configure an existing, dedicated copy of the official YR CnCNet client.
# Windows PowerShell 5.1+; no Python, DLL injection, registry or asset installation.
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [ValidateSet('Install', 'Restore')][string]$Action = 'Install',
    [Parameter(Mandatory = $true)][string]$ClientDirectory,
    [string]$VeraExecutable,
    [string]$RetailDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $ClientDirectory).ProviderPath.TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath $root -PathType Container) -or ((Get-Item -LiteralPath $root).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'ClientDirectory must be a real directory, not a link.' }
if ($root -eq [IO.Path]::GetPathRoot($root).TrimEnd('\', '/')) { throw 'ClientDirectory cannot be a drive root.' }
$stateDirectory = Join-Path $root 'VERA20k-client-backup'
$manifestPath = Join-Path $stateDirectory 'manifest.json'
$utf8 = New-Object System.Text.UTF8Encoding($false)

function LocalPath([string]$Relative) {
    if ([IO.Path]::IsPathRooted($Relative)) { throw "Expected a relative path: $Relative" }
    $path = [IO.Path]::GetFullPath((Join-Path $root $Relative))
    if (-not $path.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path escapes client directory: $Relative"
    }
    return $path
}

function ReadIni([string]$Path) {
    $sections = [ordered]@{}
    $section = $null
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $trimmed = $line.Trim().TrimStart([char]0xfeff)
        if ($trimmed -match '^\[([^\]]+)\]\s*(?:;.*)?$') {
            $name = $Matches[1]
            if ($sections.Contains($name)) { throw "Duplicate INI section [$name] in $Path" }
            $section = [ordered]@{}
            $sections[$name] = $section
        } elseif ($trimmed -and -not $trimmed.StartsWith(';') -and $null -ne $section) {
            $eq = $trimmed.IndexOf('=')
            if ($eq -gt 0) { $section[$trimmed.Substring(0, $eq).Trim()] = $trimmed.Substring($eq + 1).Trim() }
        }
    }
    return ,$sections
}

function IniText($Sections) {
    $text = New-Object Text.StringBuilder
    [void]$text.AppendLine('; VERA20k offline client profile. Original file is in VERA20k-client-backup.')
    foreach ($section in $Sections.Keys) {
        [void]$text.AppendLine("[$section]")
        foreach ($key in $Sections[$section].Keys) { [void]$text.AppendLine("$key=$($Sections[$section][$key])") }
        [void]$text.AppendLine()
    }
    return $text.ToString()
}

function MergeSection($Sections, [string]$Name, $Values) {
    if (-not $Sections.Contains($Name)) { $Sections[$Name] = [ordered]@{} }
    foreach ($key in $Values.Keys) { $Sections[$Name][$key] = $Values[$key] }
}

function RequireFile([string]$Relative) {
    $path = LocalPath $Relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Not an expected YR CnCNet installation: missing $Relative" }
    if ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing linked file: $path" }
    return $path
}

# A running client may rewrite its configuration while this utility works.
foreach ($process in Get-Process) {
    try { $exePath = $process.Path } catch { continue }
    if ($exePath -and $exePath.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Close processes running from this client directory first: $($process.ProcessName)"
    }
}

if ($Action -eq 'Restore') {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'No VERA20k backup manifest in this directory.' }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.Schema -ne 1 -or $manifest.ClientDirectory -ne $root) { throw 'Backup does not belong to this client directory.' }
    # Validate the complete restoration before changing any file. User edits are preserved.
    foreach ($entry in $manifest.Files) {
        $path = LocalPath $entry.RelativePath
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            if (-not $entry.AllowAbsent) { throw "Managed file is missing: $($entry.RelativePath)" }
        } elseif ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.InstalledSha256) {
            throw "File changed after installation: $($entry.RelativePath). Preserve it and restore its original from the backup manually."
        }
        if ($entry.Existed) {
            $backup = Join-Path $stateDirectory $entry.RelativePath
            if ((Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash -ne $entry.OriginalSha256) { throw "Backup hash mismatch: $($entry.RelativePath)" }
        }
    }
    if ($PSCmdlet.ShouldProcess($root, 'Restore all original client files from the VERA20k backup')) {
        foreach ($entry in $manifest.Files) {
            $path = LocalPath $entry.RelativePath
            if ($entry.Existed) {
                if (-not $entry.AllowAbsent -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    Copy-Item -LiteralPath (Join-Path $stateDirectory $entry.RelativePath) -Destination $path -Force
                }
            }
            else { Remove-Item -LiteralPath $path }
        }
        # Retain the backup and receipt; never recursively delete a directory.
        [IO.File]::Move($manifestPath, (Join-Path $stateDirectory ('restored-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '.json')))
        Write-Host 'Original client configuration restored. Backups and VERA user data were retained.'
    }
    return
}

if (-not $VeraExecutable -or -not $RetailDirectory) { throw 'Install requires -VeraExecutable and -RetailDirectory.' }
if (Test-Path -LiteralPath $stateDirectory) { throw 'Backup directory already exists. Restore/review it before configuring another profile; it will not be overwritten.' }
$vera = (Resolve-Path -LiteralPath $VeraExecutable).ProviderPath
$retail = (Resolve-Path -LiteralPath $RetailDirectory).ProviderPath.TrimEnd('\', '/')
if ($retail -eq $root) { throw 'Use a dedicated client copy, separate from the original retail installation.' }
if (-not (Test-Path -LiteralPath $vera -PathType Leaf) -or [IO.Path]::GetExtension($vera) -ne '.exe') { throw 'VeraExecutable must be an existing Windows executable.' }
foreach ($name in @('ra2.mix', 'ra2md.mix')) {
    if (-not (Test-Path -LiteralPath (Join-Path $retail $name) -PathType Leaf)) { throw "Retail directory is missing $name" }
}
$statisticsPath = 'VERA20k-native-statistics-not-provided.log'
if (Test-Path -LiteralPath (LocalPath $statisticsPath)) { throw "Reserved statistics path already exists: $statisticsPath. Preserve or relocate it before installation." }
$defs = ReadIni (RequireFile 'Resources/ClientDefinitions.ini')
if (-not $defs.Contains('Settings') -or $defs.Settings.ClientGameType -notin @('Ares', 'YR')) { throw 'Only the official YR/Ares client layout is supported.' }
foreach ($file in @('Resources/MainMenu.ini', 'Resources/GameLobbyBase.ini', 'Resources/SkirmishLobby.ini', 'Resources/CampaignSelector.ini', 'Resources/GameOptions.ini', 'INI/MPMaps.ini')) { $null = RequireFile $file }

$changes = [ordered]@{}
MergeSection $defs 'Settings' ([ordered]@{
    GameExecutableNames = 'vera20k.exe'; GameLauncherExecutableName = ''; ExtraCommandLineParams = ''
    LongGameName = 'VERA20k - offline'; WindowTitle = 'VERA20k - single player'
    SettingsFile = 'RA2MD.INI'; ModMode = 'true'; ReplaySupport = 'false'; RequiredFiles = 'vera20k.exe'
    DisableMultiplayerGameLoading = 'true'; CompatibilityCheckExecutables = ''
    CopyResolutionDependentLanguageDLL = 'false'; CopyMissionsToSpawnmapINI = 'false'
    IgnoreBattleIni = 'true'; BattleFSFileName = 'VERA20kBattles.ini'; CampaignTagSelectorEnabled = 'false'
    MPMapsPath = 'INI/VERA20kMaps.ini'; AllowedCustomGameModes = 'Battle,Standard,Custom Map'
    CustomMissionPath = 'Maps/VERA20kMissions'; DiscordAppId = ''
    StatisticsLogFileName = $statisticsPath
})
$changes['Resources/ClientDefinitions.ini'] = IniText $defs
$changes['Resources/Renderers.ini'] = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'profile/Renderers.ini'))
$changes['Resources/GameOptions.ini'] = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'profile/GameOptions.ini'))
$changes['INI/VERA20kBattles.ini'] = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'profile/VERA20kBattles.ini'))
$changes['INI/Map Code/VERA20k.ini'] = '; Intentionally empty: VERA uses retail YR Battle rules, without Ares/Phobos map patches.' + "`r`n"
$changes['INI/Map Code/GlobalCode.ini'] = '; Intentionally empty in this isolated VERA offline client profile.' + "`r`n"

# Patch every theme's menu because theme attributes override the base resource.
foreach ($menu in Get-ChildItem -LiteralPath (Join-Path $root 'Resources') -Filter MainMenu.ini -File -Recurse) {
    $relative = $menu.FullName.Substring($root.Length + 1)
    $ini = ReadIni $menu.FullName
    foreach ($control in @('btnCnCNet', 'btnQuickmatch', 'btnLan', 'btnLoadGame', 'btnStatistics', 'btnMapEditor', 'btnExtras', 'btnAres', 'btnPhobos', 'lblCnCNetStatus', 'lblCnCNetPlayerCount')) {
        MergeSection $ini $control ([ordered]@{ Visible = 'false'; Enabled = 'false' })
    }
    $changes[$relative] = IniText $ini
}

$lobby = ReadIni (LocalPath 'Resources/GameLobbyBase.ini')
# Rebuild only the options panel. Its old position expressions refer to removed controls.
$panel = ReadIni (Join-Path $PSScriptRoot 'profile/SkirmishOptions.ini')
foreach ($section in $panel.Keys) { $lobby[$section] = $panel[$section] }
foreach ($key in @($lobby.GameLobbyBase.Keys)) {
    if ($lobby.GameLobbyBase[$key] -match '^(chkAutoSave|btnPhobos|btnAres):') { $lobby.GameLobbyBase.Remove($key) }
}
$changes['Resources/GameLobbyBase.ini'] = IniText $lobby
# This entry supplies the game-speed drop-down in the original client; our panel owns it.
$skirmish = ReadIni (LocalPath 'Resources/SkirmishLobby.ini')
if ($skirmish.Contains('GameOptionsPanel')) { $skirmish.Remove('GameOptionsPanel') }
$changes['Resources/SkirmishLobby.ini'] = IniText $skirmish
$campaign = ReadIni (LocalPath 'Resources/CampaignSelector.ini')
if ($campaign.Contains('ExtraControls')) {
    foreach ($key in @($campaign.ExtraControls.Keys)) {
        if ($campaign.ExtraControls[$key] -match ':(CampaignCheckBox|CampaignDropDown)$') { $campaign.ExtraControls.Remove($key) }
    }
}
$changes['Resources/CampaignSelector.ini'] = IniText $campaign

# MapLoader creates modes from per-map metadata, even if [GameModes] omits them.
# Preserve existing Battle map previews/metadata; do not import ladder, survival or co-op modes.
$originalMaps = ReadIni (LocalPath 'INI/MPMaps.ini')
if (-not $originalMaps.Contains('MultiMaps')) { throw 'Missing [MultiMaps] in INI/MPMaps.ini.' }
$maps = [ordered]@{
    GameModes = [ordered]@{ '0' = 'Battle' }
    Battle = [ordered]@{ MapCodeIniName = 'VERA20k.ini'; ForcedSpawnIniOptions = 'BattleForcedSpawnIniOptions' }
    BattleForcedSpawnIniOptions = [ordered]@{ GameMode = '1'; Ra2Mode = 'false' }
    GameModeAliases = [ordered]@{ Standard = 'Battle'; 'Custom Map' = 'Battle' }
    MultiMaps = [ordered]@{}
}
foreach ($key in $originalMaps.MultiMaps.Keys) {
    $map = $originalMaps.MultiMaps[$key]
    if (-not $originalMaps.Contains($map)) { continue }
    $metadata = $originalMaps[$map]
    if (-not $metadata.Contains('GameModes') -or 'Battle' -notin ($metadata.GameModes -split ',' | ForEach-Object { $_.Trim() })) { continue }
    $maps.MultiMaps[$key] = $map
    # Do not silently drop authored forced options. Such mod/mode maps need a separate integration.
    $unsupported = @($metadata.Keys | Where-Object { $_ -match '^(ForcedOptions|ForcedSpawnIniOptions|MapCodeIniName|RandomizedMapCodeIniNames)$' -and $metadata[$_] })
    if ($unsupported.Count -gt 0) { $maps.MultiMaps.Remove($key); continue }
    $copy = [ordered]@{}
    foreach ($field in $metadata.Keys) { $copy[$field] = $metadata[$field] }
    $copy['GameModes'] = 'Battle'
    $maps[$map] = $copy
}
if ($maps.MultiMaps.Count -eq 0) { throw 'No ordinary Battle maps found in this client package.' }
$changes['INI/VERA20kMaps.ini'] = IniText $maps
# Share CnCNet settings in this dedicated client copy; retail assets remain read-only.
$retailToml = $retail.Replace('\', '/').Replace('"', '\"')
$changes['config.toml'] = "# Generated for the VERA20k CnCNet offline profile.`r`n[paths]`r`nra2_dir = `"$retailToml`"`r`nsettings_dir = `".`"`r`n"

$entries = @()
foreach ($relative in @($changes.Keys) + @('vera20k.exe')) {
    $path = LocalPath $relative
    # Do not follow links in a managed file or any ancestor below the explicit root.
    $parent = $path
    while ($parent -ne $root) {
        if (Test-Path -LiteralPath $parent) {
            if ((Get-Item -LiteralPath $parent).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing linked path: $parent" }
        }
        $parent = [IO.Path]::GetDirectoryName($parent)
    }
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    $entries += [pscustomobject]@{ RelativePath = $relative; Existed = $exists; OriginalSha256 = $(if ($exists) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { $null }); InstalledSha256 = ''; AllowAbsent = $false }
}
# The client's native-renderer Apply() deletes ddraw.dll on saving display options.
# Preserve that file too, and permit precisely this expected missing-file state on restore.
if (Test-Path -LiteralPath (LocalPath 'ddraw.dll') -PathType Leaf) {
    $null = RequireFile 'ddraw.dll'
    $ddrawHash = (Get-FileHash -LiteralPath (LocalPath 'ddraw.dll') -Algorithm SHA256).Hash
    $entries += [pscustomobject]@{ RelativePath = 'ddraw.dll'; Existed = $true; OriginalSha256 = $ddrawHash; InstalledSha256 = $ddrawHash; AllowAbsent = $true }
}
Write-Host "Profile: $($maps.MultiMaps.Count) existing Battle maps, 14 official YR campaign missions, one human and up to seven AI players."
Write-Host "Will update $($entries.Count) files in $root; originals will be backed up."
if (-not $PSCmdlet.ShouldProcess($root, 'Install the offline VERA20k profile and copy its executable')) { return }
[void][IO.Directory]::CreateDirectory($stateDirectory)
foreach ($entry in $entries) {
    $path = LocalPath $entry.RelativePath
    if ($entry.Existed) {
        $backup = Join-Path $stateDirectory $entry.RelativePath
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($backup))
        Copy-Item -LiteralPath $path -Destination $backup
    }
}
# Write a recovery receipt before touching originals, including intended hashes.
foreach ($entry in $entries) {
    if ($entry.AllowAbsent) { continue }
    if ($entry.RelativePath -eq 'vera20k.exe') { $entry.InstalledSha256 = (Get-FileHash -LiteralPath $vera -Algorithm SHA256).Hash }
    else {
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $entry.InstalledSha256 = [BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes($changes[$entry.RelativePath]))).Replace('-', '') }
        finally { $sha.Dispose() }
    }
}
$receipt = [ordered]@{ Schema = 1; ClientDirectory = $root; CreatedUtc = [DateTime]::UtcNow.ToString('o'); VeraSourceSha256 = (Get-FileHash -LiteralPath $vera -Algorithm SHA256).Hash; Files = $entries }
[IO.File]::WriteAllText($manifestPath, ($receipt | ConvertTo-Json -Depth 5), $utf8)
foreach ($relative in $changes.Keys) {
    $path = LocalPath $relative
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
    [IO.File]::WriteAllText($path, $changes[$relative], $utf8)
}
if ($vera -ne (LocalPath 'vera20k.exe')) { Copy-Item -LiteralPath $vera -Destination (LocalPath 'vera20k.exe') -Force }
Write-Host 'Profile installed. Open this copy of the CnCNet client and choose Skirmish or Campaign.'
Write-Host 'This profile does not enable online/LAN, native saves/replays, RA2 campaigns, or Ares/Phobos extensions.'
