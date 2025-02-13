use std::{io::{Read, Write}, net::Shutdown, os::unix::net::{UnixListener, UnixStream}, process, sync::Mutex, thread::spawn, time::Duration};

use actix_web::{get, rt::time::sleep, web::{self, Data, Payload}, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_ws::Session;
use awc::Client;
use clap::{command, Parser, Subcommand, ValueEnum};
use env_logger::Target;
use futures::{executor::block_on, StreamExt};
use log::{error, info, warn, LevelFilter};

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
            let mut stream = UnixStream::connect("~/.config/clickr/sock")
                .or_else(|e| Err(format!("Failed to open unix socket: {e}")))?;
            stream.write_all(&[0xff])
                .or_else(|e| Err(format!("Failed to write to unix socket: {e}")))?;
            stream.flush()
                .or_else(|e| Err(format!("Failed flush unix socket: {e}")))?;
            stream.shutdown(Shutdown::Both)
                .or_else(|e| Err(format!("Failed release unix socket: {e}")))?;

            Ok(())
        },
        Command::Connect(args) => {
            let client = Client::default();

            loop {
                let (res, mut ws) = client
                    .ws(format!("ws://{}:{}/heart", args.addr, args.port))
                    .connect()
                    .await
                    .or_else(|e| Err(format!("Failed to connect to websocket: {e}")))?;
                info!("Connected! Http response: {res:?}");

                loop {
                    match ws.next().await {
                        Some(msg) => {
                            if let Ok(msg) = msg {
                                info!("Got ping! {msg:?}");
                                process::Command::new("pw-cat")
                                    .arg("-p")
                                    .arg("~/.config/clickr/sound.ogg")
                                    .arg("--volume")
                                    .arg("4")
                                    .output()
                                    .or_else(|e| Err(format!("Failed to execute pw-cat: {e}")))?;
                            }
                        },
                        None => {
                            warn!("Got disconnected! Attempting to reconnect in 5 seconds.");
                            sleep(Duration::from_secs(5)).await;
                            break;
                        }
                    }
                }
            }
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

            info!("Setting up socket listener thread...");
            let listener = UnixListener::bind(shellexpand::tilde("~/.config/clickr/sock"))
                .or_else(|e| Err(format!("Failed to bind unix socket: {e}")))?;
            spawn(move || loop {
                if let Ok((mut stream, _)) = listener.accept() {
                    if let Ok(read_count) = stream.read(&mut [0x00]) {
                        if read_count > 0 {
                            let mut session = get_client_session().lock().unwrap();
                            if let Some(inner_session) = session.as_mut() {
                                if block_on(inner_session.binary(vec![0xff])).is_err() {
                                    *session = None;
                                }
                            }
                        }
                    }
                }
            });

            info!("Server configured, running...");
            server.run().await.or_else(|e| Err(format!("{e}")))
        }
    }
}

#[get("/heart")]
pub async fn heart(req: HttpRequest, stream: Payload, args: Data<Args>) -> Result<HttpResponse, Error> {
    let (res, session, stream) = actix_ws::handle(&req, stream)?;

    let mut current_session = get_client_session().lock().unwrap();
    if current_session.as_ref().is_some() {
        let _ = current_session.take().unwrap().close(None).await;
    }

    *current_session = Some(session);

    info!("Client connected");
    Ok(res)
}

fn get_client_session() -> &'static Mutex<Option<Session>> {
    static SESSION: Mutex<Option<Session>> = Mutex::new(Option::None);
    &SESSION
}
