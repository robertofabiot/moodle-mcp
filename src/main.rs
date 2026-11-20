use clap::{Parser, Subcommand};
use moodle_mcp::config::{normalize_base_url, Config};
use moodle_mcp::credentials::{resolve_credentials, CredentialStore, Credentials, StoreKind};
use moodle_mcp::mcp::{AppContext, MoodleMcp};
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::gate::Priority;
use std::io::{self, Write};
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "moodle-mcp",
    version,
    about = "Servidor MCP y cliente para UAM Virtual (Moodle)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// URL base de la instancia Moodle
    #[arg(long, global = true)]
    base_url: Option<String>,

    /// Tipo de almacén de credenciales a utilizar
    #[arg(long, global = true, value_enum, default_value_t = StoreKind::Auto)]
    store: StoreKind,
}

#[derive(Subcommand)]
enum Commands {
    /// Inicia sesión e intercambia credenciales por un wstoken
    Login {
        /// Nombre de usuario o identificador CIF
        #[arg(long)]
        username: Option<String>,
    },
    /// Cierra la sesión activa borrando las credenciales guardadas
    Logout,
    /// Muestra información de la sesión actual y del sitio Moodle
    Whoami,
    /// Arranca el servidor MCP por stdio
    Serve,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut config = Config::from_env();
    if let Some(ref base) = cli.base_url {
        config.base_url = normalize_base_url(base);
    }

    let store = CredentialStore::new(cli.store);

    match cli.command {
        Some(Commands::Login { username }) => {
            let user = match username {
                Some(u) if !u.trim().is_empty() => u.trim().to_string(),
                _ => {
                    print!("Usuario: ");
                    io::stdout().flush()?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    input.trim().to_string()
                }
            };

            let pass_str = rpassword::prompt_password("Contraseña: ")?;
            let pass = Zeroizing::new(pass_str);

            let client = MoodleClient::from_builder(
                MoodleClientBuilder::new().base_url(config.base_url.clone()),
            );

            let login_info = client.login(&user, pass).await?;
            let creds = Credentials {
                base_url: config.base_url.clone(),
                token: login_info.token,
                private_token: login_info.private_token,
                userid: login_info.userid,
                username: user,
                fullname: login_info.fullname,
            };

            store.save(&creds)?;
            println!(
                "Sesión iniciada como {} (id {}) en {}",
                creds.fullname, creds.userid, creds.base_url
            );
        }
        Some(Commands::Logout) => {
            store.delete(&config.base_url)?;
            println!("Sesión cerrada para {}.", config.base_url);
            println!(
                "Aviso: el token sigue siendo válido en Moodle hasta que expire o se revoque desde el perfil (Preferencias -> Claves de seguridad)."
            );
        }
        Some(Commands::Whoami) => {
            let client = MoodleClient::from_builder(
                MoodleClientBuilder::new().base_url(config.base_url.clone()),
            );

            let creds = resolve_credentials(&config, &store, &client).await?;
            let creds = match creds {
                Some(c) => c,
                None => {
                    eprintln!("Error: no hay sesión activa. Ejecuta 'moodle-mcp login' primero.");
                    std::process::exit(1);
                }
            };

            let site_info = client
                .get_site_info(&creds.token, Priority::Interactive)
                .await?;

            let sitename = site_info.sitename.as_deref().unwrap_or("Moodle");
            let release = site_info.release.as_deref().unwrap_or("Desconocido");
            let functions_count = site_info.functions.as_ref().map(|f| f.len()).unwrap_or(0);
            let uname = site_info.username.as_deref().unwrap_or(&creds.username);

            println!("Sitio: {}", sitename);
            println!("Release: {}", release);
            println!(
                "Usuario: {} ({}, id {})",
                site_info.fullname, uname, site_info.userid
            );
            println!("Funciones habilitadas: {}", functions_count);
        }
        Some(Commands::Serve) | None => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "moodle_mcp=info".into()),
                )
                .init();

            let client = MoodleClient::from_builder(
                MoodleClientBuilder::new().base_url(config.base_url.clone()),
            );

            let creds = resolve_credentials(&config, &store, &client)
                .await
                .ok()
                .flatten();

            let ctx = Arc::new(AppContext::new(config, creds, client));
            let server = MoodleMcp::new(ctx);
            server
                .serve(rmcp::transport::io::stdio())
                .await?
                .waiting()
                .await?;
        }
    }

    Ok(())
}
