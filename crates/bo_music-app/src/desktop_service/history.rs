//! Serializes history persistence away from the async command loop.
use super::{Command, Event};
use bo_music_domain::track::Track;
use bo_music_storage::{
    history::HistoryStore, paths::DataPaths, search_history::SearchHistoryStore,
};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

pub(super) enum HistoryWork {
    Search {
        scope: String,
        revision: u64,
        action: SearchHistoryAction,
    },
    Load {
        scope: String,
        generation: u64,
        revision: u64,
    },
    Record {
        scope: String,
        track: Box<Track>,
        revision: u64,
    },
}

pub(super) enum SearchHistoryAction {
    Load,
    Record(String),
    Clear,
}

pub(super) fn history_worker(
    paths: DataPaths,
    commands: mpsc::UnboundedSender<Command>,
    events: std_mpsc::Sender<Event>,
) -> (std_mpsc::Sender<HistoryWork>, std::thread::JoinHandle<()>) {
    let (sender, receiver) = std_mpsc::channel();
    let worker = std::thread::spawn(move || {
        let store = HistoryStore::new(paths.clone());
        let search_store = SearchHistoryStore::new(paths);
        while let Ok(work) = receiver.recv() {
            match work {
                HistoryWork::Search {
                    scope,
                    revision,
                    action,
                } => {
                    let result = match action {
                        SearchHistoryAction::Load => {
                            search_store.load(&scope).map_err(|_| "无法读取搜索历史")
                        }
                        SearchHistoryAction::Record(keyword) => search_store
                            .record(&scope, keyword)
                            .map_err(|_| "无法保存搜索历史"),
                        SearchHistoryAction::Clear => search_store
                            .clear(&scope)
                            .map(|()| Vec::new())
                            .map_err(|_| "无法清空搜索历史"),
                    };
                    let _ = commands.send(Command::SearchHistoryDone(
                        revision,
                        result.map_err(str::to_owned),
                    ));
                }
                HistoryWork::Load {
                    scope,
                    generation,
                    revision,
                } => {
                    let result = store
                        .load(&scope)
                        .map_err(|_| "读取最近播放失败".to_owned());
                    let _ = commands.send(Command::HistoryDone(generation, revision, result));
                }
                HistoryWork::Record {
                    scope,
                    track,
                    revision,
                } => {
                    if store.record(&scope, &track).is_err() {
                        let _ =
                            events.send(Event::Status("保存最近播放失败，请检查数据目录".into()));
                    } else {
                        let _ = commands.send(Command::HistoryRecorded(revision));
                    }
                }
            }
        }
    });
    (sender, worker)
}
