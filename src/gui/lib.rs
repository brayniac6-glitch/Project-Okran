//! Kenshi's user interface: its MyGUI resources (`data/gui`) read and drawn with Bevy UI.
pub mod res;
pub mod ui;
pub mod xml;

pub use res::{Gui, WidgetDef};
pub use ui::{Built, Caption, CodedColour, EditText, GuiClick, GuiEdit, GuiEditDone, GuiPlugin, GuiRes, GuiScroll, GuiWidget, Parts, Scroll, SetAlpha, SetCaption, SetColour, SetProgress, SetScroll, SetScrollTrack, SetTextColour, SetVisible, colour_code, sliced};
