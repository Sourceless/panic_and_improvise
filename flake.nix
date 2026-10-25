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

      # The release this is: 0.1 and the number of commits behind it, which is what the workflow tags a release
      # with. A tree with changes in it, or without its history, is not a release.
      version = if self ? revCount then "0.1.${toString self.revCount}" else "0.1.0-dev";

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
        # Not given to the dependencies, which would rebuild them for every commit.
        PANIC_AND_IMPROVISE_VERSION = version;
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
      windows = craneWindows.buildPackage (windowsCommon // { cargoArtifacts = windowsDeps; PANIC_AND_IMPROVISE_VERSION = version; });

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

      # A package for Debian and Ubuntu: the launcher, a menu entry, and the libraries the game opens when it
      # starts, so that installing it with apt is all that is needed. The game itself the launcher fetches, and
      # keeps up to date, so the package does not have to be installed again for each release.
      debPackage = pkgs.runCommand "panic-and-improvise-deb" { nativeBuildInputs = [ pkgs.dpkg pkgs.patchelf ]; } ''
        root=deb
        mkdir -p $root/DEBIAN $root/usr/bin $root/usr/share/applications
        cp ${launcherLinux}/bin/launcher $root/usr/bin/panic-and-improvise
        chmod 755 $root/usr/bin/panic-and-improvise
        patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath $root/usr/bin/panic-and-improvise
        cat > $root/usr/share/applications/panic-and-improvise.desktop <<'DESKTOP'
        [Desktop Entry]
        Type=Application
        Name=Panic and Improvise
        Comment=First-person survival
        Exec=panic-and-improvise
        Terminal=true
        Categories=Game;ActionGame;
        DESKTOP
        sed -i 's/^ *//' $root/usr/share/applications/panic-and-improvise.desktop
        cat > $root/DEBIAN/control <<CONTROL
        Package: panic-and-improvise
        Version: ${version}
        Architecture: amd64
        Maintainer: Laurence Pakenham-Smith <laurence@sourceless.org>
        Installed-Size: $(du -sk $root/usr | cut -f1)
        Depends: libc6 (>= 2.39), libasound2t64 | libasound2, libudev1, libvulkan1, libxkbcommon0, libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, libxcb1
        Recommends: mesa-vulkan-drivers
        Section: games
        Priority: optional
        Homepage: https://github.com/Sourceless/panic_and_improvise
        Description: Panic and Improvise, a first-person survival game
         Installs the launcher, which downloads the latest release of the game
         when it is started and keeps it up to date.
        CONTROL
        sed -i 's/^        //' $root/DEBIAN/control
        mkdir -p $out
        dpkg-deb --root-owner-group -Zxz --build $root $out/panic-and-improvise_${version}_amd64.deb
      '';

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
        cp ${debPackage}/*.deb $out/
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
        inherit linux windows launcherLinux launcherWindows debPackage linuxArchive windowsArchive;
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
