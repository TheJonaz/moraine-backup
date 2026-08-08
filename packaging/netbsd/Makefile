# $NetBSD$

DISTNAME=	moraine-0.2.2
CATEGORIES=	sysutils
MASTER_SITES=	${MASTER_SITE_GITHUB:=TheJonaz/}
GITHUB_PROJECT=	moraine-backup
GITHUB_TAG=	v${PKGVERSION_NOREV}

MAINTAINER=	info@thern.io
HOMEPAGE=	https://moraine.thern.io/
COMMENT=	Snapshot backup over SSH/rsync and rclone (CLI + GTK app)
LICENSE=	mit

# The GitHub tag archive unpacks to the repository name, which is not DISTNAME.
WRKSRC=		${WRKDIR}/${GITHUB_PROJECT}-${PKGVERSION_NOREV}

.include "cargo-depends.mk"

# Build the GTK 4 desktop app (moraine-gui) next to the CLI. `gui` is the only
# non-default feature we want: it pulls in gtk4 and the keyring. `tray` (ksni) is
# deliberately left off, matching the FreeBSD port. moraine-gui declares
# required-features = ["gui"], so enabling gui is what builds and installs it.
# (pkglint rejects non-ASCII in a Makefile, so no dashes fancier than these.)
CARGO_NO_DEFAULT_FEATURES=	YES
CARGO_FEATURES+=		gui

USE_TOOLS+=		pkg-config

# ssh(1) is in the NetBSD base system; these two are not.
DEPENDS+=	rsync-[0-9]*:../../net/rsync
DEPENDS+=	rclone-[0-9]*:../../net/rclone

INSTALLATION_DIRS+=	${PKGMANDIR}/man1
INSTALLATION_DIRS+=	share/applications
INSTALLATION_DIRS+=	share/examples/moraine
INSTALLATION_DIRS+=	share/icons/hicolor/256x256/apps
INSTALLATION_DIRS+=	share/icons/hicolor/scalable/apps
INSTALLATION_DIRS+=	share/moraine/assets

post-install:
	${INSTALL_MAN} ${WRKSRC}/man/moraine.1 \
	  ${DESTDIR}${PREFIX}/${PKGMANDIR}/man1/moraine.1
	${INSTALL_MAN} ${WRKSRC}/man/moraine-gui.1 \
	  ${DESTDIR}${PREFIX}/${PKGMANDIR}/man1/moraine-gui.1
	${INSTALL_DATA} ${WRKSRC}/moraine.example.toml \
	  ${DESTDIR}${PREFIX}/share/examples/moraine/moraine.example.toml
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-gui.desktop \
	  ${DESTDIR}${PREFIX}/share/applications/moraine-gui.desktop
	${INSTALL_DATA} ${WRKSRC}/assets/moraine.svg \
	  ${DESTDIR}${PREFIX}/share/icons/hicolor/scalable/apps/moraine.svg
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-256.png \
	  ${DESTDIR}${PREFIX}/share/icons/hicolor/256x256/apps/moraine.png
	${INSTALL_DATA} ${WRKSRC}/assets/hero-bg.png \
	  ${DESTDIR}${PREFIX}/share/moraine/assets/hero-bg.png
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-64.png \
	  ${DESTDIR}${PREFIX}/share/moraine/assets/moraine-64.png
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-256.png \
	  ${DESTDIR}${PREFIX}/share/moraine/assets/moraine-256.png

.include "../../x11/gtk4/buildlink3.mk"
.include "../../lang/rust/cargo.mk"
.include "../../mk/bsd.pkg.mk"
