# install.ps1 — Windows 一行安装（PowerShell）
#
#   irm https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.ps1 | iex
#
# 装到 %LOCALAPPDATA%\xrayctl\xrayctl.exe 并加进用户 PATH（不需要管理员权限）。
# 私有仓需要先 `gh auth login`，脚本会自动走 gh 取资产。
$ErrorActionPreference = 'Stop'

$repo = if ($env:XRAYCTL_REPO) { $env:XRAYCTL_REPO } else { 'Helioswei/vps' }
$version = if ($env:XRAYCTL_VERSION) { $env:XRAYCTL_VERSION } else { 'latest' }
$asset = 'xrayctl-windows-x86_64.exe'

$dir = Join-Path $env:LOCALAPPDATA 'xrayctl'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$out = Join-Path $dir 'xrayctl.exe'

if ($version -eq 'latest') {
    $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$repo/releases/download/$version/$asset"
}

Write-Host "下载 $asset（$version）…"
try {
    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
} catch {
    # 私有仓的直链对匿名请求是 404，退回 gh（已登录时可用）
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        $tagArgs = @('release', 'download')
        if ($version -ne 'latest') { $tagArgs += $version }
        $tagArgs += @('--repo', $repo, '--pattern', $asset, '--output', $out, '--clobber')
        & gh @tagArgs
        if ($LASTEXITCODE -ne 0) { throw "gh 下载失败，先运行 gh auth login" }
    } else {
        throw "下载失败：$($_.Exception.Message)。私有仓请先安装 gh 并 gh auth login。"
    }
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$dir*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User')
    Write-Host "已把 $dir 加入用户 PATH，重开终端生效。"
}

& $out --version
Write-Host ""
Write-Host "用法：xrayctl --host <你的ssh别名> status"
Write-Host "只装服务器、本地不装：bash install.sh --remote <ssh别名>（Git Bash/WSL 里跑）"
