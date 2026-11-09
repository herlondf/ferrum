//! Installs a newer release in place, for the formats that can update
//! themselves: the AppImage, the Windows installer and the macOS app.
//! Everything else (Flatpak, .deb/.rpm, Homebrew, winget, Scoop) is left to
//! its package manager, and a copy from `get.sh` / `get.ps1` is updated by
//! running that command again.
//!
//! Every download is checked against the release's `SHA256SUMS` before
//! anything is replaced.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::update::Release;
use crate::{Error, Result};

/// The checksums file every release carries.
pub const CHECKSUMS: &str = "SHA256SUMS";

/// How this copy of Ferrum was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// An AppImage at this path (`$APPIMAGE`).
    AppImage(PathBuf),
    /// Installed by the Windows installer into this folder.
    WindowsInstaller(PathBuf),
    /// A `Ferrum.app` bundle at this path.
    MacApp(PathBuf),
    /// Installed per user by `get.sh` (Linux) or `get.ps1` (Windows).
    Script,
    /// Updated by something else; the name says by what.
    Managed(&'static str),
}

impl Install {
    /// How the running program was installed.
    pub fn current() -> Install {
        let exe = std::env::current_exe().unwrap_or_default();
        detect(
            &exe,
            std::env::var_os("APPIMAGE").map(PathBuf::from).as_deref(),
            std::env::var_os("FLATPAK_ID").is_some(),
        )
    }

    /// Whether Ferrum can install the update by itself.
    pub fn can_update(&self) -> bool {
        !matches!(self, Install::Managed(_) | Install::Script)
    }
}

/// Works out the install kind from the executable path and the environment.
pub fn detect(exe: &Path, appimage: Option<&Path>, flatpak: bool) -> Install {
    if flatpak {
        return Install::Managed("Flatpak");
    }
    if let Some(a) = appimage.filter(|a| a.is_file()) {
        return Install::AppImage(a.to_path_buf());
    }
    let text = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    if text.contains("/scoop/apps/") {
        return Install::Managed("Scoop");
    }
    if text.contains("/cellar/") || text.contains("/caskroom/") {
        return Install::Managed("Homebrew");
    }
    // .../Ferrum.app/Contents/MacOS/ferrum
    let bundle = exe.parent().and_then(Path::parent).and_then(Path::parent);
    if let Some(b) = bundle
        && exe.parent().and_then(Path::file_name) == Some("MacOS".as_ref())
        && b.extension().is_some_and(|e| e == "app")
    {
        return Install::MacApp(b.to_path_buf());
    }
    // The installer puts its uninstaller next to the program.
    if let Some(dir) = exe.parent()
        && dir.join("unins000.exe").is_file()
    {
        return Install::WindowsInstaller(dir.to_path_buf());
    }
    // get.sh: ~/.local/bin/ferrum; get.ps1: %LOCALAPPDATA%\Ferrum\ferrum.exe.
    if text.ends_with("/.local/bin/ferrum") || text.ends_with("/appdata/local/ferrum/ferrum.exe") {
        return Install::Script;
    }
    if cfg!(target_os = "linux") {
        Install::Managed("your package manager")
    } else {
        Install::Managed("manual download")
    }
}

/// The release file for this install kind, e.g. `ferrum-linux-x86_64.AppImage`
/// (the names carry no version, so `releases/latest/download/NAME` works).
pub fn asset_name(install: &Install) -> Option<String> {
    let arch = std::env::consts::ARCH;
    match install {
        Install::AppImage(_) => Some(format!("ferrum-linux-{arch}.AppImage")),
        Install::WindowsInstaller(_) => Some(format!("ferrum-windows-{arch}-setup.exe")),
        Install::MacApp(_) => Some("ferrum-macos-universal.zip".into()),
        Install::Managed(_) | Install::Script => None,
    }
}

/// The command that installs or updates a `get.sh` / `get.ps1` copy.
pub fn script_command() -> &'static str {
    if cfg!(windows) {
        "irm https://raw.githubusercontent.com/herlondf/ferrum/main/get.ps1 | iex"
    } else {
        "curl -fsSL https://raw.githubusercontent.com/herlondf/ferrum/main/get.sh | sh"
    }
}

/// The SHA-256 of `name` in a `sha256sum` style list.
pub fn checksum_for(list: &str, name: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        // `sha256sum -b` marks binary files with a leading `*`.
        let file = file.trim().trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

/// Progress of a download, shared with the UI.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    pub cancel: Arc<AtomicBool>,
}

impl Progress {
    /// 0.0..=1.0, or None while the size is unknown.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| self.done.load(Ordering::Relaxed) as f32 / total as f32)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .user_agent("ferrum-git-client")
        .build()
        .into()
}

fn net(e: impl std::fmt::Display) -> Error {
    Error::Other(format!("network error: {e}"))
}

/// Copies `reader` into `out`, returning the SHA-256 of what was copied.
fn copy_hashed(mut reader: impl Read, out: &mut impl Write, progress: &Progress) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    out.flush()?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Downloads the release file for `install` next to where it goes, checks
/// its checksum and returns its path.
pub fn download(install: &Install, release: &Release, progress: &Progress) -> Result<PathBuf> {
    let name = asset_name(install).ok_or_else(|| {
        Error::Other("this installation is updated by its package manager".into())
    })?;
    let find = |n: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == n)
            .ok_or_else(|| Error::Other(format!("the release has no {n}")))
    };
    let asset = find(&name)?;
    let sums = find(CHECKSUMS)?;
    let agent = agent();
    let list = agent
        .get(&sums.url)
        .call()
        .map_err(net)?
        .body_mut()
        .read_to_string()
        .map_err(net)?;
    let want = checksum_for(&list, &name)
        .ok_or_else(|| Error::Other(format!("{CHECKSUMS} does not list {name}")))?;

    let dest = staging_dir(install)?.join(format!(".{name}.part"));
    let response = agent.get(&asset.url).call().map_err(net)?;
    let total = response.body().content_length().unwrap_or(asset.size);
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let mut file = std::fs::File::create(&dest)?;
    let got = copy_hashed(response.into_body().into_reader(), &mut file, progress);
    drop(file);
    match got {
        Ok(got) if got == want => Ok(dest),
        Ok(got) => {
            let _ = std::fs::remove_file(&dest);
            Err(Error::Other(format!(
                "checksum mismatch for {name}: expected {want}, got {got}"
            )))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Where the download is written: next to the installed files, so the
/// final rename stays on one disk. The Windows installer goes to temp.
fn staging_dir(install: &Install) -> Result<PathBuf> {
    let dir = match install {
        Install::AppImage(p) | Install::MacApp(p) => p.parent().map(Path::to_path_buf),
        Install::WindowsInstaller(_) => Some(std::env::temp_dir()),
        Install::Managed(_) | Install::Script => None,
    };
    dir.ok_or_else(|| Error::Other("no folder for the download".into()))
}

/// What to run to start the updated version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restart {
    /// Start this program (the replaced AppImage or app bundle).
    Program(PathBuf),
    /// The installer was started and restarts Ferrum itself: just quit.
    Quit,
}

/// Puts the downloaded file in place.
pub fn install(install: &Install, downloaded: &Path) -> Result<Restart> {
    match install {
        Install::AppImage(target) => {
            replace_file(downloaded, target)?;
            Ok(Restart::Program(target.clone()))
        }
        Install::WindowsInstaller(_) => {
            // Inno Setup: no questions, closes the running Ferrum, then
            // starts it again (see packaging/windows/ferrum.iss).
            let setup = downloaded.with_file_name("ferrum-setup.exe");
            std::fs::rename(downloaded, &setup)?;
            std::process::Command::new(&setup)
                .args([
                    "/SILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                    "/RELAUNCH=1",
                ])
                .spawn()?;
            Ok(Restart::Quit)
        }
        Install::MacApp(bundle) => {
            replace_bundle(downloaded, bundle)?;
            Ok(Restart::Program(bundle.clone()))
        }
        Install::Managed(by) => Err(Error::Other(format!("updated by {by}"))),
        Install::Script => Err(Error::Other(format!("run again: {}", script_command()))),
    }
}

/// Replaces `target` with `new` (same folder), keeping it executable.
pub fn replace_file(new: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
    }
    // A rename over the old file is atomic; the running copy keeps its inode.
    std::fs::rename(new, target)?;
    Ok(())
}

/// Unpacks the zipped `Ferrum.app` and swaps it with `bundle`.
fn replace_bundle(zip: &Path, bundle: &Path) -> Result<()> {
    let parent = bundle
        .parent()
        .ok_or_else(|| Error::Other("no folder for the app".into()))?;
    let unpacked = parent.join(".ferrum-update");
    let _ = std::fs::remove_dir_all(&unpacked);
    // `ditto` ships with macOS and keeps the bundle's attributes.
    let status = std::process::Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(&unpacked)
        .status()?;
    let _ = std::fs::remove_file(zip);
    if !status.success() {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err(Error::Other("could not unpack the update".into()));
    }
    let new_app = std::fs::read_dir(&unpacked)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| Error::Other("the update holds no app".into()))?;
    swap_dirs(&new_app, bundle)?;
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(())
}

/// Moves `new` to `target`, keeping the old one until the move worked.
pub fn swap_dirs(new: &Path, target: &Path) -> Result<()> {
    let mut old = target.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(target, &old)?;
    if let Err(e) = std::fs::rename(new, target) {
        // Put the old version back.
        let _ = std::fs::rename(&old, target);
        return Err(e.into());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Starts the updated program.
pub fn restart(restart: &Restart) -> Result<()> {
    if let Restart::Program(path) = restart {
        if cfg!(target_os = "macos") && path.extension().is_some_and(|e| e == "app") {
            std::process::Command::new("open")
                .arg("-n")
                .arg(path)
                .spawn()?;
        } else {
            std::process::Command::new(path).spawn()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_install_kind() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Ferrum.AppImage");
        std::fs::write(&image, b"").unwrap();
        let exe = Path::new("/tmp/.mount_x/usr/bin/ferrum");
        assert_eq!(detect(exe, Some(&image), false), Install::AppImage(image));
        assert_eq!(detect(exe, None, true), Install::Managed("Flatpak"));
        // $APPIMAGE pointing nowhere is ignored.
        assert!(!detect(exe, Some(Path::new("/nope")), false).can_update());

        let app = Path::new("/Applications/Ferrum.app/Contents/MacOS/ferrum");
        assert_eq!(
            detect(app, None, false),
            Install::MacApp("/Applications/Ferrum.app".into())
        );
        let brew =
            Path::new("/opt/homebrew/Caskroom/ferrum/0.2.0/Ferrum.app/Contents/MacOS/ferrum");
        assert_eq!(detect(brew, None, false), Install::Managed("Homebrew"));
        let scoop = Path::new("C:\\Users\\me\\scoop\\apps\\ferrum\\current\\ferrum.exe");
        assert_eq!(detect(scoop, None, false), Install::Managed("Scoop"));

        let local = Path::new("/home/me/.local/bin/ferrum");
        assert_eq!(detect(local, None, false), Install::Script);
        let get = Path::new("C:\\Users\\me\\AppData\\Local\\Ferrum\\ferrum.exe");
        assert_eq!(detect(get, None, false), Install::Script);
        assert!(!Install::Script.can_update());

        let win = dir.path().join("Programs").join("Ferrum");
        std::fs::create_dir_all(&win).unwrap();
        std::fs::write(win.join("unins000.exe"), b"").unwrap();
        assert_eq!(
            detect(&win.join("ferrum.exe"), None, false),
            Install::WindowsInstaller(win.clone())
        );
        assert!(!detect(Path::new("/usr/bin/ferrum"), None, false).can_update());
    }

    #[test]
    fn names_the_release_file() {
        let arch = std::env::consts::ARCH;
        let image = Install::AppImage("/x".into());
        assert_eq!(
            asset_name(&image).unwrap(),
            format!("ferrum-linux-{arch}.AppImage")
        );
        let win = Install::WindowsInstaller("/x".into());
        assert_eq!(
            asset_name(&win).unwrap(),
            format!("ferrum-windows-{arch}-setup.exe")
        );
        let mac = Install::MacApp("/x".into());
        assert_eq!(asset_name(&mac).unwrap(), "ferrum-macos-universal.zip");
        assert!(asset_name(&Install::Managed("Flatpak")).is_none());
        assert!(asset_name(&Install::Script).is_none());
    }

    #[test]
    fn reads_the_checksum_list() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let list = format!("{a}  Ferrum-1.0.0-x86_64.AppImage\n{b} *ferrum.zip\nbad line\n");
        assert_eq!(checksum_for(&list, "Ferrum-1.0.0-x86_64.AppImage"), Some(a));
        assert_eq!(checksum_for(&list, "ferrum.zip"), Some("b".repeat(64)));
        assert_eq!(checksum_for(&list, "missing"), None);
    }

    #[test]
    fn hashes_while_copying() {
        let progress = Progress::default();
        let mut out = Vec::new();
        let hash = copy_hashed(&b"abc"[..], &mut out, &progress).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(out, b"abc");
        assert_eq!(progress.done.load(Ordering::Relaxed), 3);

        progress.cancel.store(true, Ordering::Relaxed);
        let err = copy_hashed(&b"abc"[..], &mut Vec::new(), &progress).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }

    #[test]
    fn replaces_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Ferrum.AppImage");
        let new = dir.path().join(".new.part");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        replace_file(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!new.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }

        let app = dir.path().join("Ferrum.app");
        let fresh = dir.path().join("unpacked/Ferrum.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/v"), b"1").unwrap();
        std::fs::create_dir_all(fresh.join("Contents")).unwrap();
        std::fs::write(fresh.join("Contents/v"), b"2").unwrap();
        swap_dirs(&fresh, &app).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/v")).unwrap(), b"2");
        assert!(!dir.path().join("Ferrum.app.old").exists());
    }
}
