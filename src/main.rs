use compio::net::TcpListener;
use compio::runtime::spawn;
use damas::config::{Config, parse_config};
use damas::router::RouterNode;
use damas::{ServerContext, handle_connection};

#[compio::main]
async fn main() -> anyhow::Result<()> {
    let config = match parse_config("./config.kdl") {
        Ok(c) => c,
        Err(report) => {
            println!("{:?}", report);
            return Err(anyhow::Error::from_boxed(report.into()));
        }
    };
    let config: &'static Config = Box::leak(Box::new(config));
    let host = &config.server.server_name;
    let port = config.server.listen;
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
                let router = RouterNode::from_config(config).unwrap();
                spawn(async move {
                    if let Err(e) = handle_connection(
                        stream,
                        ServerContext {
                            config,
                            router: &router,
                        },
                    )
                    .await
                    {
                        eprintln!("Error handling connection: {}", e);
                    }
                })
                .detach();
            }
            Err(err) => {
                eprintln!("Failed to accept connection: {}", err);
            }
        }
    }
}
