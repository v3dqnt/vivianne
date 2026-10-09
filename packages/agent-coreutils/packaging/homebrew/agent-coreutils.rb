# Homebrew tap formula template. Fill in the sha256 values at release time.
class AgentCoreutils < Formula
  desc "Fast, model-friendly file tools for AI agents"
  homepage "https://github.com/agent-coreutils/agent-coreutils"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/agent-coreutils/agent-coreutils/releases/download/v0.1.0/release-acu-darwin-arm64"
      sha256 "FILL-IN-AT-RELEASE"
    end
    on_intel do
      url "https://github.com/agent-coreutils/agent-coreutils/releases/download/v0.1.0/release-acu-darwin-x64"
      sha256 "FILL-IN-AT-RELEASE"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/agent-coreutils/agent-coreutils/releases/download/v0.1.0/release-acu-linux-arm64-gnu"
      sha256 "FILL-IN-AT-RELEASE"
    end
    on_intel do
      url "https://github.com/agent-coreutils/agent-coreutils/releases/download/v0.1.0/release-acu-linux-x64-gnu"
      sha256 "FILL-IN-AT-RELEASE"
    end
  end

  def install
    bin.install Dir["release-acu-*"].first => "acu"
  end

  test do
    assert_match "acu #{version}", shell_output("#{bin}/acu --version")
  end
end
