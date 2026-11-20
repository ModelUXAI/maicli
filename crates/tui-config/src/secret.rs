//! Pencarian kunci API: environment variable dulu, lalu keyring OS.
//!
//! Urutan ini disengaja: env var menang supaya CI/otomasi bisa menimpa tanpa
//! menyentuh Credential Manager, sementara pengguna desktop bisa menyimpan kunci
//! sekali di keyring. Nilai kunci tidak pernah dicetak atau di-log.

/// Nama service di Windows Credential Manager.
pub const KEYRING_SERVICE: &str = "maicli-modeluxai";

/// Penyimpan kunci yang bisa diganti (keyring asli atau fake di test).
pub trait SecretBackend {
    /// Baca kunci untuk `user`.
    fn get(&self, user: &str) -> Option<String>;
    /// Simpan kunci; pesan galat tidak boleh memuat nilainya.
    fn set(&mut self, user: &str, value: &str) -> Result<(), String>;
}

/// Backend keyring OS (Windows Credential Manager di v1).
pub struct KeyringBackend;

impl SecretBackend for KeyringBackend {
    #[cfg(windows)]
    fn get(&self, user: &str) -> Option<String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, user).ok()?;
        non_empty(entry.get_password().ok())
    }

    #[cfg(not(windows))]
    fn get(&self, _user: &str) -> Option<String> {
        None
    }

    #[cfg(windows)]
    fn set(&mut self, user: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, user).map_err(|err| err.to_string())?;
        entry.set_password(value).map_err(|err| err.to_string())
    }

    #[cfg(not(windows))]
    fn set(&mut self, _user: &str, _value: &str) -> Result<(), String> {
        Err("keyring OS tidak didukung di platform ini".to_string())
    }
}

/// Simpan kunci ke keyring lewat backend tertentu; nilai kosong ditolak.
pub fn store_key(
    backend: &mut dyn SecretBackend,
    user: &str,
    value: &str,
) -> Result<(), tui_wire::MxError> {
    if user.trim().is_empty() {
        return Err(tui_wire::MxError::new(
            tui_wire::ErrorCode::Config,
            "nama kunci (env var) kosong".to_string(),
        ));
    }
    if value.trim().is_empty() {
        return Err(tui_wire::MxError::new(
            tui_wire::ErrorCode::Config,
            "kunci kosong tidak disimpan".to_string(),
        ));
    }
    backend.set(user, value).map_err(|detail| {
        tui_wire::MxError::new(
            tui_wire::ErrorCode::Config,
            format!("gagal menyimpan kunci ke keyring: {detail}"),
        )
    })
}

/// Sumber kunci yang terpakai, untuk ditampilkan di `maicli config show`.
///
/// Hanya menyebut sumbernya, tidak pernah nilainya.
pub fn source(env_name: &str) -> &'static str {
    source_from(|name| std::env::var(name).ok(), keyring_lookup, env_name)
}

/// Bentuk yang bisa diuji: sumber ditentukan dari lookup yang disuntikkan.
pub fn source_from(
    env_lookup: impl Fn(&str) -> Option<String>,
    keyring_lookup: impl Fn(&str) -> Option<String>,
    env_name: &str,
) -> &'static str {
    if non_empty(env_lookup(env_name)).is_some() {
        "env"
    } else if non_empty(keyring_lookup(env_name)).is_some() {
        "keyring"
    } else {
        "tidak ada"
    }
}

/// Cari kunci: env var `env_name`, lalu keyring dengan user `env_name`.
pub fn resolve_api_key(env_name: &str) -> Option<String> {
    resolve_with(|name| std::env::var(name).ok(), keyring_lookup, env_name)
}

/// Bentuk yang bisa diuji: lookup env dan keyring disuntik dari luar.
pub fn resolve_with(
    env_lookup: impl Fn(&str) -> Option<String>,
    keyring_lookup: impl Fn(&str) -> Option<String>,
    env_name: &str,
) -> Option<String> {
    non_empty(env_lookup(env_name)).or_else(|| non_empty(keyring_lookup(env_name)))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

#[cfg(windows)]
fn keyring_lookup(name: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name).ok()?;
    non_empty(entry.get_password().ok())
}

#[cfg(not(windows))]
fn keyring_lookup(_name: &str) -> Option<String> {
    // Platform lain tidak dipakai di v1; env var tetap satu-satunya sumber.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        value: Option<String>,
        fail: bool,
    }

    impl SecretBackend for Fake {
        fn get(&self, _user: &str) -> Option<String> {
            self.value.clone()
        }

        fn set(&mut self, _user: &str, value: &str) -> Result<(), String> {
            if self.fail {
                return Err("ditolak".to_string());
            }
            self.value = Some(value.to_string());
            Ok(())
        }
    }

    #[test]
    fn store_menolak_kunci_kosong_dan_meneruskan_galat() {
        let mut fake = Fake {
            value: None,
            fail: false,
        };
        let err = store_key(&mut fake, "K", "   ").expect_err("kosong ditolak");
        assert_eq!(err.code, tui_wire::ErrorCode::Config);
        assert!(fake.value.is_none());

        let mut failing = Fake {
            value: None,
            fail: true,
        };
        let err = store_key(&mut failing, "K", "rahasia").expect_err("backend gagal");
        assert!(
            !err.message.contains("rahasia"),
            "pesan galat tidak boleh memuat nilai kunci: {}",
            err.message
        );
    }

    #[test]
    fn store_menyimpan_lewat_backend() {
        let mut fake = Fake {
            value: None,
            fail: false,
        };
        store_key(&mut fake, "K", "nilai").expect("tersimpan");
        assert_eq!(fake.get("K").as_deref(), Some("nilai"));
    }

    #[test]
    fn env_menang_atas_keyring() {
        let key = resolve_with(
            |_| Some("dari-env".to_string()),
            |_| Some("dari-keyring".to_string()),
            "K",
        );
        assert_eq!(key.as_deref(), Some("dari-env"));
    }

    #[test]
    fn keyring_dipakai_bila_env_kosong_atau_spasi() {
        assert_eq!(
            resolve_with(|_| Some("   ".to_string()), |_| Some("k".to_string()), "K").as_deref(),
            Some("k")
        );
        assert_eq!(
            resolve_with(|_| None, |_| Some("k".to_string()), "K").as_deref(),
            Some("k")
        );
        assert_eq!(resolve_with(|_| None, |_| None, "K"), None);
    }

    #[test]
    fn nama_env_diteruskan_apa_adanya() {
        let seen = std::cell::RefCell::new(Vec::new());
        let _ = resolve_with(
            |name| {
                seen.borrow_mut().push(name.to_string());
                None
            },
            |_| None,
            "MAICLI_UJI_KEY",
        );
        assert_eq!(seen.borrow().as_slice(), ["MAICLI_UJI_KEY"]);
    }
}
