{
  description = "Timed, a Noctalia countdown timer plugin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        inherit (pkgs) lib;

        rustToolchain = pkgs.rust-bin.stable.latest.minimal.override {
          extensions = [
            "rust-src"
            "rustfmt"
            "clippy"
            "rust-analyzer"
          ];
          # The release helper, so `cargo build --target
          # x86_64-unknown-linux-musl` works from the dev shell.
          targets = ["x86_64-unknown-linux-musl"];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # The crate build reads noctalia.toml and embeds assets/, so the
        # generated plugin directory is the only thing worth filtering out.
        src = lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            !(builtins.elem (baseNameOf (toString path)) [
              "target"
              ".tmp"
              ".git"
            ]);
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "timed";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        plugin = craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pname = "timed-plugin";
            doCheck = false;
            doInstallCargoArtifacts = false;
            buildPhaseCargoCommand = "cargo build --release --locked --package timed";
            installPhaseCommand = ''
              mkdir -p $out/timed
              target/release/timed emit $out/timed timed-helper
              cp target/release/timed $out/timed/timed-helper
              chmod +x $out/timed/timed-helper
            '';
          }
        );
      in {
        formatter = pkgs.alejandra;
        packages.default = plugin;

        checks = {
          build = plugin;

          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
            }
          );

          test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "--workspace";
            }
          );

          fmt = craneLib.cargoFmt {
            inherit src;
          };
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          inputsFrom = [plugin];

          packages = with pkgs; [
            rustToolchain
            bacon
            cargo-audit
            cargo-deny
            cargo-nextest
            cargo-tarpaulin
          ];

          RUST_BACKTRACE = "1";
        };
      }
    );
}
