//! ピンチ量の蓄積とクールダウン判定。
//!
//! このモジュールは Win32 API に一切依存しない純粋なロジックなので、
//! Linux / macOS 上でも `cargo test` でテストできる。
//!
//! # なぜ蓄積が必要か
//! プレシジョンタッチパッドのピンチは、物理マウスのホイール 1 ノッチ (delta = 120) ではなく、
//! delta = 10〜40 程度の細かい Ctrl+ホイールイベントとして毎秒数十回届く。
//! これを 1 イベント = 1 段ズームにすると、1 回のピンチで一気に何段も拡大されてしまう。
//! そこで delta を合計し、閾値を超えたら 1 段だけズームし、その後しばらく (クールダウン) は
//! 入力を捨てる。

use std::time::{Duration, Instant};

/// ズームの方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
}

/// 判定に使うパラメータ (config.toml から作られる)。
#[derive(Debug, Clone, Copy)]
pub struct PinchSettings {
    /// |蓄積 delta| がこの値以上になったら 1 段ズームする。
    pub threshold: i32,
    /// 1 段ズームした後、入力を無視する時間。
    pub cooldown: Duration,
    /// 前回のイベントからこの時間以上空いたら「別のピンチ」とみなし、蓄積をリセットする。
    pub gesture_gap: Duration,
    /// true なら方向を反転する (ピンチアウトで縮小)。
    pub invert: bool,
}

/// ピンチの状態。フックのコールバックから 1 イベントごとに `feed` される。
#[derive(Debug, Default)]
pub struct PinchTracker {
    accumulated: i32,
    last_event: Option<Instant>,
    cooldown_until: Option<Instant>,
}

impl PinchTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// ホイール delta を 1 つ受け取り、ズームすべきなら方向を返す。
    ///
    /// `delta` は WM_MOUSEWHEEL の値そのもの (正 = ホイール上 = ピンチアウト)。
    /// `now` を引数で受け取るのはテストで時刻を自由に進められるようにするため。
    pub fn feed(&mut self, delta: i32, now: Instant, s: &PinchSettings) -> Option<ZoomDirection> {
        // 前のイベントから間が空いていたら、新しいピンチの始まりとして蓄積を捨てる。
        if let Some(last) = self.last_event
            && now.saturating_duration_since(last) >= s.gesture_gap
        {
            self.accumulated = 0;
        }
        self.last_event = Some(now);

        // クールダウン中は捨てる (握りつぶすかどうかは呼び出し側が決める)。
        if let Some(until) = self.cooldown_until {
            if now < until {
                self.accumulated = 0;
                return None;
            }
            self.cooldown_until = None;
        }

        // 途中で向きが変わったら、それまでの蓄積は反対方向なので捨てる。
        if delta != 0 && self.accumulated != 0 && delta.signum() != self.accumulated.signum() {
            self.accumulated = 0;
        }
        self.accumulated = self.accumulated.saturating_add(delta);

        if self.accumulated.abs() < s.threshold.max(1) {
            return None;
        }

        let zoom_in = (self.accumulated > 0) != s.invert;
        self.accumulated = 0;
        self.cooldown_until = Some(now + s.cooldown);
        Some(if zoom_in {
            ZoomDirection::In
        } else {
            ZoomDirection::Out
        })
    }

    /// 有効/無効の切り替えや設定の再読み込み時に状態を捨てる。
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PinchSettings {
        PinchSettings {
            threshold: 100,
            cooldown: Duration::from_millis(300),
            gesture_gap: Duration::from_millis(500),
            invert: false,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn accumulates_until_threshold() {
        let s = settings();
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(40, t0, &s), None);
        assert_eq!(p.feed(40, t0 + ms(10), &s), None);
        assert_eq!(p.feed(40, t0 + ms(20), &s), Some(ZoomDirection::In));
    }

    #[test]
    fn negative_delta_zooms_out() {
        let s = settings();
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(-120, t0, &s), Some(ZoomDirection::Out));
    }

    #[test]
    fn cooldown_swallows_following_events() {
        let s = settings();
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(120, t0, &s), Some(ZoomDirection::In));
        // クールダウン中はどれだけ来ても反応しない
        for i in 1..20 {
            assert_eq!(p.feed(120, t0 + ms(i * 10), &s), None);
        }
        // クールダウン明けに閾値を超えれば再び反応する
        assert_eq!(p.feed(120, t0 + ms(310), &s), Some(ZoomDirection::In));
    }

    #[test]
    fn direction_change_resets_accumulation() {
        let s = settings();
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(90, t0, &s), None);
        assert_eq!(p.feed(-20, t0 + ms(10), &s), None);
        // 90 の蓄積は捨てられているので +20 では届かない
        assert_eq!(p.feed(20, t0 + ms(20), &s), None);
    }

    #[test]
    fn gap_between_gestures_resets_accumulation() {
        let s = settings();
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(90, t0, &s), None);
        assert_eq!(p.feed(20, t0 + ms(600), &s), None);
    }

    #[test]
    fn invert_flips_direction() {
        let s = PinchSettings {
            invert: true,
            ..settings()
        };
        let t0 = Instant::now();
        let mut p = PinchTracker::new();
        assert_eq!(p.feed(120, t0, &s), Some(ZoomDirection::Out));
    }
}
