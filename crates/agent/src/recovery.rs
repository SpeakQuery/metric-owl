//! Deliberate local root recovery. There is no remote reset endpoint.
use anyhow::{Context, Result, ensure};
use guard_core::store::Store;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path},
};

fn require_root(uid: u32) -> Result<()> {
    ensure!(
        uid == 0,
        "Account recovery requires root privileges on the monitored server"
    );
    Ok(())
}

fn existing_database(dir: &Path) -> Result<std::fs::Metadata> {
    ensure!(
        dir.is_absolute()
            && !dir
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
        "Use an absolute, existing Metric Owl data directory"
    );
    for ancestor in dir.ancestors() {
        ensure!(
            !std::fs::symlink_metadata(ancestor)?
                .file_type()
                .is_symlink(),
            "Recovery refuses symbolic links in the data directory path"
        );
    }
    ensure!(dir.is_dir(), "Metric Owl data directory does not exist");
    for name in [
        "guard.sqlite",
        "guard.sqlite-wal",
        "guard.sqlite-shm",
        "guard.sock",
        "master.key",
        "setup-key",
    ] {
        let path = dir.join(name);
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            ensure!(
                !metadata.file_type().is_symlink() && metadata.nlink() <= 1,
                "Recovery refuses linked application files"
            );
        }
    }
    let metadata = std::fs::metadata(dir.join("guard.sqlite"))
        .context("Existing Metric Owl database not found")?;
    ensure!(
        metadata.is_file(),
        "Existing Metric Owl database is not a regular file"
    );
    let directory = std::fs::metadata(dir)?;
    ensure!(
        directory.uid() == metadata.uid()
            && directory.mode() & 0o022 == 0
            && metadata.mode() & 0o022 == 0,
        "Database/directory ownership or write permissions require operator review before recovery"
    );
    let vault = std::fs::metadata(dir.join("master.key")).context(
        "Existing vault key is missing; preserve and review the installation before recovery",
    )?;
    ensure!(
        vault.is_file() && vault.len() == 32,
        "Existing vault key is invalid; it was not replaced"
    );
    Ok(metadata)
}

fn username(dir: &Path) -> Result<String> {
    let db = rusqlite::Connection::open_with_flags(
        dir.join("guard.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let raw: String = db
        .query_row("SELECT value FROM kv WHERE key='admin'", [], |r| r.get(0))
        .context("No administrator exists; use normal first-account setup")?;
    let data: serde_json::Value = serde_json::from_str(&raw)?;
    let name = data["username"]
        .as_str()
        .context("Administrator record is invalid")?;
    ensure!(
        (3..=64).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || "._-@".contains(c)),
        "Administrator username is invalid"
    );
    Ok(name.into())
}

struct HiddenTerminal {
    terminal: tokio::io::unix::AsyncFd<File>,
    original: libc::termios,
}
impl HiddenTerminal {
    fn open() -> Result<Self> {
        let terminal = OpenOptions::new().read(true).write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open("/dev/tty").context("Recovery needs your interactive SSH terminal; passwords cannot be supplied through stdin, arguments or environment variables")?;
        Self::from_file(terminal)
    }
    fn from_file(terminal: File) -> Result<Self> {
        let fd = terminal.as_raw_fd();
        // fd is a live controlling terminal, and the initialized result is checked.
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        ensure!(
            unsafe { libc::tcgetattr(fd, &mut original) } == 0,
            "Cannot secure terminal input"
        );
        // This File exclusively owns a stable descriptor for the registration lifetime.
        let terminal = unsafe { tokio::io::unix::AsyncFd::register(terminal) }?;
        let mut hidden = original;
        hidden.c_lflag &= !(libc::ECHO | libc::ECHONL);
        ensure!(
            unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } == 0,
            "Cannot disable password echo"
        );
        Ok(Self { terminal, original })
    }
    async fn password(&self, prompt: &str) -> Result<String> {
        let mut output = self.terminal.get_ref();
        output.write_all(prompt.as_bytes())?;
        output.flush()?;
        let mut bytes = Vec::new();
        loop {
            let count = tokio::select! {
                _ = tokio::signal::ctrl_c() => anyhow::bail!("Recovery cancelled; no password was changed"),
                ready = self.terminal.readable() => {
                    let mut ready = ready?;
                    let mut buffer = [0u8; 257];
                    match ready.try_io(|inner| inner.get_ref().read(&mut buffer)) {
                        Ok(result) => { let count = result?; bytes.extend_from_slice(&buffer[..count]); count },
                        Err(_) => continue,
                    }
                }
            };
            ensure!(count > 0, "Recovery cancelled; no password was changed");
            ensure!(bytes.len() <= 258, "Password exceeds the 256-byte limit");
            if bytes.ends_with(b"\n") {
                output.write_all(b"\n")?;
                bytes.pop();
                if bytes.ends_with(b"\r") {
                    bytes.pop();
                }
                return String::from_utf8(bytes).context("Password must be valid UTF-8");
            }
        }
    }
}
impl Drop for HiddenTerminal {
    fn drop(&mut self) {
        restore_terminal(self.terminal.get_ref().as_raw_fd(), &self.original);
    }
}
fn restore_terminal(fd: RawFd, original: &libc::termios) {
    // The owned File remains open until this Drop completes; also discard queued input.
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, original);
        libc::tcflush(fd, libc::TCIFLUSH);
    }
}

pub async fn run(dir: &Path, show_username: bool) -> Result<()> {
    // No file creation or database mutation occurs before this authorization check.
    require_root(unsafe { libc::geteuid() })?;
    let metadata = existing_database(dir)?;
    if show_username {
        drop_to_owner(&metadata)?;
        println!("Administrator username: {}", username(dir)?);
        return Ok(());
    }
    // Open the controlling terminal while root can still access it. The owned
    // descriptor remains usable after permanently dropping to the service user.
    let terminal = HiddenTerminal::open()?;
    drop_to_owner(&metadata)?;
    // DATA belongs to the service user. Never chmod/chown a pathname there as
    // root: another process of that UID could replace it with a hostile link.
    let _offline = crate::mcp::bind_ipc(dir)
        .await
        .context("Stop server-guard before account recovery, then start it again afterwards")?;
    println!("Administrator username: {}", username(dir)?);
    println!(
        "Recovery resets the password, signs out every browser, and disables existing two-factor authentication and recovery codes. Monitoring data and integration keys are retained."
    );
    let password = terminal
        .password("New password (8 characters minimum, 256 bytes maximum): ")
        .await?;
    let confirmation = terminal.password("Enter the new password again: ").await?;
    ensure!(
        crate::auth::constant(&password, &confirmation),
        "Passwords did not match; nothing was changed"
    );
    drop(terminal);
    let store = Store::open(&dir.join("guard.sqlite"))?;
    crate::auth::recover_local_admin(&store, password).await?;
    println!(
        "Password reset. All browser sessions and previous MFA/recovery codes are invalid. Start server-guard, sign in, and enable two-factor authentication again in Security & system if desired."
    );
    Ok(())
}

fn drop_to_owner(metadata: &std::fs::Metadata) -> Result<()> {
    // Even a READ_ONLY SQLite connection can create WAL/SHM sidecars. Every
    // SQLite access, including username lookup, runs as the database owner.
    if metadata.uid() != 0 {
        ensure!(
            unsafe { libc::setgroups(0, std::ptr::null()) } == 0
                && unsafe { libc::setgid(metadata.gid()) } == 0
                && unsafe { libc::setuid(metadata.uid()) } == 0,
            "Cannot switch to the database owner; nothing was changed"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_and_path_checks_precede_writes() {
        assert!(require_root(1).is_err());
        assert!(require_root(0).is_ok());
        assert!(existing_database(Path::new(".local-data")).is_err());
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        std::os::unix::fs::symlink("/tmp", directory.join("guard.sqlite")).unwrap();
        assert!(existing_database(&directory).is_err());
    }
    #[tokio::test]
    async fn recovery_preserves_monitoring_and_secrets_but_revokes_authentication() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("guard.sqlite")).unwrap();
        let auth = crate::auth::Auth::new(store.clone(), temp.path())
            .await
            .unwrap();
        auth.prepare_admin("owner".into(), "old-password".into(), temp.path())
            .await
            .unwrap();
        let (old_token, _) = auth
            .login("owner".into(), "old-password".into(), "".into())
            .await
            .unwrap();
        store
            .put("secret:ai", "preserved-encrypted-secret".into())
            .await
            .unwrap();
        store.put("host_id", "preserved-host".into()).await.unwrap();
        store
            .put("pending_mfa", "pending-secret".into())
            .await
            .unwrap();
        store.put("auth_attempts", "[1,2,3]".into()).await.unwrap();
        store
            .write(|db| {
                let raw: String =
                    db.query_row("SELECT value FROM kv WHERE key='admin'", [], |r| r.get(0))?;
                let mut admin: serde_json::Value = serde_json::from_str(&raw)?;
                admin["mfa_enabled"] = serde_json::json!(true);
                admin["secret"] = serde_json::json!("old-mfa");
                admin["recovery"] = serde_json::json!(["old-code-hash"]);
                db.execute(
                    "UPDATE kv SET value=?1 WHERE key='admin'",
                    [admin.to_string()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            crate::auth::recover_local_admin(&store, "short".into())
                .await
                .is_err()
        );
        crate::auth::recover_local_admin(&store, "new-private-password".into())
            .await
            .unwrap();
        assert_eq!(username(temp.path()).unwrap(), "owner");
        assert!(auth.session(&old_token, false).await.is_err());
        assert!(!auth.mfa_required().await.unwrap());
        assert!(
            auth.login("owner".into(), "old-password".into(), "".into())
                .await
                .is_err()
        );
        assert!(
            auth.login("owner".into(), "new-private-password".into(), "".into())
                .await
                .is_ok()
        );
        assert_eq!(
            store.get("secret:ai").await.unwrap().as_deref(),
            Some("preserved-encrypted-secret")
        );
        assert_eq!(
            store.get("host_id").await.unwrap().as_deref(),
            Some("preserved-host")
        );
        assert!(store.get("pending_mfa").await.unwrap().is_none());
        let raw = store.get("admin").await.unwrap().unwrap();
        assert!(
            !raw.contains("new-private-password")
                && !raw.contains("old-mfa")
                && !raw.contains("old-code-hash")
        );
    }
    #[tokio::test]
    async fn hidden_terminal_does_not_echo_password_and_restores_echo_after_cancel() {
        use std::os::fd::FromRawFd;
        let (mut master, mut slave) = (-1, -1);
        // Disposable PTY descriptors are exclusively owned by the Files below.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let mut master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let spy = slave.try_clone().unwrap();
        for file in [&master, &slave] {
            let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
            assert_eq!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
                0
            );
        }
        let terminal = HiddenTerminal::from_file(slave).unwrap();
        let (password, ()) = tokio::join!(terminal.password("Hidden test password: "), async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            master.write_all(b"private-pty-password\n").unwrap();
        });
        assert_eq!(password.unwrap(), "private-pty-password");
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                terminal.password("Cancelled input: ")
            )
            .await
            .is_err()
        );
        drop(terminal);
        let mut restored = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(spy.as_raw_fd(), &mut restored) },
            0
        );
        assert_ne!(restored.c_lflag & libc::ECHO, 0);
        let mut transcript = [0u8; 1024];
        let count = master.read(&mut transcript).unwrap();
        assert!(!String::from_utf8_lossy(&transcript[..count]).contains("private-pty-password"));
    }
}
