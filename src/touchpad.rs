//! プレシジョンタッチパッドの生データ (指ごとの座標) を Raw Input で受け取る。
//!
//! ピンチの判定そのものは touch_pinch.rs (Win32 非依存)。ここは座標を取り出すところまで。
//!
//! # Raw Input とは
//! キーボード・マウス・HID デバイスの入力を、アプリ向けのメッセージ (WM_MOUSEWHEEL など) に
//! 変換される前の「デバイスが送ってきたそのままのデータ」で受け取る仕組み。
//! RegisterRawInputDevices で「このデバイスの種類 (Usage Page / Usage) を受け取りたい」と登録すると、
//! 入力のたびに WM_INPUT が自分のウィンドウに届く。
//! RIDEV_INPUTSINK を付けると、自分がフォアグラウンドでなくても (= 常駐ツールでも) 受け取れる。
//!
//! # プレシジョンタッチパッドの HID レポート
//! タッチパッドは Digitizer ページ (0x0D) の Touch Pad (0x05) として見える。レポートの中身は
//! 「指 1 本分の枠 (Finger コレクション)」が複数並んだ形で、枠ごとに
//!   - Tip Switch (0x0D/0x42) : 触れているか
//!   - Contact Identifier (0x0D/0x51) : 指の番号
//!   - X / Y (0x01/0x30, 0x01/0x31) : 座標
//!
//! があり、レポート全体に Contact Count (0x0D/0x54) がある。
//! どのバイトに何が入っているかはデバイスごとに違うので、デバイスの「レポート記述子」を
//! 解析済みの形 (preparsed data) で取得し、HidP_GetUsageValue などに読ませる。
//!
//! Raw Input は入力を覗くだけで、アプリへの配送は止められない。
//! タッチパッドのピンチを他のアプリにも渡したくない場合は Windows 側の「ピンチ操作でズーム」を
//! オフにする (README 参照)。オフにしても、ここで受け取る生データは変わらず届く。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidP_GetCaps, HidP_GetUsageValue,
    HidP_GetUsages, HidP_GetValueCaps, HidP_Input, HidP_MaxUsageListLength, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDI_PREPARSEDDATA, RIM_TYPEHID, RegisterRawInputDevices,
};

use crate::touch_pinch::{Contact, FrameAssembler, PinchDetector, Slot};

const PAGE_GENERIC_DESKTOP: u16 = 0x01;
const PAGE_DIGITIZER: u16 = 0x0D;
const USAGE_TOUCH_PAD: u16 = 0x05;
const USAGE_X: u16 = 0x30;
const USAGE_Y: u16 = 0x31;
const USAGE_TIP_SWITCH: u16 = 0x42;
const USAGE_CONTACT_ID: u16 = 0x51;
const USAGE_CONTACT_COUNT: u16 = 0x54;

/// 最後に 2 本指を検出してからこの時間は「タッチパッドでピンチ中かもしれない」とみなす
/// (hook.rs で、Windows が作った Ctrl+ホイールを二重に処理しないために使う)。
const TWO_FINGER_HOLD: Duration = Duration::from_millis(300);

/// タッチパッドから WM_INPUT を受け取るよう登録する。
/// タッチパッドが無い PC でも登録自体は成功する (何も届かないだけ)。
pub fn register(hwnd: HWND) -> windows::core::Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: PAGE_DIGITIZER,
        usUsage: USAGE_TOUCH_PAD,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
}

#[derive(Default)]
pub struct Touchpad {
    /// デバイスハンドル → 解析済みの情報 (タッチパッドが複数あってもよいように)
    devices: HashMap<isize, Option<Device>>,
    detector: PinchDetector,
    last_two_fingers: Option<Instant>,
}

impl Touchpad {
    /// WM_INPUT を 1 つ処理し、ピンチ中ならホイール換算の変化量を返す。
    pub fn on_input(&mut self, handle: HRAWINPUT) -> Option<f32> {
        let mut buf = read_raw_input(handle)?;
        // SAFETY: buf は GetRawInputData が書き込んだ RAWINPUT (u64 単位で確保しているので境界も揃っている)。
        let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
        if raw.header.dwType != RIM_TYPEHID.0 {
            return None;
        }
        let key = raw.header.hDevice.0 as isize;
        let hdevice = raw.header.hDevice;
        let (size, count) = unsafe { (raw.data.hid.dwSizeHid, raw.data.hid.dwCount) };

        // RAWHID の bRawData は「長さ dwSizeHid のレポートが dwCount 個」並んだ可変長配列
        let offset = unsafe { (&raw const raw.data.hid.bRawData) as usize } - buf.as_ptr() as usize;
        let total = (size as usize).checked_mul(count as usize)?;
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 8) };
        let reports = bytes.get_mut(offset..offset.checked_add(total)?)?;

        let device = self
            .devices
            .entry(key)
            .or_insert_with(|| Device::open(hdevice))
            .as_mut()?;

        let mut units = None;
        for report in reports.chunks_exact_mut(size.max(1) as usize) {
            let Some(contacts) = device.read(report) else {
                continue;
            };
            if let Some(u) = self.detector.update(&contacts) {
                *units.get_or_insert(0.0) += u;
            }
            if self.detector.two_fingers_down() {
                self.last_two_fingers = Some(Instant::now());
            }
        }
        units
    }

    /// 直前までタッチパッドに 2 本指が触れていたか
    pub fn two_fingers_recently(&self, now: Instant) -> bool {
        self.last_two_fingers
            .is_some_and(|t| now.saturating_duration_since(t) < TWO_FINGER_HOLD)
    }
}

/// GetRawInputData で WM_INPUT の中身を取り出す。
fn read_raw_input(handle: HRAWINPUT) -> Option<Vec<u64>> {
    let header = size_of::<RAWINPUTHEADER>() as u32;
    let mut size = 0u32;
    unsafe {
        // 1 回目はサイズの問い合わせ
        GetRawInputData(handle, RID_INPUT, None, &mut size, header);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let written = GetRawInputData(
            handle,
            RID_INPUT,
            Some(buf.as_mut_ptr().cast()),
            &mut size,
            header,
        );
        (written != u32::MAX && written >= size_of::<RAWINPUTHEADER>() as u32).then_some(buf)
    }
}

/// 指 1 本分の枠 (Finger コレクション) の番号
struct Finger {
    link: u16,
}

struct Device {
    /// preparsed data。HidP_* 関数にポインタで渡す (u64 で確保してアライメントを保証)
    preparsed: Vec<u64>,
    contact_count_link: Option<u16>,
    fingers: Vec<Finger>,
    /// 論理座標 → 物理座標 の倍率。X と Y で解像度が違うタッチパッドでも距離を正しく測るため
    scale_x: f32,
    scale_y: f32,
    max_usages: usize,
    frames: FrameAssembler,
}

impl Device {
    /// レポート記述子を調べ、タッチパッドとして使えるなら Some。
    fn open(hdevice: HANDLE) -> Option<Self> {
        let mut size = 0u32;
        unsafe {
            GetRawInputDeviceInfoW(Some(hdevice), RIDI_PREPARSEDDATA, None, &mut size);
        }
        if size == 0 {
            return None;
        }
        let mut preparsed = vec![0u64; (size as usize).div_ceil(8)];
        let got = unsafe {
            GetRawInputDeviceInfoW(
                Some(hdevice),
                RIDI_PREPARSEDDATA,
                Some(preparsed.as_mut_ptr().cast()),
                &mut size,
            )
        };
        if got == u32::MAX || got == 0 {
            return None;
        }
        let pp = PHIDP_PREPARSED_DATA(preparsed.as_ptr() as isize);

        let mut caps = HIDP_CAPS::default();
        if unsafe { HidP_GetCaps(pp, &mut caps) } != HIDP_STATUS_SUCCESS {
            return None;
        }
        let mut len = caps.NumberInputValueCaps;
        let mut values = vec![HIDP_VALUE_CAPS::default(); len as usize];
        if unsafe { HidP_GetValueCaps(HidP_Input, values.as_mut_ptr(), &mut len, pp) }
            != HIDP_STATUS_SUCCESS
        {
            return None;
        }
        values.truncate(len as usize);

        let mut device = Device {
            contact_count_link: None,
            fingers: Vec::new(),
            scale_x: 1.0,
            scale_y: 1.0,
            max_usages: unsafe {
                HidP_MaxUsageListLength(HidP_Input, Some(PAGE_DIGITIZER), pp) as usize
            },
            frames: FrameAssembler::default(),
            preparsed: Vec::new(),
        };
        for cap in &values {
            // SAFETY: IsRange に応じて union のどちらが有効かが決まる
            let usage = unsafe {
                if cap.IsRange {
                    cap.Anonymous.Range.UsageMin
                } else {
                    cap.Anonymous.NotRange.Usage
                }
            };
            match (cap.UsagePage, usage) {
                (PAGE_DIGITIZER, USAGE_CONTACT_COUNT) => {
                    device.contact_count_link = Some(cap.LinkCollection);
                }
                (PAGE_GENERIC_DESKTOP, USAGE_X) => {
                    if !device.fingers.iter().any(|f| f.link == cap.LinkCollection) {
                        device.fingers.push(Finger {
                            link: cap.LinkCollection,
                        });
                    }
                    device.scale_x = physical_scale(cap);
                }
                (PAGE_GENERIC_DESKTOP, USAGE_Y) => device.scale_y = physical_scale(cap),
                _ => {}
            }
        }
        if device.fingers.is_empty() {
            return None;
        }
        device.preparsed = preparsed;
        Some(device)
    }

    fn pp(&self) -> PHIDP_PREPARSED_DATA {
        PHIDP_PREPARSED_DATA(self.preparsed.as_ptr() as isize)
    }

    /// レポート 1 つを読み、フレームが揃ったら触れている指を返す。
    fn read(&mut self, report: &mut [u8]) -> Option<Vec<Contact>> {
        let count = self
            .contact_count_link
            .and_then(|link| self.value(report, PAGE_DIGITIZER, link, USAGE_CONTACT_COUNT));
        let slots: Vec<Slot> = self
            .fingers
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let x = self.value(report, PAGE_GENERIC_DESKTOP, f.link, USAGE_X)?;
                let y = self.value(report, PAGE_GENERIC_DESKTOP, f.link, USAGE_Y)?;
                let id = self
                    .value(report, PAGE_DIGITIZER, f.link, USAGE_CONTACT_ID)
                    .unwrap_or(i as u32);
                Some(Slot {
                    touching: self.tip_switch(report, f.link),
                    contact: Contact {
                        id,
                        x: x as f32 * self.scale_x,
                        y: y as f32 * self.scale_y,
                    },
                })
            })
            .collect();
        self.frames.push(count, &slots)
    }

    fn value(&self, report: &[u8], page: u16, link: u16, usage: u16) -> Option<u32> {
        let mut value = 0u32;
        let status = unsafe {
            HidP_GetUsageValue(
                HidP_Input,
                page,
                Some(link),
                usage,
                &mut value,
                self.pp(),
                report,
            )
        };
        (status == HIDP_STATUS_SUCCESS).then_some(value)
    }

    /// Tip Switch (ボタン扱いの usage) がオンか
    fn tip_switch(&self, report: &mut [u8], link: u16) -> bool {
        let mut usages = vec![0u16; self.max_usages.max(1)];
        let mut len = usages.len() as u32;
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                PAGE_DIGITIZER,
                Some(link),
                usages.as_mut_ptr(),
                &mut len,
                self.pp(),
                report,
            )
        };
        status == HIDP_STATUS_SUCCESS && usages[..len as usize].contains(&USAGE_TIP_SWITCH)
    }
}

/// 論理値 1 あたりの物理量。物理範囲が無ければ 1 (論理値のまま)。
fn physical_scale(cap: &HIDP_VALUE_CAPS) -> f32 {
    let logical = cap.LogicalMax as f32 - cap.LogicalMin as f32;
    let physical = cap.PhysicalMax as f32 - cap.PhysicalMin as f32;
    if logical > 0.0 && physical > 0.0 {
        physical / logical
    } else {
        1.0
    }
}
