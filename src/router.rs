use crate::Config;
use crate::config::LocationConfigType;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MatchType {
    Exact,
    Prefix,
}

#[derive(Debug)]
pub struct RouterNode {
    path: String,
    children: Vec<RouterNode>,
    match_type: MatchType,
    handler: Option<String>,
}

impl Default for RouterNode {
    fn default() -> Self {
        Self::new("", MatchType::Exact, None)
    }
}

impl RouterNode {
    fn new(path: &str, match_type: MatchType, handler: Option<String>) -> Self {
        Self {
            path: path.to_string(),
            children: Vec::with_capacity(0),
            match_type,
            handler,
        }
    }

    fn insert(&mut self, full_path: &str, match_type: MatchType, handler: Option<String>) {
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
                child.match_type = match_type;
            } else {
                child.insert(remaining_path, match_type, handler);
            }
            return;
        }
        self.children
            .push(RouterNode::new(full_path, match_type, handler));
    }

    fn split_child(&mut self, at: usize) {
        let suffix = self.path[at..].to_string();

        let new_child = RouterNode {
            path: suffix,
            children: std::mem::take(&mut self.children),
            match_type: self.match_type,
            handler: self.handler.take(),
        };

        self.path.truncate(at);

        self.handler = None;
        self.match_type = MatchType::Prefix;

        self.children.push(new_child);
    }

    pub fn search(&self, query_path: &str) -> Option<&String> {
        for child in &self.children {
            if query_path.starts_with(&child.path) {
                if query_path.len() == child.path.len() {
                    return child.handler.as_ref();
                }
                let remaining_path = &query_path[child.path.len()..];
                if let Some(handler) = child.search(remaining_path) {
                    return Some(handler);
                }

                if child.match_type == MatchType::Prefix && child.handler.is_some() {
                    return child.handler.as_ref();
                } else {
                    println!("{:?}", child);
                }
            }
        }
        None
    }

    pub fn from_config(config: &Config) -> anyhow::Result<Self, anyhow::Error> {
        let mut router = RouterNode::default();
        for loc in config.server.locations.iter() {
            let path = loc.path.to_string_lossy();
            let root = loc.root.to_string_lossy();
            let match_type = match loc.ty {
                Some(LocationConfigType::Exact) => MatchType::Exact,
                Some(LocationConfigType::Prefix) => MatchType::Prefix,
                _ => {
                    return Err(anyhow::anyhow!("Invalid location type: {:?}", loc.ty));
                }
            };
            router.insert(
                &path,
                match_type,
                Path::new(root.as_ref()).to_str().map(|s| s.to_string()),
            );
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
        root.insert("/", MatchType::Prefix, None);
        root.insert(
            "/home",
            MatchType::Prefix,
            Some("/www/var/html/home".to_string()),
        );
        root.insert(
            "/about",
            MatchType::Prefix,
            Some("/www/var/html/about".to_string()),
        );
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
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/home",
            MatchType::Prefix,
            Some("/www/var/html/home".to_string()),
        );
        root.insert(
            "/homepage",
            MatchType::Prefix,
            Some("/www/var/html/homepage".to_string()),
        );
        assert_eq!(
            root.search("/home"),
            Some(&"/www/var/html/home".to_string())
        );
        assert_eq!(
            root.search("/homepage"),
            Some(&"/www/var/html/homepage".to_string())
        );
        assert_eq!(
            root.search("/home/page"),
            Some(&"/www/var/html/home".to_string())
        );
    }

    #[test]
    fn test_router_split_node_complex() {
        let mut root = RouterNode::default();
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/apple",
            MatchType::Prefix,
            Some("/www/var/html/apple".to_string()),
        );
        root.insert(
            "/apricot",
            MatchType::Prefix,
            Some("/www/var/html/apricot".to_string()),
        );
        root.insert(
            "/app",
            MatchType::Prefix,
            Some("/www/var/html/app".to_string()),
        );
        assert_eq!(
            root.search("/apple"),
            Some(&"/www/var/html/apple".to_string())
        );
        assert_eq!(
            root.search("/apricot"),
            Some(&"/www/var/html/apricot".to_string())
        );
        assert_eq!(root.search("/app"), Some(&"/www/var/html/app".to_string()));
        assert_eq!(root.search("/ap"), Some(&"/www/var/html".to_string()));
    }

    #[test]
    fn test_router_deep_path() {
        let mut root = RouterNode::default();
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/a/b/c",
            MatchType::Prefix,
            Some("/www/var/html/a/b/c".to_string()),
        );
        root.insert(
            "/a/b",
            MatchType::Prefix,
            Some("/www/var/html/a/b".to_string()),
        );
        root.insert(
            "/x/y/z/w",
            MatchType::Prefix,
            Some("/www/var/html/x/y/z/w".to_string()),
        );

        assert_eq!(
            root.search("/a/b/c"),
            Some(&"/www/var/html/a/b/c".to_string())
        );
        assert_eq!(root.search("/a/b"), Some(&"/www/var/html/a/b".to_string()));
        assert_eq!(
            root.search("/x/y/z/w"),
            Some(&"/www/var/html/x/y/z/w".to_string())
        );
        assert_eq!(root.search("/a"), Some(&"/www/var/html".to_string()));
        assert_eq!(
            root.search("/a/b/c/d"),
            Some(&"/www/var/html/a/b/c".to_string())
        );
    }

    #[test]
    fn test_router_fallback_to_root() {
        let mut root = RouterNode::default();
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/users",
            MatchType::Prefix,
            Some("/www/var/html/users".to_string()),
        );

        assert_eq!(
            root.search("/unknown/path"),
            Some(&"/www/var/html".to_string())
        );
        assert_eq!(
            root.search("/users"),
            Some(&"/www/var/html/users".to_string())
        );
        assert_eq!(
            root.search("/users/profile/edit"),
            Some(&"/www/var/html/users".to_string())
        );
    }

    #[test]
    fn test_router_root_path_handler() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some("/www/var/html/index.html".to_string()),
        );
        assert_eq!(
            root.search("/"),
            Some(&"/www/var/html/index.html".to_string())
        );

        root.insert(
            "/foo",
            MatchType::Prefix,
            Some("/www/var/html/foo".to_string()),
        );
        assert_eq!(root.search("/foo"), Some(&"/www/var/html/foo".to_string()));
        assert_eq!(
            root.search("/"),
            Some(&"/www/var/html/index.html".to_string())
        );
    }

    #[test]
    fn test_router_overwrite_handler() {
        let mut root = RouterNode::default();
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/path",
            MatchType::Prefix,
            Some("/www/var/html/v1".to_string()),
        );
        assert_eq!(root.search("/path"), Some(&"/www/var/html/v1".to_string()));

        root.insert(
            "/path",
            MatchType::Prefix,
            Some("/www/var/html/v2".to_string()),
        );
        assert_eq!(root.search("/path"), Some(&"/www/var/html/v2".to_string()));
    }

    #[test]
    fn test_router_common_prefix_split() {
        let mut root = RouterNode::default();
        root.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root.insert(
            "/teams",
            MatchType::Exact,
            Some("/www/var/html/teams".to_string()),
        );
        root.insert(
            "/team",
            MatchType::Exact,
            Some("/www/var/html/team".to_string()),
        );
        assert_eq!(
            root.search("/team"),
            Some(&"/www/var/html/team".to_string())
        );
        assert_eq!(
            root.search("/teams"),
            Some(&"/www/var/html/teams".to_string())
        );

        let mut root2 = RouterNode::default();
        root2.insert("/", MatchType::Prefix, Some("/www/var/html".to_string()));
        root2.insert(
            "/team",
            MatchType::Prefix,
            Some("/www/var/html/team".to_string()),
        );
        root2.insert(
            "/teams",
            MatchType::Prefix,
            Some("/www/var/html/teams".to_string()),
        );
        assert_eq!(
            root2.search("/team"),
            Some(&"/www/var/html/team".to_string())
        );
        assert_eq!(
            root2.search("/teams"),
            Some(&"/www/var/html/teams".to_string())
        );
    }

    #[test]
    fn test_router_matchtype_handling() {
        let mut root = RouterNode::default();
        root.insert(
            "/exact",
            MatchType::Exact,
            Some("exact_handler".to_string()),
        );
        root.insert(
            "/prefix",
            MatchType::Prefix,
            Some("prefix_handler".to_string()),
        );

        assert_eq!(root.search("/exact"), Some(&"exact_handler".to_string()));
        assert_eq!(root.search("/exact/subpath"), None);

        assert_eq!(root.search("/prefix"), Some(&"prefix_handler".to_string()));
        assert_eq!(
            root.search("/prefix/subpath"),
            Some(&"prefix_handler".to_string())
        ); // Should match subpath
        assert_eq!(
            root.search("/prefix/another/subpath"),
            Some(&"prefix_handler".to_string())
        ); // Should match deeper subpath

        // Test non-matching paths
        assert_eq!(root.search("/no_match"), None);
        assert_eq!(root.search("/exac"), None); // Partial match, but not prefix for "exact"
        assert_eq!(root.search("/prefi"), None); // Partial match, but not prefix for "prefix"
    }
}
