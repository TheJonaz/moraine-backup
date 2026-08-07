COMMENT =		snapshot backup over SSH/rsync and rclone

GH_ACCOUNT =		TheJonaz
GH_PROJECT =		moraine-backup
GH_TAGNAME =		v0.2.2
# DISTNAME follows GH_* and would name the package after the repository;
# the program is moraine.
PKGNAME =		moraine-0.2.2

CATEGORIES =		sysutils

HOMEPAGE =		https://moraine.thern.io
MAINTAINER =		Jonaz Thern <info@thern.io>

# MIT
PERMIT_PACKAGE =	Yes

WANTLIB +=		${MODCARGO_WANTLIB}

MODULES =		devel/cargo

# rsync and the OpenSSH client cover the ssh backend; ssh is in base. rclone is
# only needed for cloud targets, but a backup tool that cannot reach the backend
# the config names is not useful, so it is a hard dependency rather than a hint.
RUN_DEPENDS =		net/rsync \
			sysutils/rclone

CONFIGURE_STYLE =	cargo
SEPARATE_BUILD =	Yes

MODCARGO_RUSTFLAGS =	-C debuginfo=0

# The GTK desktop client is deliberately not built here; see README.md.
# Without default features the `moraine-gui` binary drops out on its own (it
# declares required-features = ["gui"]), and the keyring goes with it, which is
# right for a package that will mostly run on headless machines from cron.
MODCARGO_NO_DEFAULT_FEATURES =	Yes
MODCARGO_INSTALL_ARGS +=	--bin moraine

.include "crates.inc"

post-install:
	${INSTALL_MAN} ${WRKSRC}/man/moraine.1 ${PREFIX}/man/man1/
	${INSTALL_DATA_DIR} ${PREFIX}/share/examples/moraine
	${INSTALL_DATA} ${WRKSRC}/moraine.example.toml \
	    ${PREFIX}/share/examples/moraine/

.include <bsd.port.mk>
