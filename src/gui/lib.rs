//! Kenshi's user interface: its MyGUI resources (`data/gui`) read and drawn with Bevy UI.
pub mod res;
pub mod ui;
pub mod xml;

pub use res::{Gui, WidgetDef};
pub use ui::{Built, Caption, EditText, GuiClick, GuiEdit, GuiPlugin, GuiRes, GuiScroll, GuiWidget, Parts, Scroll, SetAlpha, SetCaption, SetColour, SetProgress, SetScroll, SetTextColour, SetVisible, sliced};
