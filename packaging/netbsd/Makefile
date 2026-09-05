# $NetBSD$

DISTNAME=	moraine-0.3.0
CATEGORIES=	sysutils
MASTER_SITES=	${MASTER_SITE_GITHUB:=TheJonaz/}
GITHUB_PROJECT=	moraine-backup
GITHUB_TAG=	v${PKGVERSION_NOREV}

MAINTAINER=	info@thern.io
HOMEPAGE=	https://moraine.thern.io/
COMMENT=	Snapshot backup over SSH/rsync and rclone
LICENSE=	mit

# The GitHub tag archive unpacks to the repository name, which is not DISTNAME.
WRKSRC=		${WRKDIR}/${GITHUB_PROJECT}-${PKGVERSION_NOREV}

.include "cargo-depends.mk"

# Command-line client only; the GTK desktop application is not built here (see
# README.md). Dropping the default features also drops the moraine-gui binary,
# which declares required-features = ["gui"], and the keyring along with it:
# the right shape for a package that mostly runs headless from cron.
# (pkglint rejects non-ASCII in a Makefile, so no dashes fancier than these.)
CARGO_NO_DEFAULT_FEATURES=	YES

# ssh(1) is in the NetBSD base system; these two are not.
DEPENDS+=	rsync-[0-9]*:../../net/rsync
DEPENDS+=	rclone-[0-9]*:../../net/rclone

INSTALLATION_DIRS+=	${PKGMANDIR}/man1 share/examples/moraine

post-install:
	${INSTALL_MAN} ${WRKSRC}/man/moraine.1 \
	  ${DESTDIR}${PREFIX}/${PKGMANDIR}/man1/moraine.1
	${INSTALL_DATA} ${WRKSRC}/moraine.example.toml \
	  ${DESTDIR}${PREFIX}/share/examples/moraine/moraine.example.toml

.include "../../lang/rust/cargo.mk"
.include "../../mk/bsd.pkg.mk"
