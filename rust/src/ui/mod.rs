pub mod actions;
pub mod egui_app;
pub mod events;
pub mod panels;
pub mod state;

// 外からは SameFileApp をこれで使えるようにしておくと楽
pub use state::SameFileApp;
