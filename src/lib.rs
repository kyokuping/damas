use crate::config::Config;
use crate::error::ErrorRegistry;
use crate::index::IndexCache;
use crate::router::RouterNode;
use std::sync::Arc;

pub mod config;
pub mod error;
pub mod http;
pub mod index;
pub mod response;
pub mod router;
pub mod server;
pub mod util;

#[derive(Clone, Debug)]
pub struct ServerContext {
    config: Arc<Config>,
    router: Arc<RouterNode>,
    error_registry: Arc<ErrorRegistry>,
    index_cache: Arc<IndexCache>,
}

impl ServerContext {
    pub fn new(
        config: Config,
        router: RouterNode,
        registry: ErrorRegistry,
        index_cache: IndexCache,
    ) -> Self {
        Self {
            config: Arc::new(config),
            router: Arc::new(router),
            error_registry: Arc::new(registry),
            index_cache: Arc::new(index_cache),
        }
    }
}
