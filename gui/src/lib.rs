//! Supervisor core of the Graphene node GUI: everything except the window and the tray, so it can be
//! exercised under Wine with supervisor-cli, where WebView2 is not available.

pub mod glyphs;
pub mod i18n;
pub mod logtail;
pub mod rpc;
pub mod settings;
pub mod supervisor;
#[cfg(windows)]
pub mod win;
