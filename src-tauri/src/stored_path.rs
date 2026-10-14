use crate::{model::AppError, platform};
#[cfg(windows)]
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

const MAGIC: [u8; 2] = *b"SF";
const VERSION: u8 = 1;
const PAD: u8 = 0x80;
const COMPONENT_VERSION: u8 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FileStorage {
    Full,
    #[default]
    Basename,
}
impl FileStorage {
    pub(crate) fn from_override(value: Option<&str>) -> Self {
        if value == Some("full") {
            Self::Full
        } else {
            Self::Basename
        }
    }
    pub(crate) fn from_environment() -> Self {
        Self::from_override(std::env::var("SPACEFOLD_FILE_PATH_STORAGE").ok().as_deref())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Utf16,
    #[default]
    Wtf8,
}
impl Mode {
    pub(crate) fn from_override(value: Option<&str>) -> Self {
        if value == Some("off") {
            Self::Utf16
        } else {
            Self::Wtf8
        }
    }
    pub(crate) fn from_environment() -> Self {
        Self::from_override(std::env::var("SPACEFOLD_STORED_PATH_CODEC").ok().as_deref())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DecodeError(&'static str);
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "保存的路径编码无效：{}", self.0)
    }
}
impl std::error::Error for DecodeError {}
pub(crate) fn app_error(error: DecodeError) -> AppError {
    AppError::new("STORED_PATH_ENCODING", error.to_string())
}

pub(crate) fn encode(path: &Path, mode: Mode) -> Result<Vec<u8>, DecodeError> {
    #[cfg(windows)]
    if mode == Mode::Wtf8 {
        return encode_units(path.as_os_str().encode_wide(), path.as_os_str().len());
    }
    #[cfg(not(windows))]
    let _ = mode;
    Ok(platform::encode_path(path))
}

pub(crate) fn decode(raw: Vec<u8>) -> Result<PathBuf, DecodeError> {
    #[cfg(windows)]
    if raw.len() % 2 == 1 {
        return Ok(PathBuf::from(OsString::from_wide(&decode_units(&raw)?)));
    }
    Ok(platform::decode_path(raw))
}

pub(crate) fn is_component(raw: &[u8]) -> bool {
    raw.len() % 2 == 1
        && raw.len() >= 3
        && raw[..2] == MAGIC
        && matches!(raw[2], COMPONENT_VERSION | 0x82)
}

/// Called only by the native ordinary-file reader with its directory task path.
/// Validate the exact unit prefix and join before replacing a full path; generic
/// repository/metadata/root writers never select this representation.
pub(crate) fn encode_native_file(
    path: &Path,
    parent: &Path,
    mode: Mode,
    storage: FileStorage,
) -> Result<Vec<u8>, DecodeError> {
    #[cfg(windows)]
    if storage == FileStorage::Basename
        && mode == Mode::Wtf8
        && parent.is_absolute()
        && !parent.as_os_str().encode_wide().any(|unit| unit == 0)
    {
        let mut full = path.as_os_str().encode_wide();
        let mut last = None;
        let mut matched = true;
        for unit in parent.as_os_str().encode_wide() {
            last = Some(unit);
            if full.next() != Some(unit) {
                matched = false;
                break;
            }
        }
        if matched && last.is_some() {
            if !matches!(last, Some(47 | 92)) && full.next() != Some(92) {
                matched = false;
            }
            let length = full.clone().count();
            if matched
                && length != 0
                && !full.clone().eq([46])
                && !full.clone().eq([46, 46])
                && !full.clone().any(|unit| matches!(unit, 0 | 47 | 58 | 92))
            {
                return encode_units_version(full, length, COMPONENT_VERSION);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (parent, storage);
    encode(path, mode)
}

fn valid_component(units: &[u16]) -> bool {
    !units.is_empty()
        && units != [46]
        && units != [46, 46]
        && !units.iter().any(|unit| matches!(unit, 0 | 47 | 58 | 92))
}

pub(crate) fn decode_node(
    raw: Vec<u8>,
    kind: &str,
    parent_id: Option<i64>,
    parent_raw: Option<Vec<u8>>,
) -> Result<PathBuf, DecodeError> {
    if !is_component(&raw) {
        return decode(raw);
    }
    if kind != "file" || parent_id.is_none() {
        return Err(DecodeError("组件仅用于非根文件"));
    }
    #[cfg(windows)]
    {
        let component = decode_units_version(&raw, COMPONENT_VERSION)?;
        if !valid_component(&component) {
            return Err(DecodeError("文件组件含空值或路径跳转"));
        }
        // `decode` deliberately rejects another component: at most one lookup.
        let parent = decode(parent_raw.ok_or(DecodeError("组件父目录缺失或种类不符"))?)?;
        if !parent.is_absolute() {
            return Err(DecodeError("父目录不是完整绝对路径"));
        }
        let mut units = parent.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err(DecodeError("父路径含NUL"));
        }
        let separator = usize::from(!matches!(units.last(), Some(47 | 92)));
        units
            .len()
            .checked_add(separator)
            .and_then(|n| n.checked_add(component.len()))
            .ok_or(DecodeError("路径容量溢出"))?;
        units
            .try_reserve_exact(separator + component.len())
            .map_err(|_| DecodeError("路径容量不足"))?;
        if separator != 0 {
            units.push(92);
        }
        units.extend_from_slice(&component);
        Ok(PathBuf::from(OsString::from_wide(&units)))
    }
    #[cfg(not(windows))]
    {
        let _ = parent_raw;
        Err(DecodeError("文件组件只支持Windows"))
    }
}

// New envelopes are always odd. Every even blob remains arbitrary legacy
// UTF16, including all possible prefixes. A parity-changing corruption can
// therefore become valid legacy data; this format does not promise otherwise.
fn encode_units(
    units: impl IntoIterator<Item = u16>,
    capacity_hint: usize,
) -> Result<Vec<u8>, DecodeError> {
    encode_units_version(units, capacity_hint, VERSION)
}
fn encode_units_version(
    units: impl IntoIterator<Item = u16>,
    capacity_hint: usize,
    version: u8,
) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(
            capacity_hint
                .checked_add(4)
                .ok_or(DecodeError("容量溢出"))?,
        )
        .map_err(|_| DecodeError("路径容量不足"))?;
    output.extend_from_slice(&MAGIC);
    output.push(version);
    let mut units = units.into_iter().peekable();
    while let Some(unit) = units.next() {
        let scalar = if (0xd800..=0xdbff).contains(&unit)
            && units
                .peek()
                .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
        {
            let low = units.next().unwrap();
            0x10000 + ((u32::from(unit) - 0xd800) << 10) + (u32::from(low) - 0xdc00)
        } else {
            u32::from(unit)
        };
        if scalar <= 0x7f {
            output.push(scalar as u8);
        } else if scalar <= 0x7ff {
            output.extend_from_slice(&[(0xc0 | (scalar >> 6)) as u8, (0x80 | (scalar & 63)) as u8]);
        } else if scalar <= 0xffff {
            output.extend_from_slice(&[
                (0xe0 | (scalar >> 12)) as u8,
                (0x80 | ((scalar >> 6) & 63)) as u8,
                (0x80 | (scalar & 63)) as u8,
            ]);
        } else {
            output.extend_from_slice(&[
                (0xf0 | (scalar >> 18)) as u8,
                (0x80 | ((scalar >> 12) & 63)) as u8,
                (0x80 | ((scalar >> 6) & 63)) as u8,
                (0x80 | (scalar & 63)) as u8,
            ]);
        }
    }
    if (output.len() - 3) % 2 == 1 {
        output[2] |= PAD;
        output.push(0);
    }
    debug_assert_eq!(output.len() % 2, 1);
    Ok(output)
}

fn decode_units(raw: &[u8]) -> Result<Vec<u16>, DecodeError> {
    decode_units_version(raw, VERSION)
}
fn decode_units_version(raw: &[u8], version: u8) -> Result<Vec<u16>, DecodeError> {
    if raw.len().is_multiple_of(2) {
        return Ok(raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect());
    }
    if raw.len() < 3 || raw[..2] != MAGIC || (raw[2] != version && raw[2] != (version | PAD)) {
        return Err(DecodeError("未知奇数格式或版本"));
    }
    let end = if raw[2] & PAD != 0 {
        if raw.len() < 5 || raw.last() != Some(&0) {
            return Err(DecodeError("填充无效"));
        }
        raw.len() - 1
    } else {
        raw.len()
    };
    let payload = &raw[3..end];
    let mut output = Vec::new();
    output
        .try_reserve_exact(payload.len())
        .map_err(|_| DecodeError("路径容量不足"))?;
    let mut index = 0;
    let mut high_surrogate = false;
    while index < payload.len() {
        let first = payload[index];
        let (length, mut scalar) = match first {
            0..=0x7f => (1, u32::from(first)),
            0xc2..=0xdf => (2, u32::from(first & 31)),
            0xe0..=0xef => (3, u32::from(first & 15)),
            0xf0..=0xf4 => (4, u32::from(first & 7)),
            _ => return Err(DecodeError("WTF8 起始字节无效")),
        };
        if index + length > payload.len() {
            return Err(DecodeError("WTF8 截断"));
        }
        for &next in &payload[index + 1..index + length] {
            if next & 0xc0 != 0x80 {
                return Err(DecodeError("WTF8 continuation 无效"));
            }
            scalar = (scalar << 6) | u32::from(next & 63);
        }
        if (length == 2 && scalar < 0x80)
            || (length == 3 && scalar < 0x800)
            || (length == 4 && !(0x10000..=0x10ffff).contains(&scalar))
        {
            return Err(DecodeError("WTF8 码点范围或最短编码无效"));
        }
        if (0xdc00..=0xdfff).contains(&scalar) && high_surrogate {
            return Err(DecodeError("surrogate pair 必须采用四字节编码"));
        }
        high_surrogate = (0xd800..=0xdbff).contains(&scalar);
        if scalar <= 0xffff {
            output.push(scalar as u16);
        } else {
            let scalar = scalar - 0x10000;
            output.push((0xd800 + (scalar >> 10)) as u16);
            output.push((0xdc00 + (scalar & 1023)) as u16);
        }
        index += length;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut blob = vec![MAGIC[0], MAGIC[1], VERSION];
        blob.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            blob[2] |= PAD;
            blob.push(0);
        }
        blob
    }
    #[cfg(windows)]
    #[test]
    fn trusted_components_preserve_exact_units_prefix_case_and_fallback() {
        assert_eq!(
            FileStorage::from_override(Some("basename")),
            FileStorage::Basename
        );
        assert_eq!(FileStorage::from_override(Some("full")), FileStorage::Full);
        for value in [None, Some("FULL"), Some("full "), Some("basename ")] {
            assert_eq!(FileStorage::from_override(value), FileStorage::Basename);
        }
        for parent in [
            r"C:\MiXeD",
            r"\\?\C:\MiXeD",
            r"\\?\UNC\Server\Share\Case",
            r"C:\",
            r"C:/mixed/",
        ] {
            let parent = PathBuf::from(parent);
            let mut name: Vec<_> = "MiXeD-😀-".encode_utf16().collect();
            name.extend_from_slice(&[0xd800, 0x4e2d, 0xdc00]);
            let mut units: Vec<_> = parent.as_os_str().encode_wide().collect();
            if !matches!(units.last(), Some(47 | 92)) {
                units.push(92);
            }
            units.extend_from_slice(&name);
            let path = PathBuf::from(OsString::from_wide(&units));
            let raw =
                encode_native_file(&path, &parent, Mode::Wtf8, FileStorage::Basename).unwrap();
            assert!(is_component(&raw));
            for mode in [Mode::Utf16, Mode::Wtf8] {
                let restored = decode_node(
                    raw.clone(),
                    "file",
                    Some(1),
                    Some(encode(&parent, mode).unwrap()),
                )
                .unwrap();
                assert_eq!(
                    restored.as_os_str().encode_wide().collect::<Vec<_>>(),
                    units
                );
            }
            assert!(decode(raw).is_err());
            let other = PathBuf::from(r"C:\other");
            let full =
                encode_native_file(&path, &other, Mode::Wtf8, FileStorage::Basename).unwrap();
            assert!(!is_component(&full));
            assert_eq!(
                decode(full)
                    .unwrap()
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>(),
                units
            );
            assert!(!is_component(
                &encode_native_file(&path, &parent, Mode::Wtf8, FileStorage::Full).unwrap()
            ));
            assert!(!is_component(
                &encode_native_file(&path, &parent, Mode::Utf16, FileStorage::Basename).unwrap()
            ));
        }
    }
    #[cfg(windows)]
    #[test]
    fn components_reject_traversal_missing_wrong_and_recursive_parents() {
        let parent = encode(Path::new(r"C:\parent"), Mode::Wtf8).unwrap();
        for value in ["", ".", "..", "/x", "\\x", "C:foo", "a/b", "a\\b", "a\0b"] {
            let raw =
                encode_units_version(value.encode_utf16(), value.len(), COMPONENT_VERSION).unwrap();
            assert!(decode_node(raw, "file", Some(1), Some(parent.clone())).is_err());
        }
        let valid = encode_units_version("file".encode_utf16(), 4, COMPONENT_VERSION).unwrap();
        assert!(decode_node(valid.clone(), "file", None, Some(parent.clone())).is_err());
        assert!(decode_node(valid.clone(), "directory", Some(1), Some(parent.clone())).is_err());
        assert!(decode_node(valid.clone(), "file", Some(1), None).is_err());
        assert!(decode_node(valid.clone(), "file", Some(1), Some(valid.clone())).is_err());
        assert!(decode_node(
            valid.clone(),
            "file",
            Some(1),
            Some(encode(Path::new("relative"), Mode::Wtf8).unwrap())
        )
        .is_err());
        for raw in [
            vec![83, 70, 2, 0xc0, 0x80],
            vec![83, 70, 0x82, 65, 1],
            vec![83, 70, 0x42],
        ] {
            assert!(decode_node(raw, "file", Some(1), Some(parent.clone())).is_err());
        }
        let even = vec![83, 70, 2, 0];
        assert!(!is_component(&even));
        assert_eq!(
            decode_node(even.clone(), "file", None, None)
                .unwrap()
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>(),
            vec![0x4653, 2]
        );
    }
    #[test]
    fn mode_defaults_to_wtf8_with_exact_off_baseline() {
        assert_eq!(Mode::from_override(Some("off")), Mode::Utf16);
        for value in [
            None,
            Some("wtf8"),
            Some("utf16"),
            Some("on"),
            Some("WTF8"),
            Some("wtf8 "),
            Some("1"),
        ] {
            assert_eq!(Mode::from_override(value), Mode::Wtf8);
        }
    }
    #[test]
    fn every_single_utf16_unit_roundtrips_legacy_and_new_without_prefix_collisions() {
        for unit in 0..=u16::MAX {
            assert_eq!(decode_units(&unit.to_le_bytes()).unwrap(), [unit]);
            let new = encode_units([unit], 3).unwrap();
            assert_eq!(new.len() % 2, 1);
            assert_eq!(decode_units(&new).unwrap(), [unit]);
        }
        let legacy = [0x53, 0x46, 0x81, 0, 0, 0, 0xff, 0xff];
        assert_eq!(decode_units(&legacy).unwrap(), [0x4653, 0x0081, 0, 0xffff]);
    }
    #[test]
    fn canonical_pairs_isolated_surrogates_nul_and_long_paths_preserve_every_unit() {
        let mut fixtures = vec![
            vec![],
            vec![0],
            "C:\\a\\file.txt".encode_utf16().collect(),
            "C:\\中文\\😀.txt".encode_utf16().collect(),
            vec![0xd800, 0xdc00],
            vec![0xd800, 0xd800, 0xdc00, 0xdc00, 0, 0xffff],
        ];
        fixtures.push(vec![0x4e2d; 32767]);
        for units in fixtures {
            let new = encode_units(units.iter().copied(), units.len() * 3).unwrap();
            assert_eq!(decode_units(&new).unwrap(), units);
        }
        assert_eq!(
            encode_units([0xd800, 0xdc00], 4).unwrap(),
            framed(&[0xf0, 0x90, 0x80, 0x80])
        );
    }
    #[test]
    fn odd_malformed_envelopes_and_noncanonical_wtf8_are_rejected() {
        for blob in [
            vec![0],
            vec![0, 0, 1],
            vec![0x53, 0x46, 2],
            vec![0x53, 0x46, 0x41],
            vec![0x53, 0x46, 0x81],
            vec![0x53, 0x46, 0x81, 65, 1],
        ] {
            assert!(decode_units(&blob).is_err(), "{blob:?}");
        }
        for bytes in [
            &[0xc0, 0x80][..],
            &[0x80],
            &[0xe0, 0x80, 0x80],
            &[0xf0, 0x80, 0x80, 0x80],
            &[0xf4, 0x90, 0x80, 0x80],
            &[0xf5, 0x80, 0x80, 0x80],
            &[0xe1, 65, 0x80],
            &[0xc2],
            &[0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80],
        ] {
            assert!(decode_units(&framed(bytes)).is_err(), "{bytes:?}");
        }
        let new = framed(b"ab");
        // Losing one byte changes parity; accepting all arbitrary even legacy
        // blobs makes this corruption indistinguishable from legacy UTF16.
        assert!(decode_units(&new[..new.len() - 1]).is_ok());
    }
}
