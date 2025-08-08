#!/usr/bin/env python3
"""
🎵 PinePods Firewood Remote Control Test Script

This script provides comprehensive testing and manual control capabilities
for Firewood remote players on your local network.

SETUP:
1. Create virtual environment: python3 -m venv venv
2. Activate it: source venv/bin/activate  
3. Install dependencies: pip install -r requirements.txt
4. Run script: python test_remote_control.py --help

FEATURES:
- Auto-discovery via mDNS scanning
- Interactive playback control  
- Real-time status monitoring
- Episode beaming capabilities
- Multi-player support

USAGE EXAMPLES:
- Discovery: python test_remote_control.py --discover
- Interactive: python test_remote_control.py -u http://IP:PORT -i
- Status only: python test_remote_control.py -u http://IP:PORT
"""

import argparse
import json
import requests
import time
from zeroconf import ServiceBrowser, ServiceListener, Zeroconf
import threading


class FirewoodPlayerListener(ServiceListener):
    def __init__(self):
        self.players = {}
        self.lock = threading.Lock()
    
    def add_service(self, zeroconf, type, name):
        info = zeroconf.get_service_info(type, name)
        if info:
            with self.lock:
                host = info.parsed_addresses()[0]
                address = f"http://{host}:{info.port}"
                self.players[name] = {
                    'address': address,
                    'host': host,
                    'port': info.port,
                    'properties': {k.decode(): v.decode() for k, v in info.properties.items()}
                }
                print(f"🎵 Discovered Firewood player: {name}")
                print(f"   Address: {address}")
                if info.properties:
                    print(f"   Properties: {self.players[name]['properties']}")
    
    def remove_service(self, zeroconf, type, name):
        with self.lock:
            if name in self.players:
                print(f"❌ Lost Firewood player: {name}")
                del self.players[name]
    
    def update_service(self, zeroconf, type, name):
        pass  # We don't need to handle updates for this test
    
    def get_players(self):
        with self.lock:
            return dict(self.players)


class FirewoodRemoteControl:
    def __init__(self, base_url):
        self.base_url = base_url.rstrip('/')
        self.session = requests.Session()
        self.session.headers.update({'Content-Type': 'application/json'})
    
    def get_player_info(self):
        """Get basic player information."""
        try:
            response = self.session.get(f"{self.base_url}/")
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to get player info: {e}")
            return None
    
    def get_status(self):
        """Get current playback status."""
        try:
            response = self.session.get(f"{self.base_url}/status")
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to get status: {e}")
            return None
    
    def play_episode(self, episode_data):
        """Play an episode."""
        try:
            response = self.session.post(f"{self.base_url}/play", json=episode_data)
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to play episode: {e}")
            return None
    
    def pause(self):
        """Pause playback."""
        try:
            response = self.session.post(f"{self.base_url}/pause")
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to pause: {e}")
            return None
    
    def resume(self):
        """Resume playback."""
        try:
            response = self.session.post(f"{self.base_url}/resume")
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to resume: {e}")
            return None
    
    def stop(self):
        """Stop playback."""
        try:
            response = self.session.post(f"{self.base_url}/stop")
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to stop: {e}")
            return None
    
    def skip(self, seconds):
        """Skip forward or backward."""
        try:
            response = self.session.post(f"{self.base_url}/skip", json={"seconds": seconds})
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to skip: {e}")
            return None
    
    def seek(self, position):
        """Seek to specific position."""
        try:
            response = self.session.post(f"{self.base_url}/seek", json={"position": position})
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to seek: {e}")
            return None
    
    def set_volume(self, volume):
        """Set volume (0.0 to 1.0)."""
        try:
            response = self.session.post(f"{self.base_url}/volume", json={"volume": volume})
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"❌ Failed to set volume: {e}")
            return None


def discover_players(timeout=5):
    """Discover Firewood players on the network."""
    print(f"🔍 Discovering Firewood players for {timeout} seconds...")
    
    zeroconf = Zeroconf()
    listener = FirewoodPlayerListener()
    browser = ServiceBrowser(zeroconf, "_pinepods-remote._tcp.local.", listener)
    
    time.sleep(timeout)
    
    browser.cancel()
    zeroconf.close()
    
    return listener.get_players()


def format_duration(seconds):
    """Format duration in seconds to HH:MM:SS or MM:SS."""
    hours = seconds // 3600
    minutes = (seconds % 3600) // 60
    secs = seconds % 60
    
    if hours > 0:
        return f"{hours:02d}:{minutes:02d}:{secs:02d}"
    else:
        return f"{minutes:02d}:{secs:02d}"


def print_status(controller):
    """Print current playback status."""
    status_data = controller.get_status()
    if not status_data or not status_data.get('success'):
        print("❌ Failed to get status")
        return
    
    status = status_data.get('data', {})
    is_playing = status.get('is_playing', False)
    position = status.get('position', 0)
    duration = status.get('duration', 0)
    volume = status.get('volume', 0.0)
    current_episode = status.get('current_episode')
    
    print(f"\n🎵 Playback Status:")
    print(f"   State: {'Playing' if is_playing else 'Paused/Stopped'}")
    print(f"   Position: {format_duration(position)} / {format_duration(duration)}")
    print(f"   Volume: {int(volume * 100)}%")
    
    if current_episode:
        print(f"   Episode: {current_episode.get('episode_title', 'Unknown')}")
        print(f"   Podcast: {current_episode.get('podcast_name', 'Unknown')}")


def interactive_control(controller):
    """Interactive control mode."""
    print("\n🎮 Interactive Control Mode")
    print("Commands:")
    print("  s/status     - Show playback status")
    print("  p/pause      - Pause/Resume toggle")
    print("  stop         - Stop playback")
    print("  +15/-15      - Skip forward/backward 15 seconds")
    print("  +5/+30       - Skip forward 5/30 seconds")
    print("  -5/-30       - Skip backward 5/30 seconds") 
    print("  vol [0-100]  - Set volume (e.g., 'vol 75')")
    print("  play         - Play test episode")
    print("  play-url     - Play episode from URL")
    print("  beam [URL]   - Beam audio file URL directly to player")
    print("  info         - Show player information")
    print("  monitor      - Live status monitoring")
    print("  q/quit       - Quit")
    print()
    
    while True:
        try:
            command = input("🎵 > ").strip().lower()
            
            if command == 'q' or command == 'quit':
                break
            elif command == 's' or command == 'status':
                print_status(controller)
            elif command == 'p' or command == 'pause':
                # Get current status to decide pause or resume
                status_data = controller.get_status()
                if status_data and status_data.get('success'):
                    is_playing = status_data.get('data', {}).get('is_playing', False)
                    if is_playing:
                        result = controller.pause()
                        print("⏸️  Paused" if result and result.get('success') else "❌ Failed to pause")
                    else:
                        result = controller.resume()
                        print("▶️  Resumed" if result and result.get('success') else "❌ Failed to resume")
                else:
                    print("❌ Failed to get current status")
            elif command == 'stop':
                result = controller.stop()
                print("⏹️  Stopped" if result and result.get('success') else "❌ Failed to stop")
            elif command in ['+5', '+15', '+30']:
                seconds = int(command[1:])
                result = controller.skip(seconds)
                print(f"⏭️  Skipped forward {seconds}s" if result and result.get('success') else "❌ Failed to skip")
            elif command in ['-5', '-15', '-30']:
                seconds = int(command[1:])
                result = controller.skip(seconds)  # Already negative
                print(f"⏮️  Skipped backward {abs(seconds)}s" if result and result.get('success') else "❌ Failed to skip")
            elif command.startswith('vol '):
                try:
                    vol_percent = int(command[4:])
                    vol_decimal = max(0, min(100, vol_percent)) / 100.0
                    result = controller.set_volume(vol_decimal)
                    print(f"🔊 Volume set to {int(vol_decimal * 100)}%" if result and result.get('success') else "❌ Failed to set volume")
                except ValueError:
                    print("❌ Invalid volume. Use 'vol [0-100]'")
            elif command == 'play':
                # Play a test episode
                test_episode = {
                    "episode_url": "https://www.soundjay.com/misc/beep-07a.wav",  # Short test audio
                    "episode_title": "Test Episode",
                    "podcast_name": "Test Podcast",
                    "episode_duration": 60,
                    "episode_artwork": None
                }
                result = controller.play_episode(test_episode)
                print("🎵 Playing test episode" if result and result.get('success') else "❌ Failed to play test episode")
            elif command == 'play-url':
                print("🎵 Enter episode details:")
                episode_url = input("Episode URL: ").strip()
                if not episode_url:
                    print("❌ URL is required")
                    continue
                episode_title = input("Episode Title (optional): ").strip() or "Custom Episode"
                podcast_name = input("Podcast Name (optional): ").strip() or "Custom Podcast"
                
                episode = {
                    "episode_url": episode_url,
                    "episode_title": episode_title,
                    "podcast_name": podcast_name,
                    "episode_duration": 3600,  # Default 1 hour
                    "episode_artwork": None
                }
                result = controller.play_episode(episode)
                print(f"🎵 Playing '{episode_title}'" if result and result.get('success') else "❌ Failed to play episode")
            elif command.startswith('beam '):
                # Extract URL from command
                url_part = command[5:].strip()
                if not url_part:
                    print("❌ Usage: beam [URL]")
                    continue
                
                # Beam the URL directly
                episode = {
                    "episode_url": url_part,
                    "episode_title": "Beamed Audio",
                    "podcast_name": "Direct URL",
                    "episode_duration": 3600,  # Default 1 hour
                    "episode_artwork": None
                }
                result = controller.play_episode(episode)
                print(f"🎵 Beaming audio from: {url_part}" if result and result.get('success') else "❌ Failed to beam audio")
            elif command == 'info':
                info = controller.get_player_info()
                if info and info.get('success'):
                    data = info.get('data', {})
                    print(f"\n🎵 Player Information:")
                    print(f"   Name: {data.get('name', 'Unknown')}")
                    print(f"   Version: {data.get('version', 'Unknown')}")
                    print(f"   Server: {data.get('server_url', 'None')}")
                else:
                    print("❌ Failed to get player info")
            elif command == 'monitor':
                print("🔄 Live status monitoring (Ctrl+C to stop)...")
                try:
                    while True:
                        print_status(controller)
                        time.sleep(2)
                except KeyboardInterrupt:
                    print("\n⏹️  Monitoring stopped")
            else:
                print("❌ Unknown command. Type 'q' to quit.")
        except KeyboardInterrupt:
            print("\n👋 Goodbye!")
            break


def main():
    parser = argparse.ArgumentParser(
        description="🎵 Test PinePods Firewood remote control",
        epilog="Examples:\n"
               "  python test_remote_control.py -d                    # Discover players\n" 
               "  python test_remote_control.py -u http://IP:PORT     # Connect to player\n"
               "  python test_remote_control.py -u http://IP:PORT -i  # Interactive mode\n"
               "  python test_remote_control.py -d -t 10              # 10 second discovery",
        formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument('--discover', '-d', action='store_true', 
                       help='Discover Firewood players on network via mDNS')
    parser.add_argument('--url', '-u', 
                       help='Direct URL to player (e.g., http://192.168.1.100:8042)')
    parser.add_argument('--timeout', '-t', type=int, default=5, 
                       help='Discovery timeout in seconds (default: 5)')
    parser.add_argument('--interactive', '-i', action='store_true', 
                       help='Start interactive control mode after connection')
    parser.add_argument('--list-all', action='store_true',
                       help='Show all discovered players without connecting')
    parser.add_argument('--json', action='store_true',
                       help='Output discovery results in JSON format')
    parser.add_argument('--beam-url', 
                       help='URL to beam directly to player (combine with -u or --discover)')
    
    args = parser.parse_args()
    
    if args.discover or not args.url:
        players = discover_players(args.timeout)
        
        if args.json:
            # JSON output for programmatic use
            output = {
                "players_found": len(players),
                "players": [
                    {
                        "name": name,
                        "address": info['address'],
                        "host": info['host'], 
                        "port": info['port'],
                        "properties": info['properties']
                    }
                    for name, info in players.items()
                ]
            }
            print(json.dumps(output, indent=2))
            return

        if not players:
            print("❌ No Firewood players found on the network")
            print("💡 Make sure a Firewood player is running and connected to the same network")
            print("💡 Check that devices are on the same network and mDNS is not blocked")
            return
        
        print(f"\n✅ Found {len(players)} player(s):")
        for i, (name, info) in enumerate(players.items(), 1):
            print(f"{i}. {name}")
            print(f"   Address: {info['address']}")
            print(f"   Host: {info['host']}:{info['port']}")
            if info['properties']:
                server = info['properties'].get('server', 'Unknown')
                version = info['properties'].get('version', 'Unknown')
                print(f"   Server: {server}")
                print(f"   Version: {version}")
        
        if args.list_all:
            return  # Just show the list, don't connect
        
        if not args.url and players:
            # Use the first discovered player
            first_player = next(iter(players.values()))
            args.url = first_player['address']
            print(f"\n🎯 Using first discovered player: {args.url}")
    
    if args.url:
        print(f"\n🔗 Connecting to player at {args.url}")
        controller = FirewoodRemoteControl(args.url)
        
        # Test basic connection
        info = controller.get_player_info()
        if info and info.get('success'):
            player_data = info.get('data', {})
            print(f"✅ Connected to {player_data.get('name', 'Unknown Player')}")
            print(f"   Version: {player_data.get('version', 'Unknown')}")
            if player_data.get('server_url'):
                print(f"   PinePods Server: {player_data['server_url']}")
            
            # Show initial status
            print_status(controller)
            
            # Handle URL beaming if specified
            if args.beam_url:
                episode = {
                    "episode_id": 999999,  # Fake ID for beamed content
                    "episode_url": args.beam_url,
                    "episode_title": "Beamed Audio",
                    "podcast_name": "Direct URL",
                    "episode_duration": None,  # Let server parse duration
                    "episode_artwork": "https://changelog.com/images/brand/changelog-icon.png",
                    "start_position": 0
                }
                result = controller.play_episode(episode)
                if result and result.get('success'):
                    print(f"🎵 Successfully beamed: {args.beam_url}")
                else:
                    print(f"❌ Failed to beam: {args.beam_url}")
            
            if args.interactive:
                interactive_control(controller)
        else:
            print("❌ Failed to connect to player")
    
    print("\n👋 Test complete!")


def show_detailed_help():
    """Show comprehensive help information."""
    help_text = """
🎵 PinePods Firewood Remote Control - Detailed Help

INSTALLATION:
  1. python3 -m venv venv
  2. source venv/bin/activate  # On Windows: venv\\Scripts\\activate
  3. pip install -r requirements.txt

DISCOVERY:
  python test_remote_control.py --discover
  python test_remote_control.py -d -t 10          # 10 second timeout  
  python test_remote_control.py -d --json         # JSON output
  python test_remote_control.py -d --list-all     # Show all, don't connect

CONTROL:
  python test_remote_control.py -u http://IP:PORT
  python test_remote_control.py -u http://IP:PORT -i  # Interactive mode

INTERACTIVE COMMANDS:
  s, status    - Show current playback status
  p, pause     - Toggle pause/resume
  stop         - Stop playback completely
  +5, +15, +30 - Skip forward N seconds
  -5, -15, -30 - Skip backward N seconds
  vol 75       - Set volume to 75%
  play         - Play test episode (short beep)
  play-url     - Play episode from custom URL
  info         - Show player information
  monitor      - Live status updates (Ctrl+C to stop)
  q, quit      - Exit interactive mode

API ENDPOINTS:
  GET  /           - Player information
  GET  /status     - Playback status
  POST /play       - Play episode (JSON body required)
  POST /pause      - Pause playback
  POST /resume     - Resume playback  
  POST /stop       - Stop playback
  POST /skip       - Skip seconds: {"seconds": 15}
  POST /seek       - Seek to position: {"position": 120}
  POST /volume     - Set volume: {"volume": 0.7}

TROUBLESHOOTING:
  • No players found: Check network connectivity and mDNS
  • Connection refused: Verify Firewood is running with remote control enabled
  • Import errors: Make sure virtual environment is activated
  • Firewall issues: Allow mDNS traffic (port 5353) and HTTP (port 8042+)

INTEGRATION:
  This script demonstrates how to integrate Firewood remote control
  into web applications or other automation systems.
"""
    print(help_text)


if __name__ == "__main__":
    try:
        # Try to import required dependencies
        import zeroconf
    except ImportError:
        print("❌ Missing required dependency 'zeroconf'")
        print("💡 Install with: pip install -r requirements.txt")
        print("\n📋 Setup Instructions:")
        print("1. python3 -m venv venv")
        print("2. source venv/bin/activate")
        print("3. pip install -r requirements.txt")
        print("4. python test_remote_control.py --help")
        exit(1)
    
    import sys
    if len(sys.argv) > 1 and sys.argv[1] in ['--detailed-help', '--help-detailed']:
        show_detailed_help()
        exit(0)
    
    main()