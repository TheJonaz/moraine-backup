#!/usr/bin/env bash
# Set up a signed APT repository for Moraine on cdn.thern.io (run ON the VPS).
#
# Prerequisites on the server:
#   - Debian/Ubuntu with sudo, nginx, and a DNS A record for cdn.thern.io.
#   - This script + ../debian-built moraine_*.deb copied to the server.
#
# After this runs, users add the repo with:
#   curl -fsSL https://cdn.thern.io/moraine.gpg | sudo tee /usr/share/keyrings/moraine.gpg >/dev/null
#   echo "deb [signed-by=/usr/share/keyrings/moraine.gpg] https://cdn.thern.io/apt stable main" \
#       | sudo tee /etc/apt/sources.list.d/moraine.list
#   sudo apt update && sudo apt install moraine
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-/srv/apt}"          # reprepro lives here; nginx serves $REPO_ROOT
WEB_ROOT="${WEB_ROOT:-/var/www/cdn.thern.io}"
GPG_NAME="${GPG_NAME:-Moraine APT repository}"
GPG_EMAIL="${GPG_EMAIL:-info@thern.io}"

echo "==> Installing reprepro + nginx"
sudo apt-get update
sudo apt-get install -y reprepro nginx gnupg

echo "==> Creating a repo signing key (if absent)"
if ! gpg --list-secret-keys "$GPG_EMAIL" >/dev/null 2>&1; then
  cat > /tmp/moraine-key.batch <<KEY
%no-protection
Key-Type: eddsa
Key-Curve: ed25519
Key-Usage: sign
Name-Real: $GPG_NAME
Name-Email: $GPG_EMAIL
Expire-Date: 0
%commit
KEY
  gpg --batch --generate-key /tmp/moraine-key.batch
  rm -f /tmp/moraine-key.batch
fi

echo "==> Initialising reprepro at $REPO_ROOT"
sudo mkdir -p "$REPO_ROOT/conf"
sudo cp "$(dirname "$0")/apt-distributions" "$REPO_ROOT/conf/distributions"
sudo chown -R "$USER" "$REPO_ROOT"

echo "==> Importing any moraine_*.deb in the current directory"
for deb in moraine_*.deb; do
  [ -e "$deb" ] || { echo "  (no .deb here — copy one next to this script)"; break; }
  reprepro -b "$REPO_ROOT" includedeb stable "$deb"
done

echo "==> Exporting the public key to the web root"
sudo mkdir -p "$WEB_ROOT/apt"
sudo ln -sfn "$REPO_ROOT/dists" "$WEB_ROOT/apt/dists"
sudo ln -sfn "$REPO_ROOT/pool" "$WEB_ROOT/apt/pool"
gpg --armor --export "$GPG_EMAIL" | sudo tee "$WEB_ROOT/moraine.gpg" >/dev/null

cat <<NGINX

==> Add an nginx server block (then: sudo nginx -t && sudo systemctl reload nginx):

server {
    listen 443 ssl http2;
    server_name cdn.thern.io;
    root $WEB_ROOT;
    autoindex on;
    # ssl_certificate ... (use certbot)
}

==> Done. To publish a new version later:
    reprepro -b $REPO_ROOT includedeb stable moraine_X.Y.Z-1_amd64.deb
NGINX
