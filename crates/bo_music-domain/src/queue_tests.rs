use super::*;
fn track(id: i64) -> Track {
    Track {
        id,
        title: id.to_string(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 0,
        available_qualities: vec![],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    }
}

#[test]
fn modes_distinguish_manual_next_and_natural_end() {
    let mut queue = PlayQueue::with_seed(1);
    queue.replace(vec![track(1), track(2), track(3)], 0);
    queue.set_mode(PlayMode::SingleLoop);
    assert_eq!(queue.on_finished(), FinishAction::ReplayCurrent);
    assert_eq!(queue.current().unwrap().id, 1);
    assert!(queue.move_next());
    assert_eq!(queue.current().unwrap().id, 2);
    queue.set_mode(PlayMode::ListLoop);
    queue.move_to(2);
    assert_eq!(queue.on_finished(), FinishAction::PlayNext);
    assert_eq!(queue.current().unwrap().id, 1);
    assert!(queue.previous());
    assert_eq!(queue.current().unwrap().id, 3);
    queue.set_mode(PlayMode::Sequential);
    assert_eq!(queue.on_finished(), FinishAction::Stop);
}

#[test]
fn mutations_preserve_current_and_deduplicate_bulk_additions() {
    let mut queue = PlayQueue::with_seed(2);
    assert_eq!(
        queue.append_range(vec![track(1), track(2), track(2), track(0), track(3)]),
        3
    );
    queue.move_to(1);
    assert!(queue.reorder(0, 2));
    assert_eq!(queue.current().unwrap().id, 2);
    queue.insert_next(track(4));
    assert_eq!(queue.peek_next().unwrap().id, 4);
    assert!(queue.remove(queue.current_index().unwrap()));
    assert_eq!(queue.current().unwrap().id, 4);
    assert!(!queue.append(track(4)));
    assert!(!queue.remove(99));
    queue.clear();
    assert!(queue.current().is_none());
    assert_eq!(queue.on_finished(), FinishAction::Stop);
}

#[test]
fn shuffle_visits_every_item_then_avoids_immediate_repeat() {
    let mut queue = PlayQueue::with_seed(42);
    queue.replace((1..=8).map(track).collect(), 3);
    queue.set_mode(PlayMode::Shuffle);
    assert_eq!(queue.current().unwrap().id, 4);
    assert!(!queue.reorder(0, 1));
    let mut seen = HashSet::new();
    for _ in 0..8 {
        assert!(seen.insert(queue.current().unwrap().id));
        if seen.len() < 8 {
            assert!(queue.move_next());
        }
    }
    assert!(queue.peek_next().is_none());
    let previous = queue.current().unwrap().id;
    assert!(queue.move_next());
    assert_ne!(queue.current().unwrap().id, previous);
    assert!(!queue.previous());
}
