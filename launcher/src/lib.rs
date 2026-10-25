//! Keeps the game up to date. The newest release on GitHub is looked up; when it is not the one installed, its
//! archive for this system is downloaded, checked, unpacked beside the old one and swapped in, so a download that
//! fails half way leaves the game that was there. Where the game is kept is `Install`; what `update` did is `Outcome`.

use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Where the releases are.
pub const REPOSITORY: &str = "Sourceless/panic_and_improvise";
pub const GITHUB_API: &str = "https://api.github.com";

/// Set to keep the game somewhere other than the usual place for the system.
pub const HOME_VARIABLE: &str = "PANIC_AND_IMPROVISE_HOME";

/// What a release lists for download.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    /// `sha256:` and the hash, when GitHub has worked it out.
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    TarGz,
    Zip,
}

/// What this system is given: the archive's name and how it is packed, and the game's program in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Platform {
    pub archive: &'static str,
    pub kind: Kind,
    pub program: &'static str,
}

impl Platform {
    pub const LINUX: Platform = Platform { archive: "panic-and-improvise-linux-x86_64.tar.gz", kind: Kind::TarGz, program: "fps_prototype" };
    pub const WINDOWS: Platform = Platform { archive: "panic-and-improvise-windows-x86_64.zip", kind: Kind::Zip, program: "fps_prototype.exe" };

    pub fn current() -> Platform {
        if cfg!(windows) { Platform::WINDOWS } else { Platform::LINUX }
    }

    /// The asset of `release` for this system.
    pub fn asset<'a>(&self, release: &'a Release) -> Option<&'a Asset> {
        release.assets.iter().find(|a| a.name == self.archive)
    }
}

/// The game as kept on this machine: `game/` holds what was unpacked and `version` the release it came from.
#[derive(Debug, Clone)]
pub struct Install {
    pub root: PathBuf,
    pub platform: Platform,
}

impl Install {
    pub fn at(root: impl Into<PathBuf>, platform: Platform) -> Install {
        Install { root: root.into(), platform }
    }

    /// The usual place for this system, or where `PANIC_AND_IMPROVISE_HOME` says.
    pub fn usual() -> Result<Install> {
        let root = if let Some(root) = std::env::var_os(HOME_VARIABLE).filter(|v| !v.is_empty()) {
            PathBuf::from(root)
        } else if cfg!(windows) {
            let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
            PathBuf::from(base).join("PanicAndImprovise")
        } else if let Some(base) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            PathBuf::from(base).join("panic_and_improvise")
        } else {
            let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
            PathBuf::from(home).join(".local/share/panic_and_improvise")
        };
        Ok(Install::at(root, Platform::current()))
    }

    pub fn game_dir(&self) -> PathBuf {
        self.root.join("game")
    }

    pub fn program(&self) -> PathBuf {
        self.game_dir().join(self.platform.program)
    }

    fn version_file(&self) -> PathBuf {
        self.root.join("version")
    }

    /// The release that is installed, if the game is there to run.
    pub fn installed(&self) -> Option<String> {
        if !self.program().is_file() {
            return None;
        }
        let tag = fs::read_to_string(self.version_file()).ok()?;
        Some(tag.trim().to_string()).filter(|t| !t.is_empty())
    }
}

/// What `update` found and did.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The release installed is the newest.
    Current(String),
    /// A newer release was downloaded and is now the one installed.
    Updated { from: Option<String>, to: String },
    /// Nothing has been released yet.
    NothingReleased,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("panic-and-improvise-launcher/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// The newest release of `repository` (as `owner/name`) at the GitHub API at `api`; none when there is none.
pub fn latest_release(api: &str, repository: &str) -> Result<Option<Release>> {
    let url = format!("{api}/repos/{repository}/releases/latest");
    match agent().get(&url).header("Accept", "application/vnd.github+json").call() {
        Ok(mut response) => {
            let body = response.body_mut().read_to_string()?;
            Ok(Some(serde_json::from_str(&body).map_err(|e| format!("could not read the release from {url}: {e}"))?))
        }
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(format!("could not look up the latest release ({url}): {e}").into()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `asset` to `to`, calling `progress(done, total)` as it goes, and checks it is the size and the
/// hash it is listed as. The file is removed if it is not.
pub fn download(asset: &Asset, to: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let result = download_to(asset, to, progress);
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

fn download_to(asset: &Asset, to: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let response = agent().get(&asset.browser_download_url).call().map_err(|e| format!("could not download {}: {e}", asset.name))?;
    let mut body = response.into_body().into_reader();
    let mut file = File::create(to)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = body.read(&mut buffer).map_err(|e| format!("the download of {} broke off: {e}", asset.name))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        done += n as u64;
        progress(done, asset.size);
    }
    file.flush()?;
    if done != asset.size {
        return Err(format!("{} came to {done} bytes, not the {} it should be", asset.name, asset.size).into());
    }
    if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let got = hex(&hash.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            return Err(format!("{} is not what the release published (sha256 {got}, not {expected})", asset.name).into());
        }
    }
    Ok(())
}

/// Unpacks the archive at `archive` into the new, empty directory `to`.
pub fn extract(kind: Kind, archive: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    match kind {
        Kind::TarGz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(File::open(archive)?));
            tar.set_preserve_permissions(true);
            // Entries that would land outside `to` are skipped by the library.
            tar.unpack(to)?;
        }
        Kind::Zip => zip::ZipArchive::new(File::open(archive)?)?.extract(to)?,
    }
    Ok(())
}

/// The one directory an archive was packed in.
fn only_directory(dir: &Path) -> Result<PathBuf> {
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    match entries.as_mut_slice() {
        [one] if one.path().is_dir() => Ok(one.path()),
        _ => Err("the archive is not one directory".into()),
    }
}

#[cfg(unix)]
fn make_runnable(program: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(program, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_runnable(_: &Path) -> Result<()> {
    Ok(())
}

/// Makes `release` the one installed: downloaded and unpacked out of the way, and only then put in place of
/// the game that is there.
pub fn install(install: &Install, release: &Release, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let asset = install.platform.asset(release).ok_or_else(|| format!("release {} has no {} to download", release.tag_name, install.platform.archive))?;
    fs::create_dir_all(&install.root)?;
    let download_path = install.root.join("download.part");
    let staging = install.root.join("staging");
    let old = install.root.join("game.old");
    remove_dir(&staging)?;
    remove_dir(&old)?;

    download(asset, &download_path, progress)?;
    let unpacked = extract(install.platform.kind, &download_path, &staging).and_then(|()| only_directory(&staging));
    let _ = fs::remove_file(&download_path);
    let unpacked = match unpacked {
        Ok(dir) => dir,
        Err(e) => {
            let _ = remove_dir(&staging);
            return Err(e);
        }
    };
    let program = unpacked.join(install.platform.program);
    if !program.is_file() {
        let _ = remove_dir(&staging);
        return Err(format!("{} has no {} in it", asset.name, install.platform.program).into());
    }
    make_runnable(&program)?;

    let game = install.game_dir();
    let had_game = game.exists();
    if had_game {
        fs::rename(&game, &old)?;
    }
    if let Err(e) = fs::rename(&unpacked, &game) {
        if had_game {
            let _ = fs::rename(&old, &game);
        }
        let _ = remove_dir(&staging);
        return Err(format!("could not put the new version in place: {e}").into());
    }
    fs::write(install.version_file(), &release.tag_name)?;
    let _ = remove_dir(&staging);
    let _ = remove_dir(&old);
    Ok(())
}

fn remove_dir(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Looks up the newest release and installs it unless it is the one installed already.
pub fn update(installation: &Install, api: &str, repository: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<Outcome> {
    let Some(release) = latest_release(api, repository)? else {
        return Ok(Outcome::NothingReleased);
    };
    let from = installation.installed();
    if from.as_deref() == Some(release.tag_name.as_str()) {
        return Ok(Outcome::Current(release.tag_name));
    }
    install(installation, &release, progress)?;
    Ok(Outcome::Updated { from, to: release.tag_name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A directory of its own that is removed afterwards.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("launcher-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// An archive for `platform`: a directory holding the game, whose program says which version it is.
    fn archive(platform: Platform, version: &str) -> Vec<u8> {
        let dir = "panic-and-improvise-test";
        let program = format!("#!/bin/sh\necho {version}\n");
        match platform.kind {
            Kind::TarGz => {
                let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
                let mut header = tar::Header::new_gnu();
                header.set_size(program.len() as u64);
                header.set_mode(0o644);
                tar.append_data(&mut header, format!("{dir}/{}", platform.program), program.as_bytes()).unwrap();
                let mut header = tar::Header::new_gnu();
                header.set_size(5);
                header.set_mode(0o644);
                tar.append_data(&mut header, format!("{dir}/assets/hello.txt"), &b"hello"[..]).unwrap();
                tar.into_inner().unwrap().finish().unwrap()
            }
            Kind::Zip => {
                let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file(format!("{dir}/{}", platform.program), options).unwrap();
                zip.write_all(program.as_bytes()).unwrap();
                zip.start_file(format!("{dir}/assets/hello.txt"), options).unwrap();
                zip.write_all(b"hello").unwrap();
                zip.finish().unwrap().into_inner()
            }
        }
    }

    /// A stand-in for GitHub on this machine: says the latest release is `tag` (or none), and serves its archive.
    struct Github {
        api: String,
        downloads: Arc<AtomicUsize>,
        latest: Arc<Mutex<Option<(String, Vec<u8>)>>>,
        corrupt: Arc<Mutex<bool>>,
    }

    impl Github {
        fn start(platform: Platform) -> Github {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let downloads = Arc::new(AtomicUsize::new(0));
            let latest: Arc<Mutex<Option<(String, Vec<u8>)>>> = Arc::new(Mutex::new(None));
            let corrupt = Arc::new(Mutex::new(false));
            let (d, l, c) = (downloads.clone(), latest.clone(), corrupt.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    serve(stream, port, platform, &d, &l, &c);
                }
            });
            Github { api: format!("http://127.0.0.1:{port}"), downloads, latest, corrupt }
        }

        fn release(&self, tag: &str, platform: Platform) {
            *self.latest.lock().unwrap() = Some((tag.to_string(), archive(platform, tag)));
        }
    }

    fn serve(stream: TcpStream, port: u16, platform: Platform, downloads: &AtomicUsize, latest: &Mutex<Option<(String, Vec<u8>)>>, corrupt: &Mutex<bool>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
        }
        let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
        let latest = latest.lock().unwrap().clone();
        let (status, body): (&str, Vec<u8>) = match (path.as_str(), latest) {
            (p, None) if p.ends_with("/releases/latest") => ("404 Not Found", b"{}".to_vec()),
            (p, Some((tag, bytes))) if p.ends_with("/releases/latest") => {
                let sum = hex(&Sha256::digest(&bytes));
                let digest = if *corrupt.lock().unwrap() { "sha256:00".to_string() } else { format!("sha256:{sum}") };
                let json = format!(
                    r#"{{"tag_name":"{tag}","assets":[{{"name":"other.txt","browser_download_url":"http://127.0.0.1:{port}/x","size":1}},{{"name":"{}","browser_download_url":"http://127.0.0.1:{port}/download/{tag}","size":{},"digest":"{digest}"}}]}}"#,
                    platform.archive,
                    bytes.len()
                );
                ("200 OK", json.into_bytes())
            }
            (p, Some((_, bytes))) if p.starts_with("/download/") => {
                downloads.fetch_add(1, Ordering::SeqCst);
                ("200 OK", bytes)
            }
            _ => ("404 Not Found", Vec::new()),
        };
        let mut stream = stream;
        write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(&body).unwrap();
    }

    fn no_progress() -> impl FnMut(u64, u64) {
        |_, _| {}
    }

    #[test]
    fn the_asset_for_a_system_is_found_by_its_name() {
        let release = Release {
            tag_name: "v1".into(),
            assets: vec![
                Asset { name: Platform::WINDOWS.archive.into(), browser_download_url: "w".into(), size: 1, digest: None },
                Asset { name: Platform::LINUX.archive.into(), browser_download_url: "l".into(), size: 1, digest: None },
            ],
        };
        assert_eq!(Platform::LINUX.asset(&release).unwrap().browser_download_url, "l");
        assert_eq!(Platform::WINDOWS.asset(&release).unwrap().browser_download_url, "w");
        assert!(Platform::LINUX.asset(&Release { tag_name: "v1".into(), assets: vec![] }).is_none());
    }

    #[test]
    fn a_release_is_read_from_what_github_sends_ignoring_what_it_does_not_need() {
        let json = r#"{"id":1,"tag_name":"v0.2.0","draft":false,"assets":[{"id":9,"name":"a.zip","size":12,"browser_download_url":"https://x/a.zip","digest":"sha256:ab","state":"uploaded"},{"name":"b","size":3,"browser_download_url":"https://x/b"}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.assets[0].digest.as_deref(), Some("sha256:ab"));
        assert_eq!(release.assets[1].digest, None);
    }

    #[test]
    fn it_installs_the_latest_release_then_leaves_it_alone_until_there_is_a_newer_one() {
        let platform = Platform::LINUX;
        let github = Github::start(platform);
        let scratch = Scratch::new("update");
        let installation = Install::at(scratch.0.join("home"), platform);
        assert_eq!(installation.installed(), None);

        assert_eq!(update(&installation, &github.api, "o/r", &mut no_progress()).unwrap(), Outcome::NothingReleased);
        assert_eq!(installation.installed(), None);

        github.release("v1", platform);
        let mut last = (0, 0);
        let outcome = update(&installation, &github.api, "o/r", &mut |done, total| last = (done, total)).unwrap();
        assert_eq!(outcome, Outcome::Updated { from: None, to: "v1".into() });
        assert_eq!(last.0, last.1, "progress ran to the end");
        assert!(last.1 > 0);
        assert_eq!(installation.installed().as_deref(), Some("v1"));
        assert_eq!(fs::read_to_string(installation.program()).unwrap(), "#!/bin/sh\necho v1\n");
        assert_eq!(fs::read_to_string(installation.game_dir().join("assets/hello.txt")).unwrap(), "hello");
        assert_eq!(github.downloads.load(Ordering::SeqCst), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(installation.program()).unwrap().permissions().mode() & 0o111, 0o111, "it can be run");
        }

        let outcome = update(&installation, &github.api, "o/r", &mut no_progress()).unwrap();
        assert_eq!(outcome, Outcome::Current("v1".into()));
        assert_eq!(github.downloads.load(Ordering::SeqCst), 1, "nothing was downloaded again");

        github.release("v2", platform);
        let outcome = update(&installation, &github.api, "o/r", &mut no_progress()).unwrap();
        assert_eq!(outcome, Outcome::Updated { from: Some("v1".into()), to: "v2".into() });
        assert_eq!(fs::read_to_string(installation.program()).unwrap(), "#!/bin/sh\necho v2\n");
        assert_eq!(installation.installed().as_deref(), Some("v2"));
        let left: Vec<_> = fs::read_dir(&installation.root).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(left.len(), 2, "only the game and its version are kept: {left:?}");
    }

    #[test]
    fn a_download_that_is_not_what_was_published_is_not_installed_and_the_game_that_was_there_stays() {
        let platform = Platform::LINUX;
        let github = Github::start(platform);
        let scratch = Scratch::new("corrupt");
        let installation = Install::at(scratch.0.join("home"), platform);
        github.release("v1", platform);
        update(&installation, &github.api, "o/r", &mut no_progress()).unwrap();

        github.release("v2", platform);
        *github.corrupt.lock().unwrap() = true;
        let error = update(&installation, &github.api, "o/r", &mut no_progress()).unwrap_err().to_string();
        assert!(error.contains("not what the release published"), "{error}");
        assert_eq!(installation.installed().as_deref(), Some("v1"));
        assert_eq!(fs::read_to_string(installation.program()).unwrap(), "#!/bin/sh\necho v1\n");
        assert!(!installation.root.join("download.part").exists());
    }

    #[test]
    fn a_release_with_nothing_for_this_system_is_an_error_and_changes_nothing() {
        let github = Github::start(Platform::WINDOWS);
        github.release("v1", Platform::WINDOWS);
        let scratch = Scratch::new("missing");
        let installation = Install::at(scratch.0.join("home"), Platform::LINUX);
        let error = update(&installation, &github.api, "o/r", &mut no_progress()).unwrap_err().to_string();
        assert!(error.contains("has no panic-and-improvise-linux-x86_64.tar.gz"), "{error}");
        assert_eq!(installation.installed(), None);
    }

    #[test]
    fn it_cannot_reach_github() {
        // A port nothing is listening on.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let scratch = Scratch::new("offline");
        let installation = Install::at(scratch.0.join("home"), Platform::LINUX);
        let error = update(&installation, &format!("http://127.0.0.1:{port}"), "o/r", &mut no_progress()).unwrap_err().to_string();
        assert!(error.contains("could not look up the latest release"), "{error}");
    }

    #[test]
    fn a_zip_is_unpacked_like_a_tar() {
        let scratch = Scratch::new("zip");
        let file = scratch.0.join("a.zip");
        fs::write(&file, archive(Platform::WINDOWS, "v9")).unwrap();
        let to = scratch.0.join("out");
        extract(Kind::Zip, &file, &to).unwrap();
        let dir = only_directory(&to).unwrap();
        assert_eq!(fs::read_to_string(dir.join("fps_prototype.exe")).unwrap(), "#!/bin/sh\necho v9\n");
        assert_eq!(fs::read_to_string(dir.join("assets/hello.txt")).unwrap(), "hello");
    }

    #[test]
    fn an_archive_cannot_write_outside_where_it_is_unpacked() {
        let scratch = Scratch::new("escape");
        let mut raw = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut raw);
            let mut header = tar::Header::new_gnu();
            let name = b"../escaped.txt";
            header.as_old_mut().name[..name.len()].copy_from_slice(name);
            header.set_size(3);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, &b"bad"[..]).unwrap();
            tar.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        let file = scratch.0.join("evil.tar.gz");
        fs::write(&file, gz.finish().unwrap()).unwrap();
        let to = scratch.0.join("out");
        let _ = extract(Kind::TarGz, &file, &to);
        assert!(!scratch.0.join("escaped.txt").exists());
    }

    #[test]
    fn the_usual_place_follows_the_home_variable() {
        // Only this test touches the variable.
        unsafe { std::env::set_var(HOME_VARIABLE, "/somewhere/else") };
        assert_eq!(Install::usual().unwrap().root, PathBuf::from("/somewhere/else"));
        unsafe { std::env::remove_var(HOME_VARIABLE) };
    }
}
