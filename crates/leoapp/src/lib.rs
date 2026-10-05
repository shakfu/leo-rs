//! leoapp: the state and commands of a leolib front end, with no renderer.
//!
//! `App` takes keys as `keys::KeyEvent` and holds everything a view draws:
//! modes, the vim body editor, the minibuffer, search, colouring and theme.
//! A front end converts its own key events, and draws from `App`.

pub mod app;
pub mod bindings;
pub mod commands;
pub mod config;
pub mod editor;
pub mod highlight;
pub mod history;
pub mod keys;
pub mod keywords;
pub mod minibuffer;
pub mod search;
pub mod substitute;
pub mod theme;
pub mod treesit;
pub mod view;
