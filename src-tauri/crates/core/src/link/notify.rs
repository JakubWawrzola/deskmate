//! `tag`, `priority` and `image_placement` on a `notify` frame and the
//! `notify_clear` frame (docs/LINK.md, "Notification tags"). They are values
//! from Home Assistant that end up in the notification system and the app's
//! window, so they are checked against a strict pattern first.

use serde::Serialize;
use serde_json::Value;

/// Longest tag. Windows accepts 64 characters for a toast tag.
pub const MAX_TAG_CHARS: usize = 64;

/// How insistently a notification asks for attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyPriority {
    #[default]
    Normal,
    High,
    Urgent,
}

impl NotifyPriority {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "normal" => Some(Self::Normal),
            "high" => Some(Self::High),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}

/// Where the picture of a notification is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImagePlacement {
    /// Small, next to the text.
    #[default]
    Logo,
    /// Across the top of the notification.
    Hero,
    /// Under the text, at full width.
    Inline,
}

impl ImagePlacement {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "logo" => Some(Self::Logo),
            "hero" => Some(Self::Hero),
            "inline" => Some(Self::Inline),
            _ => None,
        }
    }
}

/// The optional fields of a `notify` frame: the two that the `notify_tag`
/// feature adds, and the placement of the picture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotifyExtras {
    /// A notification with a tag replaces the earlier one with the same tag.
    pub tag: Option<String>,
    pub priority: NotifyPriority,
    pub image_placement: ImagePlacement,
}

/// Why a frame was refused. The message goes back to Home Assistant in the
/// `ack` and never repeats the value received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyFieldError {
    Tag,
    Priority,
    ImagePlacement,
}

impl NotifyFieldError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Tag => "invalid notification tag",
            Self::Priority => "invalid notification priority",
            Self::ImagePlacement => "invalid notification image placement",
        }
    }
}

/// 1 to 64 ASCII letters, digits, underscores or hyphens.
pub fn is_notify_tag(text: &str) -> bool {
    (1..=MAX_TAG_CHARS).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Reads `tag`, `priority` and `image_placement` of a `notify` frame. A
/// missing field, or one that is null, takes its default: no tag, normal
/// priority and the small picture. Any other value that is not in the
/// expected form refuses the whole notification.
pub fn parse_notify_extras(frame: &Value) -> Result<NotifyExtras, NotifyFieldError> {
    let tag = match frame.get("tag") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|tag| is_notify_tag(tag))
                .ok_or(NotifyFieldError::Tag)?
                .to_string(),
        ),
    };
    let priority = match frame.get("priority") {
        None | Some(Value::Null) => NotifyPriority::Normal,
        Some(value) => value
            .as_str()
            .and_then(NotifyPriority::parse)
            .ok_or(NotifyFieldError::Priority)?,
    };
    let image_placement = match frame.get("image_placement") {
        None | Some(Value::Null) => ImagePlacement::Logo,
        Some(value) => value
            .as_str()
            .and_then(ImagePlacement::parse)
            .ok_or(NotifyFieldError::ImagePlacement)?,
    };
    Ok(NotifyExtras {
        tag,
        priority,
        image_placement,
    })
}

/// The tag of a `notify_clear` frame, which must carry one.
pub fn parse_notify_clear(frame: &Value) -> Result<String, NotifyFieldError> {
    frame
        .get("tag")
        .and_then(Value::as_str)
        .filter(|tag| is_notify_tag(tag))
        .map(str::to_string)
        .ok_or(NotifyFieldError::Tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_notify_without_the_new_fields_is_what_it_was() {
        let frame = json!({"t": "notify", "id": 1, "title": "Door", "message": "Open"});
        assert_eq!(parse_notify_extras(&frame), Ok(NotifyExtras::default()));
        let nulls = json!({"t": "notify", "tag": null, "priority": null});
        assert_eq!(parse_notify_extras(&nulls), Ok(NotifyExtras::default()));
    }

    #[test]
    fn reads_the_image_placement() {
        for (text, placement) in [
            ("logo", ImagePlacement::Logo),
            ("hero", ImagePlacement::Hero),
            ("inline", ImagePlacement::Inline),
        ] {
            let frame = json!({"image": "https://ha.example/a.jpg", "image_placement": text});
            assert_eq!(
                parse_notify_extras(&frame).unwrap().image_placement,
                placement
            );
        }
    }

    #[test]
    fn an_unknown_image_placement_refuses_the_notification() {
        for bad in [
            json!("Hero"),
            json!("banner"),
            json!(""),
            json!(1),
            json!(["hero"]),
        ] {
            let frame = json!({"image_placement": bad});
            assert_eq!(
                parse_notify_extras(&frame),
                Err(NotifyFieldError::ImagePlacement)
            );
        }
        assert_eq!(
            NotifyFieldError::ImagePlacement.message(),
            "invalid notification image placement"
        );
    }

    #[test]
    fn reads_tag_and_priority() {
        let long = "a".repeat(64);
        for tag in ["door", "Door_1", "washer-done", "0", long.as_str()] {
            let frame = json!({"tag": tag, "priority": "high"});
            assert_eq!(
                parse_notify_extras(&frame),
                Ok(NotifyExtras {
                    tag: Some(tag.to_string()),
                    priority: NotifyPriority::High,
                    ..NotifyExtras::default()
                })
            );
        }
        for (text, priority) in [
            ("normal", NotifyPriority::Normal),
            ("high", NotifyPriority::High),
            ("urgent", NotifyPriority::Urgent),
        ] {
            let frame = json!({"priority": text});
            assert_eq!(
                parse_notify_extras(&frame).map(|e| e.priority),
                Ok(priority)
            );
        }
    }

    #[test]
    fn refuses_a_bad_tag() {
        let bad = [
            json!(""),
            json!("a".repeat(65)),
            json!("two words"),
            json!("dot.ted"),
            json!("slash/ed"),
            json!("caf\u{e9}"),
            json!("\u{0660}"),
            json!("tag\n"),
            json!("<b>"),
            json!(7),
            json!(true),
            json!(["door"]),
            json!({"tag": "door"}),
        ];
        for tag in bad {
            let frame = json!({"tag": tag});
            assert_eq!(
                parse_notify_extras(&frame),
                Err(NotifyFieldError::Tag),
                "{tag}"
            );
            let clear = json!({"t": "notify_clear", "id": 3, "tag": tag});
            assert_eq!(
                parse_notify_clear(&clear),
                Err(NotifyFieldError::Tag),
                "{tag}"
            );
        }
    }

    #[test]
    fn refuses_a_bad_priority() {
        for priority in [
            json!(""),
            json!("High"),
            json!("low"),
            json!("urgent "),
            json!(2),
            json!(["high"]),
        ] {
            let frame = json!({"tag": "door", "priority": priority});
            assert_eq!(
                parse_notify_extras(&frame),
                Err(NotifyFieldError::Priority),
                "{priority}"
            );
        }
    }

    #[test]
    fn notify_clear_needs_a_tag() {
        let frame = json!({"t": "notify_clear", "id": 3, "tag": "door"});
        assert_eq!(parse_notify_clear(&frame), Ok("door".to_string()));
        assert_eq!(
            parse_notify_clear(&json!({"t": "notify_clear", "id": 3})),
            Err(NotifyFieldError::Tag)
        );
        assert_eq!(
            parse_notify_clear(&json!({"t": "notify_clear", "tag": null})),
            Err(NotifyFieldError::Tag)
        );
    }

    #[test]
    fn errors_do_not_repeat_the_value() {
        assert_eq!(NotifyFieldError::Tag.message(), "invalid notification tag");
        assert_eq!(
            NotifyFieldError::Priority.message(),
            "invalid notification priority"
        );
    }

    #[test]
    fn priority_serializes_in_lowercase() {
        assert_eq!(json!(NotifyPriority::Urgent), json!("urgent"));
        assert_eq!(json!(NotifyPriority::Normal), json!("normal"));
    }
}
