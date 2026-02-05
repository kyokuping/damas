use crate::Config;
use std::path::Path;

#[derive(Debug)]
pub struct RouterNode {
    path: String,
    children: Vec<RouterNode>,
    handler: Option<String>,
}

impl Default for RouterNode {
    fn default() -> Self {
        Self::new("", None)
    }
}

impl RouterNode {
    fn new(path: &str, handler: Option<String>) -> Self {
        Self {
            path: path.to_string(),
            children: Vec::with_capacity(0),
            handler,
        }
    }

    fn insert(&mut self, full_path: &str, handler: Option<String>) {
        for child in self.children.iter_mut() {
            let Some(edge_char) = child.path.chars().next() else {
                continue;
            };
            if !full_path.starts_with(edge_char) {
                continue;
            }

            let common = calculate_common_prefix_len(&child.path, full_path);
            if common < child.path.len() {
                child.split_child(common);
            }

            let remaining_path = &full_path[common..];

            if remaining_path.is_empty() {
                child.handler = handler;
            } else {
                child.insert(remaining_path, handler);
            }
            return;
        }
        self.children.push(RouterNode::new(full_path, handler));
    }

    fn split_child(&mut self, at: usize) {
        let suffix = self.path[at..].to_string();

        let new_child = RouterNode {
            path: suffix,
            children: std::mem::take(&mut self.children),
            handler: self.handler.take(),
        };

        self.path.truncate(at);
        self.children.push(new_child);
    }

    pub fn search(&self, query_path: &str) -> Option<&String> {
        for child in &self.children {
            if query_path.starts_with(&child.path) {
                if query_path.len() == child.path.len() {
                    return child.handler.as_ref();
                }
                return child.search(&query_path[child.path.len()..]);
            }
        }
        None
    }

    pub fn from_config(config: &Config) -> anyhow::Result<Self, anyhow::Error> {
        let mut router = RouterNode::default();
        for loc in config.server.locations.iter() {
            let path = loc.path.to_string_lossy();
            let root = loc.root.to_string_lossy();

            if let Some(first_idx) = &loc.index.first() {
                router.insert(
                    &path,
                    Path::new(root.as_ref())
                        .join(first_idx)
                        .to_str()
                        .map(|s| s.to_string()),
                );
            } else {
                router.insert(
                    &path,
                    Path::new(root.as_ref()).to_str().map(|s| s.to_string()),
                );
            }
            println!("Route registered: {}", path);
        }

        Ok(router)
    }
}

fn calculate_common_prefix_len(first_path: &str, input_path: &str) -> usize {
    let first_path_bytes = first_path.as_bytes();
    let input_path_bytes = input_path.as_bytes();

    let limit_len = std::cmp::min(first_path_bytes.len(), input_path_bytes.len());

    let mut matched_len = 0;
    while matched_len < limit_len && first_path_bytes[matched_len] == input_path_bytes[matched_len]
    {
        matched_len += 1;
    }
    matched_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_insert_and_search() {
        let mut root = RouterNode::default();
        root.insert("/", None);
        root.insert("/home", Some("/www/var/html/home".to_string()));
        root.insert("/about", Some("/www/var/html/about".to_string()));

        assert_eq!(
            root.search("/home"),
            Some(&"/www/var/html/home".to_string())
        );
        assert_eq!(
            root.search("/about"),
            Some(&"/www/var/html/about".to_string())
        );
        assert_eq!(root.search("/"), None);
    }

    #[test]
    fn test_router_split_node_simple() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/home", Some("/www/var/html/home".to_string()));
        root.insert("/homepage", Some("/www/var/html/homepage".to_string()));

        assert_eq!(
            root.search("/home"),
            Some(&"/www/var/html/home".to_string())
        );
        assert_eq!(
            root.search("/homepage"),
            Some(&"/www/var/html/homepage".to_string())
        );
        assert_eq!(root.search("/home/page"), None);
    }

    #[test]
    fn test_router_split_node_complex() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/apple", Some("/www/var/html/apple".to_string()));
        root.insert("/apricot", Some("/www/var/html/apricot".to_string()));
        root.insert("/app", Some("/www/var/html/app".to_string()));

        assert_eq!(
            root.search("/apple"),
            Some(&"/www/var/html/apple".to_string())
        );
        assert_eq!(
            root.search("/apricot"),
            Some(&"/www/var/html/apricot".to_string())
        );
        assert_eq!(root.search("/app"), Some(&"/www/var/html/app".to_string()));
        assert_eq!(root.search("/ap"), None);
    }

    #[test]
    fn test_router_deep_path() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/a/b/c", Some("/www/var/html/a/b/c".to_string()));
        root.insert("/a/b", Some("/www/var/html/a/b".to_string()));
        root.insert("/x/y/z/w", Some("/www/var/html/x/y/z/w".to_string()));

        assert_eq!(
            root.search("/a/b/c"),
            Some(&"/www/var/html/a/b/c".to_string())
        );
        assert_eq!(root.search("/a/b"), Some(&"/www/var/html/a/b".to_string()));
        assert_eq!(
            root.search("/x/y/z/w"),
            Some(&"/www/var/html/x/y/z/w".to_string())
        );
        assert_eq!(root.search("/a"), None);
        assert_eq!(root.search("/a/b/c/d"), None);
    }

    #[test]
    fn test_router_search_not_found() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/home", Some("/www/var/html/home".to_string()));
        root.insert(
            "/users/profile",
            Some("/www/var/html/users/profile".to_string()),
        );

        assert_eq!(root.search("/notfound"), None);
        assert_eq!(root.search("/users"), None);
        assert_eq!(root.search("/users/profile/edit"), None);
    }

    #[test]
    fn test_router_root_path_handler() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html/index.html".to_string()));
        assert_eq!(
            root.search("/"),
            Some(&"/www/var/html/index.html".to_string())
        );

        root.insert("/foo", Some("/www/var/html/foo".to_string()));
        assert_eq!(root.search("/foo"), Some(&"/www/var/html/foo".to_string()));
        assert_eq!(
            root.search("/"),
            Some(&"/www/var/html/index.html".to_string())
        );
    }

    #[test]
    fn test_router_overwrite_handler() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/path", Some("/www/var/html/v1".to_string()));
        assert_eq!(root.search("/path"), Some(&"/www/var/html/v1".to_string()));

        root.insert("/path", Some("/www/var/html/v2".to_string()));
        assert_eq!(root.search("/path"), Some(&"/www/var/html/v2".to_string()));
    }

    #[test]
    fn test_router_common_prefix_split() {
        let mut root = RouterNode::default();
        root.insert("/", Some("/www/var/html".to_string()));
        root.insert("/teams", Some("/www/var/html/teams".to_string()));
        root.insert("/team", Some("/www/var/html/team".to_string()));
        assert_eq!(
            root.search("/team"),
            Some(&"/www/var/html/team".to_string())
        );
        assert_eq!(
            root.search("/teams"),
            Some(&"/www/var/html/teams".to_string())
        );

        let mut root2 = RouterNode::default();
        root2.insert("/", Some("/www/var/html".to_string()));
        root2.insert("/team", Some("/www/var/html/team".to_string()));
        root2.insert("/teams", Some("/www/var/html/teams".to_string()));
        assert_eq!(
            root2.search("/team"),
            Some(&"/www/var/html/team".to_string())
        );
        assert_eq!(
            root2.search("/teams"),
            Some(&"/www/var/html/teams".to_string())
        );
    }
}
