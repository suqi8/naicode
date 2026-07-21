//! CLI handling for local state database startup failures.
//!
//! This keeps user-facing backup and lock-contention handling out of the main
//! CLI dispatch path while preserving the TUI startup error as the boundary type.

use codex_state::RuntimeDbBackup;
use codex_tui::LocalStateDbStartupError;
use std::io::IsTerminal;
use std::path::Path;

pub(crate) fn startup_error(err: &std::io::Error) -> Option<&LocalStateDbStartupError> {
    err.get_ref()
        .and_then(|err| err.downcast_ref::<LocalStateDbStartupError>())
}

pub(crate) fn is_locked(detail: &str) -> bool {
    codex_state::sqlite_error_detail_is_lock(detail)
}

pub(crate) fn is_corruption(detail: &str) -> bool {
    codex_state::sqlite_error_detail_is_corruption(detail)
}

pub(crate) fn is_auto_backup_recoverable(startup_error: &LocalStateDbStartupError) -> bool {
    is_corruption(startup_error.detail()) || sqlite_home_is_blocking_file(startup_error)
}

fn sqlite_home_is_blocking_file(startup_error: &LocalStateDbStartupError) -> bool {
    startup_error
        .database_path()
        .parent()
        .and_then(|path| std::fs::metadata(path).ok())
        .is_some_and(|metadata| metadata.is_file())
}

pub(crate) fn print_auto_backup_start(startup_error: &LocalStateDbStartupError) {
    eprintln!("NaiCode 无法启动，本地数据库可能已损坏。");
    eprintln!("正在备份损坏的数据库，NaiCode 将使用已保存的数据重新建立数据库。");
    print_technical_details(startup_error);
}

pub(crate) async fn backup_files_for_fresh_start(
    startup_error: &LocalStateDbStartupError,
) -> std::io::Result<Vec<RuntimeDbBackup>> {
    codex_state::backup_runtime_db_for_fresh_start(startup_error.database_path()).await
}

pub(crate) fn confirm_fresh_start_rebuild(
    startup_error: &LocalStateDbStartupError,
    backups: &[RuntimeDbBackup],
) -> std::io::Result<()> {
    eprintln!("NaiCode 已重新建立本地数据库。");
    eprintln!("NaiCode 检测到本地数据库损坏，已将其移至备份目录，并将使用新的数据库继续启动。");
    eprintln!("数据库路径：{}", startup_error.database_path().display());
    if let Some(backup_folder) = backup_folder(backups) {
        eprintln!("备份目录：{}", backup_folder.display());
    } else {
        eprintln!("备份目录：不可用");
    }

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("按 Enter 继续。");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
    } else {
        eprintln!("正在使用新的本地数据库继续启动...");
    }
    Ok(())
}

pub(crate) fn print_diagnostic_guidance(startup_error: &LocalStateDbStartupError) {
    eprintln!("NaiCode 无法启动，本地数据库可能已损坏。");
    eprintln!("请运行 `naicode doctor` 检查环境并获取修复建议。");
    eprintln!("如果问题持续出现，请在寻求帮助时提供下方技术详情。");
    print_technical_details(startup_error);
}

pub(crate) fn print_locked_guidance(startup_error: &LocalStateDbStartupError) {
    eprintln!("NaiCode 无法启动，另一个 NaiCode 进程正在使用本地数据。");
    eprintln!("请退出其他仍在运行的 NaiCode 进程，然后重试。");
    print_technical_details(startup_error);
}

fn print_technical_details(startup_error: &LocalStateDbStartupError) {
    eprintln!("技术详情：");
    eprintln!("  位置：{}", startup_error.database_path().display());
    eprintln!("  原因：{}", startup_error.detail());
}

fn backup_folder(backups: &[RuntimeDbBackup]) -> Option<&Path> {
    backups.first()?.backup_path.parent()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[tokio::test]
    async fn backup_backs_up_only_failed_database_file() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let state_path = codex_state::state_db_path(temp_dir.path());
        let failed_db_path = codex_state::logs_db_path(temp_dir.path());
        tokio::fs::write(state_path.as_path(), b"state").await?;
        tokio::fs::write(failed_db_path.as_path(), b"logs").await?;

        let startup_error =
            LocalStateDbStartupError::new(failed_db_path.clone(), "corrupt".to_string());
        let backups = backup_files_for_fresh_start(&startup_error).await?;

        assert_eq!(
            backups
                .iter()
                .map(|backup| &backup.original_path)
                .collect::<Vec<_>>(),
            vec![&failed_db_path]
        );
        assert!(!tokio::fs::try_exists(failed_db_path.as_path()).await?);
        assert!(tokio::fs::try_exists(state_path.as_path()).await?);
        assert!(tokio::fs::try_exists(backups[0].backup_path.as_path()).await?);
        Ok(())
    }

    #[tokio::test]
    async fn backup_replaces_blocking_sqlite_home_file() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let sqlite_home = temp_dir.path().join("sqlite-home");
        tokio::fs::write(sqlite_home.as_path(), b"not-a-directory").await?;
        let startup_error = LocalStateDbStartupError::new(
            codex_state::state_db_path(sqlite_home.as_path()),
            "File exists".to_string(),
        );

        assert!(is_auto_backup_recoverable(&startup_error));
        let backups = backup_files_for_fresh_start(&startup_error).await?;

        assert_eq!(backups.len(), 1);
        assert!(tokio::fs::metadata(sqlite_home.as_path()).await?.is_dir());
        assert!(tokio::fs::try_exists(backups[0].backup_path.as_path()).await?);
        Ok(())
    }

    #[test]
    fn backup_folder_uses_parent_of_first_backup_path() {
        let backups = vec![RuntimeDbBackup {
            original_path: PathBuf::from("/tmp/state_5.sqlite"),
            backup_path: PathBuf::from("/tmp/db-backups/sqlite-1-0/state_5.sqlite"),
        }];

        assert_eq!(
            backup_folder(&backups),
            Some(Path::new("/tmp/db-backups/sqlite-1-0"))
        );
    }
}
