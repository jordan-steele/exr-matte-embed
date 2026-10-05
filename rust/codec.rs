use clap::ValueEnum;
use exr::prelude::Compression;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    None,
    Rle,
    Zip,
    Zips,
    #[default]
    Piz,
    Pxr24,
    B44,
    B44a,
    Dwaa,
    Dwab,
}

impl Codec {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Rle,
        Self::Zip,
        Self::Zips,
        Self::Piz,
        Self::Pxr24,
        Self::B44,
        Self::B44a,
        Self::Dwaa,
        Self::Dwab,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rle => "rle",
            Self::Zip => "zip",
            Self::Zips => "zips",
            Self::Piz => "piz",
            Self::Pxr24 => "pxr24",
            Self::B44 => "b44",
            Self::B44a => "b44a",
            Self::Dwaa => "dwaa",
            Self::Dwab => "dwab",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Rle => "RLE",
            Self::Zip => "ZIP",
            Self::Zips => "ZIPS",
            Self::Piz => "PIZ",
            Self::Pxr24 => "PXR24",
            Self::B44 => "B44",
            Self::B44a => "B44A",
            Self::Dwaa => "DWAA",
            Self::Dwab => "DWAB",
        }
    }

    /// Which samples a lossy codec can change; `None` for lossless codecs.
    /// Verified per sample type against the `exr` encoder, not just its name.
    pub fn lossy_note(self) -> Option<&'static str> {
        match self {
            Self::Pxr24 => {
                Some("PXR24 rounds FLOAT channels to 24 bits. HALF and UINT stay exact.")
            }
            Self::B44 | Self::B44a => {
                Some("B44 compresses HALF channels lossily. FLOAT and UINT stay exact.")
            }
            Self::Dwaa | Self::Dwab => {
                Some("DWA compresses R, G and B lossily. Matte channels stay exact.")
            }
            _ => None,
        }
    }
}

impl From<Codec> for Compression {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::None => Self::Uncompressed,
            Codec::Rle => Self::RLE,
            Codec::Zip => Self::ZIP16,
            Codec::Zips => Self::ZIP1,
            Codec::Piz => Self::PIZ,
            Codec::Pxr24 => Self::PXR24,
            Codec::B44 => Self::B44,
            Codec::B44a => Self::B44A,
            Codec::Dwaa => Self::DWAA(None),
            Codec::Dwab => Self::DWAB(None),
        }
    }
}
