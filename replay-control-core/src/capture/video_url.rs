use serde::{Deserialize, Serialize};
use url::Url;

/// Supported video platforms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoPlatform {
    YouTube,
    Twitch,
    Vimeo,
    Dailymotion,
}

impl VideoPlatform {
    pub fn as_str(&self) -> &'static str {
        match self {
            VideoPlatform::YouTube => "youtube",
            VideoPlatform::Twitch => "twitch",
            VideoPlatform::Vimeo => "vimeo",
            VideoPlatform::Dailymotion => "dailymotion",
        }
    }
}

impl std::fmt::Display for VideoPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of parsing a video URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedVideo {
    pub platform: VideoPlatform,
    pub video_id: String,
    pub canonical_url: String,
    pub embed_url: String,
}

/// Parse a video URL into its components, stripping tracking parameters
/// and computing canonical + embed URLs.
///
/// Returns an error for unrecognized or invalid URLs.
pub fn parse_video_url(raw_url: &str) -> Result<ParsedVideo, String> {
    let raw_url = raw_url.trim();

    // Try to parse as URL; if missing scheme, prepend https://
    let url = Url::parse(raw_url)
        .or_else(|_| Url::parse(&format!("https://{raw_url}")))
        .map_err(|e| format!("Invalid URL: {e}"))?;

    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?
        .to_lowercase();

    // YouTube
    if host == "youtube.com"
        || host == "www.youtube.com"
        || host == "m.youtube.com"
        || host == "youtu.be"
        || host == "www.youtube-nocookie.com"
    {
        return parse_youtube(&url, &host);
    }

    // Twitch
    if host == "twitch.tv"
        || host == "www.twitch.tv"
        || host == "clips.twitch.tv"
        || host == "player.twitch.tv"
    {
        return parse_twitch(&url, &host);
    }

    // Vimeo
    if host == "vimeo.com" || host == "www.vimeo.com" || host == "player.vimeo.com" {
        return parse_vimeo(&url);
    }

    // Dailymotion
    if host == "dailymotion.com" || host == "www.dailymotion.com" || host == "dai.ly" {
        return parse_dailymotion(&url, &host);
    }

    Err(format!(
        "Unsupported video platform: {host}. Supported: YouTube, Twitch, Vimeo, Dailymotion."
    ))
}

fn parse_youtube(url: &Url, host: &str) -> Result<ParsedVideo, String> {
    let video_id = if host == "youtu.be" {
        // youtu.be/{VIDEO_ID}
        url.path()
            .trim_start_matches('/')
            .split('/')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        let path = url.path();
        if path.starts_with("/watch") {
            // youtube.com/watch?v={VIDEO_ID}
            url.query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default()
        } else if let Some(rest) = path.strip_prefix("/embed/") {
            // youtube.com/embed/{VIDEO_ID}
            rest.split('/').next().unwrap_or("").to_string()
        } else if let Some(rest) = path.strip_prefix("/shorts/") {
            // youtube.com/shorts/{VIDEO_ID}
            rest.split('/').next().unwrap_or("").to_string()
        } else if let Some(rest) = path.strip_prefix("/v/") {
            // youtube.com/v/{VIDEO_ID}
            rest.split('/').next().unwrap_or("").to_string()
        } else {
            String::new()
        }
    };

    if video_id.is_empty() || video_id.len() != 11 {
        return Err(format!(
            "Could not extract YouTube video ID from URL (got '{video_id}')"
        ));
    }

    Ok(ParsedVideo {
        platform: VideoPlatform::YouTube,
        canonical_url: format!("https://www.youtube.com/watch?v={video_id}"),
        embed_url: format!("https://www.youtube-nocookie.com/embed/{video_id}"),
        video_id,
    })
}

fn parse_twitch(url: &Url, host: &str) -> Result<ParsedVideo, String> {
    let path = url.path().trim_start_matches('/');

    if host == "clips.twitch.tv" {
        // clips.twitch.tv/{CLIP_SLUG}
        let clip_slug = path.split('/').next().unwrap_or("").to_string();
        if clip_slug.is_empty() {
            return Err("Could not extract Twitch clip slug from URL".to_string());
        }
        return Ok(ParsedVideo {
            platform: VideoPlatform::Twitch,
            canonical_url: format!("https://clips.twitch.tv/{clip_slug}"),
            embed_url: format!(
                "https://clips.twitch.tv/embed?clip={clip_slug}&parent={{{{host}}}}"
            ),
            video_id: clip_slug,
        });
    }

    // twitch.tv/videos/{VOD_ID}
    if let Some(rest) = path.strip_prefix("videos/") {
        let vod_id = rest.split('/').next().unwrap_or("").to_string();
        if vod_id.is_empty() {
            return Err("Could not extract Twitch VOD ID from URL".to_string());
        }
        return Ok(ParsedVideo {
            platform: VideoPlatform::Twitch,
            canonical_url: format!("https://www.twitch.tv/videos/{vod_id}"),
            embed_url: format!("https://player.twitch.tv/?video={vod_id}&parent={{{{host}}}}"),
            video_id: vod_id,
        });
    }

    // twitch.tv/{channel}/clip/{CLIP_SLUG}
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() >= 3 && segments[1] == "clip" {
        let clip_slug = segments[2].to_string();
        if !clip_slug.is_empty() {
            return Ok(ParsedVideo {
                platform: VideoPlatform::Twitch,
                canonical_url: format!("https://clips.twitch.tv/{clip_slug}"),
                embed_url: format!(
                    "https://clips.twitch.tv/embed?clip={clip_slug}&parent={{{{host}}}}"
                ),
                video_id: clip_slug,
            });
        }
    }

    Err(
        "Could not parse Twitch URL. Supported: twitch.tv/videos/ID, clips.twitch.tv/SLUG"
            .to_string(),
    )
}

fn parse_vimeo(url: &Url) -> Result<ParsedVideo, String> {
    let path = url.path().trim_start_matches('/');

    // player.vimeo.com/video/{ID} or vimeo.com/{ID}
    let video_id = if let Some(rest) = path.strip_prefix("video/") {
        rest.split('/').next().unwrap_or("")
    } else {
        path.split('/').next().unwrap_or("")
    };

    if video_id.is_empty() || !video_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Could not extract Vimeo video ID from URL (got '{video_id}')"
        ));
    }

    Ok(ParsedVideo {
        platform: VideoPlatform::Vimeo,
        video_id: video_id.to_string(),
        canonical_url: format!("https://vimeo.com/{video_id}"),
        embed_url: format!("https://player.vimeo.com/video/{video_id}"),
    })
}

fn parse_dailymotion(url: &Url, host: &str) -> Result<ParsedVideo, String> {
    let path = url.path().trim_start_matches('/');

    let video_id = if host == "dai.ly" {
        // dai.ly/{ID}
        path.split('/').next().unwrap_or("").to_string()
    } else if let Some(rest) = path.strip_prefix("video/") {
        // dailymotion.com/video/{ID}
        // ID might have a slug suffix after underscore: x8abc_slug → x8abc
        let raw = rest.split('/').next().unwrap_or("");
        raw.split('_').next().unwrap_or(raw).to_string()
    } else if let Some(rest) = path.strip_prefix("embed/video/") {
        rest.split('/').next().unwrap_or("").to_string()
    } else {
        String::new()
    };

    if video_id.is_empty() {
        return Err("Could not extract Dailymotion video ID from URL".to_string());
    }

    Ok(ParsedVideo {
        platform: VideoPlatform::Dailymotion,
        video_id: video_id.clone(),
        canonical_url: format!("https://www.dailymotion.com/video/{video_id}"),
        embed_url: format!("https://www.dailymotion.com/embed/video/{video_id}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_watch() {
        let v = parse_video_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=abc123").unwrap();
        assert_eq!(v.platform, VideoPlatform::YouTube);
        assert_eq!(v.video_id, "dQw4w9WgXcQ");
        assert_eq!(
            v.canonical_url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(
            v.embed_url,
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"
        );
    }

    #[test]
    fn youtube_short_url() {
        let v = parse_video_url("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(v.video_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn youtube_shorts() {
        let v = parse_video_url("https://www.youtube.com/shorts/dQw4w9WgXcQ").unwrap();
        assert_eq!(v.video_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn youtube_embed() {
        let v = parse_video_url("https://www.youtube.com/embed/dQw4w9WgXcQ").unwrap();
        assert_eq!(v.video_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn youtube_mobile() {
        let v = parse_video_url("https://m.youtube.com/watch?v=dQw4w9WgXcQ&feature=share").unwrap();
        assert_eq!(v.video_id, "dQw4w9WgXcQ");
    }

    #[test]
    fn twitch_vod() {
        let v = parse_video_url("https://www.twitch.tv/videos/123456789").unwrap();
        assert_eq!(v.platform, VideoPlatform::Twitch);
        assert_eq!(v.video_id, "123456789");
    }

    #[test]
    fn twitch_clip() {
        let v = parse_video_url("https://clips.twitch.tv/SomeClipSlug").unwrap();
        assert_eq!(v.platform, VideoPlatform::Twitch);
        assert_eq!(v.video_id, "SomeClipSlug");
    }

    #[test]
    fn vimeo() {
        let v = parse_video_url("https://vimeo.com/76979871").unwrap();
        assert_eq!(v.platform, VideoPlatform::Vimeo);
        assert_eq!(v.video_id, "76979871");
        assert_eq!(v.embed_url, "https://player.vimeo.com/video/76979871");
    }

    #[test]
    fn dailymotion() {
        let v = parse_video_url("https://www.dailymotion.com/video/x5e9eog_slug").unwrap();
        assert_eq!(v.platform, VideoPlatform::Dailymotion);
        assert_eq!(v.video_id, "x5e9eog");
    }

    #[test]
    fn unsupported_url() {
        assert!(parse_video_url("https://www.example.com/video/123").is_err());
    }
}
