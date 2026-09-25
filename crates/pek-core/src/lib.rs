//! PekSwitcher core: everything that does not depend on the operating system.
//!
//! * [`keys`], [`layout`] — physical keys and keyboard layouts;
//! * [`model`] with [`dict`], [`charmodel`], [`yo`] — language knowledge;
//! * [`detect`], [`typo`] — deciding what to change in a word;
//! * [`engine`] — the state machine driven by a platform keyboard hook;
//! * [`settings`], [`convert`], [`wav`] — helpers for the app.

pub mod charmodel;
pub mod convert;
pub mod detect;
pub mod dict;
pub mod engine;
pub mod keys;
pub mod layout;
pub mod model;
pub mod settings;
pub mod text;
pub mod typo;
pub mod wav;
pub mod yo;

pub use engine::{Action, Engine, EngineConfig, KeyEvent, Op, Origin, RawKey, Sound, Stroke, TypeItem};
pub use keys::Code;
pub use layout::{Lang, LayoutMap, Layouts};
pub use model::Model;
pub use settings::Settings;
