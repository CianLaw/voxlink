#!/bin/bash
# ============================================================================
# VoxLink macOS 代码签名与公证脚本
# 功能：使用 Apple Developer 证书对 .app/.dmg 进行签名、公证、装订
# 前置条件：
#   1. 已安装 Xcode Command Line Tools
#   2. 证书已导入 Keychain
#   3. 已设置环境变量（APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD）
# ============================================================================

set -euo pipefail

# 配置
APP_NAME="VoxLink"
APP_PATH="${1:-}"
DEVELOPER_ID_APP="${DEVELOPER_ID_APP:-}"
DEVELOPER_ID_INSTALLER="${DEVELOPER_ID_INSTALLER:-}"
APPLE_ID="${APPLE_ID:-}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-}"
APPLE_APP_PASSWORD="${APPLE_APP_PASSWORD:-}"
NOTARY_TOOL="xcrun notarytool"
STAPLER="xcrun stapler"
KEYCHAIN_PROFILE="voxlink-notary"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# 检查必要工具
check_tools() {
    log_info "检查必要工具..."

    if ! command -v xcrun &> /dev/null; then
        log_error "未找到 xcrun，请安装 Xcode Command Line Tools"
        exit 1
    fi

    if ! command -v codesign &> /dev/null; then
        log_error "未找到 codesign"
        exit 1
    fi

    log_success "工具检查通过"
}

# 导入证书到临时 Keychain（CI 环境）
import_certificate() {
    local cert_path="${BUILD_CERTIFICATE_BASE64:-}"
    local cert_password="${P12_PASSWORD:-}"
    local keychain_path="${RUNNER_TEMP:-/tmp}/voxlink-build.keychain-db"

    if [ -z "$cert_path" ] && [ -z "${P12_FILE_PATH:-}" ]; then
        log_warn "未提供证书文件，跳过导入"
        return
    fi

    log_info "导入 Apple 开发者证书..."

    if [ -n "${BUILD_CERTIFICATE_BASE64:-}" ]; then
        echo "$BUILD_CERTIFICATE_BASE64" | base64 --decode > /tmp/voxlink_cert.p12
        cert_path="/tmp/voxlink_cert.p12"
    else
        cert_path="${P12_FILE_PATH}"
    fi

    # 创建临时 Keychain
    security create-keychain -p "" "$keychain_path"
    security default-keychain -s "$keychain_path"
    security unlock-keychain -p "" "$keychain_path"

    # 导入证书
    security import "$cert_path" -k "$keychain_path" -P "$cert_password" -T /usr/bin/codesign -T /usr/bin/productsign -T /usr/bin/security
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "" "$keychain_path"

    log_success "证书导入完成"
}

# 签名应用
sign_app() {
    local app_path="$1"
    log_info "签名应用: $app_path"

    if [ -z "$DEVELOPER_ID_APP" ]; then
        log_error "未设置 DEVELOPER_ID_APP 环境变量"
        exit 1
    fi

    # 深度签名所有二进制文件
    find "$app_path" -type f -name "*.dylib" -exec codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID_APP" {} \; || true
    find "$app_path" -type f -name "*.framework" -exec codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID_APP" {} \; || true

    # 签名主应用
    codesign --force --options runtime --timestamp \
        --entitlements "$APP_PATH/../Entitlements.plist" \
        --sign "$DEVELOPER_ID_APP" \
        --deep \
        "$app_path"

    # 验证签名
    codesign --verify --verbose=4 "$app_path"
    log_success "应用签名完成"
}

# 签名 DMG
sign_dmg() {
    local dmg_path="$1"
    log_info "签名 DMG: $dmg_path"

    if [ -z "$DEVELOPER_ID_INSTALLER" ]; then
        log_warn "未设置 DEVELOPER_ID_INSTALLER，跳过 DMG 签名"
        return
    fi

    codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID_INSTALLER" "$dmg_path"
    codesign --verify --verbose=4 "$dmg_path"
    log_success "DMG 签名完成"
}

# 公证
notarize() {
    local file_path="$1"
    log_info "提交公证: $file_path"

    if [ -z "$APPLE_ID" ] || [ -z "$APPLE_TEAM_ID" ] || [ -z "$APPLE_APP_PASSWORD" ]; then
        log_warn "缺少 Apple 账户信息，跳过公证"
        return
    }

    # 存储凭证到 Keychain
    $NOTARY_TOOL store-credentials "$KEYCHAIN_PROFILE" \
        --apple-id "$APPLE_ID" \
        --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD"

    # 提交公证
    local submission_id
    submission_id=$($NOTARY_TOOL submit "$file_path" \
        --keychain-profile "$KEYCHAIN_PROFILE" \
        --wait \
        --output-format json | jq -r '.id')

    log_info "公证提交 ID: $submission_id"

    # 获取公证状态
    local status
    status=$($NOTARY_TOOL info "$submission_id" \
        --keychain-profile "$KEYCHAIN_PROFILE" \
        --output-format json | jq -r '.status')

    if [ "$status" = "Accepted" ]; then
        log_success "公证通过"
    else
        local log_url
        log_url=$($NOTARY_TOOL log "$submission_id" \
            --keychain-profile "$KEYCHAIN_PROFILE" | head -1)
        log_error "公证失败！日志: $log_url"
        exit 1
    fi
}

# 装订（Staple）
staple() {
    local file_path="$1"
    log_info "装订票据: $file_path"

    if command -v stapler &> /dev/null; then
        $STAPLER staple "$file_path"
        log_success "装订完成"
    else
        log_warn "stapler 不可用，跳过装订"
    fi
}

# 主流程
main() {
    check_tools
    import_certificate

    if [ -z "$APP_PATH" ]; then
        # 自动查找 .app
        APP_PATH=$(find . -name "*.app" -type d -maxdepth 3 | head -1)
        if [ -z "$APP_PATH" ]; then
            log_error "未找到 .app 文件，请指定路径"
            exit 1
        fi
    fi

    log_info "应用路径: $APP_PATH"
    sign_app "$APP_PATH"

    # 查找 DMG
    local dmg_path
    dmg_path=$(find . -name "*.dmg" -type f -maxdepth 3 | head -1)
    if [ -n "$dmg_path" ]; then
        sign_dmg "$dmg_path"
        notarize "$dmg_path"
        staple "$dmg_path"
    fi

    log_success "所有签名和公证操作完成!"
}

main "$@"