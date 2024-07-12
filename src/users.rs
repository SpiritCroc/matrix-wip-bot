use config::{Config, Value};
use matrix_sdk::ruma::OwnedUserId;

fn is_user_matched(mxid: &OwnedUserId, allowed: Vec<String>) -> bool {
    for a in allowed {
        if a.contains("@") {
            if a == *mxid {
                return true;
            }
        } else if *a == *mxid.server_name() {
            return true;
        }
    }
    false
}

fn is_user_matched_with_config(mxid: &OwnedUserId, config: Config, key: &str) -> bool {
    is_user_matched(
        mxid,
        config.get_array(key)
            .unwrap_or_default()
            .into_iter()
            .map(Value::into_string)
            .map(Result::unwrap_or_default)
            .collect()
    )
}

pub fn is_user_vip(mxid: &OwnedUserId, config: Config) -> bool {
    is_user_matched_with_config(mxid, config, "users.vip")
}

pub fn is_user_trusted(mxid: &OwnedUserId, config: Config) -> bool {
    is_user_matched_with_config(mxid, config, "users.trusted")
}
