//! BIMCraft's desktop front end, in egui: a thin shell over the core's [`Session`]. Panels read
//! the session's model and change it only through [`Session::execute`], so the desktop, the CLI,
//! scripts and agents share one set of commands, rules and undo history. The layout is Revit's:
//! the Quick Access Toolbar over the ribbon's tabs, a contextual Modify tab for the tool or the
//! selection, the Options Bar, Properties over the Project Browser on the left, views in tabs
//! (plans, the 3D view, schedules), and the tool's prompt in the status bar.

use crate::popup::Modal;

use bimcraft_i18n::msg;

mod array;
mod background_grid;
mod browser;
mod cad;
mod camera;
mod camgrips;
mod canvas;
mod checks;
mod clipboard;
mod clouds;
mod context;
pub mod control;
mod dialog;
mod dimgrips;
mod dimtypes;
mod display;
mod dock;
mod draw;
mod editor;
mod electrical;
mod favorites;
mod fillpatterns;
mod filter;
mod find;
mod flip;
mod fly;
mod footer;
mod gpu;
pub use gpu::limits_for;
mod graphics;
mod groups;
pub mod host;
pub mod hostview;
pub mod icons;
mod images;
mod inquiry;
pub mod lang;
mod linestyles;
mod lineweights;
mod links;
mod mep;
mod minimap;
mod notes;
mod objectstyles;
mod orbit;
mod paper;
mod pdfread;
mod phasing;
mod pin;
#[cfg(feature = "plugins")]
mod plugins;
mod popup;
mod presets;
mod properties;
mod purge;
pub mod recent;
mod reinforce;
mod renames;
mod retained;
pub mod revit;
mod ribbon;
mod section;
mod section_box;
mod section_worker;
mod selecting;
pub mod settings;
mod shape;
pub mod shortcuts;
mod site;
mod sketch;
mod sketch3d;
pub mod spacemouse;
mod standards;
mod start;
mod structure;
mod style;
mod tempdimprops;
mod templates;
#[cfg(test)]
mod tests;
mod titlebar;
mod titleblocks;
mod triad;
mod typeprops;
#[cfg(feature = "updater")]
mod updates;
mod view_worker;
mod warnings;
pub mod window;
mod workplane;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use bimcraft_core::dims::Placing as DimMode;
use bimcraft_core::engine::{self, Session};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{CEILING_HEIGHT, DimKind, Element, Id, Justify, Location, Model, RAMP_WIDTH, Reference, RoomArea, STAIR_WIDTH};
use bimcraft_core::snap::{Kind as Snap, Snaps};
use bimcraft_core::units::Units;
use bimcraft_core::{element_json_bare, render, schedule, stairs};
use egui::containers::menu::{MenuButton, SubMenuButton};
use egui::{
    Align, Align2, Area, CentralPanel, Color32, Context, CornerRadius, Event, FontId, Frame, Grid, Id as UiId, Key, KeyboardShortcut,
    Layout, Margin, Modifiers, Order, Panel, Popup, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use bimcraft_core::language::Language;
mod autosave;
use icons::Icon;
use lang::{Lang, t, tf};
use ribbon::panel;
use style::pal;

/// A view: what the middle of the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sheet {
    /// A level's floor plan.
    Plan(Id),
    /// A level's reflected ceiling plan.
    Ceiling(Id),
    /// The default 3D view, `{3D}`.
    View3D,
    /// A section's or an elevation's view.
    Section(Id),
    /// A sheet: paper with a title block, and views placed on it.
    Paper(Id),
    /// One of the schedules every project has, by its category (`schedule::VIEWS`): the project's
    /// own once it's changed.
    Schedule(&'static str),
    /// Another schedule of the project's, by id.
    Table(Id),
}

/// What clicks on the plan do.
#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    /// Select the element under the cursor.
    Modify,
    /// The points of the wall chain clicked so far.
    Wall(Vec<P2>),
    Door,
    Window,
    Room,
    /// Room separation lines, one a click from the point before: the last point clicked, while the chain goes on.
    RoomSeparator(Option<P2>),
    /// The Options Bar asks for the new level's elevation.
    Level,
    /// The grid's start, once clicked.
    Grid(Option<P2>),
    /// Move the selection: its base point, once clicked.
    Move(Option<P2>),
    /// Copy the selection: its base point, once clicked.
    Copy(Option<P2>),
    /// Paste what was copied: outlined at the cursor, put down by a click.
    Paste,
    /// Import PDF or Image: the image (`BimApp::placing`) follows the cursor, crossed, until a click places it.
    PlaceImage,
    /// Revit's Scale (RE), graphical: the origin, then the first reference point, once clicked; the second reference point,
    /// or the length typed in the Options Bar, scales the selected images and imports.
    Scale(Vec<P2>),
    /// Paste Aligned to Picked Level: in an elevation or a section, a click on a level line pastes what was copied onto it.
    PasteLevel,
    /// Move Witness Line: a click on the line a temporary dimension's witness line goes to, `far` for the one at the other end,
    /// of the wall's `side`.
    MoveWitness {
        wall: Id,
        side: bimcraft_core::tempdims::Side,
        far: bool,
    },
    /// Rotate the selection about `centre`: the start of the angle, once clicked; with `place`, the
    /// next click moves the centre.
    Rotate {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Array the selection (Linear: `start` is the first point once clicked; Radial: as Rotate, about `centre`, `start` the
    /// angle's start, and with `place` the next click moves the centre).
    Array {
        centre: P2,
        start: Option<P2>,
        place: bool,
    },
    /// Mirror the selection across a line picked, or with `draw`, a line drawn from `start`.
    Mirror {
        draw: bool,
        start: Option<P2>,
    },
    /// Line an element up with a reference line, once picked.
    Align(Option<(P2, P2)>),
    /// Trim or extend two walls to a corner: the first wall, and where it was picked.
    Trim(Option<(Id, P2)>),
    /// Trim/Extend Single or Multiple Elements: the boundary, once picked; then each line clicked goes to it (with `many`,
    /// until Esc; else one, and the boundary is picked again).
    TrimTo {
        boundary: Option<Id>,
        many: bool,
    },
    /// Add Vertex, or with `delete` Delete Vertex: on each line clicked, where it was clicked.
    Vertex {
        delete: bool,
    },
    /// Split the wall picked.
    Split,
    /// Copy (or move) the wall picked to the cursor's side, by the Options Bar's distance.
    Offset,
    /// Take the element picked down in the phase of the view on screen, or, if it comes down in it already, back up.
    Demolish,
    /// Draw the edge clicked in the Options Bar's line style, in this view only, as Revit's Linework does.
    Linework,
    /// Place Detail Group: the Options Bar's group type where clicked.
    DetailGroup,
    /// Repeating Detail: the Options Bar's group along a line, its start once clicked.
    RepeatingDetail(Option<P2>),
    /// Lay the material in the Options Bar over the face clicked in the 3D view, as Revit's Paint does.
    Paint,
    /// Take the paint off the face clicked in the 3D view.
    Unpaint,
    /// An aligned or linear dimension, laid as its mode says: the references and points picked so far. A click away from
    /// any places it.
    Dimension(DimMode, Vec<Reference>),
    /// An angular dimension: the lines picked so far, two that meet; then a click where its arc goes.
    Angular(Vec<Reference>),
    /// A radial, diameter or arc length dimension: what round was picked (an arc length's, then two points on its arc, if
    /// it measures part of it), then a click toward where it goes.
    Round(DimKind, Vec<Reference>),
    /// A text note where clicked, saying what the Options Bar says.
    Text,
    /// A spot elevation where clicked.
    Spot,
    /// A spot slope where clicked: how steeply the floor's or roof's top falls there.
    SpotSlope,
    /// A ruler laid between two points: the first, once clicked.
    Measure(Option<P2>),
    /// Tag the door or window clicked.
    Tag,
    /// A section: its start, once clicked.
    Section(Option<P2>),
    /// An elevation, looking at the side of the building clicked beside.
    Elevation,
    /// A camera: the point its eye stands at, once clicked; then its target. The eye and target heights
    /// are the Options Bar's over the plan's level (Revit's Camera tool).
    Camera(Option<P2>),
    /// A column where clicked.
    Column,
    /// A run (a beam, a pipe, a conduit…, by category, as `beam` and `pipe` name them): its first point, once clicked.
    Run(&'static str, Option<P2>),
    /// A structural column or an isolated footing (`structuralcolumn`, `footing`) where clicked.
    Stand(&'static str),
    /// A family's type where clicked: the one picked, turned by Space.
    Component,
    /// A fixture or a device of one tab's kinds (`mep::FIXTURES`, by key) where clicked: the family picked among the
    /// families of those kinds, turned by Space.
    Fixture(&'static str),
    /// A stair: the points clicked, its start and where its first run ends; the next click, which
    /// way its next run turns from the landing.
    Stair(Vec<P2>),
    /// A ramp, clicked as a stair is.
    Ramp(Vec<P2>),
    /// A railing's path, as clicked so far; with none, a click on a stair or a ramp puts one down
    /// each side.
    Railing(Vec<P2>),
    /// A roof over the walls of the plan's level, on the level above: Revit's Roof by Footprint,
    /// with every wall picked at once.
    Roof,
    /// A roof by extrusion, on the level above: its line's start and end, as clicked; the next
    /// click, how far across it's swept.
    ExtrusionRoof(Vec<P2>),
    /// A dormer where a roof's slope is clicked.
    Dormer,
    /// The roof footprint edge clicked stands upright, a gable end, or slopes again.
    GableEdge,
    /// Attach the selected walls' tops (or, as the Options Bar says, their bases) to the roof or
    /// floor picked.
    Attach,
    /// A floor under the walls of the plan's level.
    Floor,
    /// A ceiling in the room clicked, out to its walls' faces.
    Ceiling,
    /// A shaft opening: its first corner, once clicked.
    Shaft(Option<P2>),
    /// A mass: the corners of its footprint, as clicked so far.
    Mass(Vec<P2>),
    /// A tool of the Massing & Site tab's site panels (`site.rs`): placing where clicked; one that draws an outline or a
    /// path sketches it.
    Site(site::SiteTool),
    /// A detail line in the Options Bar's line style: its points, as clicked so far.
    DetailLine(Vec<P2>),
    /// The selected floor's slope arrow: its tail, once clicked.
    SlopeArrow(Option<P2>),
    /// Shape Editing's Modify Sub Elements: a point of the selected floor's shape, a corner, or both ends of an edge picked,
    /// to the Options Bar's elevation.
    ModifySubElements,
    /// Shape Editing's Add Point: a point on the selected floor's top at the Options Bar's elevation.
    AddPoint,
    /// Shape Editing's Add Split Line: a crease across the selected floor's top; its first end, once clicked.
    AddSplitLine(Option<P2>),
    /// Sketch mode's Draw panel: the points clicked so far of the shape it draws (Line: since the last loop closed).
    SketchLine(Vec<P2>),
    /// Sketch mode's Pick Walls: a line along each wall clicked.
    SketchPick,
    /// Fill Patterns' New Pattern > Custom: the tile's corners, the first once clicked (the lines are picked already).
    PatternCorner(Option<P2>),
}

/// Why the last command failed, shown in a box over the view.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub text: String,
    pub error: bool,
}

impl BimApp {
    /// Whether an update is being put in place, which holds the window until it has gone through or failed.
    pub(crate) fn installing(&self) -> bool {
        #[cfg(feature = "updater")]
        return self.updates.as_ref().is_some_and(|u| matches!(u.state(), bimcraft_update::State::Installing { .. }));
        #[cfg(not(feature = "updater"))]
        false
    }

    /// Whether the Plugins window is open.
    fn plugins_open(&self) -> bool {
        #[cfg(feature = "plugins")]
        return self.plugins_dialog;
        #[cfg(not(feature = "plugins"))]
        false
    }
}

/// `text` in the status box, as an error.
pub(crate) fn failed(text: String) -> Status {
    Status { text, error: true }
}

/// A snap override typed as a shortcut, for the next point picked: Revit's SE, SM, SI, SC, SP and
/// SN (that snap alone), and SO (none).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Once {
    Only(Snap),
    Off,
}

/// The ribbon's tabs. `Context` is the contextual Modify tab of the tool or the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tab {
    Architecture,
    Structure,
    Plumbing,
    Electrical,
    Lightning,
    Data,
    Insert,
    Annotate,
    Massing,
    View,
    Manage,
    Modify,
    Context,
    /// A tab a plugin declared, by its place in [`BimApp::plugin_tabs`].
    #[cfg(feature = "plugins")]
    Plugin(usize),
}

/// A key for the tab, for the frame data egui keeps between frames: a plain sort in reading order.
fn tab_key(tab: Tab) -> u64 {
    match tab {
        Tab::Architecture => 0,
        Tab::Structure => 1,
        Tab::Plumbing => 2,
        Tab::Electrical => 3,
        Tab::Lightning => 4,
        Tab::Data => 5,
        Tab::Insert => 6,
        Tab::Annotate => 7,
        Tab::Massing => 8,
        Tab::View => 9,
        Tab::Manage => 10,
        Tab::Modify => 11,
        Tab::Context => 12,
        #[cfg(feature = "plugins")]
        Tab::Plugin(i) => 100 + i as u64,
    }
}

/// A step that would drop unsaved changes, waiting on the user.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Recover(bimcraft_core::autosave::Backup),
    New,
    Open,
    /// The sample house, as a project nobody has saved ([`bimcraft_core::sample::SAMPLE`]).
    Sample,
    /// A file the system asked to open ([`OsEvent::Open`]).
    OpenPath(PathBuf),
    Close,
    /// Close, so the installer can replace the program, and start it again updated.
    #[cfg(feature = "updater")]
    Update,
}

/// What the system asks of the app from outside its window. macOS sends a file Finder, the Dock or
/// Open With opens, and Quit from the Dock, as Apple events rather than as arguments; the front end
/// hands them over through [`BimApp::os_events`].
#[derive(Clone, Debug, PartialEq)]
pub enum OsEvent {
    Open(PathBuf),
    Quit,
}

/// The schedules: the category `schedule` takes, and the view's name.
pub(crate) const SCHEDULES: [(&str, &str); 6] = schedule::VIEWS;

/// Ribbon buttons that pick tools: icon, label, tooltip, tool.
type Tools<'a> = &'a [(Icon, &'a str, &'a str, Tool)];

/// The Modify panel's small buttons: icon, label, tooltip, the tool (none for Delete), enabled.
type Buttons<'a> = &'a [(Icon, &'a str, &'a str, Option<Tool>, bool)];

/// The desktop's own two-letter shortcuts, which [`BimApp::shortcut`] takes before the commands'
/// aliases, and what Keyboard Shortcuts (KS) calls them.
const SHORTCUTS: [(&str, &str); 64] = [
    ("3D", "3D View"),
    ("MD", "Modify"),
    ("SB", "Floor: Structural"),
    ("RF", "Reinforce"),
    ("MM", msg!("Mirror Pick Axis")),
    ("DM", msg!("Mirror Draw Axis")),
    ("AL", msg!("Align")),
    ("OF", "Offset"),
    ("DW", "Demolish"),
    ("ZF", "Zoom to Fit"),
    ("ZE", "Zoom to Fit"),
    ("ZX", "Zoom to Fit"),
    ("ZA", msg!("Zoom All to Fit")),
    ("ZS", msg!("Zoom Sheet Size")),
    ("ZR", msg!("Zoom Region")),
    ("ZZ", msg!("Zoom Region")),
    ("ZP", msg!("Previous Zoom")),
    ("ZC", msg!("Previous Zoom")),
    ("ZO", msg!("Zoom Out")),
    ("ZV", msg!("Zoom Out")),
    ("RC", msg!("Repeat Last Command")),
    ("TL", "Thin Lines"),
    ("CS", msg!("Create Similar")),
    ("PP", "Properties"),
    ("VP", "Properties"),
    ("NP", "Project Browser"),
    ("SA", "Select All Instances in Entire Project"),
    ("AG", msg!("Alignment Guides")),
    ("OG", msg!("Orthogonal Guides")),
    ("SE", msg!("Snap to Endpoints")),
    ("SM", msg!("Snap to Midpoints")),
    ("SI", msg!("Snap to Intersections")),
    ("SC", msg!("Snap to Centres")),
    ("SP", msg!("Snap to Perpendiculars")),
    ("SQ", msg!("Snap to Quadrants")),
    ("ST", msg!("Snap to Tangents")),
    ("SN", msg!("Snap to Nearest")),
    ("SO", msg!("Snaps Off")),
    ("SS", msg!("Turn Snap Override Off")),
    ("VV", "Visibility/Graphics"),
    ("VG", "Visibility/Graphics"),
    ("EH", "Hide Element"),
    ("VH", "Hide Category"),
    ("HH", msg!("Temporary Hide Element")),
    ("HC", msg!("Temporary Hide Category")),
    ("HI", msg!("Temporary Isolate Element")),
    ("IC", msg!("Temporary Isolate Category")),
    ("HR", msg!("Reset Temporary Hide/Isolate")),
    ("RH", "Reveal Hidden Elements"),
    ("EU", msg!("Unhide Elements")),
    ("UE", "Unhide Elements"),
    ("VU", "Unhide Category"),
    ("EO", msg!("Override Graphics by Element")),
    ("FT", "Filters"),
    ("VT", "View Templates"),
    ("PH", "Phasing"),
    ("DO", "Design Options"),
    ("WF", "Wireframe"),
    ("HL", "Hidden Line"),
    ("SD", "Shaded"),
    ("KS", "Keyboard Shortcuts"),
    ("BX", "Section Box"),
    ("GD", "Graphic Display Options"),
    // Revit has none for its title blocks; no command or tool takes TB.
    ("TB", "Title Blocks"),
];

/// Every two-letter shortcut and what it does: the desktop's own, then the commands' aliases.
fn shortcuts() -> Vec<(&'static str, &'static str)> {
    let aliases = engine::commands().iter().flat_map(|c| c.aliases.iter().map(move |a| (*a, c.label)));
    SHORTCUTS
        .into_iter()
        .chain(mep::FIXTURES.iter().map(|f| (f.letters, f.label)))
        .chain(aliases.filter(|(a, _)| !SHORTCUTS.iter().any(|(s, _)| s == a)))
        .collect()
}

/// What an IFC import brought in, and the first few things it left out (the core's reasons, in
/// English).
fn import_note(file: &str, reply: &Value) -> String {
    let counts: Vec<String> = reply["elements"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(cat, n)| match n.as_u64().unwrap_or(0) {
            1 => format!("1 {}", lang::category(cat, false).to_lowercase()),
            n => format!("{n} {}", lang::category(cat, true).to_lowercase()),
        })
        .collect();
    let mut text = tf("Imported {}: {}.", &[file, &counts.join(", ")]);
    let skipped: Vec<&str> = reply["skipped"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !skipped.is_empty() {
        let more = match skipped.len().saturating_sub(3) {
            0 => String::new(),
            n => tf("; and {} more", &[&n.to_string()]),
        };
        text += &tf(" Left out: {}{}.", &[&skipped.iter().take(3).copied().collect::<Vec<_>>().join("; "), &more]);
    }
    if let Some([x, y]) = reply["moved"].as_array().map(Vec::as_slice) {
        text += &tf(" Moved {}, {} mm to bring it near the origin.", &[&x.to_string(), &y.to_string()]);
    }
    text
}

/// Elements' JSON (`element_json`) by id, and the model it was found in.
type KeptJson = (Weak<Model>, HashMap<Id, Arc<Value>>);

pub struct BimApp {
    pub session: Session,
    /// The system's requests since the last frame, read once a frame. None where it sends none.
    pub os_events: Option<Box<dyn FnMut() -> Vec<OsEvent>>>,
    /// The window draws its own title bar (`titlebar.rs`): the desktop's `main` says so where the system lets it (Windows and
    /// Linux), unless `system_title_bar`, the user's ask for the system's (Settings > User Interface, kept in `settings.json`).
    pub own_title_bar: bool,
    pub system_title_bar: bool,
    /// The active view, which takes the tools: a tab of `views`, or one of `floating`.
    pub sheet: Sheet,
    pub tool: Tool,
    /// The selected elements, in the order they were picked.
    pub selection: Vec<Id>,
    pub status: Status,
    /// What Revit would warn of after the last action ("Pinned objects were not deleted."): a warning, not an error, in the
    /// status box's corner until the next command works or it is dismissed. An error said since takes its place.
    pub(crate) warned: Vec<String>,
    /// The interface's own zoom (egui's zoom factor) on top of the system's scale, read each frame: Zoom Sheet Size takes it out.
    pub(crate) ui_zoom: f32,
    /// The interface's language: English unless told otherwise (the app asks the environment).
    pub lang: Lang,
    /// Where a font with Chinese glyphs comes from, when the window is put in Chinese.
    font_finder: lang::FontFinder,
    /// The views docked as tabs, and the one of them the views' area shows: the active view, unless that floats.
    views: Vec<Sheet>,
    front: Sheet,
    /// Views floating in windows of their own.
    floating: Vec<dock::FloatView>,
    /// A view other than the active one is being drawn: it takes no clicks.
    passive: bool,
    /// Where the palettes are, and what is being dragged.
    docking: dock::Docking,
    /// The hosting page's checks of the model, which the Checks palette lists (`checks.rs`).
    checks: checks::Checks,
    dragging: Option<dock::Drag>,
    /// What a drag may land on, as drawn this frame: each stack's title (the palette it shows, and whether it floats),
    /// the views' tab bar and its tabs, and the docked view's drawing, along whose edges palettes dock; and the pointer, last
    /// seen.
    drop_headers: Vec<(dock::Palette, Rect, bool)>,
    tab_bar: Rect,
    /// How far down the view the tabs reach where they float over it (else the view's top): what floats over the view
    /// (the Options Bar) and what docks along it start below.
    tabs_foot: f32,
    tab_rects: Vec<(Sheet, Rect)>,
    work: Rect,
    pointer: Pos2,
    /// Where a floating window's title was taken from, from its corner.
    grab: egui::Vec2,
    /// The ribbon folded to its tabs (a tab's name double-clicked), and its tools shown under them for now (one clicked
    /// while folded); where its tabs end.
    ribbon_min: bool,
    ribbon_peek: bool,
    /// What High Contrast Point Clouds fills a view's scans with: the user's (Settings > Graphics), crimson at first.
    scan_contrast: [u8; 3],
    background_grid_dialog: Option<background_grid::GridDialog>,
    /// Global grid snapping preference, independent of view graphics and templates.
    grid_snapping: bool,
    /// The origin marker on a plan, and the view's name in its corner (File > Settings): working aids, drawn on the screen and
    /// never in a drawing that is printed or exported.
    show_origin: bool,
    show_view_name: bool,
    ribbon_bottom: Pos2,
    /// The Compact layout (File > Settings): the bars and palettes flush to the edges, not floating cards.
    compact: bool,
    /// The layout a hosting page last asked for (`set_host_look`): asked again, the same, it leaves the person's own choice.
    host_compact: Option<bool>,
    /// A preset's colours when dark, and the user's accent (Settings > User Interface > Theme).
    scheme: style::Scheme,
    accent: Option<[u8; 3]>,
    /// The user's own panel, canvas and text colours over any scheme (the same page).
    panel_colour: Option<[u8; 3]>,
    canvas_colour: Option<[u8; 3]>,
    text_colour: Option<[u8; 3]>,
    /// The category the Properties palette's filter narrows a selection of several to, and the selection it was picked for: a new
    /// selection drops it (`BimApp::palette_ids`).
    palette_filter: Option<(&'static str, Vec<Id>)>,
    /// What `element_json` says of the elements the palette has shown, of the model it was found in (`BimApp::elements_json`):
    /// the palette reads it every frame, and a component's room costs its level's room boundaries.
    palette_json: RefCell<Option<KeptJson>>,
    /// File > Settings, while open, at its page; the settings file, when the app was given one, and what it holds; the
    /// theme it asks for, put in force on the next frame.
    settings_page: Option<settings::Page>,
    /// Find (Ctrl+K), while it's open: what is typed in it.
    find: Option<String>,
    /// The project's scale, the tools the user took out, whether Edit Tools is on, and the kept presets (`presets.rs`); with what
    /// Settings' preset name field holds and why the last save was refused.
    scale: presets::Scale,
    tools_hidden: BTreeSet<String>,
    editing_tools: bool,
    presets: Vec<presets::Preset>,
    preset_name: String,
    preset_note: Option<String>,
    settings_path: Option<PathBuf>,
    settings_kept: Option<settings::Settings>,
    /// The window's size, kept and brought within its screen (`window.rs`).
    window: window::Tracker,
    theme_wanted: Option<egui::ThemePreference>,
    /// Keep drawing backgrounds light even when the interface is dark.
    light_canvas: bool,
    tab: Tab,
    /// Where the ribbon goes back to when the contextual tab goes.
    back: Tab,
    /// The type each placing tool uses, by category; the core's default when missing.
    types: BTreeMap<&'static str, String>,
    /// The family a door or window type picked belongs to (none: one of the project's own); by category.
    opening_families: BTreeMap<&'static str, String>,
    /// The Wall tool's top: `None` as the core does it (up to the next level up), `Some(None)`
    /// unconnected at `wall_height`, `Some(Some(level))` up to that level.
    wall_top: Option<Option<Id>>,
    wall_height: String,
    /// The Column tool's top, as `wall_top` and `wall_height` are the Wall tool's.
    column_top: Option<Option<Id>>,
    column_height: String,
    /// The Floor tool makes structural floors; the Beam tool's justification and height over the level; the footing tools'
    /// thickness and the height of their tops.
    floor_structural: bool,
    run_justify: Justify,
    beam_offset: String,
    /// The Options Bar's justification and offset of the run tools not on screen, by category, and whose are.
    run_kept: BTreeMap<&'static str, (Justify, String)>,
    run_loaded: &'static str,
    footing_thickness: String,
    footing_top: String,
    /// The Stair tool's width; the Roof tool's slope, overhang and gable ends; the Roof by
    /// Extrusion tool's profile, a shed's rather than a gable's; the Dormer tool's width and height,
    /// and whether its front has a window.
    stair_width: String,
    roof_slope: String,
    /// The scale being typed in the view control bar's Scale menu.
    scale_text: String,
    roof_overhang: String,
    roof_gable: bool,
    roof_shed: bool,
    dormer_width: String,
    dormer_height: String,
    dormer_window: bool,
    /// Stairs and ramps made with a railing down each side; the Ramp tool's top, width and slope.
    railings: bool,
    ramp_top: Option<Option<Id>>,
    ramp_height: String,
    ramp_width: String,
    ramp_slope: String,
    /// How high the Ceiling tool's ceilings are; the Slope Arrow's slope; the Shaft tool's top.
    ceiling_height: String,
    floor_slope: String,
    /// Add Point's elevation: above the floor's top as it would be without its shape, or with `shape_absolute`, above its
    /// level; empty, the top's height where it's clicked.
    shape_z: String,
    /// What the Options Bar's elevations count from.
    pub(crate) shape_base: crate::shape::Base,
    /// Add Point's mode, as Revit's: along the surface as it is, `shape_offset` over it; else Absolute, at `shape_z` above
    /// the floor's level.
    pub(crate) shape_along: bool,
    pub(crate) shape_offset: String,
    /// The selected floor's shape point a drag under Modify Sub Elements holds, where it was.
    shape_grip: Option<P2>,
    /// The selected floor as a grip's drag would leave it, plan or 3D, while it's dragged: its creases drawn over it.
    pub(crate) shape_preview: Option<bimcraft_core::model::Floor>,
    /// What Modify Sub Elements has selected of the shaped floor's shape.
    pub(crate) shape_picked: Vec<crate::shape::Sub>,
    /// A shape point's height label clicked into a field: the point, and what's typed.
    pub(crate) shape_label_edit: Option<(P2, String)>,
    shaft_top: Option<Option<Id>>,
    shaft_height: String,
    /// The Mass tool's top, as the Shaft tool's.
    mass_top: Option<Option<Id>>,
    mass_height: String,
    /// The site tools' Options Bars (`site.rs`), the preview of a site path being sketched, and the selected site element's
    /// grip being dragged: its prop and the point's place in it.
    site: site::SiteChoices,
    site_ghost: site::GhostCache,
    site_grip: Option<(&'static str, usize)>,
    /// A selected camera's grip being dragged on a plan (`camgrips.rs`): the camera view, and which grip.
    camera_grip: Option<(Id, usize)>,
    /// A selected camera's eye (0) or target (1) being raised or lowered in a section or elevation.
    camera_section_grip: Option<(Id, usize)>,
    /// The Camera tool's eye and target heights over the plan's level, and its lens, as typed.
    camera_eye_height: String,
    camera_target_height: String,
    camera_lens: String,
    /// Each click ends a wall and starts the next; off, two clicks make one wall.
    chain: bool,
    /// The line the Wall tool's walls stand on.
    wall_location: Location,
    /// Attach Top/Base attaches bases.
    attach_base: bool,
    /// Thin Lines: every line one pixel wide.
    thin: bool,
    /// Move and Copy keep to horizontal and vertical.
    constrain: bool,
    /// Copy places copy after copy from the same base point.
    multiple: bool,
    /// The Options Bar's Rotate angle, Copy for Rotate and for Mirror, and Offset distance and Copy.
    angle: String,
    rotate_copy: bool,
    /// Array's Options Bar.
    array: array::Options,
    mirror_copy: bool,
    offset: String,
    offset_copy: bool,
    /// Tags placed with a leader.
    tag_leader: bool,
    /// The field the Tag tool's tags show; blank for the category's usual one.
    tag_label: String,
    /// What the Text tool writes.
    note_text: String,
    /// The note being written on the canvas.
    note_edit: Option<notes::NoteEdit>,
    /// Text Type Properties, while it's open.
    text_type_dialog: Option<notes::TextTypeDialog>,
    dimension_type_dialog: Option<dimtypes::DimensionTypeDialog>,
    dimension_text_dialog: Option<dimtypes::DimensionTextDialog>,
    /// The line style the Detail Line tool draws in; blank for the project's Thin Lines.
    line_style: String,
    /// The Linework tool's line style: a style's name, or `<Invisible lines>` or `<By Category>` (in English).
    linework_style: String,
    /// The detail group type Place Detail Group places.
    group_type: String,
    /// The Repeating Detail tool's spacing, as typed.
    repeat_spacing: String,
    /// The Detail Line tool's Chain: each click ends a line and starts the next; off, two clicks make one line.
    line_chain: bool,
    /// The shape each Draw panel draws, and the Options Bar's Sides, Radius and Offset.
    pub(crate) drawing: draw::Drawing,
    /// The project units the Options Bar's lengths were last written in.
    units: Units,
    /// A length typed while picking points: the listening dimension.
    pub(crate) typed_length: String,
    /// What each view hides or reveals for now (not the project's), and the model it is drawn from.
    temporary: HashMap<Sheet, footer::Temp>,
    shadow: Option<footer::Shadow>,
    /// Linked 3D View, while it's on: where the 3D view was before it floated beside the plan.
    linked_3d: Option<section_box::Was>,
    /// The project, and what the views draw of it with its links built in: see [`BimApp::view_model`].
    seen: Option<(Arc<Model>, Arc<Model>)>,
    /// Link Model was asked for: its file dialog opens on the next frame, so a shortcut typed alone never waits on one.
    link_dialog: bool,
    /// Insert > Point Cloud was pressed: its file dialog opens this frame.
    cloud_dialog: bool,
    /// Bringing scan files in: the box, while it's open.
    cloud_box: Option<clouds::CloudBox>,
    /// Insert > Import CAD (false) or Link CAD (true) was pressed: its file dialog opens this frame.
    cad_dialog: Option<bool>,
    /// Bringing a CAD drawing in: the box, while it's open.
    cad_box: Option<cad::CadBox>,
    /// Insert > Import PDF (true) or Image (false) was pressed: its file dialog opens this frame.
    image_dialog: Option<bool>,
    /// Import PDF's box, while it's open.
    pdf_box: Option<images::PdfBox>,
    /// The image being placed by a click (Tool::PlaceImage).
    placing: Option<images::Placing>,
    /// The PDF files being read apart from the window, and their answers (`pdfread.rs`).
    pdf_reader: pdfread::PdfReader,
    /// Manage Images is open, and the image picked in it.
    images_window: bool,
    images_picked: Option<String>,
    /// Scale's Options Bar: the length the first reference becomes, as typed.
    scale_length: String,
    /// A selected element's temporary dimension being typed: the element, which of its dimensions (a wall has one, a
    /// door or window two: from its wall's start, from its end), the length so far.
    dim_edit: Option<(Id, usize, String)>,
    /// A selected wall's angle being typed: the wall, whether it turns about its start (else its
    /// end), the angle so far.
    angle_edit: Option<(Id, bool, String)>,
    /// The snaps in force (Manage > Snaps), and whether their dialog is open.
    snaps: Snaps,
    show_snaps: bool,
    /// An override of them for the next point picked, typed as a shortcut.
    snap_once: Option<Once>,
    /// The Measure tool's last ruler: where it starts and ends.
    measured: Option<(P2, P2)>,
    /// Tab pressed over the plan, taken from egui's focus keys: cycle what's under the cursor.
    tab_cycle: bool,
    /// What the hand picks and repeats: a tool waiting for its elements, the last tool, Tab's chain (`selecting.rs`).
    flow: selecting::Flow,
    /// What a window handed over that `pace` kept back for the frames after the one it came in.
    held: Vec<Event>,
    /// The Keyboard Shortcuts dialog is open (KS); the Manage tab drops its Project Units menu open
    /// the next time it shows (UN).
    show_shortcuts: bool,
    pub custom_shortcuts: BTreeMap<String, String>,
    pub shortcuts_dialog: shortcuts::ShortcutsDialog,
    open_units: bool,
    /// Where the Options Bar was drawn last frame, for a view's navigation bar to keep clear of it (nothing when it isn't there).
    options_rect: Rect,
    canvas: canvas::Canvas,
    orbit: orbit::Orbit,
    /// Which 3D view the 3D tab shows: a view's id, or none for the default `{3D}` (made when it is first changed). Set by
    /// opening a camera view (`open_3d`), as Revit's 3D views each have their own tab.
    active_3d: Option<Id>,
    /// The graphics card the 3D view may be drawn on (Settings > Performance).
    card: gpu::Card,
    cut: section::Cut,
    minimap: minimap::Minimap,
    paper: paper::Papers,
    /// The model as last opened or saved: any other is unsaved.
    saved: Arc<Model>,
    autosave: autosave::State,
    /// The palette field being typed in: element, field, text so far.
    edit: Option<(Id, String, String)>,
    /// The Level tool's elevation and name, as typed in the Options Bar.
    new_level: (String, String),
    /// The last letters typed over the plan, for two-letter shortcuts.
    typed: String,
    pending: Option<Pending>,
    type_dialog: Option<properties::TypeDialog>,
    materials_dialog: Option<properties::MaterialsDialog>,
    family_dialog: Option<properties::FamilyDialog>,
    /// The Favorites window, while it's open; where the favorites file is, when the app was told (else the usual place).
    favorites_dialog: Option<favorites::FavoritesDialog>,
    /// The Family Editor, while it's open.
    family_editor: Option<editor::FamilyEditor>,
    favorites_path: Option<std::path::PathBuf>,
    /// The files File > Open Recent lists, and where they are kept (see [`BimApp::keep_recent_in`]; none until the app is told).
    recent: recent::Recent,
    recent_path: Option<std::path::PathBuf>,
    /// The start page: up when the window opens on no project, and from the quick access bar's Home.
    home: start::Home,
    parameters_dialog: Option<properties::ParametersDialog>,
    schedule_dialog: Option<properties::ScheduleDialog>,
    /// Visibility/Graphics, Filters and View Templates, while they're open.
    vg_dialog: Option<graphics::VgDialog>,
    filters_dialog: Option<graphics::FiltersDialog>,
    templates_dialog: Option<templates::TemplatesDialog>,
    /// Phasing and Design Options, while they're open, and the design option (`set: option`) new
    /// elements are made in: none is the main model.
    phasing_dialog: Option<phasing::PhasingDialog>,
    /// Graphic Display Options (GD), while open.
    display_dialog: Option<display::DisplayDialog>,
    /// Structure > Reinforce, while open, and what was typed in it for each kind of host.
    reinforce_dialog: Option<reinforce::ReinforceDialog>,
    reinforce_kept: BTreeMap<reinforce::Form, BTreeMap<&'static str, String>>,
    /// Manage > Line Weights, while open.
    line_weights_dialog: Option<lineweights::LineWeightsDialog>,
    /// Manage > Object Styles, while open.
    object_styles_dialog: Option<objectstyles::ObjectStylesDialog>,
    /// Manage > Line Styles, while open.
    line_styles_dialog: Option<linestyles::LineStylesDialog>,
    /// View > Title Blocks, while open.
    title_blocks_dialog: Option<titleblocks::TitleBlocksDialog>,
    fill_patterns_dialog: Option<fillpatterns::FillPatternsDialog>,
    /// Sketch mode: the lines of an outline being drawn, until Finish or Cancel.
    pub sketch: Option<sketch::Sketch>,
    /// The project the window is on (its `uid`, which a project keeps for life): one put in the session's place meanwhile is
    /// another, and everything held of the first (a sketch, a dialog's draft, the views open) is of nothing.
    project: u64,
    /// How wide the ribbon's Mode panel was last drawn, to stand it at the ribbon's right end.
    mode_width: f32,
    /// Manage > Technical Standards, while open.
    standards_dialog: Option<standards::StandardsDialog>,
    /// The plugins of the user's folder, the tabs they put on the ribbon, and the Plugins window while it is open.
    #[cfg(feature = "plugins")]
    pub(crate) plugins: bimcraft_plugin::Plugins,
    #[cfg(feature = "plugins")]
    pub(crate) plugin_tabs: Vec<bimcraft_plugin::RibbonTab>,
    #[cfg(feature = "plugins")]
    pub(crate) plugins_dialog: bool,
    /// A report a plugin's function asked the window to show, while it is open.
    #[cfg(feature = "plugins")]
    pub(crate) plugin_report: Option<plugins::PluginReport>,
    /// A plugin's function whose path the user is choosing in the system's dialog: its command, and the parameters with
    /// the path left to set. Running a function is one click on the ribbon, and a function that needs a file asks for
    /// it there.
    #[cfg(feature = "plugins")]
    pub(crate) plugin_pick: Option<(String, serde_json::Map<String, serde_json::Value>)>,
    /// The folder being read on a thread of its own, so the first frame isn't kept waiting by it.
    #[cfg(feature = "plugins")]
    plugins_job: Option<std::sync::mpsc::Receiver<bimcraft_plugin::Plugins>>,
    /// Manage > Electrical Settings, while open.
    electrical_dialog: Option<electrical::ElectricalDialog>,
    options_dialog: Option<phasing::OptionsDialog>,
    purge_dialog: Option<purge::PurgeDialog>,
    /// Manage > Select by ID and IDs of Selection, while open.
    select_by_id: Option<inquiry::SelectById>,
    /// Go To View, as Find Referring Views opened it.
    go_to_view: Option<context::GoToView>,
    /// The selected wall or run whose end grip the last right click landed on, and the view it was in: the menu offers its
    /// join there.
    menu_end: Option<(Sheet, Id)>,
    /// The selection as last seen, and the one before it, which Select Previous brings back.
    selection_seen: Vec<Id>,
    selection_before: Vec<Id>,
    /// The Work Plane dialog's draft, while it's open or picking its points in the 3D view.
    work_plane: Option<workplane::Draft>,
    ids_of_selection: Option<inquiry::IdsOfSelection>,
    /// Manage > Review Warnings, while open.
    warnings_dialog: Option<warnings::WarningsDialog>,
    selection_filter: Option<filter::SelectionFilter>,
    /// Paste Aligned to Selected Levels: the levels dialog, while open.
    paste_levels: Option<clipboard::PasteLevels>,
    /// The lines a person set the witness lines of each wall's temporary dimensions to, for the session as Revit keeps them.
    witnesses: std::collections::BTreeMap<Id, bimcraft_core::tempdims::Witness>,
    /// Manage > Temporary Dimensions, while open.
    temp_dim_dialog: Option<tempdimprops::TempDimDialog>,
    paste_views: Option<clipboard::PasteViews>,
    /// Delete on a type with instances in the Project Browser: Revit's warning with their number, while open.
    delete_type: Option<browser::DeleteType>,
    /// What was last put on the clipboard.
    pub copied: Option<String>,
    active_option: Option<String>,
    new_schedule: Option<properties::NewSchedule>,
    /// The material the Paint tool lays; the first of the library's while it isn't one it has.
    paint_material: String,
    /// The family and type the Component tool places, as picked, and how far it's turned, degrees.
    component: Option<(String, String)>,
    component_angle: f64,
    /// The hand and facing Space has stepped the doors or windows being placed to, on top of the cursor's side of the wall; the
    /// next one put down keeps them.
    place_flip: flip::PlaceFlip,
    /// Whether Space has put the exterior side of the walls being drawn on the right of the way they are drawn.
    wall_flipped: bool,
    /// The Properties palette's folded groups.
    collapsed: BTreeSet<&'static str>,
    /// Where views show, last frame.
    view_rect: Rect,
    /// Where the last view drawn showed, whether it was the active view or not.
    pane: Rect,
    /// What the Project Browser keeps to, and its search.
    browser_tab: browser::Tab,
    browser_find: String,
    /// Unsaved changes were dealt with: the window may close.
    closing: bool,
    title: String,
    styled: bool,
    /// The lettering a hosting page sets its text in (`host.rs`), which the window wears; and whether it was given since the
    /// window last set its fonts.
    page_fonts: Option<style::PageFonts>,
    refont: bool,
    /// The updater, which the desktop app attaches: a window starts without one, and so never phones home.
    #[cfg(feature = "updater")]
    pub updates: Option<bimcraft_update::Updater>,
    /// The 3D mouse's reader, which the desktop app attaches: a window starts without one, so a test never opens a device.
    pub space_mouse: Option<spacemouse::SpaceMouse>,
    /// The reader of Revit files, which the desktop app attaches (`bimcraft-rvt` is Apache-2.0 only, so this crate never depends on
    /// it): a window starts without one, so it offers no `.rvt` and a test never reads a real one.
    pub revit: Option<revit::Importer>,
    /// The control port, when the window was started with one or Settings opened it: agents work on this project through it.
    control: Option<control::Control>,
    /// File > Settings > Agents: whether the window opens the port, on which, and whether agents may edit or only read.
    agents: control::Agents,
    /// The Revit file being read, on a thread of its own.
    revit_job: Option<revit::Job>,
    /// Files to open at the next frame: the command line's, and what the system asked for while a Revit file was being read.
    opens: Vec<PathBuf>,
    /// The file dialog that is open, if one is: it is on a thread of its own, and answers in a later frame.
    dialog: Option<dialog::Dialog>,
    /// What the last Revit import left out and brought in, for as long as its project is the one open.
    revit_report: Option<revit::Report>,
    /// The window is inside a page that keeps the project (Arqiflow's editor, `host.rs`): no files, start page, updater,
    /// plugins or control port, and Save asks the page.
    hosted: bool,
    /// The hosting page's colours over the look (`style::Skin`).
    skin: style::Skin,
    /// Save was asked for while hosted: the page takes it ([`BimApp::take_save_request`]).
    save_asked: bool,
    /// A 3D view the person asked to render while hosted: the page takes it and opens its Render dialog
    /// ([`BimApp::take_render_request`]).
    render_asked: Option<host::RenderRequest>,
}

/// The Options Bar's Height: up to a level above the plan's `level` (by default the next one up),
/// or Unconnected, with a field for the height. `top` and `height` are the tool's own.
fn height_options(ui: &mut Ui, m: &Model, level: Id, top: &mut Option<Option<Id>>, height: &mut String, salt: &str) {
    let z = m.elevation(level);
    let above: Vec<(Id, &str)> = m.levels().into_iter().filter(|(_, l)| l.elevation > z).map(|(id, l)| (id, l.name.as_str())).collect();
    let now = top.unwrap_or_else(|| above.first().map(|(id, _)| *id));
    let current = now.and_then(|id| m.level(id)).map_or(t("Unconnected"), |l| l.name.as_str());
    let label = ui.label(t("Height:"));
    let combo = crate::icons::combo(salt).selected_text(current).width(130.0).show_ui(ui, |ui| {
        for (id, name) in &above {
            if ui.selectable_label(now == Some(*id), *name).clicked() {
                *top = Some(Some(*id));
            }
        }
        if ui.selectable_label(now.is_none(), t("Unconnected")).clicked() {
            *top = Some(None);
        }
    });
    combo.response.labelled_by(label.id);
    if now.is_none() {
        ui.add(TextEdit::singleline(height).desired_width(64.0).hint_text(m.units.name())).labelled_by(label.id);
    }
}

/// The params for the top an Options Bar set from the plan of `level`: up to a level, or
/// unconnected at a height.
fn top_params(m: &Model, level: Id, top: Option<Option<Id>>, height: &str) -> Result<Value, String> {
    match top.unwrap_or_else(|| m.level_above(level).and_then(|g| m.elements.id(g))) {
        Some(t) => Ok(json!({"top_level": t})),
        None => m.units.parse(height).map(|h| json!({"height": h})).ok_or_else(|| tf("`{}` is not a height", &[height])),
    }
}

/// A menu's item that opens a menu of its own, with an arrow that IBM Plex has: egui's own (`⏵`) is only in egui's fonts,
/// which a page's window is built without.
pub(crate) fn submenu(label: &str) -> SubMenuButton<'_> {
    SubMenuButton::from_button(egui::Button::new(label).right_text("›"))
}

/// A ribbon button: an icon over its label, tinted while its tool is on; as wide as its label, which breaks over two lines
/// when one would make it wider than `WIDE`.
fn big_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    ribbon_button(ui, icon, label, active, tip, true)
}

/// A ribbon button whose label is a plugin's data: it isn't translated, as its manifest wrote it, and it has no icon
/// (icons come later, from a limited set of glyphs in the manifest).
#[cfg(feature = "plugins")]
fn plugin_button(ui: &mut Ui, label: &str, tip: &str) -> Response {
    const WIDE: f32 = 78.0;
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    if ribbon::listing() {
        return plugin_row(ui, label, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (label.to_string(), t(tip));
    let font = FontId::proportional(12.0);
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), pal().text).size().x;
    let one = wide(&label);
    let lines: Vec<String> = match label.char_indices().filter(|(_, c)| *c == ' ').min_by_key(|(i, _)| i.abs_diff(label.len() / 2)) {
        Some((at, _)) if one + 12.0 > WIDE => vec![label[..at].to_string(), label[at + 1..].to_string()],
        _ => vec![label.clone()],
    };
    let width = lines.iter().map(|l| wide(l)).fold(0.0, f32::max) + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(54.0), 60.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, false, &label));
    let (fill, stroke, ink) = match r.hovered() && enabled {
        true => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().text),
        false => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().text } else { pal().muted.gamma_multiply(0.6) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    ui.painter().rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    let top = rect.center_top() + vec2(0.0, 30.0 - 6.5 * (lines.len().saturating_sub(1)) as f32);
    for (i, line) in lines.iter().enumerate() {
        ui.painter().text(top + vec2(0.0, 13.0 * i as f32), Align2::CENTER_CENTER, line, font.clone(), ink);
    }
    r.on_hover_text(tip)
}

/// A plugin's button in its panel's list: its row is text alone, as its ribbon button has no icon.
#[cfg(feature = "plugins")]
fn plugin_row(ui: &mut Ui, label: &str, tip: &str) -> Response {
    let width = ui.available_width().max(200.0);
    let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, false, label));
    let (fill, ink) = match r.hovered() && enabled {
        true => (pal().hover_fill, pal().text),
        false => (Color32::TRANSPARENT, if enabled { pal().text } else { pal().muted.gamma_multiply(0.6) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { Stroke::NONE };
    let p = ui.painter();
    p.rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
    p.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.5), ink);
    r.on_hover_text(t(tip))
}

fn ribbon_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str, translate: bool) -> Response {
    const WIDE: f32 = 78.0;
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    if ribbon::listing() {
        return list_row_in(ui, icon, label, active, tip, translate);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = if translate { (t(label).to_string(), t(tip)) } else { (label.to_string(), t(tip)) };
    let font = FontId::proportional(12.0);
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), pal().text).size().x;
    let one = wide(&label);
    // The space nearest the middle, for a label too wide for one line.
    let lines: Vec<String> = match label.char_indices().filter(|(_, c)| *c == ' ').min_by_key(|(i, _)| i.abs_diff(label.len() / 2)) {
        Some((at, _)) if one + 12.0 > WIDE => vec![label[..at].to_string(), label[at + 1..].to_string()],
        _ => vec![label.to_string()],
    };
    let width = lines.iter().map(|l| wide(l)).fold(0.0, f32::max) + 12.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(54.0), 60.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, &label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent_ink),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    // Keyboard focus shows as a ring, whatever else the button shows.
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    let p = ui.painter();
    p.rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    icons::paint(p, icon, Rect::from_center_size(rect.center_top() + vec2(0.0, 18.0), vec2(26.0, 26.0)), ink);
    let text = match (active, enabled) {
        (true, _) => pal().accent_ink,
        (false, true) => pal().text,
        (false, false) => pal().muted.gamma_multiply(0.6),
    };
    for (i, line) in lines.iter().enumerate() {
        p.text(rect.center_top() + vec2(0.0, 40.0 + 13.0 * i as f32), Align2::CENTER_CENTER, line, font.clone(), text);
    }
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// A small ribbon button: an icon alone, as in Revit's Modify panel; its name in its tooltip.
fn small_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let (rect, r) = ui.allocate_exact_size(vec2(30.0, 25.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let (fill, stroke, ink) = match (active, r.hovered() && enabled) {
        (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent), pal().accent_ink),
        (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover), pal().icon),
        (false, false) => (Color32::TRANSPARENT, Stroke::NONE, if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) }),
    };
    let stroke = if r.has_focus() { style::focus_ring() } else { stroke };
    ui.painter().rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(19.0, 19.0)), ink);
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// A ribbon button in its panel's list: a row of its icon and its name.
fn list_row(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    list_row_in(ui, icon, label, active, tip, true)
}

fn list_row_in(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str, translate: bool) -> Response {
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    let (label, tip) = if translate { (t(label).to_string(), t(tip)) } else { (label.to_string(), t(tip)) };
    // A panel's list is at least a menu wide.
    let width = ui.available_width().max(200.0);
    let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, &label));
    let fill = match (active, r.hovered() && enabled) {
        (true, _) => pal().select_fill,
        (false, true) => pal().hover_fill,
        (false, false) => Color32::TRANSPARENT,
    };
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    if r.has_focus() {
        p.rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
    let ink = if active { pal().accent_ink } else { pal().icon };
    icons::paint(p, icon, Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0)), dim(ink));
    let text = if active { pal().accent_ink } else { pal().text };
    p.text(rect.left_center() + vec2(38.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), dim(text));
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// In Edit Tools a tool's button or row is a box to tick, what the ribbon shows: a click ticks or clears it and
/// takes no tool. `name` is its English name, which the settings file keeps.
fn edit_tool(ui: &Ui, r: Response, rect: Rect, name: &str) -> Response {
    let shown = !ribbon::tool_hidden(name);
    let label = t(name);
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, shown, label));
    let p = ui.painter();
    if !shown {
        p.rect_filled(rect, 6.0, pal().panel.gamma_multiply(0.7));
    }
    let tick = Rect::from_min_size(pos2(rect.right() - 15.0, rect.top() + 3.0), vec2(12.0, 12.0));
    p.rect(tick, 2.0, if shown { pal().accent } else { pal().surface }, Stroke::new(1.0, pal().accent_ink), StrokeKind::Inside);
    if shown {
        let ends = vec![tick.left_center() + vec2(2.5, 0.0), tick.center_bottom() - vec2(1.0, 3.0), tick.right_top() + vec2(-2.5, 3.0)];
        p.line(ends, Stroke::new(1.5, Color32::WHITE));
    }
    if r.clicked() {
        ribbon::toggle(name);
    }
    ui.interact(rect, r.id.with("edit-tool"), Sense::hover()).on_hover_text(label)
}

/// Buttons stacked three high in a panel, as Revit's Settings panel stacks them; in the panel's list they are rows like any other.
pub(crate) fn stacked(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        if !ribbon::listing() {
            ui.spacing_mut().item_spacing.y = 2.0;
        }
        add(ui);
    });
}

/// A button stacked three high in a panel (see [`stacked`]): its icon and its name in a row.
pub(crate) fn stacked_button(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    if ribbon::left_out(label) {
        return ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    }
    let name = label;
    if ribbon::listing() {
        return list_row(ui, icon, label, active, tip);
    }
    if let Some(r) = ribbon::handed_over(ui) {
        return r;
    }
    let (label, tip) = (t(label), t(tip));
    let font = FontId::proportional(12.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size().x + 28.0;
    let (rect, r) = ui.allocate_exact_size(vec2(width, 16.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let fill = match (active, r.hovered() && enabled) {
        (true, _) => pal().select_fill,
        (false, true) => pal().hover_fill,
        (false, false) => Color32::TRANSPARENT,
    };
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
    let (ink, text) = if active { (pal().accent_ink, pal().accent_ink) } else { (pal().icon, pal().text) };
    ui.painter().rect_filled(rect, 3.0, fill);
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 3.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(14.0, 14.0)), dim(ink));
    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, label, font, dim(text));
    if ribbon::editing_tools() {
        return edit_tool(ui, r, rect, name);
    }
    r.on_hover_text(tip)
}

/// A small icon button, named for screen readers and tests.
fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let (ink, hover) = style::icon_inks();
    if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, style::focus_ring(), StrokeKind::Inside);
    }
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), ink);
    r.on_hover_text(label)
}

/// An undo step as Undo's list names it: what it made (Create Walls) or took away (Delete Door), or else its command.
fn step_name(st: &engine::Step) -> String {
    let one = |list: &[(&str, usize)]| match list {
        [(c, n)] => Some(lang::category(c, *n > 1)),
        _ => None,
    };
    let named = match (st.made.as_slice(), st.removed.as_slice()) {
        (made, []) if !made.is_empty() => one(made).map(|c| tf("Create {}", &[&c])),
        ([], removed) if !removed.is_empty() => one(removed).map(|c| tf("Delete {}", &[&c])),
        _ => None,
    };
    named.unwrap_or_else(|| t(&st.label).to_string())
}

/// A tab of the ribbon. Contextual tabs are green, as Revit's are (a hosting page's accent where it gives one).
fn tab_button(ui: &mut Ui, label: &str, active: bool, contextual: bool) -> Response {
    tab_button_with(ui, label, active, contextual, Sense::click(), 0.0)
}

/// A tab that senses what `sense` says (a view's drags too), `room` px wider on its right for what's drawn in it (a view's ×).
fn tab_button_with(ui: &mut Ui, label: &str, active: bool, contextual: bool, sense: Sense, room: f32) -> Response {
    let label = t(label);
    let font = if active { style::semibold(13.0) } else { FontId::proportional(13.0) };
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(vec2(size.x + 24.0 + room, 24.0), sense);
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    // A pill: the open tab in the accent's soft tint, a contextual one solid green when open and tinted green when not.
    let (fill, stroke, ink) = match (contextual, active, r.hovered()) {
        (true, true, _) => (pal().context, Stroke::NONE, pal().on_context),
        (true, false, _) => (pal().context_fill, Stroke::new(1.0, pal().context.gamma_multiply(0.5)), pal().context_ink),
        (false, true, _) => (pal().select_fill, Stroke::NONE, pal().accent_ink),
        (false, false, true) => (pal().hover_fill, Stroke::NONE, pal().text),
        (false, false, false) => (Color32::TRANSPARENT, Stroke::NONE, pal().text),
    };
    let p = ui.painter();
    let radius = CornerRadius::same(6);
    p.rect(rect, radius, fill, stroke, StrokeKind::Inside);
    if r.has_focus() {
        p.rect_stroke(rect, radius, style::focus_ring(), StrokeKind::Inside);
    }
    p.text(rect.center() - vec2(room / 2.0, 0.0), Align2::CENTER_CENTER, label, font, ink);
    r
}

/// How tall a card floating over a view is (its tabs, its view control bar), points.
const FLOATING_CARD: f32 = 30.0;

/// The ribbon's tabs, in order (the contextual tab comes after them).
const TABS: [(Tab, &str); 12] = [
    (Tab::Architecture, msg!("Architecture")),
    (Tab::Structure, msg!("Structure")),
    (Tab::Plumbing, msg!("Plumbing")),
    (Tab::Electrical, msg!("Electrical")),
    (Tab::Lightning, msg!("Lightning Protection")),
    (Tab::Data, msg!("Data")),
    (Tab::Insert, msg!("Insert")),
    (Tab::Annotate, msg!("Annotate")),
    (Tab::Massing, msg!("Massing & Site")),
    (Tab::View, msg!("View")),
    (Tab::Manage, msg!("Manage")),
    (Tab::Modify, "Modify"),
];

impl BimApp {
    pub fn new(session: Session) -> BimApp {
        let first = Sheet::Plan(session.model.levels().first().map_or(0, |(id, _)| *id));
        let units = session.model.units;
        BimApp {
            saved: Arc::clone(&session.model),
            project: session.model.uid,
            autosave: autosave::State::default(),
            session,
            os_events: None,
            own_title_bar: false,
            system_title_bar: false,
            sheet: first,
            tool: Tool::Modify,
            selection: Vec::new(),
            status: Status::default(),
            warned: Vec::new(),
            ui_zoom: 1.0,
            views: vec![first],
            front: first,
            floating: Vec::new(),
            passive: false,
            docking: dock::Docking::default(),
            checks: checks::Checks::default(),
            dragging: None,
            drop_headers: Vec::new(),
            tab_bar: Rect::NOTHING,
            tabs_foot: f32::NEG_INFINITY,
            tab_rects: Vec::new(),
            work: Rect::ZERO,
            pointer: Pos2::ZERO,
            grab: egui::Vec2::ZERO,
            ribbon_min: false,
            compact: false,
            host_compact: None,
            scheme: style::Scheme::default(),
            accent: None,
            panel_colour: None,
            canvas_colour: None,
            text_colour: None,
            palette_filter: None,
            palette_json: RefCell::new(None),
            ribbon_peek: false,
            scan_contrast: footer::HIGH_CONTRAST,
            background_grid_dialog: None,
            grid_snapping: false,
            show_origin: true,
            show_view_name: true,
            ribbon_bottom: Pos2::ZERO,
            settings_page: None,
            find: None,
            scale: presets::Scale::default(),
            tools_hidden: BTreeSet::new(),
            editing_tools: false,
            presets: Vec::new(),
            preset_name: String::new(),
            preset_note: None,
            settings_path: None,
            settings_kept: None,
            window: window::Tracker::default(),
            theme_wanted: None,
            light_canvas: false,
            tab: Tab::Architecture,
            back: Tab::Architecture,
            types: BTreeMap::new(),
            opening_families: BTreeMap::new(),
            wall_top: None,
            wall_height: units.length(3000.0),
            column_top: None,
            column_height: units.length(3000.0),
            floor_structural: false,
            run_justify: Justify::Top,
            beam_offset: units.length(0.0),
            run_kept: BTreeMap::new(),
            run_loaded: "beam",
            footing_thickness: units.length(400.0),
            footing_top: units.length(0.0),
            stair_width: units.length(STAIR_WIDTH),
            roof_slope: "30".into(),
            scale_text: String::new(),
            roof_overhang: units.length(500.0),
            roof_gable: false,
            roof_shed: false,
            dormer_width: units.length(1600.0),
            dormer_height: units.length(1200.0),
            dormer_window: true,
            railings: true,
            ramp_top: Some(None),
            ramp_height: units.length(600.0),
            ramp_width: units.length(RAMP_WIDTH),
            ramp_slope: "8.33".into(),
            ceiling_height: units.length(CEILING_HEIGHT),
            floor_slope: "5".into(),
            shape_z: String::new(),
            shape_base: crate::shape::Base::default(),
            shape_along: true,
            shape_offset: "0".into(),
            shape_grip: None,
            shape_preview: None,
            shape_label_edit: None,
            shape_picked: Vec::new(),
            shaft_top: None,
            shaft_height: units.length(3000.0),
            mass_top: None,
            mass_height: units.length(3000.0),
            site: site::SiteChoices::default(),
            site_ghost: Default::default(),
            site_grip: None,
            camera_grip: None,
            camera_section_grip: None,
            camera_eye_height: units.length(1600.0),
            camera_target_height: units.length(1200.0),
            camera_lens: "35".into(),
            chain: true,
            wall_location: Location::Centre,
            attach_base: false,
            thin: false,
            constrain: false,
            multiple: false,
            angle: String::new(),
            rotate_copy: false,
            array: array::Options::default(),
            mirror_copy: true,
            offset: units.length(1000.0),
            offset_copy: true,
            tag_leader: false,
            tag_label: String::new(),
            note_text: String::new(),
            note_edit: None,
            text_type_dialog: None,
            dimension_type_dialog: None,
            dimension_text_dialog: None,
            line_style: String::new(),
            linework_style: String::new(),
            group_type: String::new(),
            repeat_spacing: "200".into(),
            line_chain: true,
            drawing: draw::Drawing::default(),
            units,
            lang: Lang::English,
            font_finder: lang::cjk_fonts,
            typed_length: String::new(),
            dim_edit: None,
            temporary: HashMap::new(),
            shadow: None,
            linked_3d: None,
            seen: None,
            link_dialog: false,
            cloud_dialog: false,
            cloud_box: None,
            cad_dialog: None,
            cad_box: None,
            image_dialog: None,
            pdf_box: None,
            placing: None,
            pdf_reader: Default::default(),
            images_window: false,
            images_picked: None,
            scale_length: String::new(),
            tab_cycle: false,
            flow: selecting::Flow::default(),
            held: Vec::new(),
            show_shortcuts: false,
            custom_shortcuts: BTreeMap::new(),
            shortcuts_dialog: shortcuts::ShortcutsDialog::default(),
            angle_edit: None,
            snaps: Snaps::default(),
            show_snaps: false,
            snap_once: None,
            measured: None,
            open_units: false,
            options_rect: Rect::NOTHING,
            canvas: canvas::Canvas::default(),
            orbit: orbit::Orbit::default(),
            active_3d: None,
            card: gpu::Card::default(),
            cut: section::Cut::default(),
            minimap: minimap::Minimap::default(),
            paper: paper::Papers::default(),
            edit: None,
            new_level: (String::new(), String::new()),
            typed: String::new(),
            pending: None,
            type_dialog: None,
            materials_dialog: None,
            family_dialog: None,
            favorites_dialog: None,
            family_editor: None,
            favorites_path: None,
            recent: recent::Recent::default(),
            recent_path: None,
            home: start::Home::default(),
            paint_material: String::new(),
            parameters_dialog: None,
            schedule_dialog: None,
            vg_dialog: None,
            filters_dialog: None,
            templates_dialog: None,
            phasing_dialog: None,
            display_dialog: None,
            reinforce_dialog: None,
            reinforce_kept: BTreeMap::new(),
            line_weights_dialog: None,
            object_styles_dialog: None,
            line_styles_dialog: None,
            title_blocks_dialog: None,
            fill_patterns_dialog: None,
            sketch: None,
            mode_width: 0.0,
            standards_dialog: None,
            #[cfg(feature = "plugins")]
            plugins: bimcraft_plugin::Plugins::empty(),
            #[cfg(feature = "plugins")]
            plugin_tabs: Vec::new(),
            #[cfg(feature = "plugins")]
            plugins_dialog: false,
            #[cfg(feature = "plugins")]
            plugin_report: None,
            #[cfg(feature = "plugins")]
            plugin_pick: None,
            #[cfg(feature = "plugins")]
            plugins_job: None,
            electrical_dialog: None,
            options_dialog: None,
            purge_dialog: None,
            select_by_id: None,
            go_to_view: None,
            menu_end: None,
            selection_seen: Vec::new(),
            selection_before: Vec::new(),
            work_plane: None,
            ids_of_selection: None,
            warnings_dialog: None,
            selection_filter: None,
            paste_levels: None,
            witnesses: std::collections::BTreeMap::new(),
            temp_dim_dialog: None,
            paste_views: None,
            delete_type: None,
            copied: None,
            active_option: None,
            new_schedule: None,
            component: None,
            component_angle: 0.0,
            place_flip: flip::PlaceFlip::default(),
            wall_flipped: false,
            collapsed: BTreeSet::new(),
            view_rect: Rect::ZERO,
            pane: Rect::ZERO,
            browser_tab: browser::Tab::All,
            browser_find: String::new(),
            closing: false,
            title: String::new(),
            styled: false,
            page_fonts: None,
            refont: false,
            #[cfg(feature = "updater")]
            updates: None,
            space_mouse: None,
            revit: None,
            control: None,
            agents: control::Agents::default(),
            revit_job: None,
            opens: Vec::new(),
            dialog: None,
            revit_report: None,
            hosted: false,
            skin: style::Skin::default(),
            save_asked: false,
            render_asked: None,
        }
    }

    /// Whether the model changed since it was last opened or saved.
    pub fn dirty(&self) -> bool {
        !Arc::ptr_eq(&self.saved, &self.session.model)
    }

    /// Where model point `p` shows on screen, on the plan drawn last.
    pub fn screen(&self, p: P2) -> Pos2 {
        self.canvas.screen(p)
    }

    /// Run a command; when it fails, say why.
    fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        self.made_in();
        match self.session.execute(id, &params) {
            Ok(v) => {
                self.status = Status::default();
                self.warned.clear();
                // The core renames and drops the option with its set.
                self.active_option = self.session.option.clone();
                Some(v)
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                None
            }
        }
    }

    /// The contextual tab's name while a tool is on or something is selected.
    fn context(&self) -> Option<String> {
        if let Some(sk) = &self.sketch {
            return Some(format!("{} | {}", t("Modify"), t(sk.title())));
        }
        let place = match self.tool {
            Tool::Modify
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Paste
            | Tool::PasteLevel
            | Tool::MoveWitness { .. }
            | Tool::Rotate { .. }
            | Tool::Array { .. }
            | Tool::Mirror { .. }
            | Tool::Align(_)
            | Tool::Trim(_)
            | Tool::TrimTo { .. }
            | Tool::Vertex { .. }
            | Tool::Split
            | Tool::Offset
            | Tool::Demolish
            | Tool::Linework
            | Tool::Attach
            | Tool::GableEdge
            | Tool::SlopeArrow(_)
            | Tool::ModifySubElements
            | Tool::AddPoint
            | Tool::AddSplitLine(_)
            | Tool::SketchLine(_)
            | Tool::SketchPick
            | Tool::PatternCorner(_) => None,
            Tool::Wall(_) => Some(msg!("Place Wall")),
            Tool::Paint => Some("Paint"),
            Tool::Unpaint => Some("Remove Paint"),
            Tool::Door => Some(msg!("Place Door")),
            Tool::Window => Some(msg!("Place Window")),
            Tool::Room => Some(msg!("Place Room")),
            Tool::RoomSeparator(_) => Some(msg!("Place Room Separation")),
            Tool::Level => Some(msg!("Place Level")),
            Tool::Grid(_) => Some(msg!("Place Grid")),
            Tool::Dimension(..) | Tool::Angular(_) | Tool::Round(..) => Some(msg!("Place Dimensions")),
            Tool::Text => Some(msg!("Place Text")),
            Tool::PlaceImage => Some(msg!("Place Image")),
            Tool::Scale(_) => Some("Scale"),
            Tool::DetailGroup => Some(msg!("Place Detail Group")),
            Tool::RepeatingDetail(_) => Some(msg!("Place Repeating Detail")),
            Tool::DetailLine(_) if self.drawing.insulation.is_some() => Some(msg!("Place Insulation")),
            Tool::DetailLine(_) if self.drawing.cloud.is_some() => Some(msg!("Place Revision Cloud")),
            Tool::DetailLine(_) => Some(msg!("Place Detail Lines")),
            Tool::Spot => Some("Spot Elevation"),
            Tool::SpotSlope => Some(msg!("Spot Slope")),
            Tool::Measure(_) => Some("Measure"),
            Tool::Tag => Some("Tag"),
            Tool::Section(_) => Some("Section"),
            Tool::Elevation => Some("Elevation"),
            Tool::Camera(_) => Some("Camera"),
            Tool::Column => Some(msg!("Place Column")),
            Tool::Run(..) | Tool::Stand(_) => structure::title(&self.tool),
            Tool::Component => Some(msg!("Place Component")),
            Tool::Fixture(key) => mep::fixture(key).map(|f| f.title),
            Tool::Stair(_) => Some(msg!("Create Stair")),
            Tool::Ramp(_) => Some(msg!("Create Ramp")),
            Tool::Railing(_) => Some(msg!("Create Railing Path")),
            Tool::Roof => Some(msg!("Create Roof Footprint")),
            Tool::ExtrusionRoof(_) => Some(msg!("Create Extrusion Roof")),
            Tool::Dormer => Some("Dormer"),
            Tool::Floor => Some(msg!("Create Floor")),
            Tool::Ceiling => Some(msg!("Place Ceiling")),
            Tool::Shaft(_) => Some(msg!("Create Shaft Opening")),
            Tool::Mass(_) => Some(msg!("Create Mass")),
            Tool::Site(t) => Some(t.title()),
        };
        if let Some(what) = place {
            return Some(format!("{} | {}", t("Modify"), t(what)));
        }
        let m = &self.session.model;
        let categories: BTreeSet<&str> = self.selection.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
        match categories.len() {
            // Revit's mode has a tab of its own while nothing is selected.
            0 if self.revealing() => Some(format!("{} | {}", t("Modify"), t("Reveal Hidden Elements"))),
            0 => None,
            1 => categories.first().map(|c| format!("{} | {}", t("Modify"), lang::category(c, true))),
            _ => Some(format!("{} | {}", t("Modify"), t("Multi-Select"))),
        }
    }

    /// Bring the contextual tab up, as Revit does for a tool or a selection.
    fn raise_context(&mut self) {
        if self.tab != Tab::Context {
            self.back = self.tab;
            self.tab = Tab::Context;
        }
    }

    fn view_name(&self, sheet: Sheet) -> String {
        match sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) => self.session.model.view_name(key).map_or_else(|| "Floor Plan".into(), String::from),
            Sheet::View3D => {
                self.active_3d().and_then(|id| self.session.model.view_name(id).map(String::from)).unwrap_or_else(|| "{3D}".into())
            }
            Sheet::Section(id) => self.session.model.section(id).map_or_else(|| "Section".into(), |v| v.name.clone()),
            Sheet::Paper(id) => self.session.model.sheet(id).map_or_else(|| "Sheet".into(), |s| format!("{} - {}", s.number, s.name)),
            Sheet::Schedule(cat) => match schedule::standard(&self.session.model, cat) {
                (Some(_), sc) => sc.name,
                (None, _) => t(SCHEDULES.iter().find(|(c, _)| *c == cat).map_or(cat, |(_, name)| name)).to_string(),
            },
            Sheet::Table(id) => match self.session.model.elements.get(id) {
                Some(Element::Schedule(sc)) => sc.name.clone(),
                _ => "Schedule".into(),
            },
        }
    }

    /// The view as the title bar names it: `Floor Plan: Level 1`.
    fn view_title(&self, sheet: Sheet) -> String {
        let kind = match sheet {
            Sheet::Plan(_) => "Floor Plan",
            Sheet::Ceiling(_) => "Ceiling Plan",
            Sheet::View3D => "3D View",
            Sheet::Section(id) if matches!(self.session.model.elements.get(id), Some(Element::Elevation(_))) => "Elevation",
            Sheet::Section(_) => "Section",
            Sheet::Paper(_) => "Sheet",
            Sheet::Schedule(_) | Sheet::Table(_) => "Schedule",
        };
        format!("{}: {}", t(kind), self.view_name(sheet))
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.ui_zoom = ctx.zoom_factor();
        // What `pace` held is for the next frame, which a window with nothing new to say won't run.
        if !self.held.is_empty() {
            ctx.request_repaint();
        }
        if !self.styled {
            style::apply(&ctx, self.page_fonts.as_ref());
            (self.styled, self.refont) = (true, false);
        } else if std::mem::take(&mut self.refont) {
            style::refont(&ctx, self.page_fonts.as_ref());
        }
        if let Some(theme) = self.theme_wanted.take() {
            ctx.set_theme(theme);
        }
        let look = style::Look {
            light_canvas: self.light_canvas,
            compact: self.compact,
            scheme: self.scheme,
            accent: self.accent,
            panel_colour: self.panel_colour,
            canvas_colour: self.canvas_colour,
            text_colour: self.text_colour,
            skin: self.skin,
        };
        style::follow(&ctx, look);
        ribbon::tool_rules(self.tools_hidden.clone(), self.scale, self.editing_tools);
        // The fonts with the hanzi and the Hangul go behind the window's own whatever language it speaks: content — a floor's
        // name from a Korean file — is not the interface's. Chinese needs its glyphs, which the system may not have: then the
        // window stays in English and says why.
        let hanzi = lang::use_cjk_font(&ctx, self.font_finder);
        if self.lang.is_chinese() && !hanzi {
            self.lang = Lang::English;
            self.status = failed(t(lang::NO_CHINESE_FONT).to_string());
        }
        lang::set(self.lang);
        // What agents asked of the project comes before the tidying, which puts right what a command left stale.
        self.answer_agents(&ctx);
        // A project put in the session's place under the window (the control port turns an agent's New and Open away, but a
        // script or an embedder may do them) starts it over, as File > New does: its sketch and dialogs were of the old one, and
        // it gets a recovery copy of its own. (The old one's stays until that is written: nobody was asked about its changes.)
        if self.session.model.uid != self.project {
            self.reopened();
            self.autosave_document();
        }
        self.tidy();
        self.note_selection();
        #[cfg(feature = "updater")]
        self.poll_updates(&ctx);
        self.poll_revit(&ctx);
        #[cfg(feature = "plugins")]
        self.poll_plugins(&ctx);
        self.poll_autosave(&ctx);
        self.keys(&ctx);
        self.os_requests(&ctx);
        self.dialog_answered();
        self.watch_window(&ctx);
        let view = self.view_title(self.sheet);
        let title = format!("{}{} · {view} · BIMCraft", self.session.model.name, if self.dirty() { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && (self.installing() || self.dirty()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            // An update being put in place holds the window: it closes it when it's in.
            if !self.installing() {
                self.pending = Some(Pending::Close);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.dirty() && !self.installing() {
            self.retire_autosave();
        }
        if self.home.shown {
            if self.own_bar() {
                self.start_title_bar(ui);
            }
            self.start_page(ui);
            self.drag_and_drop(&ctx);
            self.warning(&ctx);
            self.dialogs(&ctx);
            self.find_window(&ctx);
            self.keep_settings(&ctx);
            if self.own_bar() {
                titlebar::frame(ui);
            }
            return;
        }
        // The view's ground under the cards: what shows in the gaps round them is the view going on beneath.
        ui.painter().rect_filled(ui.max_rect(), 0.0, self.backdrop());
        let app_bar = Frame::new().fill(pal().app_bar).inner_margin(Margin::symmetric(8, 5));
        Panel::top("quick-access").frame(app_bar).show_separator_line(false).show(ui, |ui| {
            // Where the window draws its own title bar, this is it: its ground moves the window, its corner closes it.
            let bar = ui.max_rect().expand2(vec2(8.0, 5.0));
            if let Some(rule) = style::app_bar_rule() {
                ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, rule);
            }
            let own = self.own_bar();
            if own {
                titlebar::drag_area(ui, bar);
            }
            style::on_app_bar(|| self.quick_access(ui, &view));
            if own {
                titlebar::caption_buttons(ui, bar);
            }
        });
        let card = style::card([true, true, true, false]);
        Panel::top("ribbon").frame(card).show_separator_line(false).show(ui, |ui| self.ribbon(ui));
        #[cfg(feature = "updater")]
        if self.has_update_banner() {
            Panel::top("update").frame(Frame::NONE).show_separator_line(false).show(ui, |ui| self.update_banner(ui));
        }
        let status = style::card([true, true, false, true]).inner_margin(Margin::symmetric(10, 2));
        Panel::bottom("status").frame(status).show_separator_line(false).show(ui, |ui| self.status_bar(ui));
        self.docks(ui);
        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            let g = style::gap().max(4);
            let front = self.front;
            // A schedule's table keeps its rows clear of the tabs.
            let table = matches!(self.sheet, Sheet::Schedule(_) | Sheet::Table(_));
            if style::compact() || table {
                // Compact: a strip across the top of the view, the tabs standing on its foot (Revit's document tabs).
                let strip = style::compact();
                let frame = match strip {
                    // No room over the tabs: they stand right under the ribbon.
                    true => Frame::new().fill(style::tab_strip()).inner_margin(Margin { left: 4, right: 0, top: 0, bottom: 0 }),
                    false => Frame::new().inner_margin(Margin { left: g, right: g, top: g, bottom: 4 }),
                };
                Panel::top("view-tabs").frame(frame).show_separator_line(false).show(ui, |ui| {
                    self.view_tabs(ui, strip);
                });
                self.view_pane(ui, front);
                self.tabs_foot = f32::NEG_INFINITY;
            } else {
                // A drawn view fills the pane, and its tabs float over its top left corner on a card of their own: drawn after
                // it, so they are over it for the hand too.
                let pane = ui.available_rect_before_wrap();
                ui.scope_builder(UiBuilder::new().max_rect(pane), |ui| self.view_pane(ui, front));
                let card = style::card([true, false, true, false]).inner_margin(Margin::same(2));
                // A child that takes no room of the pane's: over the view, not after it.
                let mut over = ui.new_child(UiBuilder::new().id_salt("view-tabs").max_rect(pane));
                self.tabs_foot = card.show(&mut over, |ui| self.view_tabs(ui, false)).response.rect.bottom();
            }
            self.work = self.pane.with_min_y(self.pane.top().max(self.tabs_foot));
        });
        self.options_bar(&ctx);
        self.edge_palette(&ctx);
        if self.ribbon_min && self.ribbon_peek {
            self.ribbon_peek(&ctx);
        }
        self.palette_windows(&ctx);
        self.view_windows(&ctx);
        self.drag_and_drop(&ctx);
        self.warning(&ctx);
        self.dialogs(&ctx);
        self.find_window(&ctx);
        // What Edit Tools ticked or cleared on the ribbon.
        for name in ribbon::take_toggled() {
            if !self.tools_hidden.remove(&name) {
                self.tools_hidden.insert(name);
            }
        }
        self.keep_settings(&ctx);
        self.track_sketch();
        if self.own_bar() {
            titlebar::frame(ui);
        }
    }

    /// The start page's title bar, where the window draws its own: the app bar's colour, the app's name, and the caption buttons.
    fn start_title_bar(&mut self, ui: &mut Ui) {
        Panel::top("start-title-bar").frame(Frame::new().fill(pal().app_bar)).exact_size(34.0).show_separator_line(false).show(ui, |ui| {
            let bar = ui.max_rect();
            titlebar::drag_area(ui, bar);
            if let Some(rule) = style::app_bar_rule() {
                ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, rule);
            }
            ui.painter().text(
                bar.left_center() + vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                "BIMCraft",
                FontId::proportional(12.5),
                pal().app_bar_muted,
            );
            titlebar::caption_buttons(ui, bar);
        });
    }

    /// What the active view is drawn on: paper for a drawing, the desk round a sheet, a schedule's ground.
    fn backdrop(&self) -> Color32 {
        match self.sheet {
            Sheet::Paper(_) => pal().desk,
            Sheet::Schedule(_) | Sheet::Table(_) => pal().surface,
            _ => canvas::color(render::PAPER),
        }
    }

    /// A view and its view control bar, as the active view draws it: the bar floating over its foot, or (Compact) under it.
    fn view_body(&mut self, ui: &mut Ui) {
        let floating = !style::compact();
        if !floating {
            self.view_control_bar(ui);
        }
        // What the floating tabs and view control bar take of the view's height: a tab and a bar each, and a gap round each.
        self.canvas.room = if floating { 2.0 * (FLOATING_CARD + f32::from(style::gap())) } else { 0.0 };
        self.view_rect = ui.available_rect_before_wrap();
        self.pane = self.view_rect;
        // The plan sets the cursor each frame it shows.
        self.canvas.cursor = None;
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) if self.session.model.plan_level(key).is_some() => self.canvas(ui, key),
            Sheet::Plan(_) | Sheet::Ceiling(_) => {
                ui.centered_and_justified(|ui| ui.label(t("This project has no levels. Add one with the Level tool.")));
            }
            Sheet::View3D => self.view3d(ui),
            Sheet::Section(id) => self.section_view(ui, id),
            Sheet::Paper(id) => self.paper_view(ui, id),
            Sheet::Schedule(_) | Sheet::Table(_) => self.schedule(ui),
        }
        if floating {
            self.view_control_bar(ui);
        }
        self.temp_frame(ui);
    }

    /// Properties (PP): shown, or hidden.
    fn toggle_properties(&mut self) {
        let p = dock::Palette::Properties;
        self.show_palette(p, !self.docking.shown(p));
    }

    /// Toggle temporary alignment guides (smart snapping) on or off.
    pub(crate) fn toggle_alignment_guides(&mut self) {
        self.snaps.alignment_guides = !self.snaps.alignment_guides;
        let text = if self.snaps.alignment_guides { t("Alignment guides on") } else { t("Alignment guides off") };
        self.status = Status { text: text.to_string(), error: false };
    }

    /// Follow what the last command removed: the selection, views of levels that are gone, and
    /// the contextual tab once its tool or selection is.
    fn tidy(&mut self) {
        // The Options Bar's lengths, rewritten in the project's units when they change.
        let (from, to) = (self.units, self.session.model.units);
        if from != to {
            let fields = [
                &mut self.wall_height,
                &mut self.column_height,
                &mut self.stair_width,
                &mut self.roof_overhang,
                &mut self.offset,
                &mut self.ceiling_height,
                &mut self.ramp_height,
                &mut self.ramp_width,
                &mut self.shaft_height,
                &mut self.mass_height,
            ];
            for field in fields {
                if let Some(mm) = from.parse(field) {
                    *field = to.length(mm);
                }
            }
            self.units = to;
        }
        let m = &self.session.model;
        self.selection.retain(|id| m.elements.contains_key(id));
        self.active_option = self.active_option.take().filter(|o| bimcraft_core::phasing::option_exists(m, o));
        let exists = |s: &Sheet| match s {
            Sheet::Plan(key) | Sheet::Ceiling(key) => m.plan_level(*key).is_some(),
            Sheet::Section(id) => m.section(*id).is_some(),
            Sheet::Paper(id) => m.sheet(*id).is_some(),
            Sheet::View3D | Sheet::Schedule(_) => true,
            Sheet::Table(id) => matches!(m.elements.get(id), Some(Element::Schedule(_))),
        };
        self.views.retain(exists);
        self.floating.retain(|f| exists(&f.sheet));
        if !exists(&self.sheet) {
            let floated = self.floating.last().map(|f| f.sheet);
            self.sheet = self.views.last().copied().or(floated).unwrap_or(Sheet::Plan(m.levels().first().map_or(0, |(id, _)| *id)));
        }
        let floats = self.floating.iter().any(|f| f.sheet == self.sheet);
        // The tab bar is never empty: the last view floating comes back to it.
        if floats && self.views.is_empty() {
            let sheet = self.sheet;
            self.floating.retain(|f| f.sheet != sheet);
        } else if !floats && !self.views.contains(&self.sheet) {
            self.views.push(self.sheet);
        }
        if !self.floating.iter().any(|f| f.sheet == self.sheet) {
            self.front = self.sheet;
        }
        if !self.views.contains(&self.front) {
            self.front = self.views.last().copied().unwrap_or(self.sheet);
        }
        if self.tab == Tab::Context && self.context().is_none() {
            self.tab = self.back;
        }
    }

    fn keys(&mut self, ctx: &Context) {
        // A note being written takes the keys, but for a field's that has them; another tool finishes it.
        if self.note_edit.is_some() {
            if !matches!(self.tool, Tool::Text | Tool::Modify) {
                self.finish_note();
            } else if !ctx.egui_wants_keyboard_input() && self.text_type_dialog.is_none() {
                self.note_keys(ctx);
                return;
            }
        }
        // A dialog takes the keys while it's open.
        if self.type_dialog.is_some()
            || self.text_type_dialog.is_some()
            || self.dimension_type_dialog.is_some()
            || self.dimension_text_dialog.is_some()
            || self.materials_dialog.is_some()
            || self.family_dialog.is_some()
            || self.favorites_dialog.is_some()
            || self.family_editor.is_some()
            || self.parameters_dialog.is_some()
            || self.schedule_dialog.is_some()
            || self.vg_dialog.is_some()
            || self.filters_dialog.is_some()
            || self.templates_dialog.is_some()
            || self.phasing_dialog.is_some()
            || self.display_dialog.is_some()
            || self.reinforce_dialog.is_some()
            || self.line_weights_dialog.is_some()
            || self.electrical_dialog.is_some()
            || self.object_styles_dialog.is_some()
            || self.line_styles_dialog.is_some()
            || self.fill_patterns_dialog.as_ref().is_some_and(|d| d.picking.is_none())
            || self.options_dialog.is_some()
            || self.purge_dialog.is_some()
            || self.select_by_id.is_some()
            || self.go_to_view.is_some()
            || self.work_plane.as_ref().is_some_and(|d| d.picking.is_none())
            || self.ids_of_selection.is_some()
            || self.warnings_dialog.is_some()
            || self.selection_filter.is_some()
            || self.paste_levels.is_some()
            || self.temp_dim_dialog.is_some()
            || self.paste_views.is_some()
            || self.settings_page.is_some()
            || self.plugins_open()
            || self.find.is_some()
            || self.new_schedule.is_some()
            || self.show_shortcuts
            || self.show_snaps
            || self.pending.is_some()
            || self.autosave.show_recovery
            || self.installing()
            || self.importing()
            || self.report_open()
            // An open menu takes the keys: Esc closes it, not the tool.
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // A camera view's own keys: Shift+W walks (Revit's SteeringWheels shortcut), Shift+F shows the safe frame (3ds Max's).
        // While it flies (the right button held, or walking) W, A, S, D, Q and E are the flight's, never a shortcut's: so
        // typing WA still places a wall whenever the view isn't flying.
        // Every other shortcut (Ctrl+S, Ctrl+Z, Delete…) works while it flies; only the letters typed are the flight's.
        let flight = self.sheet == Sheet::View3D && !ctx.egui_wants_keyboard_input() && (self.orbit.fly.is_some() || self.orbit.walk);
        if self.sheet == Sheet::View3D && !ctx.egui_wants_keyboard_input() {
            // Shift+W pressed afresh (a held key's repeats are none): while the right button flies, Shift and W are the flight's
            // (forward, faster), so it toggles Walk only when the view isn't flying that way.
            let fresh = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(e, Event::Key { key: Key::W, pressed: true, repeat: false, modifiers, .. }
                        if modifiers.matches_logically(Modifiers::SHIFT) && !modifiers.command && !modifiers.alt)
                })
            });
            if pressed(Modifiers::SHIFT, Key::W) {
                if fresh && self.orbit.fly.is_none() {
                    self.toggle_walk();
                }
                self.typed.clear();
                return;
            }
            if pressed(Modifiers::SHIFT, Key::F) {
                self.orbit.safe_frame = !self.orbit.safe_frame;
                self.typed.clear();
                return;
            }
            if self.orbit.walk && pressed(Modifiers::NONE, Key::Escape) {
                self.orbit.walk = false;
                self.typed.clear();
                return;
            }
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save_as(ctx, None);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.save(ctx, None);
        }
        // A hosted window's project is the page's: it opens and starts none of its own.
        if pressed(Modifiers::COMMAND, Key::O) && !self.hosted {
            self.guard(Pending::Open, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::N) && !self.hosted {
            self.guard(Pending::New, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::K) {
            self.find = Some(String::new());
        }
        if pressed(Modifiers::COMMAND, Key::Comma) {
            self.settings_page = Some(settings::Page::Interface);
        }
        if pressed(Modifiers::COMMAND, Key::Num1) {
            self.toggle_properties();
        }
        if pressed(Modifiers::COMMAND, Key::P) && !self.hosted {
            self.export(ctx, "PDF", "pdf");
        }
        // The rest belongs to a text field while one has the keyboard.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // Ctrl+C and Ctrl+V: the clipboard of the model's elements.
        self.clipboard_keys(ctx);
        if pressed(Modifiers::COMMAND, Key::D) {
            self.toggle_home();
        }
        self.nudge(ctx);
        if self.sketch.is_some() {
            // While sketching, Ctrl+Z takes back a line; the model's steps wait until Finish or Cancel.
            if pressed(Modifiers::COMMAND, Key::Z) {
                self.unsketch();
            }
        } else if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.run("redo", json!({}));
        } else if pressed(Modifiers::COMMAND, Key::Z) {
            self.run("undo", json!({}));
        }
        // Esc while a custom fill pattern is picked brings its dialog back.
        if pressed(Modifiers::NONE, Key::Escape) && !self.cancel_pattern_pick() {
            self.escape();
        }
        // Enter starts the tool waiting for its elements, or repeats the last tool when none is running.
        let enter = pressed(Modifiers::NONE, Key::Enter);
        if enter && (self.enter_for_pending() || self.enter_to_repeat()) {
            self.snap_once = None;
        } else if enter && let Some(level) = self.plan_level() {
            // Enter places a typed length, else builds the wall chain.
            match (&self.tool, self.typed_length.is_empty() && self.drawing.other.is_empty()) {
                _ if self.picking_pattern_lines() => self.finish_pattern_lines(),
                (Tool::Railing(_), true) => self.finish_railing(level),
                (Tool::Mass(_), true) => self.finish_mass(level),
                (Tool::DetailLine(_), true) => self.finish_detail_line(level),
                (Tool::SketchLine(_), true) if self.shape().open_ended() => self.finish_open_shape(),
                (_, true) => self.finish_wall(),
                (_, false) => self.place_typed(level),
            }
            // A snap override typed was for that point.
            self.snap_once = None;
        }
        if !self.typed_length.is_empty() && pressed(Modifiers::NONE, Key::Backspace) {
            self.typed_length.pop();
        }
        if pressed(Modifiers::NONE, Key::Delete) {
            self.delete();
        }
        // Space flips: what is selected, the door or window about to be placed, the wall about to be drawn.
        if matches!(self.tool, Tool::Modify | Tool::Door | Tool::Window | Tool::Wall(_)) && pressed(Modifiers::NONE, Key::Space) {
            self.flip_key();
        }
        self.reset_flips();
        // Space steps the Dimension tool's lay: Auto, Aligned, Horizontal, Vertical.
        if let Tool::Dimension(mode, _) = &mut self.tool
            && pressed(Modifiers::NONE, Key::Space)
        {
            *mode = mode.next();
        }
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) && pressed(Modifiers::NONE, Key::Space) {
            self.component_angle = (self.component_angle + 90.0) % 360.0;
        }
        if matches!(self.tool, Tool::Site(_)) && pressed(Modifiers::NONE, Key::Space) {
            self.site.angle = (self.site.angle + 90.0) % 360.0;
        }
        // F8 is the one function key for the guides. F11 is free: the system may use it for full screen.
        if pressed(Modifiers::NONE, Key::F8) {
            self.toggle_alignment_guides();
        }
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        });
        // A digit starts a length while a tool is picking its next point, and everything typed
        // after it (feet, inches, fractions, a unit) is the length's until Enter; letters before
        // one are shortcuts.
        let mut shortcut = String::new();
        if flight {
            self.typed.clear();
            return;
        }
        for ch in typed.chars() {
            let listening = self.base().is_some() || self.filleting();
            if listening && (ch.is_ascii_digit() || ch == '.' || !self.typed_length.is_empty()) {
                self.typed_length.push(ch);
            } else {
                shortcut.push(ch);
            }
        }
        for ch in shortcut.chars().filter(|c| c.is_ascii_alphanumeric()) {
            self.typed.push(ch.to_ascii_uppercase());
            if self.typed.len() > 2 {
                self.typed.remove(0);
            }
            if self.typed.len() == 2 && self.shortcut() {
                self.typed.clear();
            }
        }
    }

    /// Revit's two-letter shortcuts: the desktop's own ([`SHORTCUTS`]), then the commands' aliases
    /// (WA, DR, CR…). Whether `typed` is one.
    fn shortcut(&mut self) -> bool {
        if let Some(action) = self.custom_shortcuts.get(&self.typed).cloned()
            && self.execute_shortcut_action(&action)
        {
            return true;
        }
        let tool = match self.typed.as_str() {
            "3D" => {
                self.open_3d(None);
                return true;
            }
            "MD" => Tool::Modify,
            "MM" => Tool::Mirror { draw: false, start: None },
            "DM" => Tool::Mirror { draw: true, start: None },
            "AL" => Tool::Align(None),
            "OF" => Tool::Offset,
            "DW" => Tool::Demolish,
            "LW" => Tool::Linework,
            "ZF" | "ZE" | "ZX" => {
                self.fit();
                return true;
            }
            "ZA" => {
                self.fit_all();
                return true;
            }
            "ZS" => {
                self.zoom_sheet_size();
                return true;
            }
            "ZR" | "ZZ" => {
                self.zoom_region();
                return true;
            }
            "ZP" | "ZC" => {
                self.previous_zoom();
                return true;
            }
            "ZO" | "ZV" => {
                self.zoom_view(0.5);
                return true;
            }
            "RC" => {
                self.repeat_last();
                return true;
            }
            "TL" => {
                self.thin = !self.thin;
                return true;
            }
            "CS" => {
                self.create_similar();
                return true;
            }
            "PP" | "VP" => {
                self.toggle_properties();
                return true;
            }
            "NP" => {
                self.toggle_browser();
                return true;
            }
            // Revit's SA is Select All Instances: In Entire Project.
            "SA" => {
                self.select_all_instances_in_project();
                return true;
            }
            "AG" | "OG" => {
                self.toggle_alignment_guides();
                return true;
            }
            "KS" => {
                self.show_shortcuts = true;
                return true;
            }
            // Floor: Structural.
            "SB" => {
                self.structural_floor();
                return true;
            }
            // Reinforce the element selected.
            "RF" => {
                self.open_reinforce();
                return true;
            }
            "VV" | "VG" => {
                self.open_graphics(0);
                return true;
            }
            "EH" | "VH" => {
                self.hide_selection(self.typed == "VH");
                return true;
            }
            "WF" | "HL" | "SD" => {
                let style = match self.typed.as_str() {
                    "WF" => bimcraft_core::graphics::VisualStyle::Wireframe,
                    "HL" => bimcraft_core::graphics::VisualStyle::HiddenLine,
                    _ => bimcraft_core::graphics::VisualStyle::Shaded,
                };
                self.set_visual_style(style);
                return true;
            }
            "HH" | "HC" | "HI" | "IC" | "HR" | "RH" => {
                match self.typed.as_str() {
                    "HH" => self.temp_hide(false),
                    "HC" => self.temp_hide(true),
                    "HI" => self.temp_isolate(false),
                    "IC" => self.temp_isolate(true),
                    "HR" => self.temp_reset(),
                    _ => self.temp_reveal(),
                }
                return true;
            }
            // What the selection hides comes back; with nothing of it hidden, the list of what the view hides.
            "PU" => {
                self.open_purge();
                return true;
            }
            "LK" => {
                self.link_dialog = true;
                return true;
            }
            "DP" => {
                self.duplicate_plan("duplicate");
                return true;
            }
            "BX" => {
                self.toggle_section_box();
                return true;
            }
            "GD" => {
                self.open_display();
                return true;
            }
            "TB" => {
                self.open_title_blocks(None);
                return true;
            }
            "EU" | "UE" => {
                if self.hidden_selection().0.is_empty() {
                    self.open_graphics(graphics::ELEMENTS_TAB);
                } else {
                    self.unhide_selection(false);
                }
                return true;
            }
            "VU" => {
                self.unhide_selection(true);
                return true;
            }
            "EO" => {
                self.override_selection();
                return true;
            }
            "FT" => {
                self.filters_dialog = Some(graphics::FiltersDialog::default());
                return true;
            }
            "VT" => {
                self.open_templates();
                return true;
            }
            "PH" => {
                self.phasing_dialog = Some(phasing::PhasingDialog::default());
                return true;
            }
            "DO" => {
                self.options_dialog = Some(phasing::OptionsDialog::default());
                return true;
            }
            // Revit's snap overrides: for the next point picked, that snap alone, or none.
            "SE" | "SM" | "SI" | "SC" | "SP" | "SQ" | "ST" | "SN" | "SO" | "SS" => {
                self.snap_once = match self.typed.as_str() {
                    "SQ" => Some(Once::Only(Snap::Quadrant)),
                    "ST" => Some(Once::Only(Snap::Tangent)),
                    "SE" => Some(Once::Only(Snap::Endpoint)),
                    "SM" => Some(Once::Only(Snap::Midpoint)),
                    "SI" => Some(Once::Only(Snap::Intersection)),
                    "SC" => Some(Once::Only(Snap::Centre)),
                    "SP" => Some(Once::Only(Snap::Perpendicular)),
                    "SN" => Some(Once::Only(Snap::Nearest)),
                    "SO" => Some(Once::Off),
                    _ => None,
                };
                return true;
            }
            code if mep::fixture_with(code).is_some() => Tool::Fixture(mep::fixture_with(code).map_or("", |f| f.key)),
            code if mep::run_with(code).is_some() => Tool::Run(mep::run_with(code).unwrap_or("beam"), None),
            code => match engine::find(code).map(|c| c.id) {
                Some("wall") => Tool::Wall(Vec::new()),
                Some("door") => Tool::Door,
                Some("window") => Tool::Window,
                Some("room") => Tool::Room,
                Some("room_separator") => Tool::RoomSeparator(None),
                Some("level") => Tool::Level,
                Some("grid") => Tool::Grid(None),
                Some("pin") => {
                    self.pin_selection(true);
                    return true;
                }
                Some("unpin") => {
                    self.pin_selection(false);
                    return true;
                }
                Some("move") => Tool::Move(None),
                Some("copy") => Tool::Copy(None),
                Some("rotate") => Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false },
                Some("array") => Self::new_array(),
                Some("scale_elements") => Tool::Scale(Vec::new()),
                Some("corner") => Tool::Trim(None),
                Some("split") => Tool::Split,
                Some("linework") => Tool::Linework,
                Some("detail_group") => Tool::DetailGroup,
                Some("repeating_detail") => Tool::RepeatingDetail(None),
                Some("create_group") => {
                    self.create_group();
                    return true;
                }
                Some("ungroup") => {
                    self.ungroup();
                    return true;
                }
                Some("dimension") => Tool::Dimension(DimMode::Aligned, Vec::new()),
                Some("text") => Tool::Text,
                Some("detail_line") => {
                    (self.drawing.insulation, self.drawing.cloud) = (None, None);
                    Tool::DetailLine(Vec::new())
                }
                Some("spot") => Tool::Spot,
                Some("tag") => Tool::Tag,
                // Revit's CL is the structural column on its Structure tab, the architectural one elsewhere.
                Some("column") if self.tab == Tab::Structure || (self.tab == Tab::Context && self.back == Tab::Structure) => {
                    Tool::Stand("structuralcolumn")
                }
                Some("column") => Tool::Column,
                Some("footing") => Tool::Stand("footing"),
                Some("mass") => Tool::Mass(Vec::new()),
                Some("toposolid") => Tool::Site(site::SiteTool::Toposolid),
                Some("building_pad") => Tool::Site(site::SiteTool::BuildingPad),
                Some("property_line") => Tool::Site(site::SiteTool::PropertyLine),
                Some("show_mass") => {
                    self.show_mass();
                    return true;
                }
                Some("filled_region") => {
                    if let Some(view) = self.plan_key() {
                        self.start_sketch(sketch::Of::Region { view, masking: false }, Vec::new());
                    }
                    return true;
                }
                Some(c @ ("mass_floors" | "wall_by_face" | "floor_by_face" | "roof_by_face" | "update_by_face")) => {
                    self.by_mass(c);
                    return true;
                }
                Some("component") => Tool::Component,
                Some("section") => Tool::Section(None),
                Some("measure") => Tool::Measure(None),
                Some("paint") => Tool::Paint,
                Some("match_properties") => {
                    self.match_properties();
                    return true;
                }
                Some("delete") => {
                    self.delete();
                    return true;
                }
                Some("fillet") => {
                    self.start_tool(Tool::DetailLine(Vec::new()));
                    self.set_shape(draw::Target::Detail, draw::Shape::Fillet);
                    return true;
                }
                Some("join_polyline") => {
                    self.join_polyline();
                    return true;
                }
                Some("explode") => {
                    self.explode_polyline();
                    return true;
                }
                // Revit's opens the Project Units dialog; BIMCraft's is the Manage tab's menu.
                Some("units") => {
                    self.tab = Tab::Manage;
                    self.open_units = true;
                    return true;
                }
                _ => return false,
            },
        };
        self.start_tool(tool);
        true
    }

    /// The snaps in force for the next point picked: Manage > Snaps, unless a shortcut (SE, SM, SI,
    /// SC, SP, SN, SO) has overridden them for it.
    pub(crate) fn snaps_now(&self) -> Snaps {
        match self.snap_once {
            Some(Once::Only(kind)) => Snaps::only(kind),
            Some(Once::Off) => Snaps::none(),
            None => self.snaps,
        }
    }

    /// What the status bar says of a snap override waiting for the next point.
    fn snap_note(&self) -> Option<String> {
        match self.snap_once? {
            Once::Only(kind) => Some(tf("Snap: {} (next point)", &[t(kind.name())])),
            Once::Off => Some(t("Snaps off (next point)").to_string()),
        }
    }

    /// Create Similar (CS): the tool that makes what's selected, one element, with its type picked.
    fn create_similar(&mut self) {
        let m = Arc::clone(&self.session.model);
        let picked = match self.selection.as_slice() {
            &[id] => m.elements.get(id).map(|e| (id, e)),
            _ => None,
        };
        let tool = match picked.map(|(_, e)| e) {
            Some(Element::Wall(_)) => Tool::Wall(Vec::new()),
            Some(Element::Door(_)) => Tool::Door,
            Some(Element::Window(_)) => Tool::Window,
            Some(Element::Floor(_)) => Tool::Floor,
            Some(Element::Ceiling(_)) => Tool::Ceiling,
            Some(Element::Shaft(_)) => Tool::Shaft(None),
            Some(Element::Mass(_)) => Tool::Mass(Vec::new()),
            Some(Element::Roof(r)) if r.extrusion.is_some() => Tool::ExtrusionRoof(Vec::new()),
            Some(Element::Roof(_)) => Tool::Roof,
            Some(Element::Column(_)) => Tool::Column,
            Some(Element::StructuralColumn(_)) => Tool::Stand("structuralcolumn"),
            Some(Element::Footing(_)) => Tool::Stand("footing"),
            Some(e) if e.run().is_some() => Tool::Run(e.category(), None),
            Some(Element::Component(c)) => {
                self.component = Some((c.family.clone(), c.type_name.clone()));
                Tool::Component
            }
            Some(Element::Stair(_)) => Tool::Stair(Vec::new()),
            Some(Element::Ramp(_)) => Tool::Ramp(Vec::new()),
            Some(Element::Railing(_)) => Tool::Railing(Vec::new()),
            Some(Element::Room(_)) => Tool::Room,
            Some(Element::RoomSeparator(_)) => Tool::RoomSeparator(None),
            Some(Element::Level(_)) => Tool::Level,
            Some(Element::Grid(_)) => Tool::Grid(None),
            Some(Element::Dimension(d)) => match d.kind {
                DimKind::Aligned => Tool::Dimension(DimMode::Aligned, Vec::new()),
                DimKind::Linear if d.orient == Some(bimcraft_core::model::Orient::Vertical) => {
                    Tool::Dimension(DimMode::Vertical, Vec::new())
                }
                DimKind::Linear => Tool::Dimension(DimMode::Horizontal, Vec::new()),
                DimKind::Angular => Tool::Angular(Vec::new()),
                k @ (DimKind::Radial | DimKind::Diameter | DimKind::ArcLength) => Tool::Round(k, Vec::new()),
            },
            Some(Element::Text(_)) => Tool::Text,
            Some(Element::DetailLine(l)) => {
                self.line_style.clone_from(&l.style);
                Tool::DetailLine(Vec::new())
            }
            Some(Element::Spot(sp)) if sp.slope => Tool::SpotSlope,
            Some(Element::Spot(_)) => Tool::Spot,
            Some(Element::Tag(_)) => Tool::Tag,
            Some(Element::Section(_)) => Tool::Section(None),
            Some(Element::Elevation(_)) => Tool::Elevation,
            // Nothing, several, or a sheet's viewport: nothing to place one like.
            _ => {
                self.status = Status { text: t("select one element to create one like it first").into(), error: true };
                return;
            }
        };
        if let Some((id, e)) = picked
            && let Some(name) = element_json_bare(&m, id)["type"].as_str()
        {
            self.types.insert(e.category(), name.to_string());
        }
        self.start_tool(tool);
        // A structural floor's likeness is structural.
        self.floor_structural = matches!(picked.map(|(_, e)| e), Some(Element::Floor(f)) if f.structural);
    }

    /// Select All Instances > Visible in View: every element the open view shows of a type selected, a
    /// component's type being its family's.
    fn select_all_instances(&mut self) {
        self.select_instances(false);
    }

    /// Select All Instances > In Entire Project (SA): every element of the types selected, shown in the open view or not.
    pub(crate) fn select_all_instances_in_project(&mut self) {
        self.select_instances(true);
    }

    fn select_instances(&mut self, project: bool) {
        let m = Arc::clone(&self.session.model);
        // One look at every element: a component's room is found among its level's rooms once.
        let rooms = bimcraft_core::rooms::Rooms::new(&m);
        let type_of = |id: &Id| {
            let e = bimcraft_core::element_json_with(&m, *id, &rooms);
            e["type"].is_string().then(|| [e["category"].clone(), e["family"].clone(), e["type"].clone()])
        };
        let types: Vec<[Value; 3]> = self.selection.iter().filter_map(type_of).collect();
        if types.is_empty() {
            self.status = Status { text: t("select an element that has a type first").into(), error: true };
            return;
        }
        let from: BTreeSet<Id> = if project { m.elements.keys().copied().collect() } else { self.shown() };
        let found = from.into_iter().filter(|id| type_of(id).is_some_and(|k| types.contains(&k))).collect();
        self.pick_up(found, Modifiers::NONE);
    }

    /// Match Properties (MA): with one element selected, its type is the one the next element drawn
    /// of its category takes; with several, the `match_properties` command gives the first one's type
    /// to the others of its category.
    pub(crate) fn match_properties(&mut self) {
        let Some(&first) = self.selection.first() else {
            self.status = Status { text: t("Select an element first to match its properties").into(), error: true };
            return;
        };
        let src = element_json_bare(&self.session.model, first);
        let Some(typ) = src["type"].as_str().filter(|t| !t.is_empty()) else {
            self.status = Status { text: t("The selected element has no type to match").into(), error: true };
            return;
        };
        if self.selection.len() == 1 {
            if let Some(el) = self.session.model.elements.get(first) {
                self.types.insert(el.category(), typ.to_string());
            }
            if let Some(f) = src["family"].as_str() {
                self.component = Some((f.to_string(), typ.to_string()));
            }
            self.status = Status {
                text: tf("Match Properties: source type '{}' selected. Select target elements and press MA again.", &[typ]),
                error: false,
            };
        } else if let Some(r) = self.run("match_properties", json!({"from": first, "ids": &self.selection[1..]})) {
            let n = r["modified"].as_array().map_or(0, Vec::len);
            self.status =
                Status { text: tf("Matched properties: applied type '{}' to {} element(s)", &[typ, &n.to_string()]), error: false };
        }
    }

    /// What the open view shows: what a plan draws, in its view range and crop region; what a
    /// section or an elevation cuts and what its picture shows past its line; in 3D, the whole
    /// model but for what is hidden. Nothing on a sheet or in a schedule has a type to select.
    fn shown(&mut self) -> BTreeSet<Id> {
        let m = Arc::clone(&self.session.model);
        let owners = |d: render::Drawing| d.owners.into_iter().flatten().collect::<BTreeSet<Id>>();
        // Fitted at any size: what a view shows doesn't depend on it.
        let (w, h) = (1600, 1000);
        match self.sheet {
            Sheet::Plan(level) => owners(render::plan(&m, level, w, h)),
            Sheet::Ceiling(level) => owners(render::ceiling_plan(&m, level, w, h)),
            Sheet::Section(id) => {
                let view = render::section_fit(&m, id, w, h);
                // ponytail: the picture leaves out what's hidden behind something nearer; test the
                // solids against the view's extent if SA should find those too.
                let seen = render::section_picture(&m, id, view, &[]).map(|i| i.shows()).unwrap_or_default();
                owners(render::section(&m, id, view)).into_iter().chain(seen).collect()
            }
            // The whole model, but for what the 3D view on screen hides (a camera view's own Visibility/Graphics).
            Sheet::View3D => {
                let look = crate::orbit::look_of(&m, self.active_3d());
                m.elements.keys().copied().filter(|id| look.shows(*id)).collect()
            }
            Sheet::Paper(_) | Sheet::Schedule(_) | Sheet::Table(_) => BTreeSet::new(),
        }
    }

    /// The material the Paint tool lays: the one picked while the library has it, else the library's first.
    pub(crate) fn painting(&self) -> Option<String> {
        let m = &self.session.model;
        Some(self.paint_material.clone()).filter(|n| m.materials.contains_key(n)).or_else(|| m.materials.keys().next().cloned())
    }

    /// Start `tool` as the person asked for it, from the ribbon, a shortcut or a menu: Repeat (Enter, RC) can start it again.
    /// A tool the app starts for itself goes through [`set_tool`](Self::set_tool), which remembers nothing.
    fn start_tool(&mut self, tool: Tool) {
        self.flow.by_user = true;
        self.set_tool(tool);
        self.flow.by_user = false;
    }

    fn set_tool(&mut self, mut tool: Tool) {
        // Trim/Extend to Corner with two lines or walls selected (or picked, sketching) joins them at once, keeping the
        // parts toward their middles; with any other selection it asks for the picks.
        if tool == Tool::Trim(None) && self.corner_selected() {
            return;
        }
        // Paint works in the 3D view: open it, as the tool asks.
        if matches!(tool, Tool::Paint | Tool::Unpaint) && self.sheet != Sheet::View3D {
            self.open_sheet(Sheet::View3D);
        }
        // Move, Copy, Rotate and Mirror take the selection; Align, Trim, Split and Offset pick walls, Demolish anything.
        let edits =
            matches!(tool, Tool::Move(_) | Tool::Copy(_) | Tool::Rotate { .. } | Tool::Array { .. } | Tool::Mirror { .. } | Tool::Scale(_));
        let picks = matches!(
            tool,
            Tool::Align(_)
                | Tool::Trim(_)
                | Tool::TrimTo { .. }
                | Tool::Vertex { .. }
                | Tool::Split
                | Tool::Offset
                | Tool::Demolish
                | Tool::Linework
        );
        // A tool of the hand's own ends Pan and Zoom Region, and one asked for before its elements waits for them (Enter).
        self.end_navigation();
        self.flow.pending = None;
        if edits && self.selection.is_empty() {
            self.remember_tool(&tool);
            self.ask_selection_for(tool);
            return;
        }
        // A pinned element stays where it is: Unpin (UP) frees it.
        let moves = match tool {
            Tool::Move(_) => true,
            Tool::Rotate { .. } => !self.rotate_copy,
            Tool::Mirror { .. } => !self.mirror_copy,
            _ => false,
        };
        let refused = if matches!(tool, Tool::Rotate { .. }) { self.refuse_pinned_turning() } else { self.refuse_pinned() };
        if moves && refused {
            return;
        }
        self.remember_tool(&tool);
        if tool == Tool::Attach && self.selected_walls().is_empty() {
            self.status = Status { text: t("select the walls to attach first").into(), error: true };
            return;
        }
        if matches!(tool, Tool::SlopeArrow(_)) && self.selected_floor().is_none() {
            self.status = Status { text: t("select the floor to slope first").into(), error: true };
            return;
        }
        if matches!(tool, Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_)) && self.selected_shapeable().is_none() {
            self.status = Status { text: t("select the floor or flat roof to shape first").into(), error: true };
            return;
        }
        // Modify Sub Elements starts with nothing of the shape picked, its Elevation blank until something is.
        if tool == Tool::ModifySubElements && self.tool != Tool::ModifySubElements {
            self.shape_picked.clear();
            self.shape_z.clear();
        }
        if let Tool::Rotate { centre, .. } | Tool::Array { centre, .. } = &mut tool
            && let Some(middle) = self.middle()
        {
            *centre = middle;
        }
        if tool == Tool::Level {
            let top = self.session.model.levels().last().map_or(0.0, |(_, l)| l.elevation);
            self.new_level = (self.session.model.units.length(top + 3000.0), String::new());
        }
        let shaping = matches!(tool, Tool::SlopeArrow(_) | Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_));
        if tool != Tool::Modify && tool != Tool::Attach && tool != Tool::GableEdge && !shaping && !edits {
            self.selection.clear();
        }
        if tool != Tool::Modify && !picks {
            self.raise_context();
        }
        self.typed.clear();
        self.clear_typed();
        // Insulation and Revision Cloud are the Detail Line tool's; the lines Fillet Arc and Trim/Extend have picked were
        // picked for them, and are not for the tool that comes next.
        if !matches!(tool, Tool::DetailLine(_)) {
            (self.drawing.insulation, self.drawing.cloud) = (None, None);
        }
        self.forget_picks();
        // The Floor tool makes structural floors only when the Structure tab asks (`structural_floor`).
        self.floor_structural = false;
        if let Tool::Run(category, _) = &tool {
            self.keep_run_options(category);
        }
        if !matches!(tool, Tool::Measure(_)) {
            self.measured = None;
        }
        self.snap_once = None;
        self.tool = tool;
        // A site tool that draws an outline or a path sketches it at once, as Floor does.
        if let Tool::Site(t) = self.tool {
            self.site.angle = 0.0;
            self.begin_site(t);
            return;
        }
        // Floor sketches its outline at once, as Revit's does (Walls' Outline puts the walls' in the sketch).
        if self.tool == Tool::Floor
            && self.sketch.is_none()
            && matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_))
            && let Some(level) = self.plan_level()
        {
            self.start_sketch(crate::sketch::Of::Floor(level), Vec::new());
            return;
        }
        // In the 3D view the tools that sketch an outline sketch it on the work plane at once.
        if self.sheet == Sheet::View3D
            && self.sketch.is_none()
            && matches!(self.tool, Tool::Floor | Tool::Ceiling | Tool::Roof | Tool::Shaft(_))
        {
            self.sketch_on_work_plane();
        }
    }

    /// The walls among the selection.
    fn selected_walls(&self) -> Vec<Id> {
        self.selection.iter().copied().filter(|id| self.session.model.wall(*id).is_some()).collect()
    }

    /// The roof selected, when one roof is all that is.
    pub(crate) fn selected_roof(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if matches!(self.session.model.elements.get(id), Some(Element::Roof(_))) => Some(id),
            _ => None,
        }
    }

    /// Shape Editing's own tools for floor or flat roof `id`: small, two by two as the Modify panel's are, so the panels
    /// after them keep their room.
    fn shape_tools(&mut self, ui: &mut egui::Ui, id: Id) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                let tip =
                    msg!("Modify Sub Elements: select a point, a corner or an edge of the top, then type its height on the Options Bar or its label");
                let active = self.tool == Tool::ModifySubElements;
                if small_button(ui, Icon::ModifySubElements, "Modify Sub Elements", active, tip).clicked() {
                    self.start_tool(Tool::ModifySubElements);
                }
                let tip = msg!("Add Point: click on the top to raise or lower it there to the Options Bar's elevation");
                if small_button(ui, Icon::AddPoint, "Add Point", self.tool == Tool::AddPoint, tip).clicked() {
                    self.start_tool(Tool::AddPoint);
                }
                // Convert Lines, as Revit's: on while Modify Sub Elements has folding lines selected.
                let tip = msg!("Convert Lines: make the selected folding lines split lines, so the top keeps folding along them");
                let can = self.tool == Tool::ModifySubElements && !self.shape_picked_folds().is_empty();
                if ui.add_enabled_ui(can, |ui| small_button(ui, Icon::ConvertLines, msg!("Convert Lines"), false, tip)).inner.clicked() {
                    self.shape_convert_lines();
                }
            });
            ui.horizontal(|ui| {
                let tip = msg!("Add Split Line: click two points; the top folds along the line between them");
                let active = matches!(self.tool, Tool::AddSplitLine(_));
                if small_button(ui, Icon::AddSplitLine, "Add Split Line", active, tip).clicked() {
                    self.start_tool(Tool::AddSplitLine(None));
                }
                let tip = msg!("Reset Shape: take the points and split lines off, the top flat again");
                if small_button(ui, Icon::ResetShape, "Reset Shape", false, tip).clicked() {
                    self.run("reset_shape", serde_json::json!({"id": id}));
                    self.tool = Tool::Modify;
                }
            });
        });
    }

    /// Shape Editing's Elevation Base on the Options Bar, as Revit's: what its elevations count from.
    fn elevation_base(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Elevation Base:"));
        let now = self.shape_base;
        crate::icons::combo("shape-elevation-base")
            .selected_text(t(now.name()))
            .show_ui(ui, |ui| {
                for b in crate::shape::Base::ALL {
                    ui.selectable_value(&mut self.shape_base, b, t(b.name()));
                }
            })
            .response
            .labelled_by(l.id);
        if self.shape_base != now {
            self.shape_refresh();
        }
    }

    /// What Shape Editing works on, when it's all that's selected: a floor, or a flat roof by footprint.
    pub(crate) fn selected_shapeable(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if self.session.model.shapeable(id).is_ok() => Some(id),
            _ => None,
        }
    }

    /// The floor selected, when one floor is all that is.
    pub(crate) fn selected_floor(&self) -> Option<Id> {
        match self.selection.as_slice() {
            &[id] if matches!(self.session.model.elements.get(id), Some(Element::Floor(_))) => Some(id),
            _ => None,
        }
    }

    /// The selected walls turned round about their location lines, and the selected doors and windows a step through their
    /// four ways: Revit's flip arrows, or Space.
    pub(crate) fn flip(&mut self) {
        let walls = self.selected_walls();
        let openings = self.opening_step();
        if walls.is_empty() && openings.is_empty() {
            return;
        }
        let ids: Vec<Id> = walls.into_iter().chain(openings.iter().copied()).collect();
        if openings.is_empty() {
            self.run("flip", json!({"ids": ids}));
        } else {
            self.run("flip", json!({"ids": ids, "step": true}));
        }
    }

    /// The middle of what's selected: where Rotate turns it about until told otherwise.
    fn middle(&self) -> Option<P2> {
        let m = &self.session.model;
        let mut pts = Vec::new();
        for id in &self.selection {
            match m.elements.get(id) {
                Some(Element::Wall(w)) => pts.extend([w.start, w.end]),
                Some(Element::Grid(g)) => pts.extend([g.start, g.end]),
                Some(Element::Floor(f)) => pts.extend(&f.boundary),
                Some(Element::Room(r)) => pts.push(r.point),
                Some(Element::Door(_) | Element::Window(_)) => pts.extend(m.anchor(*id)),
                Some(Element::Dimension(d)) => pts.push(d.at),
                Some(Element::Section(v) | Element::Elevation(v)) => pts.extend([v.start, v.end]),
                Some(Element::Tag(t)) => pts.extend(m.anchor(t.host).map(|a| a + t.offset)),
                Some(Element::Column(c)) => pts.push(c.at),
                Some(Element::Component(c)) => pts.push(c.at),
                Some(e @ (Element::Stair(_) | Element::Ramp(_))) => {
                    pts.extend(stairs::layout(m, e).into_iter().flat_map(|l| l.walk.into_iter().map(|(p, _)| p)));
                }
                Some(Element::Railing(rl)) => pts.extend(stairs::railing(m, rl).into_iter().map(|(p, _)| p)),
                Some(Element::Roof(r)) => pts.extend(&r.boundary),
                // A camera stands on the plan at its eye: turning it turns it about that point unless Place Centre says otherwise.
                Some(Element::View3D(v)) => pts.extend(v.camera.map(|c| P2(c.eye[0], c.eye[1]))),
                _ => {}
            }
        }
        let first = *pts.first()?;
        let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1))));
        Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
    }

    /// The point the tool measures its next one from, while it has one.
    fn base(&self) -> Option<P2> {
        match &self.tool {
            Tool::Wall(chain) => chain.last().copied(),
            Tool::Run(_, start) => *start,
            Tool::Grid(start)
            | Tool::RoomSeparator(start)
            | Tool::Move(start)
            | Tool::Copy(start)
            | Tool::Section(start)
            | Tool::SlopeArrow(start)
            | Tool::AddSplitLine(start) => *start,
            Tool::Array { start, .. } if !self.array.radial => *start,
            Tool::Stair(pts) | Tool::Ramp(pts) | Tool::Railing(pts) | Tool::Mass(pts) | Tool::DetailLine(pts) | Tool::SketchLine(pts) => {
                pts.last().copied()
            }
            _ => None,
        }
    }

    /// The level of the plan on screen, a floor plan or a ceiling plan (of a plan view, its level's).
    pub(crate) fn plan_level(&self) -> Option<Id> {
        self.plan_key().and_then(|key| self.session.model.plan_level(key))
    }

    /// The key of the plan on screen: its level's, or a plan view's.
    pub(crate) fn plan_key(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) => Some(key),
            _ => None,
        }
    }

    /// Escape: dismiss the error, drop the chain being drawn, else leave the tool, else clear the
    /// selection.
    fn escape(&mut self) {
        // Pick in View goes back to its dialog.
        if self.stop_work_plane_pick() {
            return;
        }
        // Pan and Zoom Region end first, then a tool waiting for its elements.
        if self.end_navigation() || self.flow.pending.take().is_some() {
            return;
        }
        self.typed.clear();
        self.ribbon_peek = false;
        // A snap override waiting for its point goes first, leaving what's being drawn.
        if self.snap_once.take().is_some() {
            return;
        }
        self.clear_typed();
        self.status = Status::default();
        self.tool = match std::mem::replace(&mut self.tool, Tool::Modify) {
            Tool::Wall(chain) if !chain.is_empty() => Tool::Wall(Vec::new()),
            Tool::Grid(Some(_)) => Tool::Grid(None),
            Tool::RoomSeparator(Some(_)) => Tool::RoomSeparator(None),
            Tool::Run(c, Some(_)) => Tool::Run(c, None),
            Tool::Mirror { draw, start: Some(_) } => Tool::Mirror { draw, start: None },
            Tool::Align(Some(_)) => {
                self.drawing.align_reference = None;
                Tool::Align(None)
            }
            Tool::Trim(Some(_)) => Tool::Trim(None),
            Tool::TrimTo { boundary: Some(_), many } => Tool::TrimTo { boundary: None, many },
            Tool::Dimension(mode, refs) if !refs.is_empty() => Tool::Dimension(mode, Vec::new()),
            Tool::Angular(refs) if !refs.is_empty() => Tool::Angular(Vec::new()),
            Tool::Round(k, picked) if !picked.is_empty() => Tool::Round(k, Vec::new()),
            Tool::Section(Some(_)) => Tool::Section(None),
            Tool::Measure(Some(_)) => Tool::Measure(None),
            // The ruler laid goes first, then the tool.
            Tool::Measure(None) if self.measured.take().is_some() => Tool::Measure(None),
            Tool::Stair(pts) if !pts.is_empty() => Tool::Stair(Vec::new()),
            Tool::Ramp(pts) if !pts.is_empty() => Tool::Ramp(Vec::new()),
            Tool::Railing(pts) if !pts.is_empty() => Tool::Railing(Vec::new()),
            Tool::Mass(pts) if !pts.is_empty() => Tool::Mass(Vec::new()),
            Tool::DetailLine(pts) if !pts.is_empty() => Tool::DetailLine(Vec::new()),
            Tool::Shaft(Some(_)) => Tool::Shaft(None),
            Tool::SlopeArrow(Some(_)) => Tool::SlopeArrow(None),
            Tool::AddSplitLine(Some(_)) => Tool::AddSplitLine(None),
            Tool::ExtrusionRoof(pts) if !pts.is_empty() => Tool::ExtrusionRoof(Vec::new()),
            Tool::SketchLine(pts) if !pts.is_empty() => Tool::SketchLine(Vec::new()),
            // Fillet Arc's lines picked so far go first, then the tool.
            Tool::DetailLine(pts) if !self.drawing.fillet.is_empty() => {
                self.drawing.fillet.clear();
                Tool::DetailLine(pts)
            }
            Tool::SketchLine(pts) if !self.drawing.fillet.is_empty() => {
                self.drawing.fillet.clear();
                Tool::SketchLine(pts)
            }
            // While sketching, Trim/Extend's first pick likewise.
            tool @ (Tool::Trim(_) | Tool::TrimTo { .. }) if self.drawing.trim.is_some() => {
                self.drawing.trim = None;
                tool
            }
            Tool::Modify => {
                match &mut self.sketch {
                    Some(sk) => sk.picked.clear(),
                    None => self.selection.clear(),
                }
                Tool::Modify
            }
            _ => Tool::Modify,
        };
    }

    fn delete(&mut self) {
        // Under Modify Sub Elements, what of the shape is selected.
        if self.shape_delete() {
            return;
        }
        if self.sketch.is_some() {
            self.delete_sketch_lines();
            return;
        }
        if !self.selection.is_empty() {
            self.delete_selection();
        }
    }

    /// Zoom the view on screen by `factor` about its middle.
    fn zoom_view(&mut self, factor: f64) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.zoom(id, factor),
            Sheet::View3D => {
                if let Some(c) = self.orbit.camera {
                    self.orbit.camera = Some(c.zoom(factor, (f64::from(c.width) / 2.0, f64::from(c.height) / 2.0)));
                }
            }
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
    }

    /// Zoom All to Fit (ZA): every open view, in a tab or a window of its own, shows all of what it holds again.
    pub(crate) fn fit_all(&mut self) {
        let open: Vec<Sheet> = self.views.iter().copied().chain(self.floating.iter().map(|f| f.sheet)).chain([self.sheet]).collect();
        for sheet in open {
            match sheet {
                Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.fit(id),
                Sheet::View3D => self.fit3d(),
                Sheet::Schedule(_) | Sheet::Table(_) => {}
            }
        }
    }

    /// Zoom Sheet Size (ZS), as Revit's: the view at the size it prints, a millimetre of paper as long on the screen as a
    /// millimetre is, about its middle. A sheet is its paper; a plan, a ceiling plan, a section or an elevation is printed at
    /// its own scale (1 : n), so a metre of it is 1000 / n mm of paper. A screen is taken at 96 points to the inch, the unit
    /// egui draws in at the system's scale; the interface's own zoom on top of that is taken out, so the size is the physical
    /// one however large the interface is drawn.
    pub(crate) fn zoom_sheet_size(&mut self) {
        let mm = 96.0 / 25.4 / f64::from(self.ui_zoom.max(0.01));
        let m = &self.session.model;
        match self.sheet {
            Sheet::Paper(id) => self.canvas.set_scale(id, mm),
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) => {
                let n = bimcraft_core::graphics::scale_of(m, id).max(1.0);
                self.canvas.set_scale(id, mm / n);
            }
            Sheet::View3D | Sheet::Schedule(_) | Sheet::Table(_) => {
                self.status = Status { text: t("Zoom Sheet Size: not in this view").into(), error: true }
            }
        }
    }

    fn fit(&mut self) {
        match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) => self.canvas.fit(id),
            Sheet::View3D => self.fit3d(),
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
    }

    /// The 3D view the 3D tab shows: one opened on it (a camera view, or `{3D}` itself), else the default `{3D}` — none
    /// while the project has neither.
    pub(crate) fn active_3d(&self) -> Option<Id> {
        let m = &self.session.model;
        self.active_3d.filter(|id| matches!(m.elements.get(id), Some(Element::View3D(_)))).or_else(|| m.default_3d())
    }

    /// Open a 3D view in the 3D tab: the default `{3D}` (none), or a camera view by id. Its camera stands the view where
    /// it looks from the next time it is drawn.
    pub(crate) fn open_3d(&mut self, id: Option<Id>) {
        self.active_3d = id;
        self.orbit.camera = None;
        self.orbit.built = None;
        self.open_sheet(Sheet::View3D);
    }

    /// Open a view in a tab, or bring its tab forward.
    fn open_sheet(&mut self, sheet: Sheet) {
        if !matches!(sheet, Sheet::Section(_)) {
            self.cut.suspend();
        }
        if !matches!(sheet, Sheet::View3D) {
            self.orbit.suspend();
            if let Some(gpu) = &mut self.card.gpu {
                gpu.suspend_upload();
            }
        }
        self.canvas.drawing.suspend();
        if !self.views.contains(&sheet) && !self.floats(sheet) {
            self.views.push(sheet);
        }
        self.sheet = sheet;
        if !self.floats(sheet) {
            self.front = sheet;
        }
        // The selection is the model's, not the view's: it stays, whatever view comes up.
        self.clear_typed();
        self.snap_once = None;
        // A ruler belongs to the view it was laid in.
        self.measured = None;
        // The tools that pick points work in plans (Text in sections and on sheets too, Spot
        // Elevation in sections, Demolish in sections and 3D); any other view takes clicks to select.
        let works = match sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) => true,
            Sheet::Section(_) => matches!(self.tool, Tool::Text | Tool::Spot | Tool::Demolish),
            Sheet::View3D => matches!(self.tool, Tool::Demolish | Tool::Paint | Tool::Unpaint),
            Sheet::Paper(_) => self.tool == Tool::Text,
            _ => false,
        };
        // (A sketch is held in its plan: it takes the window back, with its tool, [`BimApp::track_sketch`].)
        if !works && self.tool != Tool::Level && self.sketch_plan().is_none() {
            self.tool = Tool::Modify;
        }
        match &mut self.tool {
            Tool::Wall(chain) => chain.clear(),
            Tool::Grid(start) | Tool::RoomSeparator(start) | Tool::Run(_, start) => *start = None,
            Tool::Dimension(_, refs) | Tool::Angular(refs) => refs.clear(),
            Tool::Round(_, picked) => picked.clear(),
            Tool::Section(start) | Tool::Shaft(start) | Tool::Measure(start) | Tool::Camera(start) => *start = None,
            Tool::Stair(pts)
            | Tool::Ramp(pts)
            | Tool::Railing(pts)
            | Tool::ExtrusionRoof(pts)
            | Tool::Mass(pts)
            | Tool::DetailLine(pts) => pts.clear(),
            Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Array { .. }
            | Tool::SlopeArrow(_)
            | Tool::ModifySubElements
            | Tool::AddPoint
            | Tool::AddSplitLine(_)
            | Tool::GableEdge => self.tool = Tool::Modify,
            _ => {}
        }
    }

    /// The views on no sheet yet, as Place View lists them: plans, sections and elevations, the
    /// 3D view and the schedules; each as `place` takes it, by id, or by name while it isn't an
    /// element yet.
    fn unplaced_views(&self) -> Vec<(Value, String)> {
        let m = &self.session.model;
        let placed: BTreeSet<&str> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Viewport(v) => m.view_name(v.view),
                _ => None,
            })
            .collect();
        let plans = m.levels().into_iter().flat_map(|(id, l)| {
            let own = std::iter::once((json!(id), tf("Floor Plan: {}", &[&l.name]), l.name.as_str()));
            own.chain(m.plan_views(id).into_iter().map(|(v, pv)| (json!(v), tf("Floor Plan: {}", &[&pv.name]), pv.name.as_str())))
        });
        let cuts = m.elements.iter().filter_map(|(id, e)| match e {
            Element::Section(v) => Some((json!(id), tf("Section: {}", &[&v.name]), v.name.as_str())),
            Element::Elevation(v) => Some((json!(id), tf("Elevation: {}", &[&v.name]), v.name.as_str())),
            _ => None,
        });
        // The schedules every project has, by their English names, then the project's others.
        let builtin = SCHEDULES.iter().map(|(cat, name)| match schedule::standard(m, cat) {
            (Some(id), sc) => (json!(id), tf("Schedule: {}", &[&sc.name]), m.view_name(id).unwrap_or(name)),
            (None, _) => (json!(name), tf("Schedule: {}", &[t(name)]), *name),
        });
        let made = self.schedules().into_iter().map(|(id, name)| (json!(id), tf("Schedule: {}", &[name]), name));
        let others = std::iter::once((json!("{3D}"), t("3D View: {3D}").to_string(), "{3D}")).chain(builtin).chain(made);
        plans.chain(cuts).chain(others).filter(|(_, _, name)| !placed.contains(name)).map(|(v, label, _)| (v, label)).collect()
    }

    /// The project's schedules other than those every project has: their ids and names, by name.
    pub(crate) fn schedules(&self) -> Vec<(Id, &str)> {
        let m = &self.session.model;
        let mut out: Vec<(Id, &str)> = m
            .rare("schedule")
            .into_iter()
            .filter_map(|id| match m.elements.get(id) {
                Some(Element::Schedule(sc)) if schedule::builtin(Some(&sc.name), None).is_none() => Some((id, sc.name.as_str())),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// Close a view's tab; the last one stays open.
    fn close_view(&mut self, sheet: Sheet) {
        if self.floats(sheet) {
            self.floating.retain(|f| f.sheet != sheet);
            if self.sheet == sheet {
                self.open_sheet(self.front);
            }
            return;
        }
        if self.views.len() < 2 {
            return;
        }
        self.views.retain(|v| *v != sheet);
        if self.sheet == sheet
            && let Some(&last) = self.views.last()
        {
            self.open_sheet(last);
        }
    }

    // ---------- files ----------

    /// Save the project (`.bimcraft`, or any other name) or export by extension: `.ifc`, the 3D scene as `.glb`, or the
    /// view on screen as `.png` (a plan or the 3D view) or `.svg` (a plan). Whether it worked; the
    /// warning box says why not.
    pub fn save_to(&mut self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = path.to_string_lossy();
        if matches!(ext.as_str(), "png" | "svg") {
            let mut p = json!({"path": file, "width": 1600, "height": 1000});
            match (self.sheet, self.orbit.camera) {
                (Sheet::Plan(id) | Sheet::Section(id) | Sheet::Paper(id), _) => p["view"] = json!(id),
                (Sheet::Ceiling(id), _) => (p["view"], p["ceiling"]) = (json!(id), json!(true)),
                (Sheet::View3D, camera) => {
                    p["view"] = json!("{3D}");
                    if let Some(c) = camera {
                        (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                    }
                }
                (Sheet::Schedule(_) | Sheet::Table(_), _) => {}
            }
            return self.run("render", p).is_some();
        }
        let saved = self.run("save", json!({"path": file})).is_some();
        // Exports (IFC, glTF, the sheets' PDF) leave the project unsaved.
        if saved && !matches!(ext.as_str(), "ifc" | "glb" | "pdf") {
            self.saved = Arc::clone(&self.session.model);
            self.note_recent(path);
            self.autosave_saved();
        }
        saved
    }

    /// Saves to the project's file, or asks where first. `then` is done once it is saved.
    fn save(&mut self, ctx: &Context, then: Option<Pending>) {
        if self.hosted {
            return self.ask_host_to_save(ctx, then);
        }
        match self.session.path.clone() {
            Some(p) => {
                if self.save_to(&p)
                    && let Some(then) = then
                {
                    self.proceed(then, ctx);
                }
            }
            None => self.save_as(ctx, then),
        }
    }

    fn save_as(&mut self, ctx: &Context, then: Option<Pending>) {
        if self.hosted {
            return self.ask_host_to_save(ctx, then);
        }
        let later = ctx.clone();
        self.ask_path(ctx, "Save", msg!("BIMCraft project"), "bimcraft", move |app, p| {
            if app.save_to(&p)
                && let Some(then) = then
            {
                app.proceed(then, &later);
            }
        });
    }

    fn export(&mut self, ctx: &Context, what: &str, ext: &str) {
        self.ask_path(ctx, "Export", what, ext, |app, p| {
            app.save_to(&p);
        });
    }

    /// Asks, in the system's file dialog titled `title`, where to save, and calls `then` with the path, ending in `.ext`, in a
    /// later frame.
    fn ask_path(&mut self, ctx: &Context, title: &str, what: &str, ext: &str, then: impl FnOnce(&mut BimApp, PathBuf) + 'static) {
        let name = format!("{}.{ext}", self.session.model.name.replace(['/', '\\'], "-"));
        let ext = ext.to_string();
        let dialog = crate::dialog::Ask::new().add_filter(t(what), &[&ext]).set_file_name(name).set_title(t(title));
        self.ask_file(ctx, dialog, true, move |app, p| {
            then(app, if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(&ext)) { p } else { p.with_extension(&ext) });
        });
    }

    /// Open a project, or import an IFC file and say what came in and what was left out.
    pub(crate) fn open_path(&mut self, path: &Path) -> bool {
        if revit::is_revit(path) {
            return self.open_revit(path);
        }
        let Some(reply) = self.run("open", json!({"path": path.to_string_lossy()})) else {
            self.forget_recent_if_gone(path);
            // Even a damaged original can have a healthy recovery copy.
            if !self.hosted && !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")) {
                self.inspect_recovery(path.to_path_buf());
            }
            return false;
        };
        self.retire_autosave();
        self.reopened();
        self.note_recent(path);
        self.autosave_document();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if reply["format"].is_string() {
            self.status = Status { text: import_note(&name, &reply), error: false };
        } else if let Some(left_out) = reply["left_out"].as_array() {
            let count = reply["left_out_count"].as_u64().map_or(left_out.len(), |n| n as usize);
            // A newer BIMCraft's fields: what this one doesn't know, it can't keep.
            let text =
                tf("{} opened without {} fields this version doesn't know; saving it leaves them out.", &[&name, &count.to_string()]);
            self.status = Status { text, error: false };
        }
        true
    }

    /// The sample house, the model `bimcraft --sample` builds, as a project with no file: Save asks where to put it. Built
    /// all or nothing (a line that fails leaves the project as it was), and its steps are not undo steps, as with `--sample`.
    fn open_sample(&mut self) {
        match self.session.script_atomic(bimcraft_core::sample::SAMPLE) {
            Ok(_) => {
                let model = (*self.session.model).clone();
                self.session.reset(model, None);
                self.retire_autosave();
                self.reopened();
                self.autosave_document();
            }
            Err(e) => self.status = Status { text: e, error: true },
        }
    }

    /// Start over on the model the session holds now (after New and Open), keeping how the
    /// window is set up.
    fn reopened(&mut self) {
        let mut fresh = BimApp::new(std::mem::take(&mut self.session));
        fresh.status = std::mem::take(&mut self.status);
        fresh.os_events = self.os_events.take();
        (fresh.hosted, fresh.save_asked, fresh.skin) = (self.hosted, self.save_asked, self.skin);
        (fresh.own_title_bar, fresh.system_title_bar) = (self.own_title_bar, self.system_title_bar);
        (fresh.thin, fresh.lang, fresh.snaps, fresh.ribbon_min) = (self.thin, self.lang, self.snaps, self.ribbon_min);
        (fresh.compact, fresh.scale) = (self.compact, self.scale);
        (fresh.tools_hidden, fresh.presets) = (std::mem::take(&mut self.tools_hidden), std::mem::take(&mut self.presets));
        fresh.font_finder = self.font_finder;
        fresh.scan_contrast = self.scan_contrast;
        fresh.grid_snapping = self.grid_snapping;
        (fresh.show_origin, fresh.show_view_name) = (self.show_origin, self.show_view_name);
        fresh.canvas.nav.wheel = self.canvas.nav.wheel;
        fresh.custom_shortcuts = std::mem::take(&mut self.custom_shortcuts);
        (fresh.light_canvas, fresh.scheme, fresh.accent) = (self.light_canvas, self.scheme, self.accent);
        (fresh.panel_colour, fresh.canvas_colour, fresh.text_colour) = (self.panel_colour, self.canvas_colour, self.text_colour);
        fresh.styled = self.styled;
        (fresh.page_fonts, fresh.refont) = (self.page_fonts.take(), self.refont);
        fresh.theme_wanted = self.theme_wanted.take();
        fresh.docking = std::mem::take(&mut self.docking);
        fresh.checks = std::mem::take(&mut self.checks);
        (fresh.settings_path, fresh.settings_kept) = (self.settings_path.take(), self.settings_kept.take());
        fresh.window = std::mem::take(&mut self.window);
        #[cfg(feature = "updater")]
        {
            fresh.updates = self.updates.take();
        }
        fresh.revit = self.revit.take();
        // The 3D mouse's reader runs for the window, not the project: without it the cap would move nothing once a project is open.
        fresh.space_mouse = self.space_mouse.take();
        fresh.control = self.control.take();
        fresh.agents = std::mem::take(&mut self.agents);
        fresh.opens = std::mem::take(&mut self.opens);
        (fresh.recent, fresh.recent_path) = (std::mem::take(&mut self.recent), self.recent_path.take());
        fresh.autosave = std::mem::take(&mut self.autosave);
        fresh.card = std::mem::take(&mut self.card);
        fresh.home = self.home.put_away();
        // A new or opened project keeps the plugins and their tabs: they are the user's folder, not the project's, and the
        // session's external commands were already registered. Re-registering puts them back if the project replaced them.
        #[cfg(feature = "plugins")]
        {
            (fresh.plugins, fresh.plugin_tabs) = (std::mem::take(&mut self.plugins), std::mem::take(&mut self.plugin_tabs));
            fresh.plugins_job = self.plugins_job.take();
            fresh.plugins_dialog = self.plugins_dialog;
            fresh.plugins.register(&mut fresh.session);
            fresh.plugin_tabs = fresh.plugins.tabs();
        }
        *self = fresh;
    }

    /// What the system asked for, as File > Open and closing the window do it: a file opened in turn
    /// (the last one wins), and Quit, each asking first about unsaved changes.
    fn os_requests(&mut self, ctx: &Context) {
        // An update being put in place holds the window, so what the system asks waits in its inbox until that has gone through
        // (the window closes) or failed.
        if self.installing() {
            return;
        }
        let events = self.os_events.as_mut().map(|poll| poll()).unwrap_or_default();
        for event in events {
            match event {
                OsEvent::Open(path) => self.opens.push(path),
                OsEvent::Quit => self.guard(Pending::Close, ctx),
            }
        }
        // A file dropped on the window opens as one the system asked for.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        self.opens.extend(dropped);
        // A Revit file being read holds the files that come meanwhile, which are opened in turn once it is over: the last one wins.
        if !self.importing()
            && let Some(path) = self.opens.pop()
        {
            self.opens.clear();
            self.guard(Pending::OpenPath(path), ctx);
        }
    }

    /// Do `p` now, or first ask about unsaved changes.
    fn guard(&mut self, p: Pending, ctx: &Context) {
        if self.dirty() { self.pending = Some(p) } else { self.proceed(p, ctx) }
    }

    fn proceed(&mut self, p: Pending, ctx: &Context) {
        match p {
            Pending::Recover(backup) => self.recover_backup(backup),
            Pending::New => {
                // In the interface's language: its levels named in it.
                if self.run("new", json!({"language": self.lang.project().code()})).is_some() {
                    self.retire_autosave();
                    self.reopened();
                    self.autosave_document();
                }
            }
            Pending::Sample => self.open_sample(),
            Pending::OpenPath(path) => {
                self.open_path(&path);
            }
            Pending::Open => {
                // A hosted window's page reads a Revit model itself, and gives it as the project it becomes.
                let revit = self.revit.is_some() || self.hosted;
                let name = t(if revit { msg!("BIMCraft project, IFC or Revit") } else { "BIMCraft project or IFC" });
                let dialog = crate::dialog::Ask::new().add_filter(name, &revit::open_extensions(revit)).set_title(t("Open"));
                self.ask_file(ctx, dialog, false, |app, path| {
                    app.open_path(&path);
                });
            }
            Pending::Close => {
                self.retire_autosave();
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            #[cfg(feature = "updater")]
            Pending::Update => {
                self.retire_autosave();
                self.install_update();
            }
        }
    }

    // ---------- the top: Quick Access Toolbar, ribbon, Options Bar ----------

    fn quick_access(&mut self, ui: &mut Ui, view: &str) {
        let ctx = ui.ctx().clone();
        let row = ui
            .horizontal(|ui| {
                if !self.hosted && icon_button(ui, Icon::Home, "Home").clicked() {
                    self.toggle_home();
                }
                let file = egui::Button::new(RichText::new(t("File")).color(pal().app_bar_text)).frame(false).min_size(vec2(44.0, 24.0));
                MenuButton::from_button(file).ui(ui, |ui| self.file_menu(ui, &ctx));
                if !self.hosted && icon_button(ui, Icon::Open, "Open").clicked() {
                    self.guard(Pending::Open, &ctx);
                }
                if icon_button(ui, Icon::Save, "Save").clicked() {
                    self.save(&ctx, None);
                }
                ui.separator();
                ui.spacing_mut().item_spacing.x = 0.0;
                if icon_button(ui, Icon::Undo, "Undo").clicked() {
                    self.run("undo", json!({}));
                }
                self.steps_list(ui, false);
                ui.add_space(4.0);
                if icon_button(ui, Icon::Redo, "Redo").clicked() {
                    self.run("redo", json!({}));
                }
                self.steps_list(ui, true);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.separator();
                if icon_button(ui, Icon::View3D, "Default 3D View").clicked() {
                    self.open_3d(None);
                }
                ui.separator();
                if icon_button(ui, Icon::Settings, "Settings").clicked() {
                    self.settings_page = Some(settings::Page::Interface);
                }
                // Find and the project's scale.
                ui.separator();
                if icon_button(ui, Icon::Search, "Find").clicked() {
                    self.find = Some(String::new());
                }
                if !self.hosted {
                    self.scale_chip(ui);
                }
            })
            .response
            .rect;
        // The project and the view, centred where Revit's title bar has them: in the window's middle when they fit between the
        // buttons on the left and, where the window draws its own title bar, the caption buttons on the right; else as near it
        // as that room allows, and cut short where even it is too little.
        let gap = 12.0;
        // Which palettes show, at the bar's right end where an editor keeps its layout's buttons: a closed palette comes back from
        // here. The title's middle stays the window's.
        let end = ui.max_rect().right() + if self.own_bar() { 8.0 - 3.0 * titlebar::BUTTON_WIDTH } else { 0.0 } - 4.0;
        let at = Rect::from_min_size(pos2(end - 26.0, row.center().y - 12.0), vec2(26.0, 24.0));
        let palettes = ui.put(at, |ui: &mut Ui| icon_button(ui, Icon::Interface, msg!("Palettes")));
        egui::Popup::menu(&palettes).show(|ui| self.palette_switches(ui));
        let right = at.left() - gap;
        let left = row.right() + gap;
        let mut title = egui::text::LayoutJob::simple_singleline(
            format!("{} · {view}", self.session.model.name),
            FontId::proportional(12.5),
            pal().app_bar_muted,
        );
        title.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
        let galley = ui.painter().layout_job(title);
        let width = galley.size().x;
        let x = ui.max_rect().center().x.clamp(left + width / 2.0, (right - width / 2.0).max(left + width / 2.0));
        ui.painter().galley(pos2(x - width / 2.0, row.center().y - galley.size().y / 2.0), galley, pal().app_bar_muted);
    }

    /// The arrow beside Undo (or with `redo`, Redo): a list of the steps, the next first, as Revit's. Hovering one marks it
    /// and every step before it, and a click undoes (or redoes) them all.
    fn steps_list(&mut self, ui: &mut Ui, redo: bool) {
        let what = if redo { "Redo" } else { "Undo" };
        let names: Vec<String> = self.session.steps(redo).into_iter().map(step_name).collect();
        let (rect, arrow) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::click());
        let label = t(if redo { msg!("Redo History") } else { msg!("Undo History") });
        arrow.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !names.is_empty(), label));
        let (icon, hover) = style::icon_inks();
        if arrow.hovered() && !names.is_empty() {
            ui.painter().rect_filled(rect, 3.0, hover);
        }
        let ink = if names.is_empty() { icon.gamma_multiply(0.4) } else { icon };
        icons::paint_chevron_at(ui.painter(), rect.center(), 12.0, ink);
        if names.is_empty() {
            return;
        }
        let arrow = arrow.on_hover_text(label);
        let mut chosen = None;
        egui::Popup::menu(&arrow).width(240.0).show(|ui| {
            ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let rows: Vec<Response> = names
                    .iter()
                    .map(|name| {
                        let (_, row) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                        row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
                        row
                    })
                    .collect();
                let reach = rows.iter().position(Response::hovered);
                for (i, (row, name)) in rows.iter().zip(&names).enumerate() {
                    if reach.is_some_and(|h| i <= h) {
                        ui.painter().rect_filled(row.rect, 0.0, pal().select_fill);
                    }
                    ui.painter().text(
                        row.rect.left_center() + vec2(8.0, 0.0),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(13.0),
                        pal().text,
                    );
                    if row.clicked() {
                        chosen = Some(i + 1);
                    }
                }
                ui.separator();
                let n = reach.map_or(1, |h| h + 1);
                let foot = tf(if redo { msg!("Redo {} steps") } else { msg!("Undo {} steps") }, &[&n.to_string()]);
                let foot = if n == 1 { tf(if redo { msg!("Redo {}") } else { msg!("Undo {}") }, &[&names[0]]) } else { foot };
                ui.label(RichText::new(foot).color(pal().muted));
            });
        });
        if let Some(count) = chosen {
            self.run(&what.to_lowercase(), json!({"count": count}));
        }
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin::symmetric(6, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // The tabs, scrolling sideways (with the wheel) in a window too narrow for them; the fold button at the end.
                let room = ui.available_width() - 32.0;
                // The tab that has just been picked (or a contextual tab that has just come) is brought into view: the row
                // scrolls in a window too narrow for it, and its last tabs are out of sight.
                let seen = UiId::new("ribbon-tab-seen");
                let now = tab_key(self.tab);
                let changed = ui.ctx().data(|d| d.get_temp::<u64>(seen)) != Some(now);
                ui.ctx().data_mut(|d| d.insert_temp(seen, now));
                #[cfg(feature = "plugins")]
                let plugin_tabs: Vec<String> = self.plugin_tabs.iter().map(|p| self.plugin_tab_name(p)).collect();
                ScrollArea::horizontal()
                    .id_salt("ribbon-tabs")
                    .max_width(room)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (tab, label) in TABS {
                                let r = tab_button(ui, label, self.tab == tab, false);
                                if changed && self.tab == tab {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, tab);
                            }
                            // The plugins' tabs come after BIMCraft's, each as its manifest declared it.
                            #[cfg(feature = "plugins")]
                            for (i, label) in plugin_tabs.iter().enumerate() {
                                let tab = Tab::Plugin(i);
                                let r = tab_button(ui, label, self.tab == tab, false);
                                if changed && self.tab == tab {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, tab);
                            }
                            if let Some(name) = self.context() {
                                let r = tab_button(ui, &name, self.tab == Tab::Context, true);
                                if changed && self.tab == Tab::Context {
                                    r.scroll_to_me(None);
                                }
                                self.ribbon_tab(&r, Tab::Context);
                            }
                        });
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (icon, label) = if self.ribbon_min {
                        (Icon::Expand, msg!("Expand the Ribbon"))
                    } else {
                        (Icon::Minimize, msg!("Minimize the Ribbon"))
                    };
                    if icon_button(ui, icon, label).clicked() {
                        self.fold_ribbon(!self.ribbon_min);
                    }
                    // Edit Tools on: the ribbon's buttons are boxes to tick until this.
                    if self.editing_tools && ui.button(t("Done")).clicked() {
                        self.editing_tools = false;
                    }
                });
            });
        });
        let base = ui.min_rect();
        if !self.ribbon_min {
            ui.painter().hline(base.x_range(), base.bottom(), Stroke::new(1.0, pal().border));
        }
        self.ribbon_bottom = ui.min_rect().left_bottom();
        if !self.ribbon_min {
            self.ribbon_panels(ui);
        }
    }

    /// A ribbon tab's clicks: one picks it (and, while the ribbon is folded, shows its tools under the tabs, or hides
    /// them again), two fold the ribbon to its tabs or open it again, as Revit's do.
    fn ribbon_tab(&mut self, r: &Response, tab: Tab) {
        if r.double_clicked() {
            self.fold_ribbon(!self.ribbon_min);
        } else if r.clicked() {
            self.ribbon_peek = self.ribbon_min && !(self.ribbon_peek && self.tab == tab);
            self.tab = tab;
        }
    }

    fn fold_ribbon(&mut self, folded: bool) {
        self.ribbon_min = folded;
        self.ribbon_peek = false;
    }

    /// The folded ribbon's tools, under its tabs over everything else, until one is picked or a click lands elsewhere.
    fn ribbon_peek(&mut self, ctx: &Context) {
        let tool = self.tool.clone();
        let frame = Frame::popup(&ctx.global_style()).fill(pal().surface).inner_margin(0.0);
        let shown = Area::new(UiId::new("ribbon-peek"))
            .order(Order::Foreground)
            .fixed_pos(self.ribbon_bottom)
            .show(ctx, |ui| frame.show(ui, |ui| self.ribbon_panels(ui)).response.rect);
        let rect = shown.inner;
        let (released, at) = ctx.input(|i| (i.pointer.primary_released(), i.pointer.interact_pos()));
        let inside = at.is_some_and(|p| rect.contains(p));
        let on_tabs = at.is_some_and(|p| p.y < rect.top());
        // A tool picked, a button done (one that opens a menu keeps it open), or a click elsewhere.
        if self.tool != tool || (released && !Popup::is_any_open(ctx) && (inside || !on_tabs)) {
            self.ribbon_peek = false;
        }
    }

    /// The panels of `tab`, side by side.
    fn tab_content(&mut self, ui: &mut Ui, tab: Tab) {
        match tab {
            Tab::Architecture => self.architecture_tab(ui),
            Tab::Structure => self.structure_tab(ui),
            Tab::Plumbing => self.plumbing_tab(ui),
            Tab::Electrical => self.electrical_tab(ui),
            Tab::Lightning => self.lightning_tab(ui),
            Tab::Data => self.data_tab(ui),
            Tab::Insert => self.insert_tab(ui),
            Tab::Annotate => self.annotate_tab(ui),
            Tab::Massing => self.massing_tab(ui),
            Tab::View => self.view_tab(ui),
            Tab::Manage => self.manage_tab(ui),
            Tab::Modify | Tab::Context => self.modify_tab(ui),
            #[cfg(feature = "plugins")]
            Tab::Plugin(i) => self.plugin_tab(ui, i),
        }
    }

    /// The ribbon tab's panels of tools.
    fn ribbon_panels(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin::symmetric(6, 4)).show(ui, |ui| {
            // A tab wider than the window folds its last panels; one still too wide scrolls sideways, each tab where it was
            // left.
            let key = UiId::new(("ribbon-widths", tab_key(self.tab)));
            let room = ui.available_width();
            egui::ScrollArea::horizontal().id_salt(tab_key(self.tab)).auto_shrink([false, true]).show(ui, |ui| {
                ribbon::tab(ui, key, room, |ui| {
                    ui.horizontal(|ui| self.tab_content(ui, self.tab));
                });
            });
        });
    }

    /// The application menu, under the File tab.
    fn file_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        // A Mac's shortcuts as its symbols (⌘S) where the window has egui's fonts, which have them; else in words (Cmd+S),
        // which IBM Plex has.
        #[cfg(feature = "egui-fonts")]
        let written = |k: &KeyboardShortcut| ctx.format_shortcut(k);
        #[cfg(not(feature = "egui-fonts"))]
        let written = |k: &KeyboardShortcut| k.format(&egui::ModifierNames::NAMES, ctx.os() == egui::os::OperatingSystem::Mac);
        let item = |ui: &mut Ui, label: &str, keys: Option<KeyboardShortcut>| {
            let b = egui::Button::new(t(label));
            ui.add(match keys {
                Some(k) => b.shortcut_text(written(&k)),
                None => b,
            })
            .clicked()
        };
        let cmd = |k: Key| Some(KeyboardShortcut::new(Modifiers::COMMAND, k));
        // A hosted window's files are the page's: it saves the project there, and keeps its settings. What it opens and
        // exports, the page picks and keeps (`dialog.rs`).
        if self.hosted {
            if item(ui, "Save", cmd(Key::S)) {
                self.save(ctx, None);
            }
            if item(ui, msg!("Open IFC or Revit Model…"), None) {
                self.guard(Pending::Open, ctx);
            }
            self.export_menu(ui, ctx);
            ui.separator();
            if item(ui, msg!("Settings…"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))) {
                self.settings_page = Some(settings::Page::Interface);
            }
            return;
        }
        if item(ui, "New", cmd(Key::N)) {
            self.guard(Pending::New, ctx);
        }
        if item(ui, msg!("Open…"), cmd(Key::O)) {
            self.guard(Pending::Open, ctx);
        }
        self.recent_menu(ui, ctx);
        if item(ui, "Save", cmd(Key::S)) {
            self.save(ctx, None);
        }
        if item(ui, msg!("Save As…"), Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S))) {
            self.save_as(ctx, None);
        }
        if !self.autosave.candidates.is_empty() && item(ui, msg!("Recover Autosave…"), None) {
            self.autosave.show_recovery = true;
        }
        self.export_menu(ui, ctx);
        self.report_menu(ui);
        #[cfg(feature = "updater")]
        self.update_menu(ui);
        ui.separator();
        if item(ui, msg!("Settings…"), Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))) {
            self.settings_page = Some(settings::Page::Interface);
        }
        ui.separator();
        if item(ui, msg!("Exit"), None) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// File > Export: the model as IFC, the view as a picture, the sheets as PDF, the 3D scene as glTF.
    fn export_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        submenu(t("Export")).ui(ui, |ui| {
            if ui.button(t("IFC…")).clicked() {
                self.export(ctx, "IFC4", "ifc");
            }
            if ui.button(t("View as PNG…")).clicked() {
                self.export(ctx, msg!("PNG image"), "png");
            }
            if ui.button(t("Plan as SVG…")).clicked() {
                self.export(ctx, msg!("SVG drawing"), "svg");
            }
            if ui.button(t("Sheets as PDF…")).clicked() {
                self.export(ctx, "PDF", "pdf");
            }
            if ui.button(t("3D Scene as glTF…")).clicked() {
                self.export(ctx, msg!("glTF binary"), "glb");
            }
        });
    }

    /// Ribbon buttons that pick tools, under the panel's name.
    /// The Select panel each tab starts with: Modify, and Revit's Filter while something is selected, to keep some of it by
    /// category or by the values of a parameter they all have.
    fn select_panel(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Select"), |ui| {
            if big_button(ui, Icon::Modify, "Modify", structure::same_tool(&self.tool, &Tool::Modify), msg!("Modify (MD): select elements"))
                .clicked()
            {
                self.start_tool(Tool::Modify);
            }
            if !self.selection.is_empty() {
                let tip = msg!("Filter: keep some of the selection, by category or by the values of a parameter they all have");
                if big_button(ui, Icon::Filter, "Filter", self.selection_filter.is_some(), tip).clicked() {
                    self.open_filter();
                }
            }
        });
    }

    fn tools(&mut self, ui: &mut Ui, name: &str, tools: Tools) {
        panel(ui, name, |ui| {
            for (icon, label, tip, tool) in tools {
                let active = structure::same_tool(&self.tool, tool);
                if big_button(ui, *icon, label, active, tip).clicked() {
                    self.start_tool(tool.clone());
                }
            }
        });
    }

    fn massing_tab(&mut self, ui: &mut Ui) {
        let tip = msg!("Mass (MS): click the corners of the footprint; Enter, or the first corner again, finishes; Esc cancels");
        let shown = !self.graphics_now().resolved(&self.session.model).categories.get("mass").is_some_and(|o| o.hidden);
        panel(ui, msg!("Conceptual Mass"), |ui| {
            if big_button(ui, Icon::Mass, "Mass", structure::same_tool(&self.tool, &Tool::Mass(Vec::new())), tip).clicked() {
                self.start_tool(Tool::Mass(Vec::new()));
            }
            let tip = msg!("Show Mass (MH): hides the masses in this view, or shows them again");
            if big_button(ui, Icon::Visibility, "Show Mass", shown, tip).clicked() {
                self.show_mass();
            }
        });
        let selected = !self.selected_masses().is_empty();
        panel(ui, msg!("Model by Face"), |ui| {
            ui.add_enabled_ui(selected, |ui| {
                for (icon, label, tip, command) in [
                    (Icon::Floor, "Mass Floors", msg!("Mass Floors (MF): a floor at each level the selected masses cross"), "mass_floors"),
                    (
                        Icon::Wall,
                        "Wall by Face",
                        msg!("Wall by Face (WB): a wall on each edge of the selected masses' footprints"),
                        "wall_by_face",
                    ),
                    (Icon::Floor, "Floor by Face", msg!("Floor by Face (FB): a floor on the selected masses' top faces"), "floor_by_face"),
                    (Icon::Roof, "Roof by Face", msg!("Roof by Face (RB): a roof on the selected masses' top faces"), "roof_by_face"),
                    (
                        Icon::Align,
                        "Update to Face",
                        msg!("Update to Face (UF): the walls, floors and roofs made by face from the selected masses follow them again"),
                        "update_by_face",
                    ),
                ] {
                    if big_button(ui, icon, label, false, tip).clicked() {
                        self.by_mass(command);
                    }
                }
            });
        });
        self.site_panels(ui);
    }

    /// Show Mass (MH): the masses in the view on screen, hidden if they show and shown if they're hidden.
    fn show_mass(&mut self) {
        match self.graphic_view() {
            Some(view) => {
                self.run("show_mass", json!({"view": view}));
            }
            None => self.status = Status { text: t("This view has no graphics to override.").to_string(), error: true },
        }
    }

    /// The masses selected.
    fn selected_masses(&self) -> Vec<Id> {
        self.selection.iter().copied().filter(|id| matches!(self.session.model.elements.get(id), Some(Element::Mass(_)))).collect()
    }

    /// A Model by Face button or shortcut: `command` for each mass selected.
    fn by_mass(&mut self, command: &str) {
        let masses = self.selected_masses();
        if masses.is_empty() {
            self.status = Status { text: t("Select a mass first.").into(), error: true };
        }
        for id in masses {
            self.run(command, json!({"mass": id}));
        }
    }

    fn architecture_tab(&mut self, ui: &mut Ui) {
        self.select_panel(ui);
        self.tools(
            ui,
            msg!("Build"),
            &[
                (Icon::Wall, "Wall", msg!("Wall (WA): each click builds a wall; Enter or Esc ends the chain"), Tool::Wall(Vec::new())),
                (Icon::Door, "Door", msg!("Door (DR): click a wall"), Tool::Door),
                (Icon::Window, "Window", msg!("Window (WN): click a wall"), Tool::Window),
                (Icon::Column, "Column", msg!("Column (CL): click where it stands"), Tool::Column),
                (Icon::Component, "Component", msg!("Component (CM): a family's type, where you click; Space turns it"), Tool::Component),
                (Icon::Roof, "Roof", msg!("Roof by Footprint: click inside the walls to roof them, on the level above"), Tool::Roof),
                (
                    Icon::ExtrusionRoof,
                    msg!("Roof by Extrusion"),
                    msg!(
                        "Roof by Extrusion: click a line's start and end, then how far across it's swept; its profile rises from the line"
                    ),
                    Tool::ExtrusionRoof(Vec::new()),
                ),
                (Icon::Floor, "Floor", msg!("Floor: click inside the walls to floor them"), Tool::Floor),
                (Icon::Ceiling, "Ceiling", msg!("Ceiling: click inside a room for a ceiling out to its walls"), Tool::Ceiling),
            ],
        );
        self.tools(
            ui,
            msg!("Circulation"),
            &[
                (
                    Icon::Stair,
                    "Stair",
                    msg!("Stair: click where it starts, then where its run ends; a landing turns it"),
                    Tool::Stair(Vec::new()),
                ),
                (
                    Icon::Ramp,
                    "Ramp",
                    msg!("Ramp: click where it starts, then where its run ends; a landing turns it"),
                    Tool::Ramp(Vec::new()),
                ),
                (
                    Icon::Railing,
                    "Railing",
                    msg!("Railing: click a stair or a ramp for one down each side, or click a path and press Enter"),
                    Tool::Railing(Vec::new()),
                ),
            ],
        );
        self.tools(
            ui,
            msg!("Room & Area"),
            &[
                (Icon::Room, "Room", msg!("Room (RM): click inside walls"), Tool::Room),
                (
                    Icon::RoomSeparator,
                    "Room Separator",
                    msg!("Room Separator: lines that bound rooms as walls do, to split an open space; click from point to point"),
                    Tool::RoomSeparator(None),
                ),
            ],
        );
        self.tools(
            ui,
            msg!("Opening"),
            &[
                (
                    Icon::Shaft,
                    msg!("Shaft"),
                    msg!("Shaft Opening: click two opposite corners; it cuts the floors, ceilings and roofs it passes through"),
                    Tool::Shaft(None),
                ),
                (
                    Icon::Dormer,
                    "Dormer",
                    msg!("Dormer: click a roof's slope where its front wall goes; the roof opens under it"),
                    Tool::Dormer,
                ),
            ],
        );
        // Datum, and the Work Plane's Set beside its levels and grids.
        let datum: Tools = &[
            (Icon::Level, "Level", msg!("Level (LL): a new level at an elevation"), Tool::Level),
            (Icon::Grid, "Grid", msg!("Grid (GR): click its two ends"), Tool::Grid(None)),
        ];
        panel(ui, msg!("Datum"), |ui| {
            for (icon, label, tip, tool) in datum {
                let active = structure::same_tool(&self.tool, tool);
                if big_button(ui, *icon, label, active, tip).clicked() {
                    self.start_tool(tool.clone());
                }
            }
            self.work_plane_set(ui);
        });
    }

    /// Annotate > Dimension, as Revit's: Aligned and Linear big, the others small beside them, two rows of three.
    fn dimension_panel(&mut self, ui: &mut Ui) {
        let big: [(Icon, &str, &str, Tool); 2] = [
            (
                Icon::Dimension,
                msg!("Aligned"),
                msg!(
                    "Aligned Dimension (DI): pick parallel wall faces, wall ends, doors, windows or grids, or endpoints, then click \
                     where the dimension line goes"
                ),
                Tool::Dimension(DimMode::Aligned, Vec::new()),
            ),
            (
                Icon::Linear,
                msg!("Linear"),
                msg!(
                    "Linear Dimension: pick endpoints (wall ends and corners, line ends, grid ends), then click where it goes: \
                     horizontal or vertical as the cursor lies"
                ),
                Tool::Dimension(DimMode::Auto, Vec::new()),
            ),
        ];
        let small: [(Icon, &str, &str, Tool); 6] = [
            (
                Icon::Angular,
                msg!("Angular"),
                msg!("Angular Dimension: pick two lines that meet, then click where the arc goes"),
                Tool::Angular(Vec::new()),
            ),
            (
                Icon::Radial,
                msg!("Radial"),
                msg!("Radial Dimension: pick a round column, a curved wall, a circle or an arc, then click where the radius goes"),
                Tool::Round(DimKind::Radial, Vec::new()),
            ),
            (
                Icon::Diameter,
                msg!("Diameter"),
                msg!("Diameter Dimension: pick a round column, a curved wall, a circle or an arc, then click where it goes"),
                Tool::Round(DimKind::Diameter, Vec::new()),
            ),
            (
                Icon::ArcLength,
                msg!("Arc Length"),
                msg!("Arc Length Dimension: pick a curved wall or an arc, then click where the dimension's arc goes"),
                Tool::Round(DimKind::ArcLength, Vec::new()),
            ),
            (
                Icon::Spot,
                "Spot Elevation",
                msg!("Spot Elevation (EL): click a floor, a roof or a stair; in a section, a point"),
                Tool::Spot,
            ),
            (
                Icon::SpotSlope,
                msg!("Spot Slope"),
                msg!("Spot Slope: click a floor or a roof in a plan; an arrow points downhill with how steeply its top falls"),
                Tool::SpotSlope,
            ),
        ];
        panel(ui, "Dimension", |ui| {
            for (icon, label, tip, tool) in &big {
                if big_button(ui, *icon, label, structure::same_tool(&self.tool, tool), tip).clicked() {
                    self.start_tool(tool.clone());
                }
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in small.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (icon, label, tip, tool) in row {
                            if small_button(ui, *icon, label, structure::same_tool(&self.tool, tool), tip).clicked() {
                                self.start_tool(tool.clone());
                            }
                        }
                    });
                }
            });
        });
    }

    fn annotate_tab(&mut self, ui: &mut Ui) {
        self.select_panel(ui);
        self.dimension_panel(ui);
        self.detail_panel(ui);
        self.region_panel(ui);
        self.tools(
            ui,
            "Text",
            &[(Icon::Text, "Text", msg!("Text (TX): type the note in the Options Bar, then click where it goes"), Tool::Text)],
        );
        self.tools(
            ui,
            "Measure",
            &[(
                Icon::Measure,
                "Measure",
                msg!("Measure (ME): click two points; their length and angle read beside the ruler between them"),
                Tool::Measure(None),
            )],
        );
        panel(ui, "Tag", |ui| {
            let tip =
                msg!("Tag by Category (TG): click an element to tag it with its type, a door's or window's mark, or the field in Label");
            if big_button(ui, Icon::Tag, "Tag by Category", self.tool == Tool::Tag, tip).clicked() {
                self.start_tool(Tool::Tag);
            }
            if big_button(ui, Icon::TagAll, "Tag All", false, "Tag every door and window in this view that has no tag").clicked() {
                self.tag_all();
            }
        });
    }

    /// Duplicate View (DP): a new plan view of the plan on screen (`mode` as the command says), opened as the kind of plan it
    /// was made from.
    pub(crate) fn duplicate_plan(&mut self, mode: &'static str) {
        match self.plan_key() {
            Some(key) => self.duplicate_view_of(key, mode),
            None => self.status = Status { text: t("Duplicate View: open a plan first").into(), error: true },
        }
    }

    /// A duplicate of plan `key`, opened as the kind of plan (floor or ceiling) on screen.
    pub(crate) fn duplicate_view_of(&mut self, key: Id, mode: &'static str) {
        if let Some(id) = self.run("duplicate_view", json!({"view": key, "mode": mode})).and_then(|r| r["id"].as_u64()) {
            self.open_sheet(if matches!(self.sheet, Sheet::Ceiling(_)) { Sheet::Ceiling(id) } else { Sheet::Plan(id) });
        }
    }

    /// Tag All: the doors and windows of the plan's level that have no tag yet.
    fn tag_all(&mut self) {
        if let Sheet::Plan(level) = self.sheet {
            self.run("tag", json!({"level": level, "leader": self.tag_leader}));
        }
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Graphics", |ui| {
            let tip = msg!("Visibility/Graphics (VV): what this view hides, and how it draws the rest, by category, filter and element");
            let on = self.graphic_view().is_some();
            if ui
                .add_enabled_ui(on, |ui| big_button(ui, Icon::Visibility, "Visibility/Graphics", self.vg_dialog.is_some(), tip))
                .inner
                .clicked()
            {
                self.open_graphics(0);
            }
            let tip = msg!("Filters (FT): rules that pick out elements for a view to hide or draw another way");
            if big_button(ui, Icon::Filter, "Filters", self.filters_dialog.is_some(), tip).clicked() {
                self.filters_dialog = Some(graphics::FiltersDialog::default());
            }
            let tip = msg!("View Templates (VT): a view's settings under a name, for other views to follow");
            if big_button(ui, Icon::Template, "View Templates", self.templates_dialog.is_some(), tip).clicked() {
                self.open_templates();
            }
            if big_button(ui, Icon::ThinLines, "Thin Lines", self.thin, "Thin Lines (TL): every line one pixel wide").clicked() {
                self.thin = !self.thin;
            }
            let dark = style::dark();
            let tip = msg!("Choose a theme, including a dark interface with a light canvas");
            let r = big_button(ui, Icon::Theme, "Dark Mode", dark, tip);
            icons::paint_chevron_at(ui.painter(), r.rect.right_top() + vec2(-8.0, 9.0), 12.0, pal().muted);
            ribbon::choices(ui, &r, false, |ui| self.theme_choices(ui, true));
            // A plan's or a section's crop region: on to what it shows, or off.
            let view = match self.sheet {
                Sheet::Plan(id) | Sheet::Section(id) => Some(id),
                _ => None,
            };
            let cropped = view.is_some_and(|id| match self.session.model.elements.get(id) {
                Some(Element::Level(_) | Element::PlanView(_)) => self.session.model.crop_of(id).is_some(),
                Some(Element::Section(v) | Element::Elevation(v)) => v.crop.is_some(),
                _ => false,
            });
            let tip = msg!("Crop View: show only the crop region; drag its edges by their grips");
            if ui.add_enabled_ui(view.is_some(), |ui| big_button(ui, Icon::Crop, "Crop View", cropped, tip)).inner.clicked()
                && let Some(id) = view
            {
                self.run("crop", if cropped { json!({"view": id, "region": null}) } else { json!({"view": id}) });
            }
        });
        // Revit's Presentation: Render, of the 3D view in front. A hosted window's page renders it (`host.rs`); a window no
        // page hosts has no renderer to ask.
        if self.offers_render() {
            panel(ui, msg!("Presentation"), |ui| {
                let tip = msg!("Render: a picture of this 3D view or camera, made by the renderer of the page the model is kept in");
                let on = self.sheet == Sheet::View3D;
                if ui.add_enabled_ui(on, |ui| big_button(ui, Icon::Render, "Render", false, tip)).inner.clicked() {
                    let view = self.active_3d();
                    self.ask_host_to_render(ui.ctx(), view);
                }
            });
        }
        panel(ui, "Create", |ui| {
            if big_button(ui, Icon::View3D, "3D View", self.sheet == Sheet::View3D, "Open the default 3D view").clicked() {
                self.open_3d(None);
            }
            let tip = msg!(
                "Duplicate View (DP): a new plan of this level with its own annotation; with detailing, it starts with this plan's; as a dependent, it follows this plan's range and graphics"
            );
            let r = ui.add_enabled_ui(self.plan_key().is_some(), |ui| big_button(ui, Icon::Copy, "Duplicate View", false, tip)).inner;
            ribbon::choices(ui, &r, false, |ui| {
                for (label, mode) in
                    [("Duplicate", "duplicate"), ("Duplicate with Detailing", "detailing"), ("Duplicate as Dependent", "dependent")]
                {
                    if ui.button(t(label)).clicked() {
                        self.duplicate_plan(mode);
                    }
                }
            });
            let tip = msg!("Section (CR): click its start, then its end; it looks to the left of the line");
            if big_button(ui, Icon::Section, "Section", matches!(self.tool, Tool::Section(_)), tip).clicked() {
                self.start_tool(Tool::Section(None));
            }
            let tip = msg!("Elevation: click beside the building, on the side to look at");
            if big_button(ui, Icon::Elevation, "Elevation", self.tool == Tool::Elevation, tip).clicked() {
                self.start_tool(Tool::Elevation);
            }
            let tip = msg!("Camera: click where its eye stands, then what it looks at: a 3D view in perspective from there");
            if big_button(ui, Icon::Camera, "Camera", matches!(self.tool, Tool::Camera(_)), tip).clicked() {
                self.start_tool(Tool::Camera(None));
            }
            let r = big_button(ui, Icon::Schedule, "Schedules", false, "Make a schedule, or open one");
            ribbon::choices(ui, &r, false, |ui| {
                if ui.button(t("Schedule/Quantities…")).clicked() {
                    self.new_schedule = Some(properties::NewSchedule::default());
                }
                ui.separator();
                for (cat, name) in SCHEDULES {
                    if ui.button(t(name)).clicked() {
                        self.open_sheet(Sheet::Schedule(cat));
                    }
                }
                let made: Vec<(Id, String)> = self.schedules().into_iter().map(|(id, n)| (id, n.to_string())).collect();
                for (id, name) in made {
                    if ui.button(name).clicked() {
                        self.open_sheet(Sheet::Table(id));
                    }
                }
            });
        });
        panel(ui, msg!("Sheet Composition"), |ui| {
            if big_button(ui, Icon::Sheet, "Sheet", false, "A new sheet, with a title block").clicked()
                && let Some(id) = self.run("sheet", json!({})).and_then(|made| made["id"].as_u64())
            {
                self.open_sheet(Sheet::Paper(id));
            }
            let tip = msg!("Title Blocks (TB): your own, read from a DXF or typed, with fields each sheet or the whole project fills in");
            if big_button(ui, Icon::Sheet, "Title Blocks", self.title_blocks_dialog.is_some(), tip).clicked() {
                self.open_title_blocks(None);
            }
            let sheet = match self.sheet {
                Sheet::Paper(id) => Some(id),
                _ => None,
            };
            let tip = msg!("Put a view on this sheet: one not on a sheet yet");
            let r = ui.add_enabled_ui(sheet.is_some(), |ui| big_button(ui, Icon::PlaceView, "Place View", false, tip)).inner;
            if let Some(sheet) = sheet {
                let views = self.unplaced_views();
                ribbon::choices(ui, &r, false, |ui| {
                    if views.is_empty() {
                        ui.label(t("Every view is on a sheet."));
                    }
                    for (view, label) in views {
                        if ui.button(label).clicked() {
                            let mut p = json!({"sheet": sheet, "view": view});
                            // The 3D view as it's turned on screen.
                            if let (Some("{3D}"), Some(c)) = (p["view"].as_str(), self.orbit.camera) {
                                (p["azimuth"], p["elevation"]) = (json!(c.azimuth), json!(c.elevation));
                            }
                            self.run("place", p);
                        }
                    }
                });
            }
        });
        panel(ui, "Windows", |ui| {
            if big_button(ui, Icon::CloseInactive, "Close Inactive", false, "Close every view but this one").clicked() {
                let active = self.sheet;
                self.views.retain(|v| *v == active);
                self.floating.retain(|f| f.sheet == active);
            }
            let tip = msg!("Show or hide the palettes, and pick the interface's language");
            let r = big_button(ui, Icon::Interface, "User Interface", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                for p in self.palettes() {
                    let mut on = self.docking.shown(p);
                    if ui.checkbox(&mut on, t(p.name())).changed() {
                        self.show_palette(p, on);
                    }
                }
                // A hosted window speaks its page's language: the page decides it.
                if !self.hosted {
                    ui.separator();
                    ui.label(RichText::new(t("Language")).color(pal().muted));
                    for l in Lang::ALL {
                        if ui.selectable_label(self.lang == l, l.label()).clicked() {
                            self.set_lang(ui.ctx(), l);
                        }
                    }
                }
            });
        });
    }

    /// Manage: the project's settings, as Revit's tab has them.
    /// Insert: what comes into the project from other files, as Revit's tab. Link: other projects, and laser scans.
    fn insert_tab(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Link"), |ui| {
            let tip = msg!(
                "Link Model (LK): another project, a BIMCraft file or an IFC, shown in this one as a reference that can't be edited: halftone in the plans, the sections and the 3D view, and snapped to"
            );
            if big_button(ui, Icon::Link, "Link Model", false, tip).clicked() {
                self.link_dialog = true;
            }
            let tip = msg!(
                "Point Cloud: laser scans (PTS, XYZ, PLY, LAS or E57) brought into the project, one file or a survey's stations as one scan, drawn in the 3D view, plans and sections and snapped to"
            );
            if big_button(ui, Icon::PointCloud, "Point Cloud", self.cloud_box.is_some(), tip).clicked() {
                self.cloud_dialog = true;
            }
            let tip = msg!(
                "Link CAD: a DWG or DXF drawing shown in the plans of a level, read again from its file on Reload and when the project opens"
            );
            if big_button(ui, Icon::LinkCad, "Link CAD", self.cad_box.as_ref().is_some_and(|b| b.linked()), tip).clicked() {
                self.cad_dialog = Some(true);
            }
        });
        panel(ui, msg!("Import"), |ui| {
            let tip = msg!(
                "Import CAD: a DWG or DXF drawing brought into the project, drawn under the model in the plans of a level, its layers, colours and linetypes kept, and snapped to"
            );
            if big_button(ui, Icon::ImportCad, "Import CAD", self.cad_box.as_ref().is_some_and(|b| !b.linked()), tip).clicked() {
                self.cad_dialog = Some(false);
            }
            let tip = msg!(
                "Import PDF: a page of a PDF placed in the view, at the resolution you pick, its vector lines snapped to as you trace over it"
            );
            let placing_pdf = self.pdf_box.is_some() || (self.tool == Tool::PlaceImage && self.placing_command() == Some("import_pdf"));
            if big_button(ui, Icon::ImportPdf, "Import PDF", placing_pdf, tip).clicked() {
                self.image_dialog = Some(true);
            }
            let tip = msg!("Image: a picture (PNG, JPEG or BMP) placed in the view, behind the model");
            let placing_image = self.tool == Tool::PlaceImage && self.placing_command() == Some("import_image");
            if big_button(ui, Icon::InsertImage, "Image", placing_image, tip).clicked() {
                self.image_dialog = Some(false);
            }
            let tip = msg!("Manage Images: the images and PDFs the project holds; place one again, or delete it");
            if big_button(ui, Icon::ManageImages, "Manage Images", self.images_window, tip).clicked() {
                self.images_window = true;
            }
        });
    }

    fn manage_tab(&mut self, ui: &mut Ui) {
        panel(ui, "Settings", |ui| {
            let tip = msg!("Materials: the project's library, each material's colour in 3D and the patterns its cut and its surfaces show");
            if big_button(ui, Icon::Materials, "Materials", self.materials_dialog.is_some(), tip).clicked() {
                self.materials_dialog = Some(properties::MaterialsDialog::new(&self.session.model));
            }
            let tip = msg!("Favorites: the materials and families you keep for every project; add one to this project with a click");
            if big_button(ui, Icon::Favorite, "Favorites", self.favorites_dialog.is_some(), tip).clicked() {
                self.open_favorites();
            }
            #[cfg(feature = "plugins")]
            {
                let tip = msg!(
                    "Plugins: the WASM extensions in your plugin folder, what became of each, their log, and switching them on and off"
                );
                if !self.hosted && big_button(ui, Icon::Code, "Plugins", self.plugins_dialog, tip).clicked() {
                    self.plugins_dialog = true;
                }
            }
            let tip = msg!("Project Parameters: values of your own each element of their categories holds, for schedules and IFC");
            if big_button(ui, Icon::Parameters, "Project Parameters", self.parameters_dialog.is_some(), tip).clicked() {
                self.parameters_dialog = Some(properties::ParametersDialog::new(&self.session.model));
            }
            // What annotation draws with: pens, line styles and patterns, fill patterns; one button that lists them.
            let open = self.line_weights_dialog.is_some() || self.line_styles_dialog.is_some() || self.fill_patterns_dialog.is_some();
            let tip = msg!("Additional Settings: line weights, line styles, line patterns, fill patterns and temporary dimensions");
            let r = big_button(ui, Icon::AnnotationSettings, "Additional Settings", open, tip);
            ribbon::choices(ui, &r, false, |ui| {
                // As wide as its longest name, not the window.
                ui.set_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let tip = msg!("Line Weights: what each pen draws on paper, the model's at each scale and the annotation's at any");
                if list_row(ui, Icon::LineWeights, "Line Weights", false, tip).clicked() {
                    self.line_weights_dialog = Some(lineweights::LineWeightsDialog::new(&self.session.model));
                }
                let tip = msg!("Line Styles: the pen, colour and line pattern detail lines draw in");
                if list_row(ui, Icon::ThinLines, "Line Styles", false, tip).clicked() {
                    self.line_styles_dialog = Some(linestyles::LineStylesDialog::new(&self.session.model));
                }
                let tip = msg!("Line Patterns: the dashes, dots, spaces and shapes lines are drawn with");
                if list_row(ui, Icon::LinePatterns, "Line Patterns", false, tip).clicked() {
                    self.line_styles_dialog = Some(linestyles::LineStylesDialog::patterns(&self.session.model));
                }
                let tip = msg!("Fill Patterns: the patterns filled regions and materials fill with, simple or made from lines drawn");
                if list_row(ui, Icon::FillPatterns, "Fill Patterns", false, tip).clicked() {
                    self.fill_patterns_dialog = Some(fillpatterns::FillPatternsDialog::new(&self.session.model));
                }
                let tip = msg!(
                    "Temporary Dimensions: what a selected wall's, door's and window's temporary dimensions measure to and from: centerlines, faces, the core, openings"
                );
                if list_row(ui, Icon::Dimension, "Temporary Dimensions", self.temp_dim_dialog.is_some(), tip).clicked() {
                    self.open_temp_dim_properties();
                }
            });
            let tip = msg!(
                "Purge Unused (PU): take out the types, families and the rest nothing in the project uses; each category keeps a type"
            );
            if big_button(ui, Icon::Delete, "Purge Unused", self.purge_dialog.is_some(), tip).clicked() {
                self.open_purge();
            }
            let now_std = self.session.model.standard();
            let tip = tf("Technical Standards: configure drawing standards (NBR, ISO, ANSI, EU); now {}", &[t(now_std.short_name())]);
            if big_button(ui, Icon::Settings, "Technical Standards", self.standards_dialog.is_some(), &tip).clicked() {
                self.standards_dialog = Some(standards::StandardsDialog::new(&self.session.model));
            }
            let tip = msg!(
                "Electrical Settings: the voltages, distribution systems and wire types circuits and panels refer to; sections in mm²"
            );
            if big_button(ui, Icon::Wire, "Electrical Settings", self.electrical_dialog.is_some(), tip).clicked() {
                self.electrical_dialog = Some(electrical::ElectricalDialog::new(&self.session.model));
            }
            // Object styles, snaps and units, stacked.
            stacked(ui, |ui| {
                let tip = msg!("Object Styles: the pen each category draws in, seen and cut, in every view");
                if stacked_button(ui, Icon::ObjectStyles, "Object Styles", self.object_styles_dialog.is_some(), tip).clicked() {
                    self.object_styles_dialog = Some(objectstyles::ObjectStylesDialog::new(&self.session.model));
                }
                let tip = msg!(
                    "Snaps: what a picked point sticks to (ends, middles, crossings, centres, perpendiculars), and the angles it steps by; SE, SM, SI, SC, SP and SN take one alone for the next point, SO none"
                );
                if stacked_button(ui, Icon::Snaps, "Snaps", self.show_snaps, tip).clicked() {
                    self.show_snaps = true;
                }
                let now = self.session.model.units;
                let tip = tf("Project Units (UN): how lengths show and read; now {}", &[&t(now.label()).to_lowercase()]);
                let r = stacked_button(ui, Icon::Units, msg!("Project Units"), false, &tip);
                let asked = self.open_units;
                let opened = ribbon::choices(ui, &r, asked, |ui| {
                    for u in Units::ALL {
                        if ui.selectable_label(u == now, t(u.label())).clicked() {
                            self.run("units", json!({"length": u.name()}));
                        }
                    }
                });
                self.open_units &= !opened;
            });
            let tip = msg!("Keyboard Shortcuts (KS): customize, edit, and load shortcut keys");
            if big_button(ui, Icon::Keyboard, "Shortcuts", self.show_shortcuts, tip).clicked() {
                self.show_shortcuts = true;
            }
            // Where a room's area is measured, as Revit's Room Area Computation.
            let now = self.session.model.room_area;
            let tip = tf("Room Area Computation: where room areas are measured; now {}", &[&t(now.label()).to_lowercase()]);
            let r = big_button(ui, Icon::Settings, "Room Area Computation", false, &tip);
            ribbon::choices(ui, &r, false, |ui| {
                for a in RoomArea::ALL {
                    if ui.selectable_label(a == now, t(a.label())).clicked() {
                        self.run("room_area", json!({"at": a.name()}));
                    }
                }
            });
            // What the drawings say, and how new things are named.
            let now = self.session.model.language;
            let tip = tf("Project Language: what the drawings say and how new things are named; now {}", &[now.label()]);
            let r = big_button(ui, Icon::Text, "Project Language", false, &tip);
            ribbon::choices(ui, &r, false, |ui| {
                for l in Language::ALL {
                    if ui.selectable_label(l == now, l.label()).clicked() {
                        self.run("language", json!({"code": l.code()}));
                    }
                }
            });
        });
        panel(ui, msg!("Links"), |ui| {
            let tip = "Link Model (LK): another project, a BIMCraft file or an IFC, shown in this one as a reference that can't be edited: halftone in the plans, the sections and the 3D view, and snapped to";
            if big_button(ui, Icon::Link, "Link Model", false, tip).clicked() {
                self.link_dialog = true;
            }
        });
        panel(ui, "Family Editor", |ui| {
            let tip = msg!("New Family: start a family of your own, of a category, to edit its parameters, types and forms");
            let r = big_button(ui, Icon::FamilyEditor, "New Family", false, tip);
            ribbon::choices(ui, &r, false, |ui| {
                for c in editor::CATEGORIES {
                    if ui.button(editor::category_label(c)).clicked() {
                        self.new_family(c);
                    }
                }
            });
            let tip = msg!("Edit Family: the family of what is selected, or of the component being placed");
            let family = self.family_in_hand();
            if ui.add_enabled_ui(family.is_some(), |ui| big_button(ui, Icon::FamilyEditor, "Edit Family", false, tip)).inner.clicked()
                && let Some(f) = family
            {
                self.edit_family(&f);
            }
        });
        panel(ui, msg!("Inquiry"), |ui| {
            let tip = msg!("Select by ID: select elements by their IDs, GUIDs or IFC GlobalIds, as many as you paste, and show them");
            if big_button(ui, Icon::SelectById, "Select by ID", self.select_by_id.is_some(), tip).clicked() {
                self.open_select_by_id();
            }
            let tip = self.selection_ids_tip();
            let some = !self.selection.is_empty();
            if ui
                .add_enabled_ui(some, |ui| big_button(ui, Icon::IdsOfSelection, "IDs of Selection", self.ids_of_selection.is_some(), &tip))
                .inner
                .clicked()
            {
                self.open_ids_of_selection();
            }
            let n = self.project_warnings().len();
            let tip = if n == 0 {
                t(msg!("Review Warnings: what is questionable in the project without being wrong. There are none")).to_string()
            } else {
                tf(
                    msg!("Review Warnings: what is questionable in the project without being wrong, with Show to select it. There are {}"),
                    &[&n.to_string()],
                )
            };
            if big_button(ui, Icon::Warning, "Review Warnings", self.warnings_dialog.is_some(), &tip).clicked() {
                self.open_warnings();
            }
        });
        panel(ui, "Phasing", |ui| {
            let tip = msg!("Phasing (PH): the project's phases in order, the phase filters that show them, and how a filter draws each");
            if big_button(ui, Icon::Phasing, "Phasing", self.phasing_dialog.is_some(), tip).clicked() {
                self.phasing_dialog = Some(phasing::PhasingDialog::default());
            }
        });
        panel(ui, "Design Options", |ui| {
            let tip = msg!("Design Options (DO): sets of options for a part of the design, and the one new elements are made in");
            if big_button(ui, Icon::Options, "Design Options", self.options_dialog.is_some(), tip).clicked() {
                self.options_dialog = Some(phasing::OptionsDialog::default());
            }
        });
    }

    /// Revit's Reveal Hidden Elements panel, while the view is in Reveal Hidden Elements mode: Unhide Element and Unhide Category, on
    /// only for a selection the view hides by element or by category, and Toggle Reveal Hidden Elements Mode to leave.
    fn reveal_hidden_panel(&mut self, ui: &mut Ui) {
        if !self.revealing() {
            return;
        }
        panel(ui, "Reveal Hidden Elements", |ui| {
            // What can be picked while hidden is what the mode draws: its selection can be brought back.
            let (elements, categories) = self.hidden_selection();
            let tip = msg!("Unhide Element (EU): bring back the selected elements this view hides");
            let unhide = |ui: &mut Ui, on: bool, label: &str, tip: &str| {
                ui.add_enabled_ui(on, |ui| big_button(ui, Icon::Visibility, label, false, tip)).inner.clicked()
            };
            if unhide(ui, !elements.is_empty(), msg!("Unhide Element"), tip) {
                self.unhide_selection(false);
            }
            let tip = msg!("Unhide Category (VU): bring back every category the selection is of that this view hides");
            if unhide(ui, !categories.is_empty(), msg!("Unhide Category"), tip) {
                self.unhide_selection(true);
            }
            let tip = msg!("Toggle Reveal Hidden Elements Mode (RH): leave the mode; what the view hides is hidden again");
            if big_button(ui, Icon::Reveal, msg!("Toggle Reveal Hidden Elements Mode"), true, tip).clicked() {
                self.temp_reveal();
            }
        });
    }

    fn modify_tab(&mut self, ui: &mut Ui) {
        self.select_panel(ui);
        if self.sketch.is_some() {
            self.sketch_draw(ui);
            self.walls_outline_panel(ui);
            self.sketch_modify_panel(ui);
            self.mode_panel(ui);
            return;
        }
        self.sketch_starters(ui);
        panel(ui, "Properties", |ui| {
            let shown = self.docking.shown(dock::Palette::Properties);
            if big_button(ui, Icon::Properties, "Properties", shown, "Properties (PP): show or hide the palette").clicked() {
                self.toggle_properties();
            }
            let typed = self.type_subject().is_some();
            if ui
                .add_enabled_ui(typed, |ui| big_button(ui, Icon::TypeProperties, "Type Properties", false, "Edit the type"))
                .inner
                .clicked()
            {
                self.edit_type();
            }
        });
        // Hide in View, and Override Graphics: what the selection does in the view on screen.
        if !self.selection.is_empty() && self.graphic_view().is_some() {
            panel(ui, "View", |ui| {
                let tip = msg!("Hide Element (EH): leave the selected elements out of this view");
                if big_button(ui, Icon::Visibility, "Hide Element", false, tip).clicked() {
                    self.hide_selection(false);
                }
                let tip = msg!("Hide Category (VH): leave every category the selection is of out of this view");
                if big_button(ui, Icon::Visibility, "Hide Category", false, tip).clicked() {
                    self.hide_selection(true);
                }
                let tip = msg!("Override Graphics by Element (EO): draw the selected elements their own way in this view");
                if big_button(ui, Icon::Visibility, "Override Graphics", false, tip).clicked() {
                    self.override_selection();
                }
            });
        }
        self.reveal_hidden_panel(ui);
        // Revit's Modify panel: small buttons in rows. Those taking the selection ask for it when there is none.
        let some = !self.selection.is_empty();
        // The button lit while its tool is on (the two Mirrors told apart by how they take the axis).
        let on = |t: &Tool, other: &Tool| match (t, other) {
            (Tool::Mirror { draw: a, .. }, Tool::Mirror { draw: b, .. }) => a == b,
            _ => std::mem::discriminant(t) == std::mem::discriminant(other),
        };
        let rows: [Buttons; 2] = [
            &[
                (
                    Icon::Align,
                    "Align",
                    msg!("Align (AL): pick a reference line, then what to line up with it"),
                    Some(Tool::Align(None)),
                    true,
                ),
                (
                    Icon::Offset,
                    "Offset",
                    msg!("Offset (OF): pick a wall or a line; a copy goes to the cursor's side"),
                    Some(Tool::Offset),
                    true,
                ),
                (
                    Icon::MirrorPick,
                    "Mirror Pick Axis",
                    msg!("Mirror, Pick Axis (MM): pick a wall or grid to mirror across"),
                    Some(Tool::Mirror { draw: false, start: None }),
                    true,
                ),
                (
                    Icon::MirrorDraw,
                    "Mirror Draw Axis",
                    msg!("Mirror, Draw Axis (DM): click the axis's two ends"),
                    Some(Tool::Mirror { draw: true, start: None }),
                    true,
                ),
                (
                    Icon::Split,
                    msg!("Split Element"),
                    msg!("Split Element (SL): click a wall or a line where to split it"),
                    Some(Tool::Split),
                    true,
                ),
                (
                    Icon::Linework,
                    msg!("Linework"),
                    msg!("Linework (LW): click an edge to draw it in another line style, or hide it, in this view only"),
                    Some(Tool::Linework),
                    true,
                ),
                (
                    Icon::Scale,
                    "Scale",
                    msg!("Scale (RE): images, PDFs and CAD imports, by an origin and two reference points, or a length typed"),
                    Some(Tool::Scale(Vec::new())),
                    self.scalable_selected(),
                ),
                (
                    Icon::TrimSingle,
                    msg!("Trim/Extend Single Element"),
                    msg!("Trim/Extend Single Element: pick the line to trim or extend to, then the line to change, on the part to keep"),
                    Some(Tool::TrimTo { boundary: None, many: false }),
                    true,
                ),
            ],
            &[
                (Icon::Move, "Move", msg!("Move (MV): click a base point, then where it goes"), Some(Tool::Move(None)), true),
                (Icon::Copy, "Copy", msg!("Copy (CO): click a base point, then where the copy goes"), Some(Tool::Copy(None)), true),
                (
                    Icon::Rotate,
                    "Rotate",
                    msg!("Rotate (RO): click the angle's start, then its end"),
                    Some(Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false }),
                    true,
                ),
                (
                    Icon::Array,
                    "Array",
                    msg!("Array (AR): repeat the selection along a line or about a point"),
                    Some(Self::new_array()),
                    some,
                ),
                (
                    Icon::Trim,
                    "Trim/Extend to Corner",
                    msg!("Trim/Extend to Corner (TR): pick two walls or two lines, on the parts to keep"),
                    Some(Tool::Trim(None)),
                    true,
                ),
                (
                    Icon::TrimMultiple,
                    msg!("Trim/Extend Multiple Elements"),
                    msg!(
                        "Trim/Extend Multiple Elements: pick the line to trim or extend to, then each line to change, on the part to keep"
                    ),
                    Some(Tool::TrimTo { boundary: None, many: true }),
                    true,
                ),
                (
                    Icon::Pin,
                    "Pin",
                    msg!("Pin (PN): hold the selection where it is; it can't be dragged, moved or turned until it is unpinned"),
                    None,
                    some,
                ),
                (Icon::Unpin, "Unpin", msg!("Unpin (UP): let the selected elements that are pinned be moved again"), None, some),
                (Icon::Delete, "Delete", msg!("Delete (DE) the selection"), None, some),
            ],
        ];
        // The Wall tool's and the Detail Line tool's Draw panels: lines, rectangles, polygons, circles, arcs and the rest.
        match self.tool {
            Tool::Wall(_) => self.draw_panel(ui, draw::Target::Wall),
            Tool::DetailLine(_) => self.draw_panel(ui, draw::Target::Detail),
            _ => {}
        }
        // Placing a component: Revit's Mode panel, which loads a family from a file.
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) {
            panel(ui, msg!("Mode"), |ui| {
                let tip = msg!("Load Family: a family saved as a .json file, BIMCraft's own");
                if big_button(ui, Icon::LoadFamily, "Load Family", false, tip).clicked() {
                    let dialog = crate::dialog::Ask::new().add_filter(t("BIMCraft family"), &["json"]).set_title(t("Load Family"));
                    self.ask_file(ui.ctx(), dialog, false, |app, path| {
                        if let Some(r) = app.run("family", json!({"path": path.to_string_lossy()})) {
                            let name = r["name"].as_str().unwrap_or_default().to_string();
                            let first = r["family"]["types"].as_object().and_then(|t| t.keys().next().cloned()).unwrap_or_default();
                            app.component = Some((name, first));
                        }
                    });
                }
            });
        }
        // Detail lines selected, or sketch lines picked: Join into Polyline and Explode Polyline.
        let picked = self.sketch.as_ref().map(|sk| sk.picked.len());
        if (self.lines_selected() && self.tool == Tool::Modify) || (picked.is_some() && self.tool == Tool::Modify) {
            panel(ui, msg!("Modify Lines"), |ui| {
                let two = picked.unwrap_or(self.selection.len()) >= 2;
                let tip = msg!("Join into Polyline (PJ): the lines selected, meeting end to end, become one polyline");
                if ui.add_enabled_ui(two, |ui| big_button(ui, Icon::JoinPolyline, "Join into Polyline", false, tip)).inner.clicked() {
                    self.join_polyline();
                }
                let tip = msg!("Explode Polyline (EX): a polyline taken apart into a line or an arc for each segment");
                if big_button(ui, Icon::ExplodePolyline, "Explode Polyline", false, tip).clicked() {
                    self.explode_polyline();
                }
                self.vertex_buttons(ui);
            });
        }
        // Images selected: Revit's Arrange panel, their draw order among the view's images (Draw Layer is in Properties).
        if !self.selected_images().is_empty() && self.tool == Tool::Modify {
            panel(ui, msg!("Arrange"), |ui| {
                let tip = msg!("Bring to Front: the images selected draw in front of every other image in the view");
                if big_button(ui, Icon::BringToFront, "Bring to Front", false, tip).clicked() {
                    self.arrange("front");
                }
                let tip = msg!("Bring Forward: the images selected move one step closer to the front of the view's images");
                if big_button(ui, Icon::BringForward, "Bring Forward", false, tip).clicked() {
                    self.arrange("forward");
                }
                let tip = msg!("Send Backward: the images selected move one step closer to the back of the view's images");
                if big_button(ui, Icon::SendBackward, "Send Backward", false, tip).clicked() {
                    self.arrange("backward");
                }
                let tip = msg!("Send to Back: the images selected draw behind every other image in the view");
                if big_button(ui, Icon::SendToBack, "Send to Back", false, tip).clicked() {
                    self.arrange("back");
                }
            });
        }
        // Walls selected: Revit's Modify Wall panel.
        let walls = self.selected_walls();
        if !walls.is_empty() && matches!(self.tool, Tool::Modify | Tool::Attach) {
            panel(ui, msg!("Modify Wall"), |ui| {
                let tip = msg!("Flip (Space): the walls' exterior sides go to the other side of their location lines");
                if big_button(ui, Icon::Flip, "Flip", false, tip).clicked() {
                    self.flip();
                }
                let tip = msg!("Attach Top/Base: pick a roof or a floor for the walls' tops, or bases, to follow");
                if big_button(ui, Icon::Attach, "Attach Top/Base", self.tool == Tool::Attach, tip).clicked() {
                    self.start_tool(Tool::Attach);
                }
                if big_button(ui, Icon::Detach, "Detach Top/Base", false, "Detach Top/Base: the walls' tops and bases their own again")
                    .clicked()
                {
                    self.run("modify", json!({"ids": walls, "attach_top": null, "attach_base": null}));
                }
            });
        }
        // A roof selected: which of its edges stand upright; a flat one's shape too.
        let shaping = matches!(self.tool, Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_));
        if let Some(roof) = self.selected_roof()
            && (matches!(self.tool, Tool::Modify | Tool::GableEdge) || shaping)
        {
            panel(ui, msg!("Shape Editing"), |ui| {
                let tip = msg!("Gable Edge: click an edge of a roof's footprint to stand it upright, a gable end; click again to slope it");
                if big_button(ui, Icon::GableEdge, "Gable Edge", self.tool == Tool::GableEdge, tip).clicked() {
                    self.start_tool(Tool::GableEdge);
                }
                if self.selected_shapeable().is_some() {
                    self.shape_tools(ui, roof);
                }
            });
        }
        // A floor selected: its slope arrow, or its shape's points and split lines.
        if let Some(floor) = self.selected_floor()
            && matches!(self.tool, Tool::Modify | Tool::SlopeArrow(_) | Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_))
        {
            panel(ui, "Shape Editing", |ui| {
                let tip = msg!("Slope Arrow: click its tail, then its head; the floor rises toward the head at the Options Bar's slope");
                if big_button(ui, Icon::SlopeArrow, "Slope Arrow", matches!(self.tool, Tool::SlopeArrow(_)), tip).clicked() {
                    self.start_tool(Tool::SlopeArrow(None));
                }
                self.shape_tools(ui, floor);
            });
        }
        panel(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (n, row) in rows.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        for (icon, label, tip, tool, enabled) in row {
                            let active = tool
                                .as_ref()
                                .is_some_and(|t| on(t, self.flow.pending.as_ref().unwrap_or(&self.tool)) && *t != Tool::Modify);
                            if ui.add_enabled_ui(*enabled, |ui| small_button(ui, *icon, label, active, tip)).inner.clicked() {
                                match (tool, icon) {
                                    (Some(t), _) => self.start_tool(t.clone()),
                                    (None, Icon::Pin) => self.pin_selection(true),
                                    (None, Icon::Unpin) => self.pin_selection(false),
                                    (None, _) => self.delete(),
                                }
                            }
                        }
                        // Copy to Clipboard and Paste end the second, shorter row.
                        if n == 1 {
                            self.clipboard_buttons(ui);
                        }
                    });
                }
            });
        });
        panel(ui, "Phasing", |ui| {
            let tip = msg!("Demolish (DW): click an element to take it down in the view's phase; click it again to take it back");
            let on = self.graphic_view().is_some();
            if ui.add_enabled_ui(on, |ui| big_button(ui, Icon::Demolish, "Demolish", self.tool == Tool::Demolish, tip)).inner.clicked() {
                self.start_tool(Tool::Demolish);
            }
        });
        panel(ui, msg!("Geometry"), |ui| {
            let tip = msg!("Paint (PT): lay a material, and its texture, over the face you click in the 3D view");
            if big_button(ui, Icon::Paint, "Paint", self.tool == Tool::Paint, tip).clicked() {
                self.start_tool(Tool::Paint);
            }
            let tip = msg!("Remove Paint: take the paint off the face you click in the 3D view");
            if big_button(ui, Icon::Unpaint, "Remove Paint", self.tool == Tool::Unpaint, tip).clicked() {
                self.start_tool(Tool::Unpaint);
            }
        });
        self.mode_panel(ui);
    }

    /// The Options Bar, while there's something on it (a contextual tab's name, a tool's options): a card floating centred at
    /// the view's top, so the view never moves when it comes and goes, and taking the clicks that land on it.
    fn options_bar(&mut self, ctx: &Context) {
        if self.context().is_none() && matches!(self.tool, Tool::Modify) && self.note_edit.is_none() {
            self.options_rect = Rect::NOTHING;
            return;
        }
        let g = f32::from(style::gap());
        let view = self.view_rect;
        let frame = style::card([false; 4]).inner_margin(Margin::symmetric(10, 3));
        // Below the tabs where they float over the view.
        let top = view.top().max(self.tabs_foot - g + 4.0);
        let at = pos2(view.center().x, top + g);
        let area = Area::new(UiId::new("options-bar")).order(Order::Middle).pivot(Align2::CENTER_TOP).fixed_pos(at);
        let shown = area.show(ctx, |ui| {
            // Centred, so kept as far from both sides as the view's zoom buttons are from the right, as far as its fields
            // wrap (a drop-down doesn't).
            ui.set_max_width((view.width() - 2.0 * (g + 60.0)).max(200.0));
            frame.show(ui, |ui| self.options(ui));
        });
        self.options_rect = shown.response.rect;
    }

    /// Where a view's navigation bar starts: at its top right, or under the Options Bar when that reaches over it.
    pub(crate) fn nav_corner(&self, view: Rect) -> Pos2 {
        let at = view.right_top() + vec2(-46.0, 14.0);
        let o = self.options_rect;
        if view.contains(o.left_top()) && o.right() > at.x - 6.0 && o.bottom() + 6.0 > at.y {
            return pos2(at.x, o.bottom() + 6.0);
        }
        at
    }

    fn options(&mut self, ui: &mut Ui) {
        // Wrapped: in a narrow view (the window's least) the bar takes a second line rather than run over the zoom buttons.
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(24.0);
            let rule = self.context().map(|name| {
                // Painted: the tab already names it for screen readers.
                let font = FontId::proportional(13.0);
                let size = ui.painter().layout_no_wrap(name.clone(), font.clone(), pal().text).size();
                let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, name, font, pal().text);
                ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover()).0
            });
            let before = ui.min_rect();
            self.tool_options(ui);
            // The rule parts the name from the tool's options; a tool with none (Place Door) leaves the name on its own.
            if let Some(r) = rule
                && ui.min_rect() != before
            {
                ui.painter().vline(r.center().x, r.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
            }
        });
    }

    /// The tool's own options on the Options Bar, after its name.
    fn tool_options(&mut self, ui: &mut Ui) {
        if self.note_edit.is_some() {
            self.note_options(ui);
            return;
        }
        if let Some(sketch::Of::Site { tool, .. }) = self.sketch.as_ref().map(|s| s.of) {
            self.site_options(ui, tool);
        }
        if self.sketch.is_some() {
            self.sketch_options(ui);
            match self.tool {
                Tool::SketchLine(_) => self.shape_options(ui),
                Tool::Offset => self.offset_options(ui),
                _ => {}
            }
            return;
        }
        match self.tool {
            Tool::Wall(_) => self.wall_options(ui),
            Tool::Linework => self.linework_options(ui),
            Tool::DetailGroup => self.group_options(ui),
            Tool::Scale(_) => self.scale_options(ui),
            Tool::RepeatingDetail(_) => {
                self.group_options(ui);
                let l = ui.label(t("Spacing:"));
                ui.add(egui::TextEdit::singleline(&mut self.repeat_spacing).desired_width(70.0)).labelled_by(l.id);
            }
            Tool::Paint => {
                let label = ui.label(t("Material:"));
                let names: Vec<String> = self.session.model.materials.keys().cloned().collect();
                let now = self.painting().unwrap_or_default();
                if let Some(n) = properties::combo(ui, (0, "paint-material"), label.id, &now, &names) {
                    self.paint_material = n;
                }
            }
            Tool::Column => {
                if let Some(level) = self.plan_level() {
                    height_options(ui, &self.session.model, level, &mut self.column_top, &mut self.column_height, "column-top");
                }
            }
            Tool::Run(category, _) => self.run_options(ui, category),
            Tool::Stand(category) => self.stand_options(ui, category),
            Tool::Shaft(_) => {
                if let Some(level) = self.plan_level() {
                    height_options(ui, &self.session.model, level, &mut self.shaft_top, &mut self.shaft_height, "shaft-top");
                }
            }
            Tool::Mass(_) => {
                if let Some(level) = self.plan_level() {
                    height_options(ui, &self.session.model, level, &mut self.mass_top, &mut self.mass_height, "mass-top");
                }
            }
            Tool::Site(t) => self.site_options(ui, t),
            Tool::Ceiling => {
                let l = ui.label(t("Height Offset:"));
                let hint = self.session.model.units.name();
                ui.add(TextEdit::singleline(&mut self.ceiling_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
            }
            Tool::Camera(_) => {
                let hint = self.session.model.units.name();
                let l = ui.label(t("Eye Height:"));
                ui.add(TextEdit::singleline(&mut self.camera_eye_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                let l = ui.label(t("Target Height:"));
                ui.add(TextEdit::singleline(&mut self.camera_target_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                let l = ui.label(t("Lens:"));
                ui.add(TextEdit::singleline(&mut self.camera_lens).desired_width(48.0).hint_text("mm")).labelled_by(l.id);
            }
            Tool::SlopeArrow(_) => {
                let l = ui.label(t("Slope:"));
                ui.add(TextEdit::singleline(&mut self.floor_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
            }
            // Add Point along the surface as it is, an offset over it; or Absolute, an elevation above the level.
            Tool::AddPoint => {
                ui.radio_value(&mut self.shape_along, true, t("Along Surface"));
                ui.radio_value(&mut self.shape_along, false, t("Absolute"));
                let hint = self.session.model.units.name();
                if self.shape_along {
                    let l = ui.label(t("Offset from Surface:"));
                    ui.add(TextEdit::singleline(&mut self.shape_offset).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                } else {
                    let l = ui.label(t("Elevation:"));
                    ui.add(TextEdit::singleline(&mut self.shape_z).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                    self.elevation_base(ui);
                }
            }
            Tool::ModifySubElements => {
                let l = ui.label(t("Elevation:"));
                let hint = self.session.model.units.name();
                let field = ui.add(TextEdit::singleline(&mut self.shape_z).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                // Under Modify Sub Elements, Enter sets the selection to it.
                if self.tool == Tool::ModifySubElements && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.shape_apply();
                }
                self.elevation_base(ui);
            }
            Tool::Stair(_) => {
                let l = ui.label(t("Width:"));
                let hint = self.session.model.units.name();
                ui.add(TextEdit::singleline(&mut self.stair_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                ui.checkbox(&mut self.railings, t("Railings"));
            }
            Tool::Ramp(_) => {
                if let Some(level) = self.plan_level() {
                    height_options(ui, &self.session.model, level, &mut self.ramp_top, &mut self.ramp_height, "ramp-top");
                }
                let hint = self.session.model.units.name();
                let l = ui.label(t("Width:"));
                ui.add(TextEdit::singleline(&mut self.ramp_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                let l = ui.label(t("Slope:"));
                ui.add(TextEdit::singleline(&mut self.ramp_slope).desired_width(48.0).hint_text("%")).labelled_by(l.id);
                ui.checkbox(&mut self.railings, t("Railings"));
            }
            Tool::Roof => {
                let l = ui.label(t("Slope:"));
                ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                let l = ui.label(t("Overhang:"));
                let hint = self.session.model.units.name();
                ui.add(TextEdit::singleline(&mut self.roof_overhang).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                ui.checkbox(&mut self.roof_gable, t("Gable"));
            }
            Tool::ExtrusionRoof(_) => {
                let l = ui.label(t("Slope:"));
                ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                ui.checkbox(&mut self.roof_shed, t("Shed"));
            }
            Tool::Dormer => {
                let hint = self.session.model.units.name();
                let l = ui.label(t("Width:"));
                ui.add(TextEdit::singleline(&mut self.dormer_width).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                let l = ui.label(t("Height:"));
                ui.add(TextEdit::singleline(&mut self.dormer_height).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
                let l = ui.label(t("Slope:"));
                ui.add(TextEdit::singleline(&mut self.roof_slope).desired_width(48.0).hint_text("°")).labelled_by(l.id);
                ui.checkbox(&mut self.dormer_window, t("Window"));
            }
            Tool::Level => self.level_options(ui),
            Tool::Move(_) => {
                ui.checkbox(&mut self.constrain, t("Constrain"));
            }
            Tool::Copy(_) => {
                ui.checkbox(&mut self.constrain, t("Constrain"));
                ui.checkbox(&mut self.multiple, t("Multiple"));
            }
            Tool::Rotate { .. } => self.rotate_options(ui),
            Tool::Array { .. } => self.array_options(ui),
            Tool::Mirror { .. } => {
                ui.checkbox(&mut self.mirror_copy, t("Copy"));
            }
            Tool::Tag => {
                ui.checkbox(&mut self.tag_leader, t("Leader"));
                let l = ui.label(t("Label:"));
                ui.add(TextEdit::singleline(&mut self.tag_label).desired_width(160.0).hint_text(t("a field, such as Type or Height")))
                    .labelled_by(l.id);
            }
            Tool::DetailLine(_) => self.line_style_options(ui),
            // Between endpoints: as the cursor lies, aligned, horizontal or vertical (Space steps them).
            Tool::Dimension(mode, _) => {
                ui.label(t("Place:"));
                for (m, label) in [
                    (DimMode::Auto, msg!("Auto")),
                    (DimMode::Aligned, msg!("Aligned")),
                    (DimMode::Horizontal, msg!("Horizontal")),
                    (DimMode::Vertical, msg!("Vertical")),
                ] {
                    if ui.selectable_label(mode == m, t(label)).clicked()
                        && let Tool::Dimension(now, _) = &mut self.tool
                    {
                        *now = m;
                    }
                }
            }
            Tool::Text => {
                let l = ui.label(t("Text:"));
                ui.add(TextEdit::singleline(&mut self.note_text).desired_width(240.0).hint_text(t("what the note says"))).labelled_by(l.id);
            }
            Tool::Attach => {
                ui.label(t("Attach Wall:"));
                if ui.selectable_label(!self.attach_base, t("Top")).clicked() {
                    self.attach_base = false;
                }
                if ui.selectable_label(self.attach_base, t("Base")).clicked() {
                    self.attach_base = true;
                }
            }
            Tool::Offset => self.offset_options(ui),
            _ => {}
        }
    }

    /// Offset's Options Bar: the distance, and Copy.
    fn offset_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Offset:"));
        let hint = self.session.model.units.name();
        ui.add(TextEdit::singleline(&mut self.offset).desired_width(70.0).hint_text(hint)).labelled_by(l.id);
        ui.checkbox(&mut self.offset_copy, t("Copy"));
    }

    fn wall_options(&mut self, ui: &mut Ui) {
        let Some(level) = self.plan_level() else { return };
        height_options(ui, &self.session.model, level, &mut self.wall_top, &mut self.wall_height, "wall-top");
        let l = ui.label(t("Location Line:"));
        let now = self.wall_location;
        let c = crate::icons::combo("wall-location").selected_text(t(now.label())).width(150.0).show_ui(ui, |ui| {
            for loc in Location::ALL {
                if ui.selectable_label(loc == now, t(loc.label())).clicked() {
                    self.wall_location = loc;
                }
            }
        });
        c.response.labelled_by(l.id);
        if self.shape().chains() {
            ui.checkbox(&mut self.chain, t("Chain"));
        }
        self.shape_options(ui);
    }

    /// Rotate's Options Bar: an angle to type (Enter turns by it), Place Centre, and Copy.
    fn rotate_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Angle:"));
        let r = ui.add(TextEdit::singleline(&mut self.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match (self.angle.trim().parse::<f64>(), &self.tool) {
                (Ok(a), &Tool::Rotate { centre, .. }) if a.is_finite() => self.turn(centre, a),
                _ => self.status = Status { text: tf("rotate: `{}` is not an angle in degrees", &[&self.angle]), error: true },
            }
        }
        if ui.button(t("Place Centre")).clicked()
            && let Tool::Rotate { place, .. } = &mut self.tool
        {
            *place = true;
        }
        ui.checkbox(&mut self.rotate_copy, t("Copy"));
    }

    fn level_options(&mut self, ui: &mut Ui) {
        let l = ui.label(t("Elevation"));
        ui.add(TextEdit::singleline(&mut self.new_level.0).desired_width(80.0)).labelled_by(l.id);
        let l = ui.label(t("Name"));
        ui.add(TextEdit::singleline(&mut self.new_level.1).hint_text(t("the next free name")).desired_width(140.0)).labelled_by(l.id);
        if ui.button(t("Create")).clicked() {
            match self.session.model.units.parse(&self.new_level.0) {
                Some(elevation) => {
                    let mut p = json!({"elevation": elevation});
                    if !self.new_level.1.trim().is_empty() {
                        p["name"] = json!(self.new_level.1.trim());
                    }
                    if self.run("level", p).is_some() {
                        self.tool = Tool::Modify;
                    }
                }
                None => self.status = Status { text: tf("level: `{}` is not an elevation", &[&self.new_level.0]), error: true },
            }
        }
    }

    // ---------- the bottom: status bar, warnings ----------

    /// What the tool wants next, in the status bar as Revit puts it.
    fn prompt(&self) -> &'static str {
        if let Some(hint) = self.sketch_prompt() {
            return hint;
        }
        if self.picking_pattern_lines() {
            return msg!("Select fill pattern lines then press Enter");
        }
        if let Some(prompt) = self.selection_prompt() {
            return prompt;
        }
        match self.tool {
            Tool::PatternCorner(None) => return msg!("Click on Bottom Left Corner"),
            Tool::PatternCorner(Some(_)) => return msg!("Click on Top Right Corner"),
            _ => {}
        }
        match (self.sheet, &self.tool) {
            (Sheet::View3D, Tool::Modify) => {
                return msg!(
                    "Click to select, drag a box to select several. Shift and the middle button orbit, the middle button pans, the wheel zooms."
                );
            }
            (Sheet::Paper(_), Tool::Modify) => {
                return msg!("Click a view on the sheet to select it, drag it to move it, double-click it to open it.");
            }
            (Sheet::Section(_), Tool::Modify) => {
                return msg!("Click to select what the view cuts or sees; the middle button pans, the wheel zooms.");
            }
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), Tool::Level) => {
                return msg!("Type the new level's elevation in the Options Bar, then Create.");
            }
            (Sheet::Section(_) | Sheet::Paper(_), Tool::Text) => {
                return msg!("Type the note in the Options Bar, then click where it goes.");
            }
            (Sheet::Section(_), Tool::Spot) => return msg!("Click a point to mark its height."),
            // Demolish picks in these views as it does in a plan; Paint in the 3D view alone.
            (Sheet::View3D | Sheet::Section(_), Tool::Demolish) | (Sheet::View3D, Tool::Paint | Tool::Unpaint) => {}
            // Shape Editing works on the floor's top in the 3D view too.
            (Sheet::View3D, Tool::ModifySubElements) => {
                return msg!(
                    "Click a point, a corner or an edge to select it (Shift adds), then type its height or drag its arrows; Delete removes it."
                );
            }
            (Sheet::View3D, Tool::AddPoint | Tool::AddSplitLine(_)) => {}
            (Sheet::View3D | Sheet::Section(_) | Sheet::Paper(_), _) => {
                return msg!("This tool works in plan views: open one from the Project Browser.");
            }
            _ => {}
        }
        if matches!(self.target(), Some(draw::Target::Detail | draw::Target::Wall))
            && let Some(prompt) = self.shape_prompt()
        {
            return prompt;
        }
        match &self.tool {
            Tool::Modify => msg!("Click to select, Tab for what's underneath, Ctrl adds, Shift takes away; drag to box or to move."),
            Tool::Wall(chain) if chain.is_empty() => msg!("Click to enter the wall's start point."),
            Tool::Wall(_) => msg!("Click to build the wall to the next point. Enter or Esc ends the chain, Shift keeps it square."),
            Tool::Attach if self.attach_base => msg!("Pick a roof or a floor: the selected walls' bases stand on it."),
            Tool::Attach => msg!("Pick a roof or a floor: the selected walls' tops follow its underside."),
            Tool::Door => msg!("Click a wall to place the door."),
            Tool::Window => msg!("Click a wall to place the window."),
            Tool::Room => msg!("Click inside walls to place a room; the outline shows the room you'd get."),
            Tool::RoomSeparator(None) => msg!("Click the separation line's start."),
            Tool::RoomSeparator(Some(_)) => msg!("Click the line's next point; each click places a line. Esc ends the chain."),
            Tool::Level => "Type the new level's elevation in the Options Bar, then Create.",
            Tool::Grid(None) => msg!("Click to enter the grid's start point."),
            Tool::Grid(Some(_)) => msg!("Click to enter the grid's end point."),
            Tool::Move(None) | Tool::Copy(None) => msg!("Click a base point to measure from."),
            Tool::Move(Some(_)) => msg!("Click where the base point goes, or type a distance and press Enter. Esc cancels."),
            Tool::Copy(Some(_)) => msg!("Click where the copy's base point goes, or type a distance and press Enter. Esc cancels."),
            Tool::Paste => msg!("Click where to paste what was copied. Esc cancels."),
            Tool::PlaceImage => msg!("Click where the image's middle goes. Esc cancels."),
            Tool::Scale(pts) if pts.is_empty() => msg!("Click the origin: the point that stays where it is."),
            Tool::Scale(pts) if pts.len() == 1 => msg!("Click the first reference point: the end of a length you know."),
            Tool::Scale(_) => msg!("Click the second reference point, or type the length in the Options Bar and press Enter."),
            Tool::PasteLevel => msg!("Click a level line to paste what was copied onto that level. Esc cancels."),
            Tool::MoveWitness { .. } => msg!("Click the line the witness line goes to. Esc cancels."),
            Tool::Rotate { place: true, .. } => msg!("Click the centre of rotation."),
            Tool::Rotate { start: None, .. } => msg!("Click the start of the angle, or type an angle in the Options Bar."),
            Tool::Rotate { start: Some(_), .. } => msg!("Click the end of the angle. Esc cancels."),
            Tool::Array { place: true, .. } if self.array.radial => msg!("Click the centre of rotation."),
            Tool::Array { start: None, .. } if self.array.radial => {
                msg!("Click the start of the angle, or type an angle in the Options Bar.")
            }
            Tool::Array { start: Some(_), .. } if self.array.radial => msg!("Click the end of the angle. Esc cancels."),
            Tool::Array { start: None, .. } => msg!("Click the first point of the array's distance."),
            Tool::Array { start: Some(_), .. } => {
                msg!("Click the second point: how far the second member, or the last, goes. Esc cancels.")
            }
            Tool::Mirror { draw: false, .. } => msg!("Pick a wall or grid to mirror across."),
            Tool::Mirror { draw: true, start: None } => msg!("Click the start of the mirror axis."),
            Tool::Mirror { draw: true, start: Some(_) } => msg!("Click the end of the mirror axis."),
            Tool::Align(None) => msg!("Pick a reference edge or centre line; the line under the cursor is highlighted."),
            Tool::Align(Some(_)) => msg!("Pick the element's highlighted edge to move and turn onto the reference."),
            Tool::Trim(None) => msg!("Pick the first wall or line, on the part to keep."),
            Tool::Trim(Some(_)) => msg!("Pick the second wall or line, on the part to keep."),
            Tool::TrimTo { boundary: None, .. } => msg!("Pick the line to trim or extend to: a line, a wall or a grid."),
            Tool::TrimTo { many: false, .. } => msg!("Click the line to trim or extend to it, on the part to keep."),
            Tool::TrimTo { many: true, .. } => {
                msg!("Click each line to trim or extend to it, on the part to keep; Esc picks another boundary.")
            }
            Tool::Vertex { delete: false } => msg!("Click a polyline or a spline where a vertex goes."),
            Tool::Vertex { delete: true } => msg!("Click a polyline's corner or a spline's point to take it out."),
            Tool::Split => msg!("Click a wall or a line where to split it."),
            Tool::Offset => msg!("Point to a wall or a line: the offset goes to the cursor's side. Click to place it."),
            Tool::Demolish => msg!("Click an element to take it down in the view's phase; click it again to take it back."),
            Tool::Linework => msg!("Click an edge to draw it in the line style in the Options Bar; <By Category> puts it back."),
            Tool::DetailGroup => msg!("Click where the group's origin goes."),
            Tool::RepeatingDetail(None) => msg!("Click where the repeating detail starts."),
            Tool::RepeatingDetail(Some(_)) => msg!("Click where it ends: the group repeats along the line every spacing."),
            Tool::Paint => msg!("Click a face to paint it with the material in the Options Bar; the 3D view orbits as ever."),
            Tool::Unpaint => msg!("Click a painted face to take its paint off."),
            Tool::Dimension(_, refs) if refs.is_empty() => {
                msg!("Pick a wall face, centre line or end, a door, a window, a grid, or an endpoint to measure from.")
            }
            Tool::Dimension(_, refs) if refs.len() == 1 => msg!("Pick the next reference or endpoint."),
            Tool::Dimension(..) => msg!(
                "Pick more references, or click away from them where the dimension line goes; Space steps Auto, Aligned, Horizontal, \
                 Vertical. Esc cancels."
            ),
            Tool::Measure(None) => msg!("Click the ruler's start: points snap as they do drawing."),
            Tool::Measure(Some(_)) => msg!("Click its end: its length and angle read beside it. Esc cancels."),
            Tool::Tag => msg!("Click an element to tag it with its type, a door's or window's with its mark, or with the field in Label."),
            Tool::Angular(refs) if refs.len() < 2 => msg!("Pick a wall, a grid or another line; then one that meets it."),
            Tool::Angular(_) => msg!("Click where the arc goes: the angle on that side. Esc cancels."),
            Tool::Round(DimKind::ArcLength, picked) if picked.is_empty() => msg!("Pick a curved wall or an arc."),
            Tool::Round(_, picked) if picked.is_empty() => msg!("Pick a round column, a curved wall, a circle or an arc."),
            Tool::Round(DimKind::ArcLength, picked) if picked.len() == 2 => msg!("Pick the second point on the arc. Esc cancels."),
            Tool::Round(DimKind::ArcLength, _) => {
                msg!("Click where the dimension's arc goes, or pick two points on the arc to measure between. Esc cancels.")
            }
            Tool::Round(DimKind::Diameter, _) => msg!("Click where the diameter goes. Esc cancels."),
            Tool::Round(..) => msg!("Click where the radius goes. Esc cancels."),
            Tool::Text => msg!("Type the note in the Options Bar, then click where it goes: its top left corner."),
            Tool::Spot => msg!("Click a floor, a roof, a stair or a wall: the spot elevation reads the height of its top there."),
            Tool::SpotSlope => msg!("Click a floor or a roof: the spot slope reads how steeply its top falls there, and which way."),
            Tool::Section(None) => msg!("Click the section's start: it looks to the left of the line you draw."),
            Tool::Section(Some(_)) => msg!("Click the section's end."),
            Tool::Elevation => msg!("Click beside the building: the elevation looks at the side you click by."),
            Tool::Camera(None) => msg!("Click where the camera's eye stands: it looks at the point you click next."),
            Tool::Camera(Some(_)) => msg!("Click what the camera looks at; its eye and target heights are the Options Bar's."),
            Tool::Column => msg!("Click to place a column, as high as the Options Bar says."),
            Tool::Run(..) | Tool::Stand(_) => structure::hint(&self.tool).unwrap_or_default(),
            Tool::Component | Tool::Fixture(_) => msg!("Click where the component goes; Space turns it a quarter turn."),
            Tool::Site(_) => msg!("Click where it goes; Space turns it a quarter turn."),
            Tool::Stair(pts) if pts.is_empty() => msg!("Click where the stair starts: the middle of its first riser."),
            Tool::Stair(pts) if pts.len() == 1 => {
                msg!("Click where its run ends: past its last riser, one run to the level above; short of it, a landing. Esc cancels.")
            }
            Tool::Ramp(pts) if pts.is_empty() => msg!("Click where the ramp starts: the middle of its foot."),
            Tool::Ramp(pts) if pts.len() == 1 => {
                msg!(
                    "Click where its run ends: past its top, one run, as high as the Options Bar says; short of it, a landing. Esc cancels."
                )
            }
            Tool::Stair(_) | Tool::Ramp(_) => {
                msg!("Click which way the next run goes from the landing: on, left, right, or back beside it.")
            }
            Tool::Railing(pts) if pts.is_empty() => msg!("Click a stair or a ramp for a railing down each side, or the start of a path."),
            Tool::Railing(_) => msg!("Click the path's next point. Enter finishes, Esc cancels."),
            Tool::Roof => msg!("Click inside the walls to roof them, on the level above: the overhang runs past their faces."),
            Tool::ExtrusionRoof(pts) if pts.is_empty() => msg!("Click the start of the line the roof's profile runs along."),
            Tool::ExtrusionRoof(pts) if pts.len() == 1 => {
                msg!("Click the line's end: the profile rises from it at the Options Bar's slope.")
            }
            Tool::ExtrusionRoof(_) => msg!("Click how far across the roof is swept, to either side of the line. Esc cancels."),
            Tool::Dormer => msg!("Click a roof's slope where the dormer's front wall goes, across the slope."),
            Tool::GableEdge => msg!("Click an edge of a roof's footprint: it stands upright, a gable end; click it again to slope it."),
            Tool::Floor => msg!("Click inside the walls to floor them: the slab runs under their outline, its top at the level."),
            Tool::Ceiling => msg!("Click inside a room: a ceiling out to its walls' faces, as high as the Options Bar says."),
            Tool::Shaft(None) => msg!("Click a corner of the shaft opening."),
            Tool::Shaft(Some(_)) => {
                msg!("Click the opposite corner: the shaft cuts the floors, ceilings and roofs up to its top. Esc cancels.")
            }
            Tool::Mass(pts) if pts.is_empty() => msg!("Click the first corner of the mass's footprint."),
            Tool::Mass(pts) if pts.len() < 3 => msg!("Click the footprint's next corner. Esc cancels."),
            Tool::Mass(_) => msg!("Click the next corner; Enter, or the first corner again, finishes the mass. Esc cancels."),
            Tool::DetailLine(pts) if pts.is_empty() => msg!("Click where the detail line starts."),
            Tool::DetailLine(_) => "Click its next point; Enter finishes it, the first point again closes it. Esc cancels.",
            Tool::SlopeArrow(None) => msg!("Click the slope arrow's tail: where the floor's top stays at its height offset."),
            Tool::SlopeArrow(Some(_)) => msg!("Click its head: the floor rises toward it at the Options Bar's slope. Esc cancels."),
            Tool::ModifySubElements => {
                msg!(
                    "Click a point, a corner or an edge to select it (Shift adds), then type its height; drag a point to move it; Delete removes it."
                )
            }
            Tool::AddPoint => {
                msg!("Click on the top: a point there, the Options Bar's offset over the surface, or at its Absolute elevation.")
            }
            Tool::AddSplitLine(None) => msg!("Click where the split line starts: a point, a corner or an edge of the top."),
            Tool::AddSplitLine(Some(_)) => msg!("Click where it ends: the top folds along it. Esc cancels."),
            Tool::SketchLine(_) | Tool::SketchPick => msg!("Click where the line starts."),
            Tool::PatternCorner(_) => msg!("Click on Bottom Left Corner"),
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t(self.prompt())).color(pal().muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // What the project has to warn of, as Revit's Review Warnings counts it.
                self.warnings_button(ui);
                // How many elements are selected, by the selection filter, as Revit shows it.
                // A click on it filters the selection, as Revit's does.
                ui.label(format!(":{}", self.selection.len()));
                let (r, funnel) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                icons::paint(ui.painter(), Icon::Filter, r, pal().icon);
                funnel.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, t("Filter Selection")));
                if funnel.on_hover_text(t("Filter Selection")).clicked() {
                    self.open_filter();
                }
                // The page's checks of the model, flagged and not: a click opens the Checks palette.
                self.checks_badge(ui);
                if let Some(p) = self.canvas.cursor {
                    let u = self.session.model.units;
                    ui.monospace(format!("x {}  y {}", u.with_unit(p.0), u.with_unit(p.1)));
                }
                // What is under the cursor, as Revit's status bar names it.
                if let Some(words) = self.hover_text() {
                    ui.label(RichText::new(words).color(pal().text));
                }
                if let Some(note) = self.snap_note() {
                    ui.label(RichText::new(note).color(pal().accent_ink));
                }
                // A hosted window's page says whether its project is saved.
                if self.dirty() && !self.hosted {
                    ui.label(RichText::new(t("Unsaved changes")).color(pal().muted));
                }
                self.option_selector(ui);
            });
        });
    }

    /// Why the last command failed, or what it warns of, in a box over the view's corner until the next one works: an error in
    /// the error's colour, a warning (Revit's warning-severity failures, such as "Pinned objects were not deleted.") under the
    /// warning sign, as Review Warnings shows warnings.
    fn warning(&mut self, ctx: &Context) {
        if !self.status.text.is_empty() {
            // Said since the warning: it takes its place.
            self.warned.clear();
        }
        let warned = self.status.text.is_empty() && !self.warned.is_empty();
        if self.status.text.is_empty() && !warned {
            return;
        }
        let (title, color) = if warned {
            (t("Warning"), pal().accent_ink)
        } else if self.status.error {
            (t("Error"), pal().error)
        } else {
            (t("Note"), pal().accent_ink)
        };
        let text = if warned {
            self.warned.join(
                "
",
            )
        } else {
            self.status.text.clone()
        };
        let at = self.view_rect.right_bottom() - vec2(16.0, 16.0);
        Area::new(UiId::new("warning")).order(Order::Middle).fixed_pos(at).pivot(Align2::RIGHT_BOTTOM).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).stroke(Stroke::new(1.0, color)).show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    if warned {
                        let (sign, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                        icons::paint(ui.painter(), Icon::Warning, sign, color);
                    }
                    ui.label(RichText::new(title).strong().color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Dismiss").clicked() {
                            self.status = Status::default();
                            self.warned.clear();
                        }
                    });
                });
                ui.label(text);
            });
        });
    }

    // ---------- the palettes: Properties, Project Browser (`dock.rs` docks them) ----------

    /// The family and type the Component tool places: the one picked, while its family has it, else
    /// the first there is. A fixture tool's are of its kinds only.
    pub(crate) fn placing(&self) -> Option<(String, String)> {
        let m = &self.session.model;
        let fits = |f: &bimcraft_core::family::Family| match self.tool {
            Tool::Fixture(key) => mep::kinds(key).contains(&f.category),
            _ => true,
        };
        self.component
            .clone()
            .filter(|(f, t)| m.families.get(f).is_some_and(|f| fits(f) && f.types.contains_key(t)))
            .or_else(|| m.families.iter().filter(|(_, f)| fits(f)).find_map(|(n, f)| f.types.keys().next().map(|t| (n.clone(), t.clone()))))
    }

    // ---------- the middle: view tabs, schedules ----------

    /// The icon of a view's kind, before its name on its tab.
    fn view_icon(&self, sheet: Sheet) -> Icon {
        match sheet {
            Sheet::Plan(_) => Icon::Plan,
            Sheet::Ceiling(_) => Icon::Ceiling,
            Sheet::View3D => Icon::View3D,
            Sheet::Section(id) if matches!(self.session.model.elements.get(id), Some(Element::Elevation(_))) => Icon::Elevation,
            Sheet::Section(_) => Icon::Section,
            Sheet::Paper(_) => Icon::Sheet,
            Sheet::Schedule(_) | Sheet::Table(_) => Icon::Schedule,
        }
    }

    /// The docked views' tabs, as Revit's document tabs: rectangular, along the top of the view; the open one in the view's own
    /// colour and joined to it, the others on the strip with a thin line between; each the view's kind's icon and its name, cut
    /// short with an ellipsis, its × on the open tab and the one under the pointer. When they don't fit, the strip scrolls to the
    /// open one and a drop-down at its end lists them all. A click opens one, a drag moves it along the bar or, let go off it,
    /// floats it; a right-click floats or closes it. `strip`: across the whole width, on the strip's colour (the Compact layout);
    /// otherwise as wide as the tabs, on the card they float on.
    fn view_tabs(&mut self, ui: &mut Ui, strip: bool) {
        use egui::scroll_area::ScrollBarVisibility;
        use egui::text::{LayoutJob, TextWrapping};
        const TAB: f32 = 28.0;
        const MORE: f32 = 28.0;
        self.tab_rects.clear();
        let many = self.views.len() > 1;
        let front = self.front;
        let font = FontId::proportional(12.5);
        let tabs: Vec<(Sheet, String, Icon, Arc<egui::Galley>, f32)> = self
            .views
            .clone()
            .into_iter()
            .map(|sheet| {
                // A ceiling plan's tab says so: its level's plan has the level's name too.
                let name = if matches!(sheet, Sheet::Ceiling(_)) { self.view_title(sheet) } else { self.view_name(sheet) };
                let mut job = LayoutJob::simple_singleline(name.clone(), font.clone(), Color32::PLACEHOLDER);
                job.wrap = TextWrapping { max_width: 170.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                let galley = ui.painter().layout_job(job);
                let width = (10.0 + 14.0 + 6.0 + galley.size().x + if many { 26.0 } else { 12.0 }).ceil();
                (sheet, name, self.view_icon(sheet), galley, width)
            })
            .collect();
        let total: f32 = tabs.iter().map(|t| t.4).sum();
        let room = ui.available_width();
        let over = total > room;
        let more = if over { MORE } else { 0.0 };
        let (bar, _) = ui.allocate_exact_size(vec2(if strip { room } else { (total + more).min(room) }, TAB), Sense::hover());
        let shown = Rect::from_min_size(bar.min, vec2(bar.width() - more, TAB));
        // The open view's tab is scrolled to when it becomes the open one.
        let key = ui.id().with("view-tabs-front");
        let newly = ui.ctx().data_mut(|d| {
            let was = d.get_temp::<Sheet>(key);
            d.insert_temp(key, front);
            was != Some(front)
        });
        let mut closed = None;
        let mut opened = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(shown).layout(Layout::top_down(Align::Min)));
        child.set_clip_rect(shown.intersect(ui.clip_rect()));
        let border = Stroke::new(1.0, pal().border);
        // On the strip, the open tab is the view's colour, which it runs into; floating on a card, the field's.
        let view = if strip { self.backdrop() } else { pal().surface };
        ScrollArea::horizontal()
            .id_salt("view-tabs-scroll")
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .max_width(shown.width())
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal(|ui| {
                    let active_at = tabs.iter().position(|t| t.0 == front);
                    for (i, (sheet, name, icon, galley, width)) in tabs.iter().enumerate() {
                        let (sheet, active) = (*sheet, *sheet == front);
                        let (rect, r) = ui.allocate_exact_size(vec2(*width, TAB), Sense::click_and_drag());
                        r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, name));
                        self.tab_rects.push((sheet, rect));
                        if active && newly {
                            r.scroll_to_me(None);
                        }
                        let under = ui.rect_contains_pointer(rect);
                        let p = ui.painter();
                        if active {
                            // In the view's colour, its edges a hairline but for its foot, where it runs into the view.
                            p.rect_filled(rect, 0.0, view);
                            let (l, rt, top) = (rect.left() + 0.5, rect.right() - 0.5, rect.top() + 0.5);
                            p.line_segment([pos2(l, rect.bottom()), pos2(l, top)], border);
                            p.line_segment([pos2(l, top), pos2(rt, top)], border);
                            p.line_segment([pos2(rt, top), pos2(rt, rect.bottom())], border);
                        } else if under {
                            p.rect_filled(rect.shrink2(vec2(1.0, 3.0)), 4.0, pal().hover_fill);
                        } else if active_at != Some(i + 1) && i + 1 < tabs.len() {
                            // A thin line before the next tab, unless that is the open one.
                            let x = rect.right() - 0.5;
                            p.line_segment([pos2(x, rect.top() + 8.0), pos2(x, rect.bottom() - 8.0)], border);
                        }
                        if r.has_focus() {
                            p.rect_stroke(rect.shrink(1.0), 4.0, style::focus_ring(), StrokeKind::Inside);
                        }
                        let ink = if active || under { pal().text } else { pal().muted };
                        let at = Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), vec2(14.0, 14.0));
                        icons::paint(p, *icon, at, if active { pal().icon } else { ink });
                        p.galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), Arc::clone(galley), ink);
                        // The × is part of the tab, at its right end: drawn on the open tab and the one under the pointer.
                        if many {
                            let x = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(16.0, 16.0));
                            let label = tf("Close {}", &[name]);
                            let c = ui.interact(x, ui.id().with(("close view", sheet)), Sense::click());
                            c.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
                            if active || under || c.has_focus() {
                                let p = ui.painter();
                                if c.hovered() {
                                    p.rect_filled(x, 3.0, if active { pal().hover_fill } else { pal().panel });
                                }
                                let ink = if c.hovered() { pal().text } else { pal().muted };
                                let d = 3.5;
                                for (a, b) in [(vec2(-d, -d), vec2(d, d)), (vec2(-d, d), vec2(d, -d))] {
                                    p.line_segment([x.center() + a, x.center() + b], Stroke::new(1.2, ink));
                                }
                            }
                            if c.clicked() {
                                closed = Some(sheet);
                            }
                        }
                        if r.clicked() {
                            opened = Some(sheet);
                        }
                        if r.drag_started() {
                            self.dragging = Some(dock::Drag::View(sheet));
                        }
                        r.context_menu(|ui| {
                            if ui.add_enabled(many, egui::Button::new(t("Float"))).clicked() {
                                let at = self.view_rect.left_top() + vec2(60.0, 60.0);
                                self.float_view(sheet, at);
                            }
                            if ui.add_enabled(many, egui::Button::new(t("Close"))).clicked() {
                                self.close_view(sheet);
                            }
                        });
                    }
                });
            });
        // The strip's foot, a hairline but under the open tab, which runs into the view.
        if strip {
            let y = bar.bottom() - 0.5;
            let open = self.tab_rects.iter().find(|(s, _)| *s == front).map(|(_, r)| r.intersect(shown)).filter(|r| r.width() > 0.0);
            let p = ui.painter();
            match open {
                Some(o) => {
                    p.hline(bar.left()..=o.left() + 0.5, y, border);
                    p.hline(o.right() - 0.5..=bar.right(), y, border);
                }
                None => {
                    p.hline(bar.x_range(), y, border);
                }
            }
        }
        // When they don't all fit: every open view, from a drop-down at the strip's end.
        if over {
            let at = Rect::from_min_size(pos2(bar.right() - MORE, bar.top()), vec2(MORE, TAB)).shrink2(vec2(2.0, 3.0));
            let r = ui.interact(at, ui.id().with("all open views"), Sense::click());
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, t("All open views")));
            let p = ui.painter();
            if r.hovered() {
                p.rect_filled(at, 4.0, pal().hover_fill);
            }
            if r.has_focus() {
                p.rect_stroke(at, 4.0, style::focus_ring(), StrokeKind::Inside);
            }
            icons::paint_chevron_at(p, at.center(), 16.0, pal().muted);
            let r = r.on_hover_text(t("All open views"));
            // As wide as the longest name, and fixed: a menu's width would otherwise grow each frame.
            let size = |n: &String| ui.painter().layout_no_wrap(n.clone(), FontId::proportional(13.0), pal().text).size().x;
            let wide = tabs.iter().map(|t| size(&t.1)).fold(0.0, f32::max) + 44.0;
            Popup::menu(&r).show(|ui| {
                for (sheet, name, icon, _, _) in &tabs {
                    let (rect, b) = ui.allocate_exact_size(vec2(wide.max(180.0), 24.0), Sense::click());
                    b.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, *sheet == front, name));
                    let p = ui.painter();
                    if b.hovered() {
                        p.rect_filled(rect, 4.0, pal().hover_fill);
                    } else if *sheet == front {
                        p.rect_filled(rect, 4.0, pal().select_fill);
                    }
                    icons::paint(p, *icon, Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(14.0, 14.0)), pal().icon);
                    p.text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), pal().text);
                    if b.clicked() {
                        opened = Some(*sheet);
                        ui.close();
                    }
                }
            });
        }
        if let Some(sheet) = opened {
            self.open_sheet(sheet);
        }
        if let Some(sheet) = closed {
            self.close_view(sheet);
        }
        self.tab_bar = bar.with_max_x(ui.max_rect().right());
    }

    /// The schedule on screen: the project's, by id, and as it is (one every project has may be
    /// none of the project's yet).
    pub(crate) fn shown_schedule(&self) -> Option<(Option<Id>, bimcraft_core::model::Schedule)> {
        let m = &self.session.model;
        match self.sheet {
            Sheet::Schedule(cat) => Some(schedule::standard(m, cat)),
            Sheet::Table(id) => match m.elements.get(id) {
                Some(Element::Schedule(sc)) => Some((Some(id), sc.clone())),
                _ => None,
            },
            _ => None,
        }
    }

    /// The schedule on screen as a table: its headings, its rows, its totals under a rule.
    fn schedule(&mut self, ui: &mut Ui) {
        let Some((_, sc)) = self.shown_schedule() else { return };
        // The right-click menu, on the schedule and on each of its cells.
        let groups = self.context_items();
        let mut chosen = None;
        Frame::new().fill(pal().surface).inner_margin(16.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let ground = ui.interact(ui.max_rect(), ui.id().with("schedule menu"), egui::Sense::click());
            context::menu(&ground, &groups, &mut chosen);
            let name = self.view_name(self.sheet);
            ui.label(RichText::new(&name).heading());
            ui.add_space(8.0);
            match schedule::table(&self.session.model, &sc) {
                Err(e) => {
                    ui.label(e);
                }
                Ok(t) if t.rows.is_empty() => {
                    ui.label(RichText::new(tf("No rows yet: the {} is empty.", &[&name.to_lowercase()])).color(pal().muted));
                }
                Ok(table) => {
                    let units = self.session.model.units;
                    let said = |c: &schedule::Column, v: &Value| {
                        let text = schedule::shown(units, c.kind, v);
                        if v.is_boolean() { t(&text).to_string() } else { text }
                    };
                    // A row is highlighted while what it lists is selected; a click on it picks that.
                    let selected: Vec<bool> = table.ids.iter().map(|ids| ids.iter().any(|id| self.selection.contains(id))).collect();
                    // Striped, as a grid is; the heading is row 0.
                    let shaded = selected.clone();
                    let row_color = move |row: usize, style: &egui::Style| match row.checked_sub(1).and_then(|r| shaded.get(r)) {
                        Some(true) => Some(style.visuals.selection.bg_fill.gamma_multiply(0.5)),
                        _ => (row % 2 == 1).then_some(style.visuals.faint_bg_color),
                    };
                    let mut picked = None;
                    ScrollArea::both().show(ui, |ui| {
                        Grid::new(("schedule", &sc.name)).with_row_color(row_color).spacing(vec2(28.0, 6.0)).show(ui, |ui| {
                            for c in &table.columns {
                                let heading = schedule::Column { heading: t(&c.heading).to_string(), kind: c.kind };
                                ui.label(RichText::new(schedule::heading(units, &heading)).strong());
                            }
                            ui.end_row();
                            for ((row, ids), &selected) in table.rows.iter().zip(&table.ids).zip(&selected) {
                                for (v, c) in row.iter().zip(&table.columns) {
                                    let said = said(c, v);
                                    let text = RichText::new(&said);
                                    let text = if v.is_number() { text.monospace() } else { text };
                                    let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                                    // Selected or not, for screen readers, as a selectable label says.
                                    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, &said));
                                    if r.clicked() {
                                        picked = Some((ids.clone(), ui.input(|i| i.modifiers)));
                                    }
                                    context::menu(&r, &groups, &mut chosen);
                                }
                                ui.end_row();
                            }
                            if let Some(totals) = &table.totals {
                                for (i, (v, c)) in totals.iter().zip(&table.columns).enumerate() {
                                    match v {
                                        Value::Null if i == 0 => ui.label(RichText::new(t("Total")).strong()),
                                        v => ui.monospace(RichText::new(said(c, v)).strong()),
                                    };
                                }
                                ui.end_row();
                            }
                        });
                    });
                    if let Some((ids, mods)) = picked {
                        self.pick_up(ids, mods);
                    }
                }
            }
        });
        if let Some(action) = chosen {
            self.act(action);
        }
    }

    fn dialogs(&mut self, ctx: &Context) {
        self.type_properties(ctx);
        self.family_type_properties(ctx);
        self.materials_window(ctx);
        self.favorites_window(ctx);
        self.family_editor_window(ctx);
        self.snaps_window(ctx);
        self.parameters_window(ctx);
        self.new_schedule_window(ctx);
        self.schedule_window(ctx);
        self.graphics_window(ctx);
        self.go_to_view_window(ctx);
        self.filters_window(ctx);
        self.templates_window(ctx);
        self.phasing_window(ctx);
        self.display_window(ctx);
        self.reinforce_window(ctx);
        self.line_weights_window(ctx);
        self.object_styles_window(ctx);
        self.line_styles_window(ctx);
        self.title_blocks_window(ctx);
        self.fill_patterns_window(ctx);
        self.text_type_window(ctx);
        self.dimension_type_window(ctx);
        self.dimension_text_window(ctx);
        self.standards_window(ctx);
        self.electrical_window(ctx);
        self.options_window(ctx);
        self.purge_window(ctx);
        if std::mem::take(&mut self.link_dialog) {
            self.link_model_dialog(ctx);
        }
        if std::mem::take(&mut self.cloud_dialog) {
            self.point_cloud_dialog(ctx);
        }
        self.point_cloud_window(ctx);
        if let Some(link) = self.cad_dialog.take() {
            self.import_cad_dialog(ctx, link);
        }
        self.import_cad_window(ctx);
        match self.image_dialog.take() {
            Some(true) => self.import_pdf_dialog(ctx),
            Some(false) => self.import_image_dialog(ctx),
            None => {}
        }
        self.import_pdf_window(ctx);
        self.manage_images_window(ctx);
        self.select_by_id_window(ctx);
        self.work_plane_window(ctx);
        self.ids_of_selection_window(ctx);
        self.warnings_window(ctx);
        self.selection_filter_window(ctx);
        self.paste_levels_window(ctx);
        self.temp_dim_window(ctx);
        self.paste_views_window(ctx);
        self.delete_type_window(ctx);
        self.shortcuts_window(ctx);
        self.settings_window(ctx);
        #[cfg(feature = "plugins")]
        self.plugins_window(ctx);
        #[cfg(feature = "plugins")]
        self.plugin_report_window(ctx);
        self.background_grid_window(ctx);
        #[cfg(feature = "updater")]
        self.installing_box(ctx);
        self.importing_box(ctx);
        self.report_window(ctx);
        if self.pending.is_none() {
            self.recovery_window(ctx);
        }
        let Some(pending) = self.pending.clone() else { return };
        #[derive(PartialEq)]
        enum Choice {
            Save,
            Discard,
            Cancel,
        }
        let mut choice = None;
        let name = self.session.model.name.clone();
        let r = Modal::new(UiId::new("unsaved"), tf("Save changes to {}?", &[&name])).width(580.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t("If you don't, the changes since it was last saved are lost."));
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, c) in [("Save", Choice::Save), (msg!("Don't Save"), Choice::Discard), ("Cancel", Choice::Cancel)] {
                        if ui.button(t(label)).clicked() {
                            choice = Some(c);
                        }
                    }
                });
            });
        });
        if choice.is_some() || r.should_close() {
            self.pending = None;
        }
        match choice {
            Some(Choice::Save) => self.save(ctx, Some(pending)),
            Some(Choice::Discard) => self.proceed(pending, ctx),
            _ => {}
        }
    }
}

impl BimApp {
    /// Cut a window's `events` for one frame after the first button press or release, and hold the
    /// rest, in order, for the frames after (true while any is held). egui takes where a click was,
    /// and what lay under it, from the pointer's last position in the frame, and counts two clicks in
    /// a frame as one: a hand that moves on between one frame and the next, as in fast drawing,
    /// would click somewhere else, or once.
    pub(crate) fn pace(&mut self, events: &mut Vec<Event>) -> bool {
        let mut all = std::mem::take(&mut self.held);
        all.append(events);
        if let Some(cut) = all.iter().position(|e| matches!(e, Event::PointerButton { .. })) {
            self.held = all.split_off(cut + 1);
        }
        *events = all;
        !self.held.is_empty()
    }
}

impl eframe::App for BimApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    /// Tab over the plan cycles through what's under the cursor, as in Revit (or moves between a shape's numbers),
    /// rather than moving the keyboard between widgets.
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        self.pace(&mut raw.events);
        if self.canvas.hovering() && !ctx.egui_wants_keyboard_input() {
            self.tab_cycle |= raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));
            self.flow.back |=
                raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.shift));
            raw.events.retain(|e| !matches!(e, Event::Key { key: Key::Tab, .. }));
        }
    }
}
