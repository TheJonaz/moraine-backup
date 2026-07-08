{
  description = "Moraine — snapshot backups over SSH/rsync and rclone (CLI + GTK app)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in
    {
      packages = forAll (pkgs:
        let
          moraine = pkgs.rustPlatform.buildRustPackage rec {
            pname = "moraine";
            version = "0.1.25";

            src = pkgs.fetchFromGitHub {
              owner = "TheJonaz";
              repo = "moraine-backup";
              rev = "v${version}";
              # `nix build` prints the correct value on first run — paste it here.
              hash = pkgs.lib.fakeHash;
            };
            cargoHash = pkgs.lib.fakeHash;

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

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.system}.moraine}/bin/moraine-gui";
        };
      });
    };
}
