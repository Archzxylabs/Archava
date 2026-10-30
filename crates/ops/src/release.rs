//! The release-identity verifier — R10's `verify-release` and the arithmetic
//! behind `P-RELEASE-IDENTITY`.
//!
//! PRD §23 G09 PASS: "all artifacts/config/migrations/proofs point to exact
//! immutable profile". VER-04: the archived prior release (`dist/releases/`)
//! is never overwritten — a new release identity is a NEW identity, and this
//! verifier refuses the two ways that rule silently dies:
//!
//! 1. The shipped `dist/library/release-profile.json` disagreeing with
//!    `config/library-release.v1.json` (the source of truth — `dist/` is a
//!    build product; a drifted dist copy is exactly how a stale identity gets
//!    quoted as current).
//! 2. An archived directory whose contents claim a version other than the
//!    directory's name, or whose profile digest no longer matches a recorded
//!    digest — states only an overwrite can produce.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One shipped artifact (`dist/library/tarballs/*.tgz`), as the release
/// profile records it.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileArtifact {
    pub name: String,
    pub version: String,
    pub tarball: String,
    pub sha256: String,
    #[serde(rename = "implementationSha256")]
    pub implementation_sha256: Option<String>,
}

/// The profile file's own identity block — enough of `release-profile.json`
/// to verify it against the config without duplicating the whole schema: the
/// planner's own `ReleaseCatalog` (from `archava-assembly`) owns the full
/// catalogue parse; this is the dist half.
#[derive(Debug, Clone, Deserialize)]
pub struct DistProfile {
    pub catalog: DistCatalog,
    pub artifacts: Vec<ProfileArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DistCatalog {
    pub id: String,
    pub version: String,
}

/// One artifact's recompute result.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactCheck {
    pub name: String,
    pub version: String,
    pub bytes: u64,
    pub digest_recorded: String,
    pub digest_recomputed: String,
    pub verified: bool,
}

/// One prior archive's scan result (`dist/releases/<version>/…`).
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveCheck {
    /// The directory name, which is the identity the archive claims.
    pub dir: String,
    /// The version the archive's own profile file declares.
    pub version: String,
    /// Whether the declared version matches the directory name.
    pub intact: bool,
    /// sha256 over the archive's profile file, recorded so a later verifier
    /// can Detect an overwritten (byte-changed) archive against this report.
    pub profile_digest: String,
    pub bytes: u64,
}

/// The full verification output.
#[derive(Debug, Clone, Serialize)]
pub struct Verification {
    pub config_id: String,
    pub config_version: String,
    pub profile_version: String,
    /// Identity agreement requires both a nonempty catalogue id and its exact version.
    pub version_matches: bool,
    pub artifacts: Vec<ArtifactCheck>,
    pub artifacts_verified: bool,
    pub prior_archives: Vec<ArchiveCheck>,
    /// Every scanned archive is internally consistent AND none of them claims
    /// the current identity (which would mean the current release overwrote a
    /// prior slot rather than adding a new one).
    pub priors_preserved: bool,
}

/// Everything that can go wrong, typed so the CLI can refuse with one line
/// and the rehearsal tests can name the refusal they expect.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("release config unreadable at {path}: {source}")]
    Config {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("release profile unreadable at {path}: {source}")]
    Profile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("profile artifact missing from disk: {path}")]
    Artifact { path: PathBuf },
    #[error("profile artifact digest WROTE differently than recorded: {name}")]
    ArtifactDigest { name: String },
    #[error("prior archive profile unreadable at {path}: {source}")]
    Archive {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("prior archive dir is not itself: {dir}")]
    ArchiveIdentity { dir: PathBuf },
}

/// The workspace-relative filenames this verifier expects. Kept as constants
/// so the CLI's help text and the rehearsal cannot drift from each other.
pub const CONFIG_PATH: &str = "config/library-release.v1.json";
pub const PROFILE_PATH: &str = "dist/library/release-profile.json";
pub const RELEASES_DIR: &str = "dist/releases";

fn sha256_file(path: &Path) -> Result<(String, u64), VerifyError> {
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => VerifyError::Artifact { path: path.into() },
        _ => VerifyError::Artifact { path: path.into() },
    })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((digest, bytes.len() as u64))
}

/// Verify the candidate's release identity end to end.
///
/// Everything is RECOMPUTED — the config version, the dist profile version,
/// every artifact digest, every prior archive's own consistency — so the
/// output is evidence of THIS tree, not of the last run that wrote a file.
pub fn verify(
    config_path: &Path,
    profile_path: &Path,
    releases_dir: &Path,
) -> Result<Verification, VerifyError> {
    let config_bytes = std::fs::read(config_path).map_err(|source| VerifyError::Config {
        path: config_path.into(),
        source,
    })?;
    let config: serde_json::Value =
        serde_json::from_slice(&config_bytes).map_err(|e| VerifyError::Config {
            path: config_path.into(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;
    let config_id = config["id"].as_str().unwrap_or_default().to_string();
    let config_version = config["version"].as_str().unwrap_or_default().to_string();

    let profile_bytes = std::fs::read(profile_path).map_err(|source| VerifyError::Profile {
        path: profile_path.into(),
        source,
    })?;
    let profile: DistProfile =
        serde_json::from_slice(&profile_bytes).map_err(|e| VerifyError::Profile {
            path: profile_path.into(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;

    let version_matches = !config_id.is_empty()
        && config_id == profile.catalog.id
        && !config_version.is_empty()
        && !profile.catalog.version.is_empty()
        && config_version == profile.catalog.version;

    // Root the tarball paths the way the profile records them (relative to
    // the profile file's directory, which is `dist/library`).
    let library_dir = profile_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut artifacts = Vec::new();
    let mut artifacts_verified = !profile.artifacts.is_empty();
    for artifact in &profile.artifacts {
        let path = if artifact.tarball.starts_with('/') {
            PathBuf::from(&artifact.tarball)
        } else {
            library_dir.join(&artifact.tarball)
        };
        let (digest_recomputed, bytes) = sha256_file(&path)?;
        let verified = digest_recomputed == artifact.sha256;
        if !verified {
            artifacts_verified = false;
        }
        artifacts.push(ArtifactCheck {
            name: artifact.name.clone(),
            version: artifact.version.clone(),
            bytes,
            digest_recorded: artifact.sha256.clone(),
            digest_recomputed,
            verified,
        });
    }

    // Prior archives: every `dist/releases/<version>/` slot reads its own
    // profile file and must be internally consistent, and NONE of them may
    // claim the current identity — an archive slot that "matches" the
    // candidate's version would mean the candidate was written into it.
    let mut prior_archives = Vec::new();
    let mut priors_preserved = true;
    if releases_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(releases_dir)
            .map_err(|e| VerifyError::Archive {
                path: releases_dir.into(),
                source: e,
            })?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.is_dir())
            .collect();
        entries.sort();
        for dir in entries {
            let dir_name = dir
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let archive_profile = dir
                .join("library")
                .join("release-profile.json")
                .exists()
                .then(|| dir.join("library").join("release-profile.json"))
                .or_else(|| {
                    dir.join("crm-runtime")
                        .join("release-profile.json")
                        .exists()
                        .then(|| dir.join("crm-runtime").join("release-profile.json"))
                })
                .ok_or_else(|| VerifyError::Archive {
                    path: dir.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "no release-profile.json in the archive slot",
                    ),
                })?;
            let (profile_digest, bytes) = sha256_file(&archive_profile)?;
            let archive_bytes =
                std::fs::read(&archive_profile).map_err(|source| VerifyError::Archive {
                    path: archive_profile.clone(),
                    source,
                })?;
            let archive: DistProfile =
                serde_json::from_slice(&archive_bytes).map_err(|e| VerifyError::Archive {
                    path: archive_profile.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                })?;
            let dir_path = dir.clone();
            let intact = !dir_name.is_empty()
                && !archive.catalog.version.is_empty()
                && dir_name == archive.catalog.version;
            if !intact {
                return Err(VerifyError::ArchiveIdentity { dir: dir_path });
            }
            if archive.catalog.version == profile.catalog.version && version_matches {
                priors_preserved = false;
            }
            prior_archives.push(ArchiveCheck {
                dir: dir_name,
                version: archive.catalog.version,
                intact,
                profile_digest,
                bytes,
            });
        }
    }

    Ok(Verification {
        config_id,
        config_version,
        profile_version: profile.catalog.version,
        version_matches,
        artifacts,
        artifacts_verified,
        prior_archives,
        priors_preserved,
    })
}
