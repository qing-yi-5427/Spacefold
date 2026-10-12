//! Select only the Fixed Runtime shipped beside the executable, before Tauri starts threads.
//! An ordinary installation without that directory continues to use the system Runtime.
use std::{
    ffi::OsStr,
    fs, io,
    os::windows::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
};
use windows::{
    core::PCWSTR,
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        },
        System::SystemInformation::{GetSystemDirectoryW, OSVERSIONINFOW},
        UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
    },
};

const RUNTIME_DIRECTORY: &str = "WebView2Runtime";
const RUNTIME_EXECUTABLE: &str = "msedgewebview2.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const READ_EXECUTE_GRANTS: [&str; 2] = ["*S-1-15-2-2:(OI)(CI)(RX)", "*S-1-15-2-1:(OI)(CI)(RX)"];

fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn normal_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(error(format!(
            "运行时目录包含链接或重解析点：{}。请重新解压完整便携包到本地普通目录。",
            path.display()
        )));
    }
    Ok(metadata)
}

fn bundled_runtime(
    executable: &Path,
    explicit_override: Option<&OsStr>,
) -> io::Result<Option<PathBuf>> {
    // Respect deliberate developer/administrator WebView2 selection. An empty value
    // is equivalent to no selection and must not disable the bundled Runtime.
    if explicit_override.is_some_and(|value| !value.is_empty()) {
        return Ok(None);
    }
    let parent = executable
        .parent()
        .ok_or_else(|| error("无法确定 Spacefold 程序所在目录。"))?;
    let parent = fs::canonicalize(parent)?;
    let runtime = parent.join(RUNTIME_DIRECTORY);
    let metadata = match normal_metadata(&runtime) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let runtime_executable = normal_metadata(&runtime.join(RUNTIME_EXECUTABLE))
        .map_err(|e| error(format!("WebView2Runtime 不完整，请重新解压完整便携包：{e}")))?;
    if !metadata.is_dir() || !runtime_executable.is_file() {
        return Err(error(
            "WebView2Runtime 不完整。请重新解压完整便携包，不要只复制 Spacefold.exe。",
        ));
    }
    // No directory search or current-working-directory fallback: the only candidate
    // is this exact child of the executable's canonical package directory.
    if fs::canonicalize(&runtime)? != runtime {
        return Err(error("运行时位置与程序包目录不一致。请重新解压便携包。"));
    }
    Ok(Some(runtime))
}

fn validate_acl_tree(runtime: &Path) -> io::Result<()> {
    // Check the whole fixed tree before inheritance is granted. Do not follow links,
    // edit the executable's parent, or grant write/owner rights.
    let mut directories = vec![runtime.to_owned()];
    while let Some(directory) = directories.pop() {
        if !normal_metadata(&directory)?.is_dir() {
            return Err(error("运行时目录已改变。请重新解压便携包。"));
        }
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if normal_metadata(&path)?.is_dir() {
                directories.push(path);
            }
        }
    }
    Ok(())
}

fn windows_10() -> io::Result<bool> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion reports the real build without compatibility-manifest version lies.
    unsafe { RtlGetVersion(&mut version) }
        .ok()
        .map_err(|e| error(format!("无法确认 Windows 版本：{e}")))?;
    Ok(version.dwMajorVersion == 10 && version.dwBuildNumber < 22_000)
}

fn prepare_windows_10_permissions(runtime: &Path) -> io::Result<()> {
    // Microsoft requires these two inherited RX grants for unpackaged Fixed >=120
    // on Windows 10. ZIP files do not preserve NTFS ACLs. Windows 11 skips this work.
    // Keep the checked root from being renamed/replaced during preparation. This
    // handle does not request write/delete rights or change the parent directory.
    let _root_handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .open(runtime)?;
    validate_acl_tree(runtime)?;
    let mut buffer = [0u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(error("无法定位 Windows System32，不能准备便携运行时权限。"));
    }
    let system = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
    let result = Command::new(system.join("icacls.exe"))
        .arg(runtime)
        .arg("/grant")
        .args(READ_EXECUTE_GRANTS)
        .arg("/L")
        .arg("/Q")
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !result.status.success() {
        return Err(error(format!(
            "无法为包内 WebView2Runtime 准备 Windows 10 所需的读取执行权限（{}）。请把完整便携包解压到您可写的本地文件夹。\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}

pub(super) fn initialise() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let override_folder = std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER");
    if let Some(runtime) = bundled_runtime(&executable, override_folder.as_deref())? {
        if windows_10()? {
            prepare_windows_10_permissions(&runtime)?;
        }
        // Called at main entry before Tauri, Service or any worker is started.
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", runtime);
    }
    Ok(())
}

pub(super) fn show_startup_error(error: &io::Error) {
    let message: Vec<u16> = format!("Spacefold 便携运行时启动失败。\n\n{error}")
        .encode_utf16()
        .chain([0])
        .collect();
    let title: Vec<u16> = "Spacefold".encode_utf16().chain([0]).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_build_without_runtime_retains_system_selection() {
        let package = tempfile::tempdir().unwrap();
        assert!(bundled_runtime(&package.path().join("Spacefold.exe"), None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn runtime_is_resolved_from_executable_and_explicit_override_wins() {
        let package = tempfile::tempdir().unwrap();
        let runtime = package.path().join(RUNTIME_DIRECTORY);
        fs::create_dir(&runtime).unwrap();
        fs::write(runtime.join(RUNTIME_EXECUTABLE), b"test fixture").unwrap();
        let executable = package.path().join("Spacefold.exe");
        assert_eq!(
            bundled_runtime(&executable, Some(OsStr::new(""))).unwrap(),
            Some(fs::canonicalize(&runtime).unwrap())
        );
        assert!(
            bundled_runtime(&executable, Some(OsStr::new("C:\\explicit-runtime")))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn incomplete_bundled_runtime_fails_instead_of_claiming_system_fallback() {
        let package = tempfile::tempdir().unwrap();
        fs::create_dir(package.path().join(RUNTIME_DIRECTORY)).unwrap();
        assert!(bundled_runtime(&package.path().join("Spacefold.exe"), None).is_err());
    }

    #[test]
    fn acl_validation_rejects_nested_reparse_without_touching_its_target() {
        let package = tempfile::tempdir().unwrap();
        let runtime = package.path().join(RUNTIME_DIRECTORY);
        let outside = package.path().join("outside");
        fs::create_dir(&runtime).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel.txt"), b"unchanged").unwrap();
        let status = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(runtime.join("junction"))
            .arg(&outside)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(validate_acl_tree(&runtime).is_err());
        assert_eq!(
            fs::read(outside.join("sentinel.txt")).unwrap(),
            b"unchanged"
        );
    }
}
