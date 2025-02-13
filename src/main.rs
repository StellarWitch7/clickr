use actix_web::{get, web::{self, Data, Payload}, App, Error, HttpRequest, HttpResponse, HttpServer};
use clap::{command, Parser, Subcommand, ValueEnum};
use env_logger::Target;
use log::{error, info, LevelFilter};

#[derive(Debug, Parser)]
#[command(name = "clickr")]
#[command(about = "A P2P clicker", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Ping,
    Connect(Args),
    Host(Args)
}

#[derive(clap::Args, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, help = "The host address")]
    addr: String,
    #[arg(short, long, default_value_t = 63063, help = "The host port")]
    port: u16,
}

#[actix_web::main]
async fn main() {
    env_logger::Builder::from_default_env()
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .init();

    match run().await {
        Err(message) => error!("Server failure: {message}"),
        _ => ()
    }
}

async fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Ping => {
            //TODO: connect to ~/.config/clickr/sock as a unix socket
            //send a single byte
            //disconnect and terminate
            Ok(())
        },
        Command::Connect(args) => {
            //TODO: connect to ws://{args.addr}:{args.port}/heart
            //stay running
            //if disconnected, attempt to reconnect
            //when receiving a message through the websocket, play the audio file at
            //~/.config/clickr/sound using pw-cat
            //use awc::ws for client websocket
            Ok(())
        },
        Command::Host(args) => {
            let Args { addr, port } = args.clone();
            let server = HttpServer::new(move || {
                App::new()
                    .app_data(web::Data::new(args.clone()))
                    .service(heart)
            })
                .bind((addr, port))
                .or_else(|e| Err(format!("Failed to bind to address: {e}")))?;

            info!("Server configured, running...");
            server.run().await.or_else(|e| Err(format!("{e}")))
            //TODO: connect to ~/.config/clickr/sock as a unix socket
            //listen for a byte input into the socket, then send a byte message over the currently
            //connected websocket
            //do this on a separate thread
        }
    }
}

#[get("/heart")]
pub async fn heart(req: HttpRequest, stream: Payload, args: Data<Args>) -> Result<HttpResponse, Error> {
    let (res, session, stream) = actix_ws::handle(&req, stream)?;
    //TODO: pass `session` as connection
    //disconnect the old session if there is one. must be thread-safe. separate thread should
    //handle reading from the unix socket and sending pings through the websocket session
    info!("Client connected");
    Ok(res)
}
