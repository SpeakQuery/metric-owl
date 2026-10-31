use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, OsRng as AeadRng, rand_core::RngCore},
};
use anyhow::{Result, ensure};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use guard_core::store::Store;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use totp_rs::{Algorithm, Secret, TOTP};
#[derive(Clone)]
pub struct Auth {
    pub store: Store,
    vault: Arc<[u8; 32]>,
    lock: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Serialize, Deserialize)]
struct Admin {
    username: String,
    password: String,
    secret: String,
    last_step: u64,
    recovery: Vec<String>,
    #[serde(default = "legacy_mfa_enabled")]
    mfa_enabled: bool,
}
#[derive(Serialize, Deserialize)]
struct Bootstrap {
    hash: String,
    expires: f64,
}
#[derive(Serialize, Deserialize)]
struct InstallerEnrollment {
    username: String,
    password: String,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    username: String,
    password: String,
    secret: String,
    expires: f64,
}
#[derive(Serialize)]
pub struct Session {
    pub csrf: String,
    pub created: f64,
    pub last: f64,
}
pub fn token() -> String {
    let mut b = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut b);
    hex::encode(b)
}
pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
impl Auth {
    pub async fn new(store: Store, dir: &Path) -> Result<Self> {
        let path = dir.join("master.key");
        if !path.exists() {
            let mut b = [0; 32];
            AeadRng.fill_bytes(&mut b);
            private_file(&path, &b)?;
        }
        let bytes = std::fs::read(path)?;
        let vault: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid key file"))?;
        store
            .write(|db| {
                let tx = db.transaction()?;
                let migrated: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM kv WHERE key='browser_session_v1')",
                    [],
                    |row| row.get(0),
                )?;
                if !migrated {
                    // Do not revive tokens that expired under the previous policy.
                    // Valid existing logins continue; later startups do not time them out.
                    let now = guard_core::now();
                    tx.execute(
                        "DELETE FROM sessions WHERE created<=?1 OR last<=?2",
                        params![now - 43200., now - 1800.],
                    )?;
                    tx.execute("INSERT INTO kv VALUES('browser_session_v1','1')", [])?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(Self {
            store,
            vault: Arc::new(vault),
            lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }
    pub fn seal(&self, value: &str) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&*self.vault).unwrap();
        let mut nonce = [0; 12];
        AeadRng.fill_bytes(&mut nonce);
        let encrypted = cipher
            .encrypt((&nonce).into(), value.as_bytes())
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;
        Ok(format!("{}:{}", hex::encode(nonce), hex::encode(encrypted)))
    }
    pub fn unseal(&self, value: &str) -> Result<String> {
        let (n, c) = value
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid encrypted value"))?;
        let n = hex::decode(n)?;
        ensure!(n.len() == 12, "Invalid nonce");
        let cipher = Aes256Gcm::new_from_slice(&*self.vault).unwrap();
        let bytes = cipher
            .decrypt(n.as_slice().into(), hex::decode(c)?.as_ref())
            .map_err(|_| anyhow::anyhow!("Decryption failed"))?;
        Ok(String::from_utf8(bytes)?)
    }
    pub async fn bootstrap(&self, dir: &Path) -> Result<()> {
        if self.store.get("admin").await?.is_some() {
            return Ok(());
        }
        let file = dir.join("setup-key");
        if self.store.get("installer_pending").await?.is_some() {
            let _ = std::fs::remove_file(&file);
            return Ok(());
        }
        let existing = self
            .store
            .get("bootstrap")
            .await?
            .and_then(|s| serde_json::from_str::<Bootstrap>(&s).ok());
        if existing.is_some_and(|b| b.expires > guard_core::now()) && file.exists() {
            return Ok(());
        }
        let key = token();
        if file.exists() {
            std::fs::remove_file(&file)?;
        }
        private_file(&file, key.as_bytes())?;
        self.store
            .put(
                "bootstrap",
                serde_json::to_string(&Bootstrap {
                    hash: hash(&key),
                    expires: guard_core::now() + 1800.,
                })?,
            )
            .await?;
        eprintln!("Setup key (valid for 30 minutes): {}", file.display());
        Ok(())
    }
    pub async fn configured(&self) -> Result<bool> {
        Ok(self.store.get("admin").await?.is_some())
    }
    pub async fn mfa_required(&self) -> Result<bool> {
        self.store
            .get("admin")
            .await?
            .map(|raw| serde_json::from_str::<Admin>(&raw).map(|admin| admin.mfa_enabled))
            .transpose()
            .map(|value| value.unwrap_or(false))
            .map_err(Into::into)
    }
    pub async fn enrollment_ready(&self) -> Result<bool> {
        Ok(!self.configured().await? && self.store.get("installer_pending").await?.is_some())
    }
    /// Local installer-only account creation. Credentials arrive through bounded
    /// stdin. Existing administrators can never be replaced by this command.
    pub async fn prepare_admin(
        &self,
        username: String,
        password: String,
        dir: &Path,
    ) -> Result<()> {
        let _lock = self.lock.lock().await;
        ensure!(!self.configured().await?, "An administrator already exists");
        validate_username(&username)?;
        validate_password(&password)?;
        let pending = self.store.get("installer_pending").await?;
        if let Some(raw) = &pending {
            let enrollment: InstallerEnrollment = serde_json::from_str(&self.unseal(raw)?)?;
            ensure!(
                verify_password(password.clone(), enrollment.password).await?
                    && constant(&username, &enrollment.username),
                "Enrollment already exists with different credentials"
            );
        }
        let encoded = tokio::task::spawn_blocking(move || password_hash(&password)).await??;
        let admin = serde_json::to_string(&Admin {
            username,
            password: encoded,
            secret: String::new(),
            last_step: 0,
            recovery: Vec::new(),
            mfa_enabled: false,
        })?;
        self.store.write(move |c| {
            let tx = c.transaction()?;
            ensure!(tx.query_row("SELECT COUNT(*) FROM kv WHERE key='admin'", [], |r|r.get::<_,i64>(0))? == 0, "An administrator already exists");
            let current_pending: Option<String> = tx.query_row("SELECT value FROM kv WHERE key='installer_pending'", [], |r|r.get(0)).optional()?;
            ensure!(current_pending == pending, "Enrollment changed; run the local installer again");
            tx.execute("INSERT INTO kv VALUES('admin',?1)", [admin])?;
            tx.execute("DELETE FROM kv WHERE key IN ('bootstrap','pending_admin','installer_pending','pending_mfa')", [])?;
            tx.execute("DELETE FROM sessions", [])?;
            tx.commit()?;
            Ok(())
        }).await?;
        let _ = std::fs::remove_file(dir.join("setup-key"));
        Ok(())
    }
    pub async fn enroll(&self, username: String, password: String) -> Result<serde_json::Value> {
        let _lock = self.lock.lock().await;
        self.rate_limit().await?;
        ensure!(
            !self.configured().await? && username.len() <= 64 && password.len() <= 256,
            "Invalid credentials"
        );
        let enrolled = self
            .store
            .get("installer_pending")
            .await?
            .ok_or_else(|| anyhow::anyhow!("Invalid credentials"))?;
        let enrollment: InstallerEnrollment = serde_json::from_str(&self.unseal(&enrolled)?)?;
        let valid = verify_password(password, enrollment.password.clone()).await?;
        ensure!(
            constant(&username, &enrollment.username) && valid,
            "Invalid credentials"
        );
        let secret = Secret::generate_secret().to_encoded().to_string();
        let uri = make_totp(&secret, &enrollment.username)?.get_url();
        let key = token();
        let expires = guard_core::now() + 600.;
        let pending = self.seal(&serde_json::to_string(&Pending {
            username: enrollment.username,
            password: enrollment.password,
            secret: secret.clone(),
            expires,
        })?)?;
        let bootstrap = serde_json::to_string(&Bootstrap {
            hash: hash(&key),
            expires,
        })?;
        self.store.write(move |c| {
            let tx = c.transaction()?;
            ensure!(tx.query_row("SELECT COUNT(*) FROM kv WHERE key='admin'", [], |r|r.get::<_,i64>(0))? == 0, "Setup is already complete");
            let current: Option<String> = tx.query_row("SELECT value FROM kv WHERE key='installer_pending'", [], |r|r.get(0)).optional()?;
            ensure!(current.as_deref() == Some(enrolled.as_str()), "Enrollment has changed; sign in again");
            for (name,value) in [("bootstrap",bootstrap),("pending_admin",pending)] {
                tx.execute("INSERT INTO kv VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![name,value])?;
            }
            tx.commit()?;
            Ok(())
        }).await?;
        Ok(serde_json::json!({"secret":secret,"uri":uri,"key":key}))
    }
    async fn check_bootstrap(&self, key: &str) -> Result<()> {
        ensure!(!self.configured().await?, "Setup is already complete");
        let raw = self
            .store
            .get("bootstrap")
            .await?
            .ok_or_else(|| anyhow::anyhow!("Setup key not found"))?;
        let b: Bootstrap = serde_json::from_str(&raw)?;
        ensure!(
            b.expires > guard_core::now() && constant(&hash(key), &b.hash),
            "Invalid or expired setup key"
        );
        Ok(())
    }
    pub async fn begin(
        &self,
        key: String,
        username: String,
        password: String,
    ) -> Result<serde_json::Value> {
        let _lock = self.lock.lock().await;
        self.rate_limit().await?;
        ensure!(
            self.store.get("installer_pending").await?.is_none(),
            "Use your installer credentials to finish setup"
        );
        self.check_bootstrap(&key).await?;
        validate_username(&username)?;
        validate_password(&password)?;
        let encoded = tokio::task::spawn_blocking(move || password_hash(&password)).await??;
        let secret = Secret::generate_secret().to_encoded().to_string();
        let totp = make_totp(&secret, &username)?;
        let pending = Pending {
            username,
            password: encoded,
            secret: secret.clone(),
            expires: guard_core::now() + 600.,
        };
        let sealed = self.seal(&serde_json::to_string(&pending)?)?;
        let expected_key = hash(&key);
        self.store.write(move |c| {
            let tx = c.transaction()?;
            ensure!(tx.query_row("SELECT COUNT(*) FROM kv WHERE key IN ('admin','installer_pending')", [], |r|r.get::<_,i64>(0))? == 0, "Setup mode has changed");
            valid_bootstrap_in_transaction(&tx, &expected_key)?;
            tx.execute("INSERT INTO kv VALUES('pending_admin',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [sealed])?;
            tx.commit()?;
            Ok(())
        }).await?;
        Ok(serde_json::json!({"secret":secret,"uri":totp.get_url()}))
    }
    pub async fn finish(&self, key: String, code: String, dir: &Path) -> Result<Vec<String>> {
        let _lock = self.lock.lock().await;
        self.rate_limit().await?;
        self.check_bootstrap(&key).await?;
        let expected_pending = self
            .store
            .get("pending_admin")
            .await?
            .ok_or_else(|| anyhow::anyhow!("Setup has not been started"))?;
        let p: Pending = serde_json::from_str(&self.unseal(&expected_pending)?)?;
        let expected_key = hash(&key);
        ensure!(p.expires > guard_core::now(), "Setup has expired");
        let step = valid_step(&make_totp(&p.secret, &p.username)?, &code, 0)?;
        let codes: Vec<_> = (0..10).map(|_| token()[..20].to_string()).collect();
        let a = Admin {
            username: p.username,
            password: p.password,
            secret: self.seal(&p.secret)?,
            last_step: step,
            recovery: codes.iter().map(|c| hash(c)).collect(),
            mfa_enabled: true,
        };
        let json = serde_json::to_string(&a)?;
        self.store
            .write(move |c| {
                let tx = c.transaction()?;
                ensure!(
                    tx.query_row("SELECT COUNT(*) FROM kv WHERE key='admin'", [], |r| r
                        .get::<_, i64>(0))?
                        == 0,
                    "Setup is already complete"
                );
                valid_bootstrap_in_transaction(&tx, &expected_key)?;
                let current_pending: Option<String> = tx
                    .query_row("SELECT value FROM kv WHERE key='pending_admin'", [], |r| {
                        r.get(0)
                    })
                    .optional()?;
                ensure!(
                    current_pending.as_deref() == Some(expected_pending.as_str()),
                    "Setup changed; start again"
                );
                tx.execute("INSERT INTO kv VALUES('admin',?1)", [json])?;
                tx.execute(
                    "DELETE FROM kv WHERE key IN ('bootstrap','pending_admin','installer_pending')",
                    [],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        let _ = std::fs::remove_file(dir.join("setup-key"));
        Ok(codes)
    }
    async fn rate_limit(&self) -> Result<()> {
        let now = guard_core::now();
        self.store.write(move|c|{let raw:Option<String>=c.query_row("SELECT value FROM kv WHERE key='auth_attempts'",[],|r|r.get(0)).optional()?;let mut attempts:Vec<f64>=raw.and_then(|x|serde_json::from_str(&x).ok()).unwrap_or_default();attempts.retain(|x|now-*x<300.);ensure!(attempts.len()<10,"Too many attempts; wait 5 minutes");attempts.push(now);c.execute("INSERT INTO kv VALUES('auth_attempts',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&attempts)?])?;Ok(())}).await
    }
    async fn credentials(&self, username: String, password: String, code: String) -> Result<Admin> {
        ensure!(
            password.len() <= 256 && code.len() <= 128,
            "Invalid credentials"
        );
        self.rate_limit().await?;
        let raw = self
            .store
            .get("admin")
            .await?
            .ok_or_else(|| anyhow::anyhow!("Invalid credentials"))?;
        let mut a: Admin = serde_json::from_str(&raw)?;
        let expected = a.password.clone();
        let valid = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&expected).is_ok_and(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
        })
        .await?;
        ensure!(
            constant(&username, &a.username) && valid,
            "Invalid credentials"
        );
        if a.mfa_enabled {
            if let Some(i) = a.recovery.iter().position(|h| constant(h, &hash(&code))) {
                a.recovery.remove(i);
            } else {
                a.last_step = valid_step(
                    &make_totp(&self.unseal(&a.secret)?, &a.username)?,
                    &code,
                    a.last_step,
                )?;
            }
            self.store.put("admin", serde_json::to_string(&a)?).await?;
        }
        Ok(a)
    }
    pub async fn login(
        &self,
        username: String,
        password: String,
        code: String,
    ) -> Result<(String, Session)> {
        let _lock = self.lock.lock().await;
        self.credentials(username, password, code).await?;
        let session = token();
        let hashed = hash(&session);
        let csrf = token();
        let c = csrf.clone();
        let now = guard_core::now();
        self.store
            .write(move |db| {
                db.execute(
                    "INSERT INTO sessions VALUES(?1,?2,?2,?3,?2)",
                    params![hashed, now, c],
                )?;
                Ok(())
            })
            .await?;
        Ok((
            session,
            Session {
                csrf,
                created: now,
                last: now,
            },
        ))
    }
    pub async fn session(&self, token: &str, touch: bool) -> Result<Session> {
        let h = hash(token);
        let now = guard_core::now();
        self.store
            .write(move |c| {
                let s = c
                    .query_row(
                        "SELECT csrf,created,last FROM sessions WHERE hash=?1",
                        [&h],
                        |r| {
                            Ok(Session {
                                csrf: r.get(0)?,
                                created: r.get(1)?,
                                last: r.get(2)?,
                            })
                        },
                    )
                    .optional()?
                    .ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
                // The browser owns the session lifetime through a non-persistent
                // cookie. Reloads and long-running tabs must not expire a login.
                if touch {
                    c.execute("UPDATE sessions SET last=?1 WHERE hash=?2", params![now, h])?;
                }
                Ok(s)
            })
            .await
    }
    pub async fn logout(&self, token: &str) -> Result<()> {
        let h = hash(token);
        self.store
            .write(move |c| {
                c.execute("DELETE FROM sessions WHERE hash=?1", [h])?;
                Ok(())
            })
            .await
    }
    pub async fn require_session(&self, token: &str) -> Result<()> {
        self.session(token, false).await?;
        Ok(())
    }
    pub async fn begin_mfa_change(&self, session: &str) -> Result<serde_json::Value> {
        let _lock = self.lock.lock().await;
        self.require_session(session).await?;
        let a: Admin = serde_json::from_str(
            &self
                .store
                .get("admin")
                .await?
                .ok_or_else(|| anyhow::anyhow!("Administrator not found"))?,
        )?;
        let secret = Secret::generate_secret().to_encoded().to_string();
        let totp = make_totp(&secret, &a.username)?;
        let pending = serde_json::json!({"session":hash(session),"secret":secret,"expires":guard_core::now()+600.});
        self.store
            .put("pending_mfa", self.seal(&pending.to_string())?)
            .await?;
        Ok(serde_json::json!({"secret":secret,"uri":totp.get_url()}))
    }
    pub async fn finish_mfa_change(&self, session: &str, code: &str) -> Result<Vec<String>> {
        let _lock = self.lock.lock().await;
        self.require_session(session).await?;
        self.rate_limit().await?;
        let pending: serde_json::Value = serde_json::from_str(
            &self.unseal(
                &self
                    .store
                    .get("pending_mfa")
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("MFA rotation has not been started"))?,
            )?,
        )?;
        ensure!(
            pending["session"].as_str() == Some(hash(session).as_str())
                && pending["expires"].as_f64().unwrap_or(0.) > guard_core::now(),
            "MFA rotation expired or the session changed"
        );
        let mut a: Admin =
            serde_json::from_str(&self.store.get("admin").await?.unwrap_or_default())?;
        let secret = pending["secret"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid MFA data"))?;
        a.last_step = valid_step(&make_totp(secret, &a.username)?, code, 0)?;
        a.secret = self.seal(secret)?;
        a.mfa_enabled = true;
        let codes: Vec<_> = (0..10).map(|_| token()[..20].to_owned()).collect();
        a.recovery = codes.iter().map(|s| hash(s)).collect();
        let json = serde_json::to_string(&a)?;
        let session_hash = hash(session);
        self.store
            .write(move |c| {
                let tx = c.transaction()?;
                valid_session_in_transaction(&tx, &session_hash)?;
                tx.execute("UPDATE kv SET value=?1 WHERE key='admin'", [json])?;
                tx.execute("DELETE FROM kv WHERE key='pending_mfa'", [])?;
                tx.execute("DELETE FROM sessions", [])?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(codes)
    }
    pub async fn disable_mfa(&self, session: &str) -> Result<()> {
        let _lock = self.lock.lock().await;
        self.require_session(session).await?;
        let session_hash = hash(session);
        self.store
            .write(move |c| {
                let tx = c.transaction()?;
                valid_session_in_transaction(&tx, &session_hash)?;
                let raw: String =
                    tx.query_row("SELECT value FROM kv WHERE key='admin'", [], |r| r.get(0))?;
                let mut admin: Admin = serde_json::from_str(&raw)?;
                admin.mfa_enabled = false;
                admin.secret.clear();
                admin.last_step = 0;
                admin.recovery.clear();
                tx.execute(
                    "UPDATE kv SET value=?1 WHERE key='admin'",
                    [serde_json::to_string(&admin)?],
                )?;
                tx.execute("DELETE FROM kv WHERE key='pending_mfa'", [])?;
                tx.execute("DELETE FROM sessions", [])?;
                tx.commit()?;
                Ok(())
            })
            .await
    }
    pub async fn change_password(&self, session: &str, password: String) -> Result<()> {
        validate_password(&password)?;
        let _lock = self.lock.lock().await;
        self.require_session(session).await?;
        let p = tokio::task::spawn_blocking(move || password_hash(&password)).await??;
        let mut a: Admin =
            serde_json::from_str(&self.store.get("admin").await?.unwrap_or_default())?;
        a.password = p;
        let json = serde_json::to_string(&a)?;
        let session_hash = hash(session);
        self.store
            .write(move |c| {
                let tx = c.transaction()?;
                valid_session_in_transaction(&tx, &session_hash)?;
                tx.execute("UPDATE kv SET value=?1 WHERE key='admin'", [json])?;
                tx.execute("DELETE FROM sessions", [])?;
                tx.commit()?;
                Ok(())
            })
            .await
    }
}
fn validate_password(p: &str) -> Result<()> {
    ensure!(
        p.chars().count() >= 8 && p.len() <= 256,
        "Password must contain at least 8 characters and at most 256 bytes"
    );
    Ok(())
}
fn password_hash(p: &str) -> Result<String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    Ok(Argon2::default()
        .hash_password(p.as_bytes(), &salt)
        .map_err(|_| anyhow::anyhow!("Unable to process password"))?
        .to_string())
}

/// Local offline recovery only; deliberately separate from authenticated HTTP methods.
/// The CLI checks root ownership and holds the stopped agent's IPC path first.
pub(crate) async fn recover_local_admin(store: &Store, password: String) -> Result<()> {
    validate_password(&password)?;
    let encoded = tokio::task::spawn_blocking(move || password_hash(&password)).await??;
    store.write(move |db| {
        let tx = db.transaction()?;
        let raw: String = tx.query_row("SELECT value FROM kv WHERE key='admin'", [], |r| r.get(0))?;
        let mut admin: Admin = serde_json::from_str(&raw)?;
        validate_username(&admin.username)?;
        admin.password = encoded;
        admin.mfa_enabled = false;
        admin.secret.clear();
        admin.last_step = 0;
        admin.recovery.clear();
        tx.execute("UPDATE kv SET value=?1 WHERE key='admin'", [serde_json::to_string(&admin)?])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.execute("DELETE FROM kv WHERE key IN ('bootstrap','pending_admin','installer_pending','pending_mfa','auth_attempts')", [])?;
        tx.execute("INSERT INTO audit(at,actor,action) VALUES(?1,'local-root','Administrator password recovered; sessions and MFA reset')", [guard_core::now()])?;
        tx.commit()?;
        Ok(())
    }).await
}

pub fn make_totp(secret: &str, user: &str) -> Result<TOTP> {
    Ok(TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret.into()).to_bytes()?,
        Some("Metric Owl".into()),
        user.into(),
    )?)
}
fn valid_step(t: &TOTP, code: &str, last: u64) -> Result<u64> {
    let now = guard_core::now() as u64 / 30;
    for step in now.saturating_sub(1)..=now + 1 {
        if step > last && constant(&t.generate(step * 30), code) {
            return Ok(step);
        }
    }
    anyhow::bail!("Invalid or previously used verification code")
}
fn legacy_mfa_enabled() -> bool {
    true
}
fn valid_session_in_transaction(tx: &rusqlite::Transaction<'_>, session_hash: &str) -> Result<()> {
    // Recheck inside the mutation transaction so a concurrent logout/revocation
    // cannot commit an account change using a previously validated session.
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE hash=?1)",
        [session_hash],
        |r| r.get(0),
    )?;
    ensure!(exists, "Authentication required");
    Ok(())
}

fn validate_username(username: &str) -> Result<()> {
    ensure!(
        (3..=64).contains(&username.len())
            && username
                .chars()
                .all(|c| c.is_alphanumeric() || "._-@".contains(c)),
        "Invalid username"
    );
    Ok(())
}
async fn verify_password(password: String, expected: String) -> Result<bool> {
    Ok(tokio::task::spawn_blocking(move || {
        PasswordHash::new(&expected).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await?)
}
fn valid_bootstrap_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    expected_hash: &str,
) -> Result<()> {
    let raw: Option<String> = tx
        .query_row("SELECT value FROM kv WHERE key='bootstrap'", [], |r| {
            r.get(0)
        })
        .optional()?;
    let bootstrap: Bootstrap =
        serde_json::from_str(&raw.ok_or_else(|| anyhow::anyhow!("Setup key not found"))?)?;
    ensure!(
        bootstrap.expires > guard_core::now() && constant(&bootstrap.hash, expected_hash),
        "Invalid or expired setup key"
    );
    Ok(())
}

pub fn constant(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_minimum_counts_characters_without_complexity_requirements() {
        for short in ["abcdefg", "1234567", "ééééééé", "🦉🦉🦉🦉🦉🦉🦉"] {
            assert!(validate_password(short).is_err());
        }
        for valid in ["abcdefgh", "12345678", "éééééééé", "🦉🦉🦉🦉🦉🦉🦉🦉"]
        {
            assert!(validate_password(valid).is_ok());
        }
    }
    #[test]
    fn password_maximum_remains_256_utf8_bytes() {
        for (valid, invalid) in [
            ("a".repeat(256), "a".repeat(257)),
            ("é".repeat(128), "é".repeat(129)),
            ("🦉".repeat(64), "🦉".repeat(65)),
        ] {
            assert_eq!(valid.len(), 256);
            assert!(validate_password(&valid).is_ok());
            assert!(validate_password(&invalid).is_err());
        }
    }
    #[test]
    fn totp_replay_rejected() {
        let t = make_totp(&Secret::generate_secret().to_encoded().to_string(), "admin").unwrap();
        let step = guard_core::now() as u64 / 30;
        let code = t.generate(step * 30);
        assert!(valid_step(&t, &code, step - 1).is_ok());
        assert!(valid_step(&t, &code, step).is_err());
    }
    #[tokio::test]
    async fn vault_and_browser_session_revocation() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        let a = Auth::new(s.clone(), d.path()).await.unwrap();
        let sealed = a.seal("secret").unwrap();
        assert!(!sealed.contains("secret"));
        assert_eq!(a.unseal(&sealed).unwrap(), "secret");
        let h = hash("session");
        s.write(move |c| {
            c.execute("INSERT INTO sessions VALUES(?1,0,0,'csrf',0)", [h])?;
            Ok(())
        })
        .await
        .unwrap();
        // Even an old session survives history retention and page reload lookups.
        s.retain(guard_core::now()).await.unwrap();
        assert_eq!(a.session("session", false).await.unwrap().csrf, "csrf");
        a.require_session("session").await.unwrap();
        a.logout("session").await.unwrap();
        assert!(a.session("session", true).await.is_err());
    }
    #[tokio::test]
    async fn browser_session_upgrade_discards_expired_tokens_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        store
            .write(|db| {
                let now = guard_core::now();
                for (token, created, last) in [
                    ("active", now - 600., now - 60.),
                    ("idle-expired", now - 3600., now - 1801.),
                    ("lifetime-expired", now - 43201., now - 60.),
                ] {
                    db.execute(
                        "INSERT INTO sessions VALUES(?1,?2,?3,'csrf',0)",
                        params![hash(token), created, last],
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();
        let auth = Auth::new(store.clone(), dir.path()).await.unwrap();
        assert!(auth.session("active", false).await.is_ok());
        assert!(auth.session("idle-expired", false).await.is_err());
        assert!(auth.session("lifetime-expired", false).await.is_err());
        store
            .write(|db| {
                db.execute("UPDATE sessions SET created=0,last=0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        let reopened = Auth::new(store, dir.path()).await.unwrap();
        assert!(reopened.session("active", false).await.is_ok());
    }
}
#[cfg(test)]
mod revocation_regression {
    use super::*;
    #[tokio::test]
    async fn revoked_session_cannot_commit_pending_password_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        let auth = Auth::new(store.clone(), dir.path()).await.unwrap();
        let token = "test-session";
        let h = hash(token);
        store
            .write(move |c| {
                let now = guard_core::now();
                c.execute(
                    "INSERT INTO sessions VALUES(?1,?2,?2,'csrf',?2)",
                    params![h, now],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let lock = auth.lock.lock().await;
        let clone = auth.clone();
        let task = tokio::spawn(async move {
            clone
                .change_password(token, "new-long-password-2026".into())
                .await
        });
        auth.logout(token).await.unwrap();
        drop(lock);
        assert!(task.await.unwrap().is_err());
    }
}

#[cfg(test)]
mod installer_account_tests {
    use super::*;
    const PASSWORD: &str = "installer-password-long-2026";
    async fn fixture() -> (tempfile::TempDir, Store, Auth) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("guard.sqlite")).unwrap();
        let auth = Auth::new(store.clone(), dir.path()).await.unwrap();
        (dir, store, auth)
    }
    async fn old_pending(store: &Store, auth: &Auth) {
        let encoded = password_hash(PASSWORD).unwrap();
        let raw = auth
            .seal(
                &serde_json::to_string(&InstallerEnrollment {
                    username: "admin".into(),
                    password: encoded,
                })
                .unwrap(),
            )
            .unwrap();
        store.put("installer_pending", raw).await.unwrap();
    }
    #[tokio::test]
    async fn eight_character_password_works_for_installer_login_and_password_change() {
        let (dir, store, auth) = fixture().await;
        assert!(
            auth.prepare_admin("admin".into(), "abcdefg".into(), dir.path())
                .await
                .is_err()
        );
        assert!(!auth.configured().await.unwrap());
        auth.prepare_admin("admin".into(), "abcdefgh".into(), dir.path())
            .await
            .unwrap();
        let (session, _) = auth
            .login("admin".into(), "abcdefgh".into(), String::new())
            .await
            .unwrap();
        let before = store.get("admin").await.unwrap();
        assert!(
            auth.change_password(&session, "abcdefg".into())
                .await
                .is_err()
        );
        assert_eq!(store.get("admin").await.unwrap(), before);
        assert!(auth.session(&session, false).await.is_ok());
        auth.change_password(&session, "ijklmnop".into())
            .await
            .unwrap();
        assert!(auth.session(&session, false).await.is_err());
        assert!(
            auth.login("admin".into(), "abcdefgh".into(), String::new())
                .await
                .is_err()
        );
        assert!(
            auth.login("admin".into(), "ijklmnop".into(), String::new())
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn manual_setup_uses_the_same_eight_character_password_policy() {
        let (dir, store, auth) = fixture().await;
        auth.bootstrap(dir.path()).await.unwrap();
        let key = std::fs::read_to_string(dir.path().join("setup-key")).unwrap();
        assert!(
            auth.begin(key.clone(), "admin".into(), "abcdefg".into())
                .await
                .is_err()
        );
        assert!(store.get("pending_admin").await.unwrap().is_none());
        let result = auth
            .begin(key, "admin".into(), "abcdefgh".into())
            .await
            .unwrap();
        assert!(result["uri"].as_str().unwrap().starts_with("otpauth://"));
        let pending: Pending = serde_json::from_str(
            &auth
                .unseal(&store.get("pending_admin").await.unwrap().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(
            verify_password("abcdefgh".into(), pending.password)
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn installer_creates_password_only_admin_and_cannot_overwrite_it() {
        let (dir, store, auth) = fixture().await;
        auth.bootstrap(dir.path()).await.unwrap();
        auth.prepare_admin("admin".into(), PASSWORD.into(), dir.path())
            .await
            .unwrap();
        assert!(auth.configured().await.unwrap());
        assert!(!auth.mfa_required().await.unwrap());
        assert!(!auth.enrollment_ready().await.unwrap());
        assert!(!dir.path().join("setup-key").exists());
        let raw = store.get("admin").await.unwrap().unwrap();
        assert!(!raw.contains(PASSWORD));
        let admin: Admin = serde_json::from_str(&raw).unwrap();
        assert!(admin.password.starts_with("$argon2id$"));
        assert!(admin.secret.is_empty() && admin.recovery.is_empty());
        assert!(
            auth.prepare_admin("other".into(), PASSWORD.into(), dir.path())
                .await
                .is_err()
        );
        assert_eq!(store.get("admin").await.unwrap(), Some(raw));
        assert!(
            auth.login("admin".into(), "wrong".into(), String::new())
                .await
                .is_err()
        );
        let (token, _) = auth
            .login("admin".into(), PASSWORD.into(), String::new())
            .await
            .unwrap();
        assert!(auth.session(&token, false).await.is_ok());
        auth.bootstrap(dir.path()).await.unwrap();
        assert!(!dir.path().join("setup-key").exists());
        let reopened = Auth::new(store, dir.path()).await.unwrap();
        assert!(!reopened.mfa_required().await.unwrap());
    }
    #[tokio::test]
    async fn optional_mfa_enable_disable_use_signed_in_session_and_revoke_sessions() {
        let (dir, store, auth) = fixture().await;
        auth.prepare_admin("admin".into(), PASSWORD.into(), dir.path())
            .await
            .unwrap();
        let (token, _) = auth
            .login("admin".into(), PASSWORD.into(), String::new())
            .await
            .unwrap();
        let challenge = auth.begin_mfa_change(&token).await.unwrap();
        let code = make_totp(challenge["secret"].as_str().unwrap(), "admin")
            .unwrap()
            .generate_current()
            .unwrap();
        let recovery = auth.finish_mfa_change(&token, &code).await.unwrap();
        assert!(auth.mfa_required().await.unwrap());
        assert!(auth.session(&token, false).await.is_err());
        assert!(
            auth.login("admin".into(), PASSWORD.into(), String::new())
                .await
                .is_err()
        );
        let (protected, _) = auth
            .login("admin".into(), PASSWORD.into(), recovery[0].clone())
            .await
            .unwrap();
        store
            .write(|c| {
                c.execute("UPDATE sessions SET step_up=0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(auth.disable_mfa("missing-session").await.is_err());
        // The initial MFA login is sufficient for this entire browser session.
        auth.disable_mfa(&protected).await.unwrap();
        assert!(auth.session(&protected, false).await.is_err());
        assert!(!auth.mfa_required().await.unwrap());
        let admin: Admin =
            serde_json::from_str(&store.get("admin").await.unwrap().unwrap()).unwrap();
        assert!(admin.secret.is_empty() && admin.recovery.is_empty() && admin.last_step == 0);
        assert!(store.get("pending_mfa").await.unwrap().is_none());
        assert!(
            auth.login("admin".into(), PASSWORD.into(), String::new())
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn legacy_admin_without_flag_keeps_required_mfa() {
        let (dir, store, auth) = fixture().await;
        auth.bootstrap(dir.path()).await.unwrap();
        let key = std::fs::read_to_string(dir.path().join("setup-key")).unwrap();
        let challenge = auth
            .begin(key.clone(), "admin".into(), PASSWORD.into())
            .await
            .unwrap();
        let code = make_totp(challenge["secret"].as_str().unwrap(), "admin")
            .unwrap()
            .generate_current()
            .unwrap();
        let codes = auth.finish(key, code, dir.path()).await.unwrap();
        let mut raw: serde_json::Value =
            serde_json::from_str(&store.get("admin").await.unwrap().unwrap()).unwrap();
        raw.as_object_mut().unwrap().remove("mfa_enabled");
        store.put("admin", raw.to_string()).await.unwrap();
        let reopened = Auth::new(store.clone(), dir.path()).await.unwrap();
        assert!(reopened.mfa_required().await.unwrap());
        assert!(
            reopened
                .login("admin".into(), PASSWORD.into(), String::new())
                .await
                .is_err()
        );
        assert!(
            reopened
                .prepare_admin("admin".into(), PASSWORD.into(), dir.path())
                .await
                .is_err()
        );
        assert!(
            reopened
                .login("admin".into(), PASSWORD.into(), codes[0].clone())
                .await
                .is_ok()
        );
        assert!(reopened.mfa_required().await.unwrap());
    }
    #[tokio::test]
    async fn old_installer_pending_can_finish_its_existing_mfa_flow() {
        let (dir, store, auth) = fixture().await;
        old_pending(&store, &auth).await;
        auth.bootstrap(dir.path()).await.unwrap();
        assert!(!dir.path().join("setup-key").exists());
        assert!(auth.enrollment_ready().await.unwrap());
        let wrong_password = auth
            .enroll("admin".into(), "wrong".into())
            .await
            .unwrap_err()
            .to_string();
        let wrong_username = auth
            .enroll("unknown".into(), PASSWORD.into())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(wrong_password, "Invalid credentials");
        assert_eq!(wrong_password, wrong_username);
        let first = auth.enroll("admin".into(), PASSWORD.into()).await.unwrap();
        let mut bootstrap: Bootstrap =
            serde_json::from_str(&store.get("bootstrap").await.unwrap().unwrap()).unwrap();
        bootstrap.expires = 0.;
        store
            .put("bootstrap", serde_json::to_string(&bootstrap).unwrap())
            .await
            .unwrap();
        let first_code = make_totp(first["secret"].as_str().unwrap(), "admin")
            .unwrap()
            .generate_current()
            .unwrap();
        assert!(
            auth.finish(
                first["key"].as_str().unwrap().into(),
                first_code,
                dir.path()
            )
            .await
            .is_err()
        );
        let current = auth.enroll("admin".into(), PASSWORD.into()).await.unwrap();
        assert_ne!(first["key"], current["key"]);
        let code = make_totp(current["secret"].as_str().unwrap(), "admin")
            .unwrap()
            .generate_current()
            .unwrap();
        auth.finish(current["key"].as_str().unwrap().into(), code, dir.path())
            .await
            .unwrap();
        for key in ["bootstrap", "pending_admin", "installer_pending"] {
            assert!(store.get(key).await.unwrap().is_none());
        }
        assert!(auth.mfa_required().await.unwrap());
    }
    #[tokio::test]
    async fn password_only_login_keeps_persistent_attempt_limit() {
        let (dir, store, auth) = fixture().await;
        auth.prepare_admin("admin".into(), PASSWORD.into(), dir.path())
            .await
            .unwrap();
        for _ in 0..10 {
            assert!(
                auth.login("admin".into(), "wrong".into(), String::new())
                    .await
                    .is_err()
            );
        }
        let reopened = Auth::new(store, dir.path()).await.unwrap();
        assert!(
            reopened
                .login("admin".into(), PASSWORD.into(), String::new())
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("Too many attempts")
        );
    }
}
