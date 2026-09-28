//! Language of server-side texts (logs, errors, reports) for the current
//! request or background task: Croatian by default, English on request.
use std::cell::Cell;

thread_local!(static EN: Cell<bool> = const { Cell::new(false) });

tokio::task_local! {
    /// Language of the HTTP request being handled (set by the server middleware).
    pub static REQ_EN: bool;
}

pub fn set_en(v: bool) {
    EN.with(|c| c.set(v));
}

pub fn en() -> bool {
    REQ_EN.try_with(|v| *v).unwrap_or_else(|_| EN.with(|c| c.get()))
}

/// `spawn_blocking` that keeps the language of the calling request.
pub fn blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let en = en();
    tokio::task::spawn_blocking(move || {
        set_en(en);
        f()
    })
}

/// `tr!("hrvatski {x}", "English {x}")` — formats the text of the current language.
#[macro_export]
macro_rules! tr {
    ($hr:literal, $en:literal $(,)?) => {
        if $crate::i18n::en() { format!($en) } else { format!($hr) }
    };
    ($hr:literal, $en:literal, $($a:tt)*) => {
        if $crate::i18n::en() { format!($en, $($a)*) } else { format!($hr, $($a)*) }
    };
}
