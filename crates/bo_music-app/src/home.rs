//! Account-scoped discovery modules and AI playlist details.
use crate::desktop_service::Command;
use bo_music_api::{discovery, transport::Transport};
use bo_music_domain::{
    catalog::Playlist,
    discovery::{AiPlaylist, HomeContent, HomeFeed, HomeModule},
    track::Track,
};
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub struct HomeItem {
    pub module: HomeModule,
    pub feed: Option<HomeFeed>,
    pub error: bool,
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;

#[derive(Clone)]
pub struct AiView {
    pub title: String,
    pub loading: bool,
    pub error: bool,
    pub playlist: Option<AiPlaylist>,
}

#[derive(Clone, Default)]
pub struct HomeView {
    pub generation: u64,
    pub initialized: bool,
    pub loading: bool,
    pub layout_error: bool,
    pub has_more: bool,
    pub items: Vec<HomeItem>,
    pub ai: Option<AiView>,
}

pub struct Home {
    revision: u64,
    epoch: u64,
    version: u64,
    ai_generation: u64,
    modules: Vec<HomeModule>,
    items: Vec<HomeItem>,
    next: usize,
    loading: bool,
    layout_error: bool,
    task: Option<JoinHandle<()>>,
    ai_task: Option<JoinHandle<()>>,
    ai: Option<AiView>,
}
impl Default for Home {
    fn default() -> Self {
        Self {
            revision: u64::MAX,
            epoch: 0,
            version: 0,
            ai_generation: 0,
            modules: Vec::new(),
            items: Vec::new(),
            next: 0,
            loading: false,
            layout_error: false,
            task: None,
            ai_task: None,
            ai: None,
        }
    }
}
impl Home {
    pub fn view(&self) -> HomeView {
        HomeView {
            generation: self.version,
            initialized: self.revision != u64::MAX,
            loading: self.loading,
            layout_error: self.layout_error,
            has_more: self.next < self.modules.len(),
            items: self.items.clone(),
            ai: self.ai.clone(),
        }
    }
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.version = self.version.wrapping_add(1);
        self.ai_generation = self.ai_generation.wrapping_add(1);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(task) = self.ai_task.take() {
            task.abort();
        }
        self.revision = u64::MAX;
        self.modules.clear();
        self.items.clear();
        self.next = 0;
        self.loading = false;
        self.layout_error = false;
        self.ai = None;
    }
    pub fn reload(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        self.invalidate();
        self.revision = transport.session().snapshot().revision;
        self.loading = true;
        let epoch = self.epoch;
        let revision = self.revision;
        self.task = Some(tokio::spawn(async move {
            let result = discovery::home_modules_at_revision(&transport, revision)
                .await
                .map_err(|_| ());
            let _ = sender.send(Command::HomeLayoutDone(epoch, revision, result));
        }));
    }
    pub fn finish_layout(
        &mut self,
        epoch: u64,
        revision: u64,
        result: Result<Vec<HomeModule>, ()>,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> bool {
        if epoch != self.epoch
            || revision != self.revision
            || revision != transport.session().snapshot().revision
        {
            return false;
        }
        self.task = None;
        self.loading = false;
        self.version = self.version.wrapping_add(1);
        match result {
            Ok(modules) => {
                self.modules = modules
                    .into_iter()
                    .filter(HomeModule::is_supported)
                    .collect();
                self.load_more(4, transport, sender);
            }
            Err(()) => self.layout_error = true,
        }
        true
    }
    pub fn load_more(
        &mut self,
        count: usize,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if self.loading
            || self.next >= self.modules.len()
            || self.revision != transport.session().snapshot().revision
        {
            return;
        }
        let end = self.next.saturating_add(count).min(self.modules.len());
        let modules = self.modules[self.next..end].to_vec();
        let start = self.next;
        let epoch = self.epoch;
        let revision = self.revision;
        self.loading = true;
        self.version = self.version.wrapping_add(1);
        self.task = Some(tokio::spawn(async move {
            let mut rows = Vec::with_capacity(modules.len());
            for module in modules {
                let result = discovery::home_module_at_revision(&transport, &module, revision)
                    .await
                    .map_err(|_| ());
                rows.push((module, result));
                if transport.session().snapshot().revision != revision {
                    break;
                }
            }
            let _ = sender.send(Command::HomeBatchDone(epoch, revision, start, rows));
        }));
    }
    pub fn finish_batch(
        &mut self,
        epoch: u64,
        revision: u64,
        start: usize,
        rows: Vec<(HomeModule, Result<Option<HomeFeed>, ()>)>,
        current: u64,
    ) -> bool {
        if epoch != self.epoch
            || revision != self.revision
            || revision != current
            || start != self.next
        {
            return false;
        }
        self.task = None;
        self.loading = false;
        self.next += rows.len();
        for (module, result) in rows {
            match result {
                Ok(Some(feed)) => self.items.push(HomeItem {
                    module,
                    feed: Some(feed),
                    error: false,
                }),
                Ok(None) => {}
                Err(()) => self.items.push(HomeItem {
                    module,
                    feed: None,
                    error: true,
                }),
            }
        }
        self.version = self.version.wrapping_add(1);
        true
    }
    pub fn refresh(
        &mut self,
        index: usize,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if self.loading
            || generation != self.version
            || self.revision != transport.session().snapshot().revision
        {
            return;
        }
        let Some(item) = self.items.get(index) else {
            return;
        };
        if !item.error && !matches!(item.module.kind, 3 | 10) {
            return;
        }
        let module = item.module.clone();
        let epoch = self.epoch;
        let revision = self.revision;
        self.loading = true;
        self.version = self.version.wrapping_add(1);
        self.task = Some(tokio::spawn(async move {
            let result = discovery::home_module_at_revision(&transport, &module, revision)
                .await
                .map_err(|_| ());
            let _ = sender.send(Command::HomeRefreshDone(epoch, revision, index, result));
        }));
    }
    pub fn finish_refresh(
        &mut self,
        epoch: u64,
        revision: u64,
        index: usize,
        result: Result<Option<HomeFeed>, ()>,
        current: u64,
    ) -> bool {
        if epoch != self.epoch || revision != self.revision || revision != current {
            return false;
        }
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        self.task = None;
        self.loading = false;
        match result {
            Ok(Some(feed)) => {
                item.feed = Some(feed);
                item.error = false;
            }
            _ => item.error = true,
        }
        self.version = self.version.wrapping_add(1);
        true
    }
    pub fn open_ai(
        &mut self,
        item_index: usize,
        group_index: usize,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if generation != self.version || self.revision != transport.session().snapshot().revision {
            return;
        }
        let Some(HomeContent::AiPreviews { groups, .. }) = self
            .items
            .get(item_index)
            .and_then(|item| item.feed.as_ref())
            .map(|feed| &feed.content)
        else {
            return;
        };
        let Some(group) = groups.get(group_index) else {
            return;
        };
        let Ok(index) = i32::try_from(group.index) else {
            return;
        };
        let pass = group.pass_rec_name.clone();
        let title = group.title.clone();
        if let Some(task) = self.ai_task.take() {
            task.abort();
        }
        self.ai_generation = self.ai_generation.wrapping_add(1);
        self.version = self.version.wrapping_add(1);
        self.ai = Some(AiView {
            title,
            loading: true,
            error: false,
            playlist: None,
        });
        let epoch = self.epoch;
        let revision = self.revision;
        let ai_generation = self.ai_generation;
        self.ai_task = Some(tokio::spawn(async move {
            let result = discovery::ai_playlist_at_revision(&transport, index, &pass, revision)
                .await
                .map_err(|_| ());
            let _ = sender.send(Command::HomeAiDone(epoch, revision, ai_generation, result));
        }));
    }
    pub fn finish_ai(
        &mut self,
        epoch: u64,
        revision: u64,
        ai_generation: u64,
        result: Result<Option<AiPlaylist>, ()>,
        current: u64,
    ) -> bool {
        if epoch != self.epoch
            || revision != self.revision
            || current != revision
            || ai_generation != self.ai_generation
        {
            return false;
        }
        let Some(ai) = self.ai.as_mut() else {
            return false;
        };
        self.ai_task = None;
        ai.loading = false;
        match result {
            Ok(Some(playlist)) => ai.playlist = Some(playlist),
            _ => ai.error = true,
        }
        self.version = self.version.wrapping_add(1);
        true
    }
    pub fn close_ai(&mut self, generation: u64, revision: u64) {
        if generation != self.version || revision != self.revision {
            return;
        }
        if let Some(task) = self.ai_task.take() {
            task.abort();
        }
        self.ai_generation = self.ai_generation.wrapping_add(1);
        self.version = self.version.wrapping_add(1);
        self.ai = None;
    }
    pub fn playlist(
        &self,
        item: usize,
        index: usize,
        id: i64,
        generation: u64,
        revision: u64,
    ) -> Option<Playlist> {
        if generation != self.version || revision != self.revision || self.ai.is_some() {
            return None;
        }
        let HomeContent::Playlists(playlists) = &self.items.get(item)?.feed.as_ref()?.content
        else {
            return None;
        };
        playlists
            .get(index)
            .filter(|playlist| playlist.id == id && id > 0)
            .cloned()
    }
    pub fn track(
        &self,
        item: Option<usize>,
        index: usize,
        id: i64,
        generation: u64,
        revision: u64,
    ) -> Option<Track> {
        if generation != self.version || revision != self.revision || id <= 0 {
            return None;
        }
        let tracks = match item {
            Some(item) if self.ai.is_none() => {
                match &self.items.get(item)?.feed.as_ref()?.content {
                    HomeContent::Tracks(tracks) => tracks,
                    _ => return None,
                }
            }
            None => &self.ai.as_ref()?.playlist.as_ref()?.tracks,
            _ => return None,
        };
        tracks.get(index).filter(|track| track.id == id).cloned()
    }
    pub fn ai_tracks(&self, generation: u64, revision: u64) -> Option<Vec<Track>> {
        if generation != self.version || revision != self.revision {
            return None;
        }
        let tracks = &self.ai.as_ref()?.playlist.as_ref()?.tracks;
        (!tracks.is_empty()).then(|| tracks.clone())
    }
    pub fn module_tracks(&self, item: usize, generation: u64, revision: u64) -> Option<Vec<Track>> {
        if generation != self.version
            || revision != self.revision
            || self.loading
            || self.ai.is_some()
        {
            return None;
        }
        let item = self.items.get(item)?;
        if !matches!(item.module.kind, 3 | 10) || item.error {
            return None;
        }
        let HomeContent::Tracks(tracks) = &item.feed.as_ref()?.content else {
            return None;
        };
        (!tracks.is_empty()).then(|| tracks.clone())
    }
}
