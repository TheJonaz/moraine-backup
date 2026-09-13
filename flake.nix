{
  description = "Moraine — snapshot backups over SSH/rsync and rclone (CLI + GTK app)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
      # Track Cargo.toml so the flake never needs a separate version bump.
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
    in
    {
      packages = forAll (pkgs:
        let
          moraine = pkgs.rustPlatform.buildRustPackage {
            pname = "moraine";
            inherit version;

            # Build from the flake's own tree — no fetchFromGitHub, so no source
            # hash to keep in sync, and `nix build`/`nix run` work straight from a
            # checkout or `github:TheJonaz/moraine-backup`.
            src = self;

            # Vendor dependencies straight from Cargo.lock (all crates.io — no git
            # sources), which removes the cargoHash that used to drift on updates.
            cargoLock.lockFile = ./Cargo.lock;

            buildFeatures = [ "gui" ];

            nativeBuildInputs = with pkgs; [ pkg-config wrapGAppsHook4 ];
            buildInputs = with pkgs; [ gtk4 glib ];

            # Requires moraine >= 0.1.19: assets are resolved via XDG_DATA_DIRS,
            # which wrapGAppsHook4 points at $out/share.
            postInstall = ''
              install -Dm644 assets/moraine-gui.desktop \
                $out/share/applications/io.thern.moraine.desktop
              install -Dm644 assets/moraine.svg \
                $out/share/icons/hicolor/scalable/apps/moraine.svg
              install -Dm644 assets/moraine-256.png \
                $out/share/icons/hicolor/256x256/apps/moraine.png
              for a in hero-bg.png moraine-64.png moraine-256.png; do
                install -Dm644 "assets/$a" "$out/share/moraine/assets/$a"
              done
            '';

            # Put rsync / ssh / rclone on the runtime PATH (wrapGAppsHook4 wraps
            # both binaries and honours makeWrapperArgs).
            makeWrapperArgs = [
              "--prefix" "PATH" ":"
              (pkgs.lib.makeBinPath (with pkgs; [ rsync openssh rclone ]))
            ];

            meta = with pkgs.lib; {
              description = "Snapshot backups over SSH/rsync and rclone (CLI + GTK app)";
              homepage = "https://moraine.thern.io";
              license = licenses.mit;
              mainProgram = "moraine";
              platforms = platforms.linux;
            };
          };
        in
        {
          inherit moraine;
          default = moraine;
        });

      # `nix run github:TheJonaz/moraine-backup` runs the CLI (headless-friendly);
      # `nix run github:TheJonaz/moraine-backup#gui` launches the desktop app.
      apps = forAll (pkgs: rec {
        moraine = {
          type = "app";
          program = "${self.packages.${pkgs.system}.moraine}/bin/moraine";
        };
        gui = {
          type = "app";
          program = "${self.packages.${pkgs.system}.moraine}/bin/moraine-gui";
        };
        default = moraine;
      });
    };
}
