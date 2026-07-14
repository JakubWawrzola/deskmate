//! Most schowka PC <-> HA (arboard; na Windows clipboard-win, zero C).
//! Odczyt = sensor privacy (opt-in). Zapis = encja text z HA.

/// Odczyt tekstu ze schowka (None gdy pusty/obraz/blad). Jeden retry, bo inny
/// proces moze chwilowo trzymac schowek otwarty.
pub fn get_text() -> Option<String> {
    for _ in 0..2 {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(t) = cb.get_text() {
                return Some(t);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    None
}

/// Ustawia tekst schowka PC (komenda z HA).
pub fn set_text(s: &str) -> Result<(), String> {
    let mut last = String::from("clipboard busy");
    for _ in 0..3 {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(s.to_string())) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(last)
}
