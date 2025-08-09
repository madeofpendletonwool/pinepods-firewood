class PinepodsFirewood < Formula
  desc "Terminal UI client for PinePods podcast server"
  homepage "https://github.com/madeofpendletonwool/pinepods-firewood"
  version "__VERSION__"
  
  if OS.mac?
    if Hardware::CPU.intel?
      url "https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v__VERSION__/pinepods-firewood-macos-amd64.tar.gz"
      sha256 "__MACOS_AMD64_SHA256__"
    else
      url "https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v__VERSION__/pinepods-firewood-macos-arm64.tar.gz"
      sha256 "__MACOS_ARM64_SHA256__"
    end
  elsif OS.linux?
    if Hardware::CPU.intel?
      url "https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v__VERSION__/pinepods-firewood-linux-amd64.tar.gz"
      sha256 "__LINUX_AMD64_SHA256__"
    else
      url "https://github.com/madeofpendletonwool/pinepods-firewood/releases/download/v__VERSION__/pinepods-firewood-linux-arm64.tar.gz"
      sha256 "__LINUX_ARM64_SHA256__"
    end
  end

  def install
    bin.install "pinepods_firewood"
  end

  test do
    system "#{bin}/pinepods_firewood", "--version"
  end
end