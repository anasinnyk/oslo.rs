use serde::ser::{Serializer};
use serde::{Deserialize, Deserializer, Serialize};
use std::str::FromStr;
use void::Void;
use schemars::{gen, JsonSchema};
use schemars::schema::{InstanceType, Schema, SchemaObject, SingleOrVec, StringValidation};

#[derive(Clone, Debug)]
pub struct DurationShorthand {
    time: u32,
    r#type: DurationShorthandType,
}

#[derive(Clone, Debug)]
pub enum DurationShorthandType {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Serialize for DurationShorthand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
    S: Serializer,
    {
        let mut res = self.time.to_string();
        res.push(match self.r#type {
            DurationShorthandType::Minute => 'm',
            DurationShorthandType::Hour => 'h',
            DurationShorthandType::Day => 'd',
            DurationShorthandType::Week => 'w',
            DurationShorthandType::Month => 'M',
            DurationShorthandType::Quarter => 'Q',
            DurationShorthandType::Year => 'Y',
        });
        serializer.serialize_str(&*res)
    }
}

impl FromStr for DurationShorthand {
    type Err = Void;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let c = s.to_string().pop().unwrap();
        let time = s.parse::<u32>().unwrap();
        let r#type = (match c {
            'm' => Some(DurationShorthandType::Minute),
            'h' => Some(DurationShorthandType::Hour),
            'd' => Some(DurationShorthandType::Day),
            'w' => Some(DurationShorthandType::Week),
            'M' => Some(DurationShorthandType::Month),
            'Q' => Some(DurationShorthandType::Quarter),
            'Y' => Some(DurationShorthandType::Year),
            _ => None,
        }).unwrap();
        Ok(Self {
            time,
            r#type,
        })
    }
}

impl<'de> Deserialize<'de> for DurationShorthand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        DurationShorthand::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for DurationShorthand {
    fn schema_name() -> String {
        "Duration".to_string()
    }

    fn json_schema(_: &mut gen::SchemaGenerator) -> Schema {
        Schema::Object(
            SchemaObject{
                metadata: None,
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::String))),
                format: None,
                enum_values: None,
                const_value: None,
                subschemas: None,
                number: None,
                string: Some(Box::new(StringValidation{
                    max_length: None,
                    min_length: None,
                    pattern: Some("^[1-9][0-9]*[mhdwMQY]$".to_string()),
                })),
                array: None,
                object: None,
                reference: None,
                extensions: Default::default(),
            }
        )
    }
}
