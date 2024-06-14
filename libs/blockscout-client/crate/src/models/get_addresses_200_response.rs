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
pub struct GetAddresses200Response {
    #[serde(rename = "exchange_rate")]
    pub exchange_rate: String,
    #[serde(rename = "total_supply")]
    pub total_supply: String,
    #[serde(rename = "items")]
    pub items: Vec<models::AddressWithTxCount>,
    #[serde(rename = "next_page_params")]
    pub next_page_params: serde_json::Value,
}

impl GetAddresses200Response {
    pub fn new(
        exchange_rate: String,
        total_supply: String,
        items: Vec<models::AddressWithTxCount>,
        next_page_params: serde_json::Value,
    ) -> GetAddresses200Response {
        GetAddresses200Response {
            exchange_rate,
            total_supply,
            items,
            next_page_params,
        }
    }
}