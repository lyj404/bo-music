//! Stable audio effect identities and user-editable equalizer settings.

pub const EQ_FREQUENCIES: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
pub const MAX_CUSTOM_PRESETS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbientSound {
    pub id: &'static str,
    pub name: &'static str,
}

pub const AMBIENT_SOUNDS: &[AmbientSound] = &[
    AmbientSound {
        id: "ambient.fireplace",
        name: "壁炉",
    },
    AmbientSound {
        id: "ambient.waves",
        name: "海浪",
    },
    AmbientSound {
        id: "ambient.birdsong",
        name: "鸟鸣",
    },
    AmbientSound {
        id: "ambient.crowds",
        name: "人潮",
    },
    AmbientSound {
        id: "ambient.forest",
        name: "森林",
    },
    AmbientSound {
        id: "ambient.water",
        name: "水滴",
    },
    AmbientSound {
        id: "ambient.bell",
        name: "风铃",
    },
    AmbientSound {
        id: "ambient.running_water",
        name: "流水",
    },
    AmbientSound {
        id: "ambient.second",
        name: "秒针",
    },
    AmbientSound {
        id: "ambient.heartbeat",
        name: "心跳",
    },
    AmbientSound {
        id: "ambient.fireworks",
        name: "烟火",
    },
    AmbientSound {
        id: "ambient.rain",
        name: "雨声",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub filter: &'static str,
    pub texture: Option<&'static str>,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "galaxy.clear",
        name: "清澈人声",
        description: "保留低频，只抬中高频的通透感",
        filter: "equalizer=f=300:t=q:w=1.2:g=-2,equalizer=f=3000:t=q:w=1.1:g=3,equalizer=f=9000:t=q:w=1:g=2.5,alimiter=limit=0.95:level=0",
        texture: None,
    },
    Preset {
        id: "galaxy.bass",
        name: "重低音",
        description: "低频托底并补谐波低音",
        filter: "bass=g=6:f=80:w=0.6,equalizer=f=160:t=q:w=1:g=2,virtualbass=cutoff=140:strength=2.2,alimiter=limit=0.9:level=0",
        texture: None,
    },
    Preset {
        id: "galaxy.3d",
        name: "3D环绕",
        description: "相位展宽加长反射",
        filter: "stereotools=slev=1.9:mlev=0.88,haas=side_gain=0.55,aecho=1:1:83|141|219:0.16|0.11|0.07,alimiter=limit=0.9:level=0",
        texture: None,
    },
    Preset {
        id: "galaxy.vinyl",
        name: "黑胶",
        description: "温暖偏暗的模拟味",
        filter: "highpass=f=50,equalizer=f=120:t=q:w=0.7:g=3,equalizer=f=700:t=q:w=1.2:g=-1.5,lowpass=f=8500,acompressor=threshold=0.4:ratio=1.8:attack=25:release=180:makeup=1.2,stereotools=slev=0.9",
        texture: Some("texture.vinyl"),
    },
    Preset {
        id: "galaxy.live",
        name: "LiveHouse",
        description: "多抽头混响",
        filter: "aecho=1:1:35|60|90:0.22|0.16|0.1,stereotools=slev=1.4:mlev=0.95,volume=0.5dB,alimiter=limit=0.92:level=0",
        texture: None,
    },
    Preset {
        id: "galaxy.night",
        name: "夜晚",
        description: "压暗高频并轻压动态",
        filter: "lowpass=f=7500,equalizer=f=120:t=q:w=1:g=2,equalizer=f=350:t=q:w=1:g=0.5,equalizer=f=6000:t=q:w=1.2:g=-3,acompressor=threshold=0.35:ratio=2:attack=20:release=250:makeup=1.15,stereotools=slev=0.85",
        texture: None,
    },
    Preset {
        id: "galaxy.wide",
        name: "开阔宽音",
        description: "左右展宽",
        filter: "stereowiden=delay=22:feedback=0.25:crossfeed=0.2:drymix=0.85",
        texture: None,
    },
    Preset {
        id: "galaxy.car",
        name: "车载",
        description: "补低频、提清晰度",
        filter: "bass=g=6:f=110:w=0.7,equalizer=f=400:t=q:w=1.1:g=-2,equalizer=f=2800:t=q:w=1:g=3.5,equalizer=f=9000:t=q:w=1:g=2,stereotools=slev=1.25,alimiter=limit=0.95:level=0",
        texture: None,
    },
    Preset {
        id: "viper.3d",
        name: "3D环绕（增强）",
        description: "相位展宽加长反射",
        filter: "stereotools=slev=2.1:mlev=0.82,haas=side_gain=0.8,aecho=1:1:71|127|191:0.12|0.09|0.06,volume=0.5dB,alimiter=limit=0.9:level=0",
        texture: None,
    },
    Preset {
        id: "viper.bass",
        name: "超重低音",
        description: "下潜与谐波低音叠加",
        filter: "virtualbass=cutoff=180:strength=2.4,bass=g=7:f=70:w=0.5,equalizer=f=110:t=q:w=1:g=2,alimiter=limit=0.9:level=0",
        texture: None,
    },
    Preset {
        id: "viper.vocal",
        name: "纯净人声",
        description: "抬高中频并轻压动态",
        filter: "highpass=f=70,equalizer=f=280:t=q:w=1.2:g=-3,equalizer=f=2600:t=q:w=1.1:g=4,equalizer=f=6000:t=q:w=1:g=2,acompressor=threshold=0.35:ratio=2.5:attack=10:release=200:makeup=1.1",
        texture: None,
    },
    Preset {
        id: "viper.live",
        name: "虚拟现场",
        description: "多抽头短反射加声场展宽",
        filter: "aecho=1:1:25|45|70:0.32|0.22|0.14,stereotools=slev=1.35:mlev=1,volume=1.5dB,alimiter=limit=0.92:level=0",
        texture: None,
    },
    Preset {
        id: "viper.eq",
        name: "均衡器音效",
        description: "温和的 V 形曲线",
        filter: "volume=-3dB,equalizer=f=31:t=q:w=1.2:g=3,equalizer=f=62:t=q:w=1.2:g=2,equalizer=f=125:t=q:w=1.2:g=1,equalizer=f=500:t=q:w=1.2:g=-1,equalizer=f=1000:t=q:w=1.2:g=-1,equalizer=f=4000:t=q:w=1.2:g=1,equalizer=f=8000:t=q:w=1.2:g=2,equalizer=f=16000:t=q:w=1.2:g=3",
        texture: None,
    },
    Preset {
        id: "viper.ai",
        name: "响度均衡",
        description: "动态响度归一",
        filter: "dynaudnorm=framelen=500:maxgain=3:targetrms=0.35:compress=2:threshold=0.05,equalizer=f=2500:t=q:w=1:g=1.5,alimiter=limit=0.92:level=0",
        texture: None,
    },
    Preset {
        id: "viper.hifi",
        name: "一键HiFi",
        description: "两端轻抬并加限幅",
        filter: "equalizer=f=60:t=q:w=1:g=2,equalizer=f=12000:t=q:w=1:g=2,alimiter=limit=0.95",
        texture: None,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct CustomPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bands: [f64; 10],
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioEffects {
    pub enabled: bool,
    pub preset_id: String,
    pub ambient_id: String,
    pub ambient_volume: f64,
    pub custom_presets: Vec<CustomPreset>,
}

impl Default for AudioEffects {
    fn default() -> Self {
        Self {
            enabled: false,
            preset_id: String::new(),
            ambient_id: String::new(),
            ambient_volume: 30.0,
            custom_presets: Vec::new(),
        }
    }
}

impl AudioEffects {
    pub fn normalize(&mut self) {
        self.ambient_volume = if self.ambient_volume.is_finite() {
            self.ambient_volume.clamp(0.0, 100.0)
        } else {
            30.0
        };
        let mut unique = Vec::<String>::new();
        self.custom_presets.retain_mut(|p| {
            p.id = p.id.trim().to_owned();
            let id = p.id.to_ascii_lowercase();
            if !id.starts_with("custom.")
                || unique.len() >= MAX_CUSTOM_PRESETS
                || unique.contains(&id)
            {
                return false;
            }
            unique.push(id);
            p.name = truncate_or_default(&p.name, 24, "未命名");
            p.description = truncate_or_default(&p.description, 32, "自定义十段均衡曲线");
            for band in &mut p.bands {
                *band = if band.is_finite() {
                    band.clamp(-12.0, 12.0)
                } else {
                    0.0
                };
            }
            true
        });
        let id = self.preset_id.trim();
        self.preset_id = PRESETS
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(id))
            .map(|p| p.id.to_owned())
            .or_else(|| {
                self.custom_presets
                    .iter()
                    .find(|p| p.id.eq_ignore_ascii_case(id))
                    .map(|p| p.id.clone())
            })
            .unwrap_or_default();
        self.ambient_id = AMBIENT_SOUNDS
            .iter()
            .find(|sound| sound.id.eq_ignore_ascii_case(self.ambient_id.trim()))
            .map_or_else(String::new, |sound| sound.id.to_owned());
    }
}

fn truncate_or_default(value: &str, max: usize, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.into()
    } else {
        value.chars().take(max).collect()
    }
}
