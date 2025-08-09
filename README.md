# 🎵 PinePods Firewood

A comprehensive TUI (Terminal User Interface) podcast client for PinePods that doubles as a remote-controllable player. Stream your podcasts in style from the terminal and control playback remotely from the web interface or other devices on your network.

![Pinepods Logo](assets/pinepods-logo.jpeg)

## ✨ Features

### 🖥️ **Terminal User Interface**
- **Beautiful TUI**: Full-featured terminal interface with intuitive navigation
- **Podcast Browser**: Browse your subscribed podcasts with two-panel view
- **Episode Management**: View recent, saved, queued, and downloaded episodes
- **Audio Player**: Built-in player with play/pause, skip (±15s), volume control
- **Micro-Player**: Always-visible playback controls at bottom of every screen
- **Search**: Find episodes and podcasts quickly
- **Session Persistence**: Remembers your login between sessions

### 🌐 **Remote Control System**
- **mDNS Discovery**: Automatically discoverable on local network
- **HTTP API**: RESTful endpoints for remote control
- **Web UI Integration**: Ready for "beam to Firewood" functionality
- **Smart Port Management**: Automatic port fallback if conflicts occur
- **Multi-Instance**: Run multiple Firewood players simultaneously

### 🎧 **Audio Playback**
- **Streaming Support**: Direct episode streaming from PinePods server
- **Playback Controls**: Play/pause, skip forward/backward, seek, volume
- **Progress Tracking**: Syncs listening progress with PinePods server
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
- **r**: Refresh current page
- **q/Ctrl+C**: Quit

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
- Remote control settings
- Theme customization
- Keyboard shortcuts

## 🌐 Remote Control

Firewood includes a built-in HTTP server that makes it discoverable and controllable over your local network.

### 🔧 **Configuration**

**Environment Variables:**
```bash
# Custom port (default: 8042)
export FIREWOOD_REMOTE_PORT=8080

# Disable remote control
export FIREWOOD_REMOTE_DISABLED=true

# Run with custom settings
cargo run
```

**Port Fallback System:**
If the preferred port is busy, Firewood automatically tries:
1. Preferred port (default: 8042)
2. Preferred + 1, + 2 (8043, 8044)  
3. Common ports: 8080-8083, 3000-3002, 4000-4002, 9000-9002
4. OS-assigned random port (ultimate fallback)

### 🕵️ **Discovery**

Firewood advertises itself via mDNS as:
- **Service**: `_pinepods-remote._tcp.local.`
- **Properties**: 
  - `version`: Firewood version
  - `server`: Connected PinePods server URL
- **Auto-Discovery**: Web UI can scan and connect automatically

### 📡 **HTTP API**

| Endpoint | Method | Description | Body Example |
|----------|---------|-------------|--------------|
| `/` | GET | Get player info | - |
| `/status` | GET | Get playback status | - |
| `/play` | POST | Play episode | `{"episode_url": "...", "episode_title": "...", "podcast_name": "...", "episode_duration": 3600}` |
| `/pause` | POST | Pause playback | - |
| `/resume` | POST | Resume playback | - |
| `/stop` | POST | Stop playback | - |
| `/skip` | POST | Skip seconds | `{"seconds": 15}` (negative for backward) |
| `/seek` | POST | Seek to position | `{"position": 120}` |
| `/volume` | POST | Set volume | `{"volume": 0.7}` (0.0-1.0) |

### 🧪 **Testing & Manual Control**

A Python test script is included for discovery and manual control:

```bash
# Setup (one time)
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt

# Discover Firewood players on network
python test_remote_control.py --discover

# Interactive control
python test_remote_control.py -u http://IP:PORT --interactive

# Direct connection (if you know the IP/port)
python test_remote_control.py -u http://192.168.1.100:8042 --interactive
```

**Interactive Commands:**
- `s` - Show playback status
- `p` - Pause/resume toggle
- `stop` - Stop playback
- `+15` - Skip forward 15 seconds
- `-15` - Skip backward 15 seconds  
- `vol 75` - Set volume to 75%
- `play` - Play test episode
- `q` - Quit

## 🔗 Web UI Integration *(Future)*

When implemented in the PinePods web interface:

1. **Network Scan**: Web UI discovers Firewood players via mDNS
2. **Player List**: Shows available players with their names and server info
3. **Episode Beaming**: "Play on Firewood" button on episode pages
4. **Remote Control**: Volume, skip, pause controls in web interface
5. **Multi-Room**: Control multiple Firewood players simultaneously

## 🔧 Development

### Project Structure
```
src/
├── api/           # PinePods API client
├── audio/         # Audio playback engine
├── auth/          # Authentication & session management
├── config/        # Configuration handling
├── helpers/       # Utility functions
├── remote/        # Remote control server & mDNS
└── tui/           # Terminal user interface
    └── pages/     # Individual TUI screens
```

### Key Technologies
- **TUI**: [Ratatui](https://github.com/ratatui-org/ratatui) for terminal interface
- **Audio**: [Rodio](https://github.com/RustAudio/rodio) for playback
- **HTTP**: [Axum](https://github.com/tokio-rs/axum) for remote control server
- **mDNS**: [mdns-sd](https://github.com/keepsimple1/mdns-sd) for service discovery
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

# Test with debug logging
RUST_LOG=debug cargo run

# Test remote control
python test_remote_control.py --help

# Test port fallback
FIREWOOD_REMOTE_PORT=9999 cargo run
```

## 🐛 Troubleshooting

### Common Issues

**"ALSA lib" errors:**
```bash
# Install ALSA development libraries
sudo apt install libasound2-dev  # Ubuntu/Debian
sudo dnf install alsa-lib-devel   # Fedora
```

**Port conflicts:**
```bash
# Check what's using the port
ss -tlnp | grep 8042

# Use different port
export FIREWOOD_REMOTE_PORT=8080
```

**Authentication failures:**
- Verify PinePods server URL (include http/https)
- Check server connectivity
- Ensure correct username/password
- Complete MFA if enabled

**mDNS discovery fails:**
- Ensure devices are on same network
- Check firewall settings (allow mDNS port 5353)
- Try manual connection with IP:PORT

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
- [ ] Settings page with audio/remote config

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