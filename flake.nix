{
  description = "brows browser launcher";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          lib = pkgs.lib;
          packageSource = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.lock
              ./Cargo.toml
              ./build.rs
              ./extra/linux
              ./resources
              ./src
              ./xtask
            ];
          };
          runtimeLibraries = with pkgs; [
            fontconfig
            freetype
            libx11
            libxcb
            libxkbcommon
            vulkan-loader
            wayland
          ];
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "brows";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

            src = packageSource;

            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };

            nativeBuildInputs = with pkgs; [
              clang
              cmake
              makeWrapper
              pkg-config
            ];

            buildInputs = runtimeLibraries;

            # Keep the executable and resources together: the Linux resource
            # lookup derives the data directory from the executable path.
            installPhase = ''
              runHook preInstall

              install -Dm755 \
                "target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/brows" \
                "$out/bin/brows"

              install -Dm644 resources/i18n/en-US/builtin.ftl \
                "$out/resources/i18n/en-US/builtin.ftl"
              install -Dm644 resources/icons/512x512/me.tarik02.brows.png \
                "$out/resources/icons/512x512/me.tarik02.brows.png"
              install -Dm644 resources/repository/application-repository.toml \
                "$out/resources/repository/application-repository.toml"

              for size in 16 32 64 128 256 512; do
                install -Dm644 "resources/icons/''${size}x''${size}/me.tarik02.brows.png" \
                  "$out/share/icons/hicolor/''${size}x''${size}/apps/me.tarik02.brows.png"
              done

              install -Dm644 extra/linux/dist/me.tarik02.brows.template.desktop \
                "$out/share/applications/me.tarik02.brows.desktop"
              substituteInPlace "$out/share/applications/me.tarik02.brows.desktop" \
                --replace-fail '€ExecCommand€' "$out/bin/brows %u"

              install -Dm644 extra/linux/dist/me.tarik02.brows.service \
                "$out/share/dbus-1/services/me.tarik02.brows.service"
              substituteInPlace "$out/share/dbus-1/services/me.tarik02.brows.service" \
                --replace-fail '€ExecCommand€' "$out/bin/brows"

              # winit and wgpu load Wayland and Vulkan at runtime rather than
              # linking them directly, so their store paths must remain discoverable.
              wrapProgram "$out/bin/brows" \
                --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibraries}"

              runHook postInstall
            '';

            meta = {
              homepage = "https://github.com/tarik02-org/brows";
              description = "Open the right browser at the right time";
              license = with lib.licenses; [
                mit
                asl20
              ];
              mainProgram = "brows";
              platforms = lib.platforms.linux;
            };
          };
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/brows";
          meta = {
            description = "Open the right browser at the right time";
          };
        };
      });

      overlays.default = final: _prev: {
        brows = self.packages.${final.system}.default;
      };

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          runtimeLibraries = with pkgs; [
            fontconfig
            freetype
            libx11
            libxcb
            libxkbcommon
            vulkan-loader
            wayland
          ];
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clang
              cmake
              gcc
              pkg-config
              rustc
              rustfmt
            ];

            buildInputs = runtimeLibraries;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibraries;
          };
        }
      );
    };
}
