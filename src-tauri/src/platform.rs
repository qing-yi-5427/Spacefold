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
}
#[cfg(windows)]
pub use win::{disk_space, metadata, read_dir, wide, win_error};

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
