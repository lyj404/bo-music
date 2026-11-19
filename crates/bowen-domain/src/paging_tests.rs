use super::*;

#[test]
fn short_pages_advance_by_page_size_until_an_empty_page() {
    let mut cursor = PagedCursor::new(PageOrigin::Zero, 100);
    assert_eq!(cursor.page_number(), 0);
    cursor.advance(99);
    assert_eq!(
        (cursor.offset(), cursor.page_number(), cursor.exhausted()),
        (100, 1, false)
    );
    cursor.advance(0);
    assert!(cursor.exhausted());
    cursor.advance(5);
    assert_eq!(cursor.offset(), 100);
}

#[test]
fn origin_and_page_size_match_the_existing_client() {
    assert_eq!(PagedCursor::new(PageOrigin::One, 0).page_size(), 1);
    let cursor = PagedCursor::new(PageOrigin::One, 101);
    assert_eq!((cursor.page_size(), cursor.page_number()), (100, 1));
}
