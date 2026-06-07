use std::{
    collections::HashSet,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{config, error::CoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshHostSource {
    MoonboxConfig,
    OpensshConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostEntry {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub tags: Vec<String>,
    pub source: SshHostSource,
    pub source_path: Option<String>,
}

pub fn list_ssh_hosts() -> Result<Vec<SshHostEntry>, CoreError> {
    let mut hosts = Vec::new();
    for host in config::load_ssh_host_configs() {
        hosts.push(SshHostEntry {
            name: host.name,
            host: host.host,
            user: host.user,
            port: host.port,
            identity_file: host.identity_file,
            tags: host.tags,
            source: SshHostSource::MoonboxConfig,
            source_path: config::config_path().map(|path| path.display().to_string()),
        });
    }
    if let Some(path) = openssh_config_path() {
        hosts.extend(load_openssh_hosts(&path)?);
    }
    Ok(deduplicate_hosts(hosts))
}

fn openssh_config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("MOONBOX_SSH_CONFIG")
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".ssh").join("config"))
}

fn load_openssh_hosts(path: &Path) -> Result<Vec<SshHostEntry>, CoreError> {
    let mut visited = HashSet::new();
    load_openssh_hosts_inner(path, &mut visited, 0)
}

fn load_openssh_hosts_inner(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<Vec<SshHostEntry>, CoreError> {
    if depth > 8 {
        return Ok(Vec::new());
    }
    let normalized = normalize_path(path);
    if !visited.insert(normalized.clone()) {
        return Ok(Vec::new());
    }
    let contents = match fs::read_to_string(&normalized) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(CoreError::SshConfigRead {
                path: normalized.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    parse_openssh_config(&contents, &normalized, visited, depth)
}

fn parse_openssh_config(
    contents: &str,
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<Vec<SshHostEntry>, CoreError> {
    let mut hosts = Vec::new();
    let mut sections: Vec<OpenSshHostSection> = Vec::new();
    for line in contents.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        match keyword.to_ascii_lowercase().as_str() {
            "include" => {
                for pattern in parts {
                    for include_path in expand_include_path(path, pattern) {
                        hosts.extend(load_openssh_hosts_inner(&include_path, visited, depth + 1)?);
                    }
                }
            }
            "host" => {
                for section in sections.drain(..) {
                    hosts.push(section.into_entry(path));
                }
                sections = parts
                    .filter(|alias| is_concrete_host_alias(alias))
                    .map(OpenSshHostSection::new)
                    .collect();
            }
            "hostname" => {
                let value = parts.collect::<Vec<_>>().join(" ");
                for section in &mut sections {
                    section.host = Some(value.clone());
                }
            }
            "user" => {
                if let Some(value) = parts.next() {
                    for section in &mut sections {
                        section.user = Some(value.into());
                    }
                }
            }
            "port" => {
                if let Some(value) = parts.next().and_then(|value| value.parse::<u16>().ok()) {
                    for section in &mut sections {
                        section.port = Some(value);
                    }
                }
            }
            "identityfile" => {
                if let Some(value) = parts.next() {
                    for section in &mut sections {
                        if section.identity_file.is_none() && value != "none" {
                            section.identity_file = Some(value.into());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for section in sections {
        hosts.push(section.into_entry(path));
    }
    Ok(hosts)
}

fn strip_comment(line: &str) -> &str {
    line.split_once('#')
        .map(|(before, _)| before)
        .unwrap_or(line)
}

fn is_concrete_host_alias(alias: &str) -> bool {
    !alias.starts_with('!') && !alias.contains('*') && !alias.contains('?')
}

fn expand_include_path(current_config: &Path, pattern: &str) -> Vec<PathBuf> {
    let expanded = expand_path(current_config, pattern);
    let Some(pattern_name) = expanded.file_name().and_then(|name| name.to_str()) else {
        return vec![expanded];
    };
    if !pattern_name.contains('*') {
        return vec![expanded];
    }
    let Some(parent) = expanded.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut matches = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| wildcard_matches(pattern_name, name))
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches
}

fn expand_path(current_config: &Path, value: &str) -> PathBuf {
    if let Some(path) = value.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME")
    {
        return PathBuf::from(home).join(path);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        current_config
            .parent()
            .map(|parent| parent.join(&path))
            .unwrap_or(path)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut remaining = value;
    let mut first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if first && !pattern.starts_with('*') {
            let Some(after_prefix) = remaining.strip_prefix(part) else {
                return false;
            };
            remaining = after_prefix;
        } else if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
        first = false;
    }
    pattern.ends_with('*') || remaining.is_empty()
}

fn deduplicate_hosts(hosts: Vec<SshHostEntry>) -> Vec<SshHostEntry> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for host in hosts {
        if seen.insert(host.name.clone()) {
            output.push(host);
        }
    }
    output.sort_by(|left, right| left.name.cmp(&right.name));
    output
}

#[derive(Debug, Clone)]
struct OpenSshHostSection {
    name: String,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
}

impl OpenSshHostSection {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            host: None,
            user: None,
            port: None,
            identity_file: None,
        }
    }

    fn into_entry(self, path: &Path) -> SshHostEntry {
        SshHostEntry {
            host: self.host.unwrap_or_else(|| self.name.clone()),
            name: self.name,
            user: self.user,
            port: self.port,
            identity_file: self.identity_file,
            tags: Vec::new(),
            source: SshHostSource::OpensshConfig,
            source_path: Some(path.display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openssh_config_hosts_and_skips_wildcards() {
        let path = PathBuf::from("/tmp/ssh-config");
        let mut visited = HashSet::new();
        let hosts = parse_openssh_config(
            r#"
Host *
  User ignored

Host dev dev-short
  HostName dev.internal
  User deploy
  Port 2222
  IdentityFile ~/.ssh/dev

Host !blocked *.wild
  HostName ignored
"#,
            &path,
            &mut visited,
            0,
        )
        .expect("hosts");

        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "dev");
        assert_eq!(hosts[0].host, "dev.internal");
        assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[1].name, "dev-short");
    }

    #[test]
    fn config_hosts_take_precedence_over_openssh_duplicates() {
        let hosts = deduplicate_hosts(vec![
            SshHostEntry {
                name: "prod".into(),
                host: "prod.from.config".into(),
                user: None,
                port: None,
                identity_file: None,
                tags: Vec::new(),
                source: SshHostSource::MoonboxConfig,
                source_path: None,
            },
            SshHostEntry {
                name: "prod".into(),
                host: "prod.from.openssh".into(),
                user: None,
                port: None,
                identity_file: None,
                tags: Vec::new(),
                source: SshHostSource::OpensshConfig,
                source_path: None,
            },
        ]);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "prod.from.config");
    }

    #[test]
    fn wildcard_match_supports_include_globs() {
        assert!(wildcard_matches("*.conf", "dev.conf"));
        assert!(wildcard_matches("config-*", "config-prod"));
        assert!(!wildcard_matches("config-*", "prod-config"));
    }
}
