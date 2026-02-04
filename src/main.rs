use compio::net::TcpListener;
use compio::runtime::spawn;
use damas::handle_connection;

#[compio::main]
async fn main() {
    let host = "127.0.0.1";
    let port = 1111;
    let listener = match TcpListener::bind(format!("{}:{}", host, port)).await {
        Ok(listener) => {
            println!("Listening on {}", host);
            listener
        }
        Err(err) => {
            panic!("Failed to bind: {}", err);
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, address)) => {
                println!("Accepted connection from {}", address);
                spawn(async move { handle_connection(stream).await }).detach();
            }
            Err(err) => {
                eprintln!("Failed to accept connection: {}", err);
            }
        }
    }
}
