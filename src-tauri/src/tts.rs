//! TTS: the computer speaks text sent from HA (SAPI ISpVoice).
//! ISpVoice is not Send and requires a COM apartment, so it lives on its OWN thread,
//! which reads texts from the channel. Speak with flag 0 (sync) blocks the thread until
//! the utterance finishes -> a natural queue without losing the buffer.

#[cfg(windows)]
pub fn spawn() -> std::sync::mpsc::SyncSender<String> {
    // An MQTT publisher must not be able to build an unbounded queue while a
    // long message is being spoken.
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(16);
    std::thread::spawn(move || unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::Media::Speech::{ISpVoice, SpVoice};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let voice: ISpVoice = match CoCreateInstance(&SpVoice, None, CLSCTX_ALL) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("TTS init failed: {e}");
                return;
            }
        };
        while let Ok(text) = rx.recv() {
            if text.trim().is_empty() {
                continue;
            }
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            // flag 0 = SVSFDefault (synchronous); the 'wide' buffer stays alive for the whole Speak call
            let _ = voice.Speak(PCWSTR(wide.as_ptr()), 0, None);
        }
    });
    tx
}

#[cfg(not(windows))]
pub fn spawn() -> std::sync::mpsc::SyncSender<String> {
    let (tx, _rx) = std::sync::mpsc::sync_channel::<String>(16);
    tx
}
