# 🎵 PinePods Firewood

A TUI (terminal user interface) podcast client for PinePods. Stream your podcasts from the terminal, with listening progress kept in sync with the server.

![Pinepods Logo](assets/pinepods-logo.jpeg)

## ✨ Features

### 🖥️ **Terminal User Interface**
- **Sidebar layout**: every page one key away, with a now-playing bar and the keys for the page you're on always on screen
- **Pages**: Home, Feed, Podcasts, Queue, Saved (with your collections), Playlists, History, Downloads, Search, Devices, Now Playing and Settings, plus episode details with show notes, chapters and transcripts
- **Find and follow podcasts**: search Podcast Index, iTunes and YouTube and subscribe from the terminal
- **Filter as you type**: `/` narrows any list; Search queries your server as you type
- **Cover art**: on Now Playing, an episode's details and the now-playing bar, as real images in terminals with the kitty, sixel or iTerm2 graphics protocols and in blocks of color elsewhere (turn it off in Settings)
- **Command palette**: `:` or Ctrl+P finds any command, page, podcast or loaded episode by a few letters of its name
- **Themes**: the same 25 themes as the PinePods web app, starting with the one you picked there, or your terminal's own colors
- **Keys you can change**: vim-style and arrow keys by default, `?` lists them, `keymap.toml` rebinds them
- **Mouse**: click to select, click again to play or open, scroll lists, click the progress bar to seek
- **Never waits**: everything loads in the background, with a spinner while it does and a short message when something fails
- **Session Persistence**: Remembers your login between sessions (the API key is kept in the OS keychain)

### 🎧 **Audio Playback**
- **Streaming**: Playback starts within a second or two, and seeking anywhere fetches just that part of the episode. Episodes your server has downloaded (and YouTube videos) play from the server's copy; everything else from the feed
- **Playback Controls**: Play/pause, skip forward/backward, seek, volume
- **Speed**: 0.5× to 3× without changing pitch, starting from each podcast's speed setting in PinePods
- **Skips**: Jumps over ads and silence PinePods has detected (silence follows the podcast's trim-silence setting), and over a podcast's intro and outro skip times
- **Queue**: When an episode ends it's marked played and the next one in your queue starts. Started from a playlist, the playlist's next episode plays instead, until it runs out
- **Chapters and transcripts**: Now Playing lists an episode's chapters and follows along in its transcript; Enter jumps to a chapter or line
- **Progress Tracking**: Resumes where you left off and saves your position to the server while you listen
- **Reconnects**: Rides out short network drops and picks up where you were
- **Media keys**: the keyboard's play/pause and next/previous keys work, and what's playing shows in Control Center on macOS and in the desktop's media controls on Linux (MPRIS, so `playerctl` works too). Next and previous skip by the skip length
- **Formats**: MP3, AAC/M4A, FLAC, Ogg Vorbis and WAV

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
2. **Authentication**: Sign in with your PinePods username and password. If you sign in through OIDC, press **F2** and paste an API key from the web app (Settings → API Keys) instead
3. **Multi-Factor**: Complete MFA if enabled
4. **Timezone**: Pick your time zone (type to search) and how dates and times look; a new account asks once

Every key you type goes into the fields, so **Ctrl+C** quits the login screen. **F1** shows the password as you type it.

### Getting around
- **1–9, 0** jump to a sidebar entry (in the order shown; change it in Settings). **Tab** / **Shift+Tab** step through them
- **j/k** or **↑/↓** move, **g/G** first/last, **Ctrl+D/U** page down/up
- **Enter** plays an episode or opens a podcast. **i** (or **l**) opens an episode's details and show notes. **Esc** goes back
- **/** filters the list you're on; **Enter** keeps the filter, **Esc** clears it
- **:** or **Ctrl+P** opens the command palette: type a few letters of a command, a page ("go queue"), one of your podcasts, an episode on screen somewhere ("play …") or a recent search, then **Enter**
- **?** shows every key for the page you're on. **q** quits (**Ctrl+C** works anywhere, even mid-typing)

### Playing
- **Space** play/pause, **←/→** skip back/forward (15 s by default; change it in Settings)
- **[ / ]** slower/faster (0.5× to 3×, pitch stays the same), **+/-** volume
- **n** opens Now Playing, which adds **=** for normal speed, **t** to toggle trimming silence for the podcast, **x** to stop, and **.** / **,** for the next and previous chapter
- **v** on Now Playing or an episode's details switches between its show notes, chapters and transcript, where there are any. **Enter** on a chapter or a line of the transcript jumps there

### On any episode
- **a** add to or remove from the queue, **A** play next
- **s** save/unsave, **d** have the server download it (or delete its copy), **m** mark played/unplayed
- **o** play it on another of your devices
- **c** add it to (or take it out of) one of your collections

### Pages
- **Home**: where you left off, new episodes and your most-played podcasts
- **Feed**: recent episodes from every subscription. **f** cycles All / Unplayed / In progress / Played
- **Podcasts**: your subscriptions; **Enter** lists a podcast's episodes. **U** twice unsubscribes (from the list or the podcast's page)
- **Queue**: **K/J** move an episode up/down, **t/b** to the top/bottom, **x** remove, **S** shuffle, **C** twice to clear
- **Saved**: episodes you've saved. If you've made other collections in the web app, this lists them, with Saved first
- **Playlists**: the web app's playlists, smart ones included. Playing an episode from one carries on through the playlist
- **History**: what you've played, newest first
- **Downloads**: the episodes your server has a copy of, by podcast
- **Search**: episodes from the podcasts you follow, as you type. **f** switches to finding new podcasts on Podcast Index, iTunes or YouTube: type a name, press **Enter**, then **s** subscribes (or **Enter** shows more about one first)
- **Devices**: your other players (see [Remote Control](#-remote-control))
- **Settings**: theme, cover art, media keys, starting volume, skip length, audio output, the name other devices see, sidebar order, and the key bindings and log files

### Themes
Firewood starts with the theme you picked in the PinePods web app, so a change there
shows up the next time it opens. That includes themes you made yourself there. Picking a theme in Settings stops that. The **Terminal** theme
uses your terminal's own colors. Firewood uses 24-bit color where the terminal says it
supports it and the nearest 256-color match elsewhere; `FIREWOOD_COLOR=truecolor` or
`FIREWOOD_COLOR=256` overrides the guess.

### Key bindings
Settings → Key bindings writes `~/.config/pinepods-firewood/keymap.toml` with every
binding commented out. Uncomment a line and change its keys, then restart:

```toml
[global]
quit = ["q", "ctrl+q"]

[queue]
move_up = ["U", "shift+up"]
```

Keys look like `"q"`, `"G"`, `"space"`, `"enter"`, `"ctrl+r"`, `"shift+up"` or `"f5"`.
A mistake in the file shows as a message at startup and leaves the defaults in place.

### Mouse
Clicking selects, clicking the selected row again plays or opens it, and the wheel
scrolls. With the mouse on, selecting text in most terminals needs Shift (Option in
some macOS terminals) held down; `FIREWOOD_NO_MOUSE=1` turns it off.

### Cover art
Firewood asks the terminal at startup which image protocol it speaks: kitty (kitty,
Ghostty), sixel (foot, Konsole, xterm started with `-ti vt340`) or iTerm2 (iTerm2,
WezTerm). Anywhere else covers are drawn in half-block characters. Inside tmux,
images need `set -g allow-passthrough on`; if detection still guesses wrong, set
`FIREWOOD_GRAPHICS` to `kitty`, `sixel`, `iterm2` or `blocks`. Covers come through
your server's image proxy and are cached under the platform's cache directory
(`~/Library/Caches/pinepods-firewood/art` on macOS, `~/.cache/pinepods-firewood/art`
on Linux), trimmed to 64 MB.

## 🔧 Development

### Project Structure
```
src/
├── api/           # Typed PinePods API client
├── art/           # Cover art: fetching, caching, decoding
├── audio/         # Audio output thread
├── auth/          # Login flow and saved session
├── mediakeys/     # Media keys: MPRIS on Linux, Now Playing on macOS
├── nowplaying/    # Remote control over the server's now-playing socket
├── playback/      # Playback core and progress sync
├── theme/         # The web app's themes, adapted for a terminal
└── tui/           # Terminal user interface
    ├── app.rs     # Sidebar, now-playing bar, overlays, input routing
    ├── keymap.rs  # Default bindings and keymap.toml
    ├── palette.rs # The command palette
    ├── task.rs    # Background loading
    ├── ui/        # Shared widgets: lists, episode rows, show notes
    └── pages/     # The screens
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

## 📡 Remote Control

Firewood holds a socket to your PinePods server (`/ws/api/nowplaying`) so your other
devices can see what it's playing and control it, and it can control them. Nothing
listens on your network; the server relays everything. The web and mobile apps can
beam episodes to Firewood like to any other device, and it accepts their commands.

- Other devices see it as `Firewood · <hostname>`. Change that in Settings.
- `FIREWOOD_REMOTE_DISABLED=1` turns the socket off.
- If the Devices page says the server has no now-playing socket, the server is older
  than the now-playing release, or a reverse proxy in front of it doesn't pass
  WebSocket upgrades on `/ws/api/` through.

## ⏯️ Media Keys

Firewood shows what's playing to the system and takes its play, pause, skip and seek
commands: through MPRIS on Linux (the desktop's media widget, `playerctl`, most
desktops' media keys) and through Now Playing on macOS (Control Center, the lock
screen, the keyboard's media keys). It works the same whatever the terminal.

- Next and previous skip forward and back by the skip length.
- Turn it off in Settings, or for one run with `FIREWOOD_MEDIA_KEYS=0`.
- On Linux it needs a D-Bus session; without one Firewood logs a warning and carries on.

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

This project is licensed under the GNU General Public License v3.0; see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [PinePods](https://github.com/madeofpendletonwool/PinePods) - The podcast management platform
- [Ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [Rodio](https://github.com/RustAudio/rodio) - Cross-platform audio playback

## 🗺️ Roadmap

- [x] History, playlists and collections
- [x] Chapters and transcripts
- [x] Podcast discovery and subscribing
- [x] A command palette
- [x] Cover art in terminals that can show images
- [x] Media keys and the system's now-playing controls (MPRIS on Linux, Now Playing on macOS)
- [ ] Headless mode, for a Raspberry Pi or speaker box that other devices play on

---

**Made with ❤️ for the PinePods ecosystem**