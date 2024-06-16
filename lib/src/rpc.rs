// Collection of functions to interface with ghostd.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use log::{debug, trace};
use reqwest::Client;

#[derive(Debug, Clone, Default)]
pub struct RPCURL(String);

impl RPCURL {
    pub fn target(
        mut self,
        ip: &str,
        port: &u16,
        walletname: &str,
        user: &str,
        password: &str,
    ) -> Self {
        trace!("Constructing RPC console URL ...");
        if walletname.len() == 0 {
            if !user.is_empty() && !password.is_empty() {
                self.0 = format!("http://{}:{}@{}:{}/", user, password, ip, port);
            } else {
                self.0 = format!("http://{}:{}/", ip, port);
            }
        } else {
            if !user.is_empty() && !password.is_empty() {
                self.0 = format!(
                    "http://{}:{}@{}:{}/wallet/{}",
                    user, password, ip, port, walletname
                );
            } else {
                self.0 = format!("http://{}:{}/wallet/{}", ip, port, walletname);
            }
        }
        return self;
    }
}

fn parametrize(args: &str) -> Vec<Value> {
    trace!("Parsing arguments ...");
    let mut params: Vec<Value> = Vec::new();
    let mut multi_param: Vec<String> = Vec::new();
    for entry in args.split(" ").collect::<Vec<&str>>() {
        match serde_json::from_str(entry) {
            Ok(val) => {
                params.push(val);
            }
            Err(_) => {
                if !multi_param.is_empty() && !entry.to_string().ends_with("\"") {
                    multi_param.push(entry.to_string());
                    continue;
                }

                if entry.to_string().starts_with("\"") && multi_param.is_empty() {
                    multi_param.push(entry.strip_prefix("\"").unwrap().to_string());
                    continue;
                }
                if entry.to_string().ends_with("\"") && !multi_param.is_empty() {
                    multi_param.push(entry.strip_suffix("\"").unwrap().to_string());
                    let final_multi: String = multi_param.join(" ");
                    params.push(Value::String(final_multi));
                    multi_param = Vec::new();
                    continue;
                }
                params.push(Value::String(entry.to_string()));
            }
        }
    }

    return params;
}

#[derive(Debug, Serialize, Deserialize)]
struct Post<'r> {
    jsonrpc: &'r str,
    id: &'r str,
    method: Value,
    params: Value,
}

pub(crate) async fn call(
    args: &str,
    rpcurl: &RPCURL,
    rpc_client: &Client,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut params = parametrize(args);
    let method = params[0].clone();
    params.remove(0);

    let post = Post {
        jsonrpc: "1.0",
        id: "2",
        method,
        params: Value::Array(params),
    };
    debug!("RPC: {} {} ...", &post.method, &post.params);

    // Use the .post method with async/await
    let response = rpc_client
        .post(&rpcurl.0)
        .header("Content-Type", "application/json")
        .json(&post)
        .send()
        .await?;

    let body: Value = match response.error_for_status_ref() {
        Ok(_) => response.json().await?,
        Err(err) => {
            return Err(err.into()); // Propagate the error
        }
    };

    Ok(body["result"].clone())
}
