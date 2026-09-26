//! 画面全体の連続ズーム (拡大鏡のように「+ / -」の段階ではなく、ピンチ量に比例して滑らかに拡大) の計算。
//!
//! Win32 に依存しない純粋なロジックなので Linux でもテストできる。
//! 実際に画面を拡大する API 呼び出しは magnifier.rs。
//!
//! # 表示位置 (オフセット) の決め方
//! 全画面拡大では「画面のどの部分を拡大して映すか」を左上座標 (オフセット) で指定する。
//! 倍率 L、オフセット off のとき、画面上の点 x には元の画面の `off + x / L` が映る。
//!
//! ここで `off = cursor * (1 - 1/L)` とすると、カーソル位置 x = cursor には
//! `cursor * (1 - 1/L) + cursor / L = cursor` が映る。つまり
//! **カーソルの真下には、拡大前と同じ位置のものが常に表示される**。
//! - ピンチした場所を中心に拡大される
//! - クリックは (拡大していても) 見えているものにそのまま当たる
//!   (拡大 API はマウス入力の座標を変換しないので、これが無いとクリック位置がずれる)
//! - カーソルを画面端に動かせば、拡大した画面の端まで自然にスクロールする

/// config.toml から作るパラメータ。
#[derive(Debug, Clone, Copy)]
pub struct ScreenZoomSettings {
    /// 最大倍率
    pub max_level: f32,
    /// 倍率を 2 倍にするのに必要なホイール量 (小さいほど速く拡大する)
    pub delta_per_doubling: f32,
    /// true なら向きを反転
    pub invert: bool,
}

#[derive(Debug)]
pub struct ScreenZoom {
    level: f32,
}

impl Default for ScreenZoom {
    fn default() -> Self {
        Self { level: 1.0 }
    }
}

/// 1.0 にこれ以上近づいたら拡大終了とみなす
const SNAP_TO_ONE: f32 = 1.02;

impl ScreenZoom {
    pub fn level(&self) -> f32 {
        self.level
    }

    /// 拡大中か (倍率 1.0 = 拡大していない)
    pub fn is_active(&self) -> bool {
        self.level > 1.0
    }

    /// ホイール量を倍率に反映する。倍率が変わったら true。
    ///
    /// 倍率は掛け算で変える (`level *= 2^(delta / delta_per_doubling)`)。
    /// 足し算だと、2 倍→3 倍 と 8 倍→9 倍 が同じピンチ量になり、高倍率ほど変化が鈍く感じる。
    pub fn apply_delta(&mut self, delta: i32, s: &ScreenZoomSettings) -> bool {
        let delta = if s.invert { -delta } else { delta } as f32;
        let factor = (delta / s.delta_per_doubling.max(1.0)).exp2();
        let mut next = (self.level * factor).clamp(1.0, s.max_level.max(1.0));
        if next < SNAP_TO_ONE {
            next = 1.0;
        }
        let changed = (next - self.level).abs() > f32::EPSILON;
        self.level = next;
        changed
    }

    pub fn reset(&mut self) {
        self.level = 1.0;
    }

    /// カーソル位置に対する表示オフセット (モジュール先頭のコメント参照)。
    /// `cursor` と `screen_size` は主モニターの座標系 (左上が 0,0)。
    pub fn offset(&self, cursor: (i32, i32), screen_size: (i32, i32)) -> (i32, i32) {
        let k = 1.0 - 1.0 / self.level;
        let axis = |c: i32, size: i32| (c.clamp(0, size.max(0)) as f32 * k).round() as i32;
        (axis(cursor.0, screen_size.0), axis(cursor.1, screen_size.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ScreenZoomSettings {
        ScreenZoomSettings {
            max_level: 8.0,
            delta_per_doubling: 400.0,
            invert: false,
        }
    }

    #[test]
    fn pinch_out_doubles_per_setting() {
        let s = settings();
        let mut z = ScreenZoom::default();
        assert!(z.apply_delta(400, &s));
        assert!((z.level() - 2.0).abs() < 1e-4);
        // 細かい delta に分けても同じ倍率になる
        let mut z2 = ScreenZoom::default();
        for _ in 0..20 {
            z2.apply_delta(20, &s);
        }
        assert!((z2.level() - 2.0).abs() < 1e-3);
    }

    #[test]
    fn clamps_and_snaps() {
        let s = settings();
        let mut z = ScreenZoom::default();
        z.apply_delta(100_000, &s);
        assert_eq!(z.level(), 8.0);
        z.apply_delta(-100_000, &s);
        assert_eq!(z.level(), 1.0);
        assert!(!z.is_active());
        // 1.0 のままピンチインしても変化なし
        assert!(!z.apply_delta(-100, &s));
        // 1.0 付近まで戻したら 1.0 に吸着する
        z.apply_delta(400, &s);
        z.apply_delta(-395, &s);
        assert_eq!(z.level(), 1.0);
    }

    #[test]
    fn invert_flips() {
        let s = ScreenZoomSettings {
            invert: true,
            ..settings()
        };
        let mut z = ScreenZoom::default();
        assert!(!z.apply_delta(400, &s));
        assert!(z.apply_delta(-400, &s));
        assert!(z.is_active());
    }

    #[test]
    fn offset_keeps_point_under_cursor() {
        let s = settings();
        let mut z = ScreenZoom::default();
        z.apply_delta(800, &s); // 4 倍
        let screen = (1920, 1080);
        for cursor in [(0, 0), (960, 540), (1920, 1080), (123, 987)] {
            let (ox, oy) = z.offset(cursor, screen);
            // 画面上のカーソル位置に映る元座標 = off + cursor / L がカーソル位置と一致する
            assert!((ox as f32 + cursor.0 as f32 / z.level() - cursor.0 as f32).abs() <= 1.0);
            assert!((oy as f32 + cursor.1 as f32 / z.level() - cursor.1 as f32).abs() <= 1.0);
            // 拡大範囲が画面外にはみ出さない
            assert!(ox >= 0 && ox as f32 + 1920.0 / z.level() <= 1920.0 + 1.0);
        }
    }
}
