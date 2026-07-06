Name:           moraine
Version:        0.1.24
Release:        1%{?dist}
Summary:        Snapshot-based backup over SSH/rsync and rclone (CLI + GTK desktop app)

License:        MIT
URL:            https://github.com/TheJonaz/moraine-backup
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  desktop-file-utils

Requires:       rsync
Requires:       openssh-clients
Recommends:     rclone
Recommends:     gnupg2
Recommends:     NetworkManager

%description
Moraine creates timestamped, hard-linked snapshots over rsync/SSH (or rclone
for cloud/FTP/SMB/WebDAV/S3): every run looks like a complete tree, but
unchanged files share disk via hard links. It restores whole trees or single
files, prunes old snapshots with a GFS retention policy, and keeps a run log.

This package provides both the command-line client (moraine) and the GTK 4
desktop application (moraine-gui).

%prep
%autosetup -n moraine-backup-%{version}

%build
# Build with network access to fetch crates (enable "network" for the Copr
# project, or vendor the deps for a fully offline mock build).
cargo build --release --locked

%install
install -Dm0755 target/release/moraine     %{buildroot}%{_bindir}/moraine
install -Dm0755 target/release/moraine-gui  %{buildroot}%{_bindir}/moraine-gui

desktop-file-install --dir %{buildroot}%{_datadir}/applications assets/moraine-gui.desktop

install -Dm0644 assets/moraine.svg     %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/moraine.svg
install -Dm0644 assets/moraine-256.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/moraine.png
install -Dm0644 assets/moraine-128.png %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/moraine.png
install -Dm0644 assets/moraine-64.png  %{buildroot}%{_datadir}/icons/hicolor/64x64/apps/moraine.png
install -Dm0644 assets/moraine-48.png  %{buildroot}%{_datadir}/icons/hicolor/48x48/apps/moraine.png

# Runtime assets the GUI loads from /usr/share/moraine/assets.
install -Dm0644 assets/hero-bg.png     %{buildroot}%{_datadir}/moraine/assets/hero-bg.png
install -Dm0644 assets/moraine-64.png  %{buildroot}%{_datadir}/moraine/assets/moraine-64.png
install -Dm0644 assets/moraine-256.png %{buildroot}%{_datadir}/moraine/assets/moraine-256.png

install -Dm0644 debian/moraine.1     %{buildroot}%{_mandir}/man1/moraine.1
install -Dm0644 debian/moraine-gui.1 %{buildroot}%{_mandir}/man1/moraine-gui.1

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/moraine-gui.desktop

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/moraine
%{_bindir}/moraine-gui
%{_datadir}/applications/moraine-gui.desktop
%{_datadir}/icons/hicolor/*/apps/moraine.*
%dir %{_datadir}/moraine
%dir %{_datadir}/moraine/assets
%{_datadir}/moraine/assets/*
%{_mandir}/man1/moraine.1*
%{_mandir}/man1/moraine-gui.1*

%changelog
* Mon Jul 06 2026 Jonaz Thern <info@thern.io> - 0.1.24-1
- Desktop notifications when a backup finishes — a normal one on success, a critical one on failure (so a failed scheduled run doesn't go unnoticed).

* Mon Jul 06 2026 Jonaz Thern <info@thern.io> - 0.1.23-1
- In-app background update download with a progress bar.

* Sun Jul 05 2026 Jonaz Thern <info@thern.io> - 0.1.22-1
- System tray, in-app update check, window icon fix.

* Sat Jul 04 2026 Jonaz Thern <info@thern.io> - 0.1.19-1
- Autostart launches minimized; portable asset paths (XDG_DATA_DIRS).

* Fri Jul 03 2026 Jonaz Thern <info@thern.io> - 0.1.17-1
- Initial RPM package (CLI + GTK desktop app).
