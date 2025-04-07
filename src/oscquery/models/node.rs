use serde::de::{Error, Visitor};
use serde::ser::SerializeSeq;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct OscRootNode {
    pub(crate) root: OscNode,
}

impl OscRootNode {
    pub fn new() -> Self {
        Self {
            root: OscNode {
                full_path: "/".to_string(),
                ..Default::default()
            },
        }
    }

    pub fn with_avatar(mut self) -> Self {
        self.add_node(OscNode {
            full_path: "/avatar".to_string(),
            ..Default::default()
        })
    }

    pub fn with_tracking(mut self) -> Self {
        self.add_node(OscNode {
            full_path: "/tracking".to_string(),
            ..Default::default()
        })
    }

    pub fn with_dolly(mut self) -> Self {
        self.add_node(OscNode {
            full_path: "/dolly".to_string(),
            ..Default::default()
        })
    }

    /// Get the node at the specified path.
    /// Returns `None` if the node does not exist.
    pub fn get_node(&self, path: &str) -> Option<&OscNode> {
        let mut current = &self.root;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            current = current.contents.get(part)?;
        }
        Some(current)
    }

    /// Add a node to the tree.
    /// If a node already exists at the specified path, it will be replaced.
    pub fn add_node(mut self, node: OscNode) -> Self {
        let mut current = &mut self.root;
        let mut path = vec![];
        for part in node.full_path.split('/').filter(|p| !p.is_empty()) {
            path.push(part);
            current = current.contents.entry(part.to_string()).or_insert(OscNode {
                full_path: format!("/{}", path.join("/")),
                ..Default::default()
            });
        }
        *current = node;
        self
    }

    /// Remove a node from the tree.
    /// Returns the removed node if it exists, or `None` if it does not.
    pub fn remove_node(&mut self, path: &str) -> Option<OscNode> {
        let mut parts = path.split('/').filter(|p| !p.is_empty());
        let mut last_branch: *mut OscNode = &mut self.root;
        let mut last_branch_key: &str = parts.next()?;
        let mut current = self.root.contents.get_mut(last_branch_key)?;
        while let Some(part) = parts.next() {
            if current.contents.len() > 1 {
                last_branch = current as *mut OscNode;
                last_branch_key = part;
            }
            if let Some(node) = current.contents.get_mut(part) {
                current = node;
            } else {
                return None;
            }
        }
        unsafe {
            (*last_branch).contents.remove(last_branch_key)
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct OscNode {
    /// The full OSC address path of the node. Required for all nodes.
    pub full_path: String,

    /// A map of child nodes, if this node is a container.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub contents: HashMap<String, OscNode>,

    /// The OSC type tag string for this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<OscTypeTag>,

    /// A human-readable description of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The access mode of this node (read-only, write-only, or read-write).
    pub access: AccessMode,

    /// The current value(s) of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Vec<OscValue>>,

    /// The range of acceptable values for this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Vec<RangeInfo>>,
}

#[derive(Debug, Clone)]
pub struct OscTypeTag(Vec<OscType>);

impl OscTypeTag {
    pub fn new(types: Vec<OscType>) -> Self {
        OscTypeTag(types)
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

            // Although out of specification, VRChat's OSCQuery may return an empty map.
            fn visit_map<A>(self, _map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                Ok(OscValue::Nil)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    None,
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl Default for AccessMode {
    fn default() -> Self {
        AccessMode::None
    }
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
