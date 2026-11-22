//! The cover of what is playing, for the Link media player. Windows hands
//! out the thumbnail of the media session in whatever size and format the
//! player chose; it goes to Home Assistant as a JPEG of at most 512 pixels a
//! side, once per track. Everything here blocks (WinRT calls), so callers
//! run it off the async runtime.

use deskmate_core::link::media::MediaArt;

/// Longest side of the picture that is sent, in pixels.
#[cfg(any(windows, test))]
const MAX_SIDE_PX: u32 = 512;
/// Largest thumbnail that is read at all. A player has no reason to hand out
/// more, and the limit keeps a broken one from filling the memory.
#[cfg(windows)]
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
/// How often the thumbnail of one track is looked at. Players set the title
/// first and the picture a moment later, so the first look may still find
/// the picture of the track before.
const LOOKS_PER_TRACK: u8 = 3;

/// What was sent last and for which track, so that a picture goes out once.
#[derive(Debug)]
pub struct ArtTracker {
    track: Option<String>,
    looks: u8,
    sent: Option<String>,
}

impl Default for ArtTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// What `ArtTracker::next` asks the caller to do.
#[derive(Debug, PartialEq)]
pub enum ArtStep {
    /// Nothing to send.
    Keep,
    /// Send this picture.
    Send(MediaArt),
    /// Tell Home Assistant that nothing has a picture now.
    Remove,
}

impl ArtTracker {
    pub const fn new() -> Self {
        Self {
            track: None,
            looks: 0,
            sent: None,
        }
    }

    /// Forget everything: a new session knows nothing of the last one.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Decides what to do for `track`, the identity of what plays now (None
    /// when nothing does, or when the user keeps the title to themselves).
    /// `read` fetches the picture and is called only while the track is new.
    pub fn next(
        &mut self,
        track: Option<&str>,
        read: impl FnOnce() -> Option<MediaArt>,
    ) -> ArtStep {
        if self.track.as_deref() != track {
            self.track = track.map(str::to_string);
            self.looks = 0;
        }
        if track.is_none() {
            return self.remove();
        }
        if self.looks >= LOOKS_PER_TRACK {
            return ArtStep::Keep;
        }
        self.looks += 1;
        match read() {
            Some(art) if self.sent.as_deref() == Some(art.hash.as_str()) => ArtStep::Keep,
            Some(art) => {
                self.sent = Some(art.hash.clone());
                ArtStep::Send(art)
            }
            // No picture after the last look: the one that is showing
            // belongs to another track.
            None if self.looks == LOOKS_PER_TRACK => self.remove(),
            None => ArtStep::Keep,
        }
    }

    fn remove(&mut self) -> ArtStep {
        if self.sent.take().is_some() {
            ArtStep::Remove
        } else {
            ArtStep::Keep
        }
    }
}

/// The size to scale a picture of `width` by `height` to, so that neither
/// side is longer than `MAX_SIDE_PX`. A smaller picture keeps its size.
#[cfg(any(windows, test))]
fn scaled(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let longest = width.max(height);
    if longest <= MAX_SIDE_PX {
        return Some((width, height));
    }
    let side = |pixels: u32| {
        let scaled = u64::from(pixels) * u64::from(MAX_SIDE_PX) / u64::from(longest);
        // A very flat picture must not end up without a side.
        scaled.max(1) as u32
    };
    Some((side(width), side(height)))
}

/// The thumbnail of the current media session as a JPEG, or None when the
/// player has none or it cannot be read.
#[cfg(windows)]
pub fn current() -> Option<MediaArt> {
    match read_current() {
        Ok(art) => art,
        Err(error) => {
            // The error of a WinRT call names no content, only the call.
            log::debug!("the media picture could not be read: {}", error.code());
            None
        }
    }
}

#[cfg(not(windows))]
pub fn current() -> Option<MediaArt> {
    None
}

#[cfg(windows)]
fn read_current() -> windows::core::Result<Option<MediaArt>> {
    use deskmate_core::link::media::{art_hash, MAX_MEDIA_ART_BYTES};
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapInterpolationMode, BitmapPixelFormat,
        BitmapTransform, ColorManagementMode, ExifOrientationMode,
    };
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};

    let session = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?
        .join()?
        .GetCurrentSession()?;
    let properties = session.TryGetMediaPropertiesAsync()?.join()?;
    // A session without a picture has no Thumbnail at all.
    let Ok(thumbnail) = properties.Thumbnail() else {
        return Ok(None);
    };
    let source = thumbnail.OpenReadAsync()?.join()?;
    if !(1..=MAX_SOURCE_BYTES).contains(&source.Size()?) {
        return Ok(None);
    }
    let decoder = BitmapDecoder::CreateAsync(&source)?.join()?;
    let Some((width, height)) = scaled(decoder.PixelWidth()?, decoder.PixelHeight()?) else {
        return Ok(None);
    };
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(width)?;
    transform.SetScaledHeight(height)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    // JPEG has no transparency, so the alpha channel is dropped here.
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Ignore,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;

    let output = InMemoryRandomAccessStream::new()?;
    let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::JpegEncoderId()?, &output)?.join()?;
    encoder.SetPixelData(
        BitmapPixelFormat::Bgra8,
        BitmapAlphaMode::Ignore,
        width,
        height,
        96.0,
        96.0,
        &pixels,
    )?;
    encoder.FlushAsync()?.join()?;

    let size = output.Size()?;
    if !(1..=MAX_MEDIA_ART_BYTES as u64).contains(&size) {
        return Ok(None);
    }
    let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size as u32)?.join()?;
    let mut jpeg = vec![0u8; size as usize];
    reader.ReadBytes(&mut jpeg)?;
    Ok(Some(MediaArt {
        hash: art_hash(&jpeg),
        jpeg,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskmate_core::link::media::art_hash;

    fn art(byte: u8) -> MediaArt {
        let jpeg = vec![0xff, 0xd8, 0xff, byte];
        MediaArt {
            hash: art_hash(&jpeg),
            jpeg,
        }
    }

    #[test]
    fn a_large_picture_is_scaled_and_a_small_one_is_kept() {
        assert_eq!(scaled(1024, 1024), Some((512, 512)));
        assert_eq!(scaled(2000, 1000), Some((512, 256)));
        assert_eq!(scaled(600, 1200), Some((256, 512)));
        assert_eq!(scaled(300, 200), Some((300, 200)));
        assert_eq!(scaled(512, 512), Some((512, 512)));
        assert_eq!(scaled(10_000, 1), Some((512, 1)));
        assert_eq!(scaled(0, 100), None);
        assert_eq!(scaled(100, 0), None);
    }

    #[test]
    fn a_picture_goes_out_once_per_track() {
        let mut tracker = ArtTracker::default();
        assert_eq!(
            tracker.next(Some("a"), || Some(art(1))),
            ArtStep::Send(art(1))
        );
        // The same picture on the later looks, then no more looks at all.
        assert_eq!(tracker.next(Some("a"), || Some(art(1))), ArtStep::Keep);
        assert_eq!(tracker.next(Some("a"), || Some(art(1))), ArtStep::Keep);
        assert_eq!(
            tracker.next(Some("a"), || panic!("looked a fourth time")),
            ArtStep::Keep
        );
    }

    #[test]
    fn a_picture_that_arrives_after_the_title_is_still_sent() {
        let mut tracker = ArtTracker::default();
        assert_eq!(
            tracker.next(Some("a"), || Some(art(1))),
            ArtStep::Send(art(1))
        );
        // The next track: the player still shows the old picture at first.
        assert_eq!(tracker.next(Some("b"), || Some(art(1))), ArtStep::Keep);
        assert_eq!(
            tracker.next(Some("b"), || Some(art(2))),
            ArtStep::Send(art(2))
        );
    }

    #[test]
    fn a_track_without_a_picture_takes_the_old_one_away() {
        let mut tracker = ArtTracker::default();
        tracker.next(Some("a"), || Some(art(1)));
        assert_eq!(tracker.next(Some("b"), || None), ArtStep::Keep);
        assert_eq!(tracker.next(Some("b"), || None), ArtStep::Keep);
        assert_eq!(tracker.next(Some("b"), || None), ArtStep::Remove);
        assert_eq!(
            tracker.next(Some("b"), || panic!("looked again")),
            ArtStep::Keep
        );
    }

    #[test]
    fn nothing_playing_removes_the_picture_once() {
        let mut tracker = ArtTracker::default();
        assert_eq!(tracker.next(None, || panic!("looked")), ArtStep::Keep);
        tracker.next(Some("a"), || Some(art(1)));
        assert_eq!(tracker.next(None, || panic!("looked")), ArtStep::Remove);
        assert_eq!(tracker.next(None, || panic!("looked")), ArtStep::Keep);
    }

    #[test]
    fn a_new_session_gets_the_picture_again() {
        let mut tracker = ArtTracker::default();
        tracker.next(Some("a"), || Some(art(1)));
        tracker.reset();
        assert_eq!(
            tracker.next(Some("a"), || Some(art(1))),
            ArtStep::Send(art(1))
        );
    }
}
