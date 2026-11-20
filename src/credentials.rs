use crate::config::Config;
use crate::moodle::client::MoodleClient;
use crate::moodle::gate::Priority;
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "moodle-mcp";

/// Credenciales almacenadas de un usuario en un sitio Moodle.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub base_url: String,
    pub token: String,
    pub private_token: Option<String>,
    pub userid: i64,
    pub username: String,
    pub fullname: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("base_url", &self.base_url)
            .field("token", &"<redactado>")
            .field("private_token", &"<redactado>")
            .field("userid", &self.userid)
            .field("username", &self.username)
            .field("fullname", &self.fullname)
            .finish()
    }
}

/// Tipo de almacén de credenciales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum StoreKind {
    #[default]
    Auto,
    Keyring,
    File,
}

/// Manejador de persistencia segura de credenciales.
pub struct CredentialStore {
    kind: StoreKind,
    custom_file: Option<PathBuf>,
}

impl CredentialStore {
    pub fn new(kind: StoreKind) -> Self {
        Self {
            kind,
            custom_file: None,
        }
    }

    pub fn with_file(path: PathBuf) -> Self {
        Self {
            kind: StoreKind::File,
            custom_file: Some(path),
        }
    }

    fn account_key(base_url: &str) -> String {
        let stripped = base_url
            .strip_prefix("https://")
            .or_else(|| base_url.strip_prefix("http://"))
            .unwrap_or(base_url);
        stripped.trim_end_matches('/').to_string()
    }

    fn file_path(&self) -> Result<PathBuf> {
        if let Some(ref path) = self.custom_file {
            return Ok(path.clone());
        }
        let config_dir = dirs::config_dir()
            .context("No se pudo resolver el directorio de configuración del usuario")?;
        Ok(config_dir.join("moodle-mcp").join("credentials.json"))
    }

    /// Carga credenciales para la URL base especificada.
    pub fn load(&self, base_url: &str) -> Result<Option<Credentials>> {
        let account = Self::account_key(base_url);

        match self.kind {
            StoreKind::Keyring => self.load_keyring(&account),
            StoreKind::File => self.load_file(&account),
            StoreKind::Auto => match self.load_keyring(&account) {
                Ok(Some(creds)) => Ok(Some(creds)),
                Ok(None) => self.load_file(&account),
                Err(_) => self.load_file(&account),
            },
        }
    }

    /// Guarda credenciales en el almacén configurado.
    pub fn save(&self, creds: &Credentials) -> Result<()> {
        let account = Self::account_key(&creds.base_url);
        let serialized = serde_json::to_string(creds)?;

        match self.kind {
            StoreKind::Keyring => self.save_keyring(&account, &serialized),
            StoreKind::File => self.save_file(&account, creds),
            StoreKind::Auto => {
                if let Err(_err) = self.save_keyring(&account, &serialized) {
                    self.save_file(&account, creds)?;
                }
                Ok(())
            }
        }
    }

    /// Elimina credenciales para la URL base dada.
    pub fn delete(&self, base_url: &str) -> Result<()> {
        let account = Self::account_key(base_url);

        match self.kind {
            StoreKind::Keyring => {
                let _ = self.delete_keyring(&account);
                Ok(())
            }
            StoreKind::File => {
                self.delete_file(&account)?;
                Ok(())
            }
            StoreKind::Auto => {
                let _ = self.delete_keyring(&account);
                let _ = self.delete_file(&account);
                Ok(())
            }
        }
    }

    fn load_keyring(&self, account: &str) -> Result<Option<Credentials>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)?;
        match entry.get_password() {
            Ok(json_str) => {
                let creds: Credentials = serde_json::from_str(&json_str)?;
                Ok(Some(creds))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Fallo al acceder al keyring: {e}")),
        }
    }

    fn save_keyring(&self, account: &str, secret: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)?;
        entry
            .set_password(secret)
            .map_err(|e| anyhow::anyhow!("Fallo al escribir en el keyring: {e}"))
    }

    fn delete_keyring(&self, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("Fallo al borrar del keyring: {e}")),
        }
    }

    fn read_file_map(&self) -> Result<HashMap<String, Credentials>> {
        let path = self.file_path()?;
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let mut file = File::open(&path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }
        let map: HashMap<String, Credentials> = serde_json::from_str(&content)?;
        Ok(map)
    }

    fn write_file_map(&self, map: &HashMap<String, Credentials>) -> Result<()> {
        let path = self.file_path()?;
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut builder = std::fs::DirBuilder::new();
                    builder.recursive(true).mode(0o700);
                    builder.create(parent)?;
                }
                #[cfg(not(unix))]
                {
                    std::fs::create_dir_all(parent)?;
                }
            }
        }

        let json_bytes = serde_json::to_vec_pretty(map)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(&json_bytes)?;
            file.flush()?;
        }
        #[cfg(not(unix))]
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            file.write_all(&json_bytes)?;
            file.flush()?;
        }

        Ok(())
    }

    fn load_file(&self, account: &str) -> Result<Option<Credentials>> {
        let map = self.read_file_map()?;
        Ok(map.get(account).cloned())
    }

    fn save_file(&self, account: &str, creds: &Credentials) -> Result<()> {
        let mut map = self.read_file_map().unwrap_or_default();
        map.insert(account.to_string(), creds.clone());
        self.write_file_map(&map)
    }

    fn delete_file(&self, account: &str) -> Result<()> {
        let mut map = match self.read_file_map() {
            Ok(m) => m,
            Err(_) => return Ok(()),
        };
        if map.remove(account).is_some() {
            self.write_file_map(&map)?;
        }
        Ok(())
    }
}

/// Resuelve las credenciales activas respetando el orden de precedencia:
/// 1. Variables de entorno (`MOODLE_TOKEN` y opcional `MOODLE_PRIVATE_TOKEN`).
/// 2. Keyring del sistema.
/// 3. Archivo local de credenciales `credentials.json`.
pub async fn resolve_credentials(
    config: &Config,
    store: &CredentialStore,
    client: &MoodleClient,
) -> Result<Option<Credentials>> {
    if let Ok(token) = std::env::var("MOODLE_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            let private_token = std::env::var("MOODLE_PRIVATE_TOKEN")
                .ok()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty());

            let site_info = client.get_site_info(&token, Priority::Interactive).await?;

            return Ok(Some(Credentials {
                base_url: config.base_url.clone(),
                token,
                private_token,
                userid: site_info.userid,
                username: site_info.username.unwrap_or_else(|| "env_user".to_string()),
                fullname: site_info.fullname,
            }));
        }
    }

    store.load(&config.base_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_credentials_debug_redacts_tokens() {
        let creds = Credentials {
            base_url: "https://uamvirtual.uam.edu.ni/grado".into(),
            token: "supersecrettoken123".into(),
            private_token: Some("private456".into()),
            userid: 10001,
            username: "24000000".into(),
            fullname: "Estudiante Prueba".into(),
        };

        let debug_str = format!("{:?}", creds);
        assert!(!debug_str.contains("supersecrettoken123"));
        assert!(!debug_str.contains("private456"));
        assert!(debug_str.contains("<redactado>"));
        assert!(debug_str.contains("10001"));
        assert!(debug_str.contains("Estudiante Prueba"));
    }

    #[test]
    fn test_file_store_lifecycle_and_permissions() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("sub").join("creds.json");
        let store = CredentialStore::with_file(file_path.clone());

        let creds = Credentials {
            base_url: "https://uamvirtual.uam.edu.ni/grado".into(),
            token: "tok_abc".into(),
            private_token: Some("priv_xyz".into()),
            userid: 42,
            username: "user42".into(),
            fullname: "Alumno 42".into(),
        };

        // Guardar
        store.save(&creds).unwrap();

        // Verificar permisos Unix 0600 en archivo y 0700 en directorio
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_meta = std::fs::metadata(&file_path).unwrap();
            assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);

            let dir_meta = std::fs::metadata(file_path.parent().unwrap()).unwrap();
            assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);
        }

        // Cargar
        let loaded = store
            .load("https://uamvirtual.uam.edu.ni/grado")
            .unwrap()
            .expect("Debería encontrar credenciales guardadas");
        assert_eq!(loaded, creds);

        // Borrar
        store.delete("https://uamvirtual.uam.edu.ni/grado").unwrap();
        let after_delete = store.load("https://uamvirtual.uam.edu.ni/grado").unwrap();
        assert!(after_delete.is_none());
    }
}
