use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::gdk::ModifierType;
use serde::Serialize;

pub const FORMAT: &str = "identity-path-v1";
pub const TIME_UNIT: &str = "seconds";
pub const COORDINATE_SPACE: &str = "source_pixels_top_left_origin";
pub const PRIMARY_BUTTON: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathPoint {
    pub x: u32,
    pub y: u32,
    pub t: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathSource {
    pub uri: String,
    pub display_name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathDocument {
    #[serde(rename = "format")]
    format_name: &'static str,
    created_at_unix_ms: u64,
    source: PathSource,
    time_unit: &'static str,
    coordinate_space: &'static str,
    points: Vec<PathPoint>,
}

impl PathDocument {
    pub fn new(created_at_unix_ms: u64, source: PathSource, points: Vec<PathPoint>) -> Self {
        Self {
            format_name: FORMAT,
            created_at_unix_ms,
            source,
            time_unit: TIME_UNIT,
            coordinate_space: COORDINATE_SPACE,
            points,
        }
    }

    pub fn to_pretty_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn write_to_path(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(path, self.to_pretty_json()?)?;
        Ok(())
    }
}

pub fn should_record_click(button: u32, modifiers: ModifierType) -> bool {
    button == PRIMARY_BUTTON && !modifiers.contains(ModifierType::SHIFT_MASK)
}

pub fn point_from_image_pos(
    (x, y): (f64, f64),
    width: u32,
    height: u32,
    t: f64,
) -> Option<PathPoint> {
    if !x.is_finite() || !y.is_finite() || !t.is_finite() {
        return None;
    }

    if x < 0. || y < 0. || x >= width as f64 || y >= height as f64 {
        return None;
    }

    Some(PathPoint {
        x: x.floor() as u32,
        y: y.floor() as u32,
        t,
    })
}

pub fn unix_timestamp_ms() -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    timestamp.try_into().unwrap_or(u64::MAX)
}

pub fn output_path(created_at_unix_ms: u64) -> PathBuf {
    PathBuf::from(format!("/tmp/identity-path-{created_at_unix_ms}.json"))
}

#[cfg(test)]
mod tests {
    use gtk::gdk::ModifierType;

    use super::*;

    #[test]
    fn click_policy_records_plain_primary_clicks_only() {
        assert!(should_record_click(1, ModifierType::empty()));
        assert!(!should_record_click(1, ModifierType::SHIFT_MASK));
        assert!(!should_record_click(3, ModifierType::empty()));
    }

    #[test]
    fn image_positions_become_top_left_source_pixel_indices() {
        assert_eq!(
            point_from_image_pos((4.37, 12.33), 7680, 4320, 1.25),
            Some(PathPoint {
                x: 4,
                y: 12,
                t: 1.25,
            })
        );
        assert_eq!(
            point_from_image_pos((7679.999, 4319.999), 7680, 4320, 2.0),
            Some(PathPoint {
                x: 7679,
                y: 4319,
                t: 2.0,
            })
        );
        assert_eq!(point_from_image_pos((7680.0, 12.0), 7680, 4320, 1.0), None);
        assert_eq!(point_from_image_pos((-0.1, 12.0), 7680, 4320, 1.0), None);
    }

    #[test]
    fn document_serializes_the_path_json_contract() {
        let document = PathDocument::new(
            1_782_470_000_000,
            PathSource {
                uri: "file:///tmp/moon8k.mov".to_owned(),
                display_name: "moon8k.mov".to_owned(),
                width: 7680,
                height: 4320,
            },
            vec![
                PathPoint {
                    x: 2450,
                    y: 2100,
                    t: 12.345678,
                },
                PathPoint {
                    x: 5200,
                    y: 1850,
                    t: 165.0,
                },
            ],
        );

        let json = document.to_pretty_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["format"], "identity-path-v1");
        assert_eq!(value["created_at_unix_ms"], 1_782_470_000_000_u64);
        assert_eq!(value["time_unit"], "seconds");
        assert_eq!(value["coordinate_space"], "source_pixels_top_left_origin");
        assert_eq!(value["source"]["uri"], "file:///tmp/moon8k.mov");
        assert_eq!(value["source"]["display_name"], "moon8k.mov");
        assert_eq!(value["source"]["width"], 7680);
        assert_eq!(value["source"]["height"], 4320);
        assert_eq!(value["points"][0]["x"], 2450);
        assert_eq!(value["points"][0]["y"], 2100);
        assert_eq!(value["points"][0]["t"], 12.345678);
    }
}
