use super::*;
use std::sync::{atomic::AtomicBool, Mutex};
use windows::{
    core::{implement, Error as WinError, Ref, HRESULT, PCWSTR},
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::SW_SHOWNORMAL},
    },
};

struct ComApartment;
impl ComApartment {
    fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(platform::win_error)?;
        Ok(Self)
    }
}
fn verify_item(item: &IShellItem, expected: &Fingerprint) -> Result<()> {
    let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(platform::win_error)?;
    use std::os::windows::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe { raw.as_wide() }));
    unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
    let bound = platform::absolute(&path)
        .and_then(|p| platform::metadata(&p))
        .map_err(|_| {
            AppError::new(
                "UNSUPPORTED_SHELL_PATH",
                "Shell 绑定路径无法可靠复核，已拒绝操作",
            )
        })?;
    if bound.identity.is_none()
        || bound.identity != expected.identity
        || bound.kind != expected.kind
    {
        return Err(AppError::new(
            "UNSUPPORTED_SHELL_PATH",
            "Shell 绑定了不同文件系统对象，已拒绝操作",
        ));
    }
    Ok(())
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
fn shell_wide(path: &Path) -> Vec<u16> {
    let mut w = platform::wide(path);
    let prefix = r"\\?\".encode_utf16().collect::<Vec<_>>();
    if w.starts_with(&prefix) {
        if w.get(4..8) == Some(&[85, 78, 67, 92]) {
            let mut out = vec![92, 92];
            out.extend_from_slice(&w[8..]);
            w = out;
        } else {
            w = w[4..].to_vec();
        }
    }
    w
}
pub fn recycle_preflight(path: &Path) -> Result<()> {
    let shown = platform::display(path);
    if shown.starts_with(r"\\") {
        return Err(AppError::new(
            "RECYCLE_UNAVAILABLE",
            "网络位置不能保证回收站，已拒绝；不会永久删除",
        ));
    }
    let w = platform::wide(path);
    let mut volume = vec![0u16; 32768];
    unsafe { GetVolumePathNameW(PCWSTR(w.as_ptr()), &mut volume) }.map_err(platform::win_error)?;
    if unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) } != 3 {
        return Err(AppError::new(
            "RECYCLE_UNAVAILABLE",
            "此卷未确认支持本地回收站，已拒绝；不会永久删除",
        ));
    }
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    unsafe { SHQueryRecycleBinW(PCWSTR(volume.as_ptr()), &mut info) }.map_err(|e| {
        AppError::new(
            "RECYCLE_UNAVAILABLE",
            format!("回收站不可用：{e}；不会永久删除"),
        )
    })?;
    Ok(())
}
#[derive(Default)]
struct SinkState {
    callbacks: u32,
    error: Option<AppError>,
    actual: Option<PathBuf>,
    recycled: bool,
}
#[implement(IFileOperationProgressSink)]
struct Sink {
    state: Arc<Mutex<SinkState>>,
    cancel: *const AtomicBool,
    recycling: bool,
    root: IShellItem,
}
// COM invokes the sink synchronously on the owning STA while the JobRuntime outlives PerformOperations.
unsafe impl Send for Sink {}
unsafe impl Sync for Sink {}
impl Sink {
    fn check(&self) -> windows::core::Result<()> {
        if unsafe { (*self.cancel).load(Ordering::Acquire) } {
            return Err(WinError::from_hresult(HRESULT(0x800704c7u32 as i32)));
        }
        Ok(())
    }
    fn post(
        &self,
        source: Ref<IShellItem>,
        hr: HRESULT,
        new: Ref<IShellItem>,
        delete: bool,
    ) -> windows::core::Result<()> {
        let top = source
            .as_ref()
            .and_then(|item| unsafe { item.Compare(&self.root, SICHINT_CANONICAL.0 as u32) }.ok())
            .is_some_and(|order| order == 0);
        let mut state = self.state.lock().unwrap();
        if top {
            state.callbacks += 1;
        }
        if hr.is_err() {
            state.error = Some(platform::win_error(WinError::from_hresult(hr)));
        } else if top {
            if let Some(item) = new.as_ref() {
                let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) };
                if let Ok(raw) = raw {
                    let units = unsafe { raw.as_wide() }.to_vec();
                    use std::os::windows::ffi::OsStringExt;
                    state.actual = Some(PathBuf::from(std::ffi::OsString::from_wide(&units)));
                    unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
                }
                if delete {
                    state.recycled = true;
                }
            } else if delete {
                state.error = Some(AppError::new(
                    "RECYCLE_NOT_CONFIRMED",
                    "Shell 未返回回收站中的项目，无法确认回收成功",
                ));
            }
        }
        Ok(())
    }
}
#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        self.check()
    }
    fn FinishOperations(&self, hr: HRESULT) -> windows::core::Result<()> {
        if hr.is_err() {
            self.state.lock().unwrap().error =
                Some(platform::win_error(WinError::from_hresult(hr)));
        }
        Ok(())
    }
    fn PreRenameItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.check()
    }
    fn PostRenameItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _name: &PCWSTR,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.post(_item, hr, new, false)
    }
    fn PreMoveItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.check()
    }
    fn PostMoveItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.post(_item, hr, new, false)
    }
    fn PreCopyItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.check()
    }
    fn PostCopyItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.post(_item, hr, new, false)
    }
    fn PreDeleteItem(&self, flags: u32, _item: Ref<IShellItem>) -> windows::core::Result<()> {
        self.check()?;
        if self.recycling && flags & (TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32) == 0 {
            self.state.lock().unwrap().error = Some(AppError::new(
                "RECYCLE_UNAVAILABLE",
                "系统未确认回收语义，已拒绝永久删除",
            ));
            return Err(WinError::from_hresult(E_ABORT));
        }
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        hr: HRESULT,
        new: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        self.post(_item, hr, new, true)
    }
    fn PreNewItem(
        &self,
        _flags: u32,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.check()
    }
    fn PostNewItem(
        &self,
        _flags: u32,
        _folder: Ref<IShellItem>,
        _name: &PCWSTR,
        _template: &PCWSTR,
        _attributes: u32,
        _hr: HRESULT,
        _new: Ref<IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _total: u32, _done: u32) -> windows::core::Result<()> {
        self.check()
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}
pub fn system_operation(
    source: &PlannedSource,
    kind: &str,
    policy: &str,
    job: &JobRuntime,
) -> Result<Option<PathBuf>> {
    platform::validate_shell_path(&source.stored.path)?;
    if let Some(dest) = &source.destination {
        platform::validate_shell_path(dest)?;
    }
    let _apartment = ComApartment::new()?;
    let state = Arc::new(Mutex::new(SinkState::default()));
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }
            .map_err(platform::win_error)?;
    let flags = if kind == "trash" {
        FOFX_RECYCLEONDELETE
            | FOFX_ADDUNDORECORD
            | FOF_ALLOWUNDO
            | FOF_WANTNUKEWARNING
            | FOFX_EARLYFAILURE
            | FOF_NOERRORUI
    } else {
        let mut flags = FOFX_ADDUNDORECORD | FOF_ALLOWUNDO | FOF_NOERRORUI | FOFX_EARLYFAILURE;
        if policy == "replace" {
            flags |= FOF_NOCONFIRMATION;
        }
        flags
    };
    // SetOperationFlags HRESULT is checked, and PreDeleteItem refuses a non-recycling transfer.
    unsafe { operation.SetOperationFlags(flags) }.map_err(platform::win_error)?;
    let sourcew = shell_wide(&source.stored.path);
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(sourcew.as_ptr()), None) }
        .map_err(platform::win_error)?;
    verify_item(&item, &source.fingerprint)?;
    let sink: IFileOperationProgressSink = Sink {
        state: state.clone(),
        cancel: Arc::as_ptr(&job.cancel),
        recycling: kind == "trash",
        root: item.clone(),
    }
    .into();
    let queued = match kind {
        "trash" => unsafe { operation.DeleteItem(&item, &sink) },
        "rename" => {
            let dest = source.destination.as_ref().unwrap();
            let name = platform::wide(Path::new(dest.file_name().unwrap()));
            unsafe { operation.RenameItem(&item, PCWSTR(name.as_ptr()), &sink) }
        }
        "copy" | "move" => {
            let dest = source.destination.as_ref().unwrap();
            let folderw = shell_wide(dest.parent().unwrap());
            let folder: IShellItem =
                unsafe { SHCreateItemFromParsingName(PCWSTR(folderw.as_ptr()), None) }
                    .map_err(platform::win_error)?;
            verify_item(
                &folder,
                &source
                    .destination_parent
                    .as_ref()
                    .ok_or_else(|| AppError::new("INVALID_DESTINATION", "目标目录身份缺失"))?
                    .1,
            )?;
            let name = platform::wide(Path::new(dest.file_name().unwrap()));
            if kind == "copy" {
                unsafe { operation.CopyItem(&item, &folder, PCWSTR(name.as_ptr()), &sink) }
            } else {
                unsafe { operation.MoveItem(&item, &folder, PCWSTR(name.as_ptr()), &sink) }
            }
        }
        _ => return Err(AppError::new("INVALID_OPERATION", "操作无效")),
    };
    queued.map_err(platform::win_error)?;
    let performed = unsafe { operation.PerformOperations() };
    let aborted = unsafe { operation.GetAnyOperationsAborted() }
        .map_err(platform::win_error)?
        .as_bool();
    let state = state.lock().unwrap();
    if let Some(error) = &state.error {
        return Err(error.clone());
    }
    if aborted {
        return Err(AppError::new(
            "CANCELLED",
            "系统操作已取消，部分内容可能已完成",
        ));
    }
    performed.map_err(platform::win_error)?;
    if state.callbacks == 0 {
        return Err(AppError::new(
            "OPERATION_NOT_CONFIRMED",
            "系统未返回逐项结果，无法确认完成",
        ));
    }
    if kind == "trash" && !state.recycled {
        return Err(AppError::new(
            "RECYCLE_NOT_CONFIRMED",
            "未取得回收站逐项成功结果",
        ));
    }
    Ok(if kind == "trash" {
        None
    } else {
        Some(state.actual.clone().ok_or_else(|| {
            AppError::new(
                "OPERATION_NOT_CONFIRMED",
                "系统未返回顶层目标项目，无法确认整个项目完成",
            )
        })?)
    })
}
pub fn open_node(path: &Path, action: &str) -> Result<()> {
    platform::validate_shell_path(path)?;
    let _apartment = ComApartment::new()?;
    let w = shell_wide(path);
    let metadata = platform::metadata(path)?;
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None) }
        .map_err(platform::win_error)?;
    verify_item(&item, &metadata.fingerprint())?;
    if action == "reveal" {
        let mut pidl = std::ptr::null_mut();
        unsafe { SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None) }
            .map_err(platform::win_error)?;
        let result = unsafe { SHOpenFolderAndSelectItems(pidl, None, 0) };
        unsafe { CoTaskMemFree(Some(pidl as *const _)) };
        result.map_err(platform::win_error)
    } else if matches!(action, "open" | "properties") {
        let verb = platform::wide(Path::new(if action == "properties" {
            "properties"
        } else {
            "open"
        }));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(w.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(platform::win_error)
    } else {
        Err(AppError::new("INVALID_ACTION", "系统动作无效"))
    }
}
