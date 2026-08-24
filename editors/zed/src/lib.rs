use std::fs;
use zed::LanguageServerId;
use zed_extension_api::{self as zed, settings::LspSettings, Result};

struct PanacheBinary {
    path: String,
    args: Option<Vec<String>>,
}

struct PanacheExtension {
    cached_binary_path: Option<String>,
}

#[derive(Debug, PartialEq)]
struct GithubReleaseDetails {
    /// Candidate asset names in preference order. Linux lists the glibc build
    /// first and the static musl build as a fallback, mirroring the resolution
    /// order in `editors/code/src/installer.ts`.
    asset_names: Vec<String>,
    downloaded_file_type: zed::DownloadedFileType,
    downloaded_directory: String,
    downloaded_binary_path: String,
}

impl PanacheExtension {
    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<PanacheBinary> {
        let binary_settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.binary);

        let binary_args = binary_settings
            .as_ref()
            .and_then(|binary_settings| binary_settings.arguments.clone());

        if let Some(path) = binary_settings.and_then(|binary_settings| binary_settings.path) {
            return Ok(PanacheBinary {
                path,
                args: binary_args,
            });
        }

        if let Some(path) = worktree.which("panache") {
            return Ok(PanacheBinary {
                path,
                args: binary_args,
            });
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(PanacheBinary {
                    path: path.clone(),
                    args: binary_args,
                });
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let (platform, arch) = zed::current_platform();

        // `require_assets` resolves to the newest release that carries assets and
        // cannot filter by tag prefix. The invariant that keeps this correct: only
        // the primary `v*` CLI stream may carry assets. Every sibling tag stream in
        // the monorepo (`panache-parser-v*`, `panache-formatter-v*`,
        // `panache-code-v*`, `panache-zed-v*`) must stay asset-free, or it would
        // shadow the CLI release here. See AGENTS.md "Release Management".
        let release = zed::latest_github_release(
            "jolars/panache",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        // The host returns the tag name verbatim (e.g. `v2.54.0`); strip the `v`
        // so the cached directory reads `panache-2.54.0`.
        let version = release
            .version
            .strip_prefix('v')
            .unwrap_or(&release.version)
            .to_string();
        let release_details = GithubReleaseDetails::new(platform, arch, version)?;

        let asset = release_details
            .asset_names
            .iter()
            .find_map(|name| release.assets.iter().find(|asset| &asset.name == name))
            .ok_or_else(|| {
                format!(
                    "Panache release {} has no asset matching any of {:?}",
                    release.version, release_details.asset_names
                )
            })?;

        if !fs::metadata(&release_details.downloaded_binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &release_details.downloaded_directory,
                release_details.downloaded_file_type,
            )
            .map_err(|error| format!("Failed to download file: {error}"))?;

            let entries = fs::read_dir(".")
                .map_err(|error| format!("Failed to list working directory: {error}"))?;

            for entry in entries {
                let entry =
                    entry.map_err(|error| format!("Failed to load directory entry: {error}"))?;
                if entry.file_name().to_str() != Some(&release_details.downloaded_directory) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.cached_binary_path = Some(release_details.downloaded_binary_path.clone());

        Ok(PanacheBinary {
            path: release_details.downloaded_binary_path,
            args: binary_args,
        })
    }
}

impl GithubReleaseDetails {
    fn new(
        platform: zed_extension_api::Os,
        arch: zed_extension_api::Architecture,
        version: String,
    ) -> Result<Self> {
        let arch = match arch {
            zed::Architecture::Aarch64 => "aarch64",
            zed::Architecture::X8664 => "x86_64",
            // Panache publishes no 32-bit assets, so failing here beats asking
            // the host to download an `x86-*` archive that does not exist.
            zed::Architecture::X86 => {
                return Err("Panache does not publish binaries for 32-bit x86".into())
            }
        };

        let asset_names = match platform {
            zed::Os::Mac => vec![format!("panache-{arch}-apple-darwin.tar.gz")],
            zed::Os::Linux => vec![
                format!("panache-{arch}-unknown-linux-gnu.tar.gz"),
                format!("panache-{arch}-unknown-linux-musl.tar.gz"),
            ],
            zed::Os::Windows => vec![format!("panache-{arch}-pc-windows-msvc.zip")],
        };

        let downloaded_file_type = match platform {
            zed::Os::Mac | zed::Os::Linux => zed::DownloadedFileType::GzipTar,
            zed::Os::Windows => zed::DownloadedFileType::Zip,
        };

        let downloaded_directory = format!("panache-{version}");

        let downloaded_binary_path = match platform {
            zed::Os::Mac | zed::Os::Linux => format!("{downloaded_directory}/panache"),
            zed::Os::Windows => format!("{downloaded_directory}/panache.exe"),
        };

        Ok(Self {
            asset_names,
            downloaded_file_type,
            downloaded_directory,
            downloaded_binary_path,
        })
    }
}

impl zed::Extension for PanacheExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let panache_binary = self.language_server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: panache_binary.path,
            args: panache_binary.args.unwrap_or_else(|| vec!["lsp".into()]),
            env: vec![],
        })
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.initialization_options.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }

    fn language_server_workspace_configuration(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }
}

zed::register_extension!(PanacheExtension);

#[cfg(test)]
mod test {
    use crate::GithubReleaseDetails;
    use zed_extension_api::{Architecture, DownloadedFileType, Os};

    #[test]
    fn resolves_macos_release() {
        assert_eq!(
            GithubReleaseDetails::new(Os::Mac, Architecture::Aarch64, String::from("0.1.0")),
            Ok(GithubReleaseDetails {
                asset_names: vec![String::from("panache-aarch64-apple-darwin.tar.gz")],
                downloaded_file_type: DownloadedFileType::GzipTar,
                downloaded_directory: String::from("panache-0.1.0"),
                downloaded_binary_path: String::from("panache-0.1.0/panache"),
            })
        );
    }

    #[test]
    fn resolves_linux_release_with_musl_fallback() {
        assert_eq!(
            GithubReleaseDetails::new(Os::Linux, Architecture::X8664, String::from("0.2.0")),
            Ok(GithubReleaseDetails {
                asset_names: vec![
                    String::from("panache-x86_64-unknown-linux-gnu.tar.gz"),
                    String::from("panache-x86_64-unknown-linux-musl.tar.gz"),
                ],
                downloaded_file_type: DownloadedFileType::GzipTar,
                downloaded_directory: String::from("panache-0.2.0"),
                downloaded_binary_path: String::from("panache-0.2.0/panache"),
            })
        );
    }

    #[test]
    fn resolves_windows_release() {
        assert_eq!(
            GithubReleaseDetails::new(Os::Windows, Architecture::X8664, String::from("0.1.0")),
            Ok(GithubReleaseDetails {
                asset_names: vec![String::from("panache-x86_64-pc-windows-msvc.zip")],
                downloaded_file_type: DownloadedFileType::Zip,
                downloaded_directory: String::from("panache-0.1.0"),
                downloaded_binary_path: String::from("panache-0.1.0/panache.exe"),
            })
        );
    }

    #[test]
    fn rejects_32_bit_x86() {
        assert!(
            GithubReleaseDetails::new(Os::Linux, Architecture::X86, String::from("0.1.0")).is_err()
        );
    }
}
