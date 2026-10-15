#[cfg(test)]
mod tests {
    use crate::media::*;

    #[test]
    fn test_media_1_simple_src() {
        assert_eq!(find_media_source("img", &[("src".to_string(), "a.jpg".to_string())], ""), MediaSource::DirectDownload("a.jpg".to_string()));
    }

    #[test]
    fn test_media_2_srcset_overrides_src() {
        assert_eq!(find_media_source("img", &[("src".to_string(), "a.jpg".to_string()), ("srcset".to_string(), "b.jpg 200w".to_string())], ""), MediaSource::DirectDownload("b.jpg".to_string()));
    }

    #[test]
    fn test_media_3_srcset_width_based_best() {
        assert_eq!(find_media_source("img", &[("srcset".to_string(), "a.jpg 100w, b.jpg 200w, c.jpg 150w".to_string())], ""), MediaSource::DirectDownload("b.jpg".to_string()));
    }

    #[test]
    fn test_media_4_srcset_density_based_best() {
        assert_eq!(find_media_source("img", &[("srcset".to_string(), "a.jpg 1x, b.jpg 3x, c.jpg 2x".to_string())], ""), MediaSource::DirectDownload("b.jpg".to_string()));
    }

    #[test]
    fn test_media_5_video_direct() {
        assert_eq!(find_media_source("video", &[("src".to_string(), "vid.mp4".to_string())], ""), MediaSource::DirectDownload("vid.mp4".to_string()));
    }

    #[test]
    fn test_media_6_video_hls() {
        assert_eq!(find_media_source("video", &[("src".to_string(), "stream.m3u8".to_string())], ""), MediaSource::StreamNotDownloadable("stream.m3u8".to_string()));
    }

    #[test]
    fn test_media_7_video_dash() {
        assert_eq!(find_media_source("video", &[("src".to_string(), "stream.mpd".to_string())], ""), MediaSource::StreamNotDownloadable("stream.mpd".to_string()));
    }

    #[test]
    fn test_media_8_video_with_sources() {
        assert_eq!(find_media_source("video", &[], "<source src=\"vid.mp4\">"), MediaSource::DirectDownload("vid.mp4".to_string()));
    }

    #[test]
    fn test_media_9_video_with_multiple_sources_hls_is_present() {
        assert_eq!(find_media_source("video", &[], "<source src=\"stream.m3u8\"><source src=\"vid.mp4\">"), MediaSource::StreamNotDownloadable("stream.m3u8".to_string()));
    }

    #[test]
    fn test_media_10_audio_direct() {
        assert_eq!(find_media_source("audio", &[("src".to_string(), "aud.mp3".to_string())], ""), MediaSource::DirectDownload("aud.mp3".to_string()));
    }

    #[test]
    fn test_media_11_empty_srcset_fallback_to_src() {
        assert_eq!(find_media_source("img", &[("src".to_string(), "a.jpg".to_string()), ("srcset".to_string(), "".to_string())], ""), MediaSource::DirectDownload("a.jpg".to_string()));
    }

    #[test]
    fn test_media_12_invalid_srcset_fallback_to_src() {
        assert_eq!(find_media_source("img", &[("src".to_string(), "a.jpg".to_string()), ("srcset".to_string(), ",,,".to_string())], ""), MediaSource::DirectDownload("a.jpg".to_string()));
    }

    #[test]
    fn test_media_13_manifest_url_dash() {
        assert_eq!(find_media_source("video", &[("src".to_string(), "manifest.xml".to_string())], ""), MediaSource::StreamNotDownloadable("manifest.xml".to_string()));
    }

    #[test]
    fn test_media_14_unknown_tag() {
        assert_eq!(find_media_source("div", &[("src".to_string(), "a.jpg".to_string())], ""), MediaSource::NotFound);
    }

    #[test]
    fn test_media_15_no_sources() {
        assert_eq!(find_media_source("video", &[], "<div></div>"), MediaSource::NotFound);
    }
}
    #[test]
    fn test_media_16_audio_with_sources() {
        assert_eq!(find_media_source("audio", &[], "<source src=\"aud.mp3\">"), MediaSource::DirectDownload("aud.mp3".to_string()));
    }
    
    #[test]
    fn test_media_17_srcset_fallback_to_none_if_all_invalid() {
        assert_eq!(find_media_source("img", &[("srcset".to_string(), ",,,".to_string())], ""), MediaSource::NotFound);
    }
