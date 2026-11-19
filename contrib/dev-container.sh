#!/usr/bin/env bash
# Create the swayward build container on an immutable Fedora host.
set -euo pipefail

IMAGE=registry.fedoraproject.org/fedora-toolbox:44
NAME=swayward-dev

if ! distrobox list | awk -F ' *\| *' -v name="$NAME" 'NR > 1 && $2 == name { found = 1 } END { exit !found }'; then
	distrobox create --name "$NAME" --image "$IMAGE"
fi

distrobox enter "$NAME" -- sudo dnf install -y \
	curl \
	gcc \
	clang \
	clang-devel \
	cargo \
	rustfmt \
	pkgconf-pkg-config \
	glib2-devel \
	pango-devel \
	cairo-devel \
	cairo-gobject-devel \
	libinput-devel \
	systemd \
	systemd-devel \
	libseat-devel \
	wayland-devel \
	libxkbcommon-devel \
	libdisplay-info-devel \
	pixman-devel \
	mesa-libgbm-devel \
	libdrm-devel \
	mesa-libEGL-devel \
	dbus-devel \
	pipewire-devel \
	libadwaita-devel \
	perl-JSON-PP \
	perl-Test-Simple

# Prefer rustup's cargo over Fedora's /bin/cargo when ~/.cargo/bin exists.
# Idempotent: tee replaces the file; the snippet is a no-op if already first
# on PATH (a host-inherited PATH may list it later, behind mise). Login
# shells (bash -lc) source /etc/profile.d/.
distrobox enter "$NAME" -- sudo tee /etc/profile.d/swayward-rustup.sh >/dev/null <<'EOF'
# Prefer rustup's cargo/rustc over the Fedora packages when rustup is installed.
[ -d "$HOME/.cargo/bin" ] && case "$PATH" in "$HOME/.cargo/bin"|"$HOME/.cargo/bin:"*) ;; *) PATH="$HOME/.cargo/bin:$PATH" ;; esac
export PATH
EOF

printf '%s\n' "Done. Build with:" \
	"  distrobox enter $NAME -- bash -lc 'cd $(pwd) && cargo build'"
