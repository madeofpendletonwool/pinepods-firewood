# 🌲 PinePods Firewood Installation Guide

Multiple ways to install PinePods Firewood on your system.

## 🚀 Quick Install (Recommended)

**One-liner installer** (Linux/macOS/Windows with Git Bash):
```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh | bash
```

Or download and inspect first:
```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh -o install.sh
chmod +x install.sh
./install.sh --help
```

## 📦 Package Managers

### Homebrew (macOS/Linux)
```bash
# Add our tap (once available)
brew tap madeofpendletonwool/pinepods
brew install pinepods-firewood

# Or install directly from URL
brew install https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/pkg/homebrew/pinepods-firewood.rb
```

### Cargo (Rust)
```bash
cargo install pinepods-firewood
```

### Arch Linux (AUR)
```bash
# Using yay
yay -S pinepods-firewood

# Using paru  
paru -S pinepods-firewood

# Manual
git clone https://aur.archlinux.org/pinepods-firewood.git
cd pinepods-firewood
makepkg -si
```

### Ubuntu/Debian (.deb)
```bash
# Download and install
wget https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest/download/pinepods-firewood_VERSION_amd64.deb
sudo dpkg -i pinepods-firewood_VERSION_amd64.deb

# Fix dependencies if needed
sudo apt-get install -f
```

### Fedora/RHEL/CentOS (.rpm)
```bash
# Download and install
wget https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest/download/pinepods-firewood-VERSION-1.x86_64.rpm
sudo rpm -i pinepods-firewood-VERSION-1.x86_64.rpm

# Or with dnf
sudo dnf install ./pinepods-firewood-VERSION-1.x86_64.rpm
```

### Snap
```bash
sudo snap install pinepods-firewood
```

### Windows Package Managers

#### Scoop
```powershell
# Add bucket (once available)
scoop bucket add pinepods https://github.com/madeofpendletonwool/scoop-pinepods
scoop install pinepods-firewood
```

#### Chocolatey
```powershell
# Once available on Chocolatey
choco install pinepods-firewood
```

#### Winget
```powershell
# Once available in winget
winget install PinePods.Firewood
```

### Nix/NixOS
```bash
# Add to your configuration.nix or install directly
nix-env -iA nixpkgs.pinepods-firewood

# Or with nix-shell
nix-shell -p pinepods-firewood
```

## 📥 Manual Installation

### Pre-built Binaries

1. **Download** the latest release for your platform:
   - **Linux x64**: `pinepods-firewood-linux-amd64.tar.gz`
   - **Linux ARM64**: `pinepods-firewood-linux-arm64.tar.gz`
   - **macOS Intel**: `pinepods-firewood-macos-amd64.tar.gz`
   - **macOS Apple Silicon**: `pinepods-firewood-macos-arm64.tar.gz`
   - **Windows x64**: `pinepods-firewood-windows-amd64.zip`

2. **Extract** the archive:
   ```bash
   # Linux/macOS
   tar -xzf pinepods-firewood-*.tar.gz
   
   # Windows (PowerShell)
   Expand-Archive pinepods-firewood-*.zip
   ```

3. **Move** binary to your PATH:
   ```bash
   # Linux/macOS
   sudo mv pinepods_firewood /usr/local/bin/
   chmod +x /usr/local/bin/pinepods_firewood
   
   # Or to user directory
   mkdir -p ~/.local/bin
   mv pinepods_firewood ~/.local/bin/
   ```

### Build from Source

**Prerequisites:**
- Rust 1.75+ (`rustup` recommended)
- System dependencies:
  - **Linux**: `pkg-config`, `libasound2-dev`, `libssl-dev`
  - **macOS**: Xcode command line tools
  - **Windows**: Visual Studio Build Tools

```bash
# Clone repository
git clone https://github.com/madeofpendletonwool/pinepods-firewood.git
cd pinepods-firewood

# Install dependencies (Linux)
sudo apt update && sudo apt install -y pkg-config libasound2-dev libssl-dev

# Install dependencies (macOS)
xcode-select --install
brew install pkg-config

# Build and install
cargo build --release
sudo cp target/release/pinepods_firewood /usr/local/bin/

# Or install directly with cargo
cargo install --path .
```

## ✅ Verify Installation

```bash
pinepods_firewood --version
pinepods_firewood --help
```

## 🔄 Updates

### Automatic Updates
The installer script supports updating:
```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh | bash -s -- --force
```

### Package Manager Updates
```bash
# Homebrew
brew upgrade pinepods-firewood

# Cargo
cargo install pinepods-firewood --force

# Snap
sudo snap refresh pinepods-firewood

# AUR
yay -Syu pinepods-firewood
```

## 🗑️ Uninstallation

### Manual
```bash
# Remove binary
sudo rm /usr/local/bin/pinepods_firewood
# or
rm ~/.local/bin/pinepods_firewood

# Remove config (optional)
rm -rf ~/.config/pinepods-firewood
```

### Package Managers
```bash
# Homebrew
brew uninstall pinepods-firewood

# Cargo
cargo uninstall pinepods-firewood

# Snap
sudo snap remove pinepods-firewood

# APT
sudo apt remove pinepods-firewood

# DNF/YUM
sudo dnf remove pinepods-firewood
```

## 🛠️ Troubleshooting

### Audio Issues
**Linux**: Install ALSA development libraries
```bash
# Ubuntu/Debian
sudo apt install libasound2-dev alsa-utils

# Fedora
sudo dnf install alsa-lib-devel alsa-utils

# Arch
sudo pacman -S alsa-lib alsa-utils
```

**macOS**: Audio should work out of the box with system frameworks.

**Windows**: Audio uses Windows native APIs.

### Permission Issues
If you get permission errors:
```bash
# Use user directory instead of system
./install.sh --dir ~/.local/bin

# Make sure ~/.local/bin is in PATH
echo 'export PATH="$PATH:$HOME/.local/bin"' >> ~/.bashrc
source ~/.bashrc
```

### Network Issues
If downloads fail, try:
```bash
# Use different mirror or direct download
wget https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest/download/pinepods-firewood-linux-amd64.tar.gz

# Or build from source
git clone https://github.com/madeofpendletonwool/pinepods-firewood.git
cd pinepods-firewood && cargo build --release
```

## 📞 Support

- **Issues**: [GitHub Issues](https://github.com/madeofpendletonwool/pinepods-firewood/issues)
- **Discussions**: [GitHub Discussions](https://github.com/madeofpendletonwool/pinepods-firewood/discussions)
- **Documentation**: [Main README](README.md)