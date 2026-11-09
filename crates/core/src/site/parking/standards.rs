//! The sizes a car park is laid out to: a country's standard, and what the person changes of it (Arqiflow's
//! web/lib/parking/standards.ts). Every length in metres, as the solver works.

/// A country's parking standard.
pub struct Standard {
    pub id: &'static str,
    pub name: &'static str,
    /// A 90° bay: width along the aisle, depth from the aisle.
    pub stall_width: f64,
    pub stall_depth: f64,
    /// A two-way aisle between 90° bays.
    pub aisle: f64,
    /// A drive aisle with no bays on it.
    pub drive_aisle: f64,
    /// A way in or out one way only.
    pub one_way_entry: f64,
    pub accessible_width: f64,
    pub access_aisle: f64,
    /// Least distance from a street corner to a curb cut.
    pub entry_from_corner: f64,
    pub source: &'static str,
}

pub const SAUDI: Standard = Standard {
    id: "saudi",
    name: "Saudi Arabia (MOMAH 2023)",
    stall_width: 2.8,
    stall_depth: 5.5,
    aisle: 7.5,
    drive_aisle: 7.0,
    one_way_entry: 3.5,
    accessible_width: 2.8,
    access_aisle: 2.8,
    entry_from_corner: 9.0,
    source: "MOMAH parking design guide 2023, Table 1; Saudi universal access guide",
};

pub const US: Standard = Standard {
    id: "us",
    name: "United States (ULI, ADA 2010)",
    stall_width: 2.74,
    stall_depth: 5.49,
    aisle: 7.32,
    drive_aisle: 7.32,
    one_way_entry: 4.27,
    accessible_width: 2.44,
    access_aisle: 1.52,
    entry_from_corner: 15.0,
    source: "ULI Dimensions of Parking; ADA 2010 §208, §502",
};

pub const UK: Standard = Standard {
    id: "uk",
    name: "United Kingdom (IStructE 2023)",
    stall_width: 2.6,
    stall_depth: 5.0,
    aisle: 6.0,
    drive_aisle: 6.0,
    one_way_entry: 3.5,
    accessible_width: 2.6,
    access_aisle: 1.2,
    entry_from_corner: 10.0,
    source: "IStructE Car park design 2023; BS 8300",
};

/// The standard by its id; Saudi Arabia's when it is not one of them.
pub fn standard(id: &str) -> &'static Standard {
    match id {
        "us" => &US,
        "uk" => &UK,
        _ => &SAUDI,
    }
}

impl Standard {
    /// The aisle bays at an angle need: one-way or two-way.
    pub fn aisle_for(&self, angle: f64, one_way: bool) -> f64 {
        match self.id {
            // ULI, 9 × 18 ft bays: 75° 22 ft, 60° 18 ft, 45° 13 ft one-way.
            "us" => {
                if angle >= 89.9 || !one_way {
                    7.32
                } else if angle >= 75.0 {
                    6.71
                } else if angle >= 60.0 {
                    5.49
                } else if angle >= 45.0 {
                    3.96
                } else {
                    3.66
                }
            }
            // IStructE: 75° 4.98 m, 60° 4.2 m, 45° 3.6 m one-way.
            "uk" => {
                if angle >= 89.9 {
                    6.0
                } else if !one_way {
                    6.95
                } else if angle >= 75.0 {
                    4.98
                } else if angle >= 60.0 {
                    4.2
                } else {
                    3.6
                }
            }
            // Table 1: 30°, 45° and 60° bays on a 5.0 m one-way or 7.2 m two-way aisle; 90° on 7.5 m.
            _ => {
                if angle >= 89.9 {
                    7.5
                } else if one_way {
                    5.0
                } else {
                    7.2
                }
            }
        }
    }

    /// Accessible bays a car park of `n` bays needs.
    pub fn accessible_needed(&self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        match self.id {
            "us" => {
                for (most, need) in [(25, 1), (50, 2), (75, 3), (100, 4), (150, 5), (200, 6), (300, 7), (400, 8), (500, 9)] {
                    if n <= most {
                        return need;
                    }
                }
                if n <= 1000 {
                    return (n as f64 * 0.02).ceil() as usize;
                }
                20 + (n - 1000).div_ceil(100)
            }
            "uk" => 1usize.max((n as f64 * 0.05).ceil() as usize),
            _ => {
                for (most, need) in [(50, 1), (100, 2), (200, 4), (300, 5), (500, 6)] {
                    if n <= most {
                        return need;
                    }
                }
                6 + (n - 500).div_ceil(100)
            }
        }
    }
}

/// What a layout is set out with. Every length in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The standard's id as given (an unknown one reads as Saudi Arabia's).
    pub standard: String,
    pub stall_width: f64,
    pub stall_depth: f64,
    /// The bays' angle to the aisle: 90, or 75, 60, 45, 30.
    pub angle: f64,
    pub one_way: bool,
    pub aisle: f64,
    /// The two-way aisle at this angle.
    pub aisle_two_way: f64,
    pub drive_aisle: f64,
    pub one_way_entry: f64,
    /// The share of bays with an electric car charger (0 to 1).
    pub ev_share: f64,
    /// A car park of this many bays gets a second way in and out when none is placed.
    pub second_access_over: f64,
    pub accessible_width: f64,
    pub access_aisle: f64,
    /// A planted island closing each end of a row, its length along the row (0: none).
    pub end_island: f64,
    /// A planted island every this many bays along a row (0: none).
    pub island_every: f64,
    /// Landscape kept along the plot's edge.
    pub setback: f64,
    /// A footway kept round each building.
    pub building_clearance: f64,
    /// A run of fewer bays than this is planted instead.
    pub min_run: f64,
    pub throat: f64,
    pub entry_from_corner: f64,
    /// Below ground: a column after every this many bays along a row (0: none), and its size (square).
    pub column_every: f64,
    pub column_size: f64,
    pub floor_to_floor: f64,
    pub ramp_slope: f64,
    pub clear_height: f64,
}

/// What the person changed of the standard: each left out is the standard's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Given {
    pub standard: Option<String>,
    pub angle: Option<f64>,
    pub one_way: Option<bool>,
    pub stall_width: Option<f64>,
    pub stall_depth: Option<f64>,
    pub aisle: Option<f64>,
    pub aisle_two_way: Option<f64>,
    pub drive_aisle: Option<f64>,
    pub one_way_entry: Option<f64>,
    pub ev_share: Option<f64>,
    pub second_access_over: Option<f64>,
    pub accessible_width: Option<f64>,
    pub access_aisle: Option<f64>,
    pub end_island: Option<f64>,
    pub island_every: Option<f64>,
    pub setback: Option<f64>,
    pub building_clearance: Option<f64>,
    pub min_run: Option<f64>,
    pub throat: Option<f64>,
    pub entry_from_corner: Option<f64>,
    pub column_every: Option<f64>,
    pub column_size: Option<f64>,
    pub floor_to_floor: Option<f64>,
    pub ramp_slope: Option<f64>,
    pub clear_height: Option<f64>,
}

pub fn settings_of(g: &Given) -> Settings {
    let std = standard(g.standard.as_deref().unwrap_or("saudi"));
    let angle = g.angle.unwrap_or(90.0).clamp(30.0, 90.0);
    let one_way = g.one_way.unwrap_or(angle < 90.0);
    Settings {
        standard: g.standard.clone().unwrap_or_else(|| std.id.to_string()),
        stall_width: g.stall_width.unwrap_or(std.stall_width),
        stall_depth: g.stall_depth.unwrap_or(std.stall_depth),
        angle,
        one_way,
        aisle: g.aisle.unwrap_or(std.aisle_for(angle, one_way)),
        aisle_two_way: g.aisle_two_way.unwrap_or(std.aisle_for(angle, false)),
        one_way_entry: g.one_way_entry.unwrap_or(std.one_way_entry),
        ev_share: g.ev_share.unwrap_or(0.0),
        second_access_over: g.second_access_over.unwrap_or(250.0),
        drive_aisle: g.drive_aisle.unwrap_or(std.drive_aisle),
        accessible_width: g.accessible_width.unwrap_or(std.accessible_width),
        access_aisle: g.access_aisle.unwrap_or(std.access_aisle),
        end_island: g.end_island.unwrap_or(1.5),
        island_every: g.island_every.unwrap_or(0.0),
        setback: g.setback.unwrap_or(0.0),
        building_clearance: g.building_clearance.unwrap_or(1.5),
        min_run: g.min_run.unwrap_or(3.0),
        throat: g.throat.unwrap_or(6.0),
        entry_from_corner: g.entry_from_corner.unwrap_or(std.entry_from_corner),
        column_every: g.column_every.unwrap_or(0.0),
        column_size: g.column_size.unwrap_or(0.6),
        floor_to_floor: g.floor_to_floor.unwrap_or(3.5),
        ramp_slope: g.ramp_slope.unwrap_or(0.15),
        clear_height: g.clear_height.unwrap_or(2.2),
    }
}

pub fn accessible_needed(s: &Settings, n: usize) -> usize {
    standard(&s.standard).accessible_needed(n)
}

/// How far a row of bays reaches from its aisle: a 90° bay's depth, or an angled bay's slanted footprint.
pub fn row_depth(s: &Settings) -> f64 {
    if s.angle >= 89.9 {
        return s.stall_depth;
    }
    let t = (s.angle * std::f64::consts::PI) / 180.0;
    s.stall_depth * t.sin() + s.stall_width * t.cos()
}

/// How far apart the bays come along their aisle.
pub fn pitch_of(s: &Settings) -> f64 {
    if s.angle >= 89.9 {
        s.stall_width
    } else {
        s.stall_width / ((s.angle * std::f64::consts::PI) / 180.0).sin()
    }
}

/// How far an angled bay's back runs past its front, along the aisle (0 at 90°).
pub fn lean_of(s: &Settings) -> f64 {
    if s.angle >= 89.9 {
        0.0
    } else {
        row_depth(s) / ((s.angle * std::f64::consts::PI) / 180.0).tan()
    }
}

/// A straight ramp's length: its slope, and a transition at half the slope at each end (3.6 m each).
pub fn ramp_length(s: &Settings) -> f64 {
    let t = 3.6;
    let rise = 0f64.max(s.floor_to_floor - 2.0 * t * (s.ramp_slope / 2.0));
    super::geom::js_round((2.0 * t + rise / s.ramp_slope) * 10.0) / 10.0
}
