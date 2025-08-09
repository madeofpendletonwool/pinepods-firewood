#!/bin/bash

# Script to update package manager configurations after a release
# This script should be run after GitHub release is created

set -e

VERSION="$1"
DRY_RUN=false

if [ "$2" = "--dry-run" ]; then
    DRY_RUN=true
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

get_sha256() {
    local url="$1"
    log_info "Fetching SHA256 for $url"
    
    if command -v curl >/dev/null 2>&1; then
        curl -sL "$url" | shasum -a 256 | cut -d' ' -f1
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$url" | shasum -a 256 | cut -d' ' -f1
    else
        log_error "Neither curl nor wget available"
        exit 1
    fi
}

update_homebrew_formula() {
    local version="$1"
    local formula_file="pkg/homebrew/pinepods-firewood.rb"
    
    log_info "Updating Homebrew formula for version $version"
    
    # Get SHA256 hashes for all platforms
    local base_url="https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v$version"
    local macos_amd64_sha256
    local macos_arm64_sha256
    local linux_amd64_sha256
    local linux_arm64_sha256
    
    if [ "$DRY_RUN" = false ]; then
        log_info "Calculating SHA256 hashes (this may take a moment)..."
        macos_amd64_sha256=$(get_sha256 "$base_url/pinepods-firewood-macos-amd64.tar.gz")
        macos_arm64_sha256=$(get_sha256 "$base_url/pinepods-firewood-macos-arm64.tar.gz")
        linux_amd64_sha256=$(get_sha256 "$base_url/pinepods-firewood-linux-amd64.tar.gz")
        linux_arm64_sha256=$(get_sha256 "$base_url/pinepods-firewood-linux-arm64.tar.gz")
    else
        log_warn "DRY RUN: Skipping SHA256 calculation"
        macos_amd64_sha256="PLACEHOLDER_SHA256"
        macos_arm64_sha256="PLACEHOLDER_SHA256"
        linux_amd64_sha256="PLACEHOLDER_SHA256"
        linux_arm64_sha256="PLACEHOLDER_SHA256"
    fi
    
    # Update formula file
    if [ -f "$formula_file" ]; then
        cp "$formula_file" "$formula_file.bak"
        
        sed -e "s/__VERSION__/$version/g" \
            -e "s/__MACOS_AMD64_SHA256__/$macos_amd64_sha256/g" \
            -e "s/__MACOS_ARM64_SHA256__/$macos_arm64_sha256/g" \
            -e "s/__LINUX_AMD64_SHA256__/$linux_amd64_sha256/g" \
            -e "s/__LINUX_ARM64_SHA256__/$linux_arm64_sha256/g" \
            "$formula_file.bak" > "$formula_file"
        
        rm "$formula_file.bak"
        log_success "Updated $formula_file"
    else
        log_error "Formula file not found: $formula_file"
        exit 1
    fi
}

update_snap_version() {
    local version="$1"
    local snap_file="snap/snapcraft.yaml"
    
    log_info "Updating Snap version to $version"
    
    if [ -f "$snap_file" ]; then
        # Update version in snapcraft.yaml
        sed -i.bak "s/^version:.*/version: '$version'/" "$snap_file"
        rm "$snap_file.bak"
        log_success "Updated $snap_file"
    else
        log_warn "Snap file not found: $snap_file"
    fi
}

update_aur_pkgbuild() {
    local version="$1"
    
    log_info "Updating AUR PKGBUILD template"
    
    # Create AUR package template
    local aur_dir="pkg/aur"
    mkdir -p "$aur_dir"
    
    cat > "$aur_dir/PKGBUILD" << EOF
# Maintainer: PinePods Team <support@pinepods.online>
pkgname=pinepods-firewood
pkgver=$version
pkgrel=1
pkgdesc="Terminal UI client for PinePods podcast server"
arch=('x86_64' 'aarch64')
url="https://github.com/madeofpendletonwool/pinepods-firewood"
license=('MIT')
depends=('alsa-lib' 'openssl')
provides=('pinepods-firewood')
conflicts=('pinepods-firewood')
source_x86_64=("https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v\$pkgver/pinepods-firewood-linux-amd64.tar.gz")
source_aarch64=("https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v\$pkgver/pinepods-firewood-linux-arm64.tar.gz")
sha256sums_x86_64=('SKIP')  # Update with actual SHA256
sha256sums_aarch64=('SKIP')  # Update with actual SHA256

package() {
    install -Dm755 "\$srcdir/pinepods_firewood" "\$pkgdir/usr/bin/pinepods_firewood"
    
    # Install license if available
    if [ -f "\$srcdir/LICENSE" ]; then
        install -Dm644 "\$srcdir/LICENSE" "\$pkgdir/usr/share/licenses/\$pkgname/LICENSE"
    fi
}
EOF
    
    log_success "Created AUR PKGBUILD template at $aur_dir/PKGBUILD"
}


create_winget_manifest() {
    local version="$1"
    local manifest_dir="pkg/winget"
    
    log_info "Creating Winget manifest template"
    
    mkdir -p "$manifest_dir"
    
    cat > "$manifest_dir/PinePods.Firewood.installer.yaml" << EOF
PackageIdentifier: PinePods.Firewood
PackageVersion: $version
InstallerLocale: en-US
MinimumOSVersion: 10.0.0.0
InstallerType: zip
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v$version/pinepods-firewood-windows-amd64.zip
  InstallerSha256: PLACEHOLDER_SHA256
ManifestType: installer
ManifestVersion: 1.2.0
EOF

    cat > "$manifest_dir/PinePods.Firewood.locale.en-US.yaml" << EOF
PackageIdentifier: PinePods.Firewood
PackageVersion: $version
PackageLocale: en-US
Publisher: PinePods Team
PublisherUrl: https://pinepods.online
PackageName: PinePods Firewood
PackageUrl: https://github.com/madeofpendletonwool/pinepods-firewood
License: MIT
LicenseUrl: https://github.com/madeofpendletonwool/pinepods-firewood/blob/main/LICENSE
ShortDescription: Terminal UI client for PinePods podcast server
Description: |-
  A fast, lightweight TUI client for interacting with PinePods
  podcast server instances. Built with Rust and Ratatui.
Moniker: pinepods-firewood
Tags:
- podcast
- tui
- terminal
- audio
- media
ManifestType: defaultLocale
ManifestVersion: 1.2.0
EOF

    cat > "$manifest_dir/PinePods.Firewood.yaml" << EOF
PackageIdentifier: PinePods.Firewood
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.2.0
EOF
    
    log_success "Created Winget manifest templates in $manifest_dir"
}

main() {
    echo -e "${BLUE}"
    echo "┌─────────────────────────────────────────┐"
    echo "│     📦 Package Manager Updater         │"
    echo "│    PinePods Firewood Release Tools     │"
    echo "└─────────────────────────────────────────┘"
    echo -e "${NC}"
    
    if [ -z "$VERSION" ]; then
        log_error "Version is required"
        echo "Usage: $0 <version> [--dry-run]"
        echo "Example: $0 1.2.3"
        exit 1
    fi
    
    # Remove 'v' prefix if present
    VERSION=${VERSION#v}
    
    log_info "Updating packages for version $VERSION"
    
    # Update all package configurations
    update_homebrew_formula "$VERSION"
    update_snap_version "$VERSION"
    update_aur_pkgbuild "$VERSION"
    create_winget_manifest "$VERSION"
    
    if [ "$DRY_RUN" = false ]; then
        log_success "All package configurations updated successfully!"
        echo ""
        log_info "Next steps:"
        echo "1. Review and commit the updated package files"
        echo "2. Submit to package managers as needed:"
        echo "   - Homebrew: Create PR to homebrew-core or your tap"
        echo "   - AUR: Update the pinepods-firewood AUR package"
        echo "   - Snap: Run 'snapcraft upload' if you have store access"
        echo "   - Winget: Submit PR to microsoft/winget-pkgs"
        echo "3. Test installations from various package managers"
    else
        log_warn "DRY RUN completed - no files were actually updated"
    fi
}

main "$@"