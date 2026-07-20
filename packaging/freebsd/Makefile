PORTNAME=	moraine
DISTVERSIONPREFIX=	v
DISTVERSION=	0.2.1
CATEGORIES=	sysutils

MAINTAINER=	info@thern.io
COMMENT=	Snapshot-based backup over SSH/rsync and rclone (CLI + GTK app)
WWW=		https://moraine.thern.io

LICENSE=	MIT
LICENSE_FILE=	${WRKSRC}/LICENSE

USES=		cargo desktop-file-utils gnome pkgconfig
USE_GITHUB=	yes
GH_ACCOUNT=	TheJonaz
GH_PROJECT=	moraine-backup
USE_GNOME=	gtk40

CARGO_FEATURES=	gui

# Regenerate with `make cargo-crates` (writes the up-to-date list) on bumps:
CARGO_CRATES=	\
		android_system_properties-0.1.5 \
		anstream-1.0.0 \
		anstyle-1.0.14 \
		anstyle-parse-1.0.0 \
		anstyle-query-1.1.5 \
		anstyle-wincon-3.0.11 \
		anyhow-1.0.102 \
		async-channel-2.5.0 \
		autocfg-1.5.1 \
		bitflags-2.13.0 \
		bumpalo-3.20.3 \
		cairo-rs-0.22.0 \
		cairo-sys-rs-0.22.0 \
		cc-1.2.65 \
		cfg-expr-0.20.8 \
		cfg-if-1.0.4 \
		chrono-0.4.45 \
		clap-4.6.1 \
		clap_builder-4.6.0 \
		clap_derive-4.6.1 \
		clap_lex-1.1.0 \
		colorchoice-1.0.5 \
		concurrent-queue-2.5.0 \
		core-foundation-sys-0.8.7 \
		crossbeam-utils-0.8.21 \
		equivalent-1.0.2 \
		event-listener-5.4.1 \
		event-listener-strategy-0.5.4 \
		field-offset-0.3.6 \
		find-msvc-tools-0.1.9 \
		futures-channel-0.3.32 \
		futures-core-0.3.32 \
		futures-executor-0.3.32 \
		futures-io-0.3.32 \
		futures-macro-0.3.32 \
		futures-task-0.3.32 \
		futures-util-0.3.32 \
		gdk-pixbuf-0.22.0 \
		gdk-pixbuf-sys-0.22.0 \
		gdk4-0.11.4 \
		gdk4-sys-0.11.4 \
		gio-0.22.8 \
		gio-sys-0.22.8 \
		glib-0.22.8 \
		glib-macros-0.22.6 \
		glib-sys-0.22.8 \
		gobject-sys-0.22.6 \
		graphene-rs-0.22.8 \
		graphene-sys-0.22.8 \
		gsk4-0.11.4 \
		gsk4-sys-0.11.4 \
		gtk4-0.11.4 \
		gtk4-macros-0.11.4 \
		gtk4-sys-0.11.4 \
		hashbrown-0.17.1 \
		heck-0.5.0 \
		iana-time-zone-0.1.65 \
		iana-time-zone-haiku-0.1.2 \
		indexmap-2.14.0 \
		is_terminal_polyfill-1.70.2 \
		itoa-1.0.18 \
		js-sys-0.3.102 \
		libc-0.2.186 \
		log-0.4.33 \
		memchr-2.8.2 \
		memoffset-0.9.1 \
		num-traits-0.2.19 \
		once_cell-1.21.4 \
		once_cell_polyfill-1.70.2 \
		pango-0.22.8 \
		pango-sys-0.22.0 \
		parking-2.2.1 \
		pin-project-lite-0.2.17 \
		pkg-config-0.3.33 \
		proc-macro-crate-3.5.0 \
		proc-macro2-1.0.106 \
		quote-1.0.46 \
		rustc_version-0.4.1 \
		rustversion-1.0.22 \
		semver-1.0.28 \
		serde-1.0.228 \
		serde_core-1.0.228 \
		serde_derive-1.0.228 \
		serde_json-1.0.150 \
		serde_spanned-1.1.1 \
		shlex-2.0.1 \
		slab-0.4.12 \
		smallvec-1.15.2 \
		strsim-0.11.1 \
		syn-2.0.118 \
		system-deps-7.0.8 \
		target-lexicon-0.13.5 \
		toml-1.1.2+spec-1.1.0 \
		toml_datetime-1.1.1+spec-1.1.0 \
		toml_edit-0.25.12+spec-1.1.0 \
		toml_parser-1.1.2+spec-1.1.0 \
		toml_writer-1.1.1+spec-1.1.0 \
		unicode-ident-1.0.24 \
		utf8parse-0.2.2 \
		version-compare-0.2.1 \
		wasm-bindgen-0.2.125 \
		wasm-bindgen-macro-0.2.125 \
		wasm-bindgen-macro-support-0.2.125 \
		wasm-bindgen-shared-0.2.125 \
		windows-core-0.62.2 \
		windows-implement-0.60.2 \
		windows-interface-0.59.3 \
		windows-link-0.2.1 \
		windows-result-0.4.1 \
		windows-strings-0.5.1 \
		windows-sys-0.61.2 \
		winnow-1.0.3 \
		zmij-1.0.21

PLIST_FILES=	bin/moraine \
		bin/moraine-gui \
		share/applications/moraine-gui.desktop \
		share/icons/hicolor/scalable/apps/moraine.svg \
		share/icons/hicolor/256x256/apps/moraine.png \
		share/moraine/assets/hero-bg.png \
		share/moraine/assets/moraine-64.png \
		share/moraine/assets/moraine-256.png \
		share/man/man1/moraine.1.gz \
		share/man/man1/moraine-gui.1.gz

post-install:
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-gui.desktop \
		${STAGEDIR}${PREFIX}/share/applications/
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/icons/hicolor/scalable/apps
	${INSTALL_DATA} ${WRKSRC}/assets/moraine.svg \
		${STAGEDIR}${PREFIX}/share/icons/hicolor/scalable/apps/
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/icons/hicolor/256x256/apps
	${INSTALL_DATA} ${WRKSRC}/assets/moraine-256.png \
		${STAGEDIR}${PREFIX}/share/icons/hicolor/256x256/apps/moraine.png
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/moraine/assets
	${INSTALL_DATA} ${WRKSRC}/assets/hero-bg.png ${WRKSRC}/assets/moraine-64.png \
		${WRKSRC}/assets/moraine-256.png ${STAGEDIR}${PREFIX}/share/moraine/assets/
	${INSTALL_MAN} ${WRKSRC}/debian/moraine.1 ${WRKSRC}/debian/moraine-gui.1 \
		${STAGEDIR}${PREFIX}/share/man/man1/

.include <bsd.port.mk>
