use std::{fs, io::{Read, Write}, net::Shutdown, os::unix::net::{UnixListener, UnixStream}, process, sync::Mutex, thread::{self, spawn}, time::Duration};

use actix_web::{get, rt::time::{sleep, timeout}, web::{self, Data, Payload}, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_ws::Session;
use awc::Client;
use awc::ws::Frame::Binary;
use clap::{command, Parser, Subcommand, ValueEnum};
use env_logger::Target;
use futures::{executor::block_on, SinkExt, StreamExt};
use log::{error, info, warn, LevelFilter};

#[derive(Debug, Parser)]
#[command(name = "clickr")]
#[command(about = "A peer-to-peer clicker", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Host(ServerArgs),
    Connect(ClientArgs),
    Ping(PingArgs),
}

#[derive(clap::Args, Debug, Clone)]
#[command(about = "Run clickr as the host", long_about = None)]
struct ServerArgs {
    #[arg(short, long, help = "The address to listen on")]
    addr: String,
    #[arg(short, long, default_value_t = 63063, help = "The port to listen on")]
    port: u16,
}

#[derive(clap::Args, Debug, Clone)]
#[command(about = "Run clickr as the client", long_about = None)]
struct ClientArgs {
    #[arg(short, long, help = "The address to connect to")]
    addr: String,
    #[arg(short, long, default_value_t = 63063, help = "The port to connect to")]
    port: u16,
    #[arg(short, long, default_value_t = 2, help = "The volume of the sound when a ping is received")]
    volume: u16,
}

#[derive(clap::Args, Debug, Clone)]
#[command(about = "Ping the currently connected clickr client", long_about = None)]
struct PingArgs {
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
    fs::create_dir_all(shellexpand::tilde("~/.config/clickr").as_ref());

    match Cli::parse().command {
        Command::Ping(_) => {
            let mut stream = UnixStream::connect(shellexpand::tilde("~/.config/clickr/sock").as_ref())
                .or_else(|e| Err(format!("Failed to open unix socket: {e}")))?;
            stream.write_all(&[0xff])
                .or_else(|e| Err(format!("Failed to write to unix socket: {e}")))?;
            stream.flush()
                .or_else(|e| Err(format!("Failed to flush unix socket: {e}")))?;
            stream.shutdown(Shutdown::Both)
                .or_else(|e| Err(format!("Failed to release unix socket: {e}")))?;

            Ok(())
        },
        Command::Connect(args) => {
            let client = Client::default();

            loop {
                match client.ws(format!("ws://{}:{}/heart", args.addr, args.port)).connect().await {
                    Ok((res, mut ws)) => {
                        info!("Connected! HTTP response: {res:?}");

                        loop {
                            match timeout(Duration::from_secs(20), ws.next()).await {
                                Ok(Some(msg)) => {
                                    if let Ok(Binary(msg)) = msg {
                                        if msg[0] == 0xff {
                                            info!("Ping!");
                                            info!("{:?}", process::Command::new("pw-cat")
                                                .arg("-p")
                                                .arg(shellexpand::tilde("~/.config/clickr/sound.ogg").as_ref())
                                                .arg("--volume")
                                                .arg(format!("{}", args.volume))
                                                .output()
                                                .or_else(|e| Err(format!("Failed to execute pw-cat: {e}")))?);
                                        } else {
                                            info!("Ba-bump");
                                        }
                                    }
                                },
                                Ok(None) => {
                                    warn!("Got disconnected! Attempting to reconnect in 5 seconds.");
                                    sleep(Duration::from_secs(5)).await;
                                    break;
                                },
                                Err(_) => {
                                    warn!("Timed out! Attempting to reconnect in 5 seconds.");
                                    sleep(Duration::from_secs(5)).await;
                                    break;
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Failed to connect to websocket: {e}");
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        },
        Command::Host(args) => {
            let _ = fs::remove_file(shellexpand::tilde("~/.config/clickr/sock").as_ref());

            let ServerArgs { addr, port } = args.clone();
            let server = HttpServer::new(move || {
                App::new()
                    .app_data(web::Data::new(args.clone()))
                    .service(heart)
            })
                .bind((addr, port))
                .or_else(|e| Err(format!("Failed to bind to address: {e}")))?;

            info!("Setting up socket listener thread...");
            let listener = UnixListener::bind(shellexpand::tilde("~/.config/clickr/sock").as_ref())
                .or_else(|e| Err(format!("Failed to bind unix socket: {e}")))?;
            spawn(move || loop {
                if let Ok((mut stream, _)) = listener.accept() {
                    if let Ok(read_count) = stream.read(&mut [0x00]) {
                        if read_count > 0 {
                            let mut session = get_client_session().lock().unwrap();
                            if let Some(inner_session) = session.as_mut() {
                                if block_on(inner_session.binary(vec![0xff])).is_err() {
                                    *session = None;
                                    info!("Client disconnected!");
                                } else {
                                    info!("Client pinged");
                                }
                            }
                        }
                    }
                }
            });

            spawn(move || loop {
                thread::sleep(Duration::from_secs(10));

                let mut session = get_client_session().lock().unwrap();
                if let Some(inner_session) = session.as_mut() {
                    if block_on(inner_session.binary(vec![0x00])).is_err() {
                        *session = None;
                        info!("Client disconnected!");
                    }
                }
            });

            info!("Server configured, running...");
            server.run().await.or_else(|e| Err(format!("{e}")))
        }
    }
}

#[get("/heart")]
pub async fn heart(req: HttpRequest, stream: Payload, args: Data<ServerArgs>) -> Result<HttpResponse, Error> {
    info!("Client {} attempting to connect...", req.peer_addr().map_or("<unknown>".to_string(), |addr| addr.ip().to_canonical().to_string()));
    let (res, session, stream) = actix_ws::handle(&req, stream)?;

    let mut current_session = get_client_session().lock().unwrap();
    if current_session.as_ref().is_some() {
        let _ = current_session.take().unwrap().close(None).await;
    }

    *current_session = Some(session);
    info!("Client connected!");
    Ok(res)
}

fn get_client_session() -> &'static Mutex<Option<Session>> {
    static SESSION: Mutex<Option<Session>> = Mutex::new(Option::None);
    &SESSION
}
