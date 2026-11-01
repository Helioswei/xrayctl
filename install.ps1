# install.ps1 — Windows 一行安装（PowerShell）
#
#   irm https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.ps1 | iex
#   国内不通时：irm https://gh-proxy.com/https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.ps1 | iex
#
# 装到 %LOCALAPPDATA%\xrayctl\xrayctl.exe 并加进用户 PATH（不需要管理员权限）。
# 直链不通会自动试镜像前缀；每个来源都用 SHA256SUMS 校验，校验不符就换下一个。
$ErrorActionPreference = 'Stop'

$repo = if ($env:XRAYCTL_REPO) { $env:XRAYCTL_REPO } else { 'Helioswei/xrayctl' }
$version = if ($env:XRAYCTL_VERSION) { $env:XRAYCTL_VERSION } else { 'latest' }
$mirrors = if ($env:XRAYCTL_MIRROR) { $env:XRAYCTL_MIRROR -split '\s+' } else { @('https://gh-proxy.com/', 'https://ghfast.top/') }
$asset = 'xrayctl-windows-x86_64.exe'

$dir = Join-Path $env:LOCALAPPDATA 'xrayctl'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$out = Join-Path $dir 'xrayctl.exe'

if ($version -eq 'latest') {
    $base = "https://github.com/$repo/releases/latest/download"
} else {
    $base = "https://github.com/$repo/releases/download/$version"
}

function Get-WithMirrors([string]$name, [string]$dest) {
    $sources = @($base)
    foreach ($m in $mirrors) { if ($m) { $sources += ($m + $base) } }
    foreach ($s in $sources) {
        try {
            Invoke-WebRequest -Uri "$s/$name" -OutFile $dest -UseBasicParsing
            return $s
        } catch { continue }
    }
    return $null
}

$sums = Join-Path $dir 'SHA256SUMS'
if (-not (Get-WithMirrors 'SHA256SUMS' $sums)) { throw '拿不到 SHA256SUMS，拒绝安装' }
$want = (Select-String -Path $sums -Pattern (" " + [regex]::Escape($asset) + '$') |
         Select-Object -First 1).Line -split '\s+' | Select-Object -First 1
if (-not $want) { throw "拿不到 $asset 的 sha256，拒绝安装" }

Write-Host "下载 $asset（$version）…"
$sources = @($base)
foreach ($m in $mirrors) { if ($m) { $sources += ($m + $base) } }
$ok = $false
foreach ($s in $sources) {
    try { Invoke-WebRequest -Uri "$s/$asset" -OutFile $out -UseBasicParsing } catch { continue }
    $got = (Get-FileHash -Algorithm SHA256 -Path $out).Hash.ToLower()
    if ($got -eq $want.ToLower()) { Write-Host "  来源 $s：sha256 匹配"; $ok = $true; break }
    Write-Host "  来源 $s：校验不符，换下一个"
}
if (-not $ok) {
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        $tagArgs = @('release', 'download')
        if ($version -ne 'latest') { $tagArgs += $version }
        $tagArgs += @('--repo', $repo, '--pattern', $asset, '--output', $out, '--clobber')
        & gh @tagArgs
        if ($LASTEXITCODE -ne 0) { throw 'gh 下载失败，先运行 gh auth login' }
    } else {
        throw '所有来源都失败。可设 XRAYCTL_MIRROR 指定镜像前缀，例如 https://gh-proxy.com/'
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
