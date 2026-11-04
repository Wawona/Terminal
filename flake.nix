{
  description = "Wawona Terminal. Zig Ghostty VT (zig2nix + ghostty) + Rust libghostty-rs. nix run .#terminal";

  nixConfig = {
    extra-substituters = [ "https://ghostty.cachix.org" ];
    extra-trusted-public-keys = [
      "ghostty.cachix.org-1:QB389yTa6gTyneehvqG58y0WnHjQOqgnA+wBnpWWxns="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Granular Zig packaging (crate2nix analog for Zig).
    # https://github.com/Cloudef/zig2nix
    zig2nix.url = "github:Cloudef/zig2nix";
    # Ghostty VT archive. Pin matches uzaaft/libghostty-rs GHOSTTY_COMMIT.
    ghostty.url = "github:ghostty-org/ghostty/521cf2c5d30bd48028ca61546c64e7913764073d";
    # L3′ Zig GhosttyLib (Metal). Draw proof only until tip ABI swap.
    wwn-ghostty.url = "github:Wawona/Ghostty/development";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      zig2nix,
      ghostty,
      wwn-ghostty,
    }:
    flake-utils.lib.eachSystem flake-utils.lib.defaultSystems (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustVersion = "1.90.0";
        rustToolchain = pkgs.rust-bin.stable.${rustVersion}.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # zig2nix does not support x86_64-darwin. Use zig-env elsewhere.
        zigEnv =
          if system == "x86_64-darwin" then
            null
          else
            zig2nix.outputs.zig-env.${system} {
              inherit nixpkgs;
              zig = zig2nix.packages.${system}.zig-0_17_0;
            };

        zigPkg =
          if zigEnv != null then
            zigEnv.zig
          else
            pkgs.zig_0_17 or pkgs.zig;

        # Official Ghostty flake builds libghostty-vt with Zig (`-Demit-lib-vt`).
        # Product VT core for Wawona Terminal / libghostty-rs pkg-config path.
        libghosttyVt = ghostty.packages.${system}.libghostty-vt;
        wwnGhosttyPkgs = wwn-ghostty.packages.${system} or { };

        terminalPkg = pkgs.callPackage ./nix/package.nix {
          inherit rustPlatform libghosttyVt;
          zig = zigPkg;
          terminalSrc = self;
        };
      in
      {
        packages = {
          default = terminalPkg;
          terminal = terminalPkg;
          wawona-terminal = terminalPkg;
          zig = zigPkg;
          libghostty-vt = libghosttyVt;
        }
        # Re-export Zig GhosttyLib + draw proof. Rust VT is wwn_term_* (Phase 5);
        # tip may dual-link Zig. Product paint still GhosttyGpu until Metal host.
        // pkgs.lib.optionalAttrs (wwnGhosttyPkgs ? libghostty-macos) {
          libghostty-macos = wwnGhosttyPkgs.libghostty-macos;
          libghostty-macos-draw-proof = wwnGhosttyPkgs.libghostty-macos-draw-proof;
        };

        apps = {
          default = {
            type = "app";
            program = "${terminalPkg}/bin/wawona-terminal";
          };
          terminal = {
            type = "app";
            program = "${terminalPkg}/bin/wawona-terminal";
          };
        };

        devShells.default = pkgs.mkShell {
          packages =
            [
              rustToolchain
              pkgs.pkg-config
              pkgs.clang
              pkgs.libclang
              zigPkg
              libghosttyVt
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.gtk4
              pkgs.glib
            ]
            ++ pkgs.lib.optionals (zigEnv != null) [
              zigEnv.zig2nix
              zigEnv.zls
            ];
          shellHook = ''
            export PKG_CONFIG_PATH="${libghosttyVt}/share/pkgconfig:${libghosttyVt.dev or libghosttyVt}/share/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
            export LIBCLANG_PATH=${pkgs.libclang.lib}/lib
            echo "Wawona Terminal: zig=$(command -v zig || true); libghostty-vt pkg-config ready"
          '';
        };
      }
    );
}
