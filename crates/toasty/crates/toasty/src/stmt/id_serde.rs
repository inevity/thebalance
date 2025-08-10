use super::Id;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use toasty_core::stmt;

impl<M> Serialize for Id<M> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as the string representation
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, M: crate::Model> Deserialize<'de> for Id<M> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize from string
        let s = String::deserialize(deserializer)?;
        // Create an Id from the string using the model ID
        Ok(Id::from_untyped(stmt::Id::from_string(M::ID, s)))
    }
}