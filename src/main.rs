use config::Config;
use log::{trace, debug, info, warn, error};
use url::Url;
use matrix_sdk::{
    config::SyncSettings,
    event_handler::Ctx,
    authentication::matrix::MatrixSession,
    Client, Room, RoomState,
    ruma::{
        events::room::{
            message::{
                MessageType, OriginalSyncRoomMessageEvent,
            },
            member::StrippedRoomMemberEvent,
        },
    },
    RoomMemberships,
};
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::PathBuf;
use tokio::fs;
use tokio::time::{sleep, Duration};

mod command;
mod users;
mod image_generator;
mod bridge;
use crate::command::handle_command;
use crate::users::is_user_trusted;

// Things we want to pass to message/event handlers
#[derive(Clone)]
struct WipContext {
    config: Config,
    bot_name: String,
    bot_server: String,
    allowed_pings: Vec<String>,
    launched_ts: u128,
    media_client: Option<Client>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    let config = Config::builder()
        .add_source(config::File::with_name("config.yaml"))
        .build()
        .unwrap();

    let hs = config.get::<String>("login.homeserver_url").expect("Homeserver url missing in config");
    let hs_url = Url::parse(&hs).expect("Invalid homeserver url");
    let username = config.get::<String>("login.username").expect("Username missing in config");
    let password = config.get::<String>("login.password").expect("Password missing in config");

    let data_dir = config.get::<String>("data_path")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::data_dir().expect("no data_dir directory found").join("matrix-wip-bot"));
    let db_path = data_dir.join("db");
    let session_path = data_dir.join("session");

    let bot_name = config.get::<String>("bot.plaintext_ping").ok();
    let allowed_pings = bot_name.clone().map(|name|
        vec![
            name.clone(),
            format!("{name}:")
        ]
    ).unwrap_or_default();

    debug!("Data dir configured at {}", data_dir.to_str().unwrap_or_default());

    let device_name = config.get::<String>("login.device_name").unwrap_or(String::from("wip-bot"));

    let bot_client = get_logged_in_client(
        "bot",
        &hs_url,
        &db_path,
        &session_path,
        &username,
        &password,
        &device_name,
    ).await?;

    if !bot_client.matrix_auth().logged_in() {
        panic!("Bot not logged in");
    }

    let bot_server = bot_client.server().map(|s| s.to_string()).unwrap_or_else(|| {
        username.split(":").collect::<Vec<_>>()[1].to_string()
    });

    let media_hs = config.get::<String>("media_login.homeserver_url");
    let media_client = if let Ok(media_hs) = media_hs {
        debug!("Found media client config for {media_hs}");
        let media_hs_url = Url::parse(&media_hs).expect("Invalid media homeserver url");
        let media_username = config.get::<String>("media_login.username").expect("Username missing in config for media user");
        let media_password = config.get::<String>("media_login.password").expect("Password missing in config for media user");
        let media_db_path = data_dir.join("media_db");
        let media_session_path = data_dir.join("media_session");
        let media_device_name = config.get::<String>("media_login.device_name").unwrap_or(device_name);
        let media_client = get_logged_in_client(
            "media",
            &media_hs_url,
            &media_db_path,
            &media_session_path,
            &media_username,
            &media_password,
            &media_device_name,
        ).await?;
        if !media_client.matrix_auth().logged_in() {
            panic!("Media client not logged in");
        }
        Some(media_client)
    } else {
        None
    };

    if let Ok(recovery_key) = config.get::<String>("login.recovery_key") {
        let recovery = bot_client.encryption().recovery();
        match recovery.recover(&recovery_key).await {
            Ok(_) => info!("Recovery state: {:?}", recovery.state()),
            Err(e) => error!("Failed to restore recovery key: {}", e),
        }
    }

    let wip_context = WipContext {
        config: config.clone(),
        bot_name: bot_name.unwrap_or("WIP-Bot".to_string()),
        bot_server,
        allowed_pings,
        launched_ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        media_client,
    };

    bot_client.add_event_handler_context(wip_context);

    // This one is possibly also for old state events handled before
    bot_client.add_event_handler(handle_invites);

    // Sync once without message handler to not deal with old messages
    info!("Starting initial sync...");
    let sync_response = bot_client.sync_once(SyncSettings::default()).await.unwrap();
    info!("Initial sync finished with token {}, start listening for events", sync_response.next_batch);

    // Actual message handling and sync loop
    bot_client.add_event_handler(handle_message);
    bot_client.sync(SyncSettings::default().token(sync_response.next_batch)).await?;

    Ok(())
}

async fn get_logged_in_client(
    log_tag: &str,
    hs_url: &Url,
    db_path: &PathBuf,
    session_path: &PathBuf,
    username: &str,
    password: &str,
    device_name: &str,
) -> anyhow::Result<Client> {
    debug!("Logging {log_tag} into {hs_url} as {username}...");
    let client = Client::builder()
        .homeserver_url(hs_url)
        .sqlite_store(db_path, None)
        .build()
        .await?;

    if session_path.exists() {
        info!("Restoring old {log_tag} login...");
        let serialized_session = fs::read_to_string(session_path).await?;
        let user_session: MatrixSession = serde_json::from_str(&serialized_session)?;
        client.restore_session(user_session).await?;
    } else {
        info!("Doing a fresh {log_tag} login...");

        let matrix_auth = client.matrix_auth();
        let login_response = matrix_auth
            .login_username(username, password)
            .initial_device_display_name(device_name)
            .await?;

        info!("Logged in {log_tag} as {}", login_response.device_id);

        let user_session = matrix_auth.session().expect("A logged-in client should have a session");
        let serialized_session = serde_json::to_string(&user_session)?;
        fs::write(session_path, serialized_session).await?;
    }
    Ok(client)
}

// From https://github.com/matrix-org/matrix-rust-sdk/blob/main/examples/autojoin/src/main.rs
async fn handle_invites(
    event: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
    wip_context: Ctx<WipContext>
) {
    if event.state_key != client.user_id().unwrap() {
        return;
    }
    if !is_user_trusted(&event.sender, wip_context.0.config) {
        info!("Not auto-joining room {} by untrusted invitation from {}", room.room_id(), event.sender);
        return;
    }

    tokio::spawn(async move {
        info!("Autojoining room {} by invitation from {}", room.room_id(), event.sender);
        let mut delay = 2;

        while let Err(err) = room.join().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            warn!("Failed to join room {} ({err:?}), retrying in {delay}s", room.room_id());

            sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                warn!("Can't join room {} ({err:?})", room.room_id());
                break;
            }
        }
        info!("Successfully joined room {}", room.room_id());
    });
}

async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    wip_context: Ctx<WipContext>
) {
    trace!("Message received by {} in {}: {:?}", event.sender, room.room_id(), room.state());
    if room.state() != RoomState::Joined {
        return;
    }
    if event.sender == room.own_user_id() {
        return;
    }
    let MessageType::Text(text_content) = event.clone().content.msgtype else {
        return;
    };
    trace!("Message received by {} in {}: {}", event.sender, room.room_id(), text_content.body);

    if u128::from(event.origin_server_ts.0) < wip_context.0.launched_ts - 10_000 {
        info!("Ignore message in the past: {} in {}", event.event_id, room.room_id());
        return
    }

    let mut split_body = text_content.body.split_whitespace();
    let cmd = split_body.next().unwrap_or_default().to_ascii_lowercase();

    let is_mention = wip_context.0.allowed_pings.iter().any(|ping| ping.to_ascii_lowercase() == cmd);

    let cmd = if is_mention {
        split_body.next().unwrap_or_default().to_ascii_lowercase()
    } else {
        cmd
    };

    // Commands start with '!', or is a mention,
    // or was sent in a DM.
    if cmd.starts_with('!') {
        let cmd = &cmd[1..].to_string();
        handle_command(cmd, split_body, event, room, wip_context.0).await;
    } else if is_mention || room.members(RoomMemberships::JOIN).await.unwrap_or_default().len() == 2 {
        handle_command(&cmd, split_body, event, room, wip_context.0).await;
    }
}
