//! Lyrics on a connected USB speaker with a screen over Windows HID.
//!
//! Enji speaks the speaker's USB HID protocol (`VID 0x3837`, `PID 0x60C6`,
//! report `0xBC`) itself, reading the style set on the speaker so font,
//! alignment and animation stay as they are.

pub const VID: u16 = 0x3837;
pub const PID: u16 = 0x60C6;
pub const REPORT_ID: u8 = 0xBC;
pub const REPORT_LEN: usize = 64;

const START: u8 = 0xAA;
const VERSION: u8 = 0x01;
const TYPE_REQUEST: [u8; 2] = [0x00, 0x01];
const TYPE_SET: [u8; 2] = [0x00, 0x02];
const TYPE_NOTIFY: [u8; 2] = [0x00, 0x04];

const SUB_READ_SCREEN_STATE: u8 = 0x01;
const SUB_READ_LEGACY_LYRICS: u8 = 0x02;
const SUB_NOTIFY_SCREEN_STATE: u8 = 0x09;
const SUB_SET_LEGACY_LYRICS: u8 = 0x0B;
const SUB_READ_LYRIC_SETTINGS: u8 = 0x11;
const SUB_NOTIFY_EXIT_LYRICS: u8 = 0x20;
const SUB_SET_EXTENDED_LYRICS: u8 = 0x27;

/// Firmware `0034` (`REV_0034`) introduced multi-packet extended lyrics.
const EXTENDED_MIN_BCD: u16 = 0x0034;
const LEGACY_MAX_BYTES: usize = 52;
const EXTENDED_CHUNK_BYTES: usize = 45;
const EXTENDED_MAX_BYTES: usize = 255;

/// How the speaker draws lyrics, read from the device so Enji keeps the
/// user's own font, alignment and animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Extended {
        font: u8,
        font_size: u8,
        align: u8,
        direction: u8,
        animation: u8,
    },
    Legacy {
        scroll: u8,
    },
}

impl Style {
    pub fn default_extended() -> Self {
        Self::Extended {
            font: 0,
            font_size: 0,
            align: 0,
            direction: 0,
            animation: 5,
        }
    }

    pub fn default_legacy() -> Self {
        Self::Legacy { scroll: 5 }
    }
}

/// Events the speaker reports on its input endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    ScreenEnabled(bool),
    ExitLyrics { screen_enabled: bool },
    StyleChanged(Style),
}

/// Whether `bcd_version` (`HIDD_ATTRIBUTES.VersionNumber`, e.g. `0x0034` for
/// `REV_0034`) supports the extended multi-packet lyric command (`0x27`).
pub fn supports_extended(bcd_version: u16) -> bool {
    bcd_version >= EXTENDED_MIN_BCD
}

fn unsupported_glyph(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202F}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{2600}'..='\u{27BF}'
                | '\u{E000}'..='\u{F8FF}'
                | '\u{1F000}'..='\u{1FAFF}'
                | '\u{E0000}'..='\u{E01EF}'
        )
}

/// Strip characters the speaker's pixel font cannot draw (emoji, variation
/// selectors, control codes), collapse whitespace, and cut cleanly on a UTF-8
/// character boundary at `max_bytes`.
pub fn clean_and_truncate(raw: &str, max_bytes: usize) -> String {
    let mut cleaned = String::with_capacity(raw.len().min(max_bytes));
    let mut pending_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            pending_space = !cleaned.is_empty();
            continue;
        }
        if unsupported_glyph(c) {
            continue;
        }
        let needed = usize::from(pending_space) + c.len_utf8();
        if cleaned.len() + needed > max_bytes {
            break;
        }
        if pending_space {
            cleaned.push(' ');
            pending_space = false;
        }
        cleaned.push(c);
    }
    cleaned
}

/// Whether a lyric line is only an instrumental marker (such as NetEase's `//`
/// or `~~~` or Emoji notes) rather than words to show on a one-line screen.
pub fn is_placeholder(raw: &str) -> bool {
    let clean = clean_and_truncate(raw, EXTENDED_MAX_BYTES);
    clean.is_empty()
        || clean
            .chars()
            .all(|c| matches!(c, '/' | '-' | '~' | '～' | '.' | '…' | '·' | '*'))
}

/// Choose what a one-line speaker screen should show at `position` seconds of
/// `song_name`: the song's name during the intro or an instrumental without
/// words, and the sung line (with a `0.25s` lead) once singing begins.
pub fn screen_line(
    song_name: &str,
    lines: &[crate::lyrics::Line],
    position: f64,
) -> Option<String> {
    let title = clean_and_truncate(song_name, EXTENDED_MAX_BYTES);
    let fallback = (!title.is_empty()).then_some(title);
    let single_instrumental = lines.len() == 1 && lines[0].text.contains("纯音乐");
    if single_instrumental {
        return fallback;
    }
    let Some(first_sung) = lines.iter().find(|line| !is_placeholder(&line.text)) else {
        return fallback;
    };
    if position + 0.25 < first_sung.time {
        return fallback;
    }
    let Some(idx) = lines.iter().rposition(|line| line.time <= position + 0.25) else {
        return fallback;
    };
    let clean = clean_and_truncate(&lines[idx].text, EXTENDED_MAX_BYTES);
    if clean.is_empty() || is_placeholder(&clean) {
        fallback
    } else {
        Some(clean)
    }
}

/// Build a 64-byte HID report (`0xBC` + 63-byte MC frame, `check = false`).
pub fn build_report(msg_type: [u8; 2], sub_cmd: u8, body: &[u8]) -> [u8; REPORT_LEN] {
    assert!(body.len() <= 55, "MC body exceeds 63-byte frame capacity");
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    buf[1] = START;
    buf[2] = VERSION;
    buf[3] = 0x00;
    buf[4] = (body.len() + 1) as u8;
    buf[5] = msg_type[0];
    buf[6] = msg_type[1];
    buf[7] = sub_cmd;
    buf[8..8 + body.len()].copy_from_slice(body);
    buf
}

pub fn build_read_screen_state() -> [u8; REPORT_LEN] {
    build_report(TYPE_REQUEST, SUB_READ_SCREEN_STATE, &[])
}

pub fn build_read_lyric_settings() -> [u8; REPORT_LEN] {
    build_report(TYPE_REQUEST, SUB_READ_LYRIC_SETTINGS, &[])
}

pub fn build_read_legacy_lyrics() -> [u8; REPORT_LEN] {
    build_report(TYPE_REQUEST, SUB_READ_LEGACY_LYRICS, &[])
}

/// Encode `text` into one or more 64-byte output reports for `style`.
/// Returns an empty `Vec` when `text` has no printable characters left, so
/// the speaker is never sent a blank overwrite or `enabled = 0`.
pub fn encode_lyrics(style: Style, text: &str) -> Vec<[u8; REPORT_LEN]> {
    match style {
        Style::Extended {
            font,
            font_size,
            align,
            direction,
            animation,
        } => {
            let cleaned = clean_and_truncate(text, EXTENDED_MAX_BYTES);
            if cleaned.is_empty() {
                return Vec::new();
            }
            let bytes = cleaned.as_bytes();
            let chunks: Vec<&[u8]> = bytes.chunks(EXTENDED_CHUNK_BYTES).collect();
            let total_packets = chunks.len() as u8;
            let total_bytes = bytes.len() as u8;
            chunks
                .into_iter()
                .enumerate()
                .map(|(i, chunk)| {
                    let mut body = Vec::with_capacity(10 + chunk.len());
                    body.extend_from_slice(&[
                        1,
                        font,
                        font_size,
                        align,
                        direction,
                        animation,
                        total_packets,
                        total_bytes,
                        (i + 1) as u8,
                        chunk.len() as u8,
                    ]);
                    body.extend_from_slice(chunk);
                    build_report(TYPE_SET, SUB_SET_EXTENDED_LYRICS, &body)
                })
                .collect()
        }
        Style::Legacy { scroll } => {
            let cleaned = clean_and_truncate(text, LEGACY_MAX_BYTES);
            if cleaned.is_empty() {
                return Vec::new();
            }
            let bytes = cleaned.as_bytes();
            let scroll = if (2..=6).contains(&scroll) { scroll } else { 5 };
            let mut body = Vec::with_capacity(3 + bytes.len());
            body.extend_from_slice(&[1, scroll, bytes.len() as u8]);
            body.extend_from_slice(bytes);
            vec![build_report(TYPE_SET, SUB_SET_LEGACY_LYRICS, &body)]
        }
    }
}

/// Validate and unpack a 64-byte HID input report into `(msg_type, sub_cmd, body)`.
pub fn parse_frame(report: &[u8]) -> Option<([u8; 2], u8, &[u8])> {
    if report.len() < REPORT_LEN
        || report[0] != REPORT_ID
        || report[1] != START
        || report[2] != VERSION
    {
        return None;
    }
    let flags = report[3];
    let length = usize::from(report[4]);
    let checksum_at = 7 + length;
    if length < 1 || checksum_at >= REPORT_LEN {
        return None;
    }
    let msg_type = [report[5], report[6]];
    let sub_cmd = report[7];
    let body = &report[8..checksum_at];
    if flags == 1 {
        let mut expected = flags ^ (length as u8) ^ msg_type[0] ^ msg_type[1] ^ sub_cmd;
        for &b in body {
            expected ^= b;
        }
        if report[checksum_at] != expected {
            return None;
        }
    }
    Some((msg_type, sub_cmd, body))
}

pub fn parse_screen_enabled_response(report: &[u8]) -> Option<bool> {
    let (msg_type, sub_cmd, body) = parse_frame(report)?;
    if msg_type == TYPE_REQUEST && sub_cmd == SUB_READ_SCREEN_STATE && !body.is_empty() {
        Some(body[0] != 0)
    } else {
        None
    }
}

pub fn parse_extended_settings_response(report: &[u8]) -> Option<Style> {
    let (msg_type, sub_cmd, body) = parse_frame(report)?;
    if msg_type == TYPE_REQUEST && sub_cmd == SUB_READ_LYRIC_SETTINGS && body.len() >= 6 {
        Some(Style::Extended {
            font: body[1],
            font_size: body[2],
            align: body[3],
            direction: body[4],
            animation: body[5],
        })
    } else {
        None
    }
}

pub fn parse_legacy_settings_response(report: &[u8]) -> Option<Style> {
    let (msg_type, sub_cmd, body) = parse_frame(report)?;
    if msg_type == TYPE_REQUEST && sub_cmd == SUB_READ_LEGACY_LYRICS && body.len() >= 2 {
        let scroll = if (2..=6).contains(&body[1]) {
            body[1]
        } else {
            5
        };
        Some(Style::Legacy { scroll })
    } else {
        None
    }
}

pub fn parse_notification(report: &[u8]) -> Option<DeviceEvent> {
    let (msg_type, sub_cmd, body) = parse_frame(report)?;
    if msg_type != TYPE_NOTIFY {
        return None;
    }
    match sub_cmd {
        SUB_NOTIFY_SCREEN_STATE if !body.is_empty() => {
            Some(DeviceEvent::ScreenEnabled(body[0] != 0))
        }
        SUB_NOTIFY_EXIT_LYRICS if !body.is_empty() => Some(DeviceEvent::ExitLyrics {
            screen_enabled: body[0] != 0,
        }),
        SUB_SET_EXTENDED_LYRICS if body.len() >= 6 => {
            Some(DeviceEvent::StyleChanged(Style::Extended {
                font: body[1],
                font_size: body[2],
                align: body[3],
                direction: body[4],
                animation: body[5],
            }))
        }
        SUB_SET_LEGACY_LYRICS if body.len() >= 2 => {
            let scroll = if (2..=6).contains(&body[1]) {
                body[1]
            } else {
                5
            };
            Some(DeviceEvent::StyleChanged(Style::Legacy { scroll }))
        }
        _ => None,
    }
}

#[cfg(target_os = "windows")]
pub use win::show;

#[cfg(not(target_os = "windows"))]
pub fn show(_token: u64, _line: Option<&str>) {}

#[cfg(target_os = "windows")]
mod win {
    use super::{
        DeviceEvent, PID, REPORT_LEN, Style, VID, build_read_legacy_lyrics,
        build_read_lyric_settings, build_read_screen_state, encode_lyrics,
        parse_extended_settings_response, parse_legacy_settings_response, parse_notification,
        parse_screen_enabled_response, supports_extended,
    };
    use std::{
        ffi::c_void,
        ptr,
        sync::{Mutex, mpsc},
        time::{Duration, Instant},
    };

    fn trace(message: impl std::fmt::Display) {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *ENABLED.get_or_init(|| std::env::var_os("ENJI_K20_TRACE").is_some()) {
            eprintln!("k20: {message}");
        }
    }

    type Handle = *mut c_void;

    fn is_invalid(handle: Handle) -> bool {
        handle.is_null() || handle as isize == -1
    }

    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;

    const ERROR_IO_PENDING: u32 = 997;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;

    const CR_SUCCESS: u32 = 0;
    const CM_GET_DEVICE_INTERFACE_LIST_PRESENT: u32 = 0;
    const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
    const USAGE_PAGE_CONSUMER: u16 = 0x000C;
    const USAGE_CONSUMER_CONTROL: u16 = 0x0001;

    const QUERY_TIMEOUT_MS: u32 = 150;
    const WRITE_TIMEOUT_MS: u32 = 200;
    const PACKET_GAP: Duration = Duration::from_millis(10);
    const PROBE_COOLDOWN: Duration = Duration::from_secs(2);

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const GUID_DEVINTERFACE_HID: Guid = Guid {
        data1: 0x4D1E_55B2,
        data2: 0xF16F,
        data3: 0x11CF,
        data4: [0x88, 0xCB, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30],
    };

    #[repr(C)]
    #[allow(dead_code)]
    union OverlappedOffset {
        offset: [u32; 2],
        pointer: *mut c_void,
    }

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        u: OverlappedOffset,
        h_event: Handle,
    }

    impl Overlapped {
        fn with_event(event: Handle) -> Self {
            Self {
                internal: 0,
                internal_high: 0,
                u: OverlappedOffset { offset: [0, 0] },
                h_event: event,
            }
        }
    }

    #[repr(C)]
    struct HiddAttributes {
        size: u32,
        vendor_id: u16,
        product_id: u16,
        version_number: u16,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct HidpCaps {
        usage: u16,
        usage_page: u16,
        input_report_byte_length: u16,
        output_report_byte_length: u16,
        feature_report_byte_length: u16,
        reserved: [u16; 17],
        number_link_collection_nodes: u16,
        number_input_button_caps: u16,
        number_input_value_caps: u16,
        number_input_data_indices: u16,
        number_output_button_caps: u16,
        number_output_value_caps: u16,
        number_output_data_indices: u16,
        number_feature_button_caps: u16,
        number_feature_value_caps: u16,
        number_feature_data_indices: u16,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *const c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        fn ReadFile(
            file: Handle,
            buffer: *mut u8,
            bytes_to_read: u32,
            bytes_read: *mut u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn WriteFile(
            file: Handle,
            buffer: *const u8,
            bytes_to_write: u32,
            bytes_written: *mut u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn CloseHandle(object: Handle) -> i32;
        fn CreateEventW(
            event_attributes: *const c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
        fn ResetEvent(event: Handle) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CancelIoEx(file: Handle, overlapped: *const Overlapped) -> i32;
        fn GetOverlappedResult(
            file: Handle,
            overlapped: *const Overlapped,
            bytes_transferred: *mut u32,
            wait: i32,
        ) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "cfgmgr32")]
    unsafe extern "system" {
        fn CM_Get_Device_Interface_List_SizeW(
            len: *mut u32,
            interface_class: *const Guid,
            device_id: *const u16,
            flags: u32,
        ) -> u32;
        fn CM_Get_Device_Interface_ListW(
            interface_class: *const Guid,
            device_id: *const u16,
            buffer: *mut u16,
            buffer_len: u32,
            flags: u32,
        ) -> u32;
    }

    // Embed the import library in the Rust static library too: the Windows
    // application is finally linked by C++, not by rustc.
    #[link(name = "hid", kind = "raw-dylib")]
    unsafe extern "system" {
        fn HidD_GetAttributes(device: Handle, attributes: *mut HiddAttributes) -> u8;
        fn HidD_GetPreparsedData(device: Handle, preparsed_data: *mut *mut c_void) -> u8;
        fn HidD_FreePreparsedData(preparsed_data: *mut c_void) -> u8;
        fn HidP_GetCaps(preparsed_data: *mut c_void, capabilities: *mut HidpCaps) -> i32;
    }

    struct OwnedHandle(Handle);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !is_invalid(self.0) {
                // SAFETY: `self.0` was returned by `CreateFileW` or `CreateEventW` and is owned solely here.
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    /// Heap-pinned async reader whose `Drop` cancels and awaits any in-flight
    /// kernel `OVERLAPPED` writeback before freeing the buffer.
    struct PendingRead {
        device: Handle,
        event: OwnedHandle,
        overlapped: Overlapped,
        buf: [u8; REPORT_LEN],
        armed: bool,
    }

    impl PendingRead {
        fn new(device: Handle) -> Option<Box<Self>> {
            // SAFETY: anonymous manual-reset event with no security attributes.
            let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            if is_invalid(event) {
                return None;
            }
            Some(Box::new(Self {
                device,
                event: OwnedHandle(event),
                overlapped: Overlapped::with_event(event),
                buf: [0u8; REPORT_LEN],
                armed: false,
            }))
        }

        fn poll(&mut self, mut timeout_ms: u32) -> Result<Option<[u8; REPORT_LEN]>, ()> {
            loop {
                if !self.armed {
                    // SAFETY: `self` lives inside a `Box` whose address does not move until `Drop` cancels I/O.
                    unsafe {
                        let _ = ResetEvent(self.event.0);
                        self.overlapped = Overlapped::with_event(self.event.0);
                        let mut read_now = 0u32;
                        let ok = ReadFile(
                            self.device,
                            self.buf.as_mut_ptr(),
                            REPORT_LEN as u32,
                            &mut read_now,
                            &mut self.overlapped,
                        );
                        if ok != 0 {
                            if read_now as usize == REPORT_LEN {
                                return Ok(Some(self.buf));
                            }
                            timeout_ms = 0;
                            continue;
                        }
                        if GetLastError() != ERROR_IO_PENDING {
                            return Err(());
                        }
                        self.armed = true;
                    }
                }

                // SAFETY: `self.event` is valid and `self.overlapped` is armed on `self.device`.
                let wait = unsafe { WaitForSingleObject(self.event.0, timeout_ms) };
                match wait {
                    WAIT_OBJECT_0 => {
                        self.armed = false;
                        let mut transferred = 0u32;
                        // SAFETY: event signaled for `self.overlapped`.
                        let ok = unsafe {
                            GetOverlappedResult(self.device, &self.overlapped, &mut transferred, 0)
                        };
                        if ok == 0 {
                            return Err(());
                        }
                        if transferred as usize == REPORT_LEN {
                            return Ok(Some(self.buf));
                        }
                        timeout_ms = 0;
                    }
                    WAIT_TIMEOUT => return Ok(None),
                    _ => return Err(()),
                }
            }
        }
    }

    impl Drop for PendingRead {
        fn drop(&mut self) {
            if self.armed {
                // SAFETY: cancel this specific `OVERLAPPED` and block until the kernel releases `&mut self.buf`.
                unsafe {
                    let _ = CancelIoEx(self.device, &self.overlapped);
                    let mut transferred = 0u32;
                    let _ = GetOverlappedResult(self.device, &self.overlapped, &mut transferred, 1);
                }
                self.armed = false;
            }
        }
    }

    /// Field order matters: `reader` drops first while `device` is still open.
    struct Connected {
        reader: Box<PendingRead>,
        write_event: OwnedHandle,
        device: OwnedHandle,
        extended: bool,
        style: Style,
        style_synced: bool,
        screen_enabled: bool,
    }

    impl Connected {
        fn open() -> Option<Self> {
            for path in hid_paths() {
                if let Some(conn) = Self::try_open_path(&path) {
                    return Some(conn);
                }
            }
            None
        }

        fn try_open_path(path: &[u16]) -> Option<Self> {
            let text = String::from_utf16_lossy(path).to_ascii_lowercase();
            if !text.contains("vid_3837") || !text.contains("pid_60c6") {
                return None;
            }
            // SAFETY: `path` is NUL-terminated UTF-16 from `CM_Get_Device_Interface_ListW`.
            let raw = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    ptr::null_mut(),
                )
            };
            if is_invalid(raw) {
                return None;
            }
            let device = OwnedHandle(raw);

            let mut attrs = HiddAttributes {
                size: std::mem::size_of::<HiddAttributes>() as u32,
                vendor_id: 0,
                product_id: 0,
                version_number: 0,
            };
            // SAFETY: `device.0` is an open HID file handle and `attrs.size` matches `HIDD_ATTRIBUTES`.
            if unsafe { HidD_GetAttributes(device.0, &mut attrs) } == 0
                || attrs.vendor_id != VID
                || attrs.product_id != PID
            {
                return None;
            }

            let mut preparsed: *mut c_void = ptr::null_mut();
            // SAFETY: `device.0` is valid; `preparsed` is freed immediately below.
            if unsafe { HidD_GetPreparsedData(device.0, &mut preparsed) } == 0
                || preparsed.is_null()
            {
                return None;
            }
            let mut caps: HidpCaps = unsafe { std::mem::zeroed() };
            let status = unsafe {
                let s = HidP_GetCaps(preparsed, &mut caps);
                let _ = HidD_FreePreparsedData(preparsed);
                s
            };
            if status != HIDP_STATUS_SUCCESS
                || caps.usage_page != USAGE_PAGE_CONSUMER
                || caps.usage != USAGE_CONSUMER_CONTROL
                || usize::from(caps.input_report_byte_length) != REPORT_LEN
                || usize::from(caps.output_report_byte_length) != REPORT_LEN
            {
                return None;
            }

            // SAFETY: anonymous manual-reset event.
            let write_ev = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            if is_invalid(write_ev) {
                return None;
            }
            let write_event = OwnedHandle(write_ev);
            let reader = PendingRead::new(device.0)?;

            let extended = supports_extended(attrs.version_number);
            let mut conn = Self {
                reader,
                write_event,
                device,
                extended,
                style: if extended {
                    Style::default_extended()
                } else {
                    Style::default_legacy()
                },
                style_synced: false,
                screen_enabled: true,
            };
            conn.sync_initial_state();
            trace(format_args!(
                "connected firmware={:04x} style={:?} confirmed={} screen={}",
                attrs.version_number, conn.style, conn.style_synced, conn.screen_enabled
            ));
            Some(conn)
        }

        fn write_report(&mut self, report: &[u8; REPORT_LEN]) -> Result<(), ()> {
            // SAFETY: `overlapped` and `report` stay on stack until completion or `CancelIoEx` + blocking `GetOverlappedResult`.
            unsafe {
                let _ = ResetEvent(self.write_event.0);
                let mut overlapped = Overlapped::with_event(self.write_event.0);
                let mut written = 0u32;
                let ok = WriteFile(
                    self.device.0,
                    report.as_ptr(),
                    REPORT_LEN as u32,
                    &mut written,
                    &mut overlapped,
                );
                if ok != 0 {
                    return if written as usize == REPORT_LEN {
                        Ok(())
                    } else {
                        Err(())
                    };
                }
                if GetLastError() != ERROR_IO_PENDING {
                    return Err(());
                }
                match WaitForSingleObject(self.write_event.0, WRITE_TIMEOUT_MS) {
                    WAIT_OBJECT_0 => {
                        let done = GetOverlappedResult(self.device.0, &overlapped, &mut written, 0);
                        if done != 0 && written as usize == REPORT_LEN {
                            Ok(())
                        } else {
                            Err(())
                        }
                    }
                    _ => {
                        let _ = CancelIoEx(self.device.0, &overlapped);
                        let _ = GetOverlappedResult(self.device.0, &overlapped, &mut written, 1);
                        Err(())
                    }
                }
            }
        }

        fn sync_initial_state(&mut self) {
            let _ = self.drain_notifications();
            if self.write_report(&build_read_screen_state()).is_ok() {
                if let Some(enabled) = self.wait_for(parse_screen_enabled_response) {
                    self.screen_enabled = enabled;
                }
            }
            if self.extended {
                if self.write_report(&build_read_lyric_settings()).is_ok() {
                    if let Some(style) = self.wait_for(parse_extended_settings_response) {
                        self.style = style;
                        self.style_synced = true;
                        return;
                    }
                }
            }
            if self.write_report(&build_read_legacy_lyrics()).is_ok() {
                if let Some(style) = self.wait_for(parse_legacy_settings_response) {
                    self.style = style;
                    self.style_synced = true;
                }
            }
        }

        fn wait_for<T>(&mut self, parse: fn(&[u8]) -> Option<T>) -> Option<T> {
            let deadline = Instant::now() + Duration::from_millis(u64::from(QUERY_TIMEOUT_MS));
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let ms = left.as_millis().min(u128::from(QUERY_TIMEOUT_MS)) as u32;
                let report = self.reader.poll(ms).ok()??;
                if let Some(value) = parse(&report) {
                    return Some(value);
                }
                if let Some(enabled) = parse_screen_enabled_response(&report) {
                    self.screen_enabled = enabled;
                }
                self.apply_event(&report);
                if left.is_zero() {
                    return None;
                }
            }
        }

        /// Drain any pending notifications from the speaker (e.g. style edits
        /// from companion tools or the speaker exiting its lyric overlay).
        /// Returns `true` if the speaker exited lyrics since the last send.
        fn drain_notifications(&mut self) -> Result<bool, ()> {
            let mut exited = false;
            while let Some(report) = self.reader.poll(0)? {
                if self.apply_event(&report) {
                    exited = true;
                }
            }
            Ok(exited)
        }

        fn apply_event(&mut self, report: &[u8]) -> bool {
            match parse_notification(report) {
                Some(DeviceEvent::ScreenEnabled(on)) => {
                    let changed = self.screen_enabled != on;
                    self.screen_enabled = on;
                    changed
                }
                Some(DeviceEvent::ExitLyrics { screen_enabled }) => {
                    self.screen_enabled = screen_enabled;
                    true
                }
                Some(DeviceEvent::StyleChanged(style)) => {
                    self.style = style;
                    false
                }
                None => false,
            }
        }
    }

    /// Return NUL-terminated UTF-16 device interface paths for all present HID collections.
    fn hid_paths() -> Vec<Vec<u16>> {
        let mut size = 0u32;
        // SAFETY: queries required UTF-16 buffer character count for `GUID_DEVINTERFACE_HID`.
        if unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut size,
                &GUID_DEVINTERFACE_HID,
                ptr::null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        } != CR_SUCCESS
            || size <= 1
        {
            return Vec::new();
        }
        let cap = size.saturating_add(256);
        let mut buf = vec![0u16; cap as usize];
        // SAFETY: `buf` has `cap` `u16` elements and receives a MULTI_SZ list.
        if unsafe {
            CM_Get_Device_Interface_ListW(
                &GUID_DEVINTERFACE_HID,
                ptr::null(),
                buf.as_mut_ptr(),
                cap,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        } != CR_SUCCESS
        {
            return Vec::new();
        }
        let mut paths = Vec::new();
        let mut start = 0;
        for i in 0..buf.len() {
            if buf[i] == 0 {
                if i > start {
                    paths.push(buf[start..=i].to_vec());
                } else {
                    break;
                }
                start = i + 1;
            }
        }
        paths
    }

    type Message = (u64, Option<String>);

    static SENDER: Mutex<Option<mpsc::Sender<Message>>> = Mutex::new(None);

    pub fn show(token: u64, line: Option<&str>) {
        let mut guard = SENDER.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if line.is_none() {
                return;
            }
            let (tx, rx) = mpsc::channel();
            if std::thread::Builder::new()
                .name("enji-k20".into())
                .spawn(move || run(rx))
                .is_err()
            {
                return;
            }
            *guard = Some(tx);
        }
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send((token, line.map(str::to_string)));
        }
    }

    fn run(rx: mpsc::Receiver<Message>) {
        let mut device: Option<Connected> = None;
        let mut next_probe: Option<Instant> = None;
        let mut wanted: Option<(u64, String)> = None;
        let mut last_sent: Option<(u64, String)> = None;
        let mut pending: Option<Message> = None;
        let mut retried_sync = false;

        loop {
            if let Some(m) = pending.take() {
                let mut msg = m;
                while let Ok(newer) = rx.try_recv() {
                    msg = newer;
                }
                wanted = msg.1.map(|text| (msg.0, text));
            } else if wanted.is_some() {
                let wait = if device.is_some() {
                    Duration::from_millis(200)
                } else {
                    next_probe
                        .map(|until| until.saturating_duration_since(Instant::now()))
                        .unwrap_or(PROBE_COOLDOWN)
                        .max(Duration::from_millis(50))
                };
                match rx.recv_timeout(wait) {
                    Ok(mut msg) => {
                        while let Ok(newer) = rx.try_recv() {
                            msg = newer;
                        }
                        wanted = msg.1.map(|text| (msg.0, text));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(mut msg) => {
                        while let Ok(newer) = rx.try_recv() {
                            msg = newer;
                        }
                        wanted = msg.1.map(|text| (msg.0, text));
                    }
                    Err(_) => break,
                }
            }

            let Some((token, text)) = wanted.clone() else {
                if last_sent.take().is_some() {
                    trace("paused: no lyric writes");
                }
                continue;
            };

            if device.is_none() {
                let now = Instant::now();
                if next_probe.is_some_and(|until| now < until) {
                    continue;
                }
                device = Connected::open();
                retried_sync = false;
                last_sent = None;
                if device.is_none() {
                    next_probe = Some(now + PROBE_COOLDOWN);
                    continue;
                }
                next_probe = None;
            }

            let Some(conn) = device.as_mut() else {
                continue;
            };
            if !conn.style_synced && !retried_sync && last_sent.is_some() {
                retried_sync = true;
                conn.sync_initial_state();
            }
            match conn.drain_notifications() {
                Ok(true) => last_sent = None,
                Ok(false) => {}
                Err(()) => {
                    trace("device read failed: reconnecting");
                    device = None;
                    last_sent = None;
                    continue;
                }
            }

            // Queries can take longer than a lyric change or a pause. Never
            // write the old target after a newer one has reached the channel.
            if let Ok(newer) = rx.try_recv() {
                pending = Some(newer);
                continue;
            }
            if !conn.screen_enabled {
                continue;
            }
            if last_sent
                .as_ref()
                .is_some_and(|(was_token, was_text)| *was_token == token && was_text == &text)
            {
                continue;
            }

            let reports = encode_lyrics(conn.style, &text);
            if reports.is_empty() {
                continue;
            }

            let mut interrupted = false;
            let mut failed = false;
            let count = reports.len();
            for (i, report) in reports.iter().enumerate() {
                if conn.write_report(report).is_err() {
                    failed = true;
                    break;
                }
                if i + 1 < count {
                    std::thread::sleep(PACKET_GAP);
                    if let Ok(newer) = rx.try_recv() {
                        pending = Some(newer);
                        interrupted = true;
                        break;
                    }
                }
            }
            if failed {
                trace("device write failed: reconnecting");
                device = None;
                last_sent = None;
            } else if !interrupted {
                trace(format_args!(
                    "sent token={token} packets={count} text={text:?}"
                ));
                last_sent = Some((token, text));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_version_0034_and_above_use_extended_protocol() {
        assert!(!supports_extended(0x0033));
        assert!(supports_extended(0x0034));
        assert!(supports_extended(0x0035));
        assert!(supports_extended(0x0100));
    }

    #[test]
    fn clean_and_truncate_strips_emoji_and_keeps_utf8_char_boundaries() {
        assert_eq!(
            clean_and_truncate("  🎵 孤勇者\u{FE0F} \n  陈奕迅 ♪ ", 255),
            "孤勇者 陈奕迅"
        );
        // "海猫が鳴く頃に" = 7 * 3 = 21 UTF-8 bytes; limit 10 fits exactly 3 chars (9 bytes).
        let cut = clean_and_truncate("海猫が鳴く頃に", 10);
        assert_eq!(cut, "海猫が");
        assert!(cut.len() <= 10);
    }

    #[test]
    fn read_commands_match_official_mc_frames() {
        let r = build_read_lyric_settings();
        assert_eq!(
            &r[..9],
            &[0xBC, 0xAA, 0x01, 0x00, 0x01, 0x00, 0x01, 0x11, 0x00]
        );
        assert!(r[9..].iter().all(|&b| b == 0));

        let s = build_read_screen_state();
        assert_eq!(
            &s[..9],
            &[0xBC, 0xAA, 0x01, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00]
        );
    }

    #[test]
    fn legacy_lyrics_pack_single_0x0b_frame_and_cap_at_52_bytes() {
        let long_cn = "一二三四五六七八九十甲乙丙丁戊己庚辛壬癸"; // 20 chars = 60 bytes
        let packets = encode_lyrics(Style::Legacy { scroll: 5 }, long_cn);
        assert_eq!(packets.len(), 1);
        let p = &packets[0];
        assert_eq!(p[0], 0xBC);
        assert_eq!(p[1], 0xAA);
        assert_eq!(p[2], 0x01);
        assert_eq!(p[5..8], [0x00, 0x02, 0x0B]);
        assert_eq!(p[8], 1); // enabled
        assert_eq!(p[9], 5); // scroll
        let byte_len = usize::from(p[10]);
        assert_eq!(byte_len, 51); // 17 * 3 <= 52
        assert_eq!(usize::from(p[4]), 1 + 3 + byte_len);
        assert_eq!(
            std::str::from_utf8(&p[11..11 + byte_len]).unwrap(),
            "一二三四五六七八九十甲乙丙丁戊己庚"
        );
    }

    #[test]
    fn extended_lyrics_split_into_45_byte_packets_with_1_based_index() {
        // 20 Chinese chars = 60 UTF-8 bytes -> 2 packets: 45 bytes + 15 bytes.
        let text = "谁说站在光里的才算英雄爱你孤身走暗巷爱你不跪的模样";
        let cut = clean_and_truncate(text, 60);
        assert_eq!(cut.len(), 60);
        let style = Style::Extended {
            font: 1,
            font_size: 2,
            align: 1,
            direction: 0,
            animation: 4,
        };
        let packets = encode_lyrics(style, &cut);
        assert_eq!(packets.len(), 2);

        let p1 = &packets[0];
        assert_eq!(p1[0..8], [0xBC, 0xAA, 0x01, 0x00, 56, 0x00, 0x02, 0x27]);
        assert_eq!(&p1[8..18], &[1, 1, 2, 1, 0, 4, 2, 60, 1, 45]);
        assert_eq!(p1[63], 0); // checksum slot

        let p2 = &packets[1];
        assert_eq!(p2[0..8], [0xBC, 0xAA, 0x01, 0x00, 26, 0x00, 0x02, 0x27]);
        assert_eq!(&p2[8..18], &[1, 1, 2, 1, 0, 4, 2, 60, 2, 15]);

        let mut joined = Vec::new();
        joined.extend_from_slice(&p1[18..18 + 45]);
        joined.extend_from_slice(&p2[18..18 + 15]);
        assert_eq!(std::str::from_utf8(&joined).unwrap(), cut);
    }

    #[test]
    fn empty_or_emoji_only_text_produces_no_packets() {
        assert!(encode_lyrics(Style::default_extended(), "  🎵🎶  ").is_empty());
        assert!(encode_lyrics(Style::default_legacy(), "").is_empty());
    }

    #[test]
    fn responses_and_notifications_parse_styles_and_screen_state() {
        let resp = build_report(
            TYPE_REQUEST,
            SUB_READ_LYRIC_SETTINGS,
            &[1, 2, 1, 1, 0, 3, 12],
        );
        assert_eq!(
            parse_extended_settings_response(&resp),
            Some(Style::Extended {
                font: 2,
                font_size: 1,
                align: 1,
                direction: 0,
                animation: 3,
            })
        );

        let exit = build_report(
            TYPE_NOTIFY,
            SUB_NOTIFY_EXIT_LYRICS,
            &[1, 0, 255, 0, 1, 2, 0],
        );
        assert_eq!(
            parse_notification(&exit),
            Some(DeviceEvent::ExitLyrics {
                screen_enabled: true
            })
        );

        let off = build_report(TYPE_NOTIFY, SUB_NOTIFY_SCREEN_STATE, &[0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            parse_notification(&off),
            Some(DeviceEvent::ScreenEnabled(false))
        );
    }

    #[test]
    fn screen_line_shows_title_in_intro_and_skips_placeholders() {
        let lines = crate::lyrics::parse(
            "[00:12.00]第一句歌词\n[00:12.00]First line\n[00:20.00]//\n[00:25.00]♪ 第二句 ✨\n",
            "",
        );
        assert_eq!(
            screen_line("孤勇者", &lines, 0.0).as_deref(),
            Some("孤勇者")
        );
        assert_eq!(
            screen_line("孤勇者", &lines, 11.70).as_deref(),
            Some("孤勇者")
        );
        assert_eq!(
            screen_line("孤勇者", &lines, 11.75).as_deref(),
            Some("第一句歌词")
        );
        assert_eq!(
            screen_line("孤勇者", &lines, 20.5).as_deref(),
            Some("孤勇者")
        );
        assert_eq!(
            screen_line("孤勇者", &lines, 24.80).as_deref(),
            Some("第二句")
        );

        let instrumental = crate::lyrics::parse("[00:00.00]纯音乐，请欣赏\n", "");
        assert_eq!(
            screen_line("Summer", &instrumental, 30.0).as_deref(),
            Some("Summer")
        );
    }
}
