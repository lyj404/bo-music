use super::*;

#[test]
fn tray_layout_exports_six_rows_and_dispatches_only_clicked_actions() {
    let (commands, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let menu = Menu(commands);
    let (_, (_, root, children)) = menu.get_layout(0, -1, vec![]).unwrap();
    assert_eq!(
        <&str>::try_from(&root["children-display"]).unwrap(),
        "submenu"
    );
    assert_eq!(children.len(), 6);
    assert!(menu.get_layout(0, 0, vec![]).unwrap().1.2.is_empty());
    assert_eq!(
        menu.get_group_properties(vec![2], vec!["label"])[0].1.len(),
        1
    );
    assert_eq!(
        menu.event_group(vec![
            (99, "clicked", Value::from(0), 0),
            (2, "opened", Value::from(0), 0),
            (4, "clicked", Value::from(0), 0)
        ]),
        [99]
    );
    assert!(matches!(receiver.try_recv(), Ok(Command::Next)));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn tray_icon_pixmaps_have_the_requested_pixel_sizes() {
    let icons = tray_pixmaps();
    assert_eq!(
        icons
            .iter()
            .map(|(w, h, pixels)| (*w, *h, pixels.len()))
            .collect::<Vec<_>>(),
        [
            (22, 22, 22 * 22 * 4),
            (32, 32, 32 * 32 * 4),
            (64, 64, 64 * 64 * 4)
        ]
    );
}
