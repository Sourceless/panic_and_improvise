{
  description = "Panic and Improvise: a first-person survival game built with Bevy";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      };
      lib = pkgs.lib;
      windowsTarget = "x86_64-pc-windows-gnu";

      toolchain = p: p.rust-bin.stable.latest.minimal.override {
        targets = [ windowsTarget ];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      # Only Rust sources and the manifests: a change to the assets does not rebuild the dependencies, and
      # nothing in `src` reads a file at build time.
      src = craneLib.cleanCargoSource ./.;

      # Only the game and its bot; the previews and viewers are for development.
      bins = "--bin fps_prototype --bin bot";

      common = {
        inherit src;
        strictDeps = true;
        doCheck = false;
        pname = "panic-and-improvise";
        version = "0.1.0";
        cargoExtraArgs = bins;
      };

      # --- Linux ---------------------------------------------------------------------------------------------
      linuxLibs = with pkgs; [
        alsa-lib
        udev
        libxkbcommon
        wayland
        vulkan-loader
        libx11
        libxcursor
        libxi
        libxrandr
        libxcb
      ];
      linuxCommon = common // {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = linuxLibs;
      };
      linuxDeps = craneLib.buildDepsOnly linuxCommon;
      linux = craneLib.buildPackage (linuxCommon // {
        cargoArtifacts = linuxDeps;
        # The windowing, graphics and input libraries are opened when the game starts, not linked.
        postFixup = ''
          for exe in fps_prototype bot; do
            patchelf --add-rpath ${lib.makeLibraryPath linuxLibs} $out/bin/$exe
          done
        '';
        nativeBuildInputs = linuxCommon.nativeBuildInputs ++ [ pkgs.patchelf ];
      });

      # --- Windows (cross-compiled with MinGW) ---------------------------------------------------------------
      pkgsWindows = import nixpkgs {
        localSystem = system;
        crossSystem = {
          config = "x86_64-w64-mingw32";
          libc = "msvcrt";
        };
        overlays = [ (import rust-overlay) ];
      };
      craneWindows = (crane.mkLib pkgsWindows).overrideToolchain toolchain;
      windowsCommon = common // {
        buildInputs = [ pkgsWindows.windows.pthreads ];
      };
      windowsDeps = craneWindows.buildDepsOnly windowsCommon;
      windows = craneWindows.buildPackage (windowsCommon // { cargoArtifacts = windowsDeps; });

      # --- The launcher: a small program of its own (no Bevy), made for each system -------------------------------
      launcherCommon = common // {
        pname = "panic-and-improvise-launcher";
        cargoExtraArgs = "-p launcher";
      };
      launcherLinuxDeps = craneLib.buildDepsOnly launcherCommon;
      launcherLinux = craneLib.buildPackage (launcherCommon // {
        cargoArtifacts = launcherLinuxDeps;
        # Its tests stand a web server up on the loopback.
        doCheck = true;
      });
      launcherWindowsDeps = craneWindows.buildDepsOnly launcherCommon;
      launcherWindows = craneWindows.buildPackage (launcherCommon // { cargoArtifacts = launcherWindowsDeps; });

      # --- What is handed out: the programs and the assets they read, which are looked for next to them ---------
      assets = lib.fileset.toSource {
        root = ./.;
        fileset = ./assets;
      };

      linuxArchive = pkgs.runCommand "panic-and-improvise-linux-x86_64" { nativeBuildInputs = [ pkgs.patchelf pkgs.gnutar pkgs.gzip ]; } ''
        dir=panic-and-improvise-linux-x86_64
        mkdir -p $dir
        for exe in fps_prototype bot; do
          cp ${linux}/bin/$exe $dir/$exe
          chmod +w $dir/$exe
          # Made to run on any distribution: its own loader, and the libraries it has.
          patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath $dir/$exe
        done
        cp -r ${assets}/assets $dir/assets
        mkdir -p $out
        tar --sort=name --owner=0 --group=0 --mtime=@0 -czf $out/$dir.tar.gz $dir
        # The launcher is handed out beside it, not in it: it is what fetches the archive.
        cp ${launcherLinux}/bin/launcher $out/panic-and-improvise-launcher-linux-x86_64
        chmod +w $out/panic-and-improvise-launcher-linux-x86_64
        patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath $out/panic-and-improvise-launcher-linux-x86_64
      '';

      windowsArchive = pkgs.runCommand "panic-and-improvise-windows-x86_64" { nativeBuildInputs = [ pkgs.zip ]; } ''
        dir=panic-and-improvise-windows-x86_64
        mkdir -p $dir
        cp ${windows}/bin/fps_prototype.exe ${windows}/bin/bot.exe $dir/
        cp -r ${assets}/assets $dir/assets
        mkdir -p $out
        zip -qr -X $out/$dir.zip $dir
        cp ${launcherWindows}/bin/launcher.exe $out/panic-and-improvise-launcher-windows-x86_64.exe
      '';
    in
    {
      packages.${system} = {
        default = linux;
        inherit linux windows launcherLinux launcherWindows linuxArchive windowsArchive;
      };

      checks.${system} = {
        inherit linux windows launcherLinux launcherWindows;
      };

      devShells.${system}.default = craneLib.devShell {
        packages = [ pkgs.pkg-config ] ++ linuxLibs;
        LD_LIBRARY_PATH = lib.makeLibraryPath linuxLibs;
      };
    };
}
