use super::runtime::ServiceOptions;
use super::*;
use crate::playlist_cover::CoverEdit;
use bo_music_domain::comments::CommentSort;
use bo_music_storage::history::HistoryStore;

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[path = "desktop_service_tests_1.rs"]
mod cases_1;
#[path = "desktop_service_tests_2.rs"]
mod cases_2;
#[path = "desktop_service_tests_3.rs"]
mod cases_3;
#[path = "desktop_service_tests_4.rs"]
mod cases_4;
