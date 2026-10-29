//! Reading typography presets.
//!
//! GPUI already shapes and wraps text; this module centralises the spacing and
//! type scale that the rich-text renderer uses, so answers look consistent
//! instead of every block inventing its own padding. Formulas still go through
//! `ratex-gpui`, and body text uses the GPUI/`gpui-kit` text stack.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TypographyPreset {
    #[default]
    Standard,
    Relaxed,
    Compact,
    Custom,
}

impl TypographyPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            TypographyPreset::Standard => "standard",
            TypographyPreset::Relaxed => "relaxed",
            TypographyPreset::Compact => "compact",
            TypographyPreset::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "relaxed" => TypographyPreset::Relaxed,
            "compact" => TypographyPreset::Compact,
            "custom" => TypographyPreset::Custom,
            _ => TypographyPreset::Standard,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TypographyPreset::Standard => "标准",
            TypographyPreset::Relaxed => "宽松",
            TypographyPreset::Compact => "紧凑",
            TypographyPreset::Custom => "自定义",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            TypographyPreset::Standard => "16px / 1.75 行高，适合长时间阅读",
            TypographyPreset::Relaxed => "18px / 1.9 行高，留白更多",
            TypographyPreset::Compact => "15px / 1.55 行高，信息更密集",
            TypographyPreset::Custom => "沿用你手动调整的字号与行距",
        }
    }

    /// Body size and line-height applied when the reader picks a preset.
    /// `Custom` keeps the current values.
    pub fn canonical(self) -> Option<(f32, f32)> {
        match self {
            TypographyPreset::Standard => Some((16.0, 28.0)),
            TypographyPreset::Relaxed => Some((18.0, 34.0)),
            TypographyPreset::Compact => Some((15.0, 23.0)),
            TypographyPreset::Custom => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Typography {
    pub body_size: f32,
    pub line_height: f32,
    pub block_gap: f32,
    /// Maximum reading measure in px. Mobile windows are narrower and simply
    /// ignore it.
    pub measure: f32,
    pub quote_padding: f32,
    pub quote_radius: f32,
    pub list_indent: f32,
    pub list_item_gap: f32,
    pub code_size: f32,
    pub code_line_height: f32,
    pub code_padding: f32,
    pub image_radius: f32,
    pub caption_size: f32,
    pub link_underline: bool,
    heading_scale: [f32; 3],
}

impl Typography {
    pub fn from_settings(preset: TypographyPreset, body_size: f32, line_height: f32) -> Self {
        let body_size = body_size.max(12.0);
        let line_height = line_height.max(body_size * 1.3);
        match preset {
            TypographyPreset::Relaxed => Self {
                body_size,
                line_height,
                block_gap: 22.0,
                measure: 700.0,
                quote_padding: 16.0,
                quote_radius: 12.0,
                list_indent: 24.0,
                list_item_gap: 8.0,
                code_size: 15.0,
                code_line_height: 25.0,
                code_padding: 16.0,
                image_radius: 10.0,
                caption_size: 13.0,
                link_underline: false,
                heading_scale: [1.50, 1.32, 1.16],
            },
            TypographyPreset::Compact => Self {
                body_size,
                line_height,
                block_gap: 12.0,
                measure: 820.0,
                quote_padding: 8.0,
                quote_radius: 8.0,
                list_indent: 16.0,
                list_item_gap: 4.0,
                code_size: 13.0,
                code_line_height: 19.0,
                code_padding: 8.0,
                image_radius: 6.0,
                caption_size: 11.0,
                link_underline: true,
                heading_scale: [1.40, 1.22, 1.08],
            },
            TypographyPreset::Standard | TypographyPreset::Custom => Self {
                body_size,
                line_height,
                block_gap: 16.0,
                measure: 760.0,
                quote_padding: 12.0,
                quote_radius: 10.0,
                list_indent: 20.0,
                list_item_gap: 6.0,
                code_size: 14.0,
                code_line_height: 22.0,
                code_padding: 12.0,
                image_radius: 8.0,
                caption_size: 12.0,
                link_underline: true,
                heading_scale: [1.55, 1.35, 1.18],
            },
        }
    }

    pub fn heading_size(&self, level: u8) -> f32 {
        let index = (level as usize).saturating_sub(1).min(2);
        self.body_size * self.heading_scale[index]
    }

    pub fn heading_line_height(&self, level: u8) -> f32 {
        self.heading_size(level) * 1.35
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_roundtrip_and_labels() {
        for preset in [
            TypographyPreset::Standard,
            TypographyPreset::Relaxed,
            TypographyPreset::Compact,
            TypographyPreset::Custom,
        ] {
            assert_eq!(TypographyPreset::parse(preset.as_str()), preset);
            assert!(!preset.label().is_empty());
        }
        assert_eq!(
            TypographyPreset::parse("unknown"),
            TypographyPreset::Standard
        );
    }

    #[test]
    fn presets_change_scale_and_spacing() {
        let standard = Typography::from_settings(TypographyPreset::Standard, 16.0, 28.0);
        let relaxed = Typography::from_settings(TypographyPreset::Relaxed, 18.0, 34.0);
        let compact = Typography::from_settings(TypographyPreset::Compact, 15.0, 23.0);
        assert!(relaxed.block_gap > standard.block_gap);
        assert!(compact.block_gap < standard.block_gap);
        assert!(relaxed.measure < standard.measure);
        assert!(compact.code_size < standard.code_size);
        assert!(standard.heading_size(1) > standard.heading_size(2));
        assert!(standard.heading_size(2) > standard.heading_size(3));
        assert!(standard.heading_size(3) > standard.body_size);
    }

    #[test]
    fn custom_keeps_manual_sizes() {
        let custom = Typography::from_settings(TypographyPreset::Custom, 19.0, 32.0);
        assert_eq!(custom.body_size, 19.0);
        assert_eq!(custom.line_height, 32.0);
        assert_eq!(TypographyPreset::Standard.canonical(), Some((16.0, 28.0)));
        assert_eq!(TypographyPreset::Custom.canonical(), None);
    }
}
