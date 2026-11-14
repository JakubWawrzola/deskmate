//! The buttons of the update notification: "Install now" and "Later".
//!
//! They use a link of their own, `deskmate:update?do=...`, with single-use
//! tokens. A notification from Home Assistant always gets
//! `deskmate:action?...` for its buttons (notify.rs), so it cannot carry this
//! link, and a click on it is never sent to Home Assistant.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const LINK_TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_LINK_TOKENS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Choice {
    Install,
    Later,
}

impl Choice {
    fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Later => "later",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "install" => Some(Self::Install),
            "later" => Some(Self::Later),
            _ => None,
        }
    }
}

/// Tokens on the buttons of the update notifications this process showed.
/// Any program or web page can open a `deskmate:` link, so a click counts
/// only with one of these, for exactly the choice it was issued for.
static LINK_TOKENS: Mutex<Vec<(String, Choice, Instant)>> = Mutex::new(Vec::new());

fn link_tokens() -> MutexGuard<'static, Vec<(String, Choice, Instant)>> {
    LINK_TOKENS.lock().unwrap_or_else(|e| e.into_inner())
}

fn link_prefix() -> String {
    format!("{}:update?do=", crate::consts::PROTOCOL_SCHEME)
}

/// The link for one button, with a single-use token.
pub(super) fn issue_link(choice: Choice) -> String {
    use rand::RngCore;
    let mut raw = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let mut tokens = link_tokens();
    tokens.retain(|(_, _, issued)| issued.elapsed() < LINK_TOKEN_TTL);
    if tokens.len() >= MAX_LINK_TOKENS {
        tokens.remove(0);
    }
    tokens.push((token.clone(), choice, Instant::now()));
    format!("{}{}&t={token}", link_prefix(), choice.name())
}

/// "deskmate:update?do=install&t=ab12" -> (Install, "ab12").
fn parse_link(link: &str) -> Option<(Choice, &str)> {
    let rest = link.trim().strip_prefix(link_prefix().as_str())?;
    let (choice, token) = rest.split_once("&t=")?;
    Some((Choice::parse(choice)?, token))
}

/// Accepts a click only with a token this process put on a notification, for
/// the same choice, once.
pub(super) fn redeem_link(link: &str) -> Option<Choice> {
    let (choice, token) = parse_link(link)?;
    let mut tokens = link_tokens();
    let position = tokens
        .iter()
        .position(|(issued_token, issued_for, issued)| {
            issued_token == token && *issued_for == choice && issued.elapsed() < LINK_TOKEN_TTL
        })?;
    tokens.remove(position);
    Some(choice)
}

/// Whether `arg` is meant as an update link at all, valid or not.
pub(super) fn is_link(arg: &str) -> bool {
    arg.trim().starts_with(link_prefix().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_choice_and_the_token_from_a_link() {
        assert_eq!(
            parse_link("deskmate:update?do=install&t=ab12"),
            Some((Choice::Install, "ab12"))
        );
        assert_eq!(
            parse_link(" deskmate:update?do=later&t=cd34 "),
            Some((Choice::Later, "cd34"))
        );
        for refused in [
            "deskmate:update?do=install",
            "deskmate:update?do=uninstall&t=ab12",
            "deskmate:action?name=install&t=ab12",
            "deskmate:update?t=ab12&do=install",
            "https://example.com/deskmate:update?do=install&t=ab12",
            "",
        ] {
            assert_eq!(parse_link(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_button_works_once_and_only_for_its_own_choice() {
        let install = issue_link(Choice::Install);
        let later = issue_link(Choice::Later);
        let (_, install_token) = parse_link(&install).unwrap();

        // The token of "Later" does not install, and neither does a guess.
        let (_, later_token) = parse_link(&later).unwrap();
        let swapped = format!("deskmate:update?do=install&t={later_token}");
        assert_eq!(redeem_link(&swapped), None);
        assert_eq!(redeem_link("deskmate:update?do=install&t=0000"), None);
        assert_eq!(redeem_link("deskmate:update?do=install&t="), None);

        assert_eq!(redeem_link(&install), Some(Choice::Install));
        assert_eq!(redeem_link(&install), None);
        assert_eq!(install_token.len(), 32);
        assert_eq!(redeem_link(&later), Some(Choice::Later));
        assert_eq!(redeem_link(&later), None);
    }

    #[test]
    fn a_notification_from_home_assistant_cannot_carry_an_update_link() {
        // Its buttons are redeemed through notify.rs, which only knows
        // `deskmate:action?name=...`; that form is not an update link.
        let action = "deskmate:action?name=update%3Fdo%3Dinstall&t=ab12";
        assert_eq!(parse_link(action), None);
        assert!(crate::notify::is_action_url(action));
        assert!(!crate::notify::is_action_url(&issue_link(Choice::Install)));
    }
}
