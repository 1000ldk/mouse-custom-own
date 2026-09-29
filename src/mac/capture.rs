//! ScreenCaptureKit で 1 つのウィンドウを撮り続ける。
//!
//! 流れ:
//! 1. SCShareableContent で「撮影できるウィンドウの一覧」を取得し、番号が一致する SCWindow を探す
//!    (結果は別スレッドに届くので、メインキューに移してから続ける)
//! 2. そのウィンドウだけを撮る SCContentFilter と、解像度などの SCStreamConfiguration から SCStream を作る
//! 3. SCStreamOutput (このファイルで定義したクラス) を登録して撮影開始。
//!    フレームはメインキューに届けるよう指定するので、zoom.rs はメインスレッドで受け取れる
//!
//! 「デスクトップから独立したウィンドウ」として撮るので、前に別のウィンドウが重なっていても
//! 対象のウィンドウの中身だけが撮れる。

use std::ptr::NonNull;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_core_foundation::CGRect;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::CVPixelBufferGetIOSurface;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType, SCWindow,
};

use super::zoom;

/// 別スレッドからメインキューへ Objective-C のオブジェクトを渡すための入れ物。
/// (Rust の型としては Send ではないが、渡した先では 1 つのスレッドからしか触らない)
struct MainSend<T>(T);
unsafe impl<T> Send for MainSend<T> {}

define_class!(
    // SAFETY: NSObject を継承するだけで、特別な要件は無い。Drop も実装しない
    #[unsafe(super(NSObject))]
    #[name = "PinchZoomStreamOutput"]
    #[ivars = u64]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            // 変化が無いときのフレームなどには画像が付いていない
            // SAFETY: 受け取ったサンプルバッファから画像を取り出すだけ
            let Some(image) = (unsafe { sample_buffer.image_buffer() }) else {
                return;
            };
            let Some(surface) = CVPixelBufferGetIOSurface(Some(&image)) else {
                return;
            };
            // IOSurface は Objective-C のオブジェクトとしても扱える (toll-free bridge)
            let object = unsafe {
                &*(NonNull::from(&*surface).as_ptr() as *const objc2::runtime::AnyObject)
            };
            zoom::on_frame(*self.ivars(), object);
        }
    }
);

impl StreamOutput {
    fn new(session_id: u64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(session_id);
        // SAFETY: NSObject の init を呼ぶだけ
        unsafe { msg_send![super(this), init] }
    }
}

/// 撮影中のストリーム。Drop では止めないので、終わるときは `stop` を呼ぶ
pub struct Capture {
    stream: Retained<SCStream>,
    /// ストリームは出力先を弱く参照するだけなので、こちらで持っておく
    _output: Retained<StreamOutput>,
    /// 撮影する大きさを計算するための倍率 (ポイント → ピクセル)
    scale: f64,
}

/// ウィンドウ `window_id` の撮影を始める。結果は zoom::on_capture_ready / on_capture_failed に届く
pub fn start(session_id: u64, window_id: u32) {
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // ここは別スレッド
            // SAFETY: content / error はこのブロックの間だけ有効なポインタ (null もありうる)
            let result = match unsafe { content.as_ref() } {
                // 一覧を取得できない = 多くの場合、画面収録が許可されていない
                None => Err(unsafe { error.as_ref() }
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_default()),
                Some(content) => Ok(unsafe { content.windows() }
                    .iter()
                    .find(|w| unsafe { w.windowID() } == window_id)),
            };
            let result = MainSend(result);
            DispatchQueue::main().exec_async(move || {
                let result = result;
                match result.0 {
                    Ok(Some(window)) => on_window_found(session_id, &window),
                    Ok(None) => zoom::on_capture_failed(
                        session_id,
                        "撮影できるウィンドウが見つかりませんでした".into(),
                        false,
                    ),
                    Err(message) => zoom::on_capture_failed(session_id, message, true),
                }
            });
        },
    );
    // SAFETY: ハンドラーは呼び出しの間 (と、その後届くまで) OS が保持する
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        );
    }
}

/// メインスレッド: 見つかったウィンドウの撮影を始める
fn on_window_found(session_id: u64, window: &SCWindow) {
    // SAFETY: 取得した SCWindow からフィルタを作るだけ
    let filter = unsafe {
        SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), window)
    };
    // ポイント → ピクセルの倍率 (Retina なら 2)。macOS 14 より前には無いので確かめてから呼ぶ
    let scale = if filter.respondsToSelector(sel!(pointPixelScale)) {
        f64::from(unsafe { filter.pointPixelScale() }).max(1.0)
    } else {
        2.0
    };
    let frame = unsafe { window.frame() };
    let config = configuration(frame, scale);
    // SAFETY: 作ったフィルタと設定からストリームを作るだけ
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None)
    };
    let output = StreamOutput::new(session_id);
    // フレームはメインキューに届ける (zoom.rs はメインスレッドだけで状態を触る)
    let added = unsafe {
        stream.addStreamOutput_type_sampleHandlerQueue_error(
            ProtocolObject::from_ref(&*output),
            SCStreamOutputType::Screen,
            Some(DispatchQueue::main()),
        )
    };
    if let Err(e) = added {
        zoom::on_capture_failed(session_id, e.localizedDescription().to_string(), false);
        return;
    }
    let started = RcBlock::new(move |error: *mut NSError| {
        // 別スレッド。失敗したときだけ知らせる
        if let Some(e) = unsafe { error.as_ref() } {
            let message = e.localizedDescription().to_string();
            DispatchQueue::main()
                .exec_async(move || zoom::on_capture_failed(session_id, message, true));
        }
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&started)) };
    zoom::on_capture_ready(
        session_id,
        Capture {
            stream,
            _output: output,
            scale,
        },
    );
}

fn configuration(frame: CGRect, scale: f64) -> Retained<SCStreamConfiguration> {
    // SAFETY: 設定オブジェクトに値を入れるだけ
    unsafe {
        let config = SCStreamConfiguration::new();
        config.setWidth((frame.size.width * scale).round().max(1.0) as usize);
        config.setHeight((frame.size.height * scale).round().max(1.0) as usize);
        // 最大 60 fps
        config.setMinimumFrameInterval(CMTime::new(1, 60));
        config.setPixelFormat(u32::from_be_bytes(*b"BGRA"));
        // マウスカーソルは本物が上に表示されるので、映像には入れない
        config.setShowsCursor(false);
        config.setQueueDepth(5);
        // ウィンドウの影を入れない (入れると映像が影の分だけ大きくなり、位置がずれる)。macOS 14 以降
        if config.respondsToSelector(sel!(setIgnoreShadowsSingleWindow:)) {
            config.setIgnoreShadowsSingleWindow(true);
        }
        config
    }
}

impl Capture {
    /// ウィンドウの大きさが変わったとき
    pub fn resize(&self, frame: CGRect) {
        let config = configuration(frame, self.scale);
        unsafe {
            self.stream
                .updateConfiguration_completionHandler(&config, None)
        };
    }

    pub fn stop(self) {
        // 止まり終わるまでに届くフレームのために、ストリームと出力先は止まり終わるまで持っておく
        // (完了ハンドラーのブロックが持ち、ブロックが解放されるときに一緒に解放される)
        let Capture {
            stream, _output, ..
        } = self;
        let keep = (stream.clone(), _output);
        let done = RcBlock::new(move |_error: *mut NSError| {
            let _ = &keep;
        });
        unsafe { stream.stopCaptureWithCompletionHandler(Some(&done)) };
    }
}
