//! Original ambient loops, embedded so installed builds do not depend on the source tree.
pub fn asset(id: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match id {
        "ambient.fireplace" => (
            "ambient.fireplace.ogg",
            include_bytes!("../assets/ambient/ambient.fireplace.ogg"),
        ),
        "ambient.waves" => (
            "ambient.waves.ogg",
            include_bytes!("../assets/ambient/ambient.waves.ogg"),
        ),
        "ambient.birdsong" => (
            "ambient.birdsong.ogg",
            include_bytes!("../assets/ambient/ambient.birdsong.ogg"),
        ),
        "ambient.crowds" => (
            "ambient.crowds.ogg",
            include_bytes!("../assets/ambient/ambient.crowds.ogg"),
        ),
        "ambient.forest" => (
            "ambient.forest.ogg",
            include_bytes!("../assets/ambient/ambient.forest.ogg"),
        ),
        "ambient.water" => (
            "ambient.water.ogg",
            include_bytes!("../assets/ambient/ambient.water.ogg"),
        ),
        "ambient.bell" => (
            "ambient.bell.ogg",
            include_bytes!("../assets/ambient/ambient.bell.ogg"),
        ),
        "ambient.running_water" => (
            "ambient.running_water.ogg",
            include_bytes!("../assets/ambient/ambient.running_water.ogg"),
        ),
        "ambient.second" => (
            "ambient.second.ogg",
            include_bytes!("../assets/ambient/ambient.second.ogg"),
        ),
        "ambient.heartbeat" => (
            "ambient.heartbeat.flac",
            include_bytes!("../assets/ambient/ambient.heartbeat.flac"),
        ),
        "ambient.fireworks" => (
            "ambient.fireworks.ogg",
            include_bytes!("../assets/ambient/ambient.fireworks.ogg"),
        ),
        "ambient.rain" => (
            "ambient.rain.ogg",
            include_bytes!("../assets/ambient/ambient.rain.ogg"),
        ),
        "texture.vinyl" => (
            "texture.vinyl.ogg",
            include_bytes!("../assets/ambient/texture.vinyl.ogg"),
        ),
        _ => return None,
    })
}

#[cfg(test)]
#[path = "ambient_tests.rs"]
mod tests;
