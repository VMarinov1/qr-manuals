mod auth;
mod config;
mod db;
mod language;
mod manuals;
mod products;
mod qr;
mod storage;
mod web;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "qr-manuals", version, about = "Упътвания на продукти с QR код")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Стартира уеб сървъра (по подразбиране)
    Serve,
    /// Създава потребител или сменя паролата на съществуващ
    CreateAdmin {
        email: String,
        /// Ако липсва, се пита интерактивно
        #[arg(long, env = "ADMIN_PASSWORD", hide_env_values = true)]
        password: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "qr_manuals=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = config::Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => web::serve(config, pool).await,
        Command::CreateAdmin { email, password } => {
            let password = match password {
                Some(password) => password,
                None => {
                    let password = rpassword::prompt_password("Парола: ")?;
                    if rpassword::prompt_password("Повтори паролата: ")? != password {
                        anyhow::bail!("паролите не съвпадат");
                    }
                    password
                }
            };
            if auth::upsert_user(&pool, &email, &password).await? {
                println!("Създаден потребител {email}");
            } else {
                println!("Сменена парола на {email}");
            }
            Ok(())
        }
    }
}
