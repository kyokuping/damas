use damas::config::parse_config;
use damas::server::Server;

#[compio::main]
async fn main() -> anyhow::Result<()> {
    let config = match parse_config("./config.kdl") {
        Ok(c) => c,
        Err(report) => {
            println!("{:?}", report);
            return Err(anyhow::Error::from_boxed(report.into()));
        }
    };
    Server::from_config(config)?.run().await
}
