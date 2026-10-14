//! Lossless, versioned storage for the existing metadata fields.
use crate::{model::AppError, platform};
use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};

const HEADER: &[u8; 3] = b"SM\x01";
const RECORD: u8 = 0;
const LEGACY_JSON: u8 = 1;
const KNOWN_FLAGS: u16 = 0x01ff;
pub(crate) const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const METHODS: &[&str] = &[
    "FILE_ID_EXTD_DIR_INFO.AllocationSize",
    "FILE_STANDARD_INFO.AllocationSize",
    "directory_entry_not_counted",
    "stat.st_blocks",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    Json,
    Binary,
}
impl Mode {
    pub(crate) fn from_override(value: Option<&str>) -> Self {
        if value == Some("binary") {
            Self::Binary
        } else {
            Self::Json
        }
    }
    pub(crate) fn from_environment() -> Self {
        Self::from_override(std::env::var("SPACEFOLD_METADATA_CODEC").ok().as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Record {
    pub v: u8,
    pub b: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub f: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub t: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub i: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}
impl Record {
    pub(crate) fn from_meta(m: &platform::Metadata) -> Self {
        Self {
            v: 3,
            b: m.logical,
            f: m.attributes,
            t: m.reparse_tag,
            a: m.allocated,
            i: m.identity.clone(),
            n: m.links,
            l: m.link_type.clone(),
            g: m.link_target.clone(),
            c: m.cloud_state.clone(),
            m: m.allocation_method.clone(),
        }
    }
    pub(crate) fn metadata(self, kind: String, modified: Option<String>) -> platform::Metadata {
        platform::Metadata {
            kind,
            logical: self.b,
            allocated: self.a,
            modified,
            identity: self.i,
            links: self.n,
            attributes: self.f,
            reparse_tag: self.t,
            link_type: self.l,
            link_target: self.g,
            cloud_state: self.c,
            allocation_method: self.m,
            recurse: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Payload {
    Json(String),
    Binary(Vec<u8>),
}
impl Payload {
    pub(crate) fn capacity(&self) -> usize {
        match self {
            Self::Json(value) => value.capacity(),
            Self::Binary(value) => value.capacity(),
        }
    }
    pub(crate) fn from_ref(value: ValueRef<'_>) -> Result<Self, Error> {
        match value {
            ValueRef::Text(raw) => Ok(Self::Json(
                std::str::from_utf8(raw)
                    .map_err(|_| Error("invalid JSON UTF8"))?
                    .to_owned(),
            )),
            ValueRef::Blob(raw) => {
                decode(raw)?;
                Ok(Self::Binary(raw.to_owned()))
            }
            _ => Err(Error("metadata must be TEXT or BLOB")),
        }
    }
    #[cfg(test)]
    pub(crate) fn replacing_legacy_text(&self, text: String) -> Result<Self, Error> {
        match self {
            Self::Json(_) => Ok(Self::Json(text)),
            Self::Binary(_) => {
                let length = 4usize
                    .checked_add(uleb_len(text.len() as u64))
                    .and_then(|n| n.checked_add(text.len()))
                    .ok_or(Error("record too large"))?;
                if length > MAX_RECORD_BYTES {
                    return Err(Error("record exceeds storage limit"));
                }
                let mut raw = Vec::with_capacity(length);
                raw.extend_from_slice(HEADER);
                raw.push(LEGACY_JSON);
                put_string(&mut raw, &text);
                Ok(Self::Binary(raw))
            }
        }
    }
}
pub(crate) fn legacy_ref(value: ValueRef<'_>) -> Result<&str, Error> {
    match value {
        ValueRef::Text(raw) => std::str::from_utf8(raw).map_err(|_| Error("invalid JSON UTF8")),
        ValueRef::Blob(raw) => match decode(raw)? {
            Decoded::LegacyJson(text) => Ok(text),
            Decoded::Record(_) => Err(Error("binary record has no JSON text")),
        },
        _ => Err(Error("metadata must be TEXT or BLOB")),
    }
}
pub(crate) fn compact_ref(value: ValueRef<'_>) -> Result<Option<Record>, Error> {
    let text = match value {
        ValueRef::Blob(raw) => match decode(raw)? {
            Decoded::Record(record) => return Ok(Some(record)),
            Decoded::LegacyJson(text) => text,
        },
        _ => legacy_ref(value)?,
    };
    if text.starts_with("{\"v\":3") {
        let record: Record =
            serde_json::from_str(text).map_err(|_| Error("invalid v3 metadata JSON"))?;
        if record.v != 3 {
            return Err(Error("unknown JSON version"));
        }
        Ok(Some(record))
    } else {
        Ok(None)
    }
}
impl ToSql for Payload {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Json(text) => ValueRef::Text(text.as_bytes()),
            Self::Binary(raw) => ValueRef::Blob(raw),
        }))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Error(&'static str);
impl Error {
    pub(crate) fn unsupported_version() -> Self {
        Self("unknown JSON version")
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "保存的元数据编码无效：{}", self.0)
    }
}
impl std::error::Error for Error {}
pub(crate) fn app_error(error: Error) -> AppError {
    AppError::new("STORED_METADATA_ENCODING", error.to_string())
}
pub(crate) fn sql_error(
    index: usize,
    value_type: rusqlite::types::Type,
    error: Error,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, value_type, Box::new(error))
}

pub(crate) fn encode(metadata: &platform::Metadata, mode: Mode) -> crate::model::Result<Payload> {
    let record = Record::from_meta(metadata);
    match mode {
        Mode::Json => Ok(Payload::Json(serde_json::to_string(&record)?)),
        Mode::Binary => Ok(Payload::Binary(encode_record(&record).map_err(app_error)?)),
    }
}
fn uleb_len(mut value: u64) -> usize {
    let mut n = 1;
    while value >= 128 {
        value >>= 7;
        n += 1;
    }
    n
}
fn put_uleb(raw: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        raw.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    raw.push(value as u8);
}
fn put_string(raw: &mut Vec<u8>, value: &str) {
    put_uleb(raw, value.len() as u64);
    raw.extend_from_slice(value.as_bytes());
}
fn packed_identity(value: &str) -> Option<[u8; 24]> {
    let bytes = value.as_bytes();
    if bytes.len() != 49 || bytes[16] != b':' {
        return None;
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut raw = [0; 24];
    for (index, pair) in bytes[..16]
        .as_chunks::<2>()
        .0
        .iter()
        .chain(bytes[17..].as_chunks::<2>().0.iter())
        .enumerate()
    {
        raw[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(raw)
}
fn flags(record: &Record) -> u16 {
    let present = [
        record.f != 0,
        record.t != 0,
        record.a.is_some(),
        record.i.is_some(),
        record.n.is_some(),
        record.l.is_some(),
        record.g.is_some(),
        record.c.is_some(),
        record.m.is_some(),
    ];
    present.iter().enumerate().fold(0, |flags, (bit, present)| {
        flags | (u16::from(*present) << bit)
    })
}
fn encode_record(record: &Record) -> Result<Vec<u8>, Error> {
    if record.v != 3 {
        return Err(Error("unknown source version"));
    }
    let identity = record.i.as_deref().and_then(packed_identity);
    let method = record
        .m
        .as_deref()
        .and_then(|value| METHODS.iter().position(|item| *item == value));
    let mut length = 6 + uleb_len(record.b);
    for value in [
        (record.f != 0).then_some(record.f as u64),
        (record.t != 0).then_some(record.t as u64),
        record.a,
        record.n,
    ]
    .into_iter()
    .flatten()
    {
        length += uleb_len(value);
    }
    if let Some(value) = &record.i {
        length += 1 + if identity.is_some() {
            24
        } else {
            uleb_len(value.len() as u64) + value.len()
        };
    }
    for value in [&record.l, &record.g, &record.c].into_iter().flatten() {
        length = length
            .checked_add(uleb_len(value.len() as u64) + value.len())
            .ok_or(Error("record too large"))?;
    }
    if let Some(value) = &record.m {
        length = length
            .checked_add(
                1 + if method.is_some() {
                    0
                } else {
                    uleb_len(value.len() as u64) + value.len()
                },
            )
            .ok_or(Error("record too large"))?;
    }
    if length > MAX_RECORD_BYTES {
        return Err(Error("record exceeds storage limit"));
    }
    let mut raw = Vec::with_capacity(length);
    raw.extend_from_slice(HEADER);
    raw.push(RECORD);
    raw.extend_from_slice(&flags(record).to_le_bytes());
    put_uleb(&mut raw, record.b);
    if record.f != 0 {
        put_uleb(&mut raw, record.f as u64);
    }
    if record.t != 0 {
        put_uleb(&mut raw, record.t as u64);
    }
    if let Some(value) = record.a {
        put_uleb(&mut raw, value);
    }
    if let Some(value) = &record.i {
        if let Some(identity) = identity {
            raw.push(1);
            raw.extend_from_slice(&identity);
        } else {
            raw.push(0);
            put_string(&mut raw, value);
        }
    }
    if let Some(value) = record.n {
        put_uleb(&mut raw, value);
    }
    for value in [&record.l, &record.g, &record.c].into_iter().flatten() {
        put_string(&mut raw, value);
    }
    if let Some(value) = &record.m {
        if let Some(tag) = method {
            raw.push(tag as u8 + 1);
        } else {
            raw.push(0);
            put_string(&mut raw, value);
        }
    }
    debug_assert_eq!(raw.len(), length);
    Ok(raw)
}

struct Reader<'a> {
    remaining: &'a [u8],
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
        if length > self.remaining.len() {
            return Err(Error("truncated record"));
        }
        let (value, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn uleb(&mut self) -> Result<u64, Error> {
        let mut value = 0;
        for index in 0..10 {
            let byte = self.byte()?;
            if index == 9 && byte > 1 {
                return Err(Error("integer overflow"));
            }
            value |= ((byte & 127) as u64) << (index * 7);
            if byte & 128 == 0 {
                if index != 0 && byte == 0 {
                    return Err(Error("noncanonical integer"));
                }
                return Ok(value);
            }
        }
        Err(Error("integer overflow"))
    }
    fn string(&mut self) -> Result<&'a str, Error> {
        let length = usize::try_from(self.uleb()?).map_err(|_| Error("length overflow"))?;
        if length > MAX_RECORD_BYTES {
            return Err(Error("string exceeds storage limit"));
        }
        std::str::from_utf8(self.take(length)?).map_err(|_| Error("invalid UTF8"))
    }
}
pub(crate) enum Decoded<'a> {
    Record(Record),
    LegacyJson(&'a str),
}
pub(crate) fn decode(raw: &[u8]) -> Result<Decoded<'_>, Error> {
    if raw.len() > MAX_RECORD_BYTES {
        return Err(Error("record exceeds storage limit"));
    }
    let mut reader = Reader { remaining: raw };
    if reader.take(3)? != HEADER {
        return Err(Error("unknown magic/version"));
    }
    let variant = reader.byte()?;
    if variant == LEGACY_JSON {
        let text = reader.string()?;
        if !reader.remaining.is_empty() {
            return Err(Error("trailing bytes"));
        }
        return Ok(Decoded::LegacyJson(text));
    }
    if variant != RECORD {
        return Err(Error("unknown variant"));
    }
    let bytes = reader.take(2)?;
    let flags = u16::from_le_bytes([bytes[0], bytes[1]]);
    if flags & !KNOWN_FLAGS != 0 {
        return Err(Error("unknown presence flags"));
    }
    let mut record = Record {
        v: 3,
        b: reader.uleb()?,
        f: 0,
        t: 0,
        a: None,
        i: None,
        n: None,
        l: None,
        g: None,
        c: None,
        m: None,
    };
    for (bit, field) in [(0, &mut record.f), (1, &mut record.t)] {
        if flags & (1 << bit) != 0 {
            *field = u32::try_from(reader.uleb()?).map_err(|_| Error("u32 overflow"))?;
            if *field == 0 {
                return Err(Error("noncanonical zero field"));
            }
        }
    }
    if flags & 4 != 0 {
        record.a = Some(reader.uleb()?);
    }
    if flags & 8 != 0 {
        record.i = Some(match reader.byte()? {
            0 => {
                let value = reader.string()?;
                if packed_identity(value).is_some() {
                    return Err(Error("noncanonical identity"));
                }
                value.to_owned()
            }
            1 => {
                let bytes = reader.take(24)?;
                let mut value = String::with_capacity(49);
                const HEX: &[u8; 16] = b"0123456789abcdef";
                for (index, byte) in bytes.iter().enumerate() {
                    if index == 8 {
                        value.push(':');
                    }
                    value.push(HEX[(byte >> 4) as usize] as char);
                    value.push(HEX[(byte & 15) as usize] as char);
                }
                value
            }
            _ => return Err(Error("unknown identity tag")),
        });
    }
    if flags & 16 != 0 {
        record.n = Some(reader.uleb()?);
    }
    for (bit, field) in [(5, &mut record.l), (6, &mut record.g), (7, &mut record.c)] {
        if flags & (1 << bit) != 0 {
            *field = Some(reader.string()?.to_owned());
        }
    }
    if flags & 256 != 0 {
        let tag = reader.byte()?;
        record.m = Some(if tag == 0 {
            let value = reader.string()?;
            if METHODS.contains(&value) {
                return Err(Error("noncanonical method"));
            }
            value.to_owned()
        } else {
            METHODS
                .get(tag as usize - 1)
                .ok_or(Error("unknown method tag"))?
                .to_string()
        });
    }
    if !reader.remaining.is_empty() {
        return Err(Error("trailing bytes"));
    }
    Ok(Decoded::Record(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        Record {
            v: 3,
            b: 0,
            f: 0,
            t: 0,
            a: None,
            i: None,
            n: None,
            l: None,
            g: None,
            c: None,
            m: None,
        }
    }
    fn roundtrip(value: &Record) {
        let raw = encode_record(value).unwrap();
        match decode(&raw).unwrap() {
            Decoded::Record(decoded) => assert_eq!(&decoded, value),
            _ => panic!("record expected"),
        };
    }
    #[test]
    fn binary_metadata_preserves_integer_option_string_and_identity_boundaries() {
        for value in [
            0,
            127,
            128,
            (1 << 53) + 1,
            i64::MAX as u64,
            1 << 63,
            u64::MAX,
        ] {
            let mut r = record();
            r.b = value;
            r.a = Some(value);
            r.n = Some(value);
            r.f = u32::MAX;
            r.t = u32::MAX;
            roundtrip(&r);
        }
        for identity in [
            "",
            "0000000000000000:00000000000000000000000000000000",
            "abcdef0123456789:abcdef0123456789abcdef0123456789",
            "ABCDEF0123456789:ABCDEF0123456789ABCDEF0123456789",
            "中文\0😀:opaque",
        ] {
            for method in METHODS.iter().copied().chain(["", "unknown 中文\0😀"]) {
                let mut r = record();
                r.i = Some(identity.into());
                r.m = Some(method.into());
                r.l = Some("".into());
                r.g = Some("中文\0😀".into());
                r.c = Some("".into());
                roundtrip(&r);
            }
        }
        roundtrip(&record());
        assert_eq!(Mode::from_override(Some("binary")), Mode::Binary);
        for value in [None, Some("off"), Some("BINARY"), Some("binary ")] {
            assert_eq!(Mode::from_override(value), Mode::Json);
        }
    }
    #[test]
    fn binary_metadata_rejects_truncation_flags_lengths_tags_and_noncanonical_integers() {
        let mut r = record();
        r.b = u64::MAX;
        r.f = u32::MAX;
        r.t = u32::MAX;
        r.a = Some(u64::MAX);
        r.n = Some(u64::MAX);
        r.i = Some("opaque".into());
        r.l = Some("中文".into());
        r.g = Some("target".into());
        r.c = Some("cloud".into());
        r.m = Some("method".into());
        let raw = encode_record(&r).unwrap();
        for end in 0..raw.len() {
            assert!(decode(&raw[..end]).is_err(), "prefix {end}");
        }
        let mut trailing = raw.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        for raw in [
            b"SM\x02\0\0\0\0".to_vec(),
            b"SM\x01\x02\0\0\0".to_vec(),
            b"SM\x01\0\0\x80\0".to_vec(),
            b"SM\x01\0\0\0\x80\0".to_vec(),
            b"SM\x01\0\x08\0\0\x02".to_vec(),
            b"SM\x01\0\0\x01\0\xff".to_vec(),
            b"SM\x01\0\x20\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\x01".to_vec(),
            b"SM\x01\0\x20\0\0\x02\xff\xff".to_vec(),
            b"SM\x01\0\x01\0\0\0".to_vec(),
        ] {
            assert!(decode(&raw).is_err(), "{raw:?}");
        }
        let mut overflow = b"SM\x01\0\0\0".to_vec();
        overflow.extend_from_slice(&[255; 10]);
        assert!(decode(&overflow).is_err());
        let mut u32_overflow = b"SM\x01\0\x01\0\0".to_vec();
        put_uleb(&mut u32_overflow, 1 << 32);
        assert!(decode(&u32_overflow).is_err());
    }
    #[test]
    fn binary_metadata_legacy_envelope_and_actual_capacity_are_bounded() {
        let text = "{\"unknown\":\"中文\",\"node\":{}}";
        let mut raw = HEADER.to_vec();
        raw.push(LEGACY_JSON);
        put_string(&mut raw, text);
        assert!(matches!(decode(&raw).unwrap(),Decoded::LegacyJson(value) if value==text));
        let mut r = record();
        r.i = Some("0000000000000000:00000000000000000000000000000000".into());
        r.m = Some(METHODS[0].into());
        let raw = encode_record(&r).unwrap();
        assert_eq!(raw.len(), raw.capacity());
        assert!(raw.len() < serde_json::to_string(&r).unwrap().len());
        r.g = Some("x".repeat(MAX_RECORD_BYTES));
        assert!(encode_record(&r).is_err());
        assert!(decode(&vec![0; MAX_RECORD_BYTES + 1]).is_err());
        let mut padded = Payload::Binary(raw);
        if let Payload::Binary(raw) = &mut padded {
            raw.reserve(128);
            assert!(padded.capacity() > 0);
        }
    }
}
