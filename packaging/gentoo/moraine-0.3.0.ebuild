# Copyright 2026 Gentoo Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

# Regenerate CRATES + the full LICENSE list on version bumps with pycargoebuild:
#   pycargoebuild /path/to/moraine-backup
CRATES="
	aes@0.8.4
	aho-corasick@1.1.5
	android_system_properties@0.1.5
	anstream@1.0.0
	anstyle@1.0.14
	anstyle-parse@1.0.0
	anstyle-query@1.1.5
	anstyle-wincon@3.0.11
	anyhow@1.0.102
	apple-native-keyring-store@1.0.1
	async-broadcast@0.7.2
	async-channel@2.5.0
	async-executor@1.14.0
	async-io@2.6.0
	async-lock@3.4.2
	async-process@2.5.0
	async-recursion@1.1.1
	async-signal@0.2.14
	async-task@4.7.1
	async-trait@0.1.89
	atomic-waker@1.1.2
	autocfg@1.5.1
	bitflags@2.13.0
	block-buffer@0.10.4
	block-padding@0.3.3
	blocking@1.6.2
	bumpalo@3.20.3
	byteorder@1.5.0
	cairo-rs@0.22.0
	cairo-sys-rs@0.22.0
	cbc@0.1.2
	cc@1.2.65
	cfg-expr@0.20.8
	cfg-if@1.0.4
	chrono@0.4.45
	cipher@0.4.4
	clap@4.6.1
	clap_builder@4.6.0
	clap_derive@4.6.1
	clap_lex@1.1.0
	colorchoice@1.0.5
	concurrent-queue@2.5.0
	core-foundation@0.10.1
	core-foundation-sys@0.8.7
	cpufeatures@0.2.17
	crossbeam-utils@0.8.21
	crypto-common@0.1.7
	digest@0.10.7
	endi@1.1.1
	enumflags2@0.7.12
	enumflags2_derive@0.7.12
	equivalent@1.0.2
	errno@0.3.14
	event-listener@5.4.1
	event-listener-strategy@0.5.4
	fastrand@2.4.1
	field-offset@0.3.6
	find-msvc-tools@0.1.9
	futures-channel@0.3.32
	futures-core@0.3.32
	futures-executor@0.3.32
	futures-io@0.3.32
	futures-lite@2.6.1
	futures-macro@0.3.32
	futures-task@0.3.32
	futures-util@0.3.32
	gdk-pixbuf@0.22.0
	gdk-pixbuf-sys@0.22.0
	gdk4@0.11.4
	gdk4-sys@0.11.4
	generic-array@0.14.7
	getrandom@0.2.17
	getrandom@0.4.3
	gio@0.22.8
	gio-sys@0.22.8
	glib@0.22.8
	glib-macros@0.22.6
	glib-sys@0.22.8
	gobject-sys@0.22.6
	graphene-rs@0.22.8
	graphene-sys@0.22.8
	gsk4@0.11.4
	gsk4-sys@0.11.4
	gtk4@0.11.4
	gtk4-macros@0.11.4
	gtk4-sys@0.11.4
	hashbrown@0.17.1
	heck@0.5.0
	hermit-abi@0.5.2
	hex@0.4.3
	hkdf@0.12.4
	hmac@0.12.1
	iana-time-zone@0.1.65
	iana-time-zone-haiku@0.1.2
	indexmap@2.14.0
	inout@0.1.4
	is_terminal_polyfill@1.70.2
	itoa@1.0.18
	js-sys@0.3.102
	keyring@4.1.6
	keyring-core@1.0.0
	ksni@0.3.5
	libc@0.2.186
	linux-raw-sys@0.12.1
	log@0.4.33
	memchr@2.8.2
	memoffset@0.9.1
	num@0.4.3
	num-bigint@0.4.8
	num-complex@0.4.6
	num-integer@0.1.46
	num-iter@0.1.46
	num-rational@0.4.2
	num-traits@0.2.19
	once_cell@1.21.4
	once_cell_polyfill@1.70.2
	ordered-stream@0.2.0
	pango@0.22.8
	pango-sys@0.22.0
	parking@2.2.1
	pastey@0.2.3
	pin-project-lite@0.2.17
	piper@0.2.5
	pkg-config@0.3.33
	polling@3.11.0
	proc-macro-crate@3.5.0
	proc-macro2@1.0.106
	quote@1.0.46
	r-efi@6.0.0
	regex@1.13.1
	regex-automata@0.4.18
	regex-syntax@0.8.11
	rpassword@7.5.4
	rtoolbox@0.0.5
	rustc_version@0.4.1
	rustix@1.1.4
	rustversion@1.0.22
	secret-service@5.1.0
	security-framework@3.7.0
	security-framework-sys@2.17.0
	semver@1.0.28
	serde@1.0.228
	serde_core@1.0.228
	serde_derive@1.0.228
	serde_json@1.0.150
	serde_repr@0.1.20
	serde_spanned@1.1.1
	sha2@0.10.9
	shlex@2.0.1
	signal-hook-registry@1.4.8
	slab@0.4.12
	smallvec@1.15.2
	strsim@0.11.1
	subtle@2.6.1
	syn@2.0.118
	system-deps@7.0.8
	target-lexicon@0.13.5
	tempfile@3.27.0
	toml@1.1.2+spec-1.1.0
	toml_datetime@1.1.1+spec-1.1.0
	toml_edit@0.25.12+spec-1.1.0
	toml_parser@1.1.2+spec-1.1.0
	toml_writer@1.1.1+spec-1.1.0
	tracing@0.1.44
	tracing-attributes@0.1.31
	tracing-core@0.1.36
	typenum@1.20.1
	uds_windows@1.2.1
	unicode-ident@1.0.24
	utf8parse@0.2.2
	uuid@1.23.4
	version-compare@0.2.1
	version_check@0.9.5
	wasi@0.11.1+wasi-snapshot-preview1
	wasm-bindgen@0.2.125
	wasm-bindgen-macro@0.2.125
	wasm-bindgen-macro-support@0.2.125
	wasm-bindgen-shared@0.2.125
	windows-core@0.62.2
	windows-implement@0.60.2
	windows-interface@0.59.3
	windows-link@0.2.1
	windows-native-keyring-store@1.1.0
	windows-result@0.4.1
	windows-strings@0.5.1
	windows-sys@0.59.0
	windows-sys@0.61.2
	windows-targets@0.52.6
	windows_aarch64_gnullvm@0.52.6
	windows_aarch64_msvc@0.52.6
	windows_i686_gnu@0.52.6
	windows_i686_gnullvm@0.52.6
	windows_i686_msvc@0.52.6
	windows_x86_64_gnu@0.52.6
	windows_x86_64_gnullvm@0.52.6
	windows_x86_64_msvc@0.52.6
	winnow@1.0.3
	winresource@0.1.31
	zbus@5.16.0
	zbus-secret-service-keyring-store@1.0.0
	zbus_macros@5.16.0
	zbus_names@4.3.2
	zeroize@1.9.0
	zmij@1.0.21
	zvariant@5.12.0
	zvariant_derive@5.12.0
	zvariant_utils@3.4.0
"

inherit cargo desktop xdg

DESCRIPTION="Snapshot-based backup over SSH/rsync and rclone (CLI + GTK app)"
HOMEPAGE="https://moraine.thern.io"
SRC_URI="
	https://github.com/TheJonaz/moraine-backup/archive/refs/tags/v${PV}.tar.gz -> ${P}.tar.gz
	${CARGO_CRATE_URIS}
"
S="${WORKDIR}/moraine-backup-${PV}"

# MIT for Moraine itself; the rest are the dependent crates' licenses.
LICENSE="MIT"
# Dependent crate licenses
LICENSE+=" Apache-2.0-with-LLVM-exceptions MIT Unicode-3.0 Unlicense"
SLOT="0"
KEYWORDS="~amd64"
IUSE="+gui"

DEPEND="gui? ( gui-libs/gtk:4 )"
RDEPEND="
	${DEPEND}
	net-misc/rsync
	net-misc/openssh
"
# Optional runtime backend: net-misc/rclone (cloud/FTP/SMB/WebDAV/S3)

src_configure() {
	if use gui; then
		# Both defaults spelled out, so the build does not silently change
		# if upstream's default feature set does. `tray` is what compiles
		# the StatusNotifierItem icon in.
		local myfeatures=( gui tray )
		cargo_src_configure
	else
		# cargo_src_configure does NOT add --no-default-features on its own
		# (it only maps myfeatures to --features and forwards "$@"), so
		# without this the default gui+tray build would happen anyway. The
		# moraine-gui bin target is required-features = ["gui"], so cargo
		# then simply skips it and only the CLI is built.
		cargo_src_configure --no-default-features
	fi
}

src_install() {
	cargo_src_install

	doman man/moraine.1

	if use gui; then
		domenu assets/moraine-gui.desktop

		insinto /usr/share/icons/hicolor/scalable/apps
		doins assets/moraine.svg
		newicon -s 256 assets/moraine-256.png moraine.png
		newicon -s 128 assets/moraine-128.png moraine.png
		newicon -s 64 assets/moraine-64.png moraine.png
		newicon -s 48 assets/moraine-48.png moraine.png

		# runtime assets the GUI loads from /usr/share/moraine/assets
		insinto /usr/share/moraine/assets
		doins assets/hero-bg.png assets/moraine-64.png assets/moraine-256.png

		doman man/moraine-gui.1
	fi
}
