//! OSCQuery node types and related structures.
use crate::error::OscQueryError;
use serde::de::{Error, Visitor};
use serde::ser::SerializeSeq;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;

/// An OSCQuery node.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct OscNode {
    /// The full OSC address path of the node. Required for all nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_path: Option<String>,

    /// A map of child nodes, if this node is a container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<HashMap<String, OscNode>>,

    /// The OSC type tag string for this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<OscTypeTag>,

    /// A human-readable description of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The access mode of this node (read-only, write-only, or read-write).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessMode>,

    /// The current value(s) of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Vec<OscValue>>,

    /// The range of acceptable values for this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Vec<RangeInfo>>,

    /// An array of strings describing or categorizing this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    /// Additional type information for each value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_type: Option<ExtendedType>,

    /// The units of measurement for each value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<Vec<Unit>>,

    /// Indicates if messages to this address are of particular importance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical: Option<bool>,

    /// Specifies how out-of-range values are handled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipmode: Option<Vec<ClipMode>>,

    /// Alternative type configurations this node can accept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overloads: Option<Vec<OscNode>>,
}

impl OscNode {
    pub fn new(full_path: &str) -> OscNode {
        OscNode {
            full_path: Some(full_path.to_string()),
            contents: None,
            r#type: None,
            description: None,
            access: None,
            value: None,
            range: None,
            tags: None,
            extended_type: None,
            unit: None,
            critical: None,
            clipmode: None,
            overloads: None,
        }
    }

    pub fn contents(&self) -> Option<&HashMap<String, OscNode>> {
        self.contents.as_ref()
    }

    pub fn contents_mut(&mut self) -> Option<&mut HashMap<String, OscNode>> {
        self.contents.as_mut()
    }

    pub fn with_contents(mut self, contents: HashMap<String, OscNode>) -> Self {
        self.contents = Some(contents);
        self
    }

    pub fn r#type(&self) -> Option<&OscTypeTag> {
        self.r#type.as_ref()
    }

    pub fn with_type(mut self, r#type: OscTypeTag) -> Self {
        self.r#type = Some(r#type);
        self
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    pub fn access(&self) -> Option<AccessMode> {
        self.access
    }

    pub fn with_access(mut self, access: AccessMode) -> Self {
        self.access = Some(access);
        self
    }

    pub fn value(&self) -> Option<&[OscValue]> {
        self.value.as_ref().map(|v| v.as_slice())
    }

    pub fn with_value(mut self, value: Vec<OscValue>) -> Self {
        self.value = Some(value);
        self
    }

    pub fn set_value(&mut self, value: Vec<OscValue>) {
        self.value = Some(value);
    }

    pub fn range(&self) -> Option<&[RangeInfo]> {
        self.range.as_ref().map(|r| r.as_slice())
    }

    pub fn with_range(mut self, range: Vec<RangeInfo>) -> Self {
        self.range = Some(range);
        self
    }

    pub fn tags(&self) -> Option<&[String]> {
        self.tags.as_ref().map(|t| t.as_slice())
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn extended_type(&self) -> Option<&ExtendedType> {
        self.extended_type.as_ref()
    }

    pub fn with_extended_type(mut self, extended_type: ExtendedType) -> Self {
        self.extended_type = Some(extended_type);
        self
    }

    pub fn unit(&self) -> Option<&[Unit]> {
        self.unit.as_ref().map(|u| u.as_slice())
    }

    pub fn with_unit(mut self, unit: Vec<Unit>) -> Self {
        self.unit = Some(unit);
        self
    }

    pub fn critical(&self) -> Option<bool> {
        self.critical
    }

    pub fn with_critical(mut self, critical: bool) -> Self {
        self.critical = Some(critical);
        self
    }

    pub fn clipmode(&self) -> Option<&[ClipMode]> {
        self.clipmode.as_ref().map(|c| c.as_slice())
    }

    pub fn with_clipmode(mut self, clipmode: Vec<ClipMode>) -> Self {
        self.clipmode = Some(clipmode);
        self
    }

    pub fn overloads(&self) -> Option<&[OscNode]> {
        self.overloads.as_ref().map(|o| o.as_slice())
    }

    pub fn with_overloads(mut self, overloads: Vec<OscNode>) -> Self {
        self.overloads = Some(overloads);
        self
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!(self)
    }

    pub fn from_json(value: serde_json::Value) -> Result<OscNode, OscQueryError> {
        serde_json::from_value(value).map_err(|_| OscQueryError::DeserializationError)
    }
}

#[derive(Debug, Clone)]
pub struct OscTypeTag(Vec<OscType>);

impl OscTypeTag {
    pub fn new(types: Vec<OscType>) -> Self {
        OscTypeTag(types)
    }

    pub fn types(&self) -> &[OscType] {
        &self.0
    }

    pub fn with_type(mut self, t: OscType) -> Self {
        self.0.push(t);
        self
    }

    pub fn from_tag(tag: &str) -> Option<OscTypeTag> {
        let mut types = Vec::new();
        for c in tag.chars() {
            OscType::from_tag(&c.to_string()).map(|t| types.push(t));
        }
        Some(OscTypeTag(types))
    }
}

impl Serialize for OscTypeTag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize into string
        let mut tag = String::new();
        for t in &self.0 {
            tag.push_str(t.tag().as_str());
        }
        serializer.serialize_str(tag.as_str())
    }
}

impl<'de> Deserialize<'de> for OscTypeTag {
    fn deserialize<D>(deserializer: D) -> Result<OscTypeTag, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OscTypeTagVisitor;

        impl<'de> Visitor<'de> for OscTypeTagVisitor {
            type Value = OscTypeTag;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid OSC type tag")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut types = Vec::new();
                for c in value.chars() {
                    OscType::from_tag(&c.to_string()).map(|t| types.push(t));
                }
                Ok(OscTypeTag(types))
            }
        }

        deserializer.deserialize_str(OscTypeTagVisitor)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum OscType {
    Int32,     // "i"
    Float32,   // "f"
    OscString, // "s"
    OscBlob,   // "b"
    Int64,     // "h"
    Timetag,   // "t"
    Double,    // "d"
    Symbol,    // "S"
    Char,      // "c"
    RgbaColor, // "r"
    Midi,      // "m"
    True,      // "T"
    False,     // "F"
    Nil,       // "N"
    Infinitum, // "I"
    Array(Vec<OscType>), // "[...]"
}

impl OscType {
    pub fn tag(&self) -> String {
        match self {
            OscType::Int32 => "i".to_string(),
            OscType::Float32 => "f".to_string(),
            OscType::OscString => "s".to_string(),
            OscType::OscBlob => "b".to_string(),
            OscType::Int64 => "h".to_string(),
            OscType::Timetag => "t".to_string(),
            OscType::Double => "d".to_string(),
            OscType::Symbol => "S".to_string(),
            OscType::Char => "c".to_string(),
            OscType::RgbaColor => "r".to_string(),
            OscType::Midi => "m".to_string(),
            OscType::True => "T".to_string(),
            OscType::False => "F".to_string(),
            OscType::Nil => "N".to_string(),
            OscType::Infinitum => "I".to_string(),
            OscType::Array(array) => {
                let mut tag = String::from("[");
                for t in array {
                    tag.push_str(t.tag().as_str());
                }
                tag.push(']');
                tag
            }
        }
    }

    pub fn from_tag(tag: &str) -> Option<OscType> {
        match tag {
            "i" => Some(OscType::Int32),
            "f" => Some(OscType::Float32),
            "s" => Some(OscType::OscString),
            "b" => Some(OscType::OscBlob),
            "h" => Some(OscType::Int64),
            "t" => Some(OscType::Timetag),
            "d" => Some(OscType::Double),
            "S" => Some(OscType::Symbol),
            "c" => Some(OscType::Char),
            "r" => Some(OscType::RgbaColor),
            "m" => Some(OscType::Midi),
            "T" => Some(OscType::True),
            "F" => Some(OscType::False),
            "N" => Some(OscType::Nil),
            "I" => Some(OscType::Infinitum),
            x if x.starts_with("[") => {
                let mut types = Vec::new();
                let mut chars = tag.chars();
                while let Some(c) = chars.next() {
                    if c == ']' {
                        return Some(OscType::Array(types));
                    } else {
                        OscType::from_tag(&c.to_string()).map(|t| types.push(t));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl Serialize for OscType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.tag().as_str())
    }
}

struct OscTypeVisitor;

impl<'de> Visitor<'de> for OscTypeVisitor {
    type Value = OscType;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a valid OSC type tag")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match value {
            x if x.len() > 1 && !x.starts_with('[') => {
                let mut types = Vec::new();
                for c in x.chars() {
                    OscType::from_tag(&c.to_string()).map(|t| types.push(t));
                }
                Ok(OscType::Array(types))
            }
            x if x.len() >= 1 => OscType::from_tag(value)
                .ok_or_else(|| de::Error::custom(format!("invalid OSC type tag: {}", value))),
            _ => Err(de::Error::custom(format!(
                "invalid OSC type tag: {}",
                value
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for OscType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(OscTypeVisitor)
    }
}

#[derive(Debug, Clone)]
pub enum OscValue {
    Int(i32),
    Float(f64),
    String(String),
    Bool(bool),
    Color(String), // RGBA hex string
    Array(Vec<OscValue>),
    Nil,
}

impl Serialize for OscValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            OscValue::Int(i) => serializer.serialize_i32(*i),
            OscValue::Float(f) => serializer.serialize_f64(*f),
            OscValue::String(s) => serializer.serialize_str(s),
            OscValue::Bool(b) => serializer.serialize_bool(*b),
            OscValue::Color(c) => serializer.serialize_str(c),
            OscValue::Array(a) => {
                let mut seq = serializer.serialize_seq(Some(a.len()))?;
                for value in a {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            OscValue::Nil => serializer.serialize_unit(),
        }
    }
}

impl<'de> Deserialize<'de> for OscValue {
    fn deserialize<D>(deserializer: D) -> Result<OscValue, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OSCValueVisitor;

        impl<'de> Visitor<'de> for OSCValueVisitor {
            type Value = OscValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid OSC value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OscValue::Bool(value))
            }

            fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OscValue::Int(value))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Int(v as i32))
            }

            fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OscValue::Float(value as f64))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::Float(v))
            }

            fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(OscValue::String(v.to_string()))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OscValue::String(value.to_string()))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OscValue::Nil)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(OscValue::Array(values))
            }
        }

        deserializer.deserialize_any(OSCValueVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct RangeInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<OscValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<OscValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vals: Option<Vec<OscValue>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtendedType {
    PositionCartesian,
    PositionPolar,
    PositionSpherical,
    PositionCylindrical,
    OrientationQuaternion,
    OrientationEuler,
    OrientationAxis,
    ColorRgb,
    ColorRgba,
    ColorHsv,
    ColorHsva,
    ColorCmyk,
    ColorCmyka,
    ColorCiexyz,
    Filepath,
    Url,
    Custom(Vec<String>),
}

impl ExtendedType {
    fn type_info(&self) -> (&str, &[&str]) {
        match self {
            Self::PositionCartesian => ("position.cartesian", &["x", "y", "z"]),
            Self::PositionPolar => ("position.polar", &["r", "p"]),
            Self::PositionSpherical => ("position.spherical", &["r", "t", "p"]),
            Self::PositionCylindrical => ("position.cylindrical", &["r", "p", "z"]),
            Self::OrientationQuaternion => ("orientation.quaternion", &["a", "b", "c", "d"]),
            Self::OrientationEuler => ("orientation.euler", &["y", "p", "r"]),
            Self::OrientationAxis => ("orientation.axis", &["x", "y", "z", "w"]),
            Self::ColorRgb => ("color.rgb", &["r", "g", "b"]),
            Self::ColorRgba => ("color.rgba", &["r", "g", "b", "a"]),
            Self::ColorHsv => ("color.hsv", &["h", "s", "v"]),
            Self::ColorHsva => ("color.hsva", &["h", "s", "v", "a"]),
            Self::ColorCmyk => ("color.cmyk", &["c", "m", "y", "k"]),
            Self::ColorCmyka => ("color.cmyka", &["c", "m", "y", "k", "a"]),
            Self::ColorCiexyz => ("color.ciexyz", &["x", "y", "z"]),
            Self::Filepath => ("filepath", &[]),
            Self::Url => ("url", &[]),
            Self::Custom(_) => ("custom", &[]),
        }
    }

    fn as_vec(&self) -> Vec<String> {
        let (prefix, components) = self.type_info();
        if components.is_empty() {
            vec![prefix.to_string()]
        } else {
            components.iter().map(|&c| format!("{}.{}", prefix, c)).collect()
        }
    }

    fn from_vec(v: Vec<String>) -> Self {
        if v.is_empty() {
            return Self::Custom(v);
        }

        let parts: Vec<&str> = v[0].split('.').collect();
        if parts.len() < 2 {
            return match parts[0] {
                "filepath" => Self::Filepath,
                "url" => Self::Url,
                _ => Self::Custom(v),
            };
        }

        let prefix = format!("{}.{}", parts[0], parts[1]);
        let expected_len = v.len();

        match prefix.as_str() {
            "position.cartesian" if expected_len == 3 => Self::PositionCartesian,
            "position.polar" if expected_len == 2 => Self::PositionPolar,
            "position.spherical" if expected_len == 3 => Self::PositionSpherical,
            "position.cylindrical" if expected_len == 3 => Self::PositionCylindrical,
            "orientation.quaternion" if expected_len == 4 => Self::OrientationQuaternion,
            "orientation.euler" if expected_len == 3 => Self::OrientationEuler,
            "orientation.axis" if expected_len == 4 => Self::OrientationAxis,
            "color.rgb" if expected_len == 3 => Self::ColorRgb,
            "color.rgba" if expected_len == 4 => Self::ColorRgba,
            "color.hsv" if expected_len == 3 => Self::ColorHsv,
            "color.hsva" if expected_len == 4 => Self::ColorHsva,
            "color.cmyk" if expected_len == 4 => Self::ColorCmyk,
            "color.cmyka" if expected_len == 5 => Self::ColorCmyka,
            "color.ciexyz" if expected_len == 3 => Self::ColorCiexyz,
            _ => Self::Custom(v),
        }
    }
}

impl Serialize for ExtendedType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.as_vec().serialize(serializer)
    }
}

struct ExtendedTypeVisitor;

impl<'de> Visitor<'de> for ExtendedTypeVisitor {
    type Value = ExtendedType;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string array representing an ExtendedType")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut v = Vec::new();
        while let Some(s) = seq.next_element()? {
            v.push(s);
        }
        Ok(ExtendedType::from_vec(v))
    }
}

impl<'de> Deserialize<'de> for ExtendedType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ExtendedTypeVisitor)
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct HostInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub osc_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub osc_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub osc_transport: Option<OSCTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    None = 0,
    ReadOnly = 1,
    WriteOnly = 2,
    ReadWrite = 3,
}

impl Serialize for AccessMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = match self {
            AccessMode::None => 0,
            AccessMode::ReadOnly => 1,
            AccessMode::WriteOnly => 2,
            AccessMode::ReadWrite => 3,
        };
        serializer.serialize_u8(value)
    }
}

impl<'de> Deserialize<'de> for AccessMode {
    fn deserialize<D>(deserializer: D) -> Result<AccessMode, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        match value {
            0 => Ok(AccessMode::None),
            1 => Ok(AccessMode::ReadOnly),
            2 => Ok(AccessMode::WriteOnly),
            3 => Ok(AccessMode::ReadWrite),
            _ => Err(de::Error::custom(format!("invalid access mode: {}", value))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipMode {
    None,
    Low,
    High,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OSCTransport {
    UDP,
    TCP,
}

#[derive(Debug, Clone)]
pub enum Unit {
    // Distance Units
    DistanceM,
    DistanceKm,
    DistanceDm,
    DistanceCm,
    DistanceMm,
    DistanceUm,
    DistanceNm,
    DistancePm,
    DistanceInches,
    DistanceFeet,
    DistanceMiles,
    DistancePixels,

    // Angle Units
    AngleDegree,
    AngleRadian,

    // Gain Units
    GainLinear,
    GainMidiGain,
    GainDb,
    GainDbRaw,

    // Time and Pitch Units
    TimeSecond,
    TimeBark,
    TimeBpm,
    TimeCents,
    TimeHz,
    TimeMel,
    TimeMidiNote,
    TimeMs,
    TimeSpeed,
    TimeSamples,

    // Speed Units
    SpeedMS,
    SpeedMph,
    SpeedKmH,
    SpeedKn,
    SpeedFtS,
    SpeedFtH,

    // Custom Unit
    Custom(String),
}

impl Unit {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Distance
            Unit::DistanceM => "distance.m",
            Unit::DistanceKm => "distance.km",
            Unit::DistanceDm => "distance.dm",
            Unit::DistanceCm => "distance.cm",
            Unit::DistanceMm => "distance.mm",
            Unit::DistanceUm => "distance.um",
            Unit::DistanceNm => "distance.nm",
            Unit::DistancePm => "distance.pm",
            Unit::DistanceInches => "distance.inches",
            Unit::DistanceFeet => "distance.feet",
            Unit::DistanceMiles => "distance.miles",
            Unit::DistancePixels => "distance.pixels",

            // Angle
            Unit::AngleDegree => "angle.degree",
            Unit::AngleRadian => "angle.radian",

            // Gain
            Unit::GainLinear => "gain.linear",
            Unit::GainMidiGain => "gain.midigain",
            Unit::GainDb => "gain.db",
            Unit::GainDbRaw => "gain.db-raw",

            // Time and Pitch
            Unit::TimeSecond => "time.second",
            Unit::TimeBark => "time.bark",
            Unit::TimeBpm => "time.bpm",
            Unit::TimeCents => "time.cents",
            Unit::TimeHz => "time.hz",
            Unit::TimeMel => "time.mel",
            Unit::TimeMidiNote => "time.midinote",
            Unit::TimeMs => "time.ms",
            Unit::TimeSpeed => "time.speed",
            Unit::TimeSamples => "time.samples",

            // Speed
            Unit::SpeedMS => "speed.m/s",
            Unit::SpeedMph => "speed.mph",
            Unit::SpeedKmH => "speed.km/h",
            Unit::SpeedKn => "speed.kn",
            Unit::SpeedFtS => "speed.ft/s",
            Unit::SpeedFtH => "speed.ft/h",

            // Custom
            Unit::Custom(_) => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Unit> {
        match s {
            // Distance
            "distance.m" => Some(Unit::DistanceM),
            "distance.km" => Some(Unit::DistanceKm),
            "distance.dm" => Some(Unit::DistanceDm),
            "distance.cm" => Some(Unit::DistanceCm),
            "distance.mm" => Some(Unit::DistanceMm),
            "distance.um" => Some(Unit::DistanceUm),
            "distance.nm" => Some(Unit::DistanceNm),
            "distance.pm" => Some(Unit::DistancePm),
            "distance.inches" => Some(Unit::DistanceInches),
            "distance.feet" => Some(Unit::DistanceFeet),
            "distance.miles" => Some(Unit::DistanceMiles),
            "distance.pixels" => Some(Unit::DistancePixels),

            // Angle
            "angle.degree" => Some(Unit::AngleDegree),
            "angle.radian" => Some(Unit::AngleRadian),

            // Gain
            "gain.linear" => Some(Unit::GainLinear),
            "gain.midigain" => Some(Unit::GainMidiGain),
            "gain.db" => Some(Unit::GainDb),
            "gain.db-raw" => Some(Unit::GainDbRaw),

            // Time and Pitch
            "time.second" => Some(Unit::TimeSecond),
            "time.bark" => Some(Unit::TimeBark),
            "time.bpm" => Some(Unit::TimeBpm),
            "time.cents" => Some(Unit::TimeCents),
            "time.hz" => Some(Unit::TimeHz),
            "time.mel" => Some(Unit::TimeMel),
            "time.midinote" => Some(Unit::TimeMidiNote),
            "time.ms" => Some(Unit::TimeMs),
            "time.speed" => Some(Unit::TimeSpeed),
            "time.samples" => Some(Unit::TimeSamples),

            // Speed
            "speed.m/s" => Some(Unit::SpeedMS),
            "speed.mph" => Some(Unit::SpeedMph),
            "speed.km/h" => Some(Unit::SpeedKmH),
            "speed.kn" => Some(Unit::SpeedKn),
            "speed.ft/s" => Some(Unit::SpeedFtS),
            "speed.ft/h" => Some(Unit::SpeedFtH),

            _ => None,
        }
    }
}

impl Serialize for Unit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Unit {
    fn deserialize<D>(deserializer: D) -> Result<Unit, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Unit::from_str(&s).unwrap_or(Unit::Custom(s)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryRequest {
    pub path: String,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketCommand {
    Listen { path: String },
    Ignore { path: String },
    PathChanged { path: String },
    PathRenamed { old: String, new: String },
    PathRemoved { path: String },
    PathAdded { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub command: String,
    pub data: String,
}



impl From<WebSocketCommand> for WebSocketMessage {
    fn from(command: WebSocketCommand) -> Self {
        match command {
            WebSocketCommand::Listen { path } => WebSocketMessage {
                command: "LISTEN".to_string(),
                data: path,
            },
            WebSocketCommand::Ignore { path } => WebSocketMessage {
                command: "IGNORE".to_string(),
                data: path,
            },
            WebSocketCommand::PathChanged { path } => WebSocketMessage {
                command: "PATH_CHANGED".to_string(),
                data: path,
            },
            WebSocketCommand::PathRenamed { old, new } => WebSocketMessage {
                command: "PATH_RENAMED".to_string(),
                data: json!({ "OLD": old, "NEW": new }).to_string(),
            },
            WebSocketCommand::PathRemoved { path } => WebSocketMessage {
                command: "PATH_REMOVED".to_string(),
                data: path,
            },
            WebSocketCommand::PathAdded { path } => WebSocketMessage {
                command: "PATH_ADDED".to_string(),
                data: path,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::node::OscNode;
    use serde_json::json;

    #[test]
    fn basic() {
        let json = json!({
            "DESCRIPTION": "root node",
            "FULL_PATH": "/",
            "ACCESS": 0,
            "CONTENTS": {
                "foo": {
                    "DESCRIPTION": "demonstrates a read-only OSC node- single float value ranged 0-100",
                    "FULL_PATH": "/foo",
                    "ACCESS": 1,
                    "TYPE": "f",
                    "VALUE": [
                        0.5
                    ],
                    "RANGE": [
                        {
                            "MIN": 0.0,
                            "MAX": 100.0
                        }
                    ]
                },
                "bar": {
                    "DESCRIPTION": "demonstrates a read/write OSC node- two ints with different ranges",
                    "FULL_PATH": "/bar",
                    "ACCESS": 3,
                    "TYPE": "ii",
                    "VALUE": [
                        4,
                        51
                    ],
                    "RANGE": [
                        {
                            "MIN": 0,
                            "MAX": 50
                        },
                        {
                            "MIN": 51,
                            "MAX": 100
                        }
                    ]
                },
                "baz": {
                    "DESCRIPTION": "simple container node, with one method- qux",
                    "FULL_PATH": "/baz",
                    "ACCESS": 0,
                    "CONTENTS": {
                        "qux":	{
                            "DESCRIPTION": "read/write OSC node- accepts one of several string-type inputs",
                            "FULL_PATH": "/baz/qux",
                            "ACCESS": 3,
                            "TYPE": "s",
                            "VALUE": [
                                "half-full"
                            ],
                            "RANGE": [
                                {
                                    "VALS": [ "empty", "half-full", "full" ]
                                }
                            ]
                        }
                    }
                }
            }
        });

        let node: OscNode = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(json, node.to_json());
    }

    #[test]
    fn specific_method() {
        let json = json!({
            "DESCRIPTION": "simple container node, with one method- qux",
            "FULL_PATH": "/baz",
            "ACCESS": 0,
            "CONTENTS": {
                "qux":	{
                    "DESCRIPTION": "read/write OSC node- accepts one of several string-type inputs",
                    "FULL_PATH": "/baz/qux",
                    "ACCESS": 3,
                    "TYPE": "s",
                    "VALUE": [
                        "half-full"
                    ],
                    "RANGE": [
                        {
                            "VALS": [ "empty", "half-full", "full" ]
                        }
                    ]
                }
            }
        });

        let node: OscNode = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(json, node.to_json());
    }

    #[test]
    fn overloads_and_extended_type() {
        let json = json!({
            "FULL_PATH": "/color",
            "TYPE": "r",
            "VALUE": [
                "#FA6432FF"
            ],
            "OVERLOADS": [
                {
                    "TYPE": "ffff",
                    "VALUE": [
                        0.980392156862745,
                        0.392156862745098,
                        0.196078431372549,
                        1.0
                    ],
                    "RANGE": [
                        {	"MIN": 0.0, "MAX": 1.0	},
                        {	"MIN": 0.0, "MAX": 1.0	},
                        {	"MIN": 0.0, "MAX": 1.0	},
                        {	"MIN": 0.0, "MAX": 1.0	}
                    ],
                    "EXTENDED_TYPE": [
                        "color.rgba.r",
                        "color.rgba.g",
                        "color.rgba.b",
                        "color.rgba.a"
                    ]
                },
                {
                    "TYPE": "iiii",
                    "VALUE": [
                        250,
                        100,
                        50,
                        255
                    ],
                    "RANGE": [
                        {	"MIN": 0, "MAX": 255	},
                        {	"MIN": 0, "MAX": 255	},
                        {	"MIN": 0, "MAX": 255	},
                        {	"MIN": 0, "MAX": 255	},
                    ],
                    "EXTENDED_TYPE": [
                        "color.rgba.r",
                        "color.rgba.g",
                        "color.rgba.b",
                        "color.rgba.a"
                    ]
                }
            ]
        });

        let node: OscNode = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(json, node.to_json());
    }
}
