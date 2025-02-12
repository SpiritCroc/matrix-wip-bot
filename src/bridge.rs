use matrix_sdk::ruma::events::macros::EventContent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "m.bridge", kind = State, state_key_type = String)]
pub struct BridgeStateContent {
    #[serde(default, skip_serializing_if = "matrix_sdk::ruma::serde::is_default")]
    pub bridgebot: Option<String>,
    #[serde(default, skip_serializing_if = "matrix_sdk::ruma::serde::is_default")]
    pub creator: Option<String>,
    #[serde(default, skip_serializing_if = "matrix_sdk::ruma::serde::is_default")]
    pub protocol: Option<BridgeProtocol>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BridgeProtocol {
    #[serde(default, skip_serializing_if = "matrix_sdk::ruma::serde::is_default")]
    pub id: String,
    #[serde(default, skip_serializing_if = "matrix_sdk::ruma::serde::is_default")]
    pub displayname: String,
}
