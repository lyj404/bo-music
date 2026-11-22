//! Account-scoped music-library navigation and paginated category albums.
use crate::desktop_service::Command;
pub use bo_music_api::library::LibrarySort;
use bo_music_api::{catalog::service_cursor, library, transport::Transport};
use bo_music_domain::{
    catalog::{Album, LibraryGroup},
    paging::PagedCursor,
};
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub struct LibraryView {
    pub generation: u64,
    pub initialized: bool,
    pub groups: Vec<LibraryGroup>,
    pub selected_group: Option<usize>,
    pub selected_child: Option<usize>,
    pub sort: LibrarySort,
    pub albums: Vec<Album>,
    pub loading: bool,
    pub error: Option<String>,
    pub has_more: bool,
}

pub struct MusicLibrary {
    revision: u64,
    epoch: u64,
    version: u64,
    groups: Vec<LibraryGroup>,
    selected_group: Option<usize>,
    selected_child: Option<usize>,
    sort: LibrarySort,
    albums: Vec<Album>,
    cursor: PagedCursor,
    loading: bool,
    error: Option<String>,
    task: Option<JoinHandle<()>>,
}
impl Default for MusicLibrary {
    fn default() -> Self {
        Self {
            revision: u64::MAX,
            epoch: 0,
            version: 0,
            groups: Vec::new(),
            selected_group: None,
            selected_child: None,
            sort: LibrarySort::Curated,
            albums: Vec::new(),
            cursor: service_cursor(30),
            loading: false,
            error: None,
            task: None,
        }
    }
}
impl MusicLibrary {
    pub fn generation(&self) -> u64 {
        self.version
    }
    pub fn view(&self) -> LibraryView {
        let valid_category = self
            .selected_group
            .and_then(|index| self.groups.get(index))
            .and_then(|group| {
                self.selected_child
                    .and_then(|index| group.children.get(index))
                    .map(|child| !group.id.trim().is_empty() && !child.id.trim().is_empty())
            })
            .unwrap_or(false);
        LibraryView {
            generation: self.version,
            initialized: self.revision != u64::MAX,
            groups: self.groups.clone(),
            selected_group: self.selected_group,
            selected_child: self.selected_child,
            sort: self.sort,
            albums: self.albums.clone(),
            loading: self.loading,
            error: self.error.clone(),
            has_more: valid_category && !self.cursor.exhausted(),
        }
    }
    fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.version = self.version.wrapping_add(1);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.loading = false;
    }
    pub fn invalidate(&mut self) {
        self.cancel();
        self.revision = u64::MAX;
        self.groups.clear();
        self.selected_group = None;
        self.selected_child = None;
        self.albums.clear();
        self.error = None;
        self.cursor = service_cursor(30);
    }
    pub fn reload(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        self.invalidate();
        self.revision = transport.session().snapshot().revision;
        self.loading = true;
        let epoch = self.epoch;
        let revision = self.revision;
        self.task = Some(tokio::spawn(async move {
            let result = library::navigation_at_revision(&transport, revision)
                .await
                .map_err(|_| ());
            let _ = sender.send(Command::LibraryNavigationDone(epoch, revision, result));
        }));
    }
    pub fn finish_navigation(
        &mut self,
        epoch: u64,
        revision: u64,
        result: Result<Vec<LibraryGroup>, ()>,
        current: u64,
    ) -> bool {
        if epoch != self.epoch || revision != self.revision || revision != current {
            return false;
        }
        self.task = None;
        self.loading = false;
        self.version = self.version.wrapping_add(1);
        match result {
            Ok(groups) => {
                self.groups = groups;
                self.error = None;
            }
            Err(()) => self.error = Some("加载乐库失败，请重试".into()),
        }
        true
    }
    pub fn select_group(
        &mut self,
        index: usize,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if generation != self.version
            || self.revision != transport.session().snapshot().revision
            || index >= self.groups.len()
        {
            return;
        }
        self.cancel();
        self.selected_group = Some(index);
        self.selected_child = (!self.groups[index].children.is_empty()).then_some(0);
        self.sort = LibrarySort::Curated;
        self.albums.clear();
        self.cursor = service_cursor(30);
        self.error = None;
        self.next(transport, sender);
    }
    pub fn select_child(
        &mut self,
        index: usize,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if generation != self.version || self.revision != transport.session().snapshot().revision {
            return;
        }
        let Some(group) = self.selected_group.and_then(|group| self.groups.get(group)) else {
            return;
        };
        if index >= group.children.len() || self.selected_child == Some(index) {
            return;
        }
        self.cancel();
        self.selected_child = Some(index);
        self.albums.clear();
        self.cursor = service_cursor(30);
        self.error = None;
        self.next(transport, sender);
    }
    pub fn sort(
        &mut self,
        sort: LibrarySort,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if generation != self.version
            || self.selected_child.is_none()
            || self.sort == sort
            || self.revision != transport.session().snapshot().revision
        {
            return;
        }
        self.cancel();
        self.sort = sort;
        self.albums.clear();
        self.cursor = service_cursor(30);
        self.error = None;
        self.next(transport, sender);
    }
    pub fn back(&mut self, generation: u64, revision: u64) {
        if generation != self.version || revision != self.revision || self.selected_group.is_none()
        {
            return;
        }
        self.cancel();
        self.selected_group = None;
        self.selected_child = None;
        self.albums.clear();
        self.error = None;
    }
    pub fn next(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        if self.loading
            || self.cursor.exhausted()
            || self.revision != transport.session().snapshot().revision
        {
            return;
        }
        let Some(group) = self.selected_group.and_then(|index| self.groups.get(index)) else {
            return;
        };
        let Some(child) = self
            .selected_child
            .and_then(|index| group.children.get(index))
        else {
            return;
        };
        if group.id.trim().is_empty() || child.id.trim().is_empty() {
            self.error = Some("分类缺少标识，无法加载专辑".into());
            return;
        }
        let parent = group.id.clone();
        let child = child.id.clone();
        let sort = self.sort;
        self.cancel();
        let epoch = self.epoch;
        let revision = self.revision;
        let mut cursor = self.cursor.clone();
        self.loading = true;
        self.error = None;
        self.version = self.version.wrapping_add(1);
        self.task = Some(tokio::spawn(async move {
            let result = library::albums_at_revision(
                &transport,
                &parent,
                &child,
                sort,
                &mut cursor,
                revision,
            )
            .await
            .map(|page| page.items)
            .map_err(|_| ());
            let _ = sender.send(Command::LibraryAlbumsDone(epoch, revision, cursor, result));
        }));
    }
    pub fn finish_albums(
        &mut self,
        epoch: u64,
        revision: u64,
        cursor: PagedCursor,
        result: Result<Vec<Album>, ()>,
        current: u64,
    ) -> bool {
        if epoch != self.epoch || revision != self.revision || revision != current {
            return false;
        }
        self.task = None;
        self.loading = false;
        self.version = self.version.wrapping_add(1);
        match result {
            Ok(albums) => {
                self.cursor = cursor;
                self.albums.extend(albums);
                self.error = None;
            }
            Err(()) => self.error = Some("加载专辑失败，请重试".into()),
        }
        true
    }
    pub fn album(
        &self,
        index: Option<usize>,
        id: i64,
        generation: u64,
        revision: u64,
    ) -> Option<Album> {
        if id <= 0 || generation != self.version || revision != self.revision {
            return None;
        }
        let album = match index {
            Some(index) => self.albums.get(index),
            None => self
                .selected_group
                .and_then(|group| self.groups.get(group))
                .and_then(|group| {
                    self.selected_child
                        .and_then(|child| group.children.get(child))
                })
                .and_then(|child| child.sample_album.as_ref()),
        }?;
        (album.id == id).then(|| album.clone())
    }
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
