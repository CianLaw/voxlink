#!/usr/bin/env pwsh
# ============================================================================
# VoxLink Windows 代码签名脚本
# 功能：使用 Authenticode 证书对 .msi / .exe 进行硬签名
# 前置条件：
#   1. 已安装 Windows SDK (包含 signtool.exe)
#   2. 证书已安装到当前用户的证书存储中
#   3. 或使用 .pfx 文件进行签名
# ============================================================================

param(
    [Parameter(Mandatory=$true)]
    [string]$TargetPath,

    [Parameter(Mandatory=$false)]
    [string]$CertificateThumbprint = "",

    [Parameter(Mandatory=$false)]
    [string]$PfxPath = "",

    [Parameter(Mandatory=$false)]
    [string]$PfxPassword = "",

    [Parameter(Mandatory=$false)]
    [string]$TimestampUrl = "http://timestamp.digicert.com",

    [Parameter(Mandatory=$false)]
    [string]$Description = "VoxLink - 高精度跨平台语音输入助手"
)

$ErrorActionPreference = "Stop"

# 查找 signtool.exe
$signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if (-not $signtool) {
    $signtoolPaths = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe",
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.22000.0\x64\signtool.exe",
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.20348.0\x64\signtool.exe",
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\x64\signtool.exe"
    )

    foreach ($path in $signtoolPaths) {
        if (Test-Path $path) {
            $signtool = $path
            break
        }
    }

    if (-not $signtool) {
        Write-Error "未找到 signtool.exe。请安装 Windows SDK。"
        exit 1
    }
}

if (-not (Test-Path $TargetPath)) {
    Write-Error "目标文件不存在: $TargetPath"
    exit 1
}

Write-Host "============================================" -ForegroundColor Cyan
Write-Host "  VoxLink Windows 代码签名" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host "目标文件: $TargetPath" -ForegroundColor White
Write-Host "时间戳服务器: $TimestampUrl" -ForegroundColor White

# 构建签名参数
$signArgs = @(
    "sign",
    "/fd", "SHA256",
    "/tr", $TimestampUrl,
    "/td", "SHA256",
    "/d", $Description,
    "/du", "https://voxlink.app"
)

if ($CertificateThumbprint) {
    Write-Host "使用证书指纹: $CertificateThumbprint" -ForegroundColor White
    $signArgs += "/sha1"
    $signArgs += $CertificateThumbprint
} elseif ($PfxPath -and $PfxPassword) {
    Write-Host "使用 PFX 证书文件: $PfxPath" -ForegroundColor White
    $signArgs += "/f"
    $signArgs += $PfxPath
    $signArgs += "/p"
    $signArgs += $PfxPassword
} else {
    Write-Error "请提供证书指纹 (-CertificateThumbprint) 或 PFX 文件路径 (-PfxPath 和 -PfxPassword)"
    exit 1
}

# 添加目标文件
$signArgs += $TargetPath

Write-Host ""
Write-Host "执行签名命令:" -ForegroundColor Yellow
Write-Host "& signtool.exe $($signArgs -join ' ')" -ForegroundColor Gray

# 执行签名
try {
    $process = Start-Process -FilePath $signtool -ArgumentList $signArgs -NoNewWindow -Wait -PassThru

    if ($process.ExitCode -eq 0) {
        Write-Host ""
        Write-Host "签名成功!" -ForegroundColor Green

        # 验证签名
        Write-Host ""
        Write-Host "验证签名..." -ForegroundColor Yellow
        & $signtool verify /pa /v $TargetPath

        if ($LASTEXITCODE -eq 0) {
            Write-Host "签名验证通过!" -ForegroundColor Green
        } else {
            Write-Warning "签名验证失败，请检查证书配置"
        }
    } else {
        Write-Error "签名失败，退出码: $($process.ExitCode)"
        exit 1
    }
} catch {
    Write-Error "签名过程出错: $_"
    exit 1
}

Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host "  签名完成" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan