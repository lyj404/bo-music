//! Business command handlers expand into one exhaustive match in the runtime.

// Command groups share the worker's state and can skip the rest of its loop.
// Assemble a single exhaustive match so new commands must have a handler.
macro_rules! collect_command_arms {
    ($command:ident [$($arms:tt)*] []) => { match $command { $($arms)* } };
    ($command:ident $arms:tt [$handler:ident [$($args:tt)*] $($rest:tt)*]) => {
        $handler!($command $arms [$($rest)*]; $($args)*)
    };
}

#[macro_use]
mod settings_and_reports;
#[macro_use]
mod accounts;
#[macro_use]
mod search_and_comments;
#[macro_use]
mod playlist_edits;
#[macro_use]
mod catalog;
#[macro_use]
mod playback;
