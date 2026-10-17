# 🎵 PinePods Firewood

A TUI (terminal user interface) podcast client for PinePods. Stream your podcasts from the terminal, with listening progress kept in sync with the server.

![Pinepods Logo](assets/pinepods-logo.jpeg)

## ✨ Features

### 🖥️ **Terminal User Interface**
- **Beautiful TUI**: Full-featured terminal interface with intuitive navigation
- **Podcast Browser**: Browse your subscribed podcasts with two-panel view
- **Episode Management**: View recent, saved, queued, and downloaded episodes
- **Audio Player**: Built-in player with play/pause, skip (±15s), volume control
- **Micro-Player**: Always-visible playback controls at bottom of every screen
- **Search**: Find episodes and podcasts quickly
- **Session Persistence**: Remembers your login between sessions (the API key is kept in the OS keychain)

### 🎧 **Audio Playback**
- **Streaming Support**: Direct episode streaming from PinePods server
- **Playback Controls**: Play/pause, skip forward/backward, seek, volume
- **Progress Tracking**: Resumes where you left off and saves your position to the server while you listen
- **Format Support**: Supports all audio formats via Symphonia decoder

## 🚀 Installation

### 🎯 Quick Install (Recommended)

**One-liner installer** (Linux/macOS/Windows):
```bash
curl -sSL https://raw.githubusercontent.com/madeofpendletonwool/pinepods-firewood/main/install.sh | bash
```

### 📦 Package Managers

```bash
# Homebrew (macOS/Linux)
brew install pinepods-firewood

# Cargo (Rust)
cargo install pinepods-firewood

# Arch Linux (AUR)
yay -S pinepods-firewood

# Ubuntu/Debian
wget https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest/download/pinepods-firewood_VERSION_amd64.deb
sudo dpkg -i pinepods-firewood_*.deb
```

### 📥 Manual Downloads

Download pre-built binaries from [GitHub Releases](https://github.com/madeofpendletonwool/pinepods-firewood/releases/latest):
- **Linux**: `pinepods-firewood-linux-amd64.tar.gz`
- **macOS**: `pinepods-firewood-macos-amd64.tar.gz` (Intel) / `pinepods-firewood-macos-arm64.tar.gz` (Apple Silicon)  
- **Windows**: `pinepods-firewood-windows-amd64.zip`

### 🔨 Build from Source

**Prerequisites:**
- Rust toolchain (1.75+)
- System dependencies:
  ```bash
  # Ubuntu/Debian
  sudo apt install pkg-config libasound2-dev libssl-dev
  
  # Fedora
  sudo dnf install pkg-config alsa-lib-devel openssl-devel
  
  # Arch Linux
  sudo pacman -S pkg-config alsa-lib openssl
  
  # macOS (with Homebrew)
  brew install pkg-config
  ```

**Build:**
```bash
git clone https://github.com/madeofpendletonwool/pinepods-firewood.git
cd pinepods-firewood
cargo build --release
sudo cp target/release/pinepods_firewood /usr/local/bin/
```

📖 **[Complete Installation Guide](INSTALLATION.md)** - All methods, troubleshooting, and platform-specific instructions.

## 📱 Usage

### First Launch
1. **Server Setup**: Enter your PinePods server URL
2. **Authentication**: Login with your PinePods credentials
3. **Multi-Factor**: Complete MFA if enabled
4. **Timezone**: Select your timezone for episode timestamps

### Navigation
- **Tab**: Switch between tabs (Home, Podcasts, Episodes, Player, etc.)
- **1-9**: Quick tab switching
- **Arrow Keys/hjkl**: Navigate lists
- **Enter**: Select item
- **Space**: Global play/pause
- **Ctrl+R**: Refresh current page
- **q**: Quit (Ctrl+C anywhere, including the login screen and while typing)

### Tabs Overview

#### 🏠 **Home** *(Implemented)*
- Recent episodes from subscriptions
- Continue listening recommendations
- Quick access to saved/downloaded content

#### 🎙️ **Podcasts** *(Implemented)*
- **Left Panel**: Subscribed podcasts list
- **Right Panel**: Episodes for selected podcast
- **Navigation**: Tab/arrows to switch panels
- **Playback**: Enter to play selected episode

#### 📻 **Episodes** *(Implemented)*
- Recent episodes from all subscriptions
- **Filters**: All, In Progress, Completed
- **Auto-load**: Episodes load automatically
- **Search**: Real-time episode filtering

#### 🎵 **Player** *(Implemented)*
- Full-screen player interface
- Playback controls (play/pause/skip)
- Progress bar with time display
- Volume control
- Episode artwork and metadata

#### 📝 **Queue** *(Planned)*
- Manage episode queue
- Drag-and-drop reordering
- Bulk operations

#### ⭐ **Saved** *(Planned)*
- Bookmarked episodes
- Personal favorites
- Offline access preparation

#### 📥 **Downloads** *(Planned)*
- Downloaded episode management
- Offline playback
- Storage usage monitoring

#### 🔍 **Search** *(Planned)*
- Global episode/podcast search
- Advanced filters
- Podcast discovery

#### ⚙️ **Settings** *(Planned)*
- Audio output configuration
- Theme customization
- Keyboard shortcuts

## 🔧 Development

### Project Structure
```
src/
├── api/           # Typed PinePods API client
├── audio/         # Audio output thread
├── auth/          # Login flow and saved session
├── playback/      # Playback core and progress sync
└── tui/           # Terminal user interface
    └── pages/     # Individual TUI screens
```

### Key Technologies
- **TUI**: [Ratatui](https://github.com/ratatui-org/ratatui) for terminal interface
- **Audio**: [Rodio](https://github.com/RustAudio/rodio) for playback
- **HTTP**: [reqwest](https://github.com/seanmonstar/reqwest) with rustls
- **Async**: [Tokio](https://tokio.rs/) runtime

### Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes
4. Test thoroughly (especially TUI interactions)
5. Submit a pull request

### Testing

```bash
# Run tests
cargo test

# Debug logging (any tracing EnvFilter directive works)
FIREWOOD_LOG=debug cargo run
```

Logs are written to a daily file under the platform's local data directory
(`~/Library/Application Support/pinepods-firewood/logs` on macOS,
`~/.local/share/pinepods-firewood/logs` on Linux), and kept for a week.

## 🐛 Troubleshooting

### Common Issues

**"ALSA lib" errors:**
```bash
# Install ALSA development libraries
sudo apt install libasound2-dev  # Ubuntu/Debian
sudo dnf install alsa-lib-devel   # Fedora
```

**Authentication failures:**
- Verify PinePods server URL (include http/https)
- Check server connectivity
- Ensure correct username/password
- Complete MFA if enabled

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [PinePods](https://github.com/madeofpendletonwool/PinePods) - The podcast management platform
- [Ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [Rodio](https://github.com/RustAudio/rodio) - Cross-platform audio playback

## 🗺️ Roadmap

### Near Term (v0.2.0)
- [ ] Complete Queue management
- [ ] Saved episodes functionality  
- [ ] Download management
- [ ] Search implementation
- [ ] Settings page with audio config

### Medium Term (v0.3.0)
- [ ] Offline playback support
- [ ] Playlist management
- [ ] Keyboard shortcut customization
- [ ] Theme system
- [ ] Performance optimizations

### Long Term (v1.0.0)
- [ ] Web UI integration (PinePods side)
- [ ] Multi-room synchronization
- [ ] Plugin system
- [ ] Advanced discovery options
- [ ] Mobile companion app

---

**Made with ❤️ for the PinePods ecosystem**