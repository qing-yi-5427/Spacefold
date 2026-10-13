//! Filesystem metadata never reads file contents. Display text is never an operation path.
use crate::model::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    pub kind: String,
    pub logical: u64,
    pub allocated: Option<u64>,
    pub modified: Option<String>,
    pub identity: Option<String>,
    pub links: Option<u64>,
    pub attributes: u32,
    pub reparse_tag: u32,
    pub link_type: Option<String>,
    pub link_target: Option<String>,
    pub cloud_state: Option<String>,
    pub allocation_method: Option<String>,
    pub recurse: bool,
}
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub hint: Option<Metadata>,
}
impl DirectoryEntry {
    /// Only ordinary files can bypass the full no-recall metadata worker.
    /// Enumeration cannot supply link counts or link targets for special files.
    pub fn scan_file_metadata(&self) -> Option<&Metadata> {
        let m = self.hint.as_ref()?;
        if m.kind != "file"
            || m.link_type.is_some()
            || m.cloud_state.is_some()
            || m.reparse_tag != 0
        {
            return None;
        }
        #[cfg(windows)]
        {
            use windows::Win32::Storage::FileSystem::*;
            if m.attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT.0
                    | FILE_ATTRIBUTE_OFFLINE.0
                    | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
                    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0)
                != 0
            {
                return None;
            }
        }
        if m.allocation_method.as_deref() == Some("FILE_ID_EXTD_DIR_INFO.AllocationSize")
            && m.identity.is_none()
        {
            return None;
        }
        Some(m)
    }
}
#[cfg(not(windows))]
#[derive(Clone, Default)]
pub struct ScanDirectoryReader;
#[cfg(not(windows))]
pub struct PreparedScanDirectory(PathBuf);
#[cfg(not(windows))]
impl ScanDirectoryReader {
    pub fn new(_root: &Path, _fast: bool) -> Self {
        Self
    }
    pub fn prepare(&self, path: &Path) -> Result<PreparedScanDirectory> {
        validate_scan_directory(path)?;
        Ok(PreparedScanDirectory(path.to_owned()))
    }
}
#[cfg(not(windows))]
impl PreparedScanDirectory {
    pub fn entries(self) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>>>> {
        read_dir(&self.0)
    }
}
fn validate_scan_directory(path: &Path) -> Result<()> {
    let m = metadata(path)?;
    if m.recurse && m.kind == "directory" {
        Ok(())
    } else {
        Err(AppError::new(
            "DIRECTORY_CHANGED",
            "目录在扫描中变为跳转链接或不可本地枚举，未跟随",
        ))
    }
}
#[cfg(not(windows))]
pub fn read_dir(path: &Path) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>>>> {
    let iter = fs::read_dir(path)?;
    Ok(Box::new(iter.map(|r| {
        r.map(|e| DirectoryEntry {
            path: e.path(),
            hint: None,
        })
        .map_err(AppError::from)
    })))
}
impl Metadata {
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            identity: self.identity.clone(),
            kind: self.kind.clone(),
            logical: self.logical,
            modified: self.modified.clone(),
            attributes: self.attributes,
            reparse_tag: self.reparse_tag,
        }
    }
}
pub fn display(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(p) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{p}")
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned()
    }
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::new("INVALID_PATH", "请输入绝对路径"));
    }
    if path.as_os_str().is_empty() {
        return Err(AppError::new("INVALID_PATH", "路径不能为空"));
    }
    Ok(fs::canonicalize(path)?)
}
/// Shell strips Win32 special suffixes even when the filesystem can preserve them.
pub fn validate_shell_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                let units = name.encode_wide().collect::<Vec<_>>();
                if units.last().is_some_and(|c| *c == 46 || *c == 32) {
                    return Err(AppError::new("UNSUPPORTED_SHELL_PATH","路径分量以点或空格结尾，Windows Shell 可能指向另一对象，已拒绝此操作；扫描与 JSON 仍保留原路径"));
                }
            }
        }
    }
    Ok(())
}
/// Keep UTF-16 code units, including unpaired surrogates, through CF_UNICODETEXT.
#[cfg(windows)]
pub fn copy_raw_paths(paths: &[PathBuf], owner: isize) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::{
        Foundation::*,
        System::{DataExchange::*, Memory::*, Ole::CF_UNICODETEXT},
    };
    let mut units = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if index != 0 {
            units.extend_from_slice(&[13, 10]);
        }
        let raw = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if validate_shell_path(path).is_err() {
            // The verbatim prefix is required to identify a special suffix
            // accurately when a caller later parses the copied path.
            units.extend_from_slice(&raw);
        } else if raw.starts_with(&[92, 92, 63, 92, 85, 78, 67, 92]) {
            units.extend_from_slice(&[92, 92]);
            units.extend_from_slice(&raw[8..]);
        } else if raw.starts_with(&[92, 92, 63, 92]) {
            units.extend_from_slice(&raw[4..]);
        } else {
            units.extend_from_slice(&raw);
        }
        if units.len() * 2 > 2 * 1024 * 1024 {
            return Err(AppError::new(
                "CLIPBOARD_LIMIT",
                "所选原始路径超过剪贴板文本预算",
            ));
        }
    }
    units.push(0);
    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    struct Allocation(Option<HGLOBAL>);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if let Some(handle) = self.0 {
                unsafe {
                    let _ = GlobalFree(Some(handle));
                }
            }
        }
    }
    let mut opened = false;
    for _ in 0..20 {
        if unsafe { OpenClipboard(Some(HWND(owner as *mut _))) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err(AppError::new(
            "CLIPBOARD_ERROR",
            "剪贴板正被其他程序使用，请重试",
        ));
    }
    let _clipboard = Clipboard;
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) }.map_err(win_error)?;
    let mut allocation = Allocation(Some(handle));
    let memory = unsafe { GlobalLock(handle) };
    if memory.is_null() {
        return Err(AppError::new("CLIPBOARD_ERROR", "无法锁定剪贴板内存"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), memory.cast::<u16>(), units.len());
        let _ = GlobalUnlock(handle);
    }
    unsafe { EmptyClipboard() }.map_err(win_error)?;
    unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0))) }
        .map_err(win_error)?;
    allocation.0 = None; // Clipboard owns the allocation after successful transfer.
    Ok(())
}
pub fn path_encoding(path: &Path) -> (&'static str, String, bool) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        let valid = char::decode_utf16(units.iter().copied()).all(|c| c.is_ok());
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut data = String::with_capacity(units.len() * 4);
        for unit in units {
            for byte in unit.to_le_bytes() {
                data.push(HEX[(byte >> 4) as usize] as char);
                data.push(HEX[(byte & 15) as usize] as char);
            }
        }
        ("utf16le", data, !valid)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        (
            "unix_bytes",
            bytes.iter().map(|b| format!("{b:02x}")).collect(),
            std::str::from_utf8(bytes).is_err(),
        )
    }
}
pub fn encode_path(path: &Path) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
}
pub fn decode_path(data: Vec<u8>) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_wide(
            &data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ))
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(data))
    }
}
#[cfg(not(windows))]
pub fn metadata(path: &Path) -> Result<Metadata> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path)?;
    let islink = m.file_type().is_symlink();
    let kind = if islink {
        "symlink"
    } else if m.is_dir() {
        "directory"
    } else if m.is_file() {
        "file"
    } else {
        "other"
    };
    Ok(Metadata {
        kind: kind.into(),
        logical: if m.is_file() { m.len() } else { 0 },
        allocated: if m.is_file() {
            Some(
                m.blocks()
                    .checked_mul(512)
                    .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "分配大小溢出"))?,
            )
        } else {
            Some(0)
        },
        modified: m.modified().ok().map(time_string),
        identity: Some(format!("{}:{}", m.dev(), m.ino())),
        links: Some(m.nlink()),
        attributes: 0,
        reparse_tag: 0,
        link_type: islink.then(|| "symlink".into()),
        link_target: if islink {
            fs::read_link(path).ok().map(|p| display(&p))
        } else {
            None
        },
        cloud_state: None,
        allocation_method: Some("stat.st_blocks".into()),
        recurse: m.is_dir() && !islink,
    })
}
#[cfg(not(windows))]
pub fn disk_space(_path: &Path) -> Option<DiskSpace> {
    None
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
            Storage::FileSystem::*,
        },
    };
    pub fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    pub fn win_error(e: windows::core::Error) -> AppError {
        let v = e.code().0;
        let os = v & 0xffff;
        let code = match os {
            2 | 3 => "NOT_FOUND",
            5 => "ACCESS_DENIED",
            32 | 33 => "FILE_IN_USE",
            80 | 183 => "DESTINATION_CONFLICT",
            1223 => "CANCELLED",
            _ => "SYSTEM_ERROR",
        };
        AppError {
            code: code.into(),
            message: e.to_string(),
            retryable: true,
            node_id: None,
            os_code: Some(os),
        }
    }
    pub fn metadata(path: &Path) -> Result<Metadata> {
        let m = fs::symlink_metadata(path)?;
        let w = wide(path);
        let h = unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
                None,
            )
        }
        .map_err(win_error)?;
        if h == INVALID_HANDLE_VALUE {
            return Err(AppError::new("METADATA_ERROR", "无法取得文件元数据"));
        }
        let handle = OwnedHandle(h);
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileAttributeTagInfo,
                &mut tag as *mut _ as *mut _,
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        }
        .map_err(win_error)?;
        let mut standard = FILE_STANDARD_INFO::default();
        let allocation_ok = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileStandardInfo,
                &mut standard as *mut _ as *mut _,
                std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        }
        .is_ok();
        let mut id = FILE_ID_INFO::default();
        let hasid = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileIdInfo,
                &mut id as *mut _ as *mut _,
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        }
        .is_ok();
        let mut legacy = BY_HANDLE_FILE_INFORMATION::default();
        let haslegacy = unsafe { GetFileInformationByHandle(handle.0, &mut legacy) }.is_ok();
        let attrs = tag.FileAttributes;
        let isdir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
        // Name surrogate bit distinguishes directory redirections from cloud placeholders.
        let surrogate = reparse && (tag.ReparseTag & 0x20000000 != 0);
        let is_cloud = reparse
            && ((tag.ReparseTag & 0xffff0fff) == 0x9000001a || tag.ReparseTag == 0x80000021);
        let recall = attrs
            & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
                | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
                | FILE_ATTRIBUTE_OFFLINE.0)
            != 0;
        let kind = if surrogate {
            if tag.ReparseTag == 0xa0000003 {
                "junction"
            } else {
                "symlink"
            }
        } else if isdir {
            "directory"
        } else if m.is_file() {
            "file"
        } else {
            "other"
        };
        let logical = if !isdir && !surrogate { m.len() } else { 0 };
        integer(logical)?;
        let allocated = if isdir || surrogate {
            Some(0)
        } else if allocation_ok && standard.AllocationSize >= 0 {
            Some(standard.AllocationSize as u64)
        } else {
            None
        };
        let identity = if hasid {
            Some(format!(
                "{:016x}:{}",
                id.VolumeSerialNumber,
                id.FileId
                    .Identifier
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ))
        } else if haslegacy {
            Some(format!(
                "{:08x}:{:08x}{:08x}",
                legacy.dwVolumeSerialNumber, legacy.nFileIndexHigh, legacy.nFileIndexLow
            ))
        } else {
            None
        };
        Ok(Metadata {
            kind: kind.into(),
            logical,
            allocated,
            modified: m.modified().ok().map(time_string),
            identity,
            links: haslegacy.then_some(legacy.nNumberOfLinks as u64),
            attributes: attrs,
            reparse_tag: tag.ReparseTag,
            link_type: surrogate.then(|| format!("reparse:0x{:08x}", tag.ReparseTag)),
            link_target: if surrogate {
                fs::read_link(path).ok().map(|p| display(&p))
            } else {
                None
            },
            cloud_state: if is_cloud || recall {
                Some(
                    if recall {
                        "placeholder_not_recalled"
                    } else {
                        "cloud_metadata_only"
                    }
                    .into(),
                )
            } else {
                None
            },
            allocation_method: allocated.map(|_| "FILE_STANDARD_INFO.AllocationSize".into()),
            recurse: isdir && !surrogate && !recall && !is_cloud,
        })
    }
    struct DirectoryIterator {
        handle: OwnedFindHandle,
        root: PathBuf,
        current: Option<WIN32_FIND_DATAW>,
        done: bool,
    }
    struct OwnedFindHandle(HANDLE);
    impl Drop for OwnedFindHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = FindClose(self.0);
            }
        }
    }
    impl Iterator for DirectoryIterator {
        type Item = Result<DirectoryEntry>;
        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if self.done {
                    return None;
                }
                let data = if let Some(data) = self.current.take() {
                    data
                } else {
                    let mut data = WIN32_FIND_DATAW::default();
                    if let Err(error) = unsafe { FindNextFileW(self.handle.0, &mut data) } {
                        self.done = true;
                        if error.code().0 & 0xffff == 18 {
                            return None;
                        }
                        return Some(Err(win_error(error)));
                    }
                    data
                };
                let end = data
                    .cFileName
                    .iter()
                    .position(|v| *v == 0)
                    .unwrap_or(data.cFileName.len());
                let name = &data.cFileName[..end];
                if name == [46] || name == [46, 46] {
                    continue;
                }
                use std::os::windows::ffi::OsStringExt;
                let path = self.root.join(std::ffi::OsString::from_wide(name));
                let attrs = data.dwFileAttributes;
                let isdir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
                let tag = if reparse { data.dwReserved0 } else { 0 };
                let surrogate = reparse && tag & 0x20000000 != 0;
                let cloud = reparse && ((tag & 0xffff0fff) == 0x9000001a || tag == 0x80000021);
                let recall = attrs
                    & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
                        | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
                        | FILE_ATTRIBUTE_OFFLINE.0)
                    != 0;
                let kind = if surrogate {
                    if tag == 0xa0000003 {
                        "junction"
                    } else {
                        "symlink"
                    }
                } else if isdir {
                    "directory"
                } else {
                    "file"
                };
                let ticks = ((data.ftLastWriteTime.dwHighDateTime as u64) << 32)
                    | data.ftLastWriteTime.dwLowDateTime as u64;
                let unix_ms = ticks as i128 / 10_000 - 11_644_473_600_000i128;
                let modified = i64::try_from(unix_ms)
                    .ok()
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                let logical = if isdir || surrogate {
                    0
                } else {
                    ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64
                };
                let hint = Metadata {
                    kind: kind.into(),
                    logical,
                    allocated: if isdir || surrogate { Some(0) } else { None },
                    modified,
                    identity: None,
                    links: None,
                    attributes: attrs,
                    reparse_tag: tag,
                    link_type: surrogate.then(|| format!("reparse:0x{tag:08x}")),
                    link_target: None,
                    cloud_state: if cloud || recall {
                        Some(
                            if recall {
                                "placeholder_not_recalled"
                            } else {
                                "cloud_metadata_only"
                            }
                            .into(),
                        )
                    } else {
                        None
                    },
                    allocation_method: None,
                    recurse: isdir && !surrogate && !cloud && !recall,
                };
                return Some(Ok(DirectoryEntry {
                    path,
                    hint: Some(hint),
                }));
            }
        }
    }
    pub fn read_dir(path: &Path) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>>>> {
        let pattern = path.join("*");
        let w = wide(&pattern);
        let mut first = WIN32_FIND_DATAW::default();
        let flags = FIND_FIRST_EX_LARGE_FETCH | FIND_FIRST_EX_ON_DISK_ENTRIES_ONLY;
        let handle = unsafe {
            FindFirstFileExW(
                PCWSTR(w.as_ptr()),
                FindExInfoBasic,
                &mut first as *mut _ as *mut _,
                FindExSearchNameMatch,
                None,
                flags,
            )
        };
        let handle = match handle {
            Ok(h) => h,
            Err(e) if e.code().0 & 0xffff == 87 => unsafe {
                FindFirstFileExW(
                    PCWSTR(w.as_ptr()),
                    FindExInfoBasic,
                    &mut first as *mut _ as *mut _,
                    FindExSearchNameMatch,
                    None,
                    FIND_FIRST_EX_LARGE_FETCH,
                )
            }
            .map_err(win_error)?,
            Err(e) if e.code().0 & 0xffff == 2 => return Ok(Box::new(std::iter::empty())),
            Err(e) => return Err(win_error(e)),
        };
        Ok(Box::new(DirectoryIterator {
            handle: OwnedFindHandle(handle),
            root: path.to_owned(),
            current: Some(first),
            done: false,
        }))
    }

    // Fixed-size per reader, separate from the bounded encoded-output chunks.
    // Public Win32 enumeration works on the exact handle whose attributes were
    // checked. No raw-volume access, backup privilege, or content reads are used.
    const DIRECTORY_BUFFER_BYTES: usize = 64 * 1024;
    #[derive(Clone)]
    pub struct ScanDirectoryReader {
        ntfs_volume: Option<u64>,
    }
    pub struct PreparedScanDirectory {
        path: PathBuf,
        fast: Option<BulkDirectoryIterator>,
    }
    impl ScanDirectoryReader {
        pub fn new(root: &Path, fast: bool) -> Self {
            let ntfs_volume = fast.then(|| local_ntfs_volume(root)).flatten();
            Self { ntfs_volume }
        }
        pub fn prepare(&self, path: &Path) -> Result<PreparedScanDirectory> {
            let Some(volume) = self.ntfs_volume else {
                validate_scan_directory(path)?;
                return Ok(PreparedScanDirectory {
                    path: path.to_owned(),
                    fast: None,
                });
            };
            let handle = open_directory(path, true)?;
            let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
            unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    FileAttributeTagInfo,
                    &mut tag as *mut _ as *mut _,
                    std::mem::size_of_val(&tag) as u32,
                )
            }
            .map_err(win_error)?;
            let attributes = tag.FileAttributes;
            let reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
            let cloud = reparse
                && ((tag.ReparseTag & 0xffff0fff) == 0x9000001a || tag.ReparseTag == 0x80000021);
            let recall = attributes
                & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
                    | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
                    | FILE_ATTRIBUTE_OFFLINE.0)
                != 0;
            if attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
                || reparse && tag.ReparseTag & 0x20000000 != 0
                || cloud
                || recall
            {
                return Err(AppError::new(
                    "DIRECTORY_CHANGED",
                    "目录在扫描中变为跳转链接或不可本地枚举，未跟随",
                ));
            }
            let id = handle_id(handle.0);
            if id.as_ref().is_none_or(|id| id.VolumeSerialNumber != volume) {
                // Filesystem/identity capability changed. Revalidate the legacy
                // path before opening it; never reinterpret another volume's ID.
                drop(handle);
                validate_scan_directory(path)?;
                return Ok(PreparedScanDirectory {
                    path: path.to_owned(),
                    fast: None,
                });
            }
            Ok(PreparedScanDirectory {
                path: path.to_owned(),
                fast: Some(BulkDirectoryIterator {
                    handle,
                    root: path.to_owned(),
                    volume,
                    buffer: vec![0u64; DIRECTORY_BUFFER_BYTES / 8],
                    offset: None,
                    done: false,
                    first: true,
                    #[cfg(test)]
                    query_fault: None,
                }),
            })
        }
    }
    impl PreparedScanDirectory {
        pub fn entries(self) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>>>> {
            let Some(mut fast) = self.fast else {
                return read_dir(&self.path);
            };
            match fast.refill() {
                Ok(()) => Ok(Box::new(fast)),
                Err(error)
                    if error
                        .os_code
                        .is_some_and(|code| matches!(code, 1 | 50 | 87 | 120 | 124)) =>
                {
                    // Only the initial unsupported query may select fallback.
                    // Once any entry was returned, errors stay errors; restarting
                    // the directory would duplicate already persisted entries.
                    drop(fast);
                    validate_scan_directory(&self.path)?;
                    read_dir(&self.path)
                }
                Err(error) => Err(error),
            }
        }
    }
    fn open_directory(path: &Path, list: bool) -> Result<OwnedHandle> {
        let path = wide(path);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                FILE_READ_ATTRIBUTES.0 | if list { FILE_LIST_DIRECTORY.0 } else { 0 },
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_OPEN_NO_RECALL,
                None,
            )
        }
        .map_err(win_error)?;
        Ok(OwnedHandle(handle))
    }
    fn handle_id(handle: HANDLE) -> Option<FILE_ID_INFO> {
        let mut id = FILE_ID_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                &mut id as *mut _ as *mut _,
                std::mem::size_of_val(&id) as u32,
            )
        }
        .ok()
        .map(|_| id)
    }
    fn local_ntfs_volume(root: &Path) -> Option<u64> {
        use std::path::{Component, Prefix};
        let drive = match root.components().next()? {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
                _ => return None,
            },
            _ => return None,
        };
        let drive_root = [drive as u16, 58, 92, 0];
        if unsafe { GetDriveTypeW(PCWSTR(drive_root.as_ptr())) } == 4 {
            return None;
        }
        let handle = open_directory(root, false).ok()?;
        let mut filesystem = [0u16; 32];
        unsafe {
            GetVolumeInformationByHandleW(handle.0, None, None, None, None, Some(&mut filesystem))
        }
        .ok()?;
        if !filesystem.starts_with(&[78, 84, 70, 83, 0]) {
            return None;
        }
        handle_id(handle.0).map(|id| id.VolumeSerialNumber)
    }
    struct BulkDirectoryIterator {
        handle: OwnedHandle,
        root: PathBuf,
        volume: u64,
        // u64 alignment also satisfies FILE_ID_EXTD_DIR_INFO alignment.
        buffer: Vec<u64>,
        offset: Option<usize>,
        done: bool,
        first: bool,
        #[cfg(test)]
        query_fault: Option<u32>,
    }
    impl BulkDirectoryIterator {
        fn refill(&mut self) -> Result<()> {
            self.offset = None;
            #[cfg(test)]
            if let Some(code) = self.query_fault.take() {
                return Err(win_error(windows::core::Error::from_hresult(
                    windows::core::HRESULT::from_win32(code),
                )));
            }
            self.buffer.fill(0);
            let class = if self.first {
                FileIdExtdDirectoryRestartInfo
            } else {
                FileIdExtdDirectoryInfo
            };
            self.first = false;
            let result = unsafe {
                GetFileInformationByHandleEx(
                    self.handle.0,
                    class,
                    self.buffer.as_mut_ptr().cast(),
                    DIRECTORY_BUFFER_BYTES as u32,
                )
            };
            match result {
                Ok(()) => {
                    self.offset = Some(0);
                    Ok(())
                }
                Err(error) if error.code().0 & 0xffff == 18 => {
                    self.done = true;
                    Ok(())
                }
                Err(error) => Err(win_error(error)),
            }
        }
        fn decode(&self, offset: usize) -> Result<(Option<usize>, Vec<u16>, Metadata)> {
            // GetFileInformationByHandleEx does not expose byte-count output.
            // Buffer is zeroed before each query and every field/name read is
            // checked against the fixed buffer and the next record's boundary.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.buffer.as_ptr().cast::<u8>(),
                    DIRECTORY_BUFFER_BYTES,
                )
            };
            decode_directory_record(bytes, offset, self.volume)
        }
    }
    fn decode_directory_record(
        bytes: &[u8],
        offset: usize,
        volume: u64,
    ) -> Result<(Option<usize>, Vec<u16>, Metadata)> {
        fn invalid() -> AppError {
            AppError::new(
                "DIRECTORY_DATA_INVALID",
                "目录枚举返回了无效或不完整的元数据",
            )
        }
        const HEADER: usize = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
        if !offset.is_multiple_of(8) {
            return Err(invalid());
        }
        let data = bytes.get(offset..).ok_or_else(invalid)?;
        if data.len() < HEADER {
            return Err(invalid());
        }
        let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let i64_at = |at: usize| i64::from_le_bytes(data[at..at + 8].try_into().unwrap());
        let step = u32_at(0) as usize;
        let len = u32_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength)) as usize;
        let boundary = if step == 0 { data.len() } else { step };
        if len == 0
            || len > 510
            || !len.is_multiple_of(2)
            || boundary > data.len()
            || boundary < HEADER
            || step != 0 && !step.is_multiple_of(8)
            || len > boundary - HEADER
        {
            return Err(invalid());
        }
        let name = data[HEADER..HEADER + len]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect::<Vec<_>>();
        if name.iter().any(|v| matches!(*v, 0 | 47 | 92)) {
            return Err(invalid());
        }
        let attrs = u32_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes));
        let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
        let tag = if reparse {
            u32_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag))
        } else {
            0
        };
        let isdir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        let surrogate = reparse && tag & 0x20000000 != 0;
        let cloud = reparse && ((tag & 0xffff0fff) == 0x9000001a || tag == 0x80000021);
        let recall = attrs
            & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
                | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
                | FILE_ATTRIBUTE_OFFLINE.0)
            != 0;
        let eof = i64_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, EndOfFile));
        let allocation = i64_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, AllocationSize));
        if eof < 0 || allocation < 0 {
            return Err(invalid());
        }
        let kind = if surrogate {
            if tag == 0xa0000003 {
                "junction"
            } else {
                "symlink"
            }
        } else if isdir {
            "directory"
        } else {
            "file"
        };
        let logical = if isdir || surrogate { 0 } else { eof as u64 };
        integer(logical)?;
        let allocated = if isdir || surrogate {
            Some(0)
        } else if cloud || recall {
            None
        } else {
            Some(allocation as u64)
        };
        let ticks = i64_at(std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, LastWriteTime));
        let unix_ms = ticks as i128 / 10_000 - 11_644_473_600_000i128;
        let modified = i64::try_from(unix_ms)
            .ok()
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let id_at = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileId);
        let id = &data[id_at..id_at + 16];
        let identity = id.iter().any(|v| *v != 0).then(|| {
            // One bounded string, rather than sixteen temporary hex strings per
            // entry. Format is exactly the existing FILE_ID_INFO identity.
            use std::fmt::Write;
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut identity = String::with_capacity(49);
            write!(&mut identity, "{volume:016x}:").unwrap();
            for byte in id {
                identity.push(HEX[(byte >> 4) as usize] as char);
                identity.push(HEX[(byte & 15) as usize] as char);
            }
            identity
        });
        Ok((
            if step == 0 { None } else { Some(offset + step) },
            name,
            Metadata {
                kind: kind.into(),
                logical,
                allocated,
                modified,
                identity,
                links: None,
                attributes: attrs,
                reparse_tag: tag,
                link_type: surrogate.then(|| format!("reparse:0x{tag:08x}")),
                link_target: None,
                cloud_state: (cloud || recall).then(|| {
                    if recall {
                        "placeholder_not_recalled"
                    } else {
                        "cloud_metadata_only"
                    }
                    .into()
                }),
                allocation_method: allocated.map(|_| "FILE_ID_EXTD_DIR_INFO.AllocationSize".into()),
                recurse: isdir && !surrogate && !cloud && !recall,
            },
        ))
    }
    impl Iterator for BulkDirectoryIterator {
        type Item = Result<DirectoryEntry>;
        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if self.done {
                    return None;
                }
                if self.offset.is_none() {
                    if let Err(error) = self.refill() {
                        self.done = true;
                        return Some(Err(error));
                    }
                    if self.done {
                        return None;
                    }
                }
                let decoded = self.decode(self.offset.unwrap());
                let (next, name, hint) = match decoded {
                    Ok(record) => record,
                    Err(error) => {
                        self.done = true;
                        return Some(Err(error));
                    }
                };
                self.offset = next;
                if name == [46] || name == [46, 46] {
                    continue;
                }
                use std::os::windows::ffi::OsStringExt;
                return Some(Ok(DirectoryEntry {
                    path: self.root.join(std::ffi::OsString::from_wide(&name)),
                    hint: Some(hint),
                }));
            }
        }
    }

    pub fn disk_space(path: &Path) -> Option<DiskSpace> {
        let w = wide(path);
        let (mut avail, mut total, mut free) = (0, 0, 0);
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(w.as_ptr()),
                Some(&mut avail),
                Some(&mut total),
                Some(&mut free),
            )
        }
        .ok()?;
        Some(DiskSpace {
            total_bytes: total.to_string(),
            free_bytes: free.to_string(),
            available_bytes: avail.to_string(),
        })
    }
    #[cfg(test)]
    mod bulk_directory_tests {
        use super::*;
        use std::{collections::BTreeMap, ffi::OsString, os::windows::ffi::OsStringExt};

        fn raw(path: &Path) -> Vec<u16> {
            path.as_os_str().encode_wide().collect()
        }
        fn entries(
            iter: Box<dyn Iterator<Item = Result<DirectoryEntry>>>,
        ) -> BTreeMap<Vec<u16>, Metadata> {
            iter.map(|entry| {
                let entry = entry.unwrap();
                (raw(&entry.path), entry.hint.unwrap())
            })
            .collect()
        }
        fn record(name: &[u16]) -> Vec<u8> {
            let header = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
            let mut bytes = vec![0u8; (header + name.len() * 2).next_multiple_of(8)];
            bytes[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength)..][..4]
                .copy_from_slice(&(name.len() as u32 * 2).to_le_bytes());
            bytes[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes)..][..4]
                .copy_from_slice(&FILE_ATTRIBUTE_NORMAL.0.to_le_bytes());
            bytes[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileId)] = 19;
            for (i, unit) in name.iter().enumerate() {
                bytes[header + i * 2..][..2].copy_from_slice(&unit.to_le_bytes());
            }
            bytes
        }
        #[test]
        fn checked_parser_preserves_raw_units_and_rejects_malformed_records() {
            let name = [97, 0xd800, 0xdc00, 0xdfff];
            let bytes = record(&name);
            let (next, actual, m) = decode_directory_record(&bytes, 0, 7).unwrap();
            assert_eq!(actual, name);
            assert_eq!(next, None);
            assert_eq!(
                m.identity.as_deref(),
                Some("0000000000000007:13000000000000000000000000000000")
            );
            let mut corruptions = vec![bytes[..30].to_vec()];
            for (at, value) in [
                (0usize, 1u32),
                (0, 8),
                (0, 999992),
                (
                    std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength),
                    1,
                ),
                (
                    std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength),
                    65536,
                ),
            ] {
                let mut corrupt = bytes.clone();
                corrupt[at..at + 4].copy_from_slice(&value.to_le_bytes());
                corruptions.push(corrupt);
            }
            for at in [
                std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, EndOfFile),
                std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, AllocationSize),
            ] {
                let mut corrupt = bytes.clone();
                corrupt[at..at + 8].copy_from_slice(&(-1i64).to_le_bytes());
                corruptions.push(corrupt);
            }
            for name in [[0u16], [92u16], [47u16]] {
                corruptions.push(record(&name));
            }
            for corrupt in corruptions {
                assert_eq!(
                    decode_directory_record(&corrupt, 0, 7).unwrap_err().code,
                    "DIRECTORY_DATA_INVALID"
                );
            }
            assert_eq!(
                decode_directory_record(&bytes, 1, 7).unwrap_err().code,
                "DIRECTORY_DATA_INVALID"
            );
            let mut sparse = record(&[115]);
            sparse[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, EndOfFile)..][..8]
                .copy_from_slice(&17i64.to_le_bytes());
            let (_, _, m) = decode_directory_record(&sparse, 0, 7).unwrap();
            assert_eq!(m.logical, 17);
            assert_eq!(m.allocated, Some(0));
            let mut cloud = sparse;
            cloud[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes)..][..4]
                .copy_from_slice(
                    &(FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_OFFLINE.0).to_le_bytes(),
                );
            cloud[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag)..][..4]
                .copy_from_slice(&0x9000001au32.to_le_bytes());
            let (_, _, m) = decode_directory_record(&cloud, 0, 7).unwrap();
            assert_eq!(m.allocated, None);
            assert!(!m.recurse);
            assert_eq!(m.cloud_state.as_deref(), Some("placeholder_not_recalled"));
        }
        #[test]
        fn special_file_and_missing_identity_require_full_metadata_completion() {
            let check = |bytes: &[u8]| {
                let (_, _, hint) = decode_directory_record(bytes, 0, 7).unwrap();
                DirectoryEntry {
                    path: PathBuf::from(r"C:\fixture\file"),
                    hint: Some(hint),
                }
                .scan_file_metadata()
                .is_some()
            };
            let bytes = record(&[102]);
            assert!(check(&bytes));
            let mut zero_id = bytes.clone();
            let id_at = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileId);
            zero_id[id_at..id_at + 16].fill(0);
            assert!(!check(&zero_id));
            for flag in [
                FILE_ATTRIBUTE_REPARSE_POINT.0,
                FILE_ATTRIBUTE_OFFLINE.0,
                FILE_ATTRIBUTE_RECALL_ON_OPEN.0,
                FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0,
            ] {
                let mut special = bytes.clone();
                let at = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes);
                special[at..at + 4].copy_from_slice(&flag.to_le_bytes());
                // Even an absent/undefined tag must not make a special file ordinary.
                assert!(!check(&special));
            }
            let mut wof = bytes;
            wof[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes)..][..4]
                .copy_from_slice(&FILE_ATTRIBUTE_REPARSE_POINT.0.to_le_bytes());
            wof[std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag)..][..4]
                .copy_from_slice(&0x80000017u32.to_le_bytes());
            assert!(!check(&wof));
        }
        #[test]
        fn bulk_matches_legacy_raw_paths_and_direct_handle_metadata_across_buffers() {
            let temp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(temp.path()).unwrap();
            let fast = ScanDirectoryReader::new(&root, true);
            assert!(fast.ntfs_volume.is_some(), "fixture requires local NTFS");
            fs::write(root.join("中文🙂.txt"), b"payload").unwrap();
            fs::hard_link(root.join("中文🙂.txt"), root.join("hardlink.bin")).unwrap();
            fs::write(
                root.join(OsString::from_wide(&[117, 0xd800, 46, 116, 120, 116])),
                b"raw",
            )
            .unwrap();
            fs::write(root.join("trailing."), b"verbatim").unwrap();
            fs::create_dir(root.join("child")).unwrap();
            let sparse = root.join("sparse.bin");
            let sparse_file = fs::File::create(&sparse).unwrap();
            assert!(std::process::Command::new("fsutil")
                .args(["sparse", "setflag"])
                .arg(&sparse)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success());
            sparse_file.set_len(64 * 1024 * 1024).unwrap();
            drop(sparse_file);
            let compressed = root.join("compressed.bin");
            fs::write(&compressed, vec![0u8; 128 * 1024]).unwrap();
            assert!(std::process::Command::new("compact")
                .args(["/c", "/i", "/q"])
                .arg(temp.path().join("compressed.bin"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success());
            for i in 0..1200 {
                fs::write(
                    root.join(format!("entry-{i:04}-{}.bin", "x".repeat(70))),
                    b"x",
                )
                .unwrap();
            }
            let old = entries(read_dir(&root).unwrap());
            let new = entries(fast.prepare(&root).unwrap().entries().unwrap());
            assert_eq!(new.len(), old.len());
            assert!(old.keys().eq(new.keys()));
            for (path, m) in &new {
                let path = PathBuf::from(OsString::from_wide(path));
                let measured = metadata(&path).unwrap();
                assert_eq!(m.kind, measured.kind);
                assert_eq!(m.logical, measured.logical);
                assert_eq!(m.modified, measured.modified);
                assert_eq!(m.attributes, measured.attributes);
                assert_eq!(m.reparse_tag, measured.reparse_tag);
                assert_eq!(m.identity, measured.identity);
                assert_eq!(m.allocated, measured.allocated);
                assert_eq!(m.logical, old[&raw(&path)].logical);
            }
            let mut long = root.join("long");
            fs::create_dir(&long).unwrap();
            while raw(&long).len() < 400 {
                long = long.join("abcdefghijklmno");
                fs::create_dir(&long).unwrap();
            }
            fs::write(long.join("尾部🌌.bin"), b"long").unwrap();
            let new = entries(fast.prepare(&long).unwrap().entries().unwrap());
            assert_eq!(new.len(), 1);
            assert_eq!(new.values().next().unwrap().logical, 4);
        }
        #[test]
        fn first_unsupported_falls_back_but_middle_error_does_not_restart() {
            let temp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(temp.path()).unwrap();
            fs::write(root.join("one.bin"), b"one").unwrap();
            let reader = ScanDirectoryReader::new(&root, true);
            let mut prepared = reader.prepare(&root).unwrap();
            prepared.fast.as_mut().unwrap().query_fault = Some(87);
            assert_eq!(entries(prepared.entries().unwrap()).len(), 1);
            let mut iterator = reader.prepare(&root).unwrap().fast.unwrap();
            iterator.refill().unwrap();
            iterator.query_fault = Some(87);
            let mut results = Vec::new();
            for item in iterator.by_ref() {
                results.push(item);
            }
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
            assert_eq!(
                results.last().unwrap().as_ref().err().unwrap().os_code,
                Some(87)
            );
            assert!(iterator.next().is_none());
        }
        #[test]
        fn stream_closes_on_drop_and_reparse_directory_is_never_followed() {
            let temp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(temp.path()).unwrap();
            let source = root.join("source");
            fs::create_dir(&source).unwrap();
            fs::write(source.join("own.bin"), b"own").unwrap();
            let reader = ScanDirectoryReader::new(&root, true);
            let iterator = reader.prepare(&source).unwrap().entries().unwrap();
            drop(iterator); // Same path can then be removed, with no leaked handle.
            fs::remove_file(source.join("own.bin")).unwrap();
            fs::remove_dir(&source).unwrap();
            let junction = root.join("loop");
            let status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&junction)
                .arg(&root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
            assert_eq!(
                reader.prepare(&junction).err().unwrap().code,
                "DIRECTORY_CHANGED"
            );
            fs::remove_dir(junction).unwrap();
        }
    }
}
#[cfg(windows)]
pub use win::{
    disk_space, metadata, read_dir, wide, win_error, PreparedScanDirectory, ScanDirectoryReader,
};

#[cfg(windows)]
pub fn opened_file_identity(file: &std::fs::File) -> Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::*};
    let handle = HANDLE(file.as_raw_handle());
    let mut id = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            &mut id as *mut _ as *mut _,
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .is_ok()
    {
        return Ok(format!(
            "{:016x}:{}",
            id.VolumeSerialNumber,
            id.FileId
                .Identifier
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
    }
    let mut legacy = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut legacy) }.map_err(win_error)?;
    Ok(format!(
        "{:08x}:{:08x}{:08x}",
        legacy.dwVolumeSerialNumber, legacy.nFileIndexHigh, legacy.nFileIndexLow
    ))
}

/// Fail closed when a parent has become a redirection since planning.
pub fn parent_chain(path: &Path) -> Result<Vec<(PathBuf, Fingerprint)>> {
    let mut out = vec![];
    let mut budget = 0usize;
    let mut p = path.parent();
    while let Some(parent) = p {
        budget += encode_path(parent).len() + 256;
        if budget > 2 * 1024 * 1024 {
            return Err(AppError::new(
                "PATH_PARENT_BUDGET",
                "父目录身份复核超过 2 MiB 预算，请选用更浅的路径",
            ));
        }
        let meta = metadata(parent)?;
        if meta.kind != "directory" || meta.link_type.is_some() {
            return Err(AppError::new(
                "UNSAFE_PARENT",
                "父目录是链接或已改变，请重新扫描",
            ));
        }
        if meta.identity.is_none() {
            return Err(AppError::new(
                "IDENTITY_UNAVAILABLE",
                "无法可靠核验父目录身份",
            ));
        }
        out.push((parent.to_owned(), meta.fingerprint()));
        p = parent.parent();
    }
    Ok(out)
}
pub fn verify_parents(parents: &[(PathBuf, Fingerprint)]) -> Result<()> {
    for (p, fp) in parents {
        let m = metadata(p)?;
        if m.identity != fp.identity || m.kind != fp.kind || m.reparse_tag != fp.reparse_tag {
            return Err(AppError::new("SOURCE_CHANGED", "操作路径的父目录已变化"));
        }
    }
    Ok(())
}
