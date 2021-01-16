use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListRoutesOpt {
    pub domain: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddRouteOpt {
    pub domain: String,
    pub path: String,
    pub appid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeleteDomainOpt {
    pub domain: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeleteRouteOpt {
    pub domain: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddAppOpt {
    pub config: String,
    pub bundle_b64: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeleteAppOpt {
    pub appid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogsOpt {
    pub appid: String,
    pub since_secs: u64,
    pub limit: u32,
}
