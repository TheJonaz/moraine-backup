class Moraine < Formula
  desc "Snapshot-based backup over SSH/rsync and rclone (CLI)"
  homepage "https://github.com/TheJonaz/moraine-backup"
  url "https://github.com/TheJonaz/moraine-backup/archive/refs/tags/v0.1.26.tar.gz"
  sha256 "c4a43903d69271a4d826c9e1ebb7af8fb00d5262975a6a8524e75260e8ff6e05"
  license "MIT"
  head "https://github.com/TheJonaz/moraine-backup.git", branch: "main"

  depends_on "rust" => :build
  depends_on "rsync" # a modern rsync 3.x (macOS ships an ancient 2.6.9)
  uses_from_macos "openssh"

  # CLI only: --no-default-features drops the `gui` feature, so moraine-gui
  # (which needs GTK 4) is not built. The GTK desktop app is Linux-only.
  def install
    system "cargo", "install", "--no-default-features", *std_cargo_args
    man1.install "debian/moraine.1"
  end

  def caveats
    <<~EOS
      This is the command-line client only (the GTK desktop app is Linux-only).
      The rclone backend (cloud/SFTP/FTP/SMB/WebDAV/S3) needs rclone:
        brew install rclone
    EOS
  end

  test do
    assert_match "moraine #{version}", shell_output("#{bin}/moraine --version")
  end
end
