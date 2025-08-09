#!/bin/bash

# PinePods Firewood Installation Script
# Supports Linux, macOS, and Windows (via Git Bash/WSL)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default values
INSTALL_DIR=""
FORCE_INSTALL=false
VERSION="latest"

# Functions
print_header() {
    echo -e "${BLUE}"
    echo "┌─────────────────────────────────────────┐"
    echo "│     🌲 PinePods Firewood Installer     │"
    echo "│   Terminal UI for PinePods Server      │"
    echo "└─────────────────────────────────────────┘"
    echo -e "${NC}"
}

print_usage() {
    echo "Usage: $0 [options]"
    echo "Options:"
    echo "  -d, --dir DIR        Install directory (default: /usr/local/bin or ~/bin)"
    echo "  -v, --version VER    Install specific version (default: latest)"
    echo "  -f, --force          Force installation even if already installed"
    echo "  -h, --help           Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                           # Install latest to default location"
    echo "  $0 -d ~/.local/bin           # Install to custom directory"
    echo "  $0 -v v1.0.0                # Install specific version"
    echo ""
    echo "Quick install:"
    echo "  curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh | bash"
}

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

detect_platform() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    local arch=$(uname -m)
    
    case $os in
        linux*)
            case $arch in
                x86_64|amd64) echo "linux-amd64" ;;
                aarch64|arm64) echo "linux-arm64" ;;
                *) log_error "Unsupported architecture: $arch"; exit 1 ;;
            esac
            ;;
        darwin*)
            case $arch in
                x86_64) echo "macos-amd64" ;;
                arm64) echo "macos-arm64" ;;
                *) log_error "Unsupported architecture: $arch"; exit 1 ;;
            esac
            ;;
        mingw*|msys*|cygwin*)
            echo "windows-amd64"
            ;;
        *)
            log_error "Unsupported operating system: $os"
            exit 1
            ;;
    esac
}

get_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        echo "$INSTALL_DIR"
        return
    fi
    
    # Try to install to system directory if we have permission
    if [ -w "/usr/local/bin" ] 2>/dev/null; then
        echo "/usr/local/bin"
    elif [ -w "$(dirname "$(which bash)")" ] 2>/dev/null; then
        dirname "$(which bash)"
    else
        # Fall back to user directory
        mkdir -p "$HOME/bin"
        echo "$HOME/bin"
    fi
}

get_latest_version() {
    local url="https://api.github.com/repos/madeofpendletonwool/pinepods-firewood/releases/latest"
    
    if command -v curl >/dev/null 2>&1; then
        curl -s "$url" | grep '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$url" | grep '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/'
    else
        log_error "Neither curl nor wget is available. Please install one of them."
        exit 1
    fi
}

download_and_install() {
    local platform=$(detect_platform)
    local install_dir=$(get_install_dir)
    local version_tag
    
    if [ "$VERSION" = "latest" ]; then
        version_tag=$(get_latest_version)
        if [ -z "$version_tag" ]; then
            log_error "Failed to fetch latest version"
            exit 1
        fi
        log_info "Latest version: $version_tag"
    else
        version_tag=$VERSION
    fi
    
    # Ensure version starts with 'v'
    if [[ ! $version_tag =~ ^v ]]; then
        version_tag="v$version_tag"
    fi
    
    local binary_name="pinepods_firewood"
    local archive_ext=".tar.gz"
    
    if [[ $platform == windows-* ]]; then
        binary_name="pinepods_firewood.exe"
        archive_ext=".zip"
    fi
    
    local archive_name="pinepods-firewood-${platform}${archive_ext}"
    local download_url="https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/${version_tag}/${archive_name}"
    
    log_info "Downloading from: $download_url"
    log_info "Installing to: $install_dir"
    
    # Create temporary directory
    local tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT
    
    # Download archive
    log_info "Downloading PinePods Firewood $version_tag..."
    if command -v curl >/dev/null 2>&1; then
        if ! curl -L -o "$tmp_dir/$archive_name" "$download_url"; then
            log_error "Failed to download $archive_name"
            exit 1
        fi
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -O "$tmp_dir/$archive_name" "$download_url"; then
            log_error "Failed to download $archive_name"
            exit 1
        fi
    else
        log_error "Neither curl nor wget is available"
        exit 1
    fi
    
    # Extract archive
    log_info "Extracting archive..."
    cd "$tmp_dir"
    if [[ $platform == windows-* ]]; then
        if command -v unzip >/dev/null 2>&1; then
            unzip -q "$archive_name"
        else
            log_error "unzip is required to extract Windows archives"
            exit 1
        fi
    else
        tar -xzf "$archive_name"
    fi
    
    # Check if binary exists and is executable
    if [ ! -f "$binary_name" ]; then
        log_error "Binary $binary_name not found in archive"
        exit 1
    fi
    
    # Create install directory if needed
    mkdir -p "$install_dir"
    
    # Check if already installed
    local target_path="$install_dir/$binary_name"
    if [ -f "$target_path" ] && [ "$FORCE_INSTALL" = false ]; then
        log_warn "PinePods Firewood is already installed at $target_path"
        echo -n "Do you want to overwrite it? (y/N): "
        read -r response
        if [[ ! $response =~ ^[Yy]$ ]]; then
            log_info "Installation cancelled"
            exit 0
        fi
    fi
    
    # Install binary
    log_info "Installing binary to $target_path..."
    if ! cp "$binary_name" "$target_path"; then
        log_error "Failed to copy binary to $target_path"
        log_error "You may need to run this script with elevated permissions"
        exit 1
    fi
    
    # Make executable (not needed on Windows)
    if [[ ! $platform == windows-* ]]; then
        chmod +x "$target_path"
    fi
    
    log_success "PinePods Firewood $version_tag installed successfully!"
    
    # Check if install directory is in PATH
    if [[ ":$PATH:" != *":$install_dir:"* ]]; then
        log_warn "Warning: $install_dir is not in your PATH"
        echo "To use pinepods_firewood from anywhere, add this to your shell profile:"
        echo "  export PATH=\"\$PATH:$install_dir\""
    fi
    
    # Test installation
    log_info "Testing installation..."
    if "$target_path" --version >/dev/null 2>&1; then
        log_success "Installation verified successfully!"
    else
        log_warn "Installation may have issues. Try running: $target_path --version"
    fi
    
    echo ""
    echo -e "${GREEN}🎉 Installation complete!${NC}"
    echo ""
    echo "Usage:"
    echo "  $binary_name                    # Run with default settings"
    echo "  $binary_name --help             # Show help message"
    echo ""
    echo "Documentation: https://github.com/madeofpendletonwool/pinepods-firewood"
    echo "Issues: https://github.com/madeofpendletonwool/pinepods-firewood/issues"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        -v|--version)
            VERSION="$2"
            shift 2
            ;;
        -f|--force)
            FORCE_INSTALL=true
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            print_usage
            exit 1
            ;;
    esac
done

# Main execution
main() {
    print_header
    
    # Check for required tools
    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
        log_error "Either curl or wget is required for installation"
        exit 1
    fi
    
    log_info "Starting PinePods Firewood installation..."
    log_info "Platform: $(detect_platform)"
    
    download_and_install
}

# Run main function
main "$@"