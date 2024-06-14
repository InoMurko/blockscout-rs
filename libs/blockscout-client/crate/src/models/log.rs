/*
 * BlockScout API
 *
 * API for BlockScout web app
 *
 * The version of the OpenAPI document: 1.0.0
 * Contact: you@your-company.com
 * Generated by: https://openapi-generator.tech
 */

use crate::models;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Log {
    #[serde(rename = "address")]
    pub address: models::AddressParam,
    #[serde(rename = "block_hash", skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(rename = "block_number", skip_serializing_if = "Option::is_none")]
    pub block_number: Option<i32>,
    #[serde(rename = "data")]
    pub data: String,
    #[serde(rename = "decoded")]
    pub decoded: models::DecodedInputLog,
    #[serde(rename = "index")]
    pub index: i32,
    #[serde(rename = "smart_contract")]
    pub smart_contract: models::AddressParam,
    #[serde(rename = "topics")]
    pub topics: Vec<String>,
    #[serde(rename = "tx_hash")]
    pub tx_hash: String,
}

impl Log {
    pub fn new(
        address: models::AddressParam,
        data: String,
        decoded: models::DecodedInputLog,
        index: i32,
        smart_contract: models::AddressParam,
        topics: Vec<String>,
        tx_hash: String,
    ) -> Log {
        Log {
            address,
            block_hash: None,
            block_number: None,
            data,
            decoded,
            index,
            smart_contract,
            topics,
            tx_hash,
        }
    }
}