#!/usr/bin/env bash
# Build the Moraine snap in a container, without installing snapd on the host.
#
# Why this exists: snapcraft normally builds inside an LXD/multipass VM managed
# by snapd, and some distributions (Linux Mint, for one) deliberately block
# snapd via APT preferences. This image instead unsquashes the snapcraft, core24
# and snapd snaps into the absolute /snap/<name>/current paths their binaries
# are linked against, and runs `snapcraft --destructive-mode` directly.
#
#   ./packaging/snap/build-in-docker.sh            # build the pinned release tag
#   ./packaging/snap/build-in-docker.sh --local    # build the working tree instead
#
# The finished .snap is copied to packaging/snap/*.snap.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
image=moraine-snapcraft

local_source=false
[[ "${1:-}" == "--local" ]] && local_source=true

command -v docker >/dev/null || { echo "docker is required" >&2; exit 1; }

# The build tree MUST live outside the repository. snapcraft copies the whole
# of `source:` into parts/<part>/src, so a build directory inside the repo would
# copy the repo into itself, recursively, until the disk fills.
work="$(mktemp -d "${TMPDIR:-/tmp}/moraine-snap.XXXXXX")"
cleanup() {
	# Root-owned once snapcraft has run, so remove it as root rather than as us.
	docker run --rm -v "$work:/w" ubuntu:24.04 sh -c 'rm -rf /w/* /w/.[!.]*' 2>/dev/null || true
	rmdir "$work" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$work/build/snap" "$work/src"

if $local_source; then
	echo "==> staging the working tree (tracked files only)"
	# Tracked files only: `target/` alone is several GB, and snapcraft would
	# copy every byte of it for nothing. Uncommitted edits to tracked files are
	# included, which is the point of --local.
	git -C "$repo" ls-files -z \
		| rsync -a --files-from=- --from0 "$repo/" "$work/src/"
	sed -e 's|^    source: https://github.com/TheJonaz/moraine-backup.git|    source: /project|' \
	    -e '/^    source-tag:/d' "$here/snapcraft.yaml" > "$work/build/snap/snapcraft.yaml"
	mount_src="$work/src"
else
	echo "==> building the tag pinned in snapcraft.yaml"
	cp "$here/snapcraft.yaml" "$work/build/snap/snapcraft.yaml"
	mount_src="$repo"   # unused by the recipe, but keeps the run invocation uniform
fi

echo "==> building the $image image (cached after the first run)"
docker build -q -t "$image" -f "$here/Dockerfile.build" "$here" >/dev/null

docker run --rm \
	-v "$mount_src:/project:ro" \
	-v "$work/build:/build" \
	-w /build "$image" \
	bash -lc 'snapcraft pack --destructive-mode --verbosity=brief'

# Copy the artefact out before the trap wipes the work tree. Everything the
# container wrote is root-owned, so hand it back to the invoking user.
docker run --rm -v "$work/build:/build" -v "$here:/out" ubuntu:24.04 \
	sh -c "cp /build/*.snap /out/ && chown $(id -u):$(id -g) /out/*.snap && chmod 644 /out/*.snap"

echo
ls -la "$here"/*.snap
