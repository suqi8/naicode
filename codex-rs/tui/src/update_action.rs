#[cfg(any(not(debug_assertions), test))]
use codex_install_context::InstallContext;
#[cfg(any(not(debug_assertions), test))]
use codex_install_context::InstallMethod;
#[cfg(any(not(debug_assertions), test))]
use codex_install_context::StandalonePlatform;

/// Update action the CLI should perform after the TUI exits.
///
/// NaiCode has no Homebrew cask, so `InstallMethod::Brew` intentionally maps to
/// no action rather than running `brew upgrade`, which would pull upstream Codex
/// and replace the running install with a different product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g naicode@latest`.
    NpmGlobalLatest,
    /// Update via `bun install -g naicode@latest`.
    BunGlobalLatest,
    /// Update via `pnpm add -g naicode@latest`.
    PnpmGlobalLatest,
    /// Update by re-running the standalone installer from <https://snai.cc.cd/i.sh>.
    StandaloneUnix,
    /// Update by re-running the standalone installer from <https://snai.cc.cd/i.ps1>.
    StandaloneWindows,
}

impl UpdateAction {
    #[cfg(any(not(debug_assertions), test))]
    pub(crate) fn from_install_context(context: &InstallContext) -> Option<Self> {
        match &context.method {
            InstallMethod::Npm => Some(UpdateAction::NpmGlobalLatest),
            InstallMethod::Bun => Some(UpdateAction::BunGlobalLatest),
            InstallMethod::Pnpm => Some(UpdateAction::PnpmGlobalLatest),
            InstallMethod::Standalone { platform, .. } => Some(match platform {
                StandalonePlatform::Unix => UpdateAction::StandaloneUnix,
                StandalonePlatform::Windows => UpdateAction::StandaloneWindows,
            }),
            // NaiCode has no Homebrew cask — running `brew upgrade` would pull
            // upstream Codex and replace this install.
            InstallMethod::Brew | InstallMethod::Other => None,
        }
    }

    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => ("npm", &["install", "-g", "naicode"]),
            UpdateAction::BunGlobalLatest => ("bun", &["install", "-g", "naicode"]),
            UpdateAction::PnpmGlobalLatest => ("pnpm", &["add", "-g", "naicode"]),
            UpdateAction::StandaloneUnix => (
                "sh",
                &["-c", "curl -fsSL https://snai.cc.cd/i.sh | CODEX_NON_INTERACTIVE=1 sh"],
            ),
            UpdateAction::StandaloneWindows => (
                "powershell",
                &[
                    "-ExecutionPolicy",
                    "Bypass",
                    "-c",
                    "$env:CODEX_NON_INTERACTIVE=1; irm https://snai.cc.cd/i.ps1 | iex",
                ],
            ),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(not(debug_assertions))]
pub fn get_update_action() -> Option<UpdateAction> {
    UpdateAction::from_install_context(InstallContext::current())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_install_context::StandalonePlatform;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn maps_install_context_to_update_action() {
        let native_release_dir =
            AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("native-release"))
                .expect("temp dir path should be absolute");

        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Other,
                package_layout: None,
            }),
            None
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Npm,
                package_layout: None,
            }),
            Some(UpdateAction::NpmGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Bun,
                package_layout: None,
            }),
            Some(UpdateAction::BunGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Pnpm,
                package_layout: None,
            }),
            Some(UpdateAction::PnpmGlobalLatest)
        );
        // NaiCode has no Homebrew cask — must not run brew upgrade.
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Brew,
                package_layout: None,
            }),
            None
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Standalone {
                    platform: StandalonePlatform::Unix,
                    release_dir: native_release_dir.clone(),
                    resources_dir: Some(native_release_dir.join("codex-resources")),
                },
                package_layout: None,
            }),
            Some(UpdateAction::StandaloneUnix)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Standalone {
                    platform: StandalonePlatform::Windows,
                    release_dir: native_release_dir.clone(),
                    resources_dir: Some(native_release_dir.join("codex-resources")),
                },
                package_layout: None,
            }),
            Some(UpdateAction::StandaloneWindows)
        );
    }

    #[test]
    fn update_commands_target_naicode() {
        assert_eq!(
            UpdateAction::NpmGlobalLatest.command_args(),
            ("npm", &["install", "-g", "naicode"][..]),
        );
        assert_eq!(
            UpdateAction::BunGlobalLatest.command_args(),
            ("bun", &["install", "-g", "naicode"][..]),
        );
        assert_eq!(
            UpdateAction::PnpmGlobalLatest.command_args(),
            ("pnpm", &["add", "-g", "naicode"][..]),
        );
        assert_eq!(
            UpdateAction::NpmGlobalLatest.command_str(),
            "npm install -g naicode"
        );
        assert_eq!(
            UpdateAction::StandaloneUnix.command_args(),
            (
                "sh",
                &["-c", "curl -fsSL https://snai.cc.cd/i.sh | CODEX_NON_INTERACTIVE=1 sh"][..],
            )
        );
        assert_eq!(
            UpdateAction::StandaloneWindows.command_args(),
            (
                "powershell",
                &[
                    "-ExecutionPolicy",
                    "Bypass",
                    "-c",
                    "$env:CODEX_NON_INTERACTIVE=1; irm https://snai.cc.cd/i.ps1 | iex",
                ][..],
            )
        );
    }
}
