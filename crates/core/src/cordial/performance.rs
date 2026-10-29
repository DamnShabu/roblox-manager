//! How much of the machine one account's client may use: four levels, from a
//! client along for the ride to one played at its best.

use serde::{Deserialize, Serialize};

/// One account's performance level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Performance {
    /// 10 frames a second, the game's lowest graphics, a lighter idle loop,
    /// niced, and slower still out of focus.
    Low,
    /// 60 frames a second, reduced graphics, niced less.
    Medium,
    /// The monitor's refresh rate, the game's own graphics: a client as
    /// earlier versions started one without low power.
    #[default]
    High,
    /// The monitor's refresh rate and the game's top graphics.
    Max,
}

impl Performance {
    pub const ALL: [Performance; 4] =
        [Performance::Low, Performance::Medium, Performance::High, Performance::Max];

    pub fn label(self) -> &'static str {
        match self {
            Performance::Low => "Low",
            Performance::Medium => "Medium",
            Performance::High => "High",
            Performance::Max => "Max",
        }
    }

    /// The frame cap, given the monitor's refresh rate when it is known.
    /// None leaves the engine to read the display itself.
    pub fn fps_cap(self, display_hz: Option<u32>) -> Option<u32> {
        match self {
            Performance::Low => Some(10),
            Performance::Medium => Some(60),
            Performance::High | Performance::Max => display_hz,
        }
    }

    /// The graphics-quality preset Stacked turns into FastFlags
    /// (`CORDIAL_QUALITY`), or None for the engine's own.
    pub fn flag_preset(self) -> Option<&'static str> {
        match self {
            Performance::Low => Some("low"),
            Performance::Medium => Some("medium"),
            Performance::High => None,
            Performance::Max => Some("max"),
        }
    }

    /// The step the game's own graphics slider is set to (1-10), or None to
    /// leave it to the player.
    pub fn slider(self) -> Option<&'static str> {
        match self {
            Performance::Low => Some("1"),
            Performance::Medium => Some("4"),
            Performance::High => None,
            Performance::Max => Some("10"),
        }
    }

    /// The niceness its client runs at, or None for the default.
    pub fn nice(self) -> Option<&'static str> {
        match self {
            Performance::Low => Some("10"),
            Performance::Medium => Some("5"),
            Performance::High | Performance::Max => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_are_written_as_their_names() {
        let names: Vec<String> =
            Performance::ALL.iter().map(|p| serde_json::to_string(p).unwrap()).collect();
        assert_eq!(names, ["\"low\"", "\"medium\"", "\"high\"", "\"max\""]);
    }

    #[test]
    fn high_and_max_run_at_the_monitors_rate() {
        assert_eq!(Performance::Low.fps_cap(Some(144)), Some(10));
        assert_eq!(Performance::Medium.fps_cap(Some(144)), Some(60));
        assert_eq!(Performance::High.fps_cap(Some(144)), Some(144));
        assert_eq!(Performance::Max.fps_cap(None), None, "the engine reads it itself");
    }
}
