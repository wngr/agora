use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ChatApi {
    Message {
        message: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        origin_timestamp: chrono::DateTime<chrono::Utc>,
    },
    ChangeNickname {
        nick: String,
    },
}

//mod peerid_serializer {
//    use libp2p::PeerId;
//    use serde::{Deserialize, Deserializer, Serialize, Serializer};
//    use std::str::FromStr;
//
//    pub fn serialize<S>(value: &PeerId, serializer: S) -> Result<S::Ok, S::Error>
//    where
//        S: Serializer,
//    {
//        value.to_base58().serialize(serializer)
//    }
//
//    pub fn deserialize<'de, D>(deserializer: D) -> Result<PeerId, D::Error>
//    where
//        D: Deserializer<'de>,
//    {
//        let str = String::deserialize(deserializer)?;
//        PeerId::from_str(&str).map_err(|e| {
//            serde::de::Error::custom(format!("peer id deserialization failed for {:?}", e))
//        })
//    }
//}
