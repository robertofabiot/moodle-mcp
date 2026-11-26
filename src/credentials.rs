use crate::config::Config;
use crate::moodle::client::{MoodleClient, DEFAULT_BASE_URL};
use crate::moodle::gate::Priority;
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "moodle-mcp";

/// Índice de los sitios con sesión guardada. Solo contiene URLs, nunca tokens: el keyring no
/// permite enumerar sus entradas, así que este archivo es la única forma de listarlas.
const SESSIONS_INDEX_FILE: &str = "sessions.json";

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
#[derive(Clone)]
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

    pub fn auto_with_file(path: PathBuf) -> Self {
        Self {
            kind: StoreKind::Auto,
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

    /// Carga credenciales para la URL base especificada de forma asíncrona.
    pub async fn load(&self, base_url: &str) -> Result<Option<Credentials>> {
        let this = self.clone();
        let base_url = base_url.to_string();
        tokio::task::spawn_blocking(move || this.load_sync(&base_url))
            .await
            .context("Fallo en tarea spawn_blocking al cargar credenciales")?
    }

    /// Guarda credenciales en el almacén configurado de forma asíncrona.
    pub async fn save(&self, creds: &Credentials) -> Result<()> {
        let this = self.clone();
        let creds = creds.clone();
        tokio::task::spawn_blocking(move || this.save_sync(&creds))
            .await
            .context("Fallo en tarea spawn_blocking al guardar credenciales")?
    }

    /// Elimina credenciales para la URL base dada de forma asíncrona.
    pub async fn delete(&self, base_url: &str) -> Result<()> {
        let this = self.clone();
        let base_url = base_url.to_string();
        tokio::task::spawn_blocking(move || this.delete_sync(&base_url))
            .await
            .context("Fallo en tarea spawn_blocking al borrar credenciales")?
    }

    /// Devuelve las URLs de los sitios con sesión guardada, sin duplicados y ordenadas.
    ///
    /// Las sesiones del keyring guardadas antes de que existiera el índice aparecen después de
    /// usarlas una vez, porque `load` las registra. La de la URL por defecto se busca aquí
    /// mientras no haya índice.
    pub async fn saved_sites(&self) -> Vec<String> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.saved_sites_sync())
            .await
            .unwrap_or_default()
    }

    fn saved_sites_sync(&self) -> Vec<String> {
        let index_exists = self.index_path().is_ok_and(|path| path.exists());
        if !index_exists && self.kind != StoreKind::File {
            let account = Self::account_key(DEFAULT_BASE_URL);
            if let Ok(Some(creds)) = self.load_keyring(&account) {
                self.register_site(&creds.base_url);
            }
        }

        let mut sites = self.read_index();
        if let Ok(map) = self.read_file_map() {
            sites.extend(map.into_values().map(|creds| creds.base_url));
        }
        let mut seen = std::collections::HashSet::new();
        sites.retain(|site| seen.insert(Self::account_key(site)));
        sites.sort();
        sites
    }

    fn load_sync(&self, base_url: &str) -> Result<Option<Credentials>> {
        let account = Self::account_key(base_url);

        let creds = match self.kind {
            StoreKind::Keyring => self.load_keyring(&account),
            StoreKind::File => self.load_file(&account),
            StoreKind::Auto => match self.load_keyring(&account) {
                Ok(Some(creds)) => Ok(Some(creds)),
                Ok(None) => self.load_file(&account),
                Err(_) => self.load_file(&account),
            },
        }?;
        if let Some(ref found) = creds {
            self.register_site(&found.base_url);
        }
        Ok(creds)
    }

    fn save_sync(&self, creds: &Credentials) -> Result<()> {
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
        }?;
        self.register_site(&creds.base_url);
        Ok(())
    }

    fn delete_sync(&self, base_url: &str) -> Result<()> {
        let account = Self::account_key(base_url);
        self.unregister_site(&account);

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
        let json_bytes = serde_json::to_vec_pretty(map)?;
        write_private(&self.file_path()?, &json_bytes)
    }

    fn index_path(&self) -> Result<PathBuf> {
        Ok(self.file_path()?.with_file_name(SESSIONS_INDEX_FILE))
    }

    /// Lee el índice de sitios. Un índice ausente o dañado cuenta como vacío.
    fn read_index(&self) -> Vec<String> {
        self.index_path()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write_index(&self, sites: &[String]) -> Result<()> {
        let json_bytes = serde_json::to_vec_pretty(sites)?;
        write_private(&self.index_path()?, &json_bytes)
    }

    /// Agrega el sitio al índice si falta. Un fallo solo se registra: el índice es una ayuda y
    /// la sesión ya quedó guardada.
    fn register_site(&self, base_url: &str) {
        let account = Self::account_key(base_url);
        let mut sites = self.read_index();
        if sites.iter().any(|site| Self::account_key(site) == account) {
            return;
        }
        sites.push(base_url.to_string());
        if let Err(e) = self.write_index(&sites) {
            tracing::warn!("No se pudo actualizar el índice de sesiones: {e}");
        }
    }

    fn unregister_site(&self, account: &str) {
        let mut sites = self.read_index();
        let before = sites.len();
        sites.retain(|site| Self::account_key(site) != account);
        if sites.len() != before {
            if let Err(e) = self.write_index(&sites) {
                tracing::warn!("No se pudo actualizar el índice de sesiones: {e}");
            }
        }
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

/// Escribe un archivo con permisos `0600`, creando su carpeta con `0700` si falta.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
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

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

/// Sugerencia para cuando no hay sesión guardada para `base_url`. Si hay sesiones para otras
/// URLs, las nombra: lo habitual es que la URL no coincida, por ejemplo por omitir la ruta.
pub fn no_session_hint(base_url: &str, saved_sites: &[String]) -> String {
    let account = CredentialStore::account_key(base_url);
    let others: Vec<&str> = saved_sites
        .iter()
        .filter(|site| CredentialStore::account_key(site) != account)
        .map(String::as_str)
        .collect();
    let login = format!("'moodle-mcp --base-url {base_url} login' en una terminal");
    if others.is_empty() {
        format!("Ejecuta {login}")
    } else {
        format!(
            "Hay sesión guardada para {}. Usa esa URL exacta, con su ruta, en --base-url o MOODLE_BASE_URL, o ejecuta {login}",
            others.join(", ")
        )
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

    store.load(&config.base_url).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_keyring_load_nonexistent_account_no_panic() {
        let store = CredentialStore::new(StoreKind::Keyring);
        // Debe retornar Ok(None) o Err(_) sin causar pánico en runtime multi-hilo
        let res = store.load("http://127.0.0.1:9/test-probe").await;
        assert!(res.is_ok() || res.is_err());
    }

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

    #[tokio::test]
    async fn test_file_store_lifecycle_and_permissions() {
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
        store.save(&creds).await.unwrap();

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
            .await
            .unwrap()
            .expect("Debería encontrar credenciales guardadas");
        assert_eq!(loaded, creds);

        // Borrar
        store
            .delete("https://uamvirtual.uam.edu.ni/grado")
            .await
            .unwrap();
        let after_delete = store
            .load("https://uamvirtual.uam.edu.ni/grado")
            .await
            .unwrap();
        assert!(after_delete.is_none());
    }

    fn creds_for(base_url: &str) -> Credentials {
        Credentials {
            base_url: base_url.into(),
            token: "tok".into(),
            private_token: None,
            userid: 1,
            username: "u".into(),
            fullname: "Alumno".into(),
        }
    }

    #[tokio::test]
    async fn test_saved_sites_follows_save_and_delete() {
        let dir = tempdir().unwrap();
        let store = CredentialStore::with_file(dir.path().join("credentials.json"));
        assert!(store.saved_sites().await.is_empty());

        store
            .save(&creds_for("https://moodle.b.edu/grado"))
            .await
            .unwrap();
        store
            .save(&creds_for("https://moodle.a.edu"))
            .await
            .unwrap();
        store
            .save(&creds_for("https://moodle.b.edu/grado"))
            .await
            .unwrap();
        assert_eq!(
            store.saved_sites().await,
            vec!["https://moodle.a.edu", "https://moodle.b.edu/grado"]
        );

        let index = std::fs::read_to_string(dir.path().join(SESSIONS_INDEX_FILE)).unwrap();
        assert!(!index.contains("tok"), "el índice no debe guardar tokens");

        store.delete("https://moodle.b.edu/grado").await.unwrap();
        assert_eq!(store.saved_sites().await, vec!["https://moodle.a.edu"]);
    }

    #[tokio::test]
    async fn test_load_registers_site_missing_from_index() {
        let dir = tempdir().unwrap();
        let store = CredentialStore::with_file(dir.path().join("credentials.json"));
        store
            .save(&creds_for("https://moodle.a.edu"))
            .await
            .unwrap();
        std::fs::remove_file(dir.path().join(SESSIONS_INDEX_FILE)).unwrap();

        store.load("https://moodle.a.edu").await.unwrap().unwrap();
        let index: Vec<String> =
            serde_json::from_slice(&std::fs::read(dir.path().join(SESSIONS_INDEX_FILE)).unwrap())
                .unwrap();
        assert_eq!(index, vec!["https://moodle.a.edu"]);
    }

    #[test]
    fn test_no_session_hint_without_other_sites() {
        assert_eq!(
            no_session_hint(
                "https://moodle.a.edu",
                &["https://moodle.a.edu/".to_string()]
            ),
            "Ejecuta 'moodle-mcp --base-url https://moodle.a.edu login' en una terminal"
        );
    }

    #[test]
    fn test_no_session_hint_names_other_sites() {
        let hint = no_session_hint(
            "https://uamvirtual.uam.edu.ni",
            &["https://uamvirtual.uam.edu.ni/grado".to_string()],
        );
        assert!(hint.starts_with("Hay sesión guardada para https://uamvirtual.uam.edu.ni/grado."));
        assert!(hint.ends_with(
            "ejecuta 'moodle-mcp --base-url https://uamvirtual.uam.edu.ni login' en una terminal"
        ));
    }
}
