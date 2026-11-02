//! Linux installs: the user-local version layout our installer receipts
//! (`<support>/install.json`) updates itself by activating a new version
//! directory and switching the `current` symlink atomically. Source builds
//! (the `source-build` marker) and package-managed executables (pacman,
//! dpkg, rpm, Nix) never replace themselves. Linux has no OS code signature,
//! so every executable carries an ed25519 signature by WordInk's release key
//! (`<target>-sig`, made by `packaging/linux/sign-linux.sh`) over its SHA-256,
//! which [`verify_signature`] checks before the download is even fetched.

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;

use super::Method;
use super::protocol::is_version_name;

#[cfg(target_arch = "x86_64")]
pub const TARGET: &str = "linux-x64";
#[cfg(target_arch = "aarch64")]
pub const TARGET: &str = "linux-arm64";
pub const DOWNLOAD_TARGET: &str = TARGET;

/// WordInk's ed25519 Linux release key, `packaging/linux/wordink-signing.pub`
/// (the `WORDINK_LINUX_SIGNING_KEY` secret of the `release` environment signs
/// with its private half). Changing it needs the owner's review (CODEOWNERS);
/// rotation steps are in SECURITY.md.
pub const SIGNING_KEY: [u8; 32] = [
    0xba, 0x92, 0x2b, 0x68, 0x3b, 0x90, 0x15, 0x9b, 0xa7, 0xf5, 0xbe, 0x19, 0x1a, 0x52, 0xf9, 0x3f,
    0xa8, 0xfd, 0x92, 0x55, 0x4e, 0x38, 0x59, 0x97, 0x4f, 0x36, 0x8f, 0x36, 0xec, 0x64, 0x28, 0x07,
];

/// The manifest target holding the signature of [`TARGET`].
pub fn signature_target() -> String {
    format!("{TARGET}-sig")
}

fn signed_message(sha256: &str) -> String {
    format!("wordink.linux-artifact.v1\n{sha256}\n")
}

/// Whether `signature` is WordInk's release key signing the executable whose
/// SHA-256 is `sha256`. The manifest names that SHA-256, and the download is
/// checked against it, so this ties the bytes to the key.
pub fn verify_signature(sha256: &str, signature: &[u8]) -> Result<()> {
    verify_with(&SIGNING_KEY, sha256, signature)
}

fn verify_with(key: &[u8; 32], sha256: &str, signature: &[u8]) -> Result<()> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let signature =
        Signature::from_slice(signature).wrap_err("the Linux update's signature is malformed")?;
    VerifyingKey::from_bytes(key)
        .wrap_err("invalid Linux release key")?
        .verify_strict(signed_message(sha256).as_bytes(), &signature)
        .wrap_err("the Linux update is not signed by WordInk's release key")
}

/// The release service's distribution kinds for this architecture's `.deb`
/// and `.rpm` packages: their update commands download this architecture's
/// package.
#[cfg(target_arch = "x86_64")]
const PACKAGES: [(&str, &[&str], &str); 3] = [
    ("pacman", &["-Qqo"], "aur"),
    ("dpkg-query", &["-S"], "deb-x64"),
    ("rpm", &["-qf"], "rpm-x64"),
];
#[cfg(target_arch = "aarch64")]
const PACKAGES: [(&str, &[&str], &str); 3] = [
    ("pacman", &["-Qqo"], "aur"),
    ("dpkg-query", &["-S"], "deb-arm64"),
    ("rpm", &["-qf"], "rpm-arm64"),
];

/// The marker `scripts/install-linux.sh` leaves beside an executable it
/// built from a checkout: that install updates from its checkout.
const SOURCE_MARKER: &str = "source-build";

/// The activated executable, behind the switched `current` link.
#[derive(Clone, Debug)]
pub struct Staged(PathBuf);

pub fn method() -> Method {
    // geteuid has no preconditions and does not mutate process state.
    let user = unsafe { libc::geteuid() };
    let Ok(executable) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return Method::Unknown;
    };
    if let Some((support, version_dir)) = installed_layout(&executable, user) {
        if version_dir.join(SOURCE_MARKER).exists() {
            return Method::Source;
        }
        return match super::receipt_channel(&support) {
            Some(channel) => Method::Direct { channel },
            None => Method::Unknown,
        };
    }
    if executable.starts_with("/nix/store") {
        return Method::Package("nix");
    }
    if fs::metadata(&executable).is_ok_and(|metadata| metadata.uid() != user)
        && let Some((_, _, kind)) = PACKAGES.iter().find(|(manager, arguments, _)| {
            Command::new(manager)
                .args(*arguments)
                .arg(&executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
    {
        return Method::Package(kind);
    }
    Method::Unknown
}

pub fn updates_disabled_by_policy() -> bool {
    false
}

/// The Linux targets are the bare executable, so this build is the file
/// whose SHA-256 it has.
pub fn running_sha256() -> Option<String> {
    crate::transcription::models::sha256_hex(&std::env::current_exe().ok()?, None).ok()
}

/// The support directory and this executable's version directory, when it
/// runs from the user-local version layout and no other user can write it.
fn installed_layout(executable: &Path, user: u32) -> Option<(PathBuf, PathBuf)> {
    if user == 0 {
        return None;
    }
    let support = crate::app_paths::support_dir().ok()?.canonicalize().ok()?;
    let versions = support.join("versions").canonicalize().ok()?;
    let parent = executable.parent()?;
    (executable.starts_with(&versions)
        && [support.as_path(), versions.as_path(), parent]
            .into_iter()
            .all(|path| safe_owned_directory(path, user)))
    .then(|| (support.clone(), parent.to_path_buf()))
}

fn safe_owned_directory(path: &Path, user: u32) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.uid() == user && metadata.mode() & 0o022 == 0)
}

pub fn stage(file: &Path, version: &str) -> Result<Staged> {
    let support = crate::app_paths::support_dir()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(support.join("updates").join("update.lock"))?;
    lock.try_lock_exclusive()
        .wrap_err("another Linux update is already in progress")?;
    let staging = support.join("updates").join("hex.staging");
    let _ = fs::remove_file(&staging);
    fs::copy(file, &staging)?;
    let activation =
        validate_executable(&staging, version).and_then(|()| activate(&support, version, &staging));
    if activation.is_err() {
        let _ = fs::remove_file(&staging);
    }
    activation.map(Staged)
}

pub fn restart_into(staged: &Staged) -> Result<()> {
    if !staged.0.is_file() {
        bail!("the prepared WordInk update is no longer installed");
    }
    crate::linux::service::schedule_restart()
}

fn validate_executable(path: &Path, version: &str) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not start the downloaded Linux update")?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            != Some(version)
    {
        bail!("the downloaded Linux update reports the wrong version");
    }
    Ok(())
}

/// Stages the executable into its version directory and switches `current`
/// atomically; the running version's files are never touched, and every
/// version but the new, running, and previous one is pruned. A newer
/// release can replace a staged one, so the previous one is not always the
/// one running.
fn activate(support: &Path, version: &str, staging: &Path) -> Result<PathBuf> {
    let versions = support.join("versions");
    let version_dir = versions.join(version);
    fs::create_dir_all(&version_dir)?;
    fs::set_permissions(&versions, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&version_dir, fs::Permissions::from_mode(0o700))?;
    let executable = version_dir.join("wordink");
    let staged = version_dir.join(".hex.partial");
    let _ = fs::remove_file(&staged);
    fs::rename(staging, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    OpenOptions::new().read(true).open(&staged)?.sync_all()?;
    fs::rename(&staged, &executable)?;
    File::open(&version_dir)?.sync_all()?;

    let current = support.join("current");
    let previous = fs::read_link(&current)
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_owned()));
    let next = support.join(".current.partial");
    let _ = fs::remove_file(&next);
    symlink(Path::new("versions").join(version), &next)?;
    fs::rename(&next, &current)?;
    File::open(support)?.sync_all()?;
    for entry in fs::read_dir(&versions)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_str() != Some(version)
            && name.to_str() != Some(crate::VERSION)
            && previous.as_ref() != Some(&name)
            && name.to_str().is_some_and(is_version_name)
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not prune an old WordInk version");
        }
    }
    Ok(current.join("wordink"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_the_release_key_to_the_sha256() {
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let sha = "a".repeat(64);
        let signature = key.sign(signed_message(&sha).as_bytes()).to_bytes();
        assert!(verify_with(&public, &sha, &signature).is_ok());
        assert!(verify_with(&public, &"b".repeat(64), &signature).is_err());
        assert!(verify_with(&SIGNING_KEY, &sha, &signature).is_err());
        assert!(verify_with(&public, &sha, &signature[..63]).is_err());
    }

    #[test]
    fn the_compiled_key_is_the_published_one() {
        let packaging = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/linux");
        let pem = fs::read_to_string(packaging.join("wordink-signing.pub")).unwrap();
        // SubjectPublicKeyInfo for Ed25519: a fixed 12-byte prefix, then the key.
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(&SIGNING_KEY);
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::new();
        for chunk in der.chunks(3) {
            let bits = chunk.iter().enumerate().fold(0u32, |bits, (index, byte)| {
                bits | u32::from(*byte) << (16 - 8 * index)
            });
            for index in 0..4 {
                encoded.push(if index <= chunk.len() {
                    char::from(ALPHABET[(bits >> (18 - 6 * index) & 63) as usize])
                } else {
                    '='
                });
            }
        }
        assert!(pem.lines().any(|line| line == encoded), "{encoded}");
        let installer = fs::read_to_string(packaging.join("install-release.sh")).unwrap();
        assert!(installer.contains(&format!("\nsigning_key={encoded}\n")));
    }

    #[test]
    fn a_throwaway_key_signs_what_the_updater_verifies() {
        use ed25519_dalek::{Signer, SigningKey};
        // A key generated here, never the release key: proves the format
        // `packaging/linux/sign-linux.sh` writes (a raw 64-byte signature over
        // `wordink.linux-artifact.v1\n<sha256>\n`) is what `verify_with` takes.
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).unwrap();
        let key = SigningKey::from_bytes(&seed);
        let sha = "c".repeat(64);
        assert_eq!(
            signed_message(&sha),
            format!("wordink.linux-artifact.v1\n{sha}\n")
        );
        let signature = key.sign(signed_message(&sha).as_bytes()).to_bytes();
        assert_eq!(signature.len(), 64);
        let public = key.verifying_key().to_bytes();
        assert!(verify_with(&public, &sha, &signature).is_ok());
        // Another artifact's hash, a flipped bit, and the release key all fail.
        assert!(verify_with(&public, &"d".repeat(64), &signature).is_err());
        let mut flipped = signature;
        flipped[10] ^= 1;
        assert!(verify_with(&public, &sha, &flipped).is_err());
        assert!(verify_signature(&sha, &signature).is_err());
    }

    #[test]
    fn activation_switches_current_without_removing_the_old_version() {
        let root = std::env::temp_dir().join(format!("hex-linux-activate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions/1.0.0")).unwrap();
        fs::write(root.join("versions/1.0.0/wordink"), b"old").unwrap();
        fs::create_dir_all(root.join("versions/0.9.0")).unwrap();
        fs::write(root.join("versions/0.9.0/wordink"), b"older").unwrap();
        symlink("versions/1.0.0", root.join("current")).unwrap();
        let staging = root.join("hex.staging");
        fs::write(&staging, b"new").unwrap();

        let executable = activate(&root, "1.1.0", &staging).unwrap();
        assert_eq!(fs::read(executable).unwrap(), b"new");
        assert_eq!(
            fs::read(root.join("versions/1.0.0/wordink")).unwrap(),
            b"old"
        );
        assert!(!root.join("versions/0.9.0").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replacing_a_staged_update_keeps_the_running_version() {
        let root = std::env::temp_dir().join(format!("hex-linux-restage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let running = root.join("versions").join(crate::VERSION);
        fs::create_dir_all(&running).unwrap();
        fs::write(running.join("wordink"), b"running").unwrap();
        symlink(
            Path::new("versions").join(crate::VERSION),
            root.join("current"),
        )
        .unwrap();
        for (version, bytes) in [("1.1.0", b"first"), ("1.2.0", b"newer")] {
            let staging = root.join("hex.staging");
            fs::write(&staging, bytes).unwrap();
            activate(&root, version, &staging).unwrap();
        }
        assert_eq!(fs::read(root.join("current/wordink")).unwrap(), b"newer");
        assert_eq!(fs::read(running.join("wordink")).unwrap(), b"running");
        let _ = fs::remove_dir_all(root);
    }
}
