//! The server process: read the configuration, open the database, accept.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use armory_core::store::Store;
use armory_server::http::{self, Response};
use armory_server::{check_token, Changes, Server};

fn main() {
    // `--health` connects to the configured address and asks. It exists
    // because the image is built from a slim base with no curl and no wget,
    // and because a healthcheck that needed the bearer token would mean
    // handing the secret to Docker as well.
    if std::env::args().any(|argument| argument == "--health") {
        std::process::exit(health());
    }

    if let Err(message) = run() {
        eprintln!("armory-server: {message}");
        std::process::exit(1);
    }
}

/// Everything the process needs, from the environment.
///
/// No default for the token, on purpose: a server that invents one starts
/// successfully and protects nothing.
struct Settings {
    address: String,
    token: String,
    data: PathBuf,
}

fn settings() -> Result<Settings, String> {
    let address = std::env::var("ARMORY_ADDR").unwrap_or_else(|_| "0.0.0.0:8084".to_string());
    let token = std::env::var("ARMORY_TOKEN").map_err(|_| "set ARMORY_TOKEN".to_string())?;
    check_token(&token)?;

    let data = std::env::var("ARMORY_DATA")
        .unwrap_or_else(|_| "/var/lib/armory".to_string())
        .into();

    Ok(Settings {
        address,
        token,
        data,
    })
}

fn run() -> Result<(), String> {
    let settings = settings()?;

    std::fs::create_dir_all(&settings.data)
        .map_err(|error| format!("could not make {}: {error}", settings.data.display()))?;
    let path = settings.data.join("armory.db");
    let store = Store::open(&path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;

    // The server's own name in the log. It never writes a row of its own —
    // every change carries the machine that pushed it — but the store wants a
    // name and a client with an empty one would then be handed the server's
    // rows back as if they were somebody else's.
    store
        .set_machine("server")
        .map_err(|error| format!("could not name the store: {error}"))?;

    let listener = TcpListener::bind(&settings.address)
        .map_err(|error| format!("could not listen on {}: {error}", settings.address))?;

    // Printed rather than logged: Container Manager's Log tab is unreliable,
    // and this is the line that says the process got past its configuration.
    println!(
        "armory-server listening on {}, account at {}",
        settings.address,
        path.display()
    );

    let server = Arc::new(Server::new(
        store,
        settings.token,
        Arc::new(Changes::default()),
    ));

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        // A thread per connection. One person's three machines on a timer is a
        // few requests a minute, and most of those threads are parked in
        // `/wait` doing nothing at all — which is what a thread is good at.
        std::thread::spawn(move || serve(&server, stream));
    }

    Ok(())
}

fn serve(server: &Server, mut stream: TcpStream) {
    let response = match http::read_request(&stream) {
        Ok(request) => server.handle(&request),
        Err(error) => Response::text(400, &error.0),
    };

    if let Err(error) = http::write_response(&mut stream, &response) {
        // The client hung up. Worth a line, not worth taking anything down.
        eprintln!("armory-server: could not reply: {error}");
    }
    let _ = stream.flush();
}

/// Ask the running server whether it is up. Zero means yes.
fn health() -> i32 {
    let address = std::env::var("ARMORY_ADDR").unwrap_or_else(|_| "0.0.0.0:8084".to_string());
    // Whatever it binds, it is reachable from inside the container on
    // loopback, and `0.0.0.0` is not an address to connect to.
    let port = address.rsplit(':').next().unwrap_or("8084");

    let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{port}")) else {
        return 1;
    };
    if write!(stream, "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n").is_err() {
        return 1;
    }

    let mut reply = String::new();
    use std::io::Read;
    if stream.read_to_string(&mut reply).is_err() {
        return 1;
    }
    i32::from(!reply.starts_with("HTTP/1.1 200"))
}
