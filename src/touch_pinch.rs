//! タッチパッドの生の指の座標からピンチを検出する。
//!
//! Win32 に依存しない純粋なロジックなので Linux でもテストできる。
//! 実際に座標を読み取るのは touchpad.rs (Raw Input + HID)。
//!
//! # なぜ自前でピンチを検出するのか
//! Windows がピンチを「Ctrl+ホイール」に変換して届けるのは、ピンチを自前で処理しない
//! 古いタイプのアプリ (PowerShell のコンソールなど) だけ。Chrome / Edge / エクスプローラー /
//! Office / ストアアプリなどは DirectManipulation などでタッチパッドのジェスチャーを直接受け取るので、
//! Ctrl+ホイールは一切発生せず、マウスフック (hook.rs) では何も見えない。
//!
//! そこでタッチパッドの生データ (指ごとの座標) を Raw Input で受け取り、
//! 2 本指の間隔の変化からピンチを自分で判定する。これならフォアグラウンドのアプリが何であっても動く。
//!
//! # ホイール量への換算
//! 間隔が 2 倍になったら `UNITS_PER_DOUBLING` (= 400) として、既存のホイール量の処理
//! (gesture.rs の閾値、screen_zoom.rs の倍率) にそのまま渡す。
//! 画面ズームの既定値 `screen_zoom_speed = 400` と合わせてあるので、既定では
//! **指の間隔が 2 倍になると画面も 2 倍** という、スマホと同じ感覚になる。

/// 指の間隔が 2 倍になったときのホイール換算量
pub const UNITS_PER_DOUBLING: f32 = 400.0;

/// 2 本指を置いてから、間隔がこの割合 (log2) 以上変わったらピンチとみなす。
/// 2 本指スクロールでも指の間隔は多少ぶれるので、小さな変化は無視する。
/// 0.1 ≒ 7% の変化。
const ENGAGE_LOG2: f32 = 0.1;

/// 指の間隔の最小値 (0 除算を避ける)。単位は touchpad.rs で揃えた物理座標。
const MIN_DISTANCE: f32 = 1e-3;

/// タッチパッド上の 1 本の指
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// 指の識別番号 (指を離すまで同じ値)
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// HID レポート内の 1 つの指の枠 (まだ触れているかの判定前)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// Tip Switch: 指が触れていれば true (離した瞬間の最後の報告は false)
    pub touching: bool,
    pub contact: Contact,
}

/// HID レポートを 1 フレーム (その瞬間に触れている全部の指) にまとめる。
///
/// プレシジョンタッチパッドは 1 つのレポートに全部の指を載せる「パラレルモード」のほか、
/// 5 本の指を 2 本 + 2 本 + 1 本 のように複数レポートに分けて送る「ハイブリッドモード」がある。
/// ハイブリッドモードでは、最初のレポートの Contact Count にフレーム全体の指の数が入り、
/// 続くレポートの Contact Count は 0 になる。
#[derive(Debug, Default)]
pub struct FrameAssembler {
    expected: usize,
    slots: Vec<Slot>,
}

impl FrameAssembler {
    /// レポート 1 つ分を受け取り、フレームが揃ったら触れている指の一覧を返す。
    ///
    /// - `contact_count`: レポートの Contact Count。デバイスに無ければ None
    /// - `slots`: レポート内の指の枠 (使われていない枠も含む)
    pub fn push(&mut self, contact_count: Option<u32>, slots: &[Slot]) -> Option<Vec<Contact>> {
        match contact_count {
            // Contact Count が無いデバイス: 1 レポート = 1 フレームとみなす
            None => {
                self.expected = 0;
                self.slots.clear();
                return Some(touching(slots));
            }
            // 新しいフレームの始まり
            Some(n) if n > 0 => {
                self.expected = n as usize;
                self.slots.clear();
            }
            // 続きのレポートだが、始まりを受け取っていない → 捨てる
            Some(_) if self.expected == 0 => return None,
            Some(_) => {}
        }
        let remaining = self.expected - self.slots.len();
        self.slots.extend(slots.iter().take(remaining));
        if self.slots.len() < self.expected {
            return None;
        }
        self.expected = 0;
        Some(touching(&std::mem::take(&mut self.slots)))
    }
}

fn touching(slots: &[Slot]) -> Vec<Contact> {
    slots
        .iter()
        .filter(|s| s.touching)
        .map(|s| s.contact)
        .collect()
}

/// 2 本指の間隔の変化からピンチを検出する。
#[derive(Debug, Default)]
pub struct PinchDetector {
    pair: Option<Pair>,
}

#[derive(Debug)]
struct Pair {
    ids: (u32, u32),
    /// 2 本指を置いたときの間隔
    start: f32,
    /// 前回ホイール量を出したときの間隔
    last: f32,
    /// ピンチと判定済みか
    engaged: bool,
}

impl PinchDetector {
    /// 1 フレーム分の指を受け取り、ピンチ中ならホイール換算の変化量を返す
    /// (正 = ピンチアウト = 拡大。WM_MOUSEWHEEL の delta と同じ向き)。
    pub fn update(&mut self, contacts: &[Contact]) -> Option<f32> {
        // ちょうど 2 本のときだけ扱う (3 本指以上はウィンドウ切り替えなど別のジェスチャー)
        let [a, b] = contacts else {
            self.pair = None;
            return None;
        };
        let ids = (a.id.min(b.id), a.id.max(b.id));
        let distance = (a.x - b.x).hypot(a.y - b.y).max(MIN_DISTANCE);

        let pair = match &mut self.pair {
            Some(p) if p.ids == ids => p,
            // 置いた直後、または指が入れ替わった: ここを基準にする
            _ => {
                self.pair = Some(Pair {
                    ids,
                    start: distance,
                    last: distance,
                    engaged: false,
                });
                return None;
            }
        };

        if !pair.engaged {
            if (distance / pair.start).log2().abs() < ENGAGE_LOG2 {
                return None;
            }
            // ピンチ開始。判定までの変化分は出さず、ここから追従する (急に拡大しないように)
            pair.engaged = true;
            pair.last = distance;
            return None;
        }

        let units = (distance / pair.last).log2() * UNITS_PER_DOUBLING;
        pair.last = distance;
        (units != 0.0).then_some(units)
    }

    /// いま 2 本指がタッチパッドに触れているか
    pub fn two_fingers_down(&self) -> bool {
        self.pair.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u32, x: f32, y: f32) -> Contact {
        Contact { id, x, y }
    }

    fn slot(touching: bool, id: u32) -> Slot {
        Slot {
            touching,
            contact: c(id, id as f32, 0.0),
        }
    }

    fn ids(contacts: &[Contact]) -> Vec<u32> {
        contacts.iter().map(|c| c.id).collect()
    }

    #[test]
    fn parallel_mode_is_one_frame_per_report() {
        let mut f = FrameAssembler::default();
        // 5 枠のうち 2 本だけ使われている。3 枠目以降は未使用
        let slots = [
            slot(true, 1),
            slot(true, 2),
            slot(false, 0),
            slot(false, 0),
            slot(false, 0),
        ];
        assert_eq!(ids(&f.push(Some(2), &slots).unwrap()), vec![1, 2]);
    }

    #[test]
    fn hybrid_mode_joins_reports() {
        let mut f = FrameAssembler::default();
        // 3 本の指を 2 枠ずつのレポートで送ってくる
        assert_eq!(f.push(Some(3), &[slot(true, 1), slot(true, 2)]), None);
        let frame = f.push(Some(0), &[slot(true, 3), slot(false, 9)]).unwrap();
        assert_eq!(ids(&frame), vec![1, 2, 3]);
        // 始まりの無い続きは捨てる
        assert_eq!(f.push(Some(0), &[slot(true, 1)]), None);
    }

    #[test]
    fn lifted_finger_is_dropped() {
        let mut f = FrameAssembler::default();
        let frame = f.push(Some(2), &[slot(true, 1), slot(false, 2)]).unwrap();
        assert_eq!(ids(&frame), vec![1]);
    }

    #[test]
    fn no_contact_count_uses_tip_switch() {
        let mut f = FrameAssembler::default();
        let frame = f.push(None, &[slot(true, 1), slot(false, 2)]).unwrap();
        assert_eq!(ids(&frame), vec![1]);
    }

    #[test]
    fn spreading_fingers_zooms_in_after_dead_zone() {
        let mut p = PinchDetector::default();
        assert_eq!(p.update(&[c(1, 0.0, 0.0), c(2, 10.0, 0.0)]), None);
        assert!(p.two_fingers_down());
        // 3% の揺れはスクロール中のぶれとして無視
        assert_eq!(p.update(&[c(1, 0.0, 0.0), c(2, 10.3, 0.0)]), None);
        // 10% 広がったらピンチ開始 (この時点ではまだ出さない)
        assert_eq!(p.update(&[c(1, 0.0, 0.0), c(2, 11.0, 0.0)]), None);
        // そこから 2 倍に広げると、ちょうど UNITS_PER_DOUBLING 分出る
        let units = p.update(&[c(1, 0.0, 0.0), c(2, 22.0, 0.0)]).unwrap();
        assert!((units - UNITS_PER_DOUBLING).abs() < 1e-3);
        // 半分に縮めれば同じ量だけ負
        let units = p.update(&[c(1, 0.0, 0.0), c(2, 11.0, 0.0)]).unwrap();
        assert!((units + UNITS_PER_DOUBLING).abs() < 1e-3);
    }

    #[test]
    fn parallel_movement_is_not_a_pinch() {
        let mut p = PinchDetector::default();
        // 2 本指スクロール: 間隔は変わらず両方動く
        for i in 0..20 {
            let y = i as f32 * 5.0;
            assert_eq!(p.update(&[c(1, 0.0, y), c(2, 10.0, y)]), None);
        }
    }

    #[test]
    fn other_finger_counts_reset() {
        let mut p = PinchDetector::default();
        p.update(&[c(1, 0.0, 0.0), c(2, 10.0, 0.0)]);
        p.update(&[c(1, 0.0, 0.0), c(2, 20.0, 0.0)]);
        // 1 本離した → リセット
        assert_eq!(p.update(&[c(1, 0.0, 0.0)]), None);
        assert!(!p.two_fingers_down());
        // 置き直したら新しい基準から (いきなり出さない)
        assert_eq!(p.update(&[c(1, 0.0, 0.0), c(3, 40.0, 0.0)]), None);
        // 3 本指はピンチとして扱わない
        assert_eq!(
            p.update(&[c(1, 0.0, 0.0), c(3, 80.0, 0.0), c(4, 1.0, 1.0)]),
            None
        );
    }
}
