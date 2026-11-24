//! StatusNotifierItem and DBusMenu integration for Linux desktop trays.
use crate::desktop_service::{Command, Event};
use std::{collections::HashMap, sync::mpsc::Sender, time::Duration};
use tokio::sync::mpsc::UnboundedSender;
use zbus::{
    Proxy, connection, fdo,
    zvariant::{ObjectPath, OwnedValue, Str, Value},
};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const ICON: &[u8] = include_bytes!("../../../assets/bo_music.png");
const PROBE_PERIOD: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
type Properties = HashMap<String, OwnedValue>;
type Layout = (i32, Properties, Vec<OwnedValue>);

pub(super) fn start(commands: UnboundedSender<Command>, events: Sender<Event>) {
    tokio::spawn(async move {
        let mut available = false;
        if let Err(error) = serve(commands, &events, &mut available).await {
            if available {
                let _ = events.send(Event::LinuxTrayAvailable(false));
            }
            eprintln!("Linux 托盘不可用：{error}");
        }
    });
}

async fn serve(
    commands: UnboundedSender<Command>,
    events: &Sender<Event>,
    available: &mut bool,
) -> zbus::Result<()> {
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let connection = connection::Builder::session()?
        .serve_at(
            ITEM_PATH,
            Item {
                commands: commands.clone(),
                icon: tray_pixmaps(),
            },
        )?
        .serve_at(MENU_PATH, Menu(commands))?
        .name(name.as_str())?
        .build()
        .await?;
    let bus = fdo::DBusProxy::new(&connection).await?;
    // Query the watcher explicitly; some hosts change their availability without a property signal.
    let watcher = Proxy::new(&connection, WATCHER, WATCHER_PATH, WATCHER).await?;
    let properties = fdo::PropertiesProxy::builder(&connection)
        .destination(WATCHER)?
        .path(WATCHER_PATH)?
        .build()
        .await?;
    let mut registered_owner = None;
    loop {
        let result = tokio::time::timeout(PROBE_TIMEOUT, async {
            let owner = bus.get_name_owner(WATCHER.try_into()?).await?;
            let registered: bool = properties
                .get(WATCHER.try_into()?, "IsStatusNotifierHostRegistered")
                .await?
                .try_into()
                .map_err(zbus::Error::from)?;
            if !registered {
                return Ok(None);
            }
            if registered_owner.as_ref() != Some(&owner) {
                watcher
                    .call::<_, _, ()>("RegisterStatusNotifierItem", &(name.as_str(),))
                    .await?;
            }
            Ok::<_, zbus::Error>(Some(owner))
        })
        .await;
        registered_owner = result.ok().and_then(Result::ok).flatten();
        let next = registered_owner.is_some();
        if next != *available {
            *available = next;
            if events.send(Event::LinuxTrayAvailable(next)).is_err() {
                return Ok(());
            }
        }
        tokio::time::sleep(PROBE_PERIOD).await;
    }
}

struct Item {
    commands: UnboundedSender<Command>,
    icon: Pixmaps,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.commands.send(Command::RaiseMainWindow);
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.commands.send(Command::Toggle);
    }
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn scroll(&self, delta: i32, orientation: &str) {
        if orientation == "vertical" && delta != 0 {
            let _ = self.commands.send(Command::VolumeDelta(delta.signum() * 5));
        }
    }

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        "bo_music"
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "bo-music"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        if self.icon.is_empty() {
            "multimedia-player"
        } else {
            ""
        }
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> &Pixmaps {
        &self.icon
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (&str, Pixmaps, &str, &str) {
        ("", Vec::new(), "bo-music", "")
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(MENU_PATH)
    }
}

fn tray_pixmaps() -> Pixmaps {
    let Ok(image) = image::load_from_memory_with_format(ICON, image::ImageFormat::Png) else {
        return Vec::new();
    };
    [22, 32, 64]
        .into_iter()
        .map(|edge| {
            let icon = image
                .resize_exact(edge, edge, image::imageops::FilterType::Triangle)
                .into_rgba8();
            let mut bytes = icon.into_raw();
            for pixel in bytes.chunks_exact_mut(4) {
                pixel.rotate_right(1);
            }
            (edge as i32, edge as i32, bytes)
        })
        .collect()
}

const ROWS: [(i32, &str); 6] = [
    (1, "显示主窗口"),
    (2, "播放 / 暂停"),
    (3, "上一首"),
    (4, "下一首"),
    (5, ""),
    (6, "退出bo-music"),
];

fn row_properties(id: i32, requested: &[&str]) -> Option<Properties> {
    let fields = match id {
        0 => Properties::from([("children-display".into(), text("submenu"))]),
        5 => Properties::from([("type".into(), text("separator"))]),
        _ => {
            let (_, label) = ROWS.iter().find(|(item, _)| *item == id)?;
            Properties::from([
                ("label".into(), text(label)),
                ("enabled".into(), true.into()),
                ("visible".into(), true.into()),
            ])
        }
    };
    Some(
        fields
            .into_iter()
            .filter(|(name, _)| requested.is_empty() || requested.contains(&name.as_str()))
            .collect(),
    )
}

fn action(id: i32) -> Option<Command> {
    Some(match id {
        1 => Command::RaiseMainWindow,
        2 => Command::Toggle,
        3 => Command::Previous,
        4 => Command::Next,
        6 => Command::ExitApplication,
        _ => return None,
    })
}

fn invalid_item(id: i32) -> fdo::Error {
    fdo::Error::InvalidArgs(format!("unknown tray menu item {id}"))
}
fn text(value: &str) -> OwnedValue {
    OwnedValue::from(Str::from(value.to_owned()))
}

struct Menu(UnboundedSender<Command>);

impl Menu {
    fn dispatch(&self, id: i32, event: &str) -> bool {
        if let Some(command) = action(id)
            && event == "clicked"
        {
            let _ = self.0.send(command);
        }
        row_properties(id, &[]).is_some()
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(out_args("revision", "layout"))]
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<&str>,
    ) -> fdo::Result<(u32, Layout)> {
        let props =
            row_properties(parent_id, &property_names).ok_or_else(|| invalid_item(parent_id))?;
        let children = if parent_id == 0 && recursion_depth != 0 {
            ROWS.into_iter()
                .filter_map(|(id, _)| {
                    let row = (
                        id,
                        row_properties(id, &property_names)?,
                        Vec::<OwnedValue>::new(),
                    );
                    OwnedValue::try_from(Value::new(row)).ok()
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok((1, (parent_id, props, children)))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<&str>,
    ) -> Vec<(i32, Properties)> {
        let ids = if ids.is_empty() {
            std::iter::once(0)
                .chain(ROWS.into_iter().map(|(id, _)| id))
                .collect()
        } else {
            ids
        };
        ids.into_iter()
            .filter_map(|id| Some((id, row_properties(id, &property_names)?)))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> fdo::Result<OwnedValue> {
        row_properties(id, &[name])
            .and_then(|mut props| props.remove(name))
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown menu property {name}")))
    }

    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) -> fdo::Result<()> {
        if self.dispatch(id, event_id) {
            Ok(())
        } else {
            Err(invalid_item(id))
        }
    }

    fn event_group(&self, events: Vec<(i32, &str, Value<'_>, u32)>) -> Vec<i32> {
        events
            .into_iter()
            .filter_map(|(id, event, _, _)| (!self.dispatch(id, event)).then_some(id))
            .collect()
    }

    fn about_to_show(&self, id: i32) -> fdo::Result<bool> {
        if row_properties(id, &[]).is_some() {
            Ok(false)
        } else {
            Err(invalid_item(id))
        }
    }

    #[zbus(out_args("updates_needed", "id_errors"))]
    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (
            Vec::new(),
            ids.into_iter()
                .filter(|&id| row_properties(id, &[]).is_none())
                .collect(),
        )
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }
    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<&str> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "linux_tray_tests.rs"]
mod tests;
