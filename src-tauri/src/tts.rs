//! TTS: komputer wypowiada tekst przyslany z HA (SAPI ISpVoice).
//! ISpVoice nie jest Send i wymaga COM apartamentu, wiec zyje na WLASNYM watku,
//! ktory czyta teksty z kanalu. Speak z flaga 0 (sync) blokuje watek do konca
//! wypowiedzi -> naturalna kolejka bez gubienia bufora.

#[cfg(windows)]
pub fn spawn() -> std::sync::mpsc::Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
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
            // flaga 0 = SVSFDefault (synchronicznie); bufor 'wide' zyje przez caly Speak
            let _ = voice.Speak(PCWSTR(wide.as_ptr()), 0, None);
        }
    });
    tx
}

#[cfg(not(windows))]
pub fn spawn() -> std::sync::mpsc::Sender<String> {
    let (tx, _rx) = std::sync::mpsc::channel::<String>();
    tx
}
