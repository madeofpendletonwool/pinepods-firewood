# 🌲 PinePods Firewood Installation Guide

> The only published release so far is 0.1.0. It predates remote control, headless mode,
> cover art, the command palette and most of what the README describes. Until a newer
> release is out, [build from source](#build-from-source) to get them.

## 🚀 Install script

Linux, macOS, and Windows through Git Bash or WSL:

```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh | bash
```

It downloads the latest release for your platform and installs `pinepods_firewood` to
`/usr/local/bin`, or `~/bin` if that isn't writable. To read it first:

```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh -o install.sh
chmod +x install.sh
./install.sh --help
```

`--dir DIR` picks the install directory; `--force` reinstalls, which is also how to update.

## 📥 Release downloads

From [GitHub Releases](https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest).
The release workflow builds:

- `pinepods-firewood-linux-amd64.tar.gz`, `pinepods-firewood-linux-arm64.tar.gz`
- `pinepods-firewood-macos-amd64.tar.gz` (Intel), `pinepods-firewood-macos-arm64.tar.gz` (Apple Silicon)
- `pinepods-firewood-windows-amd64.zip`
- `pinepods-firewood_VERSION_amd64.deb` for Debian and Ubuntu

(0.1.0 has the Linux archives, the Apple Silicon archive and the `.deb`.)

```bash
# Linux/macOS: unpack and put the binary on your PATH
tar -xzf pinepods-firewood-*.tar.gz
sudo mv pinepods_firewood /usr/local/bin/

# Debian/Ubuntu
sudo dpkg -i pinepods-firewood_*_amd64.deb
```

Firewood isn't in Homebrew, crates.io, the AUR or other package managers yet.

## 🔨 Build from Source

You need a Rust toolchain (`rustup`) and:

- **Debian/Ubuntu**: `sudo apt install pkg-config libasound2-dev libssl-dev`
- **Fedora**: `sudo dnf install pkg-config alsa-lib-devel openssl-devel`
- **Arch**: `sudo pacman -S pkg-config alsa-lib openssl`
- **macOS**: `xcode-select --install`, then `brew install pkg-config`
- **Windows**: the Visual Studio Build Tools

```bash
git clone https://github.com/madeofpendletonwool/pinepods-firewood.git
cd pinepods-firewood
cargo build --release
sudo cp target/release/pinepods_firewood /usr/local/bin/
# or: cargo install --path .
```

## ✅ Check it

```bash
pinepods_firewood --version
```

## 🗑️ Uninstall

```bash
sudo rm /usr/local/bin/pinepods_firewood   # or ~/bin/pinepods_firewood
sudo apt remove pinepods-firewood          # if installed from the .deb
rm -rf ~/.config/pinepods-firewood         # settings, optional
```

The API key is kept in your OS keychain under the server's address; remove it there if
you want it gone too.

## 🛠️ Troubleshooting

**"ALSA lib" errors (Linux):** install the ALSA package for your distribution, listed
under Build from Source.

**Can't sign in:** check the server URL includes `http://` or `https://` and opens in a
browser on the same machine. Logs are under `~/.local/share/pinepods-firewood/logs`
(Linux) or `~/Library/Application Support/pinepods-firewood/logs` (macOS).
