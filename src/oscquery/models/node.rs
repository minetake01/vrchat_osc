use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{collections::HashMap, fmt};

/// Represents the root of an OSC address space tree.
/// This is a wrapper around an `OscNode` that is always at the path "/".
#[derive(Debug, Clone)]
pub struct OscRootNode {
    /// The actual root OscNode. Its `full_path` should be "/".
    pub(crate) root: OscNode,
}

impl OscRootNode {
    /// Creates a new, empty `OscRootNode`.
    /// The root node itself is initialized with `full_path = "/"`.
    pub fn new() -> Self {
        Self {
            root: OscNode {
                full_path: "/".to_string(),
                ..Default::default() // Initializes other fields with their default values.
            },
        }
    }

    /// Utility method to quickly add an "/avatar" node, common in VRChat OSC setups.
    pub fn with_avatar(self) -> Self {
        self.add_node(OscNode {
            full_path: "/avatar".to_string(),
            description: Some("Root for avatar-specific parameters.".to_string()),
            ..Default::default()
        })
    }

    /// Utility method to quickly add a "/tracking" node.
    pub fn with_tracking(self) -> Self {
        self.add_node(OscNode {
            full_path: "/tracking".to_string(),
            description: Some("Root for tracking-related OSC messages (e.g., head, hands).".to_string()),
            ..Default::default()
        })
    }
    
    /// Utility method to quickly add a "/dolly" node (example).
    pub fn with_dolly(self) -> Self {
        self.add_node(OscNode {
            full_path: "/dolly".to_string(),
            description: Some("Parameters related to camera dolly or movement.".to_string()),
            ..Default::default()
        })
    }

    /// Retrieves a reference to an `OscNode` at the specified path.
    ///
    /// # Arguments
    /// * `path` - The full OSC path (e.g., "/avatar/parameters/SomeParameter").
    ///
    /// # Returns
    /// An `Option<&OscNode>`: `Some(&node)` if found, `None` otherwise.
    pub fn get_node(&self, path: &str) -> Option<&OscNode> {
        // Handle the root path directly.
        if path == "/" {
            return Some(&self.root);
        }

        let mut current = &self.root;
        // Iterate over path segments, skipping the initial empty segment from leading '/'.
        for part in path.split('/').filter(|p| !p.is_empty()) {
            // Traverse down the tree using the `contents` map.
            current = current.contents.get(part)?; // If a segment is not found, return None.
        }
        Some(current)
    }
    
    /// Retrieves a mutable reference to an `OscNode` at the specified path.
    ///
    /// # Arguments
    /// * `path` - The full OSC path.
    ///
    /// # Returns
    /// An `Option<&mut OscNode>`: `Some(&mut node)` if found, `None` otherwise.
    pub fn get_node_mut(&mut self, path: &str) -> Option<&mut OscNode> {
        if path == "/" {
            return Some(&mut self.root);
        }
        let mut current = &mut self.root;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            current = current.contents.get_mut(part)?;
        }
        Some(current)
    }


    /// Adds an `OscNode` to the tree.
    /// If a node already exists at the specified path, its properties (excluding `contents`)
    /// will be updated by the new node. The `full_path` of the node at the target path will be
    /// set or updated from `new_node_props.full_path`. Child nodes (`contents`) of an existing node are preserved
    /// unless the new node also has contents for those specific children (contents are merged).
    /// This method takes `self` by value and returns a new `OscRootNode` to allow chaining.
    ///
    /// # Arguments
    /// * `node` - The `OscNode` to add. Its `full_path` determines its position in the tree.
    pub fn add_node(mut self, new_node_props: OscNode) -> Self {
        let path_parts: Vec<&str> = new_node_props.full_path.split('/').filter(|p| !p.is_empty()).collect();
        let mut current = &mut self.root;
        let mut constructed_path = String::from("/");

        for (i, part) in path_parts.iter().enumerate() {
            // Construct the path for the current segment.
            if i > 0 || (i == 0 && !part.is_empty()) { // Avoid double slashes if part is empty, ensure leading slash
                 if constructed_path.len() > 1 { constructed_path.push('/'); }
                 constructed_path.push_str(part);
            } else if i == 0 && part.is_empty() && constructed_path.is_empty() { // handles cases like adding "/"
                constructed_path = "/".to_string();
            }


            // Get or insert the node for the current part.
            // `or_insert_with` creates a new default node if `part` doesn't exist in `contents`.
            current = current.contents.entry(part.to_string()).or_insert_with(|| OscNode {
                full_path: constructed_path.clone(),
                ..Default::default()
            });
        }
        
        // Now `current` points to the node at `new_node_props.full_path`.
        // Update its properties with values from `new_node_props`.
        // Preserve existing children in `current.contents` unless `new_node_props.contents` overwrites them.
        current.description = new_node_props.description.or(current.description.take());
        current.r#type = new_node_props.r#type.or(current.r#type.take());
        current.access = if new_node_props.access != AccessMode::default() { new_node_props.access } else { current.access };
        current.value = new_node_props.value.or(current.value.take());
        current.range = new_node_props.range.or(current.range.take());
        // Merge contents: new_node_props.contents can add to or overwrite current.contents
        for (key, child_node) in new_node_props.contents {
            current.contents.insert(key, child_node);
        }
        // Ensure the full_path of the target node is correctly set from new_node_props,
        // as it might have been default-initialized if newly created.
        current.full_path = new_node_props.full_path;


        self
    }

    /// Removes a node (and all its children) from the tree.
    /// This is a safe implementation, replacing the previous `unsafe` one.
    ///
    /// # Arguments
    /// * `path` - The full OSC path of the node to remove.
    ///
    /// # Returns
    /// The removed `OscNode` if it existed, or `None` otherwise.
    pub fn remove_node(&mut self, path: &str) -> Option<OscNode> {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        
        // Cannot remove the root node itself, or an empty path.
        if parts.is_empty() {
            return None;
        }

        // If trying to remove the root node (e.g. path was "/"), this logic won't allow it.
        // The root node is intrinsic to OscRootNode.
        // If path is "/", parts will be empty, handled above.

        let mut current = &mut self.root;
        // Traverse to the parent of the node to be removed.
        // `parts.len() - 1` ensures we stop at the parent.
        for i in 0..parts.len() - 1 {
            current = current.contents.get_mut(parts[i])?; // If any part of the parent path is not found, return None.
        }
        
        // `parts.last().unwrap()` is safe because `parts` is not empty.
        // Remove the target node from its parent's `contents` map.
        current.contents.remove(&parts.last().unwrap().to_string())
    }
}

/// Represents a single node in the OSC address space.
/// It can be a container for other nodes or an endpoint with a value.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct OscNode {
    /// The full OSC address path of the node (e.g., "/avatar/parameters/SomeParameter"). Required.
    pub full_path: String,

    /// A map of child nodes, if this node is a container (i.e., not an endpoint).
    /// Key is the child node's name (one segment of the path).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub contents: HashMap<String, OscNode>,

    /// The OSC type tag string for this node (e.g., "f" for float, "i" for int).
    /// See OSC 1.0 specification for type tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<OscTypeTag>, // `r#` is used because `type` is a keyword in Rust.

    /// A human-readable description of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The access mode of this node (read-only, write-only, or read-write).
    /// Defaults to `AccessMode::None` if not specified.
    #[serde(default)] // Ensures AccessMode::default() is used if missing in JSON.
    pub access: AccessMode,

    /// The current value(s) of this node, if it's an endpoint.
    /// Can be a vector to support OSC messages with multiple arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Vec<OscValue>>,

    /// The range of acceptable values for this node, if applicable.
    /// Each `RangeInfo` can specify min/max or a list of allowed values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Vec<RangeInfo>>,
}

impl OscNode {
    /// Checks if the node's OSC type tag matches a specific single type.
    pub fn is_type(&self, expected_type: &OscType) -> bool {
        self.r#type.as_ref().map_or(false, |tag| tag.is_single_type(expected_type))
    }

    /// Checks if the node's OSC type tag represents a boolean.
    pub fn is_boolean_type(&self) -> bool {
        self.r#type.as_ref().map_or(false, |tag| tag.is_boolean())
    }

    /// Gets the single `OscType` if the node has exactly one type specified.
    pub fn get_single_osc_type(&self) -> Option<&OscType> {
        self.r#type.as_ref().and_then(|tag| tag.get_single_type())
    }
}

/// Represents an OSC type tag string, which is a sequence of OSC types.
/// Example: "ifsb" means an int, a float, a string, and a blob.
#[derive(Debug, Clone)]
pub struct OscTypeTag(Vec<OscType>); // Internally stores a vector of individual OscTypes.

impl OscTypeTag {
    /// Creates a new `OscTypeTag` from a vector of `OscType`s.
    pub fn new(types: Vec<OscType>) -> Self {
        OscTypeTag(types)
    }

    /// Adds an `OscType` to this `OscTypeTag` (builder pattern).
    pub fn with_type(mut self, t: OscType) -> Self {
        self.0.push(t);
        self
    }

    /// Creates an `OscTypeTag` from a string representation (e.g., "ifs").
    /// Returns `None` if the string contains invalid type characters.
    pub fn from_tag(tag_string: &str) -> Option<OscTypeTag> {
        let mut types = Vec::new();
        let mut chars = tag_string.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '[' {
                // Handle array types like "[iii]"
                let mut array_tag_str = String::from("[");
                while let Some(ac) = chars.next() {
                    array_tag_str.push(ac);
                    if ac == ']' {
                        break;
                    }
                }
                if let Some(array_type) = OscType::from_tag(&array_tag_str) {
                    types.push(array_type);
                } else {
                    return None; // Invalid array type string
                }
            } else {
                if let Some(t) = OscType::from_tag(&c.to_string()) {
                    types.push(t);
                } else {
                    return None; // Invalid basic type character
                }
            }
        }
        if types.is_empty() && !tag_string.is_empty() { // If input was not empty but no types parsed (e.g. invalid chars)
             return None;
        }
        Some(OscTypeTag(types))
    }

    /// Checks if the tag represents a single, specific OSC type.
    pub fn is_single_type(&self, expected_type: &OscType) -> bool {
        self.0.len() == 1 && &self.0[0] == expected_type
    }

    /// Checks if the tag represents a boolean type (single T or F).
    pub fn is_boolean(&self) -> bool {
        self.0.len() == 1 && self.0[0].is_boolean()
    }

    /// Gets the single `OscType` if this tag represents exactly one type.
    pub fn get_single_type(&self) -> Option<&OscType> {
        if self.0.len() == 1 {
            Some(&self.0[0])
        } else {
            None
        }
    }

    /// Checks if the tag contains a specific `OscType`.
    pub fn contains(&self, osc_type: &OscType) -> bool {
        self.0.contains(osc_type)
    }
}

/// Custom serialization for `OscTypeTag` to a string.
impl Serialize for OscTypeTag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tag_string = String::new();
        for t in &self.0 {
            tag_string.push_str(&t.tag());
        }
        serializer.serialize_str(&tag_string)
    }
}

/// Custom deserialization for `OscTypeTag` from a string.
impl<'de> Deserialize<'de> for OscTypeTag {
    fn deserialize<D>(deserializer: D) -> Result<OscTypeTag, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OscTypeTagVisitor;

        impl<'de> Visitor<'de> for OscTypeTagVisitor {
            type Value = OscTypeTag;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid OSC type tag string (e.g., 'ifs')")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                OscTypeTag::from_tag(value)
                    .ok_or_else(|| de::Error::custom(format!("invalid OSC type tag string: {}", value)))
            }
        }
        deserializer.deserialize_str(OscTypeTagVisitor)
    }
}

/// Represents individual OSC data types as defined in the OSC 1.0 specification.
#[derive(Debug, PartialEq, Clone)]
pub enum OscType {
    Int32,     // 'i' - 32-bit integer
    Float32,   // 'f' - 32-bit floating point number
    OscString, // 's' - OSC-string (a sequence of non-null ASCII characters followed by a null, etc.)
    OscBlob,   // 'b' - OSC-blob (an int32 size count followed by that many 8-bit bytes of arbitrary data)
    Int64,     // 'h' - 64-bit integer (added in some OSC extensions, not strictly OSC 1.0 core)
    Timetag,   // 't' - OSC-timetag (64-bit NTP format)
    Double,    // 'd' - 64-bit floating point number (added in some OSC extensions)
    Symbol,    // 'S' - Alternate type for OSC-string often used for symbolic names
    Char,      // 'c' - 32-bit ASCII character
    RgbaColor, // 'r' - 32-bit RGBA color (four 8-bit unsigned integers)
    Midi,      // 'm' - 4 byte MIDI message (port ID, status byte, data1, data2)
    True,      // 'T' - Represents the value True
    False,     // 'F' - Represents the value False
    Nil,       // 'N' - Represents "Nil" or "Null"
    Infinitum, // 'I' - Represents "Infinitum" or an impulse, often for triggering events
    Array(Vec<OscType>), // '[' and ']' delimit an array of types. E.g., "[ifs]"
}

impl OscType {
    /// Returns the single character tag for this `OscType`.
    /// For `Array`, it returns a string like "[...]".
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
            OscType::Array(array_types) => {
                let mut tag = String::from("[");
                for t in array_types {
                    tag.push_str(&t.tag());
                }
                tag.push(']');
                tag
            }
        }
    }

    /// Returns `true` if the OSC type represents a boolean value (True or False).
    pub fn is_boolean(&self) -> bool {
        matches!(self, OscType::True | OscType::False)
    }

    /// Returns `true` if the OSC type is a numeric type (Int32, Float32, Int64, Double).
    pub fn is_numeric(&self) -> bool {
        matches!(self, OscType::Int32 | OscType::Float32 | OscType::Int64 | OscType::Double)
    }

    /// Creates an `OscType` from its single character tag string.
    /// Handles basic types and array types like "[iii]".
    pub fn from_tag(tag_char_or_array_str: &str) -> Option<OscType> {
        if tag_char_or_array_str.starts_with('[') && tag_char_or_array_str.ends_with(']') && tag_char_or_array_str.len() >= 2 {
            let inner_tags_str = &tag_char_or_array_str[1..tag_char_or_array_str.len()-1];
            let mut types = Vec::new();
            let mut chars = inner_tags_str.chars().peekable();
            let mut current_segment = String::new();

            while let Some(c) = chars.next() {
                current_segment.push(c);
                if c == '[' {
                    let mut bracket_level = 1;
                    // Consume characters until the matching closing bracket for the current nested array
                    while let Some(next_char_in_array) = chars.peek() {
                        if *next_char_in_array == '[' {
                            bracket_level += 1;
                        } else if *next_char_in_array == ']' {
                            bracket_level -= 1;
                        }
                        current_segment.push(chars.next().unwrap()); // Consume the char
                        if bracket_level == 0 {
                            break;
                        }
                    }
                    if bracket_level != 0 { return None; } // Unmatched brackets

                    // current_segment now holds a complete array tag like "[...]"
                    if let Some(array_type_segment) = OscType::from_tag(&current_segment) {
                        types.push(array_type_segment);
                        current_segment.clear();
                    } else {
                        return None; // Invalid nested array tag
                    }
                } else {
                    // Basic type (single char)
                    // current_segment now holds a single character for a basic type
                    if let Some(basic_type) = OscType::from_tag(&current_segment) {
                        types.push(basic_type);
                        current_segment.clear();
                    } else {
                        // If current_segment is not a valid single char tag, it's an error.
                        // This happens if a multi-character segment is not an array.
                        return None;
                    }
                }
            }
            if !current_segment.is_empty() { // Should be empty if all segments parsed correctly
                return None;
            }
            return Some(OscType::Array(types));
        }

        match tag_char_or_array_str {
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
            _ => None, // Unknown type tag
        }
    }
}

/// Custom serialization for `OscType` (delegates to its tag string).
impl Serialize for OscType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.tag())
    }
}

/// Custom deserialization for `OscType` from its tag string.
impl<'de> Deserialize<'de> for OscType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OscTypeVisitor;

        impl<'de> Visitor<'de> for OscTypeVisitor {
            type Value = OscType;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid OSC type tag character (e.g., 'f') or array string (e.g., '[ii]')")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // The original deserializer had logic for multi-char strings being arrays,
                // which might conflict with array syntax `[...]`.
                // OscType::from_tag now handles `[...]` syntax.
                OscType::from_tag(value)
                    .ok_or_else(|| de::Error::custom(format!("invalid OSC type tag: {}", value)))
            }
        }
        deserializer.deserialize_str(OscTypeVisitor)
    }
}

/// Represents an RGBA color value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RgbaColorValue {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Represents an OSC value, corresponding to an `OscType`.
/// This enum covers common types used in OSC messages and OSCQuery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)] // Added Deserialize, PartialEq
#[serde(untagged)] // Allows Serde to try deserializing into variants without a specific tag field.
                   // This is useful if JSON values can be numbers, strings, booleans, or arrays directly.
pub enum OscValue {
    Int(i32),
    Float(f64), // Using f64 for floats to match JSON numbers, OSC 'f' is f32. Conversion might be needed.
    String(String),
    Bool(bool),
    Color(RgbaColorValue), // Changed from String to RgbaColorValue
    Array(Vec<OscValue>), // For OSC arrays.
    Nil, // Represents OSC Nil. Serializes to JSON null.
}

/// Represents range information for an OSC node's value.
/// Can specify a min/max range or a list of discrete allowed values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct RangeInfo {
    /// Minimum allowed value (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<OscValue>,
    /// Maximum allowed value (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<OscValue>,
    /// A list of discrete allowed values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vals: Option<Vec<OscValue>>,
}

/// Defines the access mode of an OSC node (read, write, both, or none).
/// Corresponds to the "ACCESS" attribute in OSCQuery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] // Added Default derive
pub enum AccessMode {
    #[default] // No access specified or node is not an endpoint.
    None, // 0
    ReadOnly,  // 1: Can be read, cannot be written.
    WriteOnly, // 2: Can be written, cannot be read. (Less common)
    ReadWrite, // 3: Can be read and written.
}

/// Custom serialization for `AccessMode` to its integer representation.
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

/// Custom deserialization for `AccessMode` from its integer representation.
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
            _ => Err(de::Error::custom(format!("invalid access mode integer: {}, expected 0-3", value))),
        }
    }
}
