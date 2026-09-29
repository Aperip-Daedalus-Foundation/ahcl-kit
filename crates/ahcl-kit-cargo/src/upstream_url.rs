// crates/ahcl-kit-cargo/src/upstream_url.rs - Immutable upstream URL checks.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// The AHCL-covered material identified below forms part of
// AHCL Kit and is provided under version 1.2 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// AHCL-covered portions: the whole file
//
// Subject to Section 3.1 of AHCL, after having a reasonable opportunity to
// read AHCL, all applicable Additional Restrictions, and all version notices,
// a person accepts the corresponding terms by engaging in Use of the
// AHCL-covered material identified above. Any separate or affirmative assent
// required by applicable mandatory law must still be obtained.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// Applicable LICENSE:                             LICENSE
// Paths below are relative to the directory containing that LICENSE.
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.2.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.2

use crate::adapter::CargoError;
use url::Url;

enum UrlRole {
    Evidence,
    Materials,
}

struct CheckedUrl<'a> {
    host: String,
    parts: Vec<String>,
    parsed: Url,
    revision: &'a str,
    path: &'a str,
    evidence_url: &'a str,
    role: UrlRole,
}

pub(crate) fn validate_source_url(
    url: &str,
    repository: &str,
    revision: &str,
    path: &str,
) -> Result<(), CargoError> {
    let checked = checked_provider_url(
        url,
        repository,
        revision,
        path,
        UrlRole::Evidence,
        "evidence URL must be an immutable HTTPS provider URL",
    )?;
    // Host, owner, revision, and path must all match before the URL is trusted.
    if !checked.matches()? {
        return Err(upstream_url_error(
            url,
            "evidence URL does not match the configured repository, revision, and path",
        ));
    }
    Ok(())
}

pub(crate) fn validate_materials_url(
    url: &str,
    repository: &str,
    revision: &str,
    path: &str,
) -> Result<(), CargoError> {
    let checked = checked_provider_url(
        url,
        repository,
        revision,
        path,
        UrlRole::Materials,
        "materials URL must be an immutable HTTPS provider URL",
    )?;
    // The same provider identity is required for materials URLs, including tree pages.
    if !checked.matches()? {
        return Err(upstream_url_error(
            url,
            "materials URL does not match the configured repository, revision, and path",
        ));
    }
    Ok(())
}

fn checked_provider_url<'a>(
    url: &'a str,
    repository: &str,
    revision: &'a str,
    path: &'a str,
    role: UrlRole,
    immutable_reason: &'static str,
) -> Result<CheckedUrl<'a>, CargoError> {
    let (host, parts) = repository_identity(repository).ok_or_else(|| {
        upstream_url_error(url, "repository must be an HTTPS GitHub or GitLab URL")
    })?;
    let parsed =
        immutable_source_url(url).ok_or_else(|| upstream_url_error(url, immutable_reason))?;
    Ok(CheckedUrl {
        host,
        parts,
        parsed,
        revision,
        path,
        evidence_url: url,
        role,
    })
}

impl CheckedUrl<'_> {
    fn matches(&self) -> Result<bool, CargoError> {
        let segments = self.segments();
        let path_segments: Vec<&str> = self.path.split('/').collect();
        let host = self
            .parsed
            .host_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if self.host == "github.com" {
            return self.github(&host, &segments, &path_segments);
        }
        Ok(self.gitlab(&host, &segments, &path_segments))
    }

    fn github(
        &self,
        host: &str,
        segments: &[&str],
        path_segments: &[&str],
    ) -> Result<bool, CargoError> {
        let [owner, repo] = self.parts.as_slice() else {
            return Err(upstream_url_error(
                self.evidence_url,
                "repository path must be owner/repository",
            ));
        };
        Ok(self.github_raw(host, segments, owner, repo, path_segments)
            || self.github_site(host, segments, owner, repo, path_segments))
    }

    fn github_raw(
        &self,
        host: &str,
        segments: &[&str],
        owner: &str,
        repo: &str,
        path_segments: &[&str],
    ) -> bool {
        host == "raw.githubusercontent.com"
            && segments.len() >= 3
            && segments[0].eq_ignore_ascii_case(owner)
            && segments[1].eq_ignore_ascii_case(repo)
            && segments[2].eq_ignore_ascii_case(self.revision)
            && &segments[3..] == path_segments
    }

    fn github_site(
        &self,
        host: &str,
        segments: &[&str],
        owner: &str,
        repo: &str,
        path_segments: &[&str],
    ) -> bool {
        host == "github.com"
            && segments.len() >= 5
            && segments[0].eq_ignore_ascii_case(owner)
            && segments[1].eq_ignore_ascii_case(repo)
            && self.github_kind(segments[2])
            && segments[3].eq_ignore_ascii_case(self.revision)
            && &segments[4..] == path_segments
    }

    fn github_kind(&self, segment: &str) -> bool {
        if segment == "blob" || segment == "raw" {
            return true;
        }
        self.allows_tree() && segment == "tree"
    }

    fn gitlab(&self, host: &str, segments: &[&str], path_segments: &[&str]) -> bool {
        if host != "gitlab.com" {
            return false;
        }
        self.gitlab_prefix(segments) && self.gitlab_tail(segments, path_segments)
    }

    fn gitlab_prefix(&self, segments: &[&str]) -> bool {
        let repository_len = self.parts.len();
        repository_len >= 2
            && segments.len() >= repository_len + 3
            && self.gitlab_owners(segments, repository_len)
    }

    fn gitlab_owners(&self, segments: &[&str], repository_len: usize) -> bool {
        segments[..repository_len]
            .iter()
            .zip(&self.parts)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
    }

    fn gitlab_tail(&self, segments: &[&str], path_segments: &[&str]) -> bool {
        let repository_len = self.parts.len();
        segments[repository_len] == "-"
            && self.gitlab_kind(segments[repository_len + 1])
            && segments[repository_len + 2].eq_ignore_ascii_case(self.revision)
            && &segments[repository_len + 3..] == path_segments
    }

    fn gitlab_kind(&self, segment: &str) -> bool {
        if segment == "raw" {
            return true;
        }
        self.allows_tree() && segment == "tree"
    }

    fn allows_tree(&self) -> bool {
        match self.role {
            UrlRole::Materials => true,
            UrlRole::Evidence => false,
        }
    }

    fn skips_empty_segments(&self) -> bool {
        match self.role {
            UrlRole::Materials => true,
            UrlRole::Evidence => false,
        }
    }

    fn segments(&self) -> Vec<&str> {
        let Some(raw) = self.parsed.path_segments() else {
            return Vec::new();
        };
        // Materials pages ignore empty segments. Evidence URLs keep them so a
        // trailing or doubled slash cannot satisfy a configured path.
        if self.skips_empty_segments() {
            raw.filter(|segment| !segment.is_empty()).collect()
        } else {
            raw.collect()
        }
    }
}

fn repository_identity(value: &str) -> Option<(String, Vec<String>)> {
    let parsed = Url::parse(value).ok()?;
    if !https_url_without_extras(&parsed) {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !is_supported_repository_host(&host) {
        return None;
    }
    let parts = repository_path_parts(&parsed)?;
    Some((host, parts))
}

fn immutable_source_url(value: &str) -> Option<Url> {
    let parsed = Url::parse(value).ok()?;
    if !https_url_without_extras(&parsed) || parsed.path().contains('%') {
        return None;
    }
    Some(parsed)
}

fn https_url_without_extras(parsed: &Url) -> bool {
    parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

fn is_supported_repository_host(host: &str) -> bool {
    host == "github.com" || host == "gitlab.com"
}

fn repository_path_parts(parsed: &Url) -> Option<Vec<String>> {
    let mut parts = parsed
        .path_segments()?
        .map(str::to_owned)
        .collect::<Vec<_>>();
    strip_git_suffix(&mut parts);
    if parts.len() < 2 || parts.iter().any(|part| path_part_rejected(part)) {
        return None;
    }
    Some(parts)
}

fn strip_git_suffix(parts: &mut [String]) {
    if let Some(last) = parts.last_mut() {
        if let Some(stripped) = last.strip_suffix(".git") {
            *last = stripped.to_owned();
        }
    }
}

fn path_part_rejected(part: &str) -> bool {
    part.is_empty() || part.contains('%')
}

fn upstream_url_error(url: &str, reason: &str) -> CargoError {
    CargoError::UpstreamEvidence {
        package: "unknown".to_owned(),
        version: "unknown".to_owned(),
        location: url.to_owned(),
        reason: reason.to_owned(),
    }
}
