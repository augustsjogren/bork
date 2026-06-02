//! Minimal hand-rolled TOML reader shared across config files.
//!
//! Supports a flat schema with section headers and dotted keys:
//! - `key = "value"` (string, optional quotes)
//! - `key = true` / `key = false` (or quoted equivalents)
//! - `key = 300` (integer; quoted form also accepted)
//! - `key = ["a", "b"]` (string array, optional quotes per element)
//! - `[section]` / `[section.subsection]` headers — keys inside the section
//!   are flattened into dotted keys, e.g. `[agent.claude]` + `args = [...]`
//!   becomes `agent.claude.args`.
//! - `a.b.c = "x"` (dotted keys), equivalent to placing `c = "x"` under
//!   `[a.b]`.
//! - `# comment` (line or trailing inline)
//!
//! No inline-table syntax (`{ ... }`), no array-of-tables, no multiline
//! strings. We deliberately stay off the `toml` crate dependency.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Bool(bool),
    Int(i64),
    List(Vec<String>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_int().and_then(|n| u64::try_from(n).ok())
    }

    pub fn as_list(&self) -> Option<&[String]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }
}

pub type Table = HashMap<String, Value>;

/// Parse a TOML-lite document. Unknown shapes are silently skipped so partial
/// configs do not fail; callers decide which keys they care about.
pub fn parse(contents: &str) -> Table {
    let mut table = Table::new();
    let mut section: Vec<String> = Vec::new();

    for raw in contents.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(header) = parse_section_header(line) {
            section = header;
            continue;
        }

        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        let full_key = join_key(&section, key);
        if full_key.is_empty() {
            continue;
        }
        let value = parse_value(raw_value.trim());
        table.insert(full_key, value);
    }

    table
}

/// Parse a `[a.b.c]` header. Returns the dotted segments, or `None` if the
/// line is not a well-formed header. Empty `[]` resets to the root section.
fn parse_section_header(line: &str) -> Option<Vec<String>> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let parts: Vec<String> = inner
        .split('.')
        .map(|p| p.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

/// Combine the current section path with a (possibly dotted) key. Both empty
/// section and empty key are tolerated; the result is the dotted path joined
/// with `.` and stripped of empty segments.
fn join_key(section: &[String], key: &str) -> String {
    let key_parts = key
        .split('.')
        .map(|p| p.trim().trim_matches('"').trim_matches('\''))
        .filter(|p| !p.is_empty());
    let mut out: Vec<String> = section.to_vec();
    for part in key_parts {
        out.push(part.to_string());
    }
    out.join(".")
}

/// Strip an inline `#` comment, but only when the `#` is not inside a quoted
/// string. The parser is intentionally simple: arrays should not contain `#`.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn parse_value(raw: &str) -> Value {
    if let Some(list) = parse_list(raw) {
        return Value::List(list);
    }
    let token = trim_token(raw);
    if token == "true" {
        return Value::Bool(true);
    }
    if token == "false" {
        return Value::Bool(false);
    }
    if let Ok(n) = token.parse::<i64>() {
        return Value::Int(n);
    }
    Value::String(token.to_string())
}

fn parse_list(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    for part in inner.split(',') {
        let token = trim_token(part);
        if token.is_empty() {
            continue;
        }
        items.push(token.to_string());
    }
    Some(items)
}

fn trim_token(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_scalars() {
        let t = parse(r#"name = "bork""#);
        assert_eq!(t["name"].as_str(), Some("bork"));
    }

    #[test]
    fn parses_unquoted_strings() {
        let t = parse("name = bork");
        assert_eq!(t["name"].as_str(), Some("bork"));
    }

    #[test]
    fn parses_bools() {
        let t = parse("debug = true\nverbose = false");
        assert_eq!(t["debug"].as_bool(), Some(true));
        assert_eq!(t["verbose"].as_bool(), Some(false));
    }

    #[test]
    fn parses_quoted_bools() {
        let t = parse(r#"debug = "true""#);
        assert_eq!(t["debug"].as_bool(), Some(true));
    }

    #[test]
    fn parses_ints() {
        let t = parse("ttl = 300");
        assert_eq!(t["ttl"].as_int(), Some(300));
        assert_eq!(t["ttl"].as_u64(), Some(300));
    }

    #[test]
    fn parses_quoted_ints() {
        let t = parse(r#"ttl = "600""#);
        assert_eq!(t["ttl"].as_u64(), Some(600));
    }

    #[test]
    fn parses_arrays() {
        let t = parse(r#"agents = ["claude", "opencode"]"#);
        assert_eq!(
            t["agents"].as_list(),
            Some(&["claude".to_string(), "opencode".to_string()][..])
        );
    }

    #[test]
    fn parses_arrays_with_unquoted_items() {
        let t = parse("agents = [claude, opencode]");
        assert_eq!(
            t["agents"].as_list(),
            Some(&["claude".to_string(), "opencode".to_string()][..])
        );
    }

    #[test]
    fn empty_array() {
        let t = parse("agents = []");
        assert_eq!(t["agents"].as_list(), Some(&[][..]));
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let t = parse(
            r#"
# top comment
name = "bork"   # trailing

debug = true
"#,
        );
        assert_eq!(t["name"].as_str(), Some("bork"));
        assert_eq!(t["debug"].as_bool(), Some(true));
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        let t = parse(r#"prompt = "hello # world""#);
        assert_eq!(t["prompt"].as_str(), Some("hello # world"));
    }

    #[test]
    fn skips_lines_without_equals() {
        let t = parse("not a key value pair\nname = ok");
        assert_eq!(t.len(), 1);
        assert_eq!(t["name"].as_str(), Some("ok"));
    }

    #[test]
    fn last_value_wins_on_duplicate_keys() {
        let t = parse("name = first\nname = second");
        assert_eq!(t["name"].as_str(), Some("second"));
    }

    #[test]
    fn invalid_int_falls_back_to_string() {
        let t = parse("ttl = notanumber");
        assert_eq!(t["ttl"].as_int(), None);
        assert_eq!(t["ttl"].as_str(), Some("notanumber"));
    }

    #[test]
    fn section_header_flattens_keys() {
        let t = parse(
            r#"
[agent.claude]
args = ["--dangerously-skip-permissions"]
"#,
        );
        assert_eq!(
            t["agent.claude.args"].as_list(),
            Some(&["--dangerously-skip-permissions".to_string()][..])
        );
    }

    #[test]
    fn nested_section_headers() {
        let t = parse(
            r#"
[agent.claude.mode.build]
args = ["--foo"]
"#,
        );
        assert_eq!(
            t["agent.claude.mode.build.args"].as_list(),
            Some(&["--foo".to_string()][..])
        );
    }

    #[test]
    fn dotted_keys_flatten_like_sections() {
        let t = parse(r#"agent.claude.args = ["--foo"]"#);
        assert_eq!(
            t["agent.claude.args"].as_list(),
            Some(&["--foo".to_string()][..])
        );
    }

    #[test]
    fn dotted_key_inside_section_combines_paths() {
        let t = parse(
            r#"
[agent.claude]
mode.build.args = ["--foo"]
"#,
        );
        assert_eq!(
            t["agent.claude.mode.build.args"].as_list(),
            Some(&["--foo".to_string()][..])
        );
    }

    #[test]
    fn empty_section_header_resets_to_root() {
        let t = parse(
            r#"
[agent.claude]
args = ["--foo"]
[]
top = "level"
"#,
        );
        assert_eq!(
            t["agent.claude.args"].as_list(),
            Some(&["--foo".to_string()][..])
        );
        assert_eq!(t["top"].as_str(), Some("level"));
    }

    #[test]
    fn multiple_sections_in_one_file() {
        let t = parse(
            r#"
project_name = "bork"

[agent.claude]
args = ["--foo"]

[agent.codex.mode.yolo]
args = ["--bar"]
"#,
        );
        assert_eq!(t["project_name"].as_str(), Some("bork"));
        assert_eq!(
            t["agent.claude.args"].as_list(),
            Some(&["--foo".to_string()][..])
        );
        assert_eq!(
            t["agent.codex.mode.yolo.args"].as_list(),
            Some(&["--bar".to_string()][..])
        );
    }
}
