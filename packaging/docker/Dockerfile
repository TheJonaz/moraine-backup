# Moraine CLI as a container image — for scheduled/containerised backups.
#
# Mount the paths you want backed up as volumes and point moraine at them; it
# snapshots them over SSH/rsync or to cloud/FTP/SMB/WebDAV/S3/Drive via rclone.
# rsync, the OpenSSH client and rclone are bundled, so every backend works.
#
#   docker run --rm -v /data:/data:ro -v moraine-cfg:/home/moraine/.config/moraine \
#     ghcr.io/thejonaz/moraine backup ...
#
# CLI only: --no-default-features drops the GTK app and the keyring (a container
# has no desktop session — scheduled runs read secrets from config or env),
# exactly like the snap and the Debian build.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --no-default-features --bin moraine \
 && strip target/release/moraine

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        rsync openssh-client rclone ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/moraine /usr/local/bin/moraine

# Backups rarely need root, and host paths are normally mounted read-only. uid
# 1000 matches the typical host user so mounted volumes keep sane ownership.
RUN useradd -m -u 1000 -d /home/moraine moraine
USER moraine
WORKDIR /home/moraine

ENTRYPOINT ["moraine"]
CMD ["--help"]
