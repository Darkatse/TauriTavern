Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Language = $null
$script:MessagesZh = @{
    BannerTitle = 'TauriTavern SillyTavern Migration Export'
    BannerSubtitle = '这个脚本会生成一个可直接导入 TauriTavern 的 zip。'
    NotInRoot = '当前目录不是 SillyTavern 根目录。'
    RootPrompt = '请输入 SillyTavern 根目录路径'
    RootRequirements = '路径内需要包含 data/default-user 和 public/scripts/extensions/third-party。'
    RootInvalid = '这个路径看起来不是有效的 SillyTavern 根目录，请重新输入。'
    DetectedRoot = '已检测到 SillyTavern 根目录'
    AskBackups = '是否导出 data/default-user/backups？跳过导出备份会明显更快。 [y/N]'
    SummaryTitle = '执行计划总览'
    SummaryRoot = 'SillyTavern 根目录'
    SummaryBackups = '导出 backups'
    SummaryGlobalExt = '全局扩展映射'
    Yes = '是'
    No = '否'
    StepPrepare = '准备临时工作区'
    StepCopyUser = '复制 default-user 数据'
    StepSkipBackups = '已按选择跳过 backups。'
    StepCopyExtensions = '复制全局 third-party 扩展'
    StepCountEntries = '统计压缩条目数量'
    StepZip = '开始压缩 zip'
    StepMoveDownloads = '尝试移动到 Downloads'
    MoveSuccess = '已成功移动 zip。'
    MoveFailed = '移动失败，zip 将保留在原位置。'
    FinalTitle = '导出完成'
    FinalPathLabel = 'zip 文件位置'
    FinalHint = '现在可以在 TauriTavern 的 数据迁移 扩展中导入这个 zip。'
    Info = '信息'
    Warning = '警告'
}
$script:MessagesEn = @{
    BannerTitle = 'TauriTavern SillyTavern Migration Export'
    BannerSubtitle = 'This script creates a zip that can be imported directly by TauriTavern.'
    NotInRoot = 'The current directory is not a SillyTavern root.'
    RootPrompt = 'Enter the SillyTavern root path'
    RootRequirements = 'The path must contain data/default-user and public/scripts/extensions/third-party.'
    RootInvalid = 'That path does not look like a valid SillyTavern root. Please try again.'
    DetectedRoot = 'Detected SillyTavern root'
    AskBackups = 'Export data/default-user/backups as well? Choosing No is usually much faster. [y/N]'
    SummaryTitle = 'Execution Plan'
    SummaryRoot = 'SillyTavern root'
    SummaryBackups = 'Export backups'
    SummaryGlobalExt = 'Global extension mapping'
    Yes = 'Yes'
    No = 'No'
    StepPrepare = 'Preparing temporary workspace'
    StepCopyUser = 'Copying default-user data'
    StepSkipBackups = 'Skipping backups as requested.'
    StepCopyExtensions = 'Copying global third-party extensions'
    StepCountEntries = 'Counting archive entries'
    StepZip = 'Creating zip archive'
    StepMoveDownloads = 'Trying to move the zip to Downloads'
    MoveSuccess = 'The zip was moved successfully.'
    MoveFailed = 'Moving failed. The zip will stay at the original location.'
    FinalTitle = 'Export Completed'
    FinalPathLabel = 'Zip file location'
    FinalHint = 'You can now import this zip from the data-migration extension in TauriTavern.'
    Info = 'Info'
    Warning = 'Warning'
}

function Get-Message {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Key
    )

    if ($script:Language -eq 'zh') {
        return $script:MessagesZh[$Key]
    }

    return $script:MessagesEn[$Key]
}

function Write-Banner {
    Write-Host ''
    Write-Host '============================================================'
    Write-Host (Get-Message 'BannerTitle')
    Write-Host (Get-Message 'BannerSubtitle')
    Write-Host '============================================================'
    Write-Host ''
}

function Write-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Index,
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    Write-Host ''
    Write-Host "[$Index] $Message"
}

function Write-InfoLine {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    Write-Host ("{0}: {1}" -f (Get-Message 'Info'), $Message)
}

function Write-WarningLine {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    Write-Host ("{0}: {1}" -f (Get-Message 'Warning'), $Message)
}

function Select-Language {
    while ($true) {
        Write-Host '请选择脚本语言 / Please choose a language'
        Write-Host '1) 中文'
        Write-Host '2) English'
        $choice = Read-Host '请输入 1 或 2，直接回车默认中文'
        switch ($choice) {
            '' { $script:Language = 'zh'; return }
            '1' { $script:Language = 'zh'; return }
            '2' { $script:Language = 'en'; return }
        }
    }
}

function Normalize-InputPath {
    param(
        [AllowEmptyString()]
        [string]$Path
    )

    $normalized = if ($null -eq $Path) { '' } else { $Path.Trim() }
    if ($normalized.StartsWith('"') -and $normalized.EndsWith('"')) {
        return $normalized.Trim('"')
    }
    if ($normalized.StartsWith("'") -and $normalized.EndsWith("'")) {
        return $normalized.Trim("'")
    }
    return $normalized
}

function Test-SillyTavernRoot {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return (Test-Path -LiteralPath (Join-Path $Path 'data/default-user') -PathType Container) `
        -and (Test-Path -LiteralPath (Join-Path $Path 'public/scripts/extensions/third-party') -PathType Container) `
        -and (Test-Path -LiteralPath (Join-Path $Path 'package.json') -PathType Leaf)
}

function Resolve-SillyTavernRoot {
    $currentDir = (Get-Location).Path
    if (Test-SillyTavernRoot -Path $currentDir) {
        $resolved = (Resolve-Path -LiteralPath $currentDir).ProviderPath
        Write-InfoLine ("{0}: {1}" -f (Get-Message 'DetectedRoot'), $resolved)
        return $resolved
    }

    Write-InfoLine (Get-Message 'NotInRoot')
    Write-InfoLine (Get-Message 'RootRequirements')

    while ($true) {
        $candidate = Normalize-InputPath (Read-Host (Get-Message 'RootPrompt'))
        if (-not $candidate) {
            Write-WarningLine (Get-Message 'RootInvalid')
            continue
        }
        if (-not (Test-SillyTavernRoot -Path $candidate)) {
            Write-WarningLine (Get-Message 'RootInvalid')
            continue
        }
        $resolved = (Resolve-Path -LiteralPath $candidate).ProviderPath
        Write-InfoLine ("{0}: {1}" -f (Get-Message 'DetectedRoot'), $resolved)
        return $resolved
    }
}

function Read-IncludeBackups {
    $answer = Read-Host (Get-Message 'AskBackups')
    if ($null -eq $answer) {
        return $false
    }

    switch ($answer.Trim().ToLowerInvariant()) {
        'y' { return $true }
        'yes' { return $true }
        '1' { return $true }
        '是' { return $true }
        default { return $false }
    }
}

function Copy-DirectoryChildren {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Source,
        [Parameter(Mandatory = $true)]
        [string]$Destination,
        [string[]]$ExcludeNames = @()
    )

    New-Item -ItemType Directory -Path $Destination -Force | Out-Null

    Get-ChildItem -LiteralPath $Source -Force | Where-Object {
        $ExcludeNames -notcontains $_.Name
    } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $Destination -Recurse -Force
    }
}

function Get-ZipRelativePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$BasePath
    )

    $relative = $Path.Substring($BasePath.Length).TrimStart('\', '/')
    return ($relative -replace '\\', '/')
}

function Add-DirectoryEntry {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Compression.ZipArchive]$Archive,
        [Parameter(Mandatory = $true)]
        [string]$EntryName
    )

    if (-not $EntryName.EndsWith('/')) {
        $EntryName = "$EntryName/"
    }

    [void]$Archive.CreateEntry($EntryName, [System.IO.Compression.CompressionLevel]::NoCompression)
}

function Add-FileEntry {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Compression.ZipArchive]$Archive,
        [Parameter(Mandatory = $true)]
        [string]$SourceFile,
        [Parameter(Mandatory = $true)]
        [string]$EntryName
    )

    $entry = $Archive.CreateEntry($EntryName, [System.IO.Compression.CompressionLevel]::Optimal)
    $inputStream = [System.IO.File]::OpenRead($SourceFile)
    try {
        $outputStream = $entry.Open()
        try {
            $inputStream.CopyTo($outputStream)
        }
        finally {
            $outputStream.Dispose()
        }
    }
    finally {
        $inputStream.Dispose()
    }
}

function New-MigrationArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StageRoot,
        [Parameter(Mandatory = $true)]
        [string]$ZipPath
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    $dataRoot = Join-Path $StageRoot 'data'
    $rootDirectory = Get-Item -LiteralPath $dataRoot
    $items = @($rootDirectory)
    $items += Get-ChildItem -LiteralPath $dataRoot -Force -Recurse | Sort-Object FullName
    $total = [Math]::Max($items.Count, 1)
    $index = 0

    $zipArchive = [System.IO.Compression.ZipFile]::Open($ZipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($item in $items) {
            $index += 1
            $relative = Get-ZipRelativePath -Path $item.FullName -BasePath $StageRoot
            Write-Progress -Activity (Get-Message 'StepZip') -Status $relative -PercentComplete (($index / $total) * 100)
            if ($item.PSIsContainer) {
                Add-DirectoryEntry -Archive $zipArchive -EntryName $relative
            }
            else {
                Add-FileEntry -Archive $zipArchive -SourceFile $item.FullName -EntryName $relative
            }
        }
    }
    finally {
        $zipArchive.Dispose()
        Write-Progress -Activity (Get-Message 'StepZip') -Completed
    }
}

function Write-Summary {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,
        [Parameter(Mandatory = $true)]
        [bool]$IncludeBackups
    )

    $backupsLabel = if ($IncludeBackups) { Get-Message 'Yes' } else { Get-Message 'No' }

    Write-Host ''
    Write-Host '------------------------------'
    Write-Host (Get-Message 'SummaryTitle')
    Write-Host ('- {0}: {1}' -f (Get-Message 'SummaryRoot'), $Root)
    Write-Host ('- {0}: {1}' -f (Get-Message 'SummaryBackups'), $backupsLabel)
    Write-Host ('- {0}: public/scripts/extensions/third-party -> data/extensions/third-party' -f (Get-Message 'SummaryGlobalExt'))
    Write-Host '------------------------------'
    Write-Host ''
}

function Write-FinalLocation {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    Write-Host ''
    Write-Host '============================================================'
    Write-Host (Get-Message 'FinalTitle')
    Write-Host ('{0}: {1}' -f (Get-Message 'FinalPathLabel'), $Path)
    Write-Host (Get-Message 'FinalHint')
    Write-Host '============================================================'
}

Select-Language
Write-Banner

$sillyTavernRoot = Resolve-SillyTavernRoot
$includeBackups = Read-IncludeBackups
Write-Summary -Root $sillyTavernRoot -IncludeBackups:$includeBackups

$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('tauritavern-migration-' + [System.Guid]::NewGuid().ToString('N'))
$zipPath = Join-Path $sillyTavernRoot ('tauritavern-data-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.zip')
$finalZipPath = $zipPath

try {
    Write-Step -Index '1/5' -Message (Get-Message 'StepPrepare')
    $stageDataRoot = Join-Path $workRoot 'data'
    $stageDefaultUser = Join-Path $stageDataRoot 'default-user'
    $stageGlobalExtensions = Join-Path $stageDataRoot 'extensions/third-party'
    New-Item -ItemType Directory -Path $stageDefaultUser -Force | Out-Null
    New-Item -ItemType Directory -Path $stageGlobalExtensions -Force | Out-Null

    Write-Step -Index '2/5' -Message (Get-Message 'StepCopyUser')
    $sourceDefaultUser = Join-Path $sillyTavernRoot 'data/default-user'
    if ($includeBackups) {
        Copy-DirectoryChildren -Source $sourceDefaultUser -Destination $stageDefaultUser
    }
    else {
        Copy-DirectoryChildren -Source $sourceDefaultUser -Destination $stageDefaultUser -ExcludeNames @('backups')
        Write-InfoLine (Get-Message 'StepSkipBackups')
    }

    Write-Step -Index '3/5' -Message (Get-Message 'StepCopyExtensions')
    $sourceGlobalExtensions = Join-Path $sillyTavernRoot 'public/scripts/extensions/third-party'
    Copy-DirectoryChildren -Source $sourceGlobalExtensions -Destination $stageGlobalExtensions

    Write-Step -Index '4/5' -Message (Get-Message 'StepCountEntries')
    $entryCount = (Get-ChildItem -LiteralPath $stageDataRoot -Force -Recurse | Measure-Object).Count + 1
    Write-InfoLine ("{0}: {1}" -f (Get-Message 'StepCountEntries'), $entryCount)

    Write-Step -Index '5/5' -Message (Get-Message 'StepZip')
    New-MigrationArchive -StageRoot $workRoot -ZipPath $zipPath

    Write-Step -Index 'Post' -Message (Get-Message 'StepMoveDownloads')
    $downloadsDir = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Downloads'
    if (Test-Path -LiteralPath $downloadsDir -PathType Container) {
        try {
            $movedPath = Join-Path $downloadsDir ([System.IO.Path]::GetFileName($zipPath))
            Move-Item -LiteralPath $zipPath -Destination $movedPath -Force
            $finalZipPath = $movedPath
            Write-InfoLine (Get-Message 'MoveSuccess')
        }
        catch {
            Write-WarningLine ((Get-Message 'MoveFailed') + ' ' + $_.Exception.Message)
        }
    }
    else {
        Write-WarningLine (Get-Message 'MoveFailed')
    }

    Write-FinalLocation -Path $finalZipPath
}
finally {
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force
    }
}
