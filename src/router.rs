use crate::Config;
use crate::config::LocationConfigType;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MatchType {
    Exact,
    Prefix,
}

// Destination metadata
#[derive(Clone, Debug, PartialEq)]
pub struct RouterHandler {
    pub root: Arc<str>,
    pub matched_path: Arc<str>,
    pub index: Arc<[String]>,
    pub match_type: MatchType,
}

impl Default for RouterHandler {
    fn default() -> Self {
        Self {
            root: Arc::from(""),
            matched_path: Arc::from("/"),
            index: Arc::from([]),
            match_type: MatchType::Prefix,
        }
    }
}

impl RouterHandler {
    pub fn new(root: &str, matched_path: &str, index: Vec<String>) -> Self {
        Self {
            root: Arc::from(root),
            matched_path: Arc::from(matched_path),
            index: index.into(),
            match_type: MatchType::Prefix,
        }
    }
    pub fn with_match_type(mut self, match_type: MatchType) -> Self {
        self.match_type = match_type;
        self
    }
}

/// Segment in the Radix Tree
#[derive(Debug, Clone)]
pub struct RouterNode {
    path: String,
    children: Vec<RouterNode>,
    match_type: MatchType,
    handler: Option<RouterHandler>,
}

impl Default for RouterNode {
    fn default() -> Self {
        Self::new("", MatchType::Exact, None)
    }
}

impl RouterNode {
    fn new(path: &str, match_type: MatchType, handler: Option<RouterHandler>) -> Self {
        Self {
            path: path.to_string(),
            children: Vec::with_capacity(0),
            match_type,
            handler,
        }
    }

    fn insert(&mut self, full_path: &str, match_type: MatchType, handler: Option<RouterHandler>) {
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

    pub fn search<'a>(&self, query_path: &'a str) -> Option<(RouterHandler, &'a str)> {
        for child in &self.children {
            if query_path.starts_with(&child.path) {
                let remaining_path = &query_path[child.path.len()..];

                if remaining_path.is_empty()
                    && let Some(h) = &child.handler
                {
                    return Some((h.clone(), ""));
                }

                if !remaining_path.is_empty()
                    && let Some(res) = child.search(remaining_path)
                {
                    return Some(res);
                }

                if child.match_type == MatchType::Prefix
                    && let Some(h) = &child.handler
                {
                    return Some((h.clone(), remaining_path));
                }

                println!(
                    "Path matched but no handler or deeper route for: {:?}",
                    child.path
                );
            }
        }
        None
    }

    pub fn from_config(config: &Config) -> anyhow::Result<Self, anyhow::Error> {
        let mut router = RouterNode::default();
        for loc in config.server.locations.iter() {
            let path = loc.path.to_string_lossy();
            let root = loc.root.to_string_lossy();
            let index = &loc.index;
            let match_type = match loc.ty {
                Some(LocationConfigType::Exact) => MatchType::Exact,
                _ => MatchType::Prefix,
            };
            router.insert(
                &path,
                match_type,
                Some(RouterHandler::new(
                    root.as_ref(),
                    path.as_ref(),
                    index.to_vec(),
                )),
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
            Some(RouterHandler::new(
                "/www/var/html",
                "/home",
                vec!["home.html".to_string()],
            )),
        );

        root.insert(
            "/about",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/about",
                vec!["about.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/home").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/home"),
                index: Arc::from(vec![String::from("home.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/about").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/about"),
                index: Arc::from(vec![String::from("about.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(root.search("/"), None);
    }

    #[test]
    fn test_router_split_node_simple() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/home",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/home",
                vec!["home.html".to_string()],
            )),
        );
        root.insert(
            "/homepage",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/homepage",
                vec!["homepage.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/home").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/home"),
                index: Arc::from(vec![String::from("home.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/homepage").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/homepage"),
                index: Arc::from(vec![String::from("homepage.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/home/page").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/home"),
                index: Arc::from(vec![String::from("home.html")]),
                match_type: MatchType::Prefix
            },
        );
    }

    #[test]
    fn test_router_split_node_complex() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/apple",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/apple",
                vec!["apple.html".to_string()],
            )),
        );
        root.insert(
            "/apricot",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/apricot",
                vec!["apricot.html".to_string()],
            )),
        );
        root.insert(
            "/app",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/app",
                vec!["app.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/apple").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/apple"),
                index: Arc::from(vec![String::from("apple.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/apricot").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/apricot"),
                index: Arc::from(vec![String::from("apricot.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/app").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/app"),
                index: Arc::from(vec![String::from("app.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/ap").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/"),
                index: Arc::from(vec![String::from("index.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_deep_path() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/a/b/c",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/a/b/c",
                vec!["c.html".to_string()],
            )),
        );
        root.insert(
            "/a/b",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/a/b",
                vec!["b.html".to_string()],
            )),
        );
        root.insert(
            "/x/y/z/w",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/x/y/z/w",
                vec!["w.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/a/b/c").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/a/b/c"),
                index: Arc::from(vec![String::from("c.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/a/b").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/a/b"),
                index: Arc::from(vec![String::from("b.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/x/y/z/w").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/x/y/z/w"),
                index: Arc::from(vec![String::from("w.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/a").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/"),
                index: Arc::from(vec![String::from("index.html")]),
                match_type: MatchType::Prefix
            }
        );
        assert_eq!(
            root.search("/a/b/c").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/a/b/c"),
                index: Arc::from(vec![String::from("c.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_fallback_to_root() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/users",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/users",
                vec!["users.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/unknown/path").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/"),
                index: Arc::from(vec![String::from("index.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/users").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/users"),
                index: Arc::from(vec![String::from("users.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/users/profile/edit").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/users"),
                index: Arc::from(vec![String::from("users.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_root_path_handler() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        assert_eq!(
            root.search("/").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/"),
                index: Arc::from(vec![String::from("index.html")]),
                match_type: MatchType::Prefix,
            }
        );

        root.insert(
            "/foo",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/foo",
                vec!["foo.html".to_string()],
            )),
        );
        assert_eq!(
            root.search("/foo").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/foo"),
                index: Arc::from(vec![String::from("foo.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/"),
                index: Arc::from(vec![String::from("index.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_overwrite_handler() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/path",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/path",
                vec!["path.html".to_string()],
            )),
        );
        assert_eq!(
            root.search("/path").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/path"),
                index: Arc::from(vec![String::from("path.html")]),
                match_type: MatchType::Prefix,
            }
        );

        root.insert(
            "/path",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/path",
                vec!["path_v2.html".to_string()],
            )),
        );
        assert_eq!(
            root.search("/path").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/path"),
                index: Arc::from(vec![String::from("path_v2.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_common_prefix_split() {
        let mut root = RouterNode::default();
        root.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );
        root.insert(
            "/teams",
            MatchType::Exact,
            Some(
                RouterHandler::new("/www/var/html", "/teams", vec!["teams.html".to_string()])
                    .with_match_type(MatchType::Exact),
            ),
        );
        root.insert(
            "/team",
            MatchType::Exact,
            Some(
                RouterHandler::new("/www/var/html", "/team", vec!["team.html".to_string()])
                    .with_match_type(MatchType::Exact),
            ),
        );
        assert_eq!(
            root.search("/team").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/team"),
                index: Arc::from(vec![String::from("team.html")]),
                match_type: MatchType::Exact,
            }
        );
        assert_eq!(
            root.search("/teams").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/teams"),
                index: Arc::from(vec![String::from("teams.html")]),
                match_type: MatchType::Exact,
            }
        );

        let mut root2 = RouterNode::default();
        root2.insert(
            "/",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/",
                vec!["index.html".to_string()],
            )),
        );

        root2.insert(
            "/team",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/team",
                vec!["team.html".to_string()],
            )),
        );
        root2.insert(
            "/teams",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/teams",
                vec!["teams.html".to_string()],
            )),
        );

        assert_eq!(
            root2.search("/team").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/team"),
                index: Arc::from(vec![String::from("team.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root2.search("/teams").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/teams"),
                index: Arc::from(vec![String::from("teams.html")]),
                match_type: MatchType::Prefix,
            }
        );
    }

    #[test]
    fn test_router_matchtype_handling() {
        let mut root = RouterNode::default();
        root.insert(
            "/exact",
            MatchType::Exact,
            Some(
                RouterHandler::new("/www/var/html", "/exact", vec!["exact.html".to_string()])
                    .with_match_type(MatchType::Exact),
            ),
        );
        root.insert(
            "/prefix",
            MatchType::Prefix,
            Some(RouterHandler::new(
                "/www/var/html",
                "/prefix",
                vec!["prefix.html".to_string()],
            )),
        );

        assert_eq!(
            root.search("/exact").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/exact"),
                index: Arc::from(vec![String::from("exact.html")]),
                match_type: MatchType::Exact,
            }
        );
        assert_eq!(root.search("/exact/subpath"), None);

        assert_eq!(
            root.search("/prefix").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/prefix"),
                index: Arc::from(vec![String::from("prefix.html")]),
                match_type: MatchType::Prefix,
            }
        );
        assert_eq!(
            root.search("/prefix/subpath").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/prefix"),
                index: Arc::from(vec![String::from("prefix.html")]),
                match_type: MatchType::Prefix,
            }
        ); // Should match subpath
        assert_eq!(
            root.search("/prefix/another/subpath").unwrap().0,
            RouterHandler {
                root: Arc::from("/www/var/html"),
                matched_path: Arc::from("/prefix"),
                index: Arc::from(vec![String::from("prefix.html")]),
                match_type: MatchType::Prefix,
            }
        ); // Should match deeper subpath

        // Test non-matching paths
        assert_eq!(root.search("/no_match"), None);
        assert_eq!(root.search("/exac"), None); // Partial match, but not prefix for "exact"
        assert_eq!(root.search("/prefi"), None); // Partial match, but not prefix for "prefix"
    }
}
