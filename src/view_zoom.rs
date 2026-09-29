//! ウィンドウ単位のズーム (Chrome のピンチズームのような動き) の計算。
//!
//! OS の API に依存しない純粋なロジックなので Linux でもテストできる。
//! 実際にウィンドウを拡大表示するのは Mac 版の mac/zoom.rs。
//!
//! # 座標
//! 座標はすべて「ウィンドウの左上を (0, 0) とした、ウィンドウ内の座標」(単位はポイント)。
//! 倍率 L で拡大しているとき、ウィンドウの範囲には元の内容の
//! `origin` から `origin + size / L` までが映っている。
//! 画面上でウィンドウ内の点 p に見えているのは、元の内容の `origin + p / L`。
//!
//! # Chrome のピンチとの対応
//! - ピンチした場所 (anchor) を中心に拡大する: 拡大の前後で anchor の下に映る内容が変わらないよう origin を動かす
//! - 2 本指スクロールで表示位置を動かす (pan)。端まで来たらそれ以上は動かない (呼び出し側がアプリに渡す)
//! - 表示範囲は常にウィンドウの中に収める

use crate::screen_zoom::{ScreenZoom, ScreenZoomSettings};

/// 少しでも動いたとみなす量 (ポイント)
const EPSILON: f64 = 1e-6;

#[derive(Debug)]
pub struct ViewZoom {
    /// 倍率 (滑らかに変える計算は画面ズームと共通)
    zoom: ScreenZoom,
    /// 表示している範囲の左上 (元の内容の座標)
    origin: (f64, f64),
    /// ウィンドウの大きさ
    size: (f64, f64),
}

impl ViewZoom {
    pub fn new(size: (f64, f64)) -> Self {
        Self {
            zoom: ScreenZoom::default(),
            origin: (0.0, 0.0),
            size: (size.0.max(1.0), size.1.max(1.0)),
        }
    }

    pub fn level(&self) -> f64 {
        self.zoom.level() as f64
    }

    /// 拡大中か (倍率 1.0 = 拡大していない)
    pub fn is_active(&self) -> bool {
        self.zoom.is_active()
    }

    #[cfg(test)]
    pub fn origin(&self) -> (f64, f64) {
        self.origin
    }

    pub fn size(&self) -> (f64, f64) {
        self.size
    }

    /// ピンチ量 (ホイール換算) を倍率に反映する。`anchor` の下に見えている内容は動かない。
    /// 倍率が変わったら true。
    pub fn zoom(&mut self, delta: f32, anchor: (f64, f64), s: &ScreenZoomSettings) -> bool {
        let anchor = self.clamp_point(anchor);
        let old = self.level();
        // anchor の下にいま映っている、元の内容の点
        let content = (
            self.origin.0 + anchor.0 / old,
            self.origin.1 + anchor.1 / old,
        );
        if !self.zoom.apply_delta(delta, s) {
            return false;
        }
        let new = self.level();
        // 拡大後も同じ点が anchor の下に来るように表示範囲を決める
        self.origin = (content.0 - anchor.0 / new, content.1 - anchor.1 / new);
        self.clamp_origin();
        true
    }

    /// 2 本指スクロールで表示位置を動かす。
    ///
    /// `delta` は画面上でのスクロール量 (正 = 内容が右 / 下に動く向き。Mac のスクロールイベントと同じ)。
    /// 画面上で指と同じ距離だけ内容が動くよう、倍率で割って元の内容の座標に直す。
    /// 少しでも動いたら true (false なら端に着いている → 呼び出し側はスクロールをアプリに渡す)。
    pub fn pan(&mut self, delta: (f64, f64)) -> bool {
        let level = self.level();
        let before = self.origin;
        self.origin.0 -= delta.0 / level;
        self.origin.1 -= delta.1 / level;
        self.clamp_origin();
        (self.origin.0 - before.0).abs() > EPSILON || (self.origin.1 - before.1).abs() > EPSILON
    }

    /// ウィンドウの大きさが変わったとき
    pub fn resize(&mut self, size: (f64, f64)) {
        self.size = (size.0.max(1.0), size.1.max(1.0));
        self.clamp_origin();
    }

    /// 画面上でウィンドウ内の点 `p` に見えている、元の内容の点 (= クリックを届けるべき本当の位置)
    pub fn to_content(&self, p: (f64, f64)) -> (f64, f64) {
        let p = self.clamp_point(p);
        let level = self.level();
        (self.origin.0 + p.0 / level, self.origin.1 + p.1 / level)
    }

    /// 拡大した内容を描く位置: ウィンドウ全体を倍率倍した画像の (左上 x, 左上 y, 幅, 高さ)
    pub fn content_frame(&self) -> (f64, f64, f64, f64) {
        let level = self.level();
        (
            -self.origin.0 * level,
            -self.origin.1 * level,
            self.size.0 * level,
            self.size.1 * level,
        )
    }

    fn clamp_point(&self, p: (f64, f64)) -> (f64, f64) {
        (p.0.clamp(0.0, self.size.0), p.1.clamp(0.0, self.size.1))
    }

    /// 表示範囲がウィンドウからはみ出さないようにする
    fn clamp_origin(&mut self) {
        let level = self.level();
        let max = (
            self.size.0 - self.size.0 / level,
            self.size.1 - self.size.1 / level,
        );
        self.origin = (
            self.origin.0.clamp(0.0, max.0.max(0.0)),
            self.origin.1.clamp(0.0, max.1.max(0.0)),
        );
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

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn zooms_around_anchor() {
        let s = settings();
        let mut v = ViewZoom::new((800.0, 600.0));
        let anchor = (200.0, 150.0);
        let before = v.to_content(anchor);
        assert!(v.zoom(400.0, anchor, &s)); // 2 倍
        assert!((v.level() - 2.0).abs() < 1e-4);
        // ピンチした場所の下に見えている内容は変わらない
        assert!(close(v.to_content(anchor), before));
        // さらに拡大しても同じ
        assert!(v.zoom(200.0, anchor, &s));
        assert!(close(v.to_content(anchor), before));
    }

    #[test]
    fn view_stays_inside_window() {
        let s = settings();
        let mut v = ViewZoom::new((800.0, 600.0));
        // 右下の角でピンチ → 右下の角を中心に拡大 (はみ出さない)
        v.zoom(800.0, (800.0, 600.0), &s); // 4 倍
        assert!(close(v.origin(), (600.0, 450.0)));
        assert!(close(v.to_content((800.0, 600.0)), (800.0, 600.0)));
        // ウィンドウの外の点は端に寄せる
        assert!(close(v.to_content((5000.0, -10.0)), (800.0, 450.0)));
        let (x, y, w, h) = v.content_frame();
        assert!(close((x, y), (-2400.0, -1800.0)));
        assert!(close((w, h), (3200.0, 2400.0)));
    }

    #[test]
    fn pan_moves_content_with_fingers_until_edge() {
        let s = settings();
        let mut v = ViewZoom::new((800.0, 600.0));
        v.zoom(400.0, (400.0, 300.0), &s); // 中央で 2 倍
        assert!(close(v.origin(), (200.0, 150.0)));
        // 指を上に 100 動かす (内容が上に動く = 下の方が見える)
        assert!(v.pan((0.0, -100.0)));
        assert!(close(v.origin(), (200.0, 200.0)));
        // 下端まで行ったら、それ以上は動かない
        assert!(v.pan((0.0, -10_000.0)));
        assert!(close(v.origin(), (200.0, 300.0)));
        assert!(!v.pan((0.0, -10.0)));
        // 横方向はまだ動ける
        assert!(v.pan((50.0, -10.0)));
        assert!(close(v.origin(), (175.0, 300.0)));
    }

    #[test]
    fn unzoomed_view_is_identity() {
        let v = ViewZoom::new((800.0, 600.0));
        assert!(!v.is_active());
        assert!(close(v.to_content((123.0, 456.0)), (123.0, 456.0)));
        let mut v = v;
        assert!(!v.pan((10.0, 10.0)));
    }

    #[test]
    fn zooming_back_to_one_resets_origin() {
        let s = settings();
        let mut v = ViewZoom::new((800.0, 600.0));
        v.zoom(400.0, (700.0, 500.0), &s);
        assert!(v.is_active());
        v.zoom(-400.0, (100.0, 100.0), &s);
        assert!(!v.is_active());
        assert!(close(v.origin(), (0.0, 0.0)));
    }

    #[test]
    fn resize_keeps_view_inside() {
        let s = settings();
        let mut v = ViewZoom::new((800.0, 600.0));
        v.zoom(400.0, (800.0, 600.0), &s);
        assert!(close(v.origin(), (400.0, 300.0)));
        v.resize((400.0, 300.0));
        assert!(close(v.origin(), (200.0, 150.0)));
        assert_eq!(v.size(), (400.0, 300.0));
    }
}
