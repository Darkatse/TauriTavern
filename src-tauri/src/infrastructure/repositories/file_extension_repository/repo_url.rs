use url::Url;

use crate::domain::errors::DomainError;

pub(super) const HOST_GITHUB: &str = "github.com";
pub(super) const HOST_GITLAB: &str = "gitlab.com";
pub(super) const HOST_GITEE: &str = "gitee.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoSpec {
    pub(super) host: String,
    /// `owner/repo` for GitHub/Gitee, `group/subgroup/repo` for GitLab.
    pub(super) repo_path: String,
    pub(super) reference_from_url: Option<String>,
}

impl RepoSpec {
    pub(super) fn repo_name(&self) -> &str {
        self.repo_path
            .rsplit('/')
            .next()
            .unwrap_or(self.repo_path.as_str())
    }

    pub(super) fn canonical_remote_url(&self) -> String {
        format!("https://{}/{}", self.host, self.repo_path)
    }
}

pub(super) fn normalize_requested_reference(reference: Option<String>) -> Option<String> {
    reference
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn canonicalize_host(host: &str) -> Option<&'static str> {
    let normalized = host.trim().to_ascii_lowercase();
    let normalized = normalized.strip_prefix("www.").unwrap_or(&normalized);

    match normalized {
        HOST_GITHUB => Some(HOST_GITHUB),
        HOST_GITLAB => Some(HOST_GITLAB),
        HOST_GITEE => Some(HOST_GITEE),
        _ => None,
    }
}

fn strip_dot_git(repo_segment: &str) -> &str {
    repo_segment.trim_end_matches(".git").trim()
}

fn parse_reference_query(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == "ref")
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn parse_repo_url(url: &str) -> Result<RepoSpec, DomainError> {
    let raw = url.trim();
    let parsed_url = Url::parse(raw).map_err(|error| {
        DomainError::InvalidData(format!("Invalid repository URL '{}': {}", raw, error))
    })?;

    let Some(host) = parsed_url.host_str() else {
        return Err(DomainError::InvalidData(format!(
            "Repository URL '{}' is missing a host",
            raw
        )));
    };

    let Some(host) = canonicalize_host(host) else {
        return Err(DomainError::InvalidData(format!(
            "Only {} / {} / {} repositories are supported",
            HOST_GITHUB, HOST_GITLAB, HOST_GITEE
        )));
    };

    let segments = parsed_url
        .path_segments()
        .ok_or_else(|| DomainError::InvalidData("Invalid repository URL path".to_string()))?
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<String>>();

    let query_reference = parse_reference_query(&parsed_url);

    match host {
        HOST_GITHUB | HOST_GITEE => {
            if segments.len() < 2 {
                return Err(DomainError::InvalidData(
                    "Repository URL must include owner and repository".to_string(),
                ));
            }

            let owner = segments[0].trim();
            let repo = strip_dot_git(&segments[1]);
            if owner.is_empty() || repo.is_empty() {
                return Err(DomainError::InvalidData(
                    "Repository owner/repo cannot be empty".to_string(),
                ));
            }

            let reference_from_path = if segments.len() >= 4 && segments[2] == "tree" {
                let reference = segments[3..].join("/");
                if reference.is_empty() {
                    None
                } else {
                    Some(reference)
                }
            } else {
                None
            };

            Ok(RepoSpec {
                host: host.to_string(),
                repo_path: format!("{}/{}", owner, repo),
                reference_from_url: reference_from_path.or(query_reference),
            })
        }
        HOST_GITLAB => {
            if segments.len() < 2 {
                return Err(DomainError::InvalidData(
                    "GitLab URL must include namespace and project".to_string(),
                ));
            }

            let dash_index = segments.iter().position(|value| value == "-");
            let repo_segments_end = dash_index.unwrap_or(segments.len());
            if repo_segments_end < 2 {
                return Err(DomainError::InvalidData(
                    "GitLab URL must include namespace and project".to_string(),
                ));
            }

            let mut repo_segments = segments[..repo_segments_end].to_vec();
            let last = repo_segments
                .last_mut()
                .ok_or_else(|| DomainError::InvalidData("GitLab URL path is empty".to_string()))?;
            *last = strip_dot_git(last).to_string();

            if repo_segments
                .iter()
                .any(|segment| segment.trim().is_empty())
            {
                return Err(DomainError::InvalidData(
                    "GitLab URL path contains empty segments".to_string(),
                ));
            }

            let reference_from_path = match dash_index {
                Some(index) if segments.len() > index + 2 && segments[index + 1] == "tree" => {
                    let reference = segments[index + 2..].join("/");
                    if reference.is_empty() {
                        None
                    } else {
                        Some(reference)
                    }
                }
                _ => None,
            };

            Ok(RepoSpec {
                host: host.to_string(),
                repo_path: repo_segments.join("/"),
                reference_from_url: reference_from_path.or(query_reference),
            })
        }
        _ => Err(DomainError::InvalidData(format!(
            "Unsupported repository host: {}",
            host
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_repo_url;

    #[test]
    fn github_tree_reference_overrides_ref_query_param() {
        let spec = parse_repo_url("https://github.com/owner/repo/tree/main?ref=dev")
            .expect("parse github url");
        assert_eq!(spec.reference_from_url.as_deref(), Some("main"));
    }

    #[test]
    fn github_ref_query_param_used_when_tree_missing() {
        let spec =
            parse_repo_url("https://github.com/owner/repo?ref=dev").expect("parse github url");
        assert_eq!(spec.reference_from_url.as_deref(), Some("dev"));
    }

    #[test]
    fn gitlab_tree_reference_overrides_ref_query_param() {
        let spec = parse_repo_url("https://gitlab.com/group/subgroup/repo/-/tree/main?ref=dev")
            .expect("parse gitlab url");
        assert_eq!(spec.reference_from_url.as_deref(), Some("main"));
    }
}
